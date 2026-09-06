import { describe, expect, it } from "vitest";
import { parseMechanicsMapUpdate, projectCoralMatrixBeam, projectCoralPizzaRegions, projectCoralWaveRegion, projectCursedTombChargeRegion, projectMechanicsMapEntities, projectMechanicsMapPoint, projectRaidFloorRegions, projectTinaPizzaRegion, zoomMechanicsMapAt, type MechanicsMapSignal, type MechanicsMapSnapshot } from "./mechanics-map";

function snapshot(): MechanicsMapSnapshot {
  return {
    schema_version: 1, revision: 3, session_id: "s", client_build: "global/steam-24687926",
    scene_id: 6615, map_id: 6615, scene_name: null, map_model: "player_relative_radar", map_layout: null,
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
  it("zooms around the cursor without imposing a product cap", () => {
    const zoomedIn = zoomMechanicsMapAt({ scale: 1, panX: 0, panY: 0 }, 100, 50, -10_000);
    expect(zoomedIn.scale).toBeGreaterThan(1_000_000);
    expect((100 - zoomedIn.panX) / zoomedIn.scale).toBeCloseTo(100);
    expect((50 - zoomedIn.panY) / zoomedIn.scale).toBeCloseTo(50);

    const zoomedOut = zoomMechanicsMapAt({ scale: 1, panX: 0, panY: 0 }, 0, 0, 10_000);
    expect(zoomedOut.scale).toBeLessThan(0.000001);
    expect(zoomedOut.scale).toBeGreaterThan(0);
  });

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

  it("projects Tina's packet-facing pizza wedge without guessing a safe sector", () => {
    const value = {
      ...snapshot(),
      scene_id: 1632,
      map_model: "absolute_scene_map" as const,
      map_origin_x: -20,
      map_origin_z: -20,
      map_span_x: 40,
      map_span_z: 40,
    };
    const pizza = {
      ...value.entities[1]!,
      kind: "monster" as const,
      monster_id: 300086,
      mechanic_role: "pizza_slow" as const,
      x: 0,
      z: 0,
      facing_radians: 0,
    };
    const wedge = projectTinaPizzaRegion(value, pizza);
    expect(wedge).toHaveLength(10);
    expect(wedge[0]).toMatchObject({ mapX: 50, mapY: 50 });
    expect(wedge[5]?.mapX).toBeCloseTo(50);
    expect(wedge[5]?.mapY).toBeCloseTo(10);
    expect(projectTinaPizzaRegion(value, { ...pizza, facing_radians: null })).toEqual([]);
  });

  it("projects Coral's packet-oriented safe wave band", () => {
    const value = {
      ...snapshot(), scene_id: 6565, map_model: "absolute_scene_map" as const,
      map_origin_x: -400, map_origin_z: 0, map_span_x: 200, map_span_z: 200,
    };
    const wave = {
      ...value.entities[1]!, kind: "monster" as const, monster_id: 3340219,
      mechanic_role: "ice_wave" as const, x: -330, z: 101, facing_radians: 0,
    };
    const vertical = projectCoralWaveRegion(value, wave);
    expect(vertical).toHaveLength(4);
    expect(vertical[0]?.mapX).toBeCloseTo(34);
    expect(vertical[1]?.mapX).toBeCloseTo(36);
    const horizontal = projectCoralWaveRegion(value, { ...wave, facing_radians: Math.PI / 2 });
    expect(horizontal[0]?.mapX).toBeCloseTo(18.5);
    expect(horizontal[1]?.mapX).toBeCloseTo(51.5);
  });

  it("projects Coral's matrix callout beam from its proven source", () => {
    const value = {
      ...snapshot(), scene_id: 6563, map_model: "absolute_scene_map" as const,
      map_origin_x: -100, map_origin_z: -100, map_span_x: 200, map_span_z: 200,
      entities: [
        { ...snapshot().entities[0]!, actor_id: 10, mechanic_role: "matrix_rune" as const, x: 0, z: 0 },
        { ...snapshot().entities[1]!, actor_id: 11, x: 3, z: 4 },
      ],
    };
    const signal: MechanicsMapSignal = {
      effect_id: 522602, mechanic_kind: "matrix_callout", presentation_name: null,
      instance_id: null, target_actor_id: 11, source_actor_id: 10, stacks: null,
      duration_millis: 5_000, origin_x: null, origin_z: null, facing_radians: null, applied_at_micros: 1,
    };
    const beam = projectCoralMatrixBeam(value, signal);
    expect(beam).toHaveLength(2);
    expect(beam[1]?.mapX).toBeCloseTo(64.4);
    expect(beam[1]?.mapY).toBeCloseTo(30.8);
  });

  it("projects Coral's two opposing pizza sectors and honors the purple offset", () => {
    const base = {
      ...snapshot(), scene_id: 6565, map_model: "absolute_scene_map" as const,
      map_origin_x: -30, map_origin_z: -30, map_span_x: 60, map_span_z: 60,
    };
    const cast: MechanicsMapSignal = {
      effect_id: -3340245, mechanic_kind: "pizza_indicator", presentation_name: null,
      instance_id: null, target_actor_id: 2, source_actor_id: 2, stacks: null,
      duration_millis: null, origin_x: 0, origin_z: 0, facing_radians: 0, applied_at_micros: 1,
    };
    const orange = projectCoralPizzaRegions({ ...base, mechanics: [cast, { ...cast, effect_id: 883633, mechanic_kind: "pizza_orange" }] });
    expect(orange).toHaveLength(2);
    expect(orange[0]?.kind).toBe("pizza_orange");
    expect(orange[0]?.points[7]?.mapY).toBeCloseTo(16.6666666667);
    const purple = projectCoralPizzaRegions({ ...base, mechanics: [cast, { ...cast, effect_id: 883634, mechanic_kind: "pizza_purple" }] });
    expect(purple[0]?.kind).toBe("pizza_purple");
    expect(purple[0]?.points[7]?.mapX).toBeCloseTo(83.3333333333);
  });

  it("projects the raid's edge and corner patterns onto the true 3x3 floor", () => {
    const base = {
      ...snapshot(), scene_id: 13023, map_model: "absolute_scene_map" as const, map_layout: "raid_grid" as const,
      map_origin_x: -30, map_origin_z: -27, map_span_x: 60, map_span_z: 54,
    };
    const signal = (effect_id: number, mechanic_kind: string): MechanicsMapSignal => ({
      effect_id, mechanic_kind, presentation_name: null, instance_id: null, target_actor_id: 1, source_actor_id: null,
      stacks: null, duration_millis: 5_000, origin_x: null, origin_z: null, facing_radians: null, applied_at_micros: 1,
    });
    const edge = projectRaidFloorRegions({ ...base, mechanics: [signal(829214, "phase_edge")] });
    expect(edge).toHaveLength(4);
    expect(edge.map((region) => region.kind)).toEqual(["phase_edge", "phase_edge", "phase_edge", "phase_edge"]);
    const corner = projectRaidFloorRegions({ ...base, mechanics: [signal(829215, "phase_corner")] });
    expect(corner).toHaveLength(4);
    expect(corner[0]?.points[0]?.mapX).toBeCloseTo(0);
    expect(corner[0]?.points[0]?.mapY).toBeCloseTo(36.11111111111111);
    expect(projectRaidFloorRegions({ ...base, map_layout: "raid_ring" })).toEqual([]);
  });
});
