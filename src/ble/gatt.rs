use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Session status data transmitted over BLE GATT.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(dead_code)]
pub struct SessionStatusData {
    pub id: String,
    pub project_name: String,
    pub state: String,
    pub layer: String,
}

/// Commands that a mobile client can send over BLE.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BleCommand {
    /// Send a response to a session that is waiting for input.
    Respond {
        session_id: String,
        payload: String,
    },
    /// Switch the active session.
    SwitchSession { session_id: String },
    /// Pin a session to the foreground layer.
    PinSession { session_id: String },
}

/// Internal JSON representation used for parsing incoming commands.
#[derive(Deserialize)]
#[allow(dead_code)]
struct RawCommand {
    #[serde(rename = "type")]
    cmd_type: String,
    session_id: String,
    #[serde(default)]
    payload: Option<String>,
}

/// GATT service abstraction for Orchesterm BLE communication.
///
/// Provides serialization of session data for notifications and
/// parsing of incoming commands from mobile clients.
#[derive(Debug)]
#[allow(dead_code)]
pub struct GattService;

#[allow(dead_code)]
impl GattService {
    /// Create a new GATT service instance.
    pub fn new() -> Self {
        Self
    }

    /// Serialize a list of session statuses to JSON bytes for BLE notification.
    pub fn session_list_payload(sessions: &[SessionStatusData]) -> Vec<u8> {
        serde_json::to_vec(sessions).unwrap_or_default()
    }

    /// Parse an incoming BLE command from raw bytes (JSON).
    pub fn parse_command(data: &[u8]) -> Result<BleCommand> {
        let raw: RawCommand = serde_json::from_slice(data)?;

        match raw.cmd_type.as_str() {
            "respond" => {
                let payload = raw.payload.unwrap_or_default();
                Ok(BleCommand::Respond {
                    session_id: raw.session_id,
                    payload,
                })
            }
            "switch_session" => Ok(BleCommand::SwitchSession {
                session_id: raw.session_id,
            }),
            "pin_session" => Ok(BleCommand::PinSession {
                session_id: raw.session_id,
            }),
            other => bail!("unknown command type: {}", other),
        }
    }
}

impl Default for GattService {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate a BLE command for basic correctness (non-empty fields, etc.).
#[allow(dead_code)]
pub fn validate_command(cmd: &BleCommand) -> Result<()> {
    match cmd {
        BleCommand::Respond {
            session_id,
            payload,
        } => {
            if session_id.is_empty() {
                bail!("session_id must not be empty");
            }
            if payload.is_empty() {
                bail!("payload must not be empty for Respond command");
            }
            Ok(())
        }
        BleCommand::SwitchSession { session_id } => {
            if session_id.is_empty() {
                bail!("session_id must not be empty");
            }
            Ok(())
        }
        BleCommand::PinSession { session_id } => {
            if session_id.is_empty() {
                bail!("session_id must not be empty");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gatt_service_new() {
        let _service = GattService::new();
    }

    #[test]
    fn test_gatt_service_default() {
        let _service = GattService::default();
    }

    #[test]
    fn test_session_status_data_serialize_roundtrip() {
        let data = SessionStatusData {
            id: "test-id".to_string(),
            project_name: "proj".to_string(),
            state: "Running".to_string(),
            layer: "Background".to_string(),
        };
        let json = serde_json::to_vec(&data).unwrap();
        let restored: SessionStatusData = serde_json::from_slice(&json).unwrap();
        assert_eq!(data, restored);
    }

    #[test]
    fn test_parse_command_missing_session_id() {
        let json = r#"{"type":"switch_session"}"#;
        let result = GattService::parse_command(json.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_command_respond_without_payload() {
        // payload is optional at parse level; defaults to empty string
        let json = r#"{"type":"respond","session_id":"abc"}"#;
        let cmd = GattService::parse_command(json.as_bytes()).unwrap();
        match cmd {
            BleCommand::Respond {
                session_id,
                payload,
            } => {
                assert_eq!(session_id, "abc");
                assert!(payload.is_empty());
            }
            _ => panic!("expected Respond"),
        }
    }
}
