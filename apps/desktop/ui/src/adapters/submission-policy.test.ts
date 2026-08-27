import { describe, expect, it } from "vitest";

import {
  editableSubmissionPolicy,
  parseMockSubmissionResult,
  parseSubmissionPolicy,
  parseSubmissionTransportResult,
} from "./submission-policy";

function policy() {
  return {
    schema_version: 1,
    settings_path: "C:/rLogs/runtime-data/settings/submission-policy.v1.json",
    transport_mode: "disconnected",
    endpoint_url: null,
    log_uploader: {
      enabled: false,
      automatic_combat_logs: true,
      default_visibility: "unlisted",
      successful_artifact_retention: "keep",
    },
    bpsr_profile_sync: {
      enabled: false,
      automatic_profiles: true,
    },
    issue: null,
  };
}

describe("submission policy", () => {
  it("accepts disabled-by-default independent plug-in policies", () => {
    const parsed = parseSubmissionPolicy(policy());
    expect(parsed.log_uploader.enabled).toBe(false);
    expect(parsed.bpsr_profile_sync.enabled).toBe(false);
    expect(editableSubmissionPolicy(parsed)).not.toHaveProperty("settings_path");
  });

  it("rejects connected or malformed policy claims", () => {
    expect(() =>
      parseSubmissionPolicy({ ...policy(), transport_mode: "internet" }),
    ).toThrow("invalid submission policy");
    expect(() =>
      parseSubmissionPolicy({
        ...policy(),
        log_uploader: {
          ...policy().log_uploader,
          default_visibility: "everyone",
        },
      }),
    ).toThrow("invalid submission policy");
  });

  it("requires mock receipts to prove resume and zero external requests", () => {
    const artifact = {
      file_byte_length: 100,
      file_sha256: "a".repeat(64),
      chunk_count: 2,
      canonical_content_sha256: "b".repeat(64),
    };
    const result = parseMockSubmissionResult({
      schema_version: 1,
      queue_id: "a".repeat(64),
      capture_session_id: "capture-1",
      report_id: "mock-report-a",
      final_state: "submitted",
      verification_tier: "replayed",
      chunk_count: 2,
      uploaded_bytes: 100,
      resumed_after_restart: true,
      external_network_requests: 0,
      artifact,
    });
    expect(result.report_id).toBe("mock-report-a");

    expect(() =>
      parseMockSubmissionResult({
        schema_version: 1,
        queue_id: "a".repeat(64),
        capture_session_id: "capture-1",
        report_id: "mock-report-a",
        final_state: "submitted",
        verification_tier: "replayed",
        chunk_count: 2,
        uploaded_bytes: 100,
        resumed_after_restart: false,
        external_network_requests: 0,
        artifact,
      }),
    ).toThrow("invalid mock submission");
  });

  it("accepts verified receiver receipts without exposing credentials", () => {
    const result = parseSubmissionTransportResult({
      schema_version: 1,
      queue_id: "a".repeat(64),
      capture_session_id: "capture-1",
      report_id: "rpt_0123456789abcdef0123456789abcdef",
      share_url:
        "https://donneeee.github.io/rlogs-website/?parse=rpt_0123456789abcdef0123456789abcdef&run=0#parse",
      final_state: "submitted",
      verification_tier: "replayed",
      chunk_count: 4,
      uploaded_chunk_count: 2,
      uploaded_bytes: 2048,
      resumed: true,
      duplicate: false,
    });

    expect(result.uploaded_chunk_count).toBe(2);
    expect(result.resumed).toBe(true);
    expect(result).not.toHaveProperty("api_key");
  });
});
