//! Message panel and display mode management for the terminal UI.
//!
//! The `MessagePanel` accumulates classified message chunks and converts them
//! into `TerminalCell` rows for rendering. `DisplayMode` controls whether the
//! split panel view or raw VT output is shown.

use crate::config::theme::SurftermTheme;
use crate::layer::Layer;
use crate::session::state::SessionState;
use crate::session::terminal::{Rgb, TerminalCell};
use crate::session::SessionId;

/// Resolved panel colors derived from a `SurftermTheme`.
///
/// This is an intermediate struct so that rendering code does not need to
/// reference the full theme. Built once per frame (or when the theme changes).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PanelColors {
    // Base colors
    pub background: Rgb,
    pub foreground: Rgb,

    // State colors
    pub state_running: Rgb,
    pub state_waiting: Rgb,
    pub state_error: Rgb,
    pub state_idle: Rgb,

    // Side panel
    pub side_new_session: Rgb,
    pub side_separator: Rgb,
    pub side_active_bg: Rgb,
    pub side_selected_bg: Rgb,
    pub side_text: Rgb,

    // Card
    pub card_border: Rgb,
    pub card_title_accent: Rgb,
    pub card_bg_title: Rgb,

    // State panel extras (derived from foreground)
    pub state_header: Rgb,
    pub state_tool: Rgb,
    pub state_info: Rgb,
    pub state_dim: Rgb,

    // Message panel
    pub user_input: Rgb,
    pub ai_response: Rgb,

    // Session list
    pub session_selected_bg: Rgb,

    // Main highlight color for session names, active elements
    pub main_color: Rgb,
}

impl PanelColors {
    /// Derive panel colors from a `SurftermTheme`.
    pub fn from_theme(theme: &SurftermTheme) -> Self {
        Self {
            background: theme.colors.background.to_rgb(),
            foreground: theme.colors.foreground.to_rgb(),

            state_running: theme.colors.state.running.to_rgb(),
            state_waiting: theme.colors.state.waiting.to_rgb(),
            state_error: theme.colors.state.error.to_rgb(),
            state_idle: theme.colors.state.idle.to_rgb(),

            side_new_session: theme.colors.sidebar.new_session.to_rgb(),
            side_separator: theme.colors.sidebar.separator.to_rgb(),
            side_active_bg: theme.colors.sidebar.active_bg.to_rgb(),
            side_selected_bg: theme.colors.sidebar.selected_bg.to_rgb(),
            side_text: theme.colors.sidebar.foreground.to_rgb(),

            card_border: theme.colors.card.border.to_rgb(),
            card_title_accent: theme.colors.card.active_title.to_rgb(),
            card_bg_title: theme.colors.card.title.to_rgb(),

            // These are derived / kept as Catppuccin convention from the theme's foreground
            state_header: theme.colors.sidebar.separator.to_rgb(), // #585b70
            state_tool: Rgb::new(0x89, 0xdc, 0xeb), // sky — not in theme, keep as-is
            state_info: theme.colors.foreground.to_rgb(),
            state_dim: Rgb::new(0xa6, 0xad, 0xc8), // subtext0 — not in theme

            user_input: theme.colors.state.waiting.to_rgb(), // green
            ai_response: theme.colors.foreground.to_rgb(),

            session_selected_bg: theme.colors.sidebar.active_bg.to_rgb(),
            main_color: theme.colors.main_color.to_rgb(),
        }
    }
}

impl Default for PanelColors {
    fn default() -> Self {
        Self::from_theme(&SurftermTheme::default())
    }
}

/// Background card base colors, progressively lighter per layer.
const CARD_BG_LAYERS: [Rgb; 4] = [
    Rgb::new(0x25, 0x25, 0x38),
    Rgb::new(0x2c, 0x2c, 0x42),
    Rgb::new(0x33, 0x33, 0x4c),
    Rgb::new(0x3a, 0x3a, 0x56),
];

/// A single entry in the side panel session list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct SidePanelEntry {
    pub id: SessionId,
    pub name: String,
    pub state: SessionState,
    pub is_active: bool,
}

/// Side panel showing sessions with a "New Session" button at the top.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SidePanel {
    pub sessions: Vec<SidePanelEntry>,
    pub selected_index: usize,
    /// When true, the "+ New Session" button row is selected (index 0 in the navigation).
    pub new_session_highlighted: bool,
}

impl Default for SidePanel {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SidePanel {
    /// Create a new empty side panel.
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            selected_index: 0,
            new_session_highlighted: true,
        }
    }

    /// Update the session list.
    pub fn update_sessions(&mut self, entries: Vec<SidePanelEntry>) {
        self.sessions = entries;
        // Clamp selected_index: 0 = new session button, 1..=len = session entries
        let max_index = self.sessions.len();
        if self.selected_index > max_index {
            self.selected_index = max_index;
        }
        self.new_session_highlighted = self.selected_index == 0;
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        let max_index = self.sessions.len(); // 0 = new button, 1..=len = sessions
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
        self.new_session_highlighted = self.selected_index == 0;
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
        self.new_session_highlighted = self.selected_index == 0;
    }

    /// Get the currently selected session entry (None if new-session button is selected or list empty).
    pub fn selected_entry(&self) -> Option<&SidePanelEntry> {
        if self.selected_index == 0 {
            None
        } else {
            self.sessions.get(self.selected_index - 1)
        }
    }

    /// True when the "+ New Session" button is selected (index 0).
    pub fn is_new_session_selected(&self) -> bool {
        self.selected_index == 0
    }

    /// Render the side panel as terminal cells.
    ///
    /// Layout:
    /// - Row 0: "[+ New Session]" in green, highlighted if selected
    /// - Row 1: "─────────" separator in dim
    /// - Row 2+: Each session entry with state dot and project name
    pub fn to_terminal_cells(&self, cols: u16, rows: u16, _scale_factor: f32) -> Vec<Vec<TerminalCell>> {
        self.to_terminal_cells_themed(cols, rows, _scale_factor, &PanelColors::default())
    }

    /// Render the side panel as terminal cells using the given theme colors.
    pub fn to_terminal_cells_themed(
        &self,
        cols: u16,
        rows: u16,
        _scale_factor: f32,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }

        let cols = cols as usize;
        let rows = rows as usize;
        let mut result: Vec<Vec<TerminalCell>> = Vec::with_capacity(rows);

        // Row 0: "+ New Session" button
        {
            let text = "[+ New Session]";
            let bg = if self.selected_index == 0 {
                colors.side_selected_bg
            } else {
                colors.background
            };
            let mut row = Vec::with_capacity(cols);
            for ch in text.chars().take(cols) {
                row.push(TerminalCell {
                    c: ch,
                    fg: colors.side_new_session,
                    bg,
                    bold: self.selected_index == 0,
                    italic: false,
                    underline: false,
                });
            }
            while row.len() < cols {
                row.push(TerminalCell {
                    c: ' ',
                    fg: colors.side_new_session,
                    bg,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            result.push(row);
        }

        // Row 1: separator (▁ lower one eighth block fills gap between cells)
        if result.len() < rows {
            let mut sep_row = Vec::with_capacity(cols);
            for _ in 0..cols {
                sep_row.push(TerminalCell {
                    c: '\u{2581}', // ▁
                    fg: colors.side_separator,
                    bg: colors.background,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            result.push(sep_row);
        }

        // Row 2+: session entries
        for (i, entry) in self.sessions.iter().enumerate() {
            if result.len() >= rows {
                break;
            }

            let nav_index = i + 1; // 0 = new session button, so sessions start at 1
            let is_selected = self.selected_index == nav_index;

            let dot_fg = match entry.state {
                SessionState::Running => colors.state_running,
                SessionState::WaitingForInput => colors.state_waiting,
                SessionState::Error => colors.state_error,
                SessionState::Idle => colors.state_idle,
            };

            let bg = if is_selected {
                colors.side_selected_bg
            } else if entry.is_active {
                colors.side_active_bg
            } else {
                colors.background
            };

            // Build: "● name" (truncated to cols)
            let dot = "\u{25cf}";
            let name_max = cols.saturating_sub(2); // "● " = 2 chars
            let truncated_name: String = entry.name.chars().take(name_max).collect();
            let text = format!("{} {}", dot, truncated_name);

            let mut row = Vec::with_capacity(cols);
            for (ci, ch) in text.chars().enumerate() {
                let fg = if ci == 0 { dot_fg } else { colors.main_color };
                row.push(TerminalCell {
                    c: ch,
                    fg,
                    bg,
                    bold: is_selected,
                    italic: false,
                    underline: false,
                });
            }
            while row.len() < cols {
                row.push(TerminalCell {
                    c: ' ',
                    fg: colors.side_text,
                    bg,
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            result.push(row);
        }

        // Pad remaining rows
        while result.len() < rows {
            result.push(make_row_colored("", cols, colors.side_separator, colors.background));
        }

        result
    }
}

/// Information about a single card in the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct CardInfo {
    pub session_id: SessionId,
    pub project_name: String,
    pub state: SessionState,
    pub is_active: bool,
}

/// Stacked card layout for the main content area.
///
/// The active (frontmost) card is rendered at (0,0) with full terminal content.
/// Background cards appear as title bar rows at the bottom of the main area,
/// progressively offset to the right, giving a stacked card visual effect.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CardStack {
    pub cards: Vec<CardInfo>,
}

impl Default for CardStack {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl CardStack {
    /// Create a new empty card stack.
    pub fn new() -> Self {
        Self { cards: Vec::new() }
    }

    /// Update the card stack with a new set of cards.
    /// The first card is the active (frontmost) card.
    pub fn update(&mut self, cards: Vec<CardInfo>) {
        self.cards = cards;
    }

    /// Return the number of cards in the stack.
    pub fn card_count(&self) -> usize {
        self.cards.len()
    }

    /// Get the active (frontmost) card, if any.
    pub fn active_card(&self) -> Option<&CardInfo> {
        self.cards.first()
    }

    /// Get background cards (all cards except the active one).
    pub fn background_cards(&self) -> &[CardInfo] {
        if self.cards.len() > 1 {
            &self.cards[1..]
        } else {
            &[]
        }
    }

    /// Build the active card tab as 2 rows. Right edge is open (divider handles it).
    ///
    /// Row 0: `╭──────────────────────` (top border, no right corner)
    /// Row 1: `│ name       [state]  ` (content, no right border)
    fn build_active_card_tab(
        card: &CardInfo,
        cols: usize,
        title_fg: Rgb,
        border_fg: Rgb,
        bg: Rgb,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
        if cols < 2 {
            return vec![Vec::new(), Vec::new()];
        }

        let (state_label, state_fg) = match card.state {
            SessionState::Running => ("Running", colors.state_running),
            SessionState::WaitingForInput => ("Waiting", colors.state_waiting),
            SessionState::Error => ("Error", colors.state_error),
            SessionState::Idle => ("Idle", colors.state_idle),
        };

        // Row 0: "╭────...────" (no right corner — divider closes it)
        let mut border_row = Vec::with_capacity(cols);
        border_row.push(TerminalCell {
            c: '\u{256d}', // ╭
            fg: border_fg, bg, bold: false, italic: false, underline: false,
        });
        for _ in 1..cols {
            border_row.push(TerminalCell {
                c: '\u{2500}', // ─
                fg: border_fg, bg, bold: false, italic: false, underline: false,
            });
        }

        // Row 1: "│ name   [state] " (no right border — divider closes it)
        let mut content_row = Vec::with_capacity(cols);
        content_row.push(TerminalCell {
            c: '\u{2502}', // │
            fg: border_fg, bg, bold: false, italic: false, underline: false,
        });

        let inner_cols = cols.saturating_sub(1); // only left border
        Self::fill_card_content(&mut content_row, card, inner_cols, title_fg, border_fg, state_fg, state_label, bg);

        vec![border_row, content_row]
    }

    /// Build a background card tab as 2 rows. No left/right borders (divider closes right).
    ///
    /// Row 0: `──────────────────────` (horizontal line only)
    /// Row 1: ` name       [state]  ` (content, no borders)
    fn build_bg_card_tab(
        card: &CardInfo,
        cols: usize,
        title_fg: Rgb,
        border_fg: Rgb,
        bg: Rgb,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
        if cols == 0 {
            return vec![Vec::new(), Vec::new()];
        }

        let (state_label, state_fg) = match card.state {
            SessionState::Running => ("Running", colors.state_running),
            SessionState::WaitingForInput => ("Waiting", colors.state_waiting),
            SessionState::Error => ("Error", colors.state_error),
            SessionState::Idle => ("Idle", colors.state_idle),
        };

        // Row 0: just "────...────" (divider will show ┘ at the end)
        let border_row: Vec<TerminalCell> = (0..cols)
            .map(|_| TerminalCell {
                c: '\u{2500}', // ─
                fg: border_fg, bg, bold: false, italic: false, underline: false,
            })
            .collect();

        // Row 1: " name   [state] " (no borders)
        let mut content_row = Vec::with_capacity(cols);
        Self::fill_card_content(&mut content_row, card, cols, title_fg, border_fg, state_fg, state_label, bg);

        vec![border_row, content_row]
    }

    /// Fill card content cells: " name <padding> [state] "
    fn fill_card_content(
        row: &mut Vec<TerminalCell>,
        card: &CardInfo,
        cols: usize,
        title_fg: Rgb,
        border_fg: Rgb,
        state_fg: Rgb,
        state_label: &str,
        bg: Rgb,
    ) {
        let state_part = format!("[{}]", state_label);
        let name_chars: Vec<char> = card.project_name.chars().collect();
        let state_chars: Vec<char> = state_part.chars().collect();
        let name_display_len = name_chars.len().min(cols.saturating_sub(state_chars.len() + 3));
        let state_start = cols.saturating_sub(state_chars.len() + 1);

        for i in 0..cols {
            if i == 0 {
                row.push(TerminalCell {
                    c: ' ', fg: title_fg, bg, bold: card.is_active, italic: false, underline: false,
                });
            } else if i >= 1 && i < 1 + name_display_len {
                row.push(TerminalCell {
                    c: name_chars[i - 1], fg: title_fg, bg, bold: card.is_active, italic: false, underline: false,
                });
            } else if i >= state_start && i < state_start + state_chars.len() {
                let si = i - state_start;
                let ch = state_chars[si];
                let fg = if ch != '[' && ch != ']' { state_fg } else { border_fg };
                row.push(TerminalCell {
                    c: ch, fg, bg, bold: false, italic: false, underline: false,
                });
            } else {
                row.push(TerminalCell {
                    c: ' ', fg: border_fg, bg, bold: false, italic: false, underline: false,
                });
            }
        }
    }

    /// Build terminal cells for the active card's tab using theme colors.
    /// Returns 2 rows: top border + content row. Right edge open (divider handles it).
    pub fn active_card_tab_themed(&self, cols: usize, colors: &PanelColors) -> Option<Vec<Vec<TerminalCell>>> {
        self.active_card().map(|card| {
            Self::build_active_card_tab(
                card,
                cols,
                colors.card_title_accent,
                colors.card_border,
                colors.background,
                colors,
            )
        })
    }

    /// Build terminal cells for background card tabs using theme colors.
    ///
    /// Each background card gets 2 rows (border line + content).
    /// No left/right borders — the divider provides the right edge with ┘.
    ///
    /// Returns: Vec of (left_offset_in_cells, tab_rows).
    pub fn background_card_tabs_themed(
        &self,
        cols: usize,
        scale_factor: f32,
        cell_width: f32,
        colors: &PanelColors,
    ) -> Vec<(usize, Vec<Vec<TerminalCell>>)> {
        let card_offset_px = 20.0 * scale_factor;
        let offset_cells = (card_offset_px / cell_width).ceil() as usize;

        self.background_cards()
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let left_offset = offset_cells * (i + 1);
                let available_cols = cols.saturating_sub(left_offset);
                let bg_color = CARD_BG_LAYERS[i % CARD_BG_LAYERS.len()];
                let rows = Self::build_bg_card_tab(
                    card,
                    available_cols,
                    colors.card_bg_title,
                    colors.card_border,
                    bg_color,
                    colors,
                );
                (left_offset, rows)
            })
            .collect()
    }
}

/// A single entry in the session list panel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct SessionListEntry {
    pub id: SessionId,
    pub project_name: String,
    pub state: SessionState,
    pub layer: Layer,
    /// 1-based display index.
    pub index: usize,
}

/// Session list panel displaying all sessions grouped by layer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionList {
    pub entries: Vec<SessionListEntry>,
    pub selected_index: usize,
    pub visible: bool,
}

impl Default for SessionList {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SessionList {
    /// Create a new empty session list (hidden by default).
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            selected_index: 0,
            visible: false,
        }
    }

    /// Refresh the list with new entries, clamping the selection index.
    pub fn update(&mut self, entries: Vec<SessionListEntry>) {
        self.entries = entries;
        if self.entries.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.entries.len() {
            self.selected_index = self.entries.len() - 1;
        }
    }

    /// Move selection down (j key). Clamps at the bottom.
    pub fn select_next(&mut self) {
        if !self.entries.is_empty() && self.selected_index < self.entries.len() - 1 {
            self.selected_index += 1;
        }
    }

    /// Move selection up (k key). Clamps at the top.
    pub fn select_prev(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    /// Get the SessionId of the currently selected entry.
    pub fn selected_id(&self) -> Option<SessionId> {
        self.entries.get(self.selected_index).map(|e| e.id)
    }

    /// Select by 1-based display index (keys 1-9). No-op if no matching entry.
    pub fn select_by_index(&mut self, index: usize) {
        if let Some(pos) = self.entries.iter().position(|e| e.index == index) {
            self.selected_index = pos;
        }
    }

    /// Toggle visibility of the session list panel.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// Render the session list as terminal cells.
    pub fn to_terminal_cells(&self, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
        self.to_terminal_cells_themed(cols, rows, &PanelColors::default())
    }

    /// Render the session list as terminal cells using theme colors.
    ///
    /// Layout:
    /// - Header: "── Sessions ──"
    /// - Group headers: "Foreground:" / "Background:" / "Pinned:"
    /// - Each entry: `[index] project_name [state]` (pinned entries have `*` prefix)
    /// - Selected entry has highlighted background
    pub fn to_terminal_cells_themed(
        &self,
        cols: u16,
        rows: u16,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }

        let cols = cols as usize;
        let rows = rows as usize;
        let mut result: Vec<Vec<TerminalCell>> = Vec::with_capacity(rows);

        // Header
        result.push(make_row_colored("── Sessions ──", cols, colors.state_header, colors.background));

        // Collect entries by layer group
        let pinned: Vec<&SessionListEntry> =
            self.entries.iter().filter(|e| e.layer == Layer::Pinned).collect();
        let foreground: Vec<&SessionListEntry> =
            self.entries.iter().filter(|e| e.layer == Layer::Foreground).collect();
        let background: Vec<&SessionListEntry> =
            self.entries.iter().filter(|e| e.layer == Layer::Background).collect();

        // Track flat index for selection highlighting
        let mut flat_index: usize = 0;

        // Render each group
        let groups: &[(&str, &[&SessionListEntry])] = &[
            ("Pinned:", &pinned),
            ("Foreground:", &foreground),
            ("Background:", &background),
        ];

        for &(group_name, group_entries) in groups {
            if group_entries.is_empty() {
                continue;
            }
            if result.len() >= rows {
                break;
            }
            // Group header
            result.push(make_row_colored(group_name, cols, colors.state_header, colors.background));

            for entry in group_entries {
                if result.len() >= rows {
                    break;
                }

                let is_selected = flat_index == self.selected_index;
                flat_index += 1;

                let (state_label, state_fg) = match entry.state {
                    SessionState::Running => ("Running", colors.state_running),
                    SessionState::WaitingForInput => ("WaitingForInput", colors.state_waiting),
                    SessionState::Error => ("Error", colors.state_error),
                    SessionState::Idle => ("Idle", colors.state_idle),
                };

                let pin_marker = if entry.layer == Layer::Pinned { "* " } else { "  " };
                let text = format!(
                    "{pin_marker}[{}] {} [{}]",
                    entry.index, entry.project_name, state_label
                );

                let bg = if is_selected {
                    colors.session_selected_bg
                } else {
                    colors.background
                };

                let mut row = Vec::with_capacity(cols);
                for ch in text.chars().take(cols) {
                    row.push(TerminalCell {
                        c: ch,
                        fg: state_fg,
                        bg,
                        bold: is_selected,
                        italic: false,
                        underline: false,
                    });
                }
                while row.len() < cols {
                    row.push(TerminalCell {
                        c: ' ',
                        fg: state_fg,
                        bg,
                        bold: false,
                        italic: false,
                        underline: false,
                    });
                }
                result.push(row);
            }
        }

        // Pad remaining rows
        while result.len() < rows {
            result.push(make_row_colored("", cols, colors.state_dim, colors.background));
        }

        result
    }
}

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

impl Default for MessagePanel {
    fn default() -> Self {
        Self::new()
    }
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

    /// Convert the message history into a grid of `TerminalCell` rows (default colors).
    pub fn to_terminal_cells(&self, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
        self.to_terminal_cells_themed(cols, rows, &PanelColors::default())
    }

    /// Convert the message history into a grid of `TerminalCell` rows using theme colors.
    ///
    /// Long lines are wrapped at `cols` characters. The result is clipped to
    /// `rows` visible lines starting from `scroll_offset`.
    pub fn to_terminal_cells_themed(
        &self,
        cols: u16,
        rows: u16,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
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
            let fg = if *is_user { colors.user_input } else { colors.ai_response };
            let mut row = Vec::with_capacity(cols);
            for ch in line.chars().take(cols) {
                row.push(TerminalCell {
                    c: ch,
                    fg,
                    bg: colors.background,
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
                    bg: colors.background,
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
                    fg: colors.ai_response,
                    bg: colors.background,
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

impl Default for StatePanel {
    fn default() -> Self {
        Self::new()
    }
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

    /// Render state panel content as terminal cells (default colors).
    pub fn to_terminal_cells(&self, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
        self.to_terminal_cells_themed(cols, rows, &PanelColors::default())
    }

    /// Render state panel content as terminal cells using theme colors.
    pub fn to_terminal_cells_themed(
        &self,
        cols: u16,
        rows: u16,
        colors: &PanelColors,
    ) -> Vec<Vec<TerminalCell>> {
        if cols == 0 || rows == 0 {
            return Vec::new();
        }

        let cols = cols as usize;
        let rows = rows as usize;
        let mut result: Vec<Vec<TerminalCell>> = Vec::with_capacity(rows);

        // Row 0: header
        result.push(make_row_colored("── State ──", cols, colors.state_header, colors.background));

        // Row 1: state indicator
        if result.len() < rows {
            let (label, fg) = match self.session_state {
                SessionState::Running => ("● Running", colors.state_running),
                SessionState::WaitingForInput => ("● WaitingForInput", colors.state_waiting),
                SessionState::Error => ("● Error", colors.state_error),
                SessionState::Idle => ("● Idle", colors.state_idle),
            };
            result.push(make_row_colored(label, cols, fg, colors.background));
        }

        // Row 2: current tool
        if result.len() < rows {
            let text = match &self.current_tool {
                Some(tool) => format!("Tool: {tool}"),
                None => "Tool: -".to_string(),
            };
            result.push(make_row_colored(&text, cols, colors.state_tool, colors.background));
        }

        // Row 3: cost
        if result.len() < rows {
            let text = match &self.cost {
                Some(c) => format!("Cost: {c}"),
                None => "Cost: -".to_string(),
            };
            result.push(make_row_colored(&text, cols, colors.state_info, colors.background));
        }

        // Row 4: tokens
        if result.len() < rows {
            let text = match &self.token_count {
                Some(t) => format!("Tokens: {t}"),
                None => "Tokens: -".to_string(),
            };
            result.push(make_row_colored(&text, cols, colors.state_info, colors.background));
        }

        // Row 5: separator
        if result.len() < rows {
            let sep: String = "─".repeat(cols.min(40));
            result.push(make_row_colored(&sep, cols, colors.state_header, colors.background));
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
                result.push(make_row_colored(&w, cols, colors.state_dim, colors.background));
            }
        }

        // Pad remaining rows
        while result.len() < rows {
            result.push(make_row_colored("", cols, colors.state_dim, colors.background));
        }

        result
    }
}

/// Build a single row of TerminalCells from a string with explicit fg and bg.
fn make_row_colored(text: &str, cols: usize, fg: Rgb, bg: Rgb) -> Vec<TerminalCell> {
    let mut row = Vec::with_capacity(cols);
    for ch in text.chars().take(cols) {
        row.push(TerminalCell {
            c: ch,
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false,
        });
    }
    while row.len() < cols {
        row.push(TerminalCell {
            c: ' ',
            fg,
            bg,
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

    /// Get default panel colors for testing.
    fn default_colors() -> PanelColors {
        PanelColors::default()
    }

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
        let colors = default_colors();
        let mut panel = MessagePanel::new();
        panel.push_message("AI says hello".to_string(), false);
        panel.push_message("User says hi".to_string(), true);

        let cells = panel.to_terminal_cells(20, 5);

        // AI response row: foreground color
        assert_eq!(cells[0][0].fg, colors.ai_response);
        // User input row: green
        assert_eq!(cells[1][0].fg, colors.user_input);
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
        let colors = default_colors();
        let mut panel = StatePanel::new();
        let cols = 30;
        let rows = 10;

        // Idle (default)
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Idle"), "Expected Idle, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, colors.state_idle);

        // Running
        panel.update_state(SessionState::Running);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Running"), "Expected Running, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, colors.state_running);

        // WaitingForInput
        panel.update_state(SessionState::WaitingForInput);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("WaitingForInput"), "Expected WaitingForInput, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, colors.state_waiting);

        // Error
        panel.update_state(SessionState::Error);
        let cells = panel.to_terminal_cells(cols, rows);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1_text.contains("Error"), "Expected Error, got: '{}'", row1_text.trim());
        assert_eq!(cells[1][0].fg, colors.state_error);
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

    // ── SessionList tests ──

    fn make_entry(index: usize, name: &str, state: SessionState, layer: Layer) -> SessionListEntry {
        SessionListEntry {
            id: SessionId::new(),
            project_name: name.to_string(),
            state,
            layer,
            index,
        }
    }

    fn sample_entries() -> Vec<SessionListEntry> {
        vec![
            make_entry(1, "api-server", SessionState::Running, Layer::Pinned),
            make_entry(2, "web-frontend", SessionState::WaitingForInput, Layer::Foreground),
            make_entry(3, "cli-tool", SessionState::Idle, Layer::Background),
        ]
    }

    #[test]
    fn session_list_update_entries_and_verify() {
        let mut list = SessionList::new();
        assert!(list.entries.is_empty());

        let entries = sample_entries();
        let id0 = entries[0].id;
        list.update(entries);

        assert_eq!(list.entries.len(), 3);
        assert_eq!(list.entries[0].id, id0);
        assert_eq!(list.entries[0].project_name, "api-server");
    }

    #[test]
    fn session_list_select_next_clamps_at_bottom() {
        let mut list = SessionList::new();
        list.update(sample_entries());

        assert_eq!(list.selected_index, 0);
        list.select_next();
        assert_eq!(list.selected_index, 1);
        list.select_next();
        assert_eq!(list.selected_index, 2);
        // Should clamp, not wrap
        list.select_next();
        assert_eq!(list.selected_index, 2);
    }

    #[test]
    fn session_list_select_prev_clamps_at_top() {
        let mut list = SessionList::new();
        list.update(sample_entries());

        list.selected_index = 1;
        list.select_prev();
        assert_eq!(list.selected_index, 0);
        // Should clamp at 0
        list.select_prev();
        assert_eq!(list.selected_index, 0);
    }

    #[test]
    fn session_list_selected_id_returns_correct_session() {
        let mut list = SessionList::new();
        let entries = sample_entries();
        let id1 = entries[1].id;
        list.update(entries);

        list.selected_index = 1;
        assert_eq!(list.selected_id(), Some(id1));
    }

    #[test]
    fn session_list_selected_id_empty_list() {
        let list = SessionList::new();
        assert_eq!(list.selected_id(), None);
    }

    #[test]
    fn session_list_select_by_index_works() {
        let mut list = SessionList::new();
        let entries = sample_entries();
        let id2 = entries[1].id; // index=2
        let id3 = entries[2].id; // index=3
        list.update(entries);

        list.select_by_index(2);
        assert_eq!(list.selected_id(), Some(id2));

        list.select_by_index(3);
        assert_eq!(list.selected_id(), Some(id3));

        // Non-existent index: no change
        list.select_by_index(9);
        assert_eq!(list.selected_id(), Some(id3));
    }

    #[test]
    fn session_list_toggle_visible() {
        let mut list = SessionList::new();
        assert!(!list.visible);

        list.toggle_visible();
        assert!(list.visible);

        list.toggle_visible();
        assert!(!list.visible);
    }

    #[test]
    fn session_list_to_terminal_cells_renders_correct_rows() {
        let colors = default_colors();
        let mut list = SessionList::new();
        list.update(sample_entries());

        let cells = list.to_terminal_cells(50, 20);

        // Row 0: header
        let header: String = cells[0].iter().map(|c| c.c).collect::<String>();
        assert!(header.contains("Sessions"), "Header: '{}'", header.trim());

        // Row 1: "Pinned:" group header (pinned entries come first)
        let row1: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(row1.contains("Pinned:"), "Expected Pinned group header, got: '{}'", row1.trim());

        // Row 2: pinned entry "[1] api-server [Running]"
        let row2: String = cells[2].iter().map(|c| c.c).collect::<String>();
        assert!(row2.contains("api-server"), "Expected api-server, got: '{}'", row2.trim());
        assert!(row2.contains("Running"), "Expected Running state, got: '{}'", row2.trim());
        assert!(row2.contains("*"), "Expected pin marker, got: '{}'", row2.trim());
        // Selected (index 0) should have highlighted bg
        assert_eq!(cells[2][0].bg, colors.session_selected_bg);

        // Row 3: "Foreground:" group header
        let row3: String = cells[3].iter().map(|c| c.c).collect::<String>();
        assert!(row3.contains("Foreground:"), "Expected Foreground header, got: '{}'", row3.trim());

        // Row 4: foreground entry
        let row4: String = cells[4].iter().map(|c| c.c).collect::<String>();
        assert!(row4.contains("web-frontend"), "Expected web-frontend, got: '{}'", row4.trim());
        // Not selected -> default bg
        assert_eq!(cells[4][0].bg, colors.background);

        // Row 5: "Background:" group header
        let row5: String = cells[5].iter().map(|c| c.c).collect::<String>();
        assert!(row5.contains("Background:"), "Expected Background header, got: '{}'", row5.trim());

        // Row 6: background entry
        let row6: String = cells[6].iter().map(|c| c.c).collect::<String>();
        assert!(row6.contains("cli-tool"), "Expected cli-tool, got: '{}'", row6.trim());

        // Total rows should equal requested
        assert_eq!(cells.len(), 20);
    }

    #[test]
    fn session_list_to_terminal_cells_state_colors() {
        let colors = default_colors();
        let mut list = SessionList::new();
        list.update(sample_entries());
        // Select second entry so first isn't selected
        list.selected_index = 1;

        let cells = list.to_terminal_cells(50, 20);

        // Row 2: pinned entry - Running = yellow
        assert_eq!(cells[2][0].fg, colors.state_running);
        // Row 4: foreground entry - WaitingForInput = green (selected)
        assert_eq!(cells[4][0].fg, colors.state_waiting);
        // Row 6: background entry - Idle = gray
        assert_eq!(cells[6][0].fg, colors.state_idle);
    }

    #[test]
    fn session_list_to_terminal_cells_zero_dimensions() {
        let list = SessionList::new();
        let cells = list.to_terminal_cells(0, 0);
        assert!(cells.is_empty());
    }

    #[test]
    fn session_list_empty_renders_only_header_and_padding() {
        let list = SessionList::new();
        let cells = list.to_terminal_cells(30, 5);
        assert_eq!(cells.len(), 5);
        let header: String = cells[0].iter().map(|c| c.c).collect::<String>();
        assert!(header.contains("Sessions"));
        // Remaining rows are padding (spaces)
        for row in &cells[1..] {
            for cell in row {
                assert_eq!(cell.c, ' ');
            }
        }
    }

    #[test]
    fn session_list_update_clamps_selected_index() {
        let mut list = SessionList::new();
        list.update(sample_entries());
        list.selected_index = 2;

        // Update with fewer entries
        list.update(vec![make_entry(1, "only", SessionState::Idle, Layer::Foreground)]);
        assert_eq!(list.selected_index, 0);
    }

    #[test]
    fn session_list_select_next_prev_on_empty() {
        let mut list = SessionList::new();
        // Should not panic on empty list
        list.select_next();
        assert_eq!(list.selected_index, 0);
        list.select_prev();
        assert_eq!(list.selected_index, 0);
    }

    // ── SidePanel tests ──

    fn make_side_entry(name: &str, state: SessionState, is_active: bool) -> SidePanelEntry {
        SidePanelEntry {
            id: SessionId::new(),
            name: name.to_string(),
            state,
            is_active,
        }
    }

    #[test]
    fn side_panel_new_session_at_top() {
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("project-a", SessionState::Running, true),
        ]);

        let cells = panel.to_terminal_cells(20, 10, 1.0);
        let row0_text: String = cells[0].iter().map(|c| c.c).collect::<String>();
        assert!(
            row0_text.contains("+ New Session"),
            "Expected new session button at row 0, got: '{}'",
            row0_text.trim()
        );
    }

    #[test]
    fn side_panel_separator_at_row_1() {
        let panel = SidePanel::new();
        let cells = panel.to_terminal_cells(10, 5, 1.0);
        let row1_text: String = cells[1].iter().map(|c| c.c).collect::<String>();
        assert!(
            row1_text.contains('\u{2581}'),
            "Expected separator at row 1, got: '{}'",
            row1_text
        );
    }

    #[test]
    fn side_panel_session_entries_at_row_2_plus() {
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("alpha", SessionState::Running, true),
            make_side_entry("beta", SessionState::Idle, false),
        ]);

        let cells = panel.to_terminal_cells(20, 10, 1.0);
        // Row 2 should have the first session
        let row2_text: String = cells[2].iter().map(|c| c.c).collect::<String>();
        assert!(row2_text.contains("alpha"), "Expected 'alpha', got: '{}'", row2_text.trim());
        // Row 3 should have the second session
        let row3_text: String = cells[3].iter().map(|c| c.c).collect::<String>();
        assert!(row3_text.contains("beta"), "Expected 'beta', got: '{}'", row3_text.trim());
    }

    #[test]
    fn side_panel_navigation() {
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("a", SessionState::Idle, true),
            make_side_entry("b", SessionState::Idle, false),
        ]);

        // Initially at 0 (new session button)
        assert!(panel.is_new_session_selected());
        assert!(panel.selected_entry().is_none());

        // Move down to first session
        panel.select_next();
        assert!(!panel.is_new_session_selected());
        assert_eq!(panel.selected_entry().unwrap().name, "a");

        // Move down to second session
        panel.select_next();
        assert_eq!(panel.selected_entry().unwrap().name, "b");

        // Clamp at bottom
        panel.select_next();
        assert_eq!(panel.selected_entry().unwrap().name, "b");

        // Move back up
        panel.select_prev();
        assert_eq!(panel.selected_entry().unwrap().name, "a");

        panel.select_prev();
        assert!(panel.is_new_session_selected());

        // Clamp at top
        panel.select_prev();
        assert!(panel.is_new_session_selected());
    }

    #[test]
    fn side_panel_state_dot_colors() {
        let colors = default_colors();
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("running", SessionState::Running, false),
            make_side_entry("waiting", SessionState::WaitingForInput, false),
            make_side_entry("error", SessionState::Error, false),
            make_side_entry("idle", SessionState::Idle, false),
        ]);

        let cells = panel.to_terminal_cells(20, 10, 1.0);
        // Row 2: Running dot = yellow
        assert_eq!(cells[2][0].fg, colors.state_running);
        // Row 3: WaitingForInput dot = green
        assert_eq!(cells[3][0].fg, colors.state_waiting);
        // Row 4: Error dot = red
        assert_eq!(cells[4][0].fg, colors.state_error);
        // Row 5: Idle dot = gray
        assert_eq!(cells[5][0].fg, colors.state_idle);
    }

    #[test]
    fn side_panel_active_session_highlighted_bg() {
        let colors = default_colors();
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("active", SessionState::Idle, true),
            make_side_entry("inactive", SessionState::Idle, false),
        ]);
        // Move selection away from both sessions (stay at new-session button)
        panel.selected_index = 0;

        let cells = panel.to_terminal_cells(20, 10, 1.0);
        // Row 2: active session should have side_active_bg
        assert_eq!(cells[2][0].bg, colors.side_active_bg);
        // Row 3: inactive session should have background
        assert_eq!(cells[3][0].bg, colors.background);
    }

    #[test]
    fn side_panel_selected_overrides_active_bg() {
        let colors = default_colors();
        let mut panel = SidePanel::new();
        panel.update_sessions(vec![
            make_side_entry("active", SessionState::Idle, true),
        ]);
        // Select the session (index 1)
        panel.selected_index = 1;

        let cells = panel.to_terminal_cells(20, 10, 1.0);
        // Selected should use side_selected_bg, not side_active_bg
        assert_eq!(cells[2][0].bg, colors.side_selected_bg);
    }

    #[test]
    fn side_panel_zero_dimensions() {
        let panel = SidePanel::new();
        let cells = panel.to_terminal_cells(0, 0, 1.0);
        assert!(cells.is_empty());
    }

    #[test]
    fn side_panel_empty_sessions() {
        let panel = SidePanel::new();
        let cells = panel.to_terminal_cells(20, 5, 1.0);
        assert_eq!(cells.len(), 5);
        // Row 0: new session button
        let row0_text: String = cells[0].iter().map(|c| c.c).collect::<String>();
        assert!(row0_text.contains("+ New Session"));
    }
}
