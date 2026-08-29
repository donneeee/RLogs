import { describe, expect, it } from "vitest";

import { parseCombatMeterSettings } from "./combat-meter-settings";

describe("Combat Meter settings", () => {
  it("accepts both player-detail presentation modes", () => {
    expect(
      parseCombatMeterSettings({
        schemaVersion: 1,
        playerDetailPresentation: "in_app_layer",
        showClass: true,
        showSpecialization: true,
        showLevel: true,
        showSeasonalScore: true,
        showPartyIcons: true,
      }).playerDetailPresentation,
    ).toBe("in_app_layer");
    expect(
      parseCombatMeterSettings({
        schemaVersion: 1,
        playerDetailPresentation: "popover",
        showClass: false,
        showSpecialization: false,
        showLevel: false,
        showSeasonalScore: false,
        showPartyIcons: false,
      }).playerDetailPresentation,
    ).toBe("popover");
  });

  it("defaults History sizing for settings saved before sizing controls existed", () => {
    const settings = parseCombatMeterSettings({
      schemaVersion: 1,
      playerDetailPresentation: "in_app_layer",
      showClass: true,
      showSpecialization: true,
      showLevel: true,
      showSeasonalScore: true,
      showPartyIcons: true,
    });
    expect(settings.historyBodyFontSizePx).toBe(15);
    expect(settings.historyIconSizePx).toBe(48);
    expect(settings.showHistoryPlayerColumn).toBe(true);
    expect(settings.showHistoryDamageColumn).toBe(true);
    expect(settings.showHistoryDpsColumn).toBe(true);
    expect(settings.showHistoryEncounterDpsColumn).toBe(true);
    expect(settings.showHistoryHpsColumn).toBe(true);
    expect(settings.showHistoryTpsColumn).toBe(true);
    expect(settings.showHistoryRdpsColumn).toBe(true);
    expect(settings.showHistoryApmColumn).toBe(true);
    expect(settings.showHistoryDeathsColumn).toBe(true);
    expect(settings.historyPartyColorMode).toBe("party_order");
    expect(settings.historySpecializationColors).toEqual({});
    expect(settings.historyPartyViews.map((view) => view.label)).toEqual([
      "Damage",
      "rDPS",
      "Healing",
      "Defense",
    ]);
    expect(settings.historyPartyViews[1]?.columns).toEqual([
      "player",
      "damage",
      "rdmg",
      "rdps",
      "rdpsGiven",
      "rdpsReceived",
    ]);
  });

  it("parses stable per-specialization History colors", () => {
    const settings = parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      historyPartyColorMode: "specialization",
      historySpecializationColors: {
        "117": "#F97316",
        "128": "#5EEAD4",
      },
    });
    expect(settings.historyPartyColorMode).toBe("specialization");
    expect(settings.historySpecializationColors).toEqual({
      "117": "#f97316",
      "128": "#5eead4",
    });
  });

  it("rejects unsafe History specialization colors", () => {
    expect(() => parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      historyPartyColorMode: "specialization",
      historySpecializationColors: { "117": "orange" },
    })).toThrow(/specialization color/i);
  });

  it("preserves independent History party-column visibility", () => {
    const settings = parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      showHistoryPlayerColumn: true,
      showHistoryDamageColumn: false,
      showHistoryDpsColumn: true,
      showHistoryEncounterDpsColumn: false,
      showHistoryHpsColumn: true,
      showHistoryTpsColumn: false,
      showHistoryRdpsColumn: true,
      showHistoryApmColumn: false,
      showHistoryDeathsColumn: true,
    });
    expect(settings.showHistoryDamageColumn).toBe(false);
    expect(settings.showHistoryEncounterDpsColumn).toBe(false);
    expect(settings.showHistoryTpsColumn).toBe(false);
    expect(settings.showHistoryApmColumn).toBe(false);
    expect(settings.showHistoryDeathsColumn).toBe(true);
  });

  it("preserves independent named History party views", () => {
    const settings = parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      historyPartyViews: [
        {
          id: "support",
          label: "Support",
          columns: ["player", "rdpsGiven", "effectiveHealing", "hps"],
          widths: { player: 340, rdpsGiven: 128 },
          sortKey: "rdpsGiven",
          sortDirection: "descending",
          detailMode: "healing",
        },
      ],
    });
    expect(settings.historyPartyViews).toEqual([
      {
        id: "support",
        label: "Support",
        columns: ["player", "rdpsGiven", "effectiveHealing", "hps"],
        widths: { player: 340, rdpsGiven: 128 },
        sortKey: "rdpsGiven",
        sortDirection: "descending",
        detailMode: "healing",
      },
    ]);
  });

  it("rejects a History view whose default sort is hidden", () => {
    expect(() => parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      historyPartyViews: [{
        id: "damage",
        label: "Damage",
        columns: ["player", "damage"],
        widths: {},
        sortKey: "hps",
        sortDirection: "descending",
        detailMode: "damage",
      }],
    })).toThrow(/sort column must be visible/i);
  });

  it("rejects unsafe History sizing values", () => {
    expect(() => parseCombatMeterSettings({
      ...DEFAULT_SETTINGS,
      historyIconSizePx: 1000,
    })).toThrow(/icon size/i);
  });

  it("rejects unknown presentation modes", () => {
    expect(() =>
      parseCombatMeterSettings({
        schemaVersion: 1,
        playerDetailPresentation: "detached_process",
        showClass: true,
        showSpecialization: true,
        showLevel: true,
        showSeasonalScore: true,
        showPartyIcons: true,
      }),
    ).toThrow(/presentation/i);
  });

  it("requires every independent party-row visibility setting", () => {
    expect(() =>
      parseCombatMeterSettings({
        schemaVersion: 1,
        playerDetailPresentation: "in_app_layer",
        showClass: true,
        showSpecialization: true,
        showLevel: true,
        showSeasonalScore: true,
      }),
    ).toThrow(/icon visibility/i);
  });
});

const DEFAULT_SETTINGS = {
  schemaVersion: 1,
  playerDetailPresentation: "in_app_layer",
  showClass: true,
  showSpecialization: true,
  showLevel: true,
  showSeasonalScore: true,
  showPartyIcons: true,
  showHistoryPlayerColumn: true,
  showHistoryDamageColumn: true,
  showHistoryDpsColumn: true,
  showHistoryEncounterDpsColumn: true,
  showHistoryHpsColumn: true,
  showHistoryTpsColumn: true,
  showHistoryRdpsColumn: true,
  showHistoryApmColumn: true,
  showHistoryDeathsColumn: true,
  historyPartyColorMode: "party_order",
  historySpecializationColors: {},
  historyBodyFontSizePx: 15,
  historyHeadingFontSizePx: 24,
  historyTableFontSizePx: 13,
  historyMetadataFontSizePx: 11,
  historyMetricFontSizePx: 18,
  historyIconSizePx: 48,
} as const;
