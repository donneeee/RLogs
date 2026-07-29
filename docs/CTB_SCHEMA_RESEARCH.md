# CTB schema research

Global Steam build `24252055` now has a build-aware inference record for every
named CTB table. This is the layer between the complete file inventory and
reviewed game data used by the parser.

## Current result

| Measure | Result |
| --- | ---: |
| Named tables evaluated | 565 / 565 |
| Rows evaluated | 260,189 |
| CTB bytes evaluated | 25,918,637 |
| Structurally confirmed row-key fields | 548 |
| Total corroborated semantic fields | 864 |
| Rejected/unresolved semantic seeds | 52 |
| Exact local-pool reference candidates | 1,448 |
| Exact candidates at unaligned offsets | 293 |
| Primitive candidates retained privately | 6,476 |

The unaligned results matter: CTB rows are packed. A scanner that advances only
in four-byte steps misses real fields. RLogs evaluates every byte offset and
uses alignment only as supporting evidence.

The publish gate rejects 52 assumptions. Eighteen are older assumptions: 17
named tables do not have a unique, nonzero row key at offset zero, and one
seeded combat-detail field no longer satisfies its current-build invariant.
Seventeen are candidate world/entity fields, and 17 are combat/support fields
whose current-build contents do not yet provide enough nonzero exact
references, pool boundaries, or plausible vector evidence. All remain private
review evidence and are absent from the sanitized `semantic_fields` output.

## Confirmed pool models

The complete 4,239-table corpus establishes the length-prefixed pool shapes
before individual row fields are typed:

| Pool | Nonempty tables | Element model | State |
| ---: | ---: | --- | --- |
| 1 | 3,311 | 4-byte integer arrays | structurally confirmed |
| 2 | 3 | 8-byte integer/UID arrays | structurally confirmed |
| 3 | 3,742 | 32-bit float arrays | structurally confirmed |
| 4 | 1 | opaque 8-byte elements | width confirmed, meaning unresolved |
| 6 | 3,262 | UTF-8 byte strings | structurally confirmed |
| 7 | 10 | two-float vectors | structurally confirmed |
| 8 | 7 | three-float vectors | structurally confirmed |

Pools 5, 9, and 10 are empty in this build. Pool 4 stays explicitly opaque;
its eight-byte width is not permission to invent a numeric or relationship
meaning.

## Evidence states

- `current_build_structural` identifies the unique primary row key at offset
  zero.
- `current_build_static_corroborated` means current-build table content and
  structure agree on a field meaning.
- `corroborated_reference_current_build` means a reviewed public parser
  reference supplied the meaning and the current build still satisfies its
  invariant.
- `strong_candidates` resolve exactly to records in the same CTB string or
  integer-array pool. They are useful discovery evidence, but are not assigned
  semantic names automatically.
- Localization membership and cross-table row-ID membership remain candidates.
  Shared integers do not become foreign keys by coincidence.

## First reviewed profile and equipment slice

The first static review confirms:

- profile-image row ID, display-name localization, and acquisition text;
- profile-attribute row ID, attribute label, and attribute-group label;
- medal ID, name, acquisition text, and icon name;
- name-card ID, name, acquisition text, and its three background paths;
- guild/union icon ID and icon path;
- equipment-part ID, label, and icon path;
- equipment ID and model path;
- weapon-equipment ID and design name;
- fashion ID and model name;
- weapon-skin ID, localized name, and model name;
- vehicle ID, localized name/description, icon/preview references, and movement
  curve references.

The second profile slice adds:

- profile-image objective ID and localized objective text;
- item-type ID and label;
- player-level and season-level keys;
- profession ID, localized name/description, theme color, and class/talent/
  weapon icon references;
- fashion collection level, required points, and reward item relation.

These are static definitions, not proof that a character owns or has equipped
an entry. Ownership and loadout fields must be correlated with consented
character-facing runtime routes. Login credentials, account authentication,
tokens, and private communications remain out of scope.

## First reviewed world and entity slice

The current-build world pass now confirms:

- scene IDs, names, type/subtype, parent/resource relations, packed map size
  and offset vectors, spawn point, revive/music lists, environment settings,
  minimap settings, ambient/reverb events, area, movement sync, and water;
- dungeon IDs, names/descriptions/type labels, function/play/scene selectors,
  entry rules and limits, team/solo selectors, timers, recommended monster
  power, and result HUD mode;
- map design names/background paths, scene-area names/audio events, dungeon
  stage/title text, and raid names/images/tips;
- monster IDs, names, models, type/size, health-bar count, skill and aggro
  arrays, movement/alert values, camp, HUD parameters, and turn velocity;
- NPC IDs, names, model design names, map icons, and role labels;
- static monster, NPC, scene-object, and zone entity-definition IDs plus their
  float-array positions, and zone size vectors.

An entity-definition ID from a static CTB is not a live entity UUID. Dynamic
UUIDs, owners, summons, and per-instance state must come from packet/runtime
evidence and are never inferred from these static row keys.

## First reviewed combat and support slice

The combat pass corrects and expands the static definitions for:

- skill identity, level group, effect list, targeting/damage modes, airborne
  behavior, break/armor flags, charge/cooldown settings, slot rules, and other
  packed behavior fields;
- skill-effect identity, parent skill, range arrays, tags, attribute
  descriptions, learned/installed buff relations, and battle-state flags;
- skill fight-level identity, skill/effect/level relation, costs, resource
  checks, cooldowns, and fight value;
- buff identity, level, localized name/description, type/priority/visibility,
  stacking rule, deletion flags, ability type/subtype, and source skill slot;
- damage identity, design name, linked source, and damage-kind label;
- fight and temporary attribute identities, numeric limits, composition
  relations, client/AOI sync policy, and localized descriptions;
- module and module-effect identity, names, icons, effect configuration,
  levels, negative/shield flags, and link/decomposition relations.

The prior `SkillTable` offset-16 label was corrected from `parent_skill_id` to
`skill_level_group_id`. The central recount relation is also now typed as:

```text
recount_id -> localized name -> signed 64-bit damage_id[]
```

This is a naming/grouping relation, not an rDPS formula. Support attribution
still needs runtime provider, recipient, timing, stat/mitigation delta, damage,
and overlap evidence. Uncertain buff arrays remain in the review queue rather
than being silently treated as reportable support.

## Human-readable layout

The sanitized results live at:

```text
plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24252055/schemas/
  README.md
  index.json
  review-worklist.json
  domains/
    character-profile.json
    items-and-equipment.json
    social.json
    world-and-instances.json
    entities.json
    combat.json
    ...
```

Raw rows, resolved private samples, absolute installation paths, and extraction
tools stay outside the public repository.

## Update rule

Each client build receives a separate schema inventory. A field is carried
forward only when its table identity, source digest, row shape, and validation
invariant still match. Changed fields return to candidate review instead of
silently inheriting old offsets.
