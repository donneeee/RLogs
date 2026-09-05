import { describe, expect, it } from "vitest";
import { parseMechanicsMapUpdate, projectMechanicsMapEntities, type MechanicsMapSnapshot } from "./mechanics-map";

function snapshot(): MechanicsMapSnapshot {
  return {
    schema_version: 1, revision: 3, session_id: "s", client_build: "global/steam-24687926",
    scene_id: 6615, map_id: 6615, scene_name: null, map_model: "player_relative_radar",
    world_radius: 140, background_asset_url: "/local-game-assets/global/steam-24687926/dungeon_map_bg.png",
    local_actor_id: 1, local_position_observed: true, encounter_pack: "Wasteland encounter",
    encounter_pack_reviewed: true, mechanics: [], markers: [], data_gap: null,
    last_event_sequence: 4, last_observed_micros: 4_000,
    entities: [
      { actor_id: 1, entity_uuid: 1, kind: "local", display_name: "Me", monster_id: null, x: 10, y: 0, z: 10, facing_radians: 0, dead: false, stale: false, last_observed_micros: 4_000 },
      { actor_id: 2, entity_uuid: 2, kind: "boss", display_name: "Boss", monster_id: 4701, x: 150, y: 0, z: 10, facing_radians: null, dead: false, stale: false, last_observed_micros: 4_000 },
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
});
