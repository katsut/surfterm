//! Text rendering using glyphon (cosmic-text + wgpu glyph atlas).

use anyhow::Result;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, Viewport, Weight,
};
use tracing::instrument;

use crate::session::terminal::TerminalCell;

use super::grid::GridLayout;

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
    font_size: f32,
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
        }
    }

    /// Render terminal cells into the given render pass.
    ///
    /// Each row of `cells` is rendered at the correct grid position within the
    /// panel defined by `offset_x` and `offset_y`. Foreground colors and bold
    /// weight from `TerminalCell` are applied per-character.
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all)]
    pub fn render_cells(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        grid: &GridLayout,
        cells: &[Vec<TerminalCell>],
        surface_size: (u32, u32),
        offset_x: f32,
        offset_y: f32,
        clip_rect: Option<TextBounds>,
    ) -> Result<()> {
        let metrics = Metrics::new(self.font_size, grid.cell_height);

        // Build one glyphon Buffer per row, setting per-character attributes
        // for foreground color and weight.
        let mut buffers: Vec<Buffer> = Vec::with_capacity(cells.len());

        for row_cells in cells {
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                &mut self.font_system,
                Some(surface_size.0 as f32),
                Some(grid.cell_height),
            );

            // Collect the text for this row and build an AttrsList with
            // per-character spans.
            let text: String = row_cells.iter().map(|c| c.c).collect();

            // Build spans: group consecutive cells with the same attributes.
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

        // Build TextAreas, one per row, each positioned at its row offset.
        let text_areas: Vec<TextArea<'_>> = buffers
            .iter()
            .enumerate()
            .map(|(row_idx, buffer)| TextArea {
                buffer,
                left: offset_x,
                top: offset_y + row_idx as f32 * grid.cell_height,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(205, 214, 244), // Catppuccin text
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

        self.renderer
            .render(&self.atlas, &self.viewport, pass)?;

        self.atlas.trim();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Text rendering tests require a GPU context and cannot run in CI.
    // Structural correctness is validated via `cargo check` and `cargo clippy`.
}
