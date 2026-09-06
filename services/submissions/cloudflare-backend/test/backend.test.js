import assert from "node:assert/strict";
import test from "node:test";

import backend from "../src/index.js";

function environment(values = {}) {
  const store = new Map(Object.entries(values));
  return {
    RLOGS_DATA: {
      async get(key, type) {
        const value = store.get(key);
        if (value == null) return null;
        if (type === "json") return JSON.parse(value);
        if (type === "arrayBuffer") return new TextEncoder().encode(value).buffer;
        return value;
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
    storage: "cloudflare-kv",
    public_profile_count: 1,
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
          { report_id: "rpt_a", region: "north-america" },
          { report_id: "rpt_b", region: "north-america" },
          { report_id: "rpt_c", region: "global" },
        ],
        facets: {},
      }),
    }),
  );
  const value = await response.json();
  assert.equal(value.total_entries, 2);
  assert.equal(value.next_offset, 1);
  assert.deepEqual(value.entries, [{ report_id: "rpt_a", region: "north-america" }]);
});

test("write routes fail closed until hosted verification is enabled", async () => {
  const response = await backend.fetch(
    new Request("https://backend/v1/uploads", { method: "POST" }),
    environment(),
  );
  assert.equal(response.status, 503);
  assert.equal(response.headers.get("Retry-After"), "30");
});
