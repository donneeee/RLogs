# Network

This crate is the allocation-conscious bridge between captured link-layer
frames and the game protocol:

1. decode Ethernet, Linux cooked capture, loopback, and raw IP frames;
2. expose IPv4/IPv6 TCP segments without copying their payloads;
3. reconstruct each directional TCP byte stream deterministically;
4. report malformed input, IP fragmentation, gaps, retransmissions, and
   memory-pressure decisions as typed evidence.

The normal in-order path never creates a reorder buffer. Captured bytes are
owned once by `rlogs-capture`; TCP segments and stream chunks retain cheap
shared slices into that allocation.

See [Performance](../../docs/PERFORMANCE.md) for the budgets and benchmark
commands.
