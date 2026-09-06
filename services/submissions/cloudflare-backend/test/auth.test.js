import assert from "node:assert/strict";
import test from "node:test";

import { accountView, tokenHash } from "../src/auth.js";

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
