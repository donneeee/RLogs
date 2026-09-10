import { describe, expect, it } from "vitest";

import { combatHistoryDetailRequestBody } from "./local-host-adapter";

describe("local host Combat History requests", () => {
  it("sends the selected UI locale with the history detail identity", () => {
    expect(JSON.parse(combatHistoryDetailRequestBody("capture-1", "fr-FR"))).toEqual({
      sessionId: "capture-1",
      locale: "fr-FR",
    });
  });
});
