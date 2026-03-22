//! Diff computation and rendering using the `similar` crate.

use similar::{ChangeTag, TextDiff};
use tracing::instrument;

use crate::session::terminal::{Rgb, TerminalCell};

/// A single line in a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// Unchanged line present in both old and new.
    Equal(String),
    /// Line added in the new version.
    Added(String),
    /// Line removed from the old version.
    Removed(String),
}

/// A contiguous hunk of diff lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

/// Result of a diff computation containing all hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
}

/// Compute a line-level diff between `old` and `new` text.
///
/// Returns a [`DiffResult`] with hunks grouped by contiguous change regions
/// (with context lines).
#[allow(dead_code)]
#[instrument(skip_all)]
pub fn compute_diff(old: &str, new: &str) -> DiffResult {
    let text_diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();

    // Collect all changes and group into hunks.
    // A hunk boundary occurs when we see context (Equal) lines that are far
    // apart from changes. We use a simple approach: one hunk per contiguous
    // block of changes with up to 3 lines of surrounding context.
    let changes: Vec<_> = text_diff.iter_all_changes().collect();
    if changes.is_empty() {
        return DiffResult { hunks };
    }

    // Find ranges of non-equal changes and expand with context.
    let mut change_indices: Vec<usize> = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        if change.tag() != ChangeTag::Equal {
            change_indices.push(i);
        }
    }

    if change_indices.is_empty() {
        return DiffResult { hunks };
    }

    // Group change indices into hunks (merge if context gap <= 6 lines).
    let mut groups: Vec<(usize, usize)> = Vec::new(); // (first_change_idx, last_change_idx)
    let mut group_start = change_indices[0];
    let mut group_end = change_indices[0];

    for &idx in &change_indices[1..] {
        if idx - group_end <= 6 {
            group_end = idx;
        } else {
            groups.push((group_start, group_end));
            group_start = idx;
            group_end = idx;
        }
    }
    groups.push((group_start, group_end));

    // Build hunks with context.
    for (start, end) in groups {
        let ctx_start = start.saturating_sub(3);
        let ctx_end = (end + 3).min(changes.len() - 1);

        let mut lines = Vec::new();

        for change in &changes[ctx_start..=ctx_end] {
            let text = change.value().trim_end_matches('\n').to_string();
            match change.tag() {
                ChangeTag::Equal => lines.push(DiffLine::Equal(text)),
                ChangeTag::Insert => lines.push(DiffLine::Added(text)),
                ChangeTag::Delete => lines.push(DiffLine::Removed(text)),
            }
        }

        // Determine start line numbers. For removals at the very start,
        // old_index is Some but new_index may be None. Use the first
        // available index, defaulting to 1.
        let first_change = &changes[ctx_start];
        let os = first_change.old_index().map(|i| i + 1).unwrap_or(1);
        let ns = first_change.new_index().map(|i| i + 1).unwrap_or(1);

        hunks.push(DiffHunk {
            old_start: os,
            new_start: ns,
            lines,
        });
    }

    DiffResult { hunks }
}

// Colors for diff rendering.
const ADDED_BG: Rgb = Rgb::new(0x2a, 0x35, 0x25);
const REMOVED_BG: Rgb = Rgb::new(0x3b, 0x25, 0x30);
const DEFAULT_BG: Rgb = Rgb::new(0, 0, 0);
const DEFAULT_FG: Rgb = Rgb::new(205, 214, 244);
const ADDED_FG: Rgb = Rgb::new(0xa6, 0xe3, 0xa1);
const REMOVED_FG: Rgb = Rgb::new(0xf3, 0x8b, 0xa8);
const LINE_NUMBER_COLOR: Rgb = Rgb::new(0x6c, 0x70, 0x86);
const HUNK_HEADER_FG: Rgb = Rgb::new(0x6c, 0x70, 0x86);

/// Width of line-number gutter (4 digits + space).
#[cfg(test)]
const LINE_NUMBER_WIDTH: usize = 5;

/// Convert a diff result to terminal cells for rendering.
///
/// Each hunk is preceded by a header line. Added lines get a green-tinted
/// background, removed lines get a red-tinted background, and equal lines
/// use the default background.
#[allow(dead_code)]
pub fn to_terminal_cells(diff: &DiffResult, cols: u16, rows: u16) -> Vec<Vec<TerminalCell>> {
    let cols = cols as usize;
    let rows = rows as usize;
    let mut output: Vec<Vec<TerminalCell>> = Vec::new();

    for hunk in &diff.hunks {
        if output.len() >= rows {
            break;
        }

        // Hunk header line (e.g., "@@ -1,3 +1,4 @@").
        let old_count = hunk
            .lines
            .iter()
            .filter(|l| !matches!(l, DiffLine::Added(_)))
            .count();
        let new_count = hunk
            .lines
            .iter()
            .filter(|l| !matches!(l, DiffLine::Removed(_)))
            .count();
        let header = format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, old_count, hunk.new_start, new_count,
        );
        output.push(make_row(&header, HUNK_HEADER_FG, DEFAULT_BG, cols, None));

        // Render each diff line.
        let mut old_line = hunk.old_start;
        let mut new_line = hunk.new_start;

        for diff_line in &hunk.lines {
            if output.len() >= rows {
                break;
            }

            match diff_line {
                DiffLine::Equal(text) => {
                    let line_num = new_line;
                    output.push(make_row(text, DEFAULT_FG, DEFAULT_BG, cols, Some(line_num)));
                    old_line += 1;
                    new_line += 1;
                }
                DiffLine::Added(text) => {
                    let line_num = new_line;
                    output.push(make_diff_row(
                        text,
                        ADDED_FG,
                        ADDED_BG,
                        '+',
                        cols,
                        Some(line_num),
                    ));
                    new_line += 1;
                }
                DiffLine::Removed(text) => {
                    let line_num = old_line;
                    output.push(make_diff_row(
                        text,
                        REMOVED_FG,
                        REMOVED_BG,
                        '-',
                        cols,
                        Some(line_num),
                    ));
                    old_line += 1;
                }
            }
        }
    }

    // Fill remaining rows with empty space.
    while output.len() < rows {
        output.push(vec![
            TerminalCell {
                c: ' ',
                fg: DEFAULT_FG,
                bg: DEFAULT_BG,
                bold: false,
                italic: false,
                underline: false, wide: false, wide_spacer: false,
            };
            cols
        ]);
    }

    // Truncate if we generated more rows than available.
    output.truncate(rows);
    output
}

/// Build a plain row with optional line number.
fn make_row(
    text: &str,
    fg: Rgb,
    bg: Rgb,
    cols: usize,
    line_num: Option<usize>,
) -> Vec<TerminalCell> {
    let mut row = Vec::with_capacity(cols);

    // Line number gutter.
    let num_str = match line_num {
        Some(n) => format!("{n:>4} "),
        None => "     ".to_string(),
    };
    for ch in num_str.chars() {
        if row.len() >= cols {
            break;
        }
        row.push(TerminalCell {
            c: ch,
            fg: LINE_NUMBER_COLOR,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    // Content.
    for ch in text.chars() {
        if row.len() >= cols {
            break;
        }
        row.push(TerminalCell {
            c: ch,
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    // Pad.
    while row.len() < cols {
        row.push(TerminalCell {
            c: ' ',
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    row
}

/// Build a diff row with a prefix character (+/-) and colored background.
fn make_diff_row(
    text: &str,
    fg: Rgb,
    bg: Rgb,
    prefix: char,
    cols: usize,
    line_num: Option<usize>,
) -> Vec<TerminalCell> {
    let mut row = Vec::with_capacity(cols);

    // Line number gutter.
    let num_str = match line_num {
        Some(n) => format!("{n:>4} "),
        None => "     ".to_string(),
    };
    for ch in num_str.chars() {
        if row.len() >= cols {
            break;
        }
        row.push(TerminalCell {
            c: ch,
            fg: LINE_NUMBER_COLOR,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    // Prefix character.
    if row.len() < cols {
        row.push(TerminalCell {
            c: prefix,
            fg,
            bg,
            bold: true,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    // Content.
    for ch in text.chars() {
        if row.len() >= cols {
            break;
        }
        row.push(TerminalCell {
            c: ch,
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    // Pad.
    while row.len() < cols {
        row.push(TerminalCell {
            c: ' ',
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false, wide: false, wide_spacer: false,
        });
    }

    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_diff_with_additions() {
        let old = "line1\nline2\n";
        let new = "line1\nline2\nline3\n";
        let result = compute_diff(old, new);

        assert!(!result.hunks.is_empty());
        let has_added = result
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Added(_))));
        assert!(has_added, "diff should contain added lines");
    }

    #[test]
    fn compute_diff_with_removals() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline3\n";
        let result = compute_diff(old, new);

        assert!(!result.hunks.is_empty());
        let has_removed = result
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Removed(_))));
        assert!(has_removed, "diff should contain removed lines");
    }

    #[test]
    fn compute_diff_with_equal_lines() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2_modified\nline3\n";
        let result = compute_diff(old, new);

        assert!(!result.hunks.is_empty());
        let has_equal = result
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Equal(_))));
        assert!(has_equal, "diff should contain equal lines as context");
    }

    #[test]
    fn compute_diff_empty_inputs() {
        let result = compute_diff("", "");
        assert!(
            result.hunks.is_empty(),
            "identical empty inputs should produce no hunks"
        );
    }

    #[test]
    fn compute_diff_identical_inputs() {
        let text = "hello\nworld\n";
        let result = compute_diff(text, text);
        assert!(
            result.hunks.is_empty(),
            "identical inputs should produce no hunks"
        );
    }

    #[test]
    fn compute_diff_empty_to_content() {
        let result = compute_diff("", "new line\n");
        assert!(!result.hunks.is_empty());
        let has_added = result
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Added(_))));
        assert!(has_added);
    }

    #[test]
    fn compute_diff_content_to_empty() {
        let result = compute_diff("old line\n", "");
        assert!(!result.hunks.is_empty());
        let has_removed = result
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| matches!(l, DiffLine::Removed(_))));
        assert!(has_removed);
    }

    #[test]
    fn to_terminal_cells_colors_added_green() {
        let diff = DiffResult {
            hunks: vec![DiffHunk {
                old_start: 1,
                new_start: 1,
                lines: vec![DiffLine::Added("added line".to_string())],
            }],
        };
        let cells = to_terminal_cells(&diff, 40, 5);

        // Row 0 is the hunk header, row 1 is the added line.
        assert_eq!(cells.len(), 5);
        // The added line (row 1) should have green background.
        let added_row = &cells[1];
        // Content cells (after line number gutter) should have ADDED_BG.
        assert_eq!(added_row[LINE_NUMBER_WIDTH].bg, ADDED_BG);
        assert_eq!(added_row[LINE_NUMBER_WIDTH].fg, ADDED_FG);
    }

    #[test]
    fn to_terminal_cells_colors_removed_red() {
        let diff = DiffResult {
            hunks: vec![DiffHunk {
                old_start: 1,
                new_start: 1,
                lines: vec![DiffLine::Removed("removed line".to_string())],
            }],
        };
        let cells = to_terminal_cells(&diff, 40, 5);

        let removed_row = &cells[1];
        assert_eq!(removed_row[LINE_NUMBER_WIDTH].bg, REMOVED_BG);
        assert_eq!(removed_row[LINE_NUMBER_WIDTH].fg, REMOVED_FG);
    }

    #[test]
    fn to_terminal_cells_equal_default_bg() {
        let diff = DiffResult {
            hunks: vec![DiffHunk {
                old_start: 1,
                new_start: 1,
                lines: vec![DiffLine::Equal("unchanged".to_string())],
            }],
        };
        let cells = to_terminal_cells(&diff, 40, 5);

        let equal_row = &cells[1];
        assert_eq!(equal_row[LINE_NUMBER_WIDTH].bg, DEFAULT_BG);
    }

    #[test]
    fn to_terminal_cells_fills_remaining_rows() {
        let diff = DiffResult { hunks: vec![] };
        let cells = to_terminal_cells(&diff, 20, 3);
        assert_eq!(cells.len(), 3);
        assert!(cells.iter().all(|row| row.len() == 20));
    }

    #[test]
    fn to_terminal_cells_has_line_numbers() {
        let diff = DiffResult {
            hunks: vec![DiffHunk {
                old_start: 1,
                new_start: 1,
                lines: vec![
                    DiffLine::Equal("first".to_string()),
                    DiffLine::Added("second".to_string()),
                ],
            }],
        };
        let cells = to_terminal_cells(&diff, 40, 10);

        // Row 0 is hunk header (no line number, just spaces in gutter).
        // Row 1 is Equal line starting at new_start=1.
        assert_eq!(cells[1][3].c, '1');
        assert_eq!(cells[1][3].fg, LINE_NUMBER_COLOR);
        // Row 2 is Added line at new_start=2.
        assert_eq!(cells[2][3].c, '2');
    }
}
