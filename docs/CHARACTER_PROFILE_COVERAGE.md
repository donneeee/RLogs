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
| stable character ID | `character.character_id` | verified in Global Steam build 24252055 complete snapshot | required |
| display ID | `display_id` | verified in Global Steam build 24252055 complete snapshot | opt-in public |
| character name | `display_name` | verified in Global Steam build 24252055 complete snapshot | opt-in public |
| server ID | `server_id` | decoder implemented; absent from the verified complete snapshot | required for region/world resolution |
| class/profession ID | `class_id` | verified in the complete snapshot; exact-build profession names, descriptions, icons, and theme references statically corroborated | public |
| specialization | `specialization_id` | observed through skills, talents, and buffs in Global; projection rule pending | public |
| character level | `level` | verified in the complete snapshot; `PlayerLevelTable` primary level key statically corroborated | public |
| character experience and prior-season max level | `progression` | historical schema located; current mapping not declared | public |
| combat/fight power | `combat_power` | verified in Global Steam build 24252055 complete snapshot | public |
| season strength | `season_strength` | current Global attribute candidate; profile correlation pending | public |
| season ID, level, and power | `season` | canonical contract implemented; season-level static keys corroborated, current character-state mapping still needed | public |
| gender, body size, face choices, colors, and height | `appearance` | historical character-facing schema located; current mapping not declared | explicit opt-in |
| avatar, card style, and frame IDs | `appearance` | all five `AvatarInfo` outer fields structurally verified; profile-image, objective, and name-card static definitions corroborated | opt-in public |
| game profile and half-body pictures | future reviewed image reference | URL tag and changed verification substructure verified without rendering values | explicit opt-in through reviewed CDN proxy |
| equipped gear | `equipment` | item/type, equipment part/model, and weapon-design definitions corroborated; owner loadout schema still needed | public |
| refinement, enchantments, and sets | nested equipment fields | current static candidate arrays located but intentionally unresolved; runtime schema/game-data relations needed | public |
| owned/equipped Imagines | `owned_imagines` | canonical contract and extraction requirement implemented; current source mapping needed | opt-in public |
| active skills and levels | `active_skills` | Global packet evidence exists; stable profile projection pending | public |
| talents and levels | `talents` | Global packet evidence exists; stable profile projection pending | public |
| combat-profession levels, experience, loadouts, skins, and talent nodes | `combat_professions` | profession display catalog corroborated; character progression/loadout mapping not declared | public |
| life-profession levels, experience, and specializations | `life_professions` | detailed historical schema located; current mapping not declared | opt-in public |
| owned cosmetics | `cosmetics` | fashion, weapon-skin, and vehicle definitions corroborated; ownership source mapping needed | opt-in public |
| fashion, mount, and weapon-skin ownership/collection points | `collection_summary` | fashion collection milestones corroborated; character totals and ownership remain unmapped | opt-in public |
| guild, titles, and displayed medals | `social_display` | medal/name-card/guild-icon definitions corroborated; guild identity and displayed role still need current runtime verification | public guild identity/display; opt-in titles and medals |
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

## Current Global complete-snapshot result

The process-aware `world-load-process-001` observation verified
`WorldNtf/SyncContainerData` on Global Steam build `24252055`. The selective
decoder produced one character-profile event with character UID, display UID,
display name, class ID, level, and combat power. The snapshot did not carry
server ID or scene data, so region/world and map context remain separate
evidence requirements.

The current character container has observed top-level tags through `121`.
Tags `102`, `103`, `104`, and `106` through `121` are present beyond the
historical reference ceiling and remain intentionally unmapped. They are a
current-client extraction backlog, not permission to copy old names onto new
fields.

The character-base section also contained tags `2` and `27`, historically
associated with account ID and open ID. RLogs does not declare either field in
its protobuf schema. The values were not rendered or retained, and tests inject
synthetic secrets to prove the canonical event path cannot serialize them.

Character-base tag `25` structurally matches the historical `AvatarInfo`
outer shape: avatar ID, profile picture, half-body picture, business-card
style, and avatar frame are all present. Both picture records have a URL tag
and verification record. The current verification records contain tags `3`
and `4`, rather than the historical `1`, `2`, and `3`, so their inner fields
remain unmapped. The audit reports presence only and never renders URLs.

## Current static-definition result

The exact-build CTB pass evaluates all 565 named tables and 260,189 named-table
rows without packet capture. It currently retains 864 semantic fields and
1,448 strong pool-reference candidates. Confirmed profile-facing definitions
include:

- profile-image names and acquisition text, profile-image objective text, and
  name-card background paths;
- profile-attribute labels and groups, medal names/acquisition text/icons, and
  guild-icon paths;
- profession names, descriptions, visual theme references, and class/talent/
  weapon icon families;
- item-type labels, equipment part labels/icons, equipment and weapon model
  references, fashion collection thresholds/rewards, weapon skins, and
  vehicles;
- player-level and season-level primary keys.

Static definition evidence never proves per-character ownership, equipped
state, progression, or selection. Those fields remain absent until a reviewed
current-build runtime source carries them. The exact status map is
`plugins/games/blue-protocol-star-resonance/protocol-references/profile-bridges/global-steam-24252055.json`;
the schema
inventory and unresolved worklist are under
`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24252055/schemas/`.

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
- reviewed game-hosted profile/half-body pictures, avatars, frames, titles,
  cards, fashion, dyes, mounts, and other cosmetics;
- achievement/collection summaries that are explicitly character-facing;
- profile revision evidence so website updates are idempotent and ordered.

## Explicit exclusions

The selective character decoder does not declare or materialize protobuf tags
for account ID, open ID, SDK account type, login/session state, credentials,
tokens, email, payment data, or account-security fields. Those values are not
needed to build a character profile.

The website profile contract also excludes OS/client telemetry, moderation
flags, online/offline timestamps, total playtime, private messages, arbitrary
user-supplied image URLs, and precise location history. A game profile image
may be published only through the reviewed HTTPS host allowlist and bounded
image proxy described in `PROFILE_AUTOMATION.md`; arbitrary packet-supplied
URLs remain excluded. These may coexist with character-facing data in a source
message, but proximity does not grant upload permission.

## Promotion rule

A field becomes `verified` only after its exact deployment/build, route,
protobuf field or attribute ID, units, update semantics, and privacy/upload
policy are all recorded. Fixtures prove decoder behavior; a current sanitized
capture proves the mapping.
