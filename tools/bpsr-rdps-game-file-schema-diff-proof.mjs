#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-game-file-schema-diff-proof.mjs";
const SUITE_ID = "game-file-schema-diff";
const EXPECTED_DOMAINS = new Set([
  "skills",
  "talents",
  "imagines",
  "psychoscope-factors",
  "equipment-set-bonuses",
  "buffs-effects",
  "formulas-scaling",
  "relationships-recount",
  "seasonal-activity-identity",
  "scenes-encounters-entities",
  "classes-specializations-loadouts",
  "items-weapons-profile",
  "localization-presentation-references",
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseOptions(rest);
if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildReport(values) {
  const gameBuild = required(values, "build");
  const buildManifest = source(required(values, "build-manifest"));
  const ctbDiff = source(required(values, "ctb-diff"));
  const baselineIndex = source(required(values, "baseline-index"));
  const candidateIndex = source(required(values, "candidate-index"));
  const seasonalDiff = source(required(values, "seasonal-diff"));
  const conservation = source(required(values, "conservation"));

  validateBuildManifest(buildManifest.value, gameBuild);
  validateCombatTableDiff(ctbDiff.value, gameBuild);
  validateIndex(baselineIndex.value);
  validateIndex(candidateIndex.value, gameBuild);
  assert.equal(baselineIndex.value.gameBuild, seasonalDiff.value.baselineBuild);
  assert.equal(candidateIndex.value.gameBuild, seasonalDiff.value.candidateBuild);

  const baselineDomains = loadAndValidateDomains(baselineIndex.value);
  const candidateDomains = loadAndValidateDomains(
    candidateIndex.value,
    buildManifest.value,
  );
  const comparison = validateSeasonalDiff(
    seasonalDiff.value,
    baselineDomains,
    candidateDomains,
  );
  const conservationBoundary = validateConservation(conservation.value, gameBuild);

  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: SUITE_ID,
    game_build: gameBuild,
    baseline_build: baselineIndex.value.gameBuild,
    policy: {
      exact_numeric_table_keys_and_row_ids_are_authoritative: true,
      exact_build_source_hashes_are_required: true,
      localized_names_are_formula_authority: false,
      all_changed_rows_are_retained: true,
      unresolved_rows_are_hidden: false,
      static_schema_diff_is_formula_authority: false,
      static_schema_diff_grants_provider_credit: false,
      candidate_data_is_auto_promoted: false,
    },
    sources: {
      complete_build_source_manifest: receipt(buildManifest),
      combat_table_diff: receipt(ctbDiff),
      baseline_domain_index: receipt(baselineIndex),
      candidate_domain_index: receipt(candidateIndex),
      complete_seasonal_domain_diff: receipt(seasonalDiff),
      exact_pack_conservation_boundary: receipt(conservation),
      baseline_domain_manifests: [...baselineDomains.values()].map(
        (entry) => receipt(entry.source),
      ),
      candidate_domain_manifests: [...candidateDomains.values()].map(
        (entry) => receipt(entry.source),
      ),
    },
    schema_coverage: {
      exact_build_source_files_hashed: buildManifest.value.coverage.filesHashed,
      exact_build_source_bytes_hashed: buildManifest.value.coverage.bytesHashed,
      candidate_manifest_sources_linked_to_exact_build:
        [...candidateDomains.values()].reduce(
          (sum, entry) => sum + entry.value.sources.length,
          0,
        ),
      combat_tables_compared: ctbDiff.value.summary.current_tables,
      baseline_domains: baselineDomains.size,
      candidate_domains: candidateDomains.size,
      baseline_rows: sumIndexRows(baselineIndex.value),
      candidate_rows: sumIndexRows(candidateIndex.value),
      changed_domains: comparison.changedDomains,
      unchanged_domains: comparison.unchangedDomains,
      added_rows: comparison.addedRows,
      removed_rows: comparison.removedRows,
      changed_rows: comparison.changedRows,
      changed_rows_by_authority: comparison.changedRowsByAuthority,
      missing_domain_manifests: 0,
      missing_required_inputs: 0,
      silent_omissions: 0,
    },
    conservation: {
      observed_damage_events: conservationBoundary.damage_events,
      ordinary_raw_damage: conservationBoundary.ordinary_raw_damage,
      ordinary_rdps_damage: conservationBoundary.ordinary_rdps_damage,
      exact_party_conservation: true,
      scope:
        "complete exact-build static schema diff plus gap-free exact-pack zero-transfer replay; formula, lifecycle, ownership, and attribution proofs remain separate",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: conservationBoundary.damage_events,
      exact_party_conservation: true,
      complete_current_build_inventory_proven: true,
      all_expected_domains_compared: true,
      exact_build_source_linkage_proven: true,
      game_file_schema_diff_proven: true,
      formula_stage_replay_proven: false,
      provider_recipient_replay_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
  };
  return { ...report, content_sha256: contentHash(report) };
}

function validateBuildManifest(value, gameBuild) {
  assert.equal(value.schemaVersion, 1);
  assert.equal(value.gameBuild, gameBuild);
  assert.equal(value.coverage?.complete, true);
  assert.equal(value.coverage?.silentOmissions, 0);
  assert.equal(value.coverage?.filesDiscovered, value.coverage?.filesHashed);
  assert.ok(value.coverage?.filesHashed > 0);
  assert.ok(value.coverage?.bytesHashed > 0);
  assert.deepEqual(value.missingRoots, []);
  assert.deepEqual(value.missingRequiredFiles, []);
}

function validateCombatTableDiff(value, gameBuild) {
  assert.equal(value.schema_version, 1);
  assert.equal(value.build_id, gameBuild);
  assert.equal(value.summary?.baseline_tables, value.summary?.current_tables);
  assert.equal(value.summary?.added_tables, 0);
  assert.equal(value.summary?.removed_tables, 0);
  assert.equal(
    value.summary?.unchanged_tables + value.summary?.changed_tables,
    value.summary?.current_tables,
  );
  assert.equal(value.changes?.length, value.summary?.changed_tables);
  assert.equal(value.policy?.changed_tables_auto_promoted, false);
  assert.equal(value.policy?.unresolved_tables_hidden, false);
}

function validateIndex(value, expectedBuild = null) {
  assert.equal(value.schemaVersion, 1);
  assert.equal(value.generatedBy, "tools/bpsr-seasonal-domain-scan.mjs");
  if (expectedBuild !== null) assert.equal(value.gameBuild, expectedBuild);
  assert.equal(value.policy?.allRowsRetained, true);
  assert.equal(value.policy?.unresolvedRowsHidden, false);
  assert.equal(value.policy?.candidateDataNeverAutoPromoted, true);
  assert.deepEqual(value.missingRequiredInputs, []);
  assert.deepEqual(value.missingOptionalInputs, []);
  assert.equal(value.domains.length, EXPECTED_DOMAINS.size);
  assert.deepEqual(
    [...new Set(value.domains.map((entry) => entry.domain))].sort(),
    [...EXPECTED_DOMAINS].sort(),
  );
  for (const entry of value.domains) {
    assert.equal(entry.missingRequiredCount, 0);
    assert.equal(entry.missingOptionalCount, 0);
    assert.ok(entry.sourceCount > 0);
    assert.ok(entry.rowCount > 0);
    assert.match(entry.aggregateSha256, /^[0-9a-f]{64}$/);
  }
}

function loadAndValidateDomains(index, buildManifest = null) {
  const buildFiles = buildManifest === null
    ? null
    : new Map(buildManifest.files.map((entry) => [entry.id, entry]));
  const domains = new Map();
  for (const indexEntry of index.domains) {
    const manifestSource = source(indexEntry.path);
    const value = manifestSource.value;
    assert.equal(value.schemaVersion, 1);
    assert.equal(value.generatedBy, "tools/bpsr-seasonal-domain-scan.mjs");
    assert.equal(value.gameBuild, index.gameBuild);
    assert.equal(value.domain, indexEntry.domain);
    assert.equal(value.policy?.allRowsRetained, true);
    assert.equal(value.policy?.unresolvedRowsHidden, false);
    assert.deepEqual(value.missingRequiredInputs, []);
    assert.deepEqual(value.missingOptionalInputs, []);
    assert.equal(value.summary?.sourceCount, value.sources.length);
    assert.equal(value.summary?.sourceCount, indexEntry.sourceCount);
    assert.equal(value.summary?.rowCount, indexEntry.rowCount);
    assert.equal(value.aggregateSha256, indexEntry.aggregateSha256);
    for (const sourceEntry of value.sources) {
      assert.equal(
        sourceEntry.rowCount,
        Object.keys(sourceEntry.rowFingerprints ?? {}).length,
      );
      assert.equal(sourceEntry.semanticSha256, semanticHash(sourceEntry.rowFingerprints));
      if (buildFiles !== null) {
        const root = sourceEntry.root === "decoded"
          ? "decoded-game-tables"
          : "generated-research";
        const exactBuildSource = buildFiles.get(`${root}:${sourceEntry.file}`);
        assert.ok(exactBuildSource, `missing exact-build source ${root}:${sourceEntry.file}`);
        assert.equal(sourceEntry.bytes, exactBuildSource.bytes);
        assert.equal(sourceEntry.sha256, exactBuildSource.sha256);
      }
    }
    assert.equal(
      value.aggregateSha256,
      sha256(value.sources.map(
        (entry) => `${entry.id}:${entry.semanticSha256}`,
      ).join("\n")),
    );
    domains.set(indexEntry.domain, { source: manifestSource, value });
  }
  return domains;
}

function validateSeasonalDiff(value, baselineDomains, candidateDomains) {
  assert.equal(value.schemaVersion, 1);
  assert.equal(value.generatedBy, "tools/bpsr-seasonal-domain-scan.mjs");
  assert.equal(value.policy?.allRowsRetained, true);
  assert.equal(value.policy?.unresolvedRowsHidden, false);
  assert.deepEqual(value.missingManifests, []);
  const changedByDomain = new Map(value.changedDomains.map((entry) => [entry.domain, entry]));
  const compared = new Set([
    ...changedByDomain.keys(),
    ...value.unchangedDomains,
  ]);
  assert.deepEqual([...compared].sort(), [...EXPECTED_DOMAINS].sort());
  let addedRows = 0;
  let removedRows = 0;
  let changedRows = 0;
  const changedRowsByAuthority = {};
  for (const domain of EXPECTED_DOMAINS) {
    const baseline = baselineDomains.get(domain)?.value;
    const candidate = candidateDomains.get(domain)?.value;
    assert.ok(baseline && candidate, `missing compared domain ${domain}`);
    const actual = compareDomain(baseline, candidate);
    const recorded = changedByDomain.get(domain);
    if (actual.addedRows.length || actual.removedRows.length || actual.changedRows.length ||
      actual.addedSources.length || actual.removedSources.length) {
      assert.ok(recorded, `changed domain not retained ${domain}`);
      assert.equal(recorded.aggregateChanged, baseline.aggregateSha256 !== candidate.aggregateSha256);
      assert.deepEqual(recorded.addedSources, actual.addedSources);
      assert.deepEqual(recorded.removedSources, actual.removedSources);
      assert.deepEqual(recorded.addedRows, actual.addedRows);
      assert.deepEqual(recorded.removedRows, actual.removedRows);
      assert.deepEqual(recorded.changedRows, actual.changedRows);
      assert.deepEqual(recorded.changesByAuthority, actual.changesByAuthority);
    } else {
      assert.equal(recorded, undefined);
      assert.ok(value.unchangedDomains.includes(domain));
      assert.equal(baseline.aggregateSha256, candidate.aggregateSha256);
    }
    addedRows += actual.addedRows.length;
    removedRows += actual.removedRows.length;
    changedRows += actual.changedRows.length;
    mergeAuthorityCounts(changedRowsByAuthority, actual.changesByAuthority);
  }
  return {
    changedDomains: value.changedDomains.length,
    unchangedDomains: value.unchangedDomains.length,
    addedRows,
    removedRows,
    changedRows,
    changedRowsByAuthority,
  };
}

function compareDomain(baseline, candidate) {
  const before = flattenRows(baseline);
  const after = flattenRows(candidate);
  const beforeKeys = new Set(Object.keys(before));
  const afterKeys = new Set(Object.keys(after));
  const addedRows = [...afterKeys].filter((key) => !beforeKeys.has(key)).sort();
  const removedRows = [...beforeKeys].filter((key) => !afterKeys.has(key)).sort();
  const changedRows = [...afterKeys]
    .filter((key) => beforeKeys.has(key) && before[key] !== after[key])
    .sort();
  return {
    addedSources: sourceIds(candidate).filter((id) => !sourceIds(baseline).includes(id)),
    removedSources: sourceIds(baseline).filter((id) => !sourceIds(candidate).includes(id)),
    addedRows,
    removedRows,
    changedRows,
    changesByAuthority: summarizeByAuthority(
      baseline,
      candidate,
      addedRows,
      removedRows,
      changedRows,
    ),
  };
}

function validateConservation(value, gameBuild) {
  assert.equal(value.schema_version, 1);
  assert.equal(value.game_build, gameBuild);
  const segment = value.exact_pack_gap_free_segment;
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);
  return segment;
}

function flattenRows(manifest) {
  const output = {};
  for (const sourceEntry of manifest.sources ?? []) {
    for (const [key, value] of Object.entries(sourceEntry.rowFingerprints ?? {})) {
      output[`${sourceEntry.id}#${key}`] = value;
    }
  }
  return output;
}

function sourceIds(manifest) {
  return (manifest.sources ?? []).map((entry) => entry.id);
}

function summarizeByAuthority(baseline, candidate, addedRows, removedRows, changedRows) {
  const authorityBySource = new Map();
  for (const entry of [...(baseline.sources ?? []), ...(candidate.sources ?? [])]) {
    authorityBySource.set(entry.id, entry.authority ?? "unknown");
  }
  const summary = {};
  for (const [kind, rows] of [
    ["added", addedRows],
    ["removed", removedRows],
    ["changed", changedRows],
  ]) {
    for (const row of rows) {
      const sourceId = row.split("#", 1)[0];
      const authority = authorityBySource.get(sourceId) ?? "unknown";
      summary[authority] ??= { added: 0, removed: 0, changed: 0 };
      summary[authority][kind] += 1;
    }
  }
  return summary;
}

function mergeAuthorityCounts(output, input) {
  for (const [authority, counts] of Object.entries(input)) {
    output[authority] ??= { added: 0, removed: 0, changed: 0 };
    output[authority].added += counts.added;
    output[authority].removed += counts.removed;
    output[authority].changed += counts.changed;
  }
}

function semanticHash(rowFingerprints) {
  return sha256(Object.entries(rowFingerprints ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}:${value}`)
    .join("\n"));
}

function sumIndexRows(index) {
  return index.domains.reduce((sum, entry) => sum + entry.rowCount, 0);
}

function generate(values) {
  const output = path.resolve(required(values, "output"));
  writeFileSync(output, `${JSON.stringify(buildReport(values), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(values) {
  const input = path.resolve(required(values, "input"));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildReport(values));
  console.log(input);
}

function selfTest() {
  const left = {
    sources: [{
      id: "decoded:A.json",
      authority: "exact-game-table",
      rowFingerprints: { "$:1": "a", "$:2": "b" },
    }],
  };
  const right = {
    sources: [{
      id: "decoded:A.json",
      authority: "exact-game-table",
      rowFingerprints: { "$:1": "a", "$:2": "c", "$:3": "d" },
    }],
  };
  const result = compareDomain(left, right);
  assert.deepEqual(result.addedRows, ["decoded:A.json#$:3"]);
  assert.deepEqual(result.changedRows, ["decoded:A.json#$:2"]);
  assert.deepEqual(result.removedRows, []);
  assert.deepEqual(result.changesByAuthority, {
    "exact-game-table": { added: 1, removed: 0, changed: 1 },
  });
  console.log("self-test passed");
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

function contentHash(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, stable(value[key])]),
    );
  }
  return value;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function parseOptions(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) usage(1);
    values[key.slice(2)] = value;
  }
  return values;
}

function required(values, key) {
  const value = values[key];
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
      "  node tools/bpsr-rdps-game-file-schema-diff-proof.mjs generate --build <id> --build-manifest <json> --ctb-diff <json> --baseline-index <json> --candidate-index <json> --seasonal-diff <json> --conservation <json> --output <json>\n" +
      "  node tools/bpsr-rdps-game-file-schema-diff-proof.mjs verify --build <id> --build-manifest <json> --ctb-diff <json> --baseline-index <json> --candidate-index <json> --seasonal-diff <json> --conservation <json> --input <json>\n" +
      "  node tools/bpsr-rdps-game-file-schema-diff-proof.mjs self-test",
  );
  process.exit(exitCode);
}
