# Game-data validator/compiler

This tool accepts reviewed end products only. It does not contain game-file
discovery, extraction, decryption, or conversion logic.

```text
cargo run -p rlogs-game-data-build -- <source-build-folder> <compiled.json>
```

The build fails on duplicate IDs, duplicate stable keys, mismatched
class/spec folders, missing icons, unreferenced icons, locale mismatches,
unknown top-level folders, or invalid digests.
