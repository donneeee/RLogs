import { describe, expect, it, vi } from "vitest";

import { combatHistoryDetailRequestBody, hideOverlayCanvas, openEditableOverlayCanvas, showOverlayCanvas } from "./local-host-adapter";

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

  it("keeps passive show, editable show, and full hide as distinct native commands", async () => {
    const invoke = vi.fn(async (_command: string) => undefined);
    await showOverlayCanvas(invoke);
    await openEditableOverlayCanvas(invoke);
    await hideOverlayCanvas(invoke);
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "show_overlay_canvas",
      "show_overlay_canvas_editable",
      "hide_overlay_canvas",
    ]);
  });
});
