use rlogs_capture::CapturedFrame;

use crate::{
    DecodeIssue, DecodeMetrics, DecodeResult, FrameDecoder, IpFragmentConfig,
    IpFragmentConfigError, IpFragmentDrop, IpFragmentEvent, IpFragmentMetrics,
    IpFragmentReassembler, TcpSegment,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkDecodeEvent {
    Tcp(TcpSegment),
    FragmentDropped(IpFragmentDrop),
    Ignored(DecodeIssue),
}

/// Turns captured frames into complete TCP segments while keeping IP fragment
/// reconstruction bounded and observable.
#[derive(Debug)]
pub struct NetworkDecoder {
    frame_decoder: FrameDecoder,
    fragments: IpFragmentReassembler,
}

impl Default for NetworkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkDecoder {
    pub fn new() -> Self {
        Self {
            frame_decoder: FrameDecoder::new(),
            fragments: IpFragmentReassembler::new(),
        }
    }

    pub fn try_with_fragment_config(
        fragment_config: IpFragmentConfig,
    ) -> Result<Self, IpFragmentConfigError> {
        Ok(Self {
            frame_decoder: FrameDecoder::new(),
            fragments: IpFragmentReassembler::try_with_config(fragment_config)?,
        })
    }

    pub fn decode_metrics(&self) -> &DecodeMetrics {
        self.frame_decoder.metrics()
    }

    pub fn fragment_metrics(&self) -> &IpFragmentMetrics {
        self.fragments.metrics()
    }

    pub fn process_frame(
        &mut self,
        frame: &CapturedFrame,
        mut emit: impl FnMut(NetworkDecodeEvent),
    ) {
        let Self {
            frame_decoder,
            fragments,
        } = self;

        fragments.expire(frame.observed_micros, |event| {
            if let IpFragmentEvent::Dropped(drop) = event {
                emit(NetworkDecodeEvent::FragmentDropped(drop));
            }
        });

        match frame_decoder.decode(frame) {
            DecodeResult::Tcp(segment) => emit(NetworkDecodeEvent::Tcp(segment)),
            DecodeResult::Ignored(issue) => emit(NetworkDecodeEvent::Ignored(issue)),
            DecodeResult::Fragment(fragment) => {
                fragments.process(fragment, |event| match event {
                    IpFragmentEvent::Dropped(drop) => {
                        emit(NetworkDecodeEvent::FragmentDropped(drop));
                    }
                    IpFragmentEvent::Datagram(datagram) => {
                        match frame_decoder.decode_reassembled(datagram) {
                            DecodeResult::Tcp(segment) => {
                                emit(NetworkDecodeEvent::Tcp(segment));
                            }
                            DecodeResult::Ignored(issue) => {
                                emit(NetworkDecodeEvent::Ignored(issue));
                            }
                            DecodeResult::Fragment(_) => {
                                unreachable!("reassembled datagrams cannot produce IP fragments");
                            }
                        }
                    }
                });
            }
        }
    }

    pub fn expire(&mut self, observed_micros: u64, mut emit: impl FnMut(NetworkDecodeEvent)) {
        self.fragments.expire(observed_micros, |event| {
            if let IpFragmentEvent::Dropped(drop) = event {
                emit(NetworkDecodeEvent::FragmentDropped(drop));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, TimestampNormalization};

    use super::*;

    fn captured(sequence: u64, link_type: CaptureLinkType, packet: Vec<u8>) -> CapturedFrame {
        CapturedFrame {
            sequence,
            observed_micros: sequence,
            source_timestamp_nanos: Some(sequence as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        }
    }

    fn raw_ipv4_tcp(payload: &[u8]) -> Vec<u8> {
        let tcp = TcpHeader::new(31_000, 32_000, 123, 16_384);
        let builder = PacketBuilder::ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64).tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        packet
    }

    fn ipv4_fragments(payload: &[u8]) -> [Vec<u8>; 2] {
        let packet = raw_ipv4_tcp(payload);
        let header = &packet[..20];
        let ip_payload = &packet[20..];
        let split = 24;

        let mut first = header.to_vec();
        first[2..4].copy_from_slice(&(20_u16 + split as u16).to_be_bytes());
        first[4..6].copy_from_slice(&7_u16.to_be_bytes());
        first[6..8].copy_from_slice(&0x2000_u16.to_be_bytes());
        first.extend_from_slice(&ip_payload[..split]);

        let mut second = header.to_vec();
        second[2..4].copy_from_slice(&(20_u16 + (ip_payload.len() - split) as u16).to_be_bytes());
        second[4..6].copy_from_slice(&7_u16.to_be_bytes());
        second[6..8].copy_from_slice(&((split / 8) as u16).to_be_bytes());
        second.extend_from_slice(&ip_payload[split..]);
        [first, second]
    }

    fn raw_ipv6_tcp(payload: &[u8]) -> Vec<u8> {
        let tcp = TcpHeader::new(31_000, 32_000, 123, 16_384);
        let builder = PacketBuilder::ipv6([1; 16], [2; 16], 64).tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        packet
    }

    fn ipv6_fragments(payload: &[u8]) -> [Vec<u8>; 2] {
        let packet = raw_ipv6_tcp(payload);
        let base_header = &packet[..40];
        let fragmentable = &packet[40..];
        let split = 24;

        let make_fragment = |offset: usize, more: bool, bytes: &[u8]| {
            let mut result = base_header.to_vec();
            result[4..6].copy_from_slice(&(8_u16 + bytes.len() as u16).to_be_bytes());
            result[6] = 44;
            result.extend_from_slice(&[
                6,
                0,
                (((offset / 8) as u16) << 3 >> 8) as u8,
                ((((offset / 8) as u16) << 3) as u8) | u8::from(more),
                0,
                0,
                0,
                7,
            ]);
            result.extend_from_slice(bytes);
            result
        };

        [
            make_fragment(0, true, &fragmentable[..split]),
            make_fragment(split, false, &fragmentable[split..]),
        ]
    }

    fn reassembles_out_of_order(
        link_type: CaptureLinkType,
        fragments: [Vec<u8>; 2],
        expected_payload: &[u8],
    ) {
        let mut decoder = NetworkDecoder::new();
        let mut events = Vec::new();
        decoder.process_frame(&captured(1, link_type, fragments[1].clone()), |event| {
            events.push(event)
        });
        decoder.process_frame(&captured(2, link_type, fragments[0].clone()), |event| {
            events.push(event)
        });

        let [NetworkDecodeEvent::Tcp(segment)] = events.as_slice() else {
            panic!("expected one reconstructed TCP segment, got {events:?}");
        };
        assert_eq!(segment.payload, expected_payload);
        assert_eq!(decoder.fragment_metrics().datagrams_completed, 1);
        assert_eq!(decoder.decode_metrics().reassembled_tcp_segments, 1);
    }

    #[test]
    fn reconstructs_out_of_order_ipv4_before_tcp_decode() {
        let payload = b"abcdefghijkl";
        reassembles_out_of_order(CaptureLinkType::RawIpv4, ipv4_fragments(payload), payload);
    }

    #[test]
    fn reconstructs_out_of_order_ipv6_before_tcp_decode() {
        let payload = b"abcdefghijkl";
        reassembles_out_of_order(CaptureLinkType::RawIpv6, ipv6_fragments(payload), payload);
    }
}
