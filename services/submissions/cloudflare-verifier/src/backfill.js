import {
  BACKFILL_SOURCE_SCHEMA_VERSION, BACKFILL_TARGET_SCHEMA_VERSION,
  catalogEntry, compatibleProfileName, isSchema12BackfillCandidate,
  runOneShotVerifier, sameChunkCommitments, validateBackfillOutput,
} from "./core.js";

const REPORT_ID = /^rpt_[a-f0-9]{32}$/;
const UPLOAD_ID = /^up_[a-f0-9]{32}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const LEASE_MILLIS = 15 * 60 * 1000;
export const PROJECTION_BACKFILL_PAUSE_CODE = "migration_paused_v7";
export const PROJECTION_BACKFILL_PAUSE_DETAIL =
  "projection publication is staged for schema 17 / projection 11 / timeline 7 but remains paused pending an explicit operator rollout";
export const PROJECTION_BACKFILL_MIGRATION_PAUSED = true;

async function first(env, sql, ...values) {
  return env.RLOGS_DB.prepare(sql).bind(...values).first();
}

async function all(env, sql, ...values) {
  const result = await env.RLOGS_DB.prepare(sql).bind(...values).all();
  return result.results ?? [];
}

async function sha256(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (value) => value.toString(16).padStart(2, "0")).join("");
}

function trimDetail(value) {
  return String(value?.message ?? value).slice(0, 2000);
}

async function markJob(env, jobId, state, code, detail) {
  const now = Date.now();
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_jobs SET state=?2,
    failure_code=?3,failure_detail=?4,updated_unix_millis=?5,
    completed_unix_millis=CASE WHEN ?2 IN ('published','skipped','rejected','superseded') THEN ?5 ELSE NULL END
    WHERE job_id=?1`).bind(jobId, state, code, detail ? trimDetail(detail) : null, now).run();
}

export async function committedProjection(env, row) {
  if (!REPORT_ID.test(String(row.report_id ?? "")) || !UPLOAD_ID.test(String(row.upload_id ?? "")) ||
      !DIGEST.test(String(row.artifact_sha256 ?? "")) || !DIGEST.test(String(row.projection_sha256 ?? "")) ||
      row.report_id !== `rpt_${row.artifact_sha256.slice(0, 32)}` ||
      row.upload_id !== `up_${row.artifact_sha256.slice(0, 32)}` ||
      row.projection_object_key !== `reports/${row.report_id}/projection-${row.projection_sha256}.json`) {
    throw new TypeError("report row failed content-addressed identity validation");
  }
  const object = await env.RLOGS_ARTIFACTS.get(row.projection_object_key);
  if (!object) throw new TypeError("current projection object is missing");
  const bytes = new Uint8Array(await object.arrayBuffer());
  if (await sha256(bytes) !== row.projection_sha256) {
    throw new TypeError("current projection digest does not match D1");
  }
  try {
    return { bytes, report: JSON.parse(new TextDecoder().decode(bytes)) };
  } catch {
    throw new TypeError("current projection is not valid JSON");
  }
}

export function parseRetainedManifest(value) {
  try {
    const manifest = JSON.parse(value);
    if (manifest == null || typeof manifest !== "object" || Array.isArray(manifest)) throw new Error();
    return manifest;
  } catch {
    throw new TypeError("retained upload manifest is not valid JSON metadata");
  }
}

async function replayRequest(env, batch, row, original) {
  const manifest = parseRetainedManifest(row.manifest_json);
  if ((manifest?.metadata?.purpose ?? "combat_run") !== "combat_run") {
    throw new TypeError("only combat-run uploads can be projection-backfilled");
  }
  const chunks = (await all(env, `SELECT sequence,sha256,byte_length,object_key
    FROM upload_chunks WHERE upload_id=?1 ORDER BY sequence`, row.upload_id)).map((chunk) => ({
    sequence: Number(chunk.sequence), sha256: String(chunk.sha256),
    byte_length: Number(chunk.byte_length), object_key: String(chunk.object_key),
  }));
  const expectedChunks = chunks.map((chunk, sequence) => ({
    sequence, sha256: chunk.sha256, byte_length: chunk.byte_length,
    object_key: `uploads/${row.upload_id}/chunks/${String(sequence).padStart(8, "0")}-${chunk.sha256}.bin`,
  }));
  if (chunks.length === 0 || !sameChunkCommitments(chunks, expectedChunks)) {
    throw new TypeError("retained upload chunks failed their immutable commitment layout");
  }
  const profiles = await all(env, `SELECT character_id,deployment_id,region_id,public_projection_json
    FROM profiles WHERE submitter_id=?1`, row.submitter_id);
  const names = {};
  for (const profile of profiles) {
    const name = compatibleProfileName(profile, manifest);
    if (name) names[profile.character_id] = name;
  }
  const wakeup = {
    schema_version: 1, upload_id: row.upload_id, artifact_sha256: row.artifact_sha256,
    expected_report_id: row.report_id, chunks,
  };
  const request = {
    ...wakeup,
    created_unix_millis: Number(row.created_unix_millis), manifest,
    submission_provenance: { submitter_id: row.submitter_id, authentication: "discord_device_token" },
    verified_names_by_character: names,
  };
  const container = env.RLOGS_VERIFIER_CONTAINER.getByName(`backfill-${row.report_id}-${batch.batch_id}`);
  const { response, result } = await runOneShotVerifier(container,
    new Request("http://container/internal/v1/verify", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(request),
    }));
  if (!response.ok) {
    const error = new Error(result?.error ?? `verifier returned HTTP ${response.status}`);
    error.permanent = response.status === 422;
    throw error;
  }
  // Visibility belongs to the existing D1 publication decision, never to the
  // historical manifest or to the replay output.
  result.report.visibility = row.visibility;
  if (!validateBackfillOutput(result, wakeup, original, row, batch.target_verifier_release)) {
    throw new TypeError("backfill verifier output failed identity and evidence-preservation validation");
  }
  return { result, names, gameId: manifest.metadata.game_plugin_id };
}

export async function persistReplay(env, batch, row, original, jobId, result, names, gameId) {
  const projectionBytes = new TextEncoder().encode(JSON.stringify(result.report));
  const membershipBytes = new TextEncoder().encode(JSON.stringify(result.membership));
  const projectionSha256 = await sha256(projectionBytes);
  const membershipSha256 = await sha256(membershipBytes);
  const projectionKey = `reports/${row.report_id}/projection-${projectionSha256}.json`;
  const membershipKey = `private/reports/${row.report_id}/membership-${membershipSha256}.json`;
  await Promise.all([
    env.RLOGS_ARTIFACTS.put(projectionKey, projectionBytes, { httpMetadata: { contentType: "application/json" } }),
    env.RLOGS_ARTIFACTS.put(membershipKey, membershipBytes, { httpMetadata: { contentType: "application/json" } }),
  ]);

  const now = Date.now();
  const sourceGuard = `EXISTS (SELECT 1 FROM reports current WHERE current.report_id=?1
    AND current.upload_id=?2 AND current.artifact_sha256=?3 AND current.visibility='public'
    AND current.projection_sha256=?4 AND current.projection_object_key=?5)`;
  const candidateGuard = `EXISTS (SELECT 1 FROM reports current WHERE current.report_id=?1
    AND current.projection_sha256=?2 AND current.projection_object_key=?3
    AND current.upload_id=?4 AND current.artifact_sha256=?5 AND current.visibility='public')`;
  const statements = [
    env.RLOGS_DB.prepare(`INSERT INTO report_projection_versions
      (report_id,projection_sha256,projection_object_key,schema_version,verifier_release,
       artifact_sha256,backfill_job_id,created_unix_millis)
      SELECT ?1,?4,?5,?6,?7,?3,NULL,?8 WHERE ${sourceGuard}
      ON CONFLICT(report_id,projection_sha256) DO NOTHING`).bind(
      row.report_id, row.upload_id, row.artifact_sha256, row.projection_sha256,
      row.projection_object_key, original.schema_version, row.verifier_release,
      Number(row.verified_unix_millis),
    ),
    env.RLOGS_DB.prepare(`INSERT INTO report_projection_versions
      (report_id,projection_sha256,projection_object_key,schema_version,verifier_release,
       artifact_sha256,backfill_job_id,created_unix_millis)
      SELECT ?1,?6,?7,?8,?9,?3,?10,?11 WHERE ${sourceGuard}
      ON CONFLICT(report_id,projection_sha256) DO NOTHING`).bind(
      row.report_id, row.upload_id, row.artifact_sha256, row.projection_sha256,
      row.projection_object_key, projectionSha256, projectionKey,
      result.report.schema_version, batch.target_verifier_release, jobId, now,
    ),
    // D1 batch() is one transaction.  Advance the guarded pointer first; every
    // catalog/membership mutation below is conditioned on seeing that exact
    // candidate pointer within the same transaction.  A lost source guard
    // therefore cannot delete the prior indexes.
    env.RLOGS_DB.prepare(`UPDATE reports SET run_group_id=?6,
      verifier_release=?7,projection_sha256=?8,projection_object_key=?9
      WHERE report_id=?1 AND upload_id=?2 AND artifact_sha256=?3 AND visibility='public'
        AND projection_sha256=?4 AND projection_object_key=?5`).bind(
      row.report_id, row.upload_id, row.artifact_sha256, row.projection_sha256,
      row.projection_object_key, result.report.runs[0].run_group_id,
      batch.target_verifier_release, projectionSha256, projectionKey,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_runs WHERE report_id=?1 AND ${candidateGuard}`).bind(
      row.report_id, projectionSha256, projectionKey, row.upload_id, row.artifact_sha256,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_memberships WHERE report_id=?1 AND ${candidateGuard}`).bind(
      row.report_id, projectionSha256, projectionKey, row.upload_id, row.artifact_sha256,
    ),
  ];
  for (const run of result.report.runs) {
    const entry = catalogEntry(result.report, run);
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_runs
      (report_id,run_index,run_group_id,catalog_entry_json,created_unix_millis)
      SELECT ?1,?6,?7,?8,?9 WHERE ${candidateGuard}`).bind(
      row.report_id, projectionSha256, projectionKey, row.upload_id,
      row.artifact_sha256, run.run_index, entry.run_group_id,
      JSON.stringify(entry), result.report.created_unix_millis,
    ));
  }
  const actorByCharacter = new Map(Object.entries(result.membership.character_by_actor ?? {})
    .map(([actor, character]) => [character, actor]));
  const characters = new Set(result.membership.runs.flatMap((run) => run.character_ids));
  for (const characterId of characters) {
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_memberships
      (report_id,game_id,character_id,actor_id,player_name)
      SELECT ?1,?6,?7,?8,?9 WHERE ${candidateGuard}`).bind(
      row.report_id, projectionSha256, projectionKey, row.upload_id,
      row.artifact_sha256, gameId, characterId,
      actorByCharacter.get(characterId) ?? null, names[characterId] ?? null,
    ));
  }
  statements.push(env.RLOGS_DB.prepare(`UPDATE projection_backfill_jobs SET state='published',
    candidate_projection_sha256=?2,candidate_projection_object_key=?3,
    candidate_membership_sha256=?4,candidate_membership_object_key=?5,
    updated_unix_millis=?6,completed_unix_millis=?6
    WHERE job_id=?1 AND EXISTS (SELECT 1 FROM reports WHERE report_id=?7
      AND projection_sha256=?2 AND projection_object_key=?3)`).bind(
    jobId, projectionSha256, projectionKey, membershipSha256, membershipKey, now, row.report_id,
  ));
  try {
    await env.RLOGS_DB.batch(statements);
  } catch (cause) {
    // D1 batch delivery can be uncertain.  Treat the guarded current pointer as
    // authoritative before scheduling a second replay or downgrading the job.
    const committed = await first(env, `SELECT projection_sha256 FROM reports
      WHERE report_id=?1 AND projection_sha256=?2 AND projection_object_key=?3`,
    row.report_id, projectionSha256, projectionKey).catch(() => null);
    if (!committed) throw cause;
  }
  const published = await first(env, `SELECT projection_sha256 FROM reports
    WHERE report_id=?1 AND projection_sha256=?2 AND projection_object_key=?3`,
  row.report_id, projectionSha256, projectionKey);
  if (!published) {
    await markJob(env, jobId, "superseded", "projection_changed",
      "the report projection changed before atomic publication");
    return { published: false, runGroupIds: [] };
  }
  return {
    published: true,
    runGroupIds: [...new Set([
      ...(original.runs ?? []).map((run) => run.run_group_id),
      ...result.report.runs.map((run) => run.run_group_id),
    ].filter(Boolean))],
  };
}

async function claimBatch(env) {
  const release = String(env.VERIFIER_RELEASE ?? "");
  if (!release) return null;
  const now = Date.now();
  const candidate = await first(env, `SELECT * FROM projection_backfill_batches
    WHERE target_verifier_release=?1 AND (state IN ('pending','retryable_failure')
      OR (state='running' AND updated_unix_millis<=?2))
    ORDER BY created_unix_millis,batch_id LIMIT 1`, release, now - LEASE_MILLIS);
  if (!candidate) return null;
  const lease = crypto.randomUUID();
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET state='running',
    lease_token=?2,updated_unix_millis=?3,failure_code=NULL,failure_detail=NULL
    WHERE batch_id=?1 AND target_verifier_release=?4 AND (state IN ('pending','retryable_failure')
      OR (state='running' AND updated_unix_millis<=?5))`).bind(
    candidate.batch_id, lease, now, release, now - LEASE_MILLIS,
  ).run();
  return first(env, `SELECT * FROM projection_backfill_batches
    WHERE batch_id=?1 AND lease_token=?2 AND state='running'`, candidate.batch_id, lease);
}

async function updateBatchProgress(env, batch, row, delta) {
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET cursor_report_id=?2,
    inspected_count=inspected_count+1,eligible_count=eligible_count+?3,
    published_count=published_count+?4,skipped_count=skipped_count+?5,
    consecutive_retry_count=0,updated_unix_millis=?6
    WHERE batch_id=?1 AND lease_token=?7 AND state='running'`).bind(
    batch.batch_id, row.report_id, delta.eligible, delta.published, delta.skipped,
    Date.now(), batch.lease_token,
  ).run();
}

async function retryBatch(env, batch, code, cause) {
  const exhausted = Number(batch.consecutive_retry_count ?? 0) >= 2;
  const now = Date.now();
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET state=?2,
    consecutive_retry_count=consecutive_retry_count+1,failure_code=?3,failure_detail=?4,
    completed_unix_millis=CASE WHEN ?2='rejected' THEN ?5 ELSE NULL END,
    updated_unix_millis=?5 WHERE batch_id=?1 AND lease_token=?6`).bind(
    batch.batch_id, exhausted ? "rejected" : "retryable_failure",
    exhausted ? "retry_exhausted" : code, trimDetail(cause), now, batch.lease_token,
  ).run();
  return exhausted;
}

export async function runProjectionBackfillBatch(env, context, reconcileRunGroup) {
  const batch = await claimBatch(env);
  if (!batch) return { claimed: false };
  if (Number(batch.source_schema_version) !== BACKFILL_SOURCE_SCHEMA_VERSION ||
      Number(batch.target_schema_version) !== BACKFILL_TARGET_SCHEMA_VERSION ||
      Number(batch.maximum_reports) < 1 || Number(batch.maximum_reports) > 25 ||
      ![0, 1].includes(Number(batch.dry_run))) {
    await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET state='rejected',
      failure_code='invalid_request',failure_detail='backfill request violates fixed schema or batch bounds',
      completed_unix_millis=?2,updated_unix_millis=?2 WHERE batch_id=?1 AND lease_token=?3`)
      .bind(batch.batch_id, Date.now(), batch.lease_token).run();
    return { claimed: true, rejected: true };
  }
  // Evidence-only dry runs are safe while publication remains disabled. They
  // retain bounded eligibility audit rows but never invoke replay, write a
  // candidate object, advance a report pointer, or wake reconciliation.
  if (PROJECTION_BACKFILL_MIGRATION_PAUSED && Number(batch.dry_run) !== 1) {
    const pausedAt = Date.now();
    await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET state='rejected',
      lease_token=NULL,failure_code=?2,failure_detail=?3,
      completed_unix_millis=?4,updated_unix_millis=?4
      WHERE batch_id=?1 AND lease_token=?5 AND state='running'`).bind(
      batch.batch_id, PROJECTION_BACKFILL_PAUSE_CODE, PROJECTION_BACKFILL_PAUSE_DETAIL,
      pausedAt, batch.lease_token,
    ).run();
    return {
      claimed: true,
      rejected: true,
      permanent: true,
      code: PROJECTION_BACKFILL_PAUSE_CODE,
    };
  }
  const remaining = Number(batch.maximum_reports) - Number(batch.inspected_count);
  const cursor = batch.cursor_report_id || batch.after_report_id || "";
  const reconciliationGroups = new Set();
  const wakeReconciliations = () => {
    for (const runGroupId of reconciliationGroups) {
      const task = reconcileRunGroup(env, runGroupId).catch((cause) =>
        console.error("rLogs backfill reconciliation wake-up failed", runGroupId, cause));
      if (context?.waitUntil) context.waitUntil(task);
    }
  };
  // One immutable artifact per scheduled turn keeps container and Worker wall
  // time bounded even when an operator requests a larger resumable batch.
  const rows = remaining > 0 ? await all(env, `SELECT r.*,u.submitter_id,u.manifest_json,
      u.created_unix_millis FROM reports r JOIN upload_sessions u ON u.upload_id=r.upload_id
    WHERE r.visibility='public' AND r.verification_tier='replayed' AND r.report_id>?1
    ORDER BY r.report_id LIMIT 1`, cursor) : [];
  for (const row of rows) {
    let original;
    try {
      ({ report: original } = await committedProjection(env, row));
    } catch (cause) {
      if (cause instanceof TypeError) {
        await updateBatchProgress(env, batch, row, { eligible: 0, published: 0, skipped: 1 });
        continue;
      }
      const exhausted = await retryBatch(env, batch, "projection_unavailable", cause);
      return { claimed: true, retryable: !exhausted, rejected: exhausted };
    }
    if (!isSchema12BackfillCandidate(original, row)) {
      await updateBatchProgress(env, batch, row, { eligible: 0, published: 0, skipped: 1 });
      continue;
    }
    const jobId = `bfj_${(await sha256(new TextEncoder().encode(
      `${batch.batch_id}:${row.report_id}:${row.projection_sha256}:${batch.target_verifier_release}`))).slice(0, 32)}`;
    const now = Date.now();
    await env.RLOGS_DB.prepare(`INSERT INTO projection_backfill_jobs
      (job_id,batch_id,report_id,upload_id,artifact_sha256,source_projection_sha256,
       source_projection_object_key,target_verifier_release,state,created_unix_millis,updated_unix_millis)
      VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)
      ON CONFLICT(batch_id,report_id) DO NOTHING`).bind(
      jobId, batch.batch_id, row.report_id, row.upload_id, row.artifact_sha256,
      row.projection_sha256, row.projection_object_key, batch.target_verifier_release,
      Number(batch.dry_run) === 1 ? "planned" : "running", now,
    ).run();
    const existingJob = await first(env, `SELECT state FROM projection_backfill_jobs
      WHERE job_id=?1 AND batch_id=?2 AND report_id=?3`, jobId, batch.batch_id, row.report_id);
    if (["published", "skipped", "rejected", "superseded"].includes(existingJob?.state)) {
      await updateBatchProgress(env, batch, row, {
        eligible: 1, published: existingJob.state === "published" ? 1 : 0,
        skipped: existingJob.state === "published" ? 0 : 1,
      });
      continue;
    }
    if (Number(batch.dry_run) === 1) {
      await updateBatchProgress(env, batch, row, { eligible: 1, published: 0, skipped: 0 });
      continue;
    }
    try {
      const { result, names, gameId } = await replayRequest(env, batch, row, original);
      const persisted = await persistReplay(env, batch, row, original, jobId, result, names, gameId);
      await updateBatchProgress(env, batch, row, {
        eligible: 1, published: persisted.published ? 1 : 0, skipped: persisted.published ? 0 : 1,
      });
      for (const runGroupId of persisted.runGroupIds) reconciliationGroups.add(runGroupId);
    } catch (cause) {
      if (cause?.permanent || cause instanceof TypeError) {
        await markJob(env, jobId, "rejected", "replay_rejected", cause);
        await updateBatchProgress(env, batch, row, { eligible: 1, published: 0, skipped: 1 });
        continue;
      }
      await markJob(env, jobId, "retryable_failure", "replay_unavailable", cause);
      const exhausted = await retryBatch(env, batch, "replay_unavailable", cause);
      wakeReconciliations();
      return { claimed: true, retryable: !exhausted, rejected: exhausted };
    }
  }
  const completed = rows.length === 0 || Number(batch.inspected_count) + rows.length >= Number(batch.maximum_reports);
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_batches SET state=?2,
    lease_token=NULL,completed_unix_millis=CASE WHEN ?2='completed' THEN ?3 ELSE NULL END,
    updated_unix_millis=?3 WHERE batch_id=?1 AND lease_token=?4 AND state='running'`).bind(
    batch.batch_id, completed ? "completed" : "pending", Date.now(), batch.lease_token,
  ).run();
  wakeReconciliations();
  return { claimed: true, completed, inspected: rows.length };
}
