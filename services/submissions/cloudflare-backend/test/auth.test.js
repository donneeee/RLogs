import assert from "node:assert/strict";
import test from "node:test";

import { accountView, RLogsAuthState, tokenHash } from "../src/auth.js";

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
