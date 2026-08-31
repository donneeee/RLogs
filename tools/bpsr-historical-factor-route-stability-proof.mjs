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
    factorClosure: path.resolve(required(parsed, "factor-closure")),
    correlationBundle: path.resolve(required(parsed, "correlation-bundle")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  requireFile(context.factorClosure, "factor closure");
  requireFile(context.correlationBundle, "historical factor correlation bundle");
  const closure = readJson(context.factorClosure, "factor closure");
  const bundle = readJson(context.correlationBundle, "historical factor correlation bundle");
  validateClosure(closure, context.build);
  const reports = validateHistoricalBundle(bundle, context.build);
  const itemIndex = buildItemIndex(closure.families ?? []);
  const routeKeys = uniqueText(reports.flatMap((report) => (report.rule_summaries ?? []).map((summary) => routeKey(summary.factor_item_id, summary.effect_id))));
  const routes = routeKeys.map((key) => buildRoute(key, reports, itemIndex)).sort(compareRoute);

  const report = {
    schema_version: 2,
    generated_by: "tools/bpsr-historical-factor-route-stability-proof.mjs",
    current_game_build: context.build,
    proof_state: "historical-runtime-routes-current-static-identities-audited-current-runtime-gates-open",
    policy: {
      historical_packets_are_never_current_build_runtime_proof: true,
      catalog_build_and_observed_packet_build_are_distinct: true,
      stable_item_effect_identity_is_only_a_prioritization_receipt: true,
      historical_provider_recipient_windows_do_not_prove_current_transferability: true,
      historical_action_matches_do_not_prove_current_counterfactuals: true,
      historical_evidence_never_promotes_rdps: true,
      unresolved_or_changed_routes_are_never_hidden: true,
    },
    inputs: {
      current_factor_closure: fileDescriptor(context.factorClosure),
      historical_factor_correlation_bundle: fileDescriptor(context.correlationBundle),
    },
    provenance_contract: {
      current_catalog_build_route: "current_game_build plus reports[].game_build",
      observed_historical_build_route: "reports[].observed_client_builds",
      observed_historical_protocol_pack_route: "reports[].observed_protocol_pack_digests",
      selection_route: "reports[].selection_observations[].selected_factor_item_ids",
      lifecycle_route: "reports[].windows[]",
      action_route: "reports[].windows[].action_damage and reports[].rule_summaries[].matched_action_damage",
      current_static_identity_route: "psychoscope-factor-offline-closure.families[].grade_routes[]",
    },
    summary: summarize(routes, reports),
    routes,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Historical factor route stability proof built: ${routes.length} routes, ${report.summary.current_static_identity_stable_routes} stable current identities, ${report.summary.historical_unique_matched_action_events} unique historical action events, zero rDPS promotions.`);
}

function buildItemIndex(families) {
  const index = new Map();
  for (const family of families) {
    for (const grade of family.grade_routes ?? []) {
      const itemId = numeric(grade.item_id);
      if (!itemId) continue;
      if (index.has(itemId)) throw new Error(`Factor item ${itemId} appears in more than one current family`);
      index.set(itemId, { family, grade });
    }
  }
  return index;
}

function buildRoute(key, reports, itemIndex) {
  const [itemIdText, effectIdText] = key.split(":");
  const itemId = Number(itemIdText);
  const effectId = Number(effectIdText);
  const current = itemIndex.get(itemId) ?? null;
  const currentEffectId = numeric(current?.grade?.source_buff_id);
  const identityState = !current
    ? "historical-item-absent-from-current-factor-closure"
    : currentEffectId !== effectId
      ? "historical-item-current-effect-identity-changed"
      : "historical-item-effect-identity-stable-in-current-static-closure";
  const selected = [];
  const summaries = [];
  const windows = [];
  const observedBuilds = new Set();
  const observedDigests = new Set();

  for (const report of reports) {
    for (const build of report.observed_client_builds ?? []) observedBuilds.add(String(build));
    for (const digest of report.observed_protocol_pack_digests ?? []) observedDigests.add(String(digest));
    for (const observation of report.selection_observations ?? []) {
      if (!(observation.selected_factor_item_ids ?? []).map(Number).includes(itemId)) continue;
      selected.push({
        session_id: String(report.session_id ?? ""),
        sequence: numeric(observation.sequence),
        observed_micros: numeric(observation.observed_micros),
        character_id: String(observation.character_id ?? ""),
      });
    }
    for (const summary of report.rule_summaries ?? []) {
      if (numeric(summary.factor_item_id) !== itemId || numeric(summary.effect_id) !== effectId) continue;
      summaries.push({ session_id: String(report.session_id ?? ""), ...structuredClone(summary) });
    }
    for (const window of report.windows ?? []) {
      const itemMatch = (window.factor_item_ids ?? []).map(Number).includes(itemId);
      if (numeric(window.effect_id) !== effectId || !itemMatch) continue;
      windows.push(compactWindow(report.session_id, window));
    }
  }

  const actionRows = windows.flatMap((window) => window.action_damage.map((action) => ({ session_id: window.session_id, window_id: window.window_id, ...action })));
  const matchedAction = aggregateTotals(summaries.map((summary) => summary.matched_action_damage));
  const providerRecipient = {
    total_windows: windows.length,
    self_windows: windows.filter((window) => window.provider_entity_uuid && window.provider_entity_uuid === window.recipient_entity_uuid).length,
    distinct_provider_recipient_windows: windows.filter((window) => window.provider_entity_uuid && window.recipient_entity_uuid && window.provider_entity_uuid !== window.recipient_entity_uuid).length,
    unknown_provider_or_recipient_windows: windows.filter((window) => !window.provider_entity_uuid || !window.recipient_entity_uuid).length,
  };
  const stillRequired = [
    "current-build-selected-grade-observation",
    "current-build-source-effect-lifecycle",
    "current-build-exact-provider-recipient-binding",
    "current-build-trigger-and-output-correlation",
    "integer-counterfactual-projection",
    "party-damage-conservation",
  ];

  return {
    route_id: `historical-factor:${itemId}:effect:${effectId}`,
    historical_identity: { factor_item_id: itemId, effect_id: effectId },
    current_static_identity: current ? {
      family_id: numeric(current.family.family_id),
      family_name: String(current.family.family_name ?? ""),
      class_gate_ids: uniqueNumeric(current.family.class_gate_ids ?? []),
      slot_category: current.family.slot_category ?? null,
      runtime_role: current.family.runtime_role ?? null,
      current_runtime_eligible: current.family.current_runtime_eligible === true,
      grade: numeric(current.grade.grade),
      factor_item_id: itemId,
      source_buff_id: currentEffectId,
      parameter_values: (current.grade.parameter_values ?? []).map(Number),
      energy_behavior: current.grade.energy_behavior ?? null,
      energy_amount: numberOrNull(current.grade.energy_amount),
      resolved_description: current.grade.resolved_description ?? null,
      mechanic_classes: uniqueText(current.family.mechanic_classes ?? []),
      exact_recount_ids: uniqueNumeric(current.family.exact_recount_ids ?? []),
      direct_damage_ids: uniqueNumeric(current.family.direct_damage_ids ?? []),
      generated_damage_families: structuredClone(current.family.generated_damage_families ?? []),
      generated_output_families: structuredClone(current.family.generated_output_families ?? []),
      offline_route_state: current.family.offline_route_state ?? null,
      final_validation_obligations: uniqueText(current.family.final_validation_obligations ?? []),
    } : null,
    identity_state: identityState,
    historical_packet_provenance: {
      observed_client_builds: [...observedBuilds].sort(compareText),
      observed_protocol_pack_digests: [...observedDigests].sort(compareText),
    },
    historical_runtime_evidence: {
      sessions: uniqueText([...selected, ...summaries, ...windows].map((entry) => entry.session_id)),
      selection_observations: selected.sort(compareObservation),
      rule_summary_count: summaries.length,
      lifecycle_windows: windows.sort(compareWindow),
      lifecycle_totals: {
        apply_count: sum(windows, "apply_count"),
        refresh_count: sum(windows, "refresh_count"),
        stack_count: sum(windows, "stack_count"),
        consume_count: sum(windows, "consume_count"),
        remove_count: sum(windows, "remove_count"),
      },
      provider_recipient: providerRecipient,
      matched_action_damage: matchedAction,
      action_rows: actionRows.sort(compareAction),
      observed_action_ability_ids: uniqueNumeric(actionRows.map((entry) => entry.ability_id)),
      observed_action_recount_group_ids: uniqueNumeric(actionRows.map((entry) => entry.recount_group_id)),
    },
    current_runtime_proof_state: "historical-prior-only-current-build-runtime-proof-required",
    current_runtime_gates_closed: 0,
    rdps_promoted: false,
    hidden_omissions: 0,
    still_required_current_runtime_gates: stillRequired,
  };
}

function compactWindow(sessionId, window) {
  return {
    session_id: String(sessionId ?? ""),
    window_id: String(window.window_id ?? ""),
    effect_id: numeric(window.effect_id),
    provider_entity_uuid: String(window.provider_entity_uuid ?? ""),
    recipient_entity_uuid: String(window.recipient_entity_uuid ?? ""),
    opened_sequence: numeric(window.opened_sequence),
    opened_observed_micros: numeric(window.opened_observed_micros),
    closed_sequence: numberOrNull(window.closed_sequence),
    closed_observed_micros: numberOrNull(window.closed_observed_micros),
    close_reason: window.close_reason ?? null,
    minimum_observed_stacks: numeric(window.minimum_observed_stacks),
    maximum_observed_stacks: numeric(window.maximum_observed_stacks),
    apply_count: numeric(window.apply_count),
    refresh_count: numeric(window.refresh_count),
    stack_count: numeric(window.stack_count),
    consume_count: numeric(window.consume_count),
    remove_count: numeric(window.remove_count),
    action_damage: (window.action_damage ?? []).map((action) => ({
      ability_id: numeric(action.ability_id),
      recount_group_id: numeric(action.recount_group_id),
      relation_kind: action.relation_kind ?? null,
      action_role: action.action_role ?? null,
      actor_relation: action.actor_relation ?? null,
      totals: normalizeTotals(action.totals),
    })),
  };
}

function summarize(routes, reports) {
  const routeWindows = routes.flatMap((route) => route.historical_runtime_evidence.lifecycle_windows);
  const uniqueWindows = uniqueBy(routeWindows, physicalWindowKey);
  const routeActionRows = routes.flatMap((route) => route.historical_runtime_evidence.action_rows);
  const uniqueActionRows = uniqueBy(routeActionRows, physicalActionKey);
  const routeAction = aggregateTotals(routeActionRows.map((row) => row.totals));
  const uniqueAction = aggregateTotals(uniqueActionRows.map((row) => row.totals));
  return {
    historical_capture_reports: reports.length,
    historical_observed_client_builds: uniqueText(reports.flatMap((report) => report.observed_client_builds ?? [])),
    historical_observed_protocol_pack_digests: uniqueText(reports.flatMap((report) => report.observed_protocol_pack_digests ?? [])),
    historical_item_effect_routes: routes.length,
    current_static_identity_stable_routes: routes.filter((route) => route.identity_state === "historical-item-effect-identity-stable-in-current-static-closure").length,
    current_static_identity_changed_routes: routes.filter((route) => route.identity_state === "historical-item-current-effect-identity-changed").length,
    current_static_identity_missing_routes: routes.filter((route) => route.identity_state === "historical-item-absent-from-current-factor-closure").length,
    historical_selection_observations: routes.reduce((total, route) => total + route.historical_runtime_evidence.selection_observations.length, 0),
    historical_route_window_memberships: routeWindows.length,
    historical_unique_lifecycle_windows: uniqueWindows.length,
    historical_route_self_provider_recipient_window_memberships: routeWindows.filter(isSelfWindow).length,
    historical_unique_self_provider_recipient_windows: uniqueWindows.filter(isSelfWindow).length,
    historical_route_distinct_provider_recipient_window_memberships: routeWindows.filter(isDistinctProviderRecipientWindow).length,
    historical_unique_distinct_provider_recipient_windows: uniqueWindows.filter(isDistinctProviderRecipientWindow).length,
    historical_route_matched_action_event_memberships: routeAction.event_count,
    historical_unique_matched_action_events: uniqueAction.event_count,
    historical_route_matched_action_amount_memberships: routeAction.amount,
    historical_unique_matched_action_amount: uniqueAction.amount,
    current_runtime_gates_closed: routes.reduce((total, route) => total + route.current_runtime_gates_closed, 0),
    rdps_routes_promoted: routes.filter((route) => route.rdps_promoted).length,
    hidden_omissions: routes.reduce((total, route) => total + route.hidden_omissions, 0),
  };
}

function validateClosure(closure, build) {
  if (closure.schema_version !== 1 || String(closure.game_build) !== build || !Array.isArray(closure.families)) throw new Error("Current factor closure is incompatible");
}

function validateHistoricalBundle(bundle, currentBuild) {
  if (Number(bundle.schema_version) < 6 || !Array.isArray(bundle.reports) || !bundle.reports.length) throw new Error("Historical correlation bundle must use schema 6 or newer and contain reports");
  for (const report of bundle.reports) {
    if (Number(report.schema_version) < 6) throw new Error(`Historical report ${report.session_id} lacks packet provenance`);
    if (String(report.game_build) !== currentBuild) throw new Error(`Historical report ${report.session_id} was not interpreted with current catalog ${currentBuild}`);
    const builds = uniqueText(report.observed_client_builds ?? []);
    if (!builds.length || builds.includes(currentBuild)) throw new Error(`Historical report ${report.session_id} does not exclusively contain older packet builds`);
    if (!uniqueText(report.observed_protocol_pack_digests ?? []).length) throw new Error(`Historical report ${report.session_id} lacks an observed protocol-pack digest`);
    if (report.rdps_attribution_enabled !== false) throw new Error(`Historical report ${report.session_id} unexpectedly enables rDPS attribution`);
  }
  return bundle.reports;
}

function verify(input) {
  const report = readJson(input, "historical factor route stability proof");
  if (report.schema_version !== 2 || report.generated_by !== "tools/bpsr-historical-factor-route-stability-proof.mjs" || report.proof_state !== "historical-runtime-routes-current-static-identities-audited-current-runtime-gates-open") throw new Error("Invalid historical factor route stability schema/generator/state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Historical factor route stability content hash mismatch");
  const policy = report.policy ?? {};
  for (const key of ["historical_packets_are_never_current_build_runtime_proof", "catalog_build_and_observed_packet_build_are_distinct", "stable_item_effect_identity_is_only_a_prioritization_receipt", "historical_provider_recipient_windows_do_not_prove_current_transferability", "historical_action_matches_do_not_prove_current_counterfactuals", "historical_evidence_never_promotes_rdps", "unresolved_or_changed_routes_are_never_hidden"]) if (policy[key] !== true) throw new Error(`Unsafe or missing policy ${key}`);
  const routes = report.routes ?? [];
  if (new Set(routes.map((route) => route.route_id)).size !== routes.length) throw new Error("Historical factor route IDs are not unique");
  if (routes.some((route) => route.current_runtime_gates_closed !== 0 || route.rdps_promoted || route.hidden_omissions !== 0 || !route.still_required_current_runtime_gates?.length)) throw new Error("Historical evidence closed, promoted, hid, or dropped a current gate");
  const expected = summarize(routes, Array.from({ length: report.summary.historical_capture_reports }, () => ({ observed_client_builds: report.summary.historical_observed_client_builds, observed_protocol_pack_digests: report.summary.historical_observed_protocol_pack_digests })));
  if (stableStringify(expected) !== stableStringify(report.summary)) throw new Error("Historical factor route stability summary mismatch");
  console.log(`Historical factor route stability verified: ${routes.length} routes, ${report.summary.current_static_identity_stable_routes} stable identities, zero current runtime gates closed.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-historical-factor-stability-test-"));
  try {
    const closureFile = path.join(root, "closure.json");
    const bundleFile = path.join(root, "bundle.json");
    const output = path.join(root, "proof.json");
    writeJson(closureFile, { schema_version: 1, game_build: "2", families: [{ family_id: 7, family_name: "A", class_gate_ids: [1], source_buff_ids: [90], grade_routes: [{ grade: 1, item_id: 10, source_buff_id: 90, parameter_values: [3], resolved_description: "A" }], mechanic_classes: ["damage-modifier"], exact_recount_ids: [4], final_validation_obligations: ["runtime"] }] });
    writeJson(bundleFile, { schema_version: 6, reports: [{ schema_version: 6, game_build: "2", observed_client_builds: ["1"], observed_protocol_pack_digests: ["sha256:old"], session_id: "s", rdps_attribution_enabled: false, selection_observations: [{ sequence: 1, observed_micros: 2, character_id: "3", selected_factor_item_ids: [10] }], rule_summaries: [{ factor_item_id: 10, effect_id: 90, window_count: 1, matched_action_damage: { event_count: 2, amount: 20 } }], windows: [{ window_id: "w", effect_id: 90, factor_item_ids: [10], provider_entity_uuid: "p", recipient_entity_uuid: "p", opened_sequence: 2, opened_observed_micros: 3, apply_count: 1, action_damage: [{ ability_id: 11, recount_group_id: 4, totals: { event_count: 2, amount: 20 } }] }] }] });
    build({ build: "2", factorClosure: closureFile, correlationBundle: bundleFile, output });
    const proof = verify(output);
    if (proof.summary.current_static_identity_stable_routes !== 1 || proof.summary.historical_unique_matched_action_events !== 2 || proof.summary.historical_route_matched_action_event_memberships !== 2) throw new Error("Self-test stable route mismatch");
    const changed = readJson(closureFile, "closure");
    changed.families[0].grade_routes[0].source_buff_id = 91;
    writeJson(closureFile, changed);
    const changedOutput = path.join(root, "changed.json");
    build({ build: "2", factorClosure: closureFile, correlationBundle: bundleFile, output: changedOutput });
    if (verify(changedOutput).summary.current_static_identity_changed_routes !== 1) throw new Error("Self-test did not preserve changed identity");
    console.log("bpsr-historical-factor-route-stability-proof self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function normalizeTotals(value) { return { event_count: numeric(value?.event_count), amount: numeric(value?.amount), first_observed_micros: numberOrNull(value?.first_observed_micros), last_observed_micros: numberOrNull(value?.last_observed_micros) }; }
function aggregateTotals(values) { const normalized = values.filter(Boolean).map(normalizeTotals); return { event_count: normalized.reduce((sum, value) => sum + value.event_count, 0), amount: normalized.reduce((sum, value) => sum + value.amount, 0), first_observed_micros: minNullable(normalized.map((value) => value.first_observed_micros)), last_observed_micros: maxNullable(normalized.map((value) => value.last_observed_micros)) }; }
function minNullable(values) { const present = values.filter((value) => value !== null); return present.length ? Math.min(...present) : null; }
function maxNullable(values) { const present = values.filter((value) => value !== null); return present.length ? Math.max(...present) : null; }
function sum(entries, key) { return entries.reduce((total, entry) => total + numeric(entry[key]), 0); }
function numeric(value) { const parsed = Number(value ?? 0); return Number.isFinite(parsed) ? parsed : 0; }
function numberOrNull(value) { return value === null || value === undefined ? null : numeric(value); }
function routeKey(itemId, effectId) { return `${numeric(itemId)}:${numeric(effectId)}`; }
function compareRoute(left, right) { return numeric(left.historical_identity.factor_item_id) - numeric(right.historical_identity.factor_item_id) || numeric(left.historical_identity.effect_id) - numeric(right.historical_identity.effect_id); }
function compareObservation(left, right) { return compareText(`${left.session_id}:${String(left.sequence).padStart(20, "0")}`, `${right.session_id}:${String(right.sequence).padStart(20, "0")}`); }
function compareWindow(left, right) { return compareText(`${left.session_id}:${String(left.opened_sequence).padStart(20, "0")}:${left.window_id}`, `${right.session_id}:${String(right.opened_sequence).padStart(20, "0")}:${right.window_id}`); }
function compareAction(left, right) { return compareText(`${left.session_id}:${left.window_id}:${left.ability_id}:${left.recount_group_id}`, `${right.session_id}:${right.window_id}:${right.ability_id}:${right.recount_group_id}`); }
function physicalWindowKey(window) { return stableStringify([window.session_id, window.window_id, window.effect_id, window.provider_entity_uuid, window.recipient_entity_uuid, window.opened_sequence, window.opened_observed_micros]); }
function physicalActionKey(action) { return stableStringify([action.session_id, action.window_id, action.ability_id, action.recount_group_id, action.relation_kind, action.action_role, action.actor_relation, action.totals]); }
function isSelfWindow(window) { return Boolean(window.provider_entity_uuid) && window.provider_entity_uuid === window.recipient_entity_uuid; }
function isDistinctProviderRecipientWindow(window) { return Boolean(window.provider_entity_uuid) && Boolean(window.recipient_entity_uuid) && window.provider_entity_uuid !== window.recipient_entity_uuid; }
function uniqueBy(values, keyOf) { const seen = new Set(); return values.filter((value) => { const key = keyOf(value); if (seen.has(key)) return false; seen.add(key); return true; }); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function uniqueText(values) { return [...new Set(values.map(String).filter(Boolean))].sort(compareText); }
function uniqueNumeric(values) { return [...new Set(values.map(Number).filter((value) => Number.isFinite(value) && value !== 0))].sort((left, right) => left - right); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for ${arg}`); parsed[arg.slice(2)] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-historical-factor-route-stability-proof.mjs build --build <id> --factor-closure <json> --correlation-bundle <json> --output <json>\n  node tools/bpsr-historical-factor-route-stability-proof.mjs verify --input <json>\n  node tools/bpsr-historical-factor-route-stability-proof.mjs self-test"); process.exit(exitCode); }
