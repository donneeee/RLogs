import assert from "node:assert/strict";
import test from "node:test";

import { canonicalManifest, routeUpload, uploadsEnabled } from "../src/uploads.js";

const digest = (character) => character.repeat(64);

function manifest(bytes, chunkDigest) {
  return {
    metadata: {
      schema_version: 2,
      game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
      local_log_id: "log_fixture",
      log_format_version: 2,
      capture_session_id: "capture_fixture",
      game_region: "north-america",
      client_build: "24687926",
      protocol_pack_digest: digest("a"),
      privacy_policy_digest: digest("b"),
      visibility: "public",
    },
    chunks: [{ sequence: 0, file_offset: 0, byte_length: bytes.length, sha256: chunkDigest }],
    sealed_log_digest: digest("c"),
  };
}

class FakeD1 {
  constructor() {
    this.sessions = new Map();
    this.chunks = new Map();
    this.reports = new Map();
    this.jobs = new Map();
  }

  prepare(sql) {
    return { bind: (...values) => ({
      first: async () => this.first(sql, values),
      all: async () => ({ results: this.all(sql, values) }),
      run: async () => this.run(sql, values),
    }) };
  }

  first(sql, values) {
    if (sql.includes("FROM upload_sessions WHERE artifact_sha256")) {
      return [...this.sessions.values()].find((row) => row.artifact_sha256 === values[0]) ?? null;
    }
    if (sql.includes("FROM upload_sessions WHERE upload_id")) return this.sessions.get(values[0]) ?? null;
    if (sql.includes("FROM reports WHERE upload_id")) {
      return [...this.reports.values()].find((row) => row.upload_id === values[0]) ?? null;
    }
    if (sql.includes("FROM upload_chunks WHERE upload_id") && sql.includes("AND sequence")) {
      return this.chunks.get(`${values[0]}:${values[1]}`) ?? null;
    }
    throw new Error(`unhandled first query: ${sql}`);
  }

  all(sql, values) {
    if (!sql.includes("FROM upload_chunks WHERE upload_id")) throw new Error(`unhandled all query: ${sql}`);
    return [...this.chunks.values()]
      .filter((row) => row.upload_id === values[0])
      .sort((left, right) => left.sequence - right.sequence);
  }

  run(sql, values) {
    if (sql.includes("INSERT INTO accounts") || sql.includes("INSERT INTO device_tokens")) return { success: true };
    if (sql.includes("INSERT INTO upload_sessions")) {
      const [upload_id, artifact_sha256, submitter_id, device_id, byte_length, chunk_size,
        chunk_count, manifest_json, now] = values;
      this.sessions.set(upload_id, {
        upload_id, artifact_sha256, submitter_id, device_id, state: "receiving", byte_length,
        chunk_size, chunk_count, manifest_json, artifact_object_key: null,
        rejection_code: null, rejection_detail: null, created_unix_millis: now,
        updated_unix_millis: now, finalized_unix_millis: null,
      });
      return { success: true };
    }
    if (sql.includes("INSERT INTO upload_chunks")) {
      const [upload_id, sequence, sha256, byte_length, object_key, acknowledged_unix_millis] = values;
      this.chunks.set(`${upload_id}:${sequence}`, {
        upload_id, sequence, sha256, byte_length, object_key, acknowledged_unix_millis,
      });
      return { success: true };
    }
    if (sql.includes("UPDATE upload_sessions SET updated_unix_millis")) {
      this.sessions.get(values[0]).updated_unix_millis = values[1];
      return { success: true };
    }
    if (sql.includes("INSERT INTO verification_jobs")) {
      if (!this.jobs.has(values[0])) this.jobs.set(values[0], { upload_id: values[0], state: "queued" });
      return { success: true };
    }
    if (sql.includes("UPDATE upload_sessions SET state = 'queued'")) {
      const row = this.sessions.get(values[0]);
      if (["receiving", "assembled", "queued"].includes(row.state)) row.state = "queued";
      row.updated_unix_millis = values[1];
      row.finalized_unix_millis ??= values[1];
      return { success: true };
    }
    throw new Error(`unhandled run query: ${sql}`);
  }
}

async function sha256(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (value) => value.toString(16).padStart(2, "0")).join("");
}

function environment(onVerify = async () => new Response(null, { status: 202 })) {
  const db = new FakeD1();
  const objects = new Map();
  const env = {
    RLOGS_PARSE_UPLOADS_ENABLED: "true",
    WEBSITE_URL: "https://rlogs-app.github.io",
    RLOGS_DB: db,
    RLOGS_ARTIFACTS: {
      async put(key, bytes, options) { objects.set(key, { bytes: bytes.slice(0), options }); },
      async head(key) {
        const object = objects.get(key);
        return object ? { size: object.bytes.byteLength, customMetadata: object.options.customMetadata } : null;
      },
    },
    RLOGS_VERIFIER: { fetch: (request) => onVerify(request, db) },
    AUTH_STATE: {
      idFromName: () => "global",
      get: () => ({ fetch: async (request) => {
        if (request.headers.get("Authorization") !== "Bearer rld_valid") {
          return Response.json({ error: "unauthorized" }, { status: 401 });
        }
        return Response.json({
          schema_version: 1,
          submitter_id: "usr_fixture",
          device_id: "dev_fixture",
          device_token_hash: digest("d"),
          device_created_unix_millis: 1,
          account: {
            account_id: 100000000001,
            username: "fixture",
            discord_user_hash: digest("e"),
            discord_username: "Fixture",
            discord_global_name: null,
            discord_avatar_url: null,
            created_unix_millis: 1,
            updated_unix_millis: 1,
          },
        });
      } }),
    },
  };
  return { env, db, objects };
}

function authorized(url, init = {}) {
  return new Request(url, {
    ...init,
    headers: { Authorization: "Bearer rld_valid", ...(init.headers ?? {}) },
  });
}

test("manifest validation enforces contiguous sealed chunks", () => {
  const value = manifest(new Uint8Array([1]), digest("a"));
  value.chunks[0].file_offset = 1;
  assert.throws(() => canonicalManifest(value), /contiguously/u);
  value.chunks[0].file_offset = 0;
  value.chunks[0].sequence = 1;
  assert.throws(() => canonicalManifest(value), /zero based/u);
});

test("upload routes remain disabled without the explicit promotion flag", async () => {
  const { env } = environment();
  delete env.RLOGS_PARSE_UPLOADS_ENABLED;
  assert.equal(uploadsEnabled(env), false);
  assert.equal(await routeUpload(new Request("https://backend/v1/uploads", { method: "POST" }), env, "/v1/uploads"), null);
});

test("authenticated chunks resume idempotently and reject altered bytes", async () => {
  const bytes = new Uint8Array([1, 2, 3, 4]);
  const chunkDigest = await sha256(bytes);
  const body = JSON.stringify(manifest(bytes, chunkDigest));
  const { env, objects } = environment();
  const begin = await routeUpload(authorized("https://backend/v1/uploads", { method: "POST", body }), env, "/v1/uploads");
  assert.equal(begin.status, 200);
  const started = await begin.json();
  assert.deepEqual(started.missing_chunks, [0]);
  const chunkPath = `/v1/uploads/${started.upload_id}/chunks/0`;
  const bad = await routeUpload(authorized(`https://backend${chunkPath}`, {
    method: "PUT", body: new Uint8Array([9, 9, 9, 9]),
  }), env, chunkPath);
  assert.equal(bad.status, 400);
  const first = await routeUpload(authorized(`https://backend${chunkPath}`, { method: "PUT", body: bytes }), env, chunkPath);
  assert.deepEqual(await first.json(), { schema_version: 1, sequence: 0, sha256: chunkDigest, duplicate: false });
  assert.equal(objects.size, 1);
  const duplicate = await routeUpload(authorized(`https://backend${chunkPath}`, { method: "PUT", body: bytes }), env, chunkPath);
  assert.equal((await duplicate.json()).duplicate, true);
  assert.equal(objects.size, 1);
  const resumed = await routeUpload(authorized("https://backend/v1/uploads", { method: "POST", body }), env, "/v1/uploads");
  assert.deepEqual((await resumed.json()).missing_chunks, []);
});

test("a verifier HTTP success cannot accept a parse without committed replay state", async () => {
  const bytes = new Uint8Array([5, 6, 7]);
  const body = JSON.stringify(manifest(bytes, await sha256(bytes)));
  const { env, db } = environment(async () => Response.json({ accepted: true }));
  const started = await (await routeUpload(authorized("https://backend/v1/uploads", { method: "POST", body }), env, "/v1/uploads")).json();
  const chunkPath = `/v1/uploads/${started.upload_id}/chunks/0`;
  await routeUpload(authorized(`https://backend${chunkPath}`, { method: "PUT", body: bytes }), env, chunkPath);
  const finalizePath = `/v1/uploads/${started.upload_id}/finalize`;
  const finalized = await routeUpload(authorized(`https://backend${finalizePath}`, { method: "POST" }), env, finalizePath);
  assert.equal(finalized.status, 503);
  assert.equal((await finalized.json()).error, "hosted verification is pending");
  assert.equal(db.sessions.get(started.upload_id).state, "queued");
});

test("finalization returns a receipt only after the verifier commits replay acceptance", async () => {
  const bytes = new Uint8Array([8, 9]);
  const body = JSON.stringify(manifest(bytes, await sha256(bytes)));
  const { env } = environment(async (request, db) => {
    const value = await request.json();
    const session = db.sessions.get(value.upload_id);
    session.state = "accepted";
    db.reports.set(value.expected_report_id, {
      report_id: value.expected_report_id,
      upload_id: value.upload_id,
      verification_tier: "replayed",
    });
    return Response.json({ schema_version: 1, queued: false });
  });
  const started = await (await routeUpload(authorized("https://backend/v1/uploads", { method: "POST", body }), env, "/v1/uploads")).json();
  const chunkPath = `/v1/uploads/${started.upload_id}/chunks/0`;
  await routeUpload(authorized(`https://backend${chunkPath}`, { method: "PUT", body: bytes }), env, chunkPath);
  const finalizePath = `/v1/uploads/${started.upload_id}/finalize`;
  const finalized = await routeUpload(authorized(`https://backend${finalizePath}`, { method: "POST" }), env, finalizePath);
  assert.equal(finalized.status, 200);
  assert.deepEqual(await finalized.json(), {
    schema_version: 1,
    report_id: `rpt_${digest("c").slice(0, 32)}`,
    accepted_log_digest: digest("c"),
    verification_tier: "replayed",
    share_url: `https://rlogs-app.github.io/parses/?parse=rpt_${digest("c").slice(0, 32)}#parse`,
    duplicate: false,
  });
});
