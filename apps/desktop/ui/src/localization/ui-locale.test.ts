import { describe, expect, it } from "vitest";

import { loadUiLocalizer, localeFallbackChain, parseUiMessageShard, type UiMessageLoaders } from "./ui-locale";

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
