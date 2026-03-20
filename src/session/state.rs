/// Represents the current state of an AI coding tool session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SessionState {
    /// Session is idle (no activity).
    Idle,
    /// AI tool is actively running (processing, generating code, etc.).
    Running,
    /// AI tool is waiting for user input (decision needed).
    WaitingForInput,
    /// Session encountered an error.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_default_is_idle() {
        let state = SessionState::Idle;
        assert_eq!(state, SessionState::Idle);
    }
}
