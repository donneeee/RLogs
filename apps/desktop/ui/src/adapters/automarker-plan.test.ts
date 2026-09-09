import { describe, expect, it } from "vitest";
import { planAutomarkerPreview, type AutomarkerRule } from "./automarker-plan";
import type { MechanicsMapSignal, MechanicsMapSnapshot } from "./mechanics-map";

function signal(actorId: number, at: number): MechanicsMapSignal {
  return {
    effect_id: 884_162, mechanic_kind: "sticky_bomb", presentation_name: "Sticky Bomb",
    instance_id: actorId, target_actor_id: actorId, source_actor_id: 99, stacks: null,
    duration_millis: 10_000, origin_x: null, origin_z: null, facing_radians: null,
    applied_at_micros: at,
  };
}

function snapshot(): MechanicsMapSnapshot {
  const player = {
    actor_id: 20, entity_uuid: 200, display_name: "Local", current_hp: 1, max_hp: 1,
    hp_percent: 100, current_shield: null, max_shield: null, shield_percent: null,
    dead: false, stale: false, statuses: [],
  } as const;
  return {
    schema_version: 13, revision: 1, session_id: "session", client_build: "24687926",
    scene_id: 1150, map_id: 1150, scene_name: "Void Towering Ruin",
    map_model: "absolute_scene_map", map_layout: null, world_radius: 140,
    map_origin_x: 0, map_origin_z: 0, map_span_x: 100, map_span_z: 100,
    background_asset_url: "/map.png", local_actor_id: 20, local_position_observed: true,
    player, party: [{ ...player, actor_id: 10, entity_uuid: 100, display_name: "Remote" }],
    action_controls: [], resources: [], encounter_pack: "void-towering-ruin",
    encounter_pack_reviewed: true, target: null, dungeon: null, entities: [],
    mechanics: [signal(20, 20), signal(10, 10), signal(99, 30)], markers: [],
    data_gap: null, last_event_sequence: 4, last_observed_micros: 30,
  };
}

describe("automarker placement planning", () => {
  it("builds a deterministic preview only from reviewed packet targets in the roster", () => {
    const rules: readonly AutomarkerRule[] = [{
      reviewState: "reviewed_current_build",
      clientBuild: "24687926", sceneIds: [1150], mechanicKind: "sticky_bomb",
      labels: ["B1", "B2"], maxTargets: 2,
    }];
    const plan = planAutomarkerPreview(snapshot(), rules);
    expect(plan.nativePlacement).toEqual({
      supported: false, reason: "native_party_marker_protocol_unverified",
    });
    expect(plan.assignments.map(({ actorId, label }) => ({ actorId, label }))).toEqual([
      { actorId: 10, label: "B1" }, { actorId: 20, label: "B2" },
    ]);
  });

  it("fails closed across build, scene, encounter-review and packet-gap boundaries", () => {
    const value = snapshot();
    expect(planAutomarkerPreview({ ...value, client_build: "24687927" }).assignments).toEqual([]);
    expect(planAutomarkerPreview({ ...value, scene_id: 1153 }).assignments).toEqual([]);
    expect(planAutomarkerPreview({ ...value, encounter_pack_reviewed: false }).assignments).toEqual([]);
    expect(planAutomarkerPreview({ ...value, data_gap: "lost packet" }).assignments).toEqual([]);
  });

  it("deduplicates repeated observations of the same target", () => {
    const value = snapshot();
    value.mechanics = [signal(20, 10), signal(20, 20)];
    expect(planAutomarkerPreview(value).assignments).toEqual([
      expect.objectContaining({ actorId: 20, label: "BOMB", appliedAtMicros: 20 }),
    ]);
  });
});
