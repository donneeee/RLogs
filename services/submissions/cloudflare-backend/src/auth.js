import { publishProfilePackage, publishProfilePhoto } from "./profile.js";

const encoder = new TextEncoder();
const WEB_SESSION_LIFETIME_MILLIS = 30 * 24 * 60 * 60 * 1000;
const OAUTH_STATE_LIFETIME_MILLIS = 10 * 60 * 1000;
const LOGIN_CODE_LIFETIME_MILLIS = 5 * 60 * 1000;
const MAXIMUM_QUERY_LIMIT = 250;
const REPORT_ID_PATTERN = /^rpt_[a-f0-9]{32}$/;
const VISIBILITIES = new Set(["public", "unlisted", "private"]);

function json(value, status = 200) {
  return Response.json(value, {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
    },
  });
}

function error(message, status) {
  return json({ error: message }, status);
}

function bearer(request) {
  const value = request.headers.get("Authorization") ?? "";
  return value.startsWith("Bearer ") ? value.slice(7).trim() : "";
}

function randomToken(prefix) {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return `${prefix}_${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}`;
}

async function tokenHash(domain, token, pepper) {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    encoder.encode(`rlogs-auth-v1\0${domain}\0${pepper}\0${token}`),
  );
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
}

async function legacyJson(env, path) {
  return env.RLOGS_DATA.get(`fs:accounts/${path}`, "json");
}

async function legacyText(env, path) {
  return env.RLOGS_DATA.get(`fs:accounts/${path}`, "text");
}

function accountView(record, env) {
  if (!record || !Number.isSafeInteger(record.account_id) || typeof record.username !== "string") {
    return null;
  }
  const developers = new Set(String(env.DEVELOPER_DISCORD_USER_IDS ?? "").split(",").map((value) => value.trim()));
  return {
    schema_version: 1,
    submitter_id: record.submitter_id,
    account_id: record.account_id,
    username: record.username,
    discord_username: record.discord_username,
    discord_global_name: record.discord_global_name ?? null,
    discord_avatar_url: record.discord_avatar_url ?? null,
    publish_verified_parses: record.publish_verified_parses === true,
    developer: developers.has(record.discord_user_id),
  };
}

async function parseBody(request) {
  try {
    const value = await request.json();
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

export class RLogsAuthState {
  constructor(state, env) {
    this.state = state;
    this.storage = state.storage;
    this.env = env;
  }

  async fetch(request) {
    try {
      return await this.route(request);
    } catch (cause) {
      console.error("rLogs authentication failure", cause);
      return error("authentication service is temporarily unavailable", 503);
    }
  }

  async route(request) {
    const url = new URL(request.url);
    const path = url.pathname;
    const now = Date.now();
    if (request.method === "GET" && path === "/v1/auth/discord/start") {
      return this.beginDiscord(now);
    }
    if (request.method === "POST" && path === "/v1/auth/discord/complete") {
      return this.completeDiscord(request, now);
    }
    if (request.method === "POST" && path === "/v1/auth/session/exchange") {
      return this.exchangeSession(request, now);
    }
    if (request.method === "GET" && path === "/v1/auth/me") {
      const account = await this.authenticateWeb(request, now);
      return account ? json(accountView(account, this.env)) : error("account authentication failed", 401);
    }
    if (request.method === "PATCH" && path === "/v1/auth/me") {
      return this.updateUsername(request, now);
    }
    if (request.method === "PATCH" && path === "/v1/auth/me/parse-publication") {
      return this.updateParsePublication(request, now);
    }
    if (request.method === "POST" && path === "/v1/auth/app-tokens") {
      return this.issueDeviceToken(request, now);
    }
    if (request.method === "GET" && path === "/v1/auth/device") {
      const identity = await this.authenticateDevice(request);
      if (!identity) return error("write authorization failed", 401);
      return json({ schema_version: 1, ...identity, authentication: "device_token" });
    }
    if (request.method === "GET" && path === "/v1/auth/profiles") {
      return this.ownedProfiles(request, now);
    }
    if (request.method === "POST" && path === "/v1/games/blue-protocol-star-resonance/profiles") {
      return this.publishProfile(request, now);
    }
    let match = /^\/v1\/games\/blue-protocol-star-resonance\/profiles\/(prf_[a-z0-9_]{32})\/photo-wall\/([1-9][0-9]*)$/.exec(path);
    if (request.method === "PUT" && match) {
      return this.publishPhoto(request, now, match[1], Number(match[2]));
    }
    if (request.method === "GET" && path === "/v1/photos") {
      return this.photoCatalog(request, now, url);
    }
    match = /^\/v1\/users\/([1-9][0-9]{11})$/.exec(path);
    if (request.method === "GET" && match) {
      return this.publicAccount(match[1]);
    }
    match = /^\/v1\/profiles\/(prf_[a-z0-9_]{32})\/photo-wall\/([1-9][0-9]*)\/like$/.exec(path);
    if ((request.method === "PUT" || request.method === "DELETE") && match) {
      return this.setPhotoLike(request, now, match[1], Number(match[2]), request.method === "PUT");
    }
    if (request.method === "GET" && path === "/v1/auth/parses") {
      return this.myParses(request, now, url);
    }
    match = /^\/v1\/auth\/parses\/(rpt_[a-f0-9]{32})$/.exec(path);
    if (request.method === "GET" && match) {
      return this.accountParse(request, now, match[1]);
    }
    match = /^\/v1\/auth\/parses\/(rpt_[a-f0-9]{32})\/visibility$/.exec(path);
    if (request.method === "PATCH" && match) {
      return this.updateParseVisibility(request, now, match[1]);
    }
    if (request.method === "GET" && path === "/internal/visibility-overrides") {
      return json(await this.visibilityOverrides());
    }
    return error("route not found", 404);
  }

  async beginDiscord(now) {
    if (!this.configured()) return error("account authentication is not configured", 503);
    const state = randomToken("state");
    const hash = await tokenHash("oauth-state", state, this.env.AUTH_TOKEN_PEPPER);
    await this.storage.put(`oauth:${hash}`, { expires_unix_millis: now + OAUTH_STATE_LIFETIME_MILLIS });
    const url = new URL("https://discord.com/oauth2/authorize");
    url.searchParams.set("client_id", this.env.DISCORD_CLIENT_ID);
    url.searchParams.set("redirect_uri", this.env.DISCORD_CALLBACK_URL);
    url.searchParams.set("response_type", "code");
    url.searchParams.set("scope", "identify");
    url.searchParams.set("state", state);
    url.searchParams.set("prompt", "consent");
    return Response.redirect(url, 307);
  }

  async completeDiscord(request, now) {
    if (!this.configured()) return error("account authentication is not configured", 503);
    const body = await parseBody(request);
    if (typeof body?.code !== "string" || typeof body?.state !== "string") {
      return error("invalid or expired authentication code", 400);
    }
    const stateHash = await tokenHash("oauth-state", body.state, this.env.AUTH_TOKEN_PEPPER);
    const stateKey = `oauth:${stateHash}`;
    const state = await this.storage.get(stateKey);
    await this.storage.delete(stateKey);
    if (!state || state.expires_unix_millis < now) {
      return error("invalid or expired authentication code", 400);
    }
    const tokenResponse = await fetch("https://discord.com/api/oauth2/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: this.env.DISCORD_CLIENT_ID,
        client_secret: this.env.DISCORD_CLIENT_SECRET,
        grant_type: "authorization_code",
        code: body.code,
        redirect_uri: this.env.DISCORD_CALLBACK_URL,
      }),
    });
    if (!tokenResponse.ok) return error("Discord authentication is temporarily unavailable", 503);
    const token = await tokenResponse.json();
    const discordResponse = await fetch("https://discord.com/api/users/@me", {
      headers: { Authorization: `Bearer ${token.access_token}` },
    });
    if (!discordResponse.ok) return error("Discord authentication is temporarily unavailable", 503);
    const discord = await discordResponse.json();
    if (!discord?.id || !discord?.username) return error("Discord authentication is temporarily unavailable", 503);
    const account = await this.upsertDiscord(discord, now);
    const loginCode = randomToken("login");
    const loginHash = await tokenHash("login-code", loginCode, this.env.AUTH_TOKEN_PEPPER);
    await this.storage.put(`login:${loginHash}`, {
      submitter_id: account.submitter_id,
      expires_unix_millis: now + LOGIN_CODE_LIFETIME_MILLIS,
    });
    return json({ schema_version: 1, login_code: loginCode });
  }

  async exchangeSession(request, now) {
    const body = await parseBody(request);
    if (typeof body?.code !== "string") return error("invalid or expired authentication code", 400);
    const hash = await tokenHash("login-code", body.code, this.env.AUTH_TOKEN_PEPPER);
    const key = `login:${hash}`;
    const record = await this.storage.get(key);
    await this.storage.delete(key);
    if (!record || record.expires_unix_millis < now) return error("invalid or expired authentication code", 400);
    const accessToken = randomToken("rlw");
    const sessionHash = await tokenHash("web-session", accessToken, this.env.AUTH_TOKEN_PEPPER);
    const expires = now + WEB_SESSION_LIFETIME_MILLIS;
    await this.storage.put(`web:${sessionHash}`, {
      submitter_id: record.submitter_id,
      expires_unix_millis: expires,
    });
    const account = await this.account(record.submitter_id);
    return json({
      schema_version: 1,
      access_token: accessToken,
      expires_unix_millis: expires,
      account: accountView(account, this.env),
    });
  }

  async updateUsername(request, now) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const body = await parseBody(request);
    const username = typeof body?.username === "string" ? body.username.trim().toLocaleLowerCase() : "";
    if (!/^[a-z0-9][a-z0-9._-]{2,23}$/.test(username)) {
      return error("username must contain 3-24 lowercase letters, numbers, dots, underscores, or hyphens", 400);
    }
    const existing = await this.index("username", username);
    if (existing && existing !== account.submitter_id) return error("username is already in use", 409);
    if (account.username !== username) {
      await this.storage.put(`index:username:${username}`, account.submitter_id);
      account.username = username;
      account.updated_unix_millis = now;
      await this.storage.put(`user:${account.submitter_id}`, account);
    }
    return json(accountView(account, this.env));
  }

  async updateParsePublication(request, now) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const body = await parseBody(request);
    if (typeof body?.publish_verified_parses !== "boolean") return error("invalid publication preference", 400);
    account.publish_verified_parses = body.publish_verified_parses;
    account.updated_unix_millis = now;
    await this.storage.put(`user:${account.submitter_id}`, account);
    return json(accountView(account, this.env));
  }

  async issueDeviceToken(request, now) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const deviceToken = randomToken("rld");
    const deviceId = `dev_${crypto.randomUUID().replaceAll("-", "")}`;
    const hash = await tokenHash("device-token", deviceToken, this.env.AUTH_TOKEN_PEPPER);
    await this.storage.put(`device:${hash}`, {
      schema_version: 1,
      submitter_id: account.submitter_id,
      device_id: deviceId,
      created_unix_millis: now,
      revoked_unix_millis: null,
    });
    return json({ schema_version: 1, device_token: deviceToken, device_id: deviceId, created_unix_millis: now });
  }

  async ownedProfiles(request, now) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const catalog = await this.env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json");
    if (!catalog || !Array.isArray(catalog.profiles)) return error("profile catalog unavailable", 503);
    const profiles = [];
    for (const profile of catalog.profiles) {
      const claim = await this.env.RLOGS_DATA.get(`fs:profiles/${profile.profile_id}/claim.json`, "json");
      if (claim?.submitter_id === account.submitter_id) profiles.push(profile);
    }
    return json({ schema_version: 1, profiles });
  }

  async publishProfile(request, now) {
    const deviceToken = bearer(request);
    const identity = await this.authenticateDevice(request);
    if (!identity) return error("write authorization failed", 401);
    const packageValue = await parseBody(request);
    if (!packageValue) return error("profile package is invalid: malformed JSON", 400);
    const result = await publishProfilePackage(this.env, packageValue, identity, deviceToken, now);
    return result.error ? error(result.error, result.status) : json(result.value);
  }

  async publishPhoto(request, now, profileId, photoId) {
    const identity = await this.authenticateDevice(request);
    if (!identity) return error("write authorization failed", 401);
    const bytes = await request.arrayBuffer();
    const result = await publishProfilePhoto(this.env, profileId, photoId, bytes, identity, now);
    return result.error ? error(result.error, result.status) : json(result.value);
  }

  async photoCatalog(request, now, url) {
    let viewer = null;
    if (bearer(request)) {
      viewer = await this.authenticateWeb(request, now);
      if (!viewer) return error("account authentication failed", 401);
    }
    const entries = [];
    for (const key of await this.listKvKeys("fs:profiles/")) {
      if (!/\/photo-wall\/photo-[1-9][0-9]*\.json$/.test(key)) continue;
      const metadata = await this.env.RLOGS_DATA.get(key, "json");
      if (!metadata?.profile_id || !Number.isSafeInteger(metadata.photo_id)) continue;
      const profile = await this.env.RLOGS_DATA.get(`fs:profiles/${metadata.profile_id}/public.json`, "json");
      if (!profile) continue;
      const likeState = await this.photoLikeState(metadata.profile_id, metadata.photo_id, viewer?.submitter_id);
      entries.push({
        profile_id: metadata.profile_id,
        character_id: profile.character_id,
        display_name: profile.display_name ?? null,
        photo_id: metadata.photo_id,
        image_path: metadata.image_path,
        uploaded_unix_millis: metadata.uploaded_unix_millis || profile.updated_unix_millis,
        like_count: likeState.count,
        viewer_liked: likeState.viewerLiked,
      });
    }
    const sort = url.searchParams.get("sort") === "popular" ? "popular" : "newest";
    entries.sort((left, right) => sort === "popular"
      ? right.like_count - left.like_count || right.uploaded_unix_millis - left.uploaded_unix_millis || left.profile_id.localeCompare(right.profile_id) || left.photo_id - right.photo_id
      : right.uploaded_unix_millis - left.uploaded_unix_millis || right.like_count - left.like_count || left.profile_id.localeCompare(right.profile_id) || left.photo_id - right.photo_id);
    const requested = Number.parseInt(url.searchParams.get("limit") ?? "24", 10);
    const limit = Number.isSafeInteger(requested) ? Math.min(100, Math.max(1, requested)) : 24;
    return json({ schema_version: 1, total_entries: entries.length, entries: entries.slice(0, limit) });
  }

  async setPhotoLike(request, now, profileId, photoId, liked) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const metadata = await this.env.RLOGS_DATA.get(`fs:profiles/${profileId}/photo-wall/photo-${photoId}.json`, "json");
    if (!metadata) return error("not found", 404);
    const digest = await photoLikeDigest(account.submitter_id);
    const key = `photo-like:${profileId}:${photoId}:${digest}`;
    if (liked) await this.storage.put(key, { liked_unix_millis: now }); else await this.storage.delete(key);
    const state = await this.photoLikeState(profileId, photoId, account.submitter_id);
    return json({ schema_version: 1, profile_id: profileId, photo_id: photoId, liked, like_count: state.count });
  }

  async photoLikeState(profileId, photoId, viewerSubmitterId) {
    const prefix = `photo-like:${profileId}:${photoId}:`;
    const current = await this.storage.list({ prefix });
    const legacyPrefix = `fs:profiles/${profileId}/photo-wall/likes/photo-${photoId}/`;
    const legacy = await this.listKvKeys(legacyPrefix);
    let viewerLiked = false;
    if (viewerSubmitterId) {
      const digest = await photoLikeDigest(viewerSubmitterId);
      viewerLiked = current.has(`${prefix}${digest}`) || legacy.includes(`${legacyPrefix}${digest}.json`);
    }
    return { count: current.size + legacy.length, viewerLiked };
  }

  async publicAccount(accountId) {
    const submitterId = await this.index("account", accountId);
    if (!submitterId) return error("not found", 404);
    const account = await this.account(submitterId);
    if (!account) return error("not found", 404);
    const catalog = await this.env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json");
    const profiles = [];
    for (const profile of catalog?.profiles ?? []) {
      const claim = await this.env.RLOGS_DATA.get(`fs:profiles/${profile.profile_id}/claim.json`, "json");
      if (claim?.submitter_id === submitterId) profiles.push(profile);
    }
    return json({
      schema_version: 1,
      account: { schema_version: 1, account_id: account.account_id, username: account.username },
      profiles,
    });
  }

  async myParses(request, now, url) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const claimedCharacterIds = await this.claimedCharacterIds(account.submitter_id);
    const baseCatalog = await this.env.RLOGS_DATA.get("fs:catalog.v1.json", "json");
    const catalogEntries = Array.isArray(baseCatalog?.entries) ? baseCatalog.entries : [];
    const byRun = new Map(catalogEntries.map((entry) => [`${entry.report_id}:${entry.run_index}`, entry]));
    const entries = [];
    for (const key of await this.listKvKeys("fs:projections/")) {
      if (!key.endsWith(".json")) continue;
      const report = await this.env.RLOGS_DATA.get(key, "json");
      if (!report || !REPORT_ID_PATTERN.test(report.report_id) || !Array.isArray(report.runs)) continue;
      const visibility = await this.reportVisibility(report);
      const submittedByYou = report.submission_provenance?.submitter_id === account.submitter_id;
      if (visibility === "private" && !submittedByYou) continue;
      const membership = await this.env.RLOGS_DATA.get(`fs:memberships/${report.report_id}.json`, "json");
      for (const run of report.runs) {
        const runMembership = Array.isArray(membership?.runs)
          ? membership.runs.find((candidate) => candidate.run_index === run.run_index)
          : null;
        const matchedCharacterIds = (runMembership?.character_ids ?? [])
          .filter((characterId) => claimedCharacterIds.has(String(characterId)))
          .map(String);
        if (!submittedByYou && matchedCharacterIds.length === 0) continue;
        const parse = byRun.get(`${report.report_id}:${run.run_index}`) ?? catalogEntry(report, run);
        entries.push({ ...parse, visibility, submitted_by_you: submittedByYou, matched_character_ids: matchedCharacterIds });
      }
    }
    entries.sort((left, right) =>
      right.created_unix_millis - left.created_unix_millis ||
      left.report_id.localeCompare(right.report_id) ||
      left.run_index - right.run_index,
    );
    const requestedOffset = Number.parseInt(url.searchParams.get("offset") ?? "0", 10);
    const requestedLimit = Number.parseInt(url.searchParams.get("limit") ?? "100", 10);
    const offset = Number.isSafeInteger(requestedOffset) && requestedOffset > 0
      ? Math.min(requestedOffset, entries.length)
      : 0;
    const limit = Number.isSafeInteger(requestedLimit)
      ? Math.min(MAXIMUM_QUERY_LIMIT, Math.max(1, requestedLimit))
      : 100;
    const page = entries.slice(offset, offset + limit);
    return json({
      schema_version: 1,
      total_entries: entries.length,
      offset,
      next_offset: offset + page.length < entries.length ? offset + page.length : null,
      claimed_character_ids: [...claimedCharacterIds].sort(),
      entries: page,
    });
  }

  async accountParse(request, now, reportId) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const report = await this.env.RLOGS_DATA.get(`fs:projections/${reportId}.json`, "json");
    if (!report) return error("not found", 404);
    const visibility = await this.reportVisibility(report);
    const submittedByYou = report.submission_provenance?.submitter_id === account.submitter_id;
    if (!submittedByYou) {
      if (visibility === "private") return error("not found", 404);
      const claimedCharacterIds = await this.claimedCharacterIds(account.submitter_id);
      const membership = await this.env.RLOGS_DATA.get(`fs:memberships/${reportId}.json`, "json");
      const participates = (membership?.runs ?? []).some((run) =>
        (run.character_ids ?? []).some((characterId) => claimedCharacterIds.has(String(characterId))),
      );
      if (!participates) return error("not found", 404);
    }
    return json({ ...report, visibility });
  }

  async updateParseVisibility(request, now, reportId) {
    const account = await this.authenticateWeb(request, now);
    if (!account) return error("account authentication failed", 401);
    const report = await this.env.RLOGS_DATA.get(`fs:projections/${reportId}.json`, "json");
    if (!report || report.submission_provenance?.submitter_id !== account.submitter_id) {
      return error("not found", 404);
    }
    const body = await parseBody(request);
    if (!body || Object.keys(body).length !== 1 || !VISIBILITIES.has(body.visibility)) {
      return error("invalid visibility", 400);
    }
    await this.storage.put(`visibility:${reportId}`, body.visibility);
    return json({
      schema_version: 1,
      report_id: reportId,
      visibility: body.visibility,
      share_url: body.visibility === "private"
        ? null
        : `${String(this.env.WEBSITE_URL).replace(/\/$/, "")}/parses/?parse=${reportId}#parse`,
    });
  }

  async claimedCharacterIds(submitterId) {
    const catalog = await this.env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json");
    const claimed = new Set();
    for (const profile of catalog?.profiles ?? []) {
      const claim = await this.env.RLOGS_DATA.get(`fs:profiles/${profile.profile_id}/claim.json`, "json");
      if (claim?.submitter_id === submitterId) claimed.add(String(profile.character_id));
    }
    return claimed;
  }

  async reportVisibility(report) {
    return await this.storage.get(`visibility:${report.report_id}`) ?? report.visibility;
  }

  async visibilityOverrides() {
    const overrides = {};
    for (const key of await this.listDoKeys("visibility:")) {
      overrides[key.slice("visibility:".length)] = await this.storage.get(key);
    }
    return overrides;
  }

  async listDoKeys(prefix) {
    const values = await this.storage.list({ prefix });
    return [...values.keys()];
  }

  async listKvKeys(prefix) {
    const keys = [];
    let cursor;
    do {
      const page = await this.env.RLOGS_DATA.list({ prefix, cursor });
      keys.push(...page.keys.map((entry) => entry.name));
      cursor = page.list_complete ? undefined : page.cursor;
    } while (cursor);
    return keys;
  }

  async authenticateWeb(request, now) {
    const token = bearer(request);
    if (!token.startsWith("rlw_")) return null;
    const hash = await tokenHash("web-session", token, this.env.AUTH_TOKEN_PEPPER);
    let record = await this.storage.get(`web:${hash}`);
    record ??= await legacyJson(this.env, `web-sessions/${hash}.json`);
    if (!record || record.expires_unix_millis < now) return null;
    return this.account(record.submitter_id);
  }

  async authenticateDevice(request) {
    const token = bearer(request);
    if (!token.startsWith("rld_")) return null;
    const hash = await tokenHash("device-token", token, this.env.AUTH_TOKEN_PEPPER);
    let record = await this.storage.get(`device:${hash}`);
    record ??= await legacyJson(this.env, `device-tokens/${hash}.json`);
    if (!record || record.revoked_unix_millis != null) return null;
    return { submitter_id: record.submitter_id, device_id: record.device_id };
  }

  async account(submitterId) {
    let record = await this.storage.get(`user:${submitterId}`);
    record ??= await legacyJson(this.env, `users/${submitterId}.json`);
    return record;
  }

  async index(kind, value) {
    let result = await this.storage.get(`index:${kind}:${value}`);
    if (result) return result;
    if (kind === "username") return legacyText(this.env, `username-index/${value}.json`).then(parseStringJson);
    if (kind === "discord") return legacyText(this.env, `discord-index/${value}.json`).then(parseStringJson);
    if (kind === "account") return legacyText(this.env, `account-id-index/${value}.json`).then(parseStringJson);
    return null;
  }

  async upsertDiscord(discord, now) {
    const discordHash = await tokenHash("discord-user", discord.id, this.env.AUTH_TOKEN_PEPPER);
    let submitterId = await this.index("discord", discordHash);
    if (!submitterId) {
      const submitterHash = await tokenHash("submitter", discord.id, this.env.AUTH_TOKEN_PEPPER);
      submitterId = `usr_${submitterHash.slice(0, 32)}`;
    }
    const existing = await this.account(submitterId);
    const accountId = existing?.account_id ?? await this.allocateAccountId(submitterId);
    const username = existing?.username ?? await this.allocateUsername(discord.username, submitterId, accountId);
    const record = {
      schema_version: 1,
      submitter_id: submitterId,
      account_id: accountId,
      username,
      discord_user_id: discord.id,
      discord_username: discord.username,
      discord_global_name: discord.global_name ?? null,
      discord_avatar_url: discord.avatar
        ? `https://cdn.discordapp.com/avatars/${discord.id}/${discord.avatar}.png`
        : null,
      publish_verified_parses: existing?.publish_verified_parses === true,
      created_unix_millis: existing?.created_unix_millis ?? now,
      updated_unix_millis: now,
    };
    await this.storage.put({
      [`user:${submitterId}`]: record,
      [`index:discord:${discordHash}`]: submitterId,
      [`index:account:${accountId}`]: submitterId,
      [`index:username:${username}`]: submitterId,
    });
    return record;
  }

  async allocateAccountId(submitterId) {
    const digest = await crypto.subtle.digest("SHA-256", encoder.encode(`rlogs-account-id-v1\0${submitterId}`));
    const bytes = new Uint8Array(digest);
    let seed = 0n;
    for (const value of bytes.slice(0, 8)) seed = (seed << 8n) | BigInt(value);
    for (let offset = 0n; offset < 900_000_000_000n; offset += 1n) {
      const candidate = Number(100_000_000_000n + ((seed + offset) % 900_000_000_000n));
      const owner = await this.index("account", String(candidate));
      if (!owner || owner === submitterId) return candidate;
    }
    throw new Error("could not allocate account ID");
  }

  async allocateUsername(discordName, submitterId, accountId) {
    const base = String(discordName).toLocaleLowerCase().replace(/[^a-z0-9._-]+/g, "").slice(0, 20) || "player";
    for (const candidate of [base, `${base}-${String(accountId).slice(-4)}`, `player-${String(accountId).slice(-6)}`]) {
      const owner = await this.index("username", candidate);
      if (!owner || owner === submitterId) return candidate;
    }
    throw new Error("could not allocate username");
  }

  configured() {
    return Boolean(
      this.env.DISCORD_CLIENT_ID &&
      this.env.DISCORD_CLIENT_SECRET &&
      this.env.DISCORD_CALLBACK_URL &&
      this.env.AUTH_TOKEN_PEPPER,
    );
  }
}

function parseStringJson(value) {
  if (value == null) return null;
  try {
    const parsed = JSON.parse(value);
    return typeof parsed === "string" ? parsed : null;
  } catch {
    return null;
  }
}

async function photoLikeDigest(submitterId) {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(`rlogs-photo-like-v1\0${submitterId}`));
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
}

function catalogEntry(report, run) {
  return {
    report_id: report.report_id,
    report_ids: [report.report_id],
    run_index: run.run_index,
    run_group_id: run.run_group_id || `legacy_${report.report_id}_${run.run_index}`,
    contribution_count: 1,
    distinct_submitter_count: report.submission_provenance?.submitter_id ? 1 : 0,
    local_profile_witness_character_count: run.local_profile_character_ids?.length ?? 0,
    attribution_reconciliation_status: "single_vantage",
    created_unix_millis: report.created_unix_millis,
    deployment_id: report.deployment_id,
    region_id: report.region_id,
    activity_id: run.activity_id,
    activity_family_id: run.activity_family_id,
    activity_category_id: run.activity_category_id,
    scene_id: run.scene_id ?? null,
    scene_name: run.scene_name ?? null,
    difficulty_family: run.difficulty_family,
    difficulty_tier: run.difficulty_tier ?? null,
    terminal_state: run.terminal_state,
    total_run_time_micros: run.total_run_time_micros ?? null,
    participant_count: run.participants?.length ?? 0,
  };
}

export { accountView, catalogEntry, tokenHash };
