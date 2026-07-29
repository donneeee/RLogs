# Game-data research boundary

## Three distinct layers

1. **Private acquisition workspace** discovers and exports candidate end
   products. It is not part of RLogs.
2. **Human-readable reviewed game data** lives under each trusted game plug-in,
   currently
   `plugins/games/blue-protocol-star-resonance/game-data/catalog/`.
   Reviewers can navigate classes, specs, skills, monsters, maps, dungeons,
   icons, and official localization without opening generated indexes.
3. **Compiled runtime artifact** is produced by `rlogs-game-data-build`.
   Capture loads this digested artifact once and performs indexed lookups.

No extractor is a parser dependency. No raw game-file scan runs during packet
capture.

## Review lifecycle

Every discovery moves through:

```text
needed -> candidate -> corroborated -> verified -> compiled
```

A record is verified only against its exact deployment and client build.
Historical parsers may identify candidate field names, but cannot promote a
current record without fresh game-data or packet evidence.

## Organization and performance gates

- one shared source catalog with explicit per-record build availability;
- one symbol per readable JSON file;
- class/spec folders are mandatory for skills and class-owned effects;
- official localization and icons mirror the same hierarchy;
- duplicate IDs, keys, locale entries, paths, or conflicting assignments fail;
- unreferenced icons fail;
- compiled payloads are deterministically sorted and SHA-256 addressed;
- runtime indexes are built once and provide direct ID/key lookup;
- capture threads never parse source JSON or touch the source folder tree.

Build-specific evidence remains separated under `research/`. The reviewed
catalog is not separated by player region. Region still belongs to canonical
capture identity and future submission routing.

## UUID rule

Live entity UUIDs and static game-data IDs are not interchangeable. Canonical
events preserve the exact dynamic wire UUID plus a compact log-local actor ID.
Game-data records explain static monster, skill, scene, map, dungeon, item,
Imagine, and other IDs. Any build-specific UUID bit-layout interpretation
must be a reviewed game-data/protocol-pack fact with provenance.
