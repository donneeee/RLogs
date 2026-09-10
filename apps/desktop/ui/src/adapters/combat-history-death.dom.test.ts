// @vitest-environment happy-dom

import { describe, expect, it } from "vitest";

import type { HistoryDeathEvent } from "./combat-history";
import { historyDeathMarker, historyDeathSummary } from "./combat-history-surface";

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

  it("exposes localized cause plus raw IDs to hover and keyboard focus", () => {
    const summary = historyDeathSummary("Alice", death());
    const marker = historyDeathMarker(10, 20, summary);

    expect(summary).toContain("Alice died at 0:03.500");
    expect(summary).toContain("Powerdraw [ability 2233]");
    expect(summary).toContain("Tina - Void Reverie [actor 9]");
    expect(marker.getAttribute("tabindex")).toBe("0");
    expect(marker.getAttribute("role")).toBe("img");
    expect(marker.getAttribute("aria-label")).toBe(summary);
    expect(marker.querySelector("title")?.textContent).toBe(summary);
  });

  it("keeps malicious presentation text inert", () => {
    const event = death();
    event.cause!.final_hit.source_presentation!.name = "Boss <script>bad()</script>";
    const marker = historyDeathMarker(10, 20, historyDeathSummary("Alice", event));

    expect(marker.querySelector("script")).toBeNull();
    expect(marker.querySelector("title")?.textContent).toContain("<script>bad()</script>");
  });

  it("retains a useful fallback when exact cause is unavailable", () => {
    expect(historyDeathSummary("Alice", { at_micros: 3_000_000, cause: null }))
      .toBe("Alice died at 0:03.000; cause unavailable.");
  });
});
