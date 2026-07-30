import { describe, expect, it } from "vitest";

import { projectedRunCount } from "./run-projection";

describe("run projection output", () => {
  it("counts only the Encounter Recorder run snapshot", () => {
    expect(
      projectedRunCount([
        {
          type: "snapshot",
          schema_id: "app.rlogs.combat-meter.snapshot",
          payload: { runs: ["not-an-encounter-projection"] },
        },
        {
          type: "snapshot",
          schema_id: "app.rlogs.encounter-recorder.runs",
          payload: { runs: [{ id: 1 }, { id: 2 }] },
        },
      ]),
    ).toBe(2);
  });

  it("fails closed for missing or malformed outputs", () => {
    expect(projectedRunCount([])).toBe(0);
    expect(
      projectedRunCount([
        {
          type: "snapshot",
          schema_id: "app.rlogs.encounter-recorder.runs",
          payload: { runs: "not-an-array" },
        },
      ]),
    ).toBe(0);
  });
});
