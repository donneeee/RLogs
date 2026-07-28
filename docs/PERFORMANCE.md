# Native parser performance

Parser efficiency is a correctness requirement, not a late optimization pass.
The trusted native pipeline follows these rules:

- capture adapters perform one ownership copy when their source exposes
  temporary borrowed memory;
- decoded TCP payloads and reconstructed chunks are immutable shared slices,
  not copied buffers;
- IP fragments retain shared capture slices and perform one exact-size
  coalescing allocation only when a datagram completes;
- the ordinary in-order TCP path never creates a reorder buffer or an output
  collection;
- a BPSR frame contained in one TCP chunk remains a shared slice; a frame
  crossing chunks is coalesced once;
- zstd output and cumulative nested decompression are strictly capped;
- out-of-order work is isolated to the affected directional flow;
- flow count, queued segment count, per-flow bytes, total queued bytes, and
  idle lifetime are bounded;
- forced advancement, eviction, fragmentation, malformed input,
  retransmissions, overlaps, and capture gaps remain visible as typed evidence;
- counters expose fast-path use, buffering, high-water memory, duplicate work,
  gaps, and output bytes.

## Initial safety budgets

The defaults are deliberately configurable and conservative:

| Budget | Default |
| --- | ---: |
| Active directional flows | 4,096 |
| Reordered segments per flow | 256 |
| Reordered payload bytes per flow | 4 MiB |
| Reordered payload bytes across all flows | 64 MiB |
| Idle flow lifetime | 120 seconds |

These limits bound RLogs-owned reassembly state. If a limit would be exceeded,
the reassembler advances deterministically and emits a gap event. It never
silently discards uncertainty to make metrics look complete.

| IP fragment budget | Default |
| --- | ---: |
| Active fragmented datagrams | 1,024 |
| Fragments per datagram | 128 |
| Reassembled datagram bytes | 65,535 |
| Fragment bytes across all datagrams | 32 MiB |
| Absolute reassembly lifetime | 60 seconds |

IPv6 overlap drops follow RFC 5722. IPv4 accepts only an identical duplicate;
all ambiguous overlaps are dropped with typed evidence.

| BPSR framing budget | Default |
| --- | ---: |
| Active directional game streams | 128 |
| Complete wire frame | 16 MiB |
| Buffered TCP chunks per stream | 4,096 |
| Buffered frame bytes per stream | 16 MiB |
| Buffered frame bytes across all streams | 64 MiB |
| Cumulative decompressed bytes per top-level frame | 64 MiB |
| Nested wrapper depth | 8 |
| Idle framing lifetime | 120 seconds |

These are safety ceilings, not normal working-set targets. Ordinary traffic
usually retains only a few shared chunks long enough to read one frame header.

## Verification

Correctness and structural fast-path guarantees:

```text
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Optimized local microbenchmarks:

```text
cargo bench -p rlogs-network --bench packet_pipeline
cargo bench -p rlogs-protocol --bench framing
```

The benchmarks cover Ethernet/IPv4/TCP decoding, sustained in-order and
reordered TCP reconstruction, out-of-order IP fragment reconstruction, and
contiguous versus split BPSR framing. Timing is recorded for release builds
but is not used as a brittle CI assertion across dissimilar machines.
Repeatable replay benchmarks and allocation/CPU budgets will be added before
the live parser is considered release-ready.
