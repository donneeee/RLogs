# rLogs replay format

This crate writes and streams the versioned `.rlog` container. A file contains
one header, ordered canonical event records, and one integrity seal. It never
contains raw packets, credentials, login payloads, private chat, or account
data.

Format v2 writes the header and integrity seal around independently compressed,
bounded zstd event blocks. Each block has explicit compressed, decoded, and
event-count limits, so replay remains incremental and corruption cannot request
an unbounded allocation. The SHA-256 seal is still calculated over the same
canonical event JSON plus newline; compact encoding does not alter, omit, or
reorder an event.

The reader auto-detects and replays legacy v1 newline-delimited JSON logs. New
writers default to v2, while fixtures may deliberately request v1 when testing
compatibility.

The reader delivers events incrementally. It does not load the entire dungeon
timeline into memory. Consumers may pull one event at a time, pause between
bounded pages, and receive the verified replay summary only after the seal and
end-of-file are accepted. Full plug-in replay uses that same incremental path.
