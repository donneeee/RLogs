# Resonance Logs CN reference audit

Status: source-level audit of the current reference build, used to plan
clean-room RLogs implementations.

## Reference identity and validation

- Repository: <https://github.com/fudiyangjin/resonance-logs-cn>
- Reviewed commit: `bd71d2dfd3c7289e6398c4cd042f4357d4f35721`
- Product version: `0.2.4`, one commit after tag `0.2.4`
- Review date: 2026-09-11
- License: `AGPL-3.0-only`

The post-tag change only corrects monster-name/type extraction data. The
feature architecture is the tagged 0.2.4 architecture.

The committed source is not fully reproducible as-is. `npm ci` rejects the
out-of-sync lockfile. After a non-lockfile-mutating dependency install, all 309
Vitest tests pass, but `npm run check` reports one invalid typed route
(`/live/dps/`) and 41 Svelte accessibility/reactivity warnings. The production
frontend compilation completes, then prerendering fails because `/hud-overlay`
links to a missing `/favicon.png`. This audit therefore treats the source as
behavioral evidence, not as a defect-free or directly importable
implementation.

RLogs must not copy source, assets, generated protocol tables, or native code
from this reference without a separate license decision. The default path is a
clean-room implementation from the behavior described here, RLogs-owned packet
captures, and independently verified game data.

## Product architecture

The reference has three native Tauri windows:

1. a separate transparent live combat meter;
2. the main settings/history application;
3. one unified transparent HUD window containing game, monster, and minimap
   domains.

The unified HUD supersedes retired separate game, monster, and minimap windows.
It uses one versioned pull feed, logical per-domain visibility, saved geometry,
edit-mode pointer input, and normal-mode click-through. This matches the RLogs
product boundary: the combat meter remains separate; all other movable HUD
modules share one canvas whose controls do not belong to any individual module.

The backend is a single deterministic live actor. Ordered decoded batches and
control commands feed independent combat, status, buff, monster, fantasy,
minimap, death, voice, counter, and history projections. High-frequency windows
pull versioned snapshots instead of recreating or event-pushing entire windows.
That pattern is especially relevant to eliminating overlay refresh flashes.

## Full feature and dependency matrix

| Area | Reference behavior | Evidence/dependency | RLogs decision |
|---|---|---|---|
| Capture | WinDivert sniffing by default; optional Npcap; Windows TCP ownership filters; ordered TCP reassembly; zstd framing; bounded queues and explicit gaps | Native Windows APIs, drivers, packet captures, build-specific framing | Preserve RLogs's broader capture contract and ExitLag-compatible late ownership. Adopt only the bounded ordered-pipeline behavior. |
| Protocol | Static service/method IDs and generated protobuf decode entities, positions, buffs, hits, heals, party, scene, dungeon flow, resources, markers, matchmaking and votes | Exact region/build packet evidence; generated source has no committed proto source | Never treat CN IDs or meanings as Global authority. Keep immutable deployment/build protocol packs. |
| Combat meter | Exact integer totals; damage, heal, effective heal, taken, crit/lucky/block/trigger, extrema, boss-only and per-target views; elapsed, damage-active and display clocks; manual pause/reset and dummy mode | Decoded hit/identity/scene facts and explicit segmentation rules | Retain RLogs canonical clocks and separate combat-meter window. Continue correct rDPS and multi-POV reconciliation, which the reference does not implement. |
| History | SQLite summaries plus chunked MessagePack/zstd event journal; range replay; player/boss/scene filters, favorites and deletion | Local immutable event stream and database migrations | Preserve sealed RLogs logs as authority. Add equivalent fast range queries without weakening auditability. |
| Graph | Damage curves derived from raw hit events; shared zoom/pan/brush viewport and minimap; selected ranges recalculate summary details | Timestamped hit events and a stable encounter clock | Use the canonical run clock on app and site. Keep website playback as an additional RLogs feature, not a parser clock dependency. |
| Skill timeline | Separate lanes for boss skills, a static whitelist of key player skills, fantasy summons and watched buff spans; it does **not** record every skill | Packet events plus build-specific marker whitelist, skill names and icons | RLogs timeline should support every captured skill use, with progressive-density rendering. Player graph visibility controls that player's timeline lanes. |
| Death replay | Captures a death and the preceding two seconds of accepted damage, including killer, skill/property/mode and relevant victim/attacker buffs | Death/hit ordering and build-localized ability, monster and buff catalogs | Keep uncircled skull-and-crossbones markers in each player's graph color. App and site expose cause summary on hover and full replay on demand. |
| Website playback | No equivalent current-source website playback or multi-POV site synchronization | Not implemented upstream | RLogs-only advancement: site play/pause/scrub over canonical time; metrics and visible lines update at the selected time; synchronized POVs share one reconciled run. |
| HUD editor | Unified full-window HUD; drag/resize; saved layout; edit-mode interaction; normal click-through; logical domain visibility | Transparent always-on-top native window, monitor geometry and cursor passthrough | One canvas for game/monster/map modules. Canvas-level Done, Hide, Reset and Esc controls; edit scaffold is translucent over a darkened game view; normal backgrounds honor user opacity and default transparent. |
| Combat overlay | Separate transparent always-on-top meter with click-through, blur options and global shortcuts | Native window and pull feed | Keep physically separate from HUD canvas. Preview and native rendering must share the same renderer/layout/opacity state. Scroll Lock is the default configurable global hide/show shortcut. |
| Skill/buff HUD | Cooldowns, skill durations, grouped/individual buffs, buff coverage, attributes, resources, shields and custom progress/counter panels | Live status/resource events and large build-derived tables | Reimplement as independently movable RLogs modules with exact-build mechanics and stable reviewed display labels. |
| Monster HUD | Boss HP, stun, threat, buffs, teammate buff matrix, fantasy casts and DBM-style mechanic panels | Monster identity, position, aggro, buff and cast packet facts | Reimplement behind typed verified events and role/scene filtering; never display unsupported mechanics speculatively. |
| Mechanics minimap | Real-time players, monsters, facing, casts, buffs and observed in-game markers for six hardcoded scene families; procedural arena drawings | Exact scene/buff/monster IDs and calibrated coordinate transforms | Use real licensed/extracted game-map imagery, not the reference's opaque/procedural map. Show a mechanics module only when the current scene is supported and a mechanic is active; prioritize legibility. |
| Observed waymarks | Reads marker 1-6 positions from passive-skill observations | Captured packets; sniff-only | Useful evidence for RLogs marker capture, but not an automarker implementation. |
| Automarkers | No save/load placement, no transmit path and no packet injection | Absent upstream | Separate RLogs custom-trigger subsystem. Multiple named Save/Save As presets per normalized scene family; local Load is independent of DPS/run clocks; Place in game remains disabled until an authorized, verified game command/packet path exists. |
| Custom counters/triggers | Typed buff/skill/hit/tick/distance/attribute/dungeon-state rules, thresholds, freeze windows, slots and actions | Canonical events plus exact-build IDs/formulas | Reuse the behavioral concepts inside RLogs Custom Triggers, with beginner sentence rules and advanced details progressively disclosed. |
| Challenge Watch | User-selected forbidden damage IDs add a warning to party rows when someone is hit | Damage-name catalog and current combat segment | Treat as an avoidable-damage/challenge tracker. It is unrelated to the profile uploader's `challenge_dungeon_info` value. |
| Challenge dungeon profile flag | Generated protobuf contains `ChallengeDungeonInfo`, but the current runtime does not decode or use it | Unresolved packet/profile semantics | Low priority. Do not infer meaning from the UI feature called Challenge Watch; resolve using independent profile-upload captures. |
| Voice | Alert rules, priority/cooldown queue, phrases, WAV playback, optional Qwen3 CPU/Vulkan generation and voice profiles | Large native sidecars/model weights, signed downloads and audio permissions | Start with trigger-to-cue and user-provided/pre-generated audio. Treat local TTS models as a separately licensed, secured optional component. |
| Module optimizer | Packet-derived inventory; scored four/five-module search; CPU/CUDA/OpenCL backends; progress and attribute breakdown | Exact character serialization, formula tables and native toolchains | Preserve RLogs optimizer score and localization on desktop/site. Maintain the separately audited compatibility implementation and do not silently update it from this reference. |
| Profiles/loadouts | Multiple monitor profiles, specialization association and automatic switching; import/export and migration | Character specialization observations and validated settings schema | Carry modular Setup Profiles forward with explicit manifests, migrations and no silent permissions. |
| Upload/sync | Database columns and UI state scaffolding exist, but no complete encounter upload/reconciliation service is present | Incomplete upstream | RLogs remains the authority here: sealed resumable upload, server replay, multi-POV reconciliation, rankings and privacy boundaries. |
| Localization | Chinese, English and Japanese UI plus very large generated game-name catalogs | Reviewed UI translations versus machine/generated game data | RLogs target locales remain broader. Stable reviewed labels carry across ordinary builds; mechanics, formulas, assets and protocol semantics require exact authority. Seasonal invalidation is explicit, not assumed for every build number. |
| Lifecycle | Single instance, tray controls, hidden-on-close, diagnostics bundle, settings backup/recovery and signed updater | Tauri/Windows integration and release infrastructure | Preserve one versioned installer and newest website download link. Release after a coherent major feature or regression-repair slice passes CI and native smoke tests. |

## Important negative findings

The reference is not an authority for several of RLogs's central features:

- It has no rDPS or support-contribution attribution.
- It has no multi-POV log reconciliation.
- It has no complete active encounter upload/sync implementation.
- It does not save, load, or transmit automarkers.
- Its WinDivert capture is sniff-only; it has no packet-send path.
- Its mechanics map does not use real game-map imagery.
- Its timeline intentionally includes selected/key skills, not every skill use.
- Its process-owned endpoint filter is likely less compatible with ExitLag than
  RLogs's late loopback/proxy admission path.

## RLogs implementation order

### Gate 1: retain regression repairs

- Keep localization presentation across ordinary build changes while gating
  mechanics/formulas/assets on exact authority.
- Keep module names/effects/score resolved on both desktop and website.
- Keep a single versioned executable and verify the website's newest download.

### Gate 2: native overlay truth

- Make preview and native overlay use one renderer and saved state.
- Keep the combat meter separate.
- Make every non-meter module appear on the unified canvas.
- Verify actual packaged-app Open/Edit/Done/Esc/Hide behavior, transparency,
  click-through, opacity changes and no refresh flash.

### Gate 3: history and site timeline

- Persist every captured skill use, boss action, buff window, death and cause on
  the canonical run clock.
- Render the skill timeline separately from the damage graph.
- Couple player line visibility to player timeline-lane visibility.
- Add site-only play/pause/scrub with time-relative DPS variants and death
  summaries.

### Gate 4: map and automarkers

- Calibrate real map assets and coordinates per normalized scene family.
- Render only supported, active mechanics at high contrast.
- Finish independent Save/Save As/Load marker presets.
- Analyze RLogs-owned captures for a safe leader-authorized placement action;
  do not enable Place in game based on observation packets alone.

### Gate 5: contribution correctness

- Reconcile five or more POVs into one canonical run without double counting.
- Align each contribution ledger entry to canonical time and retain source POV
  evidence.
- Prove rDPS conservation and attribution for parties and raids before rankings
  depend on it.

## Source map

The most useful reference locations are:

- native capture and reassembly: `src-tauri/src/packets/`;
- deterministic runtime: `src-tauri/src/live/live_core.rs` and
  `src-tauri/src/live/runtime/`;
- combat/death/timeline/minimap projections:
  `src-tauri/src/live/projections/`;
- event journal and range replay: `src-tauri/src/database/` and
  `src-tauri/src/live/history_writer.rs`;
- unified HUD: `src/routes/hud-overlay/`;
- skill/buff modules: `src/routes/game-overlay/`;
- monster modules: `src/routes/monster-overlay/`;
- mechanics map: `src/routes/minimap-overlay/`;
- encounter timeline: `src/lib/components/encounter-timeline/`;
- death replay: `src/lib/components/death-replay/`;
- optimizer: `src-tauri/src/module_optimizer/`;
- voice: `src-tauri/src/voice/`;
- native window and updater configuration: `src-tauri/tauri.conf.json`.
