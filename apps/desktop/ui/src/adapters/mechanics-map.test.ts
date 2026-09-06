import { describe, expect, it } from "vitest";
import { parseMechanicsMapUpdate, projectCursedTombChargeRegion, projectMechanicsMapEntities, projectMechanicsMapPoint, type MechanicsMapSignal, type MechanicsMapSnapshot } from "./mechanics-map";

function snapshot(): MechanicsMapSnapshot {
  return {
    schema_version: 1, revision: 3, session_id: "s", client_build: "global/steam-24687926",
    scene_id: 6615, map_id: 6615, scene_name: null, map_model: "player_relative_radar",
    world_radius: 140, background_asset_url: "/local-game-assets/global/steam-24687926/dungeon_map_bg.png",
    map_origin_x: null, map_origin_z: null, map_span_x: null, map_span_z: null,
    local_actor_id: 1, local_position_observed: true, encounter_pack: "Wasteland encounter",
    encounter_pack_reviewed: true, mechanics: [], markers: [], data_gap: null,
    last_event_sequence: 4, last_observed_micros: 4_000,
    entities: [
      { actor_id: 1, entity_uuid: 1, kind: "local", display_name: "Me", monster_id: null, mechanic_role: null, x: 10, y: 0, z: 10, facing_radians: 0, dead: false, stale: false, last_observed_micros: 4_000 },
      { actor_id: 2, entity_uuid: 2, kind: "boss", display_name: "Boss", monster_id: 4701, mechanic_role: "boss", x: 150, y: 0, z: 10, facing_radians: null, dead: false, stale: false, last_observed_micros: 4_000 },
    ],
  };
}

describe("Mechanics Map", () => {
  it("accepts the bounded host contract", () => {
    expect(parseMechanicsMapUpdate({ schema_version: 1, revision: 3, snapshot: snapshot() }).snapshot.scene_id).toBe(6615);
  });

  it("uses the exact player-relative 140-unit projection", () => {
    const projected = projectMechanicsMapEntities(snapshot(), false);
    expect(projected[0]).toMatchObject({ mapX: 50, mapY: 50, visible: true });
    expect(projected[1]).toMatchObject({ mapX: 100, mapY: 50, visible: true });
  });

  it("fails closed when no local position was joined", () => {
    expect(projectMechanicsMapEntities({ ...snapshot(), local_actor_id: null }, true)).toEqual([]);
  });

  it("projects an absolute scene map from game-owned region data", () => {
    const value = {
      ...snapshot(),
      map_model: "absolute_scene_map" as const,
      map_origin_x: -149,
      map_origin_z: -377,
      map_span_x: 450,
      map_span_z: 450,
    };
    const projected = projectMechanicsMapEntities(value, true);
    expect(projected[0]?.mapX).toBeCloseTo(35.333333333333336);
    expect(projected[0]?.mapY).toBeCloseTo(14);
    expect(projected[0]?.visible).toBe(true);
    const bossArena = projectMechanicsMapPoint(value, 69, -307, false);
    expect(bossArena?.mapX).toBeCloseTo(48.44444444444444);
    expect(bossArena?.mapY).toBeCloseTo(84.44444444444444);
    expect(bossArena?.visible).toBe(true);
  });

  it("clips reviewed clone charges to the correct half of the Cursed Tomb arena", () => {
    const value = {
      ...snapshot(),
      scene_id: 6513,
      map_model: "absolute_scene_map" as const,
      map_origin_x: -149,
      map_origin_z: -377,
      map_span_x: 450,
      map_span_z: 450,
    };
    const signal: MechanicsMapSignal = {
      effect_id: -3390117, mechanic_kind: "clone_charge_left", presentation_name: null,
      instance_id: null, target_actor_id: 2, source_actor_id: 2, stacks: null,
      duration_millis: 10_000, origin_x: 69, origin_z: -307, facing_radians: 0,
      applied_at_micros: 1,
    };
    const left = projectCursedTombChargeRegion(value, signal);
    const right = projectCursedTombChargeRegion(value, { ...signal, effect_id: -3390118, mechanic_kind: "clone_charge_right" });
    expect(left).toHaveLength(4);
    expect(right).toHaveLength(4);
    expect(Math.max(...left.map((point) => point.mapX))).toBeCloseTo(48.44444444444444);
    expect(Math.min(...right.map((point) => point.mapX))).toBeCloseTo(48.44444444444444);
  });
});
