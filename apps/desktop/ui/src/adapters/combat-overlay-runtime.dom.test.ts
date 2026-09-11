// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  mountCombatOverlayRuntimeApp,
  parseCombatOverlaySettings,
  type CombatOverlayRuntimeWindow,
} from "../../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function response(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    headers: { "Content-Type": "application/json" },
  });
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => window.setTimeout(resolve, 70));
  await Promise.resolve();
}

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  document.documentElement.className = "";
});

describe("mounted Combat Overlay runtime", () => {
  it("retains the painted canvas when a newer feed revision has identical visual state", async () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1,
      canvasWidth: 460,
      canvasHeight: 520,
      opacityPercent: 92,
      barOpacityPercent: 25,
      summaryOpacityPercent: 85,
      backgroundMode: "solid",
      backgroundColor: "#0b1522",
      backgroundOpacityPercent: 92,
      customBackgroundRevision: null,
      liveOverlayEnabled: true,
      alwaysOnTop: true,
      clickThrough: false,
      autoHideOutsideCombat: false,
      autoHideDelaySeconds: 5,
      refreshIntervalMillis: 50,
      dynamicHeight: true,
      allowLiveResize: false,
      showViewTabs: false,
      maxVisiblePlayers: 20,
      scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "dps"], headerWidths: { name: 190, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"],
        summaryItemOrder: ["encounter_time", "button:dummy"],
        summaryItemRows: { encounter_time: 0, "button:dummy": 0 },
        buttons: [{ id: "dummy", label: "Dummy", action: "toggle_training_dummy", width: 74 }],
      }],
    });
    const firstWait = deferred<Response>();
    const secondWait = deferred<Response>();
    const parkedWait = deferred<Response>();
    let waitCalls = 0;
    let servedSettings = settings;
    let clock = 1_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => (clock += 2_000));
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay") return Promise.resolve(response(servedSettings));
      if (route === "/api/settings/core") {
        return Promise.resolve(response({
          pauseOverlayTimersOutsideCombat: false,
          overlayTimerInactivitySeconds: 8,
        }));
      }
      if (route === "/api/runtime/live/combat/wait") {
        waitCalls += 1;
        return waitCalls === 1 ? firstWait.promise : waitCalls === 2 ? secondWait.promise : parkedWait.promise;
      }
      throw new Error(`Unexpected route ${route}`);
    }));
    const resolved = async () => undefined;
    const setSize = vi.fn(async () => undefined);
    let resized: ((width: number, height: number) => void) | null = null;
    let showRequested: (() => void | Promise<void>) | null = null;
    const container = document.createElement("div");
    document.body.append(container);
    const showIfRequested = vi.fn(async () => {
      const revealedLayer = container.querySelector<HTMLElement>(".combat-overlay-layer");
      expect(revealedLayer?.dataset.backgroundMode).toBe("custom");
      expect(revealedLayer?.style.getPropertyValue("--overlay-background-opacity")).toBe("0.37");
    });
    const appWindow: CombatOverlayRuntimeWindow = {
      close: resolved, hide: resolved, hideTemporarily: resolved, showIfRequested,
      setEnabled: resolved, setAutomaticallyHidden: resolved, setAlwaysOnTop: resolved,
      setSize, setIgnoreCursorEvents: resolved, startDragging: resolved,
      startResizeDragging: resolved, heartbeat: resolved,
      onShowRequested: async (handler) => { showRequested = handler; return () => undefined; },
      onResized: async (handler) => { resized = handler; return () => undefined; },
    };
    await mountCombatOverlayRuntimeApp(container, appWindow);
    const initialCanvas = container.querySelector(".combat-overlay-canvas-runtime");
    const inactiveDummy = container.querySelector<HTMLButtonElement>(
      "[data-button-action='toggle_training_dummy']",
    );
    const dummyWidth = inactiveDummy?.style.width;
    expect(inactiveDummy?.textContent).toBe("Dummy");
    expect(inactiveDummy?.getAttribute("aria-pressed")).toBe("false");

    const update = {
      revision: 1,
      snapshot: {
        actors: [], combat_active: false, last_hostile_micros: 1,
        latest_event_micros: 2, combat_inactivity_timeout_micros: 3,
        combat_started_micros: 4, attempt_damage_elapsed_micros: 5,
        encounter_terminal_micros: null, run_terminal_micros: null,
      },
      training_dummy: {
        phase: "armed", durationMicros: 180_000_000, remainingMicros: 180_000_000,
        totalDamage: 0, dps: 0, valid: true,
      },
    };
    firstWait.resolve(response(update));
    await flush();
    const paintedCanvas = container.querySelector(".combat-overlay-canvas-runtime");
    expect(paintedCanvas).not.toBe(initialCanvas);
    expect(initialCanvas?.isConnected).toBe(false);
    expect(container.querySelectorAll(".combat-overlay-canvas-runtime")).toHaveLength(1);
    expect(container.querySelector(".combat-overlay-runtime-loading")).toBeNull();
    expect(paintedCanvas?.textContent).toContain("Dummy");
    const activeDummy = paintedCanvas?.querySelector<HTMLButtonElement>(
      "[data-button-action='toggle_training_dummy']",
    );
    expect(activeDummy?.textContent).toBe("Dummy");
    expect(activeDummy?.style.width).toBe(dummyWidth);
    expect(activeDummy?.dataset.active).toBe("true");
    expect(activeDummy?.getAttribute("aria-pressed")).toBe("true");
    setSize.mockClear();

    servedSettings = {
      ...settings,
      alwaysOnTop: false,
      clickThrough: true,
      liveOverlayEnabled: false,
      autoHideOutsideCombat: true,
      autoHideDelaySeconds: 17,
      refreshIntervalMillis: 2_000,
    };
    secondWait.resolve(response({
      ...update,
      revision: 2,
      snapshot: {
        ...update.snapshot,
        combat_active: true,
        last_hostile_micros: 11,
        latest_event_micros: 22,
        combat_inactivity_timeout_micros: 33,
        combat_started_micros: 44,
        attempt_damage_elapsed_micros: 55,
        encounter_terminal_micros: 66,
        run_terminal_micros: 77,
      },
    }));
    await flush();
    expect(container.querySelector(".combat-overlay-canvas-runtime")).toBe(paintedCanvas);
    expect(container.querySelector(".combat-overlay-runtime-loading")).toBeNull();
    expect(setSize).not.toHaveBeenCalled();
    expect(showIfRequested).not.toHaveBeenCalled();

    servedSettings = {
      ...servedSettings,
      backgroundMode: "custom",
      backgroundColor: "#123456",
      backgroundOpacityPercent: 37,
      customBackgroundRevision: 9,
    };
    expect(showRequested).not.toBeNull();
    await showRequested!();
    await flush();
    expect(showIfRequested).toHaveBeenCalledOnce();
    const refreshedCanvas = container.querySelector(".combat-overlay-canvas-runtime");
    expect(refreshedCanvas).not.toBe(paintedCanvas);
    const refreshedLayer = refreshedCanvas?.querySelector<HTMLElement>(".combat-overlay-layer");
    expect(refreshedLayer?.dataset.backgroundMode).toBe("custom");
    expect(refreshedLayer?.style.getPropertyValue("--overlay-background-opacity")).toBe("0.37");
    expect(refreshedLayer?.style.getPropertyValue("--overlay-background-color")).toBe("#123456");
    expect(refreshedLayer?.style.getPropertyValue("--overlay-background-image"))
      .toContain("background?v=9");

    setSize.mockClear();
    expect(resized).not.toBeNull();
    resized!(900, 900);
    await Promise.resolve();
    expect(setSize).toHaveBeenLastCalledWith(460, 80);
    expect(container.querySelector(".combat-overlay-canvas-runtime")).toBe(refreshedCanvas);

    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ ...update, revision: 3 }));
  });

  it("adopts live resize and defers a reveal until its saved layout is repainted", async () => {
    let servedSettings = parseCombatOverlaySettings({
      schemaVersion: 1, canvasWidth: 460, canvasHeight: 520, opacityPercent: 92,
      barOpacityPercent: 25, summaryOpacityPercent: 85, backgroundMode: "solid",
      backgroundColor: "#0b1522", backgroundOpacityPercent: 92,
      customBackgroundRevision: null, liveOverlayEnabled: true, alwaysOnTop: true,
      clickThrough: false, autoHideOutsideCombat: false, autoHideDelaySeconds: 5,
      refreshIntervalMillis: 50, dynamicHeight: false, allowLiveResize: true,
      showViewTabs: false, maxVisiblePlayers: 20, scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "dps"], headerWidths: { name: 190, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"], buttons: [],
      }],
    });
    const parkedWait = deferred<Response>();
    const saveResponse = deferred<Response>();
    let postedSettings: typeof servedSettings | null = null;
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay" && init?.method === "POST") {
        postedSettings = parseCombatOverlaySettings(JSON.parse(String(init.body)));
        return saveResponse.promise;
      }
      if (route === "/api/settings/combat-overlay") return Promise.resolve(response(servedSettings));
      if (route === "/api/settings/core") return Promise.resolve(response({ pauseOverlayTimersOutsideCombat: false, overlayTimerInactivitySeconds: 8 }));
      if (route === "/api/runtime/live/combat/wait") return parkedWait.promise;
      throw new Error(`Unexpected route ${route}`);
    }));
    const resolved = async () => undefined;
    const setSize = vi.fn(async () => undefined);
    const showIfRequested = vi.fn(async () => undefined);
    let resized: ((width: number, height: number) => void) | null = null;
    let showRequested: (() => void | Promise<void>) | null = null;
    const appWindow: CombatOverlayRuntimeWindow = {
      close: resolved, hide: resolved, hideTemporarily: resolved, showIfRequested,
      setEnabled: resolved, setAutomaticallyHidden: resolved, setAlwaysOnTop: resolved,
      setSize, setIgnoreCursorEvents: resolved, startDragging: resolved,
      startResizeDragging: resolved, heartbeat: resolved,
      onShowRequested: async (handler) => { showRequested = handler; return () => undefined; },
      onResized: async (handler) => { resized = handler; return () => undefined; },
    };
    const container = document.createElement("div"); document.body.append(container);
    await mountCombatOverlayRuntimeApp(container, appWindow);
    const originalCanvas = container.querySelector(".combat-overlay-canvas-runtime");
    setSize.mockClear();
    resized!(690, 780);
    await Promise.resolve();
    expect(setSize).not.toHaveBeenCalled();
    expect(container.querySelector(".combat-overlay-canvas-runtime")).not.toBe(originalCanvas);
    expect(showRequested).not.toBeNull();
    const reveal = Promise.resolve(showRequested!());
    expect(postedSettings).toBeNull();
    expect(showIfRequested).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(postedSettings).not.toBeNull());
    expect(showIfRequested).not.toHaveBeenCalled();
    servedSettings = postedSettings!;
    saveResponse.resolve(response(servedSettings));
    await reveal;
    expect(showIfRequested).toHaveBeenCalledOnce();
    expect(container.querySelector<HTMLElement>(".combat-overlay-canvas-runtime")?.style.width)
      .toBe("690px");
    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ revision: 0, snapshot: null }));
  });

  it("keeps an identical timeout poll native- and DOM-inert", async () => {
    const settings = parseCombatOverlaySettings({
      schemaVersion: 1, canvasWidth: 460, canvasHeight: 520, opacityPercent: 92,
      barOpacityPercent: 25, summaryOpacityPercent: 85, backgroundMode: "transparent",
      backgroundColor: "#0b1522", backgroundOpacityPercent: 0,
      customBackgroundRevision: null, liveOverlayEnabled: true, alwaysOnTop: true,
      clickThrough: false, autoHideOutsideCombat: false, autoHideDelaySeconds: 5,
      refreshIntervalMillis: 50, dynamicHeight: false, allowLiveResize: false,
      showViewTabs: false, maxVisiblePlayers: 20, scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "dps"], headerWidths: { name: 190, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"], buttons: [],
      }],
    });
    const timeoutWait = deferred<Response>();
    const parkedWait = deferred<Response>();
    let waits = 0;
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay") return Promise.resolve(response(settings));
      if (route === "/api/settings/core") {
        return Promise.resolve(response({
          pauseOverlayTimersOutsideCombat: false,
          overlayTimerInactivitySeconds: 8,
        }));
      }
      if (route === "/api/runtime/live/combat/wait") {
        waits += 1;
        return waits === 1 ? timeoutWait.promise : parkedWait.promise;
      }
      throw new Error(`Unexpected route ${route}`);
    }));
    const native = {
      close: vi.fn(async () => undefined),
      hide: vi.fn(async () => undefined),
      hideTemporarily: vi.fn(async () => undefined),
      showIfRequested: vi.fn(async () => undefined),
      setEnabled: vi.fn(async () => undefined),
      setAutomaticallyHidden: vi.fn(async () => undefined),
      setAlwaysOnTop: vi.fn(async () => undefined),
      setSize: vi.fn(async () => undefined),
      setIgnoreCursorEvents: vi.fn(async () => undefined),
      startDragging: vi.fn(async () => undefined),
      startResizeDragging: vi.fn(async () => undefined),
      heartbeat: vi.fn(async () => undefined),
    };
    const appWindow: CombatOverlayRuntimeWindow = {
      ...native,
      onShowRequested: async () => () => undefined,
      onResized: async () => () => undefined,
    };
    const container = document.createElement("div");
    document.body.append(container);
    await mountCombatOverlayRuntimeApp(container, appWindow);
    const paintedCanvas = container.querySelector(".combat-overlay-canvas-runtime");
    for (const method of Object.values(native)) method.mockClear();

    timeoutWait.resolve(response({ revision: 0, snapshot: null }));
    await flush();
    expect(container.querySelector(".combat-overlay-canvas-runtime")).toBe(paintedCanvas);
    expect(container.querySelectorAll(".combat-overlay-canvas-runtime")).toHaveLength(1);
    expect(container.querySelector(".combat-overlay-runtime-loading")).toBeNull();
    for (const method of Object.values(native)) expect(method).not.toHaveBeenCalled();

    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ revision: 0, snapshot: null }));
  });

  it("refetches when a resize starts and finishes during the reveal GET", async () => {
    let serverSettings = parseCombatOverlaySettings({
      schemaVersion: 1, canvasWidth: 460, canvasHeight: 520, opacityPercent: 92,
      barOpacityPercent: 25, summaryOpacityPercent: 85, backgroundMode: "solid",
      backgroundColor: "#0b1522", backgroundOpacityPercent: 92,
      customBackgroundRevision: null, liveOverlayEnabled: true, alwaysOnTop: true,
      clickThrough: false, autoHideOutsideCombat: false, autoHideDelaySeconds: 5,
      refreshIntervalMillis: 50, dynamicHeight: false, allowLiveResize: true,
      showViewTabs: false, maxVisiblePlayers: 20, scalePercent: 100,
      layers: [{
        id: "party-meter", title: "Party damage", metric: "dps", x: 0, y: 0, width: 460,
        headerFields: ["name", "dps"], headerWidths: { name: 190, dps: 90 },
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"], buttons: [],
      }],
    });
    const staleSettings = serverSettings;
    const staleGet = deferred<Response>();
    const parkedWait = deferred<Response>();
    let deferNextSettingsGet = false;
    let deferredGetStarted = false;
    let settingsGetCount = 0;
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay" && init?.method === "POST") {
        serverSettings = parseCombatOverlaySettings(JSON.parse(String(init.body)));
        return Promise.resolve(response(serverSettings));
      }
      if (route === "/api/settings/combat-overlay") {
        settingsGetCount += 1;
        if (deferNextSettingsGet) {
          deferNextSettingsGet = false;
          deferredGetStarted = true;
          return staleGet.promise;
        }
        return Promise.resolve(response(serverSettings));
      }
      if (route === "/api/settings/core") {
        return Promise.resolve(response({
          pauseOverlayTimersOutsideCombat: false,
          overlayTimerInactivitySeconds: 8,
        }));
      }
      if (route === "/api/runtime/live/combat/wait") return parkedWait.promise;
      throw new Error(`Unexpected route ${route}`);
    }));
    const resolved = async () => undefined;
    const showIfRequested = vi.fn(async () => undefined);
    let resized: ((width: number, height: number) => void) | null = null;
    let showRequested: (() => void | Promise<void>) | null = null;
    const appWindow: CombatOverlayRuntimeWindow = {
      close: resolved, hide: resolved, hideTemporarily: resolved, showIfRequested,
      setEnabled: resolved, setAutomaticallyHidden: resolved, setAlwaysOnTop: resolved,
      setSize: resolved, setIgnoreCursorEvents: resolved, startDragging: resolved,
      startResizeDragging: resolved, heartbeat: resolved,
      onShowRequested: async (handler) => { showRequested = handler; return () => undefined; },
      onResized: async (handler) => { resized = handler; return () => undefined; },
    };
    const container = document.createElement("div");
    document.body.append(container);
    await mountCombatOverlayRuntimeApp(container, appWindow);

    deferNextSettingsGet = true;
    const reveal = Promise.resolve(showRequested!());
    await vi.waitFor(() => expect(deferredGetStarted).toBe(true));
    resized!(690, 780);
    await vi.waitFor(() => expect(serverSettings.scalePercent).toBe(150));
    expect(showIfRequested).not.toHaveBeenCalled();
    staleGet.resolve(response(staleSettings));
    await reveal;

    expect(settingsGetCount).toBe(3);
    expect(serverSettings.scalePercent).toBe(150);
    expect(container.querySelector<HTMLElement>(".combat-overlay-canvas-runtime")?.style.width)
      .toBe("690px");
    expect(showIfRequested).toHaveBeenCalledOnce();
    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ revision: 0, snapshot: null }));
  });

  it("keeps live modules frameless while honoring the shared configurable background", async () => {
    const style = document.querySelector<HTMLStyleElement>("#rlogs-combat-overlay-styles");
    expect(style?.textContent).toContain(
      ".combat-overlay-canvas-runtime .combat-overlay-layer { border:0; border-radius:0; background:transparent; box-shadow:none; }",
    );
    expect(style?.textContent).not.toContain(
      ".combat-overlay-canvas-runtime .combat-overlay-layer::before { display:none; }",
    );
    expect(style?.textContent).toContain("opacity:var(--overlay-background-opacity, .92)");
    expect(style?.textContent).toContain("[data-background-mode='transparent']::before { opacity:0; }");
    expect(style?.textContent).toContain(
      "background:color-mix(in srgb,var(--accent,#64dfd2) 38%,#071018)",
    );
    expect(style?.textContent).toContain(
      ".combat-overlay-actor-row::before",
    );
  });
});
