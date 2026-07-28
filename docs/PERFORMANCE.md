# Native parser performance

Parser efficiency is a correctness requirement, not a late optimization pass.
The trusted native pipeline follows these rules:

- capture adapters perform one ownership copy when their source exposes
  temporary borrowed memory;
- decoded TCP payloads and reconstructed chunks are immutable shared slices,
  not copied buffers;
- the ordinary in-order TCP path never creates a reorder buffer or an output
  collection;
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

IP fragment reassembly is intentionally separate from TCP reassembly. Until
the bounded IP fragment stage is implemented, fragmented datagrams are counted
and reported rather than partially decoded as if they were complete.

## Verification

Correctness and structural fast-path guarantees:

```text
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Optimized local microbenchmarks:

```text
cargo bench -p rlogs-network --bench packet_pipeline
```

The benchmark covers Ethernet/IPv4/TCP decoding plus sustained in-order and
reordered reassembly. Timing is recorded for release builds but is not used as
a brittle CI assertion across dissimilar machines. Repeatable replay
benchmarks and allocation/CPU budgets will be added before the live parser is
considered release-ready.
