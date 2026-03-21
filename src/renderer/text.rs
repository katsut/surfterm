//! Text rendering using glyphon (cosmic-text + wgpu glyph atlas).

use anyhow::Result;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, Viewport, Weight,
};
use tracing::instrument;

use crate::session::terminal::TerminalCell;

use super::grid::GridLayout;

/// A rectangular region of terminal cells to be rendered at a specific position.
/// Each region has its own origin and cell dimensions, allowing side-by-side
/// placement of sidebar and main content.
pub struct RenderRegion {
    /// 2D grid of terminal cells (rows x cols).
    pub cells: Vec<Vec<TerminalCell>>,
    /// Physical pixel X offset for this region's top-left corner.
    pub origin_x: f32,
    /// Physical pixel Y offset for this region's top-left corner.
    pub origin_y: f32,
    /// Width of a single cell in physical pixels.
    pub cell_width: f32,
    /// Height of a single cell in physical pixels.
    pub cell_height: f32,
}

/// Text renderer wrapping glyphon's font system, glyph cache, texture atlas,
/// and text renderer pipeline.
#[allow(dead_code)]
pub struct TextRenderer {
    pub font_system: FontSystem,
    swash_cache: SwashCache,
    cache: Cache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: glyphon::TextRenderer,
    /// Font size in pixels used for terminal cell rendering.
    pub font_size: f32,
    /// Font family name. Empty string means system default monospace.
    pub font_family: String,
}

#[allow(dead_code)]
impl TextRenderer {
    /// Create a new text renderer bound to the given wgpu device and surface format.
    #[instrument(skip_all)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = glyphon::TextRenderer::new(
            &mut atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        Self {
            font_system,
            swash_cache,
            cache,
            viewport,
            atlas,
            renderer,
            font_size: 16.0,
            font_family: String::new(),
        }
    }

    /// Prepare multiple render regions using per-cell Buffer positioning.
    ///
    /// Each non-space cell gets its own glyphon Buffer placed at exact
    /// `(origin_x + col * cell_width, origin_y + row * cell_height)` coordinates.
    /// This ensures fixed-grid positioning regardless of proportional font metrics.
    #[instrument(skip_all)]
    pub fn render_grid_prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        regions: &[RenderRegion],
        surface_size: (u32, u32),
    ) -> Result<()> {
        // Collect all per-cell Buffers across all regions.
        // We store (Buffer, left, top) tuples so we can build TextAreas after.
        struct CellBuffer {
            buffer: Buffer,
            left: f32,
            top: f32,
        }

        let mut cell_buffers: Vec<CellBuffer> = Vec::new();

        let font_family_str = self.font_family.clone();

        for region in regions {
            let metrics = Metrics::new(self.font_size, region.cell_height);

            for (row_idx, row_cells) in region.cells.iter().enumerate() {
                for (col_idx, cell) in row_cells.iter().enumerate() {
                    // Skip empty/space cells for performance.
                    if cell.c == ' ' {
                        continue;
                    }

                    let mut buffer = Buffer::new(&mut self.font_system, metrics);
                    buffer.set_size(
                        &mut self.font_system,
                        Some(region.cell_width),
                        Some(region.cell_height),
                    );

                    let weight = if cell.bold {
                        Weight::BOLD
                    } else {
                        Weight::NORMAL
                    };

                    let family = if font_family_str.is_empty() {
                        Family::Monospace
                    } else {
                        Family::Name(&font_family_str)
                    };

                    let ch = cell.c.to_string();
                    let attrs = Attrs::new()
                        .family(family)
                        .weight(weight)
                        .color(Color::rgb(cell.fg.r, cell.fg.g, cell.fg.b));

                    buffer.set_rich_text(
                        &mut self.font_system,
                        [(&*ch, attrs)],
                        &Attrs::new().family(family),
                        Shaping::Advanced,
                        None,
                    );
                    buffer.shape_until_scroll(&mut self.font_system, false);

                    let left = region.origin_x + col_idx as f32 * region.cell_width;
                    let top = region.origin_y + row_idx as f32 * region.cell_height;

                    cell_buffers.push(CellBuffer { buffer, left, top });
                }
            }
        }

        // Update viewport resolution.
        self.viewport.update(
            queue,
            Resolution {
                width: surface_size.0,
                height: surface_size.1,
            },
        );

        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: surface_size.0 as i32,
            bottom: surface_size.1 as i32,
        };

        let text_areas: Vec<TextArea<'_>> = cell_buffers
            .iter()
            .map(|cb| TextArea {
                buffer: &cb.buffer,
                left: cb.left,
                top: cb.top,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(205, 214, 244),
                custom_glyphs: &[],
            })
            .collect();

        tracing::debug!(
            total_cell_buffers = cell_buffers.len(),
            num_regions = regions.len(),
            "render_grid_prepare"
        );

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        )?;

        self.atlas.trim();

        Ok(())
    }

    /// Prepare terminal cells for rendering (glyphon prepare step).
    /// Must be called before `render_pass()`.
    ///
    /// Deprecated: Use `render_grid_prepare` for correct cell-level positioning.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all)]
    pub fn render_cells_prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GridLayout,
        cells: &[Vec<TerminalCell>],
        surface_size: (u32, u32),
        offset_x: f32,
        offset_y: f32,
        clip_rect: Option<TextBounds>,
    ) -> Result<()> {
        let metrics = Metrics::new(self.font_size, grid.cell_height);

        let mut buffers: Vec<Buffer> = Vec::with_capacity(cells.len());

        for row_cells in cells {
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                &mut self.font_system,
                Some(surface_size.0 as f32),
                Some(grid.cell_height),
            );

            let text: String = row_cells.iter().map(|c| c.c).collect();
            let mut spans: Vec<(&str, Attrs)> = Vec::new();
            let mut byte_offset = 0;

            for cell in row_cells {
                let ch_str_start = byte_offset;
                byte_offset += cell.c.len_utf8();
                let ch_str = &text[ch_str_start..byte_offset];

                let weight = if cell.bold {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                };

                let attrs = Attrs::new()
                    .family(Family::Monospace)
                    .weight(weight)
                    .color(Color::rgb(cell.fg.r, cell.fg.g, cell.fg.b));

                spans.push((ch_str, attrs));
            }

            buffer.set_rich_text(
                &mut self.font_system,
                spans,
                &Attrs::new().family(Family::Monospace),
                Shaping::Basic,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push(buffer);
        }

        // Update viewport resolution.
        self.viewport.update(
            queue,
            Resolution {
                width: surface_size.0,
                height: surface_size.1,
            },
        );

        let bounds = clip_rect.unwrap_or(TextBounds {
            left: 0,
            top: 0,
            right: surface_size.0 as i32,
            bottom: surface_size.1 as i32,
        });

        let text_areas: Vec<TextArea<'_>> = buffers
            .iter()
            .enumerate()
            .map(|(row_idx, buffer)| TextArea {
                buffer,
                left: offset_x,
                top: offset_y + row_idx as f32 * grid.cell_height,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(205, 214, 244),
                custom_glyphs: &[],
            })
            .collect();

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        )?;

        self.atlas.trim();

        Ok(())
    }

    /// Build glyphon Buffers from terminal cells.
    #[allow(dead_code)]
    fn build_buffers(
        &mut self,
        grid: &GridLayout,
        cells: &[Vec<TerminalCell>],
        surface_width: f32,
    ) -> Vec<Buffer> {
        let metrics = Metrics::new(self.font_size, grid.cell_height);
        let mut buffers = Vec::with_capacity(cells.len());

        for row_cells in cells {
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                &mut self.font_system,
                Some(surface_width),
                Some(grid.cell_height),
            );

            let text: String = row_cells.iter().map(|c| c.c).collect();
            let mut spans: Vec<(&str, Attrs)> = Vec::new();
            let mut byte_offset = 0;

            for cell in row_cells {
                let ch_str_start = byte_offset;
                byte_offset += cell.c.len_utf8();
                let ch_str = &text[ch_str_start..byte_offset];

                let weight = if cell.bold {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                };

                let attrs = Attrs::new()
                    .family(Family::Monospace)
                    .weight(weight)
                    .color(Color::rgb(cell.fg.r, cell.fg.g, cell.fg.b));

                spans.push((ch_str, attrs));
            }

            buffer.set_rich_text(
                &mut self.font_system,
                spans,
                &Attrs::new().family(Family::Monospace),
                Shaping::Basic,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push(buffer);
        }

        buffers
    }

    /// Prepare two regions (sidebar + main) in a single glyphon prepare call.
    ///
    /// Deprecated: Use `render_grid_prepare` for correct cell-level positioning.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all)]
    pub fn render_two_regions_prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GridLayout,
        sidebar_cells: &[Vec<TerminalCell>],
        sidebar_x: f32,
        sidebar_y: f32,
        sidebar_bounds: TextBounds,
        main_cells: &[Vec<TerminalCell>],
        main_x: f32,
        main_y: f32,
        main_bounds: TextBounds,
        surface_size: (u32, u32),
    ) -> Result<()> {
        let sidebar_buffers = self.build_buffers(grid, sidebar_cells, grid.sidebar_width);
        let main_buffers = self.build_buffers(grid, main_cells, surface_size.0 as f32);

        self.viewport.update(
            queue,
            Resolution {
                width: surface_size.0,
                height: surface_size.1,
            },
        );

        let mut text_areas: Vec<TextArea<'_>> = Vec::new();

        // Sidebar text areas
        for (row_idx, buffer) in sidebar_buffers.iter().enumerate() {
            text_areas.push(TextArea {
                buffer,
                left: sidebar_x,
                top: sidebar_y + row_idx as f32 * grid.cell_height,
                scale: 1.0,
                bounds: sidebar_bounds,
                default_color: Color::rgb(205, 214, 244),
                custom_glyphs: &[],
            });
        }

        // Main text areas
        for (row_idx, buffer) in main_buffers.iter().enumerate() {
            text_areas.push(TextArea {
                buffer,
                left: main_x,
                top: main_y + row_idx as f32 * grid.cell_height,
                scale: 1.0,
                bounds: main_bounds,
                default_color: Color::rgb(205, 214, 244),
                custom_glyphs: &[],
            });
        }

        if !text_areas.is_empty() {
            let first = &text_areas[0];
            let last_sidebar = if !sidebar_buffers.is_empty() { &text_areas[sidebar_buffers.len() - 1] } else { first };
            let first_main = if sidebar_buffers.len() < text_areas.len() { &text_areas[sidebar_buffers.len()] } else { first };
            tracing::info!(
                total_areas = text_areas.len(),
                sidebar_areas = sidebar_buffers.len(),
                main_areas = main_buffers.len(),
                first_sidebar_left = first.left,
                first_sidebar_top = first.top,
                last_sidebar_top = last_sidebar.top,
                first_main_left = first_main.left,
                first_main_top = first_main.top,
                "TextArea layout"
            );
        }

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        )?;

        self.atlas.trim();

        Ok(())
    }

    /// Execute the glyphon render pass (must be called after prepare).
    #[instrument(skip_all)]
    pub fn render_pass(&mut self, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        self.renderer
            .render(&self.atlas, &self.viewport, pass)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Text rendering tests require a GPU context and cannot run in CI.
    // Structural correctness is validated via `cargo check` and `cargo clippy`.
}
