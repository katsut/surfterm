//! BLE GATT Peripheral server using ble-peripheral-rust.
//!
//! Advertises a Surfterm service with two characteristics:
//! - Session List (read + notify): JSON array of session statuses
//! - Command (write): JSON commands from mobile client

use anyhow::Result;
use ble_peripheral_rust::{
    gatt::{
        characteristic::Characteristic,
        peripheral_event::{
            PeripheralEvent, ReadRequestResponse, RequestResponse, WriteRequestResponse,
        },
        properties::{AttributePermission, CharacteristicProperty},
        service::Service,
    },
    Peripheral, PeripheralImpl,
};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use super::gatt::{BleCommand, GattService, SessionStatusData};

// Surfterm BLE UUIDs (custom 128-bit)
const SERVICE_UUID: &str = "5572f001-7846-4d32-a1a4-5f7a4e3b6c10";
const SESSION_LIST_CHAR_UUID: &str = "5572f002-7846-4d32-a1a4-5f7a4e3b6c10";
const COMMAND_CHAR_UUID: &str = "5572f003-7846-4d32-a1a4-5f7a4e3b6c10";

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
pub struct BlePeripheralHandle {
    peripheral: Peripheral,
    session_list_uuid: Uuid,
}

impl BlePeripheralHandle {
    /// Update the session list and notify subscribed clients.
    pub async fn update_sessions(&mut self, sessions: &[SessionStatusData]) -> Result<()> {
        let payload = GattService::session_list_payload(sessions);
        self.peripheral
            .update_characteristic(self.session_list_uuid, payload)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update BLE characteristic: {e}"))?;
        Ok(())
    }
}

/// Start the BLE peripheral server.
///
/// Returns a handle for updating session data and a receiver for incoming commands.
pub async fn start_peripheral() -> Result<(BlePeripheralHandle, mpsc::Receiver<BleEvent>)> {
    let service_uuid = Uuid::parse_str(SERVICE_UUID)?;
    let session_list_uuid = Uuid::parse_str(SESSION_LIST_CHAR_UUID)?;
    let command_uuid = Uuid::parse_str(COMMAND_CHAR_UUID)?;

    let service = Service {
        uuid: service_uuid,
        primary: true,
        characteristics: vec![
            // Session list: readable + subscribable (notify)
            Characteristic {
                uuid: session_list_uuid,
                properties: vec![
                    CharacteristicProperty::Read,
                    CharacteristicProperty::Notify,
                ],
                permissions: vec![AttributePermission::Readable],
                value: Some(b"[]".to_vec()), // empty session list initially
                descriptors: vec![],
            },
            // Command: writable by mobile client
            Characteristic {
                uuid: command_uuid,
                properties: vec![CharacteristicProperty::Write],
                permissions: vec![AttributePermission::Writeable],
                value: None,
                descriptors: vec![],
            },
        ],
    };

    let (ble_tx, mut ble_rx) = mpsc::channel::<PeripheralEvent>(256);
    let (event_tx, event_rx) = mpsc::channel::<BleEvent>(64);

    let mut peripheral = Peripheral::new(ble_tx)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create BLE peripheral: {e}"))?;

    // Wait for Bluetooth to power on
    let mut attempts = 0;
    while !peripheral.is_powered().await.unwrap_or(false) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        attempts += 1;
        if attempts > 50 {
            anyhow::bail!("Bluetooth did not power on within 5 seconds");
        }
    }
    info!("Bluetooth powered on");

    peripheral
        .add_service(&service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to add BLE service: {e}"))?;
    info!("BLE service added");

    peripheral
        .start_advertising("Surfterm", &[service_uuid])
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start BLE advertising: {e}"))?;
    info!("BLE advertising started as 'Surfterm'");

    // Spawn event handler task
    tokio::spawn(async move {
        while let Some(event) = ble_rx.recv().await {
            match event {
                PeripheralEvent::ReadRequest {
                    request,
                    offset: _,
                    responder,
                } => {
                    info!(uuid = %request.characteristic, "BLE read request");
                    let _ = responder.send(ReadRequestResponse {
                        value: b"[]".to_vec(),
                        response: RequestResponse::Success,
                    });
                }
                PeripheralEvent::WriteRequest {
                    request,
                    offset: _,
                    value,
                    responder,
                } => {
                    info!(uuid = %request.characteristic, len = value.len(), "BLE write request");
                    let _ = responder.send(WriteRequestResponse {
                        response: RequestResponse::Success,
                    });

                    // Parse command
                    match GattService::parse_command(&value) {
                        Ok(cmd) => {
                            let _ = event_tx.send(BleEvent::CommandReceived(cmd)).await;
                        }
                        Err(e) => {
                            warn!("Invalid BLE command: {e}");
                        }
                    }
                }
                PeripheralEvent::CharacteristicSubscriptionUpdate {
                    request,
                    subscribed,
                } => {
                    info!(uuid = %request.characteristic, subscribed, "BLE subscription update");
                    if subscribed {
                        let _ = event_tx.send(BleEvent::ClientSubscribed).await;
                    } else {
                        let _ = event_tx.send(BleEvent::ClientUnsubscribed).await;
                    }
                }
                PeripheralEvent::StateUpdate { is_powered } => {
                    info!(is_powered, "BLE state update");
                }
            }
        }
    });

    let handle = BlePeripheralHandle {
        peripheral,
        session_list_uuid,
    };

    Ok((handle, event_rx))
}
