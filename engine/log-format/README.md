# rLogs replay format

This crate writes and streams the versioned `.rlog` container. A file contains
one header, ordered canonical event records, and one integrity seal. It never
contains raw packets, credentials, login payloads, private chat, or account
data.

The first format is newline-delimited JSON so fixtures remain inspectable and
language-neutral. Replay is still bounded: the reader caps line length and
event count, validates session/region/schema identity and monotonic sequences,
and verifies a SHA-256 digest over canonical event content before accepting the
seal.

The reader delivers events incrementally. It does not load the entire dungeon
timeline into memory. Consumers may pull one event at a time, pause between
bounded pages, and receive the verified replay summary only after the seal and
end-of-file are accepted. Full plug-in replay uses that same incremental path.
