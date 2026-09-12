import type { MountedSurface } from "../shell/types";
import type {
  AutomarkerLocalLoadResult,
  AutomarkerPoint,
  AutomarkerPresetView,
  LoadAutomarkerPresetRequest,
  ObservedMarkerSnapshot,
  SaveAutomarkerPresetRequest,
} from "./automarker-presets";
import { AUTOMARKER_EXCHANGE_MAX_BYTES, AUTOMARKER_PREVIEW_STORAGE_KEY, automarkerExportFilename, automarkerResponseIsCurrent, automarkerSaveRequest, newlyCreatedPresetId, observedMarkersMatchPresetView, parseAutomarkerPresetExchange, publishAutomarkerPreview, serializeAutomarkerPresetExchange } from "./automarker-presets";

export interface AutomarkerPresetDependencies {
  loadPresets(): Promise<AutomarkerPresetView>;
  loadObservedMarkers(): Promise<ObservedMarkerSnapshot>;
  saveCurrent(request: SaveAutomarkerPresetRequest): Promise<AutomarkerPresetView>;
  loadPreset(request: LoadAutomarkerPresetRequest): Promise<AutomarkerLocalLoadResult>;
  openOverlay(): Promise<void>;
}

export function mountAutomarkerPresetsSurface(
  container: HTMLElement,
  dependencies: AutomarkerPresetDependencies,
): MountedSurface {
  let alive = true;
  let view: AutomarkerPresetView | null = null;
  let selectedId: string | null = null;
  let observedMarkers: ObservedMarkerSnapshot | null = null;
  let busy = false;
  let editorDirty = false;
  let contextRefresh: number | null = null;
  let catalogRequestGeneration = 0;

  const root = el("div", "plugin-surface overlay-workspace-surface automarker-presets-surface");
  // Re-entering the editor starts a fresh deliberate preview gesture. Old
  // preview state must never silently reappear from a previous visit.
  window.localStorage.removeItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
  const intro = el("section", "content-card overlay-workspace-intro");
  const heading = el("div", "overlay-workspace-heading");
  heading.append(
    text("span", "AUTOMARKERS", "eyebrow"),
    text("h2", "Marker presets"),
    text("p", "Enter 1–6 numbered XYZ points, save them locally, and preview them on the Mechanics Map without sending anything to the game.", "card-copy"),
  );
  const sceneBadge = text("span", "NO SCENE", "overlay-menu-preview-badge");
  intro.append(heading, sceneBadge);

  const card = el("section", "content-card automarker-preset-card");
  const nameLabel = text("label", "Preset name", "automarker-field");
  const name = document.createElement("input");
  name.type = "text";
  name.maxLength = 80;
  name.placeholder = "M1 opener";
  nameLabel.append(name);
  const presetLabel = text("label", "Load → saved setup", "automarker-field");
  const select = document.createElement("select");
  presetLabel.append(select);
  const editor = el("section", "automarker-point-editor");
  const editorHeading = el("div", "automarker-point-editor-heading");
  editorHeading.append(text("strong", "Manual marker points"), text("span", "Local coordinates only"));
  const pointRows = el("div", "automarker-point-rows");
  const addPoint = button("Add point", "quiet-button");
  editor.append(editorHeading, pointRows, addPoint);
  const controls = el("div", "automarker-controls");
  const captureCurrent = button("Capture current markers", "quiet-button");
  const save = button("Save", "primary-button");
  const saveAs = button("Save As…", "quiet-button");
  const load = button("Load", "primary-button");
  const exportPreset = button("Export", "quiet-button");
  const importPreset = button("Import…", "quiet-button");
  const importFile = document.createElement("input");
  importFile.type = "file";
  importFile.accept = "application/json,.json";
  importFile.hidden = true;
  importFile.className = "automarker-import-file";
  const preview = button("Preview on map", "quiet-button");
  const placeInGame = button("Place in game", "primary-button");
  const refresh = button("Refresh scene", "quiet-button");
  controls.append(captureCurrent, save, saveAs, load, exportPreset, importPreset, preview, placeInGame, refresh, importFile);
  const status = text("p", "Connecting to the local marker store…", "card-copy automarker-status");
  const detail = el("div", "automarker-preset-detail");
  card.append(nameLabel, presetLabel, editor, controls, status, detail);
  root.append(intro, card);
  container.replaceChildren(root);

  select.addEventListener("change", () => {
    selectedId = select.value || null;
    render();
  });
  addPoint.addEventListener("click", () => {
    if (pointRows.children.length >= 6) return;
    const used = new Set(readEditorPoints(false).map((point) => point.markerNumber));
    const markerNumber = [1, 2, 3, 4, 5, 6].find((candidate) => !used.has(candidate)) ?? 1;
    appendPointRow({ markerNumber, x: 0, y: 0, z: 0 });
    editorDirty = true;
    render();
  });
  save.addEventListener("click", () => void persist(false));
  saveAs.addEventListener("click", () => void persist(true));
  preview.addEventListener("click", () => void previewOnMap());
  load.addEventListener("click", () => void requestLoad());
  exportPreset.addEventListener("click", exportSelectedPreset);
  importPreset.addEventListener("click", () => importFile.click());
  importFile.addEventListener("change", () => void importSelectedFile());
  captureCurrent.addEventListener("click", () => void captureCurrentMarkers());
  refresh.addEventListener("click", () => void refreshView());
  setEditorPoints([{ markerNumber: 1, x: 0, y: 0, z: 0 }]);
  void refreshView();
  contextRefresh = window.setInterval(() => { void refreshContext(); }, 2_000);

  async function refreshContext(): Promise<void> {
    if (!alive || busy) return;
    const requestGeneration = ++catalogRequestGeneration;
    try {
      const [next, nextObserved] = await Promise.all([
        dependencies.loadPresets(),
        dependencies.loadObservedMarkers(),
      ]);
      if (!alive || !automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration) ||
          (automarkerPresetContextKey(next) === automarkerPresetContextKey(view) &&
            observedMarkerSnapshotKey(nextObserved) === observedMarkerSnapshotKey(observedMarkers))) return;
      const contextChanged = automarkerPresetContextKey(next) !== automarkerPresetContextKey(view);
      view = next;
      observedMarkers = nextObserved;
      if (contextChanged) {
        selectedId = view.presets[0]?.presetId ?? null;
        name.value = view.presets[0]?.name ?? "";
        setEditorPoints(view.presets[0]?.points ?? [{ markerNumber: 1, x: 0, y: 0, z: 0 }]);
      }
      status.textContent = view.context === null
        ? "Enter a scene and wait for its build/map identity before saving or selecting presets."
        : view.presets.length === 0
          ? "No marker setups are saved for this dungeon family yet."
          : `${view.presets.length} compatible setup${view.presets.length === 1 ? "" : "s"} available for this scene.`;
      render();
    } catch {
      // Keep the last verified scene catalog visible. Explicit Refresh reports errors.
    }
  }

  async function refreshView(): Promise<void> {
    const requestGeneration = ++catalogRequestGeneration;
    busy = true;
    render();
    try {
      const [next, nextObserved] = await Promise.all([
        dependencies.loadPresets(),
        dependencies.loadObservedMarkers(),
      ]);
      if (!alive || !automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) return;
      view = next;
      observedMarkers = nextObserved;
      if (!view.presets.some((preset) => preset.presetId === selectedId)) selectedId = view.presets[0]?.presetId ?? null;
      const preset = selectedPreset();
      if (preset !== undefined && name.value.trim() === "") name.value = preset.name;
      if (!editorDirty && preset !== undefined) setEditorPoints(preset.points);
      status.textContent = view.context === null
        ? "Enter a scene and wait for its build/map identity before saving or selecting presets."
        : view.presets.length === 0
          ? "No marker setups are saved for this dungeon family yet."
          : `${view.presets.length} compatible setup${view.presets.length === 1 ? "" : "s"} available for this scene.`;
    } catch (error) {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        status.textContent = message(error);
      }
    } finally {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        busy = false;
        if (alive) render();
      }
    }
  }

  async function persist(saveAsNew: boolean): Promise<void> {
    if (view?.context === null || view === null) return;
    let request: SaveAutomarkerPresetRequest;
    try {
      request = automarkerSaveRequest(
        saveAsNew ? "save-as" : "save",
        selectedId,
        name.value,
        readEditorPoints(),
        view.context,
      );
    } catch (error) {
      status.textContent = message(error);
      return;
    }
    const requestGeneration = ++catalogRequestGeneration;
    busy = true;
    render();
    try {
      const priorIds = new Set(view.presets.map((preset) => preset.presetId));
      const next = await dependencies.saveCurrent(request);
      if (!alive || !automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) return;
      view = next;
      if (saveAsNew) selectedId = newlyCreatedPresetId(priorIds, view) ?? view.presets[0]?.presetId ?? null;
      editorDirty = false;
      status.textContent = saveAsNew ? "Saved a new marker setup on this computer." : "Updated the selected marker setup on this computer.";
    } catch (error) {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        status.textContent = message(error);
      }
    } finally {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        busy = false;
        if (alive) render();
      }
    }
  }

  async function previewOnMap(): Promise<void> {
    if (view?.context === null || view === null) return;
    try {
      const points = readEditorPoints();
      publishAutomarkerPreview(
        window.localStorage,
        view.context,
        name.value,
        points,
        view.previewSessionId,
      );
      status.textContent = `Previewing ${points.length} local marker${points.length === 1 ? "" : "s"} on the Mechanics Map. Nothing was sent to the game.`;
      await dependencies.openOverlay();
    } catch (error) {
      status.textContent = message(error);
    }
  }

  async function requestLoad(): Promise<void> {
    if (selectedId === null || view?.context === null || view === null) return;
    const requestedPresetId = selectedId;
    const requestedContextKey = automarkerPresetContextKey(view);
    const requestedPreviewSessionId = view.previewSessionId;
    const requestedCaptureSessionId = view.captureSessionId;
    const requestedDeploymentId = view.deploymentId;
    const requestedProtocolPackDigest = view.protocolPackDigest;
    const expectedContext = { ...view.context };
    const requestGeneration = ++catalogRequestGeneration;
    busy = true;
    render();
    try {
      const result = await dependencies.loadPreset({
        presetId: requestedPresetId,
        expectedContext,
      });
      if (!alive || !automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration) ||
          requestedContextKey !== automarkerPresetContextKey({
            context: result.context,
            previewSessionId: requestedPreviewSessionId,
            captureSessionId: requestedCaptureSessionId,
            deploymentId: requestedDeploymentId,
            protocolPackDigest: requestedProtocolPackDigest,
          }) || result.preset.presetId !== requestedPresetId) return;
      name.value = result.preset.name;
      setEditorPoints(result.preset.points);
      editorDirty = false;
      status.textContent = `Loaded ${result.preset.name} into the local editor. Nothing was sent to the game.`;
    } catch (error) {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        status.textContent = message(error);
      }
    } finally {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        busy = false;
        if (alive) render();
      }
    }
  }

  async function captureCurrentMarkers(): Promise<void> {
    if (!observedMarkerCaptureAvailability(view, observedMarkers).enabled || view === null) return;
    const requestedContextKey = automarkerPresetContextKey(view);
    const requestGeneration = ++catalogRequestGeneration;
    busy = true;
    render();
    try {
      const [freshView, snapshot] = await Promise.all([
        dependencies.loadPresets(),
        dependencies.loadObservedMarkers(),
      ]);
      if (!alive || !automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration) ||
          automarkerPresetContextKey(freshView) !== requestedContextKey) return;
      if (!observedMarkersMatchPresetView(snapshot, freshView)) {
        throw new Error(observedMarkerCaptureAvailability(freshView, snapshot).reason);
      }
      view = freshView;
      observedMarkers = snapshot;
      setEditorPoints(snapshot.markers);
      editorDirty = true;
      status.textContent = `Captured ${snapshot.markers.length} current in-game marker${snapshot.markers.length === 1 ? "" : "s"} into the local editor. Nothing was sent to the game.`;
    } catch (error) {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        status.textContent = message(error);
      }
    } finally {
      if (automarkerResponseIsCurrent(requestGeneration, catalogRequestGeneration)) {
        busy = false;
        if (alive) render();
      }
    }
  }

  function exportSelectedPreset(): void {
    const preset = selectedPreset();
    if (preset === undefined) return;
    const blob = new Blob([serializeAutomarkerPresetExchange(preset)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = automarkerExportFilename(preset.name);
    anchor.hidden = true;
    document.body.append(anchor);
    try {
      anchor.click();
      status.textContent = `Exported ${preset.name} without account, character, session, or build identity.`;
    } finally {
      anchor.remove();
      URL.revokeObjectURL(url);
    }
  }

  async function importSelectedFile(): Promise<void> {
    const file = importFile.files?.[0];
    if (file === undefined || view?.context === null || view === null) return;
    try {
      if (file.size > AUTOMARKER_EXCHANGE_MAX_BYTES) {
        throw new Error("The automarker preset file exceeds the 64 KiB safety limit.");
      }
      const contents = await file.text();
      if (!alive || view?.context === null || view === null) return;
      const imported = parseAutomarkerPresetExchange(contents, view.context.activityFamilyId);
      selectedId = null;
      name.value = imported.name;
      setEditorPoints(imported.points);
      editorDirty = true;
      status.textContent = `Imported ${imported.name} into the editor. Use Save As… to persist it; nothing was sent to the game.`;
    } catch (error) {
      if (alive) status.textContent = message(error);
    } finally {
      importFile.value = "";
      if (alive) render();
    }
  }

  function render(): void {
    const context = view?.context ?? null;
    sceneBadge.textContent = context === null ? "NO SCENE" : context.sceneName ?? `SCENE ${context.sceneId}`;
    sceneBadge.dataset.state = context === null ? "waiting" : "ready";
    select.replaceChildren();
    const showUnsavedDraft = context !== null && selectedId === null && editorDirty;
    if (showUnsavedDraft) {
      const option = new Option("Imported setup · Save As required", "");
      option.selected = true;
      select.append(option);
    }
    if (view === null || (view.presets.length === 0 && !showUnsavedDraft)) {
      const option = new Option(context === null ? "Enter a scene" : "No saved setups for this dungeon family", "");
      select.append(option);
      selectedId = null;
    } else if (view !== null) {
      for (const preset of view.presets) {
        const option = new Option(`${preset.name} · ${preset.points.length} marks`, preset.presetId);
        option.selected = preset.presetId === selectedId;
        select.append(option);
      }
    }
    const preset = selectedPreset();
    select.disabled = busy || context === null || view?.presets.length === 0;
    save.disabled = busy || context === null || preset === undefined;
    saveAs.disabled = busy || context === null;
    preview.disabled = busy || context === null;
    save.title = "Overwrite the selected setup with these explicitly entered local points";
    saveAs.title = "Save these explicitly entered local points as a new setup";
    preview.title = "Draw these points on the local Mechanics Map only";
    refresh.disabled = busy;
    load.disabled = busy || preset === undefined || context === null;
    load.title = "Restore the selected saved setup into this local editor";
    exportPreset.disabled = busy || preset === undefined;
    exportPreset.title = "Download the selected setup without local or game identity";
    importPreset.disabled = busy || context === null;
    importPreset.title = "Import an identity-free JSON setup for this dungeon family";
    const captureAvailability = observedMarkerCaptureAvailability(view, observedMarkers);
    captureCurrent.disabled = busy || !captureAvailability.enabled;
    captureCurrent.title = captureAvailability.reason;
    placeInGame.disabled = true;
    placeInGame.title = `The marker request is verified, but native placement is unavailable until rLogs can invoke the game's own request path (${view?.nativeLoadReason ?? "native_waymark_transport_unavailable"})`;
    detail.replaceChildren();
    if (context !== null) {
      detail.append(text("p", `${context.activityFamilyId} · Build ${context.clientBuild} · Scene ${context.sceneId} · Map ${context.mapId}`, "card-copy"));
    }
    if (preset !== undefined) {
      const list = el("ol", "automarker-point-list");
      for (const point of preset.points) {
        list.append(text("li", `${point.markerNumber}: X ${format(point.x)} · Y ${format(point.y)} · Z ${format(point.z)}`));
      }
      detail.append(list);
    }
    if (view !== null && !view.nativeLoadSupported) {
      detail.append(text("p", `Place in game is disabled (${view.nativeLoadReason}): the request format is verified, but rLogs does not yet have a safe native transport.`, "card-copy automarker-safety-note"));
    }
    if (observedMarkers?.captureActive && observedMarkers.requestObserverSupported) {
      const last = observedMarkers.lastVerifiedRequestMarkerNumber === null ||
        observedMarkers.lastVerifiedRequestObservedMicros === null
        ? "Place a marker through the normal game UI to test recognition."
        : `Last recognized: marker ${observedMarkers.lastVerifiedRequestMarkerNumber} at ${(observedMarkers.lastVerifiedRequestObservedMicros / 1_000_000).toFixed(3)}s of this capture.`;
      detail.append(text(
        "p",
        `Verified request observer active · ${observedMarkers.verifiedRequestCount} recognized. ${last}`,
        "card-copy automarker-request-diagnostic",
      ));
    }
    if (!captureAvailability.enabled) {
      detail.append(text("p", `Capture current markers is unavailable: ${captureAvailability.reason}`, "card-copy automarker-safety-note"));
    }
  }

  function setEditorPoints(points: readonly AutomarkerPoint[]): void {
    pointRows.replaceChildren();
    for (const point of points) appendPointRow(point);
    editorDirty = false;
  }

  function appendPointRow(point: AutomarkerPoint): void {
    const row = el("div", "automarker-point-row");
    const number = coordinateInput("#", point.markerNumber, 1, 6, 1);
    const x = coordinateInput("X", point.x);
    const y = coordinateInput("Y", point.y);
    const z = coordinateInput("Z", point.z);
    number.input.dataset.coordinate = "markerNumber";
    x.input.dataset.coordinate = "x";
    y.input.dataset.coordinate = "y";
    z.input.dataset.coordinate = "z";
    const remove = button("Remove", "quiet-button");
    remove.addEventListener("click", () => {
      if (pointRows.children.length <= 1) return;
      row.remove();
      editorDirty = true;
      render();
    });
    for (const field of [number.input, x.input, y.input, z.input]) {
      field.addEventListener("input", () => { editorDirty = true; });
    }
    row.append(number.label, x.label, y.label, z.label, remove);
    pointRows.append(row);
  }

  function readEditorPoints(strict = true): AutomarkerPoint[] {
    const points = [...pointRows.querySelectorAll<HTMLElement>(".automarker-point-row")].map((row) => {
      const values = ["markerNumber", "x", "y", "z"].map((coordinate) =>
        row.querySelector<HTMLInputElement>(`[data-coordinate="${coordinate}"]`)?.value ?? "",
      );
      if (strict && values.some((value) => value.trim() === "")) {
        throw new Error("Every marker needs a number plus X, Y, and Z coordinates.");
      }
      return { markerNumber: Number(values[0]), x: Number(values[1]), y: Number(values[2]), z: Number(values[3]) };
    });
    if (strict && points.some((point) => !Number.isFinite(point.markerNumber + point.x + point.y + point.z))) {
      throw new Error("Every marker needs finite X, Y, and Z coordinates.");
    }
    return points;
  }

  function selectedPreset() {
    return view?.presets.find((preset) => preset.presetId === selectedId);
  }

  return { dispose() { alive = false; catalogRequestGeneration += 1; if (contextRefresh !== null) window.clearInterval(contextRefresh); } };
}

export function automarkerPresetContextKey(
  view: Pick<AutomarkerPresetView, "context" | "previewSessionId" | "captureSessionId" | "deploymentId" | "protocolPackDigest"> | null,
): string {
  if (view === null || view.context === null) return "none";
  const context = view.context;
  return `${view.previewSessionId}:${view.captureSessionId ?? ""}:${view.deploymentId ?? ""}:${view.protocolPackDigest ?? ""}:${context.activityFamilyId}:${context.clientBuild}:${context.sceneId}:${context.mapId}`;
}

export function observedMarkerCaptureAvailability(
  view: AutomarkerPresetView | null,
  snapshot: ObservedMarkerSnapshot | null,
): { enabled: boolean; reason: string } {
  if (view?.context === null || view === null) return { enabled: false, reason: "Enter a supported dungeon scene first." };
  if (snapshot === null) return { enabled: false, reason: "Waiting for the live marker observer." };
  if (!snapshot.captureActive) return { enabled: false, reason: "Live packet monitoring is not running." };
  if (!snapshot.protocolSupported) {
    return { enabled: false, reason: `Marker observation is not protocol-verified for build ${snapshot.clientBuild ?? view.context.clientBuild}.` };
  }
  if (!observedMarkersMatchPresetView(snapshot, view)) {
    if (snapshot.reason === "no_fully_positioned_markers_observed") {
      return { enabled: false, reason: "No fully positioned in-game markers are currently observed." };
    }
    if (snapshot.reason === "observed_marker_snapshot_invalid") {
      return { enabled: false, reason: "The observed marker set is incomplete or ambiguous." };
    }
    return { enabled: false, reason: "Waiting for marker observations from this exact capture session, build, scene, and map." };
  }
  return { enabled: true, reason: "Copy the current in-game marker positions into the local editor." };
}

function observedMarkerSnapshotKey(snapshot: ObservedMarkerSnapshot | null): string {
  return snapshot === null ? "none" : `${snapshot.revision}:${snapshot.sessionId ?? ""}:${snapshot.sceneId ?? ""}:${snapshot.mapId ?? ""}`;
}

function format(value: number): string { return value.toFixed(3).replace(/\.0+$/, "").replace(/(\.\d*?)0+$/, "$1"); }
function message(error: unknown): string { return error instanceof Error ? error.message : String(error); }
function button(label: string, className: string): HTMLButtonElement { const node = text("button", label, className); node.type = "button"; return node; }
function coordinateInput(labelText: string, value: number, min?: number, max?: number, step: string | number = "any") {
  const label = text("label", labelText);
  const input = document.createElement("input");
  input.type = "number";
  input.value = String(value);
  input.step = String(step);
  if (min !== undefined) input.min = String(min);
  if (max !== undefined) input.max = String(max);
  label.append(input);
  return { label, input };
}
function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const node = document.createElement(tag); if (className) node.className = className; return node; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = el(tag, className); node.textContent = value; return node; }
