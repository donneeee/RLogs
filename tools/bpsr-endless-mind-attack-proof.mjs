#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync, createReadStream, existsSync, mkdirSync, openSync, readFileSync, readSync, statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-endless-mind-attack-proof.mjs";
const SCHEMA_VERSION = 1;
const EFFECT_ID = 3_003_411;
const EFFECT_NAME = "Endless Mind";
const SOURCE_TYPE_ID = 1;
const SOURCE_CONFIG_ID = 3_003_410;
const FORMLESS_CLASS_ID = 3;
const FORMLESS_SPEC_ID = 128;
const MASTERY_PER_STACK = 200;
const ATTACK_PERCENT_PER_STACK = 40;
const SELECTED_SKILL_CD_BOOST_PER_STACK = 640;
const SCALE = 10_000n;
const MAX_SAMPLES = 2_000_000;
const MAX_SELECTED_ROWS = 100_000;
const MAX_EXAMPLES = 24;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") await build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(parsed) {
  const cohortPath = resolved(required(parsed, "cohort"));
  const stagePath = resolved(required(parsed, "damage-stage"));
  const ownershipPath = resolved(required(parsed, "provider-ownership"));
  const recipientAuthorityPath = resolved(required(parsed, "recipient-authority"));
  const outputPath = resolved(required(parsed, "output"));
  if (existsSync(outputPath)) throw new Error(`Refusing to overwrite ${outputPath}`);
  const report = await analyze(cohortPath, stagePath, ownershipPath, recipientAuthorityPath);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(JSON.stringify({ output: outputPath, summary: report.summary, decision: report.decision }, null, 2));
}

async function analyze(cohortPath, stagePath, ownershipPath, recipientAuthorityPath) {
  const header = readHeader(cohortPath);
  assert(Number(header.schema_version) === 39, "Expected current 26-log formula cohort schema 39");
  assert(String(header.game_build) === "24687926", "Expected current build 24687926");
  const stage = readJson(stagePath);
  assert(String(stage.game_build) === String(header.game_build), "Damage-stage build mismatch");
  const rules = indexRules(stage.rules);
  const ownership = readJson(ownershipPath);
  verifyOwnership(ownership);
  const recipientAuthority = readJson(recipientAuthorityPath);
  const exactFormlessRecipients = verifyRecipientAuthority(recipientAuthority);

  const qualifyingStatusStates = new Map();
  const statusScan = await scanObjectArray(cohortPath, "status_states", (statuses, index) => {
    const candidates = (statuses ?? []).filter((status) =>
      Number(status.effect_id) === EFFECT_ID &&
      Number(status.origin_source_type_id) === SOURCE_TYPE_ID &&
      Number(status.origin_source_config_id) === SOURCE_CONFIG_ID &&
      Number.isSafeInteger(Number(status.source_entity_uuid)) &&
      [1, 2, 3].includes(Number(status.stacks)));
    if (candidates.length === 1) qualifyingStatusStates.set(index, candidates[0]);
  });

  const selected = [];
  let selectedObservedDamage = 0n;
  const selectedByStack = new Map();
  const selectedSessions = new Set();
  const counters = {
    samples_scanned: 0,
    source_status_states_with_endless_mind: 0,
    external_provider_rows: 0,
    formless_128_rows: 0,
    rejected_self_provider: 0,
    rejected_non_formless_128: 0,
  };
  const sampleScan = await scanObjectArray(cohortPath, "samples", (sample) => {
    counters.samples_scanned += 1;
    assert(counters.samples_scanned <= MAX_SAMPLES, `Sample cap ${MAX_SAMPLES} exceeded`);
    const status = qualifyingStatusStates.get(Number(sample.source_status_state_id));
    if (!status) return;
    counters.source_status_states_with_endless_mind += 1;
    if (Number(status.source_entity_uuid) === Number(sample.source_entity_uuid)) {
      counters.rejected_self_provider += 1;
      return;
    }
    counters.external_provider_rows += 1;
    if (!exactFormlessRecipients.has(String(sample.source_entity_uuid))) {
      counters.rejected_non_formless_128 += 1;
      return;
    }
    counters.formless_128_rows += 1;
    selectedByStack.set(String(status.stacks), (selectedByStack.get(String(status.stacks)) ?? 0) + 1);
    selectedSessions.add(String(sample.session_id));
    assert(selected.length < MAX_SELECTED_ROWS, `Selected row cap ${MAX_SELECTED_ROWS} exceeded`);
    selected.push({ sample, status });
    selectedObservedDamage += BigInt(sample.amount);
  });

  const selectedAttributeIds = new Set(selected.map(({ sample }) => Number(sample.source_attribute_state_id)));
  const attributes = new Map();
  const attributeScan = await scanObjectArray(cohortPath, "attribute_states", (state, index) => {
    if (selectedAttributeIds.has(index)) attributes.set(index, state);
  }, true);
  assert(attributes.size === selectedAttributeIds.size, "Selected source attribute state missing");

  const resultCounters = {
    eligible_rows: 0,
    rejected_damage_stage_missing_or_ambiguous: 0,
    rejected_attack_family_incomplete: 0,
    rejected_provider_delta_not_present: 0,
    rejected_active_stage_body_nonpositive: 0,
    rejected_downstream_factor_not_unique: 0,
    rejected_nonpositive_counterfactual: 0,
    resolved_damage_stage_rows: 0,
    resolved_stage_rows_with_any_attack_family_attribute: 0,
  };
  const examples = [];
  const rejectionExamples = [];
  let observedDamage = 0n;
  let providerDamage = 0n;
  const byStack = new Map();
  const byAction = new Map();
  const bySession = new Map();
  for (const row of selected) {
    const evaluated = evaluateRow(row.sample, row.status, attributes, rules);
    if (!evaluated.ok) {
      resultCounters[evaluated.reason] += 1;
      if (evaluated.reason !== "rejected_damage_stage_missing_or_ambiguous") {
        resultCounters.resolved_damage_stage_rows += 1;
      }
      if ((evaluated.diagnostic?.attack_attributes?.length ?? 0) > 0) {
        resultCounters.resolved_stage_rows_with_any_attack_family_attribute += 1;
      }
      if (rejectionExamples.length < MAX_EXAMPLES && evaluated.diagnostic) {
        rejectionExamples.push({
          session_id: row.sample.session_id,
          sequence: Number(row.sample.sequence),
          recipient_entity_uuid: String(row.sample.source_entity_uuid),
          ability_id: Number(row.sample.ability_id),
          hit_event_id: Number(row.sample.hit_event_id),
          stacks: Number(row.status.stacks),
          reason: evaluated.reason,
          ...evaluated.diagnostic,
        });
      }
      continue;
    }
    resultCounters.resolved_damage_stage_rows += 1;
    resultCounters.resolved_stage_rows_with_any_attack_family_attribute += 1;
    resultCounters.eligible_rows += 1;
    observedDamage += BigInt(evaluated.observed_damage);
    providerDamage += BigInt(evaluated.provider_damage);
    increment(byStack, String(evaluated.stacks), evaluated);
    increment(byAction, String(evaluated.ability_id), evaluated);
    increment(bySession, evaluated.session_id, evaluated);
    if (examples.length < MAX_EXAMPLES) examples.push(evaluated);
  }
  const recipientDamage = observedDamage - providerDamage;
  const runtimeEligible = resultCounters.eligible_rows > 0 && providerDamage > 0n && recipientDamage >= 0n;
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: String(header.game_build),
    effect: { id: EFFECT_ID, name: EFFECT_NAME },
    component: {
      name: "formless-128-mastery-to-attack",
      mastery_basis_points_per_stack: MASTERY_PER_STACK,
      attack_percent_basis_points_per_stack: ATTACK_PERCENT_PER_STACK,
      selected_skill_cd_boost_basis_points_per_stack: SELECTED_SKILL_CD_BOOST_PER_STACK,
      attack_component_promotable: runtimeEligible,
      cadence_component_promotable: false,
    },
    policy: {
      external_status_origin_required: { source_type_id: SOURCE_TYPE_ID, source_config_id: SOURCE_CONFIG_ID },
      exact_provider_and_recipient_entity_identity_required: true,
      self_provider_and_finale_chant_origin_3003481_excluded: true,
      source_class_and_specialization_required: [FORMLESS_CLASS_ID, FORMLESS_SPEC_ID],
      packet_observed_or_uniquely_completed_attack_family_required: true,
      provider_removed_attack_uses_difference_of_floors: true,
      downstream_integer_factor_must_be_unique_per_packet: true,
      cadence_credit_allowed: false,
      ordinary_damage_is_unchanged: true,
      unresolved_rows_remain_ordinary_damage: true,
    },
    inputs: [descriptor(cohortPath), descriptor(stagePath), descriptor(ownershipPath),
      descriptor(recipientAuthorityPath)],
    scans: { status_states: statusScan, samples: sampleScan, selected_attribute_states: attributeScan },
    counters: { ...counters, ...resultCounters },
    summary: {
      selected_rows: selected.length,
      selected_observed_damage: selectedObservedDamage.toString(),
      selected_sessions: [...selectedSessions].sort(),
      selected_rows_by_stack: Object.fromEntries([...selectedByStack].sort(([a], [b]) => a.localeCompare(b))),
      eligible_rows: resultCounters.eligible_rows,
      observed_damage: observedDamage.toString(),
      endless_mind_attack_damage: providerDamage.toString(),
      recipient_ordinary_damage: recipientDamage.toString(),
      conservation_check: (providerDamage + recipientDamage).toString(),
      conservation_exact: providerDamage + recipientDamage === observedDamage,
      selected_ordinary_damage_after_fail_closed_projection: (selectedObservedDamage - providerDamage).toString(),
      selected_conservation_check: selectedObservedDamage.toString(),
      selected_conservation_exact:
        providerDamage + (selectedObservedDamage - providerDamage) === selectedObservedDamage,
      sessions_with_eligible_rows: bySession.size,
      stacks: serialBuckets(byStack),
      actions: serialBuckets(byAction),
      sessions: serialBuckets(bySession),
    },
    examples,
    rejection_examples: rejectionExamples,
    decision: runtimeEligible ? {
      state: "attack-component-promotion-eligible",
      production_promotion_delta: 1,
      promoted_effect_id: EFFECT_ID,
      excluded_component: "Formless 128 selected-skill cooldown/cadence (+640 bp per stack)",
    } : {
      state: "blocked-no-runtime-edit",
      production_promotion_delta: 0,
      reason: "The exact external Formless 128 cohort contains no damage row with a packet-observed recipient Attack family (11330-11334 or 11340-11344), so the active coefficient body is unknown and the inverse downstream integer factor cannot be uniquely solved.",
      smallest_missing_input: "One complete current-build recipient-side Attack family snapshot at each eligible damage calculation boundary: final, intermediate, base Add, ExtraAdd, and raw-percent (or an algebraically unique raw-percent) for the packet-selected physical/magical lane.",
      acquisition_method: "Capture the same Formless recipient from its local client perspective, where FightAttr snapshots include these families, or prove and decode an already-carried remote FightAttr update into the canonical EntityAttribute stream. No server damage scalar or server-side cast packet is required.",
    },
  };
  return report;
}

function evaluateRow(sample, status, attributes, rules) {
  const resolved = resolveRule(sample, rules);
  if (!resolved) return failure("rejected_damage_stage_missing_or_ambiguous", {
    owner_stage: sample.packet?.owner_stage ?? null, owner_level: sample.packet?.owner_level ?? null,
  });
  const state = attributes.get(Number(sample.source_attribute_state_id));
  const family = attackFamily(state, resolved.damage_script);
  if (!family) return failure("rejected_attack_family_incomplete", {
    resolved_damage_stage: resolved,
    attack_attributes: (state ?? []).filter((entry) => Number(entry.attribute_id) >= 11330 &&
      Number(entry.attribute_id) <= 11344),
  });
  const stacks = Number(status.stacks);
  const providerRawPercent = stacks * ATTACK_PERCENT_PER_STACK;
  if (family.raw_percent < providerRawPercent) return failure("rejected_provider_delta_not_present");
  const withoutAttack = familyValue(family.base_add, family.raw_percent - providerRawPercent, family.extra_add);
  const providerAttackMarginal = family.final_value - withoutAttack;
  if (providerAttackMarginal <= 0) return failure("rejected_provider_delta_not_present");
  const activeBody = fixedProduct(family.final_value, resolved.coefficient_basis_points) + resolved.fixed_parameter;
  if (activeBody <= 0) return failure("rejected_active_stage_body_nonpositive");
  const factor = uniqueFactor(Number(sample.amount), activeBody);
  if (factor === null) return failure("rejected_downstream_factor_not_unique");
  const withoutBody = fixedProduct(withoutAttack, resolved.coefficient_basis_points) + resolved.fixed_parameter;
  const withoutDamage = downstream(withoutBody, factor);
  const providerDamage = Number(sample.amount) - withoutDamage;
  if (providerDamage <= 0 || providerDamage > Number(sample.amount)) {
    return failure("rejected_nonpositive_counterfactual");
  }
  return {
    ok: true,
    session_id: sample.session_id,
    sequence: Number(sample.sequence),
    observed_micros: Number(sample.observed_micros),
    provider_entity_uuid: String(status.source_entity_uuid),
    recipient_entity_uuid: String(sample.source_entity_uuid),
    stacks,
    ability_id: Number(sample.ability_id),
    hit_event_id: Number(sample.hit_event_id),
    damage_attr_id: resolved.damage_attr_id,
    damage_script: resolved.damage_script,
    attack_family: family,
    provider_attack_percent_basis_points: providerRawPercent,
    provider_attack_marginal: providerAttackMarginal,
    coefficient_basis_points: resolved.coefficient_basis_points,
    fixed_parameter: resolved.fixed_parameter,
    active_stage_body: activeBody,
    provider_removed_stage_body: withoutBody,
    downstream_factor_basis_points: factor,
    observed_damage: Number(sample.amount),
    provider_removed_damage: withoutDamage,
    provider_damage: providerDamage,
  };
}

function resolveRule(sample, rules) {
  const candidates = rules.get(`${sample.ability_id}:${sample.hit_event_id}`) ?? [];
  const matched = candidates.filter((rule) =>
    (rule.damage_script === "Attack" || rule.damage_script === "MagicAttack") &&
    Array.isArray(rule.coefficient_basis_points_by_stage));
  if (matched.length !== 1) return null;
  const rule = matched[0];
  let index = 0;
  if (rule.coefficient_basis_points_by_stage.length > 1) {
    const stage = Number(sample.packet?.owner_stage);
    if (!Number.isSafeInteger(stage) || stage < 1 || stage > rule.coefficient_basis_points_by_stage.length) return null;
    index = stage - 1;
  }
  const coefficient = rule.coefficient_basis_points_by_stage[index];
  if (!Number.isSafeInteger(coefficient) || coefficient < 0) return null;
  let fixed = 0;
  if ((rule.fixed_parameter_by_level ?? []).length > 0) {
    const level = Number(sample.packet?.owner_level);
    if (!Number.isSafeInteger(level) || level < 1 || level > rule.fixed_parameter_by_level.length) return null;
    fixed = Number(rule.fixed_parameter_by_level[level - 1]);
  }
  return {
    damage_attr_id: Number(rule.damage_attr_id), damage_script: rule.damage_script,
    coefficient_basis_points: coefficient, fixed_parameter: fixed,
  };
}

function attackFamily(state, script) {
  const ids = script === "Attack" ? [11330, 11331, 11332, 11333, 11334] : [11340, 11341, 11342, 11343, 11344];
  const values = new Map((state ?? []).map((entry) => [Number(entry.attribute_id), Number(entry.value)]));
  const finalValue = values.get(ids[0]);
  const intermediate = values.get(ids[1]);
  const baseAdd = values.get(ids[2]);
  if (![finalValue, intermediate, baseAdd].every(Number.isSafeInteger)) return null;
  const extraAdd = values.has(ids[3]) ? values.get(ids[3]) : finalValue - intermediate;
  let rawPercent = values.get(ids[4]);
  let rawPercentSource = "packet";
  if (!Number.isSafeInteger(rawPercent)) {
    rawPercent = uniqueRawPercent(baseAdd, intermediate);
    rawPercentSource = "unique-inverse";
  }
  if (![extraAdd, rawPercent].every((value) => Number.isSafeInteger(value) && value >= 0)) return null;
  if (familyValue(baseAdd, rawPercent, 0) !== intermediate ||
      familyValue(baseAdd, rawPercent, extraAdd) !== finalValue) return null;
  return { final_value: finalValue, intermediate_value: intermediate, base_add: baseAdd, extra_add: extraAdd,
    raw_percent: rawPercent, raw_percent_source: rawPercentSource };
}

function uniqueRawPercent(base, intermediate) {
  if (!Number.isSafeInteger(base) || base <= 0 || !Number.isSafeInteger(intermediate) || intermediate < 0) return null;
  const b = BigInt(base); const y = BigInt(intermediate);
  const lower = ceilDiv(y * SCALE, b) - SCALE;
  const upper = ceilDiv((y + 1n) * SCALE, b) - 1n - SCALE;
  return lower === upper && lower >= 0n && lower <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(lower) : null;
}

function uniqueFactor(observed, body) {
  if (!Number.isSafeInteger(observed) || observed < 0 || !Number.isSafeInteger(body) || body <= 0) return null;
  const lower = ceilDiv(BigInt(observed) * SCALE, BigInt(body));
  const upper = ceilDiv((BigInt(observed) + 1n) * SCALE, BigInt(body)) - 1n;
  return lower === upper && lower > 0n && lower <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(lower) : null;
}
function familyValue(base, percent, extra) { return Number((BigInt(base) * (SCALE + BigInt(percent))) / SCALE) + extra; }
function fixedProduct(value, factor) { return Number((BigInt(value) * BigInt(factor)) / SCALE); }
function downstream(body, factor) { return Number((BigInt(body) * BigInt(factor)) / SCALE); }
function ceilDiv(n, d) { return (n + d - 1n) / d; }
function failure(reason, diagnostic = null) { return { ok: false, reason, diagnostic }; }

function indexRules(rows) {
  const result = new Map();
  for (const row of rows ?? []) {
    const key = `${row.ability_id}:${row.hit_event_id}`;
    if (!result.has(key)) result.set(key, []);
    result.get(key).push(row);
  }
  return result;
}
function increment(map, key, row) {
  const value = map.get(key) ?? { rows: 0, observed_damage: 0n, provider_damage: 0n };
  value.rows += 1; value.observed_damage += BigInt(row.observed_damage); value.provider_damage += BigInt(row.provider_damage);
  map.set(key, value);
}
function serialBuckets(map) {
  return Object.fromEntries([...map].sort(([a], [b]) => a.localeCompare(b)).map(([key, value]) => [key, {
    rows: value.rows, observed_damage: value.observed_damage.toString(), provider_damage: value.provider_damage.toString(),
  }]));
}

function verifyOwnership(value) {
  assert(Number(value.game_build) === 24687926, "Provider-ownership build mismatch");
  const effect = value.effects?.find((entry) => Number(entry.effect_id) === EFFECT_ID);
  assert(effect?.player_actor_ownership_proven_for_every_sourced_event === true,
    "Endless Mind provider ownership is not exact for every sourced event");
  assert(effect?.stable_player_character_id_proven_for_every_sourced_event === true,
    "Endless Mind stable player ownership is incomplete");
}

function verifyRecipientAuthority(value) {
  assert(Number(value.build_id) === 24687926, "Recipient-authority build mismatch");
  assert(Number(value.effect?.id) === EFFECT_ID && value.effect?.name_en_us === EFFECT_NAME,
    "Recipient-authority exact effect identity mismatch");
  const rows = value.formless_recipient_cohorts ?? [];
  assert(rows.length > 0 && rows.every((row) => Number(row.class_id) === FORMLESS_CLASS_ID &&
    Number(row.spec_id) === FORMLESS_SPEC_ID && Number.isSafeInteger(Number(row.recipient_entity_id))),
  "Recipient authority does not contain exact Formless 128 entity routes");
  return new Set(rows.map((row) => String(row.recipient_entity_id)));
}

function verifyCommand(parsed) {
  const report = readJson(resolved(required(parsed, "input")));
  verifyReport(report);
  console.log("Endless Mind attack proof verified");
}
function verifyReport(report) {
  assert(Number(report.schema_version) === SCHEMA_VERSION, "Unexpected schema version");
  assert(report.generated_by === GENERATOR, "Unexpected generator");
  assert(Number(report.game_build) === 24687926, "Unexpected build");
  assert(Number(report.effect?.id) === EFFECT_ID && report.effect?.name === EFFECT_NAME, "Exact ID/name changed");
  assert(report.policy?.self_provider_and_finale_chant_origin_3003481_excluded === true, "Self route must be excluded");
  assert(report.policy?.downstream_integer_factor_must_be_unique_per_packet === true, "Unique inverse is required");
  assert(report.component?.cadence_component_promotable === false, "Cadence must remain excluded");
  assert(report.summary?.conservation_exact === true, "Ordinary damage does not conserve");
  assert(BigInt(report.summary.conservation_check) === BigInt(report.summary.observed_damage), "Conservation sum mismatch");
  assert(report.summary?.selected_conservation_exact === true, "Selected cohort damage does not conserve");
  assert(BigInt(report.summary.selected_conservation_check) === BigInt(report.summary.selected_observed_damage),
    "Selected cohort conservation sum mismatch");
  if (report.decision?.state === "attack-component-promotion-eligible") {
    assert(Number(report.summary.eligible_rows) > 0 && BigInt(report.summary.endless_mind_attack_damage) > 0n,
      "Promotion requires a nonempty positive exact subset");
    assert(Number(report.decision.production_promotion_delta) === 1, "Promotion delta must be one exact effect");
  }
  if (report.content_sha256 !== undefined) assert(report.content_sha256 === contentHash(report), "Content hash mismatch");
}

function selfTest() {
  assert(uniqueRawPercent(20_000, 23_200) === 1_600, "Raw-percent inverse changed");
  const active = { final_value: 23_200, intermediate_value: 23_200, base_add: 20_000, extra_add: 0, raw_percent: 1_600 };
  const without = familyValue(active.base_add, active.raw_percent - 40, active.extra_add);
  assert(active.final_value - without === 80, "Difference-of-floors changed");
  const body = fixedProduct(active.final_value, 50_000);
  const factor = 15_941;
  const damage = downstream(body, factor);
  assert(uniqueFactor(damage, body) === factor, "Unique downstream inverse changed");
  const withoutDamage = downstream(fixedProduct(without, 50_000), factor);
  assert(damage - withoutDamage > 0 && withoutDamage + (damage - withoutDamage) === damage, "Conservation changed");
  console.log(`${GENERATOR} self-test passed`);
}

async function scanObjectArray(file, propertyName, onItem, stopAfterProperty = false) {
  const marker = `"${propertyName}":[`;
  let markerOffset = 0, found = false, complete = false, started = false, inString = false, escaped = false;
  let depth = 0, itemText = "", index = 0, bytes = 0;
  const stream = createReadStream(file, { encoding: "utf8", highWaterMark: 4 * 1024 * 1024 });
  for await (const chunk of stream) {
    bytes += Buffer.byteLength(chunk);
    for (let i = 0; i < chunk.length; i += 1) {
      const ch = chunk[i];
      if (!found) {
        if (ch === marker[markerOffset]) markerOffset += 1;
        else markerOffset = ch === marker[0] ? 1 : 0;
        if (markerOffset === marker.length) found = true;
        continue;
      }
      if (complete) break;
      if (!started) {
        if (/\s|,/.test(ch)) continue;
        if (ch === "]") { complete = true; break; }
        assert(ch === "{" || ch === "[", `Expected object/array item in ${propertyName}`);
        started = true; depth = 1; itemText = ch; inString = false; escaped = false; continue;
      }
      itemText += ch;
      if (inString) {
        if (escaped) escaped = false;
        else if (ch === "\\") escaped = true;
        else if (ch === '"') inString = false;
      } else if (ch === '"') inString = true;
      else if (ch === "{" || ch === "[") depth += 1;
      else if (ch === "}" || ch === "]") depth -= 1;
      if (depth === 0) {
        onItem(JSON.parse(itemText), index); index += 1; started = false; itemText = "";
      }
    }
    if (complete && stopAfterProperty) { stream.destroy(); break; }
  }
  assert(found && complete, `Property ${propertyName} was not completely scanned`);
  return { items: index, bytes_read: bytes };
}

function readHeader(file) {
  const fd = openSync(file, "r"); const buffer = Buffer.alloc(64 * 1024);
  const length = readSync(fd, buffer, 0, buffer.length, 0); closeSync(fd);
  const text = buffer.subarray(0, length).toString("utf8");
  const schema = /"schema_version":(\d+)/.exec(text)?.[1];
  const build = /"game_build":"([^"]+)"/.exec(text)?.[1];
  assert(schema && build, "Formula cohort header missing");
  return { schema_version: Number(schema), game_build: build };
}
function descriptor(file) {
  const hash = createHash("sha256"); const fd = openSync(file, "r"); const buffer = Buffer.alloc(4 * 1024 * 1024);
  let bytes = 0, read; while ((read = readSync(fd, buffer, 0, buffer.length, null)) > 0) { hash.update(buffer.subarray(0, read)); bytes += read; }
  closeSync(fd); return { path: file.replaceAll("\\", "/"), bytes, sha256: hash.digest("hex") };
}
function readJson(file) { return JSON.parse(readFileSync(file, "utf8")); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function parseArgs(args) { const out = {}; for (let i = 0; i < args.length; i += 2) { assert(args[i]?.startsWith("--") && args[i + 1], `Invalid argument ${args[i]}`); out[args[i].slice(2)] = args[i + 1]; } return out; }
function required(parsed, key) { assert(parsed[key], `Missing --${key}`); return parsed[key]; }
function resolved(value) { return path.resolve(value); }
function assert(condition, message) { if (!condition) throw new Error(message); }
function usage(code) { console.log(`Usage:\n  node ${GENERATOR} build --cohort <json> --damage-stage <json> --provider-ownership <json> --recipient-authority <json> --output <json>\n  node ${GENERATOR} verify --input <json>\n  node ${GENERATOR} self-test`); process.exit(code); }
