import { describe, expect, it } from "vitest";
import {
  automarkerSaveRequest,
  newlyCreatedPresetId,
  parseAutomarkerLoadResult,
  parseAutomarkerPresetView,
} from "./automarker-presets";
import { automarkerPresetContextKey } from "./automarker-presets-surface";

function view() {
  return {
    schemaVersion: 2 as const,
    context: { clientBuild: "24687926", sceneId: 1633, mapId: 1633, activityFamilyId: "tina-mindrealm", sceneName: "Tina M1" },
    presets: [{
      presetId: "preset-000000000001-0000", name: "Opener", clientBuild: "24687926",
      sceneId: 1633, mapId: 1633, activityFamilyId: "tina-mindrealm", savedAtUnixMillis: 1,
      points: [{ markerNumber: 1, x: 1.25, y: 2.5, z: -4.75 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_request_unverified",
  };
}

describe("automarker preset catalog", () => {
  it("refreshes scene provenance when difficulty or build changes within a family", () => {
    const original = view();
    const difficultyChange = view();
    difficultyChange.context.sceneId = 1631;
    difficultyChange.context.mapId = 1631;
    const buildChange = view();
    buildChange.context.clientBuild = "24699999";

    expect(automarkerPresetContextKey(difficultyChange)).not.toBe(automarkerPresetContextKey(original));
    expect(automarkerPresetContextKey(buildChange)).not.toBe(automarkerPresetContextKey(original));
  });

  it("accepts exact scene-scoped filesystem preset views", () => {
    expect(parseAutomarkerPresetView(view()).presets[0]?.points[0]?.y).toBe(2.5);
  });

  it("allows another difficulty scene in the same reviewed dungeon family", () => {
    const value = view();
    value.context.sceneId = 1631;
    value.context.mapId = 1631;
    expect(parseAutomarkerPresetView(value).presets).toHaveLength(1);
  });

  it("fails closed when the native host leaks a preset from another dungeon family", () => {
    const value = view();
    value.presets[0]!.activityFamilyId = "mech-facility";
    expect(() => parseAutomarkerPresetView(value)).toThrow(/another dungeon family/i);
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

  it("distinguishes Save overwrite from Save As creation", () => {
    expect(automarkerSaveRequest("save", "preset-existing", " Adjusted ")).toEqual({
      presetId: "preset-existing", name: "Adjusted",
    });
    expect(automarkerSaveRequest("save-as", "preset-existing", "Alternate")).toEqual({
      presetId: null, name: "Alternate",
    });
    expect(() => automarkerSaveRequest("save", null, "Missing")).toThrow(/choose an existing/i);
  });

  it("selects the distinct ID returned by Save As", () => {
    const next = view();
    next.presets.unshift({ ...next.presets[0]!, presetId: "preset-new-distinct", name: "Alternate" });
    expect(newlyCreatedPresetId(new Set(["preset-000000000001-0000"]), parseAutomarkerPresetView(next)))
      .toBe("preset-new-distinct");
  });
});
