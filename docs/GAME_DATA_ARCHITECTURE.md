# Game-data and recount architecture

RLogs separates research evidence, reviewed human sources, and runtime data.
They solve different problems and must never be collapsed into one giant file.

```text
private read-only scan
  -> sanitized research inventory
    -> reviewed human game-data records
      -> compact deterministic runtime shards
```

## Research tables

Every source row keeps its build identity:

```text
(deployment, client build, table key, row ID)
```

IDs with different meanings are stored in distinct typed columns. A CTB row
ID, skill ID, entity UID, entity UUID, scene ID, asset-address hash, bundle
hash, localization ID, and opcode are not interchangeable merely because they
fit in the same integer width.

The build-specific machine-readable identity rules live in
`plugins/games/<game>/research/game-file-inventory/<deployment>/<build>/relationships/id-namespaces.json`.
The adjacent `schema-relations.json` summarizes validated field and exact-pool
evidence while pointing back to the canonical schema shards instead of
duplicating their contents.

The research layer uses four related datasets:

1. **Source index** — one row per observed source row or asset identity, with
   build, table/hash, source location, digest, and exact provenance.
2. **ID recount** — mechanical counts of where an ID/value appears, including
   unknown and disputed evidence.
3. **Classification** — reviewed semantic meaning, domain, confidence, and
   allow/prohibit policy.
4. **Relations** — typed edges with source, target, evidence class, build
   validity, and optional runtime corroboration.

Recount is evidence, not meaning. Common values such as `0`, `1`, and reused
row IDs can appear in hundreds of tables; they remain candidates until schema
or runtime behavior distinguishes them.

## Human source tree

Approved skills remain easy to browse:

```text
plugins/games/<game>/game-data/catalog/skills/<class>/<spec>/<id>-<name>.json
```

The same rule applies to classes, statuses, monsters, scenes, maps, dungeons,
items, equipment, Imagines, professions, talents, and cosmetics. Icon paths
mirror the domain below
`assets/shared/<game-plugin-folder>/icons/`; records retain short paths such as
`icons/skills/<class>/<spec>/<file>`. During mapping, localization is grouped
by locale and visible domain beside the catalog so reference validation
remains exhaustive; one file may contain a reviewed array so the repository
does not require millions of tiny files. Once domains stabilize, those same
entries move into the data-only add-ons under
`plugins/builtin/localization/<locale>/games/<game-plugin-id>/game/`, alongside
that locale's separate `ui/` namespace. Exact
deployment/channel/client-build availability lives on each canonical record
and official game string. Player regions never split either tree.

## Runtime bundle

`rlogs-game-data-build` validates source uniqueness and produces:

```text
manifest.json
records/<kind>/<ID-bucket>.json.zst
record-keys/<key-hash-bucket>.json.zst
localization/<locale>/<key-hash-bucket>.json.zst
assets/<key-hash-bucket>.json.zst
```

Lookups compute their shard without loading a global index. The runtime cache
is bounded by shard count, compressed/uncompressed shard size, and resident
weight. This combined localization output is the mapping-stage compiler
contract. The locale add-on compiler will publish the same independently
digested locale shards separately so only the selected add-on and explicit
fallback are opened. Other locales and icon bytes are not loaded implicitly.
Each shard and manifest has an independent digest, and decompression cannot
exceed the declared length.

## Client updates

A new client build gets a new build-scoped research inventory. Diffing happens
by table key, source digest, row set, shape, localization coverage, and
reviewed relations. Unchanged canonical records gain the new build in their
availability metadata after explicit digest-backed review. Changed or unknown
rows return to the research worklist instead of silently overwriting shared
content.

The catalog is universal, while capture and submission identity remains:

```text
deployment + region + world + character UID
```

That separation permits automatic leaderboard region routing without
duplicating skills, monsters, or maps per region.
