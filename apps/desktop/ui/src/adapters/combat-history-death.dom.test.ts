// @vitest-environment happy-dom

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

import { loadUiLocalizer } from "../localization/ui-locale";
import type { HistoryActorSummary, HistoryDeathEvent } from "./combat-history";
import {
  historyDeathMarker,
  historyDeathSummary,
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
    expect(visible.querySelector("[data-timeline-play]")).toBeNull();

    const hidden = render(new Set([actor.actor_id]));
    expect(hidden.querySelector(".combat-history-character-line")).toBeNull();
    expect(hidden.querySelector(".combat-history-death-marker")).toBeNull();
    expect(hidden.querySelector(".combat-history-event-lanes")).toBeNull();
  });

  it("clusters dense exact skill starts truthfully and keeps legacy deaths bucketed", async () => {
    const ui = await loadUiLocalizer("en-US");
    const actor = {
      actor_id: "alice", display_name: "Alice", actor_kind: "player",
      death_events: [], death_seconds: [3],
      skill_events: [
        { at_micros: 1_250_000, ability_id: "2233" },
        { at_micros: 1_255_000, ability_id: "2233" },
      ],
      abilities: [{ ability_id: "2233", presentation_name: "Powerdraw" }],
      series: [{ second: 0, damage: 100, effective_healing: 0, damage_taken: 0 }],
      targets: [],
    } as unknown as HistoryActorSummary;
    const rendered = renderMetricGraph(
      [actor],
      { metric: "damage", title: "Damage", rateLabel: "DPS", description: "Damage rate" },
      3_500_000, new Set(), new Map([[actor.actor_id, "#35c2ff"]]), null, () => undefined, ui,
    );

    const skills = rendered.querySelectorAll<SVGGElement>(".combat-history-skill-event");
    expect(skills).toHaveLength(1);
    expect(skills[0]!.dataset.eventCount).toBe("2");
    expect(skills[0]!.getAttribute("aria-label")).toContain("2 recorded skill starts");
    expect(skills[0]!.querySelector(".combat-history-skill-event-badge-text")?.textContent).toBe("2");
    const deaths = rendered.querySelectorAll<SVGGElement>(".combat-history-event-lanes .combat-history-death-marker");
    expect(deaths).toHaveLength(1);
    expect(deaths[0]!.getAttribute("aria-label"))
      .toContain("death observed in the 0:03.000–0:03.500 one-second bucket");
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
