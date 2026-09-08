# Modular HUD replacement

Status: active product track. The screen-sized native Overlay Canvas, its
packet-backed Mechanics Map, current-target frame, local-player frame with
packet-backed status effects and class resources, action cooldowns, party frames, and a dungeon
tracker with an authoritative pull clock, plus reviewed mechanic alerts, are
implemented; additional HUD modules remain
disabled until their authoritative event and asset contracts pass the gates
below.

## Product goal

Allow a player who hides the native BPSR HUD to assemble an rLogs HUD from the
same movable, resizable, hideable, profile-scoped modules used by the Overlay
Editor. The complete setup should eventually cover the useful in-game HUD
without becoming one indivisible window.

Initial module families are:

- player frame, resources, shields, statuses, and class gauges;
- action controls, cooldowns, charges, key/controller labels, and equipped
  Battle Imagine controls;
- party frames and combat summaries;
- current-target HP, shield, break state, cast state, and debuffs;
- objectives, interaction prompts, encounter notices, and timers;
- minimap, full map, and encounter-mechanics layers;
- permissioned local chat tabs with localized, human-readable labels.

Every module participates in the existing Setup Profile model. Position,
dimensions, scale, opacity, visibility conditions, z-order, click-through,
locale, and input mode are saved per setup. Display groups can move or hide a
related set without merging their state or rendering code.

## Controller focus mode

The first focus-mode stage is implemented for the action-cooldown and
local-player panels. Holding controller RB, or Right Ctrl as the keyboard
fallback, enlarges those panels around their nearest screen-edge anchors and
adds the blue focus backing.
The native host observes physical hold state passively: it does not register,
consume, inject, repeat, or redirect game input. Per-setup remapping and focus
groups remain later stages.

The requested FFXI-like focus behavior is a hold action, not a permanent UI
zoom. Holding a configurable keyboard or controller modifier (RB by default
for controller setups) activates one configured display group:

- its actionable icons enlarge around a stable anchor without moving unrelated
  modules;
- the focused panel gains a configurable blue backing glow and stronger focus
  outline;
- controller/key labels remain legible and controls do not reflow while the
  modifier is held;
- releasing the modifier restores the exact prior geometry;
- global app zoom and per-module scale continue to work independently.

Input observation must be passive. The feature does not inject, repeat,
translate, block, or automate game commands. A keyboard fallback and an
always-available emergency show/hide shortcut are required before a full HUD
setup may be marked usable.

## Authoritative live state

HUD modules consume the shared resolved model described in
[`ARCHITECTURE.md`](ARCHITECTURE.md). They never rescan packets or invent an
independent BPSR interpretation.

The target frame retains the exact `AttrTargetId` selected by the local actor;
it never substitutes a recent damage recipient. Its first production stage
shows packet-observed current/max HP, death/stale state, and effects whose
reviewed game presentation explicitly classifies them as debuffs. Names and
icons use the bundled exact-ID presentation catalog, and missing HP or
localization remains unavailable rather than becoming zero. Each displayed
debuff now carries its exact packet source when that actor can be resolved and
its packet duration counts down locally without inventing a duration for
effects that do not provide one. The target's packet-provided shield list and
`EBreakingStage` value are also displayed, and an undecodable replacement
clears the projected shield instead of leaving stale state visible. Target cast
state remains gated until its actor identity and duration presentation contract
are both proven for the current build.

The local-player frame projects the same canonical status lifecycle, but only
renders effects whose exact current-build game icon belongs to a buff atlas.
It shows the game icon, localized name, stack count, packet source, and a local
countdown derived from the packet duration. Debuff-atlas and unknown effects
remain absent rather than being mislabeled as player buffs.

The frame also exposes the reviewed class-resource pairs used by the current
Global classes: Blade Intent and Thunder Sigil, Energy and Sharpness, Flame
Soul and Frenzy, and Energy and Flower. Each gauge requires both its exact
current and maximum resource ID in equal-length packet arrays. Unknown IDs,
partial arrays, unsupported classes, and missing maxima remain absent instead
of being guessed into a percentage. The gauge treatment is native CSS for now;
game-extracted art and Verdant Oracle's separate bloom-rotation presentation
remain gated follow-up work.

Parser Health separately retains the current-build local skill-request route
count, successful decodes, decode failures, and emitted canonical cast starts.
This is a diagnostic boundary, not an inferred cast count: it distinguishes no
observed `World.UseSlot` traffic from a decoder failure and from a decoded
request that failed to become a canonical cast. The counters survive an
unclean parser stop so cast-vs-hit gaps can be audited from the failed session
instead of being reported as an unexplained zero.

The overlay may preserve the last target briefly only as an explicitly styled
stale state. It must never imply that an old packet value is still live.
Reference behavior already present in Resonance Logs Global may be adapted only
after its event identities are reconciled with the current rLogs canonical
model and current client build.

## Game-owned asset boundary

Faithful HUD rendering needs a complete, build-versioned catalog of the game's
UI textures, sprites, atlases, fonts where permitted, layout relationships,
localization keys, and semantic widget identities. "Complete" means every
asset required by enabled HUD modules is indexed and accounted for; it does
not mean committing or redistributing the client's raw copyrighted bytes.

- A read-only local compiler derives the catalog from the player's installed
  game and records source build, logical identity, digest, atlas coordinates,
  dimensions, and provenance.
- Extracted render assets stay in the user's local game-asset namespace and
  are not uploaded in logs, profiles, setup shares, or submissions.
- Shared Setup Profiles refer to semantic asset IDs plus minimum catalog
  schema/build compatibility, never absolute paths or copied game payloads.
- A redistributable rLogs fallback theme keeps every control usable when a
  local game asset is missing or incompatible.
- Client updates produce a new immutable asset catalog and a diff; an old
  mapping is not silently treated as valid for a new build.

This extends the inventory rules in [`GAME_FILE_RESEARCH.md`](GAME_FILE_RESEARCH.md)
and the namespaced asset ownership rules in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Mechanics map

Mechanics Map is the first module on the shared Overlay Canvas. The native
canvas covers the active display at its actual resolution, while its backing
HTML canvas scales by the display pixel ratio so high-DPI displays retain full
pixel detail. The map module moves and resizes inside that transparent surface;
its position, dimensions, free zoom, pan, rotation, monster filter, and lock
state persist locally. It can switch between its saved windowed geometry and a
full-display map at the user's actual screen resolution. `Fit` restores the
complete map and `Center` moves the packet-observed local player to the middle
without imposing a zoom cap. The game texture and every entity, marker, and
mechanic layer share one aspect-preserving content rectangle, so a wide or tall
overlay cannot stretch the packet coordinates away from the underlying art.
Locking makes the entire native canvas click-through;
opening the canvas again from Mechanics Map is the recovery path that restores
editing. The same canvas/window contract is reserved for future target,
status, action, party, objective, and chat modules rather than creating a new
native window for each feature.

The target frame, local-player frame, action-cooldown panel, party frames, and
dungeon tracker share that canvas. Each has independent persisted position and width, while
canvas edit/click-through lock remains a single recovery-safe setting for the
transparent native window. Target selection, player and roster identity, HP,
shield, debuff and cooldown lifecycle, actor despawn, scene changes, and stale
state are projected from the same bounded live feed as the map.
The dungeon tracker starts its pull clock only on the canonical encounter-start
boundary, resets it for the next pull, and freezes it at a packet-proven wipe,
clear, or end boundary. Its retry total advances only for packet-proven wipes,
so boss-death outro packets cannot extend the displayed time. Long-poll timeouts
that carry no new revision do not rebuild any module, so
their DOM, countdown anchors, and native overlay remain visually stable.

The mechanic-alert panel is independently movable and resizable. It shows only
signals admitted by a reviewed current-build encounter pack. Packet-duration
status effects receive a local countdown anchored to the snapshot timestamp;
reviewed cast signals remain labeled `OBSERVED` because their current host
lifetime is only a stale-data bound, not proof of the game's cast duration.

The map uses the locally compiled in-game map as the visual base and keeps
mechanic knowledge in separate toggleable layers. Required foundations are an
exact scene/map identity, a proven world-to-map coordinate transform, local
player and party positions, floor/region transitions, rotation/orientation,
and stale-position handling.

Encounter packs may add localized mechanic regions, routes, objectives,
hazards, safe areas, timers, and role-filtered instructions. A marker retains
its evidence source and encounter/build range. User markers and imported packs
remain distinguishable from packet-observed state.

Resonance Logs CN's dungeon-mechanics presentation is a design and behavior
reference. Its source, license, event assumptions, and build compatibility
must be audited before code or data is adapted; rLogs keeps its own canonical
events, localization, permissions, and setup format.

The safe fallback uses the game's current-build `MiniMapSizeScale=140`
contract as a player-relative radar. A scene switches to the full in-game map
only when its exact texture and paired `region_data` transform have both been
reviewed for that client build. The host projects exact actor,
party, position, facing, life, cast, status, map-marker, scene, and data-gap
events into a bounded `/api/runtime/live/mechanics-map` feed. Scene changes
clear all scene-scoped state; positions older than five seconds are visibly
stale. Encounter signals are limited to exact numeric identities selected by a
matching reviewed scene pack. A matching pack never implies safe-area geometry
unless that geometry receives its own current-build evidence.

The Cursed Tomb pack also classifies the packet-observed boss, towers, and
left/right clones and presents reviewed tower activation/completion, energy
pillar, charge-target, puzzle-piece, and clone-charge signals. These identities
remain scoped to scenes `6513`-`6515` on `global/steam-24687926`; a different
scene or build gets no inferred mechanic role. Mechanics Map is available in
ordinary installs. When an exact reviewed map is absent, its surface invokes
the packaged local compiler once for that asset URL after a packet-observed
build and attached game process identify the matching user-owned container. A
failed or unsupported preparation remains visible and retryable; it never
substitutes another build's texture.

The official map-paper fallback can be compiled locally from an installed game
without committing or uploading it:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926
```

The first full-scene map is Cursed Tomb (`6513`-`6515`). Its current-build
texture and paired world transform can be compiled with:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --address ui/textures/scenemaps/dng_branch_6501_godvault/dng_branch_6501_godvault_dng_branch_6501_godvault `
  --object-name dng_branch_6501_godvault_dng_branch_6501_godvault `
  --asset scene-6513-cursed-tomb.png `
  --region-address ui/textures/scenemaps/dng_branch_6501_godvault/dng_branch_6501_godvault_region_data
```

Towering Ruin (`1150`-`1152`) is tied by the current game tables to
`dng_hero_1121_tower_s3`, rather than either similarly named older tower map:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --address ui/textures/scenemaps/dng_hero_1121_tower_s3/dng_hero_1121_tower_s3_dng_hero_1121_tower_s3 `
  --object-name dng_hero_1121_tower_s3_dng_hero_1121_tower_s3 `
  --asset scene-1150-towering-ruin.png `
  --region-address ui/textures/scenemaps/dng_hero_1121_tower_s3/dng_hero_1121_tower_s3_region_data
```

Tina's Mindrealm (`1631`-`1633`) uses the current-build
`dng_main_1001_tina` texture and its paired `800 x 800` region transform:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --address ui/textures/scenemaps/dng_main_1001_tina/dng_main_1001_tina_dng_main_1001_tina `
  --object-name dng_main_1001_tina_dng_main_1001_tina `
  --asset scene-1631-tina-mindrealm.png `
  --region-address ui/textures/scenemaps/dng_main_1001_tina/dng_main_1001_tina_region_data
```

Coral Sea (`6563`-`6565`) uses the current-build `dng_branch_6561_coral`
texture and paired `1000 x 1000` region transform:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --address ui/textures/scenemaps/dng_branch_6561_coral/dng_branch_6561_coral_dng_branch_6561_coral `
  --object-name dng_branch_6561_coral_dng_branch_6561_coral `
  --asset scene-6563-coral-sea.png `
  --region-address ui/textures/scenemaps/dng_branch_6561_coral/dng_branch_6561_coral_region_data
```

The compiler requires one exact address row, one exact Unity bundle entry, and
one `Texture2D` object. It writes a local catalog with the build, package,
bundle hash, dimensions, and SHA-256 digest. If that asset is missing or the
build differs, the live map remains usable with the redistributable rLogs radar
theme. The Cursed Tomb transform is read from the game's paired region-data
asset (world origin `-149, -377`, span `450 x 450`) and is build-gated
alongside the texture. The independent implementation was behaviorally
cross-checked against the newest locally audited Resonance Logs CN source; no
AGPL source is copied into rLogs.

Windows release builds compile that same reviewed script into
`resources/map-compiler/rlogs-bpsr-map-compiler.exe`. CI builds and runs the
packaged helper's synthetic binary-parser/import self-check on every change,
using the exact dependency versions in
`tools/bpsr-map-compiler-requirements.txt`; the release workflow repeats that
gate before constructing the installer. The installer contains the compiler,
not game assets. Automatic discovery follows the attached game executable only
after the packet-observed build selects an exact reviewed manifest entry. The
compiler cannot guess or silently cross an installed-client location or build
identity.

The checked-in `reviewed-map-assets.v1.json` allowlist can prepare every map
reviewed for one exact build in a single command. Before writing each image,
batch mode verifies its source and region bundle hashes, texture dimensions,
and complete X/Z transform against that allowlist:

The current Global Steam allowlist contains 57 exact asset entries covering
all 56 scene-map families joined to active scenes by the build-24687926 game
tables (plus the separately packet-proven Wasteland scene). This broad map
coverage is independent of encounter-mechanic coverage: only the six reviewed
encounter packs add mechanic semantics, but every reviewed city, open-world,
dungeon, raid, tower, world-boss, guild/activity, and housing map can render as
the full scene map. Multi-floor families currently select their primary
game-authored texture; alternate floor/foreground textures remain inventoried
for explicit layer-selection work rather than being guessed or flattened.

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --reviewed-manifest apps/desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json
```

Before review, the same compiler can inventory the complete installed
current-build scene-map catalog. Supplying exact-build scene tables joins each
asset family to all of its scene IDs and hashes the table inputs for audit:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --build global/steam-24687926 `
  --inventory-output runtime-data/map-audits/global-steam-24687926.json `
  --scene-table <exact-build-SceneTable.json> `
  --scene-resource-table <exact-build-SceneResourceTable.json>
```

This audit does not itself authorize a map. Production still requires one
unambiguous texture, its paired `region_data`, exact bundle hashes and
dimensions, a positive transform, and at least one exact-build scene ID.

Strict single-texture candidates can be materialized into a separate visual
review directory and candidate-only manifest without changing production:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/map-review `
  --build global/steam-24687926 `
  --inventory-input runtime-data/map-audits/global-steam-24687926.json `
  --candidate-manifest-output runtime-data/map-audits/global-steam-24687926-candidates.json
```

The candidate output is intentionally not accepted as a production allowlist.

## Chat tabs

The first chat milestone is a local display surface with reorderable tabs,
unread state, timestamps, channel colors, font/opacity controls, and proper
localized labels. Filters operate on reviewed channel identity, not guessed
numeric routes. Public, system, and party chat require the separate chat-read
permission already defined in [`PRIVACY.md`](PRIVACY.md).

Chat text remains local-sensitive, is excluded from `.rlog` submissions and
website/profile sync, and is never included in shared Setup Profiles. Direct
messages and private/guild communications remain prohibited. Sending chat or
replacing the native text composer is outside the first display-only milestone
and requires a separate security and game-input review.

## Delivery stages and gates

The Overlay Canvas now includes a movable dungeon-objective tracker driven by
canonical dungeon packets. It preserves the exact dungeon, difficulty,
objective IDs, values, completion state, and flow phase received by the host.
When the current build's catalog cannot resolve an objective, the module shows
`Objective <ID>` and the observed value; it does not reuse a prior-build label
or invent a required total. Packet exits and scene changes clear the tracker.

1. **Evidence inventory** — enumerate existing Global implementation code,
   current-build target/status/map/chat routes, UI asset coverage, and every
   unresolved field without promoting guesses.
2. **Target frame** — ship replay-tested entity/HP/debuff lifecycles and a
   movable module with explicit unavailable/stale states.
3. **Focus interaction** — add passive keyboard/controller hold detection,
   stable anchored scaling, glow, accessibility settings, and emergency UI
   recovery.
4. **HUD controls** — add locally asset-backed action, resource, status, party,
   objective, and prompt modules one authoritative contract at a time.
5. **Mechanics map** — prove coordinate transforms and map lifecycles before
   enabling encounter layers or shared packs.
6. **Chat tabs** — enable the separately permissioned display-only local chat
   surface after channel identities and redaction tests pass.
7. **Full-HUD setup** — publish an optional first-party Setup Profile only
   after loss-of-capture recovery, scene transitions, DPI/scaling, controller
   focus, click-through, and emergency visibility are verified together.

Each stage needs deterministic replay fixtures, current-build live evidence,
bounded memory/performance measurements, localization coverage, and visual
checks at 1080p, 1440p, 4K, ultrawide, and supported Windows DPI scales. A
hidden native HUD is never assumed during testing; the rLogs setup must fail
visibly and recoverably when its source feed or local asset catalog is absent.
