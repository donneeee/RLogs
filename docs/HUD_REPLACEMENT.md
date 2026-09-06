# Modular HUD replacement

Status: planned product track. This document records the accepted direction;
individual modules remain disabled until their authoritative event and asset
contracts pass the gates below.

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

The target frame is the first gameplay-state milestone. It must retain the
observed target entity identity and show current/max HP, shield/break state,
cast state, and each reviewed debuff's localized identity, owner, stacks, and
remaining duration when those fields are authoritative. Target change,
despawn, death, scene change, capture loss, and stale-data timeout each have a
tested lifecycle. Missing max HP, duration, ownership, or localization is shown
as unavailable or unknown; absence is never converted to zero.

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
scene or build gets no inferred mechanic role. Mechanics Map remains a
`developer_only` workspace tab until the local game-asset compiler and live
replay coverage are ready for ordinary installs. Removing that one manifest
field is the explicit graduation step.

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
not game assets. Automatic discovery/invocation remains disabled while this
workspace is developer-only, so an installed-client location and exact
packet-observed build identity cannot be guessed or silently crossed.

The checked-in `reviewed-map-assets.v1.json` allowlist can prepare every map
reviewed for one exact build in a single command. Before writing each image,
batch mode verifies its source and region bundle hashes, texture dimensions,
and complete X/Z transform against that allowlist:

```powershell
python tools/bpsr-local-map-asset.py `
  --container "C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance\bpsr\BPSR_STEAM_Data\StreamingAssets\container" `
  --runtime-root runtime-data/game-assets `
  --build global/steam-24687926 `
  --reviewed-manifest apps/desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json
```

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
