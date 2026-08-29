import { describe, expect, it } from "vitest";

import type {
  CombatHistoryCatalogEntry,
  CombatHistoryParticipant,
  CombatHistoryView,
  HistoryActorSummary,
} from "./combat-history";
import {
  activityLabel,
  actorRdpsBreakdown,
  abilitySortMaximum,
  buildActorGraphSeries,
  catalogParticipantLabel,
  catalogParticipantTooltip,
  comparePartySortValues,
  compactSpecializationName,
  displayedUnmappedRdpsSkill,
  filterAndSortHistoryEntries,
  graphScaleMaximum,
  groupDisplayedAbilities,
  historyDamageInfluenceMatchesQuery,
  historyRdpsEffectPresentation,
  historyRdpsProgressPresentation,
  historyActorColor,
  historyTargetLabel,
  incomingDamageSourceGroups,
  loadoutTierForPresentation,
  participantRows,
  partyBarPercentage,
  sortDisplayedAbilities,
  supplementalDifficultyLabel,
  terminalPresentationLabel,
} from "./combat-history-surface";

describe("Combat History archived rDPS progress", () => {
  it("reports measurable replay progress and the saved-result contract", () => {
    const presentation = historyRdpsProgressPresentation({
      session_id: "session-1",
      stage: "replaying",
      processed_events: 12_345,
      processed_bytes: 25,
      total_bytes: 100,
    });

    expect(presentation.stageLabel).toBe("Replaying sealed combat events");
    expect(presentation.percent).toBe(25);
    expect(presentation.details).toContain("12,345 canonical events processed");
    expect(presentation.details).toContain("later opens use the saved projection");
  });

  it("shows that live capture pauses rather than competes with history replay", () => {
    const presentation = historyRdpsProgressPresentation({
      session_id: "session-1",
      stage: "waiting_for_live_capture",
      processed_events: 0,
      processed_bytes: 0,
      total_bytes: 1_000,
    });

    expect(presentation.stageLabel).toContain("Paused");
    expect(presentation.details).toContain("resume after capture stops");
  });
});

describe("Combat History rDPS influence filtering", () => {
  const provider = {
    actor_id: "provider-actor",
    entity_uuid: "provider-entity",
    character_id: "1002003",
    display_name: "Support",
    presentation_name: "Support",
    presentation_class_name: "Shield Knight",
    presentation_specialization_name: "Recovery",
    abilities: [],
    effects: [{ effect_id: "2302121", presentation_name: "Team Luck & Crit" }],
  } as unknown as HistoryActorSummary;
  const recipient = {
    actor_id: "recipient-actor",
    entity_uuid: "recipient-entity",
    character_id: "3296036",
    display_name: "MarieRose",
    presentation_name: "MarieRose",
    presentation_class_name: "Marksman",
    presentation_specialization_name: "Falconry",
    abilities: [{ ability_id: "2031109", presentation_name: "Falconry hit" }],
    effects: [],
  } as unknown as HistoryActorSummary;
  const view = {
    actors: [provider, recipient],
    targets: [{
      actor_id: "target-actor",
      entity_uuid: "target-entity",
      monster_id: "70101",
      display_name: "Training Dummy",
      presentation_name: "Training Dummy",
    }],
  } as CombatHistoryView;
  const influence = {
    effect_id: "2302121",
    attribution_component: "inspiration-critical-chance",
    provider_actor_id: provider.actor_id,
    provider_entity_uuid: provider.entity_uuid,
    recipient_actor_id: recipient.actor_id,
    recipient_entity_uuid: recipient.entity_uuid,
    affected_ability_id: "2031109",
    target_actor_id: "target-actor",
    target_entity_uuid: "target-entity",
  } as CombatHistoryView["damage_influences"][number];

  it("matches exact provider and recipient UIDs", () => {
    expect(historyDamageInfluenceMatchesQuery(view, influence, "1002003")).toBe(true);
    expect(historyDamageInfluenceMatchesQuery(view, influence, "3296036")).toBe(true);
  });

  it("matches effect and affected skill identities together", () => {
    expect(historyDamageInfluenceMatchesQuery(view, influence, "Team Luck & Crit 2031109")).toBe(true);
    expect(historyDamageInfluenceMatchesQuery(view, influence, "2302121 Falconry hit")).toBe(true);
  });

  it("matches the exact attribution component", () => {
    expect(historyDamageInfluenceMatchesQuery(view, influence, "critical chance")).toBe(true);
    expect(historyDamageInfluenceMatchesQuery(view, influence, "lucky chance")).toBe(false);
  });

  it("matches participant and target presentation without broad false positives", () => {
    expect(historyDamageInfluenceMatchesQuery(view, influence, "MarieRose Training Dummy")).toBe(true);
    expect(historyDamageInfluenceMatchesQuery(view, influence, "unrelated-player")).toBe(false);
  });

  it("uses the retained entity UUID when an actor ID rotates", () => {
    const rotatedView = {
      ...view,
      actors: [{
        ...provider,
        entity_uuid: "retired-provider-entity",
        character_id: "retired-provider-uid",
        presentation_name: "Retired Support",
      }, provider, recipient],
    } as CombatHistoryView;

    expect(historyDamageInfluenceMatchesQuery(rotatedView, influence, "1002003")).toBe(true);
    expect(historyDamageInfluenceMatchesQuery(rotatedView, influence, "retired-provider-uid")).toBe(false);
  });

  it.each([
    ["55228", "Luminary Bolt Vulnerability"],
    ["55333", "Encore"],
    ["2110065", "Fiery Battle Will"],
    ["2110125", "Highland Blood"],
    ["2110140", "Mechanical Power"],
    ["2110143", "Functional Amp"],
    ["2202041", "Inspiration"],
    ["2204471", "Critical Cold"],
    ["2207252", "Stat Resonance"],
    ["2302121", "Team Luck & Crit"],
    ["3003052", "Harmony Grace"],
  ])("uses the exact-ID rDPS presentation registry for %s %s", (effectId, name) => {
    const registryView = {
      ...view,
      actors: [{ ...provider, effects: [] }, recipient],
      rdps_effect_presentations: [{
        effect_id: effectId,
        presentation_name: name,
        presentation_kind: "status-effect",
        presentation_resolution: "reviewed-source-name",
        icon_asset_path: null,
      }],
    } as CombatHistoryView;
    const registryInfluence = { ...influence, effect_id: effectId };

    expect(historyRdpsEffectPresentation(registryView, effectId)?.presentation_name).toBe(name);
    expect(historyDamageInfluenceMatchesQuery(registryView, registryInfluence, name)).toBe(true);
  });
});

describe("Combat History rDPS breakdown", () => {
  const influence = (
    providerActorId: string,
    recipientActorId: string,
    abilityId: string | null,
    component: string,
    attributedRdps: string | null,
  ) => ({
    effect_id: "2302121",
    attribution_component: component,
    provider_actor_id: providerActorId,
    provider_entity_uuid: `${providerActorId}-entity`,
    recipient_actor_id: recipientActorId,
    recipient_entity_uuid: `${recipientActorId}-entity`,
    affected_ability_id: abilityId,
    target_actor_id: "boss",
    target_entity_uuid: "boss-entity",
    first_observed_micros: 1,
    last_observed_micros: 2,
    damage_event_count: 3,
    observed_damage: "1000",
    exact_integer_delta: "0",
    exact_rational_deltas: [],
    attributed_rdps: attributedRdps,
    damage_context_complete: true,
  }) as CombatHistoryView["damage_influences"][number];

  it("groups received rDPS by damage skill and preserves each provider source", () => {
    const view = {
      damage_influences: [
        influence("provider-a", "recipient", "skill-1", "critical-damage", "9007199254740993"),
        influence("provider-a", "recipient", "skill-1", "critical-damage", "7"),
        influence("provider-b", "recipient", "skill-1", "lucky-damage", "25"),
        influence("provider-b", "recipient", "skill-2", "lucky-damage", null),
      ],
    } as CombatHistoryView;

    const breakdown = actorRdpsBreakdown(view, "recipient");

    expect(breakdown.receivedSkills).toHaveLength(2);
    expect(breakdown.receivedSkills[0]).toMatchObject({
      abilityId: "skill-1",
      attributedRdps: "9007199254741025",
      damageEventCount: 9,
      unresolvedRelationshipCount: 0,
    });
    expect(breakdown.receivedSkills[0]?.sources).toEqual([
      expect.objectContaining({
        providerActorId: "provider-a",
        attributedRdps: "9007199254741000",
      }),
      expect.objectContaining({
        providerActorId: "provider-b",
        attributedRdps: "25",
      }),
    ]);
    expect(breakdown.receivedSkills[1]).toMatchObject({
      abilityId: "skill-2",
      attributedRdps: null,
      unresolvedRelationshipCount: 1,
    });
  });

  it("groups outgoing rDPS by support effect and component", () => {
    const view = {
      damage_influences: [
        influence("provider-a", "recipient-a", "skill-1", "critical-damage", "100"),
        influence("provider-a", "recipient-b", "skill-2", "critical-damage", "50"),
        influence("provider-a", "recipient-b", "skill-2", "lucky-damage", "25"),
      ],
    } as CombatHistoryView;

    expect(actorRdpsBreakdown(view, "provider-a").grantedEffects).toEqual([
      expect.objectContaining({
        effectId: "2302121",
        attributionComponent: "critical-damage",
        attributedRdps: "150",
        damageEventCount: 6,
      }),
      expect.objectContaining({
        effectId: "2302121",
        attributionComponent: "lucky-damage",
        attributedRdps: "25",
        damageEventCount: 3,
      }),
    ]);
  });

  it("keeps attributed damage whose packet action has no mapped ability ID visible", () => {
    const view = {
      elapsed_micros: 2_000_000,
      damage_influences: [
        influence("provider-a", "recipient", null, "critical-damage", "75"),
      ],
    } as CombatHistoryView;
    const unmapped = actorRdpsBreakdown(view, "recipient").receivedSkills[0]!;

    expect(unmapped.abilityId).toBeNull();
    expect(displayedUnmappedRdpsSkill(unmapped, view)).toMatchObject({
      abilityId: "not observed",
      presentationName: "Unmapped damage actions",
      receivedRdmgExact: "75",
      receivedRdps: 37.5,
      hasRdpsRelationship: true,
    });
  });
});

describe("Combat History catalog participant identity", () => {
  it("prefers the captured public name and shared localized presentation", () => {
    const participant = {
      actor_id: "player-6",
      entity_uuid: "216009015936",
      character_id: "3296036",
      actor_kind: "player",
      presentation_kind: "player",
      display_name: "MarieRose",
      presentation_name: "Player 6",
      class_id: 11,
      specialization_id: 117,
      presentation_class_name: "Marksman",
      presentation_specialization_name: "Falconry",
    } as CombatHistoryParticipant;

    expect(catalogParticipantLabel(participant)).toBe("MarieRose");
    expect(catalogParticipantTooltip(participant)).toBe(
      "MarieRose · UID 3296036 · Marksman · Falconry",
    );
  });
});

describe("Combat History incoming damage", () => {
  it("preserves the exact incoming total and exposes any unmapped remainder", () => {
    const victim = {
      actor_id: "player-1",
      targets: [{
        actor_id: "boss-1",
        entity_uuid: "9001",
        series: [
          { second: 1, damage: 0, effective_healing: 0, damage_taken: 700 },
          { second: 2, damage: 0, effective_healing: 0, damage_taken: 300 },
        ],
      }],
    } as HistoryActorSummary;
    const source = {
      actor_id: "boss-1",
      abilities: [{
        ability_id: "attack-1",
        targets: [{
          actor_id: "player-1",
          effective_damage: 800,
          hits: 2,
        }],
      }],
    } as HistoryActorSummary;
    const view = { actors: [victim, source] } as CombatHistoryView;

    const groups = incomingDamageSourceGroups(view, victim, null);

    expect(groups).toHaveLength(1);
    expect(groups[0]?.total).toBe(1_000);
    expect(groups[0]?.abilities[0]?.damage).toBe(800);
    expect(groups[0]?.unattributed).toBe(200);
    expect(
      (groups[0]?.abilities.reduce((sum, entry) => sum + entry.damage, 0) ?? 0) +
      (groups[0]?.unattributed ?? 0),
    ).toBe(groups[0]?.total);
  });
});

describe("Combat History loadout tiers", () => {
  const slot = (tier: number | null, itemId: number | null) => ({
    slot_id: 21,
    ability_id: 3021,
    item_id: itemId,
    tier,
    item_tier: null,
    maximum_tier: null,
    presentation_name: "Thunderfall Grasp",
    icon_asset_path: "icons/skills/auxiliary/3021-thunderfall-grasp.png",
  });

  it("only presents T1 through T4 for auxiliary Imagine replacements", () => {
    expect(loadoutTierForPresentation(slot(0, 3_000_009), "role_slot")).toBeNull();
    expect(loadoutTierForPresentation(slot(1, 3_000_009), "role_slot")).toBe(1);
    expect(loadoutTierForPresentation(slot(4, 3_000_009), "role_slot")).toBe(4);
    expect(loadoutTierForPresentation(slot(5, 3_000_009), "role_slot")).toBeNull();
  });

  it("keeps primary Imagine T0 and T5 presentation intact", () => {
    expect(loadoutTierForPresentation(slot(0, 3_000_009), "imagine")).toBe(0);
    expect(loadoutTierForPresentation(slot(5, 3_000_009), "imagine")).toBe(5);
  });

  it("does not present an Imagine tier for a native role skill", () => {
    expect(loadoutTierForPresentation(slot(4, null), "role_slot")).toBeNull();
  });
});

describe("Combat History scene labels", () => {
  it("prefers the game-localized scene name while retaining raw identity fallbacks", () => {
    expect(activityLabel({
      activity_id: "scene.12023",
      activity_family_id: "guild-hunt",
      scene_id: 12023,
      presentation_scene_name: "Guild Hunt - Hard",
    })).toBe("Guild Hunt - Hard");
    expect(activityLabel({
      activity_id: "scene.1",
      activity_family_id: null,
      scene_id: 1,
      presentation_scene_name: null,
    })).toBe("Scene.1");
  });

  it("does not repeat a difficulty already contained in the activity name", () => {
    expect(supplementalDifficultyLabel({
      activity_id: "scene.12023",
      activity_family_id: "guild-hunt",
      scene_id: 12023,
      presentation_scene_name: "Guild Hunt - Hard",
      difficulty_family: "hard",
      difficulty_tier: null,
    })).toBeNull();
    expect(supplementalDifficultyLabel({
      activity_id: "scene.1621",
      activity_family_id: "unstable-tina-mindrealm",
      scene_id: 1621,
      presentation_scene_name: "Unstable - Tina's Mindrealm",
      difficulty_family: "unstable",
      difficulty_tier: null,
    })).toBeNull();
  });

  it("retains a distinct difficulty that adds information to the activity name", () => {
    expect(supplementalDifficultyLabel({
      activity_id: "scene.1632",
      activity_family_id: "tina-mindrealm",
      scene_id: 1632,
      presentation_scene_name: "Chaotic - Tina's Mindrealm",
      difficulty_family: "hard",
      difficulty_tier: null,
    })).toBe("Hard");
  });

  it("never presents a bare Master label when the packet tier is absent", () => {
    expect(supplementalDifficultyLabel({
      activity_id: "scene.6515",
      activity_family_id: "cursed-radiant-tomb",
      scene_id: 6515,
      presentation_scene_name: "Chaotic - Cursed Radiant Tomb",
      difficulty_family: "master",
      difficulty_tier: null,
    })).toBe("Master (tier unresolved)");
  });
});

describe("Combat History terminal labels", () => {
  it("presents a scene exit as a failed local attempt without changing its exact cause", () => {
    expect(terminalPresentationLabel("exited")).toBe("Failed (Exited)");
    expect(terminalPresentationLabel("failed")).toBe("Failed");
    expect(terminalPresentationLabel("completed")).toBe("Completed");
  });
});

describe("Combat History specialization labels", () => {
  it("removes the redundant English Spec suffix", () => {
    expect(compactSpecializationName("Falconry Spec")).toBe("Falconry");
    expect(compactSpecializationName("Dissonance Spec")).toBe("Dissonance");
    expect(compactSpecializationName("Smite")).toBe("Smite");
  });
});

describe("Combat History party sorting", () => {
  it("toggles numeric ordering while keeping unresolved values last", () => {
    expect(comparePartySortValues(20, 10, "descending")).toBeLessThan(0);
    expect(comparePartySortValues(20, 10, "ascending")).toBeGreaterThan(0);
    expect(comparePartySortValues(null, 10, "descending")).toBeGreaterThan(0);
    expect(comparePartySortValues(null, 10, "ascending")).toBeGreaterThan(0);
  });

  it("scales the active metric bar against the complete party maximum", () => {
    expect(partyBarPercentage(50, 100)).toBe(50);
    expect(partyBarPercentage(200, 100)).toBe(100);
    expect(partyBarPercentage(null, 100)).toBe(0);
  });

  it("keeps a support-only player visible when their rDPS relationship is nonzero", () => {
    const support = {
      actor_id: "support",
      actor_kind: "player",
      presentation_kind: "player",
      presentation_name: "Support",
      damage: 0,
      healing: 0,
      damage_taken: 0,
      deaths: 0,
      encounter_dps: 0,
      rdps_contribution_given: 500,
      rdps_contribution_received: 0,
    } as HistoryActorSummary;
    const inactive = {
      ...support,
      actor_id: "inactive",
      presentation_name: "Inactive",
      rdps_contribution_given: 0,
    };

    expect(participantRows({ actors: [support, inactive] } as CombatHistoryView))
      .toEqual([support]);
  });
});

describe("Combat History ability sorting", () => {
  const ability = (abilityId: string, name: string, damage: number, healing: number) => ({
    abilityId,
    presentationName: name,
    presentationKind: "base-skill",
    presentationResolution: "localized",
    iconAssetPath: null,
    recountGroupId: null as string | null,
    recountGroupName: null as string | null,
    damage,
    hits: damage / 10,
    casts: damage / 100,
    criticals: damage / 20,
    dps: damage / 2,
    encounterDps: damage,
    healing,
    effectiveHealing: healing,
    shielding: 0,
    hps: healing / 2,
    receivedRdmgExact: null as string | null,
    receivedRdmg: null as number | null,
    receivedRdps: null as number | null,
    rdpsSources: [],
    rdpsDamageEventCount: 0,
    rdpsUnresolvedRelationshipCount: 0,
    hasRdpsRelationship: false,
  });

  it("sorts every skill metric in either direction", () => {
    const abilities = [
      ability("20", "Beta", 200, 10),
      ability("10", "Alpha", 100, 30),
    ];

    expect(sortDisplayedAbilities(abilities, "damage", "descending").map((entry) => entry.abilityId))
      .toEqual(["20", "10"]);
    expect(sortDisplayedAbilities(abilities, "damage", "ascending").map((entry) => entry.abilityId))
      .toEqual(["10", "20"]);
    expect(sortDisplayedAbilities(abilities, "ability", "ascending").map((entry) => entry.abilityId))
      .toEqual(["10", "20"]);
    expect(sortDisplayedAbilities(abilities, "healing", "descending").map((entry) => entry.abilityId))
      .toEqual(["10", "20"]);
    const withRdps = [
      { ...abilities[0]!, receivedRdmg: 50, receivedRdps: 25 },
      { ...abilities[1]!, receivedRdmg: 75, receivedRdps: 37.5 },
    ];
    expect(sortDisplayedAbilities(withRdps, "rdmgReceived", "descending").map((entry) => entry.abilityId))
      .toEqual(["10", "20"]);
  });

  it("scales skill bars from the complete active metric", () => {
    const abilities = [
      ability("20", "Beta", 200, 10),
      ability("10", "Alpha", 100, 30),
    ];

    expect(abilitySortMaximum(abilities, "damage")).toBe(200);
    expect(abilitySortMaximum(abilities, "healing")).toBe(30);
    expect(abilitySortMaximum(abilities, "ability")).toBe(0);
    expect(partyBarPercentage(100, abilitySortMaximum(abilities, "damage"))).toBe(50);
  });

  it("adds an aggregate Recount parent without removing or rewriting its children", () => {
    const first = {
      ...ability("2203311", "Explosive Arrow", 200, 10),
      recountGroupId: "106",
      recountGroupName: "Explosive Arrow",
      receivedRdmgExact: "40",
      receivedRdmg: 40,
      receivedRdps: 20,
      rdpsDamageEventCount: 2,
      hasRdpsRelationship: true,
      rdpsSources: [{
        providerActorId: "30",
        providerEntityUuid: "1",
        effectId: "2302121",
        attributionComponent: "team-luck-critical-damage",
        attributedRdps: "40",
        damageEventCount: 2,
        unresolvedRelationshipCount: 0,
      }],
    };
    const second = {
      ...ability("2203312", "Explosive Arrow follow-up", 100, 20),
      recountGroupId: "106",
      recountGroupName: "Explosive Arrow",
      receivedRdmgExact: "10",
      receivedRdmg: 10,
      receivedRdps: 5,
      rdpsDamageEventCount: 1,
      hasRdpsRelationship: true,
      rdpsSources: [{
        providerActorId: "30",
        providerEntityUuid: "1",
        effectId: "2302121",
        attributionComponent: "team-luck-critical-damage",
        attributedRdps: "10",
        damageEventCount: 1,
        unresolvedRelationshipCount: 0,
      }],
    };
    const standalone = ability("2233", "Powerdraw", 250, 0);
    const rows = groupDisplayedAbilities(
      [first, second, standalone],
      "damage",
      "descending",
    );

    expect(rows.map((row) => [row.kind, row.ability.abilityId])).toEqual([
      ["recount-parent", "106"],
      ["recount-child", "2203311"],
      ["recount-child", "2203312"],
      ["standalone", "2233"],
    ]);
    expect(rows[0]?.ability.damage).toBe(300);
    expect(rows[0]?.ability.healing).toBe(30);
    expect(rows[0]?.ability.receivedRdmgExact).toBe("50");
    expect(rows[0]?.ability.receivedRdps).toBe(25);
    expect(rows[0]?.ability.rdpsSources).toMatchObject([{ attributedRdps: "50" }]);
    expect(rows[0]?.childCount).toBe(2);
    expect(rows[2]?.isLastChild).toBe(true);
  });
});

describe("Combat History graph scaling", () => {
  it("builds every graph metric from the selected entity's exact sparse series", () => {
    const actor = {
      actor_id: "player-1",
      death_seconds: [1],
      series: [
        { second: 0, damage: 1_000, effective_healing: 600, damage_taken: 400 },
        { second: 1, damage: 1_000, effective_healing: 400, damage_taken: 200 },
      ],
      targets: [
        {
          actor_id: "boss-1",
          series: [
            { second: 0, damage: 200, effective_healing: 0, damage_taken: 50 },
            { second: 1, damage: 300, effective_healing: 0, damage_taken: 0 },
          ],
        },
        {
          actor_id: "party-2",
          series: [
            { second: 0, damage: 0, effective_healing: 250, damage_taken: 0 },
          ],
        },
      ],
    } as HistoryActorSummary;

    expect(buildActorGraphSeries(actor, "damage", 2, "#fff", null).average).toBe(1_000);
    expect(buildActorGraphSeries(actor, "damage", 2, "#fff", "boss-1").average).toBe(250);
    expect(buildActorGraphSeries(actor, "damage_taken", 2, "#fff", "boss-1").average)
      .toBe(25);
    expect(buildActorGraphSeries(actor, "effective_healing", 2, "#fff", "party-2").average)
      .toBe(125);
    expect(buildActorGraphSeries(actor, "damage", 2, "#fff", "missing").peak).toBe(0);
  });

  it("anchors the range to every party series instead of only visible lines", () => {
    const completeParty = [
      [0, 10, 20],
      [0, 250, 100],
      [0, 75, 50],
    ];

    expect(graphScaleMaximum(completeParty)).toBe(250);
    expect(graphScaleMaximum(completeParty)).toBe(
      graphScaleMaximum(completeParty.filter((_, index) => index !== 1).concat([completeParty[1]!])),
    );
  });

  it("keeps a non-zero baseline for an empty or zero-only segment", () => {
    expect(graphScaleMaximum([])).toBe(1);
    expect(graphScaleMaximum([[0, 0, 0]])).toBe(1);
  });
});

describe("Combat History participant colors", () => {
  const actor = (actorId: string, specializationId: number | null) => ({
    actor_id: actorId,
    specialization_id: specializationId,
  });

  it("keeps randomized run colors stable and distinct", () => {
    const settings = {
      historyPartyColorMode: "randomized" as const,
      historySpecializationColors: {},
    };
    const colors = Array.from({ length: 5 }, (_, index) =>
      historyActorColor(actor(String(index), 100 + index), index, settings, "run-a"),
    );
    expect(new Set(colors).size).toBe(5);
    expect(historyActorColor(actor("0", 100), 0, settings, "run-a")).toBe(colors[0]);
  });

  it("uses stable numeric specialization IDs for custom colors", () => {
    const settings = {
      historyPartyColorMode: "specialization" as const,
      historySpecializationColors: { "117": "#f97316" },
    };
    expect(historyActorColor(actor("a", 117), 0, settings, "run-a")).toBe("#f97316");
    expect(historyActorColor(actor("b", 117), 4, settings, "run-b")).toBe("#f97316");
    expect(historyActorColor(actor("c", 116), 0, settings, "run-a")).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe("Combat History target identity", () => {
  it("keeps localized monsters distinguishable by runtime entity UUID", () => {
    expect(
      historyTargetLabel({
        actor_id: "2",
        entity_uuid: "6818431040",
        monster_id: "33701",
        display_name: null,
        actor_kind: "monster",
        presentation_name: "Tina - Void Reverie",
      }),
    ).toBe("Tina - Void Reverie · Entity 6818431040");
  });

  it("does not mislabel unresolved projectile identities as monsters", () => {
    expect(
      historyTargetLabel({
        actor_id: "3",
        entity_uuid: "1212800",
        monster_id: "2000208",
        display_name: null,
        actor_kind: "projectile",
        presentation_name: null,
      }),
    ).toBe("Projectile 2000208 · Entity 1212800");
  });
});

describe("Combat History archive index", () => {
  const entry = (
    historyId: string,
    captured: number,
    name: string,
    characterId: string,
    dps: number,
  ): CombatHistoryCatalogEntry => ({
    history_id: historyId,
    is_favorite: false,
    session_id: historyId,
    run_index: 0,
    captured_unix_millis: captured,
    activity_id: "scene.1632",
    activity_family_id: "tina-mindrealm",
    scene_id: 1632,
    presentation_scene_name: "Tina Mindrealm",
    difficulty_family: "hard",
    difficulty_tier: null,
    terminal_state: "completed",
    game_time_micros: 200,
    total_run_time_micros: 250,
    active_combat_micros: 100,
    player_count: 1,
    deployment_id: "global",
    region_id: "north-america",
    world_id: "asteria",
    team_damage: 1_000,
    team_dps: dps,
    team_encounter_dps: dps * 2,
    true_time_micros: 150,
    retry_count: 1,
    boss_retry_count: 1,
    wipe_count: 1,
    cleared_encounter_count: 1,
    last_encounter_terminal_state: "cleared",
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
        ability_score: 61_382,
        weapon_item_id: 2_000_631,
        weapon_breakthrough_count: 3,
        weapon_icon_asset_path: "/game-assets/blue-protocol-star-resonance/shared/icons/weapons/items/ch_wp_rodri_06_01.png",
        weapon_presentation_name: "Ember - Gaze of the Far Sea",
        weapon_level: 280,
        weapon_level_min: 220,
        weapon_level_max: 280,
        weapon_badge_kind: "ember_far_sea",
        seasonal_score: 3505,
        primary_loadout: [],
        auxiliary_loadout: [],
        damage: 1_000,
        dps,
        encounter_dps: dps * 2,
        character_id: characterId,
        presentation_name: name,
        presentation_kind: "player",
        icon_asset_path: null,
        presentation_role: "damage",
        presentation_accent: null,
      },
    ],
  });

  it("searches compact participant names and stable UIDs", () => {
    const entries = [
      entry("older", 1, "MarieRose", "3296036", 10),
      entry("newer", 2, "SomeoneElse", "123", 20),
    ];

    expect(filterAndSortHistoryEntries(entries, "MarieRose", "all", "newest")[0]?.history_id)
      .toBe("older");
    expect(filterAndSortHistoryEntries(entries, "3296036", "all", "newest")[0]?.history_id)
      .toBe("older");
  });

  it("sorts precomputed archive metrics without loading run detail", () => {
    const entries = [
      entry("older", 1, "MarieRose", "3296036", 10),
      entry("newer", 2, "SomeoneElse", "123", 20),
    ];

    expect(filterAndSortHistoryEntries(entries, "", "all", "team_dps").map((run) => run.history_id))
      .toEqual(["newer", "older"]);
  });

  it("filters the compact index to favorites without loading run detail", () => {
    const favorite = entry("favorite", 1, "MarieRose", "3296036", 10);
    favorite.is_favorite = true;
    const entries = [favorite, entry("ordinary", 2, "SomeoneElse", "123", 20)];

    expect(
      filterAndSortHistoryEntries(entries, "", "all", "newest", true).map(
        (run) => run.history_id,
      ),
    ).toEqual(["favorite"]);
  });
});
