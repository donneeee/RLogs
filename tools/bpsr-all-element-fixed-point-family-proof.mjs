#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...args] = process.argv.slice(2);
const options = parseArgs(args);

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
    imagineProof: path.resolve(required(parsed, "imagine-proof")),
    runtimeConfig: path.resolve(required(parsed, "runtime-config")),
    packetFormulaSource: path.resolve(required(parsed, "packet-formula-source")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of [
    ["FightAttrTable", context.fightAttrTable],
    ["Imagine proof", context.imagineProof],
    ["rDPS runtime config", context.runtimeConfig],
    ["packet formula source", context.packetFormulaSource],
  ]) requireFile(file, label);

  const table = readJson(context.fightAttrTable, "FightAttrTable");
  const imagine = readJson(context.imagineProof, "Imagine proof");
  const runtime = readJson(context.runtimeConfig, "rDPS runtime config");
  const source = readFileSync(context.packetFormulaSource, "utf8");
  if (String(imagine.game_build) !== context.build) throw new Error("Imagine proof build mismatch");

  const component = (imagine.components ?? []).find(
    (entry) => entry.component_id === "fatal-spiral-shared-all-element-bonus",
  );
  if (!component) throw new Error("Fatal Spiral component is missing from Imagine proof");
  const highland = runtime.highland_blood;
  if (!highland) throw new Error("Highland/Fatal Spiral runtime configuration is missing");
  if (highland.runtime_transfer_enabled !== false || component.attribution_contract?.runtime_rdps_enabled !== false) {
    throw new Error("Fatal Spiral must remain disabled while its combat damage stage is open");
  }

  const familyIds = [13100, 13101, 13102, 13103, 13104, 13105];
  const familyRoot = findRow(table, familyIds[0]);
  const familyMembers = validateFamilyRoot(familyRoot, familyIds);
  validateRuntimeFamily(highland.all_element_family, familyIds);
  validateComponent(component, familyIds);
  validatePacketFormulaSource(source);

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-all-element-fixed-point-family-proof.mjs",
    game: "Blue Protocol: Star Resonance",
    game_build: context.build,
    proof_state: "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open",
    policy: {
      event_time_state_only: true,
      newer_profile_snapshots_forbidden: true,
      unresolved_events_remain_visible: true,
      proof_receipt_does_not_promote_rdps_attribution: true,
      runtime_transfer_enabled: false,
    },
    identity: {
      imagine_skill_id: component.imagine_skill_id,
      imagine_name: component.imagine_name,
      component_id: component.component_id,
      effect_id: component.effect_ids[0],
      provider_marker_effect_id: component.provider_marker_effect_ids[0],
      excluded_provider_owned_damage_ids: component.excluded_owner_damage_ids,
    },
    fixed_point_family: {
      denominator: 10000,
      current_attribute_id: familyRoot.Id,
      total_attribute_id: familyRoot.AttrTotal,
      add_attribute_id: familyRoot.AttrAdd,
      extra_add_attribute_id: familyRoot.AttrExAdd,
      percent_attribute_id: familyRoot.AttrPer,
      extra_percent_attribute_id: familyRoot.AttrExPer,
      numeric_type: familyRoot.AttrNumType,
      official_name: familyRoot.OfficialName,
      description: familyRoot.AttrDes,
      packet_equations: [
        "total = floor(add * (10000 + percent) / 10000)",
        "current = total + extra_add",
      ],
      provider_marginal_equation: "current_family_value - family_value_with_all_provider_components_removed_together",
      provider_cross_term_preserved: true,
      table_storage: "single-materialized-root-with-referenced-family-member-ids",
      materialized_root: projectRow(familyRoot),
      family_members: familyMembers,
    },
    provider_scalar: {
      equation: component.equation,
      tier_basis_points: component.tier_values.map((entry) => entry.total_basis_points),
      packet_attribute_oracle: component.packet_attribute_oracle,
      duration_millis: component.duration_millis,
      same_type_lockout_millis: component.same_type_lockout_millis,
    },
    proven_scope: {
      current_build_table_family_identity: true,
      fixed_point_units: true,
      packet_family_replay_equation: true,
      provider_tier_scalars: true,
      captured_provider_application_and_removal_delta: true,
      provider_owned_direct_damage_exclusion: true,
    },
    still_required_runtime_gates: [
      "combat-damage-stage-consumer",
      "affected-damage-property-coverage",
      "integer-damage-counterfactual-projection",
      "matching-window-conservation-replay",
    ],
    evidence: {
      fight_attr_table: descriptor(context.fightAttrTable),
      imagine_formula_proof: descriptor(context.imagineProof),
      rdps_runtime_config: descriptor(context.runtimeConfig),
      packet_formula_source: descriptor(context.packetFormulaSource),
    },
    summary: {
      family_members_proven: familyMembers.length,
      tier_scalars_proven: component.tier_values.length,
      packet_oracle_correlated_status_events: component.packet_attribute_oracle.correlated_status_events,
      runtime_gates_closed: 0,
      rdps_obligations_promoted: 0,
      hidden_omissions: 0,
    },
  };

  validateReport(report);
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`All-Element fixed-point family proof built for ${context.build}; damage-stage attribution remains disabled.`);
}

function verify(input) {
  requireFile(input, "proof report");
  const report = readJson(input, "proof report");
  validateReport(report);
  console.log(`All-Element fixed-point family proof verified for build ${report.game_build}; ${report.still_required_runtime_gates.length} runtime gates remain open.`);
}

function validateReport(report) {
  if (report.schema_version !== 1 || report.proof_state !== "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open") {
    throw new Error("Unexpected All-Element proof schema or state");
  }
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("Invalid report build");
  const policy = report.policy ?? {};
  if (!policy.event_time_state_only || !policy.newer_profile_snapshots_forbidden ||
      !policy.unresolved_events_remain_visible || !policy.proof_receipt_does_not_promote_rdps_attribution ||
      policy.runtime_transfer_enabled !== false) throw new Error("Unsafe All-Element proof policy");
  const family = report.fixed_point_family ?? {};
  if (family.denominator !== 10000 || family.current_attribute_id !== 13100 ||
      family.total_attribute_id !== 13101 || family.add_attribute_id !== 13102 ||
      family.extra_add_attribute_id !== 13103 || family.percent_attribute_id !== 13104 ||
      family.extra_percent_attribute_id !== 13105 || family.numeric_type !== 1 ||
      family.provider_cross_term_preserved !== true ||
      family.table_storage !== "single-materialized-root-with-referenced-family-member-ids" ||
      family.materialized_root?.id !== 13100 || family.family_members?.length !== 6) {
    throw new Error("All-Element family identity or units changed");
  }
  if (JSON.stringify(report.provider_scalar?.tier_basis_points) !== JSON.stringify([600, 700, 800, 900, 1000])) {
    throw new Error("Fatal Spiral tier scalar set changed");
  }
  const oracle = report.provider_scalar?.packet_attribute_oracle;
  if (oracle?.effect_id !== 2110125 || oracle?.applied_delta !== 1000 || oracle?.removed_delta !== -1000 ||
      oracle?.correlated_status_events < 1) throw new Error("Packet attribute oracle is incomplete");
  const requiredGates = [
    "combat-damage-stage-consumer",
    "affected-damage-property-coverage",
    "integer-damage-counterfactual-projection",
    "matching-window-conservation-replay",
  ];
  if (JSON.stringify(report.still_required_runtime_gates) !== JSON.stringify(requiredGates)) {
    throw new Error("All-Element proof must retain every damage-stage gate");
  }
  if (report.summary?.runtime_gates_closed !== 0 || report.summary?.rdps_obligations_promoted !== 0 ||
      report.summary?.hidden_omissions !== 0) throw new Error("Offline proof promoted or hid runtime evidence");
}

function validateFamilyRoot(root, familyIds) {
  const expected = [root.Id, root.AttrTotal, root.AttrAdd, root.AttrExAdd, root.AttrPer, root.AttrExPer];
  if (JSON.stringify(expected) !== JSON.stringify(familyIds)) {
    throw new Error("FightAttrTable All-Element family mapping changed");
  }
  if (root.AttrNumType !== 1 || root.Type !== "int32" || root.OfficialName !== "All-Element Bonus") {
    throw new Error("FightAttrTable All-Element units or identity changed");
  }
  const roles = ["current", "total", "add", "extra_add", "percent", "extra_percent"];
  return familyIds.map((id, index) => ({
    id,
    role: roles[index],
    materialized_table_row: index === 0,
    referenced_by_root_field: ["Id", "AttrTotal", "AttrAdd", "AttrExAdd", "AttrPer", "AttrExPer"][index],
  }));
}

function validateRuntimeFamily(family, ids) {
  const actual = [family?.current_attribute_id, family?.total_attribute_id, family?.add_attribute_id,
    family?.extra_add_attribute_id, family?.percent_attribute_id, family?.extra_percent_attribute_id];
  if (JSON.stringify(actual) !== JSON.stringify(ids)) throw new Error("Runtime All-Element family differs from current table");
}

function validateComponent(component, familyIds) {
  if (component.imagine_skill_id !== 3957 || component.effect_ids?.[0] !== 2110125 ||
      component.provider_marker_effect_ids?.[0] !== 2110124 || component.fixed_point_denominator !== 10000 ||
      component.exact_component_scalar_available !== true || component.matching_build_external_lifecycle_observed !== true) {
    throw new Error("Fatal Spiral proof identity or lifecycle changed");
  }
  if (JSON.stringify(component.packet_attribute_oracle?.attribute_ids) !== JSON.stringify(familyIds.slice(0, 3))) {
    throw new Error("Fatal Spiral packet oracle does not target the All-Element family");
  }
  if (JSON.stringify(component.tier_values?.map((entry) => entry.total_basis_points)) !== JSON.stringify([600, 700, 800, 900, 1000])) {
    throw new Error("Fatal Spiral tier formula changed");
  }
}

function validatePacketFormulaSource(source) {
  for (const fragment of [
    "total = floor(add * (10000 + percent) / 10000)",
    "current = total + extra_add",
    "pub fn packet_attribute_family_provider_marginal(",
    "pub fn packet_attribute_family_value(",
    "current_add.checked_sub(provider_add)?",
    "current_percent.checked_sub(provider_percent)?",
    "current_extra_add.checked_sub(provider_extra_add)?",
  ]) if (!source.includes(fragment)) throw new Error(`Packet formula source contract missing: ${fragment}`);
}

function findRow(table, id) {
  const row = Array.isArray(table) ? table.find((entry) => entry.Id === id) : table[String(id)];
  if (!row) throw new Error(`FightAttrTable row ${id} is missing`);
  return row;
}

function projectRow(row) {
  return {
    id: row.Id,
    enum_name: row.EnumName,
    type: row.Type,
    attr_num_type: row.AttrNumType,
    official_name: row.OfficialName,
  };
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return { path: path.relative(process.cwd(), file), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}

function selfTest() {
  const valid = {
    schema_version: 1,
    game_build: "24687926",
    proof_state: "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open",
    policy: { event_time_state_only: true, newer_profile_snapshots_forbidden: true, unresolved_events_remain_visible: true, proof_receipt_does_not_promote_rdps_attribution: true, runtime_transfer_enabled: false },
    fixed_point_family: { denominator: 10000, current_attribute_id: 13100, total_attribute_id: 13101, add_attribute_id: 13102, extra_add_attribute_id: 13103, percent_attribute_id: 13104, extra_percent_attribute_id: 13105, numeric_type: 1, provider_cross_term_preserved: true, table_storage: "single-materialized-root-with-referenced-family-member-ids", materialized_root: { id: 13100 }, family_members: Array.from({ length: 6 }, (_, index) => ({ id: 13100 + index })) },
    provider_scalar: { tier_basis_points: [600, 700, 800, 900, 1000], packet_attribute_oracle: { effect_id: 2110125, applied_delta: 1000, removed_delta: -1000, correlated_status_events: 120 } },
    still_required_runtime_gates: ["combat-damage-stage-consumer", "affected-damage-property-coverage", "integer-damage-counterfactual-projection", "matching-window-conservation-replay"],
    summary: { runtime_gates_closed: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
  };
  validateReport(valid);
  const unsafe = structuredClone(valid);
  unsafe.policy.runtime_transfer_enabled = true;
  let rejected = false;
  try { validateReport(unsafe); } catch { rejected = true; }
  if (!rejected) throw new Error("Self-test failed to reject premature runtime promotion");
  console.log("All-Element fixed-point family proof self-test passed.");
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    if (!key?.startsWith("--") || values[index + 1] === undefined) throw new Error(`Invalid argument ${key ?? "<missing>"}`);
    parsed[key.slice(2)] = values[index + 1];
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`${label} is not valid JSON: ${error.message}`); }
}

function requireFile(file, label) {
  if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`);
}

function usage(exitCode) {
  console.log("Usage: bpsr-all-element-fixed-point-family-proof.mjs build --build <id> --fight-attr-table <json> --imagine-proof <json> --runtime-config <json> --packet-formula-source <rs> --output <json> | verify --input <json> | self-test");
  process.exit(exitCode);
}
