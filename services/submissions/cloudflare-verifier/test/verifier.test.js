import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  catalogEntry, compatibleProfileName, expectedReportId, reconcileCatalogEntry,
  sameChunkCommitments, runOneShotVerifier, validateOutput, validateReconciliationOutput,
  validateTrainingOutput, validateWakeup,
} from "../src/core.js";

const digest = "a".repeat(64);
const wakeup = {
  schema_version: 1,
  upload_id: `up_${digest.slice(0, 32)}`,
  artifact_sha256: digest,
  expected_report_id: `rpt_${digest.slice(0, 32)}`,
  chunks: [],
};

test("wake-up identities are derived from the sealed digest", () => {
  assert.equal(expectedReportId(digest), wakeup.expected_report_id);
  assert.equal(validateWakeup(wakeup), true);
  assert.equal(validateWakeup({ ...wakeup, expected_report_id: `rpt_${"b".repeat(32)}` }), false);
});

test("container output must preserve report and artifact identities", () => {
  const output = {
    schema_version: 1,
    report: { report_id: wakeup.expected_report_id, verification: { artifact_sha256: digest }, runs: [{}] },
    membership: { report_id: wakeup.expected_report_id, artifact_sha256: digest, runs: [] },
  };
  assert.equal(validateOutput(output, wakeup), true);
  output.report.verification.artifact_sha256 = "b".repeat(64);
  assert.equal(validateOutput(output, wakeup), false);
});

test("training output must be an exact server-replayed solo dummy result", () => {
  const output = {
    schema_version: 1,
    result_id: wakeup.expected_report_id,
    verification: { artifact_sha256: digest },
    character_id: "3296036",
    class_id: 4,
    specialization_id: 41,
    season_id: 3,
    target_monster_id: 115,
    duration_micros: 180_000_000,
    total_damage: 180_000,
    dps: 1_000,
  };
  assert.equal(validateTrainingOutput(output, wakeup), true);
  assert.equal(validateTrainingOutput({ ...output, target_monster_id: 999 }, wakeup), false);
  assert.equal(validateTrainingOutput({ ...output, duration_micros: 183_000_000 }, wakeup), false);
  assert.equal(validateTrainingOutput({ ...output, dps: 999 }, wakeup), false);
});

test("chunk commitments compare semantically rather than by JSON property order", () => {
  const left = [{ sequence: 0, sha256: digest, byte_length: 5, object_key: "uploads/up/chunks/0" }];
  const right = [{ sequence: 0, object_key: "uploads/up/chunks/0", byte_length: 5, sha256: digest }];
  assert.equal(sameChunkCommitments(left, right), true);
  assert.equal(sameChunkCommitments(left, [{ ...right[0], byte_length: 6 }]), false);
});

test("verified profile names follow exact claimed UIDs across refined region labels", () => {
  const manifest = { metadata: { game_region: "north-america" } };
  const row = {
    deployment_id: "global",
    region_id: "north-america",
    public_projection_json: JSON.stringify({ character: { display_name: "MarieRose" } }),
  };
  assert.equal(compatibleProfileName(row, manifest), "MarieRose");
  assert.equal(compatibleProfileName({ ...row, region_id: "asia" }, manifest), "MarieRose");
});

test("verified runs materialize the public catalog contract", () => {
  const entry = catalogEntry({
    report_id: wakeup.expected_report_id,
    created_unix_millis: 42,
    deployment_id: "global",
    region_id: "north-america",
    submission_provenance: { submitter_id: "usr_fixture" },
  }, {
    run_index: 0,
    run_group_id: "run_fixture",
    activity_id: "dungeon_fixture",
    terminal_state: "completed",
    participants: [{}, {}],
  });
  assert.equal(entry.report_id, wakeup.expected_report_id);
  assert.equal(entry.participant_count, 2);
  assert.equal(entry.submitter_id, "usr_fixture");
  assert.equal(entry.attribution_reconciliation_status, "single_vantage");
});

test("reconciliation output must preserve the exact source set and canonical spine", () => {
  const sources = [
    { report_id: `rpt_${"a".repeat(32)}`, run_index: 0 },
    { report_id: `rpt_${"b".repeat(32)}`, run_index: 1 },
  ];
  const output = {
    schema_version: 15,
    reconciliation_id: `rec_${"c".repeat(32)}`,
    run_group_id: "run_exact",
    status: "cross_vantage_evidence_available",
    canonical_spine: sources[0],
    reports: [...sources].reverse(),
  };
  assert.equal(validateReconciliationOutput(output, "run_exact", sources), true);
  assert.equal(validateReconciliationOutput({ ...output, schema_version: 14 }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, reports: [sources[0]] }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, canonical_spine: {
    report_id: `rpt_${"d".repeat(32)}`, run_index: 0,
  } }, "run_exact", sources), false);
});

test("reconciled catalog entries expose one group source set and authority status", () => {
  const result = {
    reconciliation_id: `rec_${"c".repeat(32)}`,
    status: "reconciled",
    local_vantage_character_count: 5,
    reports: [
      { report_id: `rpt_${"b".repeat(32)}` },
      { report_id: `rpt_${"a".repeat(32)}` },
    ],
  };
  const entry = reconcileCatalogEntry({ report_id: result.reports[0].report_id }, result, 2, 2);
  assert.deepEqual(entry.report_ids, result.reports.map((source) => source.report_id).sort());
  assert.equal(entry.contribution_count, 2);
  assert.equal(entry.distinct_submitter_count, 2);
  assert.equal(entry.local_profile_witness_character_count, 5);
  assert.equal(entry.attribution_reconciliation_status, "reconciled");
  assert.equal(entry.reconciliation_id, result.reconciliation_id);
});

test("reconciliation workers acquire one conditional lease before starting a container", async () => {
  const source = await readFile(new URL("../src/index.js", import.meta.url), "utf8");
  assert.match(source, /lease_token=excluded\.lease_token[\s\S]+WHERE reconciliation_jobs\.state IN \('retryable_failure','superseded'\)/u);
  assert.match(source, /WHERE job_id=\?1 AND lease_token=\?2 AND state='running'/u);
  assert.match(source, /job\?\.state === "running"[\s\S]+in_progress: true/u);
  assert.match(source, /getByName\(jobId\)/u);
  assert.ok(source.indexOf("lease_token=?2 AND state='running'") < source.indexOf("getByName(jobId)"));
});

test("one-shot verifier consumes its response before destroying the container", async () => {
  const calls = [];
  const container = {
    async fetch() {
      calls.push("fetch");
      return Response.json({ accepted: true });
    },
    async destroy() { calls.push("destroy"); },
  };
  const { response, result } = await runOneShotVerifier(container, new Request("http://container/verify"));
  assert.equal(response.status, 200);
  assert.deepEqual(result, { accepted: true });
  assert.deepEqual(calls, ["fetch", "destroy"]);
});

test("one-shot verifier destroys a container whose request fails", async () => {
  let destroyed = false;
  const container = {
    async fetch() { throw new Error("container slot failed"); },
    async destroy() { destroyed = true; },
  };
  await assert.rejects(
    runOneShotVerifier(container, new Request("http://container/verify")),
    /container slot failed/,
  );
  assert.equal(destroyed, true);
});
