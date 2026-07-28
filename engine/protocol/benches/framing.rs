use std::{
    hint::black_box,
    net::{IpAddr, Ipv4Addr},
};

use bytes::{Bytes, BytesMut};
use criterion::{Criterion, criterion_group, criterion_main};
use rlogs_network::{IpEndpoint, TcpFlowKey, TcpStreamChunk};
use rlogs_protocol::{BpsrStreamFramer, PacketDirection};

fn flow() -> TcpFlowKey {
    TcpFlowKey::new(
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 31_000),
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 32_000),
    )
}

fn notify_frame(body_bytes: usize) -> Bytes {
    let length = 6 + 16 + body_bytes;
    let mut frame = BytesMut::with_capacity(length);
    frame.extend_from_slice(&(length as u32).to_be_bytes());
    frame.extend_from_slice(&2_u16.to_be_bytes());
    frame.extend_from_slice(&7_u64.to_be_bytes());
    frame.extend_from_slice(&8_u32.to_be_bytes());
    frame.extend_from_slice(&9_u32.to_be_bytes());
    frame.resize(length, 7);
    frame.freeze()
}

fn chunk(offset: u64, capture_sequence: u64, bytes: Bytes) -> TcpStreamChunk {
    TcpStreamChunk {
        flow: flow(),
        stream_offset: offset,
        capture_sequence,
        observed_micros: capture_sequence,
        bytes,
    }
}

fn benchmark(c: &mut Criterion) {
    let frame = notify_frame(1_400);
    c.bench_function("frame_contiguous_notify_1400b", |b| {
        b.iter(|| {
            let mut framer = BpsrStreamFramer::new(PacketDirection::ServerToClient);
            framer.process(chunk(0, 1, frame.clone()), |event| {
                black_box(event);
            });
            black_box(framer.metrics().frames_emitted);
        });
    });

    c.bench_function("frame_notify_1400b_across_2_chunks", |b| {
        b.iter(|| {
            let mut framer = BpsrStreamFramer::new(PacketDirection::ServerToClient);
            framer.process(chunk(0, 1, frame.slice(..700)), |event| {
                black_box(event);
            });
            framer.process(chunk(700, 2, frame.slice(700..)), |event| {
                black_box(event);
            });
            black_box(framer.metrics().frames_emitted);
        });
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
