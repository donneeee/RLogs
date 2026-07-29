use rlogs_capture::CapturedFrame;
use rlogs_core::{
    GameConnectionFilter, TransportDirection, TransportGap, TransportOutput, TransportPipeline,
};
use rlogs_network::IpEndpoint;

use crate::{
    BpsrFramerSet, BpsrFramingEvent, BpsrFramingIssueReason, CaptureGap, CaptureGapKind,
    CaptureRecordDraft, CaptureRecordKind, NetworkEndpoint, PacketDirection, PacketEnvelope,
    PacketPayload,
};

/// BPSR's framing layer over Core's game-neutral reconstructed TCP streams.
#[derive(Debug)]
pub struct ResearchPipeline {
    connections: GameConnectionFilter,
    transport: TransportPipeline,
    framing: BpsrFramerSet,
    capture_start_unix_micros: Option<i64>,
    last_observed_micros: u64,
}

impl ResearchPipeline {
    pub fn new(connections: GameConnectionFilter) -> Self {
        Self {
            transport: TransportPipeline::new(connections.clone()),
            connections,
            framing: BpsrFramerSet::new(),
            capture_start_unix_micros: None,
            last_observed_micros: 0,
        }
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
        let capture_start = self.capture_start_unix_micros;
        self.transport.process_frame(frame, |output| {
            handle_transport_output(
                output,
                framing,
                connections,
                capture_start,
                frame.observed_micros,
                &mut emit,
            );
        });
    }

    pub fn finish(&mut self, mut emit: impl FnMut(CaptureRecordDraft)) {
        let stream_expiry = self.last_observed_micros.saturating_add(121 * 1_000_000);
        let observed_floor = self.last_observed_micros;
        let capture_start = self.capture_start_unix_micros;
        let connections = &self.connections;
        let framing = &mut self.framing;

        self.transport.finish(|output| {
            handle_transport_output(
                output,
                framing,
                connections,
                capture_start,
                observed_floor,
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
    emit: &mut impl FnMut(CaptureRecordDraft),
) {
    match output {
        TransportOutput::Stream(stream) => {
            let direction = match stream.identity.direction {
                TransportDirection::ClientToServer => PacketDirection::ClientToServer,
                TransportDirection::ServerToClient => PacketDirection::ServerToClient,
            };
            framing.process(direction, stream.event, |event| {
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
}
