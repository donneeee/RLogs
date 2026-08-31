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
| class/profession ID | `class_id` | current profession verified in the complete snapshot; exact-build profession names, descriptions, icons, and theme references statically corroborated | public |
| specialization | `specialization_id` | observed through skills, talents, and buffs in Global; projection rule pending | public |
| character level | `level` | verified in the complete snapshot; `PlayerLevelTable` primary level key statically corroborated | public |
| character experience and prior-season max level | `progression` | both fields verified in the complete snapshot | public |
| combat/fight power | `combat_power` | verified in Global Steam build 24252055 complete snapshot | public |
| season strength | `season_strength` | verified through `NotifySocialData` | public |
| season ID, level, experience, and power | `season` | ID, level, and experience verified; power remains unmapped | public |
| gender, body size, voice, face choices, colors, and height | `appearance` | active selections and unlocked face-item/voice IDs verified in the complete snapshot | explicit opt-in |
| avatar, card style, and frame IDs | `appearance` | verified through the complete snapshot and social route; profile-image, objective, and name-card static definitions corroborated | opt-in public |
| unlocked profile-image IDs | `appearance.unlocked_profile_image_ids` | current decoder verified; the saved character had no enabled entries | explicit opt-in |
| game profile and half-body pictures | future reviewed image reference | URL tag and changed verification substructure verified without rendering values | explicit opt-in through reviewed CDN proxy |
| equipped gear | `equipment` | 11 slots joined to item instance/config IDs in the complete snapshot; quality and exact-build static definitions available | public |
| refinement, enchantments, and sets | nested equipment fields | refinement/failure counts, attribute maps, and enchantment IDs/levels/types verified; effective item level and set ID remain unmapped | public |
| equipment suit-entry map | `equipment_suit_entries` | current tag-6 map shape verified with keys 5 and 6; both nested records were empty, so the keys are retained without claiming they are set-family IDs | public only after exact set identity is proven |
| equipped modules and module inventory | `modules` | verified: 5 equipped slots and 649 package-5 module instances, with browser-safe string instance IDs | opt-in public |
| module parts, initial links, upgrade history, type, level, quality, and success rate | nested `modules.inventory` fields | verified: 1,937 current parts, initial-link values on all 649 modules, and 9,526 upgrade records | opt-in public |
| owned/equipped Imagine item state | `owned_imagines` | decoder implemented; the verified snapshot carried no entries on this source | opt-in public |
| Battle Imagine skill library and equipped slots | `battle_imagine_skills` | 29 current skill records verified, including 2 equipped slots; 73 exact current-build skill-to-item definitions are statically mapped | opt-in public |
| active skills and levels | `active_skills` | current-profession loadout and skill level/remodel/skin IDs verified | public |
| current talents | `talents` | 70 selected node IDs verified for the current profession; per-node level semantics remain absent | public |
| talent point progress | `talent_progress` | total talent points verified; reset count was absent in the saved snapshot | public |
| combat-profession levels, experience, loadouts, skins, and talent nodes | `combat_professions` | all 9 profession records verified; 4 carried talent loadouts with 200 selected nodes and used-point counts; stage configuration was absent | public |
| life-profession levels, experience, and specializations | `life_professions` | all 9 profession records and specialization levels verified | opt-in public |
| owned cosmetics | `cosmetics` | generic decoder implemented; the verified snapshot carried no entries on this source | opt-in public |
| fashion, mount, weapon-skin, and dye ownership/collection points | `collection_summary` | 10 equipped fashion slots, owned IDs, and collection-point totals verified | opt-in public |
| rides, ride skins, emojis, vanity pets, Fantasy Atlas, and handbook | nested `collection_summary` fields | 1 ride, 1 ride skin, 53 emojis, 2 pets, 2 atlas stages, and 440 handbook entries verified | opt-in public |
| dungeon, master-mode, and weekly-tower progression | `activity_progress` | 133 master-mode rows and weekly-tower state verified; normal challenge arrays were empty | opt-in public |
| seasonal medals and cultivation | `season_medals`, `season_cultivation` | 7 holes, 8 nodes, 2 season records, 4 lines, and 16 areas verified; season 3 explicitly selected 2 active areas, 12 middle-node factor item IDs, and 5 big-node Fantasy IDs | opt-in public |
| reputation and current profession project | `reputations`, `current_profession_project_id` | one reputation row and a current project ID verified | opt-in public |
| guild, titles, and medal map | `social_display` | guild identity/name and title verified; medal IDs are retained locally while owned-vs-displayed semantics remain pending | public guild identity/display; opt-in titles and medals |
| combat-power component breakdown | `combat_power_breakdown` | total plus 6 function components and 4 nested subcomponents verified | public per build |
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
decoder now produces one privacy-reviewed character-profile patch containing
identity, complete face/color/avatar appearance, level progression, 11
equipped items with attributes and enchantments, 5 equipped modules, a
649-instance module inventory, 9 combat professions and their loadouts, 9 life
professions, and the fashion/mount/weapon-skin collection. The expanded replay
also verifies 29 Battle Imagine skill records with 2 equipped slots, 4
profession talent loadouts containing 200 selected nodes, total talent points,
extended ride/emoji/pet/atlas/handbook collections, 133 master-mode dungeon
rows, weekly-tower progress, seasonal medal and cultivation state, reputation,
and the current profession project. A later selective replay also verified the
equipment tag-6 `suit_info_dict` shape. It contained map keys `5` and `6`, but
both nested records had no attribute type or attribute values. RLogs therefore
retains the two entries as structural evidence and does not reinterpret either
key as a set-family ID. The snapshot did not carry server ID,
talent reset count, talent-stage configuration, or scene data, so
region/world and map context remain separate evidence requirements.

## Local and remote character scope

Character availability is tracked separately from protobuf declaration. The
machine-readable matrix is
`plugins/games/blue-protocol-star-resonance/protocol-references/exposure-matrix/global-steam-24252055.json`.

- Owner-container routes expose the complete local character snapshot,
including inventories, modules, professions, talents, collections, and the
owner's exact ordered action slots. The complete snapshot also carries the
owner's current Psychoscope cultivation selection: the current season, active
line areas, middle-node factor grade item IDs, and big-node Fantasy IDs. These
runtime IDs are resolved against the exact-build BPSR catalog by the game
plug-in; inventory ownership alone is never treated as selection.
- Party snapshots currently prove public UID, display identity, class, level,
  ability score, and seasonal strength for each member. Public equipment item
  IDs exist in the reviewed schema but were absent from the retained real-player
  team observation.
- Nearby-player AOI state exposes entity UUID, class, level, ability score,
  seasonal strength, weapon configuration, and observed skill/remodel evidence.
  That evidence can identify remote primary Imagines and observed auxiliary
  actions, but it is not an ordered remote slot map.
- The public social surface exposes public character presentation such as
  avatar/card/frame IDs, guild, title/medal display, equipment configuration,
  and current scene context. It does not contain a complete `ProfessionList`,
  talent tree, modules, or ordered auxiliary slots.

Website-assisted profile lookup is intentionally deferred. It will belong to
the BPSR Profile Sync plug-in and must be designed together with the website
API; it is not a Core or base packet-decoder responsibility.

The current character container has observed top-level tags through `121`.
The current BPSR-Deeps schema names every observed top-level tag, including
`102`, `103`, `104`, and `106` through `121` beyond the older reference
ceiling. The sanitized tag-to-name census is
`world-load-character-surfaces-001.json`. Names establish a research route;
they do not bypass per-field semantic and privacy review.

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

- the still-empty Imagine item-ownership source and exact semantics for the
  verified Battle Imagine skill levels, remodels, skins, and slots;
- gear suit/set bonuses and effective item level;
- proven names for the three remaining module-initialization roll dimensions
  and user-facing optimizer scoring presets;
- specialization selectors, passive ownership, cooldown configuration, and
  the dirty-delta path used to update already-verified seasonal cultivation
  choices without waiting for another complete snapshot;
- the website implementation of the now-promoted exact talent-board layout,
  active-stage filtering, icons, and accessible list view;
- seasonal power, rank/reduction levels, and other progression systems;
- combat-power components and complete panel-stat snapshots;
- profession equipment and remaining profession skill metadata;
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
