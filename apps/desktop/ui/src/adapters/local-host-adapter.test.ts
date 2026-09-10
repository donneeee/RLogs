import { describe, expect, it } from "vitest";

import { combatHistoryDetailRequestBody, openEditableOverlayCanvas } from "./local-host-adapter";

describe("local host Combat History requests", () => {
  it("sends the selected UI locale with the history detail identity", () => {
    expect(JSON.parse(combatHistoryDetailRequestBody("capture-1", "fr-FR"))).toEqual({
      sessionId: "capture-1",
      locale: "fr-FR",
    });
  });
});

describe("overlay canvas recovery", () => {
  it("shows or recreates a hidden/absent canvas before requesting edit interactivity", async () => {
    let available = false;
    const calls: string[] = [];
    await openEditableOverlayCanvas(async (command) => {
      calls.push(command);
      if (command === "show_overlay_canvas_editable") available = true;
    });
    expect(available).toBe(true);
    expect(calls).toEqual(["show_overlay_canvas_editable"]);
  });
});
