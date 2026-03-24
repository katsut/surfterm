use std::process::{Child, Command};

use anyhow::Result;

/// Handle to the mDNS service advertisement via native `dns-sd` command.
///
/// Dropping this will kill the dns-sd process and stop advertising.
pub struct BonjourHandle {
    child: Child,
}

impl BonjourHandle {
    /// Stop advertising and shut down.
    pub fn shutdown(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for BonjourHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Advertise the Surfterm WebSocket server via Bonjour/mDNS.
///
/// Uses the native macOS `dns-sd` command for reliable mDNS registration
/// through the system's mDNSResponder. Kills any stale dns-sd processes
/// from previous Surfterm runs before starting.
pub fn advertise(port: u16) -> Result<BonjourHandle> {
    // Kill any leftover dns-sd processes from previous runs
    let _ = Command::new("pkill")
        .args(["-f", "dns-sd.*_surfterm._tcp"])
        .output();
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "surfterm-host".to_string());

    let instance_name = format!("Surfterm ({})", hostname);

    let child = Command::new("dns-sd")
        .args([
            "-R",
            &instance_name,
            "_surfterm._tcp",
            "local.",
            &port.to_string(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn dns-sd: {e}"))?;

    tracing::info!(port, %instance_name, "Bonjour: advertising via dns-sd");

    Ok(BonjourHandle { child })
}
