# Website modules and talent boards

RLogs exposes the character state required for module optimization and a
game-like talent board without sending raw packets or a whole serialized
character object to the website.

## Ownership boundaries

The BPSR game plug-in owns:

- decoding the character's module package and equipped module slots;
- decoding total/used talent points and selected talent-node IDs per
  profession;
- emitting a typed, privacy-reviewed `CharacterProfilePatch`;
- identifying the deployment, region, realm/world, build, and public character
  UID attached to that patch.

The opt-in profile-sync plug-in owns:

- deciding whether the player publishes modules and talents;
- assembling revisions and submitting the typed profile;
- retry, consent, and revocation state.

The website owns:

- joining config/part/node IDs to an exact-build static catalog;
- localization and icon selection;
- module scoring and optimization;
- talent-tree layout and rendering.

This keeps optimization and display changes independent from packet capture.
It also prevents static names, icons, and formulas from being repeated in
every uploaded profile.

## Module profile contract

`CharacterProfilePatch.modules` contains:

- `equipped_slots`: module slot ID to string instance ID;
- `inventory`: only items from module package 5;
- each module's string instance ID, config ID, stack count, quality, load flag,
  module type, and level;
- ordered part IDs paired with their initial link values;
- ordered part-upgrade results and the reported success rate.

Module instance IDs are strings because game IDs can exceed JavaScript's exact
integer range. The optimizer must join by the string value and must never cast
it to a JavaScript `number`.

The saved Global Steam build `24252055` snapshot verifies 5 equipped slots,
649 module instances, 1,937 parts, initial link values on all 649 instances,
and 9,526 upgrade records.

The profile deliberately excludes account/login data, acquisition and expiry
timestamps, binding/source metadata, currencies, and arbitrary string effect
parameters. Optimizer formulas come from reviewed static tables instead.

## Optimizer execution model

RLogs now contains a portable Rust optimizer at
`plugins/games/blue-protocol-star-resonance/features/module-optimizer`. Its
scoring and search behavior is an attributed port of the AGPL-3.0 module
optimizer from `fudiyangjin/resonance-logs-cn` 0.2.0 at
`ccdeef23c7806be5072f95a9e80b103794af3544`.

Plugin Lab exposes the engine through a loopback-only JSON API and a real
browser screen. The page accepts an inventory array, the typed `modules`
profile object, or a full profile containing `modules.inventory`. The future
public site can place the same crate behind a server worker or a WASM wrapper;
neither packet capture nor CN's desktop/Tauri UI is a website dependency.

The optimizer input is:

```text
profile module inventory
  + exact-build module/part/effect/link catalog
  + selected role and scoring preset
  + user locks, exclusions, and minimum thresholds
```

The implemented result contains selected string instance IDs, complete module
records, threshold and total-link score components, attribute totals, search
statistics, and the exact catalog/scoring revision. Small searches use exact
enumeration. Full inventories automatically use bounded beam search with
suffix upper bounds and minimum-threshold feasibility pruning. Exact search
remains available for parity tests, and all result tie-breaking is
deterministic.

The CN-compatible scoring behavior is:

- sum each effect ID's link points across the selected modules;
- apply the catalog thresholds `1/4/8/12/16/20`;
- double threshold power for target attributes;
- remove threshold power for excluded attributes;
- enforce optional minimum attribute totals;
- add the `ModLinkEffectTable` fight value for total link points;
- select four or five modules and rank the highest scores.

Target weighting takes precedence if the same effect is also excluded, matching
CN 0.2.0. RLogs reads fight values from the exact-build catalog rather than
freezing current game values inside the web page.

The current-build static catalog now includes:

- all 12 `ModTable` module definitions, including the three current premium
  configurations;
- six `ModTypeTable` definitions and five `ModHoleTable` slot definitions;
- 21 `ModEffectTable` effects, 147 level rows, exact config records, official
  text, and icons;
- all 132 `ModEffectLibTable` relations;
- all 121 `ModLinkEffectTable` rows used for deterministic link scoring;
- 35 unique module item/effect/type/slot PNGs.

`ModInitializationTable` weights remain external evidence because its three
roll dimensions are not yet named. `AssessModuleTable` is an assessment-screen
configuration, not an optimizer formula. Neither is promoted as a scoring
fact.

## Talent-board projection

Runtime state already supplies:

- current profession;
- selected node IDs per profession;
- used points per profession when present;
- total talent points when present;
- talent stage configuration when present.

The website joins that state to static talent definitions containing class,
specialization, localized name/description keys, icon address, cost, effects,
tree-node ID, branch, and prerequisite node IDs.

The page should render:

- one class/specialization board at a time;
- all nodes in their game-like tree positions;
- selected nodes highlighted and unselected nodes dimmed;
- prerequisite edges and unmet requirements;
- used/available point totals;
- localized icon tooltips with cost and effect details;
- a compact accessible list view using the same data for mobile and screen
  readers.

The current catalog has 648 talent definitions, 1,350 exact tree nodes, 1,603
prerequisite edges, and the matching 1,603 reverse dependent edges. Every node
has its current-client `(x, y)` position. `TalentStageTable` assigns 990 nodes
to 27 active stages; 360 rows in `TalentTreeTable` are not referenced by a
current stage and are retained with
`layout_state = not_referenced_by_current_talent_stage`. Normal website boards
must render only active-stage nodes unless a research/debug view is explicitly
selected.

All 605 unique `TalentTable.icon_address` values were resolved and exported.
Shared addresses use one PNG path even when several of the 648 talent records
reference it. The normal board can therefore reproduce the game coordinates
and visuals without a fallback graph layout.

## Reference influence

The historical parser and website remain behavioral references rather than
dependencies or forks:

- `resonance-logs/resonance-logs@f4aff36e573674e04db1bb09216c603ddf9fb7f6`
- `resonance-logs/resonance-website@0baff9e4b625a11d09c9d579af19285695d38e12`

The original module page usefully separated player module state from static
module metadata and performed asynchronous candidate search. RLogs retains
that product behavior while replacing the historical whole-character JSON
storage/upload route with the typed allowlist above. The original website did
not provide the full game-like talent-board surface described here.

The CN 0.2.0 optimizer is the deliberate exception: its optimizer behavior has
been ported under the compatible AGPL-3.0 license, pinned to an immutable
commit, documented in the optimizer crate, and isolated from CN's parser and
desktop interface.
