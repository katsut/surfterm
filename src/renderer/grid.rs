//! Grid layout calculations for terminal cell positioning and panel splitting.

use tracing::instrument;

/// A rectangle defined by its origin and dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Grid layout that maps terminal cells onto the pixel surface and splits the
/// screen into left (message) and right (state) panels.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GridLayout {
    /// Width of a single monospace cell in pixels.
    pub cell_width: f32,
    /// Height of a single monospace cell in pixels (includes line spacing).
    pub cell_height: f32,
    /// Number of columns that fit the surface.
    pub cols: u16,
    /// Number of rows that fit the surface.
    pub rows: u16,
    /// Ratio of the left panel width to total width (0.0..1.0). Default 0.7.
    pub left_panel_ratio: f32,
    /// Total surface width in pixels.
    surface_width: f32,
    /// Total surface height in pixels.
    surface_height: f32,
}

/// The ratio between cell height and font size (line height factor).
/// A monospace font at `font_size` pixels typically has a cell height of
/// roughly 1.4x the font size to accommodate ascenders, descenders, and
/// line spacing.
const LINE_HEIGHT_FACTOR: f32 = 1.4;

/// The ratio between cell width and font size for a monospace font.
/// Monospace glyphs are typically about 0.6x the font size wide.
const CELL_WIDTH_FACTOR: f32 = 0.6;

#[allow(dead_code)]
impl GridLayout {
    /// Create a new grid layout from the surface dimensions and desired font size.
    ///
    /// Cell dimensions are derived from the font metrics of a typical monospace
    /// font. Columns and rows are calculated to fill the surface.
    #[instrument(skip_all, fields(surface_width, surface_height, font_size))]
    pub fn new(surface_width: u32, surface_height: u32, font_size: f32) -> Self {
        let cell_width = (font_size * CELL_WIDTH_FACTOR).ceil();
        let cell_height = (font_size * LINE_HEIGHT_FACTOR).ceil();

        let sw = surface_width as f32;
        let sh = surface_height as f32;

        let cols = if cell_width > 0.0 {
            (sw / cell_width).floor() as u16
        } else {
            0
        };
        let rows = if cell_height > 0.0 {
            (sh / cell_height).floor() as u16
        } else {
            0
        };

        Self {
            cell_width,
            cell_height,
            cols,
            rows,
            left_panel_ratio: 0.7,
            surface_width: sw,
            surface_height: sh,
        }
    }

    /// Returns the pixel rectangle for the left (message) panel.
    #[instrument(skip(self))]
    pub fn left_panel_rect(&self) -> Rect {
        let width = (self.surface_width * self.left_panel_ratio).floor();
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: self.surface_height,
        }
    }

    /// Returns the pixel rectangle for the right (state) panel.
    #[instrument(skip(self))]
    pub fn right_panel_rect(&self) -> Rect {
        let left_width = (self.surface_width * self.left_panel_ratio).floor();
        Rect {
            x: left_width,
            y: 0.0,
            width: self.surface_width - left_width,
            height: self.surface_height,
        }
    }

    /// Returns the x-coordinate of the vertical divider between panels.
    #[instrument(skip(self))]
    pub fn divider_x(&self) -> f32 {
        (self.surface_width * self.left_panel_ratio).floor()
    }

    /// Returns the number of columns that fit within the left panel.
    pub fn left_panel_cols(&self) -> u16 {
        if self.cell_width > 0.0 {
            (self.left_panel_rect().width / self.cell_width).floor() as u16
        } else {
            0
        }
    }

    /// Returns the number of columns that fit within the right panel.
    pub fn right_panel_cols(&self) -> u16 {
        if self.cell_width > 0.0 {
            (self.right_panel_rect().width / self.cell_width).floor() as u16
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_dimensions_calculated_correctly() {
        let grid = GridLayout::new(1280, 800, 16.0);

        // cell_width = ceil(16.0 * 0.6) = ceil(9.6) = 10
        assert_eq!(grid.cell_width, 10.0);
        // cell_height = ceil(16.0 * 1.4) = ceil(22.4) = 23
        assert_eq!(grid.cell_height, 23.0);

        // cols = floor(1280 / 10) = 128
        assert_eq!(grid.cols, 128);
        // rows = floor(800 / 23) = 34
        assert_eq!(grid.rows, 34);
    }

    #[test]
    fn panel_rects_respect_ratio() {
        let grid = GridLayout::new(1000, 500, 14.0);

        let left = grid.left_panel_rect();
        let right = grid.right_panel_rect();

        // Left panel starts at x=0
        assert_eq!(left.x, 0.0);
        assert_eq!(left.y, 0.0);
        assert_eq!(left.height, 500.0);

        // Right panel starts where left ends
        assert!((right.x - left.width).abs() < f32::EPSILON);
        assert_eq!(right.y, 0.0);
        assert_eq!(right.height, 500.0);

        // Total width should cover the surface
        assert!((left.width + right.width - 1000.0).abs() < 1.0);
    }

    #[test]
    fn divider_x_matches_panel_boundary() {
        let grid = GridLayout::new(1280, 800, 16.0);
        let left = grid.left_panel_rect();
        assert!((grid.divider_x() - left.width).abs() < f32::EPSILON);
    }

    #[test]
    fn default_ratio_is_seventy_thirty() {
        let grid = GridLayout::new(1000, 500, 14.0);
        assert!((grid.left_panel_ratio - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn zero_size_surface_does_not_panic() {
        let grid = GridLayout::new(0, 0, 16.0);
        assert_eq!(grid.cols, 0);
        assert_eq!(grid.rows, 0);
    }

    #[test]
    fn panel_cols_sum_reasonable() {
        let grid = GridLayout::new(1280, 800, 16.0);
        let left_cols = grid.left_panel_cols();
        let right_cols = grid.right_panel_cols();
        // The sum should be close to total cols (some pixels may be lost to rounding)
        assert!(left_cols + right_cols <= grid.cols + 1);
        assert!(left_cols > right_cols); // 70:30 ratio
    }
}
