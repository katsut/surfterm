//! Wrapper around `alacritty_terminal::Term` for VT parsing and cell buffer access.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use alacritty_terminal::Term;
use tracing::instrument;

/// RGB color value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[allow(dead_code)]
impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// A single cell in the terminal grid with resolved attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct TerminalCell {
    pub c: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// This cell is a wide character (occupies 2 columns).
    pub wide: bool,
    /// This cell is a spacer for the preceding wide character (should be skipped).
    pub wide_spacer: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Rgb::new(0xcd, 0xd6, 0xf4), // Catppuccin text
            bg: Rgb::new(0x1e, 0x1e, 0x2e),  // Catppuccin base
            bold: false,
            italic: false,
            underline: false,
            wide: false,
            wide_spacer: false,
        }
    }
}

/// Snapshot of visible terminal content.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TerminalContent {
    pub rows: Vec<Vec<TerminalCell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

/// Default 256-color table used to resolve indexed colors.
///
/// Colors 0-7: standard ANSI, 8-15: bright ANSI, 16-231: 6x6x6 cube, 232-255: grayscale ramp.
fn default_color_table() -> [Rgb; 256] {
    let mut table = [Rgb::new(0, 0, 0); 256];

    // Use Catppuccin Mocha ANSI palette (softer than classic xterm).
    table[0] = Rgb::new(0x45, 0x47, 0x5a);  // Black
    table[1] = Rgb::new(0xf3, 0x8b, 0xa8);  // Red
    table[2] = Rgb::new(0xa6, 0xe3, 0xa1);  // Green
    table[3] = Rgb::new(0xf9, 0xe2, 0xaf);  // Yellow
    table[4] = Rgb::new(0x89, 0xb4, 0xfa);  // Blue
    table[5] = Rgb::new(0xf5, 0xc2, 0xe7);  // Magenta
    table[6] = Rgb::new(0x94, 0xe2, 0xd5);  // Cyan
    table[7] = Rgb::new(0xba, 0xc2, 0xde);  // White

    // Bright ANSI colors (8-15).
    table[8] = Rgb::new(0x58, 0x5b, 0x70);  // BrightBlack
    table[9] = Rgb::new(0xf3, 0x8b, 0xa8);  // BrightRed
    table[10] = Rgb::new(0xa6, 0xe3, 0xa1); // BrightGreen
    table[11] = Rgb::new(0xf9, 0xe2, 0xaf); // BrightYellow
    table[12] = Rgb::new(0x89, 0xb4, 0xfa); // BrightBlue
    table[13] = Rgb::new(0xf5, 0xc2, 0xe7); // BrightMagenta
    table[14] = Rgb::new(0x94, 0xe2, 0xd5); // BrightCyan
    table[15] = Rgb::new(0xa6, 0xad, 0xc8); // BrightWhite

    // 6x6x6 color cube (16-231).
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                let idx = 16 + (r as usize) * 36 + (g as usize) * 6 + (b as usize);
                let to_val = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                table[idx] = Rgb::new(to_val(r), to_val(g), to_val(b));
            }
        }
    }

    // Grayscale ramp (232-255).
    for i in 0..24u8 {
        let val = 8 + 10 * i;
        table[232 + i as usize] = Rgb::new(val, val, val);
    }

    table
}

/// Resolve an alacritty `Color` to our `Rgb` using the default color table.
fn resolve_color(color: &Color, color_table: &[Rgb; 256]) -> Rgb {
    match color {
        Color::Spec(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
        Color::Indexed(idx) => color_table[*idx as usize],
        Color::Named(named) => {
            let idx = *named as usize;
            if idx < 256 {
                color_table[idx]
            } else {
                // Foreground, Background, Cursor, and other special named colors
                // fall back to sensible defaults.
                match named {
                    NamedColor::Foreground | NamedColor::BrightForeground => {
                        Rgb::new(0xcd, 0xd6, 0xf4) // Catppuccin text
                    }
                    NamedColor::Background => Rgb::new(0x1a, 0x2e, 0x1a), // theme bg
                    NamedColor::Cursor => Rgb::new(0xf5, 0xe0, 0xdc),
                    _ => Rgb::new(0xcd, 0xd6, 0xf4),
                }
            }
        }
    }
}

/// Terminal dimensions for creating an `alacritty_terminal::Term`.
struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Wrapper around `alacritty_terminal::Term` providing a simplified API for
/// feeding PTY output, resizing, and extracting visible cell content.
#[allow(dead_code)]
pub struct Terminal {
    term: Term<VoidListener>,
    processor: Processor,
    color_table: [Rgb; 256],
}

#[allow(dead_code)]
impl Terminal {
    /// Create a new terminal emulator with the given dimensions.
    #[instrument(skip_all, fields(cols, rows))]
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        let config = Config::default();
        let term = Term::new(config, &size, VoidListener);
        let processor = Processor::new();

        Self {
            term,
            processor,
            color_table: default_color_table(),
        }
    }

    /// Feed raw PTY output bytes into the terminal emulator.
    ///
    /// The bytes are parsed through the VT state machine and applied to the
    /// internal grid.
    #[instrument(skip(self, data), fields(len = data.len()))]
    pub fn feed(&mut self, data: &[u8]) {
        self.processor.advance(&mut self.term, data);
    }

    /// Resize the terminal to new dimensions.
    #[instrument(skip(self))]
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        self.term.resize(size);
    }

    /// Extract visible terminal content as a snapshot of cells with resolved
    /// colors and text attributes.
    #[instrument(skip(self))]
    pub fn content(&self) -> TerminalContent {
        let grid = self.term.grid();
        let num_lines = grid.screen_lines();
        let num_cols = grid.columns();

        let mut rows = Vec::with_capacity(num_lines);

        for line_idx in 0..num_lines {
            let line = Line(line_idx as i32);
            let row = &grid[line];
            let mut cells = Vec::with_capacity(num_cols);

            for col_idx in 0..num_cols {
                let col = Column(col_idx);
                let cell = &row[col];

                let fg = resolve_color(&cell.fg, &self.color_table);
                let bg = resolve_color(&cell.bg, &self.color_table);

                cells.push(TerminalCell {
                    c: cell.c,
                    fg,
                    bg,
                    bold: cell.flags.contains(Flags::BOLD),
                    italic: cell.flags.contains(Flags::ITALIC),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    wide: cell.flags.contains(Flags::WIDE_CHAR),
                    wide_spacer: cell.flags.contains(Flags::WIDE_CHAR_SPACER),
                });
            }

            rows.push(cells);
        }

        let cursor = &grid.cursor.point;
        let cursor_row = cursor.line.0.max(0) as usize;
        let cursor_col = cursor.column.0;

        TerminalContent {
            rows,
            cursor_row,
            cursor_col,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_hello_populates_cells() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"hello");

        let content = term.content();
        assert_eq!(content.rows.len(), 24);
        assert_eq!(content.rows[0].len(), 80);

        // First row should contain 'h', 'e', 'l', 'l', 'o'.
        let first_row = &content.rows[0];
        assert_eq!(first_row[0].c, 'h');
        assert_eq!(first_row[1].c, 'e');
        assert_eq!(first_row[2].c, 'l');
        assert_eq!(first_row[3].c, 'l');
        assert_eq!(first_row[4].c, 'o');

        // Remaining cells should be spaces.
        assert_eq!(first_row[5].c, ' ');
    }

    #[test]
    fn feed_color_escape_sets_fg() {
        let mut term = Terminal::new(80, 24);
        // ESC[31m sets foreground to red, then write 'R'.
        term.feed(b"\x1b[31mR");

        let content = term.content();
        let cell = &content.rows[0][0];
        assert_eq!(cell.c, 'R');
        // Named red (index 1) = Rgb(205, 0, 0).
        assert_eq!(cell.fg, Rgb::new(205, 0, 0));
    }

    #[test]
    fn feed_256_color_escape() {
        let mut term = Terminal::new(80, 24);
        // ESC[38;5;46m sets fg to color index 46 (bright green from cube).
        term.feed(b"\x1b[38;5;46mG");

        let content = term.content();
        let cell = &content.rows[0][0];
        assert_eq!(cell.c, 'G');
        // Index 46 = 16 + 1*36 + 5*6 + 4 = 16 + 36 + 30 + 4 = ... let's compute:
        // Actually index 46 = 16 + 30 = r=1,g=5,b=0 → (95, 255, 0).
        // 46 - 16 = 30; 30 / 36 = 0 remainder 30; 30 / 6 = 5 remainder 0.
        // So r=0, g=5, b=0 → (0, 255, 0).
        assert_eq!(cell.fg, Rgb::new(0, 255, 0));
    }

    #[test]
    fn feed_truecolor_escape() {
        let mut term = Terminal::new(80, 24);
        // ESC[38;2;100;150;200m sets fg to RGB(100, 150, 200).
        term.feed(b"\x1b[38;2;100;150;200mT");

        let content = term.content();
        let cell = &content.rows[0][0];
        assert_eq!(cell.c, 'T');
        assert_eq!(cell.fg, Rgb::new(100, 150, 200));
    }

    #[test]
    fn feed_bold_and_italic() {
        let mut term = Terminal::new(80, 24);
        // ESC[1m = bold, ESC[3m = italic.
        term.feed(b"\x1b[1;3mB");

        let content = term.content();
        let cell = &content.rows[0][0];
        assert_eq!(cell.c, 'B');
        assert!(cell.bold);
        assert!(cell.italic);
        assert!(!cell.underline);
    }

    #[test]
    fn feed_underline() {
        let mut term = Terminal::new(80, 24);
        // ESC[4m = underline.
        term.feed(b"\x1b[4mU");

        let content = term.content();
        let cell = &content.rows[0][0];
        assert_eq!(cell.c, 'U');
        assert!(cell.underline);
    }

    #[test]
    fn cursor_movement() {
        let mut term = Terminal::new(80, 24);
        // Move cursor to row 3, col 5 (1-based) then write 'X'.
        term.feed(b"\x1b[3;5HX");

        let content = term.content();
        // Row 2 (0-based), Col 4 (0-based) should be 'X'.
        assert_eq!(content.rows[2][4].c, 'X');
        // Row 0, Col 0 should still be space (untouched).
        assert_eq!(content.rows[0][0].c, ' ');
    }

    #[test]
    fn resize_works() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"hello");
        term.resize(40, 12);

        let content = term.content();
        assert_eq!(content.rows.len(), 12);
        assert_eq!(content.rows[0].len(), 40);
    }

    #[test]
    fn invalid_sequences_do_not_crash() {
        let mut term = Terminal::new(80, 24);
        // Feed a bunch of garbage bytes including invalid escape sequences.
        term.feed(b"\x1b[999;999H");
        term.feed(b"\x1b[?99999h");
        term.feed(b"\x1b]9999;\x07");
        term.feed(b"\xff\xfe\xfd\xfc");
        term.feed(b"\x1b[38;2;999;999;999m");
        // Should not crash; just verify we can still read content.
        let content = term.content();
        assert_eq!(content.rows.len(), 24);
    }

    #[test]
    fn scroll_with_newlines() {
        let mut term = Terminal::new(80, 5);
        // Write lines that exceed the screen height to trigger scrolling.
        term.feed(b"line1\r\nline2\r\nline3\r\nline4\r\nline5\r\nline6");

        let content = term.content();
        // After scrolling, line6 should be visible on the last line.
        let last_row = &content.rows[4];
        assert_eq!(last_row[0].c, 'l');
        assert_eq!(last_row[1].c, 'i');
        assert_eq!(last_row[2].c, 'n');
        assert_eq!(last_row[3].c, 'e');
        assert_eq!(last_row[4].c, '6');
    }
}
