import { describe, expect, it } from "vitest";
import {
  captureWaymarkPreset, readWaymarkPresets, requestNativeWaymarkLoad,
  storeWaymarkPreset, WAYMARK_PRESET_STORAGE_KEY,
} from "./waymark-presets";
import type { MechanicsMapSnapshot } from "./mechanics-map";

function snapshot(): MechanicsMapSnapshot {
  return {
    schema_version: 14, revision: 1, session_id: "s", client_build: "24687926",
    scene_id: 1100, map_id: 1100, scene_name: "Mech Facility M1",
    map_model: "absolute_scene_map", map_layout: null, world_radius: 100,
    map_origin_x: 0, map_origin_z: 0, map_span_x: 100, map_span_z: 100,
    background_asset_url: null, local_actor_id: null, local_position_observed: false,
    player: null, party: [], action_controls: [], resources: [], encounter_pack: null,
    encounter_pack_reviewed: false, target: null, dungeon: null, entities: [], mechanics: [],
    markers: [
      { marker_id: null, marker_number: 2, related_actor_id: null, x: 20, y: null, z: 22 },
      { marker_id: null, marker_number: 1, related_actor_id: null, x: 10, y: 1, z: 11 },
    ],
    data_gap: null, last_event_sequence: 1, last_observed_micros: 1,
  };
}

describe("waymark presets", () => {
  it("captures numbered ground markers in stable number order", () => {
    const preset = captureWaymarkPreset(snapshot(), " M1 opener ", "2026-09-09T00:00:00.000Z");
    expect(preset.name).toBe("M1 opener");
    expect(preset.points.map((point) => point.markerNumber)).toEqual([1, 2]);
  });

  it("persists bounded presets and replaces the same scene/name", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    const first = captureWaymarkPreset(snapshot(), "M1", "2026-09-09T00:00:00.000Z");
    storeWaymarkPreset(storage, first);
    storeWaymarkPreset(storage, { ...first, savedAt: "2026-09-09T00:01:00.000Z" });
    expect(readWaymarkPresets(storage)).toHaveLength(1);
    expect(values.has(WAYMARK_PRESET_STORAGE_KEY)).toBe(true);
  });

  it("fails closed on corrupt storage and native placement", () => {
    expect(readWaymarkPresets({ getItem: () => "not json" })).toEqual([]);
    expect(requestNativeWaymarkLoad(captureWaymarkPreset(snapshot(), "M1"))).toEqual({
      supported: false, reason: "native_waymark_request_unverified",
    });
  });
});
