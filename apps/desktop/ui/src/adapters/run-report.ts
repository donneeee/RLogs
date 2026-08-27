export const RUN_REPORT_SCHEMA_VERSION = 1;

export type ActivityKind = "dungeon" | "raid" | "unknown";
export type RunSegmentKind =
  | "mobbing"
  | "boss"
  | "raid_boss"
  | "gauntlet"
  | "unknown";
export type RunTerminalState =
  | "open"
  | "completed"
  | "failed"
  | "ended"
  | "exited"
  | "superseded";
export type RunSubmissionDisposition =
  | "not_completed"
  | "completed_needs_review"
  | "rank_candidate";
export type EncounterTerminalState = "open" | "cleared" | "wiped" | "ended";

export interface RunReport {
  schemaVersion: 1;
  sourceRlog: string;
  artifactDigest: string;
  integrityVerified: boolean;
  replayMetrics: {
    events_seen: number;
    events_delivered: number;
    outputs_emitted: number;
    output_bytes: number;
    plugin_elapsed_micros: number;
    wall_elapsed_micros: number;
  };
  projection: RunProjection;
}

export interface RunProjection {
  schema_version: 1;
  session_id: string;
  deployment_id: string;
  region_id: string;
  world_id: string | null;
  client_build: string;
  protocol_pack_digest: string;
  runs: RunAnalysis[];
}

export interface RunAnalysis {
  schema_version: 1;
  source_session_id: string;
  encounter_ruleset_id: string | null;
  encounter_ruleset_version: number | null;
  identity: {
    activity_kind: ActivityKind;
    activity_id: string | null;
    activity_family_id: string | null;
    scene_id: number | null;
    observed_dungeon_id: string | null;
    instance_id: string | null;
    difficulty_family: string | null;
    difficulty_id: string | null;
    difficulty_tier: number | null;
    route_id: string | null;
    raid_route_kind: "single_boss" | "gauntlet" | "unknown" | null;
  };
  partition: {
    season_id: string;
    activity_id: string;
    difficulty_id: string;
    route_id: string | null;
    encounter_ruleset_id: string;
    encounter_ruleset_version: number;
  } | null;
  terminal_state: RunTerminalState;
  authoritative_start: boolean;
  authoritative_completion: boolean;
  timing: RunTiming;
  segments: RunSegment[];
  encounters: RunEncounter[];
  manual_pauses: ManualPause[];
  data_gap_count: number;
  findings: RunFinding[];
  submission_disposition: RunSubmissionDisposition;
}

export interface RunTiming {
  started_micros: number;
  ended_micros: number | null;
  observed_until_micros: number;
  wall_time_micros: number | null;
  active_combat_micros: number;
  noncombat_micros: number | null;
  manual_pause_micros: number;
}

export interface RunSegment {
  index: number;
  kind: RunSegmentKind;
  started_micros: number;
  ended_micros: number;
  wall_time_micros: number;
  active_combat_micros: number;
  attempt_count: number;
  retry_count: number;
  total_attempt_wall_time_micros: number;
  total_attempt_active_combat_micros: number;
  elapsed_trying_micros: number;
  between_attempts_micros: number;
  successful_attempt_indices: number[];
  successful_attempt_wall_time_micros: number;
  successful_attempt_active_combat_micros: number;
  winning_attempt_index: number | null;
  winning_attempt_wall_time_micros: number | null;
  winning_attempt_active_combat_micros: number | null;
  encounter_indices: number[];
  closed_at_run_end: boolean;
}

export interface RunEncounter {
  index: number;
  encounter_id: string | null;
  kind: "mobbing" | "boss" | "raid_boss" | "gauntlet_boss" | "unknown";
  segment_index: number;
  attempt_number: number;
  is_retry: boolean;
  is_successful_attempt: boolean;
  terminal_state: EncounterTerminalState;
  started_micros: number;
  ended_micros: number;
  wall_time_micros: number;
  active_combat_micros: number;
  combat_windows: Array<{
    started_micros: number;
    ended_micros: number;
    duration_micros: number;
    closed_at_boundary: boolean;
  }>;
  closed_at_run_end: boolean;
}

export interface ManualPause {
  started_micros: number;
  resumed_micros: number;
  duration_micros: number;
  reason: string;
}

export interface RunFinding {
  finding:
    | "data_gaps"
    | "manual_recorder_pause"
    | "manual_boundary"
    | "start_not_authoritative"
    | "completion_not_authoritative"
    | "leaderboard_partition_unresolved"
    | "combat_closed_at_run_end"
    | "encounter_closed_at_run_end";
  data?: Record<string, number>;
}

const activityKinds = ["dungeon", "raid", "unknown"] as const;
const raidRouteKinds = ["single_boss", "gauntlet", "unknown"] as const;
const segmentKinds = [
  "mobbing",
  "boss",
  "raid_boss",
  "gauntlet",
  "unknown",
] as const;
const encounterKinds = [
  "mobbing",
  "boss",
  "raid_boss",
  "gauntlet_boss",
  "unknown",
] as const;
const runTerminalStates = [
  "open",
  "completed",
  "failed",
  "ended",
  "exited",
  "superseded",
] as const;
const encounterTerminalStates = ["open", "cleared", "wiped", "ended"] as const;
const submissionDispositions = [
  "not_completed",
  "completed_needs_review",
  "rank_candidate",
] as const;
const findingKinds = [
  "data_gaps",
  "manual_recorder_pause",
  "manual_boundary",
  "start_not_authoritative",
  "completion_not_authoritative",
  "leaderboard_partition_unresolved",
  "combat_closed_at_run_end",
  "encounter_closed_at_run_end",
] as const;

export function parseRunReport(value: unknown): RunReport {
  const report = record(value, "run report");
  equal(report.schemaVersion, RUN_REPORT_SCHEMA_VERSION, "run report schemaVersion");
  requiredText(report.sourceRlog, "run report sourceRlog");
  requiredText(report.artifactDigest, "run report artifactDigest");
  equal(report.integrityVerified, true, "run report integrityVerified");
  parseReplayMetrics(report.replayMetrics);
  const projection = parseProjection(report.projection);
  return {
    schemaVersion: 1,
    sourceRlog: report.sourceRlog as string,
    artifactDigest: report.artifactDigest as string,
    integrityVerified: true,
    replayMetrics: report.replayMetrics as RunReport["replayMetrics"],
    projection,
  };
}

export function formatDurationMicros(value: number | null): string {
  if (value === null) return "Open";
  const totalMillis = Math.floor(value / 1_000);
  const hours = Math.floor(totalMillis / 3_600_000);
  const minutes = Math.floor((totalMillis % 3_600_000) / 60_000);
  const seconds = Math.floor((totalMillis % 60_000) / 1_000);
  const millis = totalMillis % 1_000;
  const secondsText = `${seconds}.${millis.toString().padStart(3, "0")}`;
  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${secondsText.padStart(6, "0")}`;
  }
  return `${minutes}:${secondsText.padStart(6, "0")}`;
}

export function formatTierLabel(labelFormat: string, tier: number): string {
  return labelFormat.includes("{tier}")
    ? labelFormat.replaceAll("{tier}", String(tier))
    : `${labelFormat} ${tier}`;
}

export function formatRunDifficulty(
  identity: RunAnalysis["identity"],
  masterLabelFormat = "Master {tier}",
): string {
  const family = identity.difficulty_family?.trim();
  if (family === "master") {
    return identity.difficulty_tier === null
      ? "Master (tier unresolved)"
      : formatTierLabel(masterLabelFormat, identity.difficulty_tier);
  }
  if (family) {
    return family.charAt(0).toUpperCase() + family.slice(1).replaceAll("_", " ");
  }
  return identity.difficulty_id === null
    ? "Unresolved"
    : `Wire ID ${identity.difficulty_id}`;
}

export function dispositionCopy(disposition: RunSubmissionDisposition): {
  label: string;
  detail: string;
} {
  switch (disposition) {
    case "rank_candidate":
      return {
        label: "Rank candidate",
        detail: "Completed with authoritative boundaries and no review findings.",
      };
    case "completed_needs_review":
      return {
        label: "Completed - needs review",
        detail: "Completion was observed, but quality or boundary evidence prevents automatic ranking.",
      };
    case "not_completed":
      return {
        label: "Not leaderboard eligible",
        detail: "This run has no authoritative completed terminal state.",
      };
  }
}

export function findingCopy(finding: RunFinding): string {
  switch (finding.finding) {
    case "data_gaps":
      return `${finding.data?.count ?? 0} capture or decode gap(s)`;
    case "manual_recorder_pause":
      return `${finding.data?.count ?? 0} recorder pause(s), ${formatDurationMicros(finding.data?.duration_micros ?? 0)} total`;
    case "manual_boundary":
      return "A user-created boundary affected this run";
    case "start_not_authoritative":
      return "Run start is not authoritative";
    case "completion_not_authoritative":
      return "Run completion is not authoritative";
    case "leaderboard_partition_unresolved":
      return "Season, activity, or difficulty is not mapped to a leaderboard partition";
    case "combat_closed_at_run_end":
      return "Combat was force-closed at the run boundary";
    case "encounter_closed_at_run_end":
      return "An encounter was force-closed at the run boundary";
  }
}

function parseProjection(value: unknown): RunProjection {
  const projection = record(value, "run report projection");
  equal(projection.schema_version, 1, "projection schema_version");
  requiredText(projection.session_id, "projection session_id");
  requiredText(projection.deployment_id, "projection deployment_id");
  requiredText(projection.region_id, "projection region_id");
  optionalText(projection.world_id, "projection world_id");
  requiredText(projection.client_build, "projection client_build");
  requiredText(projection.protocol_pack_digest, "projection protocol_pack_digest");
  const runs = list(projection.runs, "projection runs", 1_024).map((run, index) =>
    parseRun(run, index, projection.session_id as string),
  );
  return { ...(projection as unknown as RunProjection), runs };
}

function parseRun(value: unknown, index: number, sessionId: string): RunAnalysis {
  const path = `projection runs[${index}]`;
  const run = record(value, path);
  equal(run.schema_version, 1, `${path} schema_version`);
  equal(run.source_session_id, sessionId, `${path} source_session_id`);
  optionalText(run.encounter_ruleset_id, `${path} encounter_ruleset_id`);
  optionalUnsigned(
    run.encounter_ruleset_version,
    `${path} encounter_ruleset_version`,
  );
  const identity = record(run.identity, `${path} identity`);
  oneOf(identity.activity_kind, activityKinds, `${path} activity_kind`);
  for (const key of [
    "activity_id",
    "activity_family_id",
    "observed_dungeon_id",
    "instance_id",
    "difficulty_family",
    "difficulty_id",
    "route_id",
  ]) {
    optionalText(identity[key], `${path} identity.${key}`);
  }
  optionalUnsigned(identity.scene_id, `${path} identity.scene_id`);
  optionalUnsigned(identity.difficulty_tier, `${path} identity.difficulty_tier`);
  optionalOneOf(
    identity.raid_route_kind,
    raidRouteKinds,
    `${path} identity.raid_route_kind`,
  );
  parsePartition(run.partition, `${path} partition`);
  oneOf(run.terminal_state, runTerminalStates, `${path} terminal_state`);
  boolean(run.authoritative_start, `${path} authoritative_start`);
  boolean(run.authoritative_completion, `${path} authoritative_completion`);
  parseTiming(run.timing, `${path} timing`);
  const segments = list(run.segments, `${path} segments`, 4_096).map(
    (segment, segmentIndex) =>
      parseSegment(segment, `${path} segments[${segmentIndex}]`),
  );
  const encounters = list(run.encounters, `${path} encounters`, 100_000).map(
    (encounter, encounterIndex) =>
      parseEncounter(encounter, `${path} encounters[${encounterIndex}]`),
  );
  const manualPauses = list(
    run.manual_pauses,
    `${path} manual_pauses`,
    100_000,
  ).map((pause, pauseIndex) =>
    parseManualPause(pause, `${path} manual_pauses[${pauseIndex}]`),
  );
  unsigned(run.data_gap_count, `${path} data_gap_count`);
  const findings = list(run.findings, `${path} findings`, 100_000).map(
    (finding, findingIndex) =>
      parseFinding(finding, `${path} findings[${findingIndex}]`),
  );
  oneOf(
    run.submission_disposition,
    submissionDispositions,
    `${path} submission_disposition`,
  );
  validateDispositionConsistency(run, findings, path);
  return {
    ...(run as unknown as RunAnalysis),
    segments,
    encounters,
    manual_pauses: manualPauses,
    findings,
  };
}

function validateDispositionConsistency(
  run: Record<string, unknown>,
  findings: RunFinding[],
  path: string,
): void {
  const disposition = run.submission_disposition as RunSubmissionDisposition;
  const completed = run.terminal_state === "completed";
  const authoritative =
    run.authoritative_start === true && run.authoritative_completion === true;
  if (
    disposition === "rank_candidate" &&
    (!completed || !authoritative || findings.length > 0)
  ) {
    throw new Error(`${path} rank_candidate evidence is inconsistent.`);
  }
  if (
    disposition === "completed_needs_review" &&
    (!completed || (authoritative && findings.length === 0))
  ) {
    throw new Error(`${path} completed_needs_review evidence is inconsistent.`);
  }
  if (disposition === "not_completed" && completed) {
    throw new Error(`${path} not_completed evidence is inconsistent.`);
  }
  if (
    findings.some((finding) => finding.finding === "start_not_authoritative") &&
    run.authoritative_start !== false
  ) {
    throw new Error(`${path} authoritative start evidence is inconsistent.`);
  }
  if (
    findings.some(
      (finding) => finding.finding === "completion_not_authoritative",
    ) &&
    run.authoritative_completion !== false
  ) {
    throw new Error(`${path} authoritative completion evidence is inconsistent.`);
  }
}

function parsePartition(value: unknown, path: string): void {
  if (value === null) return;
  const partition = record(value, path);
  for (const key of [
    "season_id",
    "activity_id",
    "difficulty_id",
    "encounter_ruleset_id",
  ]) {
    requiredText(partition[key], `${path}.${key}`);
  }
  optionalText(partition.route_id, `${path}.route_id`);
  unsigned(partition.encounter_ruleset_version, `${path}.encounter_ruleset_version`);
}

function parseTiming(value: unknown, path: string): void {
  const timing = record(value, path);
  for (const key of [
    "started_micros",
    "observed_until_micros",
    "active_combat_micros",
    "manual_pause_micros",
  ]) {
    unsigned(timing[key], `${path}.${key}`);
  }
  optionalUnsigned(timing.ended_micros, `${path}.ended_micros`);
  optionalUnsigned(timing.wall_time_micros, `${path}.wall_time_micros`);
  optionalUnsigned(timing.noncombat_micros, `${path}.noncombat_micros`);
}

function parseSegment(value: unknown, path: string): RunSegment {
  const segment = record(value, path);
  unsigned(segment.index, `${path}.index`);
  oneOf(segment.kind, segmentKinds, `${path}.kind`);
  for (const key of [
    "started_micros",
    "ended_micros",
    "wall_time_micros",
    "active_combat_micros",
    "attempt_count",
    "retry_count",
    "total_attempt_wall_time_micros",
    "total_attempt_active_combat_micros",
    "elapsed_trying_micros",
    "between_attempts_micros",
    "successful_attempt_wall_time_micros",
    "successful_attempt_active_combat_micros",
  ]) {
    unsigned(segment[key], `${path}.${key}`);
  }
  unsignedList(segment.successful_attempt_indices, `${path}.successful_attempt_indices`);
  optionalUnsigned(segment.winning_attempt_index, `${path}.winning_attempt_index`);
  optionalUnsigned(
    segment.winning_attempt_wall_time_micros,
    `${path}.winning_attempt_wall_time_micros`,
  );
  optionalUnsigned(
    segment.winning_attempt_active_combat_micros,
    `${path}.winning_attempt_active_combat_micros`,
  );
  unsignedList(segment.encounter_indices, `${path}.encounter_indices`);
  boolean(segment.closed_at_run_end, `${path}.closed_at_run_end`);
  return segment as unknown as RunSegment;
}

function parseEncounter(value: unknown, path: string): RunEncounter {
  const encounter = record(value, path);
  for (const key of [
    "index",
    "segment_index",
    "attempt_number",
    "started_micros",
    "ended_micros",
    "wall_time_micros",
    "active_combat_micros",
  ]) {
    unsigned(encounter[key], `${path}.${key}`);
  }
  optionalText(encounter.encounter_id, `${path}.encounter_id`);
  oneOf(encounter.kind, encounterKinds, `${path}.kind`);
  boolean(encounter.is_retry, `${path}.is_retry`);
  boolean(encounter.is_successful_attempt, `${path}.is_successful_attempt`);
  oneOf(encounter.terminal_state, encounterTerminalStates, `${path}.terminal_state`);
  list(encounter.combat_windows, `${path}.combat_windows`, 100_000).forEach(
    (window, index) => {
      const parsed = record(window, `${path}.combat_windows[${index}]`);
      unsigned(parsed.started_micros, `${path}.combat_windows[${index}].started_micros`);
      unsigned(parsed.ended_micros, `${path}.combat_windows[${index}].ended_micros`);
      unsigned(parsed.duration_micros, `${path}.combat_windows[${index}].duration_micros`);
      boolean(parsed.closed_at_boundary, `${path}.combat_windows[${index}].closed_at_boundary`);
    },
  );
  boolean(encounter.closed_at_run_end, `${path}.closed_at_run_end`);
  return encounter as unknown as RunEncounter;
}

function parseManualPause(value: unknown, path: string): ManualPause {
  const pause = record(value, path);
  unsigned(pause.started_micros, `${path}.started_micros`);
  unsigned(pause.resumed_micros, `${path}.resumed_micros`);
  unsigned(pause.duration_micros, `${path}.duration_micros`);
  requiredText(pause.reason, `${path}.reason`);
  return pause as unknown as ManualPause;
}

function parseFinding(value: unknown, path: string): RunFinding {
  const finding = record(value, path);
  oneOf(finding.finding, findingKinds, `${path}.finding`);
  if (finding.data !== undefined) {
    const data = record(finding.data, `${path}.data`);
    for (const [key, child] of Object.entries(data)) {
      unsigned(child, `${path}.data.${key}`);
    }
  }
  return finding as unknown as RunFinding;
}

function parseReplayMetrics(value: unknown): void {
  const metrics = record(value, "run report replayMetrics");
  for (const key of [
    "events_seen",
    "events_delivered",
    "outputs_emitted",
    "output_bytes",
    "plugin_elapsed_micros",
    "wall_elapsed_micros",
  ]) {
    unsigned(metrics[key], `run report replayMetrics.${key}`);
  }
}

function record(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${path} must be an object.`);
  }
  return value as Record<string, unknown>;
}

function list(value: unknown, path: string, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) {
    throw new Error(`${path} must be an array with at most ${maximum} entries.`);
  }
  return value;
}

function unsignedList(value: unknown, path: string): void {
  list(value, path, 100_000).forEach((entry, index) =>
    unsigned(entry, `${path}[${index}]`),
  );
}

function unsigned(value: unknown, path: string): void {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0
  ) {
    throw new Error(`${path} must be a non-negative safe integer.`);
  }
}

function optionalUnsigned(value: unknown, path: string): void {
  if (value !== null) unsigned(value, path);
}

function boolean(value: unknown, path: string): void {
  if (typeof value !== "boolean") throw new Error(`${path} must be a boolean.`);
}

function requiredText(value: unknown, path: string): void {
  if (typeof value !== "string" || value.trim() === "" || value.length > 4_096) {
    throw new Error(`${path} must be a non-empty bounded string.`);
  }
}

function optionalText(value: unknown, path: string): void {
  if (value !== null) requiredText(value, path);
}

function equal(value: unknown, expected: unknown, path: string): void {
  if (value !== expected) throw new Error(`${path} is unsupported.`);
}

function oneOf<T extends string>(
  value: unknown,
  choices: readonly T[],
  path: string,
): asserts value is T {
  if (typeof value !== "string" || !choices.includes(value as T)) {
    throw new Error(`${path} is unsupported.`);
  }
}

function optionalOneOf<T extends string>(
  value: unknown,
  choices: readonly T[],
  path: string,
): void {
  if (value !== null) oneOf(value, choices, path);
}
