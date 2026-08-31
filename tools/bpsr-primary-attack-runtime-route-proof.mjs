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
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    worklist: path.resolve(required(parsed, "worklist")),
    damageStage: path.resolve(required(parsed, "damage-stage")),
    primaryStatAttackProof: path.resolve(required(parsed, "primary-stat-attack-proof")),
    eventsSource: path.resolve(required(parsed, "events-source")),
    decoderSource: path.resolve(required(parsed, "decoder-source")),
    reducerSource: path.resolve(required(parsed, "reducer-source")),
    damageStageSource: path.resolve(required(parsed, "damage-stage-source")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    "static formula evidence": context.staticFormulaEvidence,
    "static rDPS worklist": context.worklist,
    "damage-stage artifact": context.damageStage,
    "primary-stat attack transform proof": context.primaryStatAttackProof,
    "canonical event source": context.eventsSource,
    "decoder source": context.decoderSource,
    "rDPS reducer source": context.reducerSource,
    "damage-stage source": context.damageStageSource,
  })) requireFile(file, label);

  const staticEvidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  const worklist = readJson(context.worklist, "static rDPS worklist");
  const damageStage = readJson(context.damageStage, "damage-stage artifact");
  const primaryStatAttackProof = readJson(context.primaryStatAttackProof, "primary-stat attack transform proof");
  requireBuild(staticEvidence, context.build, "static formula evidence");
  requireBuild(worklist, context.build, "static rDPS worklist");
  requireBuild(damageStage, context.build, "damage-stage artifact");
  requireBuild(primaryStatAttackProof, context.build, "primary-stat attack transform proof");
  const attackAttributes = resolveAttackAttributeIds(primaryStatAttackProof);

  const sources = (staticEvidence.sources ?? []).filter(isPrimaryAttackOpenSource);
  const candidatesBySource = uniqueIndex(worklist.formula_replay_candidates ?? [], "source_rule_id", "formula replay candidate");
  const routedSources = sources.map((source) => buildSourceRoute(source, candidatesBySource.get(String(source.source_rule_id))));
  routedSources.sort((left, right) => compareText(left.source_rule_id, right.source_rule_id));
  assertExactCoverage(routedSources);

  const codeContracts = buildCodeContracts(context);
  validateDamageStageArtifact(damageStage);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-primary-attack-runtime-route-proof.mjs",
    game_build: context.build,
    proof_state: "exact-current-build-canonical-runtime-input-route-proven",
    policy: {
      exact_current_build_only: true,
      source_selection_matches_formula_workbench_exactly: true,
      canonical_packet_attribute_values_are_preserved: true,
      authoritative_snapshots_reset_stale_actor_attack_state: true,
      attack_family_is_selected_by_exact_damage_stage_rule: true,
      source_names_or_classes_are_never_used_for_lane_inference: true,
      provider_recipient_windows_remain_required: true,
      exact_observed_damage_row_selection_remains_required: true,
      counterfactual_projection_remains_required: true,
      party_damage_conservation_remains_required: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      static_rdps_worklist: fileDescriptor(context.worklist),
      damage_stage: fileDescriptor(context.damageStage),
      primary_stat_attack_transform_proof: fileDescriptor(context.primaryStatAttackProof),
      events_source: fileDescriptor(context.eventsSource),
      decoder_source: fileDescriptor(context.decoderSource),
      reducer_source: fileDescriptor(context.reducerSource),
      damage_stage_source: fileDescriptor(context.damageStageSource),
    },
    route_contract: {
      canonical_input: "TimelineEventKind::EntityAttributes(EntityAttributeEvent)",
      snapshot_semantics: "authoritative snapshots clear stale actor state before applying the complete packet payload; deltas update only present attributes",
      physical_operand_family: "actor_state.physical_attack",
      physical_operand_attribute_id: attackAttributes.physical,
      magical_operand_family: "actor_state.magical_attack",
      magical_operand_attribute_id: attackAttributes.magical,
      damage_stage_mapping: {
        Attack: "PhysicalAttack",
        MAttack: "MagicalAttack",
      },
      per_hit_selection_key: ["ability_id", "hit_event_id", "skill_stage", "skill_level"],
      formula_term: "primaryAttack",
    },
    summary: summarize(routedSources, damageStage, codeContracts),
    code_contracts: codeContracts,
    routed_sources: routedSources,
    still_required_runtime_gates: [
      "source-active-at-hit-time",
      "provider-recipient-window",
      "exact-observed-damage-row-selection-per-event",
      "observed-final-hit-value",
      "integer-counterfactual-projection",
      "party-damage-conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Primary-attack runtime route proof built for ${context.build}: ${routedSources.length} sources, ${report.summary.route_components} ATK/MATK components, zero rDPS promotions.`);
}

function resolveAttackAttributeIds(proof) {
  const families = proof.families ?? [];
  const physical = uniqueSorted(
    families
      .filter((entry) => Number(entry.attack_add_attribute_id) === 11332)
      .map((entry) => Number(entry.attack_attribute_id)),
    (a, b) => Number(a) - Number(b),
  );
  const magical = uniqueSorted(
    families
      .filter((entry) => Number(entry.attack_add_attribute_id) === 11342)
      .map((entry) => Number(entry.attack_attribute_id)),
    (a, b) => Number(a) - Number(b),
  );
  if (physical.length !== 1 || !Number.isSafeInteger(Number(physical[0]))) {
    throw new Error(`Expected one exact physical attack attribute id, found ${JSON.stringify(physical)}`);
  }
  if (magical.length !== 1 || !Number.isSafeInteger(Number(magical[0]))) {
    throw new Error(`Expected one exact magical attack attribute id, found ${JSON.stringify(magical)}`);
  }
  if (Number(physical[0]) === Number(magical[0])) throw new Error("Physical and magical attack attribute ids must differ");
  return { physical: Number(physical[0]), magical: Number(magical[0]) };
}

function isPrimaryAttackOpenSource(source) {
  return source.static_gate_resolved === false &&
    (source.remaining_static_blockers ?? []).length === 0 &&
    (source.formula_term_ids ?? []).includes("primaryAttack");
}

function buildSourceRoute(source, candidate) {
  if (!candidate) throw new Error(`Missing formula replay candidate for ${source.source_rule_id}`);
  if (!(candidate.formula_term_ids ?? []).includes("primaryAttack")) throw new Error(`Worklist candidate ${source.source_rule_id} lacks primaryAttack`);
  const components = (candidate.relationship_components ?? [])
    .filter((entry) => (entry.formulaTermIds ?? []).includes("primaryAttack"))
    .map((entry) => {
      const stat = String(entry.stat ?? "").toUpperCase();
      if (stat !== "ATK" && stat !== "MATK") throw new Error(`PrimaryAttack component ${source.source_rule_id}/${entry.componentKey} has unsupported stat ${entry.stat}`);
      if (!Array.isArray(entry.values) || entry.values.length === 0) throw new Error(`PrimaryAttack component ${source.source_rule_id}/${entry.componentKey} has no typed values`);
      return {
        component_key: String(entry.componentKey),
        stat,
        runtime_attack_family: stat === "ATK" ? "physical_attack" : "magical_attack",
        contribution_scope: entry.contributionScope,
        transfer_eligibility: entry.transferEligibility,
        required_runtime_evidence: uniqueSorted(entry.requiredRuntimeEvidence ?? []),
        values: structuredClone(entry.values),
      };
    });
  if (components.length === 0) throw new Error(`PrimaryAttack source ${source.source_rule_id} has no ATK/MATK component`);
  const lanes = uniqueSorted(components.map((entry) => entry.stat));
  return {
    source_rule_id: String(source.source_rule_id),
    source_id: String(source.source_id),
    source_name: String(source.source_name ?? ""),
    source_kind: candidate.runtime_matcher?.source_kind ?? null,
    runtime_detection: candidate.runtime_matcher?.runtime_detection ?? null,
    effect_ids: uniqueSorted(source.effect_ids ?? [], compareIdentifiers),
    lane: lanes.length === 2 ? "ATK+MATK" : lanes[0],
    components,
    route_status: "canonical-runtime-input-route-proven-provider-and-replay-open",
    still_required_runtime_evidence: uniqueSorted([
      ...(candidate.required_runtime_evidence ?? []),
      "source active at hit time",
      "provider-recipient window",
      "integer counterfactual projection",
      "party damage conservation",
    ]),
    static_evidence_sha256: source.evidence_sha256,
  };
}

function assertExactCoverage(routes) {
  const laneCounts = countBy(routes, (entry) => entry.lane);
  const componentCounts = countBy(routes.flatMap((entry) => entry.components), (entry) => entry.stat);
  if (routes.length !== 79) throw new Error(`Expected 79 primaryAttack sources, found ${routes.length}`);
  if (laneCounts.ATK !== 58 || laneCounts.MATK !== 20 || laneCounts["ATK+MATK"] !== 1) throw new Error(`PrimaryAttack lane coverage changed: ${JSON.stringify(laneCounts)}`);
  if (componentCounts.ATK !== 59 || componentCounts.MATK !== 21) throw new Error(`PrimaryAttack component coverage changed: ${JSON.stringify(componentCounts)}`);
  const dual = routes.find((entry) => entry.lane === "ATK+MATK");
  if (dual?.source_rule_id !== "mrs:6ce598134d0f" || dual.source_id !== "buff-source:2032221") throw new Error("Dual ATK/MATK witness changed");
  const values = dual.components.map((entry) => Number(entry.values[0]?.value)).sort((a, b) => a - b);
  if (values.length !== 2 || values[0] !== 150 || values[1] !== 150) throw new Error("Dual ATK/MATK witness values changed");
}

function buildCodeContracts(context) {
  const contracts = [
    codeContract(context.eventsSource, "canonical-entity-attribute-event", [
      "EntityAttributes(EntityAttributeEvent)", "pub struct EntityAttributeEvent", "pub struct EntityAttribute", "pub raw_value: Vec<u8>", "pub decoded: Option<EntityAttributeValue>",
    ]),
    codeContract(context.decoderSource, "decoder-preserves-and-emits-attribute-payload", [
      "fn emit_attributes", "attribute.raw_data.clone().unwrap_or_default()", "decoded: decode_attribute_value(id, &raw_value)", "TimelineEventKind::EntityAttributes",
    ]),
    codeContract(context.reducerSource, "authoritative-attack-family-state-and-per-hit-selection", [
      "fn observe_attributes", "update_kind == EntityAttributeUpdateKind::Snapshot", "ActorHpState::default()", "next.physical_attack", "next.magical_attack", "select_damage_stage(", "OffensiveStatKind::PhysicalAttack", "OffensiveStatKind::MagicalAttack",
    ]),
    codeContract(context.damageStageSource, "exact-damage-script-lane-map", [
      "\"Attack\" => Some(OffensiveStatKind::PhysicalAttack)", "\"MAttack\" => Some(OffensiveStatKind::MagicalAttack)", "pub(crate) fn select_damage_stage",
    ]),
  ];
  if (contracts.some((entry) => !entry.all_required_tokens_present)) throw new Error("A required canonical primaryAttack route token is missing from source");
  return contracts;
}

function codeContract(file, contractId, requiredTokens) {
  const text = readFileSync(file, "utf8");
  const tokens = requiredTokens.map((token) => ({ token, present: text.includes(token) }));
  return {
    contract_id: contractId,
    source: fileDescriptor(file),
    required_tokens: tokens,
    all_required_tokens_present: tokens.every((entry) => entry.present),
  };
}

function validateDamageStageArtifact(stage) {
  const summary = stage.summary ?? {};
  const expected = {
    source_rows: 5700,
    lookup_keys: 5678,
    standard_attack_rules: 3110,
    standard_magic_attack_rules: 397,
    standard_rules: 3507,
    conflicting_standard_keys: 0,
  };
  for (const [key, value] of Object.entries(expected)) if (Number(summary[key]) !== value) throw new Error(`Damage-stage ${key} changed from ${value} to ${summary[key]}`);
}

function summarize(routes, damageStage, codeContracts) {
  const laneCounts = countBy(routes, (entry) => entry.lane);
  const components = routes.flatMap((entry) => entry.components);
  const componentCounts = countBy(components, (entry) => entry.stat);
  return {
    routed_sources: routes.length,
    route_components: components.length,
    atk_only_sources: laneCounts.ATK ?? 0,
    matk_only_sources: laneCounts.MATK ?? 0,
    dual_atk_matk_sources: laneCounts["ATK+MATK"] ?? 0,
    atk_components: componentCounts.ATK ?? 0,
    matk_components: componentCounts.MATK ?? 0,
    canonical_code_contracts: codeContracts.length,
    canonical_code_contracts_satisfied: codeContracts.filter((entry) => entry.all_required_tokens_present).length,
    damage_stage_standard_attack_rules: Number(damageStage.summary.standard_attack_rules),
    damage_stage_standard_magic_attack_rules: Number(damageStage.summary.standard_magic_attack_rules),
    runtime_provider_windows_proven: 0,
    observed_event_replays_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_obligations_promoted: 0,
    hidden_omissions: 0,
  };
}

function verify(input) {
  const report = readJson(input, "primary-attack runtime route proof");
  if (report.schema_version !== 1 || report.generated_by !== "tools/bpsr-primary-attack-runtime-route-proof.mjs") throw new Error("Invalid primaryAttack route proof schema/generator");
  if (report.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") throw new Error("Invalid primaryAttack route proof state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("PrimaryAttack route proof content hash mismatch");
  if (report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("PrimaryAttack route proof has an unsafe policy");
  assertExactCoverage(report.routed_sources ?? []);
  if (report.summary?.route_components !== 80 || report.summary?.canonical_code_contracts_satisfied !== report.summary?.canonical_code_contracts) throw new Error("PrimaryAttack route proof coverage mismatch");
  if (report.summary?.runtime_provider_windows_proven !== 0 || report.summary?.observed_event_replays_proven !== 0 || report.summary?.counterfactual_projections_proven !== 0 || report.summary?.conservation_proofs !== 0 || report.summary?.rdps_obligations_promoted !== 0 || report.summary?.hidden_omissions !== 0) throw new Error("PrimaryAttack route proof improperly closes runtime gates or hides evidence");
  if (!Number.isSafeInteger(report.route_contract?.physical_operand_attribute_id) || !Number.isSafeInteger(report.route_contract?.magical_operand_attribute_id) || report.route_contract.physical_operand_attribute_id === report.route_contract.magical_operand_attribute_id) throw new Error("PrimaryAttack route proof is missing distinct exact attack attribute ids");
  if (!Array.isArray(report.still_required_runtime_gates) || report.still_required_runtime_gates.length < 6) throw new Error("PrimaryAttack route proof omitted remaining runtime gates");
  console.log(`Primary-attack runtime route proof verified for build ${report.game_build}: 79 sources, 80 components, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-primary-attack-route-test-"));
  try {
    const routes = [];
    for (let index = 0; index < 79; index += 1) {
      const lane = index < 58 ? "ATK" : index < 78 ? "MATK" : "ATK+MATK";
      routes.push({
        source_rule_id: index === 78 ? "mrs:6ce598134d0f" : `mrs:test-${index}`,
        source_id: index === 78 ? "buff-source:2032221" : `buff-source:${index}`,
        lane,
        components: lane === "ATK+MATK"
          ? [{ stat: "ATK", values: [{ value: 150 }] }, { stat: "MATK", values: [{ value: 150 }] }]
          : [{ stat: lane, values: [{ value: 1 }] }],
      });
    }
    assertExactCoverage(routes);
    const output = path.join(root, "proof.json");
    const report = {
      schema_version: 1,
      generated_by: "tools/bpsr-primary-attack-runtime-route-proof.mjs",
      game_build: "1",
      proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      route_contract: { physical_operand_attribute_id: 11330, magical_operand_attribute_id: 11340 },
      summary: { route_components: 80, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      routed_sources: routes,
      still_required_runtime_gates: ["a", "b", "c", "d", "e", "f"],
    };
    report.content_sha256 = contentHash(report);
    writeJson(output, report);
    verify(output);
    console.log("Primary-attack runtime route proof self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function countBy(values, selector) { const result = {}; for (const value of values) { const key = selector(value); result[key] = (result[key] ?? 0) + 1; } return result; }
function uniqueIndex(values, key, label) { const result = new Map(); for (const value of values) { const id = String(value[key]); if (!id || result.has(id)) throw new Error(`Duplicate or missing ${label} ${id}`); result.set(id, value); } return result; }
function uniqueSorted(values, compare = compareText) { return [...new Set(values.map((value) => typeof value === "string" ? value : String(value)))].sort(compare); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en", { numeric: true }); }
function compareIdentifiers(left, right) { const a = String(left); const b = String(right); return /^\d+$/.test(a) && /^\d+$/.test(b) ? Number(a) - Number(b) : compareText(a, b); }
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
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-primary-attack-runtime-route-proof.mjs build --build <id> --static-formula-evidence <json> --worklist <json> --damage-stage <json> --primary-stat-attack-proof <json> --events-source <rs> --decoder-source <rs> --reducer-source <rs> --damage-stage-source <rs> --output <json>\n  node tools/bpsr-primary-attack-runtime-route-proof.mjs verify --input <json>\n  node tools/bpsr-primary-attack-runtime-route-proof.mjs self-test"); process.exit(exitCode); }
