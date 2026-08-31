#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    fightAttrTable: path.resolve(required(parsed, "fight-attr-table")),
    sourceManifest: path.resolve(required(parsed, "source-manifest")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    eventsSource: path.resolve(required(parsed, "events-source")),
    decoderSource: path.resolve(required(parsed, "decoder-source")),
    reducerSource: path.resolve(required(parsed, "reducer-source")),
    stateFormulaSource: path.resolve(required(parsed, "state-formula-source")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    FightAttrTable: context.fightAttrTable,
    "complete-build source manifest": context.sourceManifest,
    "static formula evidence": context.staticFormulaEvidence,
    "canonical event source": context.eventsSource,
    "decoder source": context.decoderSource,
    "rDPS reducer source": context.reducerSource,
    "state formula source": context.stateFormulaSource,
  })) requireFile(file, label);

  const table = readJson(context.fightAttrTable, "FightAttrTable");
  const manifest = readJson(context.sourceManifest, "complete-build source manifest");
  const staticEvidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  requireBuild(staticEvidence, context.build, "static formula evidence");
  validateManifest(manifest, context);
  const attributeFamilies = validateAttributeFamilies(table);
  const obligations = buildObligations(staticEvidence);
  assertObligationCoverage(obligations);
  const codeContracts = buildCodeContracts(context);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-source-hp-runtime-route-proof.mjs",
    game_build: context.build,
    proof_state: "exact-current-build-canonical-runtime-input-route-proven",
    route_kind: "source-hp-basis-state",
    policy: {
      exact_current_build_table_is_numeric_identity_authority: true,
      sync_flags_prove_transport_availability_not_hit_time_coherence: true,
      current_max_or_missing_hp_selector_is_never_inferred_from_text: true,
      missing_hp_is_only_derived_from_coherent_same_actor_current_and_max_hp: true,
      current_hp_reducer_retention_is_route_only_not_selector_proof: true,
      max_hp_reducer_route_does_not_prove_generic_source_hp_formula_use: true,
      combat_damage_stage_authority_remains_unproven: true,
      provider_recipient_windows_remain_required: true,
      counterfactual_projection_and_party_conservation_remain_required: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      fight_attr_table: fileDescriptor(context.fightAttrTable),
      complete_build_source_manifest: fileDescriptor(context.sourceManifest),
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      events_source: fileDescriptor(context.eventsSource),
      decoder_source: fileDescriptor(context.decoderSource),
      reducer_source: fileDescriptor(context.reducerSource),
      state_formula_source: fileDescriptor(context.stateFormulaSource),
    },
    attribute_families: attributeFamilies,
    route_contract: {
      canonical_input: "TimelineEventKind::EntityAttributes(EntityAttributeEvent)",
      decoder_payload: "raw attribute bytes plus decoded typed value",
      current_hp: {
        attribute_id: 11310,
        exact_packet_route_proven: true,
        exact_rdps_reducer_retention_proven: true,
        retained_field: "ActorHpState.current_value",
      },
      max_hp: {
        final_attribute_id: 11320,
        intermediate_attribute_id: 11321,
        base_add_attribute_id: 11322,
        extra_add_attribute_id: 11323,
        percentage_attribute_id: 11324,
        extra_percentage_attribute_id: 11325,
        exact_packet_route_proven: true,
        exact_rdps_reducer_family_retention_proven: true,
        retained_fields: {
          "11320": "ActorHpState.final_value",
          "11321": "ActorHpState.intermediate_value",
          "11322": "ActorHpState.base_value",
          "11323": "ActorHpState.extra_add",
          "11324": "ActorHpState.raw_percent",
          "11325": "ActorHpState.raw_extra_percent",
        },
        extra_add_11323_reducer_retention_proven: true,
      },
      missing_hp: {
        arithmetic_identity: "max(0, max_hp - current_hp)",
        arithmetic_identity_only: true,
        coherent_same_actor_snapshot_required: true,
        generic_formula_selector_proven: false,
      },
      snapshot_semantics: "authoritative reducer snapshots clear stale retained actor state before applying the complete packet payload; deltas update only present retained attributes",
      sync_availability: {
        current_hp_local: true,
        current_hp_aoi: true,
        max_hp_local: true,
        max_hp_aoi: true,
        implication: "transport flags permit observation but do not prove that every mechanic consumes the value or that current and max HP are coherent at a hit timestamp",
      },
    },
    exact_existing_max_hp_counterfactual: {
      formula: "two_stage_percent_input_marginal(base_value, current_raw_percent, provider_raw_percent, current_intermediate_value, current_raw_extra_percent)",
      scope: "one already-promoted exact Max-HP state pipeline only",
      generic_source_hp_basis_authority: false,
    },
    summary: summarize(obligations, codeContracts),
    code_contracts: codeContracts,
    blocker_obligations: obligations,
    still_required_runtime_gates: [
      "packet occurrence in an exact-build replay",
      "coherent same-actor current and max HP at the dependent event timestamp",
      "per-source selector proving current HP, Max HP, missing HP, threshold HP, or another derived HP basis",
      "per-source coefficient, operation order, clamps, and integer rounding",
      "source-active-at-dependent-event-time",
      "provider-recipient-window",
      "observed dependent output row or state transition",
      "integer-counterfactual-projection",
      "party-damage-conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Source HP runtime route proof built for ${context.build}: ${report.summary.blocker_obligations} obligations across ${report.summary.unique_sources} sources; zero rDPS promotions.`);
}

function validateManifest(manifest, context) {
  if (String(manifest.gameBuild) !== context.build) throw new Error(`Source manifest build ${manifest.gameBuild} does not match ${context.build}`);
  if (manifest.authority?.decodedGameTables !== "exact-current-build-static-data") throw new Error("Decoded game tables are not exact-current-build authority");
  const record = (manifest.files ?? []).find((entry) => entry.id === "decoded-game-tables:FightAttrTable.json");
  if (!record) throw new Error("Source manifest omits FightAttrTable.json");
  const actual = fileDescriptor(context.fightAttrTable);
  if (Number(record.bytes) !== actual.bytes || record.sha256 !== actual.sha256) throw new Error("FightAttrTable does not match the complete-build source manifest");
  if (record.authority !== "exact-current-build-static-data") throw new Error("FightAttrTable manifest record lacks exact-current-build authority");
}

function validateAttributeFamilies(table) {
  const current = validateAttributeRow(table["11310"], {
    Id: 11310,
    EnumName: "AttrHp",
    IsClass: false,
    IsSyncMe: true,
    IsSyncAoi: true,
    AttrFinal: 0,
    AttrTotal: 0,
    AttrAdd: 0,
    AttrExAdd: 0,
    AttrPer: 0,
    AttrExPer: 0,
    AttrNumType: 0,
    Type: "int64",
    BaseAttr: 0,
  });
  const maximum = validateAttributeRow(table["11320"], {
    Id: 11320,
    EnumName: "AttrMaxHp",
    IsClass: true,
    IsSyncMe: true,
    IsSyncAoi: true,
    AttrFinal: 11320,
    AttrTotal: 11321,
    AttrAdd: 11322,
    AttrExAdd: 11323,
    AttrPer: 11324,
    AttrExPer: 11325,
    AttrNumType: 0,
    OfficialName: "Max HP",
    Type: "int64",
    BaseAttr: 0,
  });
  return {
    current_hp: { ...current, lane_ids: [11310] },
    max_hp: { ...maximum, lane_ids: [11320, 11321, 11322, 11323, 11324, 11325] },
  };
}

function validateAttributeRow(row, expected) {
  if (!row) throw new Error(`FightAttrTable is missing row ${expected.Id}`);
  for (const [key, value] of Object.entries(expected)) {
    if (row[key] !== value) throw new Error(`FightAttrTable row ${expected.Id} ${key} changed from ${JSON.stringify(value)} to ${JSON.stringify(row[key])}`);
  }
  return structuredClone(expected);
}

function buildObligations(staticEvidence) {
  const result = [];
  for (const source of staticEvidence.sources ?? []) {
    if (!(source.formula_term_ids ?? []).includes("sourceHpBasis")) continue;
    result.push({
      obligation_id: `${source.source_rule_id}#sourceHpBasis`,
      source_rule_id: String(source.source_rule_id),
      source_id: String(source.source_id),
      effect_ids: uniqueSorted(source.effect_ids ?? [], compareIdentifiers),
      model_key: "runtime-input:sourcehpbasis",
      blocker: "component:sourceHpBasis:runtime-formula-input-model-required",
      route_status: "canonical-runtime-input-route-proven-selector-formula-and-downstream-runtime-gates-open",
      static_evidence_sha256: source.evidence_sha256,
    });
  }
  result.sort((left, right) => compareText(left.obligation_id, right.obligation_id));
  return result;
}

function assertObligationCoverage(obligations) {
  if (obligations.length !== 25) throw new Error(`Source HP obligation coverage changed from 25 to ${obligations.length}`);
  if (new Set(obligations.map((entry) => entry.source_rule_id)).size !== 25) throw new Error("Source HP rule coverage changed from 25 unique source rules");
  if (new Set(obligations.map((entry) => entry.source_id)).size !== 25) throw new Error("Source HP source coverage changed from 25 unique source IDs");
  if (new Set(obligations.flatMap((entry) => entry.effect_ids.map(String))).size !== 23) throw new Error("Source HP effect coverage changed from 23 unique effect IDs");
  if (obligations.some((entry) => entry.model_key !== "runtime-input:sourcehpbasis")) throw new Error("Source HP proof contains another model key");
}

function buildCodeContracts(context) {
  const contracts = [
    codeContract(context.eventsSource, "canonical-entity-attribute-event", ["EntityAttributes(EntityAttributeEvent)", "pub struct EntityAttributeEvent", "pub struct EntityAttribute"]),
    codeContract(context.decoderSource, "decoder-preserves-and-emits-hp-attributes", ["const ATTR_CURRENT_HP: i32 = 11310", "const ATTR_MAX_HP_FINAL: i32 = 11320", "const ATTR_MAX_HP_EXTRA_PERCENT: i32 = 11325", "fn emit_attributes", "attribute.raw_data.clone().unwrap_or_default()", "TimelineEventKind::EntityAttributes"]),
    codeContract(context.reducerSource, "authoritative-current-and-max-hp-family-state", ["struct ActorHpState", "current_value: Option<i64>", "extra_add: Option<i64>", "fn observe_attributes", "update_kind == EntityAttributeUpdateKind::Snapshot", "ActorHpState::default()", "next.current_value = Some(value)", "next.base_value = Some(value)", "next.final_value = Some(value)", "next.extra_add = Some(value)", "next.intermediate_value = Some(value)", "next.raw_extra_percent = Some(value)"]),
    codeContract(context.stateFormulaSource, "existing-exact-max-hp-marginal-is-scoped", ["pub fn two_stage_percent_input_marginal", "current_intermediate_value", "fixed_point_percent_input_marginal", "unrelated flat additions and stage-local rounding remain preserved"]),
  ];
  if (contracts.some((entry) => !entry.all_required_tokens_present)) throw new Error("A required canonical source-HP route token is missing from source");
  return contracts;
}

function codeContract(file, contractId, requiredTokens) {
  const source = readFileSync(file, "utf8");
  const tokens = requiredTokens.map((token) => ({ token, present: source.includes(token) }));
  return { contract_id: contractId, source: fileDescriptor(file), required_tokens: tokens, all_required_tokens_present: tokens.every((entry) => entry.present) };
}

function summarize(obligations, contracts) {
  return {
    blocker_obligations: obligations.length,
    unique_sources: new Set(obligations.map((entry) => entry.source_rule_id)).size,
    unique_source_ids: new Set(obligations.map((entry) => entry.source_id)).size,
    unique_effect_ids: new Set(obligations.flatMap((entry) => entry.effect_ids.map(String))).size,
    current_hp_packet_routes_proven: 1,
    current_hp_reducer_routes_proven: 1,
    max_hp_packet_routes_proven: 1,
    max_hp_reducer_routes_proven: 1,
    missing_hp_arithmetic_identities_recorded: 1,
    source_hp_basis_selectors_proven: 0,
    coherent_hit_time_hp_snapshots_proven: 0,
    runtime_formula_models_closed: 0,
    canonical_code_contracts: contracts.length,
    canonical_code_contracts_satisfied: contracts.filter((entry) => entry.all_required_tokens_present).length,
    runtime_provider_windows_proven: 0,
    observed_event_replays_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_obligations_promoted: 0,
    hidden_omissions: 0,
  };
}

function verify(input) {
  const report = readJson(input, "source HP runtime route proof");
  if (report.schema_version !== 1 || report.generated_by !== "tools/bpsr-source-hp-runtime-route-proof.mjs" || report.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") throw new Error("Invalid source HP route proof schema/generator/state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Source HP route proof content hash mismatch");
  if (report.policy?.current_max_or_missing_hp_selector_is_never_inferred_from_text !== true || report.policy?.current_hp_reducer_retention_is_route_only_not_selector_proof !== true || report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Source HP route proof has an unsafe policy");
  assertObligationCoverage(report.blocker_obligations ?? []);
  if (report.summary?.canonical_code_contracts_satisfied !== report.summary?.canonical_code_contracts) throw new Error("Source HP route code coverage mismatch");
  for (const key of ["source_hp_basis_selectors_proven", "coherent_hit_time_hp_snapshots_proven", "runtime_formula_models_closed", "runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (report.summary?.[key] !== 0) throw new Error(`Source HP route improperly closes ${key}`);
  }
  if (!report.still_required_runtime_gates?.length) throw new Error("Source HP route omits remaining runtime gates");
  console.log(`Source HP runtime route proof verified for build ${report.game_build}: 25 obligations, 25 source rules, 25 source IDs, 23 effects, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-source-hp-route-test-"));
  try {
    const obligations = Array.from({ length: 25 }, (_, index) => ({
      obligation_id: `mrs:${index}#sourceHpBasis`,
      source_rule_id: `mrs:${index}`,
      source_id: `source:${index}`,
      effect_ids: [index % 23],
      model_key: "runtime-input:sourcehpbasis",
    }));
    const report = {
      schema_version: 1,
      generated_by: "tools/bpsr-source-hp-runtime-route-proof.mjs",
      game_build: "1",
      proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { current_max_or_missing_hp_selector_is_never_inferred_from_text: true, current_hp_reducer_retention_is_route_only_not_selector_proof: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      summary: { canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, source_hp_basis_selectors_proven: 0, coherent_hit_time_hp_snapshots_proven: 0, runtime_formula_models_closed: 0, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      blocker_obligations: obligations,
      still_required_runtime_gates: ["selector", "coherence", "projection", "conservation"],
    };
    report.content_sha256 = contentHash(report);
    const output = path.join(root, "proof.json");
    writeJson(output, report);
    verify(output);
    console.log("Source HP runtime route proof self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function requireBuild(value, build, label) { if (String(value.game_build) !== String(build)) throw new Error(`${label} build ${value.game_build} does not match ${build}`); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function uniqueSorted(values, comparator = compareText) { return [...new Set(values)].sort(comparator); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function compareIdentifiers(left, right) { const a = Number(left); const b = Number(right); return Number.isFinite(a) && Number.isFinite(b) ? a - b : compareText(left, right); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-source-hp-runtime-route-proof.mjs build --build <id> --fight-attr-table <json> --source-manifest <json> --static-formula-evidence <json> --events-source <rs> --decoder-source <rs> --reducer-source <rs> --state-formula-source <rs> --output <json>\n  node tools/bpsr-source-hp-runtime-route-proof.mjs verify --input <json>\n  node tools/bpsr-source-hp-runtime-route-proof.mjs self-test"); process.exit(exitCode); }
