use serde::{Deserialize, Serialize};

use crate::RoutedMessage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureAdapter {
    /// Human-readable implementation name such as `npcap` or `offline-pcap`.
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkEndpoint {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionState {
    NotCompressed,
    ZstdDecoded,
    ZstdFailed,
    Unknown,
}

/// Exact fragment evidence plus the application payload, when framing and
/// decompression succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketPayload {
    /// Original BPSR fragment bytes before header stripping or decompression.
    pub wire_bytes: Vec<u8>,
    /// Header-stripped and decompressed message bytes used by a schema decoder.
    pub application_bytes: Option<Vec<u8>>,
}

impl PacketPayload {
    pub fn decode_input(&self) -> Option<&[u8]> {
        self.application_bytes.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketEnvelope {
    pub connection_id: u64,
    pub stream_id: u64,
    pub source: Option<NetworkEndpoint>,
    pub destination: Option<NetworkEndpoint>,
    /// Transport direction is retained even when no route header exists.
    #[serde(default)]
    pub direction: crate::PacketDirection,
    /// Framing identity is retained independently from optional RPC routing.
    #[serde(default)]
    pub fragment: Option<crate::FragmentKind>,
    pub route: Option<RoutedMessage>,
    pub compression: CompressionState,
    pub payload: PacketPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureGapKind {
    AdapterDrop,
    QueueDrop,
    TcpGap,
    MalformedFrame,
    DecompressionFailure,
    UnsupportedFragment,
    UnsupportedTransport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureGap {
    pub kind: CaptureGapKind,
    pub connection_id: Option<u64>,
    pub stream_id: Option<u64>,
    pub lost_bytes: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record", content = "data", rename_all = "snake_case")]
pub enum CaptureRecordKind {
    Packet(PacketEnvelope),
    Gap(CaptureGap),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRecordDraft {
    pub observed_micros: u64,
    pub wall_clock_unix_micros: Option<i64>,
    pub kind: CaptureRecordKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRecord {
    /// Stable within one capture session, beginning at one.
    pub sequence: u64,
    pub observed_micros: u64,
    pub wall_clock_unix_micros: Option<i64>,
    pub kind: CaptureRecordKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_metadata_is_additive_for_legacy_capture_lines() {
        let json = r#"{
            "connection_id": 1,
            "stream_id": 2,
            "source": null,
            "destination": null,
            "route": null,
            "compression": "not_compressed",
            "payload": {
                "wire_bytes": [0, 0, 0, 6, 0, 8],
                "application_bytes": []
            }
        }"#;

        let packet: PacketEnvelope = serde_json::from_str(json).unwrap();

        assert_eq!(packet.direction, crate::PacketDirection::Unknown);
        assert_eq!(packet.fragment, None);
    }
}
