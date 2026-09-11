import { describe, expect, it } from "vitest";
import { activeOverlaySetup, clampOverlayModule, clampOverlayModulePixelGeometry, normalizedModuleGeometry, parseOverlayLayoutSettings, raiseOverlayModule, safeDefaultOverlayLayout } from "./overlay-layout";

describe("overlay layout authority", () => {
  it("normalizes and clamps legacy pixel geometry", () => {
    expect(normalizedModuleGeometry({ x: -5, y: 2000, width: 4000, height: 10 }, 1920, 1080))
      .toEqual({ x: 0, y: 0.94, width: 1, height: 0.06 });
  });
  it("uses scaled visual bounds for live pixels and persisted normalized geometry", () => {
    const pixels = clampOverlayModulePixelGeometry({ x: 900, y: 700, width: 300, height: 200 }, 1_000, 800, 2);
    expect(pixels).toEqual({ x: 400, y: 400, width: 300, height: 200 });
    expect(normalizedModuleGeometry(pixels, 1_000, 800, 2))
      .toEqual({ x: .4, y: .5, width: .3, height: .25 });
  });
  it("clamps presentation without changing module identity", () => {
    expect(clampOverlayModule({ x: -1, y: 2, width: .2, height: .2, visible: true, zOrder: 9999, opacity: 0, backgroundOpacity: 2, scale: 4 }))
      .toEqual({ x: 0, y: .6, width: .2, height: .2, visible: true, zOrder: 1000, opacity: .2, backgroundOpacity: 1, scale: 2 });
  });
  it("rejects incomplete setup payloads", () => {
    expect(() => parseOverlayLayoutSettings({ schemaVersion: 2, revision: 0, canvasEnabled: false, selectedSetupId: "default", legacyMigrationComplete: true,
      setups: { default: { name: "Default", locked: false, modules: {} } } })).toThrow("Invalid overlay module layout");
  });
  it("migrates saved modules without a background preference to transparent", () => {
    const settings = safeDefaultOverlayLayout(2);
    const serialized = structuredClone(settings) as unknown as { setups: Record<string, { modules: Record<string, Record<string, unknown>> }> };
    for (const module of Object.values(serialized.setups.default!.modules)) delete module.backgroundOpacity;
    const migrated = parseOverlayLayoutSettings(serialized);
    expect(Object.values(migrated.setups.default!.modules).every((module) => module.backgroundOpacity === 0)).toBe(true);
  });
  it("renormalizes every layer before raising past the cap", () => {
    const setup = activeOverlaySetup(safeDefaultOverlayLayout(4));
    setup.modules.alerts.zOrder = 1000;
    expect(raiseOverlayModule(setup, "map")).toBe(6);
    expect(new Set(Object.values(setup.modules).map((module) => module.zOrder)).size).toBe(7);
    expect(setup.modules.map.zOrder).toBe(Math.max(...Object.values(setup.modules).map((module) => module.zOrder)));
  });
});
