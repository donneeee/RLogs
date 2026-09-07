export interface CastObservabilityCounters {
  local_skill_request_count?: number;
  local_skill_request_decoded_count?: number;
  local_skill_request_decode_failure_count?: number;
  canonical_cast_start_count?: number;
}

export function castObservabilityStatus(
  counters: CastObservabilityCounters,
): string {
  const requests = counters.local_skill_request_count ?? 0;
  const decoded = counters.local_skill_request_decoded_count ?? 0;
  const failures = counters.local_skill_request_decode_failure_count ?? 0;
  const casts = counters.canonical_cast_start_count ?? 0;

  if (failures > 0) {
    return `${failures.toLocaleString()} local skill request decode failure${failures === 1 ? "" : "s"}`;
  }
  if (requests > decoded) {
    return `${(requests - decoded).toLocaleString()} local skill request${requests - decoded === 1 ? "" : "s"} not decoded`;
  }
  if (decoded > 0 && casts === 0) {
    return "Skill requests decoded · canonical casts missing";
  }
  if (casts > 0) {
    return "Canonical cast observation active";
  }
  return "No local skill request observed yet";
}
