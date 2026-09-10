import assert from "node:assert/strict";
import test from "node:test";

import backend from "../src/index.js";

const PACK_A = `sha256:${"a".repeat(64)}`;
const PACK_B = `sha256:${"b".repeat(64)}`;
const RAW_PACK_A = "a".repeat(64);

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
        if (query.includes("LEFT JOIN report_runs")) {
          return { bind() { return { async all() { return { results: [] }; } }; } };
        }
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

test("legacy observed character catalogs fail closed without fabricating presentation authority", async () => {
  const catalog = {
    schema_version: 1,
    characters: [{
      observed_character_key: "chr_example", display_name: "MarieRose",
      class_id: 4, class_name: "Legacy Marksman", specialization_id: 2,
      specialization_name: "Legacy Falconry", reports: [{
        report_id: "rpt_legacy", run_index: 0, scene_id: 6500,
        scene_name: "Legacy scene", terminal_state: "completed",
      }],
    }],
  };
  const response = await backend.fetch(
    new Request("https://backend/v1/characters"),
    environment({ "fs:characters/catalog.v1.json": JSON.stringify(catalog) }),
  );
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.schema_version, 2);
  assert.equal(value.characters[0].presentation_authority, null);
  assert.equal(value.characters[0].class_id, 4);
  assert.equal(value.characters[0].specialization_id, 2);
  assert.equal(value.characters[0].class_name, null);
  assert.equal(value.characters[0].specialization_name, null);
  assert.deepEqual(value.characters[0].reports[0], {
    ...catalog.characters[0].reports[0], deployment_id: null, client_build: null,
    protocol_pack_digest: null, scene_name: null,
  });
});

test("schema-2 observed character authority must match one exact report reference", async () => {
  const authority = {
    deployment_id: "global", client_build: "24687926", protocol_pack_digest: PACK_A,
  };
  const catalog = {
    schema_version: 2,
    characters: [{
      observed_character_key: "chr_example", display_name: "MarieRose",
      presentation_authority: authority, class_id: 4, class_name: "Marksman",
      specialization_id: 2, specialization_name: "Falconry Spec", reports: [{
        report_id: "rpt_current", run_index: 0, scene_id: 6500, scene_name: "Current scene",
        terminal_state: "completed", ...authority,
      }],
    }],
  };
  const value = await (await backend.fetch(
    new Request("https://backend/v1/characters"),
    environment({ "fs:characters/catalog.v1.json": JSON.stringify(catalog) }),
  )).json();
  assert.deepEqual(value.characters[0].presentation_authority, authority);
  assert.equal(value.characters[0].class_name, "Marksman");
  assert.equal(value.characters[0].reports[0].scene_name, "Current scene");

  const malformed = structuredClone(catalog);
  malformed.characters[0].presentation_authority.protocol_pack_digest = `sha256:${"A".repeat(64)}`;
  malformed.characters[0].reports[0].protocol_pack_digest = `sha256:${"A".repeat(64)}`;
  const rejected = await (await backend.fetch(
    new Request("https://backend/v1/characters"),
    environment({ "fs:characters/catalog.v1.json": JSON.stringify(malformed) }),
  )).json();
  assert.equal(rejected.characters[0].presentation_authority, null);
  assert.equal(rejected.characters[0].class_name, null);
  assert.equal(rejected.characters[0].reports[0].protocol_pack_digest, null);
  assert.equal(rejected.characters[0].reports[0].scene_name, null);
});

test("milestone schema-2 canonicalizes exact raw D1 authority and marks unavailable legacy semantics unknown", async () => {
  const authorizedId = "rpt_authorized";
  const legacy = {
    schema_version: 1,
    entries: [authorizedId, "rpt_unavailable"].map((report_id) => ({
      kind: "master_twenty_dungeon", character_id: "3296036", report_id, run_index: 0,
      completed_unix_millis: 1, scene_id: 6500, scene_name: "Legacy scene",
      difficulty_family: "master", difficulty_tier: 20, total_run_time_micros: 10,
    })),
  };
  const env = environment({ "fs:community-milestones.v1.json": JSON.stringify(legacy) });
  env.RLOGS_DB.prepare = (query) => ({ bind(...reportIds) {
    assert.match(query, /r\.game_build AS client_build/u);
    assert.ok(reportIds.includes(authorizedId));
    return { async all() { return { results: [{
      report_id: authorizedId, run_index: 0,
      visibility: "public", verification_tier: "replayed",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }),
      client_build: "24687926", protocol_pack_digest: RAW_PACK_A,
    }] }; } };
  } });
  const value = await (await backend.fetch(
    new Request("https://backend/v1/activity/milestones"), env,
  )).json();
  assert.equal(value.schema_version, 2);
  assert.deepEqual(value.entries[0], {
    ...legacy.entries[0], deployment_id: "global", client_build: "24687926",
    protocol_pack_digest: PACK_A,
  });
  assert.equal(value.entries[1].kind, "unknown");
  assert.equal(value.entries[1].deployment_id, null);
  assert.equal(value.entries[1].client_build, null);
  assert.equal(value.entries[1].protocol_pack_digest, null);
  assert.equal(value.entries[1].scene_name, null);
  assert.equal(value.entries[1].difficulty_family, null);
  assert.equal(value.entries[1].scene_id, 6500);
  assert.equal(value.entries[1].difficulty_tier, 20);
});

test("observed public reads canonicalize raw D1 authority and remove known-ineligible reports", async () => {
  const reportIds = ["rpt_eligible", "rpt_private", "rpt_nonreplayed", "rpt_absent", "rpt_overridden"];
  const catalog = {
    schema_version: 1,
    characters: reportIds.map((report_id) => ({
      observed_character_key: `chr_${report_id}`, display_name: report_id,
      class_id: 4, class_name: "Legacy class", report_count: 1,
      reports: [{ report_id, run_index: 0, created_unix_millis: 1,
        scene_id: 6500, scene_name: `Scene ${report_id}`, terminal_state: "completed" }],
    })),
  };
  const env = environment({ "fs:characters/catalog.v1.json": JSON.stringify(catalog) });
  env.AUTH_STATE.get = () => ({ async fetch() { return Response.json({ rpt_overridden: "private" }); } });
  env.RLOGS_DB.prepare = () => ({ bind() { return { async all() { return { results: [
    { report_id: "rpt_eligible", run_index: 0, visibility: "public", verification_tier: "replayed",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
    { report_id: "rpt_private", run_index: 0, visibility: "private", verification_tier: "replayed",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
    { report_id: "rpt_nonreplayed", run_index: 0, visibility: "public", verification_tier: "corroborated",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
  ] }; } }; } });

  const value = await (await backend.fetch(new Request("https://backend/v1/characters"), env)).json();
  assert.deepEqual(value.characters.map((character) => character.display_name), ["rpt_eligible", "rpt_absent"]);
  assert.equal(value.characters[0].reports[0].scene_name, "Scene rpt_eligible");
  assert.equal(value.characters[0].reports[0].protocol_pack_digest, PACK_A);
  assert.equal(value.characters[1].reports[0].scene_name, null);
  assert.equal(value.characters[1].reports[0].protocol_pack_digest, null);
  assert.equal(value.characters[1].class_id, 4);
  assert.equal(value.characters[1].class_name, null);
});

test("milestone public reads remove D1-known ineligible and overridden entries", async () => {
  const reportIds = ["rpt_eligible", "rpt_private", "rpt_nonreplayed", "rpt_absent", "rpt_overridden"];
  const catalog = {
    schema_version: 1,
    entries: reportIds.map((report_id) => ({
      kind: "master_twenty_dungeon", character_id: report_id, report_id, run_index: 0,
      completed_unix_millis: 1, scene_id: 6500, scene_name: "Legacy scene",
      difficulty_family: "master", difficulty_tier: 20,
    })),
  };
  const env = environment({ "fs:community-milestones.v1.json": JSON.stringify(catalog) });
  env.AUTH_STATE.get = () => ({ async fetch() { return Response.json({ rpt_overridden: "private" }); } });
  env.RLOGS_DB.prepare = () => ({ bind() { return { async all() { return { results: [
    { report_id: "rpt_eligible", run_index: 0, visibility: "public", verification_tier: "replayed",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
    { report_id: "rpt_private", run_index: 0, visibility: "private", verification_tier: "replayed",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
    { report_id: "rpt_nonreplayed", run_index: 0, visibility: "public", verification_tier: "ranked",
      catalog_entry_json: JSON.stringify({ deployment_id: "global" }), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A },
  ] }; } }; } });
  const value = await (await backend.fetch(new Request("https://backend/v1/activity/milestones"), env)).json();
  assert.deepEqual(value.entries.map((entry) => entry.report_id), ["rpt_eligible", "rpt_absent"]);
  assert.equal(value.entries[0].kind, "master_twenty_dungeon");
  assert.equal(value.entries[1].kind, "unknown");
  assert.equal(value.entries[1].scene_id, 6500);
  assert.equal(value.entries[1].scene_name, null);
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
  assert.equal(value.schema_version, 7);
  assert.equal(value.total_entries, 2);
  assert.equal(value.next_offset, 1);
  assert.deepEqual(value.entries, [{
    report_id: "rpt_a",
    region_id: "north-america",
    submitter_id: "usr_owner",
    submitter_name: "Donne",
    client_build: null,
    protocol_pack_digest: null,
    activity_id: null,
    activity_family_id: null,
    activity_category_id: null,
    scene_name: null,
    difficulty_family: null,
  }]);
});

test("legacy catalog rows retain raw scene identity but cannot lend derived facets or filters", async () => {
  const legacy = {
    report_id: "rpt_legacy", run_index: 0, deployment_id: "global", region_id: "global",
    client_build: "uncontracted-build", protocol_pack_digest: "sha256:uncontracted-pack",
    scene_id: 6565, scene_name: "Unproven scene", activity_id: "chaotic",
    activity_family_id: "chaotic", activity_category_id: "dungeons",
    difficulty_family: "master", difficulty_tier: 5, terminal_state: "completed",
  };
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 6, entries: [legacy], facets: {} }),
  });
  const response = await backend.fetch(new Request("https://backend/v1/parses?scene=6565"), env);
  const value = await response.json();
  assert.equal(value.schema_version, 7);
  assert.equal(value.entries[0].scene_id, 6565);
  assert.equal(value.entries[0].difficulty_tier, 5);
  assert.equal(value.entries[0].client_build, null);
  assert.equal(value.entries[0].protocol_pack_digest, null);
  assert.equal(value.entries[0].scene_name, null);
  assert.equal(value.entries[0].activity_id, null);
  assert.equal(value.entries[0].difficulty_family, null);
  assert.deepEqual(value.facets.activities, []);
  assert.deepEqual(value.facets.difficulties, []);
  assert.deepEqual(value.facets.scenes, [{
    id: 6565, label: null, deployment_id: null, client_build: null,
    protocol_pack_digest: null, count: 1,
  }]);

  const derivedFilter = await backend.fetch(new Request("https://backend/v1/parses?activity=chaotic"), env);
  assert.deepEqual((await derivedFilter.json()).entries, []);
});

test("exact schema-7 catalog authority survives without a projection while raw authority fails closed", async () => {
  const base = {
    run_index: 0, deployment_id: "global", client_build: "24687926",
    region_id: "global", scene_id: 6565, scene_name: "Sea-Ringed Reef",
    activity_id: "scene.6565", activity_family_id: "chaotic.6565",
    activity_category_id: "dungeons", difficulty_family: "master",
    terminal_state: "completed",
  };
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 7, entries: [
      { ...base, report_id: "rpt_prefixed", protocol_pack_digest: PACK_A },
      { ...base, report_id: "rpt_raw", protocol_pack_digest: RAW_PACK_A },
    ], facets: {} }),
  });
  const read = env.RLOGS_DATA.get.bind(env.RLOGS_DATA);
  const projectionReads = [];
  env.RLOGS_DATA.get = async (key, type) => {
    if (key.startsWith("fs:projections/")) projectionReads.push(key);
    return read(key, type);
  };
  const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
  const prefixed = value.entries.find((entry) => entry.report_id === "rpt_prefixed");
  const raw = value.entries.find((entry) => entry.report_id === "rpt_raw");
  assert.equal(prefixed.protocol_pack_digest, PACK_A);
  assert.equal(prefixed.client_build, "24687926");
  assert.equal(prefixed.scene_name, "Sea-Ringed Reef");
  assert.equal(raw.protocol_pack_digest, null);
  assert.equal(raw.client_build, null);
  assert.equal(raw.scene_name, null);
  assert.equal(raw.activity_id, null);
  assert.equal(raw.difficulty_family, null);
  assert.deepEqual(projectionReads, ["fs:projections/rpt_raw.json"]);
});

test("stored catalog enrichment requires one exact authoritative report run and replaces stale semantics", async () => {
  const entry = (reportId) => ({
    report_id: reportId, run_index: 3, created_unix_millis: 10,
    deployment_id: "stale", client_build: null, protocol_pack_digest: PACK_B,
    region_id: "global", scene_id: 9999, scene_name: "Stale scene",
    activity_id: "stale.activity", activity_family_id: "stale.family",
    activity_category_id: "stale-category", difficulty_family: "stale-difficulty",
    difficulty_tier: 99, terminal_state: "completed",
  });
  const run = (overrides = {}) => ({
    run_index: 3, scene_id: 6565, scene_name: "Sea-Ringed Reef",
    activity_id: "scene.6565", activity_family_id: "chaotic.6565",
    activity_category_id: "dungeons", difficulty_family: "master", difficulty_tier: 5,
    ...overrides,
  });
  const projection = (reportId, overrides = {}) => ({
    report_id: reportId, visibility: "public", deployment_id: "global",
    client_build: "24687926", protocol_pack_digest: PACK_A, runs: [run()], ...overrides,
  });
  const ids = {
    good: "rpt_stored_good", reportMismatch: "rpt_stored_report_mismatch",
    runMismatch: "rpt_stored_run_mismatch", duplicateRun: "rpt_stored_duplicate_run",
    malformedIdentity: "rpt_stored_malformed_identity", malformedScene: "rpt_stored_malformed_scene",
    malformedProjection: "rpt_stored_malformed_projection", private: "rpt_stored_private",
  };
  const values = {
    "fs:catalog.v1.json": JSON.stringify({
      schema_version: 7, entries: Object.values(ids).map(entry), facets: {},
    }),
    [`fs:projections/${ids.good}.json`]: JSON.stringify(projection(ids.good)),
    [`fs:projections/${ids.reportMismatch}.json`]: JSON.stringify(projection("rpt_other")),
    [`fs:projections/${ids.runMismatch}.json`]: JSON.stringify(projection(ids.runMismatch, { runs: [run({ run_index: 4 })] })),
    [`fs:projections/${ids.duplicateRun}.json`]: JSON.stringify(projection(ids.duplicateRun, { runs: [run(), run()] })),
    [`fs:projections/${ids.malformedIdentity}.json`]: JSON.stringify(projection(ids.malformedIdentity, { protocol_pack_digest: RAW_PACK_A })),
    [`fs:projections/${ids.malformedScene}.json`]: JSON.stringify(projection(ids.malformedScene, { runs: [run({ scene_id: "6565" })] })),
    [`fs:projections/${ids.malformedProjection}.json`]: "{",
    [`fs:projections/${ids.private}.json`]: JSON.stringify(projection(ids.private, { visibility: "private" })),
  };
  const value = await (await backend.fetch(
    new Request("https://backend/v1/parses"), environment(values),
  )).json();
  const byId = new Map(value.entries.map((candidate) => [candidate.report_id, candidate]));
  assert.equal(byId.has(ids.private), false);
  assert.deepEqual({
    deployment_id: byId.get(ids.good).deployment_id,
    client_build: byId.get(ids.good).client_build,
    protocol_pack_digest: byId.get(ids.good).protocol_pack_digest,
    scene_id: byId.get(ids.good).scene_id,
    scene_name: byId.get(ids.good).scene_name,
    activity_id: byId.get(ids.good).activity_id,
    activity_family_id: byId.get(ids.good).activity_family_id,
    activity_category_id: byId.get(ids.good).activity_category_id,
    difficulty_family: byId.get(ids.good).difficulty_family,
    difficulty_tier: byId.get(ids.good).difficulty_tier,
  }, {
    deployment_id: "global", client_build: "24687926", protocol_pack_digest: PACK_A,
    scene_id: 6565, scene_name: "Sea-Ringed Reef", activity_id: "scene.6565",
    activity_family_id: "chaotic.6565", activity_category_id: "dungeons",
    difficulty_family: "master", difficulty_tier: 5,
  });
  for (const id of [ids.reportMismatch, ids.runMismatch, ids.duplicateRun, ids.malformedIdentity, ids.malformedScene, ids.malformedProjection]) {
    assert.equal(byId.get(id).client_build, null, id);
    assert.equal(byId.get(id).protocol_pack_digest, null, id);
    assert.equal(byId.get(id).scene_name, null, id);
    assert.equal(byId.get(id).activity_id, null, id);
    assert.equal(byId.get(id).difficulty_family, null, id);
  }
});

test("stored catalog enrichment applies the current visibility override", async () => {
  const reportId = "rpt_stored_visibility_override";
  const entry = {
    report_id: reportId, run_index: 0, deployment_id: "global", region_id: "global",
    scene_id: 6565, terminal_state: "completed",
  };
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 6, entries: [entry], facets: {} }),
    [`fs:projections/${reportId}.json`]: JSON.stringify({
      report_id: reportId, visibility: "private", deployment_id: "global",
      client_build: "24687926", protocol_pack_digest: PACK_A,
      runs: [{ run_index: 0, scene_id: 6565, scene_name: "Sea-Ringed Reef" }],
    }),
  });
  env.AUTH_STATE.get = () => ({ async fetch() { return Response.json({ [reportId]: "public" }); } });
  const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
  assert.equal(value.entries.length, 1);
  assert.equal(value.entries[0].client_build, "24687926");
  assert.equal(value.entries[0].protocol_pack_digest, PACK_A);
  assert.equal(value.entries[0].scene_name, "Sea-Ringed Reef");
});

test("stored catalog enrichment requires effective public visibility", async () => {
  const cases = [
    ["projection_unlisted", "unlisted", null],
    ["projection_missing", undefined, null],
    ["projection_arbitrary", "friends", null],
    ["override_unlisted", "public", "unlisted"],
    ["override_private", "public", "private"],
  ];
  const entries = cases.map(([suffix]) => ({
    report_id: `rpt_stored_${suffix}`, run_index: 0, deployment_id: "global",
    region_id: "global", scene_id: 6565, terminal_state: "completed",
  }));
  const values = Object.fromEntries(cases.map(([suffix, visibility]) => {
    const projection = {
      report_id: `rpt_stored_${suffix}`, deployment_id: "global",
      client_build: "24687926", protocol_pack_digest: PACK_A,
      runs: [{ run_index: 0, scene_id: 6565, scene_name: "Sea-Ringed Reef" }],
    };
    if (visibility !== undefined) projection.visibility = visibility;
    return [`fs:projections/rpt_stored_${suffix}.json`, JSON.stringify(projection)];
  }));
  values["fs:catalog.v1.json"] = JSON.stringify({ schema_version: 6, entries, facets: {} });
  const env = environment(values);
  env.AUTH_STATE.get = () => ({ async fetch() {
    return Response.json(Object.fromEntries(cases.flatMap(([suffix, , override]) =>
      override === null ? [] : [[`rpt_stored_${suffix}`, override]])));
  } });
  const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
  assert.deepEqual(value.entries, []);
  assert.equal(value.total_entries, 0);
});

test("stored catalog projection enrichment has a fixed KV read bound", async () => {
  const entries = Array.from({ length: 251 }, (_, index) => ({
    report_id: `rpt_bounded_${index}`, run_index: 0, created_unix_millis: index,
    deployment_id: "global", region_id: "global", scene_id: 6565, terminal_state: "completed",
  }));
  const values = Object.fromEntries(entries.map((entry) => [
    `fs:projections/${entry.report_id}.json`, JSON.stringify({
      report_id: entry.report_id, visibility: "public", deployment_id: "global",
      client_build: "24687926", protocol_pack_digest: PACK_A,
      runs: [{ run_index: 0, scene_id: 6565, scene_name: "Sea-Ringed Reef" }],
    }),
  ]));
  values["fs:catalog.v1.json"] = JSON.stringify({ schema_version: 6, entries, facets: {} });
  const env = environment(values);
  const read = env.RLOGS_DATA.get.bind(env.RLOGS_DATA);
  let projectionReads = 0;
  env.RLOGS_DATA.get = async (key, type) => {
    if (key.startsWith("fs:projections/")) projectionReads += 1;
    return read(key, type);
  };
  const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
  assert.equal(projectionReads, 250);
  assert.equal(value.total_entries, 251);
  assert.equal(value.entries.find((entry) => entry.report_id === "rpt_bounded_250").client_build, null);
});

test("mixed localization identities cannot lend a scene facet label", async () => {
  const exact = {
    run_index: 0, deployment_id: "global", client_build: "24687926",
    protocol_pack_digest: PACK_A, region_id: "global", scene_id: 6565,
    scene_name: "Sea-Ringed Reef", terminal_state: "completed",
  };
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 7, entries: [
      { ...exact, report_id: "rpt_a" },
      { ...exact, report_id: "rpt_b", protocol_pack_digest: PACK_B },
    ], facets: {} }),
  });
  const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
  assert.deepEqual(value.facets.scenes, [{
    id: 6565, label: null, deployment_id: null, client_build: null,
    protocol_pack_digest: null, count: 2,
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

test("new hosted catalog rows canonicalize exact raw D1 digests before publication", async () => {
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
    deployment_id: "global", client_build: "stale-build",
    protocol_pack_digest: "sha256:stale-pack",
    region_id: "north-america", scene_id: 6565, scene_name: "Current D1 scene",
    terminal_state: "completed",
  };
  const env = environment({
    "fs:catalog.v1.json": JSON.stringify({ schema_version: 7, entries: [{
      ...entry, created_unix_millis: 1, scene_id: 9999, scene_name: "Legacy KV scene",
    }], facets: {} }),
    [`fs:projections/${reportId}.json`]: JSON.stringify({
      report_id: reportId, visibility: "public", deployment_id: "global",
      client_build: "legacy-build", protocol_pack_digest: PACK_B,
      runs: [{ run_index: 0, scene_id: 9999, scene_name: "Legacy projection scene" }],
    }),
  });
  env.RLOGS_DB.prepare = (query) => {
    if (query.includes("FROM report_runs")) return { async all() { return { results: [{
      catalog_entry_json: JSON.stringify(entry), client_build: "24687926",
      protocol_pack_digest: RAW_PACK_A,
    }] }; } };
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
  assert.deepEqual((await catalog.json()).entries, [{
    ...entry,
    client_build: "24687926",
    protocol_pack_digest: PACK_A,
  }]);
  const projection = await backend.fetch(new Request(`https://backend/v1/parses/${reportId}`), env);
  assert.deepEqual(await projection.json(), {
    ...report,
    runs: [{ run_index: 0, participants: [
      { actor_id: "2", character_id: null, display_name: "MarieRose" },
      { actor_id: "3", character_id: null, display_name: "Remote Player" },
    ] }],
  });
});

test("hosted catalog accepts only exact raw lowercase D1 digests", async () => {
  const invalidAuthorities = [
    { databaseDigest: PACK_A, deploymentId: "global", clientBuild: "24687926" },
    { databaseDigest: "A".repeat(64), deploymentId: "global", clientBuild: "24687926" },
    { databaseDigest: "a".repeat(63), deploymentId: "global", clientBuild: "24687926" },
    { databaseDigest: ` ${RAW_PACK_A}`, deploymentId: "global", clientBuild: "24687926" },
    { databaseDigest: RAW_PACK_A, deploymentId: " ", clientBuild: "24687926" },
    { databaseDigest: RAW_PACK_A, deploymentId: "global", clientBuild: " " },
  ];
  for (const { databaseDigest, deploymentId, clientBuild } of invalidAuthorities) {
    const entry = {
      report_id: `rpt_${"c".repeat(32)}`, run_index: 0, created_unix_millis: 10,
      deployment_id: deploymentId, region_id: "global", scene_id: 6565,
      scene_name: "Sea-Ringed Reef", activity_id: "scene.6565",
      activity_family_id: "chaotic.6565", activity_category_id: "dungeons",
      difficulty_family: "master", terminal_state: "completed",
    };
    const env = environment({ "fs:catalog.v1.json": JSON.stringify({ schema_version: 7, entries: [], facets: {} }) });
    env.RLOGS_DB.prepare = (query) => {
      if (query.includes("FROM report_runs")) return { async all() { return { results: [{
        catalog_entry_json: JSON.stringify(entry), client_build: clientBuild,
        protocol_pack_digest: databaseDigest,
      }] }; } };
      throw new Error(`unexpected query: ${query}`);
    };
    const value = await (await backend.fetch(new Request("https://backend/v1/parses"), env)).json();
    assert.equal(value.entries[0].client_build, null, databaseDigest);
    assert.equal(value.entries[0].protocol_pack_digest, null, databaseDigest);
    assert.equal(value.entries[0].scene_name, null, databaseDigest);
    assert.equal(value.entries[0].activity_id, null, databaseDigest);
    assert.equal(value.entries[0].difficulty_family, null, databaseDigest);
  }
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
