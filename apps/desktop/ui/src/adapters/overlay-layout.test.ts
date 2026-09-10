import { describe, expect, it } from "vitest";
import { activeOverlaySetup, clampOverlayModule, normalizedModuleGeometry, parseOverlayLayoutSettings, raiseOverlayModule, safeDefaultOverlayLayout } from "./overlay-layout";

describe("overlay layout authority", () => {
  it("normalizes and clamps legacy pixel geometry", () => {
    expect(normalizedModuleGeometry({ x: -5, y: 2000, width: 4000, height: 10 }, 1920, 1080))
      .toEqual({ x: 0, y: 0.94, width: 1, height: 0.06 });
  });
  it("clamps presentation without changing module identity", () => {
    expect(clampOverlayModule({ x: -1, y: 2, width: .2, height: .2, visible: true, zOrder: 9999, opacity: 0, scale: 4 }))
      .toEqual({ x: 0, y: .6, width: .2, height: .2, visible: true, zOrder: 1000, opacity: .2, scale: 2 });
  });
  it("rejects incomplete setup payloads", () => {
    expect(() => parseOverlayLayoutSettings({ schemaVersion: 1, revision: 0, selectedSetupId: "default", legacyMigrationComplete: true,
      setups: { default: { name: "Default", locked: false, modules: {} } } })).toThrow("Invalid overlay module layout");
  });
  it("renormalizes every layer before raising past the cap", () => {
    const setup = activeOverlaySetup(safeDefaultOverlayLayout(4));
    setup.modules.alerts.zOrder = 1000;
    expect(raiseOverlayModule(setup, "map")).toBe(6);
    expect(new Set(Object.values(setup.modules).map((module) => module.zOrder)).size).toBe(7);
    expect(setup.modules.map.zOrder).toBe(Math.max(...Object.values(setup.modules).map((module) => module.zOrder)));
  });
});
