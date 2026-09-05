export interface MechanicsMapEntity {
  actor_id: number;
  entity_uuid: number;
  kind: "local" | "party" | "boss" | "player" | "monster" | "pet" | "npc" | "object";
  display_name: string | null;
  monster_id: number | null;
  x: number;
  y: number;
  z: number;
  facing_radians: number | null;
  dead: boolean;
  stale: boolean;
  last_observed_micros: number;
}

export interface MechanicsMapSignal {
  effect_id: number;
  presentation_name: string | null;
  instance_id: number | null;
  target_actor_id: number;
  source_actor_id: number | null;
  stacks: number | null;
  duration_millis: number | null;
  applied_at_micros: number;
}

export interface MechanicsMapSnapshot {
  schema_version: 1;
  revision: number;
  session_id: string | null;
  client_build: string | null;
  scene_id: number | null;
  map_id: number | null;
  scene_name: string | null;
  map_model: "player_relative_radar";
  world_radius: number;
  background_asset_url: string | null;
  local_actor_id: number | null;
  local_position_observed: boolean;
  encounter_pack: string | null;
  encounter_pack_reviewed: boolean;
  entities: readonly MechanicsMapEntity[];
  mechanics: readonly MechanicsMapSignal[];
  markers: readonly {
    marker_id: number | null;
    related_actor_id: number | null;
    x: number | null;
    y: number | null;
    z: number | null;
  }[];
  data_gap: string | null;
  last_event_sequence: number | null;
  last_observed_micros: number | null;
}

export interface MechanicsMapUpdate {
  schema_version: 1;
  revision: number;
  snapshot: MechanicsMapSnapshot;
}

export interface MechanicsMapViewEntity extends MechanicsMapEntity {
  mapX: number;
  mapY: number;
  visible: boolean;
}

export function parseMechanicsMapUpdate(value: unknown): MechanicsMapUpdate {
  if (!record(value) || value.schema_version !== 1 || !nonnegativeInteger(value.revision) || !snapshot(value.snapshot)) {
    throw new Error("The local host returned an invalid Mechanics Map update.");
  }
  return value as unknown as MechanicsMapUpdate;
}

export function projectMechanicsMapEntities(
  value: MechanicsMapSnapshot,
  rotateWithPlayer: boolean,
): MechanicsMapViewEntity[] {
  const local = value.entities.find((entity) => entity.actor_id === value.local_actor_id);
  if (local === undefined) return [];
  const rotation = rotateWithPlayer ? -(local.facing_radians ?? 0) : 0;
  const cosine = Math.cos(rotation);
  const sine = Math.sin(rotation);
  return value.entities.map((entity) => {
    const dx = entity.x - local.x;
    const dz = entity.z - local.z;
    const x = dx * cosine - dz * sine;
    const z = dx * sine + dz * cosine;
    const mapX = 50 + (x / value.world_radius) * 50;
    const mapY = 50 - (z / value.world_radius) * 50;
    return { ...entity, mapX, mapY, visible: Math.hypot(x, z) <= value.world_radius };
  });
}

function snapshot(value: unknown): value is MechanicsMapSnapshot {
  return record(value) && value.schema_version === 1 &&
    nonnegativeInteger(value.revision) && nullableString(value.session_id) && nullableString(value.client_build) &&
    nullableInteger(value.scene_id) && nullableInteger(value.map_id) && nullableString(value.scene_name) &&
    value.map_model === "player_relative_radar" && finitePositive(value.world_radius) &&
    nullableString(value.background_asset_url) && nullableInteger(value.local_actor_id) &&
    typeof value.local_position_observed === "boolean" && nullableString(value.encounter_pack) &&
    typeof value.encounter_pack_reviewed === "boolean" && Array.isArray(value.entities) && value.entities.every(entity) &&
    Array.isArray(value.mechanics) && value.mechanics.every(signal) && Array.isArray(value.markers) &&
    nullableString(value.data_gap) && nullableInteger(value.last_event_sequence) && nullableInteger(value.last_observed_micros);
}

function entity(value: unknown): boolean {
  return record(value) && nonnegativeInteger(value.actor_id) && Number.isSafeInteger(value.entity_uuid) &&
    ["local", "party", "boss", "player", "monster", "pet", "npc", "object"].includes(String(value.kind)) &&
    nullableString(value.display_name) && nullableInteger(value.monster_id) && finite(value.x) && finite(value.y) && finite(value.z) &&
    (value.facing_radians === null || finite(value.facing_radians)) && typeof value.dead === "boolean" &&
    typeof value.stale === "boolean" && nonnegativeInteger(value.last_observed_micros);
}

function signal(value: unknown): boolean {
  return record(value) && Number.isSafeInteger(value.effect_id) && nullableString(value.presentation_name) && nullableInteger(value.instance_id) &&
    nonnegativeInteger(value.target_actor_id) && nullableInteger(value.source_actor_id) && nullableInteger(value.stacks) &&
    nullableInteger(value.duration_millis) && nonnegativeInteger(value.applied_at_micros);
}

function record(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
function nullableString(value: unknown): boolean { return value === null || typeof value === "string"; }
function nullableInteger(value: unknown): boolean { return value === null || Number.isSafeInteger(value); }
function nonnegativeInteger(value: unknown): value is number { return Number.isSafeInteger(value) && (value as number) >= 0; }
function finite(value: unknown): value is number { return typeof value === "number" && Number.isFinite(value); }
function finitePositive(value: unknown): value is number { return finite(value) && value > 0; }
