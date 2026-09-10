import { describe, expect, it } from "vitest";
import { parseAutomarkerLoadResult, parseAutomarkerPresetView } from "./automarker-presets";

function view() {
  return {
    schemaVersion: 1,
    context: { clientBuild: "24687926", sceneId: 1100, mapId: 1100, sceneName: "Mech Facility M1" },
    presets: [{
      presetId: "preset-000000000001-0000", name: "Opener", clientBuild: "24687926",
      sceneId: 1100, mapId: 1100, savedAtUnixMillis: 1,
      points: [{ markerNumber: 1, x: 1.25, y: 2.5, z: -4.75 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_request_unverified",
  };
}

describe("automarker preset catalog", () => {
  it("accepts exact scene-scoped filesystem preset views", () => {
    expect(parseAutomarkerPresetView(view()).presets[0]?.points[0]?.y).toBe(2.5);
  });

  it("fails closed when the native host leaks a preset from another scene", () => {
    const value = view();
    value.presets[0]!.sceneId = 1200;
    expect(() => parseAutomarkerPresetView(value)).toThrow(/another scene/i);
  });

  it("keeps captured build as provenance across patches in the same scene", () => {
    const value = view();
    value.context.clientBuild = "24699999";
    expect(parseAutomarkerPresetView(value).presets).toHaveLength(1);
  });

  it("accepts only the explicitly locked native load response", () => {
    expect(parseAutomarkerLoadResult({ supported: false, reason: "native_waymark_request_unverified" })).toEqual({
      supported: false, reason: "native_waymark_request_unverified",
    });
    expect(() => parseAutomarkerLoadResult({ supported: true })).toThrow();
  });
});
