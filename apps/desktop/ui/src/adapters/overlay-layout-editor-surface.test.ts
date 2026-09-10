// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountOverlayLayoutEditorSurface } from "./overlay-layout-editor-surface";
import { parseOverlayLayoutSettings, type OverlayLayoutSettings } from "./overlay-layout";
import { LocalHostHttpError } from "../shell/local-host-http";

function layout(): OverlayLayoutSettings {
  const modules = Object.fromEntries(["map", "player", "actions", "party", "target", "objectives", "alerts"].map((id, index) => [id, {
    x: .1, y: .1, width: .3, height: .3, visible: true, zOrder: index, opacity: 1, scale: 1,
  }]));
  return parseOverlayLayoutSettings({ schemaVersion: 1, revision: 0, selectedSetupId: "default", legacyMigrationComplete: true,
    setups: { default: { name: "Default HUD", locked: false, modules } } });
}

afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); });
beforeEach(() => { vi.stubGlobal("Option", function Option(label = "", value = "") {
  const option = document.createElement("option"); option.text = label; option.value = value; return option;
}); });

describe("overlay layout editor", () => {
  it("queues immutable snapshots so a rapid edit survives an in-flight save", async () => {
    let resolveFirst!: (value: OverlayLayoutSettings) => void;
    const first = new Promise<OverlayLayoutSettings>((resolve) => { resolveFirst = resolve; });
    let calls = 0;
    const save = vi.fn(async (value: OverlayLayoutSettings) => {
      calls += 1;
      if (calls === 1) return first;
      return { ...structuredClone(value), revision: value.revision + 1 };
    });
    const container = document.createElement("div"); document.body.append(container);
    mountOverlayLayoutEditorSurface(container, { load: async () => layout(), save, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    (container.querySelector('.overlay-layout-module-card input[type="checkbox"]') as HTMLInputElement).click();
    const opacity = container.querySelectorAll('.overlay-layout-module-card input[type="number"]')[4] as HTMLInputElement;
    opacity.value = "55"; opacity.dispatchEvent(new Event("change", { bubbles: true }));
    const acknowledged = layout(); acknowledged.setups.default!.modules.map.visible = false; acknowledged.revision = 1;
    resolveFirst(acknowledged);
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(save).toHaveBeenCalledTimes(2);
    expect(save.mock.calls[1]![0].setups.default!.modules.map.opacity).toBe(.55);
    expect(save.mock.calls[1]![0].setups.default!.modules.map.visible).toBe(false);
  });

  it("rebases a conflict onto the newest host revision without discarding the edit", async () => {
    const initial = layout(); const fresh = layout(); fresh.revision = 5; fresh.setups.default!.modules.party.visible = false;
    let loads = 0; let saves = 0;
    const save = vi.fn(async (value: OverlayLayoutSettings) => {
      saves += 1; if (saves === 1) throw new LocalHostHttpError(409, "conflict");
      return { ...structuredClone(value), revision: 6 };
    });
    const container = document.createElement("div"); document.body.append(container);
    mountOverlayLayoutEditorSurface(container, { load: async () => (++loads === 1 ? initial : structuredClone(fresh)), save, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    const opacity = container.querySelectorAll('.overlay-layout-module-card input[type="number"]')[4] as HTMLInputElement;
    opacity.value = "60"; opacity.dispatchEvent(new Event("change", { bubbles: true }));
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(save).toHaveBeenCalledTimes(2);
    expect(save.mock.calls[1]![0].revision).toBe(5);
    expect(save.mock.calls[1]![0].setups.default!.modules.map.opacity).toBe(.6);
    expect(save.mock.calls[1]![0].setups.default!.modules.party.visible).toBe(false);
  });

  it("retains the dirty edit and visible error when the conflict retry fails", async () => {
    const initial = layout(); const fresh = layout(); fresh.revision = 4;
    let loads = 0; let saves = 0;
    const save = vi.fn(async () => { saves += 1; throw new LocalHostHttpError(saves === 1 ? 409 : 500, saves === 1 ? "conflict" : "disk failed"); });
    const container = document.createElement("div"); document.body.append(container);
    mountOverlayLayoutEditorSurface(container, { load: async () => (++loads === 1 ? initial : structuredClone(fresh)), save, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    (container.querySelector('.overlay-layout-module-card input[type="checkbox"]') as HTMLInputElement).click();
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(save).toHaveBeenCalledTimes(2);
    expect(container.querySelector(".overlay-menu-preview-badge")?.textContent).toBe("UNAVAILABLE");
    expect((container.querySelector('.overlay-layout-module-card input[type="checkbox"]') as HTMLInputElement).checked).toBe(false);
  });

  it("polls a newer editor revision only while no local edit is dirty", async () => {
    vi.useFakeTimers();
    const initial = layout(); const fresh = layout(); fresh.revision = 2; fresh.setups.default!.modules.map.opacity = .7;
    let loads = 0; const container = document.createElement("div"); document.body.append(container);
    const mounted = mountOverlayLayoutEditorSurface(container, { load: async () => (++loads === 1 ? initial : fresh), save: async (value) => value, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    await vi.advanceTimersByTimeAsync(1_000);
    const opacity = container.querySelectorAll('.overlay-layout-module-card input[type="number"]')[4] as HTMLInputElement;
    expect(opacity.value).toBe("70");
    mounted.dispose(); vi.useRealTimers();
  });

  it("saves module visibility and layer controls to the shared setup", async () => {
    let shared = layout(); const save = vi.fn(async (value: OverlayLayoutSettings) => { shared = structuredClone(value); shared.revision++; return shared; });
    const container = document.createElement("div"); document.body.append(container);
    mountOverlayLayoutEditorSurface(container, { load: async () => shared, save, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    const mapCard = container.querySelector(".overlay-layout-module-card")!;
    const visible = mapCard.querySelector('input[type="checkbox"]') as HTMLInputElement; visible.click();
    await Promise.resolve(); await Promise.resolve();
    expect(save).toHaveBeenCalled(); expect(shared.setups.default!.modules.map.visible).toBe(false);
  });

  it("requires confirmation and restores the host safe reset", async () => {
    const save = vi.fn(async (value: OverlayLayoutSettings) => value); vi.stubGlobal("confirm", () => true);
    const container = document.createElement("div"); document.body.append(container);
    mountOverlayLayoutEditorSurface(container, { load: async () => layout(), save, openOverlay: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "Reset safe layout") as HTMLButtonElement).click();
    await Promise.resolve(); expect(save).toHaveBeenCalledOnce();
  });
});
