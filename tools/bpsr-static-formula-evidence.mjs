#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")), options.index ? path.resolve(options.index) : null);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    formulaLedger: path.resolve(required(parsed, "formula-ledger")),
    index: path.resolve(required(parsed, "index")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  requireFile(context.formulaLedger, "formula magnitude ledger");
  requireFile(context.index, "semantic evidence index");
  const ledger = readJson(context.formulaLedger, "formula magnitude ledger");
  requireBuild(ledger, context.build, "static_game_build", "formula magnitude ledger");
  const db = new DatabaseSync(context.index, { readOnly: true });
  let sources;
  try {
    const metadata = Object.fromEntries(db.prepare("SELECT key, value FROM metadata").all().map((row) => [row.key, row.value]));
    if (metadata.game_build !== context.build) throw new Error(`Evidence index build ${metadata.game_build} does not match ${context.build}`);
    const buffRow = db.prepare("SELECT table_name, storage_key, row_id, row_sha256 FROM decoded_rows WHERE table_name = 'BuffTable' AND row_id = ? ORDER BY storage_key");
    sources = (ledger.candidates ?? []).map((candidate) => compileCandidate(candidate, buffRow));
  } finally {
    db.close();
  }
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-static-formula-evidence.mjs",
    game_build: context.build,
    policy: {
      exact_current_build_evidence_only: true,
      all_source_tokens_preserved: true,
      whole_numbers_are_never_percentages: true,
      decoded_formula_does_not_imply_runtime_activation_or_rdps_promotion: true,
      ambiguous_values_remain_visible: true,
    },
    inputs: {
      formula_magnitude_ledger: fileDescriptor(context.formulaLedger),
      semantic_evidence_index: fileDescriptor(context.index),
    },
    summary: summarize(sources),
    sources,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output, context.index);
  console.log(`Static formula evidence built for ${context.build}: ${sources.length} sources, ${report.summary.formula_magnitudes_resolved} magnitudes decoded, ${report.summary.static_gates_resolved} static gates closed, and ${report.summary.hidden_source_tokens} hidden tokens in ${Math.round(performance.now() - started)} ms.`);
}

function compileCandidate(candidate, buffRow) {
  const components = (candidate.current_owner_evidence ?? []).flatMap((owner) =>
    (owner.current_active_modifier_parameter_evidence ?? []).map((component, componentIndex) => ({
      owner_effect_id: owner.effect_id,
      component_index: componentIndex,
      component,
    })),
  );
  const compiled = components.map(({ owner_effect_id, component_index, component }) => compileComponent(owner_effect_id, component_index, component));
  const structured = compiled.filter((item) => item.structured_tier_proof).length;
  const accepted = compiled.flatMap((item) => item.accepted_terms);
  const rejected = compiled.flatMap((item) => item.rejected_terms);
  const percentageFormula = (candidate.formula_term_ids ?? []).some(isPercentageFormulaTerm);
  const exactPercentValues = [...new Set(accepted.filter((term) => term.unit === "percent").map((term) => term.percent_value))];
  const hasStructured = structured > 0;
  const magnitudeResolved = hasStructured || (percentageFormula && exactPercentValues.length > 0);
  const runtimeSelectorRequired = hasStructured || exactPercentValues.length > 1 || (candidate.static_blockers ?? []).some(isSelectorBlocker);
  const remainingStaticBlockers = (candidate.static_blockers ?? []).filter((blocker) => !blockerResolvedByTypedValues(blocker, magnitudeResolved, runtimeSelectorRequired));
  const classification = hasStructured
    ? "complete-structured-tier-ladder"
    : magnitudeResolved && runtimeSelectorRequired
      ? "complete-ladder-runtime-selector-required"
      : magnitudeResolved
        ? "complete-single-value"
        : accepted.length === 0
          ? "missing-value-evidence"
          : "unit-or-formula-model-required";
  const decodedRows = [];
  const rowKeys = new Set();
  for (const effectId of candidate.effect_ids ?? []) {
    for (const row of buffRow.all(String(effectId))) {
      const key = `${row.table_name}:${row.storage_key}`;
      if (rowKeys.has(key)) continue;
      rowKeys.add(key);
      decodedRows.push({ ...row });
    }
  }
  const source = {
    source_rule_id: candidate.source_rule_id,
    source_id: candidate.source_id,
    source_name: candidate.source_name,
    formula_term_ids: candidate.formula_term_ids ?? [],
    effect_ids: candidate.effect_ids ?? [],
    classification,
    formula_magnitude_resolved: magnitudeResolved,
    static_gate_resolved: magnitudeResolved && remainingStaticBlockers.length === 0,
    runtime_selector_required: runtimeSelectorRequired,
    original_static_blockers: candidate.static_blockers ?? [],
    remaining_static_blockers: remainingStaticBlockers,
    remaining_runtime_requirements: unique([
      ...(candidate.required_runtime_evidence ?? []),
      ...(runtimeSelectorRequired ? ["exact active tier, grade, stack, ramp, or threshold selector at event time"] : []),
      "observed output and counterfactual conservation replay before rDPS credit",
    ]),
    accepted_terms: accepted,
    rejected_terms: rejected,
    components: compiled,
    decoded_row_evidence: decodedRows,
  };
  source.evidence_sha256 = contentHash(source);
  return source;
}

function compileComponent(ownerEffectId, componentIndex, component) {
  const expectedPercent = (component.formulaTermIds ?? []).some(isPercentageFormulaTerm);
  const structuredTierProof = Boolean(component.proof_state && Array.isArray(component.tiers) && component.tiers.length > 0);
  const acceptedTerms = [];
  const rejectedTerms = [];
  for (const [tokenIndex, raw] of (component.valueTexts ?? []).entries()) {
    const parsed = parseValueToken(raw);
    const occurrence = { token_index: tokenIndex, raw_text: String(raw), ...parsed };
    if (expectedPercent && parsed.unit === "percent") acceptedTerms.push(occurrence);
    else if (!expectedPercent && parsed.unit !== "opaque") acceptedTerms.push(occurrence);
    else rejectedTerms.push({ ...occurrence, reason: expectedPercent ? "wrong-unit-for-percentage-formula" : "opaque-or-unsupported-unit" });
  }
  return {
    owner_effect_id: ownerEffectId,
    component_index: componentIndex,
    component_key: component.componentKey ?? null,
    label: component.label ?? null,
    direction: component.direction ?? null,
    formula_term_ids: component.formulaTermIds ?? [],
    expected_unit: expectedPercent ? "percent" : "typed-nonpercent-or-structured",
    structured_tier_proof: structuredTierProof ? {
      proof_state: component.proof_state,
      parameter_encoding: component.parameter_encoding ?? null,
      raw_units_per_decimal: component.raw_units_per_decimal ?? null,
      raw_units_per_percent: component.raw_units_per_percent ?? null,
      tiers: component.tiers,
    } : null,
    accepted_terms: acceptedTerms,
    rejected_terms: rejectedTerms,
  };
}

function parseValueToken(raw) {
  const text = String(raw).trim();
  const percent = text.match(/^([+-]?\d+(?:\.\d+)?)%$/);
  if (percent) {
    const value = Number(percent[1]);
    return { unit: "percent", percent_value: value, decimal_value: value / 100 };
  }
  const duration = text.match(/^([+-]?\d+(?:\.\d+)?)\s*s$/i);
  if (duration) return { unit: "seconds", seconds_value: Number(duration[1]) };
  if (/^[+-]?\d+(?:\.\d+)?$/.test(text)) return { unit: "flat", flat_value: Number(text) };
  return { unit: "opaque" };
}

function isPercentageFormulaTerm(term) {
  return /Pct$/.test(String(term)) || ["critMultiplier", "luckyDamagePct", "luckyChancePct"].includes(String(term));
}

function isSelectorBlocker(blocker) {
  const normalized = String(blocker).replace(/[-_:]+/g, " ").replace(/\s+/g, " ").trim();
  return /(selected factor grade|selector|required tier|value ladder|ramp|threshold)/i.test(normalized);
}

function blockerResolvedByTypedValues(blocker, magnitudeResolved, runtimeSelectorRequired) {
  if (!magnitudeResolved) return false;
  if (/no generated value proof/i.test(blocker)) return true;
  if (isSelectorBlocker(blocker) && runtimeSelectorRequired) return true;
  return false;
}

function summarize(sources) {
  const classifications = {};
  let sourceTokens = 0;
  let preservedTokens = 0;
  for (const source of sources) {
    classifications[source.classification] = (classifications[source.classification] ?? 0) + 1;
    for (const component of source.components) {
      sourceTokens += component.accepted_terms.length + component.rejected_terms.length;
      preservedTokens += component.accepted_terms.length + component.rejected_terms.length;
    }
  }
  return {
    sources: sources.length,
    formula_magnitudes_resolved: sources.filter((source) => source.formula_magnitude_resolved).length,
    static_gates_resolved: sources.filter((source) => source.static_gate_resolved).length,
    runtime_selectors_required: sources.filter((source) => source.runtime_selector_required).length,
    unresolved_magnitudes: sources.filter((source) => !source.formula_magnitude_resolved).length,
    classifications,
    source_value_tokens: sourceTokens,
    preserved_source_tokens: preservedTokens,
    hidden_source_tokens: sourceTokens - preservedTokens,
  };
}

function verify(input, indexPath = null) {
  const report = readJson(input, "static formula evidence");
  if (report.schema_version !== 1) throw new Error("Static formula evidence schema_version must be 1");
  if (report.generated_by !== "tools/bpsr-static-formula-evidence.mjs") throw new Error("Unexpected static formula evidence generator");
  const expectedHash = contentHash(report);
  if (report.content_sha256 !== expectedHash) throw new Error("Static formula evidence content hash mismatch");
  const ids = new Set();
  let tokens = 0;
  for (const source of report.sources ?? []) {
    if (ids.has(source.source_rule_id)) throw new Error(`Duplicate source rule ${source.source_rule_id}`);
    ids.add(source.source_rule_id);
    if (source.static_gate_resolved && !source.formula_magnitude_resolved) throw new Error(`${source.source_rule_id} closes its static gate without a decoded magnitude`);
    if (source.formula_magnitude_resolved && !source.components.some((component) => component.structured_tier_proof || component.accepted_terms.length > 0)) throw new Error(`${source.source_rule_id} resolves a magnitude without exact terms`);
    for (const component of source.components) {
      for (const term of [...component.accepted_terms, ...component.rejected_terms]) {
        tokens += 1;
        if (term.unit === "flat" && Object.hasOwn(term, "percent_value")) throw new Error(`${source.source_rule_id} converts a flat value to percent`);
      }
    }
  }
  if (report.summary.sources !== ids.size) throw new Error("Static formula evidence source count mismatch");
  if (report.summary.hidden_source_tokens !== 0 || report.summary.source_value_tokens !== tokens) throw new Error("Static formula evidence omitted source tokens");
  if (indexPath) requireFile(indexPath, "semantic evidence index");
  console.log(`Static formula evidence verified: ${ids.size} sources, ${tokens} value tokens, zero hidden omissions.`);
}

function selfTest() {
  const percent = parseValueToken("5.2%");
  if (percent.unit !== "percent" || percent.percent_value !== 5.2 || Math.abs(percent.decimal_value - 0.052) > Number.EPSILON) throw new Error("Percent parsing failed");
  const flat = parseValueToken("+520");
  if (flat.unit !== "flat" || Object.hasOwn(flat, "percent_value")) throw new Error("Flat parsing failed");
  const mixed = compileComponent(1, 0, { formulaTermIds: ["genericDamagePct"], valueTexts: ["+10", "+12%"] });
  if (mixed.accepted_terms.length !== 1 || mixed.accepted_terms[0].percent_value !== 12 || mixed.rejected_terms[0].unit !== "flat") throw new Error("Unit gating failed");
  const opaque = parseValueToken("depends on target HP");
  if (opaque.unit !== "opaque") throw new Error("Opaque preservation failed");
  if (!isSelectorBlocker("component:skill-specific-damage:value-ladder-selection-required:encounter-selected-factor-grade-required")) throw new Error("Hyphenated selector blocker normalization failed");
  if (!blockerResolvedByTypedValues("component:generic-damage:value-ladder-selection-required:encounter-selected-factor-grade-required", true, true)) throw new Error("Complete typed ladder did not close its static selector gate");
  console.log("Static formula evidence self-test passed.");
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return { path: path.resolve(file), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function readJson(file, label) {
  requireFile(file, label);
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Cannot parse ${label} ${file}: ${error.message}`); }
}

function requireBuild(value, build, field, label) {
  if (String(value[field]) !== build) throw new Error(`${label} ${field} ${value[field]} does not match ${build}`);
}

function requireFile(file, label) {
  if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`);
}

function unique(values) { return [...new Set(values)]; }

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-static-formula-evidence.mjs build --build <id> --formula-ledger <json> --index <sqlite> --output <json>\n  node tools/bpsr-static-formula-evidence.mjs verify --input <json> [--index <sqlite>]\n  node tools/bpsr-static-formula-evidence.mjs self-test");
  process.exit(exitCode);
}
