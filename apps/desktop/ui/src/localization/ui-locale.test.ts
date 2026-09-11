import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

import { loadUiLocalizer, localeFallbackChain, parseUiMessageShard, type UiMessageLoaders } from "./ui-locale";
import { REVIEWED_MECHANIC_KINDS } from "../adapters/mechanics-map-surface";

function shard(locale: string, messages: Record<string, string>): { default: unknown } {
  return { default: { schema_version: 1, locale, namespace: "ui.test", messages } };
}

describe("desktop UI locale packages", () => {
  it("loads the shipped English Mechanics Map shard with its complete foundational key set", async () => {
    const localizer = await loadUiLocalizer("en-US");
    expect(localizer.loadedLocales).toContain("en-US");
    for (const key of [
      "ui.mechanics_map.toolbar.rotate",
      "ui.mechanics_map.toolbar.mobs",
      "ui.mechanics_map.toolbar.dim_percent",
      "ui.mechanics_map.toolbar.dim_help",
      "ui.mechanics_map.toolbar.contrast",
      "ui.mechanics_map.toolbar.contrast_help",
      "ui.mechanics_map.toolbar.fit",
      "ui.mechanics_map.toolbar.center",
      "ui.overlay_canvas.controls.aria",
      "ui.overlay_canvas.controls.label",
      "ui.overlay_canvas.controls.mode",
      "ui.overlay_canvas.controls.lock",
      "ui.overlay_canvas.controls.unlock",
      "ui.overlay_canvas.controls.lock_help",
      "ui.overlay_canvas.controls.hide",
      "ui.overlay_canvas.controls.hide_help",
      "ui.overlay_canvas.controls.done",
      "ui.overlay_canvas.controls.done_help",
      "ui.mechanics_map.status.waiting_for_scene",
      "ui.mechanics_map.status.connecting",
      "ui.mechanics_map.status.waiting",
      "ui.mechanics_map.status.live",
      "ui.mechanics_map.status.position_needed",
      "ui.mechanics_map.notice.waiting_for_position",
      "ui.mechanics_map.source.game_map",
      "ui.mechanics_map.source.preparing_map",
      "ui.mechanics_map.source.map_asset_pending",
      "ui.mechanics_map.source.radar_fallback",
      "ui.mechanics_map.metrics.summary",
    ]) {
      expect(localizer.t(key), key).not.toBe(key);
    }
  });

  it("keeps every migrated Mechanics Map key present in the shipped English package", async () => {
    const source = [
      "../adapters/mechanics-map-overlay.ts",
      "../adapters/mechanics-map-surface.ts",
    ].map((path) => readFileSync(new URL(path, import.meta.url), "utf8")).join("\n");
    const keys = new Set((source.match(/ui\.mechanics_map\.[a-z0-9_.]+/g) ?? [])
      .filter((key) => !key.endsWith(".")));
    const localizer = await loadUiLocalizer("en-US");
    expect(keys.size).toBeGreaterThan(0);
    for (const key of keys) expect(localizer.t(key), key).not.toBe(key);
    for (const kind of REVIEWED_MECHANIC_KINDS) {
      const key = `ui.mechanics_map.mechanic.${kind}`;
      expect(localizer.t(key), key).not.toBe(key);
    }
  });

  it("loads the shipped combat history browser and graph inspection shard", async () => {
    const localizer = await loadUiLocalizer("en-US");
    for (const key of [
      "ui.combat_history.status.loading_history",
      "ui.combat_history.status.loading_runs",
      "ui.combat_history.status.reading_index",
      "ui.combat_history.status.no_indexed_runs",
      "ui.combat_history.status.indexed_runs",
      "ui.combat_history.status.loading_detail",
      "ui.combat_history.error.load_failed",
      "ui.combat_history.empty.complete_dungeon",
      "ui.combat_history.browser.title",
      "ui.combat_history.browser.description",
      "ui.combat_history.browser.result_count",
      "ui.combat_history.browser.search_placeholder",
      "ui.combat_history.browser.search_aria",
      "ui.combat_history.browser.difficulty",
      "ui.combat_history.browser.all_difficulties",
      "ui.combat_history.browser.sort_runs",
      "ui.combat_history.browser.sort_newest",
      "ui.combat_history.browser.sort_oldest",
      "ui.combat_history.browser.sort_fastest",
      "ui.combat_history.browser.sort_team_edps",
      "ui.combat_history.browser.sort_team_adps",
      "ui.combat_history.browser.favorites",
      "ui.combat_history.browser.no_matches",
      "ui.combat_history.browser.previous",
      "ui.combat_history.browser.next",
      "ui.combat_history.browser.page",
      "ui.combat_history.breakdown.party",
      "ui.combat_history.breakdown.party_view_aria",
      "ui.combat_history.breakdown.incoming_damage",
      "ui.combat_history.breakdown.incoming_damage_description",
      "ui.combat_history.breakdown.skills",
      "ui.combat_history.breakdown.healing_and_shielding",
      "ui.combat_history.breakdown.status_effects",
      "ui.combat_history.breakdown.effect_count",
      "ui.combat_history.breakdown.relative_damage_sources",
      "ui.combat_history.breakdown.granted_by_support_effect",
      "ui.combat_history.breakdown.influence_ledger",
      "ui.combat_history.graph.gallery_title",
      "ui.combat_history.graph.gallery_description",
      "ui.combat_history.graph.damage_title",
      "ui.combat_history.graph.damage_description",
      "ui.combat_history.graph.rdps_title",
      "ui.combat_history.graph.rdps_description",
      "ui.combat_history.graph.healing_title",
      "ui.combat_history.graph.healing_description",
      "ui.combat_history.graph.damage_taken_title",
      "ui.combat_history.graph.damage_taken_description",
      "ui.combat_history.graph.dps",
      "ui.combat_history.graph.rdps",
      "ui.combat_history.graph.hps",
      "ui.combat_history.graph.tps",
      "ui.combat_history.graph.metric_aria",
      "ui.combat_history.graph.show_actor_aria",
      "ui.combat_history.graph.hide_actor_aria",
      "ui.combat_history.graph.npc",
      "ui.combat_history.graph.no_values",
      "ui.combat_history.graph.all_hidden",
      "ui.combat_history.graph.average",
      "ui.combat_history.graph.peak",
      "ui.combat_history.graph.series_summary",
      "ui.combat_history.graph.aria",
      "ui.combat_history.graph.run_time",
      "ui.combat_history.graph.inspect_help",
      "ui.combat_history.graph.inspect_value",
      "ui.combat_history.graph.status_effect_fallback",
      "ui.combat_history.graph.status_span_removed",
      "ui.combat_history.graph.status_span_consumed",
      "ui.combat_history.graph.skill_cluster_disclosure",
      "ui.combat_history.graph.skill_disclosure_title",
    ]) {
      expect(localizer.t(key), key).not.toBe(key);
    }
    expect(localizer.t("ui.combat_history.graph.inspect_value", {
      actor: "MarieRose",
      value: "12,345.6",
      rate: "DPS",
    })).toBe("MarieRose 12,345.6 DPS");
    expect(localizer.t("ui.combat_history.graph.hide_actor_aria", {
      actor: "MarieRose",
    })).toBe("Hide MarieRose in timelines");
    expect(localizer.t("ui.combat_history.graph.status_effect_fallback", {
      id: "2203291",
    })).toBe("Unlocalized combat effect #2203291");
  });

  it("keeps every migrated combat-history key present in the shipped English package", async () => {
    const source = readFileSync(new URL("../adapters/combat-history-surface.ts", import.meta.url), "utf8");
    const keys = new Set(source.match(/ui\.combat_history\.[a-z0-9_.]+/g) ?? []);
    const localizer = await loadUiLocalizer("en-US");
    expect(keys.size).toBeGreaterThan(0);
    for (const key of keys) expect(localizer.t(key), key).not.toBe(key);
  });

  it("uses exact, base-language, English, then stable-key fallback", async () => {
    const loaders: UiMessageLoaders = {
      "/localization/en-US/ui/test/messages.json": async () => shard("en-US", {
        "ui.test.exact": "English exact",
        "ui.test.english": "English fallback",
      }),
      "/localization/fr/ui/test/messages.json": async () => shard("fr", {
        "ui.test.exact": "Français générique",
      }),
      "/localization/fr-CA/ui/test/messages.json": async () => shard("fr-CA", {
        "ui.test.exact": "Français canadien",
      }),
    };
    const localizer = await loadUiLocalizer("fr-CA", loaders);
    expect(localeFallbackChain("fr-CA")).toEqual(["fr-CA", "fr", "en-US"]);
    expect(localizer.loadedLocales).toEqual(["fr-CA", "fr", "en-US"]);
    expect(localizer.t("ui.test.exact")).toBe("Français canadien");
    expect(localizer.t("ui.test.english")).toBe("English fallback");
    expect(localizer.t("ui.test.missing")).toBe("ui.test.missing");
  });

  it("substitutes named placeholders and formats numbers for the requested locale", async () => {
    const localizer = await loadUiLocalizer("de-DE", {
      "/localization/en-US/ui/test/messages.json": async () => shard("en-US", {
        "ui.test.count": "{count} entities",
      }),
    });
    expect(localizer.t("ui.test.count", { count: 12 })).toBe("12 entities");
    expect(localizer.formatNumber(1234.5)).toBe("1.234,5");
  });

  it("rejects structurally invalid shards and cross-package locales", () => {
    expect(() => parseUiMessageShard({ schema_version: 1, locale: "en-US", namespace: "ui.test", messages: {
      "other.key": "Wrong namespace",
    } })).toThrow(/message/);
    expect(() => parseUiMessageShard({ schema_version: 1, locale: "fr", namespace: "ui.test", messages: {} }, "en-US"))
      .toThrow(/locale/);
  });
});
