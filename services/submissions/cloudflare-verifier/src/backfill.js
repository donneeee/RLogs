import {
  BACKFILL_SOURCE_SCHEMA_VERSION, BACKFILL_TARGET_SCHEMA_VERSION,
  catalogEntry, compatibleProfileName, isSchema12BackfillCandidate,
  runOneShotVerifier, sameChunkCommitments, validateBackfillOutput,
} from "./core.js";

const REPORT_ID = /^rpt_[a-f0-9]{32}$/;
const UPLOAD_ID = /^up_[a-f0-9]{32}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const LEASE_MILLIS = 15 * 60 * 1000;
const MAX_REPORT_RUNS = 64;
const MAX_REPORT_MEMBERSHIPS = 128;
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

function projectionKey(reportId, digest) {
  return `reports/${reportId}/projection-${digest}.json`;
}

function indexSnapshotKey(reportId, digest) {
  return `private/reports/${reportId}/backfill-index-${digest}.json`;
}

async function contentAddressedJson(env, key, expectedDigest, missingDetail) {
  const object = await env.RLOGS_ARTIFACTS.get(key);
  if (!object) throw new TypeError(missingDetail);
  const bytes = new Uint8Array(await object.arrayBuffer());
  if (await sha256(bytes) !== expectedDigest) throw new TypeError("content-addressed object digest mismatch");
  try {
    return JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw new TypeError("content-addressed object is not valid JSON");
  }
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
      row.projection_object_key !== projectionKey(row.report_id, row.projection_sha256)) {
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

async function sourceIndexSnapshot(env, row) {
  const [runs, memberships] = await Promise.all([
    all(env, `SELECT run_index,run_group_id,catalog_entry_json,created_unix_millis
      FROM report_runs WHERE report_id=?1 ORDER BY run_index`, row.report_id),
    all(env, `SELECT game_id,character_id,actor_id,player_name
      FROM report_memberships WHERE report_id=?1 ORDER BY game_id,character_id`, row.report_id),
  ]);
  if (runs.length < 1 || runs.length > MAX_REPORT_RUNS || memberships.length > MAX_REPORT_MEMBERSHIPS) {
    throw new TypeError("source report indexes violate rollback snapshot bounds");
  }
  const normalizedRuns = runs.map((run) => {
    const runIndex = Number(run.run_index);
    const created = Number(run.created_unix_millis);
    if (!Number.isInteger(runIndex) || runIndex < 0 || typeof run.run_group_id !== "string" ||
        !run.run_group_id || !Number.isSafeInteger(created) || created < 0) {
      throw new TypeError("source catalog row is invalid");
    }
    try { JSON.parse(run.catalog_entry_json); } catch { throw new TypeError("source catalog JSON is invalid"); }
    return { run_index: runIndex, run_group_id: run.run_group_id,
      catalog_entry_json: run.catalog_entry_json, created_unix_millis: created };
  });
  const normalizedMemberships = memberships.map((membership) => {
    if (![membership.game_id, membership.character_id].every((value) =>
      typeof value === "string" && value.length > 0) ||
      ![membership.actor_id, membership.player_name].every((value) => value == null || typeof value === "string")) {
      throw new TypeError("source membership row is invalid");
    }
    return { game_id: membership.game_id, character_id: membership.character_id,
      actor_id: membership.actor_id ?? null, player_name: membership.player_name ?? null };
  });
  return {
    schema_version: 1,
    report_id: row.report_id,
    upload_id: row.upload_id,
    artifact_sha256: row.artifact_sha256,
    projection_sha256: row.projection_sha256,
    projection_object_key: row.projection_object_key,
    verifier_release: row.verifier_release,
    run_group_id: row.run_group_id,
    report_runs: normalizedRuns,
    report_memberships: normalizedMemberships,
  };
}

function validateIndexSnapshot(snapshot, row, job) {
  if (snapshot?.schema_version !== 1 || snapshot.report_id !== row.report_id ||
      snapshot.upload_id !== row.upload_id || snapshot.artifact_sha256 !== row.artifact_sha256 ||
      snapshot.projection_sha256 !== job.source_projection_sha256 ||
      snapshot.projection_object_key !== job.source_projection_object_key ||
      typeof snapshot.verifier_release !== "string" || !snapshot.verifier_release ||
      typeof snapshot.run_group_id !== "string" || !snapshot.run_group_id ||
      !Array.isArray(snapshot.report_runs) || snapshot.report_runs.length < 1 ||
      snapshot.report_runs.length > MAX_REPORT_RUNS || !Array.isArray(snapshot.report_memberships) ||
      snapshot.report_memberships.length > MAX_REPORT_MEMBERSHIPS) return false;
  const runIndexes = new Set();
  for (const run of snapshot.report_runs) {
    if (!Number.isInteger(run?.run_index) || run.run_index < 0 || runIndexes.has(run.run_index) ||
        typeof run.run_group_id !== "string" || !run.run_group_id ||
        !Number.isSafeInteger(run.created_unix_millis) || run.created_unix_millis < 0 ||
        typeof run.catalog_entry_json !== "string") return false;
    try { JSON.parse(run.catalog_entry_json); } catch { return false; }
    runIndexes.add(run.run_index);
  }
  const memberships = new Set();
  for (const membership of snapshot.report_memberships) {
    if (![membership?.game_id, membership?.character_id].every((value) =>
      typeof value === "string" && value.length > 0) ||
      ![membership.actor_id, membership.player_name].every((value) => value == null || typeof value === "string")) return false;
    const key = `${membership.game_id}\0${membership.character_id}`;
    if (memberships.has(key)) return false;
    memberships.add(key);
  }
  return true;
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
  const sourceIndexes = await sourceIndexSnapshot(env, row);
  const projectionBytes = new TextEncoder().encode(JSON.stringify(result.report));
  const membershipBytes = new TextEncoder().encode(JSON.stringify(result.membership));
  const sourceIndexBytes = new TextEncoder().encode(JSON.stringify(sourceIndexes));
  const projectionSha256 = await sha256(projectionBytes);
  const membershipSha256 = await sha256(membershipBytes);
  const sourceIndexSha256 = await sha256(sourceIndexBytes);
  const candidateProjectionKey = projectionKey(row.report_id, projectionSha256);
  const membershipKey = `private/reports/${row.report_id}/membership-${membershipSha256}.json`;
  const sourceIndexKey = indexSnapshotKey(row.report_id, sourceIndexSha256);
  await Promise.all([
    env.RLOGS_ARTIFACTS.put(candidateProjectionKey, projectionBytes, { httpMetadata: { contentType: "application/json" } }),
    env.RLOGS_ARTIFACTS.put(membershipKey, membershipBytes, { httpMetadata: { contentType: "application/json" } }),
    env.RLOGS_ARTIFACTS.put(sourceIndexKey, sourceIndexBytes, { httpMetadata: { contentType: "application/json" } }),
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
      row.projection_object_key, projectionSha256, candidateProjectionKey,
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
      batch.target_verifier_release, projectionSha256, candidateProjectionKey,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_runs WHERE report_id=?1 AND ${candidateGuard}`).bind(
      row.report_id, projectionSha256, candidateProjectionKey, row.upload_id, row.artifact_sha256,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_memberships WHERE report_id=?1 AND ${candidateGuard}`).bind(
      row.report_id, projectionSha256, candidateProjectionKey, row.upload_id, row.artifact_sha256,
    ),
  ];
  for (const run of result.report.runs) {
    const entry = catalogEntry(result.report, run);
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_runs
      (report_id,run_index,run_group_id,catalog_entry_json,created_unix_millis)
      SELECT ?1,?6,?7,?8,?9 WHERE ${candidateGuard}`).bind(
      row.report_id, projectionSha256, candidateProjectionKey, row.upload_id,
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
      row.report_id, projectionSha256, candidateProjectionKey, row.upload_id,
      row.artifact_sha256, gameId, characterId,
      actorByCharacter.get(characterId) ?? null, names[characterId] ?? null,
    ));
  }
  statements.push(env.RLOGS_DB.prepare(`UPDATE projection_backfill_jobs SET state='published',
    candidate_projection_sha256=?2,candidate_projection_object_key=?3,
    candidate_membership_sha256=?4,candidate_membership_object_key=?5,
    source_indexes_sha256=?6,source_indexes_object_key=?7,
    updated_unix_millis=?8,completed_unix_millis=?8
    WHERE job_id=?1 AND EXISTS (SELECT 1 FROM reports WHERE report_id=?9
      AND projection_sha256=?2 AND projection_object_key=?3)`).bind(
    jobId, projectionSha256, candidateProjectionKey, membershipSha256, membershipKey,
    sourceIndexSha256, sourceIndexKey, now, row.report_id,
  ));
  try {
    await env.RLOGS_DB.batch(statements);
  } catch (cause) {
    // D1 batch delivery can be uncertain.  Treat the guarded current pointer as
    // authoritative before scheduling a second replay or downgrading the job.
    const committed = await first(env, `SELECT projection_sha256 FROM reports
      WHERE report_id=?1 AND projection_sha256=?2 AND projection_object_key=?3`,
    row.report_id, projectionSha256, candidateProjectionKey).catch(() => null);
    if (!committed) throw cause;
  }
  const published = await first(env, `SELECT projection_sha256 FROM reports
    WHERE report_id=?1 AND projection_sha256=?2 AND projection_object_key=?3`,
  row.report_id, projectionSha256, candidateProjectionKey);
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

export async function rollbackPublishedReplay(env, request) {
  const row = {
    report_id: request.report_id,
    upload_id: request.upload_id,
    artifact_sha256: request.artifact_sha256,
  };
  const job = {
    source_projection_sha256: request.source_projection_sha256,
    source_projection_object_key: request.source_projection_object_key,
  };
  if (!/^bfr_[a-f0-9]{32}$/.test(String(request.rollback_id ?? "")) ||
      !REPORT_ID.test(String(row.report_id ?? "")) || !UPLOAD_ID.test(String(row.upload_id ?? "")) ||
      !DIGEST.test(String(row.artifact_sha256 ?? "")) ||
      request.job_state !== "published" ||
      !DIGEST.test(String(request.expected_candidate_projection_sha256 ?? "")) ||
      request.expected_candidate_projection_object_key !==
        projectionKey(row.report_id, request.expected_candidate_projection_sha256) ||
      !DIGEST.test(String(job.source_projection_sha256 ?? "")) ||
      job.source_projection_object_key !== projectionKey(row.report_id, job.source_projection_sha256) ||
      request.target_source_projection_sha256 !== job.source_projection_sha256 ||
      request.target_source_projection_object_key !== job.source_projection_object_key ||
      request.candidate_projection_sha256 !== request.expected_candidate_projection_sha256 ||
      request.candidate_projection_object_key !== request.expected_candidate_projection_object_key ||
      !DIGEST.test(String(request.source_indexes_sha256 ?? "")) ||
      request.source_indexes_object_key !== indexSnapshotKey(row.report_id, request.source_indexes_sha256)) {
    throw new TypeError("rollback request does not identify one exact published backfill transition");
  }
  const [sourceProjection, candidateProjection, snapshot] = await Promise.all([
    contentAddressedJson(env, job.source_projection_object_key, job.source_projection_sha256,
      "registered source projection object is missing"),
    contentAddressedJson(env, request.candidate_projection_object_key,
      request.candidate_projection_sha256, "registered candidate projection object is missing"),
    contentAddressedJson(env, request.source_indexes_object_key, request.source_indexes_sha256,
      "rollback index snapshot object is missing"),
  ]);
  if (sourceProjection?.report_id !== row.report_id ||
      sourceProjection?.verification?.artifact_sha256 !== row.artifact_sha256 ||
      candidateProjection?.report_id !== row.report_id ||
      candidateProjection?.verification?.artifact_sha256 !== row.artifact_sha256 ||
      !Array.isArray(candidateProjection.runs) || candidateProjection.runs.length < 1 ||
      candidateProjection.runs.length > MAX_REPORT_RUNS || candidateProjection.runs.some((run) =>
        typeof run?.run_group_id !== "string" || !run.run_group_id) ||
      !Number.isInteger(sourceProjection.schema_version) || sourceProjection.schema_version < 1 ||
      !validateIndexSnapshot(snapshot, row, job)) {
    throw new TypeError("rollback evidence does not match the report identity");
  }

  const now = Date.now();
  const candidateGuard = `EXISTS (SELECT 1 FROM reports current WHERE current.report_id=?1
    AND current.upload_id=?2 AND current.artifact_sha256=?3 AND current.visibility='public'
    AND current.projection_sha256=?4 AND current.projection_object_key=?5)
    AND EXISTS (SELECT 1 FROM report_projection_versions source_version
      WHERE source_version.report_id=?1 AND source_version.projection_sha256=?6
        AND source_version.projection_object_key=?7 AND source_version.artifact_sha256=?3
        AND source_version.schema_version=?9 AND source_version.verifier_release=?10)
    AND EXISTS (SELECT 1 FROM report_projection_versions candidate_version
      WHERE candidate_version.report_id=?1 AND candidate_version.projection_sha256=?4
        AND candidate_version.projection_object_key=?5 AND candidate_version.artifact_sha256=?3
        AND candidate_version.backfill_job_id=?8 AND candidate_version.verifier_release=?11)`;
  const sourceGuard = `EXISTS (SELECT 1 FROM reports current WHERE current.report_id=?1
    AND current.upload_id=?2 AND current.artifact_sha256=?3 AND current.visibility='public'
    AND current.projection_sha256=?4 AND current.projection_object_key=?5)
    AND EXISTS (SELECT 1 FROM projection_backfill_rollbacks committing
      WHERE committing.rollback_id=?6 AND committing.lease_token=?7 AND committing.state='committing')`;
  const common = [row.report_id, row.upload_id, row.artifact_sha256,
    request.expected_candidate_projection_sha256, request.expected_candidate_projection_object_key,
    job.source_projection_sha256, job.source_projection_object_key, request.job_id,
    sourceProjection.schema_version, snapshot.verifier_release, request.target_verifier_release];
  const statements = [
    env.RLOGS_DB.prepare(`UPDATE projection_backfill_rollbacks SET state='committing',
      updated_unix_millis=?14 WHERE rollback_id=?12 AND lease_token=?13 AND state='running'
      AND ${candidateGuard}`).bind(...common, request.rollback_id, request.lease_token, now),
    env.RLOGS_DB.prepare(`UPDATE reports SET run_group_id=?12,verifier_release=?10,
      projection_sha256=?6,projection_object_key=?7 WHERE report_id=?1 AND ${candidateGuard}
      AND EXISTS (SELECT 1 FROM projection_backfill_rollbacks committing
        WHERE committing.rollback_id=?13 AND committing.lease_token=?14
          AND committing.state='committing')`).bind(
      ...common, snapshot.run_group_id, request.rollback_id, request.lease_token,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_runs WHERE report_id=?1 AND ${sourceGuard}`).bind(
      row.report_id, row.upload_id, row.artifact_sha256,
      job.source_projection_sha256, job.source_projection_object_key,
      request.rollback_id, request.lease_token,
    ),
    env.RLOGS_DB.prepare(`DELETE FROM report_memberships WHERE report_id=?1 AND ${sourceGuard}`).bind(
      row.report_id, row.upload_id, row.artifact_sha256,
      job.source_projection_sha256, job.source_projection_object_key,
      request.rollback_id, request.lease_token,
    ),
  ];
  for (const run of snapshot.report_runs) {
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_runs
      (report_id,run_index,run_group_id,catalog_entry_json,created_unix_millis)
      SELECT ?1,?8,?9,?10,?11 WHERE ${sourceGuard}`).bind(
      row.report_id, row.upload_id, row.artifact_sha256,
      job.source_projection_sha256, job.source_projection_object_key,
      request.rollback_id, request.lease_token,
      run.run_index, run.run_group_id, run.catalog_entry_json, run.created_unix_millis,
    ));
  }
  for (const membership of snapshot.report_memberships) {
    statements.push(env.RLOGS_DB.prepare(`INSERT INTO report_memberships
      (report_id,game_id,character_id,actor_id,player_name)
      SELECT ?1,?8,?9,?10,?11 WHERE ${sourceGuard}`).bind(
      row.report_id, row.upload_id, row.artifact_sha256,
      job.source_projection_sha256, job.source_projection_object_key,
      request.rollback_id, request.lease_token,
      membership.game_id, membership.character_id, membership.actor_id, membership.player_name,
    ));
  }
  statements.push(env.RLOGS_DB.prepare(`UPDATE projection_backfill_rollbacks SET state='restored',
    lease_token=NULL,updated_unix_millis=?2,completed_unix_millis=?2
    WHERE rollback_id=?1 AND lease_token=?6 AND state='committing' AND EXISTS (SELECT 1 FROM reports
      WHERE report_id=?3 AND projection_sha256=?4 AND projection_object_key=?5)`).bind(
    request.rollback_id, now, row.report_id,
    job.source_projection_sha256, job.source_projection_object_key, request.lease_token,
  ));
  try {
    await env.RLOGS_DB.batch(statements);
  } catch (cause) {
    const restored = await first(env, `SELECT reports.projection_sha256 FROM reports
      JOIN projection_backfill_rollbacks rb ON rb.rollback_id=?4 AND rb.state='restored'
      WHERE reports.report_id=?1 AND reports.projection_sha256=?2
        AND reports.projection_object_key=?3`, row.report_id, job.source_projection_sha256,
    job.source_projection_object_key, request.rollback_id).catch(() => null);
    if (!restored) throw cause;
  }
  const restored = await first(env, `SELECT reports.projection_sha256 FROM reports
    JOIN projection_backfill_rollbacks rb ON rb.rollback_id=?4 AND rb.state='restored'
    WHERE reports.report_id=?1 AND reports.projection_sha256=?2
      AND reports.projection_object_key=?3`, row.report_id, job.source_projection_sha256,
  job.source_projection_object_key, request.rollback_id);
  if (!restored) return { restored: false, runGroupIds: [] };
  return { restored: true, runGroupIds: [...new Set([
    snapshot.run_group_id,
    ...snapshot.report_runs.map((run) => run.run_group_id),
    ...candidateProjection.runs.map((run) => run.run_group_id),
  ].filter(Boolean))] };
}

async function claimRollback(env) {
  const now = Date.now();
  const candidate = await first(env, `SELECT rollback_id FROM projection_backfill_rollbacks
    WHERE state IN ('pending','retryable_failure') OR
      (state='running' AND updated_unix_millis<=?1)
    ORDER BY created_unix_millis,rollback_id LIMIT 1`, now - LEASE_MILLIS);
  if (!candidate) return null;
  const lease = crypto.randomUUID();
  await env.RLOGS_DB.prepare(`UPDATE projection_backfill_rollbacks SET state='running',lease_token=?2,
    attempt_count=attempt_count+1,updated_unix_millis=?3,failure_code=NULL,failure_detail=NULL
    WHERE rollback_id=?1 AND (state IN ('pending','retryable_failure') OR
      (state='running' AND updated_unix_millis<=?4))`).bind(
    candidate.rollback_id, lease, now, now - LEASE_MILLIS,
  ).run();
  return first(env, `SELECT rb.*,j.state AS job_state,j.report_id,j.upload_id,j.artifact_sha256,
      j.source_projection_sha256,j.source_projection_object_key,
      j.candidate_projection_sha256,j.candidate_projection_object_key,
      j.source_indexes_sha256,j.source_indexes_object_key,j.target_verifier_release
    FROM projection_backfill_rollbacks rb
    JOIN projection_backfill_jobs j ON j.job_id=rb.job_id
    JOIN reports r ON r.report_id=j.report_id
    WHERE rb.rollback_id=?1 AND rb.lease_token=?2 AND rb.state='running'`,
  candidate.rollback_id, lease);
}

export async function runProjectionBackfillRollback(env, context, reconcileRunGroup) {
  const request = await claimRollback(env);
  if (!request) return { claimed: false };
  let outcome;
  try {
    outcome = await rollbackPublishedReplay(env, request);
  } catch (cause) {
    const permanent = cause instanceof TypeError || Number(request.attempt_count) >= 3;
    await env.RLOGS_DB.prepare(`UPDATE projection_backfill_rollbacks SET state=?2,lease_token=NULL,
      failure_code=?3,failure_detail=?4,updated_unix_millis=?5,
      completed_unix_millis=CASE WHEN ?2='rejected' THEN ?5 ELSE NULL END
      WHERE rollback_id=?1 AND lease_token=?6 AND state='running'`).bind(
      request.rollback_id, permanent ? "rejected" : "retryable_failure",
      permanent ? "rollback_evidence_rejected" : "rollback_unavailable", trimDetail(cause),
      Date.now(), request.lease_token,
    ).run();
    return { claimed: true, rejected: permanent, retryable: !permanent };
  }
  if (!outcome.restored) {
    await env.RLOGS_DB.prepare(`UPDATE projection_backfill_rollbacks SET state='superseded',
      lease_token=NULL,failure_code='projection_changed',
      failure_detail='the candidate projection changed before atomic rollback',
      updated_unix_millis=?2,completed_unix_millis=?2
      WHERE rollback_id=?1 AND lease_token=?3 AND state='running'`).bind(
      request.rollback_id, Date.now(), request.lease_token,
    ).run();
    return { claimed: true, restored: false, superseded: true };
  }
  for (const runGroupId of outcome.runGroupIds) {
    const task = reconcileRunGroup(env, runGroupId).catch((cause) =>
      console.error("rLogs rollback reconciliation wake-up failed", runGroupId, cause));
    if (context?.waitUntil) context.waitUntil(task);
  }
  return { claimed: true, restored: true };
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
