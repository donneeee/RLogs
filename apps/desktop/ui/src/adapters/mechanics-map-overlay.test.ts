import { describe, expect, it } from "vitest";
import type { UiLocalizer } from "../localization/ui-locale";
import { automarkerResponseIsCurrent } from "./automarker-presets";

import { automarkerPresetViewSnapshotKey, formatDungeonAttemptTime, formatDungeonObjectiveValue, mechanicsMapAssetAvailability, mechanicsMapAutomarkerSnapshotKey, mechanicsMapMarkerLabel, mechanicsMapReadabilityProfile, nextMapDim, parseMechanicsMapCanvasPreferences, projectAutomarkerPreviewMarkers, shouldRenderMechanicsMapUpdate } from "./mechanics-map-overlay";
import type { MechanicsMapSnapshot } from "./mechanics-map";

describe("Mechanics Map overlay canvas preferences", () => {
  it("rejects stale automarker responses after a newer request or snapshot transition", () => {
    const master = mechanicsMapAutomarkerSnapshotKey({ client_build: "24687926", scene_id: 1633, map_id: 1633 });
    const normal = mechanicsMapAutomarkerSnapshotKey({ client_build: "24687926", scene_id: 1631, map_id: 1631 });
    expect(automarkerResponseIsCurrent(7, 7, master, master)).toBe(true);
    expect(automarkerResponseIsCurrent(6, 7, master, master)).toBe(false);
    expect(automarkerResponseIsCurrent(7, 7, master, normal)).toBe(false);
    expect(automarkerPresetViewSnapshotKey({
      schemaVersion: 4,
      context: { clientBuild: "24687926", sceneId: 1631, mapId: 1631, activityFamilyId: "tina-mindrealm", sceneName: "Tina" },
      presets: [], captureSupported: false, captureReason: "native_waymark_state_unverified",
      captureSessionId: null, deploymentId: null, protocolPackDigest: null,
      nativeLoadSupported: false, nativeLoadReason: "native_waymark_transport_unavailable",
      previewSessionId: "preview-test-session",
    })).toBe(normal);
  });

  it("prefers the visible numbered marker label", () => {
    expect(mechanicsMapMarkerLabel({ marker_id: null, marker_number: 4 })).toBe("4");
    expect(mechanicsMapMarkerLabel({ marker_id: 77, marker_number: null })).toBe("77");
    expect(mechanicsMapMarkerLabel({ marker_id: null, marker_number: null })).toBeNull();
  });
  it("projects manual preview markers with large, explicit numeric labels on the real map coordinates", () => {
    const snapshot = {
      map_model: "absolute_scene_map", map_origin_x: -50, map_origin_z: -50,
      map_span_x: 100, map_span_z: 100,
    } as MechanicsMapSnapshot;
    expect(projectAutomarkerPreviewMarkers(snapshot, [
      { markerNumber: 1, x: 0, y: 999, z: 0 },
      { markerNumber: 6, x: 25, y: -10, z: -25 },
    ], false)).toEqual([
      { markerNumber: 1, label: "1", mapX: 50, mapY: 50 },
      { markerNumber: 6, label: "6", mapX: 75, mapY: 75 },
    ]);
  });
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
      mapDim: 0.48,
      highContrastMechanics: false,
      showPlayer: false,
      showActions: true,
      showParty: false,
      showTarget: true,
      showObjectives: false,
      showAlerts: true,
      locked: true,
      expanded: true,
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
      alertsX: 1060,
      alertsY: 440,
      alertsWidth: 380,
    })).toEqual({
      scale: 37.5,
      panX: -184,
      panY: 92,
      rotateWithPlayer: false,
      showMonsters: false,
      mapDim: 0.48,
      highContrastMechanics: false,
      showPlayer: false,
      showActions: true,
      showParty: false,
      showTarget: true,
      showObjectives: false,
      showAlerts: true,
      locked: true,
      expanded: true,
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
      alertsX: 1060,
      alertsY: 440,
      alertsWidth: 380,
    });
  });

  it("replaces malformed or unsafe persisted values with production defaults", () => {
    expect(parseMechanicsMapCanvasPreferences({
      scale: 0,
      panX: Number.POSITIVE_INFINITY,
      panY: 10_000_001,
      rotateWithPlayer: "yes",
      showMonsters: null,
      mapDim: 2,
      highContrastMechanics: "yes",
      showPlayer: "yes",
      showActions: null,
      showParty: 1,
      showTarget: [],
      showObjectives: {},
      showAlerts: "no",
      locked: 1,
      expanded: "yes",
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
      alertsX: Number.NEGATIVE_INFINITY,
      alertsY: Number.NaN,
      alertsWidth: 0,
    })).toEqual({
      scale: 1,
      panX: 0,
      panY: 0,
      rotateWithPlayer: true,
      showMonsters: true,
      mapDim: 0.32,
      highContrastMechanics: true,
      showPlayer: true,
      showActions: true,
      showParty: true,
      showTarget: true,
      showObjectives: true,
      showAlerts: true,
      locked: false,
      expanded: false,
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
      alertsX: 1040,
      alertsY: 420,
      alertsWidth: 360,
    });
  });

  it("shows exact packet values without inventing an objective threshold", () => {
    expect(formatDungeonObjectiveValue(275, false)).toBe("275");
    expect(formatDungeonObjectiveValue(400, true)).toBe("400 ✓");
    expect(formatDungeonObjectiveValue(275, false, 400)).toBe("275 / 400");
    expect(formatDungeonObjectiveValue(400, true, 400)).toBe("400 / 400 ✓");
    expect(formatDungeonObjectiveValue(null, false)).toBe("Observed");
  });

  it("uses the selected UI locale formatter for objective counts", () => {
    const localizer: UiLocalizer = {
      locale: "en-US",
      loadedLocales: ["en-US"],
      t: (key) => key,
      formatNumber: (value) => `[${value}]`,
    };
    expect(formatDungeonObjectiveValue(1_000, false, 2_000, localizer))
      .toBe("[1000] / [2000]");
  });

  it("formats the packet-bounded attempt clock with stable tenths", () => {
    expect(formatDungeonAttemptTime(0)).toBe("0:00.0");
    expect(formatDungeonAttemptTime(83_456_789)).toBe("1:23.4");
    expect(formatDungeonAttemptTime(3_723_456_789)).toBe("1:02:03.4");
  });

  it("cycles real-map dimming through bounded readability levels", () => {
    expect(nextMapDim(0)).toBe(0.16);
    expect(nextMapDim(0.16)).toBe(0.32);
    expect(nextMapDim(0.48)).toBe(0.64);
    expect(nextMapDim(0.64)).toBe(0);
  });

  it("strengthens mechanic foreground separation without hiding the real map", () => {
    expect(mechanicsMapReadabilityProfile(0.32, true)).toEqual({
      mapDim: 0.32,
      entityOutlineWidth: 4,
      entityGlowBlur: 14,
      labelHaloWidth: 5,
    });
    expect(mechanicsMapReadabilityProfile(0, false)).toEqual({
      mapDim: 0,
      entityOutlineWidth: 3,
      entityGlowBlur: 10,
      labelHaloWidth: 3,
    });
    expect(mechanicsMapReadabilityProfile(2, true).mapDim).toBe(0.8);
    expect(mechanicsMapReadabilityProfile(Number.NaN, true).mapDim).toBe(0.32);
  });

  it("renders live map pixels only when a reviewed game asset is loaded", () => {
    expect(mechanicsMapAssetAvailability({
      map_model: "absolute_scene_map", background_asset_url: "/local-game-assets/24687926/map.png",
    }, true)).toBe("ready");
    expect(mechanicsMapAssetAvailability({
      map_model: "absolute_scene_map", background_asset_url: "/local-game-assets/24687926/map.png",
    }, false)).toBe("asset_pending");
    expect(mechanicsMapAssetAvailability({
      map_model: "absolute_scene_map", background_asset_url: null,
    }, true)).toBe("unsupported");
    expect(mechanicsMapAssetAvailability({
      map_model: "player_relative_radar", background_asset_url: null,
    }, false)).toBe("unsupported");
  });
});
