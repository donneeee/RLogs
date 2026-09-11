import {
  PROJECTION_BACKFILL_MIGRATION_PAUSED,
  PROJECTION_BACKFILL_PAUSE_CODE,
  persistReplay,
  runProjectionBackfillRollback,
} from "../src/backfill.js";
import {
  BACKFILL_TARGET_PROJECTION_REVISION,
  BACKFILL_TARGET_SCHEMA_VERSION,
  BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION,
  validateBackfillOutput,
} from "../src/core.js";

const GAME_ID = "app.rlogs.game.blue-protocol-star-resonance";
const TARGET_RELEASE = "isolated-rehearsal-release";

async function sha256(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (value) => value.toString(16).padStart(2, "0")).join("");
}

async function run(db, sql, ...values) {
  return db.prepare(sql).bind(...values).run();
}

async function first(db, sql, ...values) {
  return db.prepare(sql).bind(...values).first();
}

async function all(db, sql, ...values) {
  const result = await db.prepare(sql).bind(...values).all();
  return result.results ?? [];
}

function candidateTimeline(reportId) {
  return {
    schema_version: BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION,
    source: "single_report",
    canonical_report_id: reportId,
    contributing_report_ids: [reportId],
    duration_micros: 2_000_000,
    participant_tracks: [
      { actor_id: "actor-1", canonical_participant_index: 0, series_point_count: 1,
        omitted_skill_uses: 0 },
      { actor_id: "actor-2", canonical_participant_index: 1, series_point_count: 2,
        omitted_skill_uses: 1 },
    ],
    clock_anchor: {
      at_micros: 0, game_time_millis: 1_000,
      source_report_id: reportId, event_sequence: 4,
    },
    skill_uses: [{
      actor_id: "actor-1", at_micros: 500_000, action_id: "2233",
      action_instance_id: "91", state: "started", action_kind: "skill",
      evidence: [{ source_report_id: reportId, event_sequence: 12,
        game_time_millis: 1_500, kind: "exact_wire_cast_start" }],
      omitted_evidence: 0,
    }],
    hostile_source_actor_ids: ["hostile-1"],
    hostile_casts: [{
      source_actor_id: "hostile-1", hostility_evidence: "participant_outgoing_target",
      target_actor_id: "actor-1", at_micros: 600_000, action_id: "7001",
      action_instance_id: "92", state: "started",
      evidence: [{ source_report_id: reportId, event_sequence: 13,
        game_time_millis: 1_600, kind: "exact_wire_cast_start" }],
      omitted_evidence: 0,
    }],
    status_spans: [{
      target_actor_id: "actor-2", source_actor_id: "actor-1", effect_id: "3003052",
      instance_id: "93", start_micros: 700_000, end_micros: 900_000,
      terminal_state: "removed",
      evidence: [{ source_report_id: reportId, applied_event_sequence: 14,
        terminal_event_sequence: 15, applied_game_time_millis: 1_700,
        terminal_game_time_millis: 1_900 }],
      omitted_evidence: 0,
    }],
    omitted: {
      skill_uses: 1, participant_tracks: 0, series_points: 0,
      hostile_casts: 0, status_spans: 0,
    },
  };
}

async function contentDigest(bucket, key) {
  const object = await bucket.get(key);
  if (!object) return null;
  return sha256(new Uint8Array(await object.arrayBuffer()));
}

async function exercise(env) {
  const artifactSha256 = "a".repeat(64);
  const reportId = `rpt_${artifactSha256.slice(0, 32)}`;
  const uploadId = `up_${artifactSha256.slice(0, 32)}`;
  const sourceReport = {
    schema_version: 12,
    projection_revision: 6,
    report_id: reportId,
    visibility: "public",
    game_plugin_id: GAME_ID,
    deployment_id: "global",
    region_id: "north-america",
    world_id: "rehearsal-world",
    client_build: "isolated-build",
    protocol_pack_digest: `sha256:${"b".repeat(64)}`,
    created_unix_millis: 42,
    verification: {
      tier: "replayed", artifact_sha256: artifactSha256,
      canonical_content_sha256: "canonical-rehearsal-content",
      event_count: 3, privacy_policy_digest: "privacy-rehearsal-policy",
    },
    submission_provenance: { submitter_id: "usr_rehearsal" },
    runs: [{ run_index: 0, run_group_id: "old-group" }],
  };
  const sourceBytes = new TextEncoder().encode(JSON.stringify(sourceReport));
  const sourceSha256 = await sha256(sourceBytes);
  const sourceObjectKey = `reports/${reportId}/projection-${sourceSha256}.json`;
  const row = {
    report_id: reportId, upload_id: uploadId, artifact_sha256: artifactSha256,
    projection_sha256: sourceSha256, projection_object_key: sourceObjectKey,
    visibility: "public", run_group_id: "old-group", verifier_release: "old-release",
    verified_unix_millis: 41, verification_tier: "replayed", submitter_id: "usr_rehearsal",
  };
  const candidateReport = {
    ...sourceReport,
    schema_version: BACKFILL_TARGET_SCHEMA_VERSION,
    projection_revision: BACKFILL_TARGET_PROJECTION_REVISION,
    runs: [{
      run_index: 0, run_group_id: "new-group", terminal_state: "completed",
      participants: [], timeline: candidateTimeline(reportId),
    }],
  };
  const result = {
    schema_version: 1,
    report: candidateReport,
    membership: {
      report_id: reportId, artifact_sha256: artifactSha256,
      character_by_actor: { "actor-7": "new-character" },
      runs: [{ run_index: 0, character_ids: ["new-character"] }],
    },
  };
  const wakeup = {
    schema_version: 1, upload_id: uploadId, artifact_sha256: artifactSha256,
    expected_report_id: reportId, chunks: [],
  };
  if (!validateBackfillOutput(result, wakeup, sourceReport, row, TARGET_RELEASE)) {
    throw new Error("synthetic current-tuple candidate failed the production backfill validator");
  }

  await run(env.RLOGS_DB, `INSERT INTO accounts
    (submitter_id,account_id,username,discord_user_hash,discord_username,
     created_unix_millis,updated_unix_millis,publish_verified_parses)
    VALUES (?1,?2,?3,?4,?5,?6,?6,1)`,
  "usr_rehearsal", "acct_rehearsal", "rehearsal", "hash_rehearsal", "rehearsal", 1);
  await run(env.RLOGS_DB, `INSERT INTO upload_sessions
    (upload_id,artifact_sha256,submitter_id,device_id,state,byte_length,chunk_size,chunk_count,
     manifest_json,artifact_object_key,created_unix_millis,updated_unix_millis,finalized_unix_millis)
    VALUES (?1,?2,?3,?4,'accepted',1,1,1,'{}',?5,1,1,1)`,
  uploadId, artifactSha256, "usr_rehearsal", "device_rehearsal", `uploads/${uploadId}/artifact.rlog`);
  await run(env.RLOGS_DB, `INSERT INTO reports
    (report_id,upload_id,run_group_id,visibility,verification_tier,verifier_release,game_build,
     protocol_pack_digest,artifact_sha256,projection_sha256,projection_object_key,
     verified_unix_millis,published_unix_millis)
    VALUES (?1,?2,'old-group','public','replayed','old-release','isolated-build',?3,?4,?5,?6,41,41)`,
  reportId, uploadId, sourceReport.protocol_pack_digest, artifactSha256, sourceSha256, sourceObjectKey);
  await run(env.RLOGS_DB, `INSERT INTO report_runs
    (report_id,run_index,run_group_id,catalog_entry_json,created_unix_millis)
    VALUES (?1,0,'old-group','{}',42)`, reportId);
  await run(env.RLOGS_DB, `INSERT INTO report_memberships
    (report_id,game_id,character_id,actor_id,player_name)
    VALUES (?1,?2,'old-character','actor-1','Old')`, reportId, GAME_ID);
  await run(env.RLOGS_DB, `INSERT INTO projection_backfill_batches
    (batch_id,requested_by,workflow_run_url,target_verifier_release,source_schema_version,
     target_schema_version,maximum_reports,dry_run,state,lease_token,created_unix_millis,updated_unix_millis)
    VALUES ('bf_rehearsal','isolated-harness','local://rehearsal',?1,12,17,1,0,'running',
      'forward-lease',1,1)`, TARGET_RELEASE);
  await run(env.RLOGS_DB, `INSERT INTO projection_backfill_jobs
    (job_id,batch_id,report_id,upload_id,artifact_sha256,source_projection_sha256,
     source_projection_object_key,target_verifier_release,state,created_unix_millis,updated_unix_millis)
    VALUES ('bfj_rehearsal','bf_rehearsal',?1,?2,?3,?4,?5,?6,'running',1,1)`,
  reportId, uploadId, artifactSha256, sourceSha256, sourceObjectKey, TARGET_RELEASE);
  await env.RLOGS_ARTIFACTS.put(sourceObjectKey, sourceBytes,
    { httpMetadata: { contentType: "application/json" } });

  const forward = await persistReplay(env, { target_verifier_release: TARGET_RELEASE }, row,
    sourceReport, "bfj_rehearsal", result, { "new-character": "New" }, GAME_ID);
  if (!forward.published) throw new Error("isolated forward publication lost its guarded source pointer");
  const publishedJob = await first(env.RLOGS_DB,
    "SELECT * FROM projection_backfill_jobs WHERE job_id='bfj_rehearsal'");
  const publishedPointer = await first(env.RLOGS_DB,
    "SELECT projection_sha256,projection_object_key,run_group_id FROM reports WHERE report_id=?1", reportId);

  const rollbackId = `bfr_${"d".repeat(32)}`;
  await run(env.RLOGS_DB, `INSERT INTO projection_backfill_rollbacks
    (rollback_id,requested_by,workflow_run_url,job_id,expected_candidate_projection_sha256,
     expected_candidate_projection_object_key,target_source_projection_sha256,
     target_source_projection_object_key,state,created_unix_millis,updated_unix_millis)
    VALUES (?1,'isolated-harness','local://rehearsal','bfj_rehearsal',?2,?3,?4,?5,'pending',2,2)`,
  rollbackId, publishedJob.candidate_projection_sha256,
  publishedJob.candidate_projection_object_key, sourceSha256, sourceObjectKey);

  const wakeGroups = [];
  const pendingWakes = [];
  const rollback = await runProjectionBackfillRollback(env, {
    waitUntil(task) { pendingWakes.push(task); },
  }, async (_environment, runGroupId) => { wakeGroups.push(runGroupId); });
  await Promise.all(pendingWakes);

  const [restoredPointer, restoredRuns, restoredMemberships, rollbackRow, versions,
    foreignKeys, objects] = await Promise.all([
    first(env.RLOGS_DB, "SELECT projection_sha256,projection_object_key,run_group_id,verifier_release FROM reports WHERE report_id=?1", reportId),
    all(env.RLOGS_DB, "SELECT run_index,run_group_id,catalog_entry_json FROM report_runs WHERE report_id=?1 ORDER BY run_index", reportId),
    all(env.RLOGS_DB, "SELECT game_id,character_id,actor_id,player_name FROM report_memberships WHERE report_id=?1 ORDER BY game_id,character_id", reportId),
    first(env.RLOGS_DB, "SELECT state,attempt_count,failure_code FROM projection_backfill_rollbacks WHERE rollback_id=?1", rollbackId),
    all(env.RLOGS_DB, "SELECT projection_sha256,projection_object_key,backfill_job_id FROM report_projection_versions WHERE report_id=?1 ORDER BY projection_sha256", reportId),
    all(env.RLOGS_DB, "PRAGMA foreign_key_check"),
    env.RLOGS_ARTIFACTS.list({ prefix: "" }),
  ]);
  const retained = {
    source_projection: await contentDigest(env.RLOGS_ARTIFACTS, sourceObjectKey),
    candidate_projection: await contentDigest(env.RLOGS_ARTIFACTS,
      publishedJob.candidate_projection_object_key),
    candidate_membership: await contentDigest(env.RLOGS_ARTIFACTS,
      publishedJob.candidate_membership_object_key),
    source_index_snapshot: await contentDigest(env.RLOGS_ARTIFACTS,
      publishedJob.source_indexes_object_key),
  };
  const receipt = {
    schema_version: 1,
    environment: "isolated-wrangler-miniflare",
    production_bindings_present: false,
    production_publication_pause: {
      active: PROJECTION_BACKFILL_MIGRATION_PAUSED,
      code: PROJECTION_BACKFILL_PAUSE_CODE,
    },
    target: {
      report_schema_version: BACKFILL_TARGET_SCHEMA_VERSION,
      projection_revision: BACKFILL_TARGET_PROJECTION_REVISION,
      timeline_schema_version: BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION,
      verifier_release: TARGET_RELEASE,
    },
    forward: {
      candidate_validated: true,
      published: forward.published,
      pointer_advanced: publishedPointer.projection_sha256 === publishedJob.candidate_projection_sha256,
      job_state: publishedJob.state,
      source_indexes_sha256: publishedJob.source_indexes_sha256,
      candidate_projection_sha256: publishedJob.candidate_projection_sha256,
      candidate_membership_sha256: publishedJob.candidate_membership_sha256,
    },
    rollback: {
      queued_consumer_claimed: rollback.claimed,
      restored: rollback.restored,
      state: rollbackRow.state,
      attempt_count: Number(rollbackRow.attempt_count),
      pointer_restored: restoredPointer.projection_sha256 === sourceSha256 &&
        restoredPointer.projection_object_key === sourceObjectKey,
      catalog_restored: restoredRuns.length === 1 && restoredRuns[0].run_group_id === "old-group" &&
        restoredRuns[0].catalog_entry_json === "{}",
      membership_restored: restoredMemberships.length === 1 &&
        restoredMemberships[0].character_id === "old-character" &&
        restoredMemberships[0].player_name === "Old",
      reconciliation_wake_groups: wakeGroups.sort(),
    },
    immutable_evidence: {
      projection_version_count: versions.length,
      r2_object_count: objects.objects.length,
      all_digests_match_keys: retained.source_projection === sourceSha256 &&
        retained.candidate_projection === publishedJob.candidate_projection_sha256 &&
        retained.candidate_membership === publishedJob.candidate_membership_sha256 &&
        retained.source_index_snapshot === publishedJob.source_indexes_sha256,
      foreign_key_violation_count: foreignKeys.length,
    },
  };
  const valid = receipt.production_publication_pause.active === true &&
    receipt.forward.candidate_validated && receipt.forward.published &&
    receipt.forward.pointer_advanced &&
    receipt.forward.job_state === "published" && receipt.rollback.queued_consumer_claimed &&
    receipt.rollback.restored && receipt.rollback.state === "restored" &&
    receipt.rollback.attempt_count === 1 && receipt.rollback.pointer_restored &&
    receipt.rollback.catalog_restored && receipt.rollback.membership_restored &&
    JSON.stringify(receipt.rollback.reconciliation_wake_groups) ===
      JSON.stringify(["new-group", "old-group"]) &&
    receipt.immutable_evidence.projection_version_count === 2 &&
    receipt.immutable_evidence.r2_object_count === 4 &&
    receipt.immutable_evidence.all_digests_match_keys &&
    receipt.immutable_evidence.foreign_key_violation_count === 0;
  if (!valid) throw new Error(`isolated rollout receipt failed: ${JSON.stringify(receipt)}`);
  const receiptBytes = new TextEncoder().encode(JSON.stringify(receipt));
  return { ...receipt, receipt_sha256: await sha256(receiptBytes) };
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method !== "POST" || url.pathname !== "/exercise") {
      return new Response("Not found", { status: 404 });
    }
    try {
      return Response.json(await exercise(env));
    } catch (cause) {
      return Response.json({ error: String(cause?.stack ?? cause) }, { status: 500 });
    }
  },
};
