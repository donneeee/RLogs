// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import type { UiLocalizer } from "../localization/ui-locale";
import { AUTOMARKER_PREVIEW_STORAGE_KEY, AUTOMARKER_PREVIEW_TTL_MILLIS, parseAutomarkerPreview, type AutomarkerPresetView } from "./automarker-presets";
import type { MechanicsMapSnapshot, MechanicsMapUpdate } from "./mechanics-map";
import { mountMechanicsMapOverlay } from "./mechanics-map-overlay";
import { parseOverlayLayoutSettings, type OverlayLayoutSettings } from "./overlay-layout";
import { LocalHostHttpError } from "../shell/local-host-http";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function snapshot(sceneId: number, revision: number): MechanicsMapUpdate {
  const value: MechanicsMapSnapshot = {
    schema_version: 14,
    revision,
    session_id: "session-test",
    client_build: "24687926",
    scene_id: sceneId,
    map_id: sceneId,
    scene_name: `Scene ${sceneId}`,
    map_model: "player_relative_radar",
    map_layout: null,
    world_radius: 100,
    map_origin_x: null,
    map_origin_z: null,
    map_span_x: null,
    map_span_z: null,
    background_asset_url: null,
    local_actor_id: null,
    local_position_observed: false,
    player: null,
    party: [],
    action_controls: [],
    resources: [],
    encounter_pack: null,
    encounter_pack_reviewed: false,
    target: null,
    dungeon: null,
    entities: [],
    mechanics: [],
    markers: [],
    data_gap: null,
    last_event_sequence: null,
    last_observed_micros: null,
  };
  return { schema_version: 14, revision, snapshot: value };
}

function catalog(sceneId: number, familyId: string, name: string): AutomarkerPresetView {
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
      points: [{ markerNumber: 1, x: 1, y: 2, z: 3 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_request_unverified",
    previewSessionId: "preview-test-session",
  };
}

const localizer: UiLocalizer = {
  locale: "en-US",
  loadedLocales: ["en-US"],
  t: (key) => key,
  formatNumber: (value) => String(value),
};

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function layout(): OverlayLayoutSettings {
  const modules = Object.fromEntries(["map", "player", "actions", "party", "target", "objectives", "alerts"].map((id, index) => [id, {
    x: 0, y: 0, width: .3, height: .3, visible: true, zOrder: index, opacity: 1, scale: 1,
  }]));
  return parseOverlayLayoutSettings({ schemaVersion: 2, revision: 1, canvasEnabled: true, selectedSetupId: "default", legacyMigrationComplete: true,
    setups: { default: { name: "Default", locked: false, modules } } });
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
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  document.head.querySelectorAll("[data-overlay-style-test]").forEach((element) => element.remove());
  delete document.documentElement.dataset.surface;
  delete document.documentElement.dataset.background;
  delete document.body.dataset.surface;
  document.body.removeAttribute("id");
  window.localStorage.clear();
});

describe("mounted Mechanics Map automarker request ordering", () => {
  it("exits edit mode on Escape without hiding widgets or consuming editor input Escape", async () => {
    document.documentElement.dataset.surface = "overlay-canvas";
    document.documentElement.dataset.background = "aurora";
    document.body.dataset.surface = "overlay-canvas";
    const styles = document.createElement("style");
    styles.dataset.overlayStyleTest = "true";
    styles.textContent = readFileSync("src/styles/shell.css", "utf8");
    document.head.append(styles);
    const shared = layout();
    shared.setups.default!.modules.map!.opacity = 0.43;
    shared.setups.default!.modules.player!.opacity = 0.61;
    const hide = vi.fn(async () => undefined);
    const acknowledgeInteractivity = vi.fn(async () => undefined);
    const setInteractive = vi.fn(async () => undefined);
    let interactivityHandler: ((interactive: boolean) => void) | undefined;
    let layoutRefreshHandler: ((revision: number) => void) | undefined;
    const removeInteractivity = vi.fn();
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => ({
      ...structuredClone(value),
      revision: value.revision + 1,
    }));
    const container = document.createElement("div");
    container.id = "app";
    document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1),
      waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined,
      ...{ hide },
      setInteractive,
      acknowledgeInteractivity,
      onInteractivity: async (handler) => { interactivityHandler = handler; return removeInteractivity; },
      onLayoutRefresh: async (handler) => { layoutRefreshHandler = handler; return () => undefined; },
      onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({
        supported: false,
        reason: "native_waymark_request_unverified",
      }),
      loadLayout: async () => shared,
      saveLayout,
    }, localizer);
    await flushPromises();
    setInteractive.mockClear();

    const root = container.querySelector<HTMLElement>(".overlay-canvas-runtime")!;
    const map = container.querySelector<HTMLElement>(".mechanics-map-overlay-runtime")!;
    const player = container.querySelector<HTMLElement>(".player-frame-overlay-runtime")!;
    const resize = container.querySelector<HTMLElement>(".mechanics-map-overlay-resize")!;
    expect(root.dataset.locked).toBe("false");
    expect(root.dataset.mode).toBe("edit");
    expect(["transparent", "rgba(0, 0, 0, 0)"]).not.toContain(
      getComputedStyle(root).backgroundColor,
    );
    expect(["transparent", "rgba(0, 0, 0, 0)"]).not.toContain(
      getComputedStyle(map).backgroundColor,
    );
    expect(map.style.opacity).toBe("0.43");
    expect(player.style.opacity).toBe("0.61");

    const select = container.querySelector("select")!;
    const editorEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    select.dispatchEvent(editorEscape);
    await flushPromises();
    expect(editorEscape.defaultPrevented).toBe(false);
    expect(hide).not.toHaveBeenCalled();
    expect(setInteractive).not.toHaveBeenCalled();
    expect(acknowledgeInteractivity).not.toHaveBeenCalled();

    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    window.dispatchEvent(escape);
    await vi.waitFor(() => expect(setInteractive).toHaveBeenCalledWith(false));
    expect(escape.defaultPrevented).toBe(true);
    expect(acknowledgeInteractivity).toHaveBeenCalledWith(true);
    expect(hide).not.toHaveBeenCalled();
    expect(root.dataset.locked).toBe("true");
    expect(root.dataset.mode).toBe("passive");
    expect(map.dataset.locked).toBe("true");
    expect(resize.isConnected).toBe(true);
    expect(map.isConnected).toBe(true);
    expect(player.isConnected).toBe(true);
    expect(getComputedStyle(resize).display).toBe("none");
    expect(getComputedStyle(map.querySelector(".mechanics-map-overlay-toolbar")!).display).toBe("none");
    for (const surface of [document.documentElement, document.body, container, root]) {
      const computed = getComputedStyle(surface);
      expect(["transparent", "rgba(0, 0, 0, 0)"]).toContain(computed.backgroundColor);
      expect(["", "none"]).toContain(computed.backgroundImage);
      expect(["", "none"]).toContain(computed.boxShadow);
      expect(["", "none"]).toContain(computed.backdropFilter);
    }
    for (const module of container.querySelectorAll<HTMLElement>(
      ".mechanics-map-overlay-runtime, .player-frame-overlay-runtime, .action-controls-overlay-runtime, .party-frame-overlay-runtime, .target-frame-overlay-runtime, .dungeon-objectives-overlay-runtime, .mechanic-alerts-overlay-runtime",
    )) {
      const computed = getComputedStyle(module);
      expect(["", "transparent", "rgba(0, 0, 0, 0)"]).toContain(computed.backgroundColor);
      expect(["", "0px"]).toContain(computed.borderWidth);
      expect(["", "none"]).toContain(computed.boxShadow);
    }
    const runtimeCards = [
      "party-frame-overlay-member", "dungeon-objectives-overlay-attempt",
      "dungeon-objectives-overlay-row", "mechanic-alerts-overlay-row",
    ].map((className) => {
      const card = document.createElement("article");
      card.className = className;
      root.append(card);
      return card;
    });
    for (const runtimeCard of runtimeCards) {
      const computed = getComputedStyle(runtimeCard);
      expect(["", "transparent", "rgba(0, 0, 0, 0)"]).toContain(computed.backgroundColor);
      expect(["", "transparent", "rgba(0, 0, 0, 0)"]).toContain(computed.borderColor);
      expect(["", "none"]).toContain(computed.boxShadow);
    }
    expect(map.style.opacity).toBe("0.43");
    expect(player.style.opacity).toBe("0.61");
    await vi.waitFor(() => expect(saveLayout).toHaveBeenCalled());
    const saved = saveLayout.mock.calls[0]![0];
    expect(saved.setups.default!.locked).toBe(true);
    expect(saved.setups.default!.modules).toEqual(shared.setups.default!.modules);

    interactivityHandler?.(true);
    await vi.waitFor(() => expect(root.dataset.locked).toBe("false"));
    expect(root.dataset.mode).toBe("edit");
    await vi.waitFor(() => expect(saveLayout).toHaveBeenCalled());
    await flushPromises();
    setInteractive.mockClear(); saveLayout.mockClear(); hide.mockClear();
    const done = Array.from(container.querySelectorAll("button"))
      .find((button) => button.textContent === "Done") as HTMLButtonElement;
    const doneAcknowledged = deferred<undefined>();
    acknowledgeInteractivity.mockImplementationOnce(() => doneAcknowledged.promise);
    done.click();
    expect(root.dataset.locked).toBe("true");
    expect(root.dataset.mode).toBe("passive");
    const originalSharedRevision = shared.revision;
    shared.revision += 100;
    layoutRefreshHandler?.(shared.revision);
    await flushPromises();
    expect(root.dataset.mode).toBe("passive");
    shared.revision = originalSharedRevision;
    expect(getComputedStyle(resize).display).toBe("none");
    expect(getComputedStyle(document.documentElement).backgroundColor).toBe("transparent");
    expect(getComputedStyle(document.body).backgroundColor).toBe("transparent");
    expect(getComputedStyle(container).backgroundColor).toBe("transparent");
    expect(getComputedStyle(root).backgroundColor).toBe("transparent");
    expect(setInteractive).not.toHaveBeenCalled();
    doneAcknowledged.resolve(undefined);
    await vi.waitFor(() => expect(setInteractive).toHaveBeenCalledWith(false));
    await flushPromises();
    expect(root.dataset.locked).toBe("true");
    expect(map.isConnected).toBe(true);
    expect(player.isConnected).toBe(true);
    expect(hide).not.toHaveBeenCalled();

    interactivityHandler?.(true);
    await vi.waitFor(() => expect(root.dataset.locked).toBe("false"));
    expect(root.dataset.mode).toBe("edit");
    await vi.waitFor(() => expect(saveLayout).toHaveBeenCalled());
    await flushPromises();
    setInteractive.mockClear(); saveLayout.mockClear();
    setInteractive.mockRejectedValueOnce(new Error("native click-through failed"));
    done.click();
    expect(root.dataset.locked).toBe("true");
    await vi.waitFor(() => expect(root.dataset.locked).toBe("false"));
    expect(map.dataset.locked).toBe("false");
    expect(getComputedStyle(resize).display).not.toBe("none");
    expect(getComputedStyle(map.querySelector(".mechanics-map-overlay-toolbar")!).display).not.toBe("none");
    expect(saveLayout).not.toHaveBeenCalled();
    expect(JSON.parse(window.localStorage.getItem("rlogs.mechanics-map-overlay.canvas.v1") ?? "null").locked).toBe(false);
    expect(hide).not.toHaveBeenCalled();

    const interactivityCallsAfterEscape = setInteractive.mock.calls.length;
    mounted.dispose();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flushPromises();
    expect(hide).not.toHaveBeenCalled();
    expect(setInteractive).toHaveBeenCalledTimes(interactivityCallsAfterEscape);
    expect(saveLayout).not.toHaveBeenCalled();
    expect(removeInteractivity).toHaveBeenCalledOnce();
  });

  it("migrates legacy pixel geometry once into normalized host layout", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1_000 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    window.localStorage.setItem("rlogs.mechanics-map-overlay.canvas.v1", JSON.stringify({ moduleX: 100, moduleY: 80, moduleWidth: 500, moduleHeight: 400 }));
    const shared = layout(); shared.legacyMigrationComplete = false;
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => ({ ...structuredClone(value), revision: 2 }));
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => shared, saveLayout,
    }, localizer);
    await flushPromises();
    const migrated = saveLayout.mock.calls[0]![0];
    expect(migrated.legacyMigrationComplete).toBe(true);
    expect(migrated.setups.default!.modules.map).toMatchObject({ x: .1, y: .1, width: .5, height: .5 });
    mounted.dispose();
  });

  it("rebases a legacy migration conflict onto a fresh revision while the flag remains false", async () => {
    window.localStorage.setItem("rlogs.mechanics-map-overlay.canvas.v1", JSON.stringify({ moduleX: 100, moduleY: 80, moduleWidth: 500, moduleHeight: 400 }));
    const initial = layout(); initial.revision = 1; initial.legacyMigrationComplete = false;
    const fresh = structuredClone(initial); fresh.revision = 2;
    let loads = 0; let saves = 0;
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => {
      saves += 1; if (saves === 1) throw new Error("409 conflict");
      return { ...structuredClone(value), revision: 3 };
    });
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => (++loads === 1 ? initial : fresh), saveLayout,
    }, localizer);
    await flushPromises(); await flushPromises();
    expect(saveLayout).toHaveBeenCalledTimes(2);
    expect(saveLayout.mock.calls[1]![0].revision).toBe(2);
    expect(saveLayout.mock.calls[1]![0].legacyMigrationComplete).toBe(true);
    expect(window.localStorage.getItem("rlogs.mechanics-map-overlay.canvas.v1")).not.toBeNull();
    mounted.dispose();
  });

  it("signals layout initialization before acknowledging forced edit on a persisted locked canvas", async () => {
    const shared = layout(); shared.setups.default!.locked = true;
    const order: string[] = [];
    let interactivity!: (interactive: boolean) => void;
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined,
      setInteractive: async (value) => { order.push(`set:${value}`); },
      acknowledgeInteractivity: async (value) => { order.push(`ack:${value}`); },
      onLayoutInitialized: () => { order.push("initialized"); },
      onInteractivity: async (handler) => { interactivity = handler; return () => undefined; },
      onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => shared,
      saveLayout: async (value) => ({ ...structuredClone(value), revision: value.revision + 1 }),
    }, localizer);
    await flushPromises();
    expect(order.slice(0, 2)).toEqual(["set:false", "initialized"]);
    interactivity(true);
    await flushPromises(); await flushPromises();
    expect(order).toContain("set:true");
    expect(order.at(-1)).toBe("ack:true");
    mounted.dispose();
  });

  it("applies and acknowledges a required revision without allowing an older poll to roll it back", async () => {
    vi.useFakeTimers();
    const initial = layout();
    const stale = structuredClone(initial); stale.revision = 2; stale.setups.default!.modules.map.opacity = .8;
    const current = structuredClone(initial); current.revision = 3; current.setups.default!.modules.map.opacity = .45;
    const oldPoll = deferred<OverlayLayoutSettings>();
    let loads = 0;
    let requestRefresh!: (revision: number) => void;
    const acknowledged: number[] = [];
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onLayoutInitialized: (revision) => { if (revision !== undefined) acknowledged.push(revision); },
      onLayoutRefresh: async (handler) => { requestRefresh = handler; return () => undefined; },
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => {
        loads += 1;
        if (loads === 1) return initial;
        if (loads === 2) return oldPoll.promise;
        return current;
      },
      saveLayout: async (value) => value,
    }, localizer);
    await flushPromises();
    await vi.advanceTimersByTimeAsync(1_000);
    requestRefresh(3);
    await flushPromises();
    const map = container.querySelector<HTMLElement>(".mechanics-map-overlay-runtime")!;
    expect(map.style.opacity).toBe("0.45");
    expect(acknowledged).toContain(3);

    oldPoll.resolve(stale);
    await flushPromises();
    expect(map.style.opacity).toBe("0.45");
    expect(acknowledged).not.toContain(2);
    mounted.dispose();
    vi.useRealTimers();
  });

  it("persists pointer movement and raises the moved module in shared layout order", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1_000 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
    let shared = layout();
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => {
      shared = structuredClone(value); shared.revision += 1; return shared;
    });
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => shared, saveLayout,
    }, localizer);
    await flushPromises();
    const toolbar = container.querySelector(".mechanics-map-overlay-toolbar")!;
    const pointer = (type: string, x: number) => {
      const event = new MouseEvent(type, { bubbles: true, button: 0, clientX: x, clientY: 10 }) as MouseEvent & { pointerId: number };
      Object.defineProperty(event, "pointerId", { value: 7 }); return event;
    };
    toolbar.dispatchEvent(pointer("pointerdown", 10));
    toolbar.dispatchEvent(pointer("pointermove", 110));
    toolbar.dispatchEvent(pointer("pointerup", 110));
    await flushPromises();
    expect(saveLayout).toHaveBeenCalled();
    expect(shared.setups.default!.modules.map.x).toBeCloseTo(.1);
    expect(shared.setups.default!.modules.map.zOrder).toBeGreaterThan(shared.setups.default!.modules.alerts.zOrder);
    mounted.dispose();
  });

  it("queues rapid live-canvas edits as immutable operations while a save is in flight", async () => {
    const firstSave = deferred<OverlayLayoutSettings>();
    let shared = layout();
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => {
      if (saveLayout.mock.calls.length === 1) return firstSave.promise;
      shared = structuredClone(value); shared.revision += 1; return shared;
    });
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => shared, saveLayout,
    }, localizer);
    await flushPromises();
    const lock = [...container.querySelectorAll("button")].find((button) => button.textContent === "Lock")!;
    lock.click();
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Unlock")!.click();
    await flushPromises();
    firstSave.resolve({ ...structuredClone(saveLayout.mock.calls[0]![0]), revision: 2 });
    await flushPromises(); await flushPromises();
    expect(saveLayout).toHaveBeenCalledTimes(2);
    expect(saveLayout.mock.calls[0]![0].setups.default!.locked).toBe(true);
    expect(saveLayout.mock.calls[1]![0].setups.default!.locked).toBe(false);
    mounted.dispose();
  });

  it("rebases a live-canvas edit after 409 without losing concurrent host fields", async () => {
    const initial = layout();
    const fresh = layout(); fresh.revision = 2; fresh.setups.default!.modules.alerts.opacity = .55;
    let loads = 0;
    const saveLayout = vi.fn(async (value: OverlayLayoutSettings) => {
      if (saveLayout.mock.calls.length === 1) throw new LocalHostHttpError(409, "conflict");
      return { ...structuredClone(value), revision: 3 };
    });
    const container = document.createElement("div"); document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1), waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: async () => undefined, setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined, onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalog(1_633, "dungeon.1633", "Preset"),
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => (++loads === 1 ? initial : fresh), saveLayout,
    }, localizer);
    await flushPromises();
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Lock")!.click();
    await flushPromises(); await flushPromises();
    expect(saveLayout).toHaveBeenCalledTimes(2);
    expect(saveLayout.mock.calls[1]![0]).toMatchObject({ revision: 2 });
    expect(saveLayout.mock.calls[1]![0].setups.default!.locked).toBe(true);
    expect(saveLayout.mock.calls[1]![0].setups.default!.modules.alerts.opacity).toBe(.55);
    mounted.dispose();
  });

  it("previews the selected saved preset locally and clears it across scene transition and disposal", async () => {
    const transition = deferred<MechanicsMapUpdate>();
    const catalogs = [
      catalog(1_633, "dungeon.1633", "Master opener"),
      catalog(1_631, "tina-mindrealm", "Normal opener"),
    ];
    let waitCount = 0;
    const nativeLoad = vi.fn(async () => ({
      supported: false as const,
      reason: "native_waymark_request_unverified" as const,
    }));
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1),
      waitForSnapshot: () => waitCount++ === 0 ? transition.promise : new Promise(() => undefined),
      prepareLocalMaps: async () => undefined,
      setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined,
      onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: async () => catalogs.shift()!,
      loadAutomarkerPreset: nativeLoad,
      loadLayout: async () => layout(),
      saveLayout: async (value) => value,
    }, localizer);
    await flushPromises();

    const preview = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Preview locally") as HTMLButtonElement;
    expect(preview.disabled).toBe(false);
    preview.click();

    const masterRaw = window.localStorage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
    expect(masterRaw).not.toBeNull();
    const masterPreview = parseAutomarkerPreview(JSON.parse(masterRaw!));
    expect(masterPreview).toEqual({
      schemaVersion: 1,
      context: catalog(1_633, "dungeon.1633", "Master opener").context,
      name: "Master opener",
      points: [{ markerNumber: 1, x: 1, y: 2, z: 3 }],
      previewSessionId: "preview-test-session",
      issuedAtUnixMillis: masterPreview.issuedAtUnixMillis,
      expiresAtUnixMillis: masterPreview.issuedAtUnixMillis + AUTOMARKER_PREVIEW_TTL_MILLIS,
    });
    expect(container.querySelector(".automarker-overlay-picker small")?.textContent)
      .toContain("No game transmission");
    expect(nativeLoad).not.toHaveBeenCalled();

    transition.resolve(snapshot(1_631, 2));
    await flushPromises();
    expect(window.localStorage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY)).toBeNull();

    preview.click();
    const normalRaw = window.localStorage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
    expect(normalRaw).not.toBeNull();
    const normalPreview = parseAutomarkerPreview(JSON.parse(normalRaw!));
    expect(normalPreview).toEqual({
      schemaVersion: 1,
      context: catalog(1_631, "tina-mindrealm", "Normal opener").context,
      name: "Normal opener",
      points: [{ markerNumber: 1, x: 1, y: 2, z: 3 }],
      previewSessionId: "preview-test-session",
      issuedAtUnixMillis: normalPreview.issuedAtUnixMillis,
      expiresAtUnixMillis: normalPreview.issuedAtUnixMillis + AUTOMARKER_PREVIEW_TTL_MILLIS,
    });
    expect(nativeLoad).not.toHaveBeenCalled();

    mounted.dispose();
    expect(window.localStorage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY)).toBeNull();
    preview.click();
    expect(window.localStorage.getItem(AUTOMARKER_PREVIEW_STORAGE_KEY)).toBeNull();
    expect(nativeLoad).not.toHaveBeenCalled();
  });

  it("cannot resurrect an old-scene catalog after transition or disposal", async () => {
    const transition = deferred<MechanicsMapUpdate>();
    const oldSceneCatalog = deferred<AutomarkerPresetView>();
    const currentSceneCatalog = deferred<AutomarkerPresetView>();
    const disposedCatalog = deferred<AutomarkerPresetView>();
    const catalogs = [oldSceneCatalog, currentSceneCatalog, disposedCatalog];
    let waitCount = 0;
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountMechanicsMapOverlay(container, {
      loadSnapshot: async () => snapshot(1_633, 1),
      waitForSnapshot: () => waitCount++ === 0 ? transition.promise : new Promise(() => undefined),
      prepareLocalMaps: async () => undefined,
      setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined,
      onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: () => catalogs.shift()!.promise,
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
      loadLayout: async () => layout(),
      saveLayout: async (value) => value,
    }, localizer);
    await flushPromises();

    transition.resolve(snapshot(1_631, 2));
    await flushPromises();
    const picker = container.querySelector(".automarker-overlay-picker select") as HTMLSelectElement;
    expect(picker.textContent).toContain("No setups for this scene");

    currentSceneCatalog.resolve(catalog(1_631, "tina-mindrealm", "Current normal"));
    await flushPromises();
    expect(picker.textContent).toContain("Current normal");

    oldSceneCatalog.resolve(catalog(1_633, "dungeon.1633", "Stale master"));
    await flushPromises();
    expect(picker.textContent).toContain("Current normal");
    expect(picker.textContent).not.toContain("Stale master");

    const markerPresets = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Marker presets")!;
    markerPresets.click();
    mounted.dispose();
    disposedCatalog.resolve(catalog(1_631, "tina-mindrealm", "Disposed completion"));
    await flushPromises();

    expect(picker.textContent).toContain("Current normal");
    expect(picker.textContent).not.toContain("Disposed completion");
    expect(container.childElementCount).toBe(0);
  });
});
