import { describe, expect, it } from "vitest";

import {
  parseCombatHistoryCatalog,
  parseCombatHistoryDeleteResult,
  parseCombatHistorySnapshot,
} from "./combat-history";

describe("combat history contracts", () => {
  it("accepts a lightweight indexed run and exact active-time view", () => {
    const catalog = parseCombatHistoryCatalog({
      schema_version: 1,
      entries: [
        {
          history_id: "run-1:0",
          session_id: "run-1",
          run_index: 0,
          captured_unix_millis: 1,
          activity_id: "scene.1632",
          activity_family_id: "tina-mindrealm",
          scene_id: 1632,
          presentation_scene_name: "Tina Mindrealm",
          difficulty_family: "hard",
          difficulty_tier: null,
          terminal_state: "completed",
          game_time_micros: 20_000_000,
          total_run_time_micros: 24_000_000,
          active_combat_micros: 10_000_000,
          player_count: 5,
          deployment_id: "global",
          region_id: "north-america",
          world_id: "asteria",
          team_damage: 100_000,
          team_dps: 5_000,
          team_encounter_dps: 10_000,
          true_time_micros: 18_000_000,
          retry_count: 1,
          boss_retry_count: 1,
          participants: [
            {
              actor_id: "8",
              entity_uuid: "216009015936",
              display_name: null,
              actor_kind: "player",
              class_id: 11,
              specialization_id: 116,
              presentation_class_name: "Marksman",
              presentation_specialization_name: "Wildpack Spec",
              level: 2,
              seasonal_score: 3505,
              primary_loadout: [
                {
                  slot_id: 7,
                  ability_id: 3948,
                  item_id: 3000101,
                  tier: 5,
                  presentation_name: "Battle Imagine - Rorola",
                  icon_asset_path: "/game-assets/blue-protocol-star-resonance/shared/icons/imagines/battle/3000101-rorola.png",
                  item_tier: 4,
                  maximum_tier: 5,
                },
              ],
              auxiliary_loadout: [],
              damage: 100_000,
              dps: 5_000,
              encounter_dps: 10_000,
              character_id: "3296036",
              presentation_name: "MarieRose",
              presentation_kind: "player",
              icon_asset_path: "/game-assets/blue-protocol-star-resonance/shared/icons/classes/marksman/horizontal.png",
              presentation_role: "damage",
              presentation_accent: null,
            },
          ],
        },
      ],
    });
    expect(catalog.entries[0]?.difficulty_family).toBe("hard");
    expect(catalog.entries[0]?.is_favorite).toBe(false);
    expect(catalog.entries[0]?.presentation_scene_name).toBe("Tina Mindrealm");
    expect(catalog.entries[0]?.participants[0]?.primary_loadout[0]?.item_id).toBe(3000101);

    const snapshot = parseCombatHistorySnapshot({
      schema_version: 1,
      session_id: "run-1",
      deployment_id: "global",
      region_id: "north-america",
      world_id: "asteria",
      client_build: "1",
      protocol_pack_digest: "sha256:test",
      rdps_formula_identity: "sha256:fixture-formula",
      runs: [
        {
          run_index: 0,
          rdps_status: "partial_packet_proven_rules",
          true_time_micros: null,
          retry_count: 0,
          boss_retry_count: 0,
          views: [
            {
              id: "all",
              label: "Entire run",
              elapsed_micros: 20_000_000,
              active_combat_micros: 10_000_000,
              actors: [
                {
                  rdps_damage: 9_500,
                  rdps_contribution_given: 500,
                  rdps_contribution_received: 1_000,
                  targets: [
                    {
                      actor_id: "2",
                      entity_uuid: "6818431040",
                      damage: 10_000,
                      effective_damage: 9_000,
                      hits: 2,
                      critical_hits: 1,
                      effect_events: 0,
                      series: [
                        {
                          second: 4,
                          damage: 10_000,
                          effective_healing: 0,
                          damage_taken: 500,
                        },
                      ],
                    },
                  ],
                  series: [],
                },
              ],
              targets: [
                {
                  actor_id: "2",
                  entity_uuid: "6818431040",
                  monster_id: "33701",
                  display_name: null,
                  actor_kind: "monster",
                  presentation_name: "Tina - Void Reverie",
                },
                {
                  actor_id: "9",
                  entity_uuid: "164352",
                  monster_id: "2",
                  display_name: null,
                  actor_kind: "pet",
                  presentation_name: "Pet 2",
                },
                {
                  actor_id: "10",
                  entity_uuid: "819904",
                  monster_id: "1256",
                  display_name: null,
                  actor_kind: "training_dummy",
                  presentation_name: "Training Dummy 1256",
                },
                {
                  actor_id: "11",
                  entity_uuid: "557440",
                  monster_id: "10100101",
                  display_name: null,
                  actor_kind: "projectile",
                  presentation_name: "Projectile 10100101",
                },
              ],
              damage_influences: [
                {
                  effect_id: "2202041",
                  provider_actor_id: "8",
                  provider_entity_uuid: "216009015936",
                  recipient_actor_id: "9",
                  recipient_entity_uuid: "216009015937",
                  affected_ability_id: "2233",
                  target_actor_id: "2",
                  target_entity_uuid: "6818431040",
                  first_observed_micros: 4_000_000,
                  last_observed_micros: 5_000_000,
                  damage_event_count: 2,
                  observed_damage: "9007199254740993",
                  exact_integer_delta: "12345678901234567",
                  exact_rational_deltas: [
                    {
                      numerator: "7",
                      denominator: "3",
                      contribution_count: 2,
                    },
                  ],
                  damage_context_complete: true,
                },
              ],
            },
          ],
        },
      ],
    });
    expect(snapshot.runs[0]?.views[0]?.active_combat_micros).toBe(10_000_000);
    expect(snapshot.runs[0]?.views[0]?.actors[0]?.death_seconds).toEqual([]);
    expect(snapshot.runs[0]?.views[0]?.actors[0]?.rdps_damage).toBe(9_500);
    expect(snapshot.runs[0]?.views[0]?.actors[0]?.rdps_contribution_given).toBe(500);
    expect(snapshot.runs[0]?.views[0]?.actors[0]?.rdps_contribution_received).toBe(1_000);
    expect(snapshot.runs[0]?.views[0]?.actors[0]?.targets[0]?.series[0]?.damage).toBe(10_000);
    expect(snapshot.runs[0]?.views[0]?.targets[0]?.monster_id).toBe("33701");
    expect(snapshot.runs[0]?.views[0]?.targets[0]?.presentation_name).toBe(
      "Tina - Void Reverie",
    );
    expect(snapshot.runs[0]?.views[0]?.targets).toHaveLength(2);
    expect(snapshot.runs[0]?.views[0]?.targets[1]?.actor_kind).toBe("projectile");
    expect(snapshot.runs[0]?.views[0]?.targets[1]?.monster_id).toBe("10100101");
    expect(snapshot.runs[0]?.views[0]?.damage_influences[0]?.observed_damage).toBe(
      "9007199254740993",
    );
    expect(
      snapshot.runs[0]?.views[0]?.damage_influences[0]?.exact_rational_deltas[0]
        ?.numerator,
    ).toBe("7");
  });

  it("masks stale history attribution without changing ordinary damage", () => {
    const snapshot = parseCombatHistorySnapshot({
      schema_version: 1,
      session_id: "blocked-run",
      deployment_id: "global",
      region_id: "north-america",
      world_id: "asteria",
      client_build: "24687926",
      protocol_pack_digest: "sha256:test",
      rdps_formula_identity: "sha256:stale",
      runs: [{
        run_index: 0,
        rdps_status:
          "formula_runtime_blocked: exact-build promotion proof gates are incomplete",
        views: [{
          id: "all",
          label: "Entire run",
          elapsed_micros: 10_000_000,
          active_combat_micros: 10_000_000,
          actors: [{
            damage: 10_000,
            rdps: 1_100,
            rdps_damage: 11_000,
            rdps_contribution_given: 1_500,
            rdps_contribution_received: 500,
          }],
          targets: [],
          damage_influences: [{
            effect_id: "3003052",
            provider_actor_id: "4547",
            provider_entity_uuid: "1",
            recipient_actor_id: "13",
            recipient_entity_uuid: "2",
            affected_ability_id: "2352",
            target_actor_id: "99",
            target_entity_uuid: "3",
            first_observed_micros: 1,
            last_observed_micros: 2,
            damage_event_count: 1,
            observed_damage: "10000",
            exact_integer_delta: "1000",
            exact_rational_deltas: [],
            damage_context_complete: true,
          }],
        }],
      }],
    });

    const view = snapshot.runs[0]!.views[0]!;
    expect(view.actors[0]).toMatchObject({
      damage: 10_000,
      rdps: null,
      rdps_damage: null,
      rdps_contribution_given: null,
      rdps_contribution_received: null,
    });
    expect(view.damage_influences).toEqual([]);
  });

  it("validates a safe bulk-deletion result", () => {
    const result = parseCombatHistoryDeleteResult({
      requested_count: 3,
      deleted_count: 2,
      preserved_favorite_count: 1,
      unknown_history_id_count: 0,
      cleanup_warnings: [],
    });

    expect(result.deleted_count).toBe(2);
    expect(result.preserved_favorite_count).toBe(1);
  });

  it("rejects an unsupported history schema", () => {
    expect(() =>
      parseCombatHistoryCatalog({ schema_version: 2, entries: [] }),
    ).toThrow(/unsupported schema/i);
  });
});
