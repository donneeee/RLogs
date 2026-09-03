import { describe, expect, it } from "vitest";

import {
  parseAutomaticSubmissionStatus,
  parseSubmissionImportResult,
  parseSubmissionQueue,
  parseSubmissionVerificationResult,
} from "./submission-queue";

function queueEntry() {
  return {
    queue_id: "a".repeat(64),
    created_unix_millis: 1_700_000_000_000,
    capture_session_id: "session-1",
    local_artifact_path: "C:/rLogs/runtime-data/logs/session-1.rlog",
    artifact_exists: true,
    artifact_byte_length_matches: true,
    file_byte_length: 8_388_608,
    canonical_content_sha256: "b".repeat(64),
    chunk_count: 2,
    state: "draft",
    visibility: "unlisted",
    game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
    game_region: "north-america",
    client_build: "steam-24252055",
  };
}

describe("local submission queue", () => {
  it("validates observable automatic uploader progress and retry details", () => {
    const status = parseAutomaticSubmissionStatus({
      schemaVersion: 1,
      state: "retrying",
      pendingEligibleCount: 2,
      currentQueueId: "a".repeat(64),
      currentCaptureSessionId: "session-1",
      attemptCount: 3,
      successfulCount: 1,
      retryableFailureCount: 2,
      consecutiveFailures: 2,
      nextRetryUnixMillis: 1_700_000_005_000,
      lastActivityUnixMillis: 1_700_000_000_000,
      lastError: "server rejected the draft",
      lastReportId: "rpt_123",
      lastShareUrl: "https://rlogs-app.github.io/parses/?report=rpt_123",
    });

    expect(status.state).toBe("retrying");
    expect(status.pendingEligibleCount).toBe(2);
    expect(status.lastError).toBe("server rejected the draft");
  });

  it("rejects malformed automatic uploader status", () => {
    expect(() => parseAutomaticSubmissionStatus({
      schemaVersion: 1,
      state: "retrying",
      pendingEligibleCount: -1,
    })).toThrow("invalid automatic submission status");
  });

  it("accepts a bounded version-one draft queue", () => {
    const queue = parseSubmissionQueue({
      schema_version: 1,
      queue_directory: "C:/rLogs/runtime-data/submissions/queue",
      entry_count: 1,
      total_artifact_bytes: 8_388_608,
      entries: [queueEntry()],
      issues: [],
    });

    expect(queue.entries[0]?.state).toBe("draft");
    expect(queue.entries[0]?.artifact_byte_length_matches).toBe(true);
  });

  it("rejects malformed digests, counts, and entry totals", () => {
    const base = {
      schema_version: 1,
      queue_directory: "queue",
      entry_count: 1,
      total_artifact_bytes: 8_388_608,
      entries: [queueEntry()],
      issues: [],
    };
    expect(() =>
      parseSubmissionQueue({
        ...base,
        entries: [{ ...queueEntry(), queue_id: "not-a-digest" }],
      }),
    ).toThrow("invalid submission queue");
    expect(() =>
      parseSubmissionQueue({ ...base, total_artifact_bytes: Number.MAX_VALUE }),
    ).toThrow("invalid submission queue");
    expect(() =>
      parseSubmissionQueue({ ...base, entry_count: 2 }),
    ).toThrow("invalid submission queue");
    expect(() =>
      parseSubmissionQueue({ ...base, total_artifact_bytes: 12 }),
    ).toThrow("invalid submission queue");
  });

  it("rejects unsupported persisted contracts", () => {
    expect(() => parseSubmissionQueue({ schema_version: 2 })).toThrow(
      "unsupported submission queue",
    );
  });

  it("validates import and full re-verification results", () => {
    const artifact = {
      file_byte_length: 8_388_608,
      file_sha256: "a".repeat(64),
      chunk_count: 2,
      canonical_content_sha256: "b".repeat(64),
    };
    const imported = parseSubmissionImportResult({
      schema_version: 1,
      outcome: "queued",
      queue_id: "a".repeat(64),
      capture_session_id: "session-1",
      artifact,
    });
    expect(imported.outcome).toBe("queued");

    const verified = parseSubmissionVerificationResult({
      schema_version: 1,
      queue_id: "a".repeat(64),
      capture_session_id: "session-1",
      verified_unix_millis: 1_700_000_000_000,
      artifact,
    });
    expect(verified.artifact.chunk_count).toBe(2);

    expect(() =>
      parseSubmissionVerificationResult({
        schema_version: 1,
        queue_id: "c".repeat(64),
        capture_session_id: "session-1",
        verified_unix_millis: 1,
        artifact,
      }),
    ).toThrow("invalid artifact verification result");
  });
});
