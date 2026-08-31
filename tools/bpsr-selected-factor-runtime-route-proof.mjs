#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

const TARGETS = Object.freeze([
  ["mrs:10788ae100dd", 3055080],
  ["mrs:30b6e4fe37f0", 3057060],
  ["mrs:41b96665a83b", 3056070],
  ["mrs:5d8239b7cda3", 3055050],
  ["mrs:6387242328f7", 3054110],
  ["mrs:72fec2cc6953", 3059250],
  ["mrs:74c0196befe9", 3058110],
  ["mrs:a4f83e03fd8a", 3056040],
  ["mrs:bf612e6e16d7", 3054060],
  ["mrs:bf7635c65502", 3057040],
]);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    factorCatalog: path.resolve(required(parsed, "factor-catalog")),
    sourceManifest: path.resolve(required(parsed, "source-manifest")),
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    decoderSource: path.resolve(required(parsed, "decoder-source")),
    dreamscopeInferenceSource: path.resolve(required(parsed, "dreamscope-inference-source")),
    factorCorrelationSource: path.resolve(required(parsed, "factor-correlation-source")),
    rdpsValidationSource: path.resolve(required(parsed, "rdps-validation-source")),
    runtimeSelectorCatalog: path.resolve(required(parsed, "runtime-selector-catalog")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    "factor catalog": context.factorCatalog,
    "complete-build source manifest": context.sourceManifest,
    "static formula evidence": context.staticFormulaEvidence,
    "decoder source": context.decoderSource,
    "dreamscope inference source": context.dreamscopeInferenceSource,
    "factor correlation source": context.factorCorrelationSource,
    "rDPS validation source": context.rdpsValidationSource,
    "runtime selector catalog": context.runtimeSelectorCatalog,
  })) requireFile(file, label);

  const factorCatalog = readJson(context.factorCatalog, "factor catalog");
  const manifest = readJson(context.sourceManifest, "complete-build source manifest");
  const staticEvidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  const runtimeCatalog = readJson(context.runtimeSelectorCatalog, "runtime selector catalog");
  requireBuild(staticEvidence, context.build, "static formula evidence");
  requireBuild(runtimeCatalog, context.build, "runtime selector catalog");
  validateManifest(manifest, context);
  validateCatalogCoverage(factorCatalog, runtimeCatalog);
  const obligations = buildObligations(staticEvidence, factorCatalog, runtimeCatalog);
  assertObligationCoverage(obligations);
  const codeContracts = buildCodeContracts(context);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-selected-factor-runtime-route-proof.mjs",
    game_build: context.build,
    proof_state: "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open",
    route_kind: "selected-factor-item-and-grade",
    policy: {
      exact_local_full_snapshot_selection_is_proven: true,
      dirty_live_transition_selection_remains_open: true,
      remote_player_exact_selection_remains_open: true,
      localized_x_suffix_is_not_a_grade_selector: true,
      selected_factor_identity_does_not_prove_mechanics: true,
      reviewed_mechanics_catalog_remains_required_for_formula_inputs: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      factor_catalog: fileDescriptor(context.factorCatalog),
      complete_build_source_manifest: fileDescriptor(context.sourceManifest),
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      decoder_source: fileDescriptor(context.decoderSource),
      dreamscope_inference_source: fileDescriptor(context.dreamscopeInferenceSource),
      factor_correlation_source: fileDescriptor(context.factorCorrelationSource),
      rdps_validation_source: fileDescriptor(context.rdpsValidationSource),
      runtime_selector_catalog: fileDescriptor(context.runtimeSelectorCatalog),
    },
    route_contract: {
      canonical_profile_route: "SeasonCultivateLineData.seasons -> lines -> areas -> middle_nodes[item_id]",
      decoded_profile_field: "CultivationAreaProfile.middle_node_item_ids",
      exact_item_lookup: "dreamscope_factor_item_by_id(item_id)",
      exact_selected_item_index: "SelectorIndex::Item / profile_factor_item",
      exact_grade_source: "DreamscopeFactorItemIdentity.grade",
      exact_family_source: "DreamscopeFactorItemIdentity.family_id",
      local_authoritative_full_snapshot_route_proven: true,
      dirty_transition_and_snapshot_timestamp_binding_proven: false,
      remote_player_selected_item_route_proven: false,
    },
    summary: summarize(obligations, runtimeCatalog, codeContracts),
    code_contracts: codeContracts,
    blocker_obligations: obligations,
    still_required_runtime_gates: [
      "authoritative local profile snapshot bound to the encounter timestamp",
      "dirty loadout transition invalidation and replacement ordering",
      "remote-player exact selected-factor item and grade evidence",
      "per-source mechanics review independent of selected item identity",
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
  console.log(`Selected-factor runtime route proof built for ${context.build}: ${report.summary.selector_obligations} targeted obligations and ${report.summary.grade_item_routes} exact grade-item routes; zero rDPS promotions.`);
}

function validateManifest(manifest, context) {
  if (String(manifest.gameBuild) !== context.build) throw new Error(`Source manifest build ${manifest.gameBuild} does not match ${context.build}`);
  const record = (manifest.files ?? []).find((entry) => entry.id === "generated-research:SeasonPhantomFactors.json");
  if (!record) throw new Error("Source manifest omits SeasonPhantomFactors.json");
  const actual = fileDescriptor(context.factorCatalog);
  if (Number(record.bytes) !== actual.bytes || record.sha256 !== actual.sha256) throw new Error("SeasonPhantomFactors does not match the complete-build source manifest");
  if (record.authority !== "derived-current-build-index") throw new Error("SeasonPhantomFactors manifest authority changed");
}

function validateCatalogCoverage(factorCatalog, runtimeCatalog) {
  if (Number(factorCatalog.summary?.classifiedFactorFamilies) !== 383 && Number(factorCatalog.summary?.factorFamilies) !== 383) {
    const count = Object.keys(factorCatalog.factorFamiliesById ?? {}).length;
    if (count !== 383) throw new Error(`Factor family coverage changed from 383 to ${count}`);
  }
  if (Object.keys(factorCatalog.factorItemsById ?? {}).length !== 3830) throw new Error("Factor catalog item coverage changed from 3830");
  if (Number(runtimeCatalog.summary?.factor_items) !== 3830 || Object.keys(runtimeCatalog.factor_items_by_id ?? {}).length !== 3830) throw new Error("Runtime selector catalog item coverage changed from 3830");
  if (Number(runtimeCatalog.summary?.factor_families) !== 383 || Object.keys(runtimeCatalog.factor_families_by_id ?? {}).length !== 383) throw new Error("Runtime selector catalog family coverage changed from 383");
}

function buildObligations(staticEvidence, factorCatalog, runtimeCatalog) {
  const sourceByRule = new Map((staticEvidence.sources ?? []).map((source) => [String(source.source_rule_id), source]));
  return TARGETS.map(([sourceRuleId, effectId]) => {
    const source = sourceByRule.get(sourceRuleId);
    if (!source) throw new Error(`Static evidence omits targeted source rule ${sourceRuleId}`);
    if (!(source.effect_ids ?? []).map(Number).includes(effectId)) throw new Error(`${sourceRuleId} no longer contains effect ${effectId}`);
    const factor = factorCatalog.factorsByBuffId?.[String(effectId)];
    if (!factor) throw new Error(`Factor catalog omits buff ${effectId}`);
    const familyId = Number(factor.familyId);
    const family = factorCatalog.factorFamiliesById?.[String(familyId)];
    const runtimeFamily = runtimeCatalog.factor_families_by_id?.[String(familyId)];
    if (!family || !runtimeFamily) throw new Error(`Factor family ${familyId} is absent from one catalog`);
    const gradeItemRoutes = (family.gradeRows ?? factor.gradeItems ?? []).map((gradeRow) => {
      const itemId = Number(gradeRow.itemId);
      const grade = Number(gradeRow.grade);
      const catalogItem = factorCatalog.factorItemsById?.[String(itemId)];
      const runtimeItem = runtimeCatalog.factor_items_by_id?.[String(itemId)];
      if (!catalogItem || !runtimeItem) throw new Error(`Factor item ${itemId} is absent from one catalog`);
      if (Number(catalogItem.familyId) !== familyId || Number(runtimeItem.family_id) !== familyId) throw new Error(`Factor item ${itemId} family mismatch`);
      if (Number(catalogItem.grade) !== grade || Number(runtimeItem.grade) !== grade) throw new Error(`Factor item ${itemId} grade mismatch`);
      if (Number(catalogItem.primaryBuffId) !== effectId || !(runtimeItem.terminal_effect_ids ?? []).map(Number).includes(effectId)) throw new Error(`Factor item ${itemId} effect mismatch`);
      return { item_id: itemId, family_id: familyId, grade, primary_buff_id: effectId, quality_tier: Number(runtimeItem.quality_tier) };
    }).sort((left, right) => left.grade - right.grade);
    if (gradeItemRoutes.length !== 10 || gradeItemRoutes.some((row, index) => row.grade !== index + 1)) throw new Error(`${sourceRuleId} does not expose exact grades 1 through 10`);
    return {
      obligation_id: `${sourceRuleId}#selectedFactorGrade`,
      source_rule_id: sourceRuleId,
      source_id: String(source.source_id),
      effect_ids: uniqueSorted(source.effect_ids ?? [], compareIdentifiers),
      factor_family: {
        family_id: familyId,
        name: String(runtimeFamily.name),
        slot_category: String(runtimeFamily.slot_category),
        runtime_role: String(runtimeFamily.runtime_role),
      },
      grade_item_routes: gradeItemRoutes,
      route_status: "exact-local-full-snapshot-item-and-grade-route-proven-dirty-transition-remote-selection-mechanics-and-rdps-gates-open",
      static_evidence_sha256: source.evidence_sha256,
    };
  }).sort((left, right) => compareText(left.obligation_id, right.obligation_id));
}

function assertObligationCoverage(obligations) {
  if (obligations.length !== 10) throw new Error(`Selected-factor obligation coverage changed from 10 to ${obligations.length}`);
  if (new Set(obligations.map((entry) => entry.source_rule_id)).size !== 10) throw new Error("Selected-factor source rule coverage changed from 10");
  if (new Set(obligations.map((entry) => entry.source_id)).size !== 10) throw new Error("Selected-factor source ID coverage changed from 10");
  if (new Set(obligations.flatMap((entry) => entry.effect_ids.map(Number))).size !== 10) throw new Error("Selected-factor effect coverage changed from 10");
  if (obligations.reduce((sum, entry) => sum + entry.grade_item_routes.length, 0) !== 100) throw new Error("Selected-factor grade item route coverage changed from 100");
}

function buildCodeContracts(context) {
  const contracts = [
    codeContract(context.decoderSource, "decoder-retains-selected-middle-node-item", ["fn container_season_cultivation", "middle_node_item_ids", "positive_i32(node.item_id)", "CultivationAreaProfile"]),
    codeContract(context.dreamscopeInferenceSource, "exact-current-build-item-family-grade-lookup", ["DREAMSCOPE_BUILD_CATALOG_JSON", "dreamscope_factor_item_by_id", "factor_items_by_id", "grades: vec![item.grade]"]),
    codeContract(context.factorCorrelationSource, "selection-and-reviewed-mechanics-remain-separated", ["dreamscope_factor_item_by_id", "psychoscope_factor_by_item_id", "unreviewed_factor_item_ids", "formula_inputs"]),
    codeContract(context.rdpsValidationSource, "profile-item-selector-index", ["SelectorIndex::Item", "profile_factor_item", "middle_node_item_ids"]),
  ];
  if (contracts.some((entry) => !entry.all_required_tokens_present)) throw new Error("A required selected-factor route token is missing from source");
  return contracts;
}

function codeContract(file, contractId, requiredTokens) {
  const source = readFileSync(file, "utf8");
  const tokens = requiredTokens.map((token) => ({ token, present: source.includes(token) }));
  return { contract_id: contractId, source: fileDescriptor(file), required_tokens: tokens, all_required_tokens_present: tokens.every((entry) => entry.present) };
}

function summarize(obligations, runtimeCatalog, contracts) {
  return {
    selector_obligations: obligations.length,
    unique_sources: new Set(obligations.map((entry) => entry.source_rule_id)).size,
    unique_factor_buff_ids: new Set(obligations.flatMap((entry) => entry.effect_ids.map(Number))).size,
    grade_item_routes: obligations.reduce((sum, entry) => sum + entry.grade_item_routes.length, 0),
    current_runtime_selector_catalog_items: Number(runtimeCatalog.summary.factor_items),
    exact_local_full_snapshot_routes_proven: 1,
    dirty_transition_routes_proven: 0,
    remote_selection_routes_proven: 0,
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
  const report = readJson(input, "selected-factor runtime route proof");
  if (report.schema_version !== 1 || report.generated_by !== "tools/bpsr-selected-factor-runtime-route-proof.mjs" || report.proof_state !== "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open") throw new Error("Invalid selected-factor route proof schema/generator/state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Selected-factor route proof content hash mismatch");
  if (report.policy?.exact_local_full_snapshot_selection_is_proven !== true || report.policy?.dirty_live_transition_selection_remains_open !== true || report.policy?.remote_player_exact_selection_remains_open !== true || report.policy?.localized_x_suffix_is_not_a_grade_selector !== true || report.policy?.selected_factor_identity_does_not_prove_mechanics !== true || report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Selected-factor route proof has an unsafe policy");
  assertObligationCoverage(report.blocker_obligations ?? []);
  if (report.summary?.current_runtime_selector_catalog_items !== 3830) throw new Error("Selected-factor route proof runtime catalog coverage changed");
  if (report.summary?.canonical_code_contracts_satisfied !== report.summary?.canonical_code_contracts) throw new Error("Selected-factor route code coverage mismatch");
  for (const key of ["dirty_transition_routes_proven", "remote_selection_routes_proven", "runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (report.summary?.[key] !== 0) throw new Error(`Selected-factor route improperly closes ${key}`);
  }
  if (!report.still_required_runtime_gates?.length) throw new Error("Selected-factor route omits remaining runtime gates");
  console.log(`Selected-factor runtime route proof verified for build ${report.game_build}: 10 obligations, 100 grade-item routes, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-selected-factor-route-test-"));
  try {
    const obligations = TARGETS.map(([sourceRuleId, effectId], index) => ({
      obligation_id: `${sourceRuleId}#selectedFactorGrade`, source_rule_id: sourceRuleId, source_id: `phantom-factor:${effectId}`, effect_ids: [effectId],
      grade_item_routes: Array.from({ length: 10 }, (_, gradeIndex) => ({ item_id: 20_000_000 + index * 10 + gradeIndex, family_id: 200_000 + index, grade: gradeIndex + 1, primary_buff_id: effectId })),
    }));
    const report = {
      schema_version: 1, generated_by: "tools/bpsr-selected-factor-runtime-route-proof.mjs", game_build: "1", proof_state: "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open",
      policy: { exact_local_full_snapshot_selection_is_proven: true, dirty_live_transition_selection_remains_open: true, remote_player_exact_selection_remains_open: true, localized_x_suffix_is_not_a_grade_selector: true, selected_factor_identity_does_not_prove_mechanics: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      summary: { current_runtime_selector_catalog_items: 3830, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, dirty_transition_routes_proven: 0, remote_selection_routes_proven: 0, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      blocker_obligations: obligations, still_required_runtime_gates: ["dirty transition", "remote selection", "mechanics", "projection", "conservation"],
    };
    report.content_sha256 = contentHash(report);
    const output = path.join(root, "proof.json");
    writeJson(output, report);
    verify(output);
    console.log("Selected-factor runtime route proof self-test passed.");
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
function uniqueSorted(values, comparator = compareText) { return [...new Set(values)].sort(comparator); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function compareIdentifiers(left, right) { const a = Number(left); const b = Number(right); return Number.isFinite(a) && Number.isFinite(b) ? a - b : compareText(left, right); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-selected-factor-runtime-route-proof.mjs build --build <id> --factor-catalog <json> --source-manifest <json> --static-formula-evidence <json> --decoder-source <rs> --dreamscope-inference-source <rs> --factor-correlation-source <rs> --rdps-validation-source <rs> --runtime-selector-catalog <json> --output <json>\n  node tools/bpsr-selected-factor-runtime-route-proof.mjs verify --input <json>\n  node tools/bpsr-selected-factor-runtime-route-proof.mjs self-test"); process.exit(exitCode); }
