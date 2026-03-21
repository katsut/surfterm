pub mod grid;
pub mod panel;
pub mod text;

use std::sync::Arc;

use anyhow::Result;
use tracing::instrument;
use winit::window::Window;

use crate::config::theme::SurftermTheme;
use crate::session::state::SessionState;
use crate::session::terminal::TerminalContent;

use self::grid::GridLayout;
use self::panel::{CardInfo, CardStack, DisplayMode, MessagePanel, PanelColors, SidePanel, SidePanelEntry, StatePanel};
use self::text::{RenderRegion, TextRenderer};

/// Default font size in logical pixels for terminal cell rendering.
/// This is multiplied by the window's scale factor for physical pixel rendering.
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
    pub side_panel: SidePanel,
    pub card_stack: CardStack,
    pub scale_factor: f32,
    pub theme: SurftermTheme,
    pub panel_colors: PanelColors,
}

impl Renderer {
    /// Initialize wgpu: request adapter, device, and configure the surface.
    #[instrument(skip_all)]
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;

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

        let physical_font_size = DEFAULT_FONT_SIZE * scale_factor;
        let grid = GridLayout::with_scale_factor(
            size.width.max(1),
            size.height.max(1),
            physical_font_size,
            scale_factor,
        );
        let mut text_renderer = TextRenderer::new(&device, &queue, surface_format);
        text_renderer.font_size = physical_font_size;

        tracing::info!(
            width = size.width,
            height = size.height,
            cols = grid.main_cols(),
            rows = grid.main_rows(),
            sidebar_cols = grid.sidebar_cols(),
            format = ?surface_format,
            "wgpu renderer initialized"
        );

        let theme = SurftermTheme::default();
        let panel_colors = PanelColors::from_theme(&theme);

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size,
            grid,
            text_renderer,
            display_mode: DisplayMode::Raw,
            message_panel: MessagePanel::new(),
            state_panel: StatePanel::new(),
            side_panel: SidePanel::new(),
            card_stack: CardStack::new(),
            scale_factor,
            theme,
            panel_colors,
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
            let physical_font_size = DEFAULT_FONT_SIZE * self.scale_factor;
            self.grid = GridLayout::with_scale_factor(
                new_size.width,
                new_size.height,
                physical_font_size,
                self.scale_factor,
            );
            tracing::debug!(
                width = new_size.width,
                height = new_size.height,
                main_cols = self.grid.main_cols(),
                main_rows = self.grid.main_rows(),
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

    /// Render a frame: clear the screen with the theme background color.
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

        let clear_color = self.theme.background_wgpu();
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
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

    /// Render terminal content with a stacked card layout in the main area
    /// and the side panel on the right.
    ///
    /// The active card occupies most of the main area with a title bar at row 0
    /// and terminal content starting from row 1. Background cards appear as
    /// title bar rows at the bottom, progressively offset to the right.
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

        let surface_size = (self.config.width, self.config.height);
        let sidebar_rect = self.grid.sidebar_rect();
        let main_rect = self.grid.main_rect();

        // Build sidebar cells from the side panel using theme colors.
        let sidebar_cells = self.side_panel.to_terminal_cells_themed(
            self.grid.sidebar_cols(),
            self.grid.main_rows(),
            self.scale_factor,
            &self.panel_colors,
        );

        let mut regions = Vec::with_capacity(4);

        // Sidebar region.
        if !sidebar_cells.is_empty() {
            regions.push(RenderRegion {
                cells: sidebar_cells,
                origin_x: sidebar_rect.x,
                origin_y: sidebar_rect.y,
                cell_width: self.grid.cell_width,
                cell_height: self.grid.cell_height,
            });
        }

        let main_cols = self.grid.main_cols() as usize;
        let main_rows = self.grid.main_rows() as usize;
        let num_bg_cards = self.card_stack.background_cards().len();

        // Calculate how many rows are reserved for background card tabs at the bottom.
        let bg_rows = num_bg_cards.min(main_rows.saturating_sub(2)); // Leave at least 2 rows for active card
        let active_content_rows = main_rows.saturating_sub(1 + bg_rows); // 1 for title bar

        // ── Active card title bar (row 0 of main area) ──
        if let Some(title_row) = self.card_stack.active_title_bar_themed(main_cols, &self.panel_colors) {
            regions.push(RenderRegion {
                cells: vec![title_row],
                origin_x: main_rect.x,
                origin_y: main_rect.y,
                cell_width: self.grid.cell_width,
                cell_height: self.grid.cell_height,
            });
        }

        // ── Active card terminal content (rows 1..1+active_content_rows) ──
        {
            // Clip terminal content to the available rows for the active card.
            let clipped_rows: Vec<Vec<_>> = content
                .rows
                .iter()
                .take(active_content_rows)
                .cloned()
                .collect();

            if !clipped_rows.is_empty() {
                regions.push(RenderRegion {
                    cells: clipped_rows,
                    origin_x: main_rect.x,
                    origin_y: main_rect.y + self.grid.cell_height, // offset by 1 row for title bar
                    cell_width: self.grid.cell_width,
                    cell_height: self.grid.cell_height,
                });
            }
        }

        // ── Background card header rows (at bottom of main area) ──
        {
            let bg_title_bars = self.card_stack.background_title_bars_themed(
                main_cols,
                self.scale_factor,
                self.grid.cell_width,
                &self.panel_colors,
            );

            for (i, (left_offset_cells, row_cells)) in bg_title_bars.into_iter().enumerate() {
                if i >= bg_rows {
                    break;
                }
                // Position: bottom of the active card area, one row per bg card
                let row_y_index = 1 + active_content_rows + i; // after title + content
                let origin_y = main_rect.y + row_y_index as f32 * self.grid.cell_height;
                let origin_x = main_rect.x + left_offset_cells as f32 * self.grid.cell_width;

                if !row_cells.is_empty() {
                    regions.push(RenderRegion {
                        cells: vec![row_cells],
                        origin_x,
                        origin_y,
                        cell_width: self.grid.cell_width,
                        cell_height: self.grid.cell_height,
                    });
                }
            }
        }

        self.text_renderer.render_grid_prepare(
            &self.device,
            &self.queue,
            &regions,
            surface_size,
        )?;

        // Single render pass: clear + text
        let clear_color = self.theme.background_wgpu();
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.text_renderer.render_pass(&mut render_pass)?;
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

    /// Update the side panel with new session entries.
    #[allow(dead_code)]
    pub fn update_side_panel(&mut self, entries: Vec<SidePanelEntry>) {
        self.side_panel.update_sessions(entries);
    }

    /// Update the card stack with session card information.
    /// The active session should be first in the list.
    #[allow(dead_code)]
    pub fn update_card_stack(&mut self, cards: Vec<CardInfo>) {
        self.card_stack.update(cards);
    }

    /// Set the theme and update derived panel colors.
    #[allow(dead_code)]
    pub fn set_theme(&mut self, theme: SurftermTheme) {
        self.panel_colors = PanelColors::from_theme(&theme);
        self.theme = theme;
    }

    /// Calculate the terminal dimensions for the active card content area.
    ///
    /// Returns `(cols, rows)` accounting for the title bar row and background
    /// card tab rows at the bottom.
    pub fn active_card_dimensions(&self) -> (u16, u16) {
        let main_cols = self.grid.main_cols();
        let main_rows = self.grid.main_rows();
        let num_bg_cards = self.card_stack.background_cards().len();
        let bg_rows = num_bg_cards.min((main_rows as usize).saturating_sub(2));
        let content_rows = (main_rows as usize).saturating_sub(1 + bg_rows) as u16;
        (main_cols, content_rows)
    }
}

#[cfg(test)]
mod tests {}
