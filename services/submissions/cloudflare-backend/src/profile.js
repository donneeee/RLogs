const encoder = new TextEncoder();
const BPSR_ENDPOINT = "/v1/games/blue-protocol-star-resonance/profiles";
const BPSR_GAME_PLUGIN_ID = "app.rlogs.game.blue-protocol-star-resonance";
const BPSR_PROFILE_SCHEMA_ID = "app.rlogs.bpsr.character-profile";
const LIVE_CAPTURE_KIND = "continuous_process_owned_capture";
const PROFILE_BODY_LIMIT = 8 * 1024 * 1024;
const CURRENT_BUILD_FIELDS = [
  "class_id", "specialization_id", "combat_power", "combat_power_breakdown", "combat_stats",
  "season_strength", "equipment", "equipment_suit_entries", "modules", "battle_imagine_skills",
  "equipped_action_slots", "active_skills", "talents", "talent_progress",
];
const REPLACE_FIELDS = [
  "display_name", "display_id", "server_id", "class_id", "specialization_id", "level",
  "combat_power", "combat_power_breakdown", "combat_stats", "season_strength", "master_score",
  "appearance", "equipment", "equipment_suit_entries", "modules", "owned_imagines",
  "battle_imagine_skills", "equipped_action_slots", "active_skills", "talents", "talent_progress",
  "combat_professions", "life_professions", "cosmetics", "activity_progress", "season_medals",
  "season_cultivation", "reputations", "current_profession_project_id", "profession_projects",
];
const PROHIBITED_KEYS = new Set([
  "password", "passphrase", "account", "authentication", "credential", "credentials", "login",
  "secret", "clientsecret", "token", "passwordciphertext", "passwordhash", "accesstoken",
  "refreshtoken", "authtoken", "sessiontoken", "authorization", "bearer", "cookie",
  "sessioncookie", "sessionid", "accountid", "platformaccountid", "publisheraccountid", "openid",
  "loginname", "userid", "discordid", "email", "emailaddress", "phonenumber",
]);

export async function publishProfilePackage(env, packageValue, identity, deviceToken, now) {
  const validation = await validatePackage(packageValue, identity.device_id, deviceToken);
  if (validation.error) return validation;
  const body = structuredClone(packageValue.request.payload.body);
  const characterId = String(body.character.character_id);
  const catalog = await env.RLOGS_DATA.get("fs:profiles/catalog.v1.json", "json") ?? { schema_version: 1, profiles: [] };
  if (!Array.isArray(catalog.profiles)) return { error: "profile catalog unavailable", status: 503 };
  let profileId = null;
  for (const entry of catalog.profiles) {
    if (String(entry.character_id) !== characterId) continue;
    const claim = await env.RLOGS_DATA.get(`fs:profiles/${entry.profile_id}/claim.json`, "json");
    if (claim?.submitter_id !== identity.submitter_id) {
      return { error: `UID ${characterId} is already claimed by another authenticated user`, status: 409 };
    }
    profileId = entry.profile_id;
    break;
  }
  profileId ??= await newProfileId(characterId);
  const prefix = `fs:profiles/${profileId}`;
  const existingClaim = await env.RLOGS_DATA.get(`${prefix}/claim.json`, "json");
  if (existingClaim && existingClaim.submitter_id !== identity.submitter_id) {
    return { error: `UID ${characterId} is already claimed by another authenticated user`, status: 409 };
  }
  const existing = await env.RLOGS_DATA.get(`${prefix}/public.json`, "json");
  const duplicate = existing?.package_id === packageValue.package_id;
  if (existing && !duplicate && packageValue.created_unix_millis <= existing.created_unix_millis) {
    return { error: "profile package is older than the currently published profile", status: 409 };
  }

  let currentBody = body;
  const projectId = positiveInteger(body.current_profession_project_id);
  let loadout = null;
  if (projectId != null) {
    const existingLoadout = await env.RLOGS_DATA.get(`${prefix}/loadouts/${projectId}.json`, "json");
    currentBody = existingLoadout ? mergeProfileBodies(existingLoadout.envelope.body, body) : structuredClone(body);
    loadout = makeLoadout(profileId, projectId, currentBody, packageValue, now);
  }

  let accumulatedBody = existing ? mergeProfileBodies(existing.envelope.body, body) : structuredClone(body);
  if (loadout) accumulatedBody = replaceCurrentBuild(accumulatedBody, currentBody);
  accumulatedBody = preservePhotoAssets(existing?.envelope?.body, accumulatedBody);
  const modules = accumulatedBody.modules;
  const loadouts = mergeLoadoutSummaries(existing?.loadouts, loadout, accumulatedBody.profession_projects, packageValue, now);
  const routing = packageValue.request.payload.routing;
  const published = {
    schema_version: 1,
    profile_id: profileId,
    claimed: true,
    package_id: packageValue.package_id,
    created_unix_millis: Math.max(existing?.created_unix_millis ?? 0, packageValue.created_unix_millis),
    updated_unix_millis: now,
    source_client_build: packageValue.source.client_build,
    source_observation_count: packageValue.source.observation_count,
    source_last_event_sequence: packageValue.source.last_event_sequence,
    deployment: routing.deployment,
    region: routing.region,
    realm: routing.realm ?? null,
    world: routing.world ?? null,
    character_id: characterId,
    display_name: accumulatedBody.display_name ?? null,
    module_inventory_count: Array.isArray(modules?.inventory) ? modules.inventory.length : 0,
    equipped_module_count: modules?.equipped_slots && typeof modules.equipped_slots === "object"
      ? Object.keys(modules.equipped_slots).length : 0,
    loadouts,
    envelope: { ...structuredClone(packageValue.request.payload), body: accumulatedBody },
  };
  const claim = existingClaim ?? {
    schema_version: 1, profile_id: profileId, submitter_id: identity.submitter_id,
    character_id: characterId, claimed_unix_millis: now,
  };
  if (!existingClaim) await env.RLOGS_DATA.put(`${prefix}/claim.json`, JSON.stringify(claim));
  await env.RLOGS_DATA.put(`${prefix}/current.profile.json`, JSON.stringify(packageValue));
  if (loadout) await env.RLOGS_DATA.put(`${prefix}/loadouts/${projectId}.json`, JSON.stringify(loadout));
  await env.RLOGS_DATA.put(`${prefix}/public.json`, JSON.stringify(published));
  catalog.profiles = catalog.profiles.filter((entry) => entry.profile_id !== profileId);
  catalog.profiles.push(catalogEntry(published));
  catalog.profiles.sort((left, right) => right.updated_unix_millis - left.updated_unix_millis || left.profile_id.localeCompare(right.profile_id));
  await env.RLOGS_DATA.put("fs:profiles/catalog.v1.json", JSON.stringify(catalog));
  return {
    value: {
      schema_version: 1, profile_id: profileId, character_id: characterId,
      package_id: packageValue.package_id, claimed: true, duplicate,
      module_inventory_count: published.module_inventory_count,
      equipped_module_count: published.equipped_module_count,
      profile_url: `${String(env.WEBSITE_URL).replace(/\/$/, "")}/profiles/${encodeURIComponent(characterId)}/`,
    },
  };
}

async function validatePackage(value, deviceId, deviceToken) {
  if (!isObject(value) || value.schema_version !== 2 || !positiveSafe(value.created_unix_millis)) return invalid("unsupported or incomplete package");
  const source = value.source;
  const request = value.request;
  const payload = request?.payload;
  const routing = payload?.routing;
  const body = payload?.body;
  if (!isObject(source) || !isObject(request) || !isObject(payload) || !isObject(routing) || !isObject(body)) return invalid("package structure is invalid");
  if (request.relative_endpoint !== BPSR_ENDPOINT || payload.schema_version !== 1 ||
      payload.game_plugin_id !== BPSR_GAME_PLUGIN_ID || payload.payload_kind !== "character-profile" ||
      payload.payload_schema_id !== BPSR_PROFILE_SCHEMA_ID || payload.payload_schema_version !== 1) return invalid("endpoint or BPSR profile schema identity does not match");
  if (JSON.stringify(payload).length > PROFILE_BODY_LIMIT) return invalid("profile payload exceeds the size limit");
  if (!["deployment", "region", "character-id"].every((key) => nonempty(routing[key]))) return invalid("required routing identity is missing");
  if (!nonempty(source.session_id) || !nonempty(source.client_build) || !nonempty(source.protocol_pack_digest) ||
      !/^sha256:[a-f0-9]{64}$/.test(source.canonical_content_sha256) || !positiveSafe(source.observation_count) ||
      !positiveSafe(source.last_event_sequence)) return invalid("profile observation evidence is invalid");
  if (hasProhibitedKey(body) || hasProhibitedKey(routing)) return invalid("profile contains a prohibited credential or account field");
  const character = body.character;
  if (!isObject(character) || !isObject(character.region) || String(character.character_id) !== routing["character-id"] ||
      character.region.deployment_id !== routing.deployment || character.region.region_id !== routing.region ||
      nullable(character.region.realm_id) !== nullable(routing.realm) || nullable(character.region.world_id) !== nullable(routing.world)) return invalid("profile body identity does not match routing identity");
  const digest = await sha256Hex(canonicalJson(request));
  if (value.package_id !== digest) return invalid("profile package digest does not match");
  const capture = source.live_capture;
  if (!isObject(capture) || capture.capture_kind !== LIVE_CAPTURE_KIND || capture.device_id !== deviceId ||
      !/^hmac-sha256:[a-f0-9]{64}$/.test(capture.proof)) return invalid("UID claim has no valid device-bound live-capture proof");
  const expectedProof = await liveCaptureProof(value, deviceId, deviceToken);
  if (!constantTimeEqual(capture.proof, expectedProof)) return invalid("UID claim live-capture proof does not match this device");
  return {};
}

async function liveCaptureProof(packageValue, deviceId, deviceToken) {
  const parts = [encoder.encode("rlogs-live-profile-capture-v1\0")];
  for (const value of [deviceId, packageValue.package_id, packageValue.source.session_id, packageValue.source.client_build,
    packageValue.source.protocol_pack_digest, packageValue.source.canonical_content_sha256]) {
    const bytes = encoder.encode(value);
    parts.push(littleEndian64(bytes.length), bytes);
  }
  parts.push(littleEndian64(packageValue.source.observation_count), littleEndian64(packageValue.source.last_event_sequence));
  const key = await crypto.subtle.importKey("raw", encoder.encode(deviceToken), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const proof = await crypto.subtle.sign("HMAC", key, concat(parts));
  return `hmac-sha256:${hex(new Uint8Array(proof))}`;
}

function mergeProfileBodies(existing, newer) {
  const target = structuredClone(existing);
  if (String(target.character?.character_id) !== String(newer.character?.character_id)) throw new Error("profile character mismatch");
  target.character = structuredClone(newer.character);
  for (const key of REPLACE_FIELDS) if (newer[key] != null) target[key] = structuredClone(newer[key]);
  target.progression = mergeOptionalObject(target.progression, newer.progression);
  target.season = mergeOptionalObject(target.season, newer.season);
  target.social_display = mergeSocial(target.social_display, newer.social_display);
  target.collection_summary = mergeCollection(target.collection_summary, newer.collection_summary);
  return target;
}

function mergeOptionalObject(existing, newer) {
  if (newer == null) return existing;
  if (existing == null) return structuredClone(newer);
  const result = structuredClone(existing);
  for (const [key, value] of Object.entries(newer)) if (value != null) result[key] = structuredClone(value);
  return result;
}

function mergeSocial(existing, newer) {
  if (newer == null) return existing;
  const result = mergeOptionalObject(existing, newer);
  for (const key of ["title_ids", "medal_ids"]) result[key] = union(existing?.[key], newer[key]);
  if (Array.isArray(newer.medal_slots)) result.medal_slots = [...(existing?.medal_slots ?? []), ...newer.medal_slots];
  return result;
}

function mergeCollection(existing, newer) {
  if (newer == null) return existing;
  if (existing == null) return structuredClone(newer);
  const result = structuredClone(existing);
  for (const key of ["fashion_points", "mount_points", "weapon_skin_points", "summoned_vanity_pet_id"]) {
    if (newer[key] != null) result[key] = structuredClone(newer[key]);
  }
  for (const key of ["owned_fashion_ids", "owned_mount_ids", "owned_weapon_skin_ids", "owned_dye_ids", "unlocked_module_ids",
    "ride_ids", "ride_skin_ids", "unlocked_emoji_ids", "vanity_pet_ids", "photo_ids"]) result[key] = union(existing[key], newer[key]);
  if (Array.isArray(newer.equipped_fashion_ids)) result.equipped_fashion_ids = [...(existing.equipped_fashion_ids ?? []), ...newer.equipped_fashion_ids];
  result.fantasy_atlas_stages = { ...(existing.fantasy_atlas_stages ?? {}), ...(newer.fantasy_atlas_stages ?? {}) };
  result.photo_wall = { ...(existing.photo_wall ?? {}), ...(newer.photo_wall ?? {}) };
  result.handbook = mergeHandbook(existing.handbook, newer.handbook);
  result.achievements = mergeAchievements(existing.achievements, newer.achievements);
  return result;
}

function mergeHandbook(existing, newer) {
  if (newer == null) return existing;
  const result = structuredClone(existing ?? {});
  for (const key of ["important_people_ids", "reading_book_ids", "dictionary_entry_ids", "postcard_ids", "monthly_card_ids"]) result[key] = union(existing?.[key], newer[key]);
  return result;
}

function mergeAchievements(existing, newer) {
  if (newer == null) return existing;
  if (existing == null) return structuredClone(newer);
  const result = structuredClone(existing);
  result.general = mergeAchievementList(existing.general, newer.general);
  const seasons = new Map((existing.seasons ?? []).map((season) => [season.season_id, structuredClone(season)]));
  for (const season of newer.seasons ?? []) {
    const prior = seasons.get(season.season_id);
    seasons.set(season.season_id, prior ? { ...prior, achievements: mergeAchievementList(prior.achievements, season.achievements) } : structuredClone(season));
  }
  result.seasons = [...seasons.values()].sort((a, b) => a.season_id - b.season_id);
  result.initialized_season_ids = union(existing.initialized_season_ids, newer.initialized_season_ids);
  if (newer.version != null) result.version = newer.version;
  return result;
}

function mergeAchievementList(existing = [], newer = []) {
  const values = new Map(existing.map((entry) => [entry.achievement_id, structuredClone(entry)]));
  for (const entry of newer) values.set(entry.achievement_id, mergeOptionalObject(values.get(entry.achievement_id), entry));
  return [...values.values()].sort((a, b) => a.achievement_id - b.achievement_id);
}

function replaceCurrentBuild(targetValue, current) {
  const target = structuredClone(targetValue);
  for (const key of CURRENT_BUILD_FIELDS) {
    if (Object.hasOwn(current, key)) target[key] = structuredClone(current[key]); else delete target[key];
  }
  if (Object.hasOwn(current, "current_profession_project_id")) target.current_profession_project_id = current.current_profession_project_id;
  return target;
}

function preservePhotoAssets(existing, newerValue) {
  const newer = structuredClone(newerValue);
  const assets = existing?.collection_summary?.photo_assets;
  if (!Array.isArray(assets) || !newer.collection_summary) return newer;
  const ids = new Set([...(newer.collection_summary.photo_ids ?? []), ...Object.values(newer.collection_summary.photo_wall ?? {})].map(Number));
  const retained = assets.filter((asset) => ids.has(Number(asset.photo_id)));
  if (retained.length) newer.collection_summary.photo_assets = structuredClone(retained);
  return newer;
}

function makeLoadout(profileId, projectId, body, packageValue, now) {
  const modules = body.modules;
  return {
    schema_version: 1, profile_id: profileId, project_id: projectId, updated_unix_millis: now,
    source_client_build: packageValue.source.client_build, class_id: body.class_id ?? null,
    specialization_id: body.specialization_id ?? null,
    module_inventory_count: Array.isArray(modules?.inventory) ? modules.inventory.length : 0,
    equipped_module_count: modules?.equipped_slots && typeof modules.equipped_slots === "object" ? Object.keys(modules.equipped_slots).length : 0,
    envelope: { ...structuredClone(packageValue.request.payload), body },
  };
}

function mergeLoadoutSummaries(existing = [], loadout, projects = [], packageValue, now) {
  const summaries = new Map(existing.map((entry) => [entry.project_id, structuredClone(entry)]));
  if (loadout) summaries.set(loadout.project_id, {
    project_id: loadout.project_id, project_name: null, profession_id: loadout.class_id,
    snapshot_available: true, updated_unix_millis: loadout.updated_unix_millis,
    source_client_build: loadout.source_client_build, class_id: loadout.class_id,
    specialization_id: loadout.specialization_id, module_inventory_count: loadout.module_inventory_count,
    equipped_module_count: loadout.equipped_module_count,
  });
  for (const project of projects ?? []) {
    const prior = summaries.get(project.project_id);
    summaries.set(project.project_id, prior ? { ...prior, project_name: project.project_name, profession_id: project.profession_id ?? prior.profession_id, class_id: prior.class_id ?? project.profession_id ?? null } : {
      project_id: project.project_id, project_name: project.project_name, profession_id: project.profession_id ?? null,
      snapshot_available: false, updated_unix_millis: now, source_client_build: packageValue.source.client_build,
      class_id: project.profession_id ?? null, specialization_id: null, module_inventory_count: 0, equipped_module_count: 0,
    });
  }
  return [...summaries.values()].sort((a, b) => a.project_id - b.project_id);
}

function catalogEntry(profile) {
  const { profile_id, claimed, package_id, updated_unix_millis, source_client_build, deployment, region, realm, world,
    character_id, display_name, module_inventory_count, equipped_module_count } = profile;
  return { profile_id, claimed, package_id, updated_unix_millis, source_client_build, deployment, region, realm, world,
    character_id, display_name, module_inventory_count, equipped_module_count };
}

async function newProfileId(characterId) {
  return `prf_${(await sha256Hex(`rlogs-profile-character-identity-v2\0${characterId}`)).slice(0, 32)}`;
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (isObject(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

async function sha256Hex(value) {
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(value))));
}

function littleEndian64(value) {
  const bytes = new Uint8Array(8); new DataView(bytes.buffer).setBigUint64(0, BigInt(value), true); return bytes;
}
function concat(parts) { const size = parts.reduce((sum, part) => sum + part.length, 0); const out = new Uint8Array(size); let offset = 0; for (const part of parts) { out.set(part, offset); offset += part.length; } return out; }
function hex(bytes) { return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(""); }
function constantTimeEqual(left, right) { if (left.length !== right.length) return false; let difference = 0; for (let index = 0; index < left.length; index += 1) difference |= left.charCodeAt(index) ^ right.charCodeAt(index); return difference === 0; }
function hasProhibitedKey(value) { if (Array.isArray(value)) return value.some(hasProhibitedKey); if (!isObject(value)) return false; return Object.entries(value).some(([key, child]) => PROHIBITED_KEYS.has(key.replace(/[^a-z0-9]/gi, "").toLocaleLowerCase()) || hasProhibitedKey(child)); }
function union(left = [], right = []) { return [...new Set([...(left ?? []), ...(right ?? [])])].sort((a, b) => typeof a === "number" && typeof b === "number" ? a - b : String(a).localeCompare(String(b))); }
function invalid(message) { return { error: `profile package is invalid: ${message}`, status: 400 }; }
function isObject(value) { return value != null && typeof value === "object" && !Array.isArray(value); }
function nonempty(value) { return typeof value === "string" && value.trim().length > 0 && value.length <= 256; }
function positiveSafe(value) { return Number.isSafeInteger(value) && value > 0; }
function positiveInteger(value) { return Number.isSafeInteger(value) && value > 0 ? value : null; }
function nullable(value) { return value == null ? null : value; }

export { canonicalJson, liveCaptureProof, mergeProfileBodies, validatePackage };
