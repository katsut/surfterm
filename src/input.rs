use tracing::instrument;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// The current input mode of the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum InputMode {
    /// Normal mode: keys are interpreted as Surfterm commands.
    Normal,
    /// Insert mode: keys are forwarded to the PTY.
    Insert,
}

/// Internal commands that Surfterm handles itself (not sent to the PTY).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SurftermCmd {
    ToggleRawView,
    Quit,
    SwitchToNormal,
    SwitchToInsert,
}

/// The result of processing a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum InputAction {
    /// Encoded bytes to send to the PTY.
    SendToPty(Vec<u8>),
    /// A Surfterm internal command.
    SurftermCommand(SurftermCmd),
    /// No action (key ignored).
    None,
}

/// Manages the current input mode and translates key events into actions.
#[allow(dead_code)]
pub struct InputHandler {
    mode: InputMode,
    modifiers: ModifiersState,
}

#[allow(dead_code)]
impl InputHandler {
    /// Create a new `InputHandler` starting in Insert mode (Phase 1 default).
    pub fn new() -> Self {
        Self {
            mode: InputMode::Insert,
            modifiers: ModifiersState::empty(),
        }
    }

    /// Return the current input mode.
    pub fn mode(&self) -> InputMode {
        self.mode
    }

    /// Update the modifier state. Call this when `ModifiersChanged` events arrive.
    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    /// Process a winit `KeyEvent` and return the appropriate action.
    ///
    /// Only `ElementState::Pressed` events are handled; releases are ignored.
    #[instrument(skip(self, event))]
    pub fn handle_key(&mut self, event: &KeyEvent) -> InputAction {
        if event.state != ElementState::Pressed {
            return InputAction::None;
        }
        self.process_key(&event.logical_key)
    }

    /// Core key processing logic, separated from winit's `KeyEvent` for testability.
    ///
    /// Takes a logical key reference and the current modifier state (stored in self).
    fn process_key(&mut self, logical_key: &Key) -> InputAction {
        match self.mode {
            InputMode::Insert => self.handle_insert_key(logical_key),
            InputMode::Normal => self.handle_normal_key(logical_key),
        }
    }

    fn handle_insert_key(&mut self, logical_key: &Key) -> InputAction {
        // Escape switches to Normal mode.
        if *logical_key == Key::Named(NamedKey::Escape) {
            self.mode = InputMode::Normal;
            return InputAction::SurftermCommand(SurftermCmd::SwitchToNormal);
        }

        match encode_key(logical_key, self.modifiers) {
            Some(bytes) => InputAction::SendToPty(bytes),
            None => InputAction::None,
        }
    }

    fn handle_normal_key(&mut self, logical_key: &Key) -> InputAction {
        match logical_key {
            Key::Character(c) => match c.as_str() {
                "i" => {
                    self.mode = InputMode::Insert;
                    InputAction::SurftermCommand(SurftermCmd::SwitchToInsert)
                }
                "r" => InputAction::SurftermCommand(SurftermCmd::ToggleRawView),
                "q" => InputAction::SurftermCommand(SurftermCmd::Quit),
                _ => InputAction::None,
            },
            _ => InputAction::None,
        }
    }
}

/// Encode a logical key (with modifiers) into the byte sequence expected by a PTY.
#[allow(dead_code)]
pub fn encode_key(key: &Key, modifiers: ModifiersState) -> Option<Vec<u8>> {
    // Ctrl+letter produces control codes \x01 .. \x1a
    if modifiers.control_key() {
        if let Key::Character(c) = key {
            let ch = c.as_str().chars().next()?;
            if ch.is_ascii_lowercase() {
                return Some(vec![ch as u8 - b'a' + 1]);
            }
            if ch.is_ascii_uppercase() {
                return Some(vec![ch.to_ascii_lowercase() as u8 - b'a' + 1]);
            }
        }
    }

    match key {
        Key::Named(named) => encode_named_key(named),
        Key::Character(c) => Some(c.as_str().as_bytes().to_vec()),
        _ => None,
    }
}

/// Encode named (special) keys into their ANSI / VT escape sequences.
fn encode_named_key(key: &NamedKey) -> Option<Vec<u8>> {
    match key {
        NamedKey::Enter => Some(b"\r".to_vec()),
        NamedKey::Tab => Some(b"\t".to_vec()),
        NamedKey::Backspace => Some(vec![0x7f]),
        NamedKey::Escape => Some(vec![0x1b]),
        NamedKey::ArrowUp => Some(b"\x1b[A".to_vec()),
        NamedKey::ArrowDown => Some(b"\x1b[B".to_vec()),
        NamedKey::ArrowRight => Some(b"\x1b[C".to_vec()),
        NamedKey::ArrowLeft => Some(b"\x1b[D".to_vec()),
        NamedKey::Home => Some(b"\x1b[H".to_vec()),
        NamedKey::End => Some(b"\x1b[F".to_vec()),
        NamedKey::Insert => Some(b"\x1b[2~".to_vec()),
        NamedKey::Delete => Some(b"\x1b[3~".to_vec()),
        NamedKey::PageUp => Some(b"\x1b[5~".to_vec()),
        NamedKey::PageDown => Some(b"\x1b[6~".to_vec()),
        NamedKey::F1 => Some(b"\x1bOP".to_vec()),
        NamedKey::F2 => Some(b"\x1bOQ".to_vec()),
        NamedKey::F3 => Some(b"\x1bOR".to_vec()),
        NamedKey::F4 => Some(b"\x1bOS".to_vec()),
        NamedKey::F5 => Some(b"\x1b[15~".to_vec()),
        NamedKey::F6 => Some(b"\x1b[17~".to_vec()),
        NamedKey::F7 => Some(b"\x1b[18~".to_vec()),
        NamedKey::F8 => Some(b"\x1b[19~".to_vec()),
        NamedKey::F9 => Some(b"\x1b[20~".to_vec()),
        NamedKey::F10 => Some(b"\x1b[21~".to_vec()),
        NamedKey::F11 => Some(b"\x1b[23~".to_vec()),
        NamedKey::F12 => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    /// Helper: create a character Key.
    fn char_key(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    /// Helper: create a named Key.
    fn named_key(k: NamedKey) -> Key {
        Key::Named(k)
    }

    // --- Insert mode tests ---

    #[test]
    fn regular_char_in_insert_mode() {
        let mut handler = InputHandler::new();
        assert_eq!(handler.mode(), InputMode::Insert);
        let action = handler.process_key(&char_key("a"));
        assert_eq!(action, InputAction::SendToPty(b"a".to_vec()));
    }

    #[test]
    fn enter_key_sends_cr() {
        let mut handler = InputHandler::new();
        let action = handler.process_key(&named_key(NamedKey::Enter));
        assert_eq!(action, InputAction::SendToPty(b"\r".to_vec()));
    }

    #[test]
    fn ctrl_c_sends_etx() {
        let mut handler = InputHandler::new();
        handler.set_modifiers(ModifiersState::CONTROL);
        let action = handler.process_key(&char_key("c"));
        assert_eq!(action, InputAction::SendToPty(vec![0x03]));
    }

    #[test]
    fn arrow_up_sends_escape_sequence() {
        let mut handler = InputHandler::new();
        let action = handler.process_key(&named_key(NamedKey::ArrowUp));
        assert_eq!(action, InputAction::SendToPty(b"\x1b[A".to_vec()));
    }

    #[test]
    fn escape_in_insert_switches_to_normal() {
        let mut handler = InputHandler::new();
        assert_eq!(handler.mode(), InputMode::Insert);
        let action = handler.process_key(&named_key(NamedKey::Escape));
        assert_eq!(
            action,
            InputAction::SurftermCommand(SurftermCmd::SwitchToNormal)
        );
        assert_eq!(handler.mode(), InputMode::Normal);
    }

    // --- Normal mode tests ---

    #[test]
    fn i_in_normal_switches_to_insert() {
        let mut handler = InputHandler::new();
        // Switch to Normal first.
        handler.process_key(&named_key(NamedKey::Escape));
        assert_eq!(handler.mode(), InputMode::Normal);

        let action = handler.process_key(&char_key("i"));
        assert_eq!(
            action,
            InputAction::SurftermCommand(SurftermCmd::SwitchToInsert)
        );
        assert_eq!(handler.mode(), InputMode::Insert);
    }

    #[test]
    fn r_in_normal_toggles_raw_view() {
        let mut handler = InputHandler::new();
        handler.process_key(&named_key(NamedKey::Escape));
        assert_eq!(handler.mode(), InputMode::Normal);

        let action = handler.process_key(&char_key("r"));
        assert_eq!(
            action,
            InputAction::SurftermCommand(SurftermCmd::ToggleRawView)
        );
    }

    #[test]
    fn q_in_normal_quits() {
        let mut handler = InputHandler::new();
        handler.process_key(&named_key(NamedKey::Escape));
        let action = handler.process_key(&char_key("q"));
        assert_eq!(action, InputAction::SurftermCommand(SurftermCmd::Quit));
    }

    #[test]
    fn unknown_key_in_normal_mode_is_ignored() {
        let mut handler = InputHandler::new();
        handler.process_key(&named_key(NamedKey::Escape));
        let action = handler.process_key(&char_key("x"));
        assert_eq!(action, InputAction::None);
    }

    // --- Encoding tests ---

    #[test]
    fn tab_sends_tab_byte() {
        let mut handler = InputHandler::new();
        let action = handler.process_key(&named_key(NamedKey::Tab));
        assert_eq!(action, InputAction::SendToPty(b"\t".to_vec()));
    }

    #[test]
    fn backspace_sends_del() {
        let mut handler = InputHandler::new();
        let action = handler.process_key(&named_key(NamedKey::Backspace));
        assert_eq!(action, InputAction::SendToPty(vec![0x7f]));
    }

    #[test]
    fn ctrl_d_sends_eot() {
        let mut handler = InputHandler::new();
        handler.set_modifiers(ModifiersState::CONTROL);
        let action = handler.process_key(&char_key("d"));
        assert_eq!(action, InputAction::SendToPty(vec![0x04]));
    }

    #[test]
    fn ctrl_z_sends_sub() {
        let mut handler = InputHandler::new();
        handler.set_modifiers(ModifiersState::CONTROL);
        let action = handler.process_key(&char_key("z"));
        assert_eq!(action, InputAction::SendToPty(vec![0x1a]));
    }

    #[test]
    fn ctrl_l_sends_ff() {
        let mut handler = InputHandler::new();
        handler.set_modifiers(ModifiersState::CONTROL);
        let action = handler.process_key(&char_key("l"));
        assert_eq!(action, InputAction::SendToPty(vec![0x0c]));
    }

    #[test]
    fn encode_key_arrow_down() {
        let result = encode_key(&named_key(NamedKey::ArrowDown), ModifiersState::empty());
        assert_eq!(result, Some(b"\x1b[B".to_vec()));
    }

    #[test]
    fn encode_key_arrow_left() {
        let result = encode_key(&named_key(NamedKey::ArrowLeft), ModifiersState::empty());
        assert_eq!(result, Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn encode_key_arrow_right() {
        let result = encode_key(&named_key(NamedKey::ArrowRight), ModifiersState::empty());
        assert_eq!(result, Some(b"\x1b[C".to_vec()));
    }
}
