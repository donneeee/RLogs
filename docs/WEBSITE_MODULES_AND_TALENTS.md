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
records, actual threshold and total-link power, a separate preference ranking
score, attribute totals, the scored currently equipped baseline when supplied,
search statistics, and the exact catalog/scoring revision. Small searches use
exact enumeration. Full inventories automatically use bounded beam search with
three complementary candidate orderings, feasible greedy completion scores,
and minimum-threshold feasibility pruning. This avoids discarding threshold
synergies merely because their first partial set looks weak. Exact search
remains available for parity tests, and all result tie-breaking is deterministic.

The current-equipment baseline is not a recommendation and never consumes the
requested result limit. For example, a request for 20 results returns Current
plus up to 20 alternative module sets in both Plugin Lab and the website.

The browser interfaces select a conservative bounded-search beam width from
reported device CPU, memory, and mobile capabilities. The website runs the
shared Rust engine inside a Web Worker, so this adaptive CPU/WASM work does not
block the page. WebGPU is a future search backend and must not be claimed unless
the scoring and top-k reduction are actually running in a compute shader.

The CN-compatible scoring behavior is:

- sum each effect ID's link points across the selected modules;
- apply the catalog thresholds `1/4/8/12/16/20`;
- double preferred threshold contributions only in the internal ranking score;
- remove ignored threshold contributions only from the internal ranking score;
- enforce optional minimum attribute totals;
- add the `ModLinkEffectTable` fight value for total link points;
- select four or five modules and rank the highest preference scores;
- report actual module power without either preference adjustment.

Priority weighting takes precedence if the same effect is also ignored,
matching CN 0.2.0. RLogs reads fight values from the exact-build catalog rather
than freezing current game values inside the web page. The English optimizer
terminology is a reviewed alias layer; exact current-client names remain
available separately rather than being overwritten.

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

That is the complete submission boundary. A talent selection record contains
only its talent definition ID, original tree-node ID when the packet carries
one, and selected level when present. Profile submissions do not contain tree
coordinates, prerequisites, reverse dependents, names, descriptions, icons,
branches, specialization labels, or copies of unselected nodes.

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

The deployed website catalog now indexes all 9 professions and all 18 current
specializations. It owns 30 shared foundation nodes per profession and 60
specialization nodes per specialization (1,350 nodes total), with localized
names, icons, positions, and prerequisite relationships. The profile page
selects one of those boards from the submitted class/specialization identity
and highlights only the submitted node selections. No player upload is used as
a source of static tree shape.

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
