import { describe, expect, it } from "vitest";

import {
  actorLabel,
  parseCombatTimelineSnapshot,
  sortCombatActors,
} from "./combat-meter";

describe("Combat Meter snapshot contract", () => {
  it("preserves exact identifiers and sorts a copy by meter metrics", () => {
    const value = fixture();
    const snapshot = parseCombatTimelineSnapshot(value);
    const sorted = sortCombatActors(snapshot.actors, "dps", "descending");

    expect(snapshot.actors[0]?.actor_id).toBe("18446744073709551615");
    expect(sorted.map((actor) => actor.display_name)).toEqual(["B", "A"]);
    expect(snapshot.actors.map((actor) => actor.display_name)).toEqual([
      "A",
      "B",
    ]);
    expect(actorLabel({ ...snapshot.actors[0]!, display_name: null })).toBe(
      "Entity UUID -9223372036854775808",
    );
  });

  it("rejects numeric identifiers before browser precision can be ambiguous", () => {
    const value = fixture();
    value.actors[0]!.actor_id = Number.MAX_SAFE_INTEGER + 1;

    expect(() => parseCombatTimelineSnapshot(value)).toThrow(
      "invalid or unsupported",
    );
  });

  it("rejects unsafe counters and non-finite measurements", () => {
    const unsafe = fixture();
    unsafe.active_combat_micros = Number.MAX_SAFE_INTEGER + 1;
    expect(() => parseCombatTimelineSnapshot(unsafe)).toThrow(
      "invalid or unsupported",
    );

    const infinite = fixture();
    infinite.actors[0]!.dps = Number.POSITIVE_INFINITY;
    expect(() => parseCombatTimelineSnapshot(infinite)).toThrow(
      "invalid or unsupported",
    );
  });

  it("masks stale provider credit when the exact-build proof gate is closed", () => {
    const value = fixture();
    value.rdps_status =
      "formula_runtime_blocked: exact-build promotion proof gates are incomplete";
    const ordinaryDamage = value.actors[0]!.reported_damage;

    const snapshot = parseCombatTimelineSnapshot(value);

    expect(snapshot.actors[0]).toMatchObject({
      reported_damage: ordinaryDamage,
      rdps_damage: null,
      rdps: null,
      rdps_contribution_given: null,
      rdps_contribution_received: null,
    });
    expect(value.actors[0]!.rdps_damage).not.toBeNull();
  });
});

function fixture() {
  return {
    schema_version: 5,
    session_id: "fixture",
    deployment_id: "global",
    region_id: "global",
    world_id: "asteria",
    client_build: "fixture-build",
    protocol_pack_digest: "a".repeat(64),
    rdps_status: "partial_packet_proven_rules",
    encounter_id: "fixture-encounter",
    encounter_state: "cleared",
    event_count: 12,
    data_gap_count: 0,
    combat_window_count: 1,
    combat_started_micros: 1_000_000,
    combat_ended_micros: 11_000_000,
    active_combat_micros: 10_000_000,
    run_elapsed_micros: 12_000_000,
    game_time_micros: null,
    true_time_micros: null,
    closed_at_log_end: false,
    actors: [
      actor("18446744073709551615", "A", 100),
      actor("2", "B", 200),
    ],
  };
}

function actor(actorId: string, name: string, dps: number) {
  return {
    actor_id: actorId as string | number,
    entity_uuid: "-9223372036854775808",
    display_name: name,
    actor_kind: "player",
    class_id: 1,
    specialization_id: 101,
    level: 60,
    seasonal_score: 3505,
    reported_damage: dps * 10,
    effective_damage: dps * 10,
    hp_damage: dps * 10,
    shield_damage: 0,
    damage_during_combat: dps * 10,
    damage_taken: dps * 2,
    dps,
    hps: dps / 4,
    tps: dps / 5,
    rdps_damage: dps * 10,
    rdps: dps,
    rdps_contribution_given: 0,
    rdps_contribution_received: 0,
    reported_healing: 0,
    effective_healing: 0,
    overheal: 0,
    shielding: 0,
    casts: 1,
    hits: 1,
    critical_hits: 0,
    deaths: 0,
    revives: 0,
    position_samples: 0,
    path_distance: 0,
    abilities: [
      {
        ability_id: "9223372036854775807",
        casts: 1,
        hits: 1,
        critical_hits: 0,
        reported_damage: dps * 10,
        effective_damage: dps * 10,
        reported_healing: 0,
        effective_healing: 0,
        shielding: 0,
      },
    ],
  };
}
