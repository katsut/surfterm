use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use portable_pty::{MasterPty, PtySize};
use tokio::sync::Mutex;
use tracing::{info, instrument};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

use crate::detector::patterns::default_claude_code_state_patterns;
use crate::detector::StateDetector;
use crate::input::{InputAction, InputHandler, SurftermCmd};
use crate::renderer::panel::SidePanelEntry;
use crate::renderer::Renderer;
use crate::session::pty::PtyHandle;
use crate::session::stream_splitter::StreamSplitter;
use crate::session::terminal::Terminal;
use crate::session::SessionId;

/// Application event types for inter-component communication.
#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    /// New PTY output data arrived for a specific session.
    PtyOutput {
        session_id: SessionId,
        data: Vec<u8>,
    },
    /// A session's PTY process has exited.
    SessionExited(SessionId),
    /// Request a redraw of the window.
    RequestRedraw,
}

/// Per-session pipeline holding the terminal emulator, PTY handles, and processing components.
struct SessionPipeline {
    terminal: Terminal,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    splitter: StreamSplitter,
    detector: StateDetector,
    project_name: String,
}

/// Main application state.
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    event_proxy: EventLoopProxy<AppEvent>,
    tokio_handle: tokio::runtime::Handle,
    // Multi-session management
    sessions: HashMap<SessionId, SessionPipeline>,
    /// Ordered list of session IDs for stable display order.
    session_order: Vec<SessionId>,
    active_session: Option<SessionId>,
    input_handler: InputHandler,
    // Terminal dimensions (for the main area, excluding sidebar)
    cols: u16,
    rows: u16,
}

impl App {
    fn new(event_proxy: EventLoopProxy<AppEvent>, tokio_handle: tokio::runtime::Handle) -> Self {
        Self {
            window: None,
            renderer: None,
            event_proxy,
            tokio_handle,
            sessions: HashMap::new(),
            session_order: Vec::new(),
            active_session: None,
            input_handler: InputHandler::new(),
            cols: 80,
            rows: 24,
        }
    }

    /// Spawn a new PTY session and add it to the session map.
    /// Returns the new session's ID, or None on failure.
    fn spawn_session(&mut self) -> Option<SessionId> {
        let cols = self.cols;
        let rows = self.rows;
        let session_id = SessionId::new();

        // Create terminal emulator
        let terminal = Terminal::new(cols, rows);

        // Create StreamSplitter
        let patterns = StreamSplitter::default_claude_code_patterns();
        let (splitter, _channels) = StreamSplitter::new(patterns);

        // Create StateDetector
        let state_patterns = default_claude_code_state_patterns();
        let (detector, _state_rx) = StateDetector::new(state_patterns);

        // Spawn PTY
        let mut pty = match PtyHandle::spawn(rows, cols) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to spawn PTY: {e}");
                return None;
            }
        };

        info!(%session_id, "PTY session spawned ({cols}x{rows})");

        // Grab writer and master before moving pty into the reader task
        let writer = pty.writer();
        let master = pty.master();

        // Derive project name from cwd
        let project_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "shell".to_string());

        // Spawn async task to read PTY output and forward to event loop
        let proxy = self.event_proxy.clone();
        let sid = session_id;
        self.tokio_handle.spawn(async move {
            while let Some(data) = pty.read_output().await {
                if proxy
                    .send_event(AppEvent::PtyOutput {
                        session_id: sid,
                        data,
                    })
                    .is_err()
                {
                    break;
                }
            }
            let _ = proxy.send_event(AppEvent::SessionExited(sid));
            info!(%sid, "PTY output stream ended");
        });

        let pipeline = SessionPipeline {
            terminal,
            writer,
            master,
            splitter,
            detector,
            project_name,
        };

        self.sessions.insert(session_id, pipeline);
        self.session_order.push(session_id);

        // Set as active if this is the first session
        if self.active_session.is_none() {
            self.active_session = Some(session_id);
        }

        // Update side panel
        self.update_side_panel();

        Some(session_id)
    }

    /// Kill a session by ID.
    fn kill_session(&mut self, id: SessionId) {
        self.sessions.remove(&id);
        self.session_order.retain(|sid| *sid != id);

        // If we killed the active session, switch to another
        if self.active_session == Some(id) {
            self.active_session = self.session_order.first().copied();
        }

        self.update_side_panel();
    }

    /// Switch the active session.
    fn switch_to_session(&mut self, id: SessionId) {
        if self.sessions.contains_key(&id) {
            self.active_session = Some(id);
            self.update_side_panel();

            // Resize the newly active session's PTY and terminal to current dimensions
            if let Some(pipeline) = self.sessions.get_mut(&id) {
                pipeline.terminal.resize(self.cols, self.rows);
                let master = Arc::clone(&pipeline.master);
                let rows = self.rows;
                let cols = self.cols;
                self.tokio_handle.spawn(async move {
                    let m = master.lock().await;
                    let _ = m.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                });
            }
        }
    }

    /// Build side panel entries from current sessions and push to renderer.
    fn update_side_panel(&mut self) {
        let entries: Vec<SidePanelEntry> = self
            .session_order
            .iter()
            .filter_map(|id| {
                let pipeline = self.sessions.get(id)?;
                Some(SidePanelEntry {
                    id: *id,
                    name: pipeline.project_name.clone(),
                    state: pipeline.detector.current_state(),
                    is_active: self.active_session == Some(*id),
                })
            })
            .collect();

        if let Some(renderer) = self.renderer.as_mut() {
            renderer.update_side_panel(entries);
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_title("Surfterm")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match self.tokio_handle.block_on(Renderer::new(Arc::clone(&window))) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to initialize renderer: {e}");
                event_loop.exit();
                return;
            }
        };

        // Calculate terminal dimensions from main area (excluding sidebar)
        self.cols = renderer.grid.main_cols();
        self.rows = renderer.grid.main_rows();

        info!(
            width = renderer.size.width,
            height = renderer.size.height,
            cols = self.cols,
            rows = self.rows,
            "Window and renderer initialized"
        );

        self.window = Some(window);
        self.renderer = Some(renderer);

        // Spawn the initial session
        self.spawn_session();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(physical_size);
                    self.cols = renderer.grid.main_cols();
                    self.rows = renderer.grid.main_rows();
                }
                // Resize all session terminals and the active session's PTY
                for (id, pipeline) in self.sessions.iter_mut() {
                    pipeline.terminal.resize(self.cols, self.rows);
                    if self.active_session == Some(*id) {
                        let master = Arc::clone(&pipeline.master);
                        let rows = self.rows;
                        let cols = self.cols;
                        self.tokio_handle.spawn(async move {
                            let m = master.lock().await;
                            let _ = m.resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                        });
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Some(active_id) = self.active_session {
                        if let Some(pipeline) = self.sessions.get(&active_id) {
                            let content = pipeline.terminal.content();
                            if let Err(e) = renderer.render_content(&content) {
                                tracing::error!("Render error: {e}");
                            }
                        } else if let Err(e) = renderer.render() {
                            tracing::error!("Render error: {e}");
                        }
                    } else if let Err(e) = renderer.render() {
                        tracing::error!("Render error: {e}");
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let action = self.input_handler.handle_key(&event);
                match action {
                    InputAction::SendToPty(data) => {
                        if let Some(active_id) = self.active_session {
                            if let Some(pipeline) = self.sessions.get(&active_id) {
                                let writer = Arc::clone(&pipeline.writer);
                                self.tokio_handle.spawn(async move {
                                    let mut w = writer.lock().await;
                                    if let Err(e) = w.write_all(&data) {
                                        tracing::error!("Failed to write to PTY: {e}");
                                    }
                                    let _ = w.flush();
                                });
                            }
                        }
                    }
                    InputAction::SurftermCommand(cmd) => match cmd {
                        SurftermCmd::Quit => {
                            info!("Quit command received");
                            event_loop.exit();
                        }
                        SurftermCmd::ToggleRawView => {
                            if let Some(renderer) = self.renderer.as_mut() {
                                renderer.toggle_display_mode();
                            }
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                        SurftermCmd::SidePanelDown => {
                            if let Some(renderer) = self.renderer.as_mut() {
                                renderer.side_panel.select_next();
                            }
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                        SurftermCmd::SidePanelUp => {
                            if let Some(renderer) = self.renderer.as_mut() {
                                renderer.side_panel.select_prev();
                            }
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                        SurftermCmd::SidePanelEnter => {
                            let should_create = self
                                .renderer
                                .as_ref()
                                .map(|r| r.side_panel.is_new_session_selected())
                                .unwrap_or(false);

                            if should_create {
                                self.spawn_session();
                            } else if let Some(entry) = self
                                .renderer
                                .as_ref()
                                .and_then(|r| r.side_panel.selected_entry().cloned())
                            {
                                self.switch_to_session(entry.id);
                            }
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                        SurftermCmd::SidePanelKill => {
                            // Kill the selected session (but not via the new-session button)
                            let selected = self
                                .renderer
                                .as_ref()
                                .and_then(|r| r.side_panel.selected_entry().cloned());

                            if let Some(entry) = selected {
                                self.kill_session(entry.id);
                            }
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                        SurftermCmd::SwitchToNormal | SurftermCmd::SwitchToInsert => {
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                        }
                    },
                    InputAction::None => {}
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.input_handler.set_modifiers(modifiers.state());
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput { session_id, data } => {
                tracing::debug!(bytes = data.len(), %session_id, "PTY output received");

                if let Some(pipeline) = self.sessions.get_mut(&session_id) {
                    // Feed to terminal emulator
                    pipeline.terminal.feed(&data);

                    // Debug logging for active session only
                    if self.active_session == Some(session_id) {
                        let content = pipeline.terminal.content();
                        let non_empty_rows = content
                            .rows
                            .iter()
                            .filter(|r| r.iter().any(|c| c.c != ' '))
                            .count();
                        if non_empty_rows > 0 {
                            if let Some(row) =
                                content.rows.iter().find(|r| r.iter().any(|c| c.c != ' '))
                            {
                                let text: String = row
                                    .iter()
                                    .map(|c| c.c)
                                    .collect::<String>()
                                    .trim_end()
                                    .to_string();
                                let first_cell = &row[0];
                                tracing::info!(
                                    non_empty_rows,
                                    text_preview = %&text[..text.len().min(40)],
                                    fg_r = first_cell.fg.r,
                                    fg_g = first_cell.fg.g,
                                    fg_b = first_cell.fg.b,
                                    "Terminal has content"
                                );
                            }
                        }
                    }

                    // Feed to StreamSplitter
                    pipeline.splitter.classify_chunk(&data);

                    // Feed to StateDetector
                    pipeline.detector.process_chunk(&data);

                    // Update state in renderer
                    if self.active_session == Some(session_id) {
                        if let Some(renderer) = self.renderer.as_mut() {
                            renderer.update_session_state(pipeline.detector.current_state());
                        }
                    }

                    // Update side panel to reflect state changes
                    // (rebuild entries from current state)
                    let entries: Vec<SidePanelEntry> = self
                        .session_order
                        .iter()
                        .filter_map(|id| {
                            let p = self.sessions.get(id)?;
                            Some(SidePanelEntry {
                                id: *id,
                                name: p.project_name.clone(),
                                state: p.detector.current_state(),
                                is_active: self.active_session == Some(*id),
                            })
                        })
                        .collect();

                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.update_side_panel(entries);
                    }
                }

                // Request redraw
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            AppEvent::SessionExited(session_id) => {
                info!(%session_id, "Session exited");
                // We keep the session in the list so user can see its final state.
                // They can explicitly kill it with 'x'.
            }
            AppEvent::RequestRedraw => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
    }
}

/// Run the application: spawn tokio on a separate thread, then run the winit event loop.
#[instrument]
pub fn run() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let tokio_handle = runtime.handle().clone();

    std::thread::spawn(move || {
        runtime.block_on(async {
            tokio::signal::ctrl_c().await.ok();
        });
    });

    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let event_proxy = event_loop.create_proxy();

    let mut app = App::new(event_proxy, tokio_handle);
    event_loop.run_app(&mut app)?;

    info!("Surfterm exited cleanly");
    Ok(())
}

#[cfg(test)]
mod tests {}
