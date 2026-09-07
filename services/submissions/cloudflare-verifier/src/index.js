import { Container, ContainerProxy } from "@cloudflare/containers";
import {
  catalogEntry, compatibleProfileName, sameChunkCommitments, validateOutput, validateWakeup,
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

async function verifyJob(request, env) {
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
  try {
    response = await container.fetch(new Request("http://container/internal/v1/verify", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(containerRequest),
    }));
  } catch (cause) {
    return retryJob(env, wakeup.upload_id, "container_unavailable", String(cause?.message ?? cause));
  }
  const result = await response.json().catch(() => null);
  if (!response.ok) {
    const detail = String(result?.error ?? `verifier returned HTTP ${response.status}`).slice(0, 2000);
    return response.status === 422
      ? rejectJob(env, wakeup.upload_id, "replay_rejected", detail)
      : retryJob(env, wakeup.upload_id, "verifier_unavailable", detail);
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
  return json({ accepted: true, report_id: wakeup.expected_report_id, projection_sha256: projectionDigest });
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
}

RLogsVerifierContainer.outboundByHost = {
  "rlogs-artifacts.r2": async (request, env) => {
    if (request.method !== "GET") return new Response(null, { status: 405 });
    const key = decodeURIComponent(new URL(request.url).pathname.slice(1));
    if (!/^uploads\/up_[a-f0-9]{32}\/chunks\/[A-Za-z0-9._-]+$/.test(key)) {
      return new Response(null, { status: 403 });
    }
    const object = await env.RLOGS_ARTIFACTS.get(key);
    return object ? new Response(object.body, { headers: { "Content-Length": String(object.size) } })
      : new Response(null, { status: 404 });
  },
};

export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname;
    if (request.method === "POST" && /^\/v1\/verification-jobs\/up_[a-f0-9]{32}\/run$/.test(path)) {
      return verifyJob(request, env);
    }
    return json({ error: "not found" }, 404);
  },
};
