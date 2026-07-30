# Architecture

## Product boundary

RLogs has one packet-processing pipeline:

```text
capture source
    |
    v
link / IP / TCP decode
    |
    v
TCP stream reconstruction
    |
    v
trusted game plug-in
    |
    +--> message framing and bounded decompression
    +--> region/build protocol pack
    +--> privacy-reviewed selective decoder
    +--> game-owned profile and website projection
    |
    v
versioned canonical events
    |
    +--> raw-ID Event Viewer
    +--> run and encounter reducers
    +--> bundled plugins
    +--> community plugins
    +--> local overlays and integrations
    +--> private log writer
    +--> sanitized submission builder
              |
              v
         local draft queue
```

There is no legacy parser running beside it. Offline replay passes through the
same reconstruction, decode, event, and reducer interfaces as live capture.
The sealed canonical event stream is also the authoritative combat submission;
clients may preview calculations, but the service replays the evidence and
owns ranked results. See [Canonical timeline](CANONICAL_TIMELINE.md).

The local draft queue is host infrastructure, not a network client. A real
completed capture is verified once, described by exact full-file and
canonical-event SHA-256 values, split into deterministic chunk descriptors,
and atomically persisted as one bounded entry per artifact digest. Reference
fixtures are not queued. The host-only `.rlog` path never enters the upload
manifest, and no draft is transmitted until a separately enabled uploader and
user-authorized device transport exist.

The desktop exposes Log Uploader and BPSR Profile Sync as separate first-party
workspaces with independent, disabled-by-default policies. The policy store is
host-owned and atomically replaced. Enabling one never grants the other.
Before HTTP or authentication exists, Log Uploader can exercise the exact
resumable lifecycle against a bounded in-process receiver: both sender and
receiver are serialized and restored mid-upload, chunk acknowledgements and
the final receipt are validated, and no external request or artifact deletion
can occur.

Profile Sync replays the same fully sealed canonical format but consumes only
personal-gameplay character observations from the trusted game integration.
The BPSR projector merges partial local-character patches, excludes public
social lookups for other characters, and passes the result through Core's
credential/account-field rejection. Core wraps the game-owned relative request
with sealed-log source evidence and a deterministic digest. The host atomically
stores one current package per game/deployment/region/server/character UID and
revalidates it before exact JSON inspection.

Existing sealed logs use the same one-pass verifier as newly recorded logs, so
recovery does not create a weaker artifact class. Re-verification is
intentionally ephemeral: the future transport must repeat it immediately
before reading upload chunks rather than trusting a saved checkbox or
timestamp.

## Game-neutral Core

Only responsibilities that must be consistent and security-sensitive belong
in the reusable Core:

- cross-platform capture-source interfaces;
- allocation-conscious network decoding and bounded TCP reconstruction;
- selection of one trusted game integration and the versioned reconstructed
  stream handoff to it;
- canonical event ordering and provenance;
- plugin isolation, permissions, lifecycle, and resource limits;
- folder-package discovery, shared read-only resources, dependency resolution,
  and deterministic operation hooks;
- local log integrity;
- game-neutral website envelopes, relative-route validation, credential-field
  rejection, and authenticated transport.

Core contains no game names, executable names, framing rules, opcodes, route
catalogs, region endpoints, profile fields, or game-data IDs.

Core is the only network-capture owner. A game plug-in cannot open a second
capture path or silently broaden the process/connection filter. This mirrors
ACT's host-and-plug-in experience while keeping capture behavior identical
across games and platforms.

## Trusted game plug-ins

Each game integration is a privileged native plug-in because packet decoders
must receive reconstructed game streams. It owns:

- executable/process selectors;
- message framing, protocol decryption when a game requires it, bounded
  decompression, protocol packs, route and opcode knowledge;
- region/build resolution and selective decoders;
- game-data catalogs and sanitized mapping inventories;
- typed character profiles;
- projection into Core's privacy-reviewed website request.

The BPSR implementation is
`plugins/games/blue-protocol-star-resonance/`. A future game adds another
sibling folder without modifying the game-neutral Core contracts.

User-facing analysis belongs in plugins whenever practical.

## Plugin model

Bundled plugins and third-party plugins use the same versioned API. Initial
runtime targets are:

- sandboxed WebAssembly components for analyzers and integrations;
- browser overlays using a local event API;
- external-process plugins using authenticated local IPC;
- explicitly enabled native developer plugins, marked unsafe.

Normal plugins receive canonical events, never credentials, login payloads,
or unreviewed raw packets. They are not trusted game plug-ins. Protocol
research remains local to a game integration and cannot publish raw evidence.
Credential, account-authentication, and login-token fields are prohibited even
inside a game integration's normal decoder surface.

Community packages are installed as directories under `plugins/installed/`.
`plugin.toml` is only the declaration layer; the same directory owns its
entrypoint and small resources. Game-neutral private resources use the
host-derived `assets/rlogs/plugins/<plugin-folder>/` namespace. Provider-owned
resources intended for reuse use
`assets/rlogs/shared/<provider-plugin-folder>/`. Game integrations keep their
resources under `assets/<game-id>/`. Published
resources retain a single owner and other plug-ins import them by owner ID,
resource name, schema ID, and minimum schema version. The host provides
read-only access rather than copying the data or letting manifests choose
another provider's filesystem namespace.

Hooks target a named operation stage and run either before or after Core.
Dependencies, explicit `before`/`after` edges, and numeric priority produce one
deterministic topological order. Ordering cycles are rejected. Presentation
features such as UID-based locale aliases use an `after_core`
`localization_lookup` hook and cannot mutate canonical IDs or log evidence.

Chat events are local-sensitive canonical events. A plugin must receive a
separate chat-read grant, and submission builders reject them. Direct/private
message routes remain prohibited.

The plugin API version, canonical event schema version, `.rlog` format version,
and protocol-pack version evolve independently. A game update should normally
require a new protocol pack, not changes to every plugin.

## First-party plugins

Native product features are implemented as bundled plugins where the security
boundary allows:

- combat meter;
- encounter recorder;
- character profile projection;
- rDPS and support attribution;
- overlays;
- log uploader.

Capture, TCP reconstruction, website transport, and ordinary add-on
permissions remain Core services. Game decoding and game-owned privacy
allowlists live in the selected trusted game plug-in; Core still applies
cross-game credential/account-field rejection at the website boundary.

## Cross-platform implementation

Rust is the primary core language because it provides memory safety,
predictable performance, and first-class Windows and Linux support. The
default live adapter uses the portable pcap API through Npcap on Windows and
libpcap on Linux. Offline pcap and pcapng replay must work without elevated
privileges on both systems. See [Packet capture decision](CAPTURE.md).

Plugin APIs are language-neutral. SDKs may be provided for Rust, TypeScript,
Python, C#, Go, and other languages without moving packet processing into
those runtimes.
