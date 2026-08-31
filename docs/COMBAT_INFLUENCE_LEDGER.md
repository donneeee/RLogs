# Combat influence ledger

The influence ledger is the reusable evidence product behind rDPS. It answers:

> Which source changed which packet-observed damage event, and by exactly how
> much?

It is the authoritative damage-impact overview, not a build-planner
simulation. Its source may be a skill, status effect, Imagine, factor, talent,
set-equipment effect, passive, or another packet-proven mechanic. A build
planner may later consume the same proven relationships, but it must not become
a second formula authority.

The overview keeps two layers separate:

- the exact-build static relationship graph records which source/effect/formula
  relationships are possible and the evidence still required to prove them;
- the per-run influence ledger records which relationship actually affected a
  packet-observed damage event and the exact provider-removed damage delta.

Neither layer hides unresolved IDs. A static relationship never receives a
numeric contribution merely because its description or table linkage looks
plausible.

## Relationship identity

Every runtime-proven row retains:

- client build and protocol-pack digest;
- stable source identity and source-rule identity;
- effect ID;
- provider actor and entity identity;
- recipient actor and entity identity;
- affected damage/ability ID;
- damage source actor and entity identity;
- target actor and entity identity;
- unique affected canonical-event count;
- packet-observed damage; and
- the exact provider-removed counterfactual delta.

Large integers are decimal strings. Non-integer transfers remain reduced
rational terms. Consumers may format a decimal for display, but the integer or
fraction is authoritative.

## Authority boundary

Static client tables establish possible source, effect, component, formula,
and recount relationships. They do not authorize live rDPS. A runtime edge is
enabled only when the same client build proves all of the following through
canonical packet events and deterministic replay:

1. provider ownership;
2. recipient or target scope;
3. lifecycle, refresh, removal, and stacking behavior;
4. formula input and output transitions;
5. affected damage identity;
6. provider-removed counterfactual magnitude; and
7. party-damage conservation.

A mismatched-build log remains available as research evidence, but receives no
attribution from another build's runtime rules.

## Consumers

- rDPS aggregates externally provided offensive deltas by provider.
- Combat History displays the exact relationship rows for a selected run,
  player, segment, and target.
- Website reports may retain the same rows for log auditing and explanation.
- Cooldown or effect overlays may use the proven lifecycle edges without
  recalculating damage.
- A future build planner may use static and proven formula edges as inputs, but
  remains downstream from this ledger.

## Same-run observer reconciliation

The website can receive several sealed views of one exact game instance. It
must not merge those views by summing their combat events. Instead, one report
provides the canonical damage-event spine and the other reports provide
time-scoped evidence witnesses. This lets a character who was remote to the
spine observer contribute their own exact local profile, loadout, and attribute
state for the same run.

The stable game-run key includes deployment, region, scene, client build, and
the exact game instance ID. Protocol-pack digest is deliberately a replay-
compatibility gate rather than game-run identity: two parser versions can
observe the same server instance, but incompatible decoded protocols may not
be jointly replayed. Observer-local session IDs, wall-clock timestamps, player
names, durations, and damage totals are never used to guess a match. If exact
instance identity is absent, the upload stays isolated until a separately
proven server-event fingerprint can establish the same run without ambiguity.

Evidence precedence is event-local exact, same-run cross-vantage exact,
formula-bounded inferred, then unresolved. Every imported witness retains its
report and artifact digest. Conflicting exact witnesses stay visible and block
that fact; they are never averaged. Cross-vantage state may replace an inferred
input, but duplicate damage, healing, cast, or status events are never counted
twice. The reconciled influence ledger reruns conservation once over the
canonical spine and preserves each observer report unchanged beside the
derived result.

Local profile authority requires a canonical event marked
`personal_gameplay`; a public/social profile lookup is never promoted to local
state. The server records the latest qualifying snapshot at or before the run
start and every in-run change with its report, artifact digest, event sequence,
observation time, and payload digest. Multiple reports with different snapshot
sets remain unselected until their temporal relationship is resolved.

Selected personal profile events participate in the state-only replay: a
pre-run snapshot is placed immediately after canonical run entry, while an
in-run snapshot requires a strictly later canonical server game-time. This is
how a second observer can supply exact personal module/loadout state without
supplying duplicate damage, healing, cast, cooldown, or status events.

Once local character identity is established, exact entity and temporary
attribute snapshots/deltas for that runtime entity are committed separately.
The retained chain starts at the latest authoritative pre-run snapshot and
continues through run completion. Server game-time is the only cross-observer
clock accepted for automatic placement; observer-local monotonic time remains
provenance but cannot align two machines by itself. Secondary combat-result
events are never imported with this state chain.

The reconciliation gate therefore distinguishes a pre-run baseline from an
in-run change, reports participant/local-vantage coverage, and lists every
ordering blocker. Only a baseline or a game-time-aligned in-run event is
eligible for the eventual state-only merge. Inventory completion and conserved
attribution replay completion remain separate claims.

## Current-build static overview

The exact-build research batch writes
`damage-attribution-relationship-overview.v1.json`. It is a queryable graph of
source nodes and typed source-to-effect, source-to-component,
source-to-attribute, source-to-damage, and source-to-recount edges. Every
source-to-damage edge includes an amount record. That record remains explicitly
`not-computable-with-current-proof` with null amounts until matching-build
packet replay proves the affected event and counterfactual.

This graph is the bridge between current game-file research and later packet
proof. It is also the data source for a human-facing "what affected this
damage?" view. The UI should query or shard the graph rather than inventing a
second set of relationships.

## Current-build exhaustive party-route census

The build `24687926` party-route ledger is an exhaustive joined census, not a
shortlist of effects seen in one run. Its checked cardinalities are:

- 73 Aoyi parent skills and 218 reconciled descendants;
- 56 party-skill rows and 101 party-buff rows;
- 22 rogue/team-entry rows;
- 124 packet-observed external effects;
- 512 consolidated effect identities; and
- 1,584 exact ID/route rows covering 659 unique exact IDs.

Every route row carries origin, localization evidence, provider and recipient
scope, magnitude evidence, stacking, lifecycle, operation order, aliases,
runtime bindings, focused tests, reviewed disposition, and any remaining proof
obligation. The generator asserts those fields and cardinalities, including
zero missing runtime/config/test bindings for all 28 production-enabled effect
IDs. This is the coverage gate used to answer whether every promotion candidate
was actually reviewed.

Four offensive candidates remain deliberately fail-closed:

- `997520`, Energy Synergy Field resource gain: resource cap, overcap, spend,
  extra-action selection, provider overlap, and conserved scheduling remain
  unresolved;
- `2110060`, Swift Vortex: static data proves party scope, the 10-second field
  and 10-second linger, refresh-only nonstacking behavior, and the skill
  ownership route. Reconciliation schema 6 now seals exact paired
  Haste/action-speed application and symmetric-removal receipts behind a
  current-build identity and confounder gate. No qualifying receipt has yet
  supplied the missing magnitude, and the exact skill-to-status edge,
  lingering field arbitration, and action-opportunity conservation remain
  unresolved;
- `2110078`, Shock Defense Break: equipped tier selection and the exact
  defense/penetration/rounding counterfactual remain unresolved; and
- `2110092`, the shared Blade Sweep/Goblin March defense reduction: provider
  selection for the shared projectile plus the same defense-stage integer
  counterfactual remain unresolved.

Life Wave is tracked separately because it is a module-trigger relationship,
not a party-skill row. Build-locked game data proves its level-5/6 magnitudes
and five-second refreshable lifecycle. Runtime can identify self, unique
external, ambiguous, and unknown trigger ownership and marks affected totals
incomplete, but emits no guessed transfer until the selected highest-stat lane
and provider-removed damage marginal are exact. Same-run cross-vantage profile
and attribute witnesses are the evidence path for closing that gap.

## Exact-build research-journal audit

Before any candidate is eligible for counterfactual replay, the build-locked
research journal is folded into status lifecycles and effect/damage windows:

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-influence-journal-audit -- `
  --pack .\plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-24609362\protocol-pack-static-candidate.v2.json `
  --journal .\private-research\live-journals\<capture>.protocol.jsonl `
  --relationship-overview .\plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-24609362\damage-attribution-relationship-overview.v1.json `
  --output .\DEV_exports\<capture>.influence-journal-audit.json
```

The audit reads the journal incrementally with bounded memory. It requires an
exact build and protocol digest match and rejects any packet outside the
retained `World`/`WorldNtf` service set, including unrouted packets. Three
complete inventories are emitted independently of later correlation:

- every decoded cast identity, including action instance, base ability, and
  slot when the packet carries them;
- every packet-declared source-type/config-ID to status-effect edge, including
  provider, recipient, instance, stack, and lifecycle counts; and
- every decoded damage identity and amount, even when no decoded status is
  active.

Status/damage windows are a fourth, separate correlation layer. Packet-declared
durations are advanced across all journal records so idle time cannot preserve
an expired status. An absent window never removes a cast, effect edge, or
damage row from the independent inventories.

The audit also emits a compact `proof_queue`. Repeated observations are grouped
by status placement, effect ID, and affected damage ID. Each row preserves the
exact observed total and lists its remaining blockers: missing packet origin,
missing or ambiguous static source candidates, an uncatalogued source-to-damage
edge, the provider-removed counterfactual, or party-damage conservation. The
queue is diagnostic only and cannot promote a runtime attribution rule.

Correlation rows are deliberately labelled
`packet-cooccurrence-only-not-attribution-proof`. Exact origin/effect and damage
inventory rows carry their own non-attribution proof states. They narrow the
proof queue, but attributed deltas remain null until source ownership,
recipient scope, formula stage, provider-removed counterfactual, and
conservation are independently proven.

## Real-time boundary

The live meter and overlay must not load the research graph, proof queue, game
tables, or sealed-log replay machinery. Research and website validation may do
expensive counterfactual work. A separate compact runtime rule pack contains
only relationships already proven for the exact client build, with integer or
reduced-rational operations and bounded per-actor lifecycle state.

The online reducer consumes each canonical cast, status, and damage event once.
It emits provisional live rDPS immediately and may revise only the current
unsealed encounter when a late lifecycle event arrives. Historical and website
values remain authoritative only after the same rules pass sealed-log replay
and conservation. A rule that is formula-proven but not safe for bounded online
evaluation stays disabled in the overlay rather than adding latency or a guessed
approximation.

## Offline generation

The BPSR replay audit can process a single sealed log or a directory tree:

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-replay-audit -- `
  --rlog-dir .\runtime-data\logs `
  --summary-only `
  --output .\DEV_exports\damage-influence-overview.json
```

`--summary-only` drops per-event output after folding it into exact relationship
totals. It does not drop canonical input events or unresolved evidence. Files
ending in `.partial.rlog` are excluded; every included `.rlog` must have a
valid seal.

The bundle contains two complementary views:

- each report retains session-local provider, recipient, source, and target
  identities for a complete audit trail; and
- `relationship_catalog` folds all reports by exact deployment, client build,
  protocol digest, effect ID, and affected damage ID.

The catalog is the compact cross-run overview. It counts unique sessions and
damage events and preserves exact attributed totals. It never merges evidence
across game builds or protocol packs, and it labels only replay-proven rows as
proven. Static candidates and unresolved observations remain outside that
label until packet replay satisfies the authority boundary above.
