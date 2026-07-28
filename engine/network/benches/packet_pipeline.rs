use std::{
    hint::black_box,
    net::{IpAddr, Ipv4Addr},
};

use bytes::Bytes;
use criterion::{Criterion, criterion_group, criterion_main};
use etherparse::{PacketBuilder, TcpHeader};
use rlogs_capture::{CaptureLinkType, CapturedFrame, TimestampNormalization};
use rlogs_network::{
    DecodeResult, FrameDecoder, IpEndpoint, IpFragment, IpFragmentKey, IpFragmentReassembler,
    TcpFlags, TcpFlowKey, TcpReassembler, TcpSegment,
};

fn captured_frame(payload: &[u8]) -> CapturedFrame {
    let tcp = TcpHeader::new(31_000, 32_000, 100, 16_384);
    let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
        .ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64)
        .tcp_header(tcp);
    let mut packet = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut packet, payload).unwrap();
    CapturedFrame {
        sequence: 1,
        observed_micros: 0,
        source_timestamp_nanos: None,
        timestamp_normalization: TimestampNormalization::Unavailable,
        interface_id: None,
        link_type: CaptureLinkType::Ethernet,
        original_length: packet.len() as u32,
        bytes: packet.into(),
    }
}

fn flow() -> TcpFlowKey {
    TcpFlowKey::new(
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 31_000),
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 32_000),
    )
}

fn benchmark(c: &mut Criterion) {
    let frame = captured_frame(&[7; 1_400]);
    c.bench_function("decode_ethernet_ipv4_tcp_1400b", |b| {
        let mut decoder = FrameDecoder::new();
        b.iter(|| {
            let result = decoder.decode(black_box(&frame));
            black_box(matches!(result, DecodeResult::Tcp(_)));
        });
    });

    let payload = Bytes::from_static(&[7; 1_400]);
    c.bench_function("reassemble_in_order_1024x1400b", |b| {
        b.iter(|| {
            let mut reassembler = TcpReassembler::new();
            let mut sequence = 100_u32;
            for capture_sequence in 1..=1_024 {
                reassembler.process(
                    TcpSegment {
                        flow: flow(),
                        sequence_number: sequence,
                        acknowledgment_number: 0,
                        flags: TcpFlags::default(),
                        capture_sequence,
                        observed_micros: capture_sequence,
                        payload: payload.clone(),
                    },
                    |event| {
                        black_box(event);
                    },
                );
                sequence = sequence.wrapping_add(payload.len() as u32);
            }
            black_box(reassembler.metrics().stream_bytes);
        });
    });

    let reordered_payload = Bytes::from_static(&[7; 256]);
    c.bench_function("reassemble_reordered_1024x256b", |b| {
        b.iter(|| {
            let mut reassembler = TcpReassembler::new();
            let mut capture_sequence = 1_u64;
            let mut expected = 100_u32;

            reassembler.process(
                TcpSegment {
                    flow: flow(),
                    sequence_number: expected,
                    acknowledgment_number: 0,
                    flags: TcpFlags::default(),
                    capture_sequence,
                    observed_micros: capture_sequence,
                    payload: reordered_payload.clone(),
                },
                |event| {
                    black_box(event);
                },
            );
            capture_sequence += 1;
            expected = expected.wrapping_add(reordered_payload.len() as u32);

            for _ in 0..511 {
                let future = expected.wrapping_add(reordered_payload.len() as u32);
                for sequence_number in [future, expected] {
                    reassembler.process(
                        TcpSegment {
                            flow: flow(),
                            sequence_number,
                            acknowledgment_number: 0,
                            flags: TcpFlags::default(),
                            capture_sequence,
                            observed_micros: capture_sequence,
                            payload: reordered_payload.clone(),
                        },
                        |event| {
                            black_box(event);
                        },
                    );
                    capture_sequence += 1;
                }
                expected = expected.wrapping_add((reordered_payload.len() * 2) as u32);
            }
            reassembler.process(
                TcpSegment {
                    flow: flow(),
                    sequence_number: expected,
                    acknowledgment_number: 0,
                    flags: TcpFlags::default(),
                    capture_sequence,
                    observed_micros: capture_sequence,
                    payload: reordered_payload.clone(),
                },
                |event| {
                    black_box(event);
                },
            );
            black_box(reassembler.metrics().stream_bytes);
        });
    });

    let first_fragment = Bytes::from_static(&[7; 1_024]);
    let final_fragment = Bytes::from_static(&[7; 376]);
    let fragment_key = IpFragmentKey::Ipv4 {
        source: Ipv4Addr::new(10, 0, 0, 1),
        destination: Ipv4Addr::new(10, 0, 0, 2),
        protocol: 6,
        identification: 7,
    };
    c.bench_function("reassemble_ipv4_1400b_from_2_fragments", |b| {
        b.iter(|| {
            let mut reassembler = IpFragmentReassembler::new();
            for fragment in [
                IpFragment {
                    key: fragment_key,
                    offset: 1_024,
                    more_fragments: false,
                    capture_sequence: 1,
                    observed_micros: 1,
                    payload: final_fragment.clone(),
                },
                IpFragment {
                    key: fragment_key,
                    offset: 0,
                    more_fragments: true,
                    capture_sequence: 2,
                    observed_micros: 2,
                    payload: first_fragment.clone(),
                },
            ] {
                reassembler.process(fragment, |event| {
                    black_box(event);
                });
            }
            black_box(reassembler.metrics().datagram_bytes_completed);
        });
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
