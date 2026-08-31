# Blue Protocol: Star Resonance game plug-in

This is the bundled, trusted BPSR integration for the game-neutral rLogs
runtime. It is an original implementation informed by public behavioral
references; it is not a fork of another parser.

The plug-in owns everything whose meaning depends on BPSR:

- executable/process selectors for Windows and Linux;
- message framing, compression, routes, opcodes, and exact-build packs;
- region/build resolution;
- privacy-reviewed protocol decoding;
- scene, map, entity, monster, UUID, skill, status, equipment, cosmetic,
  Imagine, guild, and localization mappings;
- the typed BPSR character-profile schema;
- projection of a public character profile into Core's website-payload
  envelope and relative profile endpoint;
- sanitized mapping evidence and game-file inventories.

Core owns the reusable pcap input, network/IP/TCP reconstruction, neutral event
carrier, plug-in host contracts, website base URL/authentication, retry state,
and final transport. The BPSR plug-in cannot choose a remote host or receive
website credentials.

## Human-readable layout

```text
blue-protocol-star-resonance/
  plugin.toml                 trusted plug-in manifest and shared exports
  src/
    pipeline.rs               Core TCP streams -> BPSR frames
    framing.rs                BPSR wire framing
    decoder.rs                reviewed message -> canonical event drafts
    dirty_blob_v1.rs          bounded selective dirty-container reader
    profile.rs                typed public character profile
    profile_projection.rs     sealed local patches -> review packages
    continuous_recording.rs   always-on decode -> selective run persistence
    offline_recording.rs      pcap-decoded events -> sealed rlog and coverage
    run_segmentation.rs       authoritative dungeon entry/completion gate
    segmented_recording.rs    atomic per-run rlog writer
    website.rs                BPSR profile -> neutral website request
  protocol-packs/
    <deployment>/
      server-realms.json      readable realm names and verified endpoint rules
      <build>/                exact client-build route knowledge
  protocol-references/
    manifests/                pinned public reference evidence
    profile-bridges/          reviewed route-to-profile evidence
  game-data/
    catalog/
      skills/<class>/<spec>/
      statuses/<class>/<spec>/
      talents/<class>/<spec>/
      modules/<type>/
      entities/
      equipment/
      profile/
      overworld-scenes/
      world-areas/scene-<id>/
      subscenes/<scene-or-review-bucket>/
      activity-targets/<scene-or-resolution-bucket>/
      scene-events/<scene-or-resolution-bucket>/
      world-activities/
      localization/<locale>/<domain>/
  research/
    game-file-inventory/
      <deployment>/<build>/   sanitized schemas and mapping worklists
  tools/
    offline-recorder/         private pcap -> canonical rlog and safe coverage
    protocol-journal/         BPSR private packet journal
    protocol-coverage/        BPSR route and byte coverage
    profile-audit/            BPSR profile-field/privacy audit
    factor-event-correlation  sealed rlog factor/status/damage evidence

RLogs/assets/blue-protocol-star-resonance/shared/
  icons/                      canonical game icons exported for reuse
```

The catalog is sharded for bounded memory and human navigation. Numeric
identity remains authoritative; readable names are presentation metadata.
The icon bytes live once in the game plug-in's provider-owned shared asset
namespace. Catalog records keep portable `icons/...` paths, and consumers
import the versioned `icons` resource instead of hard-coding its install path.
Raw game files, private acquisition scripts, packet captures, account data,
passwords, tokens, private chat, and login messages are not part of this
plug-in.

For Global, `global` is the deployment while Asteria and Bahamar are separate
realms. Geographic region remains unset until independently verified. Realm
endpoint rules begin as exact user-confirmed observations; they are not widened
into guessed address ranges. The narrow `NotifyEnterWorld` decoder declares
only scene host and port fields; protobuf tags containing the account ID and
login token are intentionally absent from the compiled schema. Its endpoint
observation stays inside the trusted game plug-in and is not emitted as a
canonical website event.

The offline recorder deliberately keeps two artifacts separate: raw and opaque
packet evidence stays in private research files, while `.rlog` receives only
events accepted by the protocol pack and canonical privacy boundary. The
coverage sidecar records numeric route and outcome counts without copying
payloads or network endpoints.

The current Global build pack has reviewed decoders for world entry, scene
entry, nearby entity snapshots, public social-profile snapshots, the
owner-character snapshot and its incremental dirty stream, complete dungeon
snapshot timeline fields, current season identity, authoritative server time,
nearby deltas, and local-player deltas. The dirty-stream reader is bounded,
consumes known private account/time fields without retaining them, and stops
at an unknown field whose width is not proven. The social-profile decoder
retains public character/display IDs,
name/level, character-facing avatar IDs, profession and weapon-skin IDs,
equipped item IDs, combat power, season level/strength, guild ID/name,
titles/medals, and world context. Equivalent equipment sets are sorted before
comparison, and profile deduplication is independent from real scene/line
changes.

The complete owner snapshot additionally maps full face/color/avatar
appearance, level progression, detailed equipped item identity and attributes,
combat-profession skills/loadouts/talents, talent point totals, equipped
modules and the package-5 module inventory, life professions, and fashion,
mount, weapon-skin, and dye collections. It joins item instances to equipped
gear/module slots locally and emits only the resulting privacy-reviewed
character patch. Module instance IDs are emitted as strings for browser-safe
joins. Unrelated inventory, acquisition/expiration times, binding/source
metadata, currencies, arbitrary effect strings, and account-shaped subtrees
are not declared.

Account ID/data subtrees and user-supplied profile/half-body image URLs are
absent from the compiled social schema. A synthetic full-envelope test proves
that those skipped values cannot reach canonical events.

The local Profile Sync projector accepts only personal-gameplay profile events,
so viewing another character through the public social route cannot replace or
join the user's own package. It merges partial local updates, preserves prior
season details when a later packet carries only a new season ID, re-verifies
the complete `.rlog` seal, and produces the existing Core-validated relative
website request without a remote host or credentials.

Psychoscope factor attribution remains evidence-gated. The offline
`rlogs-bpsr-factor-event-correlation` tool replays sealed canonical logs and
joins timestamped factor-selection snapshots to exact status-effect instance
lifecycles and damage events. It preserves provider and recipient entities,
apply/refresh/stack/consume/remove samples, ambiguous factor candidates,
recount-linked trigger/target damage, concurrent instances, and unmatched
lifecycle changes. Its reports always carry `rdps_attribution_enabled: false`;
they are research evidence and never mutate Combat History or the live meter.

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-factor-event-correlation -- `
  --rlog runtime-data/logs/example.rlog `
  --output runtime-data/research/factor-correlation/example.json
```

The separate BPSR rDPS catalog promotes an effect only after current-build
tables and captures prove its provider, target scope, magnitude, stacking, and
counterfactual formula. Confirmed rules are compiled into the game plug-in and
fed to the game-neutral combat reducer; candidates remain visible but inert.

When a retained build has decoded `DamageAttrTable.json` but no matching raw
CTB surface, generate a narrowly scoped semantic bridge before running route
or coefficient proofs. The bridge retains every decoded row and reproduces
only the deterministic `TypeEnum`/ID-suffix lookup plus the decoded
`PVEDamageRadio` and `PVEFixedParameter` arrays. Its output explicitly denies
raw CTB offset and runtime formula authority, so it cannot silently relabel a
newer native surface as an older build.

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-attr-semantic-surface -- `
  --decoded-table <exact-build-DamageAttrTable.json> `
  --build <numeric-client-build> `
  --output <exact-build-semantic-surface.json>
```

For build `24252055`, Wounding Curse effects `2203031` and `2205031` are
confirmed fixed 10% enemy vulnerabilities. The retained replay validation in
the protocol pack proves nonzero attribution, zero missing providers, exact
party conservation, and bounded processing cost across the sealed corpus.

State-scaled actions use a separate BPSR-owned streaming projector because the
game-neutral reducer must not know BPSR attribute IDs or formula stages. The
projector retains every HP, healing, shield, status, and damage event, tracks
only exact-build state in O(players + active effect windows), and emits an
additional contribution transfer only after the packet's provider, recipient,
final state, and integer marginal all agree. For build `24252055`, the
two-completed-increment effect `2404261` contribution to Judgment Pursuit
(`2206290`) is enabled: two packet-observed hits transfer `107757` each to the
external provider while preserving the original damage and the party total.
Other stack counts and all other HP-dependent mechanics remain visible proof
work; they are never discarded, hidden, or rounded into an assumed transfer.

Formula-magnitude gaps use a separate reproducible ledger. The
`rlogs-bpsr-rdps-formula-gap-ledger` tool joins the current-build static
semantic audit and magnitude watchlist to a compact packet-proof corpus. It
records which exact effect IDs were observed, whether their attribute changes
were isolated or same-wire, and whether any reversible coefficient proof
exists. An optional retained-proof manifest records separately reviewed exact
historical proofs whose static source tables are byte-identical in the current
build. Those proofs stay visible as retained evidence, but never promote a
newer build. Every ledger row remains blocked until matching-build provider,
recipient, formula-input, output, damage, and conservation evidence is
complete. Schema version 4 iterates the authoritative magnitude watchlist, not
only the semantic findings, so a rule with no semantic row cannot disappear
from the ledger. It also embeds each gap's exact selected packet
attribute IDs, rejected non-BuffTable references, and current BuffTable stack
and lifetime rule. The schema-v16 current-build Aoyi origin ledger is joined by
effect ID so exact component owners, exact relationship owners, candidate-only
owners, and effects with no current owner candidate remain distinct. Static
owner evidence never promotes a formula or runtime attribution. That compact
proof matrix is the update checklist. The
optional `--gap-watchlist-output` writes a build-locked schema-v3 packet-proof
watchlist containing only those unresolved rules while preserving the full
UID-first worklist unchanged. A previously generated formula-gap watchlist is
also a valid input for deterministic regeneration. This reduces offline replay
work without hiding or deleting any evidence.

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-formula-gap-ledger -- `
  --semantic-audit <current-static-semantic-audit.json> `
  --watchlist <current-magnitude-watchlist.json> `
  --origin-ledger <current-aoyi-origin-ledger.json> `
  --packet-proof <compact-historical-or-current-packet-proof.json> `
  --retained-proofs <current-build-retained-proof-manifest.json> `
  --discovery-build <proof-corpus-build> `
  --gap-watchlist-output <current-formula-gap-watchlist.json> `
  --output <formula-magnitude-gap-ledger.json>
```

Recipient scope has its own fail-closed ledger because formula participation
does not prove transfer. The
`rlogs-bpsr-rdps-recipient-scope-ledger` command retains every formula and
exact-produced-damage candidate, including mixed rows, and routes each one as
self-only context, source-owned output, external-recipient candidate,
external-target-state candidate, target-filtered unresolved, or fully
unresolved. Historical packet observations can prioritize reproof work, but
cannot promote a newer build. Exact current-build component origins can refine
stale unresolved scope into a self-only, external-recipient, or external-target
proof queue only when every matched component agrees. Schema version 6 also
retains exact component-only effects that the generic modifier worklist does
not classify, so those effects remain visible proof obligations instead of
disappearing from rDPS review. Mixed component evidence remains unresolved.
This refinement never enables runtime attribution. A row reaches live rDPS only
after the current build proves provider, recipient or target, lifecycle,
formula inputs, output, and party conservation.

Schema version 10 emits a typed transfer gate for every retained row and keeps
directly declared effect IDs separate from related runtime child/alias IDs.
Related effects may contribute lifecycle evidence without being treated as
additional produced effects. An
external-recipient gate requires a packet provider different from the
recipient. An external-target gate can transfer only other players' marginal
damage against the same target during the proven window; the provider's own
damage remains theirs, and defensive ATK reductions never become offensive
rDPS. Self-only modifiers and source-owned produced damage are explicit
non-transfer gates. All gates remain disabled until matching-build canonical
events and conservation replay satisfy their listed evidence requirements.
The current exact-build ledger also keeps Battle Cry's declared controller
`2205310` separate from packet-observed runtime children `2205311` and
`2205312`. Its official mixed scope is retained explicitly: caster Crit DMG,
Courage, Sharp, and follow-up damage are non-transferable owner output, while
the 10% ally Haste component remains a disabled external-recipient proof
obligation until a matching-build capture identifies the responsible child,
party recipients, cadence delta, and nonstacking provider winner.
It also records Denvel's two packet-observed states as one reviewed family
without conflating their scopes: `2110137` is the casting player's self-only
damage-boost controller and `2110152` is a monster-side self-sourced gravity
counter. Both remain visible evidence, but neither is transferable rDPS.
Focused Shot is likewise normalized as an exact owner-only family. Talent
`1123` declares controller `2203230`; runtime child `2203231` supplies 1% Light
DMG per stack for three seconds, up to four stacks. The unrelated Focus buff
`55223` is deliberately excluded from this family while remaining visible
under its own sources. Historical packets confirm only recipient-owned windows,
so Focused Shot cannot transfer rDPS and still requires matching-build
lifecycle evidence for personal formula replay.
Stellar Spark is retained as a separate exact owner-only family. Talent `341`
declares controller `2208420`; runtime child `2208421` supplies 22 flat Fire ATK
per stack for ten seconds, up to ten stacks. Historical packets prove only
recipient-owned controller and stack windows. The proof artifact therefore
classifies both IDs without transferring rDPS or authorizing current-build
formula replay.

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-recipient-scope-ledger -- `
  --worklist <current-static-rdps-worklist.json> `
  --watchlist <current-magnitude-watchlist.json> `
  --semantic-audit <current-static-semantic-audit.json> `
  --display <current-ModifierDisplayTable.json> `
  --packet-proof <compact-historical-or-current-packet-proof.json> `
  --provider-audit <comprehensive-provider-mechanic-audit.json> `
  --component-packet-proof <exact-component-packet-proof.json> `
  --severed-chapter-proof <severed-chapter-effect-family-proof.json> `
  --severed-chapter-audit <severed-chapter-provider-audit.json> `
  --battle-cry-proof <battle-cry-effect-family-proof.json> `
  --battle-cry-audit <battle-cry-provider-audit.json> `
  --denvel-proof <denvel-effect-family-proof.json> `
  --denvel-audit <denvel-provider-audit.json> `
  --focused-shot-proof <focused-shot-effect-family-proof.json> `
  --focused-shot-audit <focused-shot-provider-audit.json> `
  --stellar-spark-proof <stellar-spark-effect-family-proof.json> `
  --stellar-spark-audit <stellar-spark-provider-audit.json> `
  --origin-ledger <current-aoyi-origin-ledger.json> `
  --packet-build <proof-corpus-build> `
  --output <rdps-recipient-scope-ledger.json>
```

The primary runtime audit product is a combat-influence relationship ledger,
not a build-planner model. For every packet-proven transfer it records the
effect, provider, recipient, affected damage ID, damage source, target entity,
observed damage, and exact counterfactual delta. Integer and rational terms
remain exact decimal strings. rDPS is one conserved aggregation of this ledger;
the desktop history view, website reports, and a future build planner may all
consume the same relationships without independently guessing mechanics.

The replay audit accepts individual sealed logs or recursively scans a whole
log directory. `--summary-only` omits the large event-by-event ledger while
retaining exact relationship totals and unique affected-event counts. Partial
captures are excluded, and every processed sealed log must conserve party
damage before a report is written:

```powershell
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-replay-audit -- `
  --rlog-dir .\runtime-data\logs `
  --summary-only `
  --output .\DEV_exports\damage-influence-overview.json
```

For repeated research passes, the cross-platform batch wrapper refreshes only
the derived modifier tables, rebuilds the worklist and semantic audit, emits
the recipient ledger, and writes exact before/after unresolved counters plus
the complete newly-classified rows. It never promotes runtime rDPS:

```powershell
node .\tools\rdps-research-batch.mjs --config .\plugins\games\blue-protocol-star-resonance\research\pipelines\global\steam-24609362\rdps-batch.config.json
```

Use `--skip-extractors` when the build-scoped extractor output is already
current. The generated `rdps-batch-summary.v1.json` preserves every unresolved
row and hashes both ledgers so a later build-planner catalog can trace each
classification back to its exact evidence state.

## Client-update rDPS workflow

rDPS formula evidence remains exact-build data. Updating the installed game
does not mutate the reviewed build `24252055` pack. On a same-deployment newer
build, the live projector may apply its approved rules provisionally while the
desktop host visibly reports the authored and observed builds; provisional
results never advance exact-build proof counters. Every canonical event is
still retained, so a new build can be decoded and replayed without mistaking
provisional damage math for exact-build proof.

Each new client build is prepared in an isolated candidate root that mirrors
the repository-relative paths listed by
`game-data/runtime/rdps-rescan-plan.v1.json`. Raw game acquisition and table
extraction remain offline; only sanitized source fingerprints, generated
catalogs, compact runtime packs, and proof reports enter the plug-in. The
manifest intentionally separates source catalogs from promoted runtime data so
a table identity change cannot become a live coefficient by copying a file.
The sanitized `research/game-file-inventory/<deployment>/<channel>-<build>`
tree is itself a required audit input: client table, schema, relationship,
localization, and physical-file identity changes are therefore visible even
when a generated runtime JSON has not been refreshed yet.

The update sequence is deterministic. The `prepare` command performs the
candidate snapshot and baseline diff together, writes everything beneath a
build-named folder, and emits a human-readable proof worklist. That worklist
also distinguishes compact runtime-data review from a stable formula-algorithm
review. It never promotes the candidate.

0. Run the lightweight Steam distribution preflight. SteamDB is used only as
   an early patch notification and optional record of its change number; it is
   not mechanics evidence. The installed Steam app manifest supplies the
   authoritative local build and depot-manifest identities, while five cheap
   sentinel hashes route native/schema, packaged game-data, Unity metadata,
   and presentation changes. If neither the installed build, depot manifest,
   nor a sentinel changed, skip extraction. Otherwise continue with the
   existing full client-build scanner below. The preflight stores no absolute
   install path, Steam owner ID, account data, or packet contents. A present
   but zero-byte protected metadata file is marked unusable and does not stand
   in for the private validated metadata-recovery workflow.
1. Run the read-only client-build scanner against the installed game and the
   prior reviewed physical inventory. It reads the exact Steam `buildid`,
   hashes stable files, excludes volatile log contents, writes no absolute
   paths, and routes changed source families to the required deep scans and
   proof suites. It never promotes data. Then extract only the affected source
   families into a private working folder and generate a build-scoped protocol
   pack, sanitized table inventories, and candidate catalogs. On Windows, the
   bounded IL2CPP scanner can find the running game by process name, recover
   only the validated metadata image, and hash both that image and
   `GameAssembly.dll`. Its `--identity-report` output contains hashes and sizes
   but no PID, memory address, install path, account data, or packet contents.
   Keep the recovered metadata private; put only the sanitized identity report
   into the build-scoped research inventory.
2. Generate build-scoped seasonal-domain manifests for the prior and new
   builds, then diff them. This row-level pass treats skills, talents,
   Imagines, Psychoscope factors, equipment set bonuses, buffs/effects,
   formulas/scaling, recount relationships, and seasonal activity identity as
   separate domains. Imagines are first-class mechanics data: the scan detects
   new and removed UIDs and changes to existing tier, star, remodel, summon,
   trigger, effect, coefficient, formula, and scaling rows. Localization and
   icon files remain separate reference inputs.
   The baseline refresh also inventories every decoded scalar, array, nested
   array, object, flag, and string path. That universal field manifest is the
   semantic backstop for fields that are not yet named in a domain scanner: a
   future patch cannot silently add or change a mechanics value merely because
   its table was not previously understood. Field-name routing only prioritizes
   review; it never proves a unit, formula, relationship, owner, recipient,
   stack rule, or lifetime.
3. Run `rlogs-bpsr-rdps-build-audit prepare` to generate the `candidate`
   snapshot, diff it against the prior `reviewed-baseline`, and create the
   proof worklist. A build-identity change
   requires every affected proof suite even when all generated bytes happen to
   match.
4. Replay only the suites named by the worklist, retaining every canonical and
   unresolved event and requiring exact party-damage conservation.
5. Run the `gate` command with the reviewed, digest-pinned proof manifest.
   Candidate data cannot be promoted when a report is missing, stale, for a
   different build, non-conserving, or hides unresolved evidence.
6. Promote the candidate explicitly, then retain its snapshot as the next
   baseline. Historical build packs and their proof artifacts remain intact.

For an already extracted build, the baseline orchestration command below
regenerates and verifies the installed-client manifest, complete derived-source
manifest, seasonal domains, reference graph, ID-shaped semantic ledger,
universal decoded-field manifest, unmapped catalog, completeness report, and
all available prior-build diffs. Supplying both cached depot manifests enables
the changed-file fast path. SteamDB remains the public update alarm and manifest
history index; the cached manifests and local SHA-256 inventories are the exact
physical evidence, and the decoded semantic diffs determine which proof suites
must run.

```powershell
node tools\bpsr-refresh-build-baseline.mjs refresh `
  --build <new-build> `
  --baseline-build <prior-build> `
  --appmanifest "<steam-library>\steamapps\appmanifest_3681810.acf" `
  --depot-manifest "<steam-root>\depotcache\3681812_<new-manifest>.manifest" `
  --baseline-depot-manifest "<steam-root>\depotcache\3681812_<prior-manifest>.manifest" `
  --extractor-root "<private-current-extractor-output>" `
  --baseline-extractor-root "<private-prior-extractor-output>" `
  --decoded-root "<private-current-decoded-table-root>" `
  --baseline-decoded-root "<private-prior-decoded-table-root>" `
  --il2cpp-dump "<private-current-il2cpp-root>\dump.cs"
```

The generated build folder is deliberately explicit about completion:
`complete-build-source-manifest.v1.json` proves zero source omissions;
`current-build-unmapped-catalog.v1.json` retains every unresolved item;
`current-build-mapping-completeness.v1.json` reports whether structural,
relationship, field-semantic, runtime-proof, and protocol gates are actually
closed. A complete inventory is not reported as complete mechanics until those
separate semantic and runtime gates pass.

```powershell
node tools\bpsr-steam-patch-gate.mjs snapshot --appmanifest "<steam-library>\steamapps\appmanifest_3681810.acf" --steamdb-change-number <optional-steamdb-change-number> --steamdb-build-id <optional-steamdb-build-id> --steamdb-last-record-update <optional-utc-date>

node tools\bpsr-steam-patch-gate.mjs diff --baseline "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<prior-build>\steam-distribution-snapshot.v1.json" --candidate "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\steam-distribution-snapshot.v1.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\steam-distribution-diff.v1.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-client-build-scan -- --install-root "<steam-library>\steamapps\common\Blue Protocol Star Resonance" --steam-manifest "<steam-library>\steamapps\appmanifest_3681810.acf" --baseline-physical "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<prior-build>\physical\files" --baseline-build <prior-build> --deployment global --channel steam --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\client-source-diff.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-ctb-inventory-scan --release -- --package "<game-root>\bpsr\BPSR_STEAM_Data\StreamingAssets\container\m0.pkg" --relative-package "bpsr/BPSR_STEAM_Data/StreamingAssets/container/m0.pkg" --meta "<game-root>\bpsr\BPSR_STEAM_Data\StreamingAssets\container\meta.pkg" --relative-meta "bpsr/BPSR_STEAM_Data/StreamingAssets/container/meta.pkg" --steam-manifest "<steam-library>\steamapps\appmanifest_3681810.acf" --expected-build <new-build> --deployment global --channel steam --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-indexed-inventory-v2.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-ctb-table-name-resolver -- --inventory "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-indexed-inventory-v2.json" --dump "<private-current-il2cpp-root>\dump.cs" --dump "<private-prior-il2cpp-root>\dump.cs" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-table-name-identities.v1.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-ctb-build-diff -- --current "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-indexed-inventory-v2.json" --baseline-named "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<prior-build>\tables\named" --baseline-unknown "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<prior-build>\tables\unknown" --baseline-build <prior-build> --identity-overlay "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-table-name-identities.v1.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-build-diff-v2.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-ctb-proof-worklist -- --diff "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-build-diff-v2.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-rdps-proof-worklist.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-decoded-table-fingerprint --release -- --decoded-root "<private-decoded-table-root>" --worklist "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\ctb-rdps-proof-worklist.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\decoded-direct-table-fingerprints.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-decoded-table-diff -- --baseline "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<prior-build>\decoded-direct-table-fingerprints.json" --candidate "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\decoded-direct-table-fingerprints.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\decoded-direct-table-diff.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-decoded-row-field-diff -- --baseline-root "<private-prior-decoded-table-root>" --candidate-root "<private-current-decoded-table-root>" --diff "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\decoded-direct-table-diff.json" --output "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\decoded-row-field-diff.v1.json"

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-il2cpp-metadata-scan -- --process-name BPSR_STEAM --output <private-build-root>\global-metadata.dat --game-assembly "<steam-library>\steamapps\common\Blue Protocol Star Resonance\bpsr\GameAssembly.dll" --steam-manifest "<steam-library>\steamapps\appmanifest_3681810.acf" --identity-report <candidate-root>\plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build>\client-binary-identity.json --scope private --chunk-mib 8

node tools\bpsr-seasonal-domain-scan.mjs scan --build <prior-build> --extractor-root "<private-prior-extractor-output>" --decoded-root "<private-prior-decoded-table-root>"

node tools\bpsr-seasonal-domain-scan.mjs scan --build <new-build> --extractor-root "<private-current-extractor-output>" --decoded-root "<private-current-decoded-table-root>"

node tools\bpsr-seasonal-domain-scan.mjs diff --baseline-build <prior-build> --candidate-build <new-build>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-build-audit -- prepare --plan plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-rescan-plan.v1.json --root <candidate-root> --baseline plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/observations/rdps-build-input-snapshot-001.json --build <new-build> --output-dir <audit-root>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-build-audit -- gate --diff <audit-root>/<new-build>/build-diff.json --proof-manifest <reviewed-proof-manifest.json> --output <audit-root>/<new-build>/promotion-gate.json
```

The folder component `<new-build>` must equal the manifest `buildid`. Supplying
both `--build` and `--steam-manifest` makes the scanner reject a mismatch. This
prevents a newly recovered binary or formula table from being mislabeled as an
older packet pack. Reading IL2CPP metadata is an offline research step and does
not start network capture or inspect login traffic.

The seasonal-domain diff reports changes by evidence authority. An
`exact-game-table` change is a client-data change and schedules that domain's
targeted rebuild and proof suites. A `derived-research` change records an
extractor or relationship-analysis improvement without pretending the game
table changed. A `reference-only` change refreshes localization or icon
references but cannot alter mechanics or promote an rDPS rule. No category is
silently hidden, and none is promoted without packet replay and conservation
proof.

The CTB inventory scanner is also offline and read-only. It admits a table only
when its row-key index, fixed rows, ordered pool blocks, complete extent, and
`meta.pkg` address agree. Both 32-bit and 64-bit row-key layouts are proven;
neither is inferred from a table name. The build diff joins tables by the exact
`meta.pkg` hash33 address key, never by nearby offsets or similar row counts.
Changed and unresolved tables remain visible and cannot be promoted by either
tool.

The table-name resolver runs before the build diff. It extracts generated
`*TableBase` identifiers from private IL2CPP dumps, removes only the generated
`Base` suffix, and accepts a name only when hash33 of `<name>.ctb` equals the
exact inventory address key. The checked-in identity overlay contains no
absolute paths or native metadata. It may improve a table's human-readable
identity and domain, but cannot change its exact key or promote its contents.

The CTB proof worklist preserves every changed or added table, but orders the
small exact set that can affect rDPS first: formula inputs, skill/buff/effect
origins, equipment effects, entity identity, and unknown table identity. An
unknown table is a first-priority identity problem, not an ignored table.
Unrelated changes remain in the same worklist for later dependency review.

Decoded table JSON remains a private extraction artifact. The checked-in
fingerprint catalog contains no raw rows or absolute paths. It rejects any
decoded table whose row count disagrees with the independently bounded CTB,
then records canonical per-row hashes and field-kind counts. On the next build,
the decoded-table diff reports the exact added, removed, and changed row IDs;
unchanged rows do not enter the expensive proof queue.

The field-diff stage reads those private decoded roots only for row IDs already
proven changed. Its sanitized output retains every changed top-level value,
keeps changed arrays whole so ordering context is preserved, and embeds no
unchanged rows or absolute paths. It refuses added or removed rows until they
receive a separate row-presence review. A field diff is classification evidence,
not formula authority: packet replay, provider and recipient scope, exact
counterfactual math, and party conservation remain mandatory.

Formula algorithms stay in the BPSR plug-in, not Core. Build-varying IDs,
fixed-point values, stage identities, and permitted vectors live in compact
runtime packs. The rescan manifest fingerprints both layers: the offline
scanner and proof tooling plus the stable formula, projector, and
pack-validator source files are audited alongside generated runtime data. A
source-code change therefore produces the same explicit proof worklist and
promotion block as a client-table change, so a scanner or proof-tool bug cannot
silently bless a candidate. This keeps the live path O(players + active effect
windows), while the slower rescans and full-corpus proofs run only when an
input digest or client-build identity changes.

Dungeon snapshots retain public scene-instance, difficulty, objective ID,
objective value, and completion state. Explicit `Active`/`Ready` entry opens a
segment, while `Playing` is the fallback when monitoring attaches after entry.
`End` plus the verified success result seals a completed segment. Failed,
exited, replaced, and process-ended segments remain local incomplete history
and are not queued as completed leaderboard runs. Raw observed and game
timestamps remain unchanged so cutscene and encounter timing policy can be
refined later without redefining the packet boundaries.

Continuous mode always decodes exact process-owned traffic in memory. The
segment gate controls persistence, not packet inspection: no dungeon `.rlog`
exists before an entry boundary, completion sealing runs independently, and
the decoder remains armed for the next entry. Generic overworld scene-entry
events cannot open a dungeon segment.

When a build-matched compiled game-data bundle is configured, objective events
also carry `activity-target.<id>` and any `scene-event.<id>` backreferences.
The raw packet objective ID remains authoritative, resolution failure stays
explicit, and localized display text is never sealed into the event stream.
The resolver loads only the numeric activity-target shard touched by the
packet. Private offline recordings opt into this bridge with
`--game-data <compiled-bundle>`.

## Loss rule

An unknown route is still a valid local packet record. Queue drops, TCP gaps,
decompression failures, malformed frames, and uncertain mappings are explicit
evidence. They do not disappear merely because the current decoder does not
understand them.
