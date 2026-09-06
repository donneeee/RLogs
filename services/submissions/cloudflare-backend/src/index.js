const JSON_HEADERS = {
  "Content-Type": "application/json; charset=utf-8",
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
};

function json(value, status = 200, headers = {}) {
  return Response.json(value, {
    status,
    headers: { ...JSON_HEADERS, ...headers },
  });
}

function notFound() {
  return json({ error: "not found" }, 404);
}

async function storedJson(env, key) {
  const value = await env.RLOGS_DATA.get(`fs:${key}`, "text");
  return value == null
    ? notFound()
    : new Response(value, { headers: JSON_HEADERS });
}

async function visibilityOverrides(env) {
  const id = env.AUTH_STATE.idFromName("global");
  const response = await env.AUTH_STATE.get(id).fetch("https://auth.internal/internal/visibility-overrides");
  return response.ok ? response.json() : {};
}

async function publicReport(env, reportId) {
  const report = await env.RLOGS_DATA.get(`fs:projections/${reportId}.json`, "json");
  if (!report) return notFound();
  const overrides = await visibilityOverrides(env);
  const visibility = overrides[reportId] ?? report.visibility;
  return visibility === "private" ? notFound() : json({ ...report, visibility });
}

async function storedPhoto(env, profileId, photoId) {
  const metadataKey = `fs:profiles/${profileId}/photo-wall/photo-${photoId}.json`;
  const metadata = await env.RLOGS_DATA.get(metadataKey, "json");
  if (!metadata || typeof metadata.file_name !== "string") return notFound();
  const key = `fs:profiles/${profileId}/photo-wall/${metadata.file_name}`;
  const value = await env.RLOGS_DATA.get(key, "arrayBuffer");
  if (value == null) return notFound();
  return new Response(value, {
    headers: {
      "Content-Type": metadata.media_type ?? "application/octet-stream",
      "Cache-Control": "public, max-age=300",
      ETag: `"${metadata.sha256}"`,
      "X-Content-Type-Options": "nosniff",
    },
  });
}

async function metadataDatabaseHealth(env) {
  if (!env.RLOGS_DB) return { ready: false, schemaVersion: null };
  try {
    const row = await env.RLOGS_DB.prepare(
      "SELECT schema_version FROM service_metadata WHERE component = ?1",
    ).bind("production-metadata").first();
    const schemaVersion = Number(row?.schema_version);
    return {
      ready: Number.isSafeInteger(schemaVersion) && schemaVersion > 0,
      schemaVersion: Number.isSafeInteger(schemaVersion) ? schemaVersion : null,
    };
  } catch {
    return { ready: false, schemaVersion: null };
  }
}

function serviceCapabilities(env, publicReadsReady) {
  const discordAuth = Boolean(
    env.DISCORD_CLIENT_ID && env.DISCORD_CLIENT_SECRET && env.AUTH_TOKEN_PEPPER && env.AUTH_STATE,
  );
  const artifactStorage = Boolean(env.RLOGS_ARTIFACTS);
  const hostedVerification = Boolean(env.RLOGS_VERIFIER);
  return {
    public_reads: publicReadsReady,
    discord_auth: discordAuth,
    profile_sync: publicReadsReady && discordAuth,
    artifact_storage: artifactStorage,
    hosted_verification: hostedVerification,
    parse_uploads: publicReadsReady && discordAuth && artifactStorage && hostedVerification,
  };
}

async function profileCatalog(env, url) {
  const catalog = await env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json");
  if (!catalog || !Array.isArray(catalog.profiles)) return notFound();
  const characterId = url.searchParams.get("character_id");
  const profiles = characterId
    ? catalog.profiles.filter((entry) => entry.character_id === characterId)
    : catalog.profiles;
  return json({ ...catalog, profiles });
}

async function parseCatalog(env, url) {
  const catalog = await env.RLOGS_DATA.get("fs:catalog.v1.json", "json");
  if (!catalog || !Array.isArray(catalog.entries)) return notFound();
  const overrides = await visibilityOverrides(env);
  let entries = catalog.entries.filter((entry) => overrides[entry.report_id] !== "private");
  const scalarFilters = [
    ["deployment", "deployment_id"],
    ["region", "region_id"],
    ["activity", "activity_id"],
    ["difficulty", "difficulty_family"],
    ["terminal", "terminal_state"],
  ];
  for (const [queryName, field] of scalarFilters) {
    const expected = url.searchParams.get(queryName);
    if (expected) entries = entries.filter((entry) => String(entry[field] ?? "") === expected);
  }
  const scene = url.searchParams.get("scene");
  if (scene) entries = entries.filter((entry) => String(entry.scene_id ?? "") === scene);
  const search = url.searchParams.get("search")?.trim().toLocaleLowerCase();
  if (search) {
    entries = entries.filter((entry) => JSON.stringify(entry).toLocaleLowerCase().includes(search));
  }
  const requestedOffset = Number.parseInt(url.searchParams.get("offset") ?? "0", 10);
  const requestedLimit = Number.parseInt(url.searchParams.get("limit") ?? "250", 10);
  const offset = Number.isSafeInteger(requestedOffset) && requestedOffset > 0 ? requestedOffset : 0;
  const limit = Number.isSafeInteger(requestedLimit)
    ? Math.min(250, Math.max(1, requestedLimit))
    : 250;
  const page = entries.slice(offset, offset + limit);
  return json({
    ...catalog,
    total_entries: entries.length,
    offset,
    next_offset: offset + page.length < entries.length ? offset + page.length : null,
    entries: page,
  });
}

async function route(request, env) {
  const url = new URL(request.url);
  const path = url.pathname;
  if (path === "/v1/auth/config") {
    return json({
      schema_version: 1,
      discord_enabled: Boolean(env.DISCORD_CLIENT_ID && env.DISCORD_CLIENT_SECRET && env.AUTH_TOKEN_PEPPER),
      desktop_authentication: "bearer_app_token",
    });
  }
  if (path.startsWith("/v1/auth/")) {
    const id = env.AUTH_STATE.idFromName("global");
    return env.AUTH_STATE.get(id).fetch(request);
  }
  if (
    path === "/v1/games/blue-protocol-star-resonance/profiles" ||
    path === "/v1/photos" ||
    /^\/v1\/users\/[1-9][0-9]{11}$/.test(path) ||
    /^\/v1\/games\/blue-protocol-star-resonance\/profiles\/prf_[a-z0-9_]{32}\/photo-wall\/[1-9][0-9]*$/.test(path) ||
    /^\/v1\/profiles\/prf_[a-z0-9_]{32}\/photo-wall\/[1-9][0-9]*\/like$/.test(path)
  ) {
    const id = env.AUTH_STATE.idFromName("global");
    return env.AUTH_STATE.get(id).fetch(request);
  }
  if (request.method !== "GET") {
    return json(
      { error: "hosted write service is not enabled yet", retryable: true },
      503,
      { "Retry-After": "30" },
    );
  }
  if (path === "/health") {
    const [catalog, metadata] = await Promise.all([
      env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json"),
      metadataDatabaseHealth(env),
    ]);
    const ready = Boolean(catalog) && metadata.ready;
    return json({
      status: ready ? "ok" : "degraded",
      service: "rlogs-cloudflare-backend",
      schema_version: 1,
      release: env.BACKEND_RELEASE ?? "local",
      storage: "cloudflare-kv+d1",
      metadata_schema_version: metadata.schemaVersion,
      public_profile_count: Array.isArray(catalog?.profiles) ? catalog.profiles.length : 0,
      capabilities: serviceCapabilities(env, ready),
    }, ready ? 200 : 503);
  }
  if (path === "/v1/profiles") return profileCatalog(env, url);
  if (path === "/v1/parses") return parseCatalog(env, url);
  if (path === "/v1/activity/milestones") {
    return storedJson(env, "community-milestones.v1.json");
  }
  let match = /^\/v1\/parses\/(rpt_[A-Za-z0-9_-]+)$/.exec(path);
  if (match) return publicReport(env, match[1]);
  match = /^\/v1\/run-groups\/([A-Za-z0-9_-]+)\/reconciliation$/.exec(path);
  if (match) return storedJson(env, `reconciliations/${match[1]}.json`);
  match = /^\/v1\/profiles\/(prf_[a-z0-9_]+)$/.exec(path);
  if (match) return storedJson(env, `profiles/${match[1]}/public.json`);
  match = /^\/v1\/profiles\/(prf_[a-z0-9_]+)\/loadouts\/([1-9][0-9]*)$/.exec(path);
  if (match) return storedJson(env, `profiles/${match[1]}/loadouts/${match[2]}.json`);
  match = /^\/v1\/profiles\/(prf_[a-z0-9_]+)\/photo-wall\/([1-9][0-9]*)$/.exec(path);
  if (match) return storedPhoto(env, match[1], match[2]);
  return notFound();
}

export default { fetch: route };
export { route };
export { RLogsAuthState } from "./auth.js";
