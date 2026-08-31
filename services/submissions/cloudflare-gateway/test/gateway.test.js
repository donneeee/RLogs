import assert from "node:assert/strict";
import test from "node:test";

import gateway, { configuredOrigin, routeAllowed } from "../src/index.js";

test("only the submission API surface is routable", () => {
  assert.equal(routeAllowed("GET", "/health"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/config"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/discord/start"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/discord/callback"), true);
  assert.equal(routeAllowed("POST", "/v1/auth/discord/complete"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/device"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/profiles"), true);
  assert.equal(routeAllowed("POST", "/v1/auth/session/exchange"), true);
  assert.equal(routeAllowed("POST", "/v1/auth/app-tokens"), true);
  assert.equal(routeAllowed("POST", "/v1/uploads"), true);
  assert.equal(routeAllowed("PUT", "/v1/uploads/up_123/chunks/4"), true);
  assert.equal(routeAllowed("POST", "/v1/uploads/up_123/finalize"), true);
  assert.equal(routeAllowed("GET", "/v1/parses/rpt_123"), true);
  assert.equal(routeAllowed("GET", "/v1/run-groups/run_123/reconciliation"), true);
  assert.equal(routeAllowed("GET", "/v1/profiles?character_id=1000001"), false);
  assert.equal(routeAllowed("GET", "/v1/profiles"), true);
  assert.equal(routeAllowed("GET", "/v1/profiles/prf_0123456789abcdef0123456789abcdef"), true);
  assert.equal(
    routeAllowed("POST", "/v1/games/blue-protocol-star-resonance/profiles"),
    true,
  );
  assert.equal(routeAllowed("GET", "/v1/uploads"), false);
  assert.equal(routeAllowed("GET", "/artifacts/private.rlog"), false);
  assert.equal(routeAllowed("GET", "/v1/parses/../../secret"), false);
});

test("the upstream must be one pathless HTTPS origin", () => {
  assert.equal(configuredOrigin("https://example.com").origin, "https://example.com");
  assert.throws(() => configuredOrigin("http://example.com"));
  assert.throws(() => configuredOrigin("https://example.com/private"));
  assert.throws(() => configuredOrigin("https://example.com/?target=other"));
  assert.throws(() => configuredOrigin("https://user:password@example.com"));
});

test("gateway removes permissive upstream CORS for other sites", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    Response.json({}, { headers: { "Access-Control-Allow-Origin": "*" } });
  try {
    const response = await gateway.fetch(
      new Request("https://gateway.example/health", {
        headers: { Origin: "https://untrusted.example" },
      }),
      {
        ALLOWED_ORIGIN: "https://donneeee.github.io",
        ORIGIN_BASE_URL: "https://origin.example",
      },
    );
    assert.equal(response.headers.has("Access-Control-Allow-Origin"), false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("gateway forwards allowed reads and applies constrained CORS", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (request) => {
    assert.equal(request.url, "https://origin.example/v1/parses?region=global");
    return Response.json({ schema_version: 5, entries: [] });
  };
  try {
    const response = await gateway.fetch(
      new Request("https://gateway.example/v1/parses?region=global", {
        headers: { Origin: "https://donneeee.github.io" },
      }),
      {
        ALLOWED_ORIGIN: "https://donneeee.github.io",
        ORIGIN_BASE_URL: "https://origin.example",
      },
    );
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("Access-Control-Allow-Origin"), "https://donneeee.github.io");
    assert.equal(response.headers.get("Cache-Control"), "no-store");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("unknown paths never reach the upstream", async () => {
  const response = await gateway.fetch(
    new Request("https://gateway.example/private/file"),
    { ALLOWED_ORIGIN: "https://donneeee.github.io", ORIGIN_BASE_URL: "https://origin.example" },
  );
  assert.equal(response.status, 404);
});
