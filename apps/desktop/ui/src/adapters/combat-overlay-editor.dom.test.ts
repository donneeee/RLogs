// @vitest-environment happy-dom

import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  mountCombatOverlayEditorSurface,
  parseCombatOverlaySettings,
  renderOverlayCanvas,
} from "../../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";

function response(value: unknown): Response {
  return new Response(JSON.stringify(value), { headers: { "Content-Type": "application/json" } });
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await new Promise((resolve) => window.setTimeout(resolve, 20));
  await Promise.resolve();
}

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe("Combat Overlay designer presentation", () => {
  it("does not acknowledge native readiness until the saved layout has painted", () => {
    const bootstrap = readFileSync("src/main.ts", "utf8");
    const runtimeBranch = bootstrap.indexOf("if (isCombatOverlayRuntime)");
    const themeLoad = bootstrap.indexOf("await loadAndApplyThemeSettings()", runtimeBranch);
    const mount = bootstrap.indexOf("await mountCombatOverlayRuntimeApp(root", runtimeBranch);
    const ready = bootstrap.indexOf('await invoke("combat_overlay_ready")', runtimeBranch);
    expect(runtimeBranch).toBeGreaterThanOrEqual(0);
    expect(themeLoad).toBeGreaterThan(runtimeBranch);
    expect(mount).toBeGreaterThan(themeLoad);
    expect(ready).toBeGreaterThan(mount);
  });

  it("uses the question-mark and numeric label only for a genuinely unknown weapon", () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1,
      canvasWidth: 460,
      canvasHeight: 200,
      opacityPercent: 92,
      barOpacityPercent: 25,
      summaryOpacityPercent: 85,
      backgroundMode: "transparent",
      backgroundColor: "#000000",
      backgroundOpacityPercent: 0,
      customBackgroundRevision: null,
      liveOverlayEnabled: true,
      alwaysOnTop: true,
      clickThrough: false,
      autoHideOutsideCombat: false,
      autoHideDelaySeconds: 5,
      refreshIntervalMillis: 250,
      dynamicHeight: false,
      allowLiveResize: true,
      showViewTabs: false,
      maxVisiblePlayers: 20,
      scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "weapon", "dps"],
        headerWidths: { name: 190, weapon: 32, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: [], buttons: [],
      }],
    });
    const canvas = document.createElement("div");
    renderOverlayCanvas(canvas, settings, [{
      actor_id: "unknown-weapon", display_name: "Unknown weapon", dps: 100, hps: 0, tps: 0,
      rdps: null,
      presentation: {
        character_id: null, class_id: null, specialization_id: null, class_name: null,
        specialization_name: null, class_spec_icon_asset_path: null, role: null, accent: null,
        weapon: {
          slot_id: null, ability_id: null, item_id: 9_999_999, tier: null, level: null,
          level_min: null, level_max: null, badge_kind: null,
          label: "Weapon item 9999999", icon_asset_path: null,
        },
        primary_imagines: [],
      },
    }], {
      mode: "preview", selectedLayerId: "party-meter", snapshot: null,
      encounterPresentation: null,
    });

    const weapon = canvas.querySelector<HTMLElement>(
      ".overlay-field-weapon .combat-overlay-badge",
    );
    expect(weapon?.dataset.state).toBe("fallback");
    expect(weapon?.textContent).toBe("?");
    expect(weapon?.title).toContain("Weapon item 9999999");
    expect(weapon?.querySelector("img")).toBeNull();
  });

  it("uses trusted equipment presentation and saves the visible draft before opening live", async () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1, canvasWidth: 460, canvasHeight: 520, opacityPercent: 92,
      barOpacityPercent: 25, summaryOpacityPercent: 85, backgroundMode: "solid",
      backgroundColor: "#102030", backgroundOpacityPercent: 37,
      customBackgroundRevision: null, liveOverlayEnabled: true, alwaysOnTop: true,
      clickThrough: false, autoHideOutsideCombat: false, autoHideDelaySeconds: 5,
      refreshIntervalMillis: 250, dynamicHeight: true, allowLiveResize: true,
      showViewTabs: false, maxVisiblePlayers: 20, scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "weapon", "main_imagines", "dps"],
        headerWidths: { name: 190, weapon: 32, main_imagines: 54, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"], buttons: [],
      }],
    });
    const badge = (
      itemId: number | null,
      abilityId: number | null,
      label: string,
      icon: string,
    ) => ({
      slot_id: null, ability_id: abilityId, item_id: itemId, tier: null, level: 140,
      level_min: 140, level_max: 140, badge_kind: "weapon", label, icon_asset_path: icon,
    });
    const catalog = {
      weapons: [
        badge(2_000_631, null, "Ember - Gaze of the Far Sea", "/trusted/ember.png"),
        badge(2_001_503, null, "Ragedream Axe", "/trusted/axe.png"),
        badge(2_001_505, null, "Voidforge Ring", "/trusted/ring.png"),
        badge(2_001_508, null, "Oath of the Immortal Watch", "/trusted/shield.png"),
        badge(2_000_901, null, "Daybreak Lance - Tempest Flow", "/trusted/daybreak.png"),
      ],
      primary_imagines: [
        { ...badge(3_000_101, 3_948, "Rorola", "/trusted/rorola.png"), badge_kind: null },
        { ...badge(3_000_121, 3_969, "Igoreus", "/trusted/igoreus.png"), badge_kind: null },
      ],
    };
    const events: string[] = [];
    let savedBody: Record<string, unknown> | null = null;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay" && init?.method === "POST") {
        events.push("save");
        savedBody = JSON.parse(String(init.body)) as Record<string, unknown>;
        return response(savedBody);
      }
      if (route === "/api/settings/combat-overlay") return response(settings);
      if (route === "/api/settings/combat-overlay/example-presentations") return response(catalog);
      if (route === "/api/runtime/live/combat") return response({ revision: 0, snapshot: null });
      if (route === "/api/settings/core") {
        return response({ pauseOverlayTimersOutsideCombat: false, overlayTimerInactivitySeconds: 3 });
      }
      throw new Error(`Unexpected route ${route}`);
    }));
    const openLive = vi.fn(async () => { events.push("open"); });
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountCombatOverlayEditorSurface(container, openLive);
    await flush();

    const umapyoi = [...container.querySelectorAll(".combat-overlay-actor-row")]
      .find((row) => row.textContent?.includes("Umapyoi"));
    const weapon = umapyoi?.querySelector<HTMLElement>(".overlay-field-weapon .combat-overlay-badge");
    expect(weapon?.title).toContain("Daybreak Lance - Tempest Flow");
    expect(weapon?.title).not.toContain("2000901");
    expect(weapon?.dataset.state).toBe("resolved");
    expect(weapon?.querySelector("img")?.getAttribute("src")).toBe("/trusted/daybreak.png");
    const imagineBadges = umapyoi?.querySelectorAll<HTMLElement>(
      ".overlay-field-main_imagines .combat-overlay-badge",
    );
    expect(imagineBadges?.[0]?.dataset.tier).toBe("0");
    expect(imagineBadges?.[0]?.title).toContain("Tier 0");
    expect(imagineBadges?.[0]?.title).toContain("Rorola");
    expect(imagineBadges?.[0]?.querySelector("img")?.getAttribute("src"))
      .toBe("/trusted/rorola.png");
    expect(imagineBadges?.[1]?.dataset.tier).toBeUndefined();
    expect(imagineBadges?.[1]?.title).toContain("Igoreus");
    expect(container.querySelector(".combat-overlay-badge[data-tier='5']")).not.toBeNull();

    const openButton = [...container.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent === "Open live overlay");
    openButton?.click();
    await flush();
    expect(events).toEqual(["save", "open"]);
    expect(savedBody).toMatchObject({
      liveOverlayEnabled: true,
      backgroundMode: "solid",
      backgroundColor: "#102030",
      backgroundOpacityPercent: 37,
    });
    mounted.dispose();
  });
});
