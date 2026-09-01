export type PhotoWallPublicationState =
  | "waiting_for_live_parse"
  | "disabled"
  | "waiting_for_account_connection"
  | "waiting_for_photo_wall"
  | "observed_exact_game_image"
  | "publication_queued"
  | "published"
  | "retryable_failure";

export interface PhotoWallPublicationStatus {
  schemaVersion: 1;
  state: PhotoWallPublicationState;
  observedCount: number;
  queuedCount: number;
  publishedCount: number;
  retryableFailureCount: number;
  lastActivityUnixMillis: number | null;
  lastCharacterId: string | null;
  lastPhotoId: number | null;
  lastPictureType: number | null;
  lastVersion: number | null;
  lastError: string | null;
}

const STATES = new Set<PhotoWallPublicationState>([
  "waiting_for_live_parse",
  "disabled",
  "waiting_for_account_connection",
  "waiting_for_photo_wall",
  "observed_exact_game_image",
  "publication_queued",
  "published",
  "retryable_failure",
]);

export function parsePhotoWallPublicationStatus(
  value: unknown,
): PhotoWallPublicationStatus {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    typeof value.state !== "string" ||
    !STATES.has(value.state as PhotoWallPublicationState) ||
    !isSafeCount(value.observedCount) ||
    !isSafeCount(value.queuedCount) ||
    !isSafeCount(value.publishedCount) ||
    !isSafeCount(value.retryableFailureCount) ||
    !isNullableSafeCount(value.lastActivityUnixMillis) ||
    !isNullableNonEmptyString(value.lastCharacterId) ||
    !isNullableSafeCount(value.lastPhotoId) ||
    !isNullableSafeInteger(value.lastPictureType) ||
    !isNullableSafeCount(value.lastVersion) ||
    !isNullableString(value.lastError)
  ) {
    throw new Error(
      "The local host returned an invalid Photo Wall publication status.",
    );
  }
  return value as unknown as PhotoWallPublicationStatus;
}

export function photoWallPublicationSummary(
  status: PhotoWallPublicationStatus,
): string {
  switch (status.state) {
    case "disabled":
      return "Disabled";
    case "waiting_for_account_connection":
      return "Waiting for this PC to be connected to your rLogs account";
    case "waiting_for_photo_wall":
      return "Ready — keep the parser running and open your own Photo Wall in game";
    case "observed_exact_game_image":
      return "Exact in-game Photo Wall image observed";
    case "publication_queued":
      return "Exact in-game image queued for publication";
    case "published":
      return `Published (${status.publishedCount.toLocaleString()})`;
    case "retryable_failure":
      return status.lastError === null
        ? "Publication needs another attempt"
        : `Needs another attempt — ${status.lastError}`;
    case "waiting_for_live_parse":
      return "Ready — start a live parse and open your own Photo Wall in game";
  }
}

export function photoWallLastCaptureSummary(
  status: PhotoWallPublicationStatus,
): string {
  if (status.lastCharacterId === null || status.lastPhotoId === null) {
    return "No exact in-game Photo Wall image observed this app session";
  }
  const pictureType =
    status.lastPictureType === 2
      ? "full render"
      : status.lastPictureType === 3
        ? "wall thumbnail"
        : `picture type ${status.lastPictureType ?? "unknown"}`;
  const version =
    status.lastVersion === null ? "version unknown" : `version ${status.lastVersion}`;
  return `UID ${status.lastCharacterId} · Photo ${status.lastPhotoId} · ${pictureType} · ${version}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSafeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isNullableSafeCount(value: unknown): boolean {
  return value === null || isSafeCount(value);
}

function isNullableSafeInteger(value: unknown): boolean {
  return value === null || Number.isSafeInteger(value);
}

function isNullableString(value: unknown): boolean {
  return value === null || typeof value === "string";
}

function isNullableNonEmptyString(value: unknown): boolean {
  return value === null || (typeof value === "string" && value.length > 0);
}
