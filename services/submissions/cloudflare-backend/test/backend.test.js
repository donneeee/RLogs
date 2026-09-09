import assert from "node:assert/strict";
import test from "node:test";

import backend from "../src/index.js";

function environment(values = {}) {
  const store = new Map(Object.entries(values));
  return {
    BACKEND_RELEASE: "test-release",
    DISCORD_CLIENT_ID: "discord-client",
    DISCORD_CLIENT_SECRET: "discord-secret",
    AUTH_TOKEN_PEPPER: "test-pepper",
    RLOGS_DATA: {
      async get(key, type) {
        const value = store.get(key);
        if (value == null) return null;
        if (type === "json") return JSON.parse(value);
        if (type === "arrayBuffer") return new TextEncoder().encode(value).buffer;
        return value;
      },
      async list({ prefix }) {
        return {
          keys: [...store.keys()].filter((key) => key.startsWith(prefix)).map((name) => ({ name })),
          list_complete: true,
        };
      },
    },
    AUTH_STATE: {
      idFromName(name) { return name; },
      get() {
        return { async fetch() { return Response.json({ ok: true }); } };
      },
    },
    RLOGS_DB: {
      prepare(query) {
        if (query.includes("FROM report_runs")) {
          return { async all() { return { results: [] }; } };
        }
        if (query.includes("FROM reports r JOIN upload_sessions")) {
          return { bind() { return { async first() { return null; } }; } };
        }
        if (query.includes("FROM report_memberships")) {
          return { bind() { return { async all() { return { results: [] }; } }; } };
        }
        if (query.includes("FROM accounts WHERE submitter_id")) {
          return { bind() { return { async first() { return null; } }; } };
        }
        assert.match(query, /service_metadata/u);
        return {
          bind(component) {
            assert.equal(component, "production-metadata");
            return { async first() { return { schema_version: 1 }; } };
          },
        };
      },
    },
  };
}

test("health proves that Cloudflare storage is populated", async () => {
  const response = await backend.fetch(new Request("https://backend/health"), environment({
    "fs:profiles/catalog.v1.json": JSON.stringify({ schema_version: 1, profiles: [{ profile_id: "prf_a" }] }),
  }));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), {
    status: "ok",
    service: "rlogs-cloudflare-backend",
    schema_version: 1,
    release: "test-release",
    storage: "cloudflare-kv+d1",
    metadata_schema_version: 1,
    public_profile_count: 1,
    capabilities: {
      public_reads: true,
      discord_auth: true,
      profile_sync: true,
      artifact_storage: false,
      hosted_verification: false,
      parse_uploads: false,
    },
  });
});

test("health fails closed when the production metadata schema is unavailable", async () => {
  const env = environment({
    "fs:profiles/catalog.v1.json": JSON.stringify({ schema_version: 1, profiles: [] }),
  });
  env.RLOGS_DB.prepare = () => ({
    bind() { return { async first() { throw new Error("missing migration"); } }; },
  });
  const response = await backend.fetch(new Request("https://backend/health"), env);
  assert.equal(response.status, 503);
  assert.deepEqual(await response.json(), {
    status: "degraded",
    service: "rlogs-cloudflare-backend",
    schema_version: 1,
    release: "test-release",
    storage: "cloudflare-kv+d1",
    metadata_schema_version: null,
    public_profile_count: 0,
    capabilities: {
      public_reads: false,
      discord_auth: true,
      profile_sync: false,
      artifact_storage: false,
      hosted_verification: false,
      parse_uploads: false,
    },
  });
});

test("health does not advertise uploads when bindings appear without explicit promotion", async () => {
  const env = environment({
    "fs:profiles/catalog.v1.json": JSON.stringify({ schema_version: 1, profiles: [] }),
  });
  env.RLOGS_ARTIFACTS = {};
  env.RLOGS_VERIFIER = {};
  const response = await backend.fetch(new Request("https://backend/health"), env);
  assert.equal(response.status, 200);
  assert.deepEqual((await response.json()).capabilities, {
    public_reads: true,
    discord_auth: true,
    profile_sync: true,
    artifact_storage: true,
    hosted_verification: true,
    parse_uploads: false,
  });
});

test("health advertises upload readiness only with promotion and every dependency", async () => {
  const env = environment({
    "fs:profiles/catalog.v1.json": JSON.stringify({ schema_version: 1, profiles: [] }),
  });
  env.RLOGS_ARTIFACTS = {};
  env.RLOGS_VERIFIER = {};
  env.RLOGS_PARSE_UPLOADS_ENABLED = "true";
  const response = await backend.fetch(new Request("https://backend/health"), env);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).capabilities.parse_uploads, true);
});

test("profiles come only from bound Cloudflare storage", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/profiles?character_id=2"),
    environment({
      "fs:profiles/catalog.v1.json": JSON.stringify({
        schema_version: 1,
        profiles: [
          { profile_id: "prf_a", character_id: "1", deployment: "sea", region: "asia" },
          { profile_id: "prf_b", character_id: "2", deployment: "global", region: "north-america" },
        ],
      }),
    }),
  );
  assert.deepEqual((await response.json()).profiles, [{
    profile_id: "prf_b", character_id: "2", deployment: "global", region: "north-america",
    realm: "asteria", world: null,
  }]);
});

test("legacy profiles are normalized to their reviewed server at the public boundary", async () => {
  const profileId = `prf_${"a".repeat(32)}`;
  const profile = {
    profile_id: profileId,
    deployment: "global",
    region: "north-america",
    realm: null,
    world: null,
    envelope: {
      routing: {
        deployment: "global", region: "north-america", "character-id": "3296036",
        realm: null, world: null,
      },
      body: { character: { character_id: "3296036", region: {
        deployment_id: "global", region_id: "north-america", realm_id: null, world_id: null,
      } } },
    },
  };
  const response = await backend.fetch(
    new Request(`https://backend/v1/profiles/${profileId}`),
    environment({ [`fs:profiles/${profileId}/public.json`]: JSON.stringify(profile) }),
  );
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.deployment, "global");
  assert.equal(value.region, "north-america");
  assert.equal(value.realm, "asteria");
  assert.deepEqual(value.envelope.routing, {
    deployment: "global", region: "north-america", "character-id": "3296036", realm: "asteria",
  });
  assert.equal(value.envelope.body.character.region.realm_id, "asteria");
  assert.equal(value.envelope.body.character.region.world_id, null);
});

test("legacy Asteria world evidence repairs a broad Global region at the public boundary", async () => {
  const profileId = `prf_${"d".repeat(32)}`;
  const profile = {
    profile_id: profileId,
    deployment: "global",
    region: "global",
    realm: null,
    world: "Asteria",
    envelope: {
      routing: {
        deployment: "global", region: "global", "character-id": "3296036",
        realm: null, world: "Asteria",
      },
      body: { character: { character_id: "3296036", region: {
        deployment_id: "global", region_id: "global", realm_id: null, world_id: "Asteria",
      } } },
    },
  };
  const response = await backend.fetch(
    new Request(`https://backend/v1/profiles/${profileId}`),
    environment({ [`fs:profiles/${profileId}/public.json`]: JSON.stringify(profile) }),
  );
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.region, "north-america");
  assert.equal(value.realm, "asteria");
  assert.equal(value.envelope.body.character.region.region_id, "north-america");
});

test("profile leaderboard ranks scores and exact dungeon-tier times from D1", async () => {
  const env = environment();
  const queries = [];
  env.RLOGS_DB.prepare = (query) => ({
    bind(...bindings) {
      queries.push({ query, bindings });
      return { async all() {
        if (query.includes("profile_season_rankings")) return { results: [{ display_name: "MarieRose", master_score: 3784 }] };
        return { results: [{ display_name: "MarieRose", activity_id: 6545, tier: 20, pass_time_seconds: 418 }] };
      } };
    },
  });
  const response = await backend.fetch(new Request(
    "https://backend/v1/leaderboards/profiles?season=3&region=north-america&activity=6545&tier=20",
  ), env);
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.season_id, 3);
  assert.equal(value.region_id, "north-america");
  assert.deepEqual(value.master_scores, [{ display_name: "MarieRose", master_score: 3784 }]);
  assert.deepEqual(value.dungeon_times, [{ display_name: "MarieRose", activity_id: 6545, tier: 20, pass_time_seconds: 418 }]);
  assert.deepEqual(queries.map((entry) => entry.bindings), [
    [3, "north-america", 100],
    [3, 6545, 20, "north-america", 100],
  ]);
});

test("training leaderboard supports season, region, class, and spec filters", async () => {
  const env = environment();
  let captured = null;
  env.RLOGS_DB.prepare = (query) => ({
    bind(...bindings) {
      captured = { query, bindings };
      return { async all() { return { results: [{ dps: 1_000 }] }; } };
    },
  });
  const response = await backend.fetch(new Request(
    "https://backend/v1/leaderboards/training-dummy?season=3&region=north-america&class=4&specialization=41",
  ), env);
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.duration_micros, 180_000_000);
  assert.equal(value.results[0].dps, 1_000);
  assert.match(captured.query, /FROM training_dummy_results/u);
  assert.deepEqual(captured.bindings, [3, "north-america", 4, 41, 100]);
});

test("observed character directory comes only from its materialized Cloudflare catalog", async () => {
  const catalog = {
    schema_version: 1,
    characters: [{ observed_character_key: "chr_example", display_name: "MarieRose" }],
  };
  const response = await backend.fetch(
    new Request("https://backend/v1/characters"),
    environment({ "fs:characters/catalog.v1.json": JSON.stringify(catalog) }),
  );
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), catalog);
});

test("parse catalog applies public filters and pagination", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/parses?region=north-america&limit=1"),
    environment({
      "fs:catalog.v1.json": JSON.stringify({
        schema_version: 6,
        total_entries: 2,
        offset: 0,
        next_offset: null,
        entries: [
          { report_id: "rpt_a", region_id: "north-america", submitter_id: "usr_owner" },
          { report_id: "rpt_b", region_id: "north-america" },
          { report_id: "rpt_c", region_id: "global" },
        ],
        facets: {},
      }),
      "fs:accounts/users/usr_owner.json": JSON.stringify({
        username: "donne",
        discord_global_name: "Donne",
      }),
    }),
  );
  const value = await response.json();
  assert.equal(value.total_entries, 2);
  assert.equal(value.next_offset, 1);
  assert.deepEqual(value.entries, [{
    report_id: "rpt_a",
    region_id: "north-america",
    submitter_id: "usr_owner",
    submitter_name: "Donne",
  }]);
});

test("private visibility overrides disappear from public catalogs and reports", async () => {
  const reportId = `rpt_${"a".repeat(32)}`;
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 6, entries: [{ report_id: reportId }], facets: {} }),
    [`fs:projections/${reportId}.json`]: JSON.stringify({ report_id: reportId, visibility: "public" }),
  });
  env.AUTH_STATE.get = () => ({
    async fetch() { return Response.json({ [reportId]: "private" }); },
  });
  const catalogResponse = await backend.fetch(new Request("https://backend/v1/parses"), env);
  assert.deepEqual((await catalogResponse.json()).entries, []);
  const reportResponse = await backend.fetch(new Request(`https://backend/v1/parses/${reportId}`), env);
  assert.equal(reportResponse.status, 404);
});

test("new hosted reports and catalog rows are read from D1 and R2", async () => {
  const reportId = `rpt_${"b".repeat(32)}`;
  const report = {
    report_id: reportId,
    visibility: "public",
    runs: [{ run_index: 0, participants: [
      { actor_id: "2", character_id: null, display_name: null },
      { actor_id: "3", character_id: null, display_name: "Remote Player" },
    ] }],
  };
  const entry = {
    report_id: reportId, run_index: 0, created_unix_millis: 10,
    region_id: "north-america", terminal_state: "completed",
  };
  const env = environment({ "fs:catalog.v1.json": JSON.stringify({ schema_version: 6, entries: [], facets: {} }) });
  env.RLOGS_DB.prepare = (query) => {
    if (query.includes("FROM report_runs")) return { async all() { return { results: [{ catalog_entry_json: JSON.stringify(entry) }] }; } };
    if (query.includes("FROM reports r JOIN upload_sessions")) return { bind() { return { async first() {
      return { visibility: "public", projection_object_key: "reports/new.json", submitter_id: "usr_owner" };
    } }; } };
    if (query.includes("FROM report_memberships")) return { bind(actualReportId, submitterId) {
      assert.equal(actualReportId, reportId);
      assert.equal(submitterId, "usr_owner");
      return { async all() { return { results: [{
        actor_id: "2",
        public_projection_json: JSON.stringify({ display_name: "MarieRose" }),
      }] }; } };
    } };
    if (query.includes("FROM accounts WHERE submitter_id")) return { bind() { return { async first() { return null; } }; } };
    throw new Error(`unexpected query: ${query}`);
  };
  env.RLOGS_ARTIFACTS = { async get(key) {
    assert.equal(key, "reports/new.json");
    return { async json() { return report; } };
  } };
  const catalog = await backend.fetch(new Request("https://backend/v1/parses"), env);
  assert.deepEqual((await catalog.json()).entries, [entry]);
  const projection = await backend.fetch(new Request(`https://backend/v1/parses/${reportId}`), env);
  assert.deepEqual(await projection.json(), {
    ...report,
    runs: [{ run_index: 0, participants: [
      { actor_id: "2", character_id: null, display_name: "MarieRose" },
      { actor_id: "3", character_id: null, display_name: "Remote Player" },
    ] }],
  });
});

test("run-group reconciliation reads the current public D1 pointer from R2", async () => {
  const runGroupId = "run_exact_group";
  const reconciliation = {
    schema_version: 16, reconciliation_id: `rec_${"a".repeat(32)}`, run_group_id: runGroupId,
  };
  const env = environment({
    [`fs:reconciliations/${runGroupId}.json`]: JSON.stringify({ legacy: true }),
  });
  env.RLOGS_DB.prepare = (query) => ({ bind(actualRunGroupId) {
    assert.equal(actualRunGroupId, runGroupId);
    assert.match(query, /reconciliation_current/u);
    assert.match(query, /r\.visibility<>'public'/u);
    assert.match(query, /COUNT\(\*\) FROM reconciliation_job_sources/u);
    assert.match(query, /COUNT\(\*\) FROM report_runs rr/u);
    return { async first() { return { projection_object_key: "reconciliations/version.json" }; } };
  } });
  env.RLOGS_ARTIFACTS = { async get(key) {
    assert.equal(key, "reconciliations/version.json");
    return { async text() { return JSON.stringify(reconciliation); } };
  } };
  const response = await backend.fetch(new Request(
    `https://backend/v1/run-groups/${runGroupId}/reconciliation`,
  ), env);
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), reconciliation);
});

test("run-group reconciliation falls back to the legacy object only without hosted bindings", async () => {
  const runGroupId = "run_legacy_group";
  const legacy = { schema_version: 15, run_group_id: runGroupId };
  const env = environment({ [`fs:reconciliations/${runGroupId}.json`]: JSON.stringify(legacy) });
  delete env.RLOGS_DB;
  const response = await backend.fetch(new Request(
    `https://backend/v1/run-groups/${runGroupId}/reconciliation`,
  ), env);
  assert.deepEqual(await response.json(), legacy);
});

test("an unsafe hosted reconciliation never falls back to legacy data after a source becomes private", async () => {
  const runGroupId = "run_private_source";
  const env = environment({ [`fs:reconciliations/${runGroupId}.json`]: JSON.stringify({ legacy: true }) });
  env.RLOGS_DB.prepare = (query) => ({ bind() {
    assert.match(query, /r\.visibility<>'public'/u);
    return { async first() { return null; } };
  } });
  env.RLOGS_ARTIFACTS = { async get() { throw new Error("unsafe pointer must not reach R2"); } };
  const response = await backend.fetch(new Request(
    `https://backend/v1/run-groups/${runGroupId}/reconciliation`,
  ), env);
  assert.equal(response.status, 404);
});

test("a hosted pointer fails closed when a newly public replay source is absent from its source set", async () => {
  const runGroupId = "run_new_source";
  const env = environment({ [`fs:reconciliations/${runGroupId}.json`]: JSON.stringify({ legacy: true }) });
  env.RLOGS_DB.prepare = (query) => ({ bind() {
    assert.match(query, /COUNT\(\*\) FROM reconciliation_job_sources/u);
    assert.match(query, /rr\.run_group_id=c\.run_group_id AND r\.visibility='public'/u);
    return { async first() { return null; } };
  } });
  env.RLOGS_ARTIFACTS = { async get() { throw new Error("stale pointer must not reach R2"); } };
  const response = await backend.fetch(new Request(
    `https://backend/v1/run-groups/${runGroupId}/reconciliation`,
  ), env);
  assert.equal(response.status, 404);
});

test("hosted reconciliation storage errors return retryable failure without legacy fallback", async () => {
  const runGroupId = "run_storage_error";
  const env = environment({ [`fs:reconciliations/${runGroupId}.json`]: JSON.stringify({ legacy: true }) });
  env.RLOGS_DB.prepare = () => ({ bind() { return { async first() { throw new Error("D1 offline"); } }; } });
  env.RLOGS_ARTIFACTS = {};
  const response = await backend.fetch(new Request(
    `https://backend/v1/run-groups/${runGroupId}/reconciliation`,
  ), env);
  assert.equal(response.status, 503);
  assert.equal(response.headers.get("Retry-After"), "30");
});

test("hosted name enrichment rejects placeholders and malformed profile projections", async () => {
  const reportId = `rpt_${"c".repeat(32)}`;
  const report = { report_id: reportId, runs: [{ participants: [
    { actor_id: "2", display_name: null },
    { actor_id: "3", display_name: null },
  ] }] };
  const env = environment();
  env.RLOGS_DB.prepare = (query) => {
    if (query.includes("FROM reports r JOIN upload_sessions")) return { bind() { return { async first() {
      return { visibility: "public", projection_object_key: "reports/placeholders.json", submitter_id: "usr_owner" };
    } }; } };
    if (query.includes("FROM report_memberships")) return { bind() { return { async all() { return { results: [
      { actor_id: "2", public_projection_json: JSON.stringify({ display_name: "Player 5" }) },
      { actor_id: "3", public_projection_json: "not-json" },
    ] }; } }; } };
    throw new Error(`unexpected query: ${query}`);
  };
  env.RLOGS_ARTIFACTS = { async get() { return { async json() { return report; } }; } };
  const response = await backend.fetch(new Request(`https://backend/v1/parses/${reportId}`), env);
  assert.deepEqual(await response.json(), { ...report, visibility: "public" });
});

test("write routes fail closed until hosted verification is enabled", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/uploads", { method: "POST" }),
    environment(),
  );
  assert.equal(response.status, 503);
  assert.equal(response.headers.get("Retry-After"), "30");
});

test("authentication is delegated to strongly consistent state", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/auth/me", { headers: { Authorization: "Bearer rlw_test" } }),
    environment(),
  );
  assert.deepEqual(await response.json(), { ok: true });
});
