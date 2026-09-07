import assert from "node:assert/strict";
import test from "node:test";

import {
  catalogEntry, compatibleProfileName, expectedReportId, sameChunkCommitments,
  runOneShotVerifier, validateOutput, validateWakeup,
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
