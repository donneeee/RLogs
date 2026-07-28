# Protocol engine

This crate owns RLogs' lossless, game-build-aware packet evidence.

It currently provides:

- directional BPSR route identities;
- immutable packet and capture-gap records;
- original wire bytes plus optional decompressed application bytes;
- append-only capture journals;
- bounded-memory streaming JSONL research journals;
- route catalogs with mapping provenance;
- packet, byte, known-route, and unknown-route coverage.

It deliberately does not decode protobuf messages yet. The next protocol slice
will connect protocol-pack route definitions to decoders without changing these
evidence types.

## Core rule

An unknown route is still a valid packet record. A queue drop, TCP gap,
decompression failure, or malformed frame is still a valid gap record.
Neither may disappear merely because the current decoder does not understand
it.

The JSONL stream is a transparent research format, not the final `.rlog`
container. It exists so live captures can be inspected and replayed while the
compact public log format is designed from real evidence.
