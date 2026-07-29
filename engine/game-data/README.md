# Game-data engine

This crate loads compact, validated end products through deterministic shards.
It does not locate, inspect, decrypt, or extract game files.

Human-maintained source folders and their validator/compiler live at the
workspace root under `game-data/` and `tools/game-data-build/`.

Runtime lookups compute a bucket from `(symbol kind, numeric ID)`, stable key,
or `(locale, localization key)`. Only that compressed shard is opened. Returned
records use `Arc`, so bounded least-recently-used eviction cannot invalidate a
caller. Default cache ceilings are 128 shards, 64 MiB resident weight, and
8 MiB per compressed or uncompressed shard.

The bundle is a shared catalog, not a regional pack. Build-aware record and
localization lookups check deployment/channel/client-build availability.
Player region continues to be handled by capture and submission identity.

The manifest and every compressed/uncompressed shard have independent SHA-256
digests. Relative paths are traversal-checked and canonicalized beneath the
bundle root. Decompression stops at the declared byte length.
