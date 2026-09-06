import assert from "node:assert/strict";
import test from "node:test";

import gateway, { configuredOrigin, routeAllowed } from "../src/index.js";

for (const status of [500, 502, 503, 530, null]) {
  test(`origin failure ${status ?? "network"} is retryable and never cached`, async () => {
    const originalFetch = globalThis.fetch;
    let calls = 0;
    globalThis.fetch = async () => {
      calls += 1;
      if (status === null) throw new Error("private origin hostname");
      return new Response("<html>private origin hostname</html>", { status });
    };
    try {
      for (const path of ["/health", "/v1/profiles/prf_0123456789abcdef/photo-wall/42"]) {
        const response = await gateway.fetch(
          new Request(`https://gateway.example${path}`, {
            headers: { Origin: "https://rlogs-app.github.io" },
          }),
          { ALLOWED_ORIGIN: "https://rlogs-app.github.io", ORIGIN_BASE_URL: "https://origin.example" },
        );
        assert.equal(response.status, 503);
        assert.equal(response.headers.get("Cache-Control"), "no-store");
        assert.equal(response.headers.get("Retry-After"), "30");
        assert.equal(response.headers.get("Access-Control-Allow-Origin"), "https://rlogs-app.github.io");
        assert.deepEqual(await response.json(), { error: "submission origin is unavailable", retryable: true });
      }
      assert.equal(calls, 2, "gateway must not replay requests automatically");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
}

test("missing photos are not shared-cacheable", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => Response.json({ error: "not found" }, { status: 404 });
  try {
    const response = await gateway.fetch(
      new Request("https://gateway.example/v1/profiles/prf_0123456789abcdef/photo-wall/42"),
      { ORIGIN_BASE_URL: "https://origin.example" },
    );
    assert.equal(response.status, 404);
    assert.equal(response.headers.get("Cache-Control"), "no-store");
    assert.deepEqual(await response.json(), { error: "not found" });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("only the submission API surface is routable", () => {
  assert.equal(routeAllowed("GET", "/health"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/config"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/discord/start"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/discord/callback"), true);
  assert.equal(routeAllowed("POST", "/v1/auth/discord/complete"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/device"), true);
  assert.equal(routeAllowed("PATCH", "/v1/auth/me"), true);
  assert.equal(routeAllowed("PATCH", "/v1/auth/me/parse-publication"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/profiles"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/parses"), true);
  assert.equal(routeAllowed("GET", "/v1/auth/parses/rpt_0123456789abcdef0123456789abcdef"), true);
  assert.equal(
    routeAllowed(
      "PATCH",
      "/v1/auth/parses/rpt_0123456789abcdef0123456789abcdef/visibility",
    ),
    true,
  );
  assert.equal(routeAllowed("POST", "/v1/auth/session/exchange"), true);
  assert.equal(routeAllowed("POST", "/v1/auth/app-tokens"), true);
  assert.equal(routeAllowed("POST", "/v1/uploads"), true);
  assert.equal(routeAllowed("PUT", "/v1/uploads/up_123/chunks/4"), true);
  assert.equal(routeAllowed("POST", "/v1/uploads/up_123/finalize"), true);
  assert.equal(routeAllowed("GET", "/v1/parses/rpt_123"), true);
  assert.equal(routeAllowed("GET", "/v1/activity/milestones"), true);
  assert.equal(routeAllowed("GET", "/v1/run-groups/run_123/reconciliation"), true);
  assert.equal(routeAllowed("GET", "/v1/profiles?character_id=1000001"), false);
  assert.equal(routeAllowed("GET", "/v1/profiles"), true);
  assert.equal(routeAllowed("GET", "/v1/photos"), true);
  assert.equal(routeAllowed("GET", "/v1/profiles/prf_0123456789abcdef0123456789abcdef"), true);
  assert.equal(
    routeAllowed(
      "GET",
      "/v1/profiles/prf_0123456789abcdef0123456789abcdef/loadouts/8",
    ),
    true,
  );
  assert.equal(routeAllowed("GET", "/v1/users/583104927614"), true);
  assert.equal(routeAllowed("GET", "/v1/users/3296036"), false);
  assert.equal(
    routeAllowed(
      "PUT",
      "/v1/profiles/prf_0123456789abcdef0123456789abcdef/photo-wall/42/like",
    ),
    true,
  );
  assert.equal(
    routeAllowed(
      "DELETE",
      "/v1/profiles/prf_0123456789abcdef0123456789abcdef/photo-wall/42/like",
    ),
    true,
  );
  assert.equal(
    routeAllowed(
      "GET",
      "/v1/profiles/prf_0123456789abcdef0123456789abcdef/photo-wall/42",
    ),
    true,
  );
  assert.equal(
    routeAllowed("POST", "/v1/games/blue-protocol-star-resonance/profiles"),
    true,
  );
  assert.equal(
    routeAllowed(
      "PUT",
      "/v1/games/blue-protocol-star-resonance/profiles/prf_0123456789abcdef/photo-wall/42",
    ),
    true,
  );
  assert.equal(
    routeAllowed(
      "PUT",
      "/v1/games/blue-protocol-star-resonance/profiles/prf_0123456789abcdef/photo-wall/0",
    ),
    false,
  );
  assert.equal(routeAllowed("GET", "/v1/uploads"), false);
  assert.equal(routeAllowed("POST", "/v1/auth/parses"), false);
  assert.equal(
    routeAllowed("PATCH", "/v1/auth/parses/rpt_0123456789abcdef0123456789abcdef"),
    false,
  );
  assert.equal(routeAllowed("GET", "/v1/auth/parses/../../secret"), false);
  assert.equal(routeAllowed("GET", "/artifacts/private.rlog"), false);
  assert.equal(routeAllowed("GET", "/v1/parses/../../secret"), false);
});

test("the upstream must be one pathless HTTPS origin", () => {
  assert.equal(configuredOrigin("https://example.com").origin, "https://example.com");
  assert.throws(() => configuredOrigin("http://example.com"));
  assert.throws(() => configuredOrigin("https://example.com/private"));
  assert.throws(() => configuredOrigin("https://example.com/?target=other"));
  assert.throws(() => configuredOrigin("https://user:password@example.com"));
  assert.throws(() => configuredOrigin("https://temporary-name.trycloudflare.com"));
  assert.equal(
    configuredOrigin("https://temporary-name.trycloudflare.com", {
      allowDevelopmentOrigin: true,
    }).origin,
    "https://temporary-name.trycloudflare.com",
  );
});

test("a production gateway never contacts a configured quick tunnel", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("production gateway contacted a development origin");
  };
  try {
    const response = await gateway.fetch(
      new Request("https://gateway.example/health"),
      { ORIGIN_BASE_URL: "https://temporary-name.trycloudflare.com" },
    );
    assert.equal(response.status, 503);
    assert.deepEqual(await response.json(), { error: "submission origin is not configured" });
  } finally {
    globalThis.fetch = originalFetch;
  }
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

test("Cloudflare service binding is authoritative over the legacy origin", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("legacy public origin must not be contacted");
  };
  try {
    const response = await gateway.fetch(
      new Request("https://gateway.example/v1/profiles", {
        headers: { Origin: "https://rlogs-app.github.io" },
      }),
      {
        ALLOWED_ORIGIN: "https://rlogs-app.github.io",
        ORIGIN_BASE_URL: "https://legacy-origin.example",
        BACKEND: {
          async fetch(request) {
            assert.equal(request.url, "https://gateway.example/v1/profiles");
            return Response.json({ schema_version: 1, profiles: [] });
          },
        },
      },
    );
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("Access-Control-Allow-Origin"), "https://rlogs-app.github.io");
    assert.deepEqual(await response.json(), { schema_version: 1, profiles: [] });
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

test("public Photo Wall responses retain bounded shared caching", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(new Uint8Array([1, 2, 3]), {
      headers: { "Cache-Control": "public, max-age=300", "Content-Type": "image/png" },
    });
  try {
    const response = await gateway.fetch(
      new Request(
        "https://gateway.example/v1/profiles/prf_0123456789abcdef/photo-wall/42",
      ),
      {
        ALLOWED_ORIGIN: "https://donneeee.github.io",
        ORIGIN_BASE_URL: "https://origin.example",
      },
    );
    assert.equal(response.headers.get("Cache-Control"), "public, max-age=300");
    assert.equal(response.headers.get("Content-Type"), "image/png");
    assert.equal(response.headers.get("X-Content-Type-Options"), "nosniff");
  } finally {
    globalThis.fetch = originalFetch;
  }
});
