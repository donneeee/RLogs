const IDENTIFIER = /^[A-Za-z0-9_-]{1,128}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const REPORT_ID = /^rpt_[a-f0-9]{32}$/;
const RECONCILIATION_ID = /^rec_[a-f0-9]{32}$/;
export const BACKFILL_SOURCE_SCHEMA_VERSION = 12;
export const CURRENT_REPORT_SCHEMA_VERSION = 15;
export const CURRENT_REPORT_PROJECTION_REVISION = 7;
export const CURRENT_TIMELINE_SCHEMA_VERSION = 3;
export const UPCOMING_REPORT_SCHEMA_VERSION = 15;
export const UPCOMING_REPORT_PROJECTION_REVISION = 8;
export const UPCOMING_TIMELINE_SCHEMA_VERSION = 4;
// Backfill remains pinned to the deployed producer tuple. Advance all three
// constants together only when the backfill container starts emitting v4.
export const BACKFILL_TARGET_SCHEMA_VERSION = CURRENT_REPORT_SCHEMA_VERSION;
export const BACKFILL_TARGET_PROJECTION_REVISION = CURRENT_REPORT_PROJECTION_REVISION;
export const BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION = CURRENT_TIMELINE_SCHEMA_VERSION;

function validReportTuple(report) {
  const timelineSchemaVersion = report?.runs?.[0]?.timeline?.schema_version;
  const validTuple = (
    report?.schema_version === CURRENT_REPORT_SCHEMA_VERSION &&
    report?.projection_revision === CURRENT_REPORT_PROJECTION_REVISION &&
    timelineSchemaVersion === CURRENT_TIMELINE_SCHEMA_VERSION
  ) || (
    report?.schema_version === UPCOMING_REPORT_SCHEMA_VERSION &&
    report?.projection_revision === UPCOMING_REPORT_PROJECTION_REVISION &&
    timelineSchemaVersion === UPCOMING_TIMELINE_SCHEMA_VERSION
  );
  return validTuple && report.runs.every((run) =>
    run?.timeline?.schema_version === timelineSchemaVersion);
}

export function expectedReportId(digest) {
  return `rpt_${digest.slice(0, 32)}`;
}

export function compatibleProfileName(row, manifest) {
  // The claimed character UID is the authoritative join. Region labels have
  // changed over time and must not suppress a verified name for that same UID.
  void manifest;
  try {
    const projection = JSON.parse(row.public_projection_json);
    const name = projection?.character?.display_name ?? projection?.display_name;
    return typeof name === "string" && name.trim() ? name.trim() : null;
  } catch {
    return null;
  }
}

export function validateWakeup(value) {
  return value?.schema_version === 1 && IDENTIFIER.test(value.upload_id ?? "") &&
    DIGEST.test(value.artifact_sha256 ?? "") && REPORT_ID.test(value.expected_report_id ?? "") &&
    value.expected_report_id === expectedReportId(value.artifact_sha256) && Array.isArray(value.chunks);
}

export function sameChunkCommitments(left, right) {
  return Array.isArray(left) && Array.isArray(right) && left.length === right.length &&
    left.every((chunk, index) => {
      const other = right[index];
      return Number(chunk.sequence) === index && Number(other?.sequence) === index &&
        chunk.object_key === other.object_key && Number(chunk.byte_length) === Number(other.byte_length) &&
        chunk.sha256 === other.sha256;
    });
}

export function validateOutput(value, wakeup) {
  return value?.schema_version === 1 && value.report?.report_id === wakeup.expected_report_id &&
    value.report?.verification?.artifact_sha256 === wakeup.artifact_sha256 &&
    value.membership?.report_id === wakeup.expected_report_id &&
    value.membership?.artifact_sha256 === wakeup.artifact_sha256 && Array.isArray(value.report?.runs) &&
    value.report.runs.length > 0 && validReportTuple(value.report) &&
    Array.isArray(value.membership?.runs);
}

export function isSchema12BackfillCandidate(report, row) {
  return report?.schema_version === BACKFILL_SOURCE_SCHEMA_VERSION &&
    report?.report_id === row?.report_id && report?.visibility === "public" &&
    report?.verification?.artifact_sha256 === row?.artifact_sha256 &&
    row?.visibility === "public" && row?.verification_tier === "replayed";
}

// A backfill may add fields derived by a newer trusted replay, but it may not
// change the durable identity, owner, visibility, or sealed evidence that made
// the original report publishable.
export function validateBackfillOutput(value, wakeup, original, row, targetRelease) {
  if (!validateOutput(value, wakeup) ||
      value.report.schema_version !== BACKFILL_TARGET_SCHEMA_VERSION ||
      value.report.projection_revision !== BACKFILL_TARGET_PROJECTION_REVISION ||
      value.report.runs.some((run) =>
        run?.timeline?.schema_version !== BACKFILL_TARGET_TIMELINE_SCHEMA_VERSION) ||
      value.report.visibility !== "public" ||
      value.report.verification?.tier !== "replayed" ||
      value.report.verification?.artifact_sha256 !== row.artifact_sha256 ||
      value.report.submission_provenance?.submitter_id !== row.submitter_id ||
      original.submission_provenance?.submitter_id !== row.submitter_id ||
      value.membership?.report_id !== row.report_id ||
      value.membership?.artifact_sha256 !== row.artifact_sha256) return false;

  if (typeof targetRelease !== "string" || targetRelease.length < 7) return false;
  const runIndexes = new Set();
  if (!value.report.runs.every((run) => Number.isInteger(run?.run_index) && run.run_index >= 0 &&
      typeof run.run_group_id === "string" && run.run_group_id.length > 0 &&
      run.run_group_id.length <= 96 && !runIndexes.has(run.run_index) && runIndexes.add(run.run_index))) {
    return false;
  }
  if (!value.membership.runs.every((run) => runIndexes.has(run?.run_index) &&
      Array.isArray(run.character_ids) && run.character_ids.every((id) =>
        typeof id === "string" && id.length > 0 && id.length <= 128)) ||
      value.membership.character_by_actor == null ||
      typeof value.membership.character_by_actor !== "object" ||
      Array.isArray(value.membership.character_by_actor)) return false;

  const immutable = [
    "report_id", "game_plugin_id", "deployment_id", "region_id", "world_id",
    "client_build", "protocol_pack_digest", "created_unix_millis",
  ];
  const verificationEvidence = [
    "tier", "artifact_sha256", "canonical_content_sha256", "event_count",
    "privacy_policy_digest",
  ];
  const originalVerification = original.verification ?? {};
  return typeof originalVerification.canonical_content_sha256 === "string" &&
    originalVerification.canonical_content_sha256.length > 0 &&
    typeof originalVerification.privacy_policy_digest === "string" &&
    originalVerification.privacy_policy_digest.length > 0 &&
    Number.isSafeInteger(originalVerification.event_count) && originalVerification.event_count >= 0 &&
    immutable.every((field) => value.report[field] === original[field]) &&
    verificationEvidence.every((field) =>
      value.report.verification?.[field] === original.verification?.[field]);
}

export function validateTrainingOutput(value, wakeup) {
  const damage = Number(value?.total_damage);
  const dps = Number(value?.dps);
  return value?.schema_version === 1 && value?.result_id === wakeup.expected_report_id &&
    value?.verification?.artifact_sha256 === wakeup.artifact_sha256 &&
    typeof value?.character_id === "string" && value.character_id.trim() !== "" &&
    Number.isInteger(value?.class_id) && Number.isInteger(value?.specialization_id) &&
    Number.isInteger(value?.season_id) && value.season_id > 0 &&
    (value?.target_monster_id === 115 || value?.target_monster_id === 122) &&
    value?.duration_micros === 180_000_000 && Number.isSafeInteger(damage) && damage > 0 &&
    Number.isFinite(dps) && Math.abs(dps - damage / 180) < 0.001;
}

export function reconciliationSourceIdentity(source) {
  return `${source.report_id}:${Number(source.run_index)}:${source.artifact_sha256}`;
}

export const RECONCILIATION_SCHEMA_VERSION = 18;
export const UPCOMING_RECONCILIATION_SCHEMA_VERSION = 19;
const MAXIMUM_TIMELINE_RATE_CLOCK_POINTS = 262_144;

function validConservedReplay(value) {
  return Number.isSafeInteger(value?.raw_damage) && Number.isSafeInteger(value?.rdps_damage) &&
    Number.isSafeInteger(value?.contribution_given) && Number.isSafeInteger(value?.contribution_received) &&
    value?.conserved === true && value.raw_damage === value.rdps_damage &&
    value.contribution_given === value.contribution_received;
}

function validReplayRateClock(timeline) {
  if (timeline?.source !== "reconciled_canonical_spine" ||
      !Number.isSafeInteger(timeline.duration_micros) || timeline.duration_micros < 0 ||
      !Number.isSafeInteger(timeline.series_bucket_micros) || timeline.series_bucket_micros <= 0 ||
      typeof timeline.rate_clock_complete !== "boolean" || !Array.isArray(timeline.rate_clock) ||
      !Number.isSafeInteger(timeline.omitted?.rate_clock_points) || timeline.omitted.rate_clock_points < 0 ||
      timeline.rate_clock.length > MAXIMUM_TIMELINE_RATE_CLOCK_POINTS) return false;
  if (!timeline.rate_clock_complete) return timeline.rate_clock.length === 0;
  const expected = Math.ceil(timeline.duration_micros / timeline.series_bucket_micros);
  if (expected > MAXIMUM_TIMELINE_RATE_CLOCK_POINTS || timeline.omitted.rate_clock_points !== 0 ||
      timeline.rate_clock.length !== expected) return false;
  let previousEdps = 0;
  let previousAdps = 0;
  return timeline.rate_clock.every((point, index) => {
    const bucketEndMicros = Math.min(
      timeline.duration_micros,
      (index + 1) * timeline.series_bucket_micros,
    );
    const valid = point?.second === index && Number.isSafeInteger(point.edps_elapsed_micros) &&
      Number.isSafeInteger(point.adps_elapsed_micros) && point.edps_elapsed_micros >= previousEdps &&
      point.adps_elapsed_micros >= previousAdps && point.adps_elapsed_micros <= point.edps_elapsed_micros &&
      point.edps_elapsed_micros <= bucketEndMicros;
    if (valid) {
      previousEdps = point.edps_elapsed_micros;
      previousAdps = point.adps_elapsed_micros;
    }
    return valid;
  });
}

export function validateReconciliationOutput(value, runGroupId, sources) {
  const validTuple = (
    value?.schema_version === RECONCILIATION_SCHEMA_VERSION &&
    value?.timeline?.schema_version === CURRENT_TIMELINE_SCHEMA_VERSION
  ) || (
    value?.schema_version === UPCOMING_RECONCILIATION_SCHEMA_VERSION &&
    value?.timeline?.schema_version === UPCOMING_TIMELINE_SCHEMA_VERSION
  );
  if (!validTuple || value?.run_group_id !== runGroupId ||
      !RECONCILIATION_ID.test(value?.reconciliation_id ?? "") ||
      !Array.isArray(value?.reports) || !value?.canonical_spine) return false;
  if (value.reports.some((report) =>
    typeof report?.deployment_id !== "string" || report.deployment_id.length === 0 ||
    typeof report?.client_build !== "string" || report.client_build.length === 0 ||
    typeof report?.protocol_pack_digest !== "string" || report.protocol_pack_digest.length === 0)) {
    return false;
  }
  const expected = sources.map(reconciliationSourceIdentity).sort();
  const actual = value.reports.map(reconciliationSourceIdentity).sort();
  if (actual.length !== expected.length || actual.some((identity, index) => identity !== expected[index])) {
    return false;
  }
  const unique = new Set(actual);
  if (unique.size !== actual.length || !unique.has(reconciliationSourceIdentity(value.canonical_spine))) {
    return false;
  }
  const validStatus = [
    "single_vantage", "multiple_reports_no_additional_vantage",
    "cross_vantage_evidence_available", "reconciled",
  ].includes(value.status);
  if (!validStatus || typeof value.attribution_replay_completed !== "boolean") return false;
  if (!value.attribution_replay_completed) {
    return value.status !== "reconciled" && value.rdps_status === null;
  }
  return value.status === "reconciled" && typeof value.rdps_status === "string" &&
    value.rdps_status.trim().length > 0 && value.rdps_status.length <= 4_096 &&
    Array.isArray(value.reconciled_participants) && value.reconciled_participants.length > 0 &&
    validConservedReplay(value.conservation) && validReplayRateClock(value.timeline);
}

export function reconcileCatalogEntry(entry, result, sourceCount, distinctSubmitterCount) {
  return {
    ...entry,
    report_ids: result.reports.map((report) => report.report_id).sort(),
    contribution_count: sourceCount,
    distinct_submitter_count: distinctSubmitterCount,
    local_profile_witness_character_count: Number(result.local_vantage_character_count ?? 0),
    attribution_reconciliation_status: result.status,
    reconciliation_id: result.reconciliation_id,
  };
}

export async function runOneShotVerifier(container, request) {
  try {
    const response = await container.fetch(request);
    const result = await response.json().catch(() => null);
    return { response, result };
  } finally {
    // The verifier is stateless and each upload receives a dedicated instance.
    // Always release that instance after consuming its response so a process
    // that ignores SIGTERM cannot exhaust the bounded production pool.
    await container.destroy().catch((cause) => {
      console.error("could not destroy completed verifier container", cause);
    });
  }
}

export function catalogEntry(report, run) {
  const runGroupId = run.run_group_id || `legacy_${report.report_id}_${run.run_index}`;
  return {
    report_id: report.report_id,
    report_ids: [report.report_id],
    run_index: run.run_index,
    run_group_id: runGroupId,
    contribution_count: 1,
    distinct_submitter_count: report.submission_provenance?.submitter_id ? 1 : 0,
    local_profile_witness_character_count: run.local_profile_character_ids?.length ?? 0,
    attribution_reconciliation_status: "single_vantage",
    created_unix_millis: report.created_unix_millis,
    ...(report.submission_provenance?.submitter_id ? { submitter_id: report.submission_provenance.submitter_id } : {}),
    deployment_id: report.deployment_id,
    client_build: report.client_build,
    protocol_pack_digest: report.protocol_pack_digest,
    region_id: report.region_id,
    activity_id: run.activity_id ?? null,
    activity_family_id: run.activity_family_id ?? null,
    ...(run.activity_category_id ? { activity_category_id: run.activity_category_id } : {}),
    scene_id: run.scene_id ?? null,
    scene_name: run.scene_name ?? null,
    difficulty_family: run.difficulty_family ?? null,
    difficulty_tier: run.difficulty_tier ?? null,
    terminal_state: run.terminal_state,
    total_run_time_micros: run.total_run_time_micros ?? null,
    participant_count: run.participants?.length ?? 0,
  };
}
