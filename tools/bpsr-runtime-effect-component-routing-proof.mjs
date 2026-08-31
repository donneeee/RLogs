#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    contribution: path.resolve(required(parsed, "contribution")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  requireFile(context.contribution, "modifier contribution runtime");
  const contribution = readJson(context.contribution, "modifier contribution runtime");
  if (!contribution.sourcesByRuleId || typeof contribution.sourcesByRuleId !== "object") {
    throw new Error("Modifier contribution runtime has no sourcesByRuleId map");
  }

  const routes = new Map();
  const unboundComponents = [];
  let relationshipComponents = 0;

  for (const [sourceRuleId, source] of Object.entries(contribution.sourcesByRuleId)) {
    for (const component of source.relationshipComponents ?? []) {
      relationshipComponents += 1;
      const binding = component.proofBinding ?? {};
      const runtimeBuffId = normalizeIdentifier(binding.runtimeBuffId);
      const evidence = {
        source_rule_id: sourceRuleId,
        source_id: source.sourceId ?? null,
        source_name: source.sourceName ?? null,
        component_key: component.componentKey ?? null,
        label: component.label ?? null,
        effect_class: component.effectClass ?? null,
        direction: component.direction ?? null,
        stat: component.stat ?? null,
        contribution_scope: component.contributionScope ?? null,
        value_scope: component.valueScope ?? null,
        transfer_eligibility: component.transferEligibility ?? null,
        formula_replay_status: component.formulaReplayStatus ?? null,
        formula_term_ids: uniqueSorted(component.formulaTermIds ?? []),
        contribution_groups: uniqueSorted(component.contributionGroups ?? []),
        predicate_tags: uniqueSorted(component.predicateTags ?? []),
        required_runtime_evidence: uniqueSorted(component.requiredRuntimeEvidence ?? []),
        value_resolution: component.valueResolution ?? null,
        values: normalizeComponentValues(component.values ?? []),
        // Retain the legacy singular fields when an older extractor supplies
        // them, but never substitute them for the current-build arrays above.
        value: component.value ?? null,
        predicate: component.predicate ?? null,
        proof_binding: {
          source_node_id: binding.sourceNodeId ?? null,
          semantic_buff_id: normalizeIdentifier(binding.semanticBuffId),
          runtime_buff_id: runtimeBuffId,
          origin_source_config_id: normalizeIdentifier(binding.originSourceConfigId),
        },
      };
      if (runtimeBuffId === null) {
        unboundComponents.push(evidence);
        continue;
      }
      const values = routes.get(runtimeBuffId) ?? [];
      values.push(evidence);
      routes.set(runtimeBuffId, values);
    }
  }

  const effectRoutes = [...routes.entries()].map(([effectId, components]) => {
    const sortedComponents = components.sort(compareComponentEvidence);
    const external = sortedComponents.filter(isExternalCandidate);
    const nonOutgoing = sortedComponents.filter(isProvenNonOutgoing);
    const unresolved = sortedComponents.filter((entry) => !isExternalCandidate(entry) && !isProvenNonOutgoing(entry));
    const routeClass = classifyRoute(external.length, nonOutgoing.length, unresolved.length);
    return {
      effect_id: effectId,
      route_class: routeClass,
      runtime_credit_candidate: external.length > 0,
      proven_no_outgoing_attribution: external.length === 0 && unresolved.length === 0 && nonOutgoing.length > 0,
      component_counts: {
        total: sortedComponents.length,
        external_candidate: external.length,
        proven_non_outgoing: nonOutgoing.length,
        unresolved: unresolved.length,
      },
      source_rule_ids: uniqueSorted(sortedComponents.map((entry) => entry.source_rule_id)),
      components: sortedComponents,
    };
  }).sort((left, right) => compareIdentifiers(left.effect_id, right.effect_id));

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-runtime-effect-component-routing-proof.mjs",
    game_build: context.build,
    policy: {
      component_level_routing_precedes_mixed_source_aggregation: true,
      non_outgoing_requires_every_bound_component_to_be_explicitly_non_outgoing: true,
      unknown_or_mixed_components_remain_open: true,
      no_runtime_effect_is_hidden_or_deleted: true,
      source_file_is_bound_by_sha256: true,
    },
    inputs: {
      modifier_contribution_runtime: fileDescriptor(context.contribution),
    },
    summary: {
      source_rules: Object.keys(contribution.sourcesByRuleId).length,
      relationship_components: relationshipComponents,
      runtime_bound_components: effectRoutes.reduce((sum, entry) => sum + entry.component_counts.total, 0),
      components_without_runtime_effect_binding: unboundComponents.length,
      runtime_effect_routes: effectRoutes.length,
      route_class_counts: countBy(effectRoutes, (entry) => entry.route_class),
      runtime_credit_candidates: effectRoutes.filter((entry) => entry.runtime_credit_candidate).length,
      proven_non_outgoing_runtime_effects: effectRoutes.filter((entry) => entry.proven_no_outgoing_attribution).length,
      unresolved_runtime_effect_routes: effectRoutes.filter((entry) => entry.component_counts.unresolved > 0).length,
      hidden_omissions: 0,
    },
    effect_routes: effectRoutes,
    components_without_runtime_effect_binding: unboundComponents.sort(compareComponentEvidence),
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(
    `Runtime-effect component routing proof built for ${context.build}: ${effectRoutes.length} bound effects, ` +
    `${report.summary.proven_non_outgoing_runtime_effects} proven non-outgoing, ` +
    `${report.summary.runtime_credit_candidates} external candidates, zero hidden omissions.`,
  );
}

function verify(input) {
  requireFile(input, "runtime-effect component routing proof");
  const report = readJson(input, "runtime-effect component routing proof");
  if (report.schema_version !== 1) throw new Error(`Unsupported routing proof schema ${report.schema_version}`);
  if (report.generated_by !== "tools/bpsr-runtime-effect-component-routing-proof.mjs") {
    throw new Error(`Unexpected routing proof generator ${report.generated_by}`);
  }
  if (!/^\d+$/.test(String(report.game_build ?? ""))) throw new Error("Routing proof has an invalid game build");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Routing proof content hash mismatch");
  if (!report.policy?.component_level_routing_precedes_mixed_source_aggregation ||
    !report.policy?.non_outgoing_requires_every_bound_component_to_be_explicitly_non_outgoing ||
    !report.policy?.unknown_or_mixed_components_remain_open ||
    !report.policy?.no_runtime_effect_is_hidden_or_deleted) {
    throw new Error("Runtime-effect component routing policy is unsafe");
  }

  const routes = uniqueIndex(report.effect_routes ?? [], "effect_id", "runtime-effect route");
  for (const route of routes.values()) {
    if (!Array.isArray(route.components) || route.components.length === 0) {
      throw new Error(`Runtime effect ${route.effect_id} has no preserved components`);
    }
    const external = route.components.filter(isExternalCandidate).length;
    const nonOutgoing = route.components.filter(isProvenNonOutgoing).length;
    const unresolved = route.components.length - external - nonOutgoing;
    if (route.route_class !== classifyRoute(external, nonOutgoing, unresolved)) {
      throw new Error(`Runtime effect ${route.effect_id} route class is inconsistent with its components`);
    }
    if (route.runtime_credit_candidate !== (external > 0)) {
      throw new Error(`Runtime effect ${route.effect_id} external-candidate flag is inconsistent`);
    }
    const provenNoOutgoing = external === 0 && unresolved === 0 && nonOutgoing > 0;
    if (route.proven_no_outgoing_attribution !== provenNoOutgoing) {
      throw new Error(`Runtime effect ${route.effect_id} non-outgoing proof is unsafe`);
    }
    for (const component of route.components) verifyComponentValues(component, route.effect_id);
  }
  const routeClasses = countBy([...routes.values()], (entry) => entry.route_class);
  if (stableStringify(routeClasses) !== stableStringify(report.summary?.route_class_counts ?? {})) {
    throw new Error("Runtime-effect route class summary mismatch");
  }
  if (routes.size !== Number(report.summary?.runtime_effect_routes) || Number(report.summary?.hidden_omissions) !== 0) {
    throw new Error("Runtime-effect routing summary count mismatch");
  }
  console.log(
    `Runtime-effect component routing proof verified for ${report.game_build}: ${routes.size} effects, ` +
    `${report.summary.proven_non_outgoing_runtime_effects} proven non-outgoing.`,
  );
  return report;
}

function inspect(input, parsed) {
  const report = verify(input);
  const effectId = parsed.effect ? String(parsed.effect) : null;
  const routes = effectId
    ? report.effect_routes.filter((entry) => entry.effect_id === effectId)
    : report.effect_routes;
  if (effectId && routes.length === 0) throw new Error(`Unknown runtime effect ${effectId}`);
  const limit = parsed.limit === undefined ? 20 : positiveInteger(parsed.limit, "limit");
  console.log(JSON.stringify({ summary: report.summary, effect_routes: routes.slice(0, limit) }, null, 2));
}

function classifyRoute(external, nonOutgoing, unresolved) {
  if (external > 0 && nonOutgoing > 0) return unresolved > 0
    ? "mixed-external-non-outgoing-and-unresolved"
    : "mixed-external-and-non-outgoing";
  if (external > 0) return unresolved > 0 ? "external-candidate-with-unresolved-components" : "external-candidate";
  if (nonOutgoing > 0 && unresolved === 0) return "proven-non-outgoing-context";
  if (nonOutgoing > 0) return "non-outgoing-with-unresolved-components";
  return "unresolved";
}

function isExternalCandidate(component) {
  return component.transfer_eligibility === "external-recipient-candidate";
}

function isProvenNonOutgoing(component) {
  return component.transfer_eligibility === "non-outgoing-context" &&
    component.formula_replay_status === "not-outgoing-rdps";
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-runtime-effect-routing-"));
  try {
    const contribution = path.join(root, "ModifierContributionRuntime.json");
    const output = path.join(root, "routing.json");
    writeJson(contribution, {
      schemaVersion: 1,
      generatedBy: "fixture",
      sourcesByRuleId: {
        "mrs:test": {
          sourceId: "season-talent-node:1501",
          relationshipComponents: [
            {
              componentKey: "damage",
              label: "Target vulnerability",
              transferEligibility: "external-recipient-candidate",
              formulaReplayStatus: "blocked-formula-placement-unproven",
              predicateTags: ["target.vulnerability", "direction.damage-dealt"],
              requiredRuntimeEvidence: ["exact counterfactual formula placement", "provider actor UID"],
              valueResolution: "single",
              values: [{ rawText: "10%", unit: "percent", value: 10, decimalValue: 0.1, formulaAmount: true }],
              proofBinding: { runtimeBuffId: 3003012 },
            },
            { componentKey: "attack-reduction", transferEligibility: "non-outgoing-context", formulaReplayStatus: "not-outgoing-rdps", proofBinding: { runtimeBuffId: 3003012 } },
            { componentKey: "slow", transferEligibility: "non-outgoing-context", formulaReplayStatus: "not-outgoing-rdps", proofBinding: { runtimeBuffId: 3003014 } },
            { componentKey: "unbound", transferEligibility: "external-recipient-candidate", formulaReplayStatus: "blocked", proofBinding: {} },
          ],
        },
      },
    });
    build({ build: "1", contribution, output });
    const report = verify(output);
    const mixed = report.effect_routes.find((entry) => entry.effect_id === "3003012");
    const slow = report.effect_routes.find((entry) => entry.effect_id === "3003014");
    if (mixed?.route_class !== "mixed-external-and-non-outgoing" || !mixed.runtime_credit_candidate ||
      mixed.proven_no_outgoing_attribution) {
      throw new Error("Self-test failed to preserve a mixed external runtime effect");
    }
    const damage = mixed.components.find((entry) => entry.component_key === "damage");
    if (damage?.label !== "Target vulnerability" || damage.value_resolution !== "single" ||
      damage.values?.length !== 1 || damage.values[0].raw_text !== "10%" ||
      damage.values[0].decimal_value !== 0.1 || damage.values[0].formula_amount !== true ||
      damage.predicate_tags.join(",") !== "direction.damage-dealt,target.vulnerability" ||
      damage.required_runtime_evidence.length !== 2) {
      throw new Error("Self-test failed to preserve current-build component scalar evidence");
    }
    if (slow?.route_class !== "proven-non-outgoing-context" || slow.runtime_credit_candidate ||
      !slow.proven_no_outgoing_attribution) {
      throw new Error("Self-test failed to close an exact non-outgoing runtime effect");
    }
    if (report.summary.components_without_runtime_effect_binding !== 1) {
      throw new Error("Self-test failed to preserve an unbound relationship component");
    }
    console.log("bpsr-runtime-effect-component-routing-proof self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function normalizeIdentifier(value) {
  if (value === undefined || value === null || value === "") return null;
  const text = String(value);
  return /^\d+$/.test(text) ? String(BigInt(text)) : text;
}
function normalizeComponentValues(values) {
  if (!Array.isArray(values)) throw new Error("Relationship component values must be an array");
  return values.map((entry) => ({
    scope: entry?.scope ?? null,
    raw_text: entry?.rawText ?? null,
    unit: entry?.unit ?? null,
    value: entry?.value ?? null,
    decimal_value: entry?.decimalValue ?? null,
    formula_amount: entry?.formulaAmount === true,
    inferred_from: entry?.inferredFrom ?? null,
  })).sort((left, right) => compareText(left.scope, right.scope) ||
    compareText(left.raw_text, right.raw_text) || compareText(left.value, right.value));
}
function verifyComponentValues(component, effectId) {
  if (!Array.isArray(component.predicate_tags) || !Array.isArray(component.required_runtime_evidence) ||
    !Array.isArray(component.values)) {
    throw new Error(`Runtime effect ${effectId} has incomplete component evidence arrays`);
  }
  if (component.value_resolution === "single" && component.values.length !== 1) {
    throw new Error(`Runtime effect ${effectId} has an invalid single-value component`);
  }
  for (const value of component.values) {
    if (!value || typeof value !== "object" || typeof value.formula_amount !== "boolean") {
      throw new Error(`Runtime effect ${effectId} has malformed component scalar evidence`);
    }
  }
}
function compareComponentEvidence(left, right) {
  return compareIdentifiers(left.proof_binding?.runtime_buff_id ?? "", right.proof_binding?.runtime_buff_id ?? "") ||
    compareText(left.source_rule_id, right.source_rule_id) || compareText(left.component_key, right.component_key);
}
function uniqueIndex(values, key, label) { const result = new Map(); for (const value of values) { const id = value?.[key]; if (id === undefined || id === null || id === "") throw new Error(`${label} is missing ${key}`); if (result.has(String(id))) throw new Error(`Duplicate ${label} ${id}`); result.set(String(id), value); } return result; }
function countBy(values, selector) { const counts = {}; for (const value of values) { const key = selector(value); counts[key] = (counts[key] ?? 0) + 1; } return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right))); }
function uniqueSorted(values) { return [...new Set(values.filter((value) => value !== undefined && value !== null).map(String))].sort(compareText); }
function compareIdentifiers(left, right) { const a = Number(left); const b = Number(right); return Number.isSafeInteger(a) && Number.isSafeInteger(b) && a !== b ? a - b : compareText(left, right); }
function compareText(left, right) { return String(left ?? "").localeCompare(String(right ?? ""), "en"); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function positiveInteger(value, label) { const parsed = Number(value); if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`--${label} must be a positive integer`); return parsed; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-runtime-effect-component-routing-proof.mjs build --build <id> --contribution <ModifierContributionRuntime.json> --output <json>\n  node tools/bpsr-runtime-effect-component-routing-proof.mjs verify --input <json>\n  node tools/bpsr-runtime-effect-component-routing-proof.mjs inspect --input <json> [--effect <id>] [--limit <count>]\n  node tools/bpsr-runtime-effect-component-routing-proof.mjs self-test"); process.exit(exitCode); }
