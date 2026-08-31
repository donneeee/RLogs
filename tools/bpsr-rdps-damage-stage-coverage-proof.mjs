import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-damage-stage-coverage-proof.mjs";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      fail(`invalid option near ${key ?? "<end>"}`);
    }
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

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function sum(values) {
  return values.reduce((total, value) => total + value, 0);
}

function observedDamageByScript(rows) {
  const scripts = new Map();
  for (const row of rows) {
    const damageScript = row.damage_script ?? "<missing>";
    const current = scripts.get(damageScript) ?? { rows: 0, events: 0 };
    current.rows += 1;
    current.events += row.packet_damage_results;
    scripts.set(damageScript, current);
  }
  return Object.fromEntries([...scripts.entries()].sort(([left], [right]) => left.localeCompare(right)));
}

function worklistDamageAttrIds(worklist) {
  const ids = new Set();
  for (const family of worklist.families) {
    for (const signature of family.formula_signatures) {
      for (const item of signature.work_items) {
        ids.add(String(item.damage_attr.damage_attr_id));
      }
    }
  }
  return ids;
}

function buildReport(options) {
  const gameBuild = required(options, "build");
  const coefficientProof = source(required(options, "coefficient-proof"));
  const candidate = source(required(options, "candidate"));
  const runtime = source(required(options, "runtime"));
  const worklist = source(required(options, "worklist"));
  const resolution = source(required(options, "resolution"));
  const conservation = source(required(options, "conservation"));

  assert.equal(coefficientProof.value.schema_version, 11);
  assert.equal(coefficientProof.value.game_build, gameBuild);
  assert.equal(coefficientProof.value.packet_build, gameBuild);
  assert.equal(coefficientProof.value.policy?.runtime_formula_authority, false);
  assert.equal(coefficientProof.value.policy?.unresolved_packet_evidence_is_hidden, false);
  assert.equal(coefficientProof.value.decoded_table_source?.every_surface_row_semantically_joined, true);
  assert.equal(coefficientProof.value.decoded_table_source?.surface_arrays_match_decoded_semantic_fields, true);
  assert.equal(coefficientProof.value.coverage?.observed_damage_rows,
    coefficientProof.value.observed_damage_rows.length);

  assert.equal(candidate.value.game_build, gameBuild);
  assert.equal(candidate.value.policy?.runtime_formula_authority, false);
  assert.equal(candidate.value.policy?.unresolved_events,
    "canonical damage is always retained and never hidden");
  assert.equal(candidate.value.summary?.standard_rules, candidate.value.rules.length);
  assert.equal(candidate.value.summary?.coverage_gap_records, candidate.value.coverage_gaps.length);

  assert.equal(runtime.value.game_build, gameBuild);
  assert.equal(runtime.value.summary?.standard_rules, runtime.value.rules.length);
  assert.equal(runtime.value.policy?.unresolved_events,
    "canonical damage is always retained and never hidden");
  assert.equal(runtime.value.rules.length, candidate.value.rules.length);

  assert.equal(worklist.value.game_build, gameBuild);
  assert.equal(worklist.value.policy?.runtime_authority, false);
  assert.equal(worklist.value.policy?.candidate_retention,
    "every catalog coverage-gap key and candidate row is retained; no unresolved event is hidden or discarded");
  assert.equal(worklist.value.summary?.candidate_rows, candidate.value.summary?.nonstandard_or_missing_script_candidate_rows);

  assert.equal(resolution.value.game_build, gameBuild);
  assert.equal(resolution.value.policy?.unresolved_evidence_hidden, false);
  assert.equal(resolution.value.policy?.static_formula_is_runtime_authority, false);
  assert.equal(resolution.value.summary?.standard_static_formula_candidates
    + resolution.value.summary?.nonstandard_or_missing_formula_candidates,
  resolution.value.summary?.candidate_rows);

  const segment = conservation.value.exact_pack_gap_free_segment;
  assert.equal(conservation.value.game_build, gameBuild);
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);
  assert.equal(segment?.attributed_bonus_damage, 0);

  const runtimeIds = new Set(runtime.value.rules.map((rule) => String(rule.damage_attr_id)));
  const candidateIds = new Set(candidate.value.rules.flatMap((rule) => [
    rule.damage_attr_id,
    ...(rule.equivalent_damage_attr_ids ?? []),
  ]).map(String));
  const worklistIds = worklistDamageAttrIds(worklist.value);
  const standardScripts = new Set(["Attack", "MAttack"]);
  const standardRows = [];
  const nonstandardRows = [];

  for (const row of coefficientProof.value.observed_damage_rows) {
    assert.ok(Number.isSafeInteger(row.packet_damage_results) && row.packet_damage_results >= 0);
    const damageAttrId = String(row.semantic_row.Id);
    if (standardScripts.has(row.damage_script)) {
      assert.equal(candidateIds.has(damageAttrId), true,
        `observed standard row missing from candidate ${damageAttrId}`);
      assert.equal(runtimeIds.has(damageAttrId), true,
        `observed standard row missing from runtime ${damageAttrId}`);
      standardRows.push(row);
    } else {
      assert.equal(runtimeIds.has(damageAttrId), false,
        `nonstandard row unexpectedly enabled in runtime ${damageAttrId}`);
      assert.equal(worklistIds.has(damageAttrId), true,
        `observed nonstandard row missing from worklist ${damageAttrId}`);
      nonstandardRows.push(row);
    }
  }

  const sessionDamageEvents = sum(coefficientProof.value.sessions.map((session) => session.damage_events));
  const unresolvedEvents = sum(coefficientProof.value.sessions.map(
    (session) => session.unresolved_combat_results));
  const mappedDamageEvents = sum(coefficientProof.value.observed_damage_rows.map(
    (row) => row.packet_damage_results));
  const standardDamageEvents = sum(standardRows.map((row) => row.packet_damage_results));
  const nonstandardDamageEvents = sum(nonstandardRows.map((row) => row.packet_damage_results));

  assert.equal(mappedDamageEvents, standardDamageEvents + nonstandardDamageEvents);
  assert.equal(sessionDamageEvents, mappedDamageEvents + unresolvedEvents);
  assert.equal(coefficientProof.value.coverage?.mapped_damage_results_by_damage_script.Attack
    + coefficientProof.value.coverage?.mapped_damage_results_by_damage_script.MAttack,
  standardDamageEvents);

  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: "damage-stage-event-coverage",
    game_build: gameBuild,
    policy: {
      exact_numeric_damage_attr_ids_are_authoritative: true,
      standard_scripts_enabled: ["Attack", "MAttack"],
      nonstandard_scripts_are_runtime_formula_authority: false,
      unresolved_events_are_retained: true,
      event_coverage_is_not_formula_stage_replay: true,
      event_coverage_grants_provider_rdps_credit: false,
    },
    sources: {
      exact_build_packet_coefficient_proof: receipt(coefficientProof),
      exact_build_damage_stage_candidate: receipt(candidate),
      exact_build_damage_stage_runtime: receipt(runtime),
      exact_build_nonstandard_worklist: receipt(worklist),
      exact_build_damage_resolution_ledger: receipt(resolution),
      exact_pack_conservation_boundary: receipt(conservation),
    },
    catalog_coverage: {
      decoded_damage_attr_rows: coefficientProof.value.decoded_table_source.surface_rows,
      candidate_standard_rules: candidate.value.rules.length,
      runtime_standard_rules: runtime.value.rules.length,
      nonstandard_candidate_rows: worklist.value.summary.candidate_rows,
      nonstandard_script_families: worklist.value.summary.script_families,
      unresolved_static_route_candidates: resolution.value.summary.candidates_blocked_on_source_and_formula
        + resolution.value.summary.candidates_blocked_on_source_only,
    },
    observed_event_partition: {
      sessions: coefficientProof.value.sessions.length,
      total_damage_events: sessionDamageEvents,
      standard_runtime_mapped_events: standardDamageEvents,
      mapped_nonstandard_events: nonstandardDamageEvents,
      explicitly_unresolved_events: unresolvedEvents,
      partition_conserved: true,
      observed_damage_attr_rows: coefficientProof.value.observed_damage_rows.length,
      standard_runtime_rows: standardRows.length,
      nonstandard_worklist_rows: nonstandardRows.length,
      by_damage_script: observedDamageByScript(coefficientProof.value.observed_damage_rows),
    },
    conservation: {
      observed_damage_events: segment.damage_events,
      ordinary_raw_damage: segment.ordinary_raw_damage,
      ordinary_rdps_damage: segment.ordinary_rdps_damage,
      exact_party_conservation: true,
      scope: "separate gap-free exact-pack zero-transfer replay; the seven-journal event partition proves stage coverage, not formula counterfactual conservation",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: sessionDamageEvents,
      exact_party_conservation: true,
      every_observed_damage_event_partitioned: true,
      every_observed_standard_row_present_in_runtime: true,
      every_observed_nonstandard_row_retained_outside_runtime: true,
      unresolved_events_retained: true,
      formula_stage_replay_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
  };
  return { ...report, content_sha256: contentSha256(report) };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  writeFileSync(output, `${JSON.stringify(buildReport(options), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildReport(options));
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-rdps-damage-stage-coverage-proof.mjs generate --build <id> --coefficient-proof <json> --candidate <json> --runtime <json> --worklist <json> --resolution <json> --conservation <json> --output <json>\n  node tools/bpsr-rdps-damage-stage-coverage-proof.mjs verify --build <id> --coefficient-proof <json> --candidate <json> --runtime <json> --worklist <json> --resolution <json> --conservation <json> --input <json>");
  process.exit(1);
}
