//! Message panel and display mode management for the terminal UI.
//!
//! The `MessagePanel` accumulates classified message chunks and converts them
//! into `TerminalCell` rows for rendering. `DisplayMode` controls whether the
//! split panel view or raw VT output is shown.

use crate::session::state::SessionState;
use crate::session::terminal::{Rgb, TerminalCell};

/// User input foreground color: light green (#a6e3a1, Catppuccin green).
const USER_INPUT_FG: Rgb = Rgb::new(0xa6, 0xe3, 0xa1);

/// AI response foreground color: white (#cdd6f4, Catppuccin text).
const AI_RESPONSE_FG: Rgb = Rgb::new(0xcd, 0xd6, 0xf4);

/// Default background color (transparent/black — actual bg is rendered by the clear pass).
const DEFAULT_BG: Rgb = Rgb::new(0x1e, 0x1e, 0x2e);

// ── State Panel colors ──

/// Header / separator dim color (#585b70, Catppuccin surface2).
const STATE_HEADER_FG: Rgb = Rgb::new(0x58, 0x5b, 0x70);

/// Running indicator: yellow (#f9e2af, Catppuccin yellow).
const STATE_RUNNING_FG: Rgb = Rgb::new(0xf9, 0xe2, 0xaf);

/// WaitingForInput indicator: green (#a6e3a1, Catppuccin green).
const STATE_WAITING_FG: Rgb = Rgb::new(0xa6, 0xe3, 0xa1);

/// Error indicator: red (#f38ba8, Catppuccin red).
const STATE_ERROR_FG: Rgb = Rgb::new(0xf3, 0x8b, 0xa8);

/// Idle indicator: gray (#6c7086, Catppuccin overlay0).
const STATE_IDLE_FG: Rgb = Rgb::new(0x6c, 0x70, 0x86);

/// Tool name: cyan (#89dceb, Catppuccin sky).
const STATE_TOOL_FG: Rgb = Rgb::new(0x89, 0xdc, 0xeb);

/// Normal info text: white (#cdd6f4, Catppuccin text).
const STATE_INFO_FG: Rgb = Rgb::new(0xcd, 0xd6, 0xf4);

/// Dim state lines: (#a6adc8, Catppuccin subtext0).
const STATE_DIM_FG: Rgb = Rgb::new(0xa6, 0xad, 0xc8);

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

/// State panel showing tool execution status, cost, tokens, and session state.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StatePanel {
    /// Current session state.
    pub session_state: SessionState,
    /// Currently executing tool name (if any).
    pub current_tool: Option<String>,
    /// Cost string (e.g. "$0.05").
    pub cost: Option<String>,
    /// Token count string.
    pub token_count: Option<String>,
    /// Recent state channel output lines.
    pub state_lines: Vec<String>,
    /// Maximum number of state lines to retain.
    pub max_lines: usize,
}

#[allow(dead_code)]
impl StatePanel {
    /// Create a new empty state panel.
    pub fn new() -> Self {
        Self {
            session_state: SessionState::Idle,
            current_tool: None,
            cost: None,
            token_count: None,
            state_lines: Vec::new(),
            max_lines: 100,
        }
    }

    /// Update the session state.
    pub fn update_state(&mut self, state: SessionState) {
        self.session_state = state;
    }

    /// Push a state channel line, extracting tool/cost/token info.
    pub fn push_state_line(&mut self, line: String) {
        if let Some(tool) = extract_tool_name(&line) {
            self.current_tool = Some(tool);
        }
        if let Some(cost) = extract_cost(&line) {
            self.cost = Some(cost);
        }
        if let Some(tokens) = extract_tokens(&line) {
            self.token_count = Some(tokens);
        }
        self.state_lines.push(line);
        if self.state_lines.len() > self.max_lines {
            let excess = self.state_lines.len() - self.max_lines;
            self.state_lines.drain(..excess);
        }
    }

    /// Render state panel content as terminal cells.
    pub fn to_terminal_cells(&self, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }

        let cols = cols as usize;
        let rows = rows as usize;
        let mut result: Vec<Vec<TerminalCell>> = Vec::with_capacity(rows);

        // Row 0: header
        result.push(make_row("── State ──", cols, STATE_HEADER_FG));

        // Row 1: state indicator
        if result.len() < rows {
            let (label, fg) = match self.session_state {
                SessionState::Running => ("● Running", STATE_RUNNING_FG),
                SessionState::WaitingForInput => ("● WaitingForInput", STATE_WAITING_FG),
                SessionState::Error => ("● Error", STATE_ERROR_FG),
                SessionState::Idle => ("● Idle", STATE_IDLE_FG),
            };
            result.push(make_row(label, cols, fg));
        }

        // Row 2: current tool
        if result.len() < rows {
            let text = match &self.current_tool {
                Some(tool) => format!("Tool: {tool}"),
                None => "Tool: -".to_string(),
            };
            result.push(make_row(&text, cols, STATE_TOOL_FG));
        }

        // Row 3: cost
        if result.len() < rows {
            let text = match &self.cost {
                Some(c) => format!("Cost: {c}"),
                None => "Cost: -".to_string(),
            };
            result.push(make_row(&text, cols, STATE_INFO_FG));
        }

        // Row 4: tokens
        if result.len() < rows {
            let text = match &self.token_count {
                Some(t) => format!("Tokens: {t}"),
                None => "Tokens: -".to_string(),
            };
            result.push(make_row(&text, cols, STATE_INFO_FG));
        }

        // Row 5: separator
        if result.len() < rows {
            let sep: String = "─".repeat(cols.min(40));
            result.push(make_row(&sep, cols, STATE_HEADER_FG));
        }

        // Row 6+: recent state lines
        for line in &self.state_lines {
            if result.len() >= rows {
                break;
            }
            let wrapped = wrap_text(line, cols);
            for w in wrapped {
                if result.len() >= rows {
                    break;
                }
                result.push(make_row(&w, cols, STATE_DIM_FG));
            }
        }

        // Pad remaining rows
        while result.len() < rows {
            result.push(make_row("", cols, STATE_DIM_FG));
        }

        result
    }
}

/// Build a single row of TerminalCells from a string.
fn make_row(text: &str, cols: usize, fg: Rgb) -> Vec<TerminalCell> {
    let mut row = Vec::with_capacity(cols);
    for ch in text.chars().take(cols) {
        row.push(TerminalCell {
            c: ch,
            fg,
            bg: DEFAULT_BG,
            bold: false,
            italic: false,
            underline: false,
        });
    }
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
    row
}

/// Extract tool name from lines like "⏺ Read src/main.rs" or "Read src/main.rs".
fn extract_tool_name(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches('⏺').trim();
    let tools = ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "Skill", "TodoWrite", "Agent"];
    for tool in &tools {
        if trimmed.starts_with(tool) {
            return Some((*tool).to_string());
        }
    }
    None
}

/// Extract cost from lines like "Cost: $0.05".
fn extract_cost(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if let Some(idx) = lower.find("cost:") {
        let rest = line[idx + 5..].trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

/// Extract token info from lines containing "token".
fn extract_tokens(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if lower.contains("token") {
        Some(line.trim().to_string())
    } else {
        None
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

    // ── StatePanel tests ──

    #[test]
    fn state_indicator_shows_correct_text_for_each_state() {
        let mut panel = StatePanel::new();
        let cols = 30;
        let rows = 10;

        // Idle (default)
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Idle"), "Expected Idle, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, STATE_IDLE_FG);

        // Running
        panel.update_state(SessionState::Running);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Running"), "Expected Running, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, STATE_RUNNING_FG);

        // WaitingForInput
        panel.update_state(SessionState::WaitingForInput);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("WaitingForInput"), "Expected WaitingForInput, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, STATE_WAITING_FG);

        // Error
        panel.update_state(SessionState::Error);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Error"), "Expected Error, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, STATE_ERROR_FG);
    }

    #[test]
    fn push_state_line_extracts_tool_name() {
        let mut panel = StatePanel::new();
        assert!(panel.current_tool.is_none());

        panel.push_state_line("⏺ Read src/main.rs".to_string());
        assert_eq!(panel.current_tool.as_deref(), Some("Read"));

        panel.push_state_line("⏺ Bash ls -la".to_string());
        assert_eq!(panel.current_tool.as_deref(), Some("Bash"));

        panel.push_state_line("Edit src/lib.rs".to_string());
        assert_eq!(panel.current_tool.as_deref(), Some("Edit"));
    }

    #[test]
    fn push_state_line_extracts_cost() {
        let mut panel = StatePanel::new();
        assert!(panel.cost.is_none());

        panel.push_state_line("Cost: $0.05".to_string());
        assert_eq!(panel.cost.as_deref(), Some("$0.05"));

        panel.push_state_line("cost: $1.23 total".to_string());
        assert_eq!(panel.cost.as_deref(), Some("$1.23 total"));
    }

    #[test]
    fn state_lines_accumulate_and_respect_max_lines() {
        let mut panel = StatePanel::new();
        panel.max_lines = 5;

        for i in 0..10 {
            panel.push_state_line(format!("line {i}"));
        }

        assert_eq!(panel.state_lines.len(), 5);
        // Should have the last 5 lines
        assert_eq!(panel.state_lines[0], "line 5");
        assert_eq!(panel.state_lines[4], "line 9");
    }
}
