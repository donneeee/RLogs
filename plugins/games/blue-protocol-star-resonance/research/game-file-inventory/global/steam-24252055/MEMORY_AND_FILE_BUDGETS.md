# File and memory budgets

- Private research shards target less than 16 MiB; the current largest shard is
  below 9 MiB.
- Sanitized inventory shards target less than 2 MiB and are never loaded by the
  live parser.
- Human-reviewed source records stay one symbol per JSON file under
  this plug-in's `game-data/catalog/`, with exact client-build availability on
  each record.
- Runtime output uses independently digested, compressed shards.
- Record lookup computes a shard from kind plus numeric ID.
- Localization lookup computes a shard from locale plus key hash; only the
  selected locale is eligible for loading.
- Icon/media bytes are filesystem resources and are never embedded in the core
  lookup index.
- Runtime caches have explicit byte and shard limits. Eviction is deterministic;
  nothing grows for the lifetime of a capture without a ceiling.
- A build switch selects availability metadata in the shared catalog. Evidence
  from two client builds is never silently treated as the same definition.
- Unknown and disputed mappings stay in research catalogs; runtime artifacts
  contain reviewed records only.
