pub mod gatt;

use anyhow::{bail, Result};
use tracing::instrument;

/// BLE Peripheral server (stub implementation).
///
/// This struct provides the BLE-ready architecture without depending on
/// actual BLE hardware or `btleplug`. The real transport will be added later.
#[derive(Debug)]
pub struct BleServer {
    enabled: bool,
    connected_devices: Vec<String>,
}

#[allow(dead_code)]
impl BleServer {
    /// Create a new BLE server. Pass `enabled: true` to indicate the BLE
    /// subsystem should be active.
    #[instrument]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            connected_devices: Vec::new(),
        }
    }

    /// Whether BLE is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Number of currently connected devices.
    pub fn connected_device_count(&self) -> usize {
        self.connected_devices.len()
    }

    /// Register a connected device.
    pub fn add_device(&mut self, device_id: String) {
        if !self.connected_devices.contains(&device_id) {
            self.connected_devices.push(device_id);
        }
    }

    /// Remove a device by ID.
    pub fn remove_device(&mut self, device_id: &str) {
        self.connected_devices.retain(|d| d != device_id);
    }
}

// ---------------------------------------------------------------------------
// Chunk protocol for BLE MTU-constrained transfers (STORY-027)
// ---------------------------------------------------------------------------

/// A single chunk in a chunked BLE transfer.
///
/// Wire format: `[seq_hi, seq_lo, total_hi, total_lo, ...payload]`
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Chunk {
    pub sequence: u16,
    pub total: u16,
    pub payload: Vec<u8>,
}

#[allow(dead_code)]
impl Chunk {
    /// Serialize to wire bytes: 4-byte header (sequence BE u16 + total BE u16) followed by payload.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.payload.len());
        buf.extend_from_slice(&self.sequence.to_be_bytes());
        buf.extend_from_slice(&self.total.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Parse a chunk from wire bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            bail!("chunk data too short: need at least 4 bytes, got {}", data.len());
        }
        let sequence = u16::from_be_bytes([data[0], data[1]]);
        let total = u16::from_be_bytes([data[2], data[3]]);
        let payload = data[4..].to_vec();
        Ok(Self {
            sequence,
            total,
            payload,
        })
    }
}

/// Splits large payloads into MTU-sized [`Chunk`]s and reassembles them.
#[derive(Debug)]
pub struct ChunkProtocol {
    mtu: usize,
}

#[allow(dead_code)]
impl ChunkProtocol {
    /// Create a new protocol with the given MTU (bytes).
    /// The usable payload per chunk is `mtu - 4` (4 bytes for the header).
    pub fn new(mtu: usize) -> Self {
        Self { mtu }
    }

    /// Maximum payload bytes per chunk after the 4-byte header.
    fn max_payload(&self) -> usize {
        self.mtu.saturating_sub(4)
    }

    /// Split `data` into a sequence of [`Chunk`]s.
    ///
    /// If the data is empty, a single chunk with an empty payload is returned
    /// so the receiver still gets a complete transfer.
    pub fn chunk_data(&self, data: &[u8]) -> Vec<Chunk> {
        let max = self.max_payload();
        if max == 0 {
            // Degenerate MTU — pack everything into one oversized chunk.
            return vec![Chunk {
                sequence: 0,
                total: 1,
                payload: data.to_vec(),
            }];
        }

        if data.is_empty() {
            return vec![Chunk {
                sequence: 0,
                total: 1,
                payload: Vec::new(),
            }];
        }

        let chunks_needed = data.len().div_ceil(max);
        let total = chunks_needed as u16;

        data.chunks(max)
            .enumerate()
            .map(|(i, slice)| Chunk {
                sequence: i as u16,
                total,
                payload: slice.to_vec(),
            })
            .collect()
    }

    /// Reassemble a set of [`Chunk`]s back into the original data.
    ///
    /// Chunks are sorted by `sequence` before concatenation.
    /// Returns an error if the chunks are inconsistent.
    pub fn reassemble(chunks: &[Chunk]) -> Result<Vec<u8>> {
        if chunks.is_empty() {
            bail!("no chunks to reassemble");
        }

        let expected_total = chunks[0].total;
        if chunks.len() != expected_total as usize {
            bail!(
                "expected {} chunks but got {}",
                expected_total,
                chunks.len()
            );
        }

        let mut sorted: Vec<&Chunk> = chunks.iter().collect();
        sorted.sort_by_key(|c| c.sequence);

        // Verify all chunks agree on total and sequences are contiguous.
        for (i, chunk) in sorted.iter().enumerate() {
            if chunk.total != expected_total {
                bail!(
                    "chunk {} has total={} but expected {}",
                    i,
                    chunk.total,
                    expected_total
                );
            }
            if chunk.sequence != i as u16 {
                bail!(
                    "expected sequence {} but got {}",
                    i,
                    chunk.sequence
                );
            }
        }

        let data: Vec<u8> = sorted.iter().flat_map(|c| c.payload.iter().copied()).collect();
        Ok(data)
    }
}

// ---------------------------------------------------------------------------
// Allowed devices whitelist (STORY-028)
// ---------------------------------------------------------------------------

/// Whitelist of device IDs allowed to send commands over BLE.
#[derive(Debug, Default)]
pub struct AllowedDevices {
    devices: Vec<String>,
}

#[allow(dead_code)]
impl AllowedDevices {
    /// Create a new empty whitelist.
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Add a device to the whitelist.
    pub fn allow(&mut self, device_id: &str) {
        if !self.devices.iter().any(|d| d == device_id) {
            self.devices.push(device_id.to_string());
        }
    }

    /// Check whether a device is on the whitelist.
    pub fn is_allowed(&self, device_id: &str) -> bool {
        self.devices.iter().any(|d| d == device_id)
    }

    /// Remove a device from the whitelist.
    pub fn revoke(&mut self, device_id: &str) {
        self.devices.retain(|d| d != device_id);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ble::gatt::{BleCommand, GattService, SessionStatusData};

    // -----------------------------------------------------------------------
    // BleServer tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ble_server_new_enabled() {
        let server = BleServer::new(true);
        assert!(server.is_enabled());
        assert_eq!(server.connected_device_count(), 0);
    }

    #[test]
    fn test_ble_server_new_disabled() {
        let server = BleServer::new(false);
        assert!(!server.is_enabled());
    }

    #[test]
    fn test_ble_server_add_device() {
        let mut server = BleServer::new(true);
        server.add_device("device-1".to_string());
        assert_eq!(server.connected_device_count(), 1);
        server.add_device("device-2".to_string());
        assert_eq!(server.connected_device_count(), 2);
    }

    #[test]
    fn test_ble_server_add_duplicate_device() {
        let mut server = BleServer::new(true);
        server.add_device("device-1".to_string());
        server.add_device("device-1".to_string());
        assert_eq!(server.connected_device_count(), 1);
    }

    #[test]
    fn test_ble_server_remove_device() {
        let mut server = BleServer::new(true);
        server.add_device("device-1".to_string());
        server.add_device("device-2".to_string());
        server.remove_device("device-1");
        assert_eq!(server.connected_device_count(), 1);
    }

    #[test]
    fn test_ble_server_remove_nonexistent_device() {
        let mut server = BleServer::new(true);
        server.add_device("device-1".to_string());
        server.remove_device("device-999");
        assert_eq!(server.connected_device_count(), 1);
    }

    // -----------------------------------------------------------------------
    // GattService tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gatt_service_session_list_payload() {
        let sessions = vec![
            SessionStatusData {
                id: "abc-123".to_string(),
                project_name: "my-project".to_string(),
                state: "Running".to_string(),
                layer: "Background".to_string(),
            },
            SessionStatusData {
                id: "def-456".to_string(),
                project_name: "other".to_string(),
                state: "Idle".to_string(),
                layer: "Foreground".to_string(),
            },
        ];

        let payload = GattService::session_list_payload(&sessions);
        let parsed: Vec<SessionStatusData> =
            serde_json::from_slice(&payload).expect("valid JSON");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "abc-123");
        assert_eq!(parsed[1].project_name, "other");
    }

    #[test]
    fn test_gatt_service_session_list_empty() {
        let payload = GattService::session_list_payload(&[]);
        let parsed: Vec<SessionStatusData> =
            serde_json::from_slice(&payload).expect("valid JSON");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_gatt_parse_command_respond() {
        let json = r#"{"type":"respond","session_id":"abc","payload":"yes"}"#;
        let cmd = GattService::parse_command(json.as_bytes()).unwrap();
        match cmd {
            BleCommand::Respond {
                session_id,
                payload,
            } => {
                assert_eq!(session_id, "abc");
                assert_eq!(payload, "yes");
            }
            _ => panic!("expected Respond"),
        }
    }

    #[test]
    fn test_gatt_parse_command_switch() {
        let json = r#"{"type":"switch_session","session_id":"xyz"}"#;
        let cmd = GattService::parse_command(json.as_bytes()).unwrap();
        match cmd {
            BleCommand::SwitchSession { session_id } => {
                assert_eq!(session_id, "xyz");
            }
            _ => panic!("expected SwitchSession"),
        }
    }

    #[test]
    fn test_gatt_parse_command_pin() {
        let json = r#"{"type":"pin_session","session_id":"s1"}"#;
        let cmd = GattService::parse_command(json.as_bytes()).unwrap();
        match cmd {
            BleCommand::PinSession { session_id } => {
                assert_eq!(session_id, "s1");
            }
            _ => panic!("expected PinSession"),
        }
    }

    #[test]
    fn test_gatt_parse_command_invalid_json() {
        let result = GattService::parse_command(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_gatt_parse_command_unknown_type() {
        let json = r#"{"type":"delete","session_id":"x"}"#;
        let result = GattService::parse_command(json.as_bytes());
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // BleCommand validation tests (STORY-028)
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_respond_ok() {
        let cmd = BleCommand::Respond {
            session_id: "abc".to_string(),
            payload: "hello".to_string(),
        };
        assert!(gatt::validate_command(&cmd).is_ok());
    }

    #[test]
    fn test_validate_respond_empty_session_id() {
        let cmd = BleCommand::Respond {
            session_id: String::new(),
            payload: "hello".to_string(),
        };
        assert!(gatt::validate_command(&cmd).is_err());
    }

    #[test]
    fn test_validate_respond_empty_payload() {
        let cmd = BleCommand::Respond {
            session_id: "abc".to_string(),
            payload: String::new(),
        };
        assert!(gatt::validate_command(&cmd).is_err());
    }

    #[test]
    fn test_validate_switch_ok() {
        let cmd = BleCommand::SwitchSession {
            session_id: "abc".to_string(),
        };
        assert!(gatt::validate_command(&cmd).is_ok());
    }

    #[test]
    fn test_validate_switch_empty_id() {
        let cmd = BleCommand::SwitchSession {
            session_id: String::new(),
        };
        assert!(gatt::validate_command(&cmd).is_err());
    }

    #[test]
    fn test_validate_pin_ok() {
        let cmd = BleCommand::PinSession {
            session_id: "s1".to_string(),
        };
        assert!(gatt::validate_command(&cmd).is_ok());
    }

    #[test]
    fn test_validate_pin_empty_id() {
        let cmd = BleCommand::PinSession {
            session_id: String::new(),
        };
        assert!(gatt::validate_command(&cmd).is_err());
    }

    // -----------------------------------------------------------------------
    // Chunk tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_chunk_to_bytes_from_bytes_roundtrip() {
        let chunk = Chunk {
            sequence: 3,
            total: 10,
            payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };
        let bytes = chunk.to_bytes();
        let restored = Chunk::from_bytes(&bytes).unwrap();
        assert_eq!(chunk, restored);
    }

    #[test]
    fn test_chunk_from_bytes_too_short() {
        let result = Chunk::from_bytes(&[0x00, 0x01]);
        assert!(result.is_err());
    }

    #[test]
    fn test_chunk_from_bytes_empty_payload() {
        let bytes = [0x00, 0x00, 0x00, 0x01]; // seq=0, total=1, no payload
        let chunk = Chunk::from_bytes(&bytes).unwrap();
        assert_eq!(chunk.sequence, 0);
        assert_eq!(chunk.total, 1);
        assert!(chunk.payload.is_empty());
    }

    // -----------------------------------------------------------------------
    // ChunkProtocol tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_chunk_small_data_single_chunk() {
        let proto = ChunkProtocol::new(512);
        let data = b"hello world";
        let chunks = proto.chunk_data(data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].sequence, 0);
        assert_eq!(chunks[0].total, 1);
        assert_eq!(chunks[0].payload, data);
    }

    #[test]
    fn test_chunk_large_data_multiple_chunks() {
        // MTU=10 => max_payload=6 bytes per chunk
        let proto = ChunkProtocol::new(10);
        let data = vec![0xAA; 20]; // 20 bytes => ceil(20/6) = 4 chunks
        let chunks = proto.chunk_data(&data);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].total, 4);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.sequence, i as u16);
            assert_eq!(chunk.total, 4);
        }
    }

    #[test]
    fn test_chunk_roundtrip() {
        let proto = ChunkProtocol::new(10);
        let data = b"The quick brown fox jumps over the lazy dog";
        let chunks = proto.chunk_data(data);
        let reassembled = ChunkProtocol::reassemble(&chunks).unwrap();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn test_chunk_empty_data() {
        let proto = ChunkProtocol::new(512);
        let chunks = proto.chunk_data(b"");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].payload.is_empty());
        let reassembled = ChunkProtocol::reassemble(&chunks).unwrap();
        assert!(reassembled.is_empty());
    }

    #[test]
    fn test_chunk_exact_mtu_boundary() {
        // MTU=12 => max_payload=8 bytes. Data exactly 8 bytes => 1 chunk.
        let proto = ChunkProtocol::new(12);
        let data = vec![0xFF; 8];
        let chunks = proto.chunk_data(&data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].payload.len(), 8);

        // Data exactly 16 bytes => 2 chunks.
        let data2 = vec![0xFF; 16];
        let chunks2 = proto.chunk_data(&data2);
        assert_eq!(chunks2.len(), 2);
        let reassembled = ChunkProtocol::reassemble(&chunks2).unwrap();
        assert_eq!(reassembled, data2);
    }

    #[test]
    fn test_reassemble_no_chunks_error() {
        let result = ChunkProtocol::reassemble(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reassemble_mismatched_total_error() {
        let chunks = vec![
            Chunk {
                sequence: 0,
                total: 3,
                payload: vec![1],
            },
            Chunk {
                sequence: 1,
                total: 3,
                payload: vec![2],
            },
        ];
        let result = ChunkProtocol::reassemble(&chunks);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // AllowedDevices tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_allowed_devices_new_empty() {
        let ad = AllowedDevices::new();
        assert!(!ad.is_allowed("any"));
    }

    #[test]
    fn test_allowed_devices_allow_and_check() {
        let mut ad = AllowedDevices::new();
        ad.allow("phone-1");
        assert!(ad.is_allowed("phone-1"));
        assert!(!ad.is_allowed("phone-2"));
    }

    #[test]
    fn test_allowed_devices_revoke() {
        let mut ad = AllowedDevices::new();
        ad.allow("phone-1");
        ad.revoke("phone-1");
        assert!(!ad.is_allowed("phone-1"));
    }

    #[test]
    fn test_allowed_devices_revoke_nonexistent() {
        let mut ad = AllowedDevices::new();
        ad.allow("phone-1");
        ad.revoke("phone-2"); // no-op
        assert!(ad.is_allowed("phone-1"));
    }

    #[test]
    fn test_allowed_devices_duplicate_allow() {
        let mut ad = AllowedDevices::new();
        ad.allow("phone-1");
        ad.allow("phone-1");
        // Should still have only one entry
        ad.revoke("phone-1");
        assert!(!ad.is_allowed("phone-1"));
    }
}
