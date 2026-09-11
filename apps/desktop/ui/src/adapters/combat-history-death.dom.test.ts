// @vitest-environment happy-dom

import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import { loadUiLocalizer } from "../localization/ui-locale";
import type {
  CombatHistoryCatalog,
  CombatHistorySnapshot,
  CombatHistoryView,
  HistoryActorSummary,
  HistoryDeathEvent,
} from "./combat-history";
import {
  historyDeathMarker,
  historyDeathSummary,
  mountCombatHistorySurface,
  renderMetricGraph,
  wireHistoryDeathSummaries,
} from "./combat-history-surface";

describe("Combat History death presentation", () => {
  const death = (): HistoryDeathEvent => ({
    at_micros: 3_500_123,
    cause: {
      evidence: "packet_terminal_damage",
      final_hit: {
        at_micros: 3_500_123,
        source_actor_id: "9",
        source_entity_uuid: "private-909",
        source_presentation: {
          actor_id: "9",
          name: "Tina - Void Reverie",
          provenance: "exact_build_monster_catalog",
        },
        ability_id: "5",
        breakdown_ability_id: "2233",
        ability_presentation: {
          ability_id: "2233",
          name: "Powerdraw",
          provenance: "exact_build_action_catalog",
        },
        reported_damage: 100,
        effective_damage: 90,
        critical: true,
      },
      prior_hits: [],
      prior_hits_truncated: false,
    },
  });

  it("exposes localized cause plus raw IDs to hover and keyboard focus", async () => {
    const ui = await loadUiLocalizer("en-US");
    const summary = historyDeathSummary("Alice", death(), ui);
    const marker = historyDeathMarker(10, 20, summary, "#35c2ff");

    expect(summary).toContain("Alice died at 0:03.500");
    expect(summary).toContain("Powerdraw [ability 2233]");
    expect(summary).toContain("Tina - Void Reverie [actor 9]");
    expect(marker.getAttribute("tabindex")).toBe("0");
    expect(marker.getAttribute("role")).toBe("img");
    expect(marker.getAttribute("aria-label")).toBe(summary);
    expect(marker.style.getPropertyValue("--death-marker-color")).toBe("#35c2ff");
    expect(marker.querySelector(".combat-history-death-marker-skull")?.tagName.toLowerCase()).toBe("path");
    expect(marker.querySelector(".combat-history-death-marker-bones")?.tagName.toLowerCase()).toBe("path");
    expect(marker.textContent).not.toContain("☠");
    expect(marker.querySelector("title")?.textContent).toBe(summary);
  });

  it("uses the neutral fallback only when a death is genuinely unscoped", () => {
    const marker = historyDeathMarker(10, 20, "Unscoped death", null);
    const styles = readFileSync("src/styles/shell.css", "utf8");

    expect(marker.style.getPropertyValue("--death-marker-color")).toBe("");
    expect(marker.querySelector(".combat-history-death-marker-halo")).toBeNull();
    expect(marker.querySelector(".combat-history-death-marker-hitbox")).not.toBeNull();
    expect(styles).toMatch(/\.combat-history-death-marker-hitbox\s*\{[^}]*fill:\s*transparent;[^}]*stroke:\s*none/su);
    expect(styles).toMatch(/\.combat-history-death-marker-skull\s*\{[^}]*fill:\s*var\(--death-marker-color,\s*#d8cfb4\)/su);
    expect(styles).toMatch(/\.combat-history-death-marker-bones\s*\{[^}]*stroke:\s*var\(--death-marker-color,\s*#d8cfb4\)/su);
  });

  it("keeps participant color and visibility coupled to the matching graph series", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice",
      display_name: "Alice",
      actor_kind: "player",
      death_events: [death()],
      death_seconds: [],
      skill_events: [{ at_micros: 1_250_000, ability_id: "2233" }],
      status_events: [
        { at_micros: 500_000, effect_id: "11", instance_id: "a", state: "applied" },
        { at_micros: 1_000_000, effect_id: "11", instance_id: "a", state: "removed" },
        { at_micros: 2_000_000, effect_id: "12", instance_id: "b", state: "applied" },
        { at_micros: 3_000_000, effect_id: "12", instance_id: "b", state: "consumed" },
      ],
      abilities: [{
        ability_id: "2233", presentation_name: "Powerdraw",
        icon_asset_path: "/assets/bpsr/current/abilities/2233.webp",
      }],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
      targets: [],
    } as unknown as HistoryActorSummary;
    const render = (hidden: ReadonlySet<string>) => renderMetricGraph(
      [actor],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_500_123,
      hidden,
      new Map([[actor.actor_id, "#35c2ff"]]),
      null,
      () => undefined,
      ui,
      {
        actors: [actor],
        hostile_casts: [],
        status_effect_presentations: [{
          effect_id: "11", presentation_name: "Reviewed Ward",
          presentation_kind: "status-effect", presentation_resolution: "localized-status-effect",
          icon_asset_path: null,
        }],
      } as unknown as CombatHistoryView,
    );

    const visible = render(new Set());
    expect(visible.querySelector(".combat-history-character-line")?.getAttribute("stroke")).toBe("#35c2ff");
    expect((visible.querySelector(".combat-history-death-marker") as SVGElement).style
      .getPropertyValue("--death-marker-color")).toBe("#35c2ff");
    expect(visible.querySelector(".combat-history-death-summary")?.getAttribute("role")).toBe("tooltip");
    expect(visible.querySelector(".combat-history-event-lanes")).not.toBeNull();
    expect(visible.querySelector(".combat-history-skill-event")?.getAttribute("aria-label"))
      .toContain("Alice used Powerdraw at 0:01.250");
    expect((visible.querySelector(".combat-history-skill-event") as SVGElement).style
      .getPropertyValue("--series-color")).toBe("#35c2ff");
    expect(visible.querySelector(".combat-history-skill-event-icon")?.getAttribute("href"))
      .toBe("/assets/bpsr/current/abilities/2233.webp");
    expect(visible.querySelector(".combat-history-skill-event-glyph")).toBeNull();
    const statusSpans = visible.querySelectorAll<SVGGElement>(".combat-history-status-span");
    expect(statusSpans).toHaveLength(2);
    expect(statusSpans[0]!.getAttribute("aria-label"))
      .toContain("Reviewed Ward applied at 0:00.500 and removed at 0:01.000");
    expect(statusSpans[1]!.getAttribute("aria-label"))
      .toContain("Unlocalized combat effect #12 applied at 0:02.000 and consumed at 0:03.000");
    expect(visible.querySelector("[data-timeline-play]")).toBeNull();

    const hidden = render(new Set([actor.actor_id]));
    expect(hidden.querySelector(".combat-history-character-line")).toBeNull();
    expect(hidden.querySelector(".combat-history-death-marker")).toBeNull();
    expect(hidden.querySelector(".combat-history-status-span")).toBeNull();
    expect(hidden.querySelector(".combat-history-event-lanes")).toBeNull();
  });

  it("uses the real legend toggle to hide and restore both the graph trace and player event lane", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice", entity_uuid: "101", monster_id: null, character_id: "501",
      display_name: "Alice", actor_kind: "player", presentation_name: "Alice",
      presentation_kind: "player", class_id: null, specialization_id: null,
      presentation_class_name: null, presentation_specialization_name: null,
      icon_asset_path: null, presentation_role: null, presentation_accent: null,
      level: null, ability_score: null, weapon_item_id: null, weapon_breakthrough_count: null,
      weapon_icon_asset_path: null, weapon_presentation_name: null, weapon_level: null,
      weapon_level_min: null, weapon_level_max: null, weapon_badge_kind: null,
      seasonal_score: null, primary_loadout: [], auxiliary_loadout: [],
      damage: 100, effective_damage: 100, damage_taken: 0, healing: 0,
      effective_healing: 0, shielding: 0, hits: 1, critical_hits: 0, deaths: 1,
      death_seconds: [], death_events: [death()],
      skill_events: [{ at_micros: 1_250_000, ability_id: "2233" }],
      status_events: [], dps: 25, encounter_dps: 25, hps: 0, tps: 0,
      rdps: null, rdps_damage: null, rdps_contribution_given: null,
      rdps_contribution_received: null, rdps_incomplete: true, apm: null,
      observed_cast_events: 1,
      abilities: [{
        ability_id: "2233", presentation_name: "Powerdraw", presentation_kind: "skill",
        presentation_resolution: "localized-action", icon_asset_path: null,
        presentation_recount_group_id: null, presentation_recount_group_name: null,
        damage: 100, effective_damage: 100, damage_taken: 0, healing: 0,
        effective_healing: 0, shielding: 0, hits: 1, critical_hits: 0, casts: 1,
        targets: [], effects: [],
      }],
      targets: [], effects: [],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
    } as unknown as HistoryActorSummary;
    const view = {
      id: "all", label: "Entire run", kind: "run", segment_indices: [0],
      elapsed_micros: 4_000_000, active_combat_micros: 4_000_000,
      rate_clock: [], rate_clock_complete: false, actors: [actor], targets: [], hostile_casts: [],
      damage_influences: [], rdps_effect_presentations: [], status_effect_presentations: [],
    } as CombatHistoryView;
    const snapshot = {
      schema_version: 1, session_id: "session-1", deployment_id: "global", region_id: "global",
      world_id: null, client_build: "24687926", protocol_pack_digest: "pack",
      rdps_formula_identity: null,
      runs: [{
        run_index: 0, activity_id: "scene.1", activity_family_id: null, scene_id: 1,
        presentation_scene_name: "Test Run", instance_id: "instance-1", difficulty_family: null,
        difficulty_tier: null, terminal_state: "completed", entered_micros: 0,
        started_micros: 0, first_combat_micros: 0, ended_micros: 4_000_000,
        load_time_micros: 0, precombat_time_micros: 0, total_run_time_micros: 4_000_000,
        game_time_micros: 4_000_000, true_time_micros: 4_000_000, retry_count: 0,
        boss_retry_count: 0, wipe_count: 0, cleared_encounter_count: 1,
        last_encounter_terminal_state: "completed", rdps_status: "unavailable",
        apm_status: "unavailable", views: [view],
      }],
    } as CombatHistorySnapshot;
    const catalog = {
      schema_version: 1,
      entries: [{
        history_id: "history-1", is_favorite: false, session_id: "session-1", run_index: 0,
        captured_unix_millis: 1, activity_id: "scene.1", activity_family_id: null,
        scene_id: 1, presentation_scene_name: "Test Run", difficulty_family: null,
        difficulty_tier: null, terminal_state: "completed", game_time_micros: 4_000_000,
        total_run_time_micros: 4_000_000, active_combat_micros: 4_000_000, player_count: 1,
        deployment_id: "global", client_build: "24687926", protocol_pack_digest: "pack",
        region_id: "global", world_id: null, team_damage: 100, team_dps: 25,
        team_encounter_dps: 25, true_time_micros: 4_000_000, retry_count: 0,
        boss_retry_count: 0, wipe_count: 0, cleared_encounter_count: 1,
        last_encounter_terminal_state: "completed", participants: [actor],
      }],
    } as unknown as CombatHistoryCatalog;
    vi.stubGlobal("Option", function Option(text = "", value = "") {
      const option = document.createElement("option");
      option.text = text;
      option.value = value;
      return option;
    });
    const container = document.createElement("main");
    document.body.append(container);
    const mounted = mountCombatHistorySurface(container, async () => catalog, async () => snapshot, ui);
    try {
      await new Promise((resolve) => setTimeout(resolve, 0));
      container.querySelector<HTMLElement>(".combat-history-run-button")!.click();
      await new Promise((resolve) => setTimeout(resolve, 0));

      let toggle = container.querySelector<HTMLButtonElement>(".combat-history-series-toggle")!;
      expect(toggle, container.textContent ?? "").not.toBeNull();
      const contextKicker = container.querySelector<HTMLElement>(".run-report-kicker")!;
      expect(contextKicker.textContent).toBe("global");
      expect(contextKicker.title).toBe("Scene ID 1");
      expect(contextKicker.dataset.sceneId).toBe("1");
      expect(toggle.getAttribute("aria-pressed")).toBe("true");
      expect(container.querySelector(".combat-history-character-line")).not.toBeNull();
      expect(container.querySelector(".combat-history-player-event-lane")).not.toBeNull();
      expect(container.querySelector(".combat-history-skill-event")).not.toBeNull();
      expect(container.querySelector(".combat-history-death-marker")).not.toBeNull();
      expect(container.querySelector("[data-timeline-play]")).toBeNull();

      toggle.click();
      toggle = container.querySelector<HTMLButtonElement>(".combat-history-series-toggle")!;
      expect(toggle.getAttribute("aria-pressed")).toBe("false");
      expect(container.querySelector(".combat-history-character-line")).toBeNull();
      expect(container.querySelector(".combat-history-player-event-lane")).toBeNull();
      expect(container.querySelector(".combat-history-skill-event")).toBeNull();
      expect(container.querySelector(".combat-history-death-marker")).toBeNull();

      toggle.click();
      toggle = container.querySelector<HTMLButtonElement>(".combat-history-series-toggle")!;
      expect(toggle.getAttribute("aria-pressed")).toBe("true");
      expect(container.querySelector(".combat-history-character-line")).not.toBeNull();
      expect(container.querySelector(".combat-history-player-event-lane")).not.toBeNull();
    } finally {
      mounted.dispose();
      container.remove();
      vi.unstubAllGlobals();
    }
  });

  it("keeps the participant event lane when the selected metric has no trace", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice", display_name: "Alice", actor_kind: "player",
      death_events: [death()], death_seconds: [],
      skill_events: [{ at_micros: 1_250_000, ability_id: "2233" }],
      status_events: [
        { at_micros: 500_000, effect_id: "11", instance_id: "ward", state: "applied" },
        { at_micros: 1_000_000, effect_id: "11", instance_id: "ward", state: "removed" },
      ],
      abilities: [{ ability_id: "2233", presentation_name: "Powerdraw" }],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
      targets: [],
    } as unknown as HistoryActorSummary;
    const render = (metric: "damage" | "rdps", hidden = new Set<string>()) =>
      renderMetricGraph(
        [actor],
        { metric, title: metric, rateLabel: metric === "damage" ? "DPS" : "rDPS", description: metric },
        3_500_123, hidden, new Map([[actor.actor_id, "#35c2ff"]]), null,
        () => undefined, ui,
      );

    const damage = render("damage");
    expect(damage.querySelector(".combat-history-character-line")).not.toBeNull();
    expect(damage.querySelector(".combat-history-player-event-lane")).not.toBeNull();

    const rdps = render("rdps");
    expect(rdps.querySelector(".combat-history-character-line")).toBeNull();
    expect(rdps.querySelector(".combat-history-player-event-lane")).not.toBeNull();
    expect(rdps.querySelector(".combat-history-skill-event")).not.toBeNull();
    expect(rdps.querySelector(".combat-history-status-span")).not.toBeNull();
    expect(rdps.querySelector(".combat-history-death-marker")).not.toBeNull();

    const hidden = render("rdps", new Set([actor.actor_id]));
    expect(hidden.querySelector(".combat-history-player-event-lane")).toBeNull();
    expect(hidden.querySelector(".combat-history-skill-event")).toBeNull();
    expect(render("rdps").querySelector(".combat-history-player-event-lane"))
      .not.toBeNull();
  });

  it("rescales the graph from visible traces only", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = (actorId: string, damage: number) => ({
      actor_id: actorId, display_name: actorId, actor_kind: "player",
      death_events: [], death_seconds: [], skill_events: [], status_events: [], abilities: [],
      series: [{ second: 0, damage, effective_healing: 0, damage_taken: 0 }], targets: [],
    }) as unknown as HistoryActorSummary;
    const dominant = actor("dominant", 10_000);
    const remaining = actor("remaining", 100);
    const render = (hidden: ReadonlySet<string>) => renderMetricGraph(
      [dominant, remaining],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      1_000_000, hidden,
      new Map([[dominant.actor_id, "#35c2ff"], [remaining.actor_id, "#ffcc66"]]),
      null, () => undefined, ui,
    );
    const maximumLabel = (rendered: HTMLElement) =>
      [...rendered.querySelectorAll(".combat-history-y-label")].at(-1)?.textContent;

    expect(maximumLabel(render(new Set()))).toBe("10K");
    const dominantHidden = render(new Set([dominant.actor_id]));
    expect(dominantHidden.querySelectorAll(".combat-history-character-line")).toHaveLength(1);
    expect(maximumLabel(dominantHidden)).toBe("100");
  });

  it("uses canonical fractional time and complete clocks for visible eDPS and aDPS cursor rates", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = (actorId: string, scale: number) => ({
      actor_id: actorId, display_name: actorId, actor_kind: "player",
      death_events: [], death_seconds: [], skill_events: [], status_events: [], abilities: [], targets: [],
      series: [
        { second: 0, damage: 100 * scale, effective_healing: 0, damage_taken: 0 },
        { second: 1, damage: 200 * scale, effective_healing: 0, damage_taken: 0 },
        { second: 2, damage: 50 * scale, effective_healing: 0, damage_taken: 0 },
      ],
    }) as unknown as HistoryActorSummary;
    const alice = actor("Alice", 1);
    const bob = actor("Bob", 0.5);
    const hidden = actor("Hidden", 0.25);
    alice.death_events = [{ at_micros: 2_500_000, cause: null }];
    const view = {
      elapsed_micros: 2_500_000, active_combat_micros: 1_500_000,
      rate_clock_complete: true,
      rate_clock: [
        { second: 0, edps_elapsed_micros: 1_000_000, adps_elapsed_micros: 1_000_000 },
        { second: 1, edps_elapsed_micros: 2_000_000, adps_elapsed_micros: 1_000_000 },
        { second: 2, edps_elapsed_micros: 2_500_000, adps_elapsed_micros: 1_500_000 },
      ],
      actors: [alice, bob, hidden], hostile_casts: [],
    } as unknown as CombatHistoryView;
    const rendered = renderMetricGraph(
      [alice, bob, hidden],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      view.elapsed_micros, new Set([hidden.actor_id]),
      new Map([[alice.actor_id, "#35c2ff"], [bob.actor_id, "#ffcc66"], [hidden.actor_id, "#ff6688"]]),
      null, () => undefined, ui, view,
    );
    const chart = rendered.querySelector<SVGSVGElement>(".combat-history-chart")!;
    chart.focus();
    chart.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    const readout = rendered.querySelector(".combat-history-graph-inspection-readout")!;

    expect(readout.querySelector("strong")?.textContent).toBe("0:02.500");
    expect(readout.textContent).toContain("Visible total · 1s eDPS/aDPS 150 / 150");
    expect(readout.textContent).toContain("5s 210 / 350 · 10s 210 / 350 · run 210 / 350");
    expect(readout.textContent).toContain("Alice · 1s eDPS/aDPS 100 / 100");
    expect(readout.textContent).toContain("Bob · 1s eDPS/aDPS 50 / 50");
    expect(readout.textContent).not.toContain("Hidden");
    expect(rendered.querySelector("[data-timeline-play]")).toBeNull();
    expect([...rendered.querySelectorAll(".combat-history-x-label")].at(-1)?.textContent).toBe("0:02.500");
    const graphEndpoint = rendered.querySelector(".combat-history-character-line")
      ?.getAttribute("points")?.trim().split(" ").at(-1)?.split(",")[0];
    const laneEndpoint = rendered.querySelector(".combat-history-event-lanes .combat-history-death-marker")
      ?.getAttribute("transform")?.match(/^translate\(([^ ]+)/u)?.[1];
    expect(graphEndpoint).toBe("1096.00");
    expect(laneEndpoint).toBe(graphEndpoint);
  });

  it("does not substitute wall-time DPS pairs when the canonical rate clock is incomplete", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "Alice", display_name: "Alice", actor_kind: "player",
      death_events: [], death_seconds: [], skill_events: [], status_events: [], abilities: [], targets: [],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
    } as unknown as HistoryActorSummary;
    const rendered = renderMetricGraph(
      [actor], { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      1_000_000, new Set(), new Map([[actor.actor_id, "#35c2ff"]]), null,
      () => undefined, ui,
      { elapsed_micros: 1_000_000, active_combat_micros: 1_000_000, rate_clock_complete: false,
        rate_clock: [], actors: [actor] } as unknown as CombatHistoryView,
    );
    const chart = rendered.querySelector<SVGSVGElement>(".combat-history-chart")!;
    chart.focus();
    chart.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));

    expect(rendered.querySelector(".combat-history-graph-inspection-readout")?.textContent)
      .toContain("eDPS/aDPS unavailable (complete rate clock required)");
    expect(rendered.querySelector(".combat-history-graph-inspection-readout")?.textContent)
      .not.toContain("100 / 100");
  });

  it("discloses every dense exact skill start by keyboard and keeps player visibility coupled", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice", display_name: "Alice", actor_kind: "player",
      death_events: [], death_seconds: [3],
      skill_events: [
        { at_micros: 1_250_000, ability_id: "2233" },
        { at_micros: 1_255_000, ability_id: "9999" },
      ],
      abilities: [{ ability_id: "2233", presentation_name: "Powerdraw" }],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
      targets: [],
    } as unknown as HistoryActorSummary;
    const render = (hidden: ReadonlySet<string>) => renderMetricGraph(
      [actor],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_500_000, hidden, new Map([[actor.actor_id, "#35c2ff"]]), null, () => undefined, ui,
    );
    const rendered = render(new Set());
    document.body.append(rendered);

    const skills = rendered.querySelectorAll<SVGGElement>(".combat-history-skill-event");
    expect(skills).toHaveLength(1);
    expect(skills[0]!.dataset.eventCount).toBe("2");
    expect(skills[0]!.getAttribute("aria-label")).toContain("2 recorded skill starts");
    expect(skills[0]!.getAttribute("role")).toBe("button");
    expect(skills[0]!.getAttribute("aria-expanded")).toBe("false");
    expect(skills[0]!.querySelector(".combat-history-skill-event-badge-text")?.textContent).toBe("2");
    const disclosure = rendered.querySelector<HTMLElement>(".combat-history-skill-disclosure")!;
    expect(disclosure.hidden).toBe(true);
    expect(skills[0]!.getAttribute("aria-controls")).toBe(disclosure.id);

    skills[0]!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(skills[0]!.getAttribute("aria-expanded")).toBe("true");
    expect(disclosure.hidden).toBe(false);
    const exactEvents = [...disclosure.querySelectorAll<HTMLButtonElement>("button")];
    expect(exactEvents).toHaveLength(2);
    expect(exactEvents[0]!.textContent).toBe("Alice used Powerdraw at 0:01.250");
    expect(exactEvents[1]!.textContent).toBe("Alice used Ability 9999 at 0:01.255");
    exactEvents[1]!.focus();
    expect(document.activeElement).toBe(exactEvents[1]);
    exactEvents[1]!.click();
    expect(exactEvents[0]!.getAttribute("aria-pressed")).toBe("false");
    expect(exactEvents[1]!.getAttribute("aria-pressed")).toBe("true");
    expect(skills[0]!.classList.contains("is-event-selected")).toBe(true);
    expect(skills[0]!.dataset.selectedSkillEvent).toBe("1");
    expect(disclosure.querySelector("output")?.textContent)
      .toBe("Alice used Ability 9999 at 0:01.255");
    expect(rendered.querySelector("[data-timeline-play]")).toBeNull();

    exactEvents[1]!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(disclosure.hidden).toBe(true);
    expect(skills[0]!.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(skills[0]);

    const deaths = rendered.querySelectorAll<SVGGElement>(".combat-history-event-lanes .combat-history-death-marker");
    expect(deaths).toHaveLength(1);
    expect(deaths[0]!.getAttribute("aria-label"))
      .toContain("death observed in the 0:03.000–0:03.500 one-second bucket");

    const hidden = render(new Set([actor.actor_id]));
    expect(hidden.querySelector(".combat-history-skill-event")).toBeNull();
    expect(hidden.querySelector(".combat-history-skill-disclosure")).toBeNull();
    rendered.remove();
  });

  it("keeps a neutral glyph for mixed-ability clusters instead of showing a misleading icon", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice", display_name: "Alice", actor_kind: "player",
      death_events: [], death_seconds: [],
      skill_events: [
        { at_micros: 1_250_000, ability_id: "2233" },
        { at_micros: 1_255_000, ability_id: "2244" },
      ],
      abilities: [
        { ability_id: "2233", presentation_name: "Powerdraw", icon_asset_path: "/assets/2233.webp" },
        { ability_id: "2244", presentation_name: "Volley", icon_asset_path: "/assets/2244.webp" },
      ],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }], targets: [],
    } as unknown as HistoryActorSummary;
    const rendered = renderMetricGraph(
      [actor],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_500_000, new Set(), new Map([[actor.actor_id, "#35c2ff"]]), null, () => undefined, ui,
    );

    expect(rendered.querySelectorAll(".combat-history-skill-event")).toHaveLength(1);
    expect(rendered.querySelector(".combat-history-skill-event-icon")).toBeNull();
    expect(rendered.querySelector(".combat-history-skill-event-glyph")).not.toBeNull();
    expect(rendered.querySelector(".combat-history-skill-event")?.getAttribute("aria-label"))
      .toContain("Powerdraw, Volley");
  });

  it("renders hostile-source cast lanes before player lanes and outside player visibility controls", async () => {
    const ui = await loadUiLocalizer("en-US");
    const player = {
      actor_id: "alice", display_name: "Alice", presentation_name: "Alice", actor_kind: "player",
      death_events: [], death_seconds: [],
      skill_events: [{ at_micros: 1_250_000, ability_id: "100" }],
      abilities: [{ ability_id: "100", presentation_name: "Player action" }],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }], targets: [],
    } as unknown as HistoryActorSummary;
    const hostile = {
      actor_id: "9", monster_id: "33701", actor_kind: "monster",
      display_name: "Packet creature", presentation_name: "Tina - Void Reverie",
      abilities: [{
        ability_id: "2233", presentation_name: "Powerdraw",
        presentation_resolution: "reviewed-action", icon_asset_path: "/assets/2233.webp",
      }],
    } as unknown as HistoryActorSummary;
    const view = {
      actors: [player, hostile],
      targets: [{ actor_id: "9", presentation_name: "Tina - Void Reverie" }],
      hostile_casts: [{
        source_actor_id: "9", hostility_evidence: "participant_outgoing_target",
        target_actor_id: "alice", at_micros: 750_000, action_id: "2233", state: "started",
      }],
    } as unknown as CombatHistoryView;
    const render = (hidden: ReadonlySet<string>) => renderMetricGraph(
      [player],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_000_000, hidden, new Map([[player.actor_id, "#35c2ff"]]), null, () => undefined, ui, view,
    );

    const visible = render(new Set());
    expect([...visible.querySelectorAll<SVGGElement>(".combat-history-event-lane")]
      .map((lane) => lane.dataset.laneKind)).toEqual(["hostile", "player"]);
    const hostileCast = visible.querySelector(".combat-history-hostile-cast-event");
    expect(hostileCast?.getAttribute("aria-label"))
      .toBe("Tina - Void Reverie began Powerdraw at 0:00.750 targeting Alice");
    expect(hostileCast?.querySelector(".combat-history-skill-event-icon")?.getAttribute("href"))
      .toBe("/assets/2233.webp");

    const playerHidden = render(new Set([player.actor_id]));
    expect(playerHidden.querySelector(".combat-history-player-event-lane")).toBeNull();
    expect(playerHidden.querySelector(".combat-history-hostile-event-lane")).not.toBeNull();

    const hostileHidden = renderMetricGraph(
      [player],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_000_000, new Set(), new Map([[player.actor_id, "#35c2ff"]]), null,
      () => undefined, ui, view, false,
    );
    expect(hostileHidden.querySelector(".combat-history-hostile-event-lane")).toBeNull();
    expect(hostileHidden.querySelector(".combat-history-player-event-lane")).not.toBeNull();
  });

  it("uses neutral hostile cast labels and no icon without trusted presentation", async () => {
    const ui = await loadUiLocalizer("en-US");
    const view = {
      actors: [{
        actor_id: "9", monster_id: "33701", actor_kind: "monster",
        display_name: "Untrusted packet name", presentation_name: "Untrusted packet name",
        abilities: [{ ability_id: "2233", presentation_name: "Untrusted action", presentation_resolution: null }],
      }],
      targets: [{ actor_id: "9" }],
      hostile_casts: [{
        source_actor_id: "9", hostility_evidence: "participant_outgoing_target",
        at_micros: 750_000, action_id: "2233", state: "started",
      }],
    } as unknown as CombatHistoryView;
    const rendered = renderMetricGraph(
      [],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_000_000, new Set(["9"]), new Map(), null, () => undefined, ui, view,
    );

    const cast = rendered.querySelector(".combat-history-hostile-cast-event");
    expect(cast?.getAttribute("aria-label"))
      .toBe("Hostile source 9 began Action 2233 at 0:00.750");
    expect(cast?.querySelector(".combat-history-skill-event-icon")).toBeNull();
    expect(cast?.querySelector(".combat-history-hostile-cast-event-glyph")).not.toBeNull();
  });

  it("shows the same cause summary on pointer hover and keyboard focus", async () => {
    const ui = await loadUiLocalizer("en-US");
    const summary = historyDeathSummary("Alice", death(), ui);
    const marker = historyDeathMarker(10, 20, summary, "#35c2ff");
    const preview = document.createElement("div");
    preview.id = "death-summary";
    preview.hidden = true;
    wireHistoryDeathSummaries([marker], preview);

    marker.dispatchEvent(new PointerEvent("pointerenter"));
    expect(preview.hidden).toBe(false);
    expect(preview.textContent).toBe(summary);
    expect(marker.getAttribute("aria-describedby")).toBe(preview.id);
    marker.dispatchEvent(new PointerEvent("pointerleave"));
    expect(preview.hidden).toBe(true);

    marker.dispatchEvent(new FocusEvent("focus"));
    expect(preview.hidden).toBe(false);
    expect(preview.textContent).toContain("Final hit: Powerdraw");
    marker.dispatchEvent(new FocusEvent("blur"));
    expect(preview.hidden).toBe(true);
    expect(marker.hasAttribute("aria-describedby")).toBe(false);
  });

  it("keeps malicious presentation text inert", async () => {
    const ui = await loadUiLocalizer("en-US");
    const event = death();
    event.cause!.final_hit.source_presentation!.name = "Boss <script>bad()</script>";
    const marker = historyDeathMarker(10, 20, historyDeathSummary("Alice", event, ui));

    expect(marker.querySelector("script")).toBeNull();
    expect(marker.querySelector("title")?.textContent).toContain("<script>bad()</script>");
  });

  it("retains a useful fallback when exact cause is unavailable", async () => {
    const ui = await loadUiLocalizer("en-US");
    expect(historyDeathSummary("Alice", { at_micros: 3_000_000, cause: null }, ui))
      .toBe("Alice died at 0:03.000; cause unavailable.");
  });

  it("preserves capped half-open timing for legacy one-second observations", async () => {
    const ui = await loadUiLocalizer("en-US");
    expect(historyDeathSummary(
      "Alice",
      { at_micros: 3_000_000, cause: null },
      ui,
      "one_second_bucket",
      3_500_000,
    )).toBe("Alice death observed in the 0:03.000–0:03.500 one-second bucket; cause unavailable.");
  });

  it("falls back to shipped English prose while preserving requested-locale number formatting", async () => {
    const ui = await loadUiLocalizer("de-DE");
    const event = death();
    event.cause!.final_hit.reported_damage = 1_234;
    event.cause!.final_hit.effective_damage = 1_200;

    expect(ui.loadedLocales).toEqual(["en-US"]);
    expect(historyDeathSummary("Alice", event, ui)).toContain(
      "1.234 damage, 1.200 effective, critical.",
    );
  });
});
