use std::net::SocketAddr;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;

use super::{parse_ws_message, WsEvent, WsOutMessage};

/// Handle to the running WebSocket server.
///
/// Provides methods to broadcast messages to all connected clients.
#[derive(Debug, Clone)]
pub struct WsServerHandle {
    broadcast_tx: broadcast::Sender<String>,
    port: u16,
}

impl WsServerHandle {
    /// Broadcast a message to all connected clients.
    pub fn broadcast(&self, msg: &WsOutMessage) {
        if let Ok(json) = serde_json::to_string(msg) {
            let _ = self.broadcast_tx.send(json);
        }
    }

    /// The TCP port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Start the WebSocket server on a random available port.
///
/// Returns a handle for broadcasting and a receiver for incoming client commands.
pub async fn start_ws_server(event_tx: mpsc::Sender<WsEvent>) -> Result<WsServerHandle> {
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();

    let (broadcast_tx, _) = broadcast::channel::<String>(256);
    let handle = WsServerHandle {
        broadcast_tx: broadcast_tx.clone(),
        port,
    };

    tracing::info!(port, "WebSocket server listening");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    tracing::info!(%addr, "WebSocket client connecting");
                    let event_tx = event_tx.clone();
                    let broadcast_rx = broadcast_tx.subscribe();
                    tokio::spawn(handle_connection(stream, addr, event_tx, broadcast_rx));
                }
                Err(e) => {
                    tracing::warn!("WebSocket accept error: {e}");
                }
            }
        }
    });

    Ok(handle)
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    event_tx: mpsc::Sender<WsEvent>,
    mut broadcast_rx: broadcast::Receiver<String>,
) {
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(%addr, "WebSocket handshake failed: {e}");
            return;
        }
    };

    tracing::info!(%addr, "WebSocket client connected");
    let _ = event_tx.send(WsEvent::ClientConnected).await;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Forward broadcast messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read messages from the client
    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                match parse_ws_message(&text) {
                    Ok(cmd) => {
                        let _ = event_tx.send(WsEvent::CommandReceived(cmd)).await;
                    }
                    Err(e) => {
                        tracing::warn!(%addr, "Invalid WebSocket message: {e}");
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::debug!(%addr, "WebSocket read error: {e}");
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
    let _ = event_tx.send(WsEvent::ClientDisconnected).await;
    tracing::info!(%addr, "WebSocket client disconnected");
}
