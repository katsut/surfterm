pub mod pty;
pub mod state;
pub mod stream_splitter;
pub mod terminal;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Result};
use tokio::sync::watch;
use tracing::instrument;
use uuid::Uuid;

use crate::detector::patterns::default_claude_code_state_patterns;
use crate::detector::StateDetector;
use crate::session::pty::PtyHandle;
use crate::session::state::SessionState;
use crate::session::stream_splitter::StreamSplitter;
use crate::session::terminal::Terminal;

/// Unique identifier for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SessionId {
    /// Create a new random session ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl From<Uuid> for SessionId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single managed session with its own PTY, terminal emulator,
/// stream splitter, and state detector.
#[allow(dead_code)]
pub struct Session {
    id: SessionId,
    pty: PtyHandle,
    terminal: Terminal,
    splitter: StreamSplitter,
    detector: StateDetector,
    state_rx: watch::Receiver<SessionState>,
    channels: stream_splitter::Channels,
    project_name: String,
    cwd: PathBuf,
    created_at: Instant,
}

#[allow(dead_code)]
impl Session {
    /// Return the current detected session state.
    pub fn state(&self) -> SessionState {
        *self.state_rx.borrow()
    }

    /// Return the session's unique identifier.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Return the project name associated with this session.
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
}

/// Manages multiple concurrent sessions, each with independent PTY,
/// terminal, stream splitter, and state detector.
#[allow(dead_code)]
pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
    active_session: Option<SessionId>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl SessionManager {
    /// Create a new empty session manager.
    #[instrument]
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            active_session: None,
        }
    }

    /// Create a new session, spawning a PTY with the given (or default) shell.
    ///
    /// If `command` is `Some`, it is stored as metadata; the PTY currently
    /// always spawns the user's default shell (extending `PtyHandle` for
    /// custom commands is planned).
    ///
    /// If no session is currently active, the new session becomes active.
    #[instrument(skip(self, _command, cwd))]
    pub fn create_session(
        &mut self,
        _command: Option<&str>,
        cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId> {
        let id = SessionId::new();

        let cwd = cwd.unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        });

        let project_name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Spawn PTY
        let pty = PtyHandle::spawn(rows, cols, "", "")?;

        // Create terminal emulator
        let terminal = Terminal::new(cols, rows);

        // Create stream splitter with default Claude Code patterns
        let splitter_patterns = StreamSplitter::default_claude_code_patterns();
        let (splitter, channels) = StreamSplitter::new(splitter_patterns);

        // Create state detector with default patterns
        let detector_patterns = default_claude_code_state_patterns();
        let (detector, state_rx) = StateDetector::new(detector_patterns);

        let session = Session {
            id,
            pty,
            terminal,
            splitter,
            detector,
            state_rx,
            channels,
            project_name,
            cwd,
            created_at: Instant::now(),
        };

        self.sessions.insert(id, session);

        // Set as active if this is the first session
        if self.active_session.is_none() {
            self.active_session = Some(id);
        }

        Ok(id)
    }

    /// Remove a session and clean up its resources.
    ///
    /// If the killed session was the active session, the manager switches
    /// to another available session (if any).
    #[instrument(skip(self))]
    pub fn kill_session(&mut self, id: &SessionId) -> Result<()> {
        if self.sessions.remove(id).is_none() {
            bail!("session {} not found", id);
        }

        // If the killed session was active, switch to another
        if self.active_session == Some(*id) {
            self.active_session = self.sessions.keys().next().copied();
        }

        Ok(())
    }

    /// Return a reference to the currently active session, if any.
    pub fn active_session(&self) -> Option<&Session> {
        self.active_session
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    /// Return a mutable reference to the currently active session, if any.
    pub fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.active_session
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
    }

    /// Switch the active session to the given ID.
    #[instrument(skip(self))]
    pub fn switch_to(&mut self, id: &SessionId) -> Result<()> {
        if !self.sessions.contains_key(id) {
            bail!("session {} not found", id);
        }
        self.active_session = Some(*id);
        Ok(())
    }

    /// Return a list of all session IDs.
    pub fn session_ids(&self) -> Vec<SessionId> {
        self.sessions.keys().copied().collect()
    }

    /// Return the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Look up a session by ID.
    pub fn get_session(&self, id: &SessionId) -> Option<&Session> {
        self.sessions.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_manager_with_session() -> (SessionManager, SessionId) {
        let mut mgr = SessionManager::new();
        let id = mgr
            .create_session(None, None, 80, 24)
            .expect("create session");
        (mgr, id)
    }

    #[tokio::test]
    async fn test_create_session_exists() {
        let (mgr, id) = create_manager_with_session();
        assert!(mgr.get_session(&id).is_some());
    }

    #[tokio::test]
    async fn test_create_multiple_sessions() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        let id2 = mgr.create_session(None, None, 80, 24).unwrap();
        let id3 = mgr.create_session(None, None, 80, 24).unwrap();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_eq!(mgr.session_count(), 3);
        assert!(mgr.get_session(&id1).is_some());
        assert!(mgr.get_session(&id2).is_some());
        assert!(mgr.get_session(&id3).is_some());
    }

    #[tokio::test]
    async fn test_kill_session_removes_it() {
        let (mut mgr, id) = create_manager_with_session();
        assert_eq!(mgr.session_count(), 1);

        mgr.kill_session(&id).expect("kill session");
        assert_eq!(mgr.session_count(), 0);
        assert!(mgr.get_session(&id).is_none());
    }

    #[tokio::test]
    async fn test_active_session_set_on_creation() {
        let (mgr, id) = create_manager_with_session();
        let active = mgr.active_session().expect("should have active session");
        assert_eq!(active.id(), id);
    }

    #[tokio::test]
    async fn test_first_session_becomes_active() {
        let mut mgr = SessionManager::new();
        assert!(mgr.active_session().is_none());

        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        assert_eq!(mgr.active_session().unwrap().id(), id1);

        // Second session should NOT change active
        let _id2 = mgr.create_session(None, None, 80, 24).unwrap();
        assert_eq!(mgr.active_session().unwrap().id(), id1);
    }

    #[tokio::test]
    async fn test_switch_to_changes_active() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        let id2 = mgr.create_session(None, None, 80, 24).unwrap();

        assert_eq!(mgr.active_session().unwrap().id(), id1);

        mgr.switch_to(&id2).expect("switch to id2");
        assert_eq!(mgr.active_session().unwrap().id(), id2);

        mgr.switch_to(&id1).expect("switch to id1");
        assert_eq!(mgr.active_session().unwrap().id(), id1);
    }

    #[test]
    fn test_switch_to_nonexistent_fails() {
        let mut mgr = SessionManager::new();
        let fake_id = SessionId::new();
        assert!(mgr.switch_to(&fake_id).is_err());
    }

    #[tokio::test]
    async fn test_kill_active_switches_to_another() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        let id2 = mgr.create_session(None, None, 80, 24).unwrap();

        assert_eq!(mgr.active_session().unwrap().id(), id1);

        mgr.kill_session(&id1).expect("kill active session");
        assert_eq!(mgr.session_count(), 1);

        // Active should now be id2
        let active = mgr.active_session().expect("should have active");
        assert_eq!(active.id(), id2);
    }

    #[tokio::test]
    async fn test_kill_last_session_clears_active() {
        let (mut mgr, id) = create_manager_with_session();
        mgr.kill_session(&id).expect("kill session");
        assert!(mgr.active_session().is_none());
    }

    #[tokio::test]
    async fn test_session_count() {
        let mut mgr = SessionManager::new();
        assert_eq!(mgr.session_count(), 0);

        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        assert_eq!(mgr.session_count(), 1);

        let _id2 = mgr.create_session(None, None, 80, 24).unwrap();
        assert_eq!(mgr.session_count(), 2);

        mgr.kill_session(&id1).unwrap();
        assert_eq!(mgr.session_count(), 1);
    }

    #[tokio::test]
    async fn test_session_ids_returns_all() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.create_session(None, None, 80, 24).unwrap();
        let id2 = mgr.create_session(None, None, 80, 24).unwrap();

        let mut ids = mgr.session_ids();
        ids.sort_by_key(|id| id.0);
        let mut expected = vec![id1, id2];
        expected.sort_by_key(|id| id.0);
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn test_session_state_initial() {
        let (mgr, id) = create_manager_with_session();
        let session = mgr.get_session(&id).unwrap();
        assert_eq!(session.state(), SessionState::Idle);
    }

    #[tokio::test]
    async fn test_session_project_name() {
        let mut mgr = SessionManager::new();
        let cwd = PathBuf::from("/home/user/my-project");
        let id = mgr.create_session(None, Some(cwd), 80, 24).unwrap();
        let session = mgr.get_session(&id).unwrap();
        assert_eq!(session.project_name(), "my-project");
    }

    #[test]
    fn test_kill_nonexistent_session_fails() {
        let mut mgr = SessionManager::new();
        let fake_id = SessionId::new();
        assert!(mgr.kill_session(&fake_id).is_err());
    }

    #[tokio::test]
    async fn test_active_session_mut() {
        let (mut mgr, id) = create_manager_with_session();
        let active = mgr.active_session_mut().expect("should have active");
        assert_eq!(active.id(), id);
    }
}
