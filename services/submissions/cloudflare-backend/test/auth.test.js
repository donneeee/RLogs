import assert from "node:assert/strict";
import test from "node:test";

import { accountView, catalogEntry, RLogsAuthState, tokenHash } from "../src/auth.js";
import { canonicalJson, canonicalPublishedRouting, liveCaptureProof, profileLeaderboardProjection, reconcileCatalog, reconcilePublishedRouting } from "../src/profile.js";

const PACK_A = `sha256:${"a".repeat(64)}`;
const RAW_PACK_A = "a".repeat(64);

function authFixture() {
  const durable = new Map();
  const kv = new Map();
  const d1 = [];
  const backgroundTasks = [];
  durable.set("user:usr_owner", {
    submitter_id: "usr_owner",
    account_id: 100000000001,
    username: "fixture",
    discord_user_id: "123456789",
    discord_username: "Fixture",
    discord_global_name: "Fixture User",
    discord_avatar_url: null,
    publish_verified_parses: false,
    created_unix_millis: 50,
    updated_unix_millis: 90,
  });
  const storage = {
    async get(key) { return durable.get(key); },
    async put(key, value) {
      if (typeof key === "object") for (const [entryKey, entryValue] of Object.entries(key)) durable.set(entryKey, entryValue);
      else durable.set(key, value);
    },
    async delete(key) { durable.delete(key); },
    async list({ prefix }) { return new Map([...durable].filter(([key]) => key.startsWith(prefix))); },
  };
  const env = {
    WEBSITE_URL: "https://rlogs-app.github.io",
    AUTH_TOKEN_PEPPER: "0123456789abcdef0123456789abcdef",
    RLOGS_DATA: {
      async get(key, type) {
        const value = kv.get(key);
        if (value == null) return null;
        return type === "text" ? JSON.stringify(value) : structuredClone(value);
      },
      async list({ prefix }) {
        return { keys: [...kv.keys()].filter((key) => key.startsWith(prefix)).map((name) => ({ name })), list_complete: true };
      },
      async put(key, value) { kv.set(key, typeof value === "string" ? JSON.parse(value) : value); },
    },
    RLOGS_DB: {
      prepare(query) {
        return {
          async all() { return { results: [] }; },
          bind(...bindings) {
            return {
              query,
              bindings,
              async first() { return null; },
              async all() { return { results: [] }; },
              async run() {
                d1.push({ query, bindings });
                return { success: true };
              },
            };
          },
        };
      },
      async batch(statements) {
        d1.push(...statements);
        return statements.map(() => ({ success: true }));
      },
    },
  };
  const auth = new RLogsAuthState({ storage, waitUntil(task) { backgroundTasks.push(task); } }, env);
  auth.authenticateWeb = async () => ({ submitter_id: "usr_owner" });
  return { auth, durable, kv, d1, backgroundTasks };
}

test("token hashes remain compatible with the Rust authentication domain separator", async () => {
  assert.equal(
    await tokenHash("web-session", "rlw_example", "0123456789abcdef0123456789abcdef"),
    "e338255bf6b06636cebe145525ead0d3a0ef95891ca6fe2cb606d9bddf155b9c",
  );
});

test("the internal upload identity binds a device token to its current account", async () => {
  const { auth, durable } = authFixture();
  const token = "rld_upload-fixture";
  const deviceHash = await tokenHash("device-token", token, auth.env.AUTH_TOKEN_PEPPER);
  durable.set(`device:${deviceHash}`, {
    submitter_id: "usr_owner",
    device_id: "dev_upload",
    created_unix_millis: 100,
    revoked_unix_millis: null,
  });
  durable.set("user:usr_owner", {
    submitter_id: "usr_owner",
    account_id: 100000000001,
    username: "fixture",
    discord_user_id: "123456789",
    discord_username: "Fixture",
    discord_global_name: "Fixture User",
    discord_avatar_url: null,
    created_unix_millis: 50,
    updated_unix_millis: 90,
  });
  const response = await auth.fetch(new Request("https://auth.internal/internal/device-identity", {
    headers: { Authorization: `Bearer ${token}` },
  }));
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.submitter_id, "usr_owner");
  assert.equal(value.device_id, "dev_upload");
  assert.equal(value.device_token_hash, deviceHash);
  assert.equal(value.account.username, "fixture");
  assert.equal(value.account.discord_user_hash,
    await tokenHash("discord-user", "123456789", auth.env.AUTH_TOKEN_PEPPER));
  assert.equal(value.account.publish_verified_parses, false);
  assert.equal("discord_user_id" in value.account, false);
});

test("profile package digest and live-capture proof match the Rust implementation", async () => {
  const request = {
    relative_endpoint: "/v1/games/blue-protocol-star-resonance/profiles",
    payload: {
      schema_version: 1,
      game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
      payload_kind: "character-profile",
      payload_schema_id: "app.rlogs.bpsr.character-profile",
      payload_schema_version: 1,
      routing: { "character-id": "3296036", deployment: "global", region: "north-america" },
      body: { character: { character_id: "3296036", region: { deployment_id: "global", realm_id: null, region_id: "north-america", world_id: null } }, display_name: "MarieRose" },
    },
  };
  const packageValue = {
    schema_version: 2,
    package_id: "7a62005bd2e7243b05ea6fd1e8d3be3868f4b478c8aaa607a0609580c841c2b5",
    created_unix_millis: 100,
    source: {
      session_id: "session-one", client_build: "24687926",
      protocol_pack_digest: `sha256:${"a".repeat(64)}`,
      canonical_content_sha256: `sha256:${"b".repeat(64)}`,
      observation_count: 2, last_event_sequence: 3,
    },
    request,
  };
  assert.equal(await digest(canonicalJson(request)), packageValue.package_id);
  assert.equal(
    await liveCaptureProof(packageValue, "dev_device", "rld_device-secret"),
    "hmac-sha256:55c4ebc91d72d6ba25909fbc7c6ac3c4d15a28dcecf1c47b422e1c9e4a98983a",
  );
});

test("profile leaderboard projection distinguishes seasonal activities from Master tiers", () => {
  const projection = profileLeaderboardProjection({
    envelope: { body: {
      season: { season_id: 3 },
      master_score: 3784,
      activity_progress: { master_mode_dungeons: [
        { season_id: 3, difficulty_id: 6545, dungeon: { dungeon_id: 20, pass_time: 418, score: 700, completion_count: 2 } },
        { season_id: 3, difficulty_id: 6545, dungeon: { dungeon_id: 21, pass_time: 300, score: 701 } },
        { season_id: 3, difficulty_id: 6565, dungeon: { dungeon_id: 20, pass_time: null, score: 700 } },
      ] },
    } },
  });
  assert.deepEqual(projection, {
    ranking: { season_id: 3, master_score: 3784 },
    records: [{
      season_id: 3, activity_id: 6545, tier: 20, score: 700,
      pass_time_seconds: 418, completion_count: 2,
    }],
  });
});

test("profile catalog reconciliation collapses legacy IDs for the same observed UID", () => {
  const canonical = {
    profile_id: `prf_${"a".repeat(32)}`,
    claimed: true,
    package_id: "new-package",
    updated_unix_millis: 30,
    source_client_build: "24687926",
    deployment: "global",
    region: "north-america",
    realm: null,
    world: null,
    character_id: "3296036",
    display_name: "MarieRose",
    module_inventory_count: 10,
    equipped_module_count: 5,
  };
  const catalog = {
    schema_version: 1,
    profiles: [
      { ...canonical, profile_id: `prf_${"b".repeat(32)}`, package_id: "legacy-package", updated_unix_millis: 10, region: "global" },
      { ...canonical, profile_id: `prf_${"c".repeat(32)}`, package_id: "other-package", character_id: "77212533", display_name: "moonglowkokomi", updated_unix_millis: 20 },
      { ...canonical, package_id: "stale-canonical", updated_unix_millis: 5, region: "global" },
    ],
  };

  reconcileCatalog(catalog, canonical);

  assert.equal(catalog.profiles.length, 2);
  assert.deepEqual(catalog.profiles.map((entry) => entry.character_id), ["3296036", "77212533"]);
  assert.equal(catalog.profiles[0].profile_id, canonical.profile_id);
  assert.equal(catalog.profiles[0].region, "north-america");
});

test("a deployment fallback cannot erase a previously observed specific region", () => {
  const existing = {
    deployment: "global",
    region: "north-america",
    realm: "na-realm",
    world: "7",
  };

  assert.deepEqual(
    reconcilePublishedRouting(existing, {
      deployment: "global",
      region: "global",
      realm: null,
      world: null,
    }),
    {
      deployment: "global",
      region: "north-america",
      realm: "asteria",
      world: "7",
    },
  );
  assert.equal(
    reconcilePublishedRouting(existing, {
      deployment: "global",
      region: "europe",
      realm: null,
      world: null,
    }).region,
    "europe",
  );
  assert.equal(
    reconcilePublishedRouting(existing, {
      deployment: "global",
      region: "europe",
      realm: null,
      world: null,
    }).realm,
    "bahamar",
  );
});

test("published routing supplies reviewed Global realms and omits unresolved optional keys", () => {
  assert.deepEqual(canonicalPublishedRouting({
    deployment: "global", region: "north-america", realm: null, world: null,
  }), {
    deployment: "global", region: "north-america", realm: "asteria",
  });
  assert.deepEqual(canonicalPublishedRouting({
    deployment: "GLOBAL", region: "global", realm: null, world: "Asteria",
  }), {
    deployment: "global", region: "north-america", realm: "asteria", world: "Asteria",
  });
  assert.deepEqual(canonicalPublishedRouting({
    deployment: "global", region: "unknown", realm: "BAHAMAR", world: null,
  }), {
    deployment: "global", region: "europe", realm: "bahamar",
  });
  assert.deepEqual(canonicalPublishedRouting({
    deployment: "sea", region: "unknown", realm: null, world: "  ",
  }), {
    deployment: "sea", region: "unknown",
  });
});

test("a legacy broad route with Asteria evidence protects later fallback snapshots", () => {
  assert.deepEqual(reconcilePublishedRouting({
    deployment: "global", region: "global", realm: null, world: "asteria",
  }, {
    deployment: "global", region: "global", realm: null, world: null,
  }), {
    deployment: "global", region: "north-america", realm: "asteria", world: "asteria",
  });
});

test("account projection does not expose Discord IDs", () => {
  const view = accountView({
    submitter_id: "usr_a",
    account_id: 123456789012,
    username: "player",
    discord_user_id: "1",
    discord_username: "discord-name",
    discord_global_name: "Display",
    discord_avatar_url: null,
    publish_verified_parses: true,
  }, { DEVELOPER_DISCORD_USER_IDS: "1" });
  assert.equal(view.developer, true);
  assert.equal("discord_user_id" in view, false);
});

test("My Parses includes uploader reports and non-private claimed-character reports", async () => {
  const { auth, kv } = authFixture();
  const ownerReportId = `rpt_${"a".repeat(32)}`;
  const participantReportId = `rpt_${"b".repeat(32)}`;
  const hiddenReportId = `rpt_${"c".repeat(32)}`;
  kv.set("fs:profiles/catalog.v1.json", { profiles: [{ profile_id: "prf_one", character_id: "3296036" }] });
  kv.set("fs:profiles/prf_one/claim.json", { submitter_id: "usr_owner" });
  for (const [reportId, visibility, submitterId, characterIds, created] of [
    [ownerReportId, "private", "usr_owner", [], 3],
    [participantReportId, "unlisted", "usr_other", ["3296036"], 2],
    [hiddenReportId, "private", "usr_other", ["3296036"], 1],
  ]) {
    kv.set(`fs:projections/${reportId}.json`, reportFixture(reportId, visibility, submitterId, created));
    kv.set(`fs:memberships/${reportId}.json`, { runs: [{ run_index: 0, character_ids: characterIds }] });
  }
  kv.set("fs:catalog.v1.json", { entries: [] });
  const response = await auth.myParses(new Request("https://backend/v1/auth/parses?limit=250"), Date.now(), new URL("https://backend/v1/auth/parses?limit=250"));
  const value = await response.json();
  assert.equal(value.schema_version, 2);
  assert.deepEqual(value.claimed_character_ids, ["3296036"]);
  assert.deepEqual(value.entries.map((entry) => entry.report_id), [ownerReportId, participantReportId]);
  assert.equal(value.entries[0].submitted_by_you, true);
  assert.deepEqual(value.entries[1].matched_character_ids, ["3296036"]);
  assert.ok(value.entries.every((entry) => entry.client_build === "24687926"));
  assert.ok(value.entries.every((entry) => entry.protocol_pack_digest === PACK_A));
});

test("legacy My Parses reports fail closed for derived localization semantics", () => {
  const entry = catalogEntry({
    report_id: "rpt_legacy", created_unix_millis: 1, deployment_id: "global", region_id: "global",
    submission_provenance: {},
  }, {
    run_index: 0, run_group_id: "run_legacy", activity_id: "chaotic",
    activity_family_id: "chaotic", activity_category_id: "dungeons", scene_id: 6565,
    scene_name: "Unproven scene", difficulty_family: "master", difficulty_tier: 5,
    terminal_state: "completed", participants: [],
  });
  assert.equal(entry.client_build, null);
  assert.equal(entry.protocol_pack_digest, null);
  assert.equal(entry.scene_id, 6565);
  assert.equal(entry.difficulty_tier, 5);
  assert.equal(entry.scene_name, null);
  assert.equal(entry.activity_id, null);
  assert.equal(entry.activity_family_id, null);
  assert.equal(entry.activity_category_id, null);
  assert.equal(entry.difficulty_family, null);
});

test("My Parses ignores conflicting schema-6 catalog semantics and rebuilds from the projection", async () => {
  const { auth, kv } = authFixture();
  const reportId = `rpt_${"8".repeat(32)}`;
  kv.set(`fs:projections/${reportId}.json`, reportFixture(reportId, "private", "usr_owner", 4));
  kv.set(`fs:memberships/${reportId}.json`, { runs: [{ run_index: 0, character_ids: [] }] });
  kv.set("fs:catalog.v1.json", {
    schema_version: 6,
    entries: [{
      report_id: reportId, run_index: 0, deployment_id: "global",
      client_build: "24687926", protocol_pack_digest: PACK_A,
      scene_id: 9999, scene_name: "Wrong cached scene", activity_id: "wrong.activity",
      activity_family_id: "wrong.family", activity_category_id: "wrong-category",
      difficulty_family: "wrong-difficulty", difficulty_tier: 1,
    }],
  });

  const response = await auth.myParses(
    new Request("https://backend/v1/auth/parses"), Date.now(),
    new URL("https://backend/v1/auth/parses"),
  );
  const value = await response.json();
  assert.equal(value.schema_version, 2);
  assert.equal(value.entries.length, 1);
  assert.equal(value.entries[0].scene_id, 1);
  assert.equal(value.entries[0].scene_name, "Dungeon");
  assert.equal(value.entries[0].activity_id, "scene.1");
  assert.equal(value.entries[0].activity_family_id, "dungeon.1");
  assert.equal(value.entries[0].activity_category_id, "dungeons");
  assert.equal(value.entries[0].difficulty_family, "master");
  assert.equal(value.entries[0].difficulty_tier, 20);
  assert.equal(value.entries[0].client_build, "24687926");
  assert.equal(value.entries[0].protocol_pack_digest, PACK_A);
});

test("My Parses canonicalizes exact raw hosted D1 digests without a per-report membership query", async () => {
  const { auth, kv } = authFixture();
  const reportId = `rpt_${"9".repeat(32)}`;
  kv.set("fs:profiles/catalog.v1.json", { profiles: [{ profile_id: "prf_one", character_id: "3296036" }] });
  kv.set("fs:profiles/prf_one/claim.json", { submitter_id: "usr_owner" });
  kv.set("fs:catalog.v1.json", { entries: [] });
  const queries = [];
  let databaseDigest = RAW_PACK_A;
  auth.env.RLOGS_DB = {
    prepare(query) {
      queries.push(query);
      return {
        bind(...bindings) {
          return {
            async first() { return { total: 1 }; },
            async all() {
              return { results: [{
                catalog_entry_json: JSON.stringify({
                  report_id: reportId, run_index: 0, created_unix_millis: 10,
                  deployment_id: "global", region_id: "north-america",
                  client_build: "stale-build", protocol_pack_digest: "sha256:stale-pack",
                  scene_id: 1, scene_name: "Dungeon", terminal_state: "completed",
                }),
                client_build: "24687926",
                protocol_pack_digest: databaseDigest,
                visibility: "unlisted",
                submitter_id: "usr_other",
                matched_character_ids: "3296036",
              }] };
            },
          };
        },
      };
    },
  };

  const response = await auth.myParses(
    new Request("https://backend/v1/auth/parses?limit=50"),
    Date.now(),
    new URL("https://backend/v1/auth/parses?limit=50"),
  );
  const value = await response.json();

  assert.equal(value.total_entries, 1);
  assert.equal(value.schema_version, 2);
  assert.deepEqual(value.entries.map((entry) => entry.report_id), [reportId]);
  assert.deepEqual(value.entries[0].matched_character_ids, ["3296036"]);
  assert.equal(value.entries[0].client_build, "24687926");
  assert.equal(value.entries[0].protocol_pack_digest, PACK_A);
  const reportQueries = queries.filter((query) => query.includes("FROM report_runs"));
  assert.equal(reportQueries.length, 2);
  assert.match(reportQueries[0], /EXISTS \(/u);
  assert.match(reportQueries[1], /GROUP_CONCAT/u);
  assert.ok(reportQueries.every((query) => !query.includes("SELECT character_id FROM report_memberships WHERE report_id=?1")));

  databaseDigest = PACK_A;
  const rejected = await (await auth.myParses(
    new Request("https://backend/v1/auth/parses?limit=50"), Date.now(),
    new URL("https://backend/v1/auth/parses?limit=50"),
  )).json();
  assert.equal(rejected.entries[0].client_build, null);
  assert.equal(rejected.entries[0].protocol_pack_digest, null);
  assert.equal(rejected.entries[0].scene_name, null);
});

test("only the uploader can change visibility and the override changes authorized reads", async () => {
  const { auth, durable, kv } = authFixture();
  const reportId = `rpt_${"d".repeat(32)}`;
  kv.set(`fs:projections/${reportId}.json`, reportFixture(reportId, "public", "usr_owner", 1));
  const patch = new Request(`https://backend/v1/auth/parses/${reportId}/visibility`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ visibility: "private" }),
  });
  const receipt = await auth.updateParseVisibility(patch, Date.now(), reportId);
  assert.deepEqual(await receipt.json(), {
    schema_version: 1,
    report_id: reportId,
    visibility: "private",
    share_url: null,
  });
  assert.equal(durable.get(`visibility:${reportId}`), "private");
  const report = await auth.accountParse(new Request(`https://backend/v1/auth/parses/${reportId}`), Date.now(), reportId);
  assert.equal((await report.json()).visibility, "private");
});

test("promoting a current hosted replay publishes a public projection and wakes each exact run group", async () => {
  const { auth, d1, backgroundTasks } = authFixture();
  const reportId = `rpt_${"e".repeat(32)}`;
  const report = {
    schema_version: 16,
    projection_revision: 8,
    report_id: reportId,
    visibility: "unlisted",
  };
  auth.hostedReport = async () => ({ report, visibility: "unlisted", submitterId: "usr_owner" });
  const puts = [];
  auth.env.RLOGS_ARTIFACTS = { async put(key, bytes) { puts.push({ key, value: JSON.parse(new TextDecoder().decode(bytes)) }); } };
  auth.env.RLOGS_DB.prepare = (query) => ({ bind(...bindings) {
    return {
      query, bindings,
      async run() { d1.push({ query, bindings }); return { success: true }; },
      async all() {
        assert.match(query, /r\.visibility='public'.*r\.verification_tier='replayed'/su);
        return { results: [{ run_group_id: "run_one" }, { run_group_id: "run_two" }] };
      },
    };
  } });
  const wakeups = [];
  auth.env.RLOGS_VERIFIER = { async fetch(request) {
    wakeups.push({ path: new URL(request.url).pathname, body: await request.json() });
    return Response.json({ accepted: true });
  } };
  const response = await auth.updateParseVisibility(new Request(
    `https://backend/v1/auth/parses/${reportId}/visibility`, {
      method: "PATCH", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ visibility: "public" }),
    },
  ), 100, reportId);
  assert.equal(response.status, 200);
  assert.equal(puts.length, 1);
  assert.equal(puts[0].value.visibility, "public");
  assert.match(puts[0].key, new RegExp(`^reports/${reportId}/projection-[a-f0-9]{64}\\.json$`, "u"));
  assert.match(d1[0].query, /projection_sha256=COALESCE/u);
  assert.equal(d1[0].bindings[1], "public");
  assert.equal(d1[0].bindings[3], puts[0].key.slice(-69, -5));
  assert.equal(d1[0].bindings[4], puts[0].key);
  assert.equal(backgroundTasks.length, 1);
  await Promise.all(backgroundTasks);
  assert.deepEqual(wakeups, ["run_one", "run_two"].map((runGroupId) => ({
    path: `/v1/reconciliation-jobs/${runGroupId}/run`,
    body: { schema_version: 1, run_group_id: runGroupId },
  })));
});

test("private transitions and stale hosted projections never schedule reconciliation", async () => {
  for (const { visibility, schemaVersion, projectionRevision } of [
    { visibility: "private", schemaVersion: 14, projectionRevision: 4 },
    { visibility: "unlisted", schemaVersion: 14, projectionRevision: 4 },
    { visibility: "public", schemaVersion: 13, projectionRevision: 3 },
  ]) {
    const { auth, backgroundTasks } = authFixture();
    const reportId = `rpt_${"f".repeat(32)}`;
    auth.hostedReport = async () => ({
      report: { schema_version: schemaVersion, projection_revision: projectionRevision, report_id: reportId, visibility: "unlisted" },
      visibility: "unlisted", submitterId: "usr_owner",
    });
    auth.env.RLOGS_ARTIFACTS = { async put() {} };
    auth.env.RLOGS_VERIFIER = { async fetch() { throw new Error("must not schedule"); } };
    const response = await auth.updateParseVisibility(new Request(
      `https://backend/v1/auth/parses/${reportId}/visibility`, {
        method: "PATCH", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ visibility }),
      },
    ), 100, reportId);
    assert.equal(response.status, 200);
    assert.equal(backgroundTasks.length, 0);
  }
});

test("a device-bound profile package claims and publishes a profile in Cloudflare storage", async () => {
  const { auth, kv, d1 } = authFixture();
  const deviceToken = "rld_device-secret";
  const deviceId = "dev_device";
  auth.authenticateDevice = async () => ({
    submitter_id: "usr_owner", device_id: deviceId, created_unix_millis: 40,
  });
  kv.set("fs:profiles/catalog.v1.json", { schema_version: 1, profiles: [] });
  const request = {
    relative_endpoint: "/v1/games/blue-protocol-star-resonance/profiles",
    payload: {
      schema_version: 1,
      game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
      payload_kind: "character-profile",
      payload_schema_id: "app.rlogs.bpsr.character-profile",
      payload_schema_version: 1,
      routing: { deployment: "global", region: "north-america", "character-id": "3296036" },
      body: {
        character: { character_id: "3296036", region: { deployment_id: "global", region_id: "north-america", realm_id: null, world_id: null } },
        display_name: "MarieRose",
        class_id: 4,
        specialization_id: 2,
        modules: { inventory: [{ instance_id: "1" }], equipped_slots: { 1: "1" } },
        current_profession_project_id: 5,
        profession_projects: [{ project_id: 5, project_name: "Falc-DS", profession_id: 4 }],
      },
    },
  };
  const packageValue = {
    schema_version: 2,
    package_id: await digest(canonicalJson(request)),
    created_unix_millis: 100,
    source: {
      session_id: "session-one",
      client_build: "24687926",
      protocol_pack_digest: `sha256:${"a".repeat(64)}`,
      canonical_content_sha256: `sha256:${"b".repeat(64)}`,
      observation_count: 2,
      last_event_sequence: 3,
      live_capture: { capture_kind: "continuous_process_owned_capture", device_id: deviceId, proof: "" },
    },
    request,
  };
  packageValue.source.live_capture.proof = await liveCaptureProof(packageValue, deviceId, deviceToken);
  const response = await auth.publishProfile(new Request("https://backend/v1/games/blue-protocol-star-resonance/profiles", {
    method: "POST",
    headers: { Authorization: `Bearer ${deviceToken}`, "Content-Type": "application/json" },
    body: JSON.stringify(packageValue),
  }), 200);
  assert.equal(response.status, 200);
  const receipt = await response.json();
  assert.equal(receipt.character_id, "3296036");
  assert.equal(receipt.claimed, true);
  assert.equal(receipt.module_inventory_count, 1);
  const published = kv.get(`fs:profiles/${receipt.profile_id}/public.json`);
  assert.equal(published.display_name, "MarieRose");
  assert.equal(published.loadouts[0].project_name, "Falc-DS");
  assert.equal(kv.get("fs:profiles/catalog.v1.json").profiles.length, 1);
  assert.equal(d1.length, 7);
  const claimStatement = d1.find((statement) => /INSERT INTO uid_claims/u.test(statement.query));
  const profileStatement = d1.find((statement) => /INSERT INTO profiles/u.test(statement.query));
  const loadoutStatement = d1.find((statement) => /INSERT INTO profile_loadouts/u.test(statement.query));
  assert.deepEqual(claimStatement.bindings.slice(0, 4), [
    "app.rlogs.game.blue-protocol-star-resonance",
    "3296036",
    receipt.profile_id,
    "usr_owner",
  ]);
  assert.equal(JSON.parse(profileStatement.bindings[9]).display_name, "MarieRose");
  const rankingStatement = d1.find((statement) => /INSERT INTO profile_season_rankings/u.test(statement.query));
  assert.equal(rankingStatement, undefined);
  assert.equal(loadoutStatement.bindings[3], "Falc-DS");
});

test("profile publication remains retryable when its D1 verifier mirror fails", async () => {
  const { auth, kv } = authFixture();
  const deviceToken = "rld_device-secret";
  const deviceId = "dev_device";
  auth.authenticateDevice = async () => ({
    submitter_id: "usr_owner", device_id: deviceId, created_unix_millis: 40,
  });
  auth.env.RLOGS_DB.batch = async () => { throw new Error("D1 unavailable"); };
  kv.set("fs:profiles/catalog.v1.json", { schema_version: 1, profiles: [] });
  const request = {
    relative_endpoint: "/v1/games/blue-protocol-star-resonance/profiles",
    payload: {
      schema_version: 1,
      game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
      payload_kind: "character-profile",
      payload_schema_id: "app.rlogs.bpsr.character-profile",
      payload_schema_version: 1,
      routing: { deployment: "global", region: "north-america", "character-id": "3296036" },
      body: {
        character: { character_id: "3296036", region: { deployment_id: "global", region_id: "north-america", realm_id: null, world_id: null } },
        display_name: "MarieRose",
      },
    },
  };
  const packageValue = {
    schema_version: 2,
    package_id: await digest(canonicalJson(request)),
    created_unix_millis: 100,
    source: {
      session_id: "session-one", client_build: "24687926",
      protocol_pack_digest: `sha256:${"a".repeat(64)}`,
      canonical_content_sha256: `sha256:${"b".repeat(64)}`,
      observation_count: 2, last_event_sequence: 3,
      live_capture: { capture_kind: "continuous_process_owned_capture", device_id: deviceId, proof: "" },
    },
    request,
  };
  packageValue.source.live_capture.proof = await liveCaptureProof(packageValue, deviceId, deviceToken);
  const originalError = console.error;
  console.error = () => {};
  let response;
  try {
    response = await auth.publishProfile(new Request("https://backend/v1/games/blue-protocol-star-resonance/profiles", {
      method: "POST",
      headers: { Authorization: `Bearer ${deviceToken}`, "Content-Type": "application/json" },
      body: JSON.stringify(packageValue),
    }), 200);
  } finally {
    console.error = originalError;
  }
  assert.equal(response.status, 503);
  assert.match((await response.json()).error, /safely stored profile will retry/u);
  assert.equal(kv.get("fs:profiles/catalog.v1.json").profiles.length, 1);
});

test("profile publication rejects a proof copied from another device", async () => {
  const { auth, kv } = authFixture();
  auth.authenticateDevice = async () => ({
    submitter_id: "usr_owner", device_id: "dev_actual", created_unix_millis: 40,
  });
  kv.set("fs:profiles/catalog.v1.json", { schema_version: 1, profiles: [] });
  const response = await auth.publishProfile(new Request("https://backend/v1/games/blue-protocol-star-resonance/profiles", {
    method: "POST",
    headers: { Authorization: "Bearer rld_secret", "Content-Type": "application/json" },
    body: JSON.stringify({ schema_version: 2 }),
  }), 200);
  assert.equal(response.status, 400);
});

test("profile owners can publish an observed Photo Wall image", async () => {
  const { auth, kv } = authFixture();
  const profileId = `prf_${"a".repeat(32)}`;
  auth.authenticateDevice = async () => ({ submitter_id: "usr_owner", device_id: "dev_one" });
  kv.set(`fs:profiles/${profileId}/claim.json`, { submitter_id: "usr_owner" });
  kv.set(`fs:profiles/${profileId}/public.json`, {
    profile_id: profileId, character_id: "3296036", display_name: "MarieRose", updated_unix_millis: 1,
    envelope: { body: { collection_summary: { photo_ids: [7] } } },
  });
  kv.set("fs:profiles/catalog.v1.json", { schema_version: 1, profiles: [] });
  const png = new Uint8Array(45);
  png.set([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82]);
  new DataView(png.buffer).setUint32(16, 1); new DataView(png.buffer).setUint32(20, 1);
  png.set([0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130], 33);
  const response = await auth.publishPhoto(new Request(`https://backend/v1/games/blue-protocol-star-resonance/profiles/${profileId}/photo-wall/7`, {
    method: "PUT", headers: { Authorization: "Bearer rld_secret" }, body: png,
  }), 500, profileId, 7);
  assert.equal(response.status, 200);
  const receipt = await response.json();
  assert.equal(receipt.media_type, "image/png");
  assert.equal(receipt.byte_length, 45);
  assert.equal(kv.get(`fs:profiles/${profileId}/public.json`).envelope.body.collection_summary.photo_assets[0].photo_id, 7);
});

test("photo likes are idempotent and feed counts are viewer-aware", async () => {
  const { auth, kv } = authFixture();
  const profileId = `prf_${"b".repeat(32)}`;
  kv.set(`fs:profiles/${profileId}/photo-wall/photo-7.json`, {
    profile_id: profileId, photo_id: 7, image_path: `/v1/profiles/${profileId}/photo-wall/7`, uploaded_unix_millis: 10,
  });
  kv.set(`fs:profiles/${profileId}/public.json`, { character_id: "3296036", display_name: "MarieRose", updated_unix_millis: 10 });
  const request = new Request(`https://backend/v1/profiles/${profileId}/photo-wall/7/like`, { method: "PUT" });
  const first = await auth.setPhotoLike(request, 20, profileId, 7, true);
  assert.equal((await first.json()).like_count, 1);
  const second = await auth.setPhotoLike(request, 21, profileId, 7, true);
  assert.equal((await second.json()).like_count, 1);
  const feed = await auth.photoCatalog(new Request("https://backend/v1/photos?sort=popular", { headers: { Authorization: "Bearer rlw_test" } }), 22, new URL("https://backend/v1/photos?sort=popular"));
  assert.deepEqual((await feed.json()).entries[0], {
    profile_id: profileId, character_id: "3296036", display_name: "MarieRose", photo_id: 7,
    image_path: `/v1/profiles/${profileId}/photo-wall/7`, uploaded_unix_millis: 10, like_count: 1, viewer_liked: true,
  });
});

test("public account pages resolve current Durable Object identity and claimed profiles", async () => {
  const { auth, durable, kv } = authFixture();
  const accountId = "556457510583";
  const profileId = `prf_${"c".repeat(32)}`;
  durable.set(`index:account:${accountId}`, "usr_owner");
  durable.set("user:usr_owner", { submitter_id: "usr_owner", account_id: Number(accountId), username: "whoisaqua" });
  kv.set("fs:profiles/catalog.v1.json", { profiles: [{ profile_id: profileId, character_id: "256017" }] });
  kv.set(`fs:profiles/${profileId}/claim.json`, { submitter_id: "usr_owner" });
  const response = await auth.publicAccount(accountId);
  assert.deepEqual(await response.json(), {
    schema_version: 1,
    account: { schema_version: 1, account_id: Number(accountId), username: "whoisaqua" },
    profiles: [{ profile_id: profileId, character_id: "256017" }],
  });
});

function reportFixture(reportId, visibility, submitterId, createdUnixMillis) {
  return {
    schema_version: 12,
    report_id: reportId,
    visibility,
    created_unix_millis: createdUnixMillis,
    deployment_id: "global",
    client_build: "24687926",
    protocol_pack_digest: PACK_A,
    region_id: "north-america",
    submission_provenance: { submitter_id: submitterId },
    runs: [{
      run_index: 0,
      run_group_id: `run_${"e".repeat(32)}`,
      activity_id: "scene.1",
      activity_family_id: "dungeon.1",
      activity_category_id: "dungeons",
      scene_id: 1,
      scene_name: "Dungeon",
      difficulty_family: "master",
      difficulty_tier: 20,
      terminal_state: "completed",
      total_run_time_micros: 1,
      participants: [],
      local_profile_character_ids: [],
    }],
  };
}

async function digest(value) {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return Array.from(new Uint8Array(bytes), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
