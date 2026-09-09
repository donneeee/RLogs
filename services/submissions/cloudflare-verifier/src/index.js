import { Container, ContainerProxy } from "@cloudflare/containers";
import {
  catalogEntry, compatibleProfileName, reconcileCatalogEntry, runOneShotVerifier,
  sameChunkCommitments, validateOutput, validateReconciliationOutput, validateTrainingOutput,
  validateWakeup,
} from "./core.js";

// Cloudflare requires this named export whenever a Container class installs
// outbound handlers. It keeps the R2 binding in the trusted Worker while the
// Rust verifier reads chunks through the virtual rlogs-artifacts.r2 host.
export { ContainerProxy };

function json(value, status = 200) {
  return Response.json(value, {
    status,
    headers: { "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" },
  });
}

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

const RUN_GROUP_ID = /^[A-Za-z0-9_-]{1,96}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const REPORT_ID = /^rpt_[a-f0-9]{32}$/;

async function reconciliationSources(env, runGroupId) {
  const rows = await all(env, `SELECT r.report_id, r.upload_id, rr.run_index,
      r.artifact_sha256, r.projection_sha256, r.projection_object_key, u.submitter_id
    FROM report_runs rr
    JOIN reports r ON r.report_id=rr.report_id
    JOIN upload_sessions u ON u.upload_id=r.upload_id
    WHERE rr.run_group_id=?1 AND r.visibility='public' AND r.verification_tier='replayed'
    ORDER BY r.report_id, rr.run_index`, runGroupId);
  if (rows.length < 2 || rows.length > 64) return null;
  const seen = new Set();
  const sources = [];
  for (const row of rows) {
    const reportId = String(row.report_id ?? "");
    const uploadId = String(row.upload_id ?? "");
    const artifactSha256 = String(row.artifact_sha256 ?? "");
    const projectionSha256 = String(row.projection_sha256 ?? "");
    const projectionObjectKey = String(row.projection_object_key ?? "");
    const runIndex = Number(row.run_index);
    if (!REPORT_ID.test(reportId) || !/^up_[a-f0-9]{32}$/.test(uploadId) ||
        reportId !== `rpt_${artifactSha256.slice(0, 32)}` || uploadId !== `up_${artifactSha256.slice(0, 32)}` ||
        !DIGEST.test(artifactSha256) || !DIGEST.test(projectionSha256) ||
        projectionObjectKey !== `reports/${reportId}/projection-${projectionSha256}.json` ||
        !Number.isInteger(runIndex) || runIndex < 0 || seen.has(reportId)) return null;
    seen.add(reportId);
    const [projection, chunks] = await Promise.all([
      env.RLOGS_ARTIFACTS.head(projectionObjectKey),
      all(env, `SELECT sequence, sha256, byte_length, object_key
        FROM upload_chunks WHERE upload_id=?1 ORDER BY sequence`, uploadId),
    ]);
    if (!projection || Number(projection.size) <= 0 || chunks.length === 0) return null;
    const normalizedChunks = chunks.map((chunk, sequence) => ({
      sequence: Number(chunk.sequence), sha256: String(chunk.sha256),
      byte_length: Number(chunk.byte_length), object_key: String(chunk.object_key),
    }));
    if (normalizedChunks.some((chunk, sequence) => chunk.sequence !== sequence ||
        !DIGEST.test(chunk.sha256) || !Number.isSafeInteger(chunk.byte_length) || chunk.byte_length <= 0 ||
        chunk.object_key !== `uploads/${uploadId}/chunks/${String(sequence).padStart(8, "0")}-${chunk.sha256}.bin`)) {
      return null;
    }
    sources.push({
      report_id: reportId, upload_id: uploadId, run_index: runIndex,
      artifact_sha256: artifactSha256,
      projection: { object_key: projectionObjectKey, byte_length: Number(projection.size), sha256: projectionSha256 },
      chunks: normalizedChunks,
      submitter_id: String(row.submitter_id ?? ""),
    });
  }
  return sources;
}

async function sourceSetDigest(runGroupId, sources) {
  const commitment = {
    run_group_id: runGroupId,
    sources: sources.map(({ submitter_id, ...source }) => source),
  };
  return sha256(new TextEncoder().encode(JSON.stringify(commitment)));
}

function currentSourceGuard() {
  return `(SELECT COUNT(*) FROM reconciliation_job_sources js
    JOIN report_runs rr ON rr.report_id=js.report_id AND rr.run_index=js.run_index
    JOIN reports r ON r.report_id=js.report_id
    WHERE js.job_id=?1 AND rr.run_group_id=?2
      AND r.visibility='public' AND r.verification_tier='replayed'
      AND r.projection_sha256=js.projection_sha256
      AND r.projection_object_key=js.projection_object_key)=?3
    AND (SELECT COUNT(*) FROM report_runs rr JOIN reports r ON r.report_id=rr.report_id
      WHERE rr.run_group_id=?2
        AND r.visibility='public' AND r.verification_tier='replayed')=?3`;
}

async function reconciliationFailure(env, jobId, leaseToken, state, code, detail) {
  await env.RLOGS_DB.prepare(`UPDATE reconciliation_jobs SET state=?2, failure_code=?3,
    failure_detail=?4, updated_unix_millis=?5 WHERE job_id=?1 AND lease_token=?6`)
    .bind(jobId, state, code, String(detail).slice(0, 2000), Date.now(), leaseToken).run();
  return json({ error: String(detail) }, state === "rejected" ? 422 : 503);
}

export async function reconcileRunGroup(env, runGroupId) {
  if (!RUN_GROUP_ID.test(runGroupId)) return json({ error: "invalid run group" }, 400);
  const sources = await reconciliationSources(env, runGroupId);
  if (!sources) return json({ error: "run group does not have 2..64 unique current public replay sources" }, 409);
  const sourceSetSha256 = await sourceSetDigest(runGroupId, sources);
  const existing = await first(env, `SELECT c.reconciliation_id, v.projection_object_key
    FROM reconciliation_current c JOIN reconciliation_versions v
      ON v.reconciliation_id=c.reconciliation_id
    WHERE c.run_group_id=?1 AND c.source_set_sha256=?2`, runGroupId, sourceSetSha256);
  if (existing) return json({ accepted: true, duplicate: true, reconciliation_id: existing.reconciliation_id });

  const jobId = `rjob_${sourceSetSha256.slice(0, 32)}`;
  const started = Date.now();
  const leaseToken = crypto.randomUUID();
  await env.RLOGS_DB.prepare(`INSERT INTO reconciliation_jobs
      (job_id,run_group_id,source_set_sha256,state,attempt_count,lease_token,created_unix_millis,updated_unix_millis)
      VALUES (?1,?2,?3,'running',1,?5,?4,?4)
      ON CONFLICT(run_group_id,source_set_sha256) DO UPDATE SET state='running',
        attempt_count=attempt_count+1,lease_token=excluded.lease_token,failure_code=NULL,
        failure_detail=NULL,completed_unix_millis=NULL,updated_unix_millis=excluded.updated_unix_millis
      WHERE reconciliation_jobs.state IN ('retryable_failure','superseded')`)
    .bind(jobId, runGroupId, sourceSetSha256, started, leaseToken).run();
  const acquired = await first(env, `SELECT state FROM reconciliation_jobs
    WHERE job_id=?1 AND lease_token=?2 AND state='running'`, jobId, leaseToken);
  if (!acquired) {
    const job = await first(env, `SELECT state FROM reconciliation_jobs
      WHERE run_group_id=?1 AND source_set_sha256=?2`, runGroupId, sourceSetSha256);
    if (job?.state === "rejected") return json({ error: "reconciliation source set was rejected" }, 422);
    if (job?.state === "running") return json({ accepted: true, duplicate: true, in_progress: true }, 202);
    return json({ error: "reconciliation job state is inconsistent with its current pointer" }, 503);
  }

  const registration = [];
  for (const source of sources) {
    registration.push(env.RLOGS_DB.prepare(`INSERT INTO reconciliation_job_sources
      (job_id,report_id,run_index,projection_sha256,projection_object_key)
      VALUES (?1,?2,?3,?4,?5) ON CONFLICT(job_id,report_id) DO UPDATE SET
        run_index=excluded.run_index,projection_sha256=excluded.projection_sha256,
        projection_object_key=excluded.projection_object_key`).bind(
      jobId, source.report_id, source.run_index, source.projection.sha256, source.projection.object_key,
    ));
  }
  try {
    await env.RLOGS_DB.batch(registration);
  } catch (cause) {
    return reconciliationFailure(env, jobId, leaseToken, "retryable_failure", "source_registration_failed",
      cause?.message ?? cause);
  }

  const container = env.RLOGS_VERIFIER_CONTAINER.getByName(jobId);
  let response;
  let result;
  try {
    ({ response, result } = await runOneShotVerifier(container, new Request("http://container/internal/v1/reconcile", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema_version: 1, run_group_id: runGroupId,
        sources: sources.map(({ submitter_id, ...source }) => source),
      }),
    })));
  } catch (cause) {
    return reconciliationFailure(env, jobId, leaseToken, "retryable_failure", "container_unavailable", cause?.message ?? cause);
  }
  if (!response.ok) {
    const detail = result?.error ?? `verifier returned HTTP ${response.status}`;
    return reconciliationFailure(env, jobId, leaseToken, response.status === 422 ? "rejected" : "retryable_failure",
      response.status === 422 ? "replay_rejected" : "verifier_unavailable", detail);
  }
  if (!validateReconciliationOutput(result, runGroupId, sources)) {
    return reconciliationFailure(env, jobId, leaseToken, "retryable_failure", "invalid_verifier_output",
      "reconciliation verifier output failed source identity validation");
  }

  const projectionBytes = new TextEncoder().encode(JSON.stringify(result));
  const projectionSha256 = await sha256(projectionBytes);
  const projectionKey = `reconciliations/${runGroupId}/versions/${result.reconciliation_id}-${projectionSha256}.json`;
  await env.RLOGS_ARTIFACTS.put(projectionKey, projectionBytes, { httpMetadata: { contentType: "application/json" } });
  const completed = Date.now();
  const guard = currentSourceGuard();
  const statements = [
    env.RLOGS_DB.prepare(`INSERT INTO reconciliation_versions
      (reconciliation_id,run_group_id,source_set_sha256,projection_sha256,projection_object_key,
       source_count,reconciliation_status,created_unix_millis)
      SELECT ?4,?2,?5,?6,?7,?3,?8,?9 WHERE ${guard}
      ON CONFLICT(reconciliation_id) DO NOTHING`).bind(
      jobId, runGroupId, sources.length, result.reconciliation_id, sourceSetSha256,
      projectionSha256, projectionKey, result.status, completed,
    ),
    env.RLOGS_DB.prepare(`INSERT INTO reconciliation_current
      (run_group_id,reconciliation_id,source_set_sha256,updated_unix_millis)
      SELECT ?2,?4,?5,?6 WHERE ${guard}
      ON CONFLICT(run_group_id) DO UPDATE SET reconciliation_id=excluded.reconciliation_id,
        source_set_sha256=excluded.source_set_sha256,updated_unix_millis=excluded.updated_unix_millis`)
      .bind(jobId, runGroupId, sources.length, result.reconciliation_id, sourceSetSha256, completed),
  ];
  const distinctSubmitters = new Set(sources.map((source) => source.submitter_id).filter(Boolean)).size;
  for (const source of sources) {
    const row = await first(env, "SELECT catalog_entry_json FROM report_runs WHERE report_id=?1 AND run_index=?2",
      source.report_id, source.run_index);
    let entry;
    try { entry = JSON.parse(row?.catalog_entry_json); } catch { entry = null; }
    if (!entry) continue;
    statements.push(env.RLOGS_DB.prepare(`UPDATE report_runs SET catalog_entry_json=?4
      WHERE report_id=?5 AND run_index=?6 AND EXISTS (
        SELECT 1 FROM reconciliation_current WHERE run_group_id=?2
          AND reconciliation_id=?7 AND source_set_sha256=?8) AND ${guard}`).bind(
      jobId, runGroupId, sources.length,
      JSON.stringify(reconcileCatalogEntry(entry, result, sources.length, distinctSubmitters)),
      source.report_id, source.run_index, result.reconciliation_id, sourceSetSha256,
    ));
  }
  statements.push(env.RLOGS_DB.prepare(`UPDATE reconciliation_jobs SET state='published',
    completed_unix_millis=?4,updated_unix_millis=?4 WHERE job_id=?1 AND ${guard}`)
    .bind(jobId, runGroupId, sources.length, completed));
  await env.RLOGS_DB.batch(statements);
  const published = await first(env, `SELECT reconciliation_id FROM reconciliation_current
    WHERE run_group_id=?1 AND reconciliation_id=?2 AND source_set_sha256=?3`,
  runGroupId, result.reconciliation_id, sourceSetSha256);
  if (!published) {
    return reconciliationFailure(env, jobId, leaseToken, "superseded", "source_set_changed",
      "public source set changed before reconciliation publication");
  }
  return json({ accepted: true, reconciliation_id: result.reconciliation_id,
    projection_sha256: projectionSha256, source_count: sources.length });
}

async function verifyJob(request, env, context) {
  const wakeup = await request.json().catch(() => null);
  if (!validateWakeup(wakeup)) return json({ error: "invalid verification wake-up" }, 400);
  const session = await first(env, "SELECT * FROM upload_sessions WHERE upload_id = ?1", wakeup.upload_id);
  const job = await first(env, "SELECT * FROM verification_jobs WHERE upload_id = ?1", wakeup.upload_id);
  if (!session || !job || session.artifact_sha256 !== wakeup.artifact_sha256) {
    return json({ error: "verification job not found" }, 404);
  }
  if (session.state === "accepted" && job.state === "accepted") return json({ accepted: true, duplicate: true });
  if (session.state === "rejected" || job.state === "rejected") return json({ error: "verification job is rejected" }, 422);

  const manifest = JSON.parse(session.manifest_json);
  const storedChunks = await all(env,
    "SELECT sequence, sha256, byte_length, object_key FROM upload_chunks WHERE upload_id = ?1 ORDER BY sequence",
    wakeup.upload_id,
  );
  if (!sameChunkCommitments(storedChunks.map(({ sequence, sha256, byte_length, object_key }) => ({
    sequence: Number(sequence), sha256, byte_length: Number(byte_length), object_key,
  })), wakeup.chunks)) {
    return rejectJob(env, wakeup.upload_id, "chunk_commitment_mismatch", "stored chunks disagree with verifier wake-up");
  }

  const now = Date.now();
  await env.RLOGS_DB.prepare(`UPDATE verification_jobs SET state='running',
      attempt_count=attempt_count+1, lease_owner=?2, lease_expires_unix_millis=?3,
      updated_unix_millis=?4, failure_code=NULL, failure_detail=NULL
    WHERE upload_id=?1 AND state IN ('queued', 'retryable_failure', 'running')`)
    .bind(wakeup.upload_id, crypto.randomUUID(), now + 10 * 60 * 1000, now).run();
  await env.RLOGS_DB.prepare("UPDATE upload_sessions SET state='verifying', updated_unix_millis=?2 WHERE upload_id=?1 AND state IN ('queued','verifying')")
    .bind(wakeup.upload_id, now).run();

  const profiles = await all(env, `SELECT character_id, deployment_id, region_id, public_projection_json
      FROM profiles WHERE submitter_id=?1`, session.submitter_id);
  const names = {};
  for (const profile of profiles) {
    const name = compatibleProfileName(profile, manifest);
    if (name) names[profile.character_id] = name;
  }
  const containerRequest = {
    schema_version: 1,
    upload_id: wakeup.upload_id,
    artifact_sha256: wakeup.artifact_sha256,
    expected_report_id: wakeup.expected_report_id,
    created_unix_millis: Number(session.created_unix_millis),
    manifest,
    chunks: wakeup.chunks,
    submission_provenance: { submitter_id: session.submitter_id, authentication: "discord_device_token" },
    verified_names_by_character: names,
  };
  const container = env.RLOGS_VERIFIER_CONTAINER.getByName(wakeup.upload_id);
  let response;
  let result;
  try {
    ({ response, result } = await runOneShotVerifier(container, new Request("http://container/internal/v1/verify", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(containerRequest),
    })));
  } catch (cause) {
    return retryJob(env, wakeup.upload_id, "container_unavailable", String(cause?.message ?? cause));
  }
  if (!response.ok) {
    const detail = String(result?.error ?? `verifier returned HTTP ${response.status}`).slice(0, 2000);
    return response.status === 422
      ? rejectJob(env, wakeup.upload_id, "replay_rejected", detail)
      : retryJob(env, wakeup.upload_id, "verifier_unavailable", detail);
  }
  if ((manifest.metadata.purpose ?? "combat_run") === "training_dummy") {
    if (!validateTrainingOutput(result, wakeup)) {
      return retryJob(env, wakeup.upload_id, "invalid_verifier_output", "training verifier output failed identity validation");
    }
    return persistTrainingDummyResult(env, {
      result, wakeup, session, job, manifest,
    });
  }
  if (!validateOutput(result, wakeup)) {
    return retryJob(env, wakeup.upload_id, "invalid_verifier_output", "verifier output failed identity validation");
  }

  const account = await first(env, "SELECT publish_verified_parses FROM accounts WHERE submitter_id=?1", session.submitter_id);
  result.report.visibility = Number(account?.publish_verified_parses) === 1 ? "public" : manifest.metadata.visibility;
  const projectionBytes = new TextEncoder().encode(JSON.stringify(result.report));
  const membershipBytes = new TextEncoder().encode(JSON.stringify(result.membership));
  const projectionDigest = await sha256(projectionBytes);
  const projectionKey = `reports/${wakeup.expected_report_id}/projection-${projectionDigest}.json`;
  const membershipKey = `private/reports/${wakeup.expected_report_id}/membership.json`;
  await env.RLOGS_ARTIFACTS.put(projectionKey, projectionBytes, { httpMetadata: { contentType: "application/json" } });
  await env.RLOGS_ARTIFACTS.put(membershipKey, membershipBytes, { httpMetadata: { contentType: "application/json" } });

  const firstRun = result.report.runs[0];
  const visibility = result.report.visibility;
  const completed = Date.now();
  const statements = [
    env.RLOGS_DB.prepare(`INSERT INTO reports (
        report_id, upload_id, run_group_id, visibility, verification_tier, verifier_release,
        game_build, protocol_pack_digest, artifact_sha256, projection_sha256,
        projection_object_key, verified_unix_millis, published_unix_millis
      ) VALUES (?1,?2,?3,?4,'replayed',?5,?6,?7,?8,?9,?10,?11,?12)
      ON CONFLICT(upload_id) DO NOTHING`).bind(
      wakeup.expected_report_id, wakeup.upload_id, firstRun.run_group_id, visibility,
      job.verifier_release, job.game_build, job.protocol_pack_digest, wakeup.artifact_sha256,
      projectionDigest, projectionKey, completed, visibility === "public" ? completed : null,
    ),
    env.RLOGS_DB.prepare(`UPDATE verification_jobs SET state='accepted', output_sha256=?2,
      completed_unix_millis=?3, updated_unix_millis=?3, lease_owner=NULL,
      lease_expires_unix_millis=NULL WHERE upload_id=?1`).bind(wakeup.upload_id, projectionDigest, completed),
    env.RLOGS_DB.prepare(`UPDATE upload_sessions SET state='accepted', updated_unix_millis=?2,
      rejection_code=NULL, rejection_detail=NULL WHERE upload_id=?1`).bind(wakeup.upload_id, completed),
  ];
  for (const run of result.report.runs) {
    const entry = catalogEntry(result.report, run);
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_runs
      (report_id, run_index, run_group_id, catalog_entry_json, created_unix_millis)
      VALUES (?1,?2,?3,?4,?5)
      ON CONFLICT(report_id,run_index) DO UPDATE SET
        run_group_id=excluded.run_group_id, catalog_entry_json=excluded.catalog_entry_json,
        created_unix_millis=excluded.created_unix_millis`).bind(
      wakeup.expected_report_id, run.run_index, entry.run_group_id,
      JSON.stringify(entry), result.report.created_unix_millis,
    ));
  }
  const identities = result.membership.character_by_actor ?? {};
  const actorsByCharacter = new Map(Object.entries(identities).map(([actor, character]) => [character, actor]));
  for (const run of result.membership.runs) {
    for (const characterId of run.character_ids) {
      statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_memberships
        (report_id, game_id, character_id, actor_id, player_name) VALUES (?1,?2,?3,?4,?5)
        ON CONFLICT(report_id,game_id,character_id) DO UPDATE SET
          actor_id=excluded.actor_id, player_name=excluded.player_name`).bind(
        wakeup.expected_report_id, manifest.metadata.game_plugin_id, characterId,
        actorsByCharacter.get(characterId) ?? null, names[characterId] ?? null,
      ));
    }
  }
  await env.RLOGS_DB.batch(statements);
  for (const runGroupId of new Set(result.report.runs.map((run) => run.run_group_id).filter(Boolean))) {
    const task = reconcileRunGroup(env, runGroupId).catch((cause) => {
      console.error("rLogs reconciliation wake-up failed", runGroupId, cause);
    });
    if (context?.waitUntil) context.waitUntil(task);
  }
  return json({ accepted: true, report_id: wakeup.expected_report_id, projection_sha256: projectionDigest });
}

async function persistTrainingDummyResult(env, { result, wakeup, session, job, manifest }) {
  const account = await first(env, "SELECT publish_verified_parses FROM accounts WHERE submitter_id=?1", session.submitter_id);
  result.visibility = Number(account?.publish_verified_parses) === 1 ? "public" : manifest.metadata.visibility;
  const projectionBytes = new TextEncoder().encode(JSON.stringify(result));
  const projectionDigest = await sha256(projectionBytes);
  const projectionKey = `training-dummy/${wakeup.expected_report_id}/projection-${projectionDigest}.json`;
  await env.RLOGS_ARTIFACTS.put(projectionKey, projectionBytes, { httpMetadata: { contentType: "application/json" } });

  const completed = Date.now();
  await env.RLOGS_DB.batch([
    env.RLOGS_DB.prepare(`INSERT INTO training_dummy_results (
        result_id, upload_id, visibility, verification_tier, verifier_release,
        submitter_id, character_id, display_name, deployment_id, region_id, realm_id,
        world_id, season_id, class_id, specialization_id, target_monster_id,
        duration_micros, total_damage, dps, artifact_sha256, projection_sha256,
        projection_object_key, created_unix_millis, verified_unix_millis, published_unix_millis
      ) VALUES (?1,?2,?3,'replayed',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)
      ON CONFLICT(upload_id) DO NOTHING`).bind(
      result.result_id,
      wakeup.upload_id,
      result.visibility,
      job.verifier_release,
      session.submitter_id,
      result.character_id,
      result.player_name ?? null,
      result.deployment_id,
      result.region_id,
      result.realm_id ?? null,
      result.world_id ?? null,
      result.season_id,
      result.class_id,
      result.specialization_id,
      result.target_monster_id,
      result.duration_micros,
      result.total_damage,
      result.dps,
      wakeup.artifact_sha256,
      projectionDigest,
      projectionKey,
      result.created_unix_millis,
      completed,
      result.visibility === "public" ? completed : null,
    ),
    env.RLOGS_DB.prepare(`UPDATE verification_jobs SET state='accepted', output_sha256=?2,
      completed_unix_millis=?3, updated_unix_millis=?3, lease_owner=NULL,
      lease_expires_unix_millis=NULL WHERE upload_id=?1`).bind(wakeup.upload_id, projectionDigest, completed),
    env.RLOGS_DB.prepare(`UPDATE upload_sessions SET state='accepted', updated_unix_millis=?2,
      rejection_code=NULL, rejection_detail=NULL WHERE upload_id=?1`).bind(wakeup.upload_id, completed),
  ]);
  return json({
    accepted: true,
    report_id: result.result_id,
    training_result_id: result.result_id,
    projection_sha256: projectionDigest,
  });
}

async function rejectJob(env, uploadId, code, detail) {
  const now = Date.now();
  await env.RLOGS_DB.batch([
    env.RLOGS_DB.prepare(`UPDATE verification_jobs SET state='rejected', failure_code=?2,
      failure_detail=?3, completed_unix_millis=?4, updated_unix_millis=?4,
      lease_owner=NULL, lease_expires_unix_millis=NULL WHERE upload_id=?1`).bind(uploadId, code, detail, now),
    env.RLOGS_DB.prepare(`UPDATE upload_sessions SET state='rejected', rejection_code=?2,
      rejection_detail=?3, updated_unix_millis=?4 WHERE upload_id=?1`).bind(uploadId, code, detail, now),
  ]);
  return json({ error: detail }, 422);
}

async function retryJob(env, uploadId, code, detail) {
  const now = Date.now();
  await env.RLOGS_DB.prepare(`UPDATE verification_jobs SET state='retryable_failure',
      failure_code=?2, failure_detail=?3, next_attempt_unix_millis=?4,
      updated_unix_millis=?5, lease_owner=NULL, lease_expires_unix_millis=NULL WHERE upload_id=?1`)
    .bind(uploadId, code, detail, now + 30_000, now).run();
  await env.RLOGS_DB.prepare("UPDATE upload_sessions SET state='queued', updated_unix_millis=?2 WHERE upload_id=?1")
    .bind(uploadId, now).run();
  return json({ error: detail }, 503);
}

export class RLogsVerifierContainer extends Container {
  defaultPort = 8080;
  // Each upload has its own stateless verifier instance. Keep it alive only
  // long enough to absorb an immediate retry, then stop billing idle memory.
  sleepAfter = "10s";

  async onActivityExpired() {
    // The SDK default sends SIGTERM, which the verifier server may not exit on
    // quickly enough to release the very small production instance pool.
    // Verification is stateless and all durable inputs live in R2/D1, so an
    // idle instance is always safe to force-destroy.
    await this.destroy();
  }
}

RLogsVerifierContainer.outboundByHost = {
  "rlogs-artifacts.r2": async (request, env) => {
    if (request.method !== "GET") return new Response(null, { status: 405 });
    const key = decodeURIComponent(new URL(request.url).pathname.slice(1));
    if (!/^uploads\/up_[a-f0-9]{32}\/chunks\/[A-Za-z0-9._-]+$/.test(key) &&
        !/^reports\/rpt_[a-f0-9]{32}\/projection-[a-f0-9]{64}\.json$/.test(key)) {
      return new Response(null, { status: 403 });
    }
    const object = await env.RLOGS_ARTIFACTS.get(key);
    return object ? new Response(object.body, { headers: { "Content-Length": String(object.size) } })
      : new Response(null, { status: 404 });
  },
};

export default {
  async fetch(request, env, context) {
    const path = new URL(request.url).pathname;
    if (request.method === "POST" && /^\/v1\/verification-jobs\/up_[a-f0-9]{32}\/run$/.test(path)) {
      return verifyJob(request, env, context);
    }
    const reconciliation = /^\/v1\/reconciliation-jobs\/([A-Za-z0-9_-]{1,96})\/run$/.exec(path);
    if (request.method === "POST" && reconciliation) {
      const body = await request.json().catch(() => null);
      if (body?.schema_version !== 1 || body?.run_group_id !== reconciliation[1]) {
        return json({ error: "invalid reconciliation wake-up" }, 400);
      }
      return reconcileRunGroup(env, reconciliation[1]);
    }
    return json({ error: "not found" }, 404);
  },
};
