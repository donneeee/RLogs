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
lossless wire-fragment journal
    |
    v
region/build protocol pack
    |
    v
versioned canonical events
    |
    +--> run and encounter reducers
    +--> bundled plugins
    +--> community plugins
    +--> local overlays and integrations
    +--> private log writer
    +--> sanitized submission builder
```

There is no legacy parser running beside it. Offline replay passes through the
same reconstruction, decode, event, and reducer interfaces as live capture.

## Trusted core

Only responsibilities that must be consistent and security-sensitive belong
in the trusted core:

- cross-platform capture-source interfaces;
- allocation-conscious network decoding, bounded stream reconstruction,
  framing, and bounded decompression;
- protocol-pack selection and decoding;
- region and client-build evidence;
- canonical event ordering and provenance;
- plugin isolation, permissions, lifecycle, and resource limits;
- local log integrity and submission allowlisting.

User-facing analysis belongs in plugins whenever practical.

## Plugin model

Bundled plugins and third-party plugins use the same versioned API. Initial
runtime targets are:

- sandboxed WebAssembly components for analyzers and integrations;
- browser overlays using a local event API;
- external-process plugins using authenticated local IPC;
- explicitly enabled native developer plugins, marked unsafe.

Normal plugins receive canonical events, never credentials, login payloads, or
unreviewed raw packets. Protocol research is a separate developer capability
and cannot publish raw evidence.

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

Capture, decoding, privacy enforcement, and plugin permissions remain core
services rather than plugins.

## Cross-platform implementation

Rust is the primary core language because it provides memory safety,
predictable performance, and first-class Windows and Linux support. The
default live adapter uses the portable pcap API through Npcap on Windows and
libpcap on Linux. Offline pcap and pcapng replay must work without elevated
privileges on both systems. See [Packet capture decision](CAPTURE.md).

Plugin APIs are language-neutral. SDKs may be provided for Rust, TypeScript,
Python, C#, Go, and other languages without moving packet processing into
those runtimes.
