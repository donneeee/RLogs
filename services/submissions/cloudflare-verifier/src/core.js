const IDENTIFIER = /^[A-Za-z0-9_-]{1,128}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const REPORT_ID = /^rpt_[a-f0-9]{32}$/;

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
    value.report.runs.length > 0 && Array.isArray(value.membership?.runs);
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
