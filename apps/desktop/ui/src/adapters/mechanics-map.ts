export interface MechanicsMapEntity {
  actor_id: number;
  entity_uuid: number;
  kind: "local" | "party" | "boss" | "player" | "monster" | "pet" | "npc" | "object";
  display_name: string | null;
  monster_id: number | null;
  mechanic_role: "boss" | "tower" | "left_clone" | "right_clone" | null;
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
  mechanic_kind: string | null;
  presentation_name: string | null;
  instance_id: number | null;
  target_actor_id: number;
  source_actor_id: number | null;
  stacks: number | null;
  duration_millis: number | null;
  origin_x: number | null;
  origin_z: number | null;
  facing_radians: number | null;
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
  map_model: "player_relative_radar" | "absolute_scene_map";
  map_layout: "raid_ring" | "raid_grid" | null;
  world_radius: number;
  map_origin_x: number | null;
  map_origin_z: number | null;
  map_span_x: number | null;
  map_span_z: number | null;
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

export interface MechanicsMapViewPoint {
  mapX: number;
  mapY: number;
  visible: boolean;
}

export interface MechanicsMapTransform {
  scale: number;
  panX: number;
  panY: number;
}

export function zoomMechanicsMapAt(
  current: MechanicsMapTransform,
  cursorX: number,
  cursorY: number,
  wheelDeltaY: number,
): MechanicsMapTransform {
  const factor = Math.exp(-wheelDeltaY * 0.0015);
  const scale = current.scale * factor;
  if (!Number.isFinite(scale) || scale <= 0) return current;
  const ratio = scale / current.scale;
  return {
    scale,
    panX: cursorX - (cursorX - current.panX) * ratio,
    panY: cursorY - (cursorY - current.panY) * ratio,
  };
}

const CURSED_TOMB_ARENA = [
  { x: 37, z: -337 }, { x: 101, z: -337 }, { x: 101, z: -277 }, { x: 37, z: -277 },
] as const;

export function projectCursedTombChargeRegion(
  value: MechanicsMapSnapshot,
  signal: MechanicsMapSignal,
): MechanicsMapViewPoint[] {
  const left = signal.mechanic_kind === "clone_charge_left";
  const right = signal.mechanic_kind === "clone_charge_right";
  if ((!left && !right) || signal.origin_x === null || signal.origin_z === null || signal.facing_radians === null) return [];
  const forwardX = Math.sin(signal.facing_radians);
  const forwardZ = Math.cos(signal.facing_radians);
  const side = (point: { x: number; z: number }): number =>
    forwardX * (point.z - signal.origin_z!) - forwardZ * (point.x - signal.origin_x!);
  const inside = (point: { x: number; z: number }): boolean => left ? side(point) >= -0.0001 : side(point) <= 0.0001;
  const clipped: { x: number; z: number }[] = [];
  for (let index = 0; index < CURSED_TOMB_ARENA.length; index += 1) {
    const current = CURSED_TOMB_ARENA[index]!;
    const previous = CURSED_TOMB_ARENA[(index + CURSED_TOMB_ARENA.length - 1) % CURSED_TOMB_ARENA.length]!;
    const currentInside = inside(current);
    const previousInside = inside(previous);
    if (currentInside !== previousInside) {
      const previousSide = side(previous);
      const currentSide = side(current);
      const denominator = previousSide - currentSide;
      if (Math.abs(denominator) > 0.0001) {
        const ratio = previousSide / denominator;
        clipped.push({ x: previous.x + (current.x - previous.x) * ratio, z: previous.z + (current.z - previous.z) * ratio });
      }
    }
    if (currentInside) clipped.push(current);
  }
  return clipped.flatMap((point) => {
    const projected = projectMechanicsMapPoint(value, point.x, point.z, false);
    return projected === null ? [] : [projected];
  });
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
  return value.entities.flatMap((entity) => {
    const point = projectMechanicsMapPoint(value, entity.x, entity.z, rotateWithPlayer);
    return point === null ? [] : [{ ...entity, ...point }];
  });
}

export function projectMechanicsMapPoint(
  value: MechanicsMapSnapshot,
  x: number,
  z: number,
  rotateWithPlayer: boolean,
): MechanicsMapViewPoint | null {
  if (value.map_model === "absolute_scene_map") {
    const { map_origin_x: originX, map_origin_z: originZ, map_span_x: spanX, map_span_z: spanZ } = value;
    if (originX === null || originZ === null || spanX === null || spanZ === null) return null;
    const mapX = ((x - originX) / spanX) * 100;
    const mapY = (1 - (z - originZ) / spanZ) * 100;
    return { mapX, mapY, visible: mapX >= 0 && mapX <= 100 && mapY >= 0 && mapY <= 100 };
  }
  const local = value.entities.find((entity) => entity.actor_id === value.local_actor_id);
  if (local === undefined) return null;
  const rotation = rotateWithPlayer ? -(local.facing_radians ?? 0) : 0;
  const cosine = Math.cos(rotation);
  const sine = Math.sin(rotation);
  const dx = x - local.x;
  const dz = z - local.z;
  const relativeX = dx * cosine - dz * sine;
  const relativeZ = dx * sine + dz * cosine;
  const mapX = 50 + (relativeX / value.world_radius) * 50;
  const mapY = 50 - (relativeZ / value.world_radius) * 50;
  return { mapX, mapY, visible: Math.hypot(relativeX, relativeZ) <= value.world_radius };
}

function snapshot(value: unknown): value is MechanicsMapSnapshot {
  return record(value) && value.schema_version === 1 &&
    nonnegativeInteger(value.revision) && nullableString(value.session_id) && nullableString(value.client_build) &&
    nullableInteger(value.scene_id) && nullableInteger(value.map_id) && nullableString(value.scene_name) &&
    ["player_relative_radar", "absolute_scene_map"].includes(String(value.map_model)) && finitePositive(value.world_radius) &&
    (value.map_layout === null || ["raid_ring", "raid_grid"].includes(String(value.map_layout))) &&
    nullableFinite(value.map_origin_x) && nullableFinite(value.map_origin_z) &&
    nullablePositive(value.map_span_x) && nullablePositive(value.map_span_z) &&
    nullableString(value.background_asset_url) && nullableInteger(value.local_actor_id) &&
    typeof value.local_position_observed === "boolean" && nullableString(value.encounter_pack) &&
    typeof value.encounter_pack_reviewed === "boolean" && Array.isArray(value.entities) && value.entities.every(entity) &&
    Array.isArray(value.mechanics) && value.mechanics.every(signal) && Array.isArray(value.markers) &&
    nullableString(value.data_gap) && nullableInteger(value.last_event_sequence) && nullableInteger(value.last_observed_micros);
}

function entity(value: unknown): boolean {
  return record(value) && nonnegativeInteger(value.actor_id) && Number.isSafeInteger(value.entity_uuid) &&
    ["local", "party", "boss", "player", "monster", "pet", "npc", "object"].includes(String(value.kind)) &&
    nullableString(value.display_name) && nullableInteger(value.monster_id) &&
    (value.mechanic_role === null || ["boss", "tower", "left_clone", "right_clone"].includes(String(value.mechanic_role))) &&
    finite(value.x) && finite(value.y) && finite(value.z) &&
    (value.facing_radians === null || finite(value.facing_radians)) && typeof value.dead === "boolean" &&
    typeof value.stale === "boolean" && nonnegativeInteger(value.last_observed_micros);
}

function signal(value: unknown): boolean {
  return record(value) && Number.isSafeInteger(value.effect_id) && nullableString(value.mechanic_kind) && nullableString(value.presentation_name) && nullableInteger(value.instance_id) &&
    nonnegativeInteger(value.target_actor_id) && nullableInteger(value.source_actor_id) && nullableInteger(value.stacks) &&
    nullableInteger(value.duration_millis) && nullableFinite(value.origin_x) && nullableFinite(value.origin_z) &&
    nullableFinite(value.facing_radians) && nonnegativeInteger(value.applied_at_micros);
}

function record(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
function nullableString(value: unknown): boolean { return value === null || typeof value === "string"; }
function nullableInteger(value: unknown): boolean { return value === null || Number.isSafeInteger(value); }
function nonnegativeInteger(value: unknown): value is number { return Number.isSafeInteger(value) && (value as number) >= 0; }
function finite(value: unknown): value is number { return typeof value === "number" && Number.isFinite(value); }
function finitePositive(value: unknown): value is number { return finite(value) && value > 0; }
function nullableFinite(value: unknown): boolean { return value === null || finite(value); }
function nullablePositive(value: unknown): boolean { return value === null || finitePositive(value); }
