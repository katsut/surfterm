use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use portable_pty::{MasterPty, PtySize};
use tokio::sync::Mutex;
use tracing::{info, instrument};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{CursorIcon, Window, WindowId},
};

use crate::detector::patterns::default_claude_code_state_patterns;
use crate::detector::StateDetector;
use crate::input::{InputAction, InputHandler, InputMode, SurftermCmd};
use crate::renderer::panel::{CardInfo, SidePanelEntry};
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
    child_pid: Option<u32>,
    last_cwd_check: std::time::Instant,
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
    /// Last known cursor position in physical pixels.
    cursor_position: PhysicalPosition<f64>,
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
            cursor_position: PhysicalPosition::new(0.0, 0.0),
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

        // Grab writer, master, and child PID before moving pty into the reader task
        let writer = pty.writer();
        let master = pty.master();
        let child_pid = pty.child_pid();

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
            child_pid,
            last_cwd_check: std::time::Instant::now(),
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

        // Also update the card stack
        self.update_card_stack();
    }

    /// Build card stack from current sessions and push to renderer.
    ///
    /// The active session is placed first (frontmost card), followed by
    /// background sessions in session_order.
    fn update_card_stack(&mut self) {
        let mut cards: Vec<CardInfo> = Vec::with_capacity(self.sessions.len());

        // Active session first
        if let Some(active_id) = self.active_session {
            if let Some(pipeline) = self.sessions.get(&active_id) {
                cards.push(CardInfo {
                    session_id: active_id,
                    project_name: pipeline.project_name.clone(),
                    state: pipeline.detector.current_state(),
                    is_active: true,
                });
            }
        }

        // Background sessions in order
        for id in &self.session_order {
            if Some(*id) == self.active_session {
                continue;
            }
            if let Some(pipeline) = self.sessions.get(id) {
                cards.push(CardInfo {
                    session_id: *id,
                    project_name: pipeline.project_name.clone(),
                    state: pipeline.detector.current_state(),
                    is_active: false,
                });
            }
        }

        if let Some(renderer) = self.renderer.as_mut() {
            renderer.update_card_stack(cards);
        }

        // Resize active session PTY to match card dimensions
        self.resize_active_session_to_card();
    }

    /// Handle a mouse click at the given physical pixel position.
    fn handle_click(&mut self, pos: PhysicalPosition<f64>) {
        let x = pos.x as f32;
        let y = pos.y as f32;

        // We need grid info from the renderer; bail out if not initialized.
        let (sidebar_rect, main_rect, cell_height, cell_width, main_cols, main_rows, scale_factor) = {
            match self.renderer.as_ref() {
                Some(r) => (
                    r.grid.sidebar_rect(),
                    r.grid.main_rect(),
                    r.grid.cell_height,
                    r.grid.cell_width,
                    r.grid.main_cols() as usize,
                    r.grid.main_rows() as usize,
                    r.scale_factor,
                ),
                None => return,
            }
        };

        if x >= sidebar_rect.x {
            // Click is in the sidebar area.
            self.handle_sidebar_click(y, cell_height);
        } else if x < main_rect.width {
            // Click is in the main area.
            self.handle_main_area_click(y, cell_height, cell_width, main_cols, main_rows, scale_factor);
        }

        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// Handle a click in the sidebar.
    ///
    /// Row 0 = [+ New Session], Row 1 = separator, Row 2+ = session entries.
    fn handle_sidebar_click(&mut self, y: f32, cell_height: f32) {
        if cell_height <= 0.0 {
            return;
        }
        let row = (y / cell_height) as usize;

        if row == 0 {
            // Click on [+ New Session]
            self.spawn_session();
        } else if row >= 2 {
            // Session entry; row 2 = sessions[0], row 3 = sessions[1], etc.
            let session_index = row - 2;
            if let Some(id) = self.session_order.get(session_index).copied() {
                self.switch_to_session(id);
            }
        }
        // Row 1 is the separator — ignore clicks on it.
    }

    /// Handle a click in the main content area (card stack).
    ///
    /// The main area layout:
    /// - Row 0: active card title bar
    /// - Rows 1..(1+active_content_rows): terminal content
    /// - Bottom rows: background card tabs
    fn handle_main_area_click(
        &mut self,
        y: f32,
        cell_height: f32,
        _cell_width: f32,
        _main_cols: usize,
        main_rows: usize,
        _scale_factor: f32,
    ) {
        if cell_height <= 0.0 {
            return;
        }
        let row = (y / cell_height) as usize;

        let num_bg_cards = self
            .renderer
            .as_ref()
            .map(|r| r.card_stack.background_cards().len())
            .unwrap_or(0);
        let bg_rows = num_bg_cards.min(main_rows.saturating_sub(2));
        let active_content_rows = main_rows.saturating_sub(1 + bg_rows);
        let bg_start_row = 1 + active_content_rows;

        if row >= bg_start_row && row < bg_start_row + bg_rows {
            // Click is on a background card tab.
            let bg_index = row - bg_start_row;

            // Retrieve the session id of the background card at this index.
            let session_id = self
                .renderer
                .as_ref()
                .and_then(|r| r.card_stack.background_cards().get(bg_index))
                .map(|card| card.session_id);

            if let Some(id) = session_id {
                self.switch_to_session(id);
            }
        } else if row >= 1 && row < bg_start_row {
            // Click is in the active card terminal content area — switch to Insert mode.
            self.input_handler.set_mode(InputMode::Insert);
        }
    }

    /// Determine whether the cursor is over a clickable element and return the
    /// appropriate cursor icon.
    fn cursor_icon_for_position(&self, pos: PhysicalPosition<f64>) -> CursorIcon {
        let x = pos.x as f32;
        let y = pos.y as f32;

        let (sidebar_rect, cell_height, main_rows) = {
            match self.renderer.as_ref() {
                Some(r) => (
                    r.grid.sidebar_rect(),
                    r.grid.cell_height,
                    r.grid.main_rows() as usize,
                ),
                None => return CursorIcon::Default,
            }
        };

        if cell_height <= 0.0 {
            return CursorIcon::Default;
        }

        if x >= sidebar_rect.x {
            // Sidebar area
            let row = (y / cell_height) as usize;
            if row == 0 {
                // [+ New Session]
                return CursorIcon::Pointer;
            } else if row >= 2 {
                let session_index = row - 2;
                if session_index < self.session_order.len() {
                    return CursorIcon::Pointer;
                }
            }
        } else {
            // Main area — check background card tab rows
            let num_bg_cards = self
                .renderer
                .as_ref()
                .map(|r| r.card_stack.background_cards().len())
                .unwrap_or(0);
            let bg_rows = num_bg_cards.min(main_rows.saturating_sub(2));
            let active_content_rows = main_rows.saturating_sub(1 + bg_rows);
            let bg_start_row = 1 + active_content_rows;
            let row = (y / cell_height) as usize;

            if row >= bg_start_row && row < bg_start_row + bg_rows {
                return CursorIcon::Pointer;
            }
        }

        CursorIcon::Default
    }

    /// Resize the active session's terminal and PTY to match the active card
    /// content area (accounting for title bar and background card tabs).
    fn resize_active_session_to_card(&mut self) {
        if let Some(renderer) = self.renderer.as_ref() {
            let (card_cols, card_rows) = renderer.active_card_dimensions();
            if card_cols > 0 && card_rows > 0 && (card_cols != self.cols || card_rows != self.rows)
            {
                self.cols = card_cols;
                self.rows = card_rows;

                if let Some(active_id) = self.active_session {
                    if let Some(pipeline) = self.sessions.get_mut(&active_id) {
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

        // Calculate terminal dimensions from main area (excluding sidebar).
        // These are initial values; they will be adjusted by update_card_stack
        // once sessions are added.
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
                }
                // Recalculate card-aware dimensions and resize active session
                self.update_card_stack();
                // Also resize non-active terminals to the base grid size
                // (they'll be resized properly when switched to)
                for (id, pipeline) in self.sessions.iter_mut() {
                    if self.active_session != Some(*id) {
                        pipeline.terminal.resize(self.cols, self.rows);
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
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = position;
                let icon = self.cursor_icon_for_position(position);
                if let Some(window) = self.window.as_ref() {
                    window.set_cursor(icon);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let pos = self.cursor_position;
                self.handle_click(pos);
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
                    // Update session name from child cwd (throttled to once per second)
                    update_session_name_from_cwd(pipeline);

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
mod tests {
    /// Test sidebar click row calculation logic.
    ///
    /// Layout: Row 0 = [+ New Session], Row 1 = separator, Row 2+ = session entries.
    #[test]
    fn sidebar_click_row_calculation() {
        let cell_height: f32 = 23.0;

        // Click at y=5 (within first row) → row 0 → New Session
        let row = (5.0_f32 / cell_height) as usize;
        assert_eq!(row, 0);

        // Click at y=23 (start of second row) → row 1 → separator
        let row = (23.0_f32 / cell_height) as usize;
        assert_eq!(row, 1);

        // Click at y=46 (start of third row) → row 2 → first session (index 0)
        let row = (46.0_f32 / cell_height) as usize;
        assert_eq!(row, 2);
        assert_eq!(row - 2, 0); // session_index

        // Click at y=69 (start of fourth row) → row 3 → second session (index 1)
        let row = (69.0_f32 / cell_height) as usize;
        assert_eq!(row, 3);
        assert_eq!(row - 2, 1); // session_index

        // Click at y=100 → row 4 → third session (index 2)
        let row = (100.0_f32 / cell_height) as usize;
        assert_eq!(row, 4);
        assert_eq!(row - 2, 2); // session_index
    }

    /// Test background card tab row calculation in the main area.
    #[test]
    fn background_card_tab_row_calculation() {
        let main_rows: usize = 34;
        let num_bg_cards: usize = 2;

        let bg_rows = num_bg_cards.min(main_rows.saturating_sub(2)); // 2
        let active_content_rows = main_rows.saturating_sub(1 + bg_rows); // 31
        let bg_start_row = 1 + active_content_rows; // 32

        assert_eq!(bg_rows, 2);
        assert_eq!(active_content_rows, 31);
        assert_eq!(bg_start_row, 32);

        // Row 32 → bg card index 0
        let row = 32_usize;
        assert!(row >= bg_start_row && row < bg_start_row + bg_rows);
        assert_eq!(row - bg_start_row, 0);

        // Row 33 → bg card index 1
        let row = 33_usize;
        assert!(row >= bg_start_row && row < bg_start_row + bg_rows);
        assert_eq!(row - bg_start_row, 1);

        // Row 31 → NOT in bg card area (it's active content)
        let row = 31_usize;
        assert!(!(row >= bg_start_row && row < bg_start_row + bg_rows));

        // Row 34 → out of bounds
        let row = 34_usize;
        assert!(!(row >= bg_start_row && row < bg_start_row + bg_rows));
    }

    #[test]
    fn extract_osc7_cwd_parses_correctly() {
        assert_eq!(
            super::extract_osc7_cwd("\x1b]7;file://localhost/Users/alice/projects/myapp\x1b\\"),
            Some("/Users/alice/projects/myapp".to_string()),
        );
        assert_eq!(
            super::extract_osc7_cwd("\x1b]7;file:///tmp/test\x07"),
            Some("/tmp/test".to_string()),
        );
        assert_eq!(super::extract_osc7_cwd("normal output"), None);
    }
}

/// Update session name from child process cwd (throttled to once per second).
fn update_session_name_from_cwd(pipeline: &mut SessionPipeline) {
    // Throttle: only check once per second
    if pipeline.last_cwd_check.elapsed() < std::time::Duration::from_secs(1) {
        return;
    }
    pipeline.last_cwd_check = std::time::Instant::now();

    #[cfg(target_os = "macos")]
    if let Some(pid) = pipeline.child_pid {
        if let Some(cwd) = crate::session::pty::child_cwd(pid as i32) {
            if let Some(dir_name) = cwd.file_name().map(|n| n.to_string_lossy().to_string()) {
                if pipeline.project_name != dir_name {
                    pipeline.project_name = dir_name;
                }
            }
        }
    }
}

/// Extract the working directory path from an OSC 7 escape sequence.
///
/// Format: `ESC ] 7 ; file://hostname/path ST`
/// where ST is `ESC \` or `BEL (\x07)`.
#[allow(dead_code)]
fn extract_osc7_cwd(text: &str) -> Option<String> {
    // Look for OSC 7 pattern: \x1b]7;file://...path... followed by \x1b\\ or \x07
    let marker = "\x1b]7;";
    let start = text.find(marker)?;
    let rest = &text[start + marker.len()..];

    // Find the string terminator (ESC \ or BEL)
    let end = rest
        .find("\x1b\\")
        .or_else(|| rest.find('\x07'))?;
    let url = &rest[..end];

    // Parse file:// URL to extract path
    if let Some(path_start) = url.strip_prefix("file://") {
        // Skip hostname (everything up to the next '/')
        if let Some(slash_pos) = path_start.find('/') {
            return Some(path_start[slash_pos..].to_string());
        }
    }

    None
}
