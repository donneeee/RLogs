# BPSR current-build patch workflow

This workflow turns each installed BPSR build into a complete, reviewable local baseline. "Complete" is measured independently at three layers: the installed client tree, the decoded/generated evidence set, and the derived semantic domains. SteamDB is an early-warning and public depot-manifest index only. It can tell us that Steam metadata changed, but it is not authoritative for installed bytes, combat mechanics, identifiers, formulas, or rDPS behavior.

## Sources of truth

1. The local Steam app manifest and depot manifest IDs establish which distribution is installed. `installed-client-file-manifest.v1.json` accounts for every installed file with a local SHA-256. Client-generated volatile files are separated from depot-authored bytes, but remain visible.
2. The extractor-output and decoded-CTB manifest proves that every derived source file was retained and classified. Unknown files remain visible as unknown evidence; they are never silently omitted.
3. The semantic domain manifests identify which game-data areas changed and which focused extraction or proof suites must run again.
4. Packet replay and captured event lifecycles remain authoritative for runtime behavior, attribution scope, stacking, timing, and formulas. Static tables can suggest a relationship but cannot promote it to an rDPS rule by themselves.

## Before refreshing a build

Run the standalone extractor and CTB decoder once for the candidate build. Asset acquisition and decoding stay outside the live parser. Their expected inputs are:

- `BPSR-UID-Extractors/output-build-<build>-exact`
- `.codex_tmp/current-build-<build>-table-extract-candidate/Excels`
- Steam `appmanifest_3681810.acf`
- `global/steam-<build>/physical/files/*.json`, produced by the read-only full client scan or materialized from its exact prior-build diff
- Steam's cached binary depot manifest when available (optional artifact identity evidence)

Do not replace the prior build folder. It is the comparison baseline.

## One-command refresh

From the RLogs repository:

```powershell
node tools/bpsr-refresh-build-baseline.mjs refresh `
  --build <new-build-id> `
  --appmanifest "<SteamLibrary>\steamapps\appmanifest_3681810.acf" `
  --extractor-root "..\BPSR-UID-Extractors\output-build-<new-build-id>-exact" `
  --decoded-root "..\.codex_tmp\current-build-<new-build-id>-table-extract-candidate\Excels" `
  --physical-root "plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<new-build-id>\physical\files" `
  --depot-manifest "<Steam>\depotcache\<depot-id>_<manifest-id>.manifest" `
  --baseline-depot-manifest "<Steam>\depotcache\<depot-id>_<prior-manifest-id>.manifest" `
  --baseline-build 24687926 `
  --steamdb-change-number <optional-alarm-value> `
  --steamdb-build-id <optional-alarm-value> `
  --steamdb-last-record-update "<optional-alarm-value>"
```

The command fails closed when the local distribution snapshot is missing, a required extraction root is absent, a build identity does not match, a source file disappears, an unexpected file appears during verification, a cached-manifest candidate disagrees with the local SHA-256 diff, or a required current-build semantic input is missing. A legacy comparison build may predate a newer domain manifest; that comparison gap remains explicit in the diff without weakening the candidate build's complete-domain requirement.

## Incremental semantic compiler

The one-command refresh invokes `bpsr-current-build-semantic-refresh.mjs refresh`. Its expensive semantic and rDPS stages are content-addressed: every stage records exact hashes for its declared evidence inputs, relevant tool sources, command identity, and generated outputs. A stage is reused only when all four still match. A changed input or tool, a missing output, or an altered output invalidates that stage automatically. Recursive build preflight remains intentionally uncached so changes behind its plan are checked on every run.

Use `rebuild` periodically to bypass the cache and regenerate every semantic stage for determinism checks:

```powershell
node tools/bpsr-current-build-semantic-refresh.mjs rebuild `
  --config plugins\games\blue-protocol-star-resonance\research\pipelines\global\current-build-semantic-refresh.config.json `
  --build <build-id> `
  --build-root plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id> `
  --extractor-root ..\BPSR-UID-Extractors\output-build-<build-id>-exact `
  --decoded-root ..\.codex_tmp\current-build-<build-id>-table-extract-candidate\Excels
```

Inspect a single generated next-work queue without rescanning the decoded tree:

```powershell
node tools/bpsr-proof-frontier-router.mjs inspect `
  --input plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\proof-frontier-router.v1.json `
  --queue table_field_adjudication
```

The other route queues are `binary_dataflow`, `runtime_candidate_correlation`, and `runtime_packet_correlation`. Binary data-flow is used only when a concrete numeric binary seed exists. A bounded set of candidate output IDs without such a seed goes to `runtime_candidate_correlation`; the unbounded runtime queue remains the last resort. Formula and recipient queues use the same command and remain separate so that magnitude evidence cannot be mistaken for attribution-scope evidence.

This cache accelerates repeated research and patch iteration; it never promotes uncertain evidence. Steam manifest diffs narrow physical acquisition, semantic row hashes narrow mechanics regeneration, and packet proof remains required for runtime behavior.

The refresh also builds `semantic-evidence-index.v1.sqlite`, a build-local research index over every decoded row, exact edge, ambiguous occurrence, reference candidate, semantic finding, mechanics-sensitive field, and unresolved dependency. It preserves the original source hashes and full row evidence while replacing repeated whole-tree JSON/JSONL scans with indexed exact-ID, reverse-reference, and mechanic queries. The index is derived acceleration only: it is neither runtime data nor an authority, and deleting it simply causes a deterministic rebuild from the hashed evidence files.

The indexed evidence also feeds `produced-damage-proof-routes.v1.json`. That compiler performs bounded, table-aware exact-ID graph traversals once per content fingerprint, retains every child damage ID from candidate recount families, rejects substring and superset lookalikes, and records the precise missing ownership edge for every blocked mechanic. Whole-name and formula matches remain quarantined candidates and can never promote a route without an exact current-build relationship or matching-build runtime proof.

Before frontier compilation, `semantic-field-adjudications.v1.json` re-proves narrowly defined structural field roles against the current decoded build. Each rule must satisfy its complete cross-table and schema proof contract before the field can leave the expensive output-routing queue. Adjudication is search reduction only: every occurrence and source row remains retained, no relationship is promoted, and any failed or changed proof automatically makes the field actionable again on that build.

The route ledger and adjudication ledger then feed `proof-frontier-workbench.v1.json`. This derived workbench batches every still-open produced-damage route into one indexed traversal, materializes its exact graph frontier and terminal stalls, and emits direct lookup commands. Outgoing unproven fields stay in the actionable proof queue; exactly adjudicated structural occurrences remain visible in a separate retained bucket; and unrelated rows that merely contain the same number are separated as incoming numeric collisions and remain quarantined. Structural frontier groups identify one table relationship or row shape whose proof can resolve several mechanics without treating similar-shaped rows as proof. It replaces repeated one-ID research scans with one content-addressed build product. It never promotes a relationship and is safely reusable until the evidence index, route ledger, adjudication proof, or workbench tool changes.

The formula ledger and semantic evidence index also compile into `static-formula-evidence.v1.json`. This typed intermediate representation parses every retained formula token once, distinguishes percentages, decimals, flat values, seconds, structured tier ladders, and opaque values, and attaches exact decoded-row hashes to the result. Raw whole numbers are never silently converted to percentages. It separately records whether a magnitude is decoded, whether the static gate is complete, and whether a runtime selector or counterfactual replay is still required. This allows unchanged formula evidence to bypass repeated table interpretation without treating a decoded number as sufficient proof for live rDPS.

Every still-open static gate then compiles into `formula-model-workbench.v1.json`. Rather than investigating the same critical-rate, mastery, haste, HP-basis, or selector question once per source, the workbench groups exact blocker obligations by shared proof model. It consumes the current build's `ModifierValueProofRuntime.json` directly and records component evidence as one of four explicit states: `exact-source-rule`, `entry-associated`, `selector-only`, or `unmatched-preserved`. A multi-component entry that lacks an exact source-rule binding is routed for manual component binding instead of being treated as proven; runtime selectors remain separate obligations. Each group retains its complete source list, effect IDs, formula terms, proof-model IDs, selector/value evidence, blocker text, evidence hashes, and a proof contract. Sources with no emitted blocker are not lost: their unresolved formula terms become explicit runtime-input or missing-value obligations. The verifier requires every obligation to occur exactly once while allowing one source to require several independent models. Proving a shared model can therefore close many component-level questions without relaxing the later provider, recipient, timing, and counterfactual gates.

Inspect the highest-yield groups or one exact proof model without opening the generated JSON:

```powershell
node tools/bpsr-formula-model-workbench.mjs inspect `
  --input plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\formula-model-workbench.v1.json `
  --limit 10

node tools/bpsr-formula-model-workbench.mjs inspect `
  --input plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\formula-model-workbench.v1.json `
  --model expected-value:critical-rate
```

The workbench, formula gap ledger, and recipient-scope ledger then feed `proof-frontier-router.v1.json`. This generated router assigns every still-open proof item to exactly one primary next workflow instead of requiring a researcher to rediscover the route after every patch. Produced-damage ownership goes first to current-build table-field adjudication when an actionable structural field exists. A concrete numeric binary seed may then route to bounded IL2CPP data-flow proof; a bounded set of candidate output IDs without a binary seed routes to one-pass runtime candidate correlation; and an unbounded targeted runtime packet correlation remains the last resort. Formula magnitude and provider/recipient scope remain independent queues because proving one does not prove the other. Every queue is content-addressed, carries stable work-item keys and exact input hashes, and verifies zero hidden omissions. It is a research accelerator only and never promotes relationships, formulas, or attribution rules.

The route artifact and indexed evidence feed `semantic-resolution-batches.v1.json`. This work queue conserves every current candidate and unresolved dependency, assigns each source rule to exactly one earliest unmet proof gate, and emits small deterministic batches in dependency order: identity/namespace, produced-damage routing, formula magnitude, provider/recipient scope, then counterfactual conservation. It accelerates review without guessing, suppressing, or promoting any candidate.

The router and resolution batches also feed `proof-attempt-ledger.v1.json`. It gives every canonical source rule a stable work-item key and a content-addressed proof-input fingerprint, then groups identical proof workflows for batch execution. A local `proof-attempt-receipts.v1.json` may record a proven, rejected, or inconclusive result together with hashed evidence. An unchanged fingerprint and unchanged evidence reuse that exact attempt; changed mechanics, tools, questions, or fixture bytes automatically make it stale and requeue the item. Inconclusive attempts stay visible, and receipt reuse never promotes a runtime rule. The receipt registry is ignored by Git because it contains private local evidence paths; the generated ledger redacts those paths and retains only hashes and byte counts.

Initialize the local receipt registry and record a completed attempt without editing JSON by hand:

```powershell
node tools/bpsr-proof-attempt-ledger.mjs init-receipts `
  --build <build-id> `
  --output plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\proof-attempt-receipts.v1.json

node tools/bpsr-proof-attempt-ledger.mjs record-receipt `
  --ledger plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\proof-attempt-ledger.v1.json `
  --receipts plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\proof-attempt-receipts.v1.json `
  --work-item <source-rule-id> `
  --status <proven|rejected|inconclusive> `
  --conclusion "<evidence-backed conclusion>" `
  --evidence-manifest <local-json-listing-kind-and-path-for-each-evidence-file>
```

The resolution batches and router also compile into `proof-correlation-manifest.v1.json`, which is consumed by the indexed Rust validation analyzer. Every currently indexable proof obligation is represented once, and every obligation that lacks a proven numeric selector remains explicit in `unindexable_work_items`; nothing is hidden merely because it cannot yet be accelerated. The runner scans each capture once for the complete manifest and caches the result by both manifest SHA-256 and capture SHA-256. Changing the proof frontier or capture bytes produces a new cache partition automatically. Generated summaries retain hashes and byte counts rather than private capture paths.

Run the complete indexable frontier against one or more existing captures:

```powershell
node tools/bpsr-proof-correlation-runner.mjs run `
  --manifest plugins\games\blue-protocol-star-resonance\research\game-file-inventory\global\steam-<build-id>\proof-correlation-manifest.v1.json `
  --cache-root private-research\rdps\.proof-correlation-cache `
  --output private-research\rdps\proof-correlation-run-<build-id>.v1.json `
  --capture <capture-1.rlog> `
  --capture <capture-2.rlog>
```

An exact manifest/capture pair is never rescanned after a successful cached run. The resulting observations are research evidence only: they still require the proof gate and cannot directly promote an rDPS rule.

## Generated evidence

Each `global/steam-<build>` folder contains:

- `steam-distribution-snapshot.v1.json`: installed app/depot identity and sentinel hashes.
- `installed-client-file-manifest.v1.json`: every installed client file, its local SHA-256, physical family, depot/volatile origin, aggregate fingerprint, exact depot-byte reconciliation, and optional cached depot-manifest artifact hash.
- `complete-build-source-manifest.v1.json`: hash, size, route, related routes, and proof-suite routing for every extracted or decoded source file. This is the derived-source layer, not the physical install tree.
- `seasonal-domains/*.v1.json`: row-level semantic fingerprints for all current combat and game-data domains.
- `damage-source-route-proof.candidate.v9.json`: exact source routing for every current-build damage key, including retained unresolved candidates.
- `damage-stage-rdps.candidate.v14.json`: standard damage stages plus explicit nonstandard or missing-script formula gaps.
- `damage-script-family-worklist.v6.json`: current formula-family and script-family proof worklist.
- `decoded-table-reference-scan.v3.json`: cross-table references for every damage target under review.
- `damage-resolution-ledger.v2.json`: conserved union of replay-ready, source-blocked, formula-blocked, and jointly blocked damage definitions.
- `ctb-table-identity-map.v1.json`: build-locked proof mapping raw hashed CTB sources to decoded table identities. A raw CTB source is never used as a mechanics dependency without exact row and required-field agreement.
- `semantic-mechanic-dependency-closure.v1.json`: every current semantic mechanics finding expanded through proven decoded rows, exact relationships, mechanics-sensitive fields, incoming/candidate evidence, and explicit unresolved proof requirements. Locale description keys remain conserved as external localization-plugin references instead of being misclassified as missing mechanics rows.
- `semantic-evidence-index.v1.sqlite`: content-addressed build-local query index for decoded rows, relationships, mechanics findings, open proof dependencies, and externally owned localization references; all original evidence and hashes remain retained.
- `produced-damage-proof-routes.v1.json`: build-local exact-route cache for every produced-damage blocker, including candidate recount families, complete child damage-ID sets, rejected lookalikes, missing ownership edges, and next admissible proof actions.
- `semantic-field-adjudications.v1.json`: current-build-reproved search-only dispositions for fields with exact structural roles, retaining every occurrence and proof result while returning failed proofs to the actionable queue.
- `proof-frontier-workbench.v1.json`: batched exact-neighborhood and shared-frontier research cache for every open produced-damage route, with terminal stalls, actionable outgoing fields to prove, separately retained structural adjudications, quarantined incoming numeric collisions, structural cross-mechanic proof groups, candidate damage-target evidence, and ready-to-run indexed lookup commands.
- `static-formula-evidence.v1.json`: typed, row-hashed formula intermediate representation preserving every source token and its unit classification while separating statically decoded magnitudes from runtime selector and conservation requirements.
- `formula-model-workbench.v1.json`: zero-omission grouping of every open static-formula obligation into reusable proof models, with exact component/selector evidence, explicit multi-component binding work, and routing for unresolved sources that previously had no emitted blocker.
- `proof-frontier-router.v1.json`: deterministic next-work queues for every open produced-damage route, formula candidate, and recipient-scope candidate, with table, binary, and runtime paths ordered by proof cost and zero-omission verification.
- `semantic-resolution-batches.v1.json`: complete dependency-ordered proof queue with one canonical work item per source rule, indexed evidence locators, exact batch membership, and zero-hidden-omissions verification.
- `proof-attempt-ledger.v1.json`: content-addressed attempt queue and reusable-result view for every canonical source rule, grouped by proof workflow and source kind, with stale evidence explicitly requeued and private evidence paths redacted.
- `proof-correlation-manifest.v1.json`: complete one-pass runtime validation manifest for every numerically indexable frontier item, plus an explicit no-omission queue for obligations that still lack a proven selector.
- `effect-activation-ledger.v1.json`: separates current-build definitions, current static incoming references, historical packet-observed family edges, and still-unproven current-build relationships without deleting dormant definitions.
- `unrouted-damage-activation-ledger.v1.json`: classifies every route-less damage definition as packet-observed, current-combat-referenced, or definition-only while retaining unrelated numeric collisions as non-authoritative review evidence.
- `distribution-diff-from-<build>.v1.json`: installed Steam/depot changes.
- `depot-manifest-diff-from-<build>.v1.json`: an offline comparison of Steam's cached binary depot manifests, retaining every added, removed, changed, and unchanged file record with exact 64-bit manifest IDs.
- `patch-rescan-plan-from-<build>.v1.json`: reconciles the cached-manifest candidate set with the local SHA-256 diff, classifies every candidate file, expands it to concrete extractor routes, and then narrows proof invalidation to row-level changed semantic domains.
- `complete-source-diff-from-<build>.v1.json`: exact added, removed, and changed files.
- `seasonal-domains/diff-from-<build>.v1.json`: changed semantic rows, explicit legacy comparison omissions, and focused rerun routing.
- `semantic-mechanic-dependency-diff-from-<build>.v1.json`: stable source-ID comparison of every mechanics finding, including added, removed, changed, and unchanged decoded rows, fields, relationships, seeds, and unresolved dependencies.
- `semantic-refresh-cache.v1.json`: exact per-stage input, tool, command, and output fingerprints used for safe incremental semantic/rDPS regeneration.
- `current-build-semantic-refresh.v1.json`: which stages executed or were reused, their durations and hashes, and the complete generated artifact inventory.
- `protocol-pack-status.v1.json`: matching-build candidate, recording, promotion-audit, and installed-pack state, including exact unvalidated routes, capture gaps, `UseSlot` evidence, and every retained promotion blocker.
- `current-build-mapping-completeness.v1.json`: explicit static coverage plus unresolved relationship, runtime-proof, and protocol blockers.

## Diff policy

- A changed Steam depot manifest first triggers a physical file diff. Only locally verified changed file families route extraction and proof work; SteamDB never decides mechanics.
- Steam's cached old/new binary manifests provide the fast candidate set without rereading unchanged 41.9 GB client files. The fast path is valid only when those candidate paths exactly equal the independent local SHA-256 diff.
- Unchanged semantic domains keep their already-proven rules.
- Unchanged mechanic dependency closures keep their static mapping and proof lineage. Changed closures identify the exact mechanic sources and dependency tables that require focused regeneration or re-proof.
- Changed domains rerun only their listed extractors, audits, and packet-proof suites.
- New or unknown files are retained and block a claim of complete routing until classified.
- Removed IDs remain in the prior build's evidence and are reported as removals; they are not deleted from history.
- Definition-only IDs with no indexed packet observation stay in the build ledger for future diffs, but do not masquerade as active relationship blockers or enter runtime rDPS.
- A packet-proven parent/child relationship may carry forward only when both current rows are byte-for-byte semantically unchanged from the proved build. This carries relationship identity, not matching-build activation, magnitude, scope, timing, stacking, or formula proof.
- A legacy baseline that predates one of the current 13 domain manifests reports that domain as missing comparison evidence. Every new baseline still generates all 13, so later transitions receive full domain-to-domain diffs.
- No changed table row automatically becomes an rDPS rule. Provider, recipient, scope, stacking, magnitude, and timing still require packet/runtime proof.
- Seasonal updates should produce broad diffs. Hotfixes should normally produce narrow diffs; the same workflow handles both without assuming their size.

## Current baseline

Build `24687926` currently accounts for all `742` installed files (`41,914,915,081` bytes): `741` depot-authored files exactly match the depot's `41,914,914,585` bytes, and one `496`-byte client-generated verification log is retained as volatile evidence. Separately, all `692` extracted/decoded files (`2,419,775,235` bytes) have zero silent omissions. Its `13` semantic domains contain `415,467` row fingerprints with no missing required or optional inputs. The cached depot transition from build `24609362` identifies `349` changed or removed candidates, exactly matching the independent local SHA-256 set; all `349` are classified and routed, with zero path disagreements. It has zero unresolved exact source/relationship blockers. The current mechanics layer retains `22` semantic findings across `65` attached decoded rows and `12` proven affected tables, with unresolved dependency/proof groups and separately retained locale references explicit rather than guessed or hidden. Typed decoded-expression extraction retains `39,777` exact edges and has closed one formerly open produced-damage route without guessing. The produced-damage frontier contains `10` open ownership routes: `1` has a bounded candidate damage-ID set and routes to indexed runtime candidate correlation, while `9` require targeted runtime packet correlation; none currently has a proven numeric binary seed. The typed formula pass preserves all `2,838` source tokens, decodes `442` magnitudes, and closes `291` static gates in roughly half a second with zero hidden tokens; runtime selector, scope, and conservation gates remain open where required. Its `337` still-open static-gate sources expand to `530` exact blocker obligations but collapse into only `78` reusable proof models, avoiding `452` repeated source-level investigations when those models are proved once. The component pass reuses `444` exact-build value-proof entries and classifies every obligation: `81` have exact source-rule component evidence, `133` are entry-associated, `199` are selector-only, and `117` remain explicitly unmatched. It routes `125` multi-component obligations for manual binding and `65` for runtime selector proof rather than guessing. The attempt ledger consolidates the complete `717`-item frontier into `25` repeatable execution groups: `260` sources whose earliest gate is static formula magnitude, `421` counterfactual/conservation checks, `25` provider/recipient-scope checks, `9` packet correlations, `1` bounded candidate correlation, and `1` remaining static produced-damage route. All `717` work items compile into indexed runtime obligations with zero unindexable items and zero hidden omissions. An unchanged semantic refresh reuses `18` content-addressed stages; only recursive safety preflight executes. The measured unchanged refresh completes in about `8.6` seconds without rerunning proof stages.

That is complete physical, derived-source, and semantic inventory coverage, not a false claim that every mechanic is already proven. Remaining relationship, runtime, and protocol blockers stay explicit in the completeness report until evidence resolves them.
