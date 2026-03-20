use tracing::instrument;

use crate::session::pty::PtyHandle;
use crate::session::terminal::Terminal;

/// A dropdown shell overlay that can be toggled on/off.
///
/// Similar to a Quake-style dropdown console, this shell occupies a
/// configurable portion of the screen from the top and can be shown/hidden
/// with a single keybind.
#[allow(dead_code)]
pub struct DropdownShell {
    visible: bool,
    height_ratio: f32,
    pty: Option<PtyHandle>,
    terminal: Option<Terminal>,
}

#[allow(dead_code)]
impl DropdownShell {
    /// Create a new hidden dropdown shell with default height ratio (40%).
    pub fn new() -> Self {
        Self {
            visible: false,
            height_ratio: 0.4,
            pty: None,
            terminal: None,
        }
    }

    /// Toggle the dropdown shell visibility.
    ///
    /// If no PTY has been spawned yet, one is created with the given
    /// dimensions. The shell uses the full width (`cols`) and a portion
    /// of the height determined by `height_ratio`.
    #[instrument(skip(self))]
    pub fn toggle(&mut self, cols: u16, rows: u16) {
        if self.pty.is_none() {
            let shell_rows = ((rows as f32) * self.height_ratio).max(1.0) as u16;
            match PtyHandle::spawn(shell_rows, cols) {
                Ok(pty) => {
                    let terminal = Terminal::new(cols, shell_rows);
                    self.pty = Some(pty);
                    self.terminal = Some(terminal);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to spawn dropdown shell PTY");
                    return;
                }
            }
        }
        self.visible = !self.visible;
    }

    /// Return whether the dropdown shell is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Return the height ratio (fraction of screen height).
    pub fn height_ratio(&self) -> f32 {
        self.height_ratio
    }

    /// Set the height ratio, clamped to `0.1..=0.9`.
    pub fn set_height_ratio(&mut self, ratio: f32) {
        self.height_ratio = ratio.clamp(0.1, 0.9);
    }
}

impl Default for DropdownShell {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let shell = DropdownShell::new();
        assert!(!shell.is_visible());
        assert!((shell.height_ratio() - 0.4).abs() < f32::EPSILON);
        assert!(shell.pty.is_none());
        assert!(shell.terminal.is_none());
    }

    #[test]
    fn test_default_trait() {
        let shell = DropdownShell::default();
        assert!(!shell.is_visible());
        assert!((shell.height_ratio() - 0.4).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_toggle_visibility() {
        let mut shell = DropdownShell::new();

        assert!(!shell.is_visible());

        shell.toggle(80, 24);
        assert!(shell.is_visible());
        // PTY should have been spawned
        assert!(shell.pty.is_some());
        assert!(shell.terminal.is_some());

        shell.toggle(80, 24);
        assert!(!shell.is_visible());
        // PTY should still exist (not destroyed on hide)
        assert!(shell.pty.is_some());

        shell.toggle(80, 24);
        assert!(shell.is_visible());
    }

    #[test]
    fn test_height_ratio_clamping() {
        let mut shell = DropdownShell::new();

        shell.set_height_ratio(0.5);
        assert!((shell.height_ratio() - 0.5).abs() < f32::EPSILON);

        shell.set_height_ratio(0.0);
        assert!((shell.height_ratio() - 0.1).abs() < f32::EPSILON);

        shell.set_height_ratio(-1.0);
        assert!((shell.height_ratio() - 0.1).abs() < f32::EPSILON);

        shell.set_height_ratio(1.0);
        assert!((shell.height_ratio() - 0.9).abs() < f32::EPSILON);

        shell.set_height_ratio(2.0);
        assert!((shell.height_ratio() - 0.9).abs() < f32::EPSILON);

        shell.set_height_ratio(0.1);
        assert!((shell.height_ratio() - 0.1).abs() < f32::EPSILON);

        shell.set_height_ratio(0.9);
        assert!((shell.height_ratio() - 0.9).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_is_visible_reflects_state() {
        let mut shell = DropdownShell::new();
        assert!(!shell.is_visible());
        shell.toggle(80, 24);
        assert!(shell.is_visible());
    }
}
