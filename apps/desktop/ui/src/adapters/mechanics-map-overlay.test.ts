import { describe, expect, it } from "vitest";

import { parseMechanicsMapCanvasPreferences, shouldRenderMechanicsMapUpdate } from "./mechanics-map-overlay";

describe("Mechanics Map overlay canvas preferences", () => {
  it("does not rebuild the overlay after an unchanged long-poll timeout", () => {
    const current = { revision: 12 };
    expect(shouldRenderMechanicsMapUpdate(current, current)).toBe(false);
    expect(shouldRenderMechanicsMapUpdate(current, { revision: 13 })).toBe(true);
  });

  it("restores valid free zoom, pan, filter, rotation, and lock state", () => {
    expect(parseMechanicsMapCanvasPreferences({
      scale: 37.5,
      panX: -184,
      panY: 92,
      rotateWithPlayer: false,
      showMonsters: false,
      locked: true,
      moduleX: 300,
      moduleY: 180,
      moduleWidth: 640,
      moduleHeight: 480,
      targetX: 720,
      targetY: 64,
      targetWidth: 460,
    })).toEqual({
      scale: 37.5,
      panX: -184,
      panY: 92,
      rotateWithPlayer: false,
      showMonsters: false,
      locked: true,
      moduleX: 300,
      moduleY: 180,
      moduleWidth: 640,
      moduleHeight: 480,
      targetX: 720,
      targetY: 64,
      targetWidth: 460,
    });
  });

  it("replaces malformed or unsafe persisted values with production defaults", () => {
    expect(parseMechanicsMapCanvasPreferences({
      scale: 0,
      panX: Number.POSITIVE_INFINITY,
      panY: 10_000_001,
      rotateWithPlayer: "yes",
      showMonsters: null,
      locked: 1,
      moduleX: Number.NaN,
      moduleY: -10_000_001,
      moduleWidth: -1,
      moduleHeight: 0,
      targetX: Number.POSITIVE_INFINITY,
      targetY: Number.NaN,
      targetWidth: 0,
    })).toEqual({
      scale: 1,
      panX: 0,
      panY: 0,
      rotateWithPlayer: true,
      showMonsters: true,
      locked: false,
      moduleX: 24,
      moduleY: 120,
      moduleWidth: 520,
      moduleHeight: 520,
      targetX: 580,
      targetY: 48,
      targetWidth: 420,
    });
  });
});
