#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const runtimePath = resolvePath(options.runtime);
const carryForwardPath = resolvePath(options.carryForward);
const rowDiffPath = resolvePath(options.rowDiff);
const outputPath = resolvePath(options.output);

const runtime = readJson(runtimePath, "rDPS runtime pack");
const carryForward = readJson(carryForwardPath, "formula carry-forward proof");
const rowDiff = readJson(rowDiffPath, "decoded row diff");

validateInputs(runtime, carryForward, rowDiff);

const changedRows = (rowDiff.tables || []).flatMap((table) =>
  [...(table.changed_row_ids || []), ...(table.added_row_ids || []), ...(table.removed_row_ids || [])]
    .map((rowId) => ({ table: table.stable_key, row_id: String(rowId) })),
);
const changedRowIds = new Set(changedRows.map((row) => row.row_id));
const stableFormulaTables = (carryForward.current_static_surface?.tables || []).map((table) => ({
  table_name: table.table_name,
  sha256: table.sha256,
}));
const formulaSurfaceStable =
  carryForward.policy?.byte_identical_static_tables_are_current_build_evidence === true
  && stableFormulaTables.length > 0
  && carryForward.current_static_surface?.reviewed_change?.damage_or_modifier_formula_field_changed === false;

const rules = ruleConfigs(runtime).map(({ ruleId, config }) => {
  const identityReferences = collectIdentityReferences(config);
  const changedIdentityReferences = identityReferences.filter((reference) =>
    changedRowIds.has(String(reference.value)),
  );
  const status = changedIdentityReferences.length > 0
    ? "requires-rule-audit"
    : formulaSurfaceStable
      ? "provisional-carry-forward"
      : "stale-dependency-proof-missing";
  return {
    rule_id: ruleId,
    authored_build: String(runtime.game_build),
    observed_build: String(carryForward.build_id),
    status,
    exact_identity_references: identityReferences,
    changed_identity_references: changedIdentityReferences,
    formula_surface_evidence: stableFormulaTables,
    runtime_behavior: status === "provisional-carry-forward"
      ? "calculate-and-warn"
      : "retain-events-and-show-rule-warning",
  };
});

const result = {
  schema_version: 1,
  generated_by: "tools/rdps-runtime-compatibility.mjs",
  game: "blue-protocol-star-resonance",
  deployment_id: runtime.deployment_id,
  authored_build: String(runtime.game_build),
  observed_build: String(carryForward.build_id),
  policy: {
    stale_same_deployment_rules_produce_blank_rdps: false,
    unchanged_dependencies_carry_forward_provisionally: true,
    changed_rules_are_isolated: true,
    unresolved_evidence_hidden: false,
    current_build_capture_upgrades_confidence_but_does_not_unlock_display: true,
  },
  inputs: {
    runtime_pack: relativePath(runtimePath),
    formula_carry_forward: relativePath(carryForwardPath),
    latest_transition_diff: relativePath(rowDiffPath),
  },
  transition: {
    baseline_build: String(rowDiff.baseline_build_id),
    candidate_build: String(rowDiff.build_id),
    unchanged_rows: rowDiff.summary?.unchanged_rows ?? 0,
    changed_rows: changedRows,
    reviewed_formula_field_changed:
      carryForward.current_static_surface?.reviewed_change?.damage_or_modifier_formula_field_changed ?? null,
  },
  summary: {
    rules: rules.length,
    provisional_carry_forward: rules.filter((rule) => rule.status === "provisional-carry-forward").length,
    requires_rule_audit: rules.filter((rule) => rule.status === "requires-rule-audit").length,
    stale_dependency_proof_missing: rules.filter((rule) => rule.status === "stale-dependency-proof-missing").length,
  },
  rules,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function ruleConfigs(value) {
  return [
    ["team-luck", value.team_luck],
    ["functional-amp", value.functional_amp],
    ["mechanical-power", value.mechanical_power],
    ["harmony-grace", value.harmony_grace],
    ["thunderwind", value.thunderwind],
    ["inspiration", value.inspiration],
  ].map(([ruleId, config]) => ({ ruleId, config }));
}

function collectIdentityReferences(value, currentPath = "") {
  if (!value || typeof value !== "object") return [];
  const references = [];
  for (const [key, child] of Object.entries(value)) {
    const childPath = currentPath ? `${currentPath}.${key}` : key;
    if (Array.isArray(child)) {
      if (key.endsWith("_ids")) {
        for (const item of child) {
          if (Number.isSafeInteger(item) && item > 0) references.push({ path: childPath, value: item });
        }
      } else {
        child.forEach((item, index) => references.push(...collectIdentityReferences(item, `${childPath}[${index}]`)));
      }
    } else if (Number.isSafeInteger(child) && child > 0 && key.endsWith("_id")) {
      references.push({ path: childPath, value: child });
    } else if (child && typeof child === "object") {
      references.push(...collectIdentityReferences(child, childPath));
    }
  }
  return references.sort((left, right) => left.path.localeCompare(right.path) || left.value - right.value);
}

function validateInputs(runtimeValue, carryValue, diffValue) {
  if (runtimeValue.deployment_id !== carryValue.deployment_id
    || runtimeValue.deployment_id !== diffValue.deployment_id) {
    throw new Error("deployment identity differs across compatibility inputs");
  }
  if (String(carryValue.build_id) !== String(diffValue.build_id)) {
    throw new Error("carry-forward proof and decoded row diff describe different candidate builds");
  }
  if (runtimeValue.policy?.same_deployment_build_mismatch !== "provisional-carry-forward"
    || runtimeValue.policy?.warn_on_build_mismatch !== true) {
    throw new Error("runtime pack does not declare the provisional carry-forward policy");
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || !value) throw new Error(`invalid argument near ${key || "end"}`);
    result[key.slice(2)] = value;
  }
  for (const key of ["runtime", "carryForward", "rowDiff", "output"]) {
    if (!result[key]) throw new Error(`--${key} is required`);
  }
  return result;
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`could not read ${label} at ${filePath}: ${error.message}`);
  }
}

function resolvePath(filePath) {
  return path.isAbsolute(filePath) ? filePath : path.resolve(repoRoot, filePath);
}

function relativePath(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
