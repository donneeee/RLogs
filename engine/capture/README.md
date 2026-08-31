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
not create another parser. Trusted game plug-ins consume the resulting
process-filtered streams; they do not capture traffic themselves.

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
written to a raw capture or journal. `NpcapLiveCapture` opens the trusted system
Npcap API directly; `DumpcapLiveCapture` remains an optional compatibility
pipe. `OwnedProcessCapture` is the mandatory privacy and persistence boundary
around either source.

Live sessions expose a cooperative stop handle. Native Npcap checks it at the
bounded read timeout; the compatibility adapter terminates only its private
dumpcap child. The ownership filter then refreshes and drains its bounded queue
before the shared file recorder
flushes and atomically publishes the process-owned PCAP and exact connection
evidence. The command-line capture tool and localhost host use this same
recorder rather than maintaining separate persistence implementations.

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
