//! WebSocket server integration tests.
//!
//! Tests the full server lifecycle: start → connect → send/receive → disconnect.

use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use surfterm::ws::server::start_ws_server;
use surfterm::ws::{WsEvent, WsOutMessage, SessionStatusData};

#[tokio::test]
async fn ws_server_starts_and_accepts_connection() {
    let (event_tx, mut event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();
    assert!(port > 0);

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");

    // Server should emit ClientConnected
    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("should receive event")
        .expect("channel should not be closed");
    assert!(matches!(event, WsEvent::ClientConnected));

    drop(ws);

    // Server should emit ClientDisconnected
    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("should receive event")
        .expect("channel should not be closed");
    assert!(matches!(event, WsEvent::ClientDisconnected));
}

#[tokio::test]
async fn ws_server_broadcasts_sessions() {
    let (event_tx, _event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");
    let (_, mut read) = ws.split();

    // Small delay to ensure connection is fully established
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast a sessions message
    handle.broadcast(&WsOutMessage::Sessions {
        data: vec![SessionStatusData {
            id: "s1".to_string(),
            project_name: "test-proj".to_string(),
            state: "Running".to_string(),
            layer: "Foreground".to_string(),
        }],
    });

    // Client should receive the message
    let msg = tokio::time::timeout(Duration::from_secs(2), read.next())
        .await
        .expect("should receive message")
        .expect("stream should not end")
        .expect("message should be valid");

    let text = msg.into_text().expect("should be text");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("should be valid JSON");
    assert_eq!(parsed["type"], "sessions");
    assert_eq!(parsed["data"][0]["id"], "s1");
    assert_eq!(parsed["data"][0]["project_name"], "test-proj");
}

#[tokio::test]
async fn ws_server_receives_commands() {
    let (event_tx, mut event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");
    let (mut write, _) = ws.split();

    // Drain the ClientConnected event
    let _ = event_rx.recv().await;

    // Send a respond command
    let cmd = r#"{"type":"respond","session_id":"s1","payload":"hello"}"#;
    write
        .send(Message::Text(cmd.into()))
        .await
        .expect("should send");

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("should receive event")
        .expect("channel should not be closed");

    match event {
        WsEvent::CommandReceived(cmd) => {
            assert_eq!(
                cmd,
                surfterm::ws::WsCommand::Respond {
                    session_id: "s1".to_string(),
                    payload: "hello".to_string(),
                }
            );
        }
        other => panic!("expected CommandReceived, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_server_broadcasts_pty_output() {
    let (event_tx, _event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");
    let (_, mut read) = ws.split();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast PTY output
    let raw_bytes = b"hello world\r\n";
    let b64 = base64::engine::general_purpose::STANDARD.encode(raw_bytes);
    handle.broadcast(&WsOutMessage::PtyOutput {
        session_id: "s1".to_string(),
        data: b64.clone(),
    });

    let msg = tokio::time::timeout(Duration::from_secs(2), read.next())
        .await
        .expect("should receive message")
        .expect("stream should not end")
        .expect("message should be valid");

    let text = msg.into_text().expect("should be text");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("should be valid JSON");
    assert_eq!(parsed["type"], "pty_output");
    assert_eq!(parsed["session_id"], "s1");

    // Verify base64 round-trip
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(parsed["data"].as_str().unwrap())
        .expect("should decode base64");
    assert_eq!(decoded, raw_bytes);
}

#[tokio::test]
async fn ws_server_handles_pty_input_command() {
    let (event_tx, mut event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");
    let (mut write, _) = ws.split();

    // Drain the ClientConnected event
    let _ = event_rx.recv().await;

    // Send a pty_input command with base64 data
    let input_bytes = b"ls -la\n";
    let b64 = base64::engine::general_purpose::STANDARD.encode(input_bytes);
    let cmd = format!(
        r#"{{"type":"pty_input","session_id":"s1","data":"{}"}}"#,
        b64
    );
    write
        .send(Message::Text(cmd.into()))
        .await
        .expect("should send");

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("should receive event")
        .expect("channel should not be closed");

    match event {
        WsEvent::CommandReceived(cmd) => {
            assert_eq!(
                cmd,
                surfterm::ws::WsCommand::PtyInput {
                    session_id: "s1".to_string(),
                    data: input_bytes.to_vec(),
                }
            );
        }
        other => panic!("expected CommandReceived, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_server_handles_resize_command() {
    let (event_tx, mut event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();

    let url = format!("ws://127.0.0.1:{}", port);
    let (ws, _) = connect_async(&url).await.expect("client should connect");
    let (mut write, _) = ws.split();

    let _ = event_rx.recv().await;

    let cmd = r#"{"type":"resize","session_id":"s1","cols":120,"rows":40}"#;
    write
        .send(Message::Text(cmd.into()))
        .await
        .expect("should send");

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("should receive event")
        .expect("channel should not be closed");

    match event {
        WsEvent::CommandReceived(cmd) => {
            assert_eq!(
                cmd,
                surfterm::ws::WsCommand::Resize {
                    session_id: "s1".to_string(),
                    cols: 120,
                    rows: 40,
                }
            );
        }
        other => panic!("expected CommandReceived, got {:?}", other),
    }
}

#[tokio::test]
async fn ws_server_multiple_clients() {
    let (event_tx, mut event_rx) = mpsc::channel::<WsEvent>(64);
    let handle = start_ws_server(event_tx).await.expect("server should start");
    let port = handle.port();
    let url = format!("ws://127.0.0.1:{}", port);

    // Connect two clients
    let (ws1, _) = connect_async(&url).await.expect("client 1 should connect");
    let (ws2, _) = connect_async(&url).await.expect("client 2 should connect");

    // Drain connected events
    let _ = event_rx.recv().await;
    let _ = event_rx.recv().await;

    let (_, mut read1) = ws1.split();
    let (_, mut read2) = ws2.split();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast should reach both clients
    handle.broadcast(&WsOutMessage::SessionStateChanged {
        session_id: "s1".to_string(),
        state: "WaitingForInput".to_string(),
    });

    let msg1 = tokio::time::timeout(Duration::from_secs(2), read1.next())
        .await
        .expect("client 1 should receive")
        .expect("stream should not end")
        .expect("message should be valid");

    let msg2 = tokio::time::timeout(Duration::from_secs(2), read2.next())
        .await
        .expect("client 2 should receive")
        .expect("stream should not end")
        .expect("message should be valid");

    let text1 = msg1.into_text().unwrap();
    let text2 = msg2.into_text().unwrap();
    assert_eq!(text1, text2);
    assert!(text1.contains("session_state_changed"));
}
