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

test("health does not advertise unimplemented uploads when storage and verifier bindings appear", async () => {
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

test("profiles come only from bound Cloudflare storage", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/profiles?character_id=2"),
    environment({
      "fs:profiles/catalog.v1.json": JSON.stringify({
        schema_version: 1,
        profiles: [
          { profile_id: "prf_a", character_id: "1" },
          { profile_id: "prf_b", character_id: "2" },
        ],
      }),
    }),
  );
  assert.deepEqual((await response.json()).profiles, [{ profile_id: "prf_b", character_id: "2" }]);
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
