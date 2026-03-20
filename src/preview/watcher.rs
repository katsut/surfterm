use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use tokio::sync::mpsc;
use tracing::instrument;

/// The kind of file change detected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

/// A file change event emitted by the watcher.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

/// Extracts file paths from State channel text (tool output lines).
#[allow(dead_code)]
pub struct ToolOutputMonitor {
    patterns: Vec<Regex>,
}

#[allow(dead_code)]
impl Default for ToolOutputMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl ToolOutputMonitor {
    /// Create a new `ToolOutputMonitor` with default extraction patterns.
    #[instrument]
    pub fn new() -> Self {
        let patterns = vec![
            // "Read src/main.rs", "Write src/lib.rs", "Edit src/app.rs"
            Regex::new(r"(?:Read|Write|Edit)\s+(\S+)").expect("invalid tool pattern"),
            // "Created file: src/new.rs"
            Regex::new(r"Created file:\s+(\S+)").expect("invalid created-file pattern"),
        ];
        Self { patterns }
    }

    /// Extract file paths from a block of tool-output text.
    ///
    /// Recognises lines such as:
    /// - `Read src/main.rs`
    /// - `Write src/lib.rs`
    /// - `Edit src/app.rs`
    /// - `Created file: src/new.rs`
    #[instrument(skip_all)]
    pub fn extract_paths(text: &str) -> Vec<PathBuf> {
        let monitor = Self::new();
        let mut paths = Vec::new();

        for line in text.lines() {
            for pattern in &monitor.patterns {
                if let Some(caps) = pattern.captures(line) {
                    if let Some(m) = caps.get(1) {
                        paths.push(PathBuf::from(m.as_str()));
                    }
                }
            }
        }

        paths
    }
}

/// Watches directories for file-system changes and emits [`FileChangeEvent`]s.
#[allow(dead_code)]
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    _event_tx: mpsc::Sender<FileChangeEvent>,
    watched_dirs: HashSet<PathBuf>,
}

#[allow(dead_code)]
impl FileWatcher {
    /// Create a new `FileWatcher`.
    ///
    /// Returns the watcher and a receiver for [`FileChangeEvent`]s.
    #[instrument(skip_all)]
    pub fn new() -> Result<(Self, mpsc::Receiver<FileChangeEvent>)> {
        let (tx, rx) = mpsc::channel::<FileChangeEvent>(256);
        let event_tx = tx.clone();

        let watcher = notify::recommended_watcher(move |res: std::result::Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let kind = match event.kind {
                    EventKind::Create(_) => Some(ChangeKind::Created),
                    EventKind::Modify(_) => Some(ChangeKind::Modified),
                    EventKind::Remove(_) => Some(ChangeKind::Deleted),
                    _ => None,
                };

                if let Some(kind) = kind {
                    for path in event.paths {
                        let _ = tx.blocking_send(FileChangeEvent {
                            path,
                            kind: kind.clone(),
                        });
                    }
                }
            }
        })
        .context("failed to create file watcher")?;

        Ok((
            Self {
                watcher,
                _event_tx: event_tx,
                watched_dirs: HashSet::new(),
            },
            rx,
        ))
    }

    /// Start watching a directory recursively.
    #[instrument(skip(self))]
    pub fn watch_dir(&mut self, path: &Path) -> Result<()> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize path: {}", path.display()))?;

        self.watcher
            .watch(&canonical, RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch directory: {}", canonical.display()))?;

        self.watched_dirs.insert(canonical);
        Ok(())
    }

    /// Stop watching a directory.
    #[instrument(skip(self))]
    pub fn unwatch_dir(&mut self, path: &Path) -> Result<()> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize path: {}", path.display()))?;

        self.watcher
            .unwatch(&canonical)
            .with_context(|| format!("failed to unwatch directory: {}", canonical.display()))?;

        self.watched_dirs.remove(&canonical);
        Ok(())
    }

    /// Return the set of currently watched directories.
    pub fn watched_dirs(&self) -> &HashSet<PathBuf> {
        &self.watched_dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    // --- ToolOutputMonitor tests ---

    #[test]
    fn test_extract_paths_from_tool_output_lines() {
        let text = "Read src/main.rs\nWrite src/lib.rs\nEdit src/app.rs";
        let paths = ToolOutputMonitor::extract_paths(text);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/lib.rs"),
                PathBuf::from("src/app.rs"),
            ]
        );
    }

    #[test]
    fn test_extract_paths_handles_various_formats() {
        let text = "Created file: src/new.rs\n⏺ Read docs/README.md\nWrite tests/test_foo.rs";
        let paths = ToolOutputMonitor::extract_paths(text);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("src/new.rs"),
                PathBuf::from("docs/README.md"),
                PathBuf::from("tests/test_foo.rs"),
            ]
        );
    }

    #[test]
    fn test_extract_paths_returns_empty_for_non_tool_lines() {
        let text = "Hello, I'll help you with that\nCost: $0.05\nsome random text";
        let paths = ToolOutputMonitor::extract_paths(text);
        assert!(paths.is_empty());
    }

    // --- FileWatcher tests ---

    #[test]
    fn test_file_watcher_creation_succeeds() {
        let result = FileWatcher::new();
        assert!(result.is_ok(), "FileWatcher::new() should succeed");
    }

    #[test]
    fn test_watch_and_unwatch_dir() {
        let tmp = TempDir::new().unwrap();
        let (mut watcher, _rx) = FileWatcher::new().unwrap();

        // watch
        watcher.watch_dir(tmp.path()).unwrap();
        assert_eq!(watcher.watched_dirs().len(), 1);
        assert!(watcher
            .watched_dirs()
            .contains(&tmp.path().canonicalize().unwrap()));

        // unwatch
        watcher.unwatch_dir(tmp.path()).unwrap();
        assert!(watcher.watched_dirs().is_empty());
    }

    #[tokio::test]
    async fn test_file_modification_triggers_event() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.txt");
        fs::write(&file_path, "initial").unwrap();

        let (mut watcher, mut rx) = FileWatcher::new().unwrap();
        watcher.watch_dir(tmp.path()).unwrap();

        // Small delay to let the watcher set up.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Modify the file.
        fs::write(&file_path, "modified").unwrap();

        // Wait for the event with a timeout.
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for file change event")
            .expect("channel closed unexpectedly");

        assert_eq!(event.path, file_path.canonicalize().unwrap());
        // On macOS the modify may come through as Created or Modified depending
        // on the backend, so we accept either.
        assert!(
            event.kind == ChangeKind::Modified || event.kind == ChangeKind::Created,
            "expected Modified or Created, got {:?}",
            event.kind
        );
    }
}
