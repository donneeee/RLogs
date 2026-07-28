# Protocol engine

This crate owns RLogs' lossless, game-build-aware packet evidence.

It currently provides:

- directional BPSR route identities;
- immutable packet and capture-gap records;
- original wire bytes plus optional decompressed application bytes;
- append-only capture journals;
- bounded-memory streaming JSONL research journals;
- route catalogs with mapping provenance;
- exact-build protocol-pack selection and content digests;
- automatic region endpoint rules with ambiguity rejection;
- selective privacy-reviewed protobuf decoders;
- exact entity UUIDs paired with bounded log-local actor IDs;
- canonical scene, entity, attribute, position, combat, cooldown, revive, and
  character-profile drafts;
- packet, byte, known-route, and unknown-route coverage.

Unknown, opaque, and prohibited routes are never sent through the selective
decoders. Their local wire evidence remains available to protocol research.
The first reference pack is deliberately unselectable for live capture until
its routes are replay-verified against an exact current client build.

## Core rule

An unknown route is still a valid packet record. A queue drop, TCP gap,
decompression failure, or malformed frame is still a valid gap record.
Neither may disappear merely because the current decoder does not understand
it.

The JSONL stream is a transparent research format, not the final `.rlog`
container. It exists so live captures can be inspected and replayed while the
compact public log format is designed from real evidence.
