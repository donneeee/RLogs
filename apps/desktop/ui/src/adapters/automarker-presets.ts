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
  activityFamilyId: string;
  savedAtUnixMillis: number;
  points: readonly AutomarkerPoint[];
}

export interface AutomarkerSceneContext {
  clientBuild: string;
  sceneId: number;
  mapId: number;
  activityFamilyId: string;
  sceneName: string | null;
}

export interface AutomarkerPresetView {
  schemaVersion: 2;
  context: AutomarkerSceneContext | null;
  presets: readonly AutomarkerPreset[];
  captureSupported: boolean;
  captureReason: "native_waymark_state_unverified";
  nativeLoadSupported: boolean;
  nativeLoadReason: "native_waymark_request_unverified";
  previewSessionId: string;
}

export interface SaveAutomarkerPresetRequest {
  presetId: string | null;
  name: string;
  points: readonly AutomarkerPoint[];
  expectedContext: AutomarkerSceneContext;
}

export interface AutomarkerPreview {
  schemaVersion: 1;
  context: AutomarkerSceneContext;
  name: string;
  points: readonly AutomarkerPoint[];
  previewSessionId: string;
  issuedAtUnixMillis: number;
  expiresAtUnixMillis: number;
}

export const AUTOMARKER_PREVIEW_STORAGE_KEY = "rlogs.automarker-preview.v1";
export const AUTOMARKER_PREVIEW_TTL_MILLIS = 5 * 60 * 1_000;

export interface AutomarkerLoadResult {
  supported: false;
  reason: "native_waymark_request_unverified";
}

export function automarkerSaveRequest(
  mode: "save" | "save-as",
  selectedPresetId: string | null,
  name: string,
  points: readonly AutomarkerPoint[],
  expectedContext: AutomarkerSceneContext,
): SaveAutomarkerPresetRequest {
  const cleanName = name.trim();
  if (cleanName.length < 1 || cleanName.length > 80) {
    throw new Error("Preset names must contain 1–80 characters.");
  }
  if (mode === "save" && selectedPresetId === null) {
    throw new Error("Choose an existing setup to overwrite, or use Save As… to create one.");
  }
  validateManualPoints(points);
  return {
    presetId: mode === "save" ? selectedPresetId : null,
    name: cleanName,
    points: points.map((point) => ({ ...point })),
    expectedContext: { ...expectedContext },
  };
}

export function validateManualPoints(points: readonly AutomarkerPoint[]): void {
  if (points.length < 1 || points.length > 6) {
    throw new Error("A preset must contain 1–6 numbered markers.");
  }
  if (!points.every(validPoint)) throw new Error("Marker coordinates must be finite values between -1,000,000 and 1,000,000.");
  if (new Set(points.map((point) => point.markerNumber)).size !== points.length) {
    throw new Error("Marker numbers must be unique.");
  }
}

export function publishAutomarkerPreview(
  storage: Pick<Storage, "setItem">,
  context: AutomarkerSceneContext,
  name: string,
  points: readonly AutomarkerPoint[],
  previewSessionId: string,
  nowUnixMillis = Date.now(),
): AutomarkerPreview {
  validateManualPoints(points);
  const preview: AutomarkerPreview = {
    schemaVersion: 1,
    context: { ...context },
    name: name.trim() || "Manual preview",
    points: points.map((point) => ({ ...point })),
    previewSessionId,
    issuedAtUnixMillis: nowUnixMillis,
    expiresAtUnixMillis: nowUnixMillis + AUTOMARKER_PREVIEW_TTL_MILLIS,
  };
  storage.setItem(AUTOMARKER_PREVIEW_STORAGE_KEY, JSON.stringify(preview));
  return preview;
}

export function parseAutomarkerPreview(value: unknown): AutomarkerPreview {
  if (!record(value) || value.schemaVersion !== 1 || !validContext(value.context) ||
      typeof value.name !== "string" || value.name.trim().length < 1 || value.name.length > 80 ||
      typeof value.previewSessionId !== "string" || value.previewSessionId.length < 8 || value.previewSessionId.length > 128 ||
      !integer(value.issuedAtUnixMillis) || !integer(value.expiresAtUnixMillis) ||
      value.expiresAtUnixMillis <= value.issuedAtUnixMillis ||
      value.expiresAtUnixMillis - value.issuedAtUnixMillis > AUTOMARKER_PREVIEW_TTL_MILLIS ||
      !Array.isArray(value.points)) {
    throw new Error("The local automarker preview is invalid.");
  }
  validateManualPoints(value.points as AutomarkerPoint[]);
  return value as unknown as AutomarkerPreview;
}

export function previewMatchesContext(
  preview: AutomarkerPreview,
  context: Pick<AutomarkerSceneContext, "clientBuild" | "sceneId" | "mapId" | "activityFamilyId">,
): boolean {
  return preview.context.clientBuild === context.clientBuild && preview.context.sceneId === context.sceneId &&
    preview.context.mapId === context.mapId && preview.context.activityFamilyId === context.activityFamilyId;
}

export function activeAutomarkerPreview(
  preview: AutomarkerPreview,
  previewSessionId: string,
  context: Pick<AutomarkerSceneContext, "clientBuild" | "sceneId" | "mapId" | "activityFamilyId">,
  nowUnixMillis = Date.now(),
): boolean {
  return preview.previewSessionId === previewSessionId && preview.issuedAtUnixMillis <= nowUnixMillis &&
    preview.expiresAtUnixMillis > nowUnixMillis && previewMatchesContext(preview, context);
}

export function readActiveAutomarkerPreview(
  storage: Pick<Storage, "getItem" | "removeItem">,
  previewSessionId: string,
  context: Pick<AutomarkerSceneContext, "clientBuild" | "sceneId" | "mapId" | "activityFamilyId">,
  nowUnixMillis = Date.now(),
): AutomarkerPreview | null {
  const raw = storage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
  if (raw === null) return null;
  try {
    const preview = parseAutomarkerPreview(JSON.parse(raw));
    if (activeAutomarkerPreview(preview, previewSessionId, context, nowUnixMillis)) return preview;
  } catch {
    // Invalid preview payloads are local UI state and are safe to discard.
  }
  storage.removeItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
  return null;
}

export function newlyCreatedPresetId(
  previousPresetIds: ReadonlySet<string>,
  next: AutomarkerPresetView,
): string | null {
  return next.presets.find((preset) => !previousPresetIds.has(preset.presetId))?.presetId ?? null;
}

export function parseAutomarkerPresetView(value: unknown): AutomarkerPresetView {
  if (!record(value) || value.schemaVersion !== 2 ||
      !(value.context === null || validContext(value.context)) ||
      !Array.isArray(value.presets) || !value.presets.every(validPreset) ||
      typeof value.captureSupported !== "boolean" ||
      value.captureReason !== "native_waymark_state_unverified" ||
      typeof value.nativeLoadSupported !== "boolean" ||
      value.nativeLoadReason !== "native_waymark_request_unverified" ||
      typeof value.previewSessionId !== "string" || value.previewSessionId.length < 8 || value.previewSessionId.length > 128) {
    throw new Error("The local host returned an invalid automarker preset catalog.");
  }
  const view = value as unknown as AutomarkerPresetView;
  if (view.context === null && view.presets.length !== 0) {
    throw new Error("Automarker presets cannot be listed without a current scene.");
  }
  if (view.context !== null && view.presets.some((preset) => !presetMatchesContext(preset, view.context!))) {
    throw new Error("The local host returned an automarker preset from another dungeon family.");
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
  return preset.activityFamilyId === context.activityFamilyId;
}

function validContext(value: unknown): value is AutomarkerSceneContext {
  return record(value) && validBuild(value.clientBuild) && integer(value.sceneId) && integer(value.mapId) &&
    validFamily(value.activityFamilyId) && (value.sceneName === null || typeof value.sceneName === "string");
}

function validPreset(value: unknown): value is AutomarkerPreset {
  return record(value) && typeof value.presetId === "string" && value.presetId.length >= 8 &&
    typeof value.name === "string" && value.name.trim().length >= 1 && value.name.length <= 80 &&
    validBuild(value.clientBuild) && integer(value.sceneId) && integer(value.mapId) &&
    validFamily(value.activityFamilyId) &&
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

function validFamily(value: unknown): value is string {
  return typeof value === "string" && value.trim().length >= 1 && value.length <= 128;
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
