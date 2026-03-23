use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_surfterm._tcp.local.";

/// Handle to the mDNS service advertisement.
///
/// Dropping this will unregister the service.
pub struct BonjourHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl BonjourHandle {
    /// Stop advertising and shut down the mDNS daemon.
    pub fn shutdown(self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Advertise the Surfterm WebSocket server via Bonjour/mDNS.
pub fn advertise(port: u16) -> Result<BonjourHandle> {
    let daemon = ServiceDaemon::new()?;

    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "surfterm-host".to_string());

    let instance_name = format!("Surfterm ({})", hostname);

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &format!("{}.", hostname),
        "",
        port,
        None,
    )?;

    let fullname = service.get_fullname().to_string();
    daemon.register(service)?;

    tracing::info!(port, %instance_name, "Bonjour: advertising WebSocket service");

    Ok(BonjourHandle { daemon, fullname })
}
