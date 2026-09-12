// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AutomarkerLocalLoadResult, AutomarkerPresetView, ObservedMarkerSnapshot } from "./automarker-presets";
import { mountAutomarkerPresetsSurface } from "./automarker-presets-surface";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
}

function unavailableObserved(build = "25247556"): ObservedMarkerSnapshot {
  return {
    schemaVersion: 2,
    revision: 1,
    captureActive: true,
    protocolSupported: false,
    requestObserverSupported: false,
    verifiedRequestCount: 0,
    lastVerifiedRequestMarkerNumber: null,
    lastVerifiedRequestObservedMicros: null,
    reason: "marker_protocol_not_verified_for_build_pack",
    sessionId: "capture-test-session",
    deploymentId: "global",
    clientBuild: build,
    protocolPackDigest: `sha256:${"a".repeat(64)}`,
    sceneId: null,
    mapId: null,
    observedMicros: null,
    markers: [],
  };
}

function capturable(sceneId = 6_525): { catalog: AutomarkerPresetView; snapshot: ObservedMarkerSnapshot } {
  const catalog = view(sceneId, "mech-facility", "Opener", 1);
  catalog.captureSupported = true;
  catalog.captureReason = "observed_waymark_state_verified";
  catalog.captureSessionId = "capture-test-session";
  catalog.deploymentId = "global";
  catalog.protocolPackDigest = `sha256:${"b".repeat(64)}`;
  return {
    catalog,
    snapshot: {
      schemaVersion: 2,
      revision: 9,
      captureActive: true,
      protocolSupported: true,
      requestObserverSupported: true,
      verifiedRequestCount: 2,
      lastVerifiedRequestMarkerNumber: 2,
      lastVerifiedRequestObservedMicros: 123_455,
      reason: "observed_markers_available",
      sessionId: catalog.captureSessionId,
      deploymentId: catalog.deploymentId,
      clientBuild: catalog.context!.clientBuild,
      protocolPackDigest: catalog.protocolPackDigest,
      sceneId: catalog.context!.sceneId,
      mapId: catalog.context!.mapId,
      observedMicros: 123_456,
      markers: [
        { markerNumber: 1, x: 11, y: 12, z: 13 },
        { markerNumber: 2, x: 21, y: 22, z: 23 },
      ],
    },
  };
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function view(sceneId: number, familyId: string, name: string, x: number): AutomarkerPresetView {
  return {
    schemaVersion: 4,
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
      activityFamilyId: familyId,
      savedAtUnixMillis: 1,
      points: [{ markerNumber: 1, x, y: 2, z: 3 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    captureSessionId: null,
    deploymentId: null,
    protocolPackDigest: null,
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_transport_unavailable",
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
  it("keeps Capture current markers disabled with the exact unverified-build reason", async () => {
    const catalog = view(6_525, "mech-facility", "Opener", 1);
    catalog.context!.clientBuild = "25247556";
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const capture = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Capture current markers")!;
    expect(capture.disabled).toBe(true);
    expect(capture.title).toContain("not protocol-verified for build 25247556");
    expect(container.querySelector(".automarker-preset-detail")?.textContent)
      .toContain("not protocol-verified for build 25247556");
    mounted.dispose();
  });

  it("populates all captured rows atomically only after fresh matching responses complete", async () => {
    const initial = capturable();
    const freshCatalog = deferred<AutomarkerPresetView>();
    const freshSnapshot = deferred<ObservedMarkerSnapshot>();
    let catalogCalls = 0;
    let snapshotCalls = 0;
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: () => ++catalogCalls === 1 ? Promise.resolve(initial.catalog) : freshCatalog.promise,
      loadObservedMarkers: () => ++snapshotCalls === 1 ? Promise.resolve(initial.snapshot) : freshSnapshot.promise,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();
    const capture = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Capture current markers")!;
    expect(capture.disabled).toBe(false);
    expect(container.querySelector(".automarker-request-diagnostic")?.textContent)
      .toContain("2 recognized");
    expect(container.querySelector(".automarker-request-diagnostic")?.textContent)
      .toContain("marker 2 at 0.123s");
    capture.click();

    freshSnapshot.resolve(initial.snapshot);
    await flushPromises();
    expect(container.querySelectorAll(".automarker-point-row")).toHaveLength(1);
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("1");

    freshCatalog.resolve(initial.catalog);
    await flushPromises();
    expect(container.querySelectorAll(".automarker-point-row")).toHaveLength(2);
    expect([...container.querySelectorAll<HTMLInputElement>('input[data-coordinate="x"]')].map((input) => input.value))
      .toEqual(["11", "21"]);
    expect(container.querySelector(".automarker-status")?.textContent).toContain("Captured 2 current in-game markers");
    mounted.dispose();
  });

  it("rejects a capture response after the capture session changes", async () => {
    const initial = capturable();
    let calls = 0;
    const nextCatalog = { ...initial.catalog, captureSessionId: "new-capture-session" };
    const nextSnapshot = { ...initial.snapshot, sessionId: "new-capture-session", revision: 10 };
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => ++calls === 1 ? initial.catalog : nextCatalog,
      loadObservedMarkers: async () => calls <= 1 ? initial.snapshot : nextSnapshot,
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Capture current markers")!
      .click();
    await flushPromises();

    expect(container.querySelectorAll(".automarker-point-row")).toHaveLength(1);
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("1");
    mounted.dispose();
  });

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
      loadObservedMarkers: async () => unavailableObserved(),
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
      loadObservedMarkers: async () => unavailableObserved(),
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
      loadObservedMarkers: async () => unavailableObserved(),
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
      loadObservedMarkers: async () => unavailableObserved(),
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
      loadObservedMarkers: async () => unavailableObserved(),
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
