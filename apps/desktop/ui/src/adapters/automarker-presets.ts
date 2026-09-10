export interface AutomarkerPoint {
  markerNumber: number;
  x: number;
  y: number;
  z: number;
}

export interface AutomarkerPreset {
  presetId: string;
  name: string;
  clientBuild: string;
  sceneId: number;
  mapId: number;
  savedAtUnixMillis: number;
  points: readonly AutomarkerPoint[];
}

export interface AutomarkerSceneContext {
  clientBuild: string;
  sceneId: number;
  mapId: number;
  sceneName: string | null;
}

export interface AutomarkerPresetView {
  schemaVersion: 1;
  context: AutomarkerSceneContext | null;
  presets: readonly AutomarkerPreset[];
  captureSupported: boolean;
  captureReason: "native_waymark_state_unverified";
  nativeLoadSupported: boolean;
  nativeLoadReason: "native_waymark_request_unverified";
}

export interface SaveAutomarkerPresetRequest {
  presetId: string | null;
  name: string;
}

export interface AutomarkerLoadResult {
  supported: false;
  reason: "native_waymark_request_unverified";
}

export function parseAutomarkerPresetView(value: unknown): AutomarkerPresetView {
  if (!record(value) || value.schemaVersion !== 1 ||
      !(value.context === null || validContext(value.context)) ||
      !Array.isArray(value.presets) || !value.presets.every(validPreset) ||
      typeof value.captureSupported !== "boolean" ||
      value.captureReason !== "native_waymark_state_unverified" ||
      typeof value.nativeLoadSupported !== "boolean" ||
      value.nativeLoadReason !== "native_waymark_request_unverified") {
    throw new Error("The local host returned an invalid automarker preset catalog.");
  }
  const view = value as unknown as AutomarkerPresetView;
  if (view.context === null && view.presets.length !== 0) {
    throw new Error("Automarker presets cannot be listed without a current scene.");
  }
  if (view.context !== null && view.presets.some((preset) => !presetMatchesContext(preset, view.context!))) {
    throw new Error("The local host returned an automarker preset from another scene.");
  }
  return view;
}

export function parseAutomarkerLoadResult(value: unknown): AutomarkerLoadResult {
  if (!record(value) || value.supported !== false || value.reason !== "native_waymark_request_unverified") {
    throw new Error("The local host returned an invalid automarker load result.");
  }
  return value as unknown as AutomarkerLoadResult;
}

export function presetMatchesContext(preset: AutomarkerPreset, context: AutomarkerSceneContext): boolean {
  return preset.sceneId === context.sceneId && preset.mapId === context.mapId;
}

function validContext(value: unknown): value is AutomarkerSceneContext {
  return record(value) && validBuild(value.clientBuild) && integer(value.sceneId) && integer(value.mapId) &&
    (value.sceneName === null || typeof value.sceneName === "string");
}

function validPreset(value: unknown): value is AutomarkerPreset {
  return record(value) && typeof value.presetId === "string" && value.presetId.length >= 8 &&
    typeof value.name === "string" && value.name.trim().length >= 1 && value.name.length <= 80 &&
    validBuild(value.clientBuild) && integer(value.sceneId) && integer(value.mapId) &&
    integer(value.savedAtUnixMillis) && Array.isArray(value.points) && value.points.length >= 1 &&
    value.points.length <= 6 && value.points.every(validPoint) &&
    new Set(value.points.map((point) => (point as AutomarkerPoint).markerNumber)).size === value.points.length;
}

function validPoint(value: unknown): value is AutomarkerPoint {
  return record(value) && integer(value.markerNumber) && Number(value.markerNumber) >= 1 &&
    Number(value.markerNumber) <= 6 && finite(value.x) && finite(value.y) && finite(value.z);
}

function validBuild(value: unknown): value is string {
  return typeof value === "string" && value.length >= 1 && value.length <= 32;
}

function integer(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 1_000_000;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
