pub mod grid;
pub mod panel;
pub mod text;

use std::sync::Arc;

use anyhow::Result;
use glyphon::TextBounds;
use tracing::instrument;
use winit::window::Window;

use crate::session::state::SessionState;
use crate::session::terminal::TerminalContent;

use self::grid::GridLayout;
use self::panel::{DisplayMode, MessagePanel, StatePanel};
use self::text::TextRenderer;

/// Default font size in pixels for terminal cell rendering.
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// GPU renderer managing wgpu surface, device, queue, grid layout, and text
/// rendering pipeline.
#[allow(dead_code)]
pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub grid: GridLayout,
    pub text_renderer: TextRenderer,
    pub display_mode: DisplayMode,
    pub message_panel: MessagePanel,
    pub state_panel: StatePanel,
}

impl Renderer {
    /// Initialize wgpu: request adapter, device, and configure the surface.
    #[instrument(skip_all)]
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window)
            .map_err(|e| anyhow::anyhow!("Failed to create wgpu surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find a suitable GPU adapter: {e}"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("surfterm_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create wgpu device: {e}"))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let grid = GridLayout::new(size.width.max(1), size.height.max(1), DEFAULT_FONT_SIZE);
        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        tracing::info!(
            width = size.width,
            height = size.height,
            cols = grid.cols,
            rows = grid.rows,
            format = ?surface_format,
            "wgpu renderer initialized"
        );

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size,
            grid,
            text_renderer,
            display_mode: DisplayMode::Panels,
            message_panel: MessagePanel::new(),
            state_panel: StatePanel::new(),
        })
    }

    /// Reconfigure the surface after a window resize.
    #[instrument(skip(self))]
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.grid = GridLayout::new(new_size.width, new_size.height, DEFAULT_FONT_SIZE);
            tracing::debug!(
                width = new_size.width,
                height = new_size.height,
                cols = self.grid.cols,
                rows = self.grid.rows,
                "Surface and grid resized"
            );
        }
    }

    /// Acquire the current surface texture.
    fn acquire_surface_texture(&self) -> Result<wgpu::SurfaceTexture> {
        self.surface
            .get_current_texture()
            .map_err(|e| anyhow::anyhow!("Failed to acquire surface texture: {e}"))
    }

    /// Render a frame: clear the screen with the dark theme background color (#1e1e2e).
    /// This is a simple clear-only fallback when no terminal content is available.
    #[instrument(skip(self))]
    pub fn render(&mut self) -> Result<()> {
        let output = self.acquire_surface_texture()?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surfterm_encoder"),
            });

        // Dark theme background: #1e1e2e (Catppuccin Mocha base)
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0x1e as f64 / 255.0,
                            g: 0x1e as f64 / 255.0,
                            b: 0x2e as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render terminal content with panel layout:
    #[allow(dead_code)]
    /// 1. Clear with dark background
    /// 2. Draw a vertical divider between left and right panels
    /// 3. Render terminal cells in the left panel area
    ///
    /// In `DisplayMode::Raw`, the full `TerminalContent` is rendered across
    /// the entire window with no panel split. In `DisplayMode::Panels`, the
    /// message panel occupies the left side and the raw content occupies the
    /// right side.
    #[instrument(skip(self, content))]
    pub fn render_content(&mut self, content: &TerminalContent) -> Result<()> {
        let output = self.acquire_surface_texture()?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surfterm_content_encoder"),
            });

        // Pass 1: Clear background (#1e1e2e Catppuccin Mocha base)
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0x1e as f64 / 255.0,
                            g: 0x1e as f64 / 255.0,
                            b: 0x2e as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // Pass 2: Render text content depending on display mode.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("text_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let surface_size = (self.config.width, self.config.height);

            match self.display_mode {
                DisplayMode::Raw => {
                    // Raw mode: render full TerminalContent across entire window.
                    let clip = TextBounds {
                        left: 0,
                        top: 0,
                        right: self.config.width as i32,
                        bottom: self.config.height as i32,
                    };

                    self.text_renderer.render_cells(
                        &self.device,
                        &self.queue,
                        &mut render_pass,
                        &self.grid,
                        &content.rows,
                        surface_size,
                        0.0,
                        0.0,
                        Some(clip),
                    )?;
                }
                DisplayMode::Panels => {
                    // Panels mode: message panel on left, raw content on right.
                    let left_rect = self.grid.left_panel_rect();
                    let left_clip = TextBounds {
                        left: left_rect.x as i32,
                        top: left_rect.y as i32,
                        right: (left_rect.x + left_rect.width) as i32,
                        bottom: (left_rect.y + left_rect.height) as i32,
                    };

                    // Render message panel cells in the left area.
                    let left_cols = self.grid.left_panel_cols();
                    let message_cells =
                        self.message_panel.to_terminal_cells(left_cols, self.grid.rows);

                    self.text_renderer.render_cells(
                        &self.device,
                        &self.queue,
                        &mut render_pass,
                        &self.grid,
                        &message_cells,
                        surface_size,
                        left_rect.x,
                        left_rect.y,
                        Some(left_clip),
                    )?;

                    // Render state panel cells in the right area.
                    let right_rect = self.grid.right_panel_rect();
                    let right_clip = TextBounds {
                        left: right_rect.x as i32,
                        top: right_rect.y as i32,
                        right: (right_rect.x + right_rect.width) as i32,
                        bottom: (right_rect.y + right_rect.height) as i32,
                    };

                    let right_cols = self.grid.right_panel_cols();
                    let state_cells =
                        self.state_panel.to_terminal_cells(right_cols, self.grid.rows);

                    self.text_renderer.render_cells(
                        &self.device,
                        &self.queue,
                        &mut render_pass,
                        &self.grid,
                        &state_cells,
                        surface_size,
                        right_rect.x,
                        right_rect.y,
                        Some(right_clip),
                    )?;
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Toggle between `Panels` and `Raw` display modes.
    #[allow(dead_code)]
    pub fn toggle_display_mode(&mut self) {
        self.display_mode = panel::toggle_display_mode(&self.display_mode);
    }

    /// Push a message into the message panel.
    #[allow(dead_code)]
    pub fn push_message(&mut self, text: String, is_user_input: bool) {
        self.message_panel.push_message(text, is_user_input);
    }

    /// Update the session state shown in the state panel.
    #[allow(dead_code)]
    pub fn update_session_state(&mut self, state: SessionState) {
        self.state_panel.update_state(state);
    }

    /// Push a state channel line into the state panel.
    #[allow(dead_code)]
    pub fn push_state_line(&mut self, line: String) {
        self.state_panel.push_state_line(line);
    }
}

#[cfg(test)]
mod tests {}
