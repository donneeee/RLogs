import { describe, expect, it } from "vitest";

import {
  dispositionCopy,
  findingCopy,
  formatDurationMicros,
  formatRunDifficulty,
  parseRunReport,
} from "./run-report";

describe("Run Report contract", () => {
  it("accepts a sealed typed run projection", () => {
    const report = parseRunReport(fixture());

    expect(report.projection.runs).toHaveLength(1);
    expect(report.projection.runs[0]?.segments[1]?.winning_attempt_index).toBe(2);
    expect(report.projection.runs[0]?.submission_disposition).toBe(
      "completed_needs_review",
    );
  });

  it("rejects a session mismatch and unsupported eligibility state", () => {
    const mismatched = fixture();
    mismatched.projection.runs[0].source_session_id = "different";
    expect(() => parseRunReport(mismatched)).toThrow("source_session_id");

    const invalid = fixture();
    invalid.projection.runs[0].submission_disposition = "ranked";
    expect(() => parseRunReport(invalid)).toThrow("submission_disposition");
  });

  it("rejects contradictory rank and authority evidence", () => {
    const ranked = fixture();
    ranked.projection.runs[0].submission_disposition = "rank_candidate";
    expect(() => parseRunReport(ranked)).toThrow(
      "rank_candidate evidence is inconsistent",
    );

    const contradictory = fixture();
    contradictory.projection.runs[0].findings = [
      { finding: "completion_not_authoritative" },
    ];
    expect(() => parseRunReport(contradictory)).toThrow(
      "authoritative completion evidence is inconsistent",
    );
  });

  it("rejects unsafe timing integers instead of losing precision", () => {
    const value = fixture();
    value.projection.runs[0].timing.wall_time_micros =
      Number.MAX_SAFE_INTEGER + 1;
    expect(() => parseRunReport(value)).toThrow("safe integer");
  });

  it("formats leaderboard timing and evidence for human review", () => {
    expect(formatDurationMicros(3_742_123_000)).toBe("1:02:22.123");
    expect(formatDurationMicros(222_123_000)).toBe("3:42.123");
    expect(formatDurationMicros(null)).toBe("Open");
    expect(dispositionCopy("rank_candidate").label).toBe("Rank candidate");
    expect(
      formatRunDifficulty({
        ...fixture().projection.runs[0].identity,
        difficulty_family: "master",
        difficulty_tier: 20,
      }),
    ).toBe("Master 20");
    expect(
      formatRunDifficulty(
        {
          ...fixture().projection.runs[0].identity,
          difficulty_family: "master",
          difficulty_tier: 3,
        },
        "Maître {tier}",
      ),
    ).toBe("Maître 3");
    expect(
      findingCopy({
        finding: "manual_recorder_pause",
        data: { count: 1, duration_micros: 2_000_000 },
      }),
    ).toContain("0:02.000");
    expect(
      findingCopy({
        finding: "leaderboard_partition_unresolved",
      }),
    ).toContain("leaderboard partition");
  });
});

function fixture(): any {
  const encounter = (
    index: number,
    segmentIndex: number,
    successful: boolean,
  ) => ({
    index,
    encounter_id: segmentIndex === 1 ? "boss-9001" : "wave-1",
    kind: segmentIndex === 1 ? "boss" : "mobbing",
    segment_index: segmentIndex,
    attempt_number: segmentIndex === 1 ? index : 1,
    is_retry: segmentIndex === 1 && index > 1,
    is_successful_attempt: successful,
    terminal_state: successful ? "cleared" : "wiped",
    started_micros: index * 1_000_000,
    ended_micros: (index + 4) * 1_000_000,
    wall_time_micros: 4_000_000,
    active_combat_micros: 3_000_000,
    combat_windows: [
      {
        started_micros: index * 1_000_000,
        ended_micros: (index + 3) * 1_000_000,
        duration_micros: 3_000_000,
        closed_at_boundary: false,
      },
    ],
    closed_at_run_end: false,
  });
  const segment = (index: number, kind: "mobbing" | "boss") => ({
    index,
    kind,
    started_micros: index * 10_000_000,
    ended_micros: (index + 1) * 10_000_000,
    wall_time_micros: 10_000_000,
    active_combat_micros: 6_000_000,
    attempt_count: kind === "boss" ? 2 : 1,
    retry_count: kind === "boss" ? 1 : 0,
    total_attempt_wall_time_micros: kind === "boss" ? 8_000_000 : 4_000_000,
    total_attempt_active_combat_micros:
      kind === "boss" ? 6_000_000 : 3_000_000,
    elapsed_trying_micros: kind === "boss" ? 9_000_000 : 4_000_000,
    between_attempts_micros: kind === "boss" ? 1_000_000 : 0,
    successful_attempt_indices: kind === "boss" ? [2] : [0],
    successful_attempt_wall_time_micros: 4_000_000,
    successful_attempt_active_combat_micros: 3_000_000,
    winning_attempt_index: kind === "boss" ? 2 : 0,
    winning_attempt_wall_time_micros: 4_000_000,
    winning_attempt_active_combat_micros: 3_000_000,
    encounter_indices: kind === "boss" ? [1, 2] : [0],
    closed_at_run_end: false,
  });
  return {
    schemaVersion: 1,
    sourceRlog: "C:\\logs\\sample.rlog",
    artifactDigest: `sha256:${"a".repeat(64)}`,
    integrityVerified: true,
    replayMetrics: {
      events_seen: 50,
      events_delivered: 20,
      outputs_emitted: 1,
      output_bytes: 4_096,
      plugin_elapsed_micros: 200,
      wall_elapsed_micros: 300,
    },
    projection: {
      schema_version: 1,
      session_id: "session-1",
      deployment_id: "global",
      region_id: "north-america",
      world_id: "asteria",
      client_build: "build-1",
      protocol_pack_digest: "sha256:pack-1",
      runs: [
        {
          schema_version: 1,
          source_session_id: "session-1",
          encounter_ruleset_id: "fixture-rules",
          encounter_ruleset_version: 1,
          identity: {
            activity_kind: "dungeon",
            activity_id: "7001",
            activity_family_id: "fixture-dungeon",
            scene_id: 7001,
            observed_dungeon_id: "7001",
            instance_id: "instance-1",
            difficulty_family: "master",
            difficulty_id: "3",
            difficulty_tier: 3,
            route_id: null,
            raid_route_kind: null,
          },
          partition: null,
          terminal_state: "completed",
          authoritative_start: true,
          authoritative_completion: true,
          timing: {
            started_micros: 0,
            ended_micros: 20_000_000,
            observed_until_micros: 20_000_000,
            wall_time_micros: 20_000_000,
            active_combat_micros: 12_000_000,
            noncombat_micros: 8_000_000,
            manual_pause_micros: 2_000_000,
          },
          segments: [segment(0, "mobbing"), segment(1, "boss")],
          encounters: [
            encounter(0, 0, true),
            encounter(1, 1, false),
            encounter(2, 1, true),
          ],
          manual_pauses: [
            {
              started_micros: 5_000_000,
              resumed_micros: 7_000_000,
              duration_micros: 2_000_000,
              reason: "user requested capture pause",
            },
          ],
          data_gap_count: 1,
          findings: [
            { finding: "data_gaps", data: { count: 1 } },
            {
              finding: "manual_recorder_pause",
              data: { count: 1, duration_micros: 2_000_000 },
            },
          ],
          submission_disposition: "completed_needs_review",
        },
      ],
    },
  };
}
