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
  const build = required(parsed, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  return {
    build,
    workbench: path.resolve(required(parsed, "workbench")),
    eventsSource: path.resolve(required(parsed, "events-source")),
    decoderSource: path.resolve(required(parsed, "decoder-source")),
    reducerSource: path.resolve(required(parsed, "reducer-source")),
    formulaSource: path.resolve(required(parsed, "formula-source")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    "formula-model workbench": context.workbench,
    "canonical event source": context.eventsSource,
    "decoder source": context.decoderSource,
    "rDPS reducer source": context.reducerSource,
    "exact formula source": context.formulaSource,
  })) requireFile(file, label);

  const workbench = readJson(context.workbench, "formula-model workbench");
  requireBuild(workbench, context.build, "formula-model workbench");
  const model = (workbench.model_groups ?? []).find((entry) => entry.model_key === "expected-value:critical-rate");
  validateModelCoverage(model);
  const codeContracts = buildCodeContracts(context);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-critical-event-state-route-proof.mjs",
    game_build: context.build,
    proof_state: "exact-current-build-canonical-critical-event-state-route-proven",
    supports_model_keys: ["expected-value:critical-rate"],
    policy: {
      exact_current_build_source_coverage_only: true,
      observed_packet_critical_outcome_is_canonical: true,
      raw_packet_flags_remain_available_as_evidence: true,
      authoritative_attribute_snapshots_clear_stale_state: true,
      exact_packet_row_formulas_remain_rational_until_conservation: true,
      observed_critical_row_attribution_is_separate_from_expected_value_prediction: true,
      expected_value_distribution_model_proven: false,
      source_active_windows_remain_required: true,
      external_provider_scope_remains_required: true,
      current_build_attribute_id_semantics_remain_required: true,
      proof_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      formula_model_workbench: fileDescriptor(context.workbench),
      events_source: fileDescriptor(context.eventsSource),
      decoder_source: fileDescriptor(context.decoderSource),
      reducer_source: fileDescriptor(context.reducerSource),
      formula_source: fileDescriptor(context.formulaSource),
    },
    route_contract: {
      observed_outcome: "DamageEvent.flags.critical",
      raw_outcome_evidence: ["DamagePacketDetail.reported_critical", "DamagePacketDetail.type_flags"],
      attribute_state_input: "TimelineEventKind::EntityAttributes(EntityAttributeEvent)",
      snapshot_semantics: "authoritative snapshots reset omitted actor state; deltas update only present attributes",
      exact_observed_row_primitives: [
        "exact_external_critical_chance_fraction",
        "exact_external_critical_damage_fraction",
        "exact_external_critical_chance_and_damage_fraction",
      ],
      conservation_representation: "reduced positive rational numerator/denominator",
      unsupported_inference: "this route never invents expected damage for a non-critical row",
    },
    current_build_model_coverage: {
      model_key: model.model_key,
      proof_contract: model.proof_contract,
      source_count: model.source_count,
      obligation_count: model.obligation_count,
      source_rule_ids: [...model.source_rule_ids],
      obligation_ids: [...model.obligation_ids],
      effect_ids: [...model.effect_ids],
      proof_model_ids: [...model.proof_model_ids],
      blocker_texts: [...model.blocker_texts],
    },
    code_contracts: codeContracts,
    summary: {
      current_build_sources_supported: model.source_count,
      current_build_obligations_supported: model.obligation_count,
      canonical_code_contracts: codeContracts.length,
      canonical_code_contracts_satisfied: codeContracts.filter((entry) => entry.all_required_tokens_present).length,
      exact_observed_row_formula_primitives: 3,
      expected_value_distribution_models_proven: 0,
      current_build_provider_windows_proven: 0,
      observed_event_replays_proven: 0,
      counterfactual_projections_proven: 0,
      party_damage_conservation_proofs: 0,
      rdps_obligations_promoted: 0,
      hidden_omissions: 0,
    },
    still_required_runtime_gates: [
      "current-build-attribute-id-semantics",
      "source-active-at-hit-time",
      "external-provider-and-recipient-window",
      "unambiguous-provider-decomposition",
      "expected-value-probability-conversion-for-non-observed-outcomes",
      "predicted-versus-observed-distribution-validation",
      "integer-counterfactual-projection",
      "party-damage-conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Critical event/state route proof built for ${context.build}: ${model.source_count} sources supported, zero expected-value closures or rDPS promotions.`);
}

function validateModelCoverage(model) {
  if (!model) throw new Error("Missing expected-value:critical-rate model group");
  if (model.source_count !== 40 || model.obligation_count !== 40) {
    throw new Error(`Critical-rate model coverage changed: ${model.source_count} sources / ${model.obligation_count} obligations`);
  }
  if ((model.source_rule_ids ?? []).length !== 40 || (model.obligation_ids ?? []).length !== 40) {
    throw new Error("Critical-rate model identifiers no longer match exact coverage");
  }
  if (!(model.blocker_texts ?? []).includes("component:critical-rate:expected-value-model-required")) {
    throw new Error("Critical-rate expected-value blocker changed");
  }
}

function buildCodeContracts(context) {
  const contracts = [
    codeContract(context.eventsSource, "canonical-critical-outcome-and-raw-evidence", [
      "pub struct DamageFlags", "pub critical: Option<bool>", "pub struct DamagePacketDetail",
      "pub reported_critical: Option<bool>", "pub type_flags: Option<i32>",
      "pub struct EntityAttributeEvent", "pub raw_value: Vec<u8>", "pub decoded: Option<EntityAttributeValue>",
    ]),
    codeContract(context.decoderSource, "decoder-preserves-critical-bit-and-reported-field", [
      "const DAMAGE_FLAG_CRITICAL", "flags & DAMAGE_FLAG_CRITICAL != 0", ".or(damage.critical)",
      "critical,", "reported_critical: damage.critical", "type_flags: damage.type_flags",
    ]),
    codeContract(context.reducerSource, "authoritative-critical-state-and-observed-outcome-routing", [
      "fn observe_attributes", "update_kind == EntityAttributeUpdateKind::Snapshot", "ActorHpState::default()",
      "next.critical_damage_raw = Some(value)", "next.critical_chance_raw = Some(value)",
      "next.critical_chance_raw_add = Some(value)", "damage.flags.critical == Some(true)",
      "damage.flags.critical != Some(true)", "if providers.next().is_some()",
    ]),
    codeContract(context.formulaSource, "exact-conserved-observed-critical-row-formulas", [
      "pub fn exact_external_critical_chance_fraction", "pub fn exact_external_critical_damage_fraction",
      "pub fn exact_external_critical_chance_and_damage_fraction", "BPSR_FIXED_POINT_SCALE",
      "reduce_positive_fraction", "does not manufacture expected damage for non-critical rows",
    ]),
  ];
  if (contracts.some((entry) => !entry.all_required_tokens_present)) {
    const missing = contracts.flatMap((entry) => entry.required_tokens.filter((token) => !token.present).map((token) => `${entry.contract_id}: ${token.token}`));
    throw new Error(`A required critical route token is missing:\n${missing.join("\n")}`);
  }
  return contracts;
}

function codeContract(file, contractId, requiredTokens) {
  const source = readFileSync(file, "utf8");
  const tokens = requiredTokens.map((token) => ({ token, present: source.includes(token) }));
  return { contract_id: contractId, source: fileDescriptor(file), required_tokens: tokens, all_required_tokens_present: tokens.every((entry) => entry.present) };
}

function verify(input) {
  const report = readJson(input, "critical event/state route proof");
  if (report.schema_version !== 1 || report.generated_by !== "tools/bpsr-critical-event-state-route-proof.mjs") throw new Error("Invalid critical route proof schema/generator");
  if (report.proof_state !== "exact-current-build-canonical-critical-event-state-route-proven") throw new Error("Invalid critical route proof state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Critical route proof content hash mismatch");
  validateModelCoverage(report.current_build_model_coverage);
  const summary = report.summary ?? {};
  if (summary.canonical_code_contracts !== 4 || summary.canonical_code_contracts_satisfied !== 4 || summary.exact_observed_row_formula_primitives !== 3) throw new Error("Critical route code/formula coverage mismatch");
  for (const key of ["expected_value_distribution_models_proven", "current_build_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "party_damage_conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (summary[key] !== 0) throw new Error(`Critical route proof improperly closes ${key}`);
  }
  if (report.policy?.expected_value_distribution_model_proven !== false || report.policy?.proof_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Critical route proof has an unsafe policy");
  if (!Array.isArray(report.still_required_runtime_gates) || report.still_required_runtime_gates.length < 8) throw new Error("Critical route proof omitted remaining runtime gates");
  console.log(`Critical event/state route proof verified for build ${report.game_build}: 40 sources supported, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-critical-route-test-"));
  try {
    const model = {
      model_key: "expected-value:critical-rate", proof_contract: "test", source_count: 40, obligation_count: 40,
      source_rule_ids: Array.from({ length: 40 }, (_, index) => `mrs:${index}`),
      obligation_ids: Array.from({ length: 40 }, (_, index) => `mrs:${index}#0`),
      effect_ids: [], proof_model_ids: ["critical-expected-v1"],
      blocker_texts: ["component:critical-rate:expected-value-model-required"],
    };
    validateModelCoverage(model);
    const output = path.join(root, "proof.json");
    const report = {
      schema_version: 1, generated_by: "tools/bpsr-critical-event-state-route-proof.mjs", game_build: "1",
      proof_state: "exact-current-build-canonical-critical-event-state-route-proven",
      policy: { expected_value_distribution_model_proven: false, proof_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      current_build_model_coverage: model,
      summary: { canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, exact_observed_row_formula_primitives: 3, expected_value_distribution_models_proven: 0, current_build_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, party_damage_conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      still_required_runtime_gates: ["a", "b", "c", "d", "e", "f", "g", "h"],
    };
    report.content_sha256 = contentHash(report);
    writeJson(output, report);
    verify(output);
    console.log("Critical event/state route proof self-test passed.");
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
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-critical-event-state-route-proof.mjs build --build <id> --workbench <json> --events-source <rs> --decoder-source <rs> --reducer-source <rs> --formula-source <rs> --output <json>\n  node tools/bpsr-critical-event-state-route-proof.mjs verify --input <json>\n  node tools/bpsr-critical-event-state-route-proof.mjs self-test"); process.exit(exitCode); }
