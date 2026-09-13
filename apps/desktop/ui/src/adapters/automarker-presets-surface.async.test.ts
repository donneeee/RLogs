// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ActivateAutomarkerPresetRequest, AutomarkerLocalLoadResult, AutomarkerNativeActivationResult, AutomarkerPresetView, ObservedMarkerSnapshot } from "./automarker-presets";
import { loadUiLocalizer } from "../localization/ui-locale";
import {
  mountAutomarkerPresetsSurface as mountLocalizedAutomarkerPresetsSurface,
  operatorPlacementCanaryCommand as localizedOperatorPlacementCanaryCommand,
  type AutomarkerPresetDependencies,
} from "./automarker-presets-surface";

const localizer = await loadUiLocalizer("en-US");
const mountAutomarkerPresetsSurface = (container: HTMLElement, dependencies: AutomarkerPresetDependencies) =>
  mountLocalizedAutomarkerPresetsSurface(container, dependencies, localizer);
const operatorPlacementCanaryCommand = (
  view: Parameters<typeof localizedOperatorPlacementCanaryCommand>[0],
  selectedPresetId: string | null,
) => localizedOperatorPlacementCanaryCommand(view, selectedPresetId, localizer);

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
}

function unavailableObserved(build = "25247556"): ObservedMarkerSnapshot {
  return {
    schemaVersion: 4,
    revision: 1,
    captureActive: true,
    protocolSupported: false,
    requestObserverSupported: false,
    verifiedRequestCount: 0,
    lastVerifiedRequestMarkerNumber: null,
    lastVerifiedRequestObservedMicros: null,
    verifiedRequests: [],
    latestLocalPlayerPosition: null,
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
      schemaVersion: 4,
      revision: 9,
      captureActive: true,
      protocolSupported: true,
      requestObserverSupported: true,
      verifiedRequestCount: 2,
      lastVerifiedRequestMarkerNumber: 2,
      lastVerifiedRequestObservedMicros: 123_455,
      verifiedRequests: [
        { markerNumber: 1, observedMicros: 123_445 },
        { markerNumber: 2, observedMicros: 123_455 },
      ],
      latestLocalPlayerPosition: null,
      reason: "observed_markers_available",
      sessionId: catalog.captureSessionId,
      deploymentId: catalog.deploymentId,
      clientBuild: catalog.context!.clientBuild,
      protocolPackDigest: catalog.protocolPackDigest,
      sceneId: catalog.context!.sceneId,
      mapId: catalog.context!.mapId,
      observedMicros: 123_456,
      markers: [
        { markerNumber: 1, x: 11, y: 12, z: 13, observedMicros: 123_450 },
        { markerNumber: 2, x: 21, y: 22, z: 23, observedMicros: 123_456 },
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
    schemaVersion: 5,
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
    nativeStatus: { observerReady: false, synCandidateObserved: false, bpsrTupleConfirmed: false, markerCarrierObserved: false, returnConfirmed: false, activePlacementEnabled: false, failureCategory: null },
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
  it("renders every sanitized native readiness checkpoint without conflating the SYN and BPSR match", async () => {
    const catalog = view(6_525, "mech-facility", "Opener", 1);
    catalog.nativeStatus = {
      observerReady: true,
      synCandidateObserved: true,
      bpsrTupleConfirmed: false,
      markerCarrierObserved: true,
      returnConfirmed: true,
      activePlacementEnabled: false,
      failureCategory: null,
    };
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

    const milestones = container.querySelector(".automarker-native-status")?.textContent ?? "";
    expect(milestones).toContain("Native driver verified; passive connection observer ready");
    expect(milestones).toContain("Process-owned connection candidate observed");
    expect(milestones).toContain("Checking whether the connection candidate is the captured BPSR connection");
    expect(milestones).toContain("Verified marker request observed from the game");
    expect(milestones).toContain("Server confirmed the observed marker request");
    expect(milestones).toContain("Active placement is still disabled");
    expect(milestones).not.toMatch(/pid|address|port|epoch|packet bytes|call id/i);
    mounted.dispose();
  });

  it("copies a gated name-based operator placement evidence test without IDs, coordinates, clicks, or activation", async () => {
    const catalog = view(1_633, "dungeon.1633", "Boss's opener", 91.25);
    catalog.context!.clientBuild = "25247556";
    const copyCanaryCommand = vi.fn(async (_command: string) => undefined);
    const activatePreset = vi.fn(async (_request: ActivateAutomarkerPresetRequest): Promise<AutomarkerNativeActivationResult> => ({ activated: false, reason: "native_waymark_canary_not_ready" }));
    const saveCurrent = vi.fn(async () => catalog);
    const loadPreset = vi.fn(async () => { throw new Error("not used"); });
    const openOverlay = vi.fn(async () => undefined);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved("25247556"),
      saveCurrent,
      loadPreset,
      activatePreset,
      copyCanaryCommand,
      openOverlay,
    });
    await flushPromises();

    const copy = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Copy placement evidence test")!;
    expect(copy.disabled).toBe(false);
    expect(copy.title).toMatch(/select Marker 1 during its ten-second preparation countdown/i);
    expect(copy.title).toMatch(/one human click after aim settles/i);
    const placeInGame = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Place in game")!;
    expect(placeInGame.disabled).toBe(true);
    copy.click();
    await flushPromises();

    expect(copyCanaryCommand).toHaveBeenCalledOnce();
    const command = copyCanaryCommand.mock.calls[0]![0];
    expect(command).toBe(".\\run-bpsr-automarker-lifecycle-probe.ps1 -ArmOperatorPlacement -PresetName 'Boss''s opener'");
    expect(command).not.toContain(catalog.presets[0]!.presetId);
    expect(command).not.toMatch(/Target[XYZ]|91\.25/);
    expect(activatePreset).not.toHaveBeenCalled();
    expect(saveCurrent).not.toHaveBeenCalled();
    expect(loadPreset).not.toHaveBeenCalled();
    expect(openOverlay).not.toHaveBeenCalled();
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/start the command first/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/during its ten-second preparation countdown return to the game/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/open the marker menu, select Marker 1, and leave its reticle active/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/do not move the mouse after selecting it/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/click exactly once after the Marker 1 reticle visibly stops moving/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/eight-second confirmation window/i);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/never synthesizes a click/i);
    mounted.dispose();
  });

  it("arms the current compatible one-point Marker 1 preset and reports both bounded outcomes", async () => {
    const catalog = view(6_525, "mech-facility", "Opener", 7);
    catalog.nativeLoadSupported = true;
    catalog.nativeLoadReason = "native_waymark_canary_available";
    const outcomes: AutomarkerNativeActivationResult[] = [
      { activated: true, reason: "native_waymark_canary_armed" },
      { activated: false, reason: "native_waymark_canary_not_ready" },
    ];
    const activatePreset = vi.fn(async (_request: ActivateAutomarkerPresetRequest) => outcomes.shift()!);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      activatePreset,
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const place = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Place in game")!;
    expect(place.disabled).toBe(false);
    place.click();
    await flushPromises();
    expect(activatePreset).toHaveBeenLastCalledWith({
      presetId: catalog.presets[0]!.presetId,
      expectedContext: catalog.context,
    });
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/Marker 1 is armed/i);

    place.click();
    await flushPromises();
    expect(activatePreset).toHaveBeenCalledTimes(2);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/fresh verified placement authority is not ready/i);
    mounted.dispose();

    const multiPoint = view(6_525, "mech-facility", "Unsafe", 7);
    multiPoint.nativeLoadSupported = true;
    multiPoint.nativeLoadReason = "native_waymark_canary_available";
    multiPoint.presets = [{
      ...multiPoint.presets[0]!,
      points: [...multiPoint.presets[0]!.points, { markerNumber: 2, x: 4, y: 5, z: 6 }],
    }];
    const secondContainer = document.createElement("div");
    document.body.append(secondContainer);
    const secondMounted = mountAutomarkerPresetsSurface(secondContainer, {
      loadPresets: async () => multiPoint,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      activatePreset,
      openOverlay: async () => undefined,
    });
    await flushPromises();
    const blockedPlace = [...secondContainer.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Place in game")!;
    expect(blockedPlace.disabled).toBe(true);
    blockedPlace.click();
    await flushPromises();
    expect(activatePreset).toHaveBeenCalledTimes(2);
    secondMounted.dispose();
  });

  it("suppresses a delayed activation result after the selected preset changes", async () => {
    const catalog = view(6_525, "mech-facility", "First", 7);
    catalog.nativeLoadSupported = true;
    catalog.nativeLoadReason = "native_waymark_canary_available";
    catalog.presets = [
      catalog.presets[0]!,
      { ...catalog.presets[0]!, presetId: "preset-6525-11111111", name: "Second" },
    ];
    const pending = deferred<AutomarkerNativeActivationResult>();
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      activatePreset: async () => pending.promise,
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const place = [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Place in game")!;
    place.click();
    const select = container.querySelector("select")!;
    select.value = catalog.presets[1]!.presetId;
    select.dispatchEvent(new Event("change"));
    pending.resolve({ activated: true, reason: "native_waymark_canary_armed" });
    await flushPromises();

    expect(container.querySelector(".automarker-status")?.textContent).not.toMatch(/Marker 1 is armed/i);
    mounted.dispose();
  });

  it("suppresses a delayed activation error after the selected preset and context change", async () => {
    const catalog = view(6_525, "mech-facility", "First", 7);
    catalog.nativeLoadSupported = true;
    catalog.nativeLoadReason = "native_waymark_canary_available";
    catalog.presets = [
      catalog.presets[0]!,
      { ...catalog.presets[0]!, presetId: "preset-6525-22222222", name: "Second" },
    ];
    const pending = deferred<AutomarkerNativeActivationResult>();
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      activatePreset: async () => pending.promise,
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const status = container.querySelector(".automarker-status")!;
    const statusBeforeActivation = status.textContent;
    [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent === "Place in game")!.click();
    const select = container.querySelector("select")!;
    select.value = catalog.presets[1]!.presetId;
    select.dispatchEvent(new Event("change"));
    catalog.context = { ...catalog.context!, sceneId: 6_526, mapId: 6_526 };
    pending.reject(new Error("stale private native failure"));
    await flushPromises();

    expect(status.textContent).toBe(statusBeforeActivation);
    expect(status.textContent).not.toContain("stale private native failure");
    mounted.dispose();
  });

  it("fails the operator placement handoff closed for the wrong build, duplicate names, or control characters", () => {
    const catalog = view(1_633, "dungeon.1633", "Opener", 1);
    const presetId = catalog.presets[0]!.presetId;
    expect(operatorPlacementCanaryCommand(catalog, presetId).enabled).toBe(false);

    catalog.context!.clientBuild = "25247556";
    catalog.presets = [...catalog.presets, { ...catalog.presets[0]!, presetId: "duplicate" }];
    expect(operatorPlacementCanaryCommand(catalog, presetId).reason).toMatch(/unique/i);

    catalog.presets = [{ ...catalog.presets[0]!, name: "Bad\nName" }];
    expect(operatorPlacementCanaryCommand(catalog, presetId).reason).toMatch(/control/i);
  });

  it("requires the exact active family and exactly one Marker 1", () => {
    const catalog = view(1_633, "dungeon.1633", "Opener", 1);
    catalog.context!.clientBuild = "25247556";
    const presetId = catalog.presets[0]!.presetId;

    catalog.presets = [{ ...catalog.presets[0]!, activityFamilyId: "other-family" }];
    expect(operatorPlacementCanaryCommand(catalog, presetId).reason).toMatch(/exact active dungeon family/i);

    catalog.presets = [{ ...catalog.presets[0]!, activityFamilyId: "dungeon.1633", points: [] }];
    expect(operatorPlacementCanaryCommand(catalog, presetId).reason).toMatch(/exactly one Marker 1/i);

    catalog.presets = [{
      ...catalog.presets[0]!,
      activityFamilyId: "dungeon.1633",
      points: [
        { markerNumber: 1, x: 1, y: 2, z: 3 },
        { markerNumber: 1, x: 4, y: 5, z: 6 },
      ],
    }];
    expect(operatorPlacementCanaryCommand(catalog, presetId).reason).toMatch(/exactly one Marker 1/i);
  });

  it("exports the selected preset as a safe identity-free JSON download", async () => {
    const catalog = view(6_525, "mech-facility", "../../Méch: opener?", 7);
    let exportedBlob: Blob | null = null;
    let downloadedAs = "";
    vi.spyOn(URL, "createObjectURL").mockImplementation((blob) => {
      exportedBlob = blob as Blob;
      return "blob:test";
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      downloadedAs = this.download;
    });
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

    [...container.querySelectorAll("button")].find((button) => button.textContent === "Export")!.click();
    const exported = JSON.parse(await exportedBlob!.text()) as Record<string, unknown>;
    expect(downloadedAs).toBe("Mech-opener.rlogs-automarker.json");
    expect(Object.keys(exported).sort()).toEqual(["activityFamilyId", "kind", "name", "points", "version"]);
    expect(JSON.stringify(exported)).not.toMatch(/presetId|savedAt|clientBuild|sceneId|mapId|session|uuid/i);
    expect(container.querySelector(".automarker-status")?.textContent).toContain("without account, character, session, or build identity");
    mounted.dispose();
  });

  it("imports matching-family JSON as dirty new content that only Save As can persist", async () => {
    const catalog = view(6_525, "mech-facility", "Existing", 1);
    const saveCurrent = vi.fn(async () => catalog);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalog,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent,
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();
    const input = container.querySelector<HTMLInputElement>(".automarker-import-file")!;
    const portable = JSON.stringify({
      kind: "rlogs-automarker-preset", version: 1, name: "Shared setup",
      activityFamilyId: "mech-facility",
      points: [{ markerNumber: 2, x: 9, y: 8, z: 7 }],
    });
    Object.defineProperty(input, "files", {
      configurable: true,
      value: [{ size: portable.length, text: async () => portable }],
    });
    input.dispatchEvent(new Event("change"));
    await flushPromises();

    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Shared setup");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("9");
    expect(container.querySelector("select")?.textContent).toContain("Save As required");
    const save = [...container.querySelectorAll("button")].find((button) => button.textContent === "Save")!;
    expect(save.disabled).toBe(true);
    expect([...container.querySelectorAll("button")].find((button) => button.textContent === "Place in game")!.disabled).toBe(true);
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Save As…")!.click();
    await flushPromises();
    expect(saveCurrent).toHaveBeenCalledWith({
      presetId: null,
      name: "Shared setup",
      points: [{ markerNumber: 2, x: 9, y: 8, z: 7 }],
      expectedContext: catalog.context,
    });
    mounted.dispose();
  });

  it("rejects cross-family, malformed, and oversized imports without changing the editor", async () => {
    const catalog = view(6_525, "mech-facility", "Existing", 1);
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
    const input = container.querySelector<HTMLInputElement>(".automarker-import-file")!;
    const dispatchFile = async (size: number, contents: string) => {
      Object.defineProperty(input, "files", {
        configurable: true,
        value: [{ size, text: async () => contents }],
      });
      input.dispatchEvent(new Event("change"));
      await flushPromises();
    };
    const otherFamily = JSON.stringify({
      kind: "rlogs-automarker-preset", version: 1, name: "Wrong",
      activityFamilyId: "dungeon.1633", points: [{ markerNumber: 1, x: 2, y: 3, z: 4 }],
    });
    await dispatchFile(otherFamily.length, otherFamily);
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/another dungeon family/i);
    await dispatchFile(1, "{");
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/valid JSON/i);
    await dispatchFile(65_537, "{}");
    expect(container.querySelector(".automarker-status")?.textContent).toMatch(/64 KiB/i);
    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Existing");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("1");
    mounted.dispose();
  });

  it("rejects an import whose file read finishes after the active scene changes", async () => {
    const initial = view(6_525, "mech-facility", "Mech setup", 1);
    const reef = view(6_541, "sea-ringed-reef", "Reef setup", 9);
    const catalogs = [initial, reef];
    const fileText = deferred<string>();
    const portable = JSON.stringify({
      kind: "rlogs-automarker-preset", version: 1, name: "Late setup",
      activityFamilyId: "mech-facility",
      points: [{ markerNumber: 2, x: 70, y: 80, z: 90 }],
    });
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalogs.shift()!,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const input = container.querySelector<HTMLInputElement>(".automarker-import-file")!;
    Object.defineProperty(input, "files", {
      configurable: true,
      value: [{ size: portable.length, text: () => fileText.promise }],
    });
    input.dispatchEvent(new Event("change"));
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Refresh scene")!
      .click();
    await flushPromises();
    fileText.resolve(portable);
    await flushPromises();

    expect(container.querySelector(".automarker-status")?.textContent).toContain("active scene changed");
    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Reef setup");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("9");
    mounted.dispose();
  });

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
    const liveSlots = [...container.querySelectorAll<HTMLElement>(".automarker-live-slot")];
    expect(liveSlots).toHaveLength(2);
    expect(liveSlots[0]?.textContent).toContain("Marker 1 · inbound confirmed");
    expect(liveSlots[0]?.textContent).toContain("0.000s before the latest marker update");
    expect(liveSlots[0]?.textContent).toContain("Verified local request observed at 0.123s");
    expect(liveSlots[1]?.textContent).toContain("latest marker update");
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

  it("captures an authoritative set into an empty family and saves it without coordinate entry", async () => {
    const current = capturable();
    current.catalog.presets = [];
    const savedCatalog: AutomarkerPresetView = {
      ...current.catalog,
      presets: [{
        presetId: "preset-captured-00000001",
        name: "Scene 6525 markers",
        activityFamilyId: current.catalog.context!.activityFamilyId,
        savedAtUnixMillis: 2,
        points: current.snapshot.markers.map(({ markerNumber, x, y, z }) => ({ markerNumber, x, y, z })),
      }],
    };
    const saveCurrent = vi.fn(async () => savedCatalog);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => current.catalog,
      loadObservedMarkers: async () => current.snapshot,
      saveCurrent,
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();

    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Capture current markers")!
      .click();
    await flushPromises();
    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value)
      .toBe("Scene 6525 markers");
    expect([...container.querySelectorAll<HTMLInputElement>('input[data-coordinate="x"]')].map((input) => input.value))
      .toEqual(["11", "21"]);

    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Save As…")!
      .click();
    await flushPromises();
    expect(saveCurrent).toHaveBeenCalledWith({
      presetId: null,
      name: "Scene 6525 markers",
      points: [
        { markerNumber: 1, x: 11, y: 12, z: 13 },
        { markerNumber: 2, x: 21, y: 22, z: 23 },
      ],
      expectedContext: current.catalog.context,
    });
    expect(container.querySelector(".automarker-status")?.textContent).toContain("Saved a new marker setup");
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
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Load into editor")!.click();
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
    [...container.querySelectorAll("button")].find((button) => button.textContent === "Load into editor")!.click();
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

  it("clears an unsaved draft when an explicit refresh enters another dungeon family", async () => {
    const initial = view(6_525, "mech-facility", "Mech setup", 1);
    const reef = view(6_541, "sea-ringed-reef", "Reef setup", 9);
    const catalogs = [initial, reef];
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalogs.shift()!,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent: async () => { throw new Error("not used"); },
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const x = container.querySelector<HTMLInputElement>('input[data-coordinate="x"]')!;
    x.value = "77";
    x.dispatchEvent(new Event("input"));
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Refresh scene")!
      .click();
    await flushPromises();

    expect(container.querySelector(".overlay-menu-preview-badge")?.textContent).toBe("Scene 6541");
    expect((container.querySelector('input[placeholder="M1 opener"]') as HTMLInputElement).value).toBe("Reef setup");
    expect((container.querySelector('input[data-coordinate="x"]') as HTMLInputElement).value).toBe("9");
    expect(container.querySelector("select")?.textContent).not.toContain("Mech setup");
    mounted.dispose();
  });

  it("preserves an unsaved draft across a scene change inside the same dungeon family", async () => {
    const initial = view(6_521, "mech-facility", "Family setup", 1);
    const next = {
      ...initial,
      context: { ...initial.context!, sceneId: 6_525, mapId: 6_525, sceneName: "Scene 6525" },
    };
    const catalogs = [initial, next];
    const saveCurrent = vi.fn(async () => next);
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountAutomarkerPresetsSurface(container, {
      loadPresets: async () => catalogs.shift()!,
      loadObservedMarkers: async () => unavailableObserved(),
      saveCurrent,
      loadPreset: async () => { throw new Error("not used"); },
      openOverlay: async () => undefined,
    });
    await flushPromises();

    const name = container.querySelector<HTMLInputElement>('input[placeholder="M1 opener"]')!;
    const x = container.querySelector<HTMLInputElement>('input[data-coordinate="x"]')!;
    name.value = "Unsaved family draft";
    name.dispatchEvent(new Event("input"));
    x.value = "77";
    x.dispatchEvent(new Event("input"));
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Refresh scene")!
      .click();
    await flushPromises();

    expect(container.querySelector(".overlay-menu-preview-badge")?.textContent).toBe("Scene 6525");
    expect(name.value).toBe("Unsaved family draft");
    expect(x.value).toBe("77");
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "Save As…")!
      .click();
    await flushPromises();
    expect(saveCurrent).toHaveBeenCalledWith(expect.objectContaining({
      presetId: null,
      name: "Unsaved family draft",
      expectedContext: next.context,
    }));
    mounted.dispose();
  });
});
