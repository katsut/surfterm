//! Syntax highlighting for file preview using syntect.

use std::path::Path;

use anyhow::{Context, Result};
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use tracing::instrument;

use crate::session::terminal::{Rgb, TerminalCell};

/// A single highlighted span of text with a foreground color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedSpan {
    pub text: String,
    pub fg: Rgb,
}

/// A highlighted line with line number and colored spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedLine {
    pub line_number: usize,
    pub spans: Vec<HighlightedSpan>,
}

/// Syntax highlighter backed by syntect.
#[allow(dead_code)]
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

#[allow(dead_code)]
impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SyntaxHighlighter {
    /// Create a new highlighter with default syntaxes and a dark theme.
    #[instrument]
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| {
                theme_set
                    .themes
                    .values()
                    .next()
                    .cloned()
                    .expect("syntect ships at least one theme")
            });
        Self { syntax_set, theme }
    }

    /// Highlight a file on disk. The syntax is chosen by the file extension.
    #[instrument(skip(self))]
    pub fn highlight_file(&self, path: &Path) -> Result<Vec<HighlightedLine>> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        Ok(self.highlight_content(&content, ext))
    }

    /// Highlight a string of content with the given file extension for syntax detection.
    #[instrument(skip(self, content))]
    pub fn highlight_content(&self, content: &str, extension: &str) -> Vec<HighlightedLine> {
        use syntect::easy::HighlightLines;
        use syntect::util::LinesWithEndings;

        let syntax = self
            .syntax_set
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();

        for (line_num, line_text) in LinesWithEndings::from(content).enumerate() {
            let regions = highlighter
                .highlight_line(line_text, &self.syntax_set)
                .unwrap_or_default();

            let spans: Vec<HighlightedSpan> = regions
                .into_iter()
                .map(|(style, text)| {
                    let fg = Rgb::new(style.foreground.r, style.foreground.g, style.foreground.b);
                    // Strip trailing newline for display
                    let cleaned = text.trim_end_matches('\n').trim_end_matches('\r');
                    HighlightedSpan {
                        text: cleaned.to_string(),
                        fg,
                    }
                })
                .filter(|s| !s.text.is_empty())
                .collect();

            lines.push(HighlightedLine {
                line_number: line_num + 1,
                spans,
            });
        }

        lines
    }
}

/// Line number color: dim gray (#6c7086).
const LINE_NUMBER_COLOR: Rgb = Rgb::new(0x6c, 0x70, 0x86);
/// Default foreground for content.
const DEFAULT_FG: Rgb = Rgb::new(205, 214, 244);
/// Default background.
const DEFAULT_BG: Rgb = Rgb::new(0, 0, 0);

/// Convert highlighted lines to terminal cells for rendering.
///
/// Applies a viewport starting at `scroll_offset`, returning at most `rows` rows,
/// each with `cols` cells. Line numbers are rendered in the left gutter (4 digits + space).
#[allow(dead_code)]
pub fn to_terminal_cells(
    lines: &[HighlightedLine],
    cols: u16,
    rows: u16,
    scroll_offset: usize,
) -> Vec<Vec<TerminalCell>> {
    let cols = cols as usize;
    let rows = rows as usize;
    let mut output = Vec::with_capacity(rows);

    let visible = lines.iter().skip(scroll_offset).take(rows);

    for line in visible {
        let mut row = Vec::with_capacity(cols);

        // Render line number right-aligned in 4 chars + 1 space.
        let num_str = format!("{:>4} ", line.line_number);
        for ch in num_str.chars() {
            if row.len() >= cols {
                break;
            }
            row.push(TerminalCell {
                c: ch,
                fg: LINE_NUMBER_COLOR,
                bg: DEFAULT_BG,
                bold: false,
                italic: false,
                underline: false, wide: false, wide_spacer: false,
            });
        }

        // Render content spans.
        for span in &line.spans {
            for ch in span.text.chars() {
                if row.len() >= cols {
                    break;
                }
                row.push(TerminalCell {
                    c: ch,
                    fg: span.fg,
                    bg: DEFAULT_BG,
                    bold: false,
                    italic: false,
                    underline: false, wide: false, wide_spacer: false,
                });
            }
        }

        // Fill remaining columns with spaces.
        while row.len() < cols {
            row.push(TerminalCell {
                c: ' ',
                fg: DEFAULT_FG,
                bg: DEFAULT_BG,
                bold: false,
                italic: false,
                underline: false, wide: false, wide_spacer: false,
            });
        }

        output.push(row);
    }

    // Fill remaining rows with empty lines.
    while output.len() < rows {
        let row = vec![
            TerminalCell {
                c: ' ',
                fg: DEFAULT_FG,
                bg: DEFAULT_BG,
                bold: false,
                italic: false,
                underline: false, wide: false, wide_spacer: false,
            };
            cols
        ];
        output.push(row);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_code_produces_colored_spans() {
        let hl = SyntaxHighlighter::new();
        let code = "fn main() {\n    println!(\"hello\");\n}\n";
        let lines = hl.highlight_content(code, "rs");

        assert_eq!(lines.len(), 3);
        // First line should have spans for `fn`, `main`, etc.
        assert!(!lines[0].spans.is_empty(), "first line should have spans");
        assert_eq!(lines[0].line_number, 1);

        // At least some spans should have non-white colors (syntax highlighting).
        let has_color = lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|s| s.fg != Rgb::new(255, 255, 255))
        });
        assert!(has_color, "syntax highlighting should produce colored spans");
    }

    #[test]
    fn highlight_unknown_extension_falls_back_to_plain_text() {
        let hl = SyntaxHighlighter::new();
        let content = "some random content\nline two\n";
        let lines = hl.highlight_content(content, "zzz_unknown_ext_zzz");

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_number, 1);
        assert_eq!(lines[1].line_number, 2);
        // Plain text should still produce spans (just uniform color).
        assert!(!lines[0].spans.is_empty());
    }

    #[test]
    fn highlight_python_code() {
        let hl = SyntaxHighlighter::new();
        let code = "def hello():\n    print('world')\n";
        let lines = hl.highlight_content(code, "py");
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].spans.is_empty());
    }

    #[test]
    fn highlight_typescript_code() {
        let hl = SyntaxHighlighter::new();
        let code = "const x: number = 42;\n";
        let lines = hl.highlight_content(code, "ts");
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
    }

    #[test]
    fn highlight_go_code() {
        let hl = SyntaxHighlighter::new();
        let code = "package main\n\nfunc main() {}\n";
        let lines = hl.highlight_content(code, "go");
        assert!(!lines.is_empty());
    }

    #[test]
    fn to_terminal_cells_renders_line_numbers() {
        let hl = SyntaxHighlighter::new();
        let code = "hello\nworld\n";
        let lines = hl.highlight_content(code, "txt");
        let cells = to_terminal_cells(&lines, 40, 5, 0);

        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0].len(), 40);

        // First 4 chars should be the line number "   1" and 5th should be space separator.
        assert_eq!(cells[0][0].c, ' ');
        assert_eq!(cells[0][1].c, ' ');
        assert_eq!(cells[0][2].c, ' ');
        assert_eq!(cells[0][3].c, '1');
        assert_eq!(cells[0][4].c, ' ');
        // Line number cells should use the dim gutter color.
        assert_eq!(cells[0][3].fg, LINE_NUMBER_COLOR);

        // Content starts at column 5.
        assert_eq!(cells[0][5].c, 'h');
        assert_eq!(cells[0][6].c, 'e');

        // Second line should have line number 2.
        assert_eq!(cells[1][3].c, '2');
    }

    #[test]
    fn to_terminal_cells_scroll_offset() {
        let lines: Vec<HighlightedLine> = (1..=10)
            .map(|i| HighlightedLine {
                line_number: i,
                spans: vec![HighlightedSpan {
                    text: format!("line {i}"),
                    fg: DEFAULT_FG,
                }],
            })
            .collect();

        let cells = to_terminal_cells(&lines, 20, 3, 5);
        assert_eq!(cells.len(), 3);
        // First visible line should be line 6 (index 5).
        assert_eq!(cells[0][3].c, '6');
    }

    #[test]
    fn to_terminal_cells_empty_input() {
        let cells = to_terminal_cells(&[], 20, 3, 0);
        assert_eq!(cells.len(), 3);
        // All cells should be spaces.
        assert!(cells.iter().all(|row| row.iter().all(|c| c.c == ' ')));
    }
}
