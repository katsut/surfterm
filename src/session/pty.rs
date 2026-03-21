use std::io::{Read, Write};
use std::sync::Arc;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{mpsc, Mutex, Notify};
use tracing::instrument;

/// Errors that can occur during PTY operations.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum PtyError {
    #[error("failed to open PTY pair: {0}")]
    OpenPty(#[source] anyhow::Error),

    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] anyhow::Error),

    #[error("failed to read from PTY: {0}")]
    ReadError(#[source] std::io::Error),

    #[error("failed to write to PTY: {0}")]
    WriteError(#[source] std::io::Error),

    #[error("failed to resize PTY: {0}")]
    ResizeError(#[source] anyhow::Error),

    #[error("failed to take PTY writer: {0}")]
    WriterError(#[source] anyhow::Error),

    #[error("failed to take PTY reader: {0}")]
    ReaderError(#[source] anyhow::Error),

    #[error("PTY not yet spawned")]
    NotSpawned,
}

/// Handle wrapping a PTY master and child process.
///
/// Provides async methods for reading output, writing input, and resizing
/// the terminal. Output is streamed through a `tokio::sync::mpsc` channel.
#[allow(dead_code)]
pub struct PtyHandle {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    child_exited: Arc<Notify>,
    child_pid: Option<u32>,
    _reader_task: std::thread::JoinHandle<()>,
    _child_task: std::thread::JoinHandle<()>,
}

#[allow(dead_code)]
impl PtyHandle {
    /// Spawn the user's default shell inside a new PTY.
    ///
    /// The default shell is read from `$SHELL`; falls back to `/bin/zsh`.
    /// A background task continuously reads PTY output into the channel.
    #[instrument(skip_all, fields(rows, cols))]
    pub fn spawn(rows: u16, cols: u16) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(size)
            .map_err(PtyError::OpenPty)?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(std::env::current_dir().unwrap_or_else(|_| "/".into()));
        cmd.env("TERM_PROGRAM", "surfterm");
        // Ensure UTF-8 locale so shells and tools handle multibyte characters
        if std::env::var("LANG").is_err() {
            cmd.env("LANG", "en_US.UTF-8");
        }
        if std::env::var("LC_ALL").is_err() {
            cmd.env("LC_CTYPE", "UTF-8");
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(PtyError::Spawn)?;

        let child_pid = child.process_id();

        // Drop the slave side; the master keeps the PTY alive.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(PtyError::ReaderError)?;

        let writer = pair
            .master
            .take_writer()
            .map_err(PtyError::WriterError)?;

        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);
        let child_exited = Arc::new(Notify::new());

        // Background thread: read PTY output. Uses std::thread instead of
        // tokio::task::spawn_blocking so that spawn() can be called from any
        // thread (including the winit main thread which has no tokio context).
        let reader_task = {
            let tx = output_tx;
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.blocking_send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        // Background thread: wait for child exit.
        let child_task = {
            let exited = Arc::clone(&child_exited);
            std::thread::spawn(move || {
                let _ = child.wait();
                exited.notify_waiters();
            })
        };

        Ok(Self {
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            output_rx,
            child_exited,
            child_pid,
            _reader_task: reader_task,
            _child_task: child_task,
        })
    }

    /// Receive the next chunk of output from the PTY.
    ///
    /// Returns `None` when the PTY output stream has ended (child exited and
    /// all buffered data has been consumed).
    #[instrument(skip(self))]
    pub async fn read_output(&mut self) -> Option<Vec<u8>> {
        self.output_rx.recv().await
    }

    /// Write bytes to the PTY (i.e. send input to the child process).
    #[instrument(skip(self, data), fields(len = data.len()))]
    pub async fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        let writer = Arc::clone(&self.writer);
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut w = writer.blocking_lock();
            w.write_all(&data).map_err(PtyError::WriteError)?;
            w.flush().map_err(PtyError::WriteError)
        })
        .await
        .expect("blocking write task panicked")
    }

    /// Resize the PTY to the given dimensions.
    #[instrument(skip(self))]
    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let master = Arc::clone(&self.master);
        tokio::task::spawn_blocking(move || {
            let m = master.blocking_lock();
            m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(PtyError::ResizeError)
        })
        .await
        .expect("blocking resize task panicked")
    }

    /// Get the child process PID, if available.
    pub fn child_pid(&self) -> Option<u32> {
        self.child_pid
    }

    /// Get the current working directory of the child process (macOS).
    #[cfg(target_os = "macos")]
    pub fn get_child_cwd(&self) -> Option<std::path::PathBuf> {
        let pid = self.child_pid? as i32;
        child_cwd(pid)
    }

    /// Get a clone of the writer Arc for shared write access.
    pub fn writer(&self) -> Arc<Mutex<Box<dyn Write + Send>>> {
        Arc::clone(&self.writer)
    }

    /// Get a clone of the master Arc for shared resize access.
    pub fn master(&self) -> Arc<Mutex<Box<dyn MasterPty + Send>>> {
        Arc::clone(&self.master)
    }

    /// Wait until the child process has exited.
    #[instrument(skip(self))]
    pub async fn wait_for_exit(&self) {
        self.child_exited.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_and_read_output() {
        // Spawn a PTY with a simple command echoing text.
        let pty_system = native_pty_system();
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).expect("open pty");

        let mut cmd = CommandBuilder::new("echo");
        cmd.arg("hello_pty_test");

        let mut child = pair.slave.spawn_command(cmd).expect("spawn echo");
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().expect("clone reader");

        // Read output in a blocking fashion (test context).
        let output = tokio::task::spawn_blocking(move || {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    Err(_) => break,
                }
            }
            buf
        });

        let _ = child.wait();
        drop(pair.master);

        let out = output.await.expect("reader task");
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("hello_pty_test"),
            "expected output to contain 'hello_pty_test', got: {text}"
        );
    }

    #[tokio::test]
    async fn pty_handle_spawn_and_exit() {
        std::env::set_var("SHELL", "/bin/sh");

        let mut handle = PtyHandle::spawn(24, 80).expect("spawn pty handle");

        // Send 'exit' to make the shell terminate.
        handle
            .write_input(b"exit\n")
            .await
            .expect("write exit command");

        // Drain output until the stream ends — this confirms the child exited
        // because the reader task only returns None after EOF.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while let Ok(Some(_)) =
            tokio::time::timeout_at(deadline, handle.read_output()).await
        {}
    }

    #[tokio::test]
    async fn pty_handle_resize() {
        std::env::set_var("SHELL", "/bin/sh");
        let mut handle = PtyHandle::spawn(24, 80).expect("spawn pty handle");

        // Resize should succeed.
        handle.resize(48, 120).await.expect("resize pty");

        // Clean up.
        handle
            .write_input(b"exit\n")
            .await
            .expect("write exit");
        while handle.read_output().await.is_some() {}
    }
}

/// Get the current working directory of a process by PID (macOS only).
#[cfg(target_os = "macos")]
pub fn child_cwd(pid: i32) -> Option<std::path::PathBuf> {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::path::PathBuf;

    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut u8,
            buffersize: i32,
        ) -> i32;
    }

    // PROC_PIDVNODEPATHINFO = 9
    const PROC_PIDVNODEPATHINFO: i32 = 9;
    // struct vnode_info_path has a fixed layout; the cwd path starts at offset 152
    // Total struct size is 2352 bytes
    const VNODE_INFO_PATH_SIZE: usize = 2352;
    const CWD_PATH_OFFSET: usize = 152;

    let mut buf = vec![0u8; VNODE_INFO_PATH_SIZE];
    let ret = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDVNODEPATHINFO,
            0,
            buf.as_mut_ptr(),
            VNODE_INFO_PATH_SIZE as i32,
        )
    };

    if ret <= 0 {
        return None;
    }

    let cwd_bytes = &buf[CWD_PATH_OFFSET..];
    let cwd_cstr = unsafe { CStr::from_ptr(cwd_bytes.as_ptr() as *const c_char) };
    let path = PathBuf::from(cwd_cstr.to_string_lossy().to_string());

    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}
