import { describe, expect, it } from "vitest";
import { claimAutomaticMapPreparation } from "./mechanics-map-surface";
import { actionControlRemainingMillis, fitMechanicsMapCanvasRect, mechanicSignalRemainingMillis, parseMechanicsMapUpdate, projectCoralMatrixBeam, projectCoralPizzaRegions, projectCoralWaveRegion, projectCursedTombChargeRegion, projectMechanicsMapEntities, projectMechanicsMapPoint, projectRaidFloorRegions, projectTinaPizzaRegion, projectVoidTowerMapAnnotations, targetDebuffRemainingMillis, zoomMechanicsMapAt, type MechanicsMapSignal, type MechanicsMapSnapshot } from "./mechanics-map";

function snapshot(): MechanicsMapSnapshot {
  return {
    schema_version: 13, revision: 3, session_id: "s", client_build: "24687926",
    scene_id: 6615, map_id: 6615, scene_name: null, map_model: "player_relative_radar", map_layout: null,
    world_radius: 140, background_asset_url: "/local-game-assets/24687926/dungeon_map_bg.png",
    map_origin_x: null, map_origin_z: null, map_span_x: null, map_span_z: null,
    local_actor_id: 1, local_position_observed: true,
    player: {
      actor_id: 1, entity_uuid: 1, display_name: "Me", current_hp: 900, max_hp: 1_000,
      hp_percent: 90, current_shield: 200, max_shield: 400, shield_percent: 50,
      dead: false, stale: false, statuses: [],
    },
    party: [{
      actor_id: 3, entity_uuid: 3, display_name: "Party", current_hp: 700, max_hp: 1_000,
      hp_percent: 70, current_shield: null, max_shield: null, shield_percent: null,
      dead: false, stale: false, statuses: [],
    }],
    action_controls: [{
      skill_level_id: 12_301, presentation_ability_id: 123, presentation_name: "Skill",
      icon_asset_path: "/game-assets/skill.png", duration_millis: 10_000,
      remaining_millis: 8_000, cooldown_type: 0, charge_count: 1, observed_at_micros: 2_000,
    }],
    resources: [{
      kind: "bar", label: "Energy", current_id: 14_011, max_id: 14_017,
      current: 72, max: 100, percent: 72,
    }],
    encounter_pack: "Wasteland encounter",
    encounter_pack_reviewed: true, target: null, dungeon: null, mechanics: [], markers: [], data_gap: null,
    last_event_sequence: 4, last_observed_micros: 4_000,
    entities: [
      { actor_id: 1, entity_uuid: 1, kind: "local", display_name: "Me", monster_id: null, mechanic_role: null, x: 10, y: 0, z: 10, facing_radians: 0, dead: false, stale: false, last_observed_micros: 4_000 },
      { actor_id: 2, entity_uuid: 2, kind: "boss", display_name: "Boss", monster_id: 4701, mechanic_role: "boss", x: 150, y: 0, z: 10, facing_radians: null, dead: false, stale: false, last_observed_micros: 4_000 },
    ],
  };
}

describe("Mechanics Map", () => {
  it("automatically prepares each missing exact-build map only once", () => {
    const attempts = new Set<string>();
    const map = "/local-game-assets/24687926/scene-6513-cursed-tomb.png";
    expect(claimAutomaticMapPreparation(map, attempts)).toBe(true);
    expect(claimAutomaticMapPreparation(map, attempts)).toBe(false);
    expect(claimAutomaticMapPreparation(`${map}?revision=2`, attempts)).toBe(true);
  });

  it("zooms around the cursor without imposing a product cap", () => {
    const zoomedIn = zoomMechanicsMapAt({ scale: 1, panX: 0, panY: 0 }, 100, 50, -10_000);
    expect(zoomedIn.scale).toBeGreaterThan(1_000_000);
    expect((100 - zoomedIn.panX) / zoomedIn.scale).toBeCloseTo(100);
    expect((50 - zoomedIn.panY) / zoomedIn.scale).toBeCloseTo(50);

    const zoomedOut = zoomMechanicsMapAt({ scale: 1, panX: 0, panY: 0 }, 0, 0, 10_000);
    expect(zoomedOut.scale).toBeLessThan(0.000001);
    expect(zoomedOut.scale).toBeGreaterThan(0);
  });

  it("letterboxes the complete game map without stretching its coordinate plane", () => {
    expect(fitMechanicsMapCanvasRect(1_200, 800, 4_096, 4_096)).toEqual({
      x: 200, y: 0, width: 800, height: 800,
    });
    expect(fitMechanicsMapCanvasRect(800, 1_200, 4_096, 4_096)).toEqual({
      x: 0, y: 200, width: 800, height: 800,
    });
    expect(fitMechanicsMapCanvasRect(1_200, 800, 1_200, 800)).toEqual({
      x: 0, y: 0, width: 1_200, height: 800,
    });
    expect(fitMechanicsMapCanvasRect(Number.NaN, 800, 4_096, 4_096)).toEqual({
      x: 0, y: 0, width: 0, height: 800,
    });
  });

  it("accepts the bounded host contract", () => {
    const parsed = parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: snapshot() }).snapshot;
    expect(parsed.scene_id).toBe(6615);
    expect(parsed.action_controls[0]?.skill_level_id).toBe(12_301);
    expect(parsed.resources[0]).toMatchObject({ label: "Energy", current: 72, max: 100 });
    expect(parsed.party[0]?.display_name).toBe("Party");
    expect(actionControlRemainingMillis(parsed.action_controls[0]!, 1_250)).toBe(6_750);
    expect(actionControlRemainingMillis(parsed.action_controls[0]!, 9_000)).toBe(0);
  });

  it("accepts bounded packet-backed player statuses", () => {
    const value = snapshot();
    value.player!.statuses = [{
      effect_id: 21_412, instance_id: 7, presentation_name: "Player buff",
      icon_asset_path: "/game-assets/buff.png", source_actor_id: 1,
      source_display_name: "Me", owned_by_local_player: true, stacks: 2, duration_millis: 10_000,
      remaining_millis: 8_000, applied_at_micros: 2_000,
    }];
    const player = parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: value }).snapshot.player;
    expect(player?.statuses[0]).toMatchObject({ effect_id: 21_412, stacks: 2 });
  });

  it("counts down only packet-duration mechanic effects", () => {
    const effect = {
      effect_id: 884_162, duration_millis: 10_000, applied_at_micros: 2_000_000,
    };
    expect(mechanicSignalRemainingMillis(effect, 4_000_000, 1_250)).toBe(6_750);
    expect(mechanicSignalRemainingMillis({ ...effect, effect_id: -3_390_117 }, 4_000_000, 1_250)).toBeNull();
    expect(mechanicSignalRemainingMillis({ ...effect, duration_millis: null }, 4_000_000, 1_250)).toBeNull();
  });

  it("accepts bounded packet-backed dungeon objectives", () => {
    const value = snapshot();
    value.dungeon = {
      dungeon_id: 6513,
      instance_id: "run-1",
      difficulty_id: 6545,
      state: "objective_updated",
      flow_phase: "playing",
      flow_state_id: 3,
      result_id: null,
      attempt_number: 1,
      retry_count: 0,
      encounter_state: "started",
      attempt_elapsed_micros: 2_000_000,
      attempt_running: true,
      objectives: [{
        objective_id: 651103,
        objective_map_key: 7,
        value: 275,
        complete: false,
        presentation_name: "Complete the spatial investigation to unlock the boss battle",
        required_count: 400,
        catalog_resolution: "unresolved_current_build",
        activity_target_key: null,
        scene_event_keys: [],
      }],
    };
    const dungeon = parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: value }).snapshot.dungeon;
    expect(dungeon?.objectives[0]).toMatchObject({ objective_id: 651103, value: 275, complete: false });
  });

  it("accepts exact target HP and bounded debuff presentation", () => {
    const value = snapshot();
    value.target = {
      actor_id: 2, entity_uuid: 2, display_name: "Boss", monster_id: 4701,
      current_hp: 500, max_hp: 1_000, hp_percent: 50,
      current_shield: 200, max_shield: 400, shield_percent: 50, breaking_stage: 0,
      dead: false, stale: false,
      debuffs: [{
        effect_id: 4501, instance_id: 99, presentation_name: "Burning", icon_asset_path: "/game-assets/debuff.png",
        source_actor_id: 1, source_display_name: "Me", owned_by_local_player: true, stacks: 2, duration_millis: 5_000,
        remaining_millis: 4_500, applied_at_micros: 4_000,
      }],
    };
    const target = parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: value }).snapshot.target;
    expect(target?.hp_percent).toBe(50);
    expect(target?.debuffs[0]?.source_display_name).toBe("Me");
    expect(target?.current_shield).toBe(200);
    expect(target?.shield_percent).toBe(50);
    expect(target?.breaking_stage).toBe(0);
    expect(targetDebuffRemainingMillis(target!.debuffs[0]!, 1_250)).toBe(3_250);
    expect(targetDebuffRemainingMillis(target!.debuffs[0]!, 9_000)).toBe(0);
  });

  it("rejects impossible negative target-effect durations", () => {
    const value = snapshot();
    value.target = {
      actor_id: 2, entity_uuid: 2, display_name: "Boss", monster_id: 4701,
      current_hp: 500, max_hp: 1_000, hp_percent: 50,
      current_shield: null, max_shield: null, shield_percent: null, breaking_stage: null,
      dead: false, stale: false,
      debuffs: [{
        effect_id: 4501, instance_id: 99, presentation_name: "Burning", icon_asset_path: null,
        source_actor_id: null, source_display_name: null, owned_by_local_player: false, stacks: null, duration_millis: 5_000,
        remaining_millis: -1, applied_at_micros: 4_000,
      }],
    };
    expect(() => parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: value })).toThrow();
  });

  it("rejects an unbounded party frame payload", () => {
    const value = snapshot();
    value.party = Array.from({ length: 40 }, (_, index) => ({
      ...value.party[0]!, actor_id: index + 10, entity_uuid: index + 10,
    }));
    expect(() => parseMechanicsMapUpdate({ schema_version: 13, revision: 3, snapshot: value })).toThrow();
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

  it("projects reviewed Void Tower portal roles and sticky targets without geometry", () => {
    const value: MechanicsMapSnapshot = {
      ...snapshot(),
      scene_id: 1151,
      map_id: 1151,
      map_model: "absolute_scene_map",
      map_origin_x: 0,
      map_origin_z: 0,
      map_span_x: 100,
      map_span_z: 100,
      entities: [
        { ...snapshot().entities[0]!, actor_id: 10, x: 25, z: 75 },
        { ...snapshot().entities[1]!, actor_id: 11, mechanic_role: "correct_portal", x: 40, z: 60 },
        { ...snapshot().entities[1]!, actor_id: 12, mechanic_role: "other_portal", x: 70, z: 20 },
        { ...snapshot().entities[1]!, actor_id: 13, mechanic_role: null, x: 50, z: 50 },
      ],
      mechanics: [{
        effect_id: 821076,
        mechanic_kind: "sticky_bomb",
        presentation_name: "Sticky bomb",
        instance_id: 9,
        target_actor_id: 13,
        source_actor_id: 11,
        stacks: null,
        duration_millis: 8_000,
        origin_x: null,
        origin_z: null,
        facing_radians: null,
        applied_at_micros: 1,
      }],
    };

    expect(projectVoidTowerMapAnnotations(value)).toEqual([
      { kind: "correct_portal", actorId: 11, mapX: 40, mapY: 40, visible: true },
      { kind: "other_portal", actorId: 12, mapX: 70, mapY: 80, visible: true },
      { kind: "sticky_bomb_target", actorId: 13, mapX: 50, mapY: 50, visible: true },
    ]);
    expect(projectVoidTowerMapAnnotations({ ...value, client_build: "24687927" })).toEqual([]);
    expect(projectVoidTowerMapAnnotations({
      ...value,
      entities: value.entities.map((entity) => entity.actor_id === 13 ? { ...entity, stale: true } : entity),
    })).not.toContainEqual(expect.objectContaining({ kind: "sticky_bomb_target" }));
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
