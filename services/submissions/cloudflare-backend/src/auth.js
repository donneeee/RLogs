const encoder = new TextEncoder();
const WEB_SESSION_LIFETIME_MILLIS = 30 * 24 * 60 * 60 * 1000;
const OAUTH_STATE_LIFETIME_MILLIS = 10 * 60 * 1000;
const LOGIN_CODE_LIFETIME_MILLIS = 5 * 60 * 1000;

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

export { accountView, tokenHash };
