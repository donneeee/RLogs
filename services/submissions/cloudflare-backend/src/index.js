import { routeUpload, uploadsEnabled } from "./uploads.js";

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

async function hostedReport(env, reportId) {
  if (!env.RLOGS_DB || !env.RLOGS_ARTIFACTS) return null;
  try {
    const row = await env.RLOGS_DB.prepare(
      `SELECT r.visibility, r.projection_object_key, u.submitter_id
       FROM reports r JOIN upload_sessions u ON u.upload_id=r.upload_id
       WHERE r.report_id=?1`,
    ).bind(reportId).first();
    if (!row || row.visibility === "private") return null;
    const object = await env.RLOGS_ARTIFACTS.get(row.projection_object_key);
    if (!object) return null;
    const report = await object.json();
    const names = await env.RLOGS_DB.prepare(`SELECT rm.actor_id, p.public_projection_json
      FROM report_memberships rm
      JOIN profiles p ON p.game_id=rm.game_id AND p.character_id=rm.character_id
      WHERE rm.report_id=?1 AND p.submitter_id=?2 AND rm.actor_id IS NOT NULL`)
      .bind(reportId, row.submitter_id).all();
    return applyVerifiedSubmitterNames(
      { ...report, visibility: row.visibility },
      names.results ?? [],
    );
  } catch (cause) {
    console.error("rLogs hosted report read failed", cause);
    return null;
  }
}

function compatibleProfileName(profile) {
  const value = profile?.display_name ?? profile?.envelope?.body?.display_name;
  if (typeof value !== "string") return null;
  const name = value.trim();
  if (!name || name.length > 128 || /^player(?:\s+\d+)?$/iu.test(name) || /^unknown$/iu.test(name)) {
    return null;
  }
  return name;
}

export function applyVerifiedSubmitterNames(report, rows) {
  const names = new Map();
  for (const row of rows) {
    if (row.actor_id == null || typeof row.public_projection_json !== "string") continue;
    try {
      const name = compatibleProfileName(JSON.parse(row.public_projection_json));
      if (name) names.set(String(row.actor_id), name);
    } catch {}
  }
  if (names.size === 0) return report;
  const enriched = structuredClone(report);
  for (const run of enriched.runs ?? []) {
    for (const participant of run.participants ?? []) {
      const name = names.get(String(participant.actor_id));
      if (name) participant.display_name = name;
    }
  }
  return enriched;
}

async function publicReport(env, reportId) {
  const hosted = await hostedReport(env, reportId);
  if (hosted) return json(hosted);
  const report = await env.RLOGS_DATA.get(`fs:projections/${reportId}.json`, "json");
  if (!report) return notFound();
  const overrides = await visibilityOverrides(env);
  const visibility = overrides[reportId] ?? report.visibility;
  return visibility === "private" ? notFound() : json({ ...report, visibility });
}

async function hostedCatalogEntries(env) {
  if (!env.RLOGS_DB) return [];
  try {
    const result = await env.RLOGS_DB.prepare(`SELECT rr.catalog_entry_json
      FROM report_runs rr JOIN reports r ON r.report_id=rr.report_id
      WHERE r.visibility='public'
      ORDER BY rr.created_unix_millis DESC, rr.report_id, rr.run_index
      LIMIT 100000`).all();
    return (result.results ?? []).flatMap((row) => {
      try { return [JSON.parse(row.catalog_entry_json)]; } catch { return []; }
    });
  } catch (cause) {
    console.error("rLogs hosted parse catalog read failed", cause);
    return [];
  }
}

function facetValues(entries, field) {
  const counts = new Map();
  for (const entry of entries) {
    const value = entry[field];
    if (value != null && value !== "") counts.set(String(value), (counts.get(String(value)) ?? 0) + 1);
  }
  return [...counts].sort(([left], [right]) => left.localeCompare(right))
    .map(([id, count]) => ({ id, count }));
}

function catalogFacets(entries) {
  const scenes = new Map();
  for (const entry of entries) {
    if (entry.scene_id == null) continue;
    const id = String(entry.scene_id);
    const current = scenes.get(id) ?? { id: Number(entry.scene_id), label: entry.scene_name ?? null, count: 0 };
    current.count += 1;
    current.label ??= entry.scene_name ?? null;
    scenes.set(id, current);
  }
  return {
    deployments: facetValues(entries, "deployment_id"),
    regions: facetValues(entries, "region_id"),
    activities: facetValues(entries, "activity_category_id"),
    scenes: [...scenes.values()].sort((left, right) => left.id - right.id),
    difficulties: facetValues(entries, "difficulty_family"),
    terminal_states: facetValues(entries, "terminal_state"),
  };
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
  // The upload routes remain unavailable until an operator deliberately sets
  // the promotion flag and all storage/verifier bindings exist. Merely adding
  // R2 cannot advertise a half-configured submission service.
  const parseUploadRoutesImplemented = uploadsEnabled(env);
  return {
    public_reads: publicReadsReady,
    discord_auth: discordAuth,
    profile_sync: publicReadsReady && discordAuth,
    artifact_storage: artifactStorage,
    hosted_verification: hostedVerification,
    parse_uploads: parseUploadRoutesImplemented
      && publicReadsReady
      && discordAuth
      && artifactStorage
      && hostedVerification,
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

async function profileLeaderboards(env, url) {
  if (!env.RLOGS_DB) return json({ error: "leaderboard storage is unavailable" }, 503);
  const requestedSeason = Number.parseInt(url.searchParams.get("season") ?? "3", 10);
  const season = Number.isSafeInteger(requestedSeason) && requestedSeason > 0 ? requestedSeason : 3;
  const region = url.searchParams.get("region")?.trim() || null;
  const requestedActivity = Number.parseInt(url.searchParams.get("activity") ?? "0", 10);
  const activity = Number.isSafeInteger(requestedActivity) && requestedActivity > 0 ? requestedActivity : null;
  const requestedTier = Number.parseInt(url.searchParams.get("tier") ?? "20", 10);
  const tier = Number.isSafeInteger(requestedTier) && requestedTier >= 1 && requestedTier <= 20 ? requestedTier : 20;
  const requestedLimit = Number.parseInt(url.searchParams.get("limit") ?? "100", 10);
  const limit = Number.isSafeInteger(requestedLimit) ? Math.min(100, Math.max(1, requestedLimit)) : 100;
  const regionClause = region ? " AND p.region_id=?2" : "";
  const scoreBindings = region ? [season, region, limit] : [season, limit];
  const scoreLimitParameter = region ? "?3" : "?2";
  const scoreQuery = env.RLOGS_DB.prepare(`SELECT
      r.profile_id, p.character_id, json_extract(p.public_projection_json, '$.display_name') AS display_name,
      p.deployment_id, p.region_id, p.realm_id, r.master_score, r.observed_unix_millis
    FROM profile_season_rankings r
    JOIN profiles p ON p.profile_id=r.profile_id
    WHERE r.season_id=?1${regionClause}
    ORDER BY r.master_score DESC, r.observed_unix_millis, r.profile_id
    LIMIT ${scoreLimitParameter}`).bind(...scoreBindings);

  let timeQuery = null;
  if (activity != null) {
    const bindings = region
      ? [season, activity, tier, region, limit]
      : [season, activity, tier, limit];
    const timeRegionClause = region ? " AND p.region_id=?4" : "";
    const timeLimitParameter = region ? "?5" : "?4";
    timeQuery = env.RLOGS_DB.prepare(`SELECT
        d.profile_id, p.character_id, json_extract(p.public_projection_json, '$.display_name') AS display_name,
        p.deployment_id, p.region_id, p.realm_id, d.activity_id, d.tier, d.score,
        d.pass_time_seconds, d.completion_count, d.observed_unix_millis
      FROM profile_dungeon_records d
      JOIN profiles p ON p.profile_id=d.profile_id
      WHERE d.season_id=?1 AND d.activity_id=?2 AND d.tier=?3${timeRegionClause}
      ORDER BY d.pass_time_seconds, d.score DESC, d.observed_unix_millis, d.profile_id
      LIMIT ${timeLimitParameter}`).bind(...bindings);
  }
  try {
    const [scores, times] = await Promise.all([
      scoreQuery.all(),
      timeQuery ? timeQuery.all() : Promise.resolve({ results: [] }),
    ]);
    return json({
      schema_version: 1,
      season_id: season,
      region_id: region,
      activity_id: activity,
      tier,
      master_scores: scores.results ?? [],
      dungeon_times: times.results ?? [],
    }, 200, { "Cache-Control": "public, max-age=60, stale-while-revalidate=300" });
  } catch (cause) {
    console.error("rLogs profile leaderboard read failed", cause);
    return json({ error: "profile leaderboard is temporarily unavailable" }, 503);
  }
}

async function parseCatalog(env, url) {
  const storedCatalog = await env.RLOGS_DATA.get("fs:catalog.v1.json", "json");
  const catalog = storedCatalog && Array.isArray(storedCatalog.entries)
    ? storedCatalog
    : { schema_version: 6, entries: [], facets: catalogFacets([]) };
  const overrides = await visibilityOverrides(env);
  const hosted = await hostedCatalogEntries(env);
  const merged = new Map();
  for (const entry of catalog.entries) {
    if (overrides[entry.report_id] !== "private") merged.set(`${entry.report_id}:${entry.run_index}`, entry);
  }
  for (const entry of hosted) merged.set(`${entry.report_id}:${entry.run_index}`, entry);
  let entries = [...merged.values()].sort((left, right) =>
    Number(right.created_unix_millis) - Number(left.created_unix_millis) ||
    String(left.report_id).localeCompare(String(right.report_id)) ||
    Number(left.run_index) - Number(right.run_index));
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
  const submitterIds = [...new Set(page.map((entry) => entry.submitter_id).filter(Boolean))];
  const submitterNames = new Map(await Promise.all(submitterIds.map(async (submitterId) => {
    let account = null;
    try {
      account = await env.RLOGS_DB.prepare(
        "SELECT username, discord_global_name FROM accounts WHERE submitter_id=?1",
      ).bind(submitterId).first();
    } catch {}
    account ??= await env.RLOGS_DATA.get(`fs:accounts/users/${submitterId}.json`, "json");
    const name = account?.discord_global_name || account?.username || null;
    return [submitterId, name];
  })));
  const presentedPage = page.map((entry) => ({
    ...entry,
    ...(entry.submitter_id && submitterNames.get(entry.submitter_id)
      ? { submitter_name: submitterNames.get(entry.submitter_id) }
      : {}),
  }));
  return json({
    ...catalog,
    facets: catalogFacets(entries),
    total_entries: entries.length,
    offset,
    next_offset: offset + page.length < entries.length ? offset + page.length : null,
    entries: presentedPage,
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
  let uploadResponse;
  try {
    uploadResponse = await routeUpload(request, env, path);
  } catch (cause) {
    console.error("rLogs upload service failure", cause);
    uploadResponse = json(
      { error: "hosted upload service is temporarily unavailable", retryable: true },
      503,
      { "Retry-After": "30" },
    );
  }
  if (uploadResponse) return uploadResponse;
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
  if (path === "/v1/leaderboards/profiles") return profileLeaderboards(env, url);
  if (path === "/v1/characters") return storedJson(env, "characters/catalog.v1.json");
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
