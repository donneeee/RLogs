# rDPS attribution

Auditable redistribution of contributed damage to buffs, mitigation, stat
changes, and other support effects under a versioned formula policy.

The canonical relationship model is allegiance-neutral and keeps two distinct
packet-observed edges:

```text
provider -> effect/status lifecycle -> recipient or enemy target
                                      |
                                      v
                           recipient damage action -> recipient or enemy target
```

The effect/status endpoint may be a teammate receiving support, an enemy under
a vulnerability or mitigation-changing effect, or another player-like entity.
The damage-action endpoint is equally allegiance-neutral: damage can target an
enemy, a teammate, or another entity. A source-side relationship exists when
the effect endpoint later performs the damage action; a target-side
relationship exists when a damage action lands on the effect endpoint. Neither
relationship is inferred merely from names or presumed allegiance.
Packet origin metadata may add an exact-build effect-family edge (for example,
buff -> child effect) when its numeric enum identity is proven for that build.
That edge is not automatically a skill edge, provider-ownership proof, or
formula authority; those relationships remain separate proof obligations.

Life Wave (`2404`, recipient child `2302421`) follows that same authority
boundary. Current-build static data proves that an HP or Max-HP change opens a
refreshable five-second window and that module levels 5 and 6 raise the
wearer's highest Crit, Luck, Mastery, Versatility, or Haste lane by 600 or
1,000 basis points. Canonical same-packet healing cohorts identify a unique
self trigger, unique external trigger, multiple candidates, or no observable
healing candidate; a later activation on the same instance resets the timer.
The projector preserves these ownership/timer facts but emits no Life Wave
transfer until the selected lane and provider-removed damage marginal are
exact. Damage under a unique-external, ambiguous, or unknown window is marked
`rdps_incomplete`; self-triggered windows remain self damage. This receipt is
preserved after run completion for live and historical views and cleared when
the next run starts. Future same-instance observer uploads may replace missing
remote state, but duplicate combat events never enter the canonical spine.
Remote-player cast packets are not required because the observing client does
not necessarily receive them; the timeline never synthesizes a cast or fills a
missing cast identity with zero. An unresolved status lifecycle remains combat
evidence and blocks rDPS transfer involving its effect endpoint until a clean
actor or run lifetime boundary, while ordinary damage totals remain unchanged.
Each retained damage edge carries its packet-observed damage actor, numeric
action ID, and damage target. A containing wire `SkillEffect` group is not an
action instance: one group may contain components from multiple action IDs and
multiple damage actors. Its packet/group/component indexes remain useful
container identity, but can never replace the unavailable remote action clock.
The BPSR party-effect audit schema 8 materializes those neutral relationships
per exact lifecycle window. Each grouped row retains the effect target, damage
actor and direct actor, numeric ability ID, damage target, first and last
canonical sequence/time, conserved event count and amount, plus bounded packet
samples. The two roles (`effect_target_is_damage_actor` and
`effect_target_is_damage_target`) are separate, so an enemy debuff and a party
buff use the same model without guessing allegiance. These are temporal
timeline joins only: every row explicitly denies causal/formula authority and
provider rDPS credit until the remaining proof gates close.

The raw BPSR support-timeline projection schema 11 uses the same allegiance-
neutral endpoints for status, damage, healing, shield, roster, and affiliation
rows. It stores a compact relationship row plus the sealed source RLOG sequence
instead of duplicating the full canonical event payload. The writer creates a
new `.partial` output, flushes and syncs it, then atomically renames it and
refuses to overwrite either an existing final or partial artifact. The source
RLOG remains the lossless canonical authority.
Schema 11 also makes an exact numeric effect filter and its optional related-
damage join explicit in the manifest and every retained relationship row, so a
small effect-specific audit cannot be confused with an unfiltered projection.

For build `24687926`, the absent remote-player cast-notification route is an
explicit structural non-obligation: the local capture surface does not receive
that packet family, its absence is not a zero cast count, and damage packets do
not synthesize provider casts. Protocol event coverage is now bound to a
receipted gap-free source segment (`1114` retained packet records, zero selected
gaps): all `11` locally observable migrated decoder routes validate exactly and
the one remote-player route remains a structural non-obligation. The installed
`protocol-packs/global/steam-24687926/pack.json` is byte-for-byte identical to
that audited candidate. This closes protocol-pack identity and observable event
coverage only. The earlier 26-recording corpus still discloses `712` actual
capture gaps, and the exact-pack segment does not prove events outside its
source interval or a closed encounter lifecycle. Its `114` damage actions
conserve `2,789,996` ordinary damage with zero transferred credit, but
closed-lifecycle canonical replay and formula-specific counterfactual
conservation remain open; runtime rDPS promotion is still forbidden.

The build audit can now materialize a complete candidate snapshot for this
checkout instead of stopping at stale artifact identities. The plan contains
`48` inputs: all `46` required inputs and one of two optional inputs are
present, so the snapshot contains `47` hashed inputs. The current candidate
snapshot is retained with the generated build-audit outputs rather than copied
into a runtime catalog. Diffing it from the reviewed build-`24252055` baseline
changes all `12` legacy inputs and adds `35`, spanning `10` domains. That diff
requires `15` proof suites. The current gate verifies six suites: build
identity, combat-table diff, damage-stage event coverage, formula-surface diff,
protobuf coverage, and observable protocol-event coverage. Nine semantic or
closed-replay suites plus review approval remain blockers, so
`runtime_promotion_allowed=false` remains mechanically enforced.

Two runtime catalogs were migrated without copying historical authority
forward. The build-`24687926` external-state catalog has zero active rules;
effect `2404261` remains a historical packet-formula candidate whose unchanged
static lineage is retained, but matching-build runtime activation,
provider-recipient replay, and conservation are absent. The Psychoscope source
receipt binds `7` exact current-build inputs and `9,293` retained static rows,
while the prior build's `12` reviewed factor rules remain disabled candidates.
Current-build factor selections are preserved as unresolved, but those
historical magnitudes do not become formula inputs or provider rDPS.

Event-time table context obeys the same direction-of-time rule. A positive
numeric season ID from a BPSR `CharacterProfileObserved` event may contextualize
only later status rows in that same sealed RLOG. A profile observed after the
status is retained as later evidence but is never backfilled, and a current
character snapshot is never substituted into an older run. The build-24687926
effect-`31602` cohort proves the importance of this gate: none of its `130`
status lifecycle rows has a prior season observation in the same RLOG; `74`
have season `3` only later and `56` have no season observation in their RLOG.
The consecutive continuous-monitor chain does provide an earlier season-`3`
candidate for all `130` rows. The schema-3 receipt now classifies every
intervening gap route: all `130` candidate rows cross decoder-failure gaps,
`107` also cross TCP gaps, only `40` have a gap-free season-source wire lane,
and only `23` cross no capture/TCP gap kind. That forward-only candidate is
retained separately. The exact-build Lua mutation/lifecycle proof scans all
`4,821` bytecode files (`29,285,269` bytes), selects and decompiles `379` route,
lifecycle, or literal-`DataMgr.Clear` candidates with zero failures, and finds
exactly two direct `CurSeasonId` writers. World method `27` feeds
`RefreshSeasonData -> CurSeasonId = seasonId`; `SeasonData.Clear` writes zero.
The exact Lua control flow also proves that normal reconnect calls inherited
no-op `SeasonData.OnReconnect`, while initialization and explicit logout clear
the model. The only direct literal `DataMgr.Clear()` callsite is the login
logout path. Dynamic or aliased writers and the absence of an explicit logout
or reinitialization between monitor segments remain unproven, and matching-
build protocol coverage is not promoted. Therefore the static season-3
transform row is not yet event-time formula authority for those rows and
authorizes zero provider credit.

Observed lifecycle shape is not substituted for server stacking semantics. For
effect `31602`, the bounded schema-1 stacking frontier streams the sealed
schema-8 window audit and finds `65` complete applied-to-removed windows, all
reporting stack `1`, with zero overlapping windows and maximum same-target
concurrency `1`. The exact BuffTable integers `RepeatAddRule=[1,1]` and
`TimeRefreshType=0` are retained, but remain opaque until their exact current-
build server meaning is proven. Those observations do not prove how distinct
providers would arbitrate, operation order, or downstream rounding.

Exact native action-speed evidence is tracked independently from that stacking
gate. For effect `31602`, closure schema `45` records the exact current-build
float32 operation order, temporary-attribute lookup ABI, native sampling point,
and scheduler speed-scaling route across `3,713` responsive damage-action
memberships. Its `54` provider-removed conditional-capacity groups are retained
as hypotheses only: bit-equivalent replay with every per-action input, action
opportunity, packet-clock correspondence, integer rounding, and conservation
remain open, so observed damage reassigned to a provider remains `0`.

Provider identity is likewise a separate gate from formula and conservation.
The current-build 26-RLOG ownership receipt covers all `14,760` lifecycle rows
for the seven packet-observed party effects (`31601`, `31602`, `55228`,
`2110124`, `2110125`, `2110140`, and `3200038`). Every sourced event resolves
to an event-time player actor and stable character ID, including explicitly
audited owned-entity and prior-instance routes for effect `55228`. Closure
schema `45` joins that receipt only after the exact cohort and event counts
match. It still grants zero provider credit until recipient scope, magnitude,
operation order, stacking, game-specific integer rounding, and conservation
are proven for the same build.

Proof-closure schema `50` resolves the role of effect `55228` without inventing
allegiance: the effect endpoint is a `damage-target-allegiance-neutral` node
joined to the target side of recipient damage actions, so it does not require
party membership or an enemy classification. Its schema-2 formula receipt
acknowledges the installed exact-build protocol pack, complete coverage of all
`11` locally observable migrated routes, and ordinary-damage conservation on
the reviewed gap-free segment. It still grants no formula, runtime, UI, or
provider-credit authority because closed-lifecycle canonical replay,
formula-specific counterfactual conservation, exact scalar, operation order,
stacking/provider split, and integer rounding remain unproven.

Closure schema `51` also binds a bounded exhaustive decoded-table ID surface:
all `577` current-build JSON tables (`172,314,811` bytes) are scanned one at a
time for every rDPS-relevant party identifier, retaining `1,242` exact
occurrences with zero omissions. It proves the typed `2209 -> 220901` skill
edge, but finds status `55228` only as a BuffTable identity and an unrelated
CollectionTable identity. Thus neither a typed `2209 -> 55228` edge nor an
exact-ID-linked scalar is invented. Action `2203291` stays on the recipient
damage-action measurement edge and is never treated as the provider skill.

Closure schema `53` makes both joins allegiance-neutral. Target-side support
resolves as `damage-target-allegiance-neutral`; recipient-side support resolves
as `damage-actor-allegiance-neutral` when the exact effect endpoint is the actor
of the conserved downstream damage edges. Party membership remains distinct
scope evidence and cannot change the endpoint role. All seven packet-observed
party-effect families now have a proven actor-side or target-side timeline
role, while every arithmetic and conservation gate remains fail-closed.

The same closure retains exact current-build Imagine scalar receipts without
promoting them prematurely. Effect `2110124` is proven to be a provider-side
routing marker and therefore transfers zero damage by design. Effect `2110125`
has the exact tier equation `500 + tier_attr_per` basis points and a tier-5
packet attribute oracle, while effect `2110140` has exact tier parameter pairs,
including the numeric loadout-tier-`0` base pair `[750,1000]` from
`SkillFightLevel` row `397101` and tier-`1` through tier-`5` combined pairs
from the current `SkillAoyiStar` rows. It also has the primary-stat runtime
input route and exact static talent-opcode candidates: Strength or Agility to
ATK uses `trunc(primary_total * 1250 / 10000)`, while Intellect to MATK uses
`trunc(primary_total * 1000 / 10000)`. Those static class routes are not by
themselves the complete packet marginal of a support effect. Event-time provider tier, recipient
snapshots where required, final damage-stage order and rounding, affected-hit
selection, and conservation remain open. No formula, runtime, UI, or
provider-credit authority follows from those scalar receipts.

The bounded effect-`2110125` timeline projection now closes the next Fatal
Spiral identity slice without changing that arithmetic gate. Across six sealed
build-`24687926` runs, all `394` lifecycle rows join to one of seven exact
run-local provider/session loadout observations: `202` applications produce
`192` closed windows and `10` preserved-open windows, with no tier filled from
a later profile. The source-side join retains `96,684` unique recipient damage
actions as `100,774` provider-window candidate edges; target-side joins retain
another `647` damage actions separately and make no allegiance inference. Of
the source-side candidates, `95,457` are external-provider edges and `5,317`
are provider-self edges. The strict single-external, elemental-property
candidate subset contains `83,267` events, but it remains a candidate subset:
the exact combat consumer, stacking behavior, damage-stage order, integer
rounding, counterfactual projection, and conservation replay are still open.
No rDPS credit or UI formula is enabled by the tier/window proof alone.

Closure schema `63` now joins a separate schema-`11` source-side damage-stage frontier for
Fatal Spiral. The same six sealed RLOGs contain `394` selected lifecycle rows,
but `173` applications cross a decode or TCP data-quality boundary and are
excluded rather than treated as continuous. This leaves `29` complete,
gap-bounded lifecycles with `27,238` audited damage-event memberships. The
formula replay matches all `27,238` memberships at the status recipient ->
damage actor endpoint and retains `27,001` eligible packet formula samples.
Exact generic-element attribute `13100` values `[316,1316]` are observed in
`6,753` retained samples, while the full attribute family `13100..13105` stays
in the control surface. Exact, relaxed, near-controlled, and cross-entity
searches still find zero otherwise-identical effect-present/effect-absent
pairs. A separate bounded receipt then partitions all `92` damage action IDs
seen in those windows into `10` high-volume and `82` remaining actions and
searches all `26` retained current-build RLOGs with both active and absent
states preserved. It reviews `488,546` samples (`169,944` beyond the six logs
that contain the effect), `68,110` exact effect-present groups, and `12,176`
broad absent candidates. All absent candidates are rejected: `23` lack the
required attribute transition and `12,153` carry source-status co-transitions.
The additional `20` RLOGs add no new structural absent candidate. This closes
the retained-capture search frontier, not the formula. The receipt therefore
closes event-time tier, affected-row selection,
and gap-bounded replay selection only. It explicitly leaves the combat
consumer, multiplier application, operation order, integer rounding, stacking,
counterfactual projection, conservation, runtime authority, UI authority, and
provider credit false.

The schema-`11` frontier also exhausts the ten exact-build `.partial.rlog`
prefixes without pretending that they were originally sealed. Nine non-empty
prefixes contain `1,039,616` reader-validated events; deterministic recovery
copies append exactly nine derived terminal capture-gap boundaries and are
then sealed and replay-verified. The derived seals authenticate that
transformation, not the original capture. Gap-aware replay retains `23`
complete `2110125` lifecycles with `16,376` source-side damage memberships.
An expanded comparison over `89` observed action IDs retains `92,161` samples
and finds zero controlled pairs. Its broad review band contains `57` pairs,
all rejected because the required `13100..13105` source-attribute transition
is absent. This closes the recovered-prefix search frontier while keeping
source-capture integrity, formula, runtime, UI, and provider-credit authority
false.

That frontier now also carries a versioned exact-build all-element consumer
search receipt. Build `24687926` proves the `13100..13105` family identity and
packet-state equations, retains all `2,192` decoded DamageScript candidate
rows, scans `4,821` Lua files, and audits the three exposed native
`AttackSimply` getters. The reviewed client inventory explicitly lacks the
server damage operator; the Lua names occur in one generated definition and
the selected getters have zero direct callers. Because indirect or unnamed
consumers are not claimed absent, this closes the reviewed client search
surface rather than the combat formula. The remaining acquisition route is an
authoritative server operator or a controlled same-build effect-present versus
effect-absent damage pair with complete event-time inputs.

The schema-`11` frontier also performs an instruction-level search for exact
native immediates `13100..13105`. Of `21` raw matches in executable sections,
only two decode as actual immediate operands inside a bounded IL2CPP method;
both are value `13104` in the unrelated
`APJSteamImp.<UploadSteamOrderId>` callback. No combat-relevant exact-immediate
consumer is found. This still does not exclude computed IDs, indirect calls,
table-driven selection, or protected/VM code, so it narrows the native search
without granting formula authority.

The all-element consumer receipt is now schema `3` and also indexes the
`370,680` generic-instantiation entries omitted by the ordinary dump method
index. Seven exact attribute-getter RVAs yield `14` raw relative-call
candidates, of which bounded method disassembly confirms `8`; all eight call
`ZAttrCollection.GetIAttr<int>`. Six pass a runtime-derived attribute index.
The only two literal indices are `227` and `0xC0000000`, neither in
`13100..13105`. The seven methods have nine exact preferred-image pointer
slots, but a bounded `34,960,159`-instruction scan finds zero exact
RIP-relative references to those slots. This closes the direct generic-call
and exact pointer-slot-reference layers only. Indexed metadata dispatch,
runtime-derived IDs, table-driven selection, and protected consumers remain
open, so formula, runtime, UI, and provider-credit authority stay false.

Schema `15` also carries a watch-ready sealed-candidate frontier. Recursive
discovery is capped, `.partial.rlog` names are excluded, every candidate must
match build `24687926` and replay to its canonical seal, and known inputs are
deduplicated by sealed-content hash. The current directory contains `55`
sealed-name files: `26` are exact-build sealed runs, `6` observe effect
`2110125`, and `3` contain complete source-side damage windows. Those three
reproduce the reviewed `29` lifecycles and `27,238` damage memberships and are
all already known, so the live refresh trigger is off. A one-RLOG unseen-seal
positive control turns it on. The corrected source-side transition audit then
checks `695,592` opposite-state comparisons and finds `229` matching damage
contexts. The raw closest pair changes `14` independently observed state
dimensions. Of those `229` pairs, `69` contain the configured endpoint
transition, and every one changes exactly `13100`, `13101`, and `13102`; none
changes `13103..13105`. Removing that proven family diagnostically leaves a
minimum of `13` independently changed dimensions. Exact-build IL2CPP enum
evidence resolves the dominant residual IDs as `EAttrType::AttrPos = 52` and
`EAttrType::AttrTargetPos = 53`; retained-raw replay decodes `131,558` and
`142,130` position payloads respectively. Spatial state remains matched because
range, direction, area, or falloff consequences have not been excluded. Zero
pairs are fully explained by the family plus permitted diagnostics, so no
strict controlled pair survives. Remote casts are not a
dependency, no missing state is zero-filled, and the receipt grants no formula
or provider credit.

The schema-`16` action/spatial receipt also collapses the `69` configured
transitions from `36` packet-component identities into `7` exact numeric action
selectors. Six selectors covering `68` transitions have a current-build static
formula candidate whose `EDamageSource` route matches the packet. The remaining
`220101` transition is explicit `EDamageSourceFakeBullet = 4`, while its tempting
`BulletTable` candidate requires `EDamageSourceBullet = 1`; it is rejected from
that route and retained unresolved. Component index/count, localized names,
empty ranges, zero weights, and separate range-attenuation script names do not
become formula inputs or prove position independence. This improves action
auditability without changing the zero-credit result.

The same receipt now consumes an exact-build fake-bullet lifecycle frontier.
Native protobuf proof fixes `AoiSyncDelta.FakeBullets` at tag `11` and all
seven `FakeBulletInfo` fields (`Uuid`, `BulletId`, `TargetId`, `PartId`,
`Offset`, `Rotate`, and `SkinId`). Canonical event schema `9` emits these as
`unresolved_action`, retaining the enclosing AOI entity, numeric instance and
action IDs, target, target part, and original bytes. The enclosing entity is a
container relation, not a proven provider. Historical canonical logs are not
backfilled, the reviewed current-build cohort has zero observed lifecycle
joins, and no ordinary damage event or provider credit is synthesized.

Schema `16` additionally replays all six relative relations among source and
target `AttrPos`/`AttrTargetPos`. The direct source-position to target-position
relation is complete for `66/69` transitions, but only `2/66` preserve the exact
displacement vector and squared distance. `64/66` therefore have observably
different direct geometry; `12/66` even differ by more than one raw coordinate
unit in distance. The tolerance counts are diagnostics, not promotion rules,
and exact equality would not by itself prove every spatial damage input equal.
Position remains an exact counterfactual dimension.

The schema-`16` receipt defines that packet-pair acquisition contract exactly.
Its primary tier-5 pair holds build, protocol pack, session/run/scene, damage
actor, target, numeric action identity, complete non-candidate source state,
complete target state, provider state, and packet calculation fields equal.
Only lifecycle `2110125` and its proven `316 -> 1316` transition on attributes
`13100`, `13101`, and `13102` may differ; `13103..13105` must remain equal.
Missing values, shield absorption, ambiguous damage surfaces, unrelated status
changes, or data-quality boundaries reject the pair. Exact integer preimage
intervals are defined for floor and nearest-half-up final-multiplier candidates,
but candidate compatibility remains diagnostic until replicated pairs select a
single operation, bind its subtotal to a proven stage, and conserve packet
integers.

Counterfactual analyzer schema `17` now executes those preimage tests
automatically for every qualifying source-transition pair. It evaluates both
floor and nearest-half-up fixed-point candidates against the intersection of
the absent and present subtotal intervals and preserves per-variant rejection
evidence. The full retained `26`-RLOG comparison frontier still supplies zero
qualifying pairs, so no
candidate is selected and damage-stage binding, operation order, rounding,
conservation, runtime, UI, and provider credit remain explicitly false.

Exact-build client files expose a hidden damage-control panel with the right
experimental axes: exact target selection, `addBuff`/`delBuff`,
`addGMAttr`/`clearGMAttr`, forced target skill use, training-hall entry, and a
packet damage view. Those commands route through `zproxy.world_proxy.GMCommand`.
They are not an executable production acquisition route: the shipping
`Panda.Core.Wrap.GameContext.IsBlockGM` getter returns true at reviewed RVA
`0x45DF690`, and `SubmitGmCmd` exits immediately when that flag is set. The
schema-`8`/closure-`60` receipt therefore records the panel as an authorized
internal/QA acquisition option only. It neither authorizes bypassing the guard
nor assumes that a production account can submit or that a server would accept
the commands.

A separate exact-build access-frontier receipt now resolves the apparent
training-hall shortcut. Numeric scene IDs `10001` and `10002` are exact
`SceneTable` rows for the two localized “Square Practice Field” scenes, but
neither has a `DungeonsTable` entry. A constant-level scan of all `4,821` Lua
chunks (zero parse failures) finds `TrainingHallId` only in the generated
global and the same hidden damage-control panel; it finds no ordinary UI or
service Lua entry route. Empty scene entry-condition arrays do not prove
ordinary account access, and absence from the reviewed routes is not claimed
as server-denial proof. These scenes therefore remain an unexecutable capture
lead unless authorized access is independently demonstrated; organic
same-build pairs in any observed scene remain valid under the existing strict
capture contract.

The event-time input frontier does not treat a remote actor's projected
loadout as a tier oracle. In the exact 26-RLOG build-`24687926` cohort, all
`136` effect-`2110140` activations lack an exact equipped provider tier from a
run-local exact loadout snapshot. A
representative provider row does retain ability `3971` and item `3000123`, but
the loadout packet evidence is explicitly `unobserved`; that row is preserved
as unresolved loadout evidence rather than promoted from the current profile.
Tier `0` itself is now a valid numeric base tier when it is observed exactly;
it is no longer rejected merely because earlier manifests listed only tiers
`1` through `5`. Recipient inputs are
selected separately through the exact-build class-to-primary/attack routes.
Of the same `136` activations, `26` have both event-time class-selected inputs,
`109` have a class but no current selected attribute values, and `1` lacks a
current class. All `26` complete cases select class-`11` Agility `11030` and
physical attack `11330`; they do not require an irrelevant magical-attack
snapshot. Actor spawn/despawn or entity-identity replacement clears this
formula state, so a reused actor ID cannot inherit an older class or attribute
value. The lifecycle fields also cannot stand in for the missing tier: every
observed application reports level `1`, duration `15000`, stack `1`, and count
`-1` even though the exact static table has six loadout-tier routes including
the base tier.

A separate schema-29 same-wire attribute audit supplies an exact but narrow
tier discriminator without requiring a remote cast. Across all `272` effect
`2110140` status events, exact single-effect removal equations pair attribute
`11034` (main-stat raw percent) with attribute `11802` (Healing Received add)
for `8` exact provider/status-instance/recipient lifecycle occurrences. Two
occurrences from one exact player provider are `[750,1000]` and uniquely match
loadout tier `0`; six occurrences from another exact player provider are
`[1500,2000]` and uniquely match tier `5`. The occurrence-scoped tier receipt
does not propagate either tier across time, other recipients, or the remaining
`128` applications. Healing Received remains a healing-only lane and never
becomes damage rDPS. Closure schema `47` retains the eight resolved occurrences
and all unresolved applications, while keeping the global provider-tier gate,
formula/runtime/UI authority, damage reassignment, and provider credit false.

Closure schema `48` joins those same eight occurrence-scoped tiers to the
canonical allegiance-neutral timeline without inventing a remote cast. All
eight have an exact `applied` -> `removed` lifecycle and a same-session,
same-application-sequence recipient snapshot for class `11`, Agility `11030`,
and physical ATK `11330`. Inside those exact windows, `12,557` canonical damage
actions are owned by the affected entity and retain their numeric action ID and
their `recipient or enemy target` endpoint. The actions span `66` distinct
endpoints; none is classified as friendly or hostile from topology alone. All
`12,557` rows have one active effect-`2110140` provider, and retain
`1,923,279,061` observed HP loss plus `1,947,659,979` reported damage as input
evidence only. No damage counterfactual is calculated and no provider credit is
granted because the primary raw-percent evaluation base, attack update order,
downstream damage stages, integer rounding, remaining `128` tiers, and
conservation replay are still open.

The latest schema-`48` closure receipt adds exact current-build attribute
transition boundaries for those eight lifecycles. The status lifecycle remains
canonical evidence, but damage eligibility begins only after the recipient's
exact stat-activation packet and ends at the stat-deactivation packet. Three
activations trail the status application by `80,253`, `89,451`, and `90,407`
microseconds. Applying the packet boundaries removes `10` premature actions
from the candidate status windows, leaving `12,547` allegiance-neutral damage
actions with `1,922,308,306` HP loss and `1,946,689,224` reported damage.
Across the `16` activation/removal boundaries, `15` exactly satisfy
`delta(ATK add 11332) = floor(after Agility 11030 * 58 / 100) -
floor(before Agility 11030 * 58 / 100)`. The remaining activation retains a
same-packet ATK-percent `11334` change and misses that marginal by one integer,
so it is explicitly unresolved rather than rounded away. The static `1/8`
talent route matches none of the `16` complete packet marginals and therefore
cannot be substituted for this effect's observed attack delta. These receipts
still calculate no damage counterfactual and grant no provider credit.

A schema-`10` four-session timeline slice now retains the exact packet damage-
stage inputs on those neutral action edges: `owner_level`, zero-based
`owner_stage`, damage source/type/flags/property, normal and lucky values,
passive/damage mode, and skill-effect component indices. Missing fields remain
`null`; they are never defaulted to zero. After the effective stat-window
filter, `12,422` of `12,547` actions resolve to one exact current-build
`DamageAttr` row and `125` remain unrouted. Exact coefficient input candidates
can be selected for `12,353` actions: `6,882` use stage-invariant one-value
vectors, `490` carry an explicit packet stage, and `4,981` preserve a raw null
packet field while separately applying the current-build optional-protobuf-
scalar semantic-zero candidate. That semantic candidate does not synthesize a
packet field and remains non-authoritative until exact formula replay validates
it. The remaining `194` actions comprise `125` unrouted rows and `69`
`AttackLucky` rows with no standard coefficient vector.
Fixed-parameter inputs can be selected for `12,389` actions while `158` remain
unresolved. Of the routed actions, `12,116` use the standard `Attack` script
and `306` use nonstandard scripts (`AutoAttack` or `AttackLucky`), which are
preserved under their own identities. Row selection is input evidence only:
damage formula stages, stat snapshots, operation order, stacking, rounding,
counterfactual damage, and conservation are still open, so provider credit is
still zero.

A schema-`45` bounded counterfactual audit now isolates the two clean tier-`0`
recipient lifecycles with exact same-wire `11034 -750` removal seeds. It finds
`47` before/after damage-pair candidates, but every candidate changes another
packet-observed status; strict controlled pairs therefore remain `0`. In the
separate event-time lane, `133` active damage actions totaling `20,443,392`
reported damage carry current Attack and the proven provider delta, but none
contains every component required to replay the full Attack family. Attributes
`11035`, `11333`, and `11334` are absent from all `133` event-time vectors,
while the other seven required primary/Attack components are present in all
`133`. The audit records all `133` failures and their damage under an explicit
missing-component coverage reason instead of silently dropping them or
zero-filling the missing fields. Exact counterfactual damage and provider
marginal therefore remain `0`; this receipt is blocker evidence, not formula,
runtime, UI, or provider-credit authority.

Schema `46` then bypasses those unobserved component fields without defaulting
them: each clean tier-`0` lifecycle is replayed separately with its exact
same-packet final-ATK removal marginal (`346` for run `0004`, `345` for run
`0010`) and its own complete transition seed. A final-ATK delta is rejected by
the proof tool unless that occurrence-scoped seed and recipient UUID are
supplied. All `133` selected damage actions now have an exact final-ATK
reversal, numeric action/hit identity, one current-build `DamageAttr` row, an
exact stage coefficient, and an exactly conserved rational Attack-stage share,
covering `20,443,392` observed damage. Seventy actions admit a compatible
downstream integer-factor interval and `67` yield one result within that
interval, but no action has one exact counterfactual across every remaining
candidate. Exact counterfactual damage and provider marginal therefore remain
`0`; the conserved rational shares are retained as offline candidate evidence
and are not yet runtime or UI attribution.

An action-wide schema-`2` diagnostic prevents those `67` locally unique
interval results from being mistaken for a general formula. The bounded
partitioned analyzer scans all `735,016` current-build cohort samples and finds
`19,858` exact ability-`2203521`/hit-`5` actions, whose unique current-build
row is `DamageAttr 2220352105` (`Attack`, coefficient `20000`, no fixed term).
No selected sample carries any of the audited physical, magical, refined, or
elemental target-mitigation axes; absence is retained as packet coverage, not
as zero defense and not as an enemy classification. Of the `16,518` actions
with event-time final Attack `11330`, `10,835` admit an integer factor interval
for `observed = floor(floor(Attack * 20000 / 10000) * factor / 10000)`, while
`5,683` are mathematically incompatible with that one-factor model. The exact
action family therefore disproves a universal one-integer-post-base-factor
shortcut. The observed owner context is identical across all `16,518`
eligible actions (`owner_level=1`, no packet `owner_stage`), so event-time
owner tier does not explain the split. Nor do the provider-window-like packet
flags: the `14,839` critical, non-lucky, property-`7` actions contain `5,046`
rejections, and `1,285` complete observed calculation-context buckets contain
both compatible and rejected actions. The missing discriminator is therefore
an unmodeled formula stage/state input or different operation order/rounding,
not target allegiance. The two tier-`0` provider windows remain a compatible
diagnostic subset only; their counterfactual and provider credit remain zero
until that actual multi-stage operation order and integer rounding are proven.

Controlling by the complete retained event-time state does not manufacture that
proof: the `16,518` eligible actions produce `16,515` distinct combinations of
observed calculation context plus source/target attribute-state and status-state
IDs. None of those near-event-unique rows mixes compatible and rejected actions,
so state identity separates observations without supplying repeated controlled
witnesses. Exact current-build critical-damage attribute `12510` is present for
all `14,839` critical actions. Testing both retained interpretations
(`10000 + 12510` and direct `12510`), both floor and nearest-half-up, and both
positions around one unresolved integer factor leaves thousands of arithmetic
rejections in every candidate. That rejects each tested two-stage reduction as
a complete action-wide formula, but does not select a critical interpretation
or prove the server's larger multi-stage pipeline.

The bounded full-cohort follow-up makes that rejection exhaustive for the four
complete retained source stages on this action. It emits `14,810` exact numeric
observations carrying Attack-derived base/output plus attributes `12510`,
`11940`, `12550`, and `13170`, then evaluates all `24` stage orders, all `16`
floor/nearest-half-up assignments, both retained critical interpretations, and
all `5` placements of one unresolved nonnegative integer factor (`3,840`
models). No model has zero rejections; the best admits only `3,705` observations
and rejects `11,105`. This is strong negative formula evidence, not permission
to pick the least-bad candidate. The observation input and ranked rejection
receipt remain replayable, while formula/runtime/UI authority and provider
credit stay false until the missing stage applicability or additional stage is
proven independently.

Retaining every packet-present candidate source-stage ID permits a stricter
presence split without re-reading the cohort. The five observed optional-field
patterns also have zero complete candidates: no optional fields rejects at
least `3,860/5,948`; `13100` rejects `3,240/4,529`; the proven alternative
representations `11840` plus `11950` reject `2,782/3,660`; those three fields
together reject `400/592`; and zero-valued `12670` plus `13100` rejects
`50/81`. IDs `11840` and `11950` remain a derived-value alias pair and are
never multiplied together. This rules out optional-field presence as the
missing selector; it does not rule out a separately proven stage consumer,
non-basis-point curve, snapshot, flat term, or target-side input.

The exact action's own status lifecycle is now retained rather than treated as
an unrelated packet stream. A streaming schema-1 relationship receipt reads a
four-session schema-10 timeline under a `256` MiB Node heap and finds `7,561`
effect-`2203521` transitions and `2,709` exact action-`2203521`/hit-`5`
damage rows. Every selected action has a preceding same-effect lifecycle whose
affected entity equals the damage target, and in all `2,709` rows the lifecycle
provider equals the damage actor. The exact observed source configs are
`2203520`, `2203620`, and `2203670`; they are preserved separately rather than
collapsed to the localized Steel Beak name. `1,661` actions share the capture
sequence of their nearest transition, but proximity alone is explicitly not
causal ancestry, formula authority, or provider-credit authority.

Packet origin takes precedence over effect-only fallback ownership. If a
status carries an exact `(source_type_id, source_config_id)` pair that is not
present in the current-build origin catalog, the resolver retains that pair as
`uncatalogued_packet_origin` with unresolved endpoint and owner identity. It
must not fall through to a different source merely because the effect ID has a
single fallback candidate. The previous resolver violated this rule when a
known effect carried an origin absent from its older three-session catalog;
that path now fails closed. Events without any packet origin may still use the
explicitly weaker effect fallback.

The retained `26`-session current-build reconciliation closes four distinct
effect-`2203521` packet origins against the exact-build static source graph:
`1:2203520` has `22,530` observations and resolves to `talent:1152` (Steel
Beak), `1:2203620` has `4,166` and resolves to `talent:1162` (Light Prism),
`1:2203650` has `59` and resolves to `talent:1165` (Steel Beak Strike), and
`1:2203670` has `5,285` and resolves to `talent:1167` (Chain Explosion). The
generated identity catalog is migrated from that full corpus: it retains
`1,699` effects and `616` packet-origin fingerprints, loses zero effects and
zero origins from the prior `521`/`144` catalog, and its zero-omission audit is
complete. All four owners are exact only when their numeric packet origin is
present; the origin-less effect fallback is now correctly ambiguous across the
four candidates. This is identity closure, not rDPS formula promotion. Effect
`2203521` remains `scope_unproven`, with no scalar and no external
provider/recipient lifecycle, so it grants zero provider credit.

An exact session-plus-canonical-sequence join attaches that lifecycle context
to `2,406` of the `14,810` source-stage observations. It is relevant but not
sufficient: conditioning repeated source-stage contexts on source config,
transition state, and stack count reduces conflicting observations from
`2,020` to `649` and the largest distinct-output set from `30` to `8`. Even
adding target entity identity leaves `141` conflicting repeated contexts
covering `426` observations, with as many as `3` outputs in one fully retained
context. Adding the exact retained source/target status-state IDs reduces that
to `55` conflicting contexts and `227` observations; adding the complete
retained attribute-state IDs changes neither count. Finally, full packet
calculation context (flags, component indexes, damage mode, and source shape)
makes all `2,406` joined contexts event-unique, leaving zero repeated controls.
That fragmentation is not formula proof. The remaining server-side operator,
snapshot, operation-order, or rounding inputs and repeated witnesses must
therefore be recovered; no stack-based shortcut is promoted.

A leave-one-field-out audit of all `18` retained packet calculation-context
fields isolates only `skill_effect_component_index`: adding that field alone
splits the `55` conflicting contexts into event-unique rows, and omitting it
from the complete context restores all `55`. This is container ordering, not a
proven damage input. Across the conflicting groups, adjacent outputs move up
`30` times, down `30` times, and remain equal `112` times as component index
increases. No monotonic component-index multiplier exists in this evidence.
Several output pairs retain a near-`10/11` diagnostic ratio, but the ratio is
not universal and no current-build `Attack` row or proven consumer ties it to
an attenuation, vulnerability, random-roll, or rounding stage. It remains an
explicit server-operator lead with zero provider credit.

The container-order lead has now been resolved as topology, not arithmetic.
For `SyncNearDeltaInfo`, the decoder obtains `skill_effect_group_index` by
enumerating the message's target deltas; for each target delta it obtains
`skill_effect_component_index` by enumerating that delta's `skill_effects.damage`
array. A bounded schema-10 timeline pass joins every one of the `2,406`
selected observations while buffering at most `1,938` combat results in one
capture. Packet-wide and per-target counts leave all `55` conflicts intact.
Ordinal fields make all `227` conflicting observations unique, but retain no
repeated control, so they cannot be formula inputs. Exact same-capture status
transition signatures for the damage source and damage target also leave all
`55` conflicts and `227` observations intact. The status endpoint remains
allegiance-neutral throughout this audit.

CurrentHP is retained as a separate diagnostic instead of being silently
discarded from the conflict key. A bounded prefix scan of the `1,982,846,108`
byte formula cohort reads only the `2,156` target attribute states referenced
by this action. Exact wire-start CurrentHP (`11310`) and MaxHP (`11320`) exist
for `2,383` observations. Subtracting prior same-target `hp_loss` in exact
container order, after proving that no selected row has preceding same-target
healing in its capture, yields an in-range pre-hit HP for `2,363` observations
and `220` of the `227` conflicting observations. Those `220` rows become
event-unique, leaving no equal-pre-hit-HP repeated controls; that is a useful
hidden-state lead, not proof of an HP threshold or curve. `20` observations
produce an impossible negative reconstruction and `23` lack both HP fields,
so they remain explicitly unresolved. Output transitions occur across many HP
percentages rather than one demonstrated threshold. No rDPS formula or
provider credit is promoted from this diagnostic.

An event-ordered HP ledger now tests that lead against later explicit HP
observations instead of assuming that the wire-message-start state is current.
It replays the four exact build-`24687926` logs, subtracts only packet
`hp_loss`, adds only canonical effective healing, and invalidates intervals
with missing transition values, MaxHP changes, or life transitions. The
candidate transition model closes exactly in only `83` of `31,292` otherwise
eligible snapshot intervals (`27` basis points), so it is not an authoritative
HP model. All `2,406` selected actions are found, but only `3` lie inside an
exactly closing interval and none of those belongs to the `227` conflicting
observations. The other selected actions remain explicit mismatches or lack a
later closure. Candidate pre-hit HP is therefore never exposed as formula
context unless its complete containing interval closes with zero residual;
this pass proves that the remaining conflicts cannot currently be resolved by
HP state without better packet semantics or controlled replay.

The exact target-identity route has now been recovered for the `2,406`
lifecycle-conditioned action observations without assuming that a target is
an enemy. Event-time `ActorEvent` state supplies a numeric identity for every
target: `2,383` are actor kind `monster`, `23` are actor kind `projectile`, and
none has an identity conflict. The monster rows join by exact `MonsterTable.Id`
through `AttributeId` to `EntityAttributeTable.Id`; the projectile rows join
by exact `BulletTable.Id`. All `2,406` routes close and `2,383` monster actions
also retain observed level `60`.

That identity closure does not yet close mitigation. The `16` distinct
monster routes expose only formula-seed metadata (`Level`, season selectors,
rank loading, and `FightValueCoe`) in `EntityAttributeTable`; the two bullet
rows expose projectile behavior but no received-damage scalar. None of those
fields is relabeled as defense or mitigation, and absent target resistance
attributes remain absent rather than zero. Adding exact target kind, numeric
ID, level, and the complete static signature leaves all `55` conflicting
contexts and `227` observations unchanged. The target join is therefore a
completed topology/static-route proof and a narrower server-state obligation,
not a damage formula or provider-credit authority.

The full-cohort mitigation frontier now uses the same neutral action topology
instead of assuming that a defense-bearing damage target is an enemy. A
bounded worklist selects `774` exact build-`24687926` actions with a
packet-observed target mitigation-family attribute from `735,016` formula
samples. Exact session/run/sequence replay finds all `774`: every target is an
active player actor, `754` targets have class `11` and specialization `117`,
and `20` retain class `11` with specialization unresolved. None exposes a
stable character ID or level, and none is zero-filled. The same replay retains
the damage actor and run scene: `773` source actors are active, `756` have an
exact numeric monster ID, and `764` actions have an exact scene ID. Feeding
only those event-time identities back into the disk-partitioned diagnostic
provides cross-capture actor/scene context for `747` physical-defense rows,
`747` magic-defense rows, and `756` refined-defense rows. Exact status,
attribute, actor-shape, calculation, and scene controls still produce zero
groups with multiple defense states and therefore zero counterfactual pairs.
The `6500` runtime-simple and `9980` refined curve candidates remain unproven.
The value `22000` has an exact current-season table identity, but its use by
combat remains unproven; this closes the action-topology gap but authorizes no
formula, runtime promotion, or provider credit.

The exact-build offline consumer search is also exhausted without promoting
those candidates. It scans `4,821` Lua files, finds the `AttackSimply` names
only in their generated definition, finds zero direct native callers of the
three exposed getters, and finds two exact `FightAttr` consumers that are
character-sheet UI evaluators rather than combat-damage consumers. The
server combat implementation, operation order, and integer rounding are not
present in the current client evidence. A controlled-replay worklist therefore
ranks the `774` exact action contexts while preserving both neutral edges:
`provider -> effect/status lifecycle -> recipient or enemy target` and
`recipient damage action -> recipient or enemy target`. It observes
`18` physical-defense values, `12` magic-defense values, and one
refined-defense value, but zero isolated axis pairs. A formula remains blocked
until repeated exact-build trials vary exactly one mitigation family, select
one integer model while rejecting alternatives, and conserve packet damage.

The current-season transform boundary is now exact rather than curve-fit
identity. The exact build selects season ID `3`; its `FightAttrTranTable`
`DefPara` row is `[22000, 1, 1, 0, 0, 0, 0]`, and the reviewed character-sheet
evaluator computes `100 * raw / (raw + 22000)` without rounding the underlying
value. This proves the table row and character-sheet operation order only. It
does not prove that server combat consumes that transform, that effect
`2110092` reduces raw defense before it, or which integer rounding occurs at
the damage stage. Formula, runtime/UI rDPS, packet conservation, and provider
credit therefore remain false.

Status presence is not interchangeable with numeric effect presence. For
effect `2110140` in build `24687926`, the damage analyzer now accepts an
externally affected action only when the exact provider lifecycle and the
uniquely joined recipient attribute activation/deactivation window agree. The
four-session replay retains exactly `12,547` effective-stat-window damage
actions and all `24,541` ordinary inactive actions. It reclassifies `1,841`
status-present rows outside the eight proven numeric windows as ambiguous
instead of unaffected or affected; this includes all `10` actions before a
delayed attribute activation. The five apparent inactive/active overlaps with
stable source attributes disappear under this gate. This is a timing and
selection correction only: it yields zero controlled counterfactual pairs,
zero reassigned damage, and no provider credit.

When an approved game formula supplies exact rational contributions, the
generic reducer sums those fractions exactly per numeric effect, provider, and
recipient before projecting to integer damage once with half-up rounding. If
the checked exact accumulator overflows, every original rational audit term is
retained but that group transfers zero damage and reports the overflow. This
closes only the engine's conserved integer projection; it never promotes an
unproven game-specific formula, operation order, or rounding rule.

For BPSR, packet `owner_stage` is a zero-based index into the selected logic
dictionary entry; it is not an `EStageType` value. Stage-family evidence must
first join the packet damage-source route to an exact current-build SkillDict,
BulletDict, or BuffDict key. Only then may the indexed StageLogic row supply
its numeric stage type. A BulletTable route may instead inherit a static speed
lane through an exact SkillEffectTable `SkillId` foreign key only when every
possible stage of the selected SkillDict key has that same lane. This proves
static current-build ancestry, not a remote action instance or action-time
speed snapshot, and therefore does not by itself permit provider rDPS credit.

Canonical attribute state distinguishes an explicit empty snapshot from a
missing wire field. An explicit empty entity-attribute or temporary-attribute
snapshot is emitted as a state clear; a missing field emits no observation.
Deltas are sparse, so an absent delta member never clears or zero-fills prior
state. This distinction is required for event-time replay and remains separate
from the unavailable remote action-start packet.

Current-build native timing evidence is also kept separate from an inferred
remote action instance. In build `24687926`, the reviewed standard `HitData`
parser's field operations are proven: it stores one parsed float directly in
`HitData.BeginTime`, divides the parsed `HitInterval` and `DamageInterval`
fields by the exact client `LogicFrameRate` of `30`, normalizes a zero damage
count to one, and calculates
`EndTime = BeginTime + HitInterval * DamageCount` using single-precision
operations. `EndTime` is presently only a proven parser window terminal. The
current-build numeric event route is proven for `ESkillEventType = 2`: the
stage-event orchestrator selects the standard parser, independent of localized
event names. The constructor is proven to copy each exact
`StageEventParamData.ParamName` and `ParamValue` directly into the runtime
event dictionary after exact stage-index filtering and numeric event-type
grouping. The remaining gap is narrower: the parser's protected lookup globals
have not yet been joined to those exact catalog parameter names. The `3,367` responsive
type-2 memberships covering `359,059,466` reported damage units are therefore
fail-closed unresolved; similarly named catalog fields are not formula
authority. Numeric type `4` selects the common parser for accepted motion
configurations, but its configuration-key and unit mapping also remains
unresolved. The relationship
between the live timing scheduler and its speed input is now exact for the
reviewed client: both hit wrappers forward `speed` unchanged, and the scheduler
divides `BeginTime`, `EndTime`, `HitInterval`, and `DamageInterval` by that
single-precision value. One outgoing CtrlSkill path loads that value from
`LogicRuntimeBlockData.TimeFactor` at exact offset `0x3c`. The reviewed action-
speed helper feeds a component setter; the matching getter is multiplied by a
companion speed factor and stored into that TimeFactor field. Exact callback
registration and dispatcher order are now also proven: the action-speed
formula body is registered on stage entry, TimeFactor composition is registered
before each stage update, and the outgoing-hit body is the registered stage
update callback. The dispatcher passes the same `LogicRuntimeBlock` through
that order, so the native lifecycle sampling point and formula-to-scheduler
mechanism are exact. The companion is now structurally identified as
`LocalAttrBattleFrameSpeedComponent.Value`; the local player controller writes
exact float32 `1.0` during its initialization path. That current/local value is
not backfilled into remote or historical actions, and the exact companion or
composed scheduler-speed value is still not joined to every observed damage
action. An executable-wide, chunked reference audit retains all `22` exact
RIP-relative references to the component-specific metadata globals with a
measured peak working set of `360,898,560` bytes. It proves the bounded static
reference surface, not indirect runtime values. Real-number cancellation of
the companion is explicitly unauthorized: float32 rounding occurs after
`action_speed * battle_frame`, so the exact products and their ratio depend on
the battle-frame value unless exact float32 `1.0` is proven for that action.
The
relationship between any parser terminal and the last damage occurrence, and the packet
timestamp's relationship to the native event clock, also remain open.
Consequently this timing proof cannot synthesize a missing cast or authorize
attribution by itself.

Functional Amp effect `2110143` is the first component-scoped promotion
frontier. Its Attack/MAttack component has historical exact-formula and
counterfactual-replay authority, and all required direct numeric rows are
byte-stable across Season 3 builds `24568685`, `24609362`, and `24687926`.
Speed remains a separate, uncredited component. A bounded scan of every
retained sealed current-build recording plus all ten validated partial
prefixes found zero `2110143` occurrences, so current-build provider-to-status
lifecycle authority and current-build damage replay authority are both false.
The schema-2 migration proof nevertheless arms the exact-build Attack/MAttack
formula for offline candidate replay: byte-stable Season 3 numeric assets,
current static native/wire identity, the historical packet formula, and the
runtime's exact IDs, delta, and script scope all agree. Formula arming is not
provider credit and does not invent an occurrence. A future matching-build
packet row can be evaluated immediately, while production activation still
requires that row's provider/recipient lifecycle, reversible transition,
supported damage stage, overlap closure, and observed-damage conservation.
The refreshed migration-proof SHA-256 is
`6522615be1f8740559741b6b93cc4b5cd198e20d54f466748ef40abf48be844e`.
The runtime supports enabling this one numeric effect independently of the
global gate, but rejects that configuration until both authorities are proven.
Observed integer damage remains the exact accounting anchor: simultaneous
proven modifiers are allocated together in operation order, provider shares
plus recipient remainder must equal the original damage exactly, and any
unresolved overlap transfers zero damage. Production promotion count remains
zero for Functional Amp pending one exact-build, exact-pack capture containing
the effect. The overall production promotion count is now two because the
separately scoped Harmony Grace and Mechanical Power rules below are active.

Harmony Grace effect `3003052` is the first production rDPS promotion. Its
scope is deliberately narrow: exact build `24687926`, protocol-pack digest
`sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395`,
recipient class `11`, supported `Attack` rows, and a single exact provider
whose lifecycle transition and damage-stage bodies are packet-proven. Each
accepted row contributes the exact rational share
`observed_damage * provider_removed_attack_stage_body /
active_attack_stage_body`. Fractions are retained losslessly, summed per
effect/provider/recipient, and projected once with half-up rounding for the
integer UI result. This is packet-proven proportional rDPS; it does not claim
the unavailable server counterfactual per-hit integer boundary.

The sealed production replay accepts `223` rows and projects `87,606` damage
from recipient actor `13` to provider actor `4547`. Its raw and redistributed
totals both equal `9,290,442,564`, so ordinary damage remains unchanged and
provider credit equals recipient debit. The refreshed production replay receipt
SHA-256 is `1f48aeb4858600cf9adf88beb9d9ee5d2e4ffa8c58935790177a9ec5e4bda0b2`;
the independent exact-rational promotion receipt SHA-256 is
`59d9c782b2c5d43907a814e9d8c97b89b0aff6560c50bc364876ea4ffe2c937e`.
Other recipient classes, unsupported damage scripts, ambiguous ownership,
same-effect overlap, and all unpromoted effects continue to transfer zero.

Mechanical Power effect `2110140` is the second production rDPS promotion.
It is limited to exact build `24687926`, the same exact protocol-pack digest,
recipient class `11`, the packet-observed tier-0 primary raw-percent transition
`+750`, supported `Attack` rows, and one exact provider/recipient lifecycle.
The runtime removes the exact packet-witnessed provider marginal from the
event-time primary state, replays the primary-to-Attack and damage coefficient
stages, retains each reduced rational contribution, and uses the same grouped
half-up projection contract as Harmony Grace. It does not generalize the
static `+1500` tier or claim the hidden server per-hit integer boundary.

The sealed production replay emits `4,261` rows and projects `22,100,227`
damage from recipient actor `7` to provider actor `5`. Raw and redistributed
totals both equal `2,671,673,080`; rational projection overflow is zero. The
production replay receipt SHA-256 is
`5e554c1fca9c9b6ecf8deb96ddefcafd4673992c72bf111a0d1c0888f0b07ae8`,
and the independent promotion receipt SHA-256 is
`b11981a739e4eefcead86c699704d76e3ff53db7b79b317afd9cd6cd66d92fbd`.
Other Mechanical Power tiers or classes, overlapping providers, rejected or
unresolved rows, and its haste/action-opportunity component transfer zero.
