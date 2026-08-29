import { describe, expect, it } from "vitest";

import { combatMeterActorColumnText } from "./combat-meter-surface";
import type { CombatActorSummary } from "./combat-meter";

const ACTOR = {
  dps: 1_234.5,
  hps: 234.5,
  tps: 34.5,
  rdps_damage: 12_000,
  rdps: 1_200,
  rdps_contribution_given: 500,
  rdps_contribution_received: 250,
  reported_damage: 12_250,
  effective_damage: 12_100,
  effective_healing: 2_345,
  deaths: 1,
} as CombatActorSummary;

describe("Combat Meter actor columns", () => {
  it("keeps rate and relative-damage fields aligned with their headers", () => {
    expect([
      combatMeterActorColumnText(ACTOR, "dps"),
      combatMeterActorColumnText(ACTOR, "hps"),
      combatMeterActorColumnText(ACTOR, "tps"),
      combatMeterActorColumnText(ACTOR, "rdps_damage"),
      combatMeterActorColumnText(ACTOR, "rdps"),
      combatMeterActorColumnText(ACTOR, "rdps_contribution_given"),
      combatMeterActorColumnText(ACTOR, "rdps_contribution_received"),
      combatMeterActorColumnText(ACTOR, "reported_damage"),
      combatMeterActorColumnText(ACTOR, "effective_damage"),
      combatMeterActorColumnText(ACTOR, "effective_healing"),
      combatMeterActorColumnText(ACTOR, "deaths"),
    ]).toEqual([
      "1,234.5",
      "234.5",
      "34.5",
      "12,000",
      "1,200",
      "500",
      "250",
      "12,250",
      "12,100",
      "2,345",
      "1",
    ]);
  });

  it("does not turn unresolved relative damage into a zero", () => {
    expect(combatMeterActorColumnText({ ...ACTOR, rdps_damage: null }, "rdps_damage"))
      .toBe("—");
    expect(combatMeterActorColumnText({ ...ACTOR, rdps: null }, "rdps"))
      .toBe("—");
  });
});
