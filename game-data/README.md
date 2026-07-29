# Human-readable game data

Game data is owned by trusted game plug-ins. The active BPSR catalog lives at
[`plugins/games/blue-protocol-star-resonance/game-data/catalog/`](../plugins/games/blue-protocol-star-resonance/game-data/catalog/).
This document defines the organization and performance rules shared by every
game catalog.

This tree contains reviewed end products only. It must never contain game-file
locators, extraction code, decryption code, raw dumps, temporary databases, or
commands used to acquire the data.

Canonical definitions live once in a shared catalog. A deployment name such as
`global` or `cn` appears only in build-availability metadata; it is not a
player region and does not create a parallel skill/entity tree.

```text
game-data/
  catalog/
    manifest.json
    promotion-summary.json
    classes/
      <class>/<class-id>-<readable-name>.json
    specializations/
      <class>/<spec>/<spec-id>-<readable-name>.json
    skills/
      <class>/<spec>/<skill-id>-<readable-name>.json
    skill-effects/
      <class>/<spec>/<effect-id>-<readable-name>.json
    recount-groups/
      <recount-id>-<readable-name>.json
    profile-images/
    name-cards/
    medals/
    guild-icons/
    status-effects/
      <class>/<spec>/<effect-id>-<readable-name>.json
    monsters/
      <family-or-type>/<monster-id>-<readable-name>.json
    npcs/
      <role>/<npc-id>-<readable-name>.json
    scenes/
    maps/
    dungeons/
      <play-type>/<dungeon-id>-<readable-name>.json
    items/
    equipment/
      models/
    weapon-equipment/
    equipment-sets/
    imagines/
    professions/
    talents/
    cosmetics/
    icons/
      classes/<class>/horizontal.png
      guilds/badges/<guild-icon-id>-<readable-name>.png
      skills/<class>/<spec>/<skill-id>-<readable-name>.webp
      status-effects/<class>/<spec>/...
      monsters/<family>/...
      equipment/<slot>/...
      imagines/...
    localization/  # mapping-stage source; migrates to locale add-ons
      en-US/<domain>/<readable-shard>.json
      zh-CN/<domain>/<readable-shard>.json
      ja-JP/<domain>/<readable-shard>.json
```

## Strict rules

1. One canonical symbol is stored once. Every record and official localization
   value lists the exact deployment/channel/client builds where that content
   was reviewed.
2. One reviewed symbol lives in one JSON file with its numeric ID in the
   filename.
3. Skill paths must include class and specialization folders. Their
   `class_key` and `spec_key` fields must match those folders.
   `shared` means class ownership is proven but specialization ownership is
   not; it is not a guess or a catch-all alias for `unclassified`.
4. Numeric IDs, stable keys, localization keys, and asset paths must be unique.
   Conflicts fail the build; later files never overwrite earlier files.
5. Every record carries build availability, a source reference, and
   confidence. References identify reviewed static sources, not private
   extraction procedures.
6. Icons use searchable paths matching the record hierarchy. Missing,
   duplicated, unsafe, and unreferenced icon paths fail validation.
7. Official game localization is mapped to stable language-neutral RLogs keys.
   Build availability stays on each value, so languages remain shared without
   hiding client differences. During mapping, localization remains beside the
   records so the compiler can enforce full reference coverage. After stable
   domain mapping, the same JSON entries move into data-only locale add-ons
   under
   `plugins/builtin/localization/<locale>/games/<game-plugin-id>/game/`; IDs
   never move with them.
8. Unknown or disputed values stay in the research ledger until reviewed.
   They are not promoted by guessing.
9. Runtime capture never walks this tree. `rlogs-game-data-build` validates and
   compiles it into independently digested zstd shards. Runtime loads only the
   needed ID bucket and selected-locale bucket through a bounded cache.
10. A game plug-in's sanitized `research/game-file-inventory/` is a research
    index, not approved runtime data. Promotion into its `game-data/catalog/`
    always requires schema and provenance review.
11. Player region remains capture/submission identity. It is deliberately not
    a static game-data folder or a field in the catalog manifest.

The validator/compiler is in `tools/game-data-build`. It consumes these end
products but performs no extraction.
