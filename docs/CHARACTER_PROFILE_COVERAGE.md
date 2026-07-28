# Automated character-profile coverage

This census tracks what RLogs can expose about a character and what still
needs current-build verification. Historical parser fields are hypotheses,
not proof that the current client uses the same schema.

Reference pins:

- historical Resonance Logs:
  `resonance-logs/resonance-logs@f4aff36e573674e04db1bb09216c603ddf9fb7f6`
- Global feature/evidence baseline:
  `donneeee/resonance-logs-global@77380cabdc8505267a8971022e38859b9400dd28`
- CN 0.2.0:
  `fudiyangjin/resonance-logs-cn@ccdeef23c7806be5072f95a9e80b103794af3544`

## Field census

| Character surface | Canonical field | Current state | Default website policy |
| --- | --- | --- | --- |
| deployment, region, realm, world | `CharacterIdentity.region` | contract implemented; automatic evidence still expanding | required |
| stable character ID | `character.character_id` | selective decoder and fixture implemented; live build verification required | required |
| display ID | `display_id` | selective decoder and fixture implemented | opt-in public |
| character name | `display_name` | selective decoder and fixture implemented | opt-in public |
| server ID | `server_id` | selective decoder and fixture implemented | required for region/world resolution |
| class/profession ID | `class_id` | historical/current-reference field implemented; live verification required | public |
| specialization | `specialization_id` | observed through skills, talents, and buffs in Global; projection rule pending | public |
| character level | `level` | selective decoder and fixture implemented | public |
| character experience and prior-season max level | `progression` | historical schema located; current mapping not declared | public |
| combat/fight power | `combat_power` | historical/current-reference field implemented; live verification required | public |
| season strength | `season_strength` | current Global attribute candidate; profile correlation pending | public |
| season ID, level, and power | `season` | canonical contract implemented; current packet/game-data mapping needed | public |
| gender, body size, face choices, colors, and height | `appearance` | historical character-facing schema located; current mapping not declared | explicit opt-in |
| avatar, card style, and frame IDs | `appearance` | historical schema located; image URLs deliberately excluded | opt-in public |
| equipped gear | `equipment` | canonical contract implemented; current complete equipment schema needed | public |
| refinement, enchantments, and sets | nested equipment fields | Global evidence exists; current schema/game-data relations needed | public |
| owned/equipped Imagines | `owned_imagines` | canonical contract and extraction requirement implemented; current source mapping needed | opt-in public |
| active skills and levels | `active_skills` | Global packet evidence exists; stable profile projection pending | public |
| talents and levels | `talents` | Global packet evidence exists; stable profile projection pending | public |
| combat-profession levels, experience, loadouts, skins, and talent nodes | `combat_professions` | detailed historical schema located; current mapping not declared | public |
| life-profession levels, experience, and specializations | `life_professions` | detailed historical schema located; current mapping not declared | opt-in public |
| owned cosmetics | `cosmetics` | canonical contract implemented; source mapping needed | opt-in public |
| fashion, mount, and weapon-skin ownership/collection points | `collection_summary` | detailed historical schema located; current mapping not declared | opt-in public |
| guild, titles, and displayed medals | `social_display` | historical schema candidates located; current semantics need verification | opt-in public |
| panel combat stats | future typed stat snapshot | many Global attribute IDs observed; current formula/unit verification required | public per log/build |
| current position | timeline `Position` event | decoder implemented for entity attributes | never part of public profile by default |

## What the historical parser actually described

The pinned historical schema included character-facing structures for:

- character/show/server IDs, name, gender, body size, face selections and
  colors, avatar ID, business-card style, avatar frame, level, experience, and
  fight power;
- equipment slots, item instance UUIDs, refinement levels and failure counts,
  recast attributes, enchantments, and suit/set attributes;
- current combat profession, profession levels and experience, active and
  slotted skills, skill levels/remodels/skins, talent points, and talent nodes;
- life-profession levels, experience, specializations, recipes, and targets;
- equipped and owned fashion, dyes, advanced variants, mounts, weapon skins,
  and their collection totals;
- seasonal IDs, progression systems, medals/nodes, profile unlocks, guild
  display, titles, and medals.

This is a research inventory, not a declaration that those old tags remain
correct. RLogs does not import that generated schema. Each useful field must be
rediscovered and proven for an exact current client build.

## Additional profile surfaces worth investigating

- equipped and owned Imagine levels, breakthroughs, passives, and slots;
- gear instance IDs, base items, slot refinement, recast rolls, enchantments,
  suit/set bonuses, quality, and effective item level;
- specialization selectors, passive ownership, active skill levels, cooldown
  configuration, talent nodes, and seasonal cultivation choices;
- character level/experience, seasonal level/strength/power, rank/reduction
  levels, and other progression systems;
- combat-power components and complete panel-stat snapshots;
- profession levels, profession equipment, and profession skill progression;
- avatars, frames, titles, cards, fashion, dyes, mounts, and other cosmetics;
- achievement/collection summaries that are explicitly character-facing;
- profile revision evidence so website updates are idempotent and ordered.

## Explicit exclusions

The selective character decoder does not declare or materialize protobuf tags
for account ID, open ID, SDK account type, login/session state, credentials,
tokens, email, payment data, or account-security fields. Those values are not
needed to build a character profile.

The website profile contract also excludes OS/client telemetry, moderation
flags, online/offline timestamps, total playtime, private messages, arbitrary
user-supplied image URLs, and precise location history. These may coexist with
character-facing data in a source message, but proximity does not grant upload
permission.

## Promotion rule

A field becomes `verified` only after its exact deployment/build, route,
protobuf field or attribute ID, units, update semantics, and privacy/upload
policy are all recorded. Fixtures prove decoder behavior; a current sanitized
capture proves the mapping.
