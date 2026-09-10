import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  BACKFILL_SOURCE_SCHEMA_VERSION, BACKFILL_TARGET_PROJECTION_REVISION,
  BACKFILL_TARGET_SCHEMA_VERSION, BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION,
  CURRENT_REPORT_PROJECTION_REVISION, CURRENT_REPORT_SCHEMA_VERSION,
  CURRENT_TIMELINE_SCHEMA_VERSION,
  LEGACY_RECONCILIATION_SCHEMA_VERSION, LEGACY_REPORT_PROJECTION_REVISION,
  LEGACY_REPORT_SCHEMA_VERSION, LEGACY_TIMELINE_SCHEMA_VERSION,
  RECONCILIATION_SCHEMA_VERSION,
  UPCOMING_RECONCILIATION_SCHEMA_VERSION, UPCOMING_REPORT_PROJECTION_REVISION,
  UPCOMING_REPORT_SCHEMA_VERSION, UPCOMING_TIMELINE_SCHEMA_VERSION,
  catalogEntry, compatibleProfileName, expectedReportId, reconcileCatalogEntry,
  isSchema12BackfillCandidate,
  sameChunkCommitments, runOneShotVerifier, validateOutput, validateReconciliationOutput,
  validateBackfillOutput, validateTrainingOutput, validateWakeup,
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
    report: {
      schema_version: CURRENT_REPORT_SCHEMA_VERSION,
      projection_revision: CURRENT_REPORT_PROJECTION_REVISION,
      report_id: wakeup.expected_report_id,
      verification: { artifact_sha256: digest },
      runs: [{ timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION } }],
    },
    membership: { report_id: wakeup.expected_report_id, artifact_sha256: digest, runs: [] },
  };
  assert.equal(validateOutput(output, wakeup), true);
  output.report.verification.artifact_sha256 = "b".repeat(64);
  assert.equal(validateOutput(output, wakeup), false);
});

test("container output accepts only the exact legacy current and upcoming report tuples", () => {
  const output = {
    schema_version: 1,
    report: {
      schema_version: CURRENT_REPORT_SCHEMA_VERSION,
      projection_revision: CURRENT_REPORT_PROJECTION_REVISION,
      report_id: wakeup.expected_report_id,
      verification: { artifact_sha256: digest },
      runs: [{ timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION } }],
    },
    membership: { report_id: wakeup.expected_report_id, artifact_sha256: digest, runs: [] },
  };
  const upcoming = {
    ...output,
    report: {
      ...output.report,
      schema_version: UPCOMING_REPORT_SCHEMA_VERSION,
      projection_revision: UPCOMING_REPORT_PROJECTION_REVISION,
      runs: [{ timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION } }],
    },
  };
  const legacy = {
    ...output,
    report: {
      ...output.report,
      schema_version: LEGACY_REPORT_SCHEMA_VERSION,
      projection_revision: LEGACY_REPORT_PROJECTION_REVISION,
      runs: [{ timeline: { schema_version: LEGACY_TIMELINE_SCHEMA_VERSION } }],
    },
  };
  assert.equal(validateOutput(legacy, wakeup), true);
  assert.equal(validateOutput(output, wakeup), true);
  assert.equal(validateOutput(upcoming, wakeup), true);
  assert.equal(validateOutput({ ...upcoming, report: {
    ...upcoming.report, schema_version: CURRENT_REPORT_SCHEMA_VERSION,
  } }, wakeup), false);
  assert.equal(validateOutput({ ...legacy, report: {
    ...legacy.report, runs: [{ timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION } }],
  } }, wakeup), false);
  assert.equal(validateOutput({ ...output, report: { ...output.report, schema_version: 14 } }, wakeup), false);
  assert.equal(validateOutput({ ...output, report: { ...output.report, projection_revision: 6 } }, wakeup), false);
  assert.equal(validateOutput({ ...output, report: {
    ...output.report, runs: [{ timeline: { schema_version: 2 } }],
  } }, wakeup), false);
  assert.equal(validateOutput({ ...output, report: {
    ...output.report, runs: [{ timeline: { schema_version: LEGACY_TIMELINE_SCHEMA_VERSION } }],
  } }, wakeup), false);
  assert.equal(validateOutput({ ...output, report: {
    ...output.report, runs: [{ timeline: undefined }],
  } }, wakeup), false);
  assert.equal(validateOutput({ ...upcoming, report: {
    ...upcoming.report, runs: [{ timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION } }],
  } }, wakeup), false);
  assert.equal(validateOutput({ ...upcoming, report: {
    ...upcoming.report, runs: [
      { timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION } },
      { timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION } },
    ],
  } }, wakeup), false);
});

test("backfill eligibility is limited to current public schema-12 replay evidence", () => {
  const row = {
    report_id: wakeup.expected_report_id, artifact_sha256: digest,
    visibility: "public", verification_tier: "replayed",
  };
  const report = {
    schema_version: BACKFILL_SOURCE_SCHEMA_VERSION, report_id: wakeup.expected_report_id,
    visibility: "public", verification: { artifact_sha256: digest },
  };
  assert.equal(isSchema12BackfillCandidate(report, row), true);
  assert.equal(isSchema12BackfillCandidate({ ...report, schema_version: 13 }, row), false);
  assert.equal(isSchema12BackfillCandidate({ ...report, visibility: "unlisted" }, row), false);
  assert.equal(isSchema12BackfillCandidate(report, { ...row, verification_tier: "ranked" }), false);
});

test("backfill output can add schema fields but cannot change identity, owner, visibility, or evidence", () => {
  const submitter = "usr_fixture";
  const row = {
    report_id: wakeup.expected_report_id, artifact_sha256: digest, submitter_id: submitter,
    visibility: "public", verifier_release: "old-release",
  };
  const original = {
    schema_version: 12, report_id: wakeup.expected_report_id, visibility: "public",
    deployment_id: "global", region_id: "north-america", created_unix_millis: 42,
    submission_provenance: { submitter_id: submitter },
    verification: {
      artifact_sha256: digest, tier: "replayed", canonical_content_sha256: "sha256:content",
      event_count: 10, privacy_policy_digest: "sha256:privacy",
    },
  };
  const output = {
    schema_version: 1,
    report: {
      ...original, schema_version: BACKFILL_TARGET_SCHEMA_VERSION,
      projection_revision: BACKFILL_TARGET_PROJECTION_REVISION,
      verification: { ...original.verification }, runs: [{
        run_index: 0, run_group_id: "run_fixture",
        // Backfill replay still emits the deployed tuple until its producer bumps.
        timeline: { schema_version: BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION },
      }],
    },
    membership: {
      report_id: wakeup.expected_report_id, artifact_sha256: digest,
      character_by_actor: {}, runs: [],
    },
  };
  assert.equal(validateBackfillOutput(output, wakeup, original, row, "new-release"), true);
  assert.equal(validateBackfillOutput({ ...output, report: {
    ...output.report, visibility: "unlisted",
  } }, wakeup, original, row, "new-release"), false);
  assert.equal(validateBackfillOutput({ ...output, report: {
    ...output.report, submission_provenance: { submitter_id: "usr_other" },
  } }, wakeup, original, row, "new-release"), false);
  assert.equal(validateBackfillOutput({ ...output, report: {
    ...output.report, region_id: "asia",
  } }, wakeup, original, row, "new-release"), false);
  assert.equal(validateBackfillOutput(output, wakeup, original, row, "short"), false);
  assert.equal(validateBackfillOutput({ ...output, report: {
    ...output.report, runs: [{ ...output.report.runs[0], timeline: { schema_version: 2 } }],
  } }, wakeup, original, row, "new-release"), false);
  assert.equal(validateBackfillOutput({ ...output, report: {
    ...output.report,
    projection_revision: UPCOMING_REPORT_PROJECTION_REVISION,
    runs: [{ ...output.report.runs[0], timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION } }],
  } }, wakeup, original, row, "new-release"), false);
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
    client_build: "24687926",
    protocol_pack_digest: "sha256:test-pack",
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
  assert.equal(entry.client_build, "24687926");
  assert.equal(entry.protocol_pack_digest, "sha256:test-pack");
});

test("reconciliation output must preserve the exact source set and canonical spine", () => {
  const sources = [
    { report_id: `rpt_${"a".repeat(32)}`, run_index: 0, artifact_sha256: "a".repeat(64) },
    { report_id: `rpt_${"b".repeat(32)}`, run_index: 1, artifact_sha256: "b".repeat(64) },
  ];
  const output = {
    schema_version: RECONCILIATION_SCHEMA_VERSION,
    reconciliation_id: `rec_${"c".repeat(32)}`,
    run_group_id: "run_exact",
    status: "cross_vantage_evidence_available",
    canonical_spine: sources[0],
    reports: [...sources].reverse().map((source) => ({
      ...source,
      deployment_id: "global",
      client_build: "24687926",
      protocol_pack_digest: "sha256:pack",
    })),
    attribution_replay_completed: false,
    rdps_status: null,
    timeline: { schema_version: CURRENT_TIMELINE_SCHEMA_VERSION },
  };
  assert.equal(validateReconciliationOutput({
    ...output,
    schema_version: LEGACY_RECONCILIATION_SCHEMA_VERSION,
    timeline: { schema_version: LEGACY_TIMELINE_SCHEMA_VERSION },
  }, "run_exact", sources), true);
  assert.equal(validateReconciliationOutput(output, "run_exact", sources), true);
  assert.equal(validateReconciliationOutput({
    ...output,
    schema_version: UPCOMING_RECONCILIATION_SCHEMA_VERSION,
    timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION },
  }, "run_exact", sources), true);
  assert.equal(validateReconciliationOutput({ ...output, rdps_status: "partial_packet_proven_rules" }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, schema_version: 16 }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, timeline: { schema_version: 2 } }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION } }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, timeline: undefined }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({
    ...output, schema_version: UPCOMING_RECONCILIATION_SCHEMA_VERSION,
  }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({
    ...output, timeline: { schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION },
  }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, reports: [sources[0]] }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, canonical_spine: {
    report_id: `rpt_${"d".repeat(32)}`, run_index: 0, artifact_sha256: "d".repeat(64),
  } }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, reports: output.reports.map((report, index) =>
    index === 0 ? { ...report, artifact_sha256: "d".repeat(64) } : report) }, "run_exact", sources), false);
  for (const field of ["deployment_id", "client_build", "protocol_pack_digest"]) {
    const missing = structuredClone(output);
    delete missing.reports[0][field];
    assert.equal(validateReconciliationOutput(missing, "run_exact", sources), false);
    const empty = structuredClone(output);
    empty.reports[0][field] = "";
    assert.equal(validateReconciliationOutput(empty, "run_exact", sources), false);
  }
});

test("completed reconciliation requires replay-authored status, conservation, and an exact rate clock", () => {
  const sources = [
    { report_id: `rpt_${"a".repeat(32)}`, run_index: 0, artifact_sha256: "a".repeat(64) },
    { report_id: `rpt_${"b".repeat(32)}`, run_index: 0, artifact_sha256: "b".repeat(64) },
  ];
  const output = {
    schema_version: RECONCILIATION_SCHEMA_VERSION,
    reconciliation_id: `rec_${"c".repeat(32)}`,
    run_group_id: "run_exact",
    status: "reconciled",
    canonical_spine: sources[0],
    reports: sources.map((source) => ({
      ...source,
      deployment_id: "global",
      client_build: "24687926",
      protocol_pack_digest: "sha256:pack",
    })),
    attribution_replay_completed: true,
    rdps_status: "partial_packet_proven_rules",
    reconciled_participants: [{ actor_id: "1" }],
    conservation: {
      raw_damage: 100,
      rdps_damage: 100,
      contribution_given: 10,
      contribution_received: 10,
      conserved: true,
    },
    timeline: {
      schema_version: CURRENT_TIMELINE_SCHEMA_VERSION,
      source: "reconciled_canonical_spine",
      duration_micros: 2_000_000,
      series_bucket_micros: 1_000_000,
      rate_clock_complete: true,
      rate_clock: [
        { second: 0, edps_elapsed_micros: 1_000_000, adps_elapsed_micros: 500_000 },
        { second: 1, edps_elapsed_micros: 2_000_000, adps_elapsed_micros: 1_500_000 },
      ],
      omitted: { rate_clock_points: 0 },
    },
  };
  assert.equal(validateReconciliationOutput(output, "run_exact", sources), true);
  assert.equal(validateReconciliationOutput({
    ...output,
    schema_version: UPCOMING_RECONCILIATION_SCHEMA_VERSION,
    timeline: { ...output.timeline, schema_version: UPCOMING_TIMELINE_SCHEMA_VERSION },
  }, "run_exact", sources), true);

  for (const rdpsStatus of [undefined, "", " "]) {
    assert.equal(validateReconciliationOutput({ ...output, rdps_status: rdpsStatus }, "run_exact", sources), false);
  }
  assert.equal(validateReconciliationOutput({ ...output, status: "cross_vantage_evidence_available" }, "run_exact", sources), false);
  assert.equal(validateReconciliationOutput({ ...output, conservation: { ...output.conservation, rdps_damage: 99 } }, "run_exact", sources), false);

  const noncontiguous = structuredClone(output);
  noncontiguous.timeline.rate_clock[1].second = 2;
  assert.equal(validateReconciliationOutput(noncontiguous, "run_exact", sources), false);
  const nonmonotonic = structuredClone(output);
  nonmonotonic.timeline.rate_clock[1].adps_elapsed_micros = 100;
  assert.equal(validateReconciliationOutput(nonmonotonic, "run_exact", sources), false);
  const pastBucketBoundary = structuredClone(output);
  pastBucketBoundary.timeline.rate_clock[0].edps_elapsed_micros = 1_000_001;
  assert.equal(validateReconciliationOutput(pastBucketBoundary, "run_exact", sources), false);
  const incompleteWithPoints = structuredClone(output);
  incompleteWithPoints.timeline.rate_clock_complete = false;
  assert.equal(validateReconciliationOutput(incompleteWithPoints, "run_exact", sources), false);
  const incomplete = structuredClone(incompleteWithPoints);
  incomplete.timeline.rate_clock = [];
  assert.equal(validateReconciliationOutput(incomplete, "run_exact", sources), true);
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
  assert.match(source, /lease_token=excluded\.lease_token[\s\S]+reconciliation_jobs\.state='running'[\s\S]+updated_unix_millis<=\?6/u);
  assert.match(source, /WHERE job_id=\?1 AND lease_token=\?2 AND state='running'/u);
  assert.match(source, /job\?\.state === "running"[\s\S]+in_progress: true/u);
  assert.match(source, /lease\.job_id=\?1 AND lease\.lease_token=\?4 AND lease\.state='running'/u);
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
