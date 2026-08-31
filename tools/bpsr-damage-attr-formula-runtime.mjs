import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-damage-attr-formula-runtime.mjs";

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

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    absolute,
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function exactField(layout, name, offset) {
  const field = layout.fields?.find((entry) => entry.field === name);
  assert.ok(field, `${name} field`);
  assert.equal(field.current_row_offset, offset, `${name} offset`);
  assert.equal(field.unreadable_current_rows, 0, `${name} unreadable rows`);
  assert.ok(field.compared_rows > 0, `${name} comparison coverage`);
  return field;
}

function currentRule(damageStage, templateRow) {
  const matches = damageStage.rules.filter((rule) =>
    rule.damage_attr_id === templateRow.damage_id &&
    rule.ability_id === templateRow.linked_source_id &&
    rule.hit_event_id === templateRow.hit_event_suffix);
  assert.equal(matches.length, 1, `unique current rule for damage ${templateRow.damage_id}`);
  return matches[0];
}

function buildRuntime(gameBuild, template, layoutSource, damageStageSource) {
  const layout = layoutSource.value;
  const damageStage = damageStageSource.value;

  assert.equal(template.schema_version, 1);
  assert.equal(layout.schema_version, 1);
  assert.equal(layout.policy?.runtime_formula_authority, false);
  assert.equal(layout.policy?.current_build_values_are_runtime_authority, true);
  assert.equal(layout.policy?.historical_reference_values_are_runtime_authority, false);
  assert.equal(layout.exact_layout_conclusion?.schema_fields_bound_without_guessing, 16);
  assert.equal(layout.exact_layout_conclusion?.nested_fields_intentionally_unbound, 2);
  assert.equal(damageStage.schema_version, 1);
  assert.equal(damageStage.game_build, gameBuild);
  assert.equal(damageStage.policy?.unresolved_events,
    "canonical damage is always retained and never hidden");
  assert.ok(Array.isArray(damageStage.rules) && damageStage.rules.length > 0);

  exactField(layout, "PVEDamageRadio", 28);
  exactField(layout, "PVEFixedParameter", 32);
  exactField(layout, "PVEStunnedDamage", 40);

  const verifiedRows = template.verified_rows.map((templateRow) => {
    const rule = currentRule(damageStage, templateRow);
    assert.equal(rule.damage_script, templateRow.damage_input);
    assert.deepEqual(rule.coefficient_basis_points_by_stage, templateRow.pve_damage_ratio);

    const fixed = rule.fixed_parameter_by_level;
    assert.ok(Array.isArray(fixed));
    if (templateRow.pve_fixed_parameter_count !== undefined) {
      assert.equal(fixed.length, templateRow.pve_fixed_parameter_count);
    }
    if (templateRow.observed_owner_level !== undefined) {
      assert.equal(fixed[templateRow.observed_owner_level - 1],
        templateRow.selected_pve_fixed_parameter_candidate);
    }

    return {
      ...templateRow,
      pve_damage_ratio: rule.coefficient_basis_points_by_stage,
      ...(fixed.length > 0 ? { pve_fixed_parameter_by_level: fixed } : {}),
    };
  });

  return {
    schema_version: 1,
    game_build: gameBuild,
    generated_by: GENERATED_BY,
    promotion_state: "current-build-field-identities-exact-final-integer-formula-blocked",
    policy: {
      runtime_formula_authority: false,
      provider_rdps_credit_allowed: false,
      canonical_damage_is_preserved: true,
      exact_integer_conservation_required_for_promotion: true,
      historical_values_are_runtime_authority: false,
    },
    current_build_evidence: {
      field_layout: receipt(layoutSource),
      damage_stage_catalog: receipt(damageStageSource),
    },
    source: {
      table: template.source.table,
      table_hash: damageStage.source.table_hash,
      decoded_surface_rows: damageStage.source.row_count,
      exact_layout_rows: layout.current_build_source.row_count,
      row_size: layout.exact_layout_conclusion.row_size_bytes,
      field_layout_proof: template.source.field_layout_proof,
    },
    coefficient_scale: template.coefficient_scale,
    selection_key: template.selection_key,
    field_semantics: template.field_semantics,
    calculation_modes: template.calculation_modes,
    verified_rows: verifiedRows,
  };
}

function loadInputs(options) {
  const gameBuild = required(options, "build");
  const templateSource = source(required(options, "template"));
  return {
    gameBuild,
    template: templateSource.value,
    layoutSource: source(required(options, "layout")),
    damageStageSource: source(required(options, "damage-stage")),
  };
}

function generate(options) {
  const inputs = loadInputs(options);
  const output = path.resolve(required(options, "output"));
  const runtime = buildRuntime(inputs.gameBuild, inputs.template,
    inputs.layoutSource, inputs.damageStageSource);
  writeFileSync(output, `${JSON.stringify(runtime, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const inputs = loadInputs(options);
  const input = path.resolve(required(options, "input"));
  const expected = buildRuntime(inputs.gameBuild, inputs.template,
    inputs.layoutSource, inputs.damageStageSource);
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), expected);
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-damage-attr-formula-runtime.mjs generate --build <id> --template <json> --layout <json> --damage-stage <json> --output <json>\n  node tools/bpsr-damage-attr-formula-runtime.mjs verify --build <id> --template <json> --layout <json> --damage-stage <json> --input <json>");
  process.exit(1);
}
