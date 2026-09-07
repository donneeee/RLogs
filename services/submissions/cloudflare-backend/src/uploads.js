const GAME_PLUGIN_ID = "app.rlogs.game.blue-protocol-star-resonance";
const MAXIMUM_MANIFEST_BYTES = 2 * 1024 * 1024;
const MAXIMUM_UPLOAD_CHUNKS = 16_384;
const MAXIMUM_UPLOAD_CHUNK_BYTES = 16 * 1024 * 1024;
const MAXIMUM_LOG_BYTES = 16 * 1024 * 1024 * 1024;
const DIGEST = /^[a-f0-9]{64}$/;
const IDENTIFIER = /^[A-Za-z0-9_-]{1,128}$/;
const VISIBILITIES = new Set(["private", "unlisted", "public"]);

function json(value, status = 200, headers = {}) {
  return Response.json(value, {
    status,
    headers: {
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      ...headers,
    },
  });
}

function failure(error, status, detail, headers) {
  return json({ error, ...(detail ? { detail } : {}) }, status, headers);
}

function nonempty(value, maximum = 128) {
  return typeof value === "string" && value.length > 0 && value.length <= maximum;
}

function normalizeDigest(value) {
  const digest = typeof value === "string" ? value.toLocaleLowerCase() : "";
  return DIGEST.test(digest) ? digest : null;
}

function canonicalManifest(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("manifest must be a JSON object");
  }
  const metadata = value.metadata;
  if (!metadata || typeof metadata !== "object" || Array.isArray(metadata)) {
    throw new Error("manifest metadata is missing");
  }
  const protocolPackDigest = normalizeDigest(metadata.protocol_pack_digest);
  const privacyPolicyDigest = normalizeDigest(metadata.privacy_policy_digest);
  const sealedLogDigest = normalizeDigest(value.sealed_log_digest);
  if (metadata.schema_version !== 2) throw new Error("submission schema version is unsupported");
  if (metadata.game_plugin_id !== GAME_PLUGIN_ID) throw new Error("game plug-in is unsupported");
  for (const [field, maximum] of [
    ["local_log_id", 128], ["capture_session_id", 128], ["game_region", 64],
    ["client_build", 128],
  ]) {
    if (!nonempty(metadata[field], maximum)) throw new Error(`${field} is invalid`);
  }
  if (!Number.isSafeInteger(metadata.log_format_version) || metadata.log_format_version <= 0) {
    throw new Error("log_format_version is invalid");
  }
  if (!protocolPackDigest || !privacyPolicyDigest || !sealedLogDigest) {
    throw new Error("manifest digests must be 64-character SHA-256 values");
  }
  if (!VISIBILITIES.has(metadata.visibility)) throw new Error("visibility is invalid");
  if (!Array.isArray(value.chunks) || value.chunks.length === 0 || value.chunks.length > MAXIMUM_UPLOAD_CHUNKS) {
    throw new Error("chunk count is outside the supported range");
  }

  let expectedOffset = 0;
  const chunks = value.chunks.map((chunk, index) => {
    const sha256 = normalizeDigest(chunk?.sha256);
    if (!Number.isSafeInteger(chunk?.sequence) || chunk.sequence !== index) {
      throw new Error("chunk sequences must be contiguous and zero based");
    }
    if (!Number.isSafeInteger(chunk.file_offset) || chunk.file_offset !== expectedOffset) {
      throw new Error("chunk offsets must cover the artifact contiguously");
    }
    if (!Number.isSafeInteger(chunk.byte_length) || chunk.byte_length <= 0 ||
        chunk.byte_length > MAXIMUM_UPLOAD_CHUNK_BYTES || !sha256) {
      throw new Error(`chunk ${index} is invalid`);
    }
    expectedOffset += chunk.byte_length;
    if (!Number.isSafeInteger(expectedOffset) || expectedOffset > MAXIMUM_LOG_BYTES) {
      throw new Error("artifact exceeds the maximum supported size");
    }
    return {
      sequence: index,
      file_offset: chunk.file_offset,
      byte_length: chunk.byte_length,
      sha256,
    };
  });

  return {
    metadata: {
      schema_version: 2,
      game_plugin_id: GAME_PLUGIN_ID,
      local_log_id: metadata.local_log_id,
      log_format_version: metadata.log_format_version,
      capture_session_id: metadata.capture_session_id,
      game_region: metadata.game_region,
      client_build: metadata.client_build,
      protocol_pack_digest: protocolPackDigest,
      privacy_policy_digest: privacyPolicyDigest,
      visibility: metadata.visibility,
    },
    chunks,
    sealed_log_digest: sealedLogDigest,
    byte_length: expectedOffset,
    chunk_size: Math.max(...chunks.map((chunk) => chunk.byte_length)),
  };
}

async function parseManifest(request) {
  const contentLength = request.headers.get("Content-Length");
  const declared = contentLength == null ? null : Number(contentLength);
  if (declared != null && Number.isFinite(declared) && declared > MAXIMUM_MANIFEST_BYTES) {
    throw new Error("manifest exceeds the size limit");
  }
  const text = await request.text();
  if (new TextEncoder().encode(text).byteLength > MAXIMUM_MANIFEST_BYTES) {
    throw new Error("manifest exceeds the size limit");
  }
  return canonicalManifest(JSON.parse(text));
}

async function deviceIdentity(request, env) {
  const id = env.AUTH_STATE.idFromName("global");
  const response = await env.AUTH_STATE.get(id).fetch(
    new Request("https://auth.internal/internal/device-identity", {
      headers: { Authorization: request.headers.get("Authorization") ?? "" },
    }),
  );
  if (!response.ok) return { error: failure("write authorization failed", response.status === 401 ? 401 : 503) };
  const identity = await response.json();
  if (!IDENTIFIER.test(identity?.submitter_id ?? "") || !IDENTIFIER.test(identity?.device_id ?? "") ||
      !DIGEST.test(identity?.device_token_hash ?? "") || !identity.account) {
    return { error: failure("authentication service returned an invalid device identity", 503) };
  }
  return { identity };
}

async function run(env, sql, ...values) {
  return env.RLOGS_DB.prepare(sql).bind(...values).run();
}

async function first(env, sql, ...values) {
  return env.RLOGS_DB.prepare(sql).bind(...values).first();
}

async function all(env, sql, ...values) {
  const result = await env.RLOGS_DB.prepare(sql).bind(...values).all();
  return result.results ?? [];
}

async function synchronizeIdentity(env, identity) {
  const account = identity.account;
  if (!Number.isSafeInteger(account.account_id) || !nonempty(account.username, 64) ||
      !DIGEST.test(account.discord_user_hash ?? "") || !nonempty(account.discord_username, 128) ||
      !Number.isSafeInteger(account.created_unix_millis) || !Number.isSafeInteger(account.updated_unix_millis) ||
      !Number.isSafeInteger(identity.device_created_unix_millis)) {
    throw new Error("authentication service returned invalid account metadata");
  }
  await run(env, `INSERT INTO accounts (
      submitter_id, account_id, username, discord_user_hash, discord_username,
      discord_global_name, discord_avatar_url, created_unix_millis, updated_unix_millis
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
    ON CONFLICT(submitter_id) DO UPDATE SET
      account_id=excluded.account_id, username=excluded.username,
      discord_user_hash=excluded.discord_user_hash, discord_username=excluded.discord_username,
      discord_global_name=excluded.discord_global_name, discord_avatar_url=excluded.discord_avatar_url,
      updated_unix_millis=excluded.updated_unix_millis`,
  identity.submitter_id, String(account.account_id), account.username, account.discord_user_hash,
  account.discord_username, account.discord_global_name, account.discord_avatar_url,
  account.created_unix_millis, account.updated_unix_millis);
  await run(env, `INSERT INTO device_tokens (
      token_hash, submitter_id, device_id, created_unix_millis, revoked_unix_millis
    ) VALUES (?1, ?2, ?3, ?4, NULL)
    ON CONFLICT(token_hash) DO UPDATE SET
      submitter_id=excluded.submitter_id, device_id=excluded.device_id`,
  identity.device_token_hash, identity.submitter_id, identity.device_id,
  identity.device_created_unix_millis);
}

function uploadId(digest) {
  return `up_${digest.slice(0, 32)}`;
}

function reportId(digest) {
  return `rpt_${digest.slice(0, 32)}`;
}

function shareUrl(env, id) {
  return `${String(env.WEBSITE_URL).replace(/\/$/, "")}/parses/?parse=${id}#parse`;
}

async function acceptedReceipt(env, session, duplicate) {
  const report = await first(env,
    "SELECT report_id, verification_tier FROM reports WHERE upload_id = ?1",
    session.upload_id,
  );
  if (!report || session.state !== "accepted") return null;
  return json({
    schema_version: 1,
    report_id: report.report_id,
    accepted_log_digest: session.artifact_sha256,
    verification_tier: report.verification_tier,
    share_url: shareUrl(env, report.report_id),
    duplicate,
  });
}

async function beginUpload(request, env) {
  const authenticated = await deviceIdentity(request, env);
  if (authenticated.error) return authenticated.error;
  let manifest;
  try {
    manifest = await parseManifest(request);
  } catch (cause) {
    return failure("invalid upload manifest", 400, String(cause?.message ?? cause));
  }
  await synchronizeIdentity(env, authenticated.identity);
  const digest = manifest.sealed_log_digest;
  const canonical = JSON.stringify({
    metadata: manifest.metadata,
    chunks: manifest.chunks,
    sealed_log_digest: digest,
  });
  let session = await first(env, "SELECT * FROM upload_sessions WHERE artifact_sha256 = ?1", digest);
  if (session) {
    if (session.submitter_id !== authenticated.identity.submitter_id ||
        session.device_id !== authenticated.identity.device_id) {
      return failure("sealed artifact belongs to another authenticated upload owner", 409);
    }
    if (session.manifest_json !== canonical) return failure("upload manifest conflicts with the sealed artifact", 409);
    const receipt = await acceptedReceipt(env, session, true);
    if (receipt) {
      const value = await receipt.json();
      return json({
        schema_version: 1,
        upload_id: null,
        missing_chunks: [],
        existing_report_id: value.report_id,
        share_url: value.share_url,
      });
    }
  } else {
    const now = Date.now();
    const id = uploadId(digest);
    await run(env, `INSERT INTO upload_sessions (
        upload_id, artifact_sha256, submitter_id, device_id, state, byte_length,
        chunk_size, chunk_count, manifest_json, created_unix_millis, updated_unix_millis
      ) VALUES (?1, ?2, ?3, ?4, 'receiving', ?5, ?6, ?7, ?8, ?9, ?9)`,
    id, digest, authenticated.identity.submitter_id, authenticated.identity.device_id,
    manifest.byte_length, manifest.chunk_size, manifest.chunks.length, canonical, now);
    session = await first(env, "SELECT * FROM upload_sessions WHERE upload_id = ?1", id);
  }
  const received = new Set((await all(env,
    "SELECT sequence FROM upload_chunks WHERE upload_id = ?1 ORDER BY sequence",
    session.upload_id,
  )).map((row) => Number(row.sequence)));
  return json({
    schema_version: 1,
    upload_id: session.upload_id,
    missing_chunks: manifest.chunks.filter((chunk) => !received.has(chunk.sequence)).map((chunk) => chunk.sequence),
    existing_report_id: null,
    share_url: null,
  });
}

async function uploadChunk(request, env, id, sequence) {
  const authenticated = await deviceIdentity(request, env);
  if (authenticated.error) return authenticated.error;
  const session = await first(env, "SELECT * FROM upload_sessions WHERE upload_id = ?1", id);
  if (!session) return failure("upload not found", 404);
  if (session.submitter_id !== authenticated.identity.submitter_id || session.device_id !== authenticated.identity.device_id) {
    return failure("upload not found", 404);
  }
  if (session.state !== "receiving") return failure("upload no longer accepts chunks", 409);
  const manifest = JSON.parse(session.manifest_json);
  const descriptor = manifest.chunks[sequence];
  if (!descriptor || descriptor.sequence !== sequence) return failure("chunk is not present in the sealed manifest", 404);
  const contentLength = request.headers.get("Content-Length");
  const declared = contentLength == null ? null : Number(contentLength);
  if (declared != null && Number.isFinite(declared) && declared !== descriptor.byte_length) {
    return failure("chunk byte length mismatch", 400);
  }
  const bytes = await request.arrayBuffer();
  if (bytes.byteLength !== descriptor.byte_length) return failure("chunk byte length mismatch", 400);
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (value) => value.toString(16).padStart(2, "0")).join("");
  if (digest !== descriptor.sha256) return failure("chunk SHA-256 mismatch", 400);
  const existing = await first(env,
    "SELECT sha256, byte_length, object_key FROM upload_chunks WHERE upload_id = ?1 AND sequence = ?2",
    id, sequence,
  );
  if (existing) {
    if (existing.sha256 !== digest || Number(existing.byte_length) !== bytes.byteLength) {
      return failure("chunk conflicts with its prior acknowledgement", 409);
    }
    const object = await env.RLOGS_ARTIFACTS.head(existing.object_key);
    if (!object || Number(object.size) !== bytes.byteLength) return failure("acknowledged chunk storage is unavailable", 503);
    return json({ schema_version: 1, sequence, sha256: digest, duplicate: true });
  }
  const objectKey = `uploads/${id}/chunks/${String(sequence).padStart(8, "0")}-${digest}.bin`;
  await env.RLOGS_ARTIFACTS.put(objectKey, bytes, {
    customMetadata: { upload_id: id, sequence: String(sequence), sha256: digest },
  });
  await run(env, `INSERT INTO upload_chunks (
      upload_id, sequence, sha256, byte_length, object_key, acknowledged_unix_millis
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)`,
  id, sequence, digest, bytes.byteLength, objectKey, Date.now());
  await run(env, "UPDATE upload_sessions SET updated_unix_millis = ?2 WHERE upload_id = ?1", id, Date.now());
  return json({ schema_version: 1, sequence, sha256: digest, duplicate: false });
}

async function finalizeUpload(request, env, id) {
  const authenticated = await deviceIdentity(request, env);
  if (authenticated.error) return authenticated.error;
  let session = await first(env, "SELECT * FROM upload_sessions WHERE upload_id = ?1", id);
  if (!session || session.submitter_id !== authenticated.identity.submitter_id ||
      session.device_id !== authenticated.identity.device_id) return failure("upload not found", 404);
  const receipt = await acceptedReceipt(env, session, true);
  if (receipt) return receipt;
  if (session.state === "rejected") {
    return failure(session.rejection_code ?? "hosted verification rejected the artifact", 422, session.rejection_detail);
  }
  const manifest = JSON.parse(session.manifest_json);
  const chunks = await all(env,
    "SELECT sequence, sha256, byte_length, object_key FROM upload_chunks WHERE upload_id = ?1 ORDER BY sequence",
    id,
  );
  const present = new Map(chunks.map((chunk) => [Number(chunk.sequence), chunk]));
  const missing = manifest.chunks.filter((chunk) => !present.has(chunk.sequence)).map((chunk) => chunk.sequence);
  if (missing.length) return json({ error: "upload is incomplete", missing_chunks: missing }, 409);
  for (const descriptor of manifest.chunks) {
    const chunk = present.get(descriptor.sequence);
    if (chunk.sha256 !== descriptor.sha256 || Number(chunk.byte_length) !== descriptor.byte_length) {
      return failure("stored chunk metadata conflicts with the sealed manifest", 409);
    }
    const object = await env.RLOGS_ARTIFACTS.head(chunk.object_key);
    if (!object || Number(object.size) !== descriptor.byte_length) {
      return failure("stored upload chunk is unavailable", 503, undefined, { "Retry-After": "30" });
    }
  }
  const now = Date.now();
  await run(env, `INSERT INTO verification_jobs (
      upload_id, state, attempt_count, verifier_release, game_build,
      protocol_pack_digest, input_sha256, created_unix_millis, updated_unix_millis
    ) VALUES (?1, 'queued', 0, ?2, ?3, ?4, ?5, ?6, ?6)
    ON CONFLICT(upload_id) DO NOTHING`,
  id, String(env.VERIFIER_RELEASE ?? "pending"), manifest.metadata.client_build,
  manifest.metadata.protocol_pack_digest, session.artifact_sha256, now);
  await run(env, "UPDATE upload_sessions SET state = 'queued', updated_unix_millis = ?2, finalized_unix_millis = COALESCE(finalized_unix_millis, ?2) WHERE upload_id = ?1 AND state IN ('receiving', 'assembled', 'queued')", id, now);

  // The verifier owns the transition to accepted/rejected. Its HTTP response is
  // only a wake-up receipt; this edge worker trusts the shared D1 state after a
  // complete replay, never an uncommitted response body.
  let verifierResponse;
  try {
    verifierResponse = await env.RLOGS_VERIFIER.fetch(
      new Request(`https://verifier.internal/v1/verification-jobs/${encodeURIComponent(id)}/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          schema_version: 1,
          upload_id: id,
          artifact_sha256: session.artifact_sha256,
          expected_report_id: reportId(session.artifact_sha256),
          chunks: chunks.map((chunk) => ({
            sequence: Number(chunk.sequence), object_key: chunk.object_key,
            byte_length: Number(chunk.byte_length), sha256: chunk.sha256,
          })),
        }),
      }),
    );
  } catch (cause) {
    console.error("rLogs hosted verifier wake-up failed", cause);
    verifierResponse = new Response(null, { status: 503 });
  }
  session = await first(env, "SELECT * FROM upload_sessions WHERE upload_id = ?1", id);
  const completed = await acceptedReceipt(env, session, false);
  if (completed) return completed;
  if (session?.state === "rejected") {
    return failure(session.rejection_code ?? "hosted verification rejected the artifact", 422, session.rejection_detail);
  }
  return failure(
    verifierResponse.ok ? "hosted verification is pending" : "hosted verification is temporarily unavailable",
    503,
    undefined,
    { "Retry-After": "30" },
  );
}

export function uploadsEnabled(env) {
  return env.RLOGS_PARSE_UPLOADS_ENABLED === "true" && Boolean(
    env.RLOGS_DB && env.RLOGS_ARTIFACTS && env.RLOGS_VERIFIER && env.AUTH_STATE,
  );
}

export async function routeUpload(request, env, path) {
  if (!uploadsEnabled(env)) return null;
  if (request.method === "POST" && path === "/v1/uploads") return beginUpload(request, env);
  let match = /^\/v1\/uploads\/(up_[a-f0-9]{32})\/chunks\/([0-9]+)$/.exec(path);
  if (request.method === "PUT" && match) {
    const sequence = Number(match[2]);
    return Number.isSafeInteger(sequence) ? uploadChunk(request, env, match[1], sequence) : failure("invalid chunk sequence", 400);
  }
  match = /^\/v1\/uploads\/(up_[a-f0-9]{32})\/finalize$/.exec(path);
  if (request.method === "POST" && match) return finalizeUpload(request, env, match[1]);
  return null;
}

export { canonicalManifest };
