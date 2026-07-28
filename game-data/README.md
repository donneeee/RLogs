# Human-readable game data

This tree contains reviewed end products only. It must never contain game-file
locators, extraction code, decryption code, raw dumps, temporary databases, or
commands used to acquire the data.

Each deployment/build gets an independent folder:

```text
game-data/
  global/
    <client-build>/
      manifest.json
      extraction-requirements.json
      classes/
        <class>/...
      specializations/
        <class>/<spec>/...
      skills/
        <class>/<spec>/<skill-id>-<readable-name>.json
      status-effects/
        <class>/<spec>/<effect-id>-<readable-name>.json
      monsters/
        <family>/<monster-id>-<readable-name>.json
      scenes/
      maps/
      dungeons/
        <dungeon>/<difficulty-or-objective>.json
      items/
      equipment/
      equipment-sets/
      imagines/
      professions/
      talents/
      cosmetics/
      icons/
        skills/<class>/<spec>/<skill-id>-<readable-name>.webp
        status-effects/<class>/<spec>/...
        monsters/<family>/...
        equipment/<slot>/...
        imagines/...
      localization/
        en-US/<same-domain-folders>/<id>-<readable-name>.json
        zh-CN/<same-domain-folders>/<id>-<readable-name>.json
        ja-JP/<same-domain-folders>/<id>-<readable-name>.json
```

## Strict rules

1. Deployment and exact client build are never mixed.
2. One reviewed symbol lives in one JSON file with its numeric ID in the
   filename.
3. Skill paths must include class and specialization folders. Their
   `class_key` and `spec_key` fields must match those folders.
4. Numeric IDs, stable keys, localization keys, and asset paths must be unique.
   Conflicts fail the build; later files never overwrite earlier files.
5. Every record carries source revision, reference, and confidence. References
   identify reviewed end products, not private extraction procedures.
6. Icons use searchable paths matching the record hierarchy. Missing,
   duplicated, unsafe, and unreferenced icon paths fail validation.
7. Official game localization stays scoped to deployment/build/locale and is
   mapped to stable language-neutral RLogs keys.
8. Unknown or disputed values stay in the research ledger until reviewed.
   They are not promoted by guessing.
9. Runtime capture never walks this tree. `rlogs-game-data-build` validates and
   compiles it into a digested artifact loaded into indexed memory once.

The validator/compiler is in `tools/game-data-build`. It consumes these end
products but performs no extraction.
