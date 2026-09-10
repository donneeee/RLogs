// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AutomarkerPresetView } from "./automarker-presets";
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
      loadPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
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
      loadPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
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
      loadPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
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
