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
        hiddenHeaderLabels: [], summaryFields: ["encounter_time"], buttons: [],
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
    const appWindow: CombatOverlayRuntimeWindow = {
      close: resolved, hide: resolved, hideTemporarily: resolved, showIfRequested: resolved,
      setEnabled: resolved, setAutomaticallyHidden: resolved, setAlwaysOnTop: resolved,
      setSize, setIgnoreCursorEvents: resolved, startDragging: resolved,
      startResizeDragging: resolved, heartbeat: resolved,
      onShowRequested: async () => () => undefined,
      onResized: async (handler) => { resized = handler; return () => undefined; },
    };
    const container = document.createElement("div");
    document.body.append(container);
    await mountCombatOverlayRuntimeApp(container, appWindow);

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
    expect(paintedCanvas?.textContent).toContain("Dummy armed");

    servedSettings = {
      ...settings,
      alwaysOnTop: false,
      clickThrough: true,
      liveOverlayEnabled: false,
      autoHideOutsideCombat: true,
      autoHideDelaySeconds: 17,
      refreshIntervalMillis: 2_000,
      backgroundMode: "transparent",
      backgroundColor: "#ffffff",
      backgroundOpacityPercent: 0,
      customBackgroundRevision: 9,
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

    setSize.mockClear();
    expect(resized).not.toBeNull();
    resized!(900, 900);
    await Promise.resolve();
    expect(setSize).toHaveBeenLastCalledWith(460, 80);
    expect(container.querySelector(".combat-overlay-canvas-runtime")).toBe(paintedCanvas);

    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ ...update, revision: 3 }));
  });

  it("adopts an external resize when live resizing is enabled", async () => {
    const settings = parseCombatOverlaySettings({
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
    vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
      const route = String(input);
      if (route === "/api/settings/combat-overlay") return Promise.resolve(response(settings));
      if (route === "/api/settings/core") return Promise.resolve(response({ pauseOverlayTimersOutsideCombat: false, overlayTimerInactivitySeconds: 8 }));
      if (route === "/api/runtime/live/combat/wait") return parkedWait.promise;
      throw new Error(`Unexpected route ${route}`);
    }));
    const resolved = async () => undefined;
    const setSize = vi.fn(async () => undefined);
    let resized: ((width: number, height: number) => void) | null = null;
    const appWindow: CombatOverlayRuntimeWindow = {
      close: resolved, hide: resolved, hideTemporarily: resolved, showIfRequested: resolved,
      setEnabled: resolved, setAutomaticallyHidden: resolved, setAlwaysOnTop: resolved,
      setSize, setIgnoreCursorEvents: resolved, startDragging: resolved,
      startResizeDragging: resolved, heartbeat: resolved,
      onShowRequested: async () => () => undefined,
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
    window.dispatchEvent(new Event("beforeunload"));
    parkedWait.resolve(response({ revision: 0, snapshot: null }));
  });

  it("styles every live module as a frameless transparent surface", async () => {
    const style = document.querySelector<HTMLStyleElement>("#rlogs-combat-overlay-styles");
    expect(style?.textContent).toContain(
      ".combat-overlay-canvas-runtime .combat-overlay-layer { border:0; border-radius:0; background:transparent; box-shadow:none; }",
    );
    expect(style?.textContent).toContain(
      ".combat-overlay-canvas-runtime .combat-overlay-layer::before { display:none; }",
    );
    expect(style?.textContent).toContain(
      ".combat-overlay-actor-row::before",
    );
  });
});
