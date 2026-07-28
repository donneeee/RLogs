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
