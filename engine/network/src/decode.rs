use std::net::IpAddr;

use bytes::Bytes;
use etherparse::{
    EtherType, IpNumber, Ipv4ExtensionsSlice, Ipv4Slice, Ipv6ExtensionSlice, Ipv6ExtensionsSlice,
    Ipv6Slice, NetSlice, SlicedPacket, TcpSlice, TransportSlice, ip_number,
};
use rlogs_capture::{CaptureLinkType, CapturedFrame};
use serde::{Deserialize, Serialize};

use crate::{
    IpEndpoint, IpFragment, IpFragmentKey, ReassembledIpDatagram, TcpFlags, TcpFlowKey, TcpSegment,
};

const LINUX_SLL2_HEADER_LEN: usize = 20;
const NULL_LOOPBACK_HEADER_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeIssue {
    UnsupportedLinkType,
    MalformedPacket,
    IpVersionMismatch,
    NonIpPacket,
    FragmentedIpPacket,
    NonTcpPacket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeResult {
    Tcp(TcpSegment),
    Fragment(IpFragment),
    Ignored(DecodeIssue),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodeMetrics {
    pub frames_seen: u64,
    pub tcp_segments: u64,
    pub tcp_payload_bytes: u64,
    pub unsupported_link_frames: u64,
    pub malformed_frames: u64,
    pub ip_version_mismatches: u64,
    pub non_ip_frames: u64,
    pub fragmented_ip_frames: u64,
    pub reassembled_datagrams_seen: u64,
    pub reassembled_tcp_segments: u64,
    pub non_tcp_frames: u64,
}

#[derive(Debug, Default)]
pub struct FrameDecoder {
    metrics: DecodeMetrics,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metrics(&self) -> &DecodeMetrics {
        &self.metrics
    }

    pub fn decode(&mut self, frame: &CapturedFrame) -> DecodeResult {
        self.metrics.frames_seen = self.metrics.frames_seen.saturating_add(1);

        let parsed = match parse_frame(frame) {
            Ok(parsed) => parsed,
            Err(issue) => return self.ignore(issue),
        };

        if matches!(
            (frame.link_type, parsed.net.as_ref()),
            (CaptureLinkType::RawIpv4, Some(NetSlice::Ipv6(_)))
                | (CaptureLinkType::RawIpv6, Some(NetSlice::Ipv4(_)))
        ) {
            return self.ignore(DecodeIssue::IpVersionMismatch);
        }

        let (source_address, destination_address) = match parsed.net.as_ref() {
            Some(NetSlice::Ipv4(ip)) if ip.is_payload_fragmented() => {
                return match ipv4_fragment(frame, ip) {
                    Ok(fragment) => self.fragment(fragment),
                    Err(issue) => self.ignore(issue),
                };
            }
            Some(NetSlice::Ipv6(ip)) if ip.is_payload_fragmented() => {
                return match ipv6_fragment(frame, ip) {
                    Ok(fragment) => self.fragment(fragment),
                    Err(issue) => self.ignore(issue),
                };
            }
            Some(NetSlice::Ipv4(ip)) => (
                IpAddr::V4(ip.header().source_addr()),
                IpAddr::V4(ip.header().destination_addr()),
            ),
            Some(NetSlice::Ipv6(ip)) => (
                IpAddr::V6(ip.header().source_addr()),
                IpAddr::V6(ip.header().destination_addr()),
            ),
            _ => return self.ignore(DecodeIssue::NonIpPacket),
        };

        let Some(TransportSlice::Tcp(tcp)) = parsed.transport else {
            return self.ignore(DecodeIssue::NonTcpPacket);
        };

        let payload = frame.bytes.slice_ref(tcp.payload());
        self.tcp(
            source_address,
            destination_address,
            tcp,
            frame.sequence,
            frame.observed_micros,
            payload,
            false,
        )
    }

    pub fn decode_reassembled(&mut self, datagram: ReassembledIpDatagram) -> DecodeResult {
        self.metrics.reassembled_datagrams_seen =
            self.metrics.reassembled_datagrams_seen.saturating_add(1);

        let (source_address, destination_address, next_header, payload) = match datagram.key {
            IpFragmentKey::Ipv4 {
                source,
                destination,
                protocol,
                ..
            } => {
                let Ok((_, next_header, payload)) =
                    Ipv4ExtensionsSlice::from_slice(IpNumber(protocol), &datagram.payload)
                else {
                    return self.ignore(DecodeIssue::MalformedPacket);
                };
                (
                    IpAddr::V4(source),
                    IpAddr::V4(destination),
                    next_header,
                    payload,
                )
            }
            IpFragmentKey::Ipv6 {
                source,
                destination,
                next_header,
                ..
            } => {
                let Ok((_, next_header, payload)) =
                    Ipv6ExtensionsSlice::from_slice(IpNumber(next_header), &datagram.payload)
                else {
                    return self.ignore(DecodeIssue::MalformedPacket);
                };
                (
                    IpAddr::V6(source),
                    IpAddr::V6(destination),
                    next_header,
                    payload,
                )
            }
        };

        if next_header != ip_number::TCP {
            return self.ignore(DecodeIssue::NonTcpPacket);
        }
        let Ok(tcp) = TcpSlice::from_slice(payload) else {
            return self.ignore(DecodeIssue::MalformedPacket);
        };
        let tcp_payload = datagram.payload.slice_ref(tcp.payload());
        self.tcp(
            source_address,
            destination_address,
            tcp,
            datagram.completed_capture_sequence,
            datagram.completed_observed_micros,
            tcp_payload,
            true,
        )
    }

    fn fragment(&mut self, fragment: IpFragment) -> DecodeResult {
        self.metrics.fragmented_ip_frames = self.metrics.fragmented_ip_frames.saturating_add(1);
        DecodeResult::Fragment(fragment)
    }

    #[allow(clippy::too_many_arguments)]
    fn tcp(
        &mut self,
        source_address: IpAddr,
        destination_address: IpAddr,
        tcp: TcpSlice<'_>,
        capture_sequence: u64,
        observed_micros: u64,
        payload: Bytes,
        reassembled: bool,
    ) -> DecodeResult {
        let segment = TcpSegment {
            flow: TcpFlowKey::new(
                IpEndpoint::new(source_address, tcp.source_port()),
                IpEndpoint::new(destination_address, tcp.destination_port()),
            ),
            sequence_number: tcp.sequence_number(),
            acknowledgment_number: tcp.acknowledgment_number(),
            flags: TcpFlags {
                ns: tcp.ns(),
                fin: tcp.fin(),
                syn: tcp.syn(),
                rst: tcp.rst(),
                psh: tcp.psh(),
                ack: tcp.ack(),
                urg: tcp.urg(),
                ece: tcp.ece(),
                cwr: tcp.cwr(),
            },
            capture_sequence,
            observed_micros,
            payload,
        };

        self.metrics.tcp_segments = self.metrics.tcp_segments.saturating_add(1);
        self.metrics.tcp_payload_bytes = self
            .metrics
            .tcp_payload_bytes
            .saturating_add(segment.payload.len() as u64);
        if reassembled {
            self.metrics.reassembled_tcp_segments =
                self.metrics.reassembled_tcp_segments.saturating_add(1);
        }
        DecodeResult::Tcp(segment)
    }

    fn ignore(&mut self, issue: DecodeIssue) -> DecodeResult {
        let counter = match issue {
            DecodeIssue::UnsupportedLinkType => &mut self.metrics.unsupported_link_frames,
            DecodeIssue::MalformedPacket => &mut self.metrics.malformed_frames,
            DecodeIssue::IpVersionMismatch => &mut self.metrics.ip_version_mismatches,
            DecodeIssue::NonIpPacket => &mut self.metrics.non_ip_frames,
            DecodeIssue::FragmentedIpPacket => &mut self.metrics.fragmented_ip_frames,
            DecodeIssue::NonTcpPacket => &mut self.metrics.non_tcp_frames,
        };
        *counter = counter.saturating_add(1);
        DecodeResult::Ignored(issue)
    }
}

fn ipv4_fragment(frame: &CapturedFrame, ip: &Ipv4Slice<'_>) -> Result<IpFragment, DecodeIssue> {
    let header = ip.header();
    let packet_start =
        subslice_start(&frame.bytes, header.slice()).ok_or(DecodeIssue::MalformedPacket)?;
    let payload_start = packet_start
        .checked_add(header.slice().len())
        .ok_or(DecodeIssue::MalformedPacket)?;
    let packet_end = packet_start
        .checked_add(usize::from(header.total_len()))
        .ok_or(DecodeIssue::MalformedPacket)?;
    let payload = frame
        .bytes
        .get(payload_start..packet_end)
        .ok_or(DecodeIssue::MalformedPacket)?;

    Ok(IpFragment {
        key: IpFragmentKey::Ipv4 {
            source: header.source_addr(),
            destination: header.destination_addr(),
            protocol: header.protocol().0,
            identification: header.identification(),
        },
        offset: u32::from(header.fragments_offset().value()) * 8,
        more_fragments: header.more_fragments(),
        capture_sequence: frame.sequence,
        observed_micros: frame.observed_micros,
        payload: frame.bytes.slice_ref(payload),
    })
}

fn ipv6_fragment(frame: &CapturedFrame, ip: &Ipv6Slice<'_>) -> Result<IpFragment, DecodeIssue> {
    let header = ip.header();
    if header.payload_length() == 0 {
        return Err(DecodeIssue::MalformedPacket);
    }
    let fragment_header = ip
        .extensions()
        .clone()
        .into_iter()
        .find_map(|extension| match extension {
            Ipv6ExtensionSlice::Fragment(fragment) if fragment.is_fragmenting_payload() => {
                Some(fragment)
            }
            _ => None,
        })
        .ok_or(DecodeIssue::FragmentedIpPacket)?;
    let packet_start =
        subslice_start(&frame.bytes, header.slice()).ok_or(DecodeIssue::MalformedPacket)?;
    let packet_end = packet_start
        .checked_add(header.slice().len())
        .and_then(|length| length.checked_add(usize::from(header.payload_length())))
        .ok_or(DecodeIssue::MalformedPacket)?;
    let payload_start = subslice_start(&frame.bytes, fragment_header.slice())
        .and_then(|start| start.checked_add(fragment_header.slice().len()))
        .ok_or(DecodeIssue::MalformedPacket)?;
    let payload = frame
        .bytes
        .get(payload_start..packet_end)
        .ok_or(DecodeIssue::MalformedPacket)?;

    Ok(IpFragment {
        key: IpFragmentKey::Ipv6 {
            source: header.source_addr(),
            destination: header.destination_addr(),
            next_header: fragment_header.next_header().0,
            identification: fragment_header.identification(),
        },
        offset: u32::from(fragment_header.fragment_offset().value()) * 8,
        more_fragments: fragment_header.more_fragments(),
        capture_sequence: frame.sequence,
        observed_micros: frame.observed_micros,
        payload: frame.bytes.slice_ref(payload),
    })
}

fn subslice_start(container: &[u8], subslice: &[u8]) -> Option<usize> {
    let container_start = container.as_ptr() as usize;
    let container_end = container_start.checked_add(container.len())?;
    let subslice_start = subslice.as_ptr() as usize;
    let subslice_end = subslice_start.checked_add(subslice.len())?;
    (subslice_start >= container_start && subslice_end <= container_end)
        .then_some(subslice_start - container_start)
}

fn parse_frame(frame: &CapturedFrame) -> Result<SlicedPacket<'_>, DecodeIssue> {
    let bytes = frame.bytes.as_ref();
    let result = match frame.link_type {
        CaptureLinkType::Ethernet => SlicedPacket::from_ethernet(bytes),
        CaptureLinkType::LinuxCookedV1 => SlicedPacket::from_linux_sll(bytes),
        CaptureLinkType::RawIp | CaptureLinkType::RawIpv4 | CaptureLinkType::RawIpv6 => {
            SlicedPacket::from_ip(bytes)
        }
        CaptureLinkType::NullLoopback => {
            let ip = bytes
                .get(NULL_LOOPBACK_HEADER_LEN..)
                .ok_or(DecodeIssue::MalformedPacket)?;
            SlicedPacket::from_ip(ip)
        }
        CaptureLinkType::LinuxCookedV2 => {
            let header = bytes
                .get(..LINUX_SLL2_HEADER_LEN)
                .ok_or(DecodeIssue::MalformedPacket)?;
            let payload = &bytes[LINUX_SLL2_HEADER_LEN..];
            let protocol = EtherType(u16::from_be_bytes([header[0], header[1]]));
            SlicedPacket::from_ether_type(protocol, payload)
        }
        CaptureLinkType::Unknown(_) => return Err(DecodeIssue::UnsupportedLinkType),
    };

    result.map_err(|_| DecodeIssue::MalformedPacket)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use etherparse::{LinuxSllPacketType, PacketBuilder, TcpHeader};
    use rlogs_capture::{CapturedFrame, TimestampNormalization};

    use super::*;

    fn ethernet_tcp_frame(payload: &[u8]) -> CapturedFrame {
        let mut tcp = TcpHeader::new(31_000, 32_000, 123, 16_384);
        tcp.ack = true;
        tcp.acknowledgment_number = 456;
        let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
            .ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64)
            .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        captured(CaptureLinkType::Ethernet, packet)
    }

    fn raw_ipv4_tcp(payload: &[u8]) -> Vec<u8> {
        let tcp = TcpHeader::new(31_000, 32_000, 123, 16_384);
        let builder = PacketBuilder::ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64).tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        packet
    }

    fn raw_ipv6_tcp(payload: &[u8]) -> Vec<u8> {
        let tcp = TcpHeader::new(31_000, 32_000, 123, 16_384);
        let builder = PacketBuilder::ipv6([1; 16], [2; 16], 64).tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        packet
    }

    fn captured(link_type: CaptureLinkType, packet: Vec<u8>) -> CapturedFrame {
        CapturedFrame {
            sequence: 7,
            observed_micros: 42,
            source_timestamp_nanos: Some(42_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type,
            original_length: packet.len() as u32,
            bytes: packet.into(),
        }
    }

    #[test]
    fn ethernet_ipv4_tcp_payload_is_a_shared_frame_slice() {
        let frame = ethernet_tcp_frame(b"rlogs");
        let frame_start = frame.bytes.as_ptr() as usize;
        let frame_end = frame_start + frame.bytes.len();
        let mut decoder = FrameDecoder::new();

        let DecodeResult::Tcp(segment) = decoder.decode(&frame) else {
            panic!("expected TCP");
        };

        assert_eq!(
            segment.flow.source.address,
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(segment.flow.source.port, 31_000);
        assert_eq!(segment.sequence_number, 123);
        assert_eq!(segment.acknowledgment_number, 456);
        assert!(segment.flags.ack);
        assert_eq!(segment.payload, b"rlogs".as_slice());
        assert!((frame_start..frame_end).contains(&(segment.payload.as_ptr() as usize)));
        assert_eq!(decoder.metrics().tcp_payload_bytes, 5);
    }

    #[test]
    fn linux_cooked_v2_is_decoded_without_rewriting_the_packet() {
        let ip_packet = raw_ipv4_tcp(b"sll2");
        let mut packet = vec![0_u8; LINUX_SLL2_HEADER_LEN];
        packet[0..2].copy_from_slice(&0x0800_u16.to_be_bytes());
        packet.extend_from_slice(&ip_packet);
        let frame = captured(CaptureLinkType::LinuxCookedV2, packet);

        let DecodeResult::Tcp(segment) = FrameDecoder::new().decode(&frame) else {
            panic!("expected TCP");
        };

        assert_eq!(segment.payload, b"sll2".as_slice());
    }

    #[test]
    fn linux_cooked_v1_and_raw_ipv6_use_the_same_tcp_model() {
        let builder =
            PacketBuilder::linux_sll(LinuxSllPacketType::OTHERHOST, 6, [1, 2, 3, 4, 5, 6, 0, 0])
                .ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64)
                .tcp(31_000, 32_000, 123, 16_384);
        let mut sll_packet = Vec::with_capacity(builder.size(3));
        builder.write(&mut sll_packet, b"sll").unwrap();
        let sll_frame = captured(CaptureLinkType::LinuxCookedV1, sll_packet);
        let ipv6_frame = captured(CaptureLinkType::RawIpv6, raw_ipv6_tcp(b"ipv6"));
        let mut decoder = FrameDecoder::new();

        let DecodeResult::Tcp(sll) = decoder.decode(&sll_frame) else {
            panic!("expected SLL TCP");
        };
        let DecodeResult::Tcp(ipv6) = decoder.decode(&ipv6_frame) else {
            panic!("expected IPv6 TCP");
        };

        assert_eq!(sll.payload, b"sll".as_slice());
        assert_eq!(ipv6.payload, b"ipv6".as_slice());
        assert!(ipv6.flow.source.address.is_ipv6());
    }

    #[test]
    fn null_loopback_ignores_the_platform_specific_family_encoding() {
        let mut packet = vec![2, 0, 0, 0];
        packet.extend_from_slice(&raw_ipv4_tcp(b"loopback"));
        let frame = captured(CaptureLinkType::NullLoopback, packet);

        let DecodeResult::Tcp(segment) = FrameDecoder::new().decode(&frame) else {
            panic!("expected TCP");
        };

        assert_eq!(segment.payload, b"loopback".as_slice());
    }

    #[test]
    fn fragmented_ip_is_extracted_without_misparsing_it_as_tcp() {
        let mut frame = ethernet_tcp_frame(b"fragment");
        // IPv4 flags/fragment-offset field after the 14-byte Ethernet header.
        frame.bytes = {
            let mut bytes = frame.bytes.to_vec();
            bytes[20] |= 0x20;
            Bytes::from(bytes)
        };
        let mut decoder = FrameDecoder::new();

        let DecodeResult::Fragment(fragment) = decoder.decode(&frame) else {
            panic!("expected an IP fragment");
        };
        assert_eq!(fragment.offset, 0);
        assert!(fragment.more_fragments);
        assert_eq!(fragment.payload, &frame.bytes[34..]);
        assert_eq!(decoder.metrics().fragmented_ip_frames, 1);
    }

    #[test]
    fn unsupported_and_malformed_frames_have_bounded_typed_results() {
        let unknown = captured(CaptureLinkType::Unknown(999), vec![1, 2, 3]);
        let malformed = captured(CaptureLinkType::Ethernet, vec![1, 2, 3]);
        let mut decoder = FrameDecoder::new();

        assert_eq!(
            decoder.decode(&unknown),
            DecodeResult::Ignored(DecodeIssue::UnsupportedLinkType)
        );
        assert_eq!(
            decoder.decode(&malformed),
            DecodeResult::Ignored(DecodeIssue::MalformedPacket)
        );
        assert_eq!(decoder.metrics().unsupported_link_frames, 1);
        assert_eq!(decoder.metrics().malformed_frames, 1);
    }

    #[test]
    fn declared_raw_ip_version_mismatches_are_visible() {
        let frame = captured(CaptureLinkType::RawIpv4, raw_ipv6_tcp(b"wrong-family"));
        let mut decoder = FrameDecoder::new();

        assert_eq!(
            decoder.decode(&frame),
            DecodeResult::Ignored(DecodeIssue::IpVersionMismatch)
        );
        assert_eq!(decoder.metrics().ip_version_mismatches, 1);
    }
}
