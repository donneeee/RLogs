# Runtime presentation bundles

These are small, generated end products consumed by the BPSR plug-in. They do
not replace the human-readable source catalogs under `catalog/`.

- `class-localization.v1.json` joins each current-build class ID and
  localization key to its name in every supported game locale.
- `specialization-presentation.v1.json` joins packet specialization IDs to the
  reviewed talent-tree specialization node, class role, and icon. An unresolved
  icon stays explicit and falls back to the class icon at presentation time.
- `specialization-detection.v1.json` contains the compact, build-scoped evidence
  needed to emit those IDs: current active-skill signatures, exact passive
  specialization selectors, and local talent-tree roots. Teammate selectors
  come directly from nearby-entity packets; ambiguous evidence remains
  unresolved instead of being guessed.
- `battle-imagine-presentation.v1.json` joins equipped Aoyi skill IDs to the
  reviewed current-build Imagine item, tier range, and human-readable icon.
- `combat-action-presentation.v1.json` is the compact current-build lookup for
  packet action IDs. It keeps action kind, resolution state, and shared icon
  path separate from localized names.
- `reviewed-combat-action-presentation.v1.json` is the small generated overlay
  for observed packet IDs whose exact parent was proven through a current-build
  SkillTable, BuffTable, or RecountTable relation. It is checked first, so a
  reviewed mapping also repairs existing saved history without replaying raw
  packet archives. Recount mappings retain a stable parent-group ID alongside
  every raw child action. The Combat Meter may sum those children into a
  separate parent row, but actor and run totals continue to count only the
  original child events.
- `status-effect-presentation.v1.json` does the same for status-effect IDs.
  Design-only rows stay explicitly unresolved rather than leaking developer
  labels into the player-facing UI.
- `rdps-effect-classification.v1.json` is the compact UID-first review table for
  effects that may contribute to rDPS. Candidate rows are retained but never
  attributed until provider, target, magnitude, and stacking are confirmed.
  Effects absent from the table remain explicit unclassified timeline evidence.
- `rdps-promotion-inventory.v1.json` is the build-locked reconciliation of the
  production effect IDs and the exact remaining fail-closed candidates. Runtime
  startup validates it against the enabled formula paths, external-state rules,
  and complete English presentation names, so a promotion, revocation, or new
  candidate cannot silently leave the reported frontier stale.
- `psychoscope-factor-attribution.v2.json` is the build- and season-scoped
  factor graph. It joins exact selected factor item IDs to direct attributes,
  primary buffs, energy generation or consumption, triggered actions, skill
  changes, and reviewed recount parents. Raw child actions remain canonical;
  the bundle deliberately keeps `attribution_enabled` false until the rDPS
  formula layer has verified provider, target, timing, and stacking behavior.
- `localization/<locale>/combat-action-names.v1.json` and
  `status-effect-names.v1.json` keep every language independently loadable.
  Missing translations fall back to the official English game name when the
  end products are generated; the runtime never loads all locales together.
- `localization/<locale>/reviewed-combat-action-names.v1.json` applies the same
  per-language rule to the reviewed observed-action overlay.
- `localization/<locale>/battle-imagine-names.v1.json` keeps each language's
  Imagine names independently loadable instead of placing all locales in RAM.
- The Combat Meter uses this bundle to recognize class-named party companions
  and display them in the selected UI language without loading the full
  localization corpus into memory.
- `localization/<locale>/monster-names.v1.json` maps packet-derived static
  monster IDs to official current-build names. Each language is a separate,
  independently parsed bundle; runtime entity UUIDs remain in history for
  per-spawn filtering and auditability.
- `scene-presentation.v1.json` keeps all 610 current-build scene identities and
  their structural fields available without loading any localized strings.
- `localization/<locale>/scene-names.v1.json` maps the 609 officially named
  scenes independently for every supported game locale. Scene `1` remains an
  explicit unnamed identity because the current client has no name for it.
- Regeneration reads the reviewed human-readable catalogs and their official
  game-localization records. Those source records remain authoritative.

Regenerate all runtime presentation bundles from reviewed catalog end products
with:

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-runtime-presentation -- \
  plugins/games/blue-protocol-star-resonance/game-data/catalog \
  plugins/games/blue-protocol-star-resonance/game-data/runtime/localization
```

Only the selected locale bundle is parsed when a display lookup first needs it.
Raw packet names and scene IDs stay in canonical events and history artifacts;
the Event Viewer therefore remains a pre-localization audit surface. These
bundles are presentation data only.

## Updating rDPS data after a client patch

rDPS extraction and formula discovery do not run in the live parser. External
research tooling writes reviewed candidate end products into an artifact tree;
the BPSR plug-in consumes only the small promoted runtime files in this folder.
`rdps-rescan-plan.v1.json` is the human-readable inventory of every formula,
effect, factor, and protocol input that can invalidate an enabled rule.

Recover notification wire routes from the exact new GameAssembly before
promoting a protocol pack. Route extraction is offline research, not live
parser work. It consumes the recovered IL2CPP `dump.cs` and
`stringliteral.json`, and it fails closed when the native dispatcher shape,
decoded method set, build identity, or duplicate routes differ. Because the
route proof records the RPC surface hash, regenerate in this order:

1. generate an initial `rpc-message-surface.json` without a route proof;
2. generate `world-ntf-route-proof.v1.json` from that surface;
3. regenerate the RPC surface with the proof applied;
4. rerun the route proof against the final surface, then regenerate the final
   surface once more;
5. recover every protobuf field tag from the exact current GameAssembly and
   recovered IL2CPP dump;
6. audit the checked-in Rust decoder against that build-locked native proof.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rpc-surface-scan -- \
  --dump <current-build-dump.cs> \
  --game-assembly <current-build-GameAssembly.dll> \
  --identity <client-binary-identity.json> \
  --output <rpc-message-surface.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rpc-dispatch-route-proof -- \
  --surface <rpc-message-surface.json> \
  --dump <current-build-dump.cs> \
  --game-assembly <current-build-GameAssembly.dll> \
  --string-literals <current-build-stringliteral.json> \
  --identity <client-binary-identity.json> \
  --service WorldNtf \
  --output <world-ntf-route-proof.v1.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rpc-surface-scan -- \
  --dump <current-build-dump.cs> \
  --game-assembly <current-build-GameAssembly.dll> \
  --identity <client-binary-identity.json> \
  --route-proof <world-ntf-route-proof.v1.json> \
  --output <rpc-message-surface.json>
```

Repeat the last two commands once so the proof embeds the final surface hash.
Then run:

```text
python plugins/games/blue-protocol-star-resonance/tools/protobuf-native-wire-proof.py \
  --surface <rpc-message-surface.json> \
  --dump <current-build-dump.cs> \
  --game-assembly <current-build-GameAssembly.dll> \
  --identity <client-binary-identity.json> \
  --output <protobuf-native-wire-proof.v1.json>

python plugins/games/blue-protocol-star-resonance/tools/use-skill-attribute-native-proof.py \
  --game-assembly <current-build-GameAssembly.dll> \
  --metadata <current-build-decrypted-global-metadata.dat> \
  --identity <client-binary-identity.json> \
  --dump <current-build-dump.cs> \
  --prior-contract <prior-use-skill-attribute-envelope.json> \
  --output <use-skill-attribute-native-proof.v1.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rpc-decoder-contract-audit -- \
  --surface <rpc-message-surface.json> \
  --wire-proof <protobuf-native-wire-proof.v1.json> \
  --decoder plugins/games/blue-protocol-star-resonance/src/game_schema_v1.rs \
  --output <rpc-decoder-contract-audit.json>
```

The native wire extractors require the offline research packages `pefile` and
`capstone`; they are not live-parser dependencies. The skill-attribute proof
does not trust IL2CPP dump labels for its gameplay boundary: it finds the exact
ordered 11720/11730/11740 reads, follows the native plaintext/envelope calls,
checks the metadata-default key bytes, and fails closed on ambiguity. This
establishes exact static method routes and protobuf tags. Decoder promotion and
semantic interpretation still require a sealed same-build packet corpus;
missing fields and messages stay visible until implemented and replayed.

After regenerating the extractor outputs, the static worklist command also
writes a build-locked packet-proof watchlist for every formula candidate,
including candidates that already have static value evidence. This keeps
occurrence, provider, recipient, lifecycle, stack, and packet-attribute proof
requirements uniform across the complete candidate set. Numeric references
that are not exact current-build `BuffTable` rows remain explicit rejected
evidence; they are never monitored as invented status effects. Formula-term-
to-attribute-family routing is reviewed code: an unknown future term fails
generation instead of being omitted.
Selected packet attributes are complete formula-state context, not attribution
authority. Self-only families such as `AttrDpsOwnEffectStr` are retained for
exact replay but explicitly marked non-attributable so they can never create
external rDPS credit.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-static-rdps-worklist -- \
  --classification <ModifierClassificationRuntime.json> \
  --contribution <ModifierContributionRuntime.json> \
  --recount <ModifierRecountTable.json> \
  --value-proof <ModifierValueProofRuntime.json> \
  --buff-table <current-build-BuffTable.json> \
  --build <new-client-build> \
  --output <static-rdps-worklist.json> \
  --watchlist-output <rdps-magnitude-proof-watchlist.v3.json>
```

Run the lifecycle proof against current-decoder `.rlog` files from that same
build. The analyzer rejects any deployment or client-build mismatch before it
can mix coefficients across patches.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-status-attribute-proof -- \
  --watchlist <rdps-magnitude-proof-watchlist.v3.json> \
  --rlog <current-build-session.rlog> \
  --output <rdps-magnitude-proof.json> \
  --source-status-context \
  --target-status-context \
  --selected-attribute-context
```

The watchlist retains each current-build `BuffTable` repeat rule, declared
maximum stack count, raw destroy parameters, and the required binary-presence
or exact-stack-delta proof model. The analyzer solves those models separately;
stack changes never fall through a binary assumption. The watchlist and
resulting ledger are offline research inputs, never live
formula authority. A reversible attribute coefficient still needs exact
provider/recipient and downstream damage conservation before promotion.

For every current-build combat-result family, correlate the packet's exact
ability, semantic hit, and `EDamageSource` to its build-locked `DamageAttr`
row before testing coefficient and fixed-parameter placement. Damage and
healing use one analyzer because BPSR transports both as `DamageInfo`.
The canonical damage and healing events therefore retain the raw attacker,
top-summoner, owner ID, death bit, `actual_value`, `hp_loss`, `shield_loss`,
normal/lucky values, owner level/stage, hit parts, component identity, and all
other packet-only formula evidence. Offline cohort keys keep the three raw
source/owner identities so semantic attribution cannot merge distinct wire
components. The death bit remains retained evidence but is excluded from
formula-input keys because it is an outcome of the result. The analyzer rejects
a stale `.rlog` or route proof and retains complete unresolved packet and hit-
part examples instead of dropping them. Every mapped result also records
whether its row came from an unambiguous current-build lookup or from the
packet's exact `damage_source` plus a current-build route proof; those two
evidence authorities are never silently conflated.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-attr-coefficient-proof -- \
  --build <new-client-build> \
  --surface <current-build-DamageFormulaSurface.json> \
  --decoded-table <current-build-DamageAttrTable.json> \
  --route-proof <current-build-damage-source-route-proof.json> \
  --rlog <current-build-session.rlog> \
  --output <damage-attr-coefficient-proof.json>
```

This output is research evidence, not runtime authority. Promotion still
requires repeated same-state observations, provider/recipient lifecycle proof,
formula selection, and exact party conservation.

The coefficient proof joins every surface row to the complete decoded
`DamageAttrTable` row and rejects any disagreement in `PVEDamageRadio` or
`PVEFixedParameter`. It preserves the full semantic row for every observed ID.
Only rows whose exact `DamageScript` is `Attack` or `MAttack` participate in
sibling coefficient tests, and only against the same script family. All other
families remain visible in per-family coverage, repeated-state evidence, and
HP-scaling evidence; their polymorphic fields are not silently interpreted as
ordinary attack coefficients.

Before a same-build capture exists, generate a conserved static resolution
ledger. It deliberately keeps four questions independent:

1. which `DamageAttr` candidate a packet source can select;
2. which `RecountTable` parent owns the row for display and aggregation;
3. whether the row uses a standard or independently unproven server script;
4. which decoded-table references are merely research leads.

Recount ownership never selects `EDamageSource` and never proves a formula.
A scalar reference elsewhere in a decoded table is also not formula authority.
The ledger refuses to write if any `DamageAttr` candidate disappears between
the route proof and formula catalogs, or if its source/formula readiness
classes do not conserve the complete candidate count.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-source-route-proof -- \
  --surface <current-build-DamageFormulaSurface.json> \
  --damage-attr-table <current-build-DamageAttrTable.json> \
  --bullet-table <current-build-BulletTable.json> \
  --bullet-run-table <current-build-BulletRunTable.json> \
  --bullet-shape-table <current-build-BulletShapeTable.json> \
  --buff-table <current-build-BuffTable.json> \
  --skill-table <current-build-SkillTable.json> \
  --skill-effect-table <current-build-SkillEffectTable.json> \
  --skill-fight-level-table <current-build-SkillFightLevelTable.json> \
  --recount-table <current-build-RecountTable.json> \
  --il2cpp-surface <current-build-il2cpp-combat-surface.json> \
  --build <new-client-build> \
  --output <damage-source-route-proof.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-stage-runtime-catalog -- \
  --surface <current-build-DamageFormulaSurface.json> \
  --route-proof <damage-source-route-proof.json> \
  --decoded-table <current-build-DamageAttrTable.json> \
  --build <new-client-build> \
  --output <damage-stage-runtime-catalog.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-script-family-worklist -- \
  --catalog <damage-stage-runtime-catalog.json> \
  --route-proof <damage-source-route-proof.json> \
  --build <new-client-build> \
  --output <damage-script-family-worklist.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-decoded-table-reference-scan -- \
  --decoded-root <current-build-decoded-Excels-folder> \
  --worklist <current-build-CTB-proof-worklist.json> \
  --route-proof <damage-source-route-proof.json> \
  --build <new-client-build> \
  --output <decoded-table-reference-scan.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-resolution-ledger -- \
  --route-proof <damage-source-route-proof.json> \
  --stage-catalog <damage-stage-runtime-catalog.json> \
  --family-worklist <damage-script-family-worklist.json> \
  --reference-scan <decoded-table-reference-scan.json> \
  --build <new-client-build> \
  --output <damage-resolution-ledger.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-script-static-input-inventory -- \
  --worklist <damage-script-family-worklist.json> \
  --il2cpp-surface <current-build-il2cpp-combat-surface.json> \
  --build <new-client-build> \
  --output <damage-script-static-input-inventory.json>

cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-damage-script-build-migration-gate -- \
  --worklist <damage-script-family-worklist.json> \
  --ctb-diff <current-build-ctb-build-diff.json> \
  --current-table <current-build-DamageAttrTable.json> \
  --baseline-table <optional-previous-build-DamageAttrTable.json> \
  --baseline-build <previous-client-build> \
  --build <new-client-build> \
  --output <damage-script-build-migration-gate.json>
```

The stage catalog validates each surface row against the decoded current-build
`DamageAttrTable` and retains every potentially semantic field, including
stagger, part-damage, abnormal-damage, type-enum, weight, light-response, and
profession flags. The family inventory deliberately treats table columns as
polymorphic server-script inputs: identical values in two different
`DamageScript` families do not imply identical units, stages, or operators.
The current client metadata defines the result wire fields and table getters,
but does not contain those server operators. Same-build packet replay remains
mandatory.

The baseline decoded table is optional so a fresh scan can finish even when an
older private extraction is unavailable. Omitting it fails closed: every
current candidate is recorded as cross-build-uncomparable, not unchanged. Even
an exactly unchanged decoded row remains only a historical replay lead because
the server operator and current packet result are separate evidence.

The same report also emits a fail-closed HP-scaling section. It reconstructs
pre-event current HP from an authoritative `11310` snapshot plus intervening
canonical HP loss and effective healing, retains authoritative max HP `11320`,
and compares current, maximum, and missing HP only inside otherwise-identical
formula families. Exact rational affine fits are reported as candidates. A
two-state line is explicitly insufficient; even a three-or-more-state exact fit
remains non-authoritative until the same-build replay proves its calculation
time, provider/recipient window, ordering, and party conservation.

Test HP-scaled healing families separately. The analyzer groups observations by
the exact packet formula identity (ability, semantic hit, source/type, level,
stage, passive, critical/lucky flags, and related discriminants), rejects stale
captures, and refuses to attribute a mixed-family wire cohort to one formula.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-state-scaling-healing-proof -- \
  --build <new-client-build> \
  --rlog <current-build-session.rlog> \
  --all-abilities \
  --output <state-scaling-healing-proof.json>
```

The output retains the complete canonical `DamageInfo` detail for every example
and keeps unresolved HP candidates visible. It is research evidence only until
the relevant exact packet family, calculation-time HP state, lifecycle, and
counterfactual replay all conserve.

Store those two reports, when a sealed same-build capture exists, at
`research/runtime-evidence/global/steam-<build>/damage-attr-coefficient-proof.json`
and
`research/runtime-evidence/global/steam-<build>/state-scaling-healing-proof.json`.
They are optional inputs to the build snapshot because a fresh client build can
be scanned before a capture exists. If present, the audit still verifies their
embedded build identity. Their absence never counts as formula proof and the
protocol pack remains a required promotion input.

After recovering the current IL2CPP metadata and generating `dump.cs`, record
the exact client combat metadata surface. This artifact proves current enum,
wire-field, and client-entrypoint identities and makes them directly diffable
after a patch. It is research evidence only: `DamageDataMgr` consumes
packet-provided `SyncDamageInfo`; it is not the authoritative server formula.

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-il2cpp-combat-surface -- \
  --dump <current-build-dump.cs> \
  --identity <client-binary-identity.json> \
  --output <il2cpp-combat-surface.json>
```

Create a deterministic candidate snapshot after regenerating those end
products:

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-build-audit -- snapshot \
  --plan plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-rescan-plan.v1.json \
  --root . \
  --build <new-client-build> \
  --state candidate \
  --output <candidate-snapshot.json>
```

Compare it with the last reviewed snapshot:

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-build-audit -- diff \
  --baseline <reviewed-snapshot.json> \
  --candidate <candidate-snapshot.json> \
  --output <build-diff.json>
```

The reviewed snapshot committed for the previous build uses
`--state reviewed-baseline`; a newly generated snapshot uses `candidate`. The
diff refuses to compare any other state pairing. It contains added, removed,
and changed hashes, useful JSON row/rule
counts, and the exact replay suites made stale by each change. A client-build
change invalidates all listed suites even when the generated bytes happen to
match. A diff never grants promotion, including a no-change diff; candidates
never become live automatically.

After the listed current-build replays have been reviewed, a proof manifest can
be gated with:

```text
cargo run -p rlogs-game-bpsr --bin rlogs-bpsr-rdps-build-audit -- gate \
  --diff <build-diff.json> \
  --proof-manifest <approved-proof-manifest.json> \
  --output <promotion-gate.json>
```

The gate verifies every required report hash, a nonzero event corpus, exact
party conservation, and the no-hiding policy. It produces an approval artifact
but deliberately does not rewrite or promote runtime files. This keeps an
updated game table from silently changing live rDPS while preserving every
unresolved packet event for the next proof pass.
