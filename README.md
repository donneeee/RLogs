# RLogs

RLogs is a new, standalone combat-analysis platform for **Blue Protocol: Star
Resonance**. Its goal is closer to Advanced Combat Tracker plus a native log
submission client than to a single DPS meter.

RLogs does not embed or run another meter. Existing projects may be studied as
behavioral references, but the RLogs runtime, data model, plugin contracts, and
cross-platform capture pipeline are implemented independently.

## What RLogs is building

- one Windows and Linux packet-capture and replay pipeline;
- region- and client-build-aware protocol packs;
- a lossless protocol research journal;
- stable canonical events that do not expose game opcodes to ordinary plugins;
- first-party and community plugins using the same public API;
- replayable `.rlog` files;
- character profiles that exclude credentials and private account data;
- resumable, privacy-reviewed uploads for a future leaderboard service;
- server-side replay and verification for DPS, healing, deaths, movement, and
  support-contribution calculations such as rDPS.

The project is intentionally early. Packet coverage, plugin compatibility, and
ranked-log verification are not yet complete.

The first offline capture slice can stream and inspect pcap/pcapng files:

```text
cargo run -p rlogs-capture-inspect -- capture.pcapng
```

The native pipeline now decodes link/IP/TCP headers, reconstructs bounded IPv4
and IPv6 fragments, rebuilds directional TCP streams, and frames BPSR
messages. It shares immutable payload storage from capture through framing
when data is contiguous, and reports fragmentation, retransmissions, overlaps,
resynchronization, decompression failures, evictions, and memory-pressure gaps
instead of hiding them. See [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

External parser audit pins are maintained in
[`docs/PARSER_REFERENCES.md`](docs/PARSER_REFERENCES.md). The language-neutral
engine and locale-pack boundary are documented in
[`docs/LOCALIZATION.md`](docs/LOCALIZATION.md).

## Repository map

| Folder | Purpose |
| --- | --- |
| [`apps/`](apps/) | User-facing desktop and future command-line applications |
| [`engine/`](engine/) | Trusted capture, protocol, event, log, and plugin-host foundations |
| [`locales/`](locales/) | First-party interface translations, separate from canonical IDs |
| [`plugins/`](plugins/) | Bundled first-party plugins built on the public plugin API |
| [`protocol-packs/`](protocol-packs/) | Region- and build-specific protocol knowledge |
| [`sdk/`](sdk/) | Plugin SDKs and examples for supported languages |
| [`services/`](services/) | Future upload, verification, profile, and leaderboard services |
| [`tools/`](tools/) | Protocol research, coverage, replay, and pack-generation tools |
| [`tests/fixtures/`](tests/fixtures/) | Sanitized replay fixtures and expected outputs |
| [`docs/`](docs/) | Architecture, privacy boundaries, and project decisions |

The initial capture decision is documented in
[`docs/CAPTURE.md`](docs/CAPTURE.md): one pcap-based live adapter using Npcap
on Windows and libpcap on Linux, with pcap/pcapng replay on either platform.

## Current checks

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo bench -p rlogs-network --bench packet_pipeline
cargo bench -p rlogs-protocol --bench framing
```

Raw packet captures are private research artifacts and are ignored by Git.
They are not website submission files.
