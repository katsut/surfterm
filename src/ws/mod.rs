pub mod bonjour;
pub mod server;

use serde::{Deserialize, Serialize};

/// Session status data transmitted over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStatusData {
    pub id: String,
    pub project_name: String,
    pub state: String,
    pub layer: String,
}

/// Commands that a mobile client can send over WebSocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsCommand {
    /// Send a text response to a session waiting for input.
    Respond {
        session_id: String,
        payload: String,
    },
    /// Switch the active (foreground) session.
    SwitchSession { session_id: String },
    /// Pin a session to the foreground layer.
    PinSession { session_id: String },
    /// Raw PTY input (base64-encoded bytes).
    PtyInput {
        session_id: String,
        data: Vec<u8>,
    },
    /// Resize the terminal for a session.
    Resize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
}

/// Events sent from the WS server to the main application.
#[derive(Debug)]
pub enum WsEvent {
    /// A command was received from a mobile client.
    CommandReceived(WsCommand),
    /// A client connected.
    ClientConnected,
    /// A client disconnected.
    ClientDisconnected,
}

/// Messages sent from the app to all connected WebSocket clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WsOutMessage {
    /// Full session list update.
    #[serde(rename = "sessions")]
    Sessions { data: Vec<SessionStatusData> },
    /// Raw PTY output for a session (base64-encoded).
    #[serde(rename = "pty_output")]
    PtyOutput { session_id: String, data: String },
    /// Session state changed.
    #[serde(rename = "session_state_changed")]
    SessionStateChanged { session_id: String, state: String },
}

/// Internal JSON representation for parsing incoming WebSocket messages.
#[derive(Deserialize)]
struct RawInMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    payload: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

/// Parse an incoming WebSocket JSON message into a WsCommand.
pub fn parse_ws_message(text: &str) -> anyhow::Result<WsCommand> {
    let raw: RawInMessage = serde_json::from_str(text)?;
    let session_id = raw
        .session_id
        .ok_or_else(|| anyhow::anyhow!("missing session_id"))?;

    if session_id.is_empty() {
        anyhow::bail!("session_id must not be empty");
    }

    match raw.msg_type.as_str() {
        "respond" => {
            let payload = raw.payload.unwrap_or_default();
            Ok(WsCommand::Respond {
                session_id,
                payload,
            })
        }
        "switch_session" => Ok(WsCommand::SwitchSession { session_id }),
        "pin_session" => Ok(WsCommand::PinSession { session_id }),
        "pty_input" => {
            let b64 = raw
                .data
                .ok_or_else(|| anyhow::anyhow!("missing data for pty_input"))?;
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(&b64)?;
            Ok(WsCommand::PtyInput {
                session_id,
                data: bytes,
            })
        }
        "resize" => {
            let cols = raw
                .cols
                .ok_or_else(|| anyhow::anyhow!("missing cols for resize"))?;
            let rows = raw
                .rows
                .ok_or_else(|| anyhow::anyhow!("missing rows for resize"))?;
            Ok(WsCommand::Resize {
                session_id,
                cols,
                rows,
            })
        }
        other => anyhow::bail!("unknown message type: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_respond() {
        let json = r#"{"type":"respond","session_id":"abc","payload":"yes"}"#;
        let cmd = parse_ws_message(json).unwrap();
        assert_eq!(
            cmd,
            WsCommand::Respond {
                session_id: "abc".to_string(),
                payload: "yes".to_string()
            }
        );
    }

    #[test]
    fn test_parse_switch_session() {
        let json = r#"{"type":"switch_session","session_id":"xyz"}"#;
        let cmd = parse_ws_message(json).unwrap();
        assert_eq!(
            cmd,
            WsCommand::SwitchSession {
                session_id: "xyz".to_string()
            }
        );
    }

    #[test]
    fn test_parse_pty_input() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"ls\n");
        let json = format!(
            r#"{{"type":"pty_input","session_id":"s1","data":"{}"}}"#,
            b64
        );
        let cmd = parse_ws_message(&json).unwrap();
        assert_eq!(
            cmd,
            WsCommand::PtyInput {
                session_id: "s1".to_string(),
                data: b"ls\n".to_vec()
            }
        );
    }

    #[test]
    fn test_parse_resize() {
        let json = r#"{"type":"resize","session_id":"s1","cols":120,"rows":40}"#;
        let cmd = parse_ws_message(json).unwrap();
        assert_eq!(
            cmd,
            WsCommand::Resize {
                session_id: "s1".to_string(),
                cols: 120,
                rows: 40
            }
        );
    }

    #[test]
    fn test_parse_missing_session_id() {
        let json = r#"{"type":"switch_session"}"#;
        assert!(parse_ws_message(json).is_err());
    }

    #[test]
    fn test_parse_empty_session_id() {
        let json = r#"{"type":"switch_session","session_id":""}"#;
        assert!(parse_ws_message(json).is_err());
    }

    #[test]
    fn test_parse_unknown_type() {
        let json = r#"{"type":"foo","session_id":"bar"}"#;
        assert!(parse_ws_message(json).is_err());
    }

    #[test]
    fn test_parse_respond_without_payload() {
        let json = r#"{"type":"respond","session_id":"abc"}"#;
        let cmd = parse_ws_message(json).unwrap();
        match cmd {
            WsCommand::Respond {
                session_id,
                payload,
            } => {
                assert_eq!(session_id, "abc");
                assert!(payload.is_empty());
            }
            _ => panic!("expected Respond"),
        }
    }

    #[test]
    fn test_out_message_sessions_serialize() {
        let msg = WsOutMessage::Sessions {
            data: vec![SessionStatusData {
                id: "id1".to_string(),
                project_name: "proj".to_string(),
                state: "Running".to_string(),
                layer: "Foreground".to_string(),
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"sessions""#));
        assert!(json.contains(r#""id":"id1""#));
    }

    #[test]
    fn test_out_message_pty_output_serialize() {
        let msg = WsOutMessage::PtyOutput {
            session_id: "s1".to_string(),
            data: "aGVsbG8=".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"pty_output""#));
        assert!(json.contains(r#""data":"aGVsbG8=""#));
    }

    #[test]
    fn test_out_message_state_changed_serialize() {
        let msg = WsOutMessage::SessionStateChanged {
            session_id: "s1".to_string(),
            state: "WaitingForInput".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"session_state_changed""#));
    }
}
