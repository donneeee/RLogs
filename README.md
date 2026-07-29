# RLogs

RLogs is a new, standalone, game-neutral analysis and log-submission platform.
Its goal is closer to a modern Advanced Combat Tracker plus a native FFLogs-
style submission client than to a single DPS meter. **Blue Protocol: Star
Resonance** is its first bundled game integration.

RLogs does not embed or run another meter. Existing projects may be studied as
behavioral references, but the RLogs runtime, data model, plugin contracts, and
cross-platform capture pipeline are implemented independently.

## What RLogs is building

- one reusable Windows and Linux packet-capture and replay pipeline;
- trusted game plug-ins with region- and client-build-aware protocol packs;
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

The native Core decodes link/IP/TCP headers, reconstructs bounded IPv4 and IPv6
fragments, and rebuilds directional TCP streams. The bundled BPSR game plug-in
then frames and decodes BPSR messages. Immutable payload storage is shared
through the boundary when data is contiguous, and fragmentation,
retransmissions, overlaps, resynchronization, decompression failures,
evictions, and memory-pressure gaps remain explicit. See
[`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

Windows live capture now follows server rotation by continuously attributing
exact TCP connections to `BPSR_STEAM`. Broad adapter traffic exists only in a
bounded in-memory ingress; unrelated frames are discarded before persistence
or protocol decoding. Linux uses the same ownership-filter contract but still
needs its native process/socket implementation.

External parser audit pins are maintained in
[`docs/PARSER_REFERENCES.md`](docs/PARSER_REFERENCES.md). The language-neutral
engine and locale-pack boundary are documented in
[`docs/LOCALIZATION.md`](docs/LOCALIZATION.md). Human-readable, build-scoped
game-data rules are in [`game-data/README.md`](game-data/README.md), and the
automated profile field census is in
[`docs/CHARACTER_PROFILE_COVERAGE.md`](docs/CHARACTER_PROFILE_COVERAGE.md).
The website-facing profile boundary is in
[`docs/PROFILE_AUTOMATION.md`](docs/PROFILE_AUTOMATION.md), and the current
38-route world-load reconciliation is in
[`docs/WORLD_LOAD_ROUTE_RESEARCH.md`](docs/WORLD_LOAD_ROUTE_RESEARCH.md).
The first complete BPSR client-file inventory is summarized in
[`docs/GAME_FILE_RESEARCH.md`](docs/GAME_FILE_RESEARCH.md), with its compact
review catalog under
[`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/`](plugins/games/blue-protocol-star-resonance/research/game-file-inventory/).
Packed CTB field inference, evidence states, and the first reviewed
profile/equipment fields are documented in
[`docs/CTB_SCHEMA_RESEARCH.md`](docs/CTB_SCHEMA_RESEARCH.md).

## Plugin Lab

The first UI is a read-only extension workbench. It rescans installed,
built-in, example, and game plug-ins; shows API compatibility, declared
capabilities, imports/exports, resource storage, dependency order, and every
before/after Core hook stage.

```text
cargo run -p rlogs-plugin-lab
```

Open `http://127.0.0.1:7418`. The lab never executes plug-in code.

## Repository map

| Folder | Purpose |
| --- | --- |
| [`apps/`](apps/) | User-facing desktop and future command-line applications |
| [`assets/`](assets/) | Host-controlled per-plug-in and provider-owned shared asset namespaces |
| [`engine/`](engine/) | Game-neutral capture, network, event, submission, and plug-in-host foundations |
| [`game-data/`](game-data/) | Shared organization rules for game-owned catalogs |
| [`locales/`](locales/) | Migration marker pointing to data-only locale add-ons |
| [`plugins/games/`](plugins/games/) | Trusted game integrations; BPSR protocol, data, profiles, and upload projection live here |
| [`plugins/builtin/`](plugins/builtin/) | Replaceable first-party features built on the ordinary add-on API |
| [`plugins/installed/`](plugins/installed/PUT_PLUGINS_HERE.md) | Obvious drop-in folder for directory-packaged community plug-ins |
| [`protocol-packs/`](protocol-packs/) | Shared pack rules; actual packs live in game plug-ins |
| [`protocol-references/`](protocol-references/) | Shared evidence rules; actual references live in game plug-ins |
| [`research/`](research/) | Shared sanitized-research rules; actual inventories live in game plug-ins |
| [`sdk/`](sdk/) | Plugin SDKs and examples for supported languages |
| [`services/`](services/) | Future upload, verification, profile, and leaderboard services |
| [`tools/`](tools/) | Protocol research, coverage, replay, and pack-generation tools |
| [`tests/fixtures/`](tests/fixtures/) | Sanitized replay fixtures and expected outputs |
| [`docs/`](docs/) | Architecture, privacy boundaries, and project decisions |

The initial capture decision is documented in
[`docs/CAPTURE.md`](docs/CAPTURE.md): one pcap-based live adapter using Npcap
on Windows and libpcap on Linux, with pcap/pcapng replay on either platform.
The current exact-build research procedure is in
[`docs/CONTROLLED_CAPTURE.md`](docs/CONTROLLED_CAPTURE.md).

## Current checks

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo bench -p rlogs-network --bench packet_pipeline
cargo bench -p rlogs-game-bpsr --bench framing
```

Raw packet captures are private research artifacts and are ignored by Git.
They are not website submission files.
