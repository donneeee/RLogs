// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UiLocalizer } from "../localization/ui-locale";
import type { AutomarkerPresetView } from "./automarker-presets";
import type { MechanicsMapSnapshot, MechanicsMapUpdate } from "./mechanics-map";
import { mountMechanicsMapOverlay } from "./mechanics-map-overlay";

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
  window.localStorage.clear();
});

describe("mounted Mechanics Map automarker request ordering", () => {
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
      hide: async () => undefined,
      setInteractive: async () => undefined,
      onInteractivity: async () => () => undefined,
      onFocusHeld: async () => () => undefined,
      loadAutomarkerPresets: () => catalogs.shift()!.promise,
      loadAutomarkerPreset: async () => ({ supported: false, reason: "native_waymark_request_unverified" }),
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
