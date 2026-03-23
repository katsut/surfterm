//! BLE GATT Peripheral server via Swift helper process.
//!
//! Spawns `surfterm-ble-helper` which uses CoreBluetooth natively.
//! Communication is via stdin/stdout JSON lines.

use anyhow::Result;
use tokio::sync::mpsc;

use super::gatt::{BleCommand, GattService, SessionStatusData};

/// Messages sent from the BLE peripheral to the main app.
#[derive(Debug)]
pub enum BleEvent {
    /// A command was received from the mobile client.
    CommandReceived(BleCommand),
    /// A client subscribed to session list notifications.
    ClientSubscribed,
    /// A client disconnected.
    ClientUnsubscribed,
}

/// Handle to the running BLE peripheral, allowing session updates.
#[derive(Debug)]
pub struct BlePeripheralHandle {
    /// Sender to write JSON lines to the Swift helper's stdin.
    stdin_tx: mpsc::Sender<String>,
}

impl BlePeripheralHandle {
    /// Update the session list and notify subscribed clients.
    pub async fn update_sessions(&mut self, sessions: &[SessionStatusData]) -> Result<()> {
        let payload = serde_json::to_string(sessions)?;
        let msg = format!("{{\"type\":\"update_sessions\",\"data\":{}}}", payload);
        self.stdin_tx
            .send(msg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send to BLE helper: {e}"))?;
        Ok(())
    }
}

/// Start the BLE peripheral by spawning the Swift helper process.
///
/// Returns a handle for updating session data and a receiver for incoming commands.
pub async fn start_peripheral() -> Result<(BlePeripheralHandle, mpsc::Receiver<BleEvent>)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    // Look for the .app bundle next to the surfterm binary, then fallback to bare binary
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let helper_path = exe_dir
        .as_ref()
        .map(|d| d.join("SurftermBLE.app/Contents/MacOS/surfterm-ble-helper"))
        .filter(|p| p.exists())
        .or_else(|| {
            exe_dir
                .as_ref()
                .map(|d| d.join("surfterm-ble-helper"))
                .filter(|p| p.exists())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("surfterm-ble-helper"));

    let mut child = Command::new(&helper_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn BLE helper at {}: {e}", helper_path.display()))?;

    let child_stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("No stdin for BLE helper"))?;
    let child_stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("No stdout for BLE helper"))?;

    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(64);
    let (event_tx, event_rx) = mpsc::channel::<BleEvent>(64);

    // Write to helper stdin
    tokio::spawn(async move {
        let mut writer = child_stdin;
        while let Some(line) = stdin_rx.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if writer.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
    });

    // Read from helper stdout
    tokio::spawn(async move {
        let reader = BufReader::new(child_stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Parse JSON line from helper
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                match value.get("type").and_then(|t| t.as_str()) {
                    Some("command") => {
                        if let Some(data) = value.get("data") {
                            if let Ok(cmd) = GattService::parse_command(data.to_string().as_bytes()) {
                                let _ = event_tx.send(BleEvent::CommandReceived(cmd)).await;
                            }
                        }
                    }
                    Some("subscribed") => {
                        let _ = event_tx.send(BleEvent::ClientSubscribed).await;
                    }
                    Some("unsubscribed") => {
                        let _ = event_tx.send(BleEvent::ClientUnsubscribed).await;
                    }
                    _ => {}
                }
            }
        }
    });

    tracing::info!("BLE helper process started");

    let handle = BlePeripheralHandle { stdin_tx };
    Ok((handle, event_rx))
}
