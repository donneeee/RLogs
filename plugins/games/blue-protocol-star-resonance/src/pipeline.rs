use std::collections::HashMap;

use rlogs_capture::CapturedFrame;
use rlogs_core::{
    ConnectionFilterError, GameConnection, GameConnectionFilter, TransportDirection, TransportGap,
    TransportOutput, TransportPipeline,
};
use rlogs_network::IpEndpoint;

use crate::{
    BpsrFramerSet, BpsrFramerSetConfig, BpsrFramerSetConfigError, BpsrFramingEvent,
    BpsrFramingIssueReason, CaptureGap, CaptureGapKind, CaptureRecordDraft, CaptureRecordKind,
    FragmentKind, NetworkEndpoint, PacketDirection, PacketEnvelope, PacketPayload, RouteKey,
    RoutedMessage,
};

const MAX_PENDING_RETURN_CORRELATIONS: usize = 4_096;
const RETURN_CORRELATION_TTL_MICROS: u64 = 120 * 1_000_000;

#[derive(Debug, Clone, Copy)]
struct PendingReturnRoute {
    service_id: u64,
    method_id: u32,
    stub_id: u32,
    observed_micros: u64,
}

/// BPSR's framing layer over Core's game-neutral reconstructed TCP streams.
#[derive(Debug)]
pub struct ResearchPipeline {
    connections: GameConnectionFilter,
    transport: TransportPipeline,
    framing: BpsrFramerSet,
    pending_returns: HashMap<(u64, u32), PendingReturnRoute>,
    capture_start_unix_micros: Option<i64>,
    last_observed_micros: u64,
}

impl ResearchPipeline {
    pub fn new(connections: GameConnectionFilter) -> Self {
        Self::try_with_framing_config(connections, BpsrFramerSetConfig::default())
            .expect("the built-in BPSR framing configuration is valid")
    }

    /// Constructs a research pipeline with an explicit framing variant.
    /// Production callers should use the exact-build protocol-pack choice;
    /// offline acquisition tools may opt into a candidate variant while
    /// retaining its non-authoritative provenance.
    pub fn try_with_framing_config(
        connections: GameConnectionFilter,
        framing: BpsrFramerSetConfig,
    ) -> Result<Self, BpsrFramerSetConfigError> {
        Ok(Self {
            transport: TransportPipeline::new(connections.clone()),
            connections,
            framing: BpsrFramerSet::try_with_config(framing)?,
            pending_returns: HashMap::new(),
            capture_start_unix_micros: None,
            last_observed_micros: 0,
        })
    }

    pub fn process_frame(
        &mut self,
        frame: &CapturedFrame,
        mut emit: impl FnMut(CaptureRecordDraft),
    ) {
        self.remember_clock(frame);
        self.last_observed_micros = frame.observed_micros;

        let connections = &self.connections;
        let framing = &mut self.framing;
        let pending_returns = &mut self.pending_returns;
        let capture_start = self.capture_start_unix_micros;
        self.transport.process_frame(frame, |output| {
            handle_transport_output(
                output,
                framing,
                connections,
                capture_start,
                frame.observed_micros,
                pending_returns,
                &mut emit,
            );
        });
    }

    /// Extends a live process-owned pipeline when the game opens another TCP
    /// socket. Existing stream and framing state remains intact.
    pub fn try_add_connection(
        &mut self,
        connection: GameConnection,
    ) -> Result<bool, ConnectionFilterError> {
        let added = self.connections.try_add_connection(connection)?;
        if added {
            let transport_added = self.transport.try_add_connection(connection)?;
            debug_assert!(transport_added);
        }
        Ok(added)
    }

    pub fn finish(&mut self, mut emit: impl FnMut(CaptureRecordDraft)) {
        let stream_expiry = self.last_observed_micros.saturating_add(121 * 1_000_000);
        let observed_floor = self.last_observed_micros;
        let capture_start = self.capture_start_unix_micros;
        let connections = &self.connections;
        let framing = &mut self.framing;
        let pending_returns = &mut self.pending_returns;

        self.transport.finish(|output| {
            handle_transport_output(
                output,
                framing,
                connections,
                capture_start,
                observed_floor,
                pending_returns,
                &mut emit,
            );
        });
        self.framing.expire(stream_expiry, |event| {
            if matches!(
                &event,
                BpsrFramingEvent::Issue(issue)
                    if issue.reason == BpsrFramingIssueReason::IdleTimeout
            ) {
                return;
            }
            emit(framing_record(
                event,
                connections,
                capture_start,
                observed_floor,
            ));
        });
    }

    fn remember_clock(&mut self, frame: &CapturedFrame) {
        if self.capture_start_unix_micros.is_some() {
            return;
        }
        self.capture_start_unix_micros = frame
            .source_timestamp_nanos
            .and_then(|value| value.checked_div(1_000))
            .and_then(|value| value.checked_sub(i64::try_from(frame.observed_micros).ok()?));
    }
}

fn handle_transport_output(
    output: TransportOutput,
    framing: &mut BpsrFramerSet,
    connections: &GameConnectionFilter,
    capture_start: Option<i64>,
    observed_floor: u64,
    pending_returns: &mut HashMap<(u64, u32), PendingReturnRoute>,
    emit: &mut impl FnMut(CaptureRecordDraft),
) {
    match output {
        TransportOutput::Stream(stream) => {
            let direction = match stream.identity.direction {
                TransportDirection::ClientToServer => PacketDirection::ClientToServer,
                TransportDirection::ServerToClient => PacketDirection::ServerToClient,
            };
            framing.process(direction, stream.event, |event| {
                let event = correlate_return_route(event, connections, pending_returns);
                emit(framing_record(
                    event,
                    connections,
                    capture_start,
                    observed_floor,
                ));
            });
        }
        TransportOutput::Gap(gap) => {
            emit(transport_gap_record(gap, capture_start, observed_floor));
        }
    }
}

fn correlate_return_route(
    mut event: BpsrFramingEvent,
    connections: &GameConnectionFilter,
    pending_returns: &mut HashMap<(u64, u32), PendingReturnRoute>,
) -> BpsrFramingEvent {
    let BpsrFramingEvent::Frame(frame) = &mut event else {
        return event;
    };
    let Some(identity) = connections.classify(frame.flow) else {
        return event;
    };

    pending_returns.retain(|_, pending| {
        frame
            .observed_micros
            .saturating_sub(pending.observed_micros)
            < RETURN_CORRELATION_TTL_MICROS
    });

    if frame.direction == PacketDirection::ClientToServer && frame.fragment == FragmentKind::Call {
        if let Some(route) = frame.route {
            if let Some(call_id) = route.call_id {
                if pending_returns.len() >= MAX_PENDING_RETURN_CORRELATIONS {
                    if let Some(oldest) = pending_returns
                        .iter()
                        .min_by_key(|(_, pending)| pending.observed_micros)
                        .map(|(key, _)| *key)
                    {
                        pending_returns.remove(&oldest);
                    }
                }
                pending_returns.insert(
                    (identity.connection_id, call_id),
                    PendingReturnRoute {
                        service_id: route.key.service_id,
                        method_id: route.key.method_id,
                        stub_id: route.stub_id,
                        observed_micros: frame.observed_micros,
                    },
                );
            }
        }
    } else if frame.direction == PacketDirection::ServerToClient
        && frame.fragment == FragmentKind::Return
    {
        if let Some(call_id) = frame.return_call_id {
            if let Some(pending) = pending_returns.remove(&(identity.connection_id, call_id)) {
                frame.route = Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Return,
                        pending.service_id,
                        pending.method_id,
                    ),
                    stub_id: pending.stub_id,
                    call_id: Some(call_id),
                });
            }
        }
    }
    event
}

fn transport_gap_record(
    gap: TransportGap,
    capture_start: Option<i64>,
    observed_floor: u64,
) -> CaptureRecordDraft {
    let observed_micros = gap.observed_micros.max(observed_floor);
    CaptureRecordDraft {
        observed_micros,
        wall_clock_unix_micros: wall_clock(capture_start, observed_micros),
        kind: CaptureRecordKind::Gap(CaptureGap {
            kind: CaptureGapKind::UnsupportedTransport,
            connection_id: None,
            stream_id: None,
            lost_bytes: gap.lost_bytes,
            detail: gap.detail,
        }),
    }
}

fn framing_record(
    event: BpsrFramingEvent,
    connections: &GameConnectionFilter,
    capture_start: Option<i64>,
    observed_floor: u64,
) -> CaptureRecordDraft {
    match event {
        BpsrFramingEvent::Frame(frame) => {
            let identity = connections.classify(frame.flow);
            let observed_micros = frame.observed_micros.max(observed_floor);
            CaptureRecordDraft {
                observed_micros,
                wall_clock_unix_micros: wall_clock(capture_start, observed_micros),
                kind: CaptureRecordKind::Packet(PacketEnvelope {
                    connection_id: identity.map_or(0, |value| value.connection_id),
                    stream_id: identity.map_or(0, |value| value.stream_id),
                    source: Some(endpoint(frame.flow.source)),
                    destination: Some(endpoint(frame.flow.destination)),
                    direction: frame.direction,
                    fragment: Some(frame.fragment),
                    route: frame.route,
                    compression: frame.compression,
                    payload: PacketPayload {
                        wire_bytes: frame.wire_bytes.to_vec(),
                        application_bytes: frame.application_bytes.map(|bytes| bytes.to_vec()),
                    },
                }),
            }
        }
        BpsrFramingEvent::Issue(issue) => {
            let identity = issue.flow.and_then(|flow| connections.classify(flow));
            let observed_micros = issue.observed_micros.max(observed_floor);
            let kind = match issue.reason {
                BpsrFramingIssueReason::TcpGap => CaptureGapKind::TcpGap,
                BpsrFramingIssueReason::DecompressionFailed
                | BpsrFramingIssueReason::DecompressedLimit => CaptureGapKind::DecompressionFailure,
                BpsrFramingIssueReason::MalformedNestedFrames
                | BpsrFramingIssueReason::NestingLimit => CaptureGapKind::UnsupportedFragment,
                _ => CaptureGapKind::MalformedFrame,
            };
            CaptureRecordDraft {
                observed_micros,
                wall_clock_unix_micros: wall_clock(capture_start, observed_micros),
                kind: CaptureRecordKind::Gap(CaptureGap {
                    kind,
                    connection_id: identity.map(|value| value.connection_id),
                    stream_id: identity.map(|value| value.stream_id),
                    lost_bytes: Some(issue.discarded_bytes),
                    detail: format!("BPSR framing issue: {:?}", issue.reason),
                }),
            }
        }
    }
}

fn endpoint(endpoint: IpEndpoint) -> NetworkEndpoint {
    NetworkEndpoint {
        address: endpoint.address.to_string(),
        port: endpoint.port,
    }
}

fn wall_clock(capture_start: Option<i64>, observed_micros: u64) -> Option<i64> {
    capture_start?.checked_add(i64::try_from(observed_micros).ok()?)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, TimestampNormalization};
    use rlogs_core::GameConnection;

    use crate::{CaptureRecordKind, FragmentKind};

    use super::*;

    fn endpoint4(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    #[test]
    fn reconstructed_tcp_bytes_reach_the_bpsr_framer() {
        let client = endpoint4(1, 31_000);
        let server = endpoint4(2, 32_000);
        let mut pipeline = ResearchPipeline::new(
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap(),
        );
        let body = [7_u8, 8, 9];
        let length = 6 + 16 + body.len();
        let mut bpsr = Vec::with_capacity(length);
        bpsr.extend_from_slice(&(length as u32).to_be_bytes());
        bpsr.extend_from_slice(&2_u16.to_be_bytes());
        bpsr.extend_from_slice(&1_664_308_034_u64.to_be_bytes());
        bpsr.extend_from_slice(&1_u32.to_be_bytes());
        bpsr.extend_from_slice(&21_u32.to_be_bytes());
        bpsr.extend_from_slice(&body);

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
        let mut packet = Vec::with_capacity(builder.size(bpsr.len()));
        builder.write(&mut packet, &bpsr).unwrap();
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

        let mut records = Vec::new();
        pipeline.process_frame(&frame, |record| records.push(record));
        pipeline.finish(|record| records.push(record));

        assert_eq!(records.len(), 1);
        let CaptureRecordKind::Packet(packet) = &records[0].kind else {
            panic!("expected packet");
        };
        assert_eq!(packet.direction, PacketDirection::ServerToClient);
        assert_eq!(packet.fragment, Some(FragmentKind::Notify));
        assert_eq!(packet.route.unwrap().key.method_id, 21);
        assert_eq!(
            packet.payload.application_bytes.as_deref(),
            Some(body.as_slice())
        );
    }

    #[test]
    fn client_call_identity_routes_the_matching_server_return() {
        let client = endpoint4(1, 31_000);
        let server = endpoint4(2, 32_000);
        let mut pipeline = ResearchPipeline::new(
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap(),
        );
        let call_id = 77_u32;
        let service_id = 904_190_988_u64;
        let method_id = 4_u32;

        let mut call_payload = Vec::new();
        call_payload.extend_from_slice(&service_id.to_be_bytes());
        call_payload.extend_from_slice(&1_u32.to_be_bytes());
        call_payload.extend_from_slice(&call_id.to_be_bytes());
        call_payload.extend_from_slice(&method_id.to_be_bytes());
        call_payload.push(1);
        let call = captured_bpsr_frame(1, client, server, 100, 1, &call_payload);

        let mut return_payload = Vec::new();
        return_payload.extend_from_slice(&1_u32.to_be_bytes());
        return_payload.extend_from_slice(&call_id.to_be_bytes());
        return_payload.extend_from_slice(&0_u32.to_be_bytes());
        return_payload.extend_from_slice(&[10, 3, 102, 111, 111]);
        let returned = captured_bpsr_frame(2, server, client, 200, 3, &return_payload);

        let mut records = Vec::new();
        pipeline.process_frame(&call, |record| records.push(record));
        pipeline.process_frame(&returned, |record| records.push(record));

        let returned = records
            .iter()
            .filter_map(|record| match &record.kind {
                CaptureRecordKind::Packet(packet)
                    if packet.fragment == Some(FragmentKind::Return) =>
                {
                    Some(packet)
                }
                _ => None,
            })
            .next()
            .expect("correlated Return packet");
        let route = returned.route.expect("correlated Return route");
        assert_eq!(route.key.service_id, service_id);
        assert_eq!(route.key.method_id, method_id);
        assert_eq!(route.call_id, Some(call_id));
        assert_eq!(
            returned.payload.application_bytes.as_deref(),
            Some([10, 3, 102, 111, 111].as_slice())
        );
    }

    fn captured_bpsr_frame(
        sequence: u64,
        source: IpEndpoint,
        destination: IpEndpoint,
        tcp_sequence: u32,
        fragment: u16,
        payload: &[u8],
    ) -> CapturedFrame {
        let length = 6 + payload.len();
        let mut bpsr = Vec::with_capacity(length);
        bpsr.extend_from_slice(&(length as u32).to_be_bytes());
        bpsr.extend_from_slice(&fragment.to_be_bytes());
        bpsr.extend_from_slice(payload);

        let tcp = TcpHeader::new(source.port, destination.port, tcp_sequence, 16_384);
        let builder = PacketBuilder::ipv4(
            match source.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            match destination.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            64,
        )
        .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(bpsr.len()));
        builder.write(&mut packet, &bpsr).unwrap();
        CapturedFrame {
            sequence,
            observed_micros: sequence,
            source_timestamp_nanos: Some(1_000_000 + sequence as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        }
    }
}
