import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";

import {
  PROJECTION_BACKFILL_PAUSE_CODE, PROJECTION_BACKFILL_PAUSE_DETAIL,
  committedProjection, parseRetainedManifest, persistReplay, runProjectionBackfillBatch,
} from "../src/backfill.js";

async function digest(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (value) => value.toString(16).padStart(2, "0")).join("");
}

function sqliteD1(database) {
  const wrap = (sql, values = []) => ({
    sql, values,
    bind(...next) { return wrap(sql, next); },
    async first() { return database.prepare(sql).get(...values) ?? null; },
    async all() { return { results: database.prepare(sql).all(...values) }; },
    async run() { return database.prepare(sql).run(...values); },
    execute() { return database.prepare(sql).run(...values); },
  });
  return {
    prepare: wrap,
    async batch(statements) {
      database.exec("BEGIN IMMEDIATE");
      try {
        for (const statement of statements) statement.execute();
        database.exec("COMMIT");
      } catch (cause) {
        database.exec("ROLLBACK");
        throw cause;
      }
    },
  };
}

function publicationFixture() {
  const database = new DatabaseSync(":memory:");
  database.exec(`
    CREATE TABLE reports (report_id TEXT PRIMARY KEY,upload_id TEXT,artifact_sha256 TEXT,
      visibility TEXT,projection_sha256 TEXT,projection_object_key TEXT,run_group_id TEXT,
      verifier_release TEXT,verified_unix_millis INTEGER);
    CREATE TABLE report_runs (report_id TEXT,run_index INTEGER,run_group_id TEXT,
      catalog_entry_json TEXT,created_unix_millis INTEGER,PRIMARY KEY(report_id,run_index));
    CREATE TABLE report_memberships (report_id TEXT,game_id TEXT,character_id TEXT,
      actor_id TEXT,player_name TEXT,PRIMARY KEY(report_id,game_id,character_id));
    CREATE TABLE projection_backfill_jobs (job_id TEXT PRIMARY KEY,state TEXT,
      candidate_projection_sha256 TEXT,candidate_projection_object_key TEXT,
      candidate_membership_sha256 TEXT,candidate_membership_object_key TEXT,
      failure_code TEXT,failure_detail TEXT,updated_unix_millis INTEGER,completed_unix_millis INTEGER);
    CREATE TABLE report_projection_versions (report_id TEXT,projection_sha256 TEXT,
      projection_object_key TEXT UNIQUE,schema_version INTEGER,verifier_release TEXT,
      artifact_sha256 TEXT,backfill_job_id TEXT,created_unix_millis INTEGER,
      PRIMARY KEY(report_id,projection_sha256));
  `);
  const artifact = "a".repeat(64);
  const reportId = `rpt_${artifact.slice(0, 32)}`;
  const row = {
    report_id: reportId, upload_id: `up_${artifact.slice(0, 32)}`, artifact_sha256: artifact,
    projection_sha256: "b".repeat(64),
    projection_object_key: `reports/${reportId}/projection-${"b".repeat(64)}.json`,
    visibility: "public", verifier_release: "old-release", verified_unix_millis: 41,
  };
  database.prepare("INSERT INTO reports VALUES (?,?,?,?,?,?,?,?,?)").run(
    row.report_id, row.upload_id, row.artifact_sha256, row.visibility, row.projection_sha256,
    row.projection_object_key, "old-group", row.verifier_release, row.verified_unix_millis,
  );
  database.prepare("INSERT INTO report_runs VALUES (?,?,?,?,?)").run(
    row.report_id, 0, "old-group", "{}", 42,
  );
  database.prepare("INSERT INTO report_memberships VALUES (?,?,?,?,?)").run(
    row.report_id, "app.rlogs.game.blue-protocol-star-resonance", "old-character", "1", "Old",
  );
  database.prepare("INSERT INTO projection_backfill_jobs (job_id,state) VALUES (?,?)")
    .run("bfj_test", "running");
  const objects = new Map();
  const env = {
    RLOGS_DB: sqliteD1(database),
    RLOGS_ARTIFACTS: { async put(key, value) { objects.set(key, value); } },
  };
  const original = { schema_version: 12, runs: [{ run_group_id: "old-group" }] };
  const result = {
    report: {
      schema_version: 15, report_id: row.report_id, created_unix_millis: 42,
      deployment_id: "global", region_id: "north-america",
      submission_provenance: { submitter_id: "usr_owner" },
      runs: [{ run_index: 0, run_group_id: "new-group", terminal_state: "completed", participants: [] }],
    },
    membership: {
      character_by_actor: { "7": "new-character" },
      runs: [{ run_index: 0, character_ids: ["new-character"] }],
    },
  };
  return { database, env, row, original, result, objects };
}

test("the v5 migration pause rejects a claimed batch before replay or publication", async () => {
  const artifact = "a".repeat(64);
  const reportId = `rpt_${artifact.slice(0, 32)}`;
  const uploadId = `up_${artifact.slice(0, 32)}`;
  const report = {
    schema_version: 12, report_id: reportId, visibility: "public",
    deployment_id: "global", region_id: "north-america", created_unix_millis: 42,
    verification: { artifact_sha256: artifact, tier: "replayed" },
    submission_provenance: { submitter_id: "usr_owner" }, runs: [],
  };
  const bytes = new TextEncoder().encode(JSON.stringify(report));
  const projection = await digest(bytes);
  const row = {
    report_id: reportId, upload_id: uploadId, artifact_sha256: artifact,
    projection_sha256: projection,
    projection_object_key: `reports/${reportId}/projection-${projection}.json`,
    visibility: "public", verification_tier: "replayed", verifier_release: "old",
    verified_unix_millis: 41, created_unix_millis: 40,
    submitter_id: "usr_owner", manifest_json: "{}",
  };
  const batch = {
    batch_id: "bf_test", target_verifier_release: "new", source_schema_version: 12,
    target_schema_version: 15, maximum_reports: 1, dry_run: 1,
    inspected_count: 0, lease_token: "lease", state: "running",
  };
  const writes = [];
  let artifactReads = 0;
  const env = {
    VERIFIER_RELEASE: "new",
    RLOGS_ARTIFACTS: {
      async get(key) {
        artifactReads += 1;
        assert.equal(key, row.projection_object_key);
        return { async arrayBuffer() { return bytes.buffer; } };
      },
    },
    RLOGS_VERIFIER_CONTAINER: { getByName() { throw new Error("dry run must not invoke replay"); } },
    RLOGS_DB: {
      prepare(sql) {
        return {
          bind(...values) {
            return {
              async first() {
                if (sql.includes("ORDER BY created_unix_millis,batch_id")) return { ...batch, lease_token: null, state: "pending" };
                if (sql.includes("WHERE batch_id=?1 AND lease_token=?2")) return { ...batch, lease_token: values[1] };
                if (sql.includes("SELECT state FROM projection_backfill_jobs")) return { state: "planned" };
                throw new Error(`unexpected first: ${sql}`);
              },
              async all() {
                if (sql.includes("FROM reports r JOIN upload_sessions")) return { results: [row] };
                throw new Error(`unexpected all: ${sql}`);
              },
              async run() { writes.push({ sql, values }); return { success: true }; },
            };
          },
        };
      },
    },
  };
  const result = await runProjectionBackfillBatch(env, {}, () => {
    throw new Error("paused migration must not reconcile");
  });
  assert.deepEqual(result, {
    claimed: true, rejected: true, permanent: true, code: PROJECTION_BACKFILL_PAUSE_CODE,
  });
  assert.equal(artifactReads, 0);
  assert.equal(writes.some(({ sql }) => sql.includes("INSERT INTO projection_backfill_jobs")), false);
  assert.equal(writes.some(({ sql }) => sql.includes("UPDATE reports SET")), false);
  assert.equal(writes.some(({ sql, values }) =>
    sql.includes("state='rejected'") && values.includes(PROJECTION_BACKFILL_PAUSE_CODE) &&
    values.includes(PROJECTION_BACKFILL_PAUSE_DETAIL)), true);
});

test("projection transport failures remain retryable while immutable evidence failures are permanent", async () => {
  const artifact = "a".repeat(64);
  const reportId = `rpt_${artifact.slice(0, 32)}`;
  const uploadId = `up_${artifact.slice(0, 32)}`;
  const invalidJson = new TextEncoder().encode("not-json");
  const projectionSha256 = await digest(invalidJson);
  const row = {
    report_id: reportId, upload_id: uploadId, artifact_sha256: artifact,
    projection_sha256: projectionSha256,
    projection_object_key: `reports/${reportId}/projection-${projectionSha256}.json`,
  };
  const transport = new Error("R2 transport unavailable");
  await assert.rejects(committedProjection({
    RLOGS_ARTIFACTS: { async get() { throw transport; } },
  }, row), (cause) => cause === transport && !(cause instanceof TypeError));
  await assert.rejects(committedProjection({
    RLOGS_ARTIFACTS: { async get() { return null; } },
  }, row), (cause) => cause instanceof TypeError && /missing/u.test(cause.message));
  await assert.rejects(committedProjection({
    RLOGS_ARTIFACTS: { async get() { return { async arrayBuffer() { return invalidJson.buffer; } }; } },
  }, row), (cause) => cause instanceof TypeError && /valid JSON/u.test(cause.message));
  const altered = new TextEncoder().encode("{}");
  await assert.rejects(committedProjection({
    RLOGS_ARTIFACTS: { async get() { return { async arrayBuffer() { return altered.buffer; } }; } },
  }, row), (cause) => cause instanceof TypeError && /digest/u.test(cause.message));
});

test("a malformed retained manifest is a permanent evidence failure", () => {
  assert.throws(() => parseRetainedManifest("{"), (cause) =>
    cause instanceof TypeError && /manifest/u.test(cause.message));
  assert.throws(() => parseRetainedManifest("null"), TypeError);
  assert.deepEqual(parseRetainedManifest('{"schema_version":1}'), { schema_version: 1 });
});

test("operator workflow removes enqueue controls while manual deploy and the paused history remain", async () => {
  const migration = await readFile(new URL("../../cloudflare-backend/migrations/0008_projection_backfills.sql", import.meta.url), "utf8");
  const workflow = await readFile(new URL("../../../../.github/workflows/deploy-cloudflare.yml", import.meta.url), "utf8");
  const worker = await readFile(new URL("../src/backfill.js", import.meta.url), "utf8");
  assert.match(migration, /source_schema_version = 12/u);
  assert.match(migration, /target_schema_version = 15/u);
  assert.match(migration, /maximum_reports BETWEEN 1 AND 25/u);
  assert.match(migration, /consecutive_retry_count BETWEEN 0 AND 3/u);
  assert.match(workflow, /github\.event_name == 'workflow_dispatch'/u);
  assert.match(workflow, /VERIFIER_RELEASE:\$\{\{ github\.sha \}\}/u);
  assert.match(workflow, /npx wrangler deploy --var "VERIFIER_RELEASE:/u);
  assert.doesNotMatch(workflow, /projection_backfill|BACKFILL_MODE|projection_backfill_batches/u);
  assert.ok(workflow.indexOf("npm run db:migrate:remote") < workflow.indexOf("VERIFIER_RELEASE:${{ github.sha }}"));
  assert.match(worker, /PROJECTION_BACKFILL_PAUSE_CODE = "migration_paused_v5"/u);
  assert.match(worker, /state='rejected',[\s\S]+failure_code=\?2,[\s\S]+return \{[\s\S]+permanent: true/u);
  assert.match(worker, /INSERT INTO report_projection_versions/u);
  assert.match(worker, /UPDATE reports SET run_group_id=\?6,[\s\S]+DELETE FROM report_runs/u);
  assert.match(worker, /DELETE FROM report_runs WHERE report_id=\?1 AND \$\{candidateGuard\}/u);
  assert.match(worker, /DELETE FROM report_memberships WHERE report_id=\?1 AND \$\{candidateGuard\}/u);
  assert.match(worker, /UPDATE reports SET run_group_id=\?6,[\s\S]+projection_sha256=\?8/u);
  assert.doesNotMatch(worker, /UPDATE verification_jobs/u);
  assert.match(worker, /consecutive_retry_count \?\? 0\) >= 2/u);
});

test("valid replay atomically advances the pointer and replaces both private indexes", async () => {
  const fixture = publicationFixture();
  const outcome = await persistReplay(fixture.env,
    { target_verifier_release: "new-release" }, fixture.row, fixture.original,
    "bfj_test", fixture.result, { "new-character": "New" },
    "app.rlogs.game.blue-protocol-star-resonance");
  assert.equal(outcome.published, true);
  const report = fixture.database.prepare("SELECT * FROM reports").get();
  assert.equal(report.verifier_release, "new-release");
  assert.equal(report.run_group_id, "new-group");
  assert.equal(report.verified_unix_millis, 41, "original verification evidence timestamp is preserved");
  assert.notEqual(report.projection_sha256, fixture.row.projection_sha256);
  assert.deepEqual([...fixture.database.prepare("SELECT run_group_id FROM report_runs").all()].map((row) => ({ ...row })), [{ run_group_id: "new-group" }]);
  assert.deepEqual([...fixture.database.prepare("SELECT character_id,player_name FROM report_memberships").all()].map((row) => ({ ...row })),
    [{ character_id: "new-character", player_name: "New" }]);
  assert.equal(fixture.database.prepare("SELECT count(*) AS count FROM report_projection_versions").get().count, 2);
  assert.equal(fixture.database.prepare("SELECT state FROM projection_backfill_jobs").get().state, "published");
});

test("a lost source-pointer guard leaves the old catalog and membership intact", async () => {
  const fixture = publicationFixture();
  fixture.database.prepare("UPDATE reports SET projection_sha256=?").run("c".repeat(64));
  const outcome = await persistReplay(fixture.env,
    { target_verifier_release: "new-release" }, fixture.row, fixture.original,
    "bfj_test", fixture.result, {}, "app.rlogs.game.blue-protocol-star-resonance");
  assert.equal(outcome.published, false);
  assert.deepEqual([...fixture.database.prepare("SELECT run_group_id FROM report_runs").all()].map((row) => ({ ...row })), [{ run_group_id: "old-group" }]);
  assert.deepEqual([...fixture.database.prepare("SELECT character_id FROM report_memberships").all()].map((row) => ({ ...row })),
    [{ character_id: "old-character" }]);
  assert.equal(fixture.database.prepare("SELECT count(*) AS count FROM report_projection_versions").get().count, 0);
  assert.equal(fixture.database.prepare("SELECT state FROM projection_backfill_jobs").get().state, "superseded");
});
