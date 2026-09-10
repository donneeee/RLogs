import type { MechanicsMapSnapshot } from "./mechanics-map";

export const WAYMARK_PRESET_STORAGE_KEY = "rlogs.waymark-presets.v1";
export const WAYMARK_PRESET_SCHEMA_VERSION = 1 as const;
const MAX_PRESETS = 32;
const MAX_ENCODED_BYTES = 64 * 1024;

export interface WaymarkPresetPoint {
  markerNumber: number;
  x: number;
  y: number | null;
  z: number;
}

export interface WaymarkPreset {
  schemaVersion: typeof WAYMARK_PRESET_SCHEMA_VERSION;
  name: string;
  clientBuild: string;
  sceneId: number;
  mapId: number;
  savedAt: string;
  points: readonly WaymarkPresetPoint[];
}

export type NativeWaymarkLoadResult = {
  supported: false;
  reason: "native_waymark_request_unverified";
};

/** Captures only numbered, packet-observed ground markers; actor assignments are not presets. */
export function captureWaymarkPreset(
  snapshot: MechanicsMapSnapshot,
  name: string,
  savedAt = new Date().toISOString(),
): WaymarkPreset {
  if (snapshot.client_build === null || snapshot.scene_id === null || snapshot.map_id === null) {
    throw new Error("A build, scene, and map must be observed before saving waymarks.");
  }
  const points = snapshot.markers
    .filter((marker) => marker.marker_number !== null && marker.x !== null && marker.z !== null)
    .map((marker) => ({
      markerNumber: marker.marker_number!, x: marker.x!, y: marker.y, z: marker.z!,
    }))
    .sort((left, right) => left.markerNumber - right.markerNumber);
  if (points.length === 0) throw new Error("No packet-observed numbered waymarks are available to save.");
  if (!validPoints(points)) throw new Error("Observed waymarks are invalid or contain duplicate numbers.");
  const cleanName = name.trim();
  if (cleanName.length < 1 || cleanName.length > 80) throw new Error("Preset names must be 1–80 characters.");
  return {
    schemaVersion: WAYMARK_PRESET_SCHEMA_VERSION,
    name: cleanName,
    clientBuild: snapshot.client_build,
    sceneId: snapshot.scene_id,
    mapId: snapshot.map_id,
    savedAt,
    points,
  };
}

export function storeWaymarkPreset(
  storage: Pick<Storage, "getItem" | "setItem">,
  preset: WaymarkPreset,
): void {
  if (!validPreset(preset)) throw new Error("Invalid waymark preset.");
  const retained = readWaymarkPresets(storage)
    .filter((candidate) => !(candidate.name === preset.name && candidate.sceneId === preset.sceneId))
    .slice(0, MAX_PRESETS - 1);
  const encoded = JSON.stringify([preset, ...retained]);
  if (new TextEncoder().encode(encoded).byteLength > MAX_ENCODED_BYTES) {
    throw new Error("Waymark preset storage limit exceeded.");
  }
  storage.setItem(WAYMARK_PRESET_STORAGE_KEY, encoded);
}

export function readWaymarkPresets(storage: Pick<Storage, "getItem">): WaymarkPreset[] {
  const encoded = storage.getItem(WAYMARK_PRESET_STORAGE_KEY);
  if (encoded === null || encoded.length > MAX_ENCODED_BYTES) return [];
  try {
    const value: unknown = JSON.parse(encoded);
    return Array.isArray(value) ? value.filter(validPreset).slice(0, MAX_PRESETS) : [];
  } catch {
    return [];
  }
}

export function requestNativeWaymarkLoad(_preset: WaymarkPreset): NativeWaymarkLoadResult {
  return { supported: false, reason: "native_waymark_request_unverified" };
}

function validPreset(value: unknown): value is WaymarkPreset {
  if (!record(value) || value.schemaVersion !== WAYMARK_PRESET_SCHEMA_VERSION ||
      typeof value.name !== "string" || value.name.length < 1 || value.name.length > 80 ||
      typeof value.clientBuild !== "string" || value.clientBuild.length < 1 || value.clientBuild.length > 32 ||
      !Number.isSafeInteger(value.sceneId) || (value.sceneId as number) < 0 ||
      !Number.isSafeInteger(value.mapId) || (value.mapId as number) < 0 ||
      typeof value.savedAt !== "string" || !Array.isArray(value.points)) return false;
  return validPoints(value.points);
}

function validPoints(value: readonly unknown[]): value is WaymarkPresetPoint[] {
  if (value.length < 1 || value.length > 6) return false;
  const numbers = new Set<number>();
  for (const point of value) {
    if (!record(point) || !Number.isSafeInteger(point.markerNumber) ||
        (point.markerNumber as number) < 1 || (point.markerNumber as number) > 6 ||
        !finite(point.x) || !finite(point.z) || (point.y !== null && !finite(point.y))) return false;
    numbers.add(point.markerNumber as number);
  }
  return numbers.size === value.length;
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 1_000_000;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
