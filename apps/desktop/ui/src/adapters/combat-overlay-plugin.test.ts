import { describe, expect, it } from "vitest";

import {
  applyOverlayTimerPause,
  applyOverlayRdpsSkillDetail,
  availableTimerFields,
  actorName,
  buttonWidthFor,
  describeOverlayRdpsAvailability,
  ensureMetricHeaderViews,
  formatOverlayNumber,
  formatOverlayPercent,
  humanizeOverlayAttributionComponent,
  isOverlayRosterActor,
  mergeProjectedActorPresentation,
  maskUnavailableOverlayRdps,
  moveRelative,
  moveSummaryLayoutItem,
  moveSummaryField,
  nextOverlayHeaderViewId,
  normalizeHeaderViewGeometry,
  parseCombatOverlaySettings,
  planCombatOverlayVisibility,
  preferredOverlayDisplayName,
  projectOverlayRatesForTimer,
  shouldIgnoreCombatOverlayCursor,
  shouldKeepCombatVisibilityTimer,
} from "../../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";

function editableSummarySettings() {
  return parseCombatOverlaySettings({
    schemaVersion: 1,
    canvasWidth: 720,
    canvasHeight: 520,
    opacityPercent: 92,
    backgroundMode: "solid",
    backgroundColor: "#0b1522",
    backgroundOpacityPercent: 92,
    customBackgroundRevision: null,
    alwaysOnTop: true,
    clickThrough: false,
    dynamicHeight: true,
    maxVisiblePlayers: 20,
    scalePercent: 100,
    layers: [{
      id: "party-meter",
      title: "Party damage",
      metric: "dps",
      x: 0,
      y: 0,
      width: 720,
      headerFields: ["name", "dps"],
      headerWidths: { name: 190, dps: 90 },
      hiddenHeaderLabels: [],
      summaryFields: ["encounter_time", "scene", "team_dps", "team_damage", "boss_health"],
      summaryFieldWidths: { scene: 184 },
      summaryFieldRows: {
        encounter_time: 0,
        scene: 0,
        team_dps: 1,
        team_damage: 1,
        boss_health: 2,
      },
      hiddenSummaryLabels: [],
      showBossDps: true,
      buttons: [],
    }],
  });
}

describe("Combat Overlay plug-in settings", () => {
  it("preserves user-owned summary widths independently from table columns", () => {
    const settings = editableSummarySettings();

    expect(settings.layers[0]?.summaryFieldWidths).toEqual({ scene: 184 });
    expect(settings.layers[0]?.headerWidths.name).toBe(190);
  });

  it("freezes elapsed presentation clocks after the global inactivity delay", () => {
    const snapshot = {
      active_combat_micros: 12_000_000,
      encounter_elapsed_micros: 25_000_000,
      run_elapsed_micros: 30_000_000,
      game_time_micros: 28_000_000,
      true_time_micros: 26_000_000,
      last_hostile_micros: 10_000_000,
      latest_event_micros: 20_000_000,
    } as Parameters<typeof applyOverlayTimerPause>[0];
    const paused = applyOverlayTimerPause(snapshot, {
      pauseOverlayTimersOutsideCombat: true,
      overlayTimerInactivitySeconds: 3,
    });

    expect(paused).toMatchObject({
      active_combat_micros: 12_000_000,
      encounter_elapsed_micros: 18_000_000,
      run_elapsed_micros: 23_000_000,
      game_time_micros: 21_000_000,
      true_time_micros: 19_000_000,
    });
    expect(snapshot!.run_elapsed_micros).toBe(30_000_000);
  });

  it("keeps internal combat identifiers out of attribution labels", () => {
    expect(humanizeOverlayAttributionComponent(
      "Encore (55333) standalone-generated-damage (Actions 230401/230501)",
    )).toBe("Encore Standalone Generated Damage");
  });

  it("groups live rDPS detail by affected skill and provider/effect/component", () => {
    const actors = [{
      actor_id: "18446744073709551615",
      entity_uuid: "-9223372036854775808",
      display_name: "Provider",
      actor_kind: "player",
      dps: 0,
      hps: 0,
      tps: 0,
      rdps: 109,
      reported_damage: 0,
      abilities: [],
    }, {
      actor_id: "2",
      entity_uuid: "102",
      display_name: "Recipient",
      actor_kind: "player",
      dps: 1_200,
      hps: 0,
      tps: 0,
      rdps: 1_091,
      reported_damage: 1_200,
      abilities: [{
        ability_id: "2203521",
        presentation_name: "Steel Beak",
        casts: 1,
        hits: 2,
        critical_hits: 0,
        reported_damage: 1_200,
        effective_damage: 1_200,
        reported_healing: 0,
        effective_healing: 0,
        shielding: 0,
      }],
    }];
    const projected = applyOverlayRdpsSkillDetail(actors, [{
      effect_id: "31602",
      attribution_component: "packet-final action-speed opportunity",
      provider_actor_id: "18446744073709551615",
      recipient_actor_id: "2",
      affected_ability_id: "2203521",
      damage_event_count: 1,
      attributed_rdps: "50",
      damage_context_complete: true,
    }, {
      effect_id: "31602",
      attribution_component: "packet-final action-speed opportunity",
      provider_actor_id: "18446744073709551615",
      recipient_actor_id: "2",
      affected_ability_id: "2203521",
      damage_event_count: 1,
      attributed_rdps: "59",
      damage_context_complete: true,
    }], [{ effect_id: "31602", presentation_name: "Inspire" }], 1_000_000, false);

    expect(projected[1]?.reported_damage).toBe(1_200);
    expect(projected[1]?.abilities?.[0]).toMatchObject({
      ability_id: "2203521",
      reported_damage: 1_200,
      rdps_received_damage: "109",
      rdps_received_rate: 109,
      rdps_unresolved_relationship_count: 0,
      rdps_sources: [{
        provider_actor_id: "18446744073709551615",
        provider_name: "Provider",
        effect_id: "31602",
        effect_name: "Inspire",
        attribution_component: "packet-final action-speed opportunity",
        attributed_rdps: "109",
        rdps: 109,
        damage_event_count: 2,
      }],
    });
    expect(projected[0]?.reported_damage).toBe(0);
    expect(projected[0]?.abilities?.[0]).toMatchObject({
      ability_id: "support-effect:31602:packet-final action-speed opportunity",
      presentation_name: "Inspire",
      reported_damage: 0,
      rdps_support_effect: true,
      rdps_effect_id: "31602",
      rdps_given_damage: "109",
      rdps_given_rate: 109,
      rdps_grants: [{
        effect_id: "31602",
        effect_name: "Inspire",
        attribution_component: "packet-final action-speed opportunity",
        attributed_rdps: "109",
        rdps: 109,
        damage_event_count: 2,
      }],
    });
  });

  it("attaches outgoing credit to a proven provider skill instead of synthesizing a link", () => {
    const actors = [{
      actor_id: "1",
      display_name: "Provider",
      dps: 10,
      hps: 0,
      tps: 0,
      rdps: 15,
      abilities: [{
        ability_id: "777",
        presentation_name: "Proven support cast",
        casts: 1,
        hits: 0,
        critical_hits: 0,
        reported_damage: 0,
        effective_damage: 0,
        reported_healing: 0,
        effective_healing: 0,
        shielding: 0,
      }],
    }];

    const projected = applyOverlayRdpsSkillDetail(actors, [{
      effect_id: "31602",
      provider_actor_id: "1",
      provider_ability_id: "777",
      recipient_actor_id: "2",
      affected_ability_id: "55",
      damage_event_count: 1,
      attributed_rdps: "5",
      damage_context_complete: true,
    }], [{ effect_id: "31602", presentation_name: "Inspire" }], 1_000_000, false);

    expect(projected[0]?.abilities).toHaveLength(1);
    expect(projected[0]?.abilities?.[0]).toMatchObject({
      ability_id: "777",
      rdps_given_damage: "5",
      rdps_given_rate: 5,
    });
    expect(projected[0]?.abilities?.[0]?.rdps_support_effect).toBeUndefined();
  });

  it("keeps projected metrics while applying the live capture-time identity and loadout", () => {
    const projected = {
      actor_id: "77",
      entity_uuid: "216009015936",
      display_name: "Player 6",
      actor_kind: "player",
      dps: 900,
      edps: 950,
      hps: 10,
      tps: 20,
      rdps: 925,
      reported_damage: 9_000,
      presentation: {
        character_id: null,
        class_id: 11,
        specialization_id: null,
        class_name: "Marksman",
        specialization_name: null,
        class_spec_icon_asset_path: null,
        role: "damage" as const,
        accent: null,
        weapon: null,
        primary_imagines: [],
      },
    };
    const live = {
      actor_id: "77",
      entity_uuid: "216009015936",
      display_name: "MarieRose",
      actor_kind: "player",
      dps: 1,
      hps: 0,
      tps: 0,
      rdps: null,
      presentation: {
        character_id: "3296036",
        class_id: 11,
        specialization_id: 117,
        class_name: "Marksman",
        specialization_name: "Falconry",
        class_spec_icon_asset_path: "specs/falconry.webp",
        role: "damage" as const,
        accent: null,
        weapon: {
          slot_id: null,
          ability_id: null,
          item_id: 2_000_631,
          tier: null,
          level: 280,
          level_min: null,
          level_max: null,
          badge_kind: "far_sea",
          label: "Far Sea Bow",
          icon_asset_path: "weapons/far-sea-bow.webp",
        },
        primary_imagines: [{
          slot_id: 11,
          ability_id: 211_005_0,
          item_id: 10_001,
          tier: 5,
          level: null,
          level_min: null,
          level_max: null,
          badge_kind: null,
          label: "Imagine A",
          icon_asset_path: "imagines/a.webp",
        }],
      },
    };

    const merged = mergeProjectedActorPresentation(projected, live);
    expect(merged).toMatchObject({
      display_name: "MarieRose",
      dps: 900,
      edps: 950,
      rdps: 925,
      reported_damage: 9_000,
      presentation: {
        character_id: "3296036",
        specialization_id: 117,
        specialization_name: "Falconry",
        weapon: { item_id: 2_000_631 },
      },
    });
    expect(merged.presentation?.primary_imagines.map((imagine) => imagine.item_id)).toEqual([10_001]);
  });

  it("does not replace a completed run name with a terminal UID fallback", () => {
    expect(preferredOverlayDisplayName("UID 3296036", "MarieRose")).toBe("MarieRose");
    expect(preferredOverlayDisplayName("3296036", "MarieRose")).toBe("MarieRose");
    expect(preferredOverlayDisplayName("Player 6", "MarieRose")).toBe("MarieRose");
    expect(preferredOverlayDisplayName("CurrentName", "MarieRose")).toBe("CurrentName");
    expect(preferredOverlayDisplayName("UID 3296036", null)).toBe("UID 3296036");
  });

  it("labels live combatants without exposing runtime entity identifiers", () => {
    const base = {
      actor_id: "77",
      entity_uuid: "216009015936",
      display_name: null,
      dps: 1,
      hps: 0,
      tps: 0,
      rdps: null,
    };
    expect(actorName({
      ...base,
      display_name: "MarieRose",
    })).toBe("MarieRose");
    expect(actorName({
      ...base,
      presentation: {
        character_id: "3296036",
        class_id: null,
        specialization_id: null,
        class_name: null,
        specialization_name: null,
        class_spec_icon_asset_path: null,
        role: null,
        accent: null,
        weapon: null,
        primary_imagines: [],
      },
    })).toBe("UID 3296036");
    expect(actorName(base)).toBe("Unidentified player");
    expect(actorName({ ...base, entity_uuid: null })).toBe("Unidentified player");
  });

  it("keeps confirmed combatants and excludes unresolved damage targets from the roster", () => {
    const base = {
      actor_id: "77",
      entity_uuid: "216009015936",
      display_name: null,
      dps: 1,
      hps: 0,
      tps: 0,
      rdps: null,
    };
    expect(isOverlayRosterActor({ ...base, actor_kind: "player" })).toBe(true);
    expect(isOverlayRosterActor({ ...base, actor_kind: "monster" })).toBe(false);
    expect(isOverlayRosterActor({ ...base, actor_kind: "unknown:0" })).toBe(false);
    expect(isOverlayRosterActor({
      ...base,
      actor_kind: "npc",
      presentation: {
        character_id: null,
        class_id: 5,
        specialization_id: null,
        class_name: "Verdant Oracle",
        specialization_name: null,
        class_spec_icon_asset_path: null,
        role: "healer",
        accent: null,
        weapon: null,
        primary_imagines: [],
      },
    })).toBe(true);
    expect(isOverlayRosterActor({ ...base, actor_kind: "npc" })).toBe(false);
  });

  it("uses one precision policy for numeric overlay values", () => {
    expect(formatOverlayNumber(18_334_123, "compact")).toBe("18M");
    expect(formatOverlayNumber(18_334_123, "detailed")).toBe("18.33M");
    expect(formatOverlayNumber(18_334_123, "full")).toBe(
      new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(18_334_123),
    );
    expect(formatOverlayPercent(67.25, "compact")).toBe("67%");
    expect(formatOverlayPercent(67.25, "detailed")).toBe("67.3%");
    expect(formatOverlayPercent(67.25, "full")).toBe("67.25%");
  });

  it("masks stale rDPS values and explains exact proof blockers", () => {
    const availability = describeOverlayRdpsAvailability(
      "formula_pack_blocked: formula=global/24687926; blockers=protocol-pack-identity,canonical-replay-conservation,protocol-event-coverage,critical-damage-factor-interpretation-authority,party-support-formula-frontier",
    );
    const actors = maskUnavailableOverlayRdps([{
      actor_id: "77",
      display_name: "MarieRose",
      dps: 900,
      hps: 10,
      tps: 20,
      rdps: 925,
      rdps_damage: 9_250,
      rdps_contribution_given: 500,
      rdps_contribution_received: 250,
    }], availability);

    expect(availability.providerCreditEnabled).toBe(false);
    expect(availability.blockerCodes).toHaveLength(5);
    expect(availability.message).toContain("critical-damage interpretation");
    expect(availability.message).toContain("party-skill and team-entry");
    expect(availability.message).toContain(
      "Structurally absent remote-player casts are not required or inferred.",
    );
    expect(actors[0]).toMatchObject({
      dps: 900,
      rdps: null,
      rdps_damage: null,
      rdps_contribution_given: null,
      rdps_contribution_received: null,
    });
  });

  it("moves items in both directions using the hovered half as the insertion side", () => {
    expect(moveRelative(["a", "b", "c"], "a", "b", "after")).toEqual(["b", "a", "c"]);
    expect(moveRelative(["a", "b", "c"], "c", "b", "before")).toEqual(["a", "c", "b"]);
  });

  it("moves summary items between existing and newly created rows", () => {
    const layer = editableSummarySettings().layers[0]!;
    const movedBesideTeamDamage = moveSummaryField(layer, "scene", 1, "team_damage", "after");
    expect(movedBesideTeamDamage.summaryFields).toEqual([
      "encounter_time",
      "team_dps",
      "team_damage",
      "scene",
      "boss_health",
    ]);
    expect(movedBesideTeamDamage.summaryFieldRows.scene).toBe(1);

    const movedToNewRow = moveSummaryField(
      movedBesideTeamDamage,
      "encounter_time",
      3,
      null,
      "after",
    );
    expect(movedToNewRow.summaryFields.at(-1)).toBe("encounter_time");
    expect(movedToNewRow.summaryFieldRows.encounter_time).toBe(2);
  });

  it("moves summary items left and right inside the same row", () => {
    const layer = editableSummarySettings().layers[0]!;
    const movedRight = moveSummaryField(layer, "encounter_time", 0, "scene", "after");
    expect(movedRight.summaryFields.slice(0, 2)).toEqual(["scene", "encounter_time"]);
    expect(movedRight.summaryFieldRows.encounter_time).toBe(0);

    const movedLeft = moveSummaryField(movedRight, "encounter_time", 0, "scene", "before");
    expect(movedLeft.summaryFields.slice(0, 2)).toEqual(["encounter_time", "scene"]);
    expect(movedLeft.summaryFieldRows.encounter_time).toBe(0);
  });

  it("moves action controls anywhere inside the unified summary rows", () => {
    const settings = editableSummarySettings();
    const base = settings.layers[0]!;
    const layer = parseCombatOverlaySettings({
      ...settings,
      layers: [{
        ...base,
        buttons: [{ id: "timer", label: "Encounter", action: "cycle_timer" }],
        summaryItemOrder: undefined,
        summaryItemRows: undefined,
      }],
    }).layers[0]!;

    expect(layer.summaryFields).not.toContain("encounter_time");
    expect(layer.summaryItemOrder[0]).toBe("button:timer");
    const moved = moveSummaryLayoutItem(
      layer,
      "button:timer",
      1,
      "summary:team_damage",
      "after",
    );
    expect(moved.summaryItemRows["button:timer"]).toBe(1);
    expect(moved.summaryItemOrder.indexOf("button:timer"))
      .toBeGreaterThan(moved.summaryItemOrder.indexOf("summary:team_damage"));
  });

  it("migrates metric cycling into complete independently configurable header views", () => {
    const settings = editableSummarySettings();
    const source = {
      ...settings.layers[0]!,
      buttons: [{ id: "metric", label: "DPS", action: "cycle_metric" as const, width: 0 }],
    };
    const views = ensureMetricHeaderViews([source]);

    expect(views.map((view) => view.metric)).toEqual(["dps", "hps", "tps", "rdps"]);
    expect(views[1]?.headerFields).toContain("effective_healing");
    expect(views[2]?.headerFields).toContain("damage_taken");
    expect(views[3]?.headerFields).toContain("contribution_given");
    expect(nextOverlayHeaderViewId(views, views[0]!.id)).toBe(views[1]!.id);
    expect(nextOverlayHeaderViewId(views, views.at(-1)!.id)).toBe(views[0]!.id);
  });

  it("migrates the cycle timer to a fixed width without fixing other controls", () => {
    const settings = editableSummarySettings();
    const layer = parseCombatOverlaySettings({
      ...settings,
      layers: [{
        ...settings.layers[0]!,
        buttons: [
          { id: "timer", label: "Encounter", action: "cycle_timer" },
          { id: "hide", label: "Hide", action: "toggle_visibility" },
        ],
        summaryItemOrder: undefined,
        summaryItemRows: undefined,
      }],
    }).layers[0]!;

    expect(buttonWidthFor(layer.buttons[0]!)).toBe(116);
    expect(buttonWidthFor(layer.buttons[1]!)).toBe(0);
  });

  it("recalculates every rate from the selected visible timer without changing totals", () => {
    const actors = [{
      actor_id: "3296036",
      display_name: "MarieRose",
      dps: 1,
      edps: 1,
      adps: 1,
      hps: 1,
      tps: 1,
      rdps: 1,
      reported_damage: 12_000,
      damage_during_combat: 9_000,
      reported_healing: 6_000,
      damage_taken: 3_000,
      rdps_damage: 15_000,
    }];
    const snapshot = {
      encounter_elapsed_micros: 6_000_000,
      run_elapsed_micros: 12_000_000,
      game_time_micros: 10_000_000,
      true_time_micros: 8_000_000,
      actors: [],
    };

    const encounter = projectOverlayRatesForTimer(actors, snapshot, "encounter_time")[0]!;
    const run = projectOverlayRatesForTimer(actors, snapshot, "run_time")[0]!;
    expect(encounter).toMatchObject({ dps: 2_000, edps: 2_000, adps: 1_500, hps: 1_000, tps: 500, rdps: 2_500 });
    expect(run).toMatchObject({ dps: 1_000, edps: 1_000, adps: 750, hps: 500, tps: 250, rdps: 1_250 });
    expect(run.reported_damage).toBe(12_000);
  });

  it("keeps every live timer selectable before a saved run projection exists", () => {
    expect(availableTimerFields(null, {
      encounter_elapsed_micros: 10_000_000,
      game_time_micros: 11_000_000,
      true_time_micros: 12_000_000,
      run_elapsed_micros: 13_000_000,
    } as Parameters<typeof availableTimerFields>[1])).toEqual([
      "encounter_time",
      "game_time",
      "true_time",
      "run_time",
    ]);
  });

  it("gives older summary layouts safe semantic rows", () => {
    const settings = editableSummarySettings();
    const legacy = JSON.parse(JSON.stringify(settings));
    delete legacy.layers[0].summaryFieldRows;
    delete legacy.layers[0].showBossDps;
    const parsed = parseCombatOverlaySettings(legacy);
    expect(parsed.layers[0]?.summaryFieldRows).toEqual({
      encounter_time: 0,
      scene: 0,
      team_dps: 1,
      team_damage: 1,
      boss_health: 2,
    });
    expect(parsed.layers[0]?.showBossDps).toBe(true);
  });

  it("accepts the shared preview/runtime layout contract", () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1,
      canvasWidth: 460,
      canvasHeight: 520,
      opacityPercent: 92,
      barOpacityPercent: 18,
      backgroundMode: "solid",
      backgroundColor: "#0b1522",
      backgroundOpacityPercent: 92,
      customBackgroundRevision: null,
      alwaysOnTop: true,
      clickThrough: false,
      dynamicHeight: true,
      maxVisiblePlayers: 20,
      scalePercent: 100,
      layers: [
        {
          id: "party-meter",
          title: "Party damage",
          metric: "dps",
          x: 18,
          y: 18,
          width: 420,
          headerFields: [
            "rank",
            "class_spec",
            "name",
            "weapon",
            "main_imagines",
            "value",
            "percent",
          ],
          headerWidths: {
            rank: 30,
            class_spec: 32,
            name: 190,
            weapon: 32,
            main_imagines: 54,
            value: 90,
            percent: 48,
          },
          hiddenHeaderLabels: ["name"],
          buttons: [
            { id: "metric", label: "DPS", action: "cycle_metric" },
          ],
        },
      ],
    });

    expect(settings.layers[0]?.headerFields).toEqual([
      "rank",
      "class_spec",
      "name",
      "weapon",
      "main_imagines",
      "dps",
      "percent",
    ]);
    expect(settings.layers[0]?.hiddenHeaderLabels).toEqual(["name"]);
    expect(settings.scalePercent).toBe(100);
    expect(settings.layers[0]?.headerWidths.name).toBe(190);
    expect(settings.showViewTabs).toBe(false);
    expect(settings.autoHideOutsideCombat).toBe(false);
    expect(settings.autoHideDelaySeconds).toBe(5);
    expect(settings.refreshIntervalMillis).toBe(250);
    expect(settings.barOpacityPercent).toBe(18);
    expect(settings.barColorMode).toBe("random");
    expect(settings.barColorOverrides).toEqual({});
    expect(settings.numberFormat).toBe("detailed");
    expect(settings.numberFormats).toEqual({
      playerMetrics: "detailed",
      percentages: "compact",
      summaryTotals: "detailed",
      bossHealth: "detailed",
      bossMetrics: "detailed",
      skillValues: "detailed",
      counts: "full",
    });
  });

  it("accepts class and specialization color overrides without name coupling", () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1,
      canvasWidth: 460,
      canvasHeight: 520,
      opacityPercent: 92,
      barOpacityPercent: 25,
      barColorMode: "specialization",
      barColorOverrides: {
        "class:11": "#d95b68",
        "specialization:117": "#f0a83b",
      },
      backgroundMode: "solid",
      backgroundColor: "#0b1522",
      backgroundOpacityPercent: 92,
      customBackgroundRevision: null,
      alwaysOnTop: true,
      clickThrough: false,
      dynamicHeight: true,
      maxVisiblePlayers: 20,
      scalePercent: 100,
      layers: [{
        id: "party-meter",
        title: "Party damage",
        metric: "dps",
        x: 0,
        y: 0,
        width: 460,
        headerFields: ["name", "dps"],
        headerWidths: { name: 190, dps: 90 },
        hiddenHeaderLabels: [],
        buttons: [],
      }],
    });

    expect(settings.barColorMode).toBe("specialization");
    expect(settings.barColorOverrides["specialization:117"]).toBe("#f0a83b");
  });

  it("hides before combat, shows on reducer combat, and retains the reducer timeout", () => {
    const visibility = { autoHideOutsideCombat: true, autoHideDelaySeconds: 3 };

    expect(planCombatOverlayVisibility(visibility, null)).toEqual({
      showNow: false,
      hideAfterMillis: 3_000,
    });
    expect(planCombatOverlayVisibility(visibility, {
      combat_active: true,
      last_hostile_micros: 10_000_000,
      latest_event_micros: 12_000_000,
      combat_inactivity_timeout_micros: 8_000_000,
      actors: [],
    })).toEqual({
      showNow: true,
      hideAfterMillis: 9_000,
    });
    expect(planCombatOverlayVisibility(visibility, {
      combat_active: false,
      actors: [],
    })).toEqual({
      showNow: false,
      hideAfterMillis: 3_000,
    });
  });

  it("does not manage visibility when combat auto-hide is disabled", () => {
    expect(planCombatOverlayVisibility({
      autoHideOutsideCombat: false,
      autoHideDelaySeconds: 5,
    }, null)).toEqual({ showNow: true, hideAfterMillis: null });
  });

  it("does not restart the same idle countdown on every live snapshot", () => {
    expect(shouldKeepCombatVisibilityTimer("idle:5", "idle:5", false)).toBe(true);
    expect(shouldKeepCombatVisibilityTimer("idle:5", "idle:6", false)).toBe(false);
    expect(shouldKeepCombatVisibilityTimer("combat:10:8:5", "idle:5", false)).toBe(true);
    expect(shouldKeepCombatVisibilityTimer("combat:10:8:5", "combat:11:8:5", true)).toBe(false);
  });

  it("keeps automatic visibility separate from explicit click-through", () => {
    expect(shouldIgnoreCombatOverlayCursor(true, false)).toBe(false);
    expect(shouldIgnoreCombatOverlayCursor(true, true)).toBe(true);
    expect(shouldIgnoreCombatOverlayCursor(false, true)).toBe(true);
    expect(shouldIgnoreCombatOverlayCursor(false, false)).toBe(false);
  });

  it("makes the visible overlay surface fill the configured window geometry", () => {
    const settings = normalizeHeaderViewGeometry(parseCombatOverlaySettings({
      schemaVersion: 1,
      canvasWidth: 720,
      canvasHeight: 520,
      opacityPercent: 92,
      barOpacityPercent: 25,
      backgroundMode: "custom",
      backgroundColor: "#0b1522",
      backgroundOpacityPercent: 100,
      customBackgroundRevision: 4,
      alwaysOnTop: true,
      clickThrough: false,
      dynamicHeight: true,
      maxVisiblePlayers: 20,
      scalePercent: 150,
      layers: [
        {
          id: "party-meter",
          title: "Party damage",
          metric: "dps",
          x: 3,
          y: 3,
          width: 500,
          headerFields: ["rank", "name", "dps"],
          headerWidths: { rank: 30, name: 190, dps: 90 },
          hiddenHeaderLabels: [],
          buttons: [],
        },
      ],
    }));

    expect(settings.layers[0]).toMatchObject({ x: 0, y: 0, width: 720 });
  });

  it("rejects unsupported layer metrics", () => {
    expect(() =>
      parseCombatOverlaySettings({
        schemaVersion: 1,
        canvasWidth: 460,
        canvasHeight: 520,
        opacityPercent: 92,
        backgroundMode: "solid",
        backgroundColor: "#0b1522",
        backgroundOpacityPercent: 92,
        customBackgroundRevision: null,
      alwaysOnTop: true,
      clickThrough: false,
      dynamicHeight: true,
      maxVisiblePlayers: 20,
      scalePercent: 100,
        layers: [
          {
            id: "party-meter",
            title: "Party damage",
            metric: "accuracy",
            x: 18,
            y: 18,
            width: 420,
            headerFields: ["name", "value"],
            headerWidths: {
              rank: 30,
              class_spec: 32,
              name: 190,
              weapon: 32,
              main_imagines: 54,
              value: 90,
              percent: 48,
            },
            buttons: [],
          },
        ],
      }),
    ).toThrow(/invalid Combat Overlay settings/);
  });

  it("rejects a custom background until an image revision exists", () => {
    expect(() =>
      parseCombatOverlaySettings({
        schemaVersion: 1,
        canvasWidth: 460,
        canvasHeight: 520,
        opacityPercent: 92,
        backgroundMode: "custom",
        backgroundColor: "#0b1522",
        backgroundOpacityPercent: 50,
        customBackgroundRevision: null,
      alwaysOnTop: true,
      clickThrough: false,
      dynamicHeight: true,
      maxVisiblePlayers: 20,
      scalePercent: 100,
        layers: [],
      }),
    ).toThrow(/invalid Combat Overlay settings/);
  });
});
