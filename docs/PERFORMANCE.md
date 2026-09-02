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

| Process-owned capture ingress budget | Default |
| --- | ---: |
| Minimum Windows socket-table refresh interval | 20 ms |
| Unattributed frame lifetime | 250 ms |
| Pending frames | 8,192 |
| Pending frame bytes | 16 MiB |

The live ingress refreshes ownership only while an unknown exact flow is
pending. Known game flows stay on an O(1) lookup path. The queue preserves
capture order across the first-SYN/socket-table race; unattributed frames are
discarded when they expire or hit the memory ceiling.

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
cargo bench -p rlogs-game-bpsr --bench framing
```

The benchmarks cover Ethernet/IPv4/TCP decoding, sustained in-order and
reordered TCP reconstruction, out-of-order IP fragment reconstruction, and
contiguous versus split BPSR framing. Timing is recorded for release builds
but is not used as a brittle CI assertion across dissimilar machines.
Repeatable replay benchmarks and allocation/CPU budgets will be added before
the live parser is considered release-ready.

## Module optimizer compute boundary

Module scoring remains deterministic across compute backends. Exact CPU search
uses Rayon prefix partitions; bounded beam search runs its independent
candidate orderings in parallel. Users may explicitly enable the optional
OpenCL 1.2 path, which supports driver-provided NVIDIA and AMD devices and
falls back to CPU on discovery, compilation, allocation, or execution failure.

The GPU path is selected only for exact searches large enough to offset device
dispatch. Its compiled program is retained for the app process, and byte-radix
thresholding plus device-side compaction return only competitive results to
the CPU. On the development RTX 5060, an accuracy-checked 3,921,225-combination
probe measured about 105 ms on the release CPU path and 7 ms after the explicit
GPU check had compiled the kernel. This is a machine-specific diagnostic, not
a portable performance guarantee; AMD compatibility follows the OpenCL 1.2
contract and still requires hardware validation on an AMD system.

## Game-data memory boundary

Game data is not one process-lifetime JSON object. Build output is divided into
deterministic ID, stable-key, asset, and locale buckets:

| Budget | Default |
| --- | ---: |
| Resident game-data shards | 128 |
| Estimated resident shard bytes | 64 MiB |
| Compressed bytes per shard | 8 MiB |
| Uncompressed bytes per shard | 8 MiB |
| Compiled manifest | 4 MiB |

Record results remain valid through reference-counted ownership if their shard
is evicted. Localization for other languages is never loaded merely because a
user selected one locale. Icon/media bytes stay outside the lookup index.
