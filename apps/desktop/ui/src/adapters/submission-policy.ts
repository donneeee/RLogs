import type { VerifiedArtifactView } from "./submission-queue";

export type SuccessfulArtifactRetention =
  | "keep"
  | "remove_after_verified_receipt";

export type SubmissionVisibility = "private" | "unlisted" | "public";

export interface SubmissionPolicy {
  schema_version: 1;
  log_uploader: {
    enabled: boolean;
    automatic_combat_logs: boolean;
    default_visibility: SubmissionVisibility;
    successful_artifact_retention: SuccessfulArtifactRetention;
  };
  bpsr_profile_sync: {
    enabled: boolean;
    automatic_profiles: boolean;
    publish_photo_wall_images: boolean;
  };
}

export interface SubmissionPolicyView extends SubmissionPolicy {
  settings_path: string;
  transport_mode: "disconnected" | "http";
  endpoint_url: string | null;
  issue: string | null;
}

export interface SubmissionTransportResult {
  schema_version: 1;
  queue_id: string;
  capture_session_id: string;
  report_id: string;
  share_url: string;
  final_state: "submitted";
  verification_tier: "uploaded" | "replayed" | "corroborated" | "ranked";
  chunk_count: number;
  uploaded_chunk_count: number;
  uploaded_bytes: number;
  resumed: boolean;
  duplicate: boolean;
}

export interface MockSubmissionResult {
  schema_version: 1;
  queue_id: string;
  capture_session_id: string;
  report_id: string;
  final_state: "submitted";
  verification_tier: "replayed";
  chunk_count: number;
  uploaded_bytes: number;
  resumed_after_restart: boolean;
  external_network_requests: 0;
  artifact: VerifiedArtifactView;
}

export function parseSubmissionPolicy(value: unknown): SubmissionPolicyView {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.settings_path !== "string" ||
    (value.transport_mode !== "disconnected" && value.transport_mode !== "http") ||
    (value.endpoint_url !== null && typeof value.endpoint_url !== "string") ||
    !isLogUploaderPolicy(value.log_uploader) ||
    !isProfileSyncPolicy(value.bpsr_profile_sync) ||
    (value.issue !== null && typeof value.issue !== "string")
  ) {
    throw new Error("The local host returned an invalid submission policy.");
  }
  return value as unknown as SubmissionPolicyView;
}

export function parseSubmissionTransportResult(
  value: unknown,
): SubmissionTransportResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !isSha256(value.queue_id) ||
    typeof value.capture_session_id !== "string" ||
    value.capture_session_id.length === 0 ||
    typeof value.report_id !== "string" ||
    value.report_id.length === 0 ||
    typeof value.share_url !== "string" ||
    !isHttpUrl(value.share_url) ||
    value.final_state !== "submitted" ||
    !["uploaded", "replayed", "corroborated", "ranked"].includes(
      String(value.verification_tier),
    ) ||
    !isSafeCount(value.chunk_count) ||
    value.chunk_count === 0 ||
    !isSafeCount(value.uploaded_chunk_count) ||
    value.uploaded_chunk_count > value.chunk_count ||
    !isSafeCount(value.uploaded_bytes) ||
    typeof value.resumed !== "boolean" ||
    typeof value.duplicate !== "boolean"
  ) {
    throw new Error("The local host returned an invalid submission receipt.");
  }
  return value as unknown as SubmissionTransportResult;
}

export function parseMockSubmissionResult(
  value: unknown,
): MockSubmissionResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !isSha256(value.queue_id) ||
    typeof value.capture_session_id !== "string" ||
    value.capture_session_id.length === 0 ||
    typeof value.report_id !== "string" ||
    value.report_id.length === 0 ||
    value.final_state !== "submitted" ||
    value.verification_tier !== "replayed" ||
    !isSafeCount(value.chunk_count) ||
    value.chunk_count === 0 ||
    !isSafeCount(value.uploaded_bytes) ||
    value.uploaded_bytes === 0 ||
    value.resumed_after_restart !== true ||
    value.external_network_requests !== 0 ||
    !isVerifiedArtifact(value.artifact) ||
    value.artifact.file_sha256 !== value.queue_id ||
    value.artifact.chunk_count !== value.chunk_count ||
    value.artifact.file_byte_length !== value.uploaded_bytes
  ) {
    throw new Error("The local host returned an invalid mock submission.");
  }
  return value as unknown as MockSubmissionResult;
}

export function editableSubmissionPolicy(
  view: SubmissionPolicyView,
): SubmissionPolicy {
  return {
    schema_version: 1,
    log_uploader: { ...view.log_uploader },
    bpsr_profile_sync: { ...view.bpsr_profile_sync },
  };
}

function isLogUploaderPolicy(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.enabled === "boolean" &&
    typeof value.automatic_combat_logs === "boolean" &&
    (value.default_visibility === "private" ||
      value.default_visibility === "unlisted" ||
      value.default_visibility === "public") &&
    (value.successful_artifact_retention === "keep" ||
      value.successful_artifact_retention ===
        "remove_after_verified_receipt")
  );
}

function isProfileSyncPolicy(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.enabled === "boolean" &&
    typeof value.automatic_profiles === "boolean" &&
    typeof value.publish_photo_wall_images === "boolean"
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

function isSha256(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isSafeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:";
  } catch {
    return false;
  }
}
