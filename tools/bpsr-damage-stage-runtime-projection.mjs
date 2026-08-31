import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function readJson(file) {
  return JSON.parse(readFileSync(path.resolve(file), "utf8"));
}

function project(candidate) {
  assert.equal(candidate.generated_by, "rlogs-bpsr-damage-stage-runtime-catalog");
  assert.equal(candidate.promotion_state, "candidate-only-current-build-packet-replay-required");
  assert.equal(candidate.policy?.runtime_formula_authority, false);
  assert.equal(candidate.policy?.packet_replay_required, true);
  assert.equal(candidate.policy?.unresolved_events, "canonical damage is always retained and never hidden");
  assert.ok(Array.isArray(candidate.rules));
  assert.equal(candidate.rules.length, candidate.summary.standard_rules);

  const rules = candidate.rules.map((rule) => {
    assert.ok(Number.isSafeInteger(rule.ability_id) && rule.ability_id > 0);
    assert.ok(Number.isSafeInteger(rule.hit_event_id) && rule.hit_event_id >= 0);
    assert.ok(rule.damage_source === null || rule.damage_source === undefined || Number.isSafeInteger(rule.damage_source));
    assert.ok(Number.isSafeInteger(rule.damage_attr_id) && rule.damage_attr_id > 0);
    assert.ok(rule.damage_script === "Attack" || rule.damage_script === "MAttack");
    const projected = {
      ability_id: rule.ability_id,
      hit_event_id: rule.hit_event_id,
      damage_attr_id: rule.damage_attr_id,
      damage_script: rule.damage_script,
      coefficient_basis_points_by_stage: rule.coefficient_basis_points_by_stage,
      fixed_parameter_by_level: rule.fixed_parameter_by_level,
    };
    if (rule.damage_source !== null && rule.damage_source !== undefined) {
      projected.damage_source = rule.damage_source;
    }
    return projected;
  });
  const uniqueKeys = new Set(rules.map((rule) =>
    `${rule.ability_id}:${rule.hit_event_id}:${rule.damage_source ?? "*"}`));
  assert.equal(uniqueKeys.size, rules.length);

  return {
    schema_version: 1,
    game_build: candidate.game_build,
    generated_by: candidate.generated_by,
    source: {
      table: candidate.source.table,
      table_hash: candidate.source.table_hash,
      row_count: candidate.source.row_count,
    },
    policy: {
      lookup_key: candidate.policy.lookup_key,
      ambiguous_keys: candidate.policy.ambiguous_keys,
      nonstandard_scripts: candidate.policy.nonstandard_scripts,
      coefficient_selection: candidate.policy.coefficient_selection,
      fixed_parameter_selection: candidate.policy.fixed_parameter_selection,
      unresolved_events: candidate.policy.unresolved_events,
    },
    summary: {
      source_rows: candidate.summary.source_rows,
      unique_lookup_keys: candidate.summary.lookup_keys,
      ambiguous_lookup_keys: candidate.summary.multi_candidate_lookup_keys,
      standard_attack_rules: candidate.summary.standard_attack_rules,
      standard_magic_attack_rules: candidate.summary.standard_magic_attack_rules,
      standard_rules: candidate.summary.standard_rules,
    },
    rules,
  };
}

function validateRuntime(runtime, candidate) {
  assert.deepEqual(runtime, project(candidate));
  assert.equal(runtime.game_build, candidate.game_build);
  assert.equal(runtime.rules.length, runtime.summary.standard_rules);
  assert.equal(runtime.rules.filter((rule) => rule.damage_source !== undefined).length,
    candidate.summary.source_specific_rules);
}

function generate(options) {
  const candidate = readJson(required(options, "candidate"));
  const output = path.resolve(required(options, "output"));
  const runtime = project(candidate);
  validateRuntime(runtime, candidate);
  writeFileSync(output, `${JSON.stringify(runtime, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const candidate = readJson(required(options, "candidate"));
  const input = path.resolve(required(options, "input"));
  validateRuntime(readJson(input), candidate);
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-damage-stage-runtime-projection.mjs generate --candidate <json> --output <json>\n  node tools/bpsr-damage-stage-runtime-projection.mjs verify --candidate <json> --input <json>");
  process.exit(1);
}
