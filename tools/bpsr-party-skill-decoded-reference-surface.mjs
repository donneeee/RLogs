#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-party-skill-decoded-reference-surface.mjs";
const MAX_TABLE_BYTES = 32 * 1024 * 1024;
const MAX_TABLE_SET_BYTES = 256 * 1024 * 1024;
const TARGET_SKILL_ID = 2209;
const TARGET_SKILL_EFFECT_ID = 220901;
const TARGET_STATUS_ID = 55228;
const TARGET_DAMAGE_ACTION_ID = 2203291;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(args) {
  const buildId = numericString(required(args, "build"), "build");
  const partyClosurePath = path.resolve(required(args, "party-closure"));
  const tablesRoot = path.resolve(required(args, "tables-root"));
  const output = path.resolve(required(args, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  requireFile(partyClosurePath, "party-skill static closure");
  requireDirectory(tablesRoot, "decoded table root");

  const partyClosure = readJson(partyClosurePath, "party-skill static closure");
  validatePartyClosure(partyClosure, buildId);
  const identifierSelection = selectIdentifiers(partyClosure);
  const scan = scanTables(tablesRoot, identifierSelection.all_ids);
  const findings = buildTargetFindings(partyClosure, scan.documents, scan.occurrences);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: buildId,
    proof_state:
      "all-current-decoded-tables-exact-id-surface-enumerated-typed-party-links-separated-from-collisions",
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_and_descriptions_are_evidence_only: true,
      table_and_field_identity_required_for_typed_relationships: true,
      equal_numbers_in_unrelated_tables_are_relationships: false,
      reviewed_candidate_links_are_exact_edges: false,
      absent_exact_id_reference_proves_indirect_or_server_formula_absence: false,
      exact_id_linked_scalar_absence_is_preserved: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      unresolved_source_edges_and_formula_state_are_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    bounded_processing: {
      maximum_table_bytes: MAX_TABLE_BYTES,
      maximum_table_set_bytes: MAX_TABLE_SET_BYTES,
      files_processed_one_at_a_time: true,
      raw_rlogs_read: false,
      whole_rlog_cohort_deserialized: false,
    },
    inputs: {
      party_skill_static_closure: descriptor(partyClosurePath),
      decoded_tables: {
        path: slash(tablesRoot),
        file_count: scan.inventory.length,
        total_bytes: scan.totalBytes,
        largest_file_bytes: scan.largestFileBytes,
        inventory_sha256: sha256Text(stableStringify(scan.inventory)),
        files: scan.inventory,
      },
    },
    identifier_selection: identifierSelection,
    occurrence_counts_by_id: countOccurrences(scan.occurrences, identifierSelection.all_ids),
    occurrences: scan.occurrences,
    target_findings: findings,
    summary: {
      decoded_table_files_scanned: scan.inventory.length,
      decoded_table_bytes_scanned: scan.totalBytes,
      authoritative_identifiers_scanned: identifierSelection.all_ids.length,
      exact_occurrences_retained: scan.occurrences.length,
      luminary_bolt_skill_to_skill_effect_edge_proven: true,
      luminary_bolt_skill_to_status_55228_edge_proven: false,
      status_55228_exact_id_linked_scalar_candidate_found: false,
      status_55228_current_build_formula_authority: false,
      hidden_occurrences: 0,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(
    `Party-skill decoded reference surface built for ${buildId}: ` +
      `${scan.inventory.length} tables, ${scan.occurrences.length} retained exact-ID occurrences; ` +
      "55228 scalar authority remains open.",
  );
}

function verify(input) {
  requireFile(input, "party-skill decoded reference surface");
  const report = readJson(input, "party-skill decoded reference surface");
  validateReport(report);
  const closurePath = path.resolve(report.inputs.party_skill_static_closure.path);
  const tablesRoot = path.resolve(report.inputs.decoded_tables.path);
  const closureDescriptor = descriptor(closurePath);
  assert(
    stableStringify(closureDescriptor) ===
      stableStringify(report.inputs.party_skill_static_closure),
    "Party-skill closure input changed",
  );
  const partyClosure = readJson(closurePath, "party-skill static closure");
  validatePartyClosure(partyClosure, String(report.game_build));
  const identifiers = selectIdentifiers(partyClosure);
  assert(
    stableStringify(identifiers) === stableStringify(report.identifier_selection),
    "Authoritative identifier selection changed",
  );
  const scan = scanTables(tablesRoot, identifiers.all_ids);
  const expectedTables = {
    path: slash(tablesRoot),
    file_count: scan.inventory.length,
    total_bytes: scan.totalBytes,
    largest_file_bytes: scan.largestFileBytes,
    inventory_sha256: sha256Text(stableStringify(scan.inventory)),
    files: scan.inventory,
  };
  assert(
    stableStringify(expectedTables) === stableStringify(report.inputs.decoded_tables),
    "Decoded-table inventory changed",
  );
  assert(
    stableStringify(scan.occurrences) === stableStringify(report.occurrences),
    "Exact-ID occurrence surface does not reproduce",
  );
  assert(
    stableStringify(buildTargetFindings(partyClosure, scan.documents, scan.occurrences)) ===
      stableStringify(report.target_findings),
    "Typed target findings do not reproduce",
  );
  console.log(
    `Party-skill decoded reference surface verified for ${report.game_build}: ` +
      `${scan.inventory.length} tables, no hidden exact-ID occurrences, no 55228 formula authority.`,
  );
}

function validatePartyClosure(closure, buildId) {
  assert(closure?.schema_version === 2, "Party-skill closure schema must be 2");
  assert(
    closure?.generated_by === "tools/bpsr-party-skill-static-closure.mjs",
    "Party-skill closure generator mismatch",
  );
  assert(String(closure?.game_build) === buildId, "Party-skill closure build mismatch");
  assert(closure?.policy?.exact_numeric_skill_effect_buff_ids_and_build_are_authoritative === true,
    "Party-skill closure lost exact-ID authority");
  assert(closure?.policy?.reviewed_candidate_links_are_exact_runtime_edges === false,
    "Reviewed candidate links were promoted");
  assert(closure?.runtime_decision?.runtime_catalog_promotion_allowed === false,
    "Party-skill closure improperly granted runtime promotion");
  assert(closure?.runtime_decision?.provider_rdps_credit_allowed === false,
    "Party-skill closure improperly granted provider credit");
}

function selectIdentifiers(closure) {
  const skills = closure.skill_candidates
    .filter((row) => row.rdps_relevant_candidate === true)
    .map((row) => Number(row.skill_id));
  const skillEffects = closure.skill_candidates
    .filter((row) => row.rdps_relevant_candidate === true)
    .flatMap((row) => row.skill_effect_ids ?? [])
    .map(Number);
  const buffs = closure.buff_candidates
    .filter((row) => row.rdps_relevant_candidate === true)
    .map((row) => Number(row.buff_id));
  for (const skill of closure.skill_candidates.filter((row) => row.rdps_relevant_candidate === true)) {
    for (const edge of skill.exact_skill_to_buff_edges ?? []) buffs.push(Number(edge.buff_id));
    for (const edge of skill.reviewed_candidate_skill_to_buff_links ?? []) {
      buffs.push(Number(edge.buff_id));
    }
  }
  for (const entry of closure.rogue_party_entry_candidates
    .filter((row) => row.rdps_relevant_candidate === true)) {
    buffs.push(Number(entry.exact_root_buff_id));
    for (const child of entry.candidate_child_buff_family ?? []) buffs.push(Number(child.buff_id));
  }
  const result = {
    rdps_relevant_skill_ids: sortedUniqueNumbers(skills),
    rdps_relevant_skill_effect_ids: sortedUniqueNumbers(skillEffects),
    rdps_relevant_or_linked_buff_ids: sortedUniqueNumbers(buffs),
  };
  result.all_ids = sortedUniqueNumbers([
    ...result.rdps_relevant_skill_ids,
    ...result.rdps_relevant_skill_effect_ids,
    ...result.rdps_relevant_or_linked_buff_ids,
    TARGET_DAMAGE_ACTION_ID,
  ]);
  assert(result.rdps_relevant_skill_ids.includes(TARGET_SKILL_ID), "Skill 2209 is missing");
  assert(result.rdps_relevant_skill_effect_ids.includes(TARGET_SKILL_EFFECT_ID),
    "Skill effect 220901 is missing");
  assert(result.rdps_relevant_or_linked_buff_ids.includes(TARGET_STATUS_ID),
    "Status 55228 is missing");
  return result;
}

function scanTables(root, ids) {
  const idStrings = new Set(ids.map(String));
  const names = readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();
  assert(names.length > 0, "Decoded table root has no JSON files");
  const inventory = [];
  const occurrences = [];
  const documents = new Map();
  let totalBytes = 0;
  let largestFileBytes = 0;
  for (const name of names) {
    const file = path.join(root, name);
    const bytes = readFileSync(file);
    assert(bytes.length <= MAX_TABLE_BYTES, `Decoded table exceeds bounded file limit: ${name}`);
    totalBytes += bytes.length;
    assert(totalBytes <= MAX_TABLE_SET_BYTES, "Decoded table set exceeds bounded total limit");
    largestFileBytes = Math.max(largestFileBytes, bytes.length);
    inventory.push({
      relative_path: name,
      bytes: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
    const document = JSON.parse(bytes.toString("utf8"));
    if ([
      "BuffTable.json",
      "ProfessionSystemTable.json",
      "SkillEffectTable.json",
      "SkillFightLevelTable.json",
      "SkillTable.json",
      "WeaponStarTable.json",
    ].includes(name)) documents.set(name, document);
    collectOccurrences(document, [], name, idStrings, occurrences);
  }
  occurrences.sort((left, right) =>
    left.id - right.id || left.relative_path.localeCompare(right.relative_path) ||
    left.pointer.localeCompare(right.pointer) || left.representation.localeCompare(right.representation));
  return { inventory, occurrences, documents, totalBytes, largestFileBytes };
}

function collectOccurrences(value, pointer, relativePath, ids, output) {
  if (Array.isArray(value)) {
    value.forEach((entry, index) =>
      collectOccurrences(entry, [...pointer, String(index)], relativePath, ids, output));
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, entry] of Object.entries(value)) {
      if (ids.has(key)) {
        output.push({
          id: Number(key),
          relative_path: relativePath,
          pointer: jsonPointer([...pointer, key]),
          representation: "object-key",
        });
      }
      collectOccurrences(entry, [...pointer, key], relativePath, ids, output);
    }
    return;
  }
  if ((typeof value === "number" || typeof value === "string") && ids.has(String(value))) {
    output.push({
      id: Number(value),
      relative_path: relativePath,
      pointer: jsonPointer(pointer),
      representation: typeof value,
    });
  }
}

function buildTargetFindings(closure, documents, occurrences) {
  const skillTable = requiredDocument(documents, "SkillTable.json");
  const effectTable = requiredDocument(documents, "SkillEffectTable.json");
  const fightLevelTable = requiredDocument(documents, "SkillFightLevelTable.json");
  const buffTable = requiredDocument(documents, "BuffTable.json");
  const weaponStarTable = requiredDocument(documents, "WeaponStarTable.json");
  const professionTable = requiredDocument(documents, "ProfessionSystemTable.json");
  const skill = skillTable[String(TARGET_SKILL_ID)];
  const effect = effectTable[String(TARGET_SKILL_EFFECT_ID)];
  const status = buffTable[String(TARGET_STATUS_ID)];
  assert(Number(skill?.Id) === TARGET_SKILL_ID, "SkillTable row 2209 is missing");
  assertExactArray(skill.EffectIDs, [TARGET_SKILL_EFFECT_ID], "Skill 2209 effect IDs");
  assert(Number(effect?.Id) === TARGET_SKILL_EFFECT_ID && Number(effect.SkillId) === TARGET_SKILL_ID,
    "SkillEffect 220901 does not bind skill 2209");
  assert(Number(status?.Id) === TARGET_STATUS_ID, "BuffTable row 55228 is missing");
  const closureSkill = only(
    closure.skill_candidates.filter((row) => Number(row.skill_id) === TARGET_SKILL_ID),
    "party-skill closure row 2209",
  );
  const reviewedStatus = only(
    closureSkill.reviewed_candidate_skill_to_buff_links
      .filter((row) => Number(row.buff_id) === TARGET_STATUS_ID),
    "reviewed skill 2209 to status 55228 link",
  );
  assert(reviewedStatus.exact_skill_to_buff_edge_proven === false,
    "Reviewed 2209 to 55228 link was promoted");
  assert(
    reviewedStatus.expected_buff_row_sha256 === sha256Text(stableStringify(status)),
    "Reviewed 55228 row digest mismatch",
  );
  const attrRows = effect.SkillAttrDes ?? [];
  const vulnerabilityRows = attrRows.filter((row) =>
    String(row?.[0] ?? "").toLowerCase() === "vulnerability");
  const durationRows = attrRows.filter((row) => String(row?.[0] ?? "").toLowerCase() === "duration");
  assert(vulnerabilityRows.length === 1 && String(vulnerabilityRows[0][1] ?? "") === "",
    "SkillEffect vulnerability expression changed");
  assert(durationRows.length === 1 && String(durationRows[0][1] ?? "") === "10s",
    "SkillEffect duration evidence changed");
  const fightRows = Object.values(fightLevelTable)
    .filter((row) => Number(row?.SkillId) === TARGET_SKILL_ID &&
      Number(row?.SkillEffectId) === TARGET_SKILL_EFFECT_ID)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  assert(fightRows.length === 30, "Skill 2209 fight-level row count changed");
  assertExactArray(fightRows.map((row) => Number(row.Level)),
    Array.from({ length: 30 }, (_, index) => index + 1), "Skill 2209 fight levels");
  assert(fightRows.every((row) => (row.FloatParameter ?? []).length === 0 &&
    (row.ShowParameter ?? []).length === 0), "Skill 2209 fight-level parameters are no longer empty");
  const weaponRows = Object.values(weaponStarTable)
    .filter((row) => Number(row?.SkillId) === TARGET_SKILL_ID)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  assert(weaponRows.length === 6, "Skill 2209 weapon-star row count changed");
  assertExactArray(weaponRows.map((row) => Number(row.Level)), [1, 2, 3, 4, 5, 6],
    "Skill 2209 weapon-star levels");
  assert(weaponRows.every((row) => (row.FloatParameter ?? []).length === 0 &&
    (row.BuffPar ?? []).length === 0), "Skill 2209 weapon-star parameters are no longer empty");
  const professions = Object.values(professionTable)
    .filter((row) => (row?.UltimateSkill ?? []).map(Number).includes(TARGET_SKILL_ID))
    .map((row) => Number(row.Id));
  assertExactArray(professions, [11], "Skill 2209 profession ownership");
  const statusOccurrences = occurrences.filter((row) => row.id === TARGET_STATUS_ID);
  assert(statusOccurrences.length === 4, "Status 55228 decoded-table occurrence count changed");
  assertExactArray(
    statusOccurrences.map((row) => `${row.relative_path}:${row.pointer}:${row.representation}`),
    [
      "BuffTable.json:/55228:object-key",
      "BuffTable.json:/55228/Id:number",
      "CollectionTable.json:/55228:object-key",
      "CollectionTable.json:/55228/Id:number",
    ],
    "Status 55228 occurrence surface",
  );
  assert((status.SpecialAttr ?? []).length === 0, "Status 55228 gained SpecialAttr parameters");
  return {
    skill_id: TARGET_SKILL_ID,
    skill_effect_id: TARGET_SKILL_EFFECT_ID,
    status_id: TARGET_STATUS_ID,
    skill_owner_profession_ids: professions,
    exact_skill_to_skill_effect_edge: {
      relationship: "SkillTable.EffectIDs + SkillEffectTable.SkillId",
      proven: true,
    },
    skill_effect_presentation_evidence: {
      vulnerability_label_present: true,
      vulnerability_expression: null,
      duration_evidence: "10s",
      localized_presentation_is_formula_authority: false,
    },
    skill_fight_level_surface: {
      rows: fightRows.length,
      levels: fightRows.map((row) => Number(row.Level)),
      rows_with_float_parameters: fightRows.filter((row) => (row.FloatParameter ?? []).length > 0).length,
      rows_with_show_parameters: fightRows.filter((row) => (row.ShowParameter ?? []).length > 0).length,
    },
    weapon_star_surface: {
      rows: weaponRows.length,
      levels: weaponRows.map((row) => Number(row.Level)),
      rows_with_float_parameters: weaponRows.filter((row) => (row.FloatParameter ?? []).length > 0).length,
      rows_with_buff_parameters: weaponRows.filter((row) => (row.BuffPar ?? []).length > 0).length,
    },
    status_static_surface: {
      level: Number(status.Level),
      repeat_add_rule: structuredClone(status.RepeatAddRule ?? []),
      destroy_param: structuredClone(status.DestroyParam ?? []),
      time_refresh_type: Number(status.TimeRefreshType),
      special_attr: structuredClone(status.SpecialAttr ?? []),
      row_sha256: sha256Text(stableStringify(status)),
    },
    status_exact_occurrences: statusOccurrences,
    unrelated_same_numeric_id_tables: ["CollectionTable.json"],
    exact_typed_skill_or_skill_effect_reference_to_status_55228_found: false,
    exact_id_linked_scalar_candidate_found: false,
    indirect_or_server_formula_ruled_out: false,
    reviewed_skill_to_status_candidate_preserved: true,
    exact_skill_to_status_edge_proven: false,
    exact_current_build_scalar_proven: false,
    operation_order_stacking_and_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
    damage_action_id_collision_boundary: {
      damage_action_id: TARGET_DAMAGE_ACTION_ID,
      is_provider_skill_id: false,
      is_provider_skill_effect_id: false,
      is_status_id: false,
      relationship: "recipient-damage-action-side-measurement-only",
    },
  };
}

function validateReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION, "Unsupported decoded reference surface schema");
  assert(report?.generated_by === GENERATOR, "Decoded reference surface generator mismatch");
  numericString(report?.game_build, "report game_build");
  assert(report?.policy?.exact_numeric_ids_and_build_are_authoritative === true,
    "Exact ID/build policy is missing");
  assert(report?.policy?.table_and_field_identity_required_for_typed_relationships === true,
    "Typed relationship policy is missing");
  assert(report?.policy?.equal_numbers_in_unrelated_tables_are_relationships === false,
    "Numeric collisions were promoted");
  assert(report?.policy?.reviewed_candidate_links_are_exact_edges === false,
    "Reviewed candidates were promoted");
  assert(report?.policy?.absent_exact_id_reference_proves_indirect_or_server_formula_absence === false,
    "Negative exact-ID scan overclaimed formula absence");
  assert(report?.policy?.remote_player_cast_packets_required === false &&
    report?.policy?.remote_player_cast_packets_synthesized === false,
  "Remote cast boundary changed");
  const target = report?.target_findings;
  assert(target?.skill_id === TARGET_SKILL_ID && target?.skill_effect_id === TARGET_SKILL_EFFECT_ID &&
    target?.status_id === TARGET_STATUS_ID, "Target identity mismatch");
  assert(target?.exact_skill_to_skill_effect_edge?.proven === true,
    "Exact skill-to-effect edge was lost");
  assert(target?.exact_typed_skill_or_skill_effect_reference_to_status_55228_found === false,
    "Unproven skill-to-status edge was added");
  assert(target?.exact_id_linked_scalar_candidate_found === false,
    "Unproven exact-ID-linked scalar was added");
  assert(target?.indirect_or_server_formula_ruled_out === false,
    "Indirect/server formula was improperly ruled out");
  assert(target?.reviewed_skill_to_status_candidate_preserved === true &&
    target?.exact_skill_to_status_edge_proven === false,
  "Reviewed source candidate boundary changed");
  assert(target?.damage_action_id_collision_boundary?.relationship ===
    "recipient-damage-action-side-measurement-only", "Damage-action collision boundary changed");
  for (const key of ["formula_authority", "runtime_authority", "ui_display_authority", "provider_rdps_credit_allowed"]) {
    assert(target?.[key] === false && report?.summary?.[key] === false,
      `Unsafe authority in decoded reference surface: ${key}`);
  }
  assert(report?.summary?.hidden_occurrences === 0, "Exact-ID occurrences were hidden");
  assert(report?.content_sha256 === contentHash(report), "Decoded reference surface digest mismatch");
}

function selfTest() {
  const output = [];
  collectOccurrences(
    { "55228": { Id: 55228, Nested: [220901] }, unrelated: "55228" },
    [],
    "FixtureTable.json",
    new Set(["55228", "220901"]),
    output,
  );
  output.sort((left, right) => left.id - right.id || left.pointer.localeCompare(right.pointer));
  assert(output.length === 4, "Self-test occurrence count mismatch");
  assert(output.some((row) => row.pointer === "/55228" && row.representation === "object-key"),
    "Self-test lost object-key identity");
  assert(output.some((row) => row.pointer === "/unrelated" && row.representation === "string"),
    "Self-test lost same-number collision evidence");
  assert(jsonPointer(["a/b", "c~d"]) === "/a~1b/c~0d", "JSON pointer escaping failed");
  console.log("bpsr-party-skill-decoded-reference-surface self-test passed");
}

function countOccurrences(occurrences, ids) {
  return Object.fromEntries(ids.map((id) => [String(id), occurrences.filter((row) => row.id === id).length]));
}

function requiredDocument(documents, name) {
  const document = documents.get(name);
  assert(document, `Required decoded table was not retained: ${name}`);
  return document;
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: slash(file),
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Cannot read ${label} ${file}: ${error.message}`); }
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return `sha256:${sha256Text(stableStringify(copy))}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function sha256Text(value) {
  return createHash("sha256").update(value).digest("hex");
}

function sortedUniqueNumbers(values) {
  return [...new Set(values.filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}

function jsonPointer(parts) {
  return `/${parts.map((part) => String(part).replaceAll("~", "~0").replaceAll("/", "~1")).join("/")}`;
}

function only(values, label) {
  assert(Array.isArray(values) && values.length === 1, `Expected exactly one ${label}`);
  return values[0];
}

function assertExactArray(actual, expected, label) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), `${label} mismatch`);
}

function requireFile(file, label) {
  assert(existsSync(file) && statSync(file).isFile(), `Missing ${label}: ${file}`);
}

function requireDirectory(directory, label) {
  assert(existsSync(directory) && statSync(directory).isDirectory(), `Missing ${label}: ${directory}`);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${flag ?? "end"}`);
    }
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  if (!(key in parsed)) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function numericString(value, label) {
  assert(/^\d+$/.test(String(value)), `${label} must be numeric`);
  return String(value);
}

function slash(value) { return value.replaceAll("\\", "/"); }
function assert(condition, message) { if (!condition) throw new Error(message); }

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-party-skill-decoded-reference-surface.mjs build --build <id> --party-closure <json> --tables-root <directory> --output <json>\n  node tools/bpsr-party-skill-decoded-reference-surface.mjs verify --input <json>\n  node tools/bpsr-party-skill-decoded-reference-surface.mjs self-test");
  process.exit(exitCode);
}
