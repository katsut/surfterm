//! Message panel and display mode management for the terminal UI.
//!
//! The `MessagePanel` accumulates classified message chunks and converts them
//! into `TerminalCell` rows for rendering. `DisplayMode` controls whether the
//! split panel view or raw VT output is shown.

use crate::session::terminal::{Rgb, TerminalCell};

/// User input foreground color: light green (#a6e3a1, Catppuccin green).
const USER_INPUT_FG: Rgb = Rgb::new(0xa6, 0xe3, 0xa1);

/// AI response foreground color: white (#cdd6f4, Catppuccin text).
const AI_RESPONSE_FG: Rgb = Rgb::new(0xcd, 0xd6, 0xf4);

/// Default background color (transparent/black — actual bg is rendered by the clear pass).
const DEFAULT_BG: Rgb = Rgb::new(0x1e, 0x1e, 0x2e);

/// A single message entry in the panel history.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct MessageEntry {
    pub text: String,
    pub is_user_input: bool,
}

/// Display mode for the main content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DisplayMode {
    /// Split panel view: Message panel (left) + State panel (right).
    Panels,
    /// Raw VT output across the entire window.
    Raw,
}

/// Toggle the display mode between `Panels` and `Raw`.
#[allow(dead_code)]
pub fn toggle_display_mode(current: &DisplayMode) -> DisplayMode {
    match current {
        DisplayMode::Panels => DisplayMode::Raw,
        DisplayMode::Raw => DisplayMode::Panels,
    }
}

/// Accumulates message history and converts it to terminal cells for rendering.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MessagePanel {
    /// Accumulated message history.
    pub messages: Vec<MessageEntry>,
    /// Number of visual lines to skip from the top (scroll offset).
    pub scroll_offset: usize,
    /// When true, new messages automatically scroll to the bottom.
    pub auto_scroll: bool,
}

#[allow(dead_code)]
impl MessagePanel {
    /// Create a new empty message panel with auto-scroll enabled.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    /// Add a message to the panel. If auto-scroll is enabled, the scroll
    /// offset is adjusted so the newest content is visible.
    pub fn push_message(&mut self, text: String, is_user_input: bool) {
        self.messages.push(MessageEntry {
            text,
            is_user_input,
        });
        if self.auto_scroll {
            // Scroll offset will be clamped in to_terminal_cells; set to max.
            self.scroll_offset = usize::MAX;
        }
    }

    /// Convert the message history into a grid of `TerminalCell` rows suitable
    /// for rendering.
    ///
    /// Long lines are wrapped at `cols` characters. The result is clipped to
    /// `rows` visible lines starting from `scroll_offset`.
    pub fn to_terminal_cells(&self, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }

        let cols = cols as usize;
        let rows = rows as usize;

        // Build all visual lines from messages.
        let mut visual_lines: Vec<(String, bool)> = Vec::new();

        for entry in &self.messages {
            let wrapped = wrap_text(&entry.text, cols);
            if wrapped.is_empty() {
                // Empty message -> blank line
                visual_lines.push((String::new(), entry.is_user_input));
            } else {
                for line in wrapped {
                    visual_lines.push((line, entry.is_user_input));
                }
            }
        }

        // Clamp scroll offset.
        let total_lines = visual_lines.len();
        let effective_offset = if total_lines <= rows {
            0
        } else {
            let max_offset = total_lines - rows;
            self.scroll_offset.min(max_offset)
        };

        // Take the visible window of lines.
        let visible: Vec<_> = visual_lines
            .into_iter()
            .skip(effective_offset)
            .take(rows)
            .collect();

        // Convert to TerminalCell rows.
        let mut result = Vec::with_capacity(rows);
        for (line, is_user) in &visible {
            let fg = if *is_user { USER_INPUT_FG } else { AI_RESPONSE_FG };
            let mut row = Vec::with_capacity(cols);
            for ch in line.chars().take(cols) {
                row.push(TerminalCell {
                    c: ch,
                    fg,
                    bg: DEFAULT_BG,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            // Pad remaining columns with spaces.
            while row.len() < cols {
                row.push(TerminalCell {
                    c: ' ',
                    fg,
                    bg: DEFAULT_BG,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            result.push(row);
        }

        // Pad remaining rows with blank lines if content is shorter than viewport.
        while result.len() < rows {
            let mut row = Vec::with_capacity(cols);
            for _ in 0..cols {
                row.push(TerminalCell {
                    c: ' ',
                    fg: AI_RESPONSE_FG,
                    bg: DEFAULT_BG,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            result.push(row);
        }

        result
    }

    /// Scroll up by one line. Disables auto-scroll.
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
        self.auto_scroll = false;
    }

    /// Scroll down by one line. Re-enables auto-scroll if at the bottom.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
        // auto_scroll is re-enabled when to_terminal_cells clamps to max
        // (caller can check), but we leave it disabled here; the user is
        // still manually scrolling.
    }
}

/// Wrap a string into lines of at most `cols` characters.
fn wrap_text(text: &str, cols: usize) -> Vec<String> {
    if cols == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let chars: Vec<char> = raw_line.chars().collect();
        for chunk in chars.chunks(cols) {
            lines.push(chunk.iter().collect());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_messages_and_verify_cell_output() {
        let mut panel = MessagePanel::new();
        panel.push_message("Hello".to_string(), false);
        panel.push_message("Hi".to_string(), true);

        let cells = panel.to_terminal_cells(10, 5);
        assert_eq!(cells.len(), 5);
        // First row: AI message "Hello"
        assert_eq!(cells[0][0].c, 'H');
        assert_eq!(cells[0][1].c, 'e');
        assert_eq!(cells[0][4].c, 'o');
        // Second row: user input "Hi"
        assert_eq!(cells[1][0].c, 'H');
        assert_eq!(cells[1][1].c, 'i');
        assert_eq!(cells[1][2].c, ' '); // padding
    }

    #[test]
    fn user_input_vs_ai_response_different_colors() {
        let mut panel = MessagePanel::new();
        panel.push_message("AI says hello".to_string(), false);
        panel.push_message("User says hi".to_string(), true);

        let cells = panel.to_terminal_cells(20, 5);

        // AI response row: white fg
        assert_eq!(cells[0][0].fg, AI_RESPONSE_FG);
        // User input row: green fg
        assert_eq!(cells[1][0].fg, USER_INPUT_FG);
        // They should be different
        assert_ne!(cells[0][0].fg, cells[1][0].fg);
    }

    #[test]
    fn display_mode_toggle() {
        let mode = DisplayMode::Panels;
        let toggled = toggle_display_mode(&mode);
        assert_eq!(toggled, DisplayMode::Raw);

        let toggled_back = toggle_display_mode(&toggled);
        assert_eq!(toggled_back, DisplayMode::Panels);
    }

    #[test]
    fn auto_scroll_moves_to_bottom() {
        let mut panel = MessagePanel::new();
        assert!(panel.auto_scroll);

        // Add more messages than the viewport can show.
        for i in 0..20 {
            panel.push_message(format!("Message {i}"), false);
        }

        let cells = panel.to_terminal_cells(20, 5);
        // Last visible row should contain the last message.
        let last_row_text: String = cells[4].iter().map(|c| c.c).collect::<String>();
        assert!(
            last_row_text.trim().starts_with("Message 19"),
            "Expected last message, got: '{}'",
            last_row_text.trim()
        );
    }

    #[test]
    fn scroll_up_disables_auto_scroll() {
        let mut panel = MessagePanel::new();
        for i in 0..20 {
            panel.push_message(format!("Line {i}"), false);
        }
        assert!(panel.auto_scroll);

        panel.scroll_up();
        assert!(!panel.auto_scroll);
    }

    #[test]
    fn line_wrapping() {
        let mut panel = MessagePanel::new();
        // A 15-char string in a 10-col viewport should wrap into 2 lines.
        panel.push_message("ABCDEFGHIJKLMNO".to_string(), false);

        let cells = panel.to_terminal_cells(10, 5);
        // First row: ABCDEFGHIJ
        let row0: String = cells[0].iter().map(|c| c.c).collect();
        assert_eq!(row0, "ABCDEFGHIJ");
        // Second row: KLMNO + padding
        let row1: String = cells[1].iter().take(5).map(|c| c.c).collect();
        assert_eq!(row1, "KLMNO");
    }

    #[test]
    fn empty_panel_returns_blank_rows() {
        let panel = MessagePanel::new();
        let cells = panel.to_terminal_cells(10, 3);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].len(), 10);
        // All spaces
        for row in &cells {
            for cell in row {
                assert_eq!(cell.c, ' ');
            }
        }
    }

    #[test]
    fn zero_dimensions_returns_empty() {
        let panel = MessagePanel::new();
        let cells = panel.to_terminal_cells(0, 0);
        assert!(cells.is_empty());
    }

    #[test]
    fn wrap_text_basic() {
        let lines = wrap_text("Hello World!", 5);
        assert_eq!(lines, vec!["Hello", " Worl", "d!"]);
    }

    #[test]
    fn wrap_text_with_newlines() {
        let lines = wrap_text("AB\nCD", 10);
        assert_eq!(lines, vec!["AB", "CD"]);
    }

    #[test]
    fn wrap_text_empty() {
        let lines = wrap_text("", 10);
        assert_eq!(lines, vec![""]);
    }
}
