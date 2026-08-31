import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-static-diff-suite-proof.mjs";
const SUITES = new Set(["combat-table-diff", "formula-surface-diff"]);

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

function buildReport(options) {
  const gameBuild = required(options, "build");
  const suiteId = required(options, "suite");
  assert.equal(SUITES.has(suiteId), true, `unsupported suite ${suiteId}`);
  const buildManifest = source(required(options, "build-manifest"));
  const ctbDiff = source(required(options, "ctb-diff"));
  const formulaMigration = source(required(options, "formula-migration"));
  const formulaInventory = source(required(options, "formula-inventory"));
  const seasonalDiff = source(required(options, "seasonal-diff"));
  const conservation = source(required(options, "conservation"));

  assert.equal(buildManifest.value.schemaVersion, 1);
  assert.equal(buildManifest.value.gameBuild, gameBuild);
  assert.equal(buildManifest.value.coverage?.complete, true);
  assert.equal(buildManifest.value.coverage?.silentOmissions, 0);
  assert.equal(buildManifest.value.missingRoots?.length, 0);
  assert.equal(buildManifest.value.missingRequiredFiles?.length, 0);
  assert.equal(buildManifest.value.coverage?.filesDiscovered,
    buildManifest.value.coverage?.filesHashed);

  assert.equal(ctbDiff.value.schema_version, 1);
  assert.equal(ctbDiff.value.build_id, gameBuild);
  assert.equal(ctbDiff.value.summary?.baseline_tables, ctbDiff.value.summary?.current_tables);
  assert.equal(ctbDiff.value.summary?.added_tables, 0);
  assert.equal(ctbDiff.value.summary?.removed_tables, 0);
  assert.equal(ctbDiff.value.summary?.unchanged_tables + ctbDiff.value.summary?.changed_tables,
    ctbDiff.value.summary?.current_tables);
  assert.equal(ctbDiff.value.changes?.length, ctbDiff.value.summary?.changed_tables);
  assert.equal(ctbDiff.value.policy?.changed_tables_auto_promoted, false);
  assert.equal(ctbDiff.value.policy?.unresolved_tables_hidden, false);

  assert.equal(formulaMigration.value.schema_version, 1);
  assert.equal(formulaMigration.value.build_id, gameBuild);
  assert.equal(formulaMigration.value.policy?.historical_packet_evidence_is_current_formula_authority,
    false);
  assert.equal(formulaMigration.value.policy?.unresolved_evidence_hidden, false);
  assert.equal(formulaMigration.value.decoded_row_comparison?.baseline_decoded_table_available, true);
  assert.equal(formulaMigration.value.decoded_row_comparison?.canonical_json_object_equality, true);
  assert.equal(formulaMigration.value.decoded_row_comparison?.changed_candidate_rows, 0);
  assert.equal(formulaMigration.value.decoded_row_comparison?.added_candidate_rows, 0);
  assert.equal(formulaMigration.value.decoded_row_comparison?.missing_current_candidate_rows, 0);
  assert.equal(formulaMigration.value.summary?.historical_static_rows_eligible_as_current_authority, 0);

  assert.equal(formulaInventory.value.schema_version, 1);
  assert.equal(formulaInventory.value.game_build, gameBuild);
  assert.equal(formulaInventory.value.policy?.runtime_formula_authority, false);
  assert.equal(formulaInventory.value.policy?.unresolved_evidence_hidden, false);
  assert.ok(formulaInventory.value.summary?.script_families > 0);
  assert.ok(formulaInventory.value.summary?.candidate_rows > 0);

  assert.equal(seasonalDiff.value.schemaVersion, 1);
  assert.equal(seasonalDiff.value.candidateBuild, gameBuild);
  assert.equal(seasonalDiff.value.policy?.candidateDataNeverAutoPromoted, true);
  assert.equal(seasonalDiff.value.policy?.unresolvedRowsHidden, false);
  const formulaDomain = seasonalDiff.value.changedDomains?.find(
    (domain) => domain.domain === "formulas-scaling");
  assert.ok(formulaDomain, "formulas-scaling diff");
  assert.equal(formulaDomain.aggregateChanged, true);
  assert.equal(seasonalDiff.value.missingManifests?.some(
    (entry) => entry.domain === "formulas-scaling"), false);

  const segment = conservation.value.exact_pack_gap_free_segment;
  assert.equal(conservation.value.schema_version, 1);
  assert.equal(conservation.value.game_build, gameBuild);
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);

  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: suiteId,
    game_build: gameBuild,
    policy: {
      exact_table_keys_and_row_ids_are_authoritative: true,
      changed_rows_are_retained: true,
      static_diff_is_not_formula_authority: true,
      changed_inputs_are_not_auto_promoted: true,
      unresolved_rows_are_not_hidden: true,
    },
    sources: {
      complete_build_source_manifest: receipt(buildManifest),
      combat_table_diff: receipt(ctbDiff),
      damage_script_migration: receipt(formulaMigration),
      damage_script_static_inputs: receipt(formulaInventory),
      seasonal_domain_diff: receipt(seasonalDiff),
      exact_pack_conservation_boundary: receipt(conservation),
    },
    diff_coverage: {
      source_files_hashed: buildManifest.value.coverage.filesHashed,
      source_bytes_hashed: buildManifest.value.coverage.bytesHashed,
      combat_tables_compared: ctbDiff.value.summary.current_tables,
      unchanged_combat_tables: ctbDiff.value.summary.unchanged_tables,
      changed_combat_tables: ctbDiff.value.summary.changed_tables,
      added_combat_tables: ctbDiff.value.summary.added_tables,
      removed_combat_tables: ctbDiff.value.summary.removed_tables,
      damage_script_families: formulaInventory.value.summary.script_families,
      current_formula_candidate_rows: formulaInventory.value.summary.candidate_rows,
      unchanged_migrated_candidate_rows:
        formulaMigration.value.decoded_row_comparison.unchanged_candidate_rows,
      formulas_scaling_domain_changed: formulaDomain.aggregateChanged,
      formulas_scaling_added_rows: formulaDomain.addedRows?.length ?? 0,
      formulas_scaling_removed_rows: formulaDomain.removedRows?.length ?? 0,
      formulas_scaling_changed_rows: formulaDomain.changedRows?.length ?? 0,
    },
    conservation: {
      observed_damage_events: segment.damage_events,
      ordinary_raw_damage: segment.ordinary_raw_damage,
      ordinary_rdps_damage: segment.ordinary_rdps_damage,
      exact_party_conservation: true,
      scope: "static diff plus gap-free exact-pack zero-transfer replay; formula stages remain separate",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: segment.damage_events,
      exact_party_conservation: true,
      complete_current_build_inventory_proven: true,
      combat_table_diff_proven: suiteId === "combat-table-diff",
      formula_surface_diff_proven: suiteId === "formula-surface-diff",
      changed_inputs_retained_without_promotion: true,
      formula_stage_replay_proven: false,
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
  console.log("Usage:\n  node tools/bpsr-rdps-static-diff-suite-proof.mjs generate --build <id> --suite <combat-table-diff|formula-surface-diff> --build-manifest <json> --ctb-diff <json> --formula-migration <json> --formula-inventory <json> --seasonal-diff <json> --conservation <json> --output <json>\n  node tools/bpsr-rdps-static-diff-suite-proof.mjs verify --build <id> --suite <id> --build-manifest <json> --ctb-diff <json> --formula-migration <json> --formula-inventory <json> --seasonal-diff <json> --conservation <json> --input <json>");
  process.exit(1);
}
