use std::sync::Arc;

use anyhow::Result;
use tracing::{info, instrument};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

use crate::renderer::Renderer;

/// Application event types for inter-component communication.
#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    /// Request a redraw of the window.
    RequestRedraw,
}

/// Main application state holding the window, renderer, and tokio handle.
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    _event_proxy: EventLoopProxy<AppEvent>,
    _tokio_handle: tokio::runtime::Handle,
}

impl App {
    fn new(event_proxy: EventLoopProxy<AppEvent>, tokio_handle: tokio::runtime::Handle) -> Self {
        Self {
            window: None,
            renderer: None,
            _event_proxy: event_proxy,
            _tokio_handle: tokio_handle,
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

        // Block on async renderer initialization using the tokio handle.
        let renderer = match self._tokio_handle.block_on(Renderer::new(Arc::clone(&window))) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to initialize renderer: {e}");
                event_loop.exit();
                return;
            }
        };

        info!(
            width = renderer.size.width,
            height = renderer.size.height,
            "Window and renderer initialized"
        );

        self.window = Some(window);
        self.renderer = Some(renderer);
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
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(e) = renderer.render() {
                        tracing::error!("Render error: {e}");
                    }
                }
            }
            WindowEvent::KeyboardInput { .. } => {
                // Phase 1: keyboard input handling will be added later
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
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
    // Build tokio runtime on a dedicated thread so winit owns the main thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let tokio_handle = runtime.handle().clone();

    // Keep the runtime alive by moving it into a background thread.
    std::thread::spawn(move || {
        runtime.block_on(async {
            // The runtime stays alive until the app exits.
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
