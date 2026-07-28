# Capture

This crate defines the one platform-neutral input boundary used by live
capture and offline replay.

Platform adapters normalize frames without interpreting game messages:

- the shared pcap API through Npcap on Windows;
- the shared pcap API through libpcap on Linux;
- pcap and pcapng replay;
- native RLogs evidence replay.

TCP reconstruction and game protocol decoding are downstream responsibilities.
The same frame stream must produce the same result regardless of its adapter.
Optional sources may be added later, but they feed this same boundary and do
not create another parser.

## Offline replay

`OfflineCapture` currently streams legacy pcap and pcapng without requiring a
native capture driver. It preserves:

- packet order and original wall-clock timestamps;
- deterministic monotonic replay time;
- pcapng interface identity and per-interface link type;
- original on-wire length when a capture was truncated;
- raw link-layer frame bytes.

Non-packet pcapng metadata—including name-resolution and decryption-secret
blocks—is ignored and never exposed as a captured frame.
