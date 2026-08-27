//! Game-neutral orchestration from captured frames to reconstructed TCP
//! streams.
//!
//! Game framing, routes, opcodes, profile schemas, and website projections
//! belong to trusted game integration plug-ins.

use std::collections::HashMap;

use rlogs_capture::CapturedFrame;
use rlogs_network::{
    DecodeIssue, GapReason, IpEndpoint, NetworkDecodeEvent, NetworkDecoder, TcpFlowKey,
    TcpReassembler, TcpStreamEvent,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const RESEARCH_CONNECTIONS_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameConnection {
    pub client: IpEndpoint,
    pub server: IpEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchConnectionFile {
    pub schema_version: u16,
    pub connections: Vec<GameConnection>,
}

impl ResearchConnectionFile {
    pub fn validate(self) -> Result<GameConnectionFilter, ConnectionFilterError> {
        if self.schema_version != RESEARCH_CONNECTIONS_SCHEMA_VERSION {
            return Err(ConnectionFilterError::UnsupportedSchemaVersion {
                actual: self.schema_version,
            });
        }
        GameConnectionFilter::try_new(self.connections)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportDirection {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionIdentity {
    pub connection_id: u64,
    pub stream_id: u64,
    pub direction: TransportDirection,
}

#[derive(Debug, Clone)]
pub struct GameConnectionFilter {
    flows: HashMap<TcpFlowKey, ConnectionIdentity>,
    connection_count: usize,
}

impl GameConnectionFilter {
    pub fn try_new(connections: Vec<GameConnection>) -> Result<Self, ConnectionFilterError> {
        if connections.is_empty() {
            return Err(ConnectionFilterError::Empty);
        }

        let mut filter = Self {
            flows: HashMap::with_capacity(connections.len().saturating_mul(2)),
            connection_count: 0,
        };
        for connection in connections {
            filter.try_add_connection(connection)?;
        }
        Ok(filter)
    }

    pub fn connection_count(&self) -> usize {
        self.connection_count
    }

    pub fn classify(&self, flow: TcpFlowKey) -> Option<ConnectionIdentity> {
        self.flows.get(&flow).copied()
    }

    /// Adds a newly observed process-owned socket without resetting existing
    /// TCP stream identities. Returns `false` when the exact connection was
    /// already present.
    pub fn try_add_connection(
        &mut self,
        connection: GameConnection,
    ) -> Result<bool, ConnectionFilterError> {
        if connection.client == connection.server {
            return Err(ConnectionFilterError::SameEndpoint(connection.client));
        }
        let client_to_server = TcpFlowKey::new(connection.client, connection.server);
        let server_to_client = client_to_server.reverse();
        match (
            self.flows.get(&client_to_server),
            self.flows.get(&server_to_client),
        ) {
            (Some(client), Some(server))
                if client.direction == TransportDirection::ClientToServer
                    && server.direction == TransportDirection::ServerToClient =>
            {
                return Ok(false);
            }
            (None, None) => {}
            _ => return Err(ConnectionFilterError::DuplicateFlow(client_to_server)),
        }
        let connection_id = u64::try_from(self.connection_count)
            .unwrap_or(u64::MAX)
            .checked_add(1)
            .ok_or(ConnectionFilterError::ConnectionIdExhausted)?;
        insert_flow(
            &mut self.flows,
            client_to_server,
            ConnectionIdentity {
                connection_id,
                stream_id: connection_id.saturating_mul(2).saturating_sub(1),
                direction: TransportDirection::ClientToServer,
            },
        )?;
        insert_flow(
            &mut self.flows,
            server_to_client,
            ConnectionIdentity {
                connection_id,
                stream_id: connection_id.saturating_mul(2),
                direction: TransportDirection::ServerToClient,
            },
        )?;
        self.connection_count = self
            .connection_count
            .checked_add(1)
            .ok_or(ConnectionFilterError::ConnectionIdExhausted)?;
        Ok(true)
    }
}

fn insert_flow(
    flows: &mut HashMap<TcpFlowKey, ConnectionIdentity>,
    flow: TcpFlowKey,
    identity: ConnectionIdentity,
) -> Result<(), ConnectionFilterError> {
    if flows.insert(flow, identity).is_some() {
        return Err(ConnectionFilterError::DuplicateFlow(flow));
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectionFilterError {
    #[error("unsupported research connection schema version {actual}")]
    UnsupportedSchemaVersion { actual: u16 },

    #[error("at least one exact game connection is required")]
    Empty,

    #[error("client and server endpoints are identical: {0:?}")]
    SameEndpoint(IpEndpoint),

    #[error("connection list defines TCP flow more than once: {0:?}")]
    DuplicateFlow(TcpFlowKey),

    #[error("game connection identity space is exhausted")]
    ConnectionIdExhausted,
}

#[derive(Debug, Clone)]
pub struct TransportStreamOutput {
    pub identity: ConnectionIdentity,
    pub event: TcpStreamEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportGapKind {
    IpFragmentDropped,
    NetworkDecodeIssue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportGap {
    pub observed_micros: u64,
    pub kind: TransportGapKind,
    pub lost_bytes: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum TransportOutput {
    Stream(TransportStreamOutput),
    Gap(TransportGap),
}

#[derive(Debug)]
pub struct TransportPipeline {
    connections: GameConnectionFilter,
    network: NetworkDecoder,
    tcp: TcpReassembler,
    last_observed_micros: u64,
}

impl TransportPipeline {
    pub fn new(connections: GameConnectionFilter) -> Self {
        Self {
            connections,
            network: NetworkDecoder::new(),
            tcp: TcpReassembler::new(),
            last_observed_micros: 0,
        }
    }

    pub fn connection_filter(&self) -> &GameConnectionFilter {
        &self.connections
    }

    pub fn try_add_connection(
        &mut self,
        connection: GameConnection,
    ) -> Result<bool, ConnectionFilterError> {
        self.connections.try_add_connection(connection)
    }

    pub fn process_frame(&mut self, frame: &CapturedFrame, mut emit: impl FnMut(TransportOutput)) {
        self.last_observed_micros = frame.observed_micros;
        let connections = &self.connections;
        let tcp = &mut self.tcp;

        self.network.process_frame(frame, |event| match event {
            NetworkDecodeEvent::Tcp(segment) => {
                let Some(identity) = connections.classify(segment.flow) else {
                    return;
                };
                tcp.process(segment, |event| {
                    emit(TransportOutput::Stream(TransportStreamOutput {
                        identity,
                        event,
                    }));
                });
            }
            NetworkDecodeEvent::FragmentDropped(drop) => {
                emit(TransportOutput::Gap(TransportGap {
                    observed_micros: drop.observed_micros,
                    kind: TransportGapKind::IpFragmentDropped,
                    lost_bytes: Some(drop.discarded_bytes),
                    detail: format!("IP fragment reassembly dropped: {:?}", drop.reason),
                }));
            }
            NetworkDecodeEvent::Ignored(issue)
                if matches!(
                    issue,
                    DecodeIssue::UnsupportedLinkType
                        | DecodeIssue::MalformedPacket
                        | DecodeIssue::IpVersionMismatch
                ) =>
            {
                emit(TransportOutput::Gap(TransportGap {
                    observed_micros: frame.observed_micros,
                    kind: TransportGapKind::NetworkDecodeIssue,
                    lost_bytes: Some(frame.bytes.len() as u64),
                    detail: format!("network decode issue: {issue:?}"),
                }));
            }
            NetworkDecodeEvent::Ignored(_) => {}
        });
    }

    pub fn finish(&mut self, mut emit: impl FnMut(TransportOutput)) {
        let network_expiry = self.last_observed_micros.saturating_add(61 * 1_000_000);
        let stream_expiry = self.last_observed_micros.saturating_add(121 * 1_000_000);
        let observed_floor = self.last_observed_micros;

        self.network.expire(network_expiry, |event| {
            if let NetworkDecodeEvent::FragmentDropped(drop) = event {
                emit(TransportOutput::Gap(TransportGap {
                    observed_micros: drop.observed_micros.max(observed_floor),
                    kind: TransportGapKind::IpFragmentDropped,
                    lost_bytes: Some(drop.discarded_bytes),
                    detail: format!("IP fragment reassembly dropped: {:?}", drop.reason),
                }));
            }
        });

        let connections = &self.connections;
        self.tcp.expire_idle(stream_expiry, |event| {
            // Advancing the idle clock at `finish` releases state. An idle gap
            // here represents the capture boundary, not observed packet loss.
            if matches!(
                &event,
                TcpStreamEvent::Gap(gap) if gap.reason == GapReason::IdleTimeout
            ) {
                return;
            }
            let flow = match &event {
                TcpStreamEvent::Chunk(chunk) => chunk.flow,
                TcpStreamEvent::Gap(gap) => gap.flow,
            };
            let Some(identity) = connections.classify(flow) else {
                return;
            };
            emit(TransportOutput::Stream(TransportStreamOutput {
                identity,
                event,
            }));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, TimestampNormalization};

    use super::*;

    fn endpoint4(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    #[test]
    fn exact_connection_filter_assigns_stable_bidirectional_streams() {
        let client = endpoint4(1, 31_000);
        let server = endpoint4(2, 32_000);
        let filter =
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap();

        let outgoing = filter.classify(TcpFlowKey::new(client, server)).unwrap();
        let incoming = filter.classify(TcpFlowKey::new(server, client)).unwrap();
        assert_eq!(outgoing.connection_id, incoming.connection_id);
        assert_eq!(outgoing.stream_id, 1);
        assert_eq!(incoming.stream_id, 2);
        assert_eq!(outgoing.direction, TransportDirection::ClientToServer);
        assert_eq!(incoming.direction, TransportDirection::ServerToClient);
    }

    #[test]
    fn live_connection_additions_preserve_existing_stream_identities() {
        let first = GameConnection {
            client: endpoint4(1, 31_000),
            server: endpoint4(2, 32_000),
        };
        let second = GameConnection {
            client: endpoint4(1, 31_001),
            server: endpoint4(3, 32_001),
        };
        let mut filter = GameConnectionFilter::try_new(vec![first]).unwrap();
        let original = filter
            .classify(TcpFlowKey::new(first.client, first.server))
            .unwrap();

        assert!(filter.try_add_connection(second).unwrap());
        assert!(!filter.try_add_connection(second).unwrap());
        assert_eq!(
            filter
                .classify(TcpFlowKey::new(first.client, first.server))
                .unwrap(),
            original
        );
        let added = filter
            .classify(TcpFlowKey::new(second.client, second.server))
            .unwrap();
        assert_eq!(added.connection_id, 2);
        assert_eq!(added.stream_id, 3);
        assert_eq!(filter.connection_count(), 2);
    }

    #[test]
    fn allowed_tcp_payload_reaches_the_game_plugin_boundary() {
        let client = endpoint4(1, 31_000);
        let server = endpoint4(2, 32_000);
        let mut pipeline = TransportPipeline::new(
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap(),
        );
        let payload = [7_u8, 8, 9];
        let tcp = TcpHeader::new(server.port, client.port, 100, 16_384);
        let builder = PacketBuilder::ipv4(
            match server.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            match client.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            64,
        )
        .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, &payload).unwrap();
        let frame = CapturedFrame {
            sequence: 1,
            observed_micros: 0,
            source_timestamp_nanos: Some(1_000_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        };

        let mut outputs = Vec::new();
        pipeline.process_frame(&frame, |output| outputs.push(output));
        pipeline.finish(|output| outputs.push(output));

        assert_eq!(outputs.len(), 1);
        let TransportOutput::Stream(stream) = &outputs[0] else {
            panic!("expected stream output");
        };
        assert_eq!(
            stream.identity.direction,
            TransportDirection::ServerToClient
        );
        let TcpStreamEvent::Chunk(chunk) = &stream.event else {
            panic!("expected stream chunk");
        };
        assert_eq!(chunk.bytes.as_ref(), payload.as_slice());
    }
}
