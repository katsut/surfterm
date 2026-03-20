pub mod patterns;
pub mod tool_registry;

use tokio::sync::watch;
use tracing::instrument;

use crate::session::state::SessionState;
use patterns::StatePattern;

/// Detects the current state of an AI coding tool session by matching
/// PTY output against a set of regex patterns.
#[allow(dead_code)]
pub struct StateDetector {
    patterns: Vec<StatePattern>,
    current_state: SessionState,
    state_tx: watch::Sender<SessionState>,
}

#[allow(dead_code)]
impl StateDetector {
    /// Create a new `StateDetector` with the given patterns.
    ///
    /// Returns the detector and a `watch::Receiver` that receives state
    /// transition notifications. The initial state is `Idle`.
    #[instrument(skip_all)]
    pub fn new(patterns: Vec<StatePattern>) -> (Self, watch::Receiver<SessionState>) {
        let (state_tx, state_rx) = watch::channel(SessionState::Idle);

        let detector = Self {
            patterns,
            current_state: SessionState::Idle,
            state_tx,
        };

        (detector, state_rx)
    }

    /// Analyze a chunk of PTY output data and update the detected state.
    ///
    /// The chunk is decoded as UTF-8 (lossy) and each line is matched against
    /// the stored patterns. The last matching pattern determines the new state.
    /// If the state changes, an event is emitted on the watch channel.
    #[instrument(skip_all)]
    pub fn process_chunk(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        let mut new_state: Option<SessionState> = None;

        for line in text.lines() {
            for pattern in &self.patterns {
                if pattern.regex.is_match(line) {
                    new_state = Some(pattern.target_state);
                    break;
                }
            }
        }

        if let Some(state) = new_state {
            if state != self.current_state {
                self.current_state = state;
                // Ignore send errors (no receivers).
                let _ = self.state_tx.send(state);
            }
        }
    }

    /// Return the current detected state.
    #[instrument(skip(self))]
    pub fn current_state(&self) -> SessionState {
        self.current_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patterns::default_claude_code_state_patterns;

    fn make_detector() -> (StateDetector, watch::Receiver<SessionState>) {
        let patterns = default_claude_code_state_patterns();
        StateDetector::new(patterns)
    }

    #[test]
    fn test_initial_state_is_idle() {
        let (detector, rx) = make_detector();
        assert_eq!(detector.current_state(), SessionState::Idle);
        assert_eq!(*rx.borrow(), SessionState::Idle);
    }

    #[test]
    fn test_waiting_for_input_detection() {
        let (mut detector, _rx) = make_detector();

        detector.process_chunk(b"> ");
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);

        // Reset then test another pattern
        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Would you like to proceed?");
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Do you want to continue?");
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Continue? Y/n");
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Proceed? yes/no");
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);
    }

    #[test]
    fn test_running_detection() {
        let (mut detector, _rx) = make_detector();
        detector.process_chunk("⏺ Read src/main.rs".as_bytes());
        assert_eq!(detector.current_state(), SessionState::Running);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Reading file contents...");
        assert_eq!(detector.current_state(), SessionState::Running);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Writing output to disk");
        assert_eq!(detector.current_state(), SessionState::Running);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Searching for pattern");
        assert_eq!(detector.current_state(), SessionState::Running);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Running cargo build");
        assert_eq!(detector.current_state(), SessionState::Running);
    }

    #[test]
    fn test_error_detection() {
        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Error: something went wrong");
        assert_eq!(detector.current_state(), SessionState::Error);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"error: compilation failed");
        assert_eq!(detector.current_state(), SessionState::Error);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"FAILED to build project");
        assert_eq!(detector.current_state(), SessionState::Error);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"thread 'main' panic at foo.rs");
        assert_eq!(detector.current_state(), SessionState::Error);

        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"Permission denied: /etc/shadow");
        assert_eq!(detector.current_state(), SessionState::Error);
    }

    #[test]
    fn test_state_transition_emits_events() {
        let (mut detector, rx) = make_detector();

        // Initial state
        assert_eq!(*rx.borrow(), SessionState::Idle);

        // Transition to Running
        detector.process_chunk("⏺ Read src/main.rs".as_bytes());
        assert_eq!(*rx.borrow(), SessionState::Running);

        // Transition to WaitingForInput
        detector.process_chunk(b"Would you like to continue?");
        assert_eq!(*rx.borrow(), SessionState::WaitingForInput);

        // Transition to Error
        detector.process_chunk(b"Error: something broke");
        assert_eq!(*rx.borrow(), SessionState::Error);
    }

    #[test]
    fn test_no_spurious_transitions() {
        let (mut detector, rx) = make_detector();

        // Unrecognized input should not change state from Idle
        detector.process_chunk(b"some random text that matches nothing");
        assert_eq!(detector.current_state(), SessionState::Idle);
        assert_eq!(*rx.borrow(), SessionState::Idle);

        // Move to Running
        detector.process_chunk("⏺ doing work".as_bytes());
        assert_eq!(detector.current_state(), SessionState::Running);

        // Same state again should not trigger a new send
        // (watch channel deduplicates by design — value stays the same)
        detector.process_chunk(b"Searching for files");
        assert_eq!(detector.current_state(), SessionState::Running);

        // Unrecognized text should not reset state
        detector.process_chunk(b"just ordinary output");
        assert_eq!(detector.current_state(), SessionState::Running);
    }

    #[test]
    fn test_multiline_chunk_uses_last_match() {
        let (mut detector, _rx) = make_detector();

        // Chunk with Running on first line, WaitingForInput on last
        let chunk = "⏺ Read file\nsome output\nWould you like to proceed?";
        detector.process_chunk(chunk.as_bytes());
        // The last matching line determines the state
        assert_eq!(detector.current_state(), SessionState::WaitingForInput);
    }

    #[test]
    fn test_empty_input_no_change() {
        let (mut detector, _rx) = make_detector();
        detector.process_chunk(b"");
        assert_eq!(detector.current_state(), SessionState::Idle);

        detector.process_chunk("⏺ work".as_bytes());
        assert_eq!(detector.current_state(), SessionState::Running);

        detector.process_chunk(b"");
        assert_eq!(detector.current_state(), SessionState::Running);
    }
}
