import assert from "node:assert/strict";
import test from "node:test";

import { accountView, RLogsAuthState, tokenHash } from "../src/auth.js";
import { canonicalJson, liveCaptureProof, reconcileCatalog, reconcilePublishedRouting } from "../src/profile.js";

function authFixture() {
  const durable = new Map();
  const kv = new Map();
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
  };
  const auth = new RLogsAuthState({ storage }, env);
  auth.authenticateWeb = async () => ({ submitter_id: "usr_owner" });
  return { auth, durable, kv };
}

test("token hashes remain compatible with the Rust authentication domain separator", async () => {
  assert.equal(
    await tokenHash("web-session", "rlw_example", "0123456789abcdef0123456789abcdef"),
    "e338255bf6b06636cebe145525ead0d3a0ef95891ca6fe2cb606d9bddf155b9c",
  );
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
      realm: "na-realm",
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
  assert.deepEqual(value.claimed_character_ids, ["3296036"]);
  assert.deepEqual(value.entries.map((entry) => entry.report_id), [ownerReportId, participantReportId]);
  assert.equal(value.entries[0].submitted_by_you, true);
  assert.deepEqual(value.entries[1].matched_character_ids, ["3296036"]);
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

test("a device-bound profile package claims and publishes a profile in Cloudflare storage", async () => {
  const { auth, kv } = authFixture();
  const deviceToken = "rld_device-secret";
  const deviceId = "dev_device";
  auth.authenticateDevice = async () => ({ submitter_id: "usr_owner", device_id: deviceId });
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
});

test("profile publication rejects a proof copied from another device", async () => {
  const { auth, kv } = authFixture();
  auth.authenticateDevice = async () => ({ submitter_id: "usr_owner", device_id: "dev_actual" });
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
