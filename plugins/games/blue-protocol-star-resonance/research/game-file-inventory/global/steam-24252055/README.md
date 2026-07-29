# Global Steam 24252055 game-file inventory

This is the sanitized, build-scoped inventory produced from a read-only client
scan. It records locations, identities, counts, digests, and relationship
evidence needed by RLogs. It contains no raw game payloads, absolute install
paths, extraction code, packet captures, credentials, login/account payloads,
private messages, or anti-cheat decoding.

The complete private research inventory remains outside this repository. These
files are compact review indexes, not runtime parser data. Reviewed symbols are
promoted separately into this plug-in's `game-data/catalog/` with this client build
recorded as availability metadata.

Start here:

- `scan-summary.json`: coverage and limitations;
- `source-map.json`: relative client locations and what each source provides;
- `tables/index.json`: table naming and shard counts;
- `tables/named/`: human-readable table catalogs grouped by domain;
- `tables/unknown/`: every unresolved table retained by hash;
- `schemas/`: packed-field evidence, corroborated semantics, and the review
  worklist for all named tables;
- `relationships/summary.json`: exact versus candidate edge policy;
- `relationships/schema-relations.json`: compact typed-field and pool-relation
  counts without duplicating schema rows;
- `relationships/id-namespaces.json`: identity keys that keep CTB rows,
  damage IDs, character UIDs, static entity definitions, and runtime UUIDs
  separate;
- `physical/files/`: all 741 physical files grouped by purpose;
- `research-priorities.json`: the mapping order for parser/profile work.

No file in this inventory is loaded by the live packet parser.
