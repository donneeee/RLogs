# RLogs shared catalog

This canonical, human-readable catalog is shared across player regions. Exact
deployment/channel/client-build availability is recorded on each definition.

It contains reviewed combat, talent, Battle Imagine, profile-display, public
guild-badge, equipment, entity, and world definitions. Static records do not
prove character ownership, equipment, guild membership, or live entity UUIDs.
Ambiguous relations remain `shared`, `unclassified`, or explicitly unresolved.

Observed packet action IDs that lack their own localized SkillTable row are
reviewed in `combat-actions/reviewed-observed.v1.json`. A row may reference an
exact current-build skill or recount-group localization key. Ambiguous parent
relations stay unresolved; this layer never selects a convenient name merely
to remove a question mark from the UI. A reviewed Recount relation preserves
the observed action as a child and records the stable group ID separately; it
never replaces or discards the child action.

`combat-actions/observed-technical.v1.json` is a generated, build-scoped
coverage layer for action IDs found in saved histories but absent from a
direct localized SkillTable row. The external game-data scan joins exact
RecountTable children where the relation is unique; otherwise it preserves the
raw design identity, actor kind, monster identity, and evidence as a technical
action name. It never hides an observed event or invents a parent relation.
`combat-actions/current-build-recount.v1.json` separately contains every
current-build DamageAttr action whose rows all resolve to one exact RecountTable
group, not only actions seen in saved histories. These relations add aggregate
parents while retaining each packet action ID, direct localized child name,
and child metrics. Parent totals are sums of those still-visible child rows.
Regenerate it with `BPSR-UID-Extractors/GenerateRLogsObservedActions.gen`, then
compile the catalog with `PromoteRLogsCombatPresentation.gen` and the
`rlogs-bpsr-runtime-presentation` binary. RLogs loads only the resulting compact
sorted runtime tables; extraction inventories and joins stay outside the
capture, history, and display paths.

The runtime profile contract now exposes typed module instances and selected
talent-node state. Website rendering must join those IDs to this exact-build
catalog. Module item definitions live under `modules/<type>/`, with separate
human-readable `module-effects/`, `module-types/`, `module-slots/`, and
`module-link-effects/` domains. Talent records remain under
`talents/<class>/<spec>/`. Their shared icon paths mirror those domains under
`assets/blue-protocol-star-resonance/shared/icons/`.

The current profile-display slice contains all 12 module configurations, 21
effects with seven exact link thresholds each, 121 total-link score rows, five
slots, six types, 1,350 talent nodes with exact coordinates and both graph
directions, and 640 unique exported module/talent PNGs. Only the 990 nodes
referenced by a current `TalentStageTable` row are marked active for normal
board rendering; 360 unassigned rows remain explicit rather than being
silently discarded. Unnamed module-initialization roll dimensions and the
generic assessment screen remain evidence-only outside the parser catalog.

Deep-Slumber Psychoscope factor review lives under
`psychoscope-factors/<season>/`. A reviewed factor slice keeps the exact item
and family IDs separate from its grade, direct attributes, primary buff,
energy rules, triggered actions, and recount-parent relationships. The compact
runtime projection belongs to the BPSR game plug-in; rDPS policy consumes that
graph later and must not infer attribution from localized descriptions alone.

The current Battle Imagine slice is organized as one human-readable JSON file
per item under `imagines/battle/`, with one matching human-readable icon under
`assets/blue-protocol-star-resonance/shared/icons/imagines/battle/`. It contains
86 current-build records, 73 exact item-to-skill relations, all five
enhancement rows for those skills, and official text from all 11 shipped
locales. Thirteen items remain unresolved rather than inheriting older
page/icon guesses. Item descriptions remain pending until their exact current
`ItemTable` field is proven.

Weapon equipment is organized as one human-readable JSON file per item under
`weapon-equipment/`. The current global Steam build contains 722 `ItemTable`
weapon IDs and references 19 exact equipment-inventory badge assets. Shared
badges are retained when multiple item rows intentionally reference the same
address; class/spec icons are never substituted for an equipped weapon, and
`WeaponSkinId` is not used as the badge because cosmetic skins are a separate
identity from the equipped item. `EquipTable` and
`EquipBreakThroughTable` provide 702 fixed level records and 18 progressive
Far Sea records. Two NPC empty-handed items have no equipment-level row and
remain explicitly level-unresolved. Regenerate the compact sorted Rust lookup
and these catalog records with `tools/generate-weapon-presentation.mjs` after a
reviewed table extraction; the live overlay and History use that lookup and do
not load the JSON catalog at runtime.

Dungeon records contain only fields whose exact current-build offsets have
been reviewed. Four labels inherited from a historical `DungeonsTable` layout
were removed after the current 218-byte row layout proved that those offsets
now hold level/season arrays and result-position bytes, not end time, monster
power, rank-loading, or result-HUD values. Unresolved late fields remain in
the build-scoped research inventory until their current layout is proven.
Runtime encounter meanings live separately under `run-rules/`; they are never
guessed from an unreviewed static field.

Difficulty display also stays separate from dungeon identity. BPSR represents
Master as one difficulty family with twenty tiers (`M1` through `M20`).
Normal and Hard do not receive a fabricated tier. The exact packet difficulty
ID remains available alongside that normalized model.

The current overworld slice is organized around 16 human-readable scene files
under `overworld-scenes/`. Each scene owns its areas, transition graph, world
points, and static monster, NPC, world-object, and zone placements. The
placement-definition ID itself encodes its owning scene in the current build;
that relationship is validated during compilation. Canonical reusable
definitions remain separate under `monsters/`, `npcs/`, and `world-objects/`.
Twenty-one IDs are reused across placement domains, so static placement
identity is the domain plus the definition ID. Static definition IDs and
positions never stand in for live packet UUIDs.

Named subareas live under `world-areas/scene-<id>/`; reusable objects are
grouped by type under `world-objects/`; daily activities and world-boss
schedules are separated under `world-events/`; and map exploration graphs are
grouped by scene under `map-stickers/`. Two current transition rows still
reference scene 5502, which is absent from the current scene table, so those
targets remain explicitly unresolved instead of being guessed.

The first behavior layer keeps 22 subscenes under `subscenes/`, 799 reusable
activity objectives under `activity-targets/`, and 42 objective graphs under
`scene-events/`. Subscene records preserve both exact `SceneTable` ownership
and weaker resource-path candidates as separate relations. Activity targets
retain parameters, positions, progress rules, special-variable display data,
their scene resolution, and generated reverse references to every scene event
that uses them. Scene events reference those same target records, with timing
and completion-action arrays intact. The paired forward/reverse references are
validated before compilation so packet-time objective resolution requires no
global runtime index. Twenty-seven target scenes and two scene-event target IDs
are missing from their present canonical tables; they remain explicitly
unresolved.

The six `WorldActTable` row IDs live under `world-activities/`, but remain
identity-only. Unnamed fields are retained in private build-scoped evidence,
not published with invented meanings. Behavior text is separated again by
locale under `localization/<locale>/behavior/`. All 11 shipped locales are
supported; one current target description is absent only from the Thai client
table, and that source gap remains missing rather than being machine-filled.

Season rotations live under `dungeon-seasons/`, one compact record per season.
Each record references canonical dungeon IDs and stores the proven M1-M20
identity once, so the catalog does not duplicate a dungeon twenty times.
Per-tier fields are added only when they differ and their current offsets are
reviewed. Historical season records remain available for archived logs, while
the sharded loader reads only a requested season. A private build-scoped scan
regenerates these records from the current client and must pass row-layout,
membership, M1-M20 completeness, scene, and localization review before
promotion.

The BPSR plug-in discovers every reviewed `dungeon-seasons/season-*.json`
record at build time; adding a future season does not require editing a Rust
source list. Promotion fails unless every master family proves the complete
M1-M20 range and the exact `dungeon_id * 100 + tier` row identity. If a dungeon
returns in a later season, an identical scene identity is accepted and retains
evidence from both seasons, while any contradiction stops the build for
review. Detailed boss, objective, and segmentation rules remain separately
reviewed and are never guessed from the compact seasonal identity.

Master display labels use `difficulty.master.label_format`, with one compact
format per shipped locale. The tier number always comes from the reviewed
numeric identity. Five Western locale packages use reviewed corrections
because 14 current-client rows display the following tier number; the raw 70
locale-row mismatches remain in research evidence. Thai consistently numbers
the rows but has two official text styles, so the overwhelmingly used
`ระดับ Master {tier}` form is the reviewed canonical format.

Official text is colocated only during mapping for exhaustive validation. Once
stable, it moves unchanged into data-only add-ons under
`plugins/builtin/localization/<locale>/games/app.rlogs.game.blue-protocol-star-resonance/game/`;
`en-US/ui/` will contain RLogs
interface text. Asset acquisition and extraction remain outside RLogs.
