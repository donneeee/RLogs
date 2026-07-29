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

## Live ownership filtering

The live adapter contract includes process ownership, not only a startup BPF
expression. World-server endpoints can rotate during a scene transition.
Windows uses continuous process/socket-table attribution with executable names
provided by the selected trusted game plug-in; the Linux
adapter will provide the equivalent attribution through its native
process/socket view.

Only an exact flow confirmed as game-owned may leave the bounded capture
ingress. Unattributed frames are held briefly for connection-table race
resolution and then discarded. They are never reconstructed, decoded, or
written to a raw capture or journal. `DumpcapLiveCapture` writes only to a pipe;
`OwnedProcessCapture` is the mandatory persistence boundary around it.

## Offline replay

`OfflineCapture` currently streams legacy pcap and pcapng without requiring a
native capture driver. It preserves:

- packet order and original wall-clock timestamps;
- deterministic monotonic replay time;
- pcapng interface identity and per-interface link type;
- original on-wire length when a capture was truncated;
- raw link-layer frame bytes.

Frame storage is immutable and reference-counted. Offline replay copies each
packet once out of the streaming parser's temporary buffer; downstream network
decoding takes shared byte-range views without copying payloads again.

Non-packet pcapng metadata—including name-resolution and decryption-secret
blocks—is ignored and never exposed as a captured frame.
