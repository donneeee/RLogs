// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AutomarkerLocalLoadResult, AutomarkerPresetView } from "./automarker-presets";
import { mountAutomarkerPresetsSurface } from "./automarker-presets-surface";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function view(sceneId: number, familyId: string, name: string, x: number): AutomarkerPresetView {
  return {
    schemaVersion: 3,
    context: {
      clientBuild: "24687926",
      sceneId,
      mapId: sceneId,
      activityFamilyId: familyId,
      sceneName: `Scene ${sceneId}`,
    },
    presets: [{
      presetId: `preset-${sceneId}-00000000`,
      name,
      clientBuild: "24687926",
      sceneId,
      mapId: sceneId,
      activityFamilyId: familyId,
      savedAtUnixMillis: 1,
      points: [{ markerNumber: 1, x, y: 2, z: 3 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_request_unverified",
    previewSessionId: "preview-test-session",
  };
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  vi.stubGlobal("Option", function Option(text = "", value = "") {
    const option = document.createElement("option");
    option.text = text;
    option.value = value;
    return option;
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  window.localStorage.clear();
});

describe("mounted automarker preset editor request ordering", () => {
  it("loads a selected saved preset into the editor without previewing or placing it", async () => {
    const catalog = view(6_525, "mech-facility", "Opener", 1);
    catalog.presets = [
      ...catalog.presets,
      {
        ...catalog.presets[0]!,
        presetId: "preset-6525-00000001",
        name: "Alternate",
        points: [{ markerNumber: 2, x: 9, y: 8, z: 7 }],
      },
    ];
    const loadPreset = vi.fn(async ({ presetId }: { presetId: string }) => ({
      context: catalog.context!,
      preset: catalog.presets.find((preset) => preset.presetId === presetId)!,
    }));
    const openOverlay = vi.fn(async () => undefined);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset,
      openOverlay,
    });
    await flushPromises();

    const select = container.querySelector("select")!;
    select.value = "preset-6525-00000001";
    select.dispatchEvent(new Event("change"));
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("1");
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Load")!.click();
    await flushPromises();

    expect(loadPreset).toHaveBeenCalledWith({
      presetId: "preset-6525-00000001",
      expectedContext: catalog.context,
    });
    expect(openOverlay).not.toHaveBeenCalled();
    expect((container.querySelector('input[data-coordinate="markerNumber"]') as HTMLInputElement).value).toBe("2");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("9");
    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Alternate");
    expect(container.querySelector(".automarker-status")?.textContent).toContain("Nothing was sent to the game");
    const place = [...container.querySelectorAll("button")].find((button) => button.textContent === "Place in game")!;
    expect(place.disabled).toBe(true);
    mounted.dispose();
  });

  it("does not apply a local load response from a changed scene context", async () => {
    const catalog = view(6_525, "mech-facility", "Opener", 1);
    const pending = deferred<AutomarkerLocalLoadResult>();
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: () => pending.promise,
      openOverlay: async () => undefined,
    });
    await flushPromises();
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Load")!.click();
    pending.resolve({
      context: { ...catalog.context!, sceneId: 1_633, mapId: 1_633, activityFamilyId: "dungeon.1633" },
      preset: { ...catalog.presets[0]!, name: "Wrong scene", activityFamilyId: "dungeon.1633" },
    });
    await flushPromises();

    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Opener");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("1");
    mounted.dispose();
  });

  it("does not let an older periodic refresh overwrite a newer explicit refresh", async () => {
    vi.useFakeTimers();
    const initial = deferred<AutomarkerPresetView>();
    const older = deferred<AutomarkerPresetView>();
    const newer = deferred<AutomarkerPresetView>();
    const requests = [initial, older, newer];
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: () => requests.shift()!.promise,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });

    initial.resolve(view(1_633, "dungeon.1633", "Initial master", 1));
    await flushPromises();
    vi.advanceTimersByTime(2_000);
    const refresh = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Refresh scene")!;
    refresh.click();

    newer.resolve(view(1_631, "tina-mindrealm", "Current normal", 31));
    await flushPromises();
    older.resolve(view(1_633, "dungeon.1633", "Stale master", 99));
    await flushPromises();

    expect(container.querySelector(".overlay-menu-preview-badge")?.textContent).toBe("Scene 1631");
    expect(container.querySelector("select")?.textContent).toContain("Current normal");
    expect(container.querySelector("select")?.textContent).not.toContain("Stale master");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("31");
    expect(container.querySelector(".automarker-status")?.textContent).toContain("1 compatible setup");
    mounted.dispose();
  });

  it("does not mutate editor state when its pending initial request completes after disposal", async () => {
    const pending = deferred<AutomarkerPresetView>();
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: () => pending.promise,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    const status = container.querySelector(".automarker-status")!;

    mounted.dispose();
    pending.resolve(view(1_633, "dungeon.1633", "Disposed master", 99));
    await flushPromises();

    expect(status.textContent).toBe("Connecting to the local marker store…");
    expect(container.querySelector("select")?.textContent).not.toContain("Disposed master");
  });

  it("does not let an older rejected refresh replace the newest status", async () => {
    vi.useFakeTimers();
    const initial = deferred<AutomarkerPresetView>();
    const older = deferred<AutomarkerPresetView>();
    const newer = deferred<AutomarkerPresetView>();
    const requests = [initial, older, newer];
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: () => requests.shift()!.promise,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    initial.resolve(view(1_633, "dungeon.1633", "Initial master", 1));
    await flushPromises();
    vi.advanceTimersByTime(2_000);
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Refresh scene")!
      .click();

    newer.resolve(view(1_631, "tina-mindrealm", "Current normal", 31));
    await flushPromises();
    older.reject(new Error("stale request failure"));
    await flushPromises();

    expect(container.querySelector(".automarker-status")?.textContent).toContain("1 compatible setup");
    expect(container.querySelector(".automarker-status")?.textContent).not.toContain("stale request failure");
    mounted.dispose();
  });
});
