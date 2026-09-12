export interface AutomarkerPoint {
  markerNumber: number;
  x: number;
  y: number;
  z: number;
}

export interface AutomarkerPreset {
  presetId: string;
  name: string;
  activityFamilyId: string;
  savedAtUnixMillis: number;
  points: readonly AutomarkerPoint[];
}

export interface AutomarkerPresetExchange {
  kind: "rlogs-automarker-preset";
  version: 1;
  name: string;
  activityFamilyId: string;
  points: readonly AutomarkerPoint[];
}

export const AUTOMARKER_EXCHANGE_MAX_BYTES = 64 * 1024;

export interface AutomarkerSceneContext {
  clientBuild: string;
  sceneId: number;
  mapId: number;
  activityFamilyId: string;
  sceneName: string | null;
}

export interface AutomarkerPresetView {
  schemaVersion: 4;
  context: AutomarkerSceneContext | null;
  presets: readonly AutomarkerPreset[];
  captureSupported: boolean;
  captureReason: "native_waymark_state_unverified" | "observed_waymark_state_verified";
  captureSessionId: string | null;
  deploymentId: string | null;
  protocolPackDigest: string | null;
  nativeLoadSupported: boolean;
  nativeLoadReason: "native_waymark_transport_unavailable";
  previewSessionId: string;
}

export interface SaveAutomarkerPresetRequest {
  presetId: string | null;
  name: string;
  points: readonly AutomarkerPoint[];
  expectedContext: AutomarkerSceneContext;
}

export interface LoadAutomarkerPresetRequest {
  presetId: string;
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

export interface AutomarkerLocalLoadResult {
  context: AutomarkerSceneContext;
  preset: AutomarkerPreset;
}

export interface ObservedMarkerSnapshot {
  schemaVersion: 2;
  revision: number;
  captureActive: boolean;
  protocolSupported: boolean;
  requestObserverSupported: boolean;
  verifiedRequestCount: number;
  lastVerifiedRequestMarkerNumber: number | null;
  lastVerifiedRequestObservedMicros: number | null;
  reason: "live_capture_not_running" | "marker_protocol_not_verified_for_build_pack" |
    "waiting_for_packet_observed_scene_and_map" | "no_fully_positioned_markers_observed" |
    "observed_marker_snapshot_invalid" | "observed_markers_available";
  sessionId: string | null;
  deploymentId: string | null;
  clientBuild: string | null;
  protocolPackDigest: string | null;
  sceneId: number | null;
  mapId: number | null;
  observedMicros: number | null;
  markers: readonly AutomarkerPoint[];
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

export function automarkerPresetExchange(preset: AutomarkerPreset): AutomarkerPresetExchange {
  return {
    kind: "rlogs-automarker-preset",
    version: 1,
    name: preset.name,
    activityFamilyId: preset.activityFamilyId,
    points: preset.points.map((point) => ({ ...point })),
  };
}

export function serializeAutomarkerPresetExchange(preset: AutomarkerPreset): string {
  return `${JSON.stringify(automarkerPresetExchange(preset), null, 2)}\n`;
}

export function parseAutomarkerPresetExchange(
  text: string,
  expectedActivityFamilyId: string,
): AutomarkerPresetExchange {
  if (new TextEncoder().encode(text).byteLength > AUTOMARKER_EXCHANGE_MAX_BYTES) {
    throw new Error("The automarker preset file exceeds the 64 KiB safety limit.");
  }
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("The automarker preset file is not valid JSON.");
  }
  if (!record(value) || !exactKeys(value, ["kind", "version", "name", "activityFamilyId", "points"]) ||
      value.kind !== "rlogs-automarker-preset" || value.version !== 1 ||
      typeof value.name !== "string" || value.name.trim().length < 1 || value.name.length > 80 ||
      !validFamily(value.activityFamilyId) || !Array.isArray(value.points)) {
    throw new Error("The automarker preset file has an invalid or unsupported format.");
  }
  validateManualPoints(value.points as AutomarkerPoint[]);
  if (value.activityFamilyId !== expectedActivityFamilyId) {
    throw new Error("The imported automarker preset belongs to another dungeon family.");
  }
  return {
    kind: "rlogs-automarker-preset",
    version: 1,
    name: value.name.trim(),
    activityFamilyId: value.activityFamilyId,
    points: value.points.map((point) => ({ ...(point as AutomarkerPoint) })),
  };
}

export function automarkerExportFilename(name: string): string {
  const stem = name.normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^A-Za-z0-9._-]+/g, "-")
    .replace(/^[. _-]+|[. _-]+$/g, "")
    .slice(0, 64)
    .replace(/[. _-]+$/g, "");
  const safeStem = stem.length > 0 && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(stem)
    ? stem
    : "automarker-preset";
  return `${safeStem}.rlogs-automarker.json`;
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
  if (!record(value) || value.schemaVersion !== 4 ||
      !(value.context === null || validContext(value.context)) ||
      !Array.isArray(value.presets) || !value.presets.every(validPreset) ||
      typeof value.captureSupported !== "boolean" ||
      !(value.captureReason === "native_waymark_state_unverified" || value.captureReason === "observed_waymark_state_verified") ||
      !optionalIdentity(value.captureSessionId, 128) ||
      !optionalIdentity(value.deploymentId, 64) ||
      !optionalDigest(value.protocolPackDigest) ||
      typeof value.nativeLoadSupported !== "boolean" ||
      value.nativeLoadReason !== "native_waymark_transport_unavailable" ||
      typeof value.previewSessionId !== "string" || value.previewSessionId.length < 8 || value.previewSessionId.length > 128) {
    throw new Error("The local host returned an invalid automarker preset catalog.");
  }
  const view = value as unknown as AutomarkerPresetView;
  const hasCaptureStamp = view.captureSessionId !== null || view.deploymentId !== null || view.protocolPackDigest !== null;
  if (hasCaptureStamp && (view.captureSessionId === null || view.deploymentId === null || view.protocolPackDigest === null)) {
    throw new Error("The local host returned a partial automarker capture identity.");
  }
  if (view.captureSupported !== (view.captureReason === "observed_waymark_state_verified") ||
      (view.captureSupported && !hasCaptureStamp)) {
    throw new Error("The local host returned an inconsistent automarker capture capability.");
  }
  if (view.context === null && view.presets.length !== 0) {
    throw new Error("Automarker presets cannot be listed without a current scene.");
  }
  if (view.context !== null && view.presets.some((preset) => !presetMatchesContext(preset, view.context!))) {
    throw new Error("The local host returned an automarker preset from another dungeon family.");
  }
  return view;
}

export function parseObservedMarkerSnapshot(value: unknown): ObservedMarkerSnapshot {
  if (!record(value) || value.schemaVersion !== 2 || !integer(value.revision) ||
      typeof value.captureActive !== "boolean" || typeof value.protocolSupported !== "boolean" ||
      typeof value.requestObserverSupported !== "boolean" || !integer(value.verifiedRequestCount) ||
      !optionalMarkerNumber(value.lastVerifiedRequestMarkerNumber) ||
      !optionalInteger(value.lastVerifiedRequestObservedMicros) ||
      !observedReason(value.reason) || !optionalIdentity(value.sessionId, 128) ||
      !optionalIdentity(value.deploymentId, 64) || !optionalBuild(value.clientBuild) ||
      !optionalDigest(value.protocolPackDigest) || !optionalInteger(value.sceneId) ||
      !optionalInteger(value.mapId) || !optionalInteger(value.observedMicros) ||
      !Array.isArray(value.markers) || !value.markers.every(validPoint) ||
      new Set(value.markers.map((point) => (point as AutomarkerPoint).markerNumber)).size !== value.markers.length) {
    throw new Error("The local host returned an invalid observed-marker snapshot.");
  }
  const snapshot = value as unknown as ObservedMarkerSnapshot;
  const fullStamp = snapshot.sessionId !== null && snapshot.deploymentId !== null &&
    snapshot.clientBuild !== null && snapshot.protocolPackDigest !== null;
  const hasContext = snapshot.sceneId !== null && snapshot.mapId !== null;
  const hasRequestDiagnostic = snapshot.lastVerifiedRequestMarkerNumber !== null &&
    snapshot.lastVerifiedRequestObservedMicros !== null;
  const validRequestDiagnostic = snapshot.captureActive
    ? snapshot.requestObserverSupported
      ? (snapshot.verifiedRequestCount === 0 ? !hasRequestDiagnostic : hasRequestDiagnostic)
      : snapshot.verifiedRequestCount === 0 && !hasRequestDiagnostic
    : !snapshot.requestObserverSupported && snapshot.verifiedRequestCount === 0 && !hasRequestDiagnostic;
  const validState = snapshot.reason === "live_capture_not_running"
    ? !snapshot.captureActive && !snapshot.protocolSupported && !hasContext && snapshot.markers.length === 0
    : snapshot.reason === "marker_protocol_not_verified_for_build_pack"
      ? snapshot.captureActive && !snapshot.protocolSupported && fullStamp && !hasContext && snapshot.markers.length === 0
      : snapshot.reason === "waiting_for_packet_observed_scene_and_map"
        ? snapshot.captureActive && snapshot.protocolSupported && fullStamp && !hasContext && snapshot.markers.length === 0
        : snapshot.reason === "observed_markers_available"
          ? snapshot.captureActive && snapshot.protocolSupported && fullStamp && hasContext &&
            snapshot.observedMicros !== null && snapshot.markers.length > 0
          : snapshot.captureActive && snapshot.protocolSupported && fullStamp && hasContext && snapshot.markers.length === 0;
  if (!validState || !validRequestDiagnostic ||
      (snapshot.lastVerifiedRequestMarkerNumber === null) !== (snapshot.lastVerifiedRequestObservedMicros === null) ||
      (snapshot.sceneId === null) !== (snapshot.mapId === null) ||
      (!snapshot.captureActive && snapshot.observedMicros !== null)) {
    throw new Error("The local host returned an inconsistent observed-marker snapshot.");
  }
  return snapshot;
}

export function observedMarkersMatchPresetView(
  snapshot: ObservedMarkerSnapshot,
  view: AutomarkerPresetView,
): boolean {
  const context = view.context;
  return context !== null && snapshot.captureActive && snapshot.protocolSupported &&
    snapshot.reason === "observed_markers_available" && snapshot.markers.length > 0 &&
    snapshot.sessionId === view.captureSessionId && snapshot.deploymentId === view.deploymentId &&
    snapshot.clientBuild === context.clientBuild && snapshot.protocolPackDigest === view.protocolPackDigest &&
    snapshot.sceneId === context.sceneId && snapshot.mapId === context.mapId;
}

export function parseAutomarkerLocalLoadResult(value: unknown): AutomarkerLocalLoadResult {
  if (!record(value) || !validContext(value.context) || !validPreset(value.preset) ||
      !presetMatchesContext(value.preset, value.context)) {
    throw new Error("The local host returned an invalid local automarker preset.");
  }
  return value as unknown as AutomarkerLocalLoadResult;
}

export function presetMatchesContext(preset: AutomarkerPreset, context: AutomarkerSceneContext): boolean {
  return preset.activityFamilyId === context.activityFamilyId;
}

export function automarkerResponseIsCurrent(
  requestGeneration: number,
  currentGeneration: number,
  requestedSnapshotKey?: string,
  currentSnapshotKey?: string,
): boolean {
  return requestGeneration === currentGeneration &&
    (requestedSnapshotKey === undefined || requestedSnapshotKey === currentSnapshotKey);
}

function validContext(value: unknown): value is AutomarkerSceneContext {
  return record(value) && validBuild(value.clientBuild) && integer(value.sceneId) && integer(value.mapId) &&
    validFamily(value.activityFamilyId) && (value.sceneName === null || typeof value.sceneName === "string");
}

function validPreset(value: unknown): value is AutomarkerPreset {
  return record(value) && exactKeys(value, [
    "presetId", "name", "activityFamilyId", "savedAtUnixMillis", "points",
  ]) && typeof value.presetId === "string" && value.presetId.length >= 8 &&
    typeof value.name === "string" && value.name.trim().length >= 1 && value.name.length <= 80 &&
    validFamily(value.activityFamilyId) &&
    integer(value.savedAtUnixMillis) && Array.isArray(value.points) && value.points.length >= 1 &&
    value.points.length <= 6 && value.points.every(validPoint) &&
    new Set(value.points.map((point) => (point as AutomarkerPoint).markerNumber)).size === value.points.length;
}

function validPoint(value: unknown): value is AutomarkerPoint {
  return record(value) && exactKeys(value, ["markerNumber", "x", "y", "z"]) &&
    integer(value.markerNumber) && Number(value.markerNumber) >= 1 &&
    Number(value.markerNumber) <= 6 && finite(value.x) && finite(value.y) && finite(value.z);
}

function validBuild(value: unknown): value is string {
  return typeof value === "string" && value.length >= 1 && value.length <= 32;
}

function optionalBuild(value: unknown): value is string | null {
  return value === null || validBuild(value);
}

function optionalIdentity(value: unknown, max: number): value is string | null {
  return value === null || (typeof value === "string" && value.length >= 1 && value.length <= max);
}

function optionalDigest(value: unknown): value is string | null {
  return value === null || (typeof value === "string" && /^sha256:[0-9a-f]{64}$/.test(value));
}

function optionalInteger(value: unknown): value is number | null {
  return value === null || integer(value);
}

function optionalMarkerNumber(value: unknown): value is number | null {
  return value === null || (integer(value) && value >= 1 && value <= 6);
}

function observedReason(value: unknown): value is ObservedMarkerSnapshot["reason"] {
  return value === "live_capture_not_running" || value === "marker_protocol_not_verified_for_build_pack" ||
    value === "waiting_for_packet_observed_scene_and_map" || value === "no_fully_positioned_markers_observed" ||
    value === "observed_marker_snapshot_invalid" || value === "observed_markers_available";
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

function exactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value);
  return keys.length === expected.length && expected.every((key) => Object.hasOwn(value, key));
}
