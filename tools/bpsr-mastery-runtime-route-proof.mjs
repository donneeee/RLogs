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
    fightAttributeProof: path.resolve(required(parsed, "fight-attribute-proof")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    eventsSource: path.resolve(required(parsed, "events-source")),
    decoderSource: path.resolve(required(parsed, "decoder-source")),
    reducerSource: path.resolve(required(parsed, "reducer-source")),
    runtimeSource: path.resolve(required(parsed, "runtime-source")),
    runtimeConfig: path.resolve(required(parsed, "runtime-config")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    "FightAttrTable": context.fightAttrTable,
    "complete-build source manifest": context.sourceManifest,
    "fight-attribute transform proof": context.fightAttributeProof,
    "static formula evidence": context.staticFormulaEvidence,
    "canonical event source": context.eventsSource,
    "decoder source": context.decoderSource,
    "rDPS reducer source": context.reducerSource,
    "rDPS runtime source": context.runtimeSource,
    "rDPS runtime config": context.runtimeConfig,
  })) requireFile(file, label);

  const table = readJson(context.fightAttrTable, "FightAttrTable");
  const manifest = readJson(context.sourceManifest, "complete-build source manifest");
  const transform = readJson(context.fightAttributeProof, "fight-attribute transform proof");
  const staticEvidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  const runtimeConfig = readJson(context.runtimeConfig, "rDPS runtime config");
  requireBuild(transform, context.build, "fight-attribute transform proof");
  requireBuild(staticEvidence, context.build, "static formula evidence");
  validateManifest(manifest, context);
  const attributeFamilies = validateAttributeFamilies(table);
  validateTransform(transform);
  const obligations = buildObligations(staticEvidence);
  assertObligationCoverage(obligations);
  const codeContracts = buildCodeContracts(context);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-mastery-runtime-route-proof.mjs",
    game_build: context.build,
    proof_state: "exact-current-build-canonical-runtime-input-route-proven",
    route_kind: "mastery-state",
    policy: {
      exact_current_build_table_is_numeric_identity_authority: true,
      localized_names_or_class_labels_never_infer_attribute_values: true,
      older_runtime_config_is_not_current_build_authority: true,
      current_table_independently_revalidates_configured_attribute_ids: true,
      local_snapshot_availability_does_not_imply_remote_aoi_availability: true,
      combat_damage_stage_authority_remains_unproven: true,
      provider_recipient_windows_remain_required: true,
      counterfactual_projection_and_party_conservation_remain_required: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      fight_attr_table: fileDescriptor(context.fightAttrTable),
      complete_build_source_manifest: fileDescriptor(context.sourceManifest),
      fight_attribute_transform_proof: fileDescriptor(context.fightAttributeProof),
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      events_source: fileDescriptor(context.eventsSource),
      decoder_source: fileDescriptor(context.decoderSource),
      reducer_source: fileDescriptor(context.reducerSource),
      runtime_source: fileDescriptor(context.runtimeSource),
      runtime_config: {
        ...fileDescriptor(context.runtimeConfig),
        authority: "approved-older-build-implementation-input-independently-revalidated-by-current-table",
        authored_game_build: String(runtimeConfig.game_build ?? runtimeConfig.gameBuild ?? "unknown"),
      },
    },
    attribute_families: attributeFamilies,
    transform_contract: {
      ui_evaluator_formula: transform.summary.evaluator_formula,
      current_season_id: 3,
      current_season_parameters: [50000, 1, 1, 0, 0, 0, 0],
      exact_ui_transform: "100 * raw / (raw + 50000)",
      combat_damage_stage_authority: false,
      rounding_scope: "UI display rounding only; not a runtime counterfactual rounding rule",
    },
    route_contract: {
      canonical_input: "TimelineEventKind::EntityAttributes(EntityAttributeEvent)",
      decoder_payload: "raw attribute bytes plus decoded typed value",
      snapshot_semantics: "authoritative snapshots clear stale actor state before applying the complete packet payload; deltas update only present attributes",
      tracked_final_mastery_attribute_id: 11940,
      tracked_additive_mastery_attribute_id: 11942,
      reducer_fields: { "11940": "ActorHpState.mastery_raw", "11942": "ActorHpState.mastery_raw_add" },
      local_character_exact_sync: true,
      remote_teammate_aoi_exact_sync: false,
      remote_value_policy: "require packet-observed recipient state/effect evidence or an opted-in profile snapshot; never infer from class, name, or static tables",
    },
    summary: summarize(obligations, codeContracts),
    code_contracts: codeContracts,
    blocker_obligations: obligations,
    still_required_runtime_gates: [
      "packet occurrence in an exact-build replay",
      "source-active-at-hit-time",
      "combat-damage-stage mastery consumer",
      "provider-recipient-window",
      "remote-recipient mastery delta evidence",
      "observed-final-hit-value",
      "integer-counterfactual-projection",
      "party-damage-conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Mastery runtime route proof built for ${context.build}: ${report.summary.blocker_obligations} obligations across ${report.summary.unique_sources} sources; zero rDPS promotions.`);
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
  const raw = validateAttributeRow(table["11140"], {
    Id: 11140, EnumName: "AttrMastery", IsSyncMe: true, IsSyncAoi: false,
    AttrFinal: 11140, AttrTotal: 11141, AttrAdd: 11142, AttrExAdd: 11143, AttrPer: 11144, AttrExPer: 11145,
    AttrNumType: 0, OfficialName: "Mastery", Type: "int32", BaseAttr: 0,
  });
  const pct = validateAttributeRow(table["11940"], {
    Id: 11940, EnumName: "AttrMasteryPct", IsSyncMe: true, IsSyncAoi: false,
    AttrFinal: 11940, AttrTotal: 11941, AttrAdd: 11942, AttrExAdd: 11943, AttrPer: 11944, AttrExPer: 11945,
    AttrNumType: 1, OfficialName: "Mastery", Type: "int32", BaseAttr: 600,
  });
  return {
    raw_mastery: { ...raw, lane_ids: [11140, 11141, 11142, 11143, 11144, 11145] },
    mastery_percentage: { ...pct, lane_ids: [11940, 11941, 11942, 11943, 11944, 11945] },
  };
}

function validateAttributeRow(row, expected) {
  if (!row) throw new Error(`FightAttrTable is missing row ${expected.Id}`);
  for (const [key, value] of Object.entries(expected)) if (row[key] !== value) throw new Error(`FightAttrTable row ${expected.Id} ${key} changed from ${JSON.stringify(value)} to ${JSON.stringify(row[key])}`);
  return structuredClone(expected);
}

function validateTransform(proof) {
  if (proof.schema_version !== 1 || proof.proof_state !== "exact-current-build-client-ui-evaluator") throw new Error("Fight-attribute transform proof is not the exact UI evaluator proof");
  if (proof.policy?.combat_damage_stage_authority !== false) throw new Error("Transform proof must deny combat damage-stage authority");
  const row = (proof.rows ?? []).find((entry) => Number(entry.season_id) === 3);
  const mastery = row?.fields?.MasteryToMasteryPct;
  if (mastery?.state !== "exact-current-build-parameter-array" || JSON.stringify(mastery.parameters) !== JSON.stringify([50000, 1, 1, 0, 0, 0, 0])) throw new Error("Current-season mastery parameters changed");
}

function buildObligations(staticEvidence) {
  const result = [];
  for (const source of staticEvidence.sources ?? []) {
    const blockers = source.remaining_static_blockers ?? [];
    blockers.forEach((blocker, blockerIndex) => {
      if (!String(blocker).includes("mastery")) return;
      const modelKey = String(blocker).endsWith("mastery-stat:runtime-formula-inputs-required")
        ? "runtime-input:mastery-stat"
        : String(blocker).includes("formula-input:mastery")
          ? "stat-conversion:mastery"
          : "stat-conversion:mastery-stat";
      result.push({
        obligation_id: `${source.source_rule_id}#${blockerIndex}`,
        source_rule_id: String(source.source_rule_id),
        source_id: String(source.source_id),
        effect_ids: uniqueSorted(source.effect_ids ?? [], compareIdentifiers),
        model_key: modelKey,
        blocker: String(blocker),
        route_status: "canonical-runtime-input-route-proven-downstream-runtime-gates-open",
        static_evidence_sha256: source.evidence_sha256,
      });
    });
  }
  result.sort((left, right) => compareText(left.obligation_id, right.obligation_id));
  return result;
}

function assertObligationCoverage(obligations) {
  const counts = countBy(obligations, (entry) => entry.model_key);
  if (obligations.length !== 59 || counts["stat-conversion:mastery-stat"] !== 30 || counts["stat-conversion:mastery"] !== 28 || counts["runtime-input:mastery-stat"] !== 1) throw new Error(`Mastery obligation coverage changed: ${JSON.stringify(counts)} / ${obligations.length}`);
  if (new Set(obligations.map((entry) => entry.source_rule_id)).size !== 54) throw new Error("Mastery source coverage changed from 54 unique sources");
  if (new Set(obligations.flatMap((entry) => entry.effect_ids.map(String))).size !== 49) throw new Error("Mastery effect coverage changed from 49 unique effect IDs");
}

function buildCodeContracts(context) {
  const contracts = [
    codeContract(context.eventsSource, "canonical-entity-attribute-event", ["EntityAttributes(EntityAttributeEvent)", "pub struct EntityAttributeEvent", "pub struct EntityAttribute"]),
    codeContract(context.decoderSource, "decoder-preserves-and-emits-attribute-payload", ["fn emit_attributes", "attribute.raw_data.clone().unwrap_or_default()", "TimelineEventKind::EntityAttributes"]),
    codeContract(context.reducerSource, "authoritative-mastery-state", ["fn observe_attributes", "update_kind == EntityAttributeUpdateKind::Snapshot", "ActorHpState::default()", "next.mastery_raw = Some(value)", "next.mastery_raw_add = Some(value)"]),
    codeContract(context.runtimeSource, "configured-mastery-attribute-identities", ["pub mastery_attribute_id: i32", "pub mastery_raw_add_attribute_id: i32"]),
  ];
  if (contracts.some((entry) => !entry.all_required_tokens_present)) throw new Error("A required canonical mastery-route token is missing from source");
  return contracts;
}

function codeContract(file, contractId, requiredTokens) {
  const source = readFileSync(file, "utf8");
  const tokens = requiredTokens.map((token) => ({ token, present: source.includes(token) }));
  return { contract_id: contractId, source: fileDescriptor(file), required_tokens: tokens, all_required_tokens_present: tokens.every((entry) => entry.present) };
}

function summarize(obligations, contracts) {
  const counts = countBy(obligations, (entry) => entry.model_key);
  return {
    blocker_obligations: obligations.length,
    unique_sources: new Set(obligations.map((entry) => entry.source_rule_id)).size,
    unique_effect_ids: new Set(obligations.flatMap((entry) => entry.effect_ids.map(String))).size,
    stat_conversion_mastery_stat_obligations: counts["stat-conversion:mastery-stat"] ?? 0,
    stat_conversion_mastery_obligations: counts["stat-conversion:mastery"] ?? 0,
    runtime_input_mastery_stat_obligations: counts["runtime-input:mastery-stat"] ?? 0,
    canonical_code_contracts: contracts.length,
    canonical_code_contracts_satisfied: contracts.filter((entry) => entry.all_required_tokens_present).length,
    runtime_provider_windows_proven: 0,
    observed_event_replays_proven: 0,
    combat_damage_stage_consumers_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_obligations_promoted: 0,
    hidden_omissions: 0,
  };
}

function verify(input) {
  const report = readJson(input, "mastery runtime route proof");
  if (report.schema_version !== 1 || report.generated_by !== "tools/bpsr-mastery-runtime-route-proof.mjs" || report.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") throw new Error("Invalid mastery route proof schema/generator/state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Mastery route proof content hash mismatch");
  if (report.policy?.combat_damage_stage_authority_remains_unproven !== true || report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Mastery route proof has an unsafe policy");
  assertObligationCoverage(report.blocker_obligations ?? []);
  if (report.summary?.canonical_code_contracts_satisfied !== report.summary?.canonical_code_contracts) throw new Error("Mastery route code coverage mismatch");
  for (const key of ["runtime_provider_windows_proven", "observed_event_replays_proven", "combat_damage_stage_consumers_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) if (report.summary?.[key] !== 0) throw new Error(`Mastery route improperly closes ${key}`);
  if (!report.still_required_runtime_gates?.length) throw new Error("Mastery route omits remaining runtime gates");
  console.log(`Mastery runtime route proof verified for build ${report.game_build}: 59 obligations, 54 sources, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-mastery-route-test-"));
  try {
    const obligations = [];
    for (let index = 0; index < 59; index += 1) obligations.push({
      obligation_id: `mrs:${String(index % 54).padStart(2, "0")}#${index}`,
      source_rule_id: `mrs:${String(index % 54).padStart(2, "0")}`,
      effect_ids: [index % 49],
      model_key: index < 30 ? "stat-conversion:mastery-stat" : index < 58 ? "stat-conversion:mastery" : "runtime-input:mastery-stat",
    });
    const report = {
      schema_version: 1, generated_by: "tools/bpsr-mastery-runtime-route-proof.mjs", game_build: "1", proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { combat_damage_stage_authority_remains_unproven: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      summary: { canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, combat_damage_stage_consumers_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      blocker_obligations: obligations, still_required_runtime_gates: ["provider", "projection", "conservation"],
    };
    report.content_sha256 = contentHash(report);
    const output = path.join(root, "proof.json");
    writeJson(output, report);
    verify(output);
    console.log("Mastery runtime route proof self-test passed.");
  } finally { rmSync(root, { recursive: true, force: true }); }
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
function countBy(values, selector) { const result = {}; for (const value of values) { const key = selector(value); result[key] = (result[key] ?? 0) + 1; } return result; }
function uniqueSorted(values, comparator = compareText) { return [...new Set(values)].sort(comparator); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function compareIdentifiers(left, right) { const a = Number(left); const b = Number(right); return Number.isFinite(a) && Number.isFinite(b) ? a - b : compareText(left, right); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-mastery-runtime-route-proof.mjs build --build <id> --fight-attr-table <json> --source-manifest <json> --fight-attribute-proof <json> --static-formula-evidence <json> --events-source <rs> --decoder-source <rs> --reducer-source <rs> --runtime-source <rs> --runtime-config <json> --output <json>\n  node tools/bpsr-mastery-runtime-route-proof.mjs verify --input <json>\n  node tools/bpsr-mastery-runtime-route-proof.mjs self-test"); process.exit(exitCode); }
