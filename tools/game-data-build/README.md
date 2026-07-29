# Game-data validator/compiler

This tool accepts reviewed end products only. It does not contain game-file
discovery, extraction, decryption, or conversion logic.

```text
cargo run -p rlogs-game-data-build -- \
  --asset-root assets/shared/blue-protocol-star-resonance \
  plugins/games/blue-protocol-star-resonance/game-data/catalog \
  <compiled-folder>
```

The build fails on duplicate IDs, duplicate stable keys, mismatched
class/spec folders, missing icons, unreferenced icons, locale mismatches,
unknown top-level folders, an existing output folder, or any shard larger than
8 MiB uncompressed.

`--asset-root` points at the provider namespace whose `icons/` folder is
referenced by catalog records. It defaults to the catalog folder for compact
fixtures, but real game plug-ins keep reusable binary assets outside the code
package.

The output is a digested directory of zstd-compressed shards:

```text
manifest.json
records/<domain>/<id-bucket>.json.zst
record-keys/<key-hash-bucket>.json.zst
localization/<locale>/<key-hash-bucket>.json.zst
assets/<key-hash-bucket>.json.zst
```

Localization source files may contain one entry or a reviewed array. This
allows readable domain files without creating millions of tiny files.
The default 6-bit bucket layout balances direct lookup size against manifest
and filesystem overhead; every uncompressed shard still has a hard 8 MiB
ceiling.

The compiled bundle contains one shared catalog. Records and localization
values retain their reviewed client-build availability, and build-aware lookup
APIs filter them without creating region-specific copies.
