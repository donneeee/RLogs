export interface ProfilePackageView {
  package_id: string;
  created_unix_millis: number;
  local_package_path: string;
  package_byte_length: number;
  game_plugin_id: string;
  deployment: string;
  region: string;
  realm: string | null;
  world: string | null;
  character_id: string;
  display_name: string | null;
  server_id: string | null;
  class_id: number | null;
  specialization_id: number | null;
  level: number | null;
  profile_field_count: number;
  source_session_id: string;
  source_client_build: string;
  source_observation_count: number;
  source_last_event_sequence: number;
}

export interface ProfilePackageStoreView {
  schema_version: 1;
  package_root: string;
  entry_count: number;
  total_package_bytes: number;
  entries: readonly ProfilePackageView[];
  issues: readonly string[];
}

export interface ProfilePackageInspection {
  schema_version: 1;
  local_package_path: string;
  package_byte_length: number;
  package: Record<string, unknown>;
}

export interface ProfileProjectionResult {
  schema_version: 1;
  source_session_id: string;
  projected_package_count: number;
  stored_packages: readonly ProfilePackageView[];
  external_network_requests: 0;
}

export interface ProfilePublishResult {
  schema_version: 1;
  profile_id: string;
  character_id: string;
  package_id: string;
  claimed: boolean;
  duplicate: boolean;
  module_inventory_count: number;
  equipped_module_count: number;
  profile_url: string;
}

export function parseProfilePackageStore(
  value: unknown,
): ProfilePackageStoreView {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.package_root !== "string" ||
    !isSafeCount(value.entry_count) ||
    !isSafeCount(value.total_package_bytes) ||
    !Array.isArray(value.entries) ||
    !value.entries.every(isProfilePackageView) ||
    value.entry_count !== value.entries.length ||
    !Array.isArray(value.issues) ||
    !value.issues.every((issue) => typeof issue === "string")
  ) {
    throw new Error("The local host returned an invalid profile package store.");
  }
  const packageBytes = value.entries.reduce(
    (total: number, entry: ProfilePackageView) =>
      total + entry.package_byte_length,
    0,
  );
  if (
    !Number.isSafeInteger(packageBytes) ||
    packageBytes !== value.total_package_bytes
  ) {
    throw new Error("The local host returned an invalid profile package store.");
  }
  return value as unknown as ProfilePackageStoreView;
}

export function parseProfilePackageInspection(
  value: unknown,
): ProfilePackageInspection {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.local_package_path !== "string" ||
    value.local_package_path.length === 0 ||
    !isSafePositiveCount(value.package_byte_length) ||
    !isLocalProfilePackage(value.package)
  ) {
    throw new Error(
      "The local host returned an invalid profile package inspection.",
    );
  }
  return value as unknown as ProfilePackageInspection;
}

export function parseProfileProjectionResult(
  value: unknown,
): ProfileProjectionResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.source_session_id !== "string" ||
    value.source_session_id.length === 0 ||
    !isSafeCount(value.projected_package_count) ||
    !Array.isArray(value.stored_packages) ||
    !value.stored_packages.every(isProfilePackageView) ||
    value.projected_package_count !== value.stored_packages.length ||
    value.external_network_requests !== 0
  ) {
    throw new Error("The local host returned an invalid profile projection.");
  }
  return value as unknown as ProfileProjectionResult;
}

export function parseProfilePublishResult(value: unknown): ProfilePublishResult {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.profile_id !== "string" ||
    !/^prf_[0-9a-f]{32}$/u.test(value.profile_id) ||
    typeof value.character_id !== "string" ||
    value.character_id.length === 0 ||
    !isSha256(value.package_id) ||
    value.claimed !== true ||
    typeof value.duplicate !== "boolean" ||
    !isSafeCount(value.module_inventory_count) ||
    !isSafeCount(value.equipped_module_count) ||
    typeof value.profile_url !== "string" ||
    !value.profile_url.startsWith("https://")
  ) {
    throw new Error("The local host returned an invalid profile publication receipt.");
  }
  return value as unknown as ProfilePublishResult;
}

function isProfilePackageView(value: unknown): value is ProfilePackageView {
  return (
    isRecord(value) &&
    isSha256(value.package_id) &&
    isSafePositiveCount(value.created_unix_millis) &&
    typeof value.local_package_path === "string" &&
    value.local_package_path.length > 0 &&
    isSafePositiveCount(value.package_byte_length) &&
    typeof value.game_plugin_id === "string" &&
    value.game_plugin_id.length > 0 &&
    typeof value.deployment === "string" &&
    value.deployment.length > 0 &&
    typeof value.region === "string" &&
    value.region.length > 0 &&
    isNullableString(value.realm) &&
    isNullableString(value.world) &&
    typeof value.character_id === "string" &&
    value.character_id.length > 0 &&
    isNullableString(value.display_name) &&
    isNullableString(value.server_id) &&
    isNullableSafeInteger(value.class_id) &&
    isNullableSafeInteger(value.specialization_id) &&
    isNullableSafeCount(value.level) &&
    isSafeCount(value.profile_field_count) &&
    typeof value.source_session_id === "string" &&
    value.source_session_id.length > 0 &&
    typeof value.source_client_build === "string" &&
    value.source_client_build.length > 0 &&
    isSafePositiveCount(value.source_observation_count) &&
    isSafePositiveCount(value.source_last_event_sequence)
  );
}

function isLocalProfilePackage(value: unknown): boolean {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !isSha256(value.package_id) ||
    !isSafePositiveCount(value.created_unix_millis) ||
    !isRecord(value.source) ||
    typeof value.source.session_id !== "string" ||
    typeof value.source.client_build !== "string" ||
    typeof value.source.protocol_pack_digest !== "string" ||
    !isPrefixedSha256(value.source.canonical_content_sha256) ||
    !isSafePositiveCount(value.source.observation_count) ||
    !isSafePositiveCount(value.source.last_event_sequence) ||
    !isRecord(value.request) ||
    typeof value.request.relative_endpoint !== "string" ||
    !value.request.relative_endpoint.startsWith("/") ||
    !isRecord(value.request.payload)
  ) {
    return false;
  }
  const payload = value.request.payload;
  return (
    payload.schema_version === 1 &&
    payload.payload_kind === "character-profile" &&
    typeof payload.game_plugin_id === "string" &&
    typeof payload.payload_schema_id === "string" &&
    isSafePositiveCount(payload.payload_schema_version) &&
    isRecord(payload.routing) &&
    typeof payload.routing["character-id"] === "string" &&
    typeof payload.routing.deployment === "string" &&
    typeof payload.routing.region === "string" &&
    isRecord(payload.body)
  );
}

function isNullableString(value: unknown): boolean {
  return value === null || typeof value === "string";
}

function isNullableSafeInteger(value: unknown): boolean {
  return value === null || Number.isSafeInteger(value);
}

function isNullableSafeCount(value: unknown): boolean {
  return value === null || isSafeCount(value);
}

function isSafePositiveCount(value: unknown): value is number {
  return isSafeCount(value) && value > 0;
}

function isSafeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isSha256(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isPrefixedSha256(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^sha256:[0-9a-f]{64}$/u.test(value)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
