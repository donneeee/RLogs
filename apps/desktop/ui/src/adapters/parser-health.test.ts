import { describe, expect, it } from "vitest";

import { castObservabilityStatus } from "./parser-health";

describe("cast observability health", () => {
  it("distinguishes absent traffic from successful canonical casts", () => {
    expect(castObservabilityStatus({})).toBe(
      "No local skill request observed yet",
    );
    expect(
      castObservabilityStatus({
        local_skill_request_count: 4,
        local_skill_request_decoded_count: 4,
        canonical_cast_start_count: 4,
      }),
    ).toBe("Canonical cast observation active");
  });

  it("surfaces decoder loss before canonical event loss", () => {
    expect(
      castObservabilityStatus({
        local_skill_request_count: 5,
        local_skill_request_decoded_count: 3,
        local_skill_request_decode_failure_count: 2,
      }),
    ).toBe("2 local skill request decode failures");
    expect(
      castObservabilityStatus({
        local_skill_request_count: 3,
        local_skill_request_decoded_count: 3,
        canonical_cast_start_count: 0,
      }),
    ).toBe("Skill requests decoded · canonical casts missing");
  });
});
