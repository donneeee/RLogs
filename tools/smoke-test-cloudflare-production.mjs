#!/usr/bin/env node

import assert from "node:assert/strict";

const baseUrl = String(process.argv[2] ?? "https://rlogs-submissions.pages.dev").replace(/\/$/u, "");
const expectedRelease = process.argv[3] ?? "";
const attempts = 8;

async function request(path, options = {}) {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(`${baseUrl}${path}`, options);
      if (response.status < 500 || attempt === attempts) return response;
      lastError = new Error(`${path} returned HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
  throw lastError;
}

async function json(path, expectedStatus = 200) {
  const response = await request(path);
  assert.equal(response.status, expectedStatus, `${path} must return HTTP ${expectedStatus}`);
  return response.json();
}

const health = await json("/health");
assert.equal(health.status, "ok");
assert.equal(health.service, "rlogs-cloudflare-backend");
assert.equal(health.storage, "cloudflare-kv");
assert.ok(Number.isSafeInteger(health.public_profile_count));
assert.ok(health.public_profile_count > 0, "production profile storage must not be empty");
if (expectedRelease) assert.equal(health.release, expectedRelease, "the requested backend revision is not live");

const auth = await json("/v1/auth/config");
assert.equal(auth.discord_enabled, true, "Discord authentication must be configured");

const profiles = await json("/v1/profiles");
assert.ok(Array.isArray(profiles.profiles));
assert.ok(profiles.profiles.length > 0, "the public profile catalog must not be empty");

const parses = await json("/v1/parses?limit=1");
assert.ok(Array.isArray(parses.entries));
assert.ok(Number.isSafeInteger(parses.total_entries));

const authorize = await request("/v1/auth/discord/start", { redirect: "manual" });
assert.equal(authorize.status, 307);
const authorizeUrl = new URL(authorize.headers.get("location"));
assert.equal(authorizeUrl.origin, "https://discord.com");
assert.equal(authorizeUrl.pathname, "/oauth2/authorize");
assert.equal(authorizeUrl.searchParams.get("redirect_uri"), "https://rlogs-app.github.io/account/");

const preflight = await request("/v1/auth/discord/complete", {
  method: "OPTIONS",
  headers: {
    Origin: "https://rlogs-app.github.io",
    "Access-Control-Request-Method": "POST",
    "Access-Control-Request-Headers": "content-type",
  },
});
assert.equal(preflight.status, 204);
assert.equal(preflight.headers.get("access-control-allow-origin"), "https://rlogs-app.github.io");

const protectedWrite = await request("/v1/games/blue-protocol-star-resonance/profiles", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: "{}",
});
assert.equal(protectedWrite.status, 401, "profile publication must reject unauthenticated writes");

console.log(JSON.stringify({
  status: "ok",
  base_url: baseUrl,
  release: health.release,
  profiles: profiles.profiles.length,
  parses: parses.total_entries,
  discord_authentication: "ready",
}));
