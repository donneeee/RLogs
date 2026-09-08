import { describe, expect, it } from "vitest";

import { formatDungeonAttemptTime, formatDungeonObjectiveValue, parseMechanicsMapCanvasPreferences, shouldRenderMechanicsMapUpdate } from "./mechanics-map-overlay";

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
      playerX: 32,
      playerY: 72,
      playerWidth: 440,
      actionsX: 40,
      actionsY: 760,
      actionsWidth: 560,
      partyX: 1080,
      partyY: 60,
      partyWidth: 380,
      objectivesX: 620,
      objectivesY: 380,
      objectivesWidth: 440,
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
      playerX: 32,
      playerY: 72,
      playerWidth: 440,
      actionsX: 40,
      actionsY: 760,
      actionsWidth: 560,
      partyX: 1080,
      partyY: 60,
      partyWidth: 380,
      objectivesX: 620,
      objectivesY: 380,
      objectivesWidth: 440,
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
      playerX: Number.NEGATIVE_INFINITY,
      playerY: Number.NaN,
      playerWidth: -20,
      actionsX: Number.POSITIVE_INFINITY,
      actionsY: Number.NaN,
      actionsWidth: 0,
      partyX: Number.NEGATIVE_INFINITY,
      partyY: Number.NaN,
      partyWidth: -1,
      objectivesX: Number.POSITIVE_INFINITY,
      objectivesY: Number.NaN,
      objectivesWidth: 0,
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
      playerX: 24,
      playerY: 48,
      playerWidth: 420,
      actionsX: 24,
      actionsY: 680,
      actionsWidth: 520,
      partyX: 1040,
      partyY: 48,
      partyWidth: 360,
      objectivesX: 580,
      objectivesY: 360,
      objectivesWidth: 420,
    });
  });

  it("shows exact packet values without inventing an objective threshold", () => {
    expect(formatDungeonObjectiveValue(275, false)).toBe("275");
    expect(formatDungeonObjectiveValue(400, true)).toBe("400 ✓");
    expect(formatDungeonObjectiveValue(275, false, 400)).toBe("275 / 400");
    expect(formatDungeonObjectiveValue(400, true, 400)).toBe("400 / 400 ✓");
    expect(formatDungeonObjectiveValue(null, false)).toBe("Observed");
  });

  it("formats the packet-bounded attempt clock with stable tenths", () => {
    expect(formatDungeonAttemptTime(0)).toBe("0:00.0");
    expect(formatDungeonAttemptTime(83_456_789)).toBe("1:23.4");
    expect(formatDungeonAttemptTime(3_723_456_789)).toBe("1:02:03.4");
  });
});
