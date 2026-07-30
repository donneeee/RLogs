export type SubmissionState =
  | "draft"
  | "uploading"
  | "finalizing"
  | "submitted";

export type ReportVisibility = "private" | "unlisted" | "public";

export interface QueuedSubmissionView {
  queue_id: string;
  created_unix_millis: number;
  capture_session_id: string;
  local_artifact_path: string;
  artifact_exists: boolean;
  artifact_byte_length_matches: boolean;
  file_byte_length: number;
  canonical_content_sha256: string;
  chunk_count: number;
  state: SubmissionState;
  visibility: ReportVisibility;
  game_plugin_id: string;
  game_region: string;
  client_build: string;
}

export interface SubmissionQueueView {
  schema_version: 1;
  queue_directory: string;
  entry_count: number;
  total_artifact_bytes: number;
  entries: readonly QueuedSubmissionView[];
  issues: readonly string[];
}

export interface VerifiedArtifactView {
  file_byte_length: number;
  file_sha256: string;
  chunk_count: number;
  canonical_content_sha256: string;
}

export interface SubmissionImportResult {
  schema_version: 1;
  outcome: "queued" | "already_queued";
  queue_id: string;
  capture_session_id: string;
  artifact: VerifiedArtifactView;
}

export interface SubmissionVerificationResult {
  schema_version: 1;
  queue_id: string;
  capture_session_id: string;
  verified_unix_millis: number;
  artifact: VerifiedArtifactView;
}

export function parseSubmissionQueue(value: unknown): SubmissionQueueView {
  if (!isRecord(value) || value.schema_version !== 1) {
    throw new Error("The local host returned an unsupported submission queue.");
  }
  if (
    typeof value.queue_directory !== "string" ||
    !isSafeCount(value.entry_count) ||
    !isSafeCount(value.total_artifact_bytes) ||
    !Array.isArray(value.entries) ||
    !value.entries.every(isQueuedSubmission) ||
    value.entry_count !== value.entries.length ||
    !Array.isArray(value.issues) ||
    !value.issues.every((issue) => typeof issue === "string")
  ) {
    throw new Error("The local host returned an invalid submission queue.");
  }
  const entries = value.entries as unknown as QueuedSubmissionView[];
  const artifactBytes = entries.reduce(
    (total, entry) => total + entry.file_byte_length,
    0,
  );
  if (
    !Number.isSafeInteger(artifactBytes) ||
    artifactBytes !== value.total_artifact_bytes
  ) {
    throw new Error("The local host returned an invalid submission queue.");
  }
  return value as unknown as SubmissionQueueView;
}

export function parseSubmissionImportResult(
  value: unknown,
): SubmissionImportResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    (value.outcome !== "queued" && value.outcome !== "already_queued") ||
    !isSha256(value.queue_id) ||
    typeof value.capture_session_id !== "string" ||
    value.capture_session_id.length === 0 ||
    !isVerifiedArtifact(value.artifact) ||
    value.artifact.file_sha256 !== value.queue_id
  ) {
    throw new Error("The local host returned an invalid import result.");
  }
  return value as unknown as SubmissionImportResult;
}

export function parseSubmissionVerificationResult(
  value: unknown,
): SubmissionVerificationResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !isSha256(value.queue_id) ||
    typeof value.capture_session_id !== "string" ||
    value.capture_session_id.length === 0 ||
    !isSafeCount(value.verified_unix_millis) ||
    value.verified_unix_millis === 0 ||
    !isVerifiedArtifact(value.artifact) ||
    value.artifact.file_sha256 !== value.queue_id
  ) {
    throw new Error(
      "The local host returned an invalid artifact verification result.",
    );
  }
  return value as unknown as SubmissionVerificationResult;
}

function isQueuedSubmission(value: unknown): value is QueuedSubmissionView {
  return (
    isRecord(value) &&
    isSha256(value.queue_id) &&
    isSafeCount(value.created_unix_millis) &&
    typeof value.capture_session_id === "string" &&
    value.capture_session_id.length > 0 &&
    typeof value.local_artifact_path === "string" &&
    value.local_artifact_path.length > 0 &&
    typeof value.artifact_exists === "boolean" &&
    typeof value.artifact_byte_length_matches === "boolean" &&
    isSafeCount(value.file_byte_length) &&
    value.file_byte_length > 0 &&
    isSha256(value.canonical_content_sha256) &&
    isSafeCount(value.chunk_count) &&
    value.chunk_count > 0 &&
    isSubmissionState(value.state) &&
    isVisibility(value.visibility) &&
    typeof value.game_plugin_id === "string" &&
    value.game_plugin_id.length > 0 &&
    typeof value.game_region === "string" &&
    value.game_region.length > 0 &&
    typeof value.client_build === "string" &&
    value.client_build.length > 0
  );
}

function isVerifiedArtifact(value: unknown): value is VerifiedArtifactView {
  return (
    isRecord(value) &&
    isSafeCount(value.file_byte_length) &&
    value.file_byte_length > 0 &&
    isSha256(value.file_sha256) &&
    isSafeCount(value.chunk_count) &&
    value.chunk_count > 0 &&
    isSha256(value.canonical_content_sha256)
  );
}

function isSubmissionState(value: unknown): value is SubmissionState {
  return (
    value === "draft" ||
    value === "uploading" ||
    value === "finalizing" ||
    value === "submitted"
  );
}

function isVisibility(value: unknown): value is ReportVisibility {
  return value === "private" || value === "unlisted" || value === "public";
}

function isSha256(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isSafeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
