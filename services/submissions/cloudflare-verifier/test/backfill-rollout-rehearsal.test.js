import assert from "node:assert/strict";
import test from "node:test";

import { runBackfillRolloutRehearsal } from "../tools/run-backfill-rollout-rehearsal.mjs";

test("isolated Wrangler rehearsal publishes then consumes a queued inverse rollback", async () => {
  const receipt = await runBackfillRolloutRehearsal();
  assert.deepEqual(receipt.target, {
    report_schema_version: 17,
    projection_revision: 12,
    timeline_schema_version: 8,
    verifier_release: "isolated-rehearsal-release",
  });
  assert.equal(receipt.forward.candidate_validated, true);
  assert.equal(receipt.forward.published, true);
  assert.equal(receipt.forward.pointer_advanced, true);
  assert.equal(receipt.rollback.queued_consumer_claimed, true);
  assert.equal(receipt.rollback.restored, true);
  assert.equal(receipt.rollback.pointer_restored, true);
  assert.equal(receipt.rollback.catalog_restored, true);
  assert.equal(receipt.rollback.membership_restored, true);
  assert.deepEqual(receipt.rollback.reconciliation_wake_groups, ["new-group", "old-group"]);
  assert.equal(receipt.immutable_evidence.projection_version_count, 2);
  assert.equal(receipt.immutable_evidence.r2_object_count, 4);
  assert.equal(receipt.immutable_evidence.all_digests_match_keys, true);
  assert.equal(receipt.immutable_evidence.foreign_key_violation_count, 0);
});
