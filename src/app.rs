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
use crate::renderer::Renderer;
use crate::session::pty::PtyHandle;
use crate::session::stream_splitter::StreamSplitter;
use crate::session::terminal::Terminal;

/// Application event types for inter-component communication.
#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    /// New PTY output data arrived.
    PtyOutput(Vec<u8>),
    /// Request a redraw of the window.
    RequestRedraw,
}

/// Main application state.
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    event_proxy: EventLoopProxy<AppEvent>,
    tokio_handle: tokio::runtime::Handle,
    // Session components
    terminal: Option<Terminal>,
    pty_writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    pty_master: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>,
    splitter: Option<StreamSplitter>,
    detector: Option<StateDetector>,
    input_handler: InputHandler,
    // Terminal dimensions
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
            terminal: None,
            pty_writer: None,
            pty_master: None,
            splitter: None,
            detector: None,
            input_handler: InputHandler::new(),
            cols: 80,
            rows: 24,
        }
    }

    /// Spawn a PTY session and wire up the output pipeline.
    fn spawn_session(&mut self) {
        let cols = self.cols;
        let rows = self.rows;

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
                return;
            }
        };

        info!("PTY session spawned ({cols}x{rows})");

        // Grab writer and master before moving pty into the reader task
        let writer = pty.writer();
        let master = pty.master();

        // Spawn async task to read PTY output and forward to event loop
        let proxy = self.event_proxy.clone();
        self.tokio_handle.spawn(async move {
            while let Some(data) = pty.read_output().await {
                if proxy.send_event(AppEvent::PtyOutput(data)).is_err() {
                    break;
                }
            }
            info!("PTY output stream ended");
        });

        self.terminal = Some(terminal);
        self.pty_writer = Some(writer);
        self.pty_master = Some(master);
        self.splitter = Some(splitter);
        self.detector = Some(detector);
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

        // Calculate terminal dimensions from renderer grid
        self.cols = renderer.grid.cols;
        self.rows = renderer.grid.rows;

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
                    self.cols = renderer.grid.cols;
                    self.rows = renderer.grid.rows;
                }
                if let Some(terminal) = self.terminal.as_mut() {
                    terminal.resize(self.cols, self.rows);
                }
                // Resize PTY
                if let Some(master) = self.pty_master.as_ref() {
                    let master = Arc::clone(master);
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
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(renderer), Some(terminal)) =
                    (self.renderer.as_mut(), self.terminal.as_ref())
                {
                    let content = terminal.content();
                    if let Err(e) = renderer.render_content(&content) {
                        tracing::error!("Render error: {e}");
                    }
                } else if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(e) = renderer.render() {
                        tracing::error!("Render error: {e}");
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let action = self.input_handler.handle_key(&event);
                match action {
                    InputAction::SendToPty(data) => {
                        if let Some(writer) = self.pty_writer.as_ref() {
                            let writer = Arc::clone(writer);
                            self.tokio_handle.spawn(async move {
                                let mut w = writer.lock().await;
                                if let Err(e) = w.write_all(&data) {
                                    tracing::error!("Failed to write to PTY: {e}");
                                }
                                let _ = w.flush();
                            });
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
                        SurftermCmd::SwitchToNormal | SurftermCmd::SwitchToInsert => {}
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
            AppEvent::PtyOutput(data) => {
                // Feed to terminal emulator
                if let Some(terminal) = self.terminal.as_mut() {
                    terminal.feed(&data);
                }
                // Feed to StreamSplitter
                if let Some(splitter) = self.splitter.as_ref() {
                    splitter.classify_chunk(&data);
                }
                // Feed to StateDetector
                if let Some(detector) = self.detector.as_mut() {
                    detector.process_chunk(&data);
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.update_session_state(detector.current_state());
                    }
                }
                // Request redraw
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
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
