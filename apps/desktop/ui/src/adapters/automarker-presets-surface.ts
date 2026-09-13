import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
import type {
  AutomarkerLocalLoadResult,
  ActivateAutomarkerPresetRequest,
  AutomarkerNativeActivationResult,
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
  // Deliberately not called while Place in game remains disabled.
  activatePreset?(request: ActivateAutomarkerPresetRequest): Promise<AutomarkerNativeActivationResult>;
  copyCanaryCommand?(command: string): Promise<void>;
  openOverlay(): Promise<void>;
}

const OPERATOR_PLACEMENT_CANARY_BUILD = "25247556";

export interface OperatorPlacementCanaryCommand {
  enabled: boolean;
  reason: string;
  command?: string;
}

export function operatorPlacementCanaryCommand(
  view: AutomarkerPresetView | null,
  selectedPresetId: string | null,
  localizer: UiLocalizer,
): OperatorPlacementCanaryCommand {
  if (view?.context === null || view === null) {
    return { enabled: false, reason: localizer.t("ui.automarkers.availability.enter_scene") };
  }
  if (view.context.clientBuild !== OPERATOR_PLACEMENT_CANARY_BUILD) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.build_only", { build: OPERATOR_PLACEMENT_CANARY_BUILD }) };
  }
  const matches = view.presets.filter((preset) => preset.presetId === selectedPresetId);
  if (matches.length !== 1) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.select_setup") };
  }
  const preset = matches[0]!;
  if (preset.activityFamilyId !== view.context.activityFamilyId) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.wrong_family") };
  }
  if (preset.points.filter((point) => point.markerNumber === 1).length !== 1) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.marker_one") };
  }
  if (preset.name.length === 0 || /\p{C}/u.test(preset.name)) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.invalid_name") };
  }
  if (view.presets.filter((candidate) => candidate.name === preset.name).length !== 1) {
    return { enabled: false, reason: localizer.t("ui.automarkers.canary.unique_name") };
  }
  const escapedName = preset.name.replaceAll("'", "''");
  return {
    enabled: true,
    reason: localizer.t("ui.automarkers.canary.ready"),
    command: `.\\run-bpsr-automarker-lifecycle-probe.ps1 -ArmOperatorPlacement -PresetName '${escapedName}'`,
  };
}

export function mountAutomarkerPresetsSurface(
  container: HTMLElement,
  dependencies: AutomarkerPresetDependencies,
  localizer: UiLocalizer,
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
    text("span", localizer.t("ui.automarkers.eyebrow"), "eyebrow"),
    text("h2", localizer.t("ui.automarkers.title")),
    text("p", localizer.t("ui.automarkers.description"), "card-copy"),
  );
  const sceneBadge = text("span", localizer.t("ui.automarkers.scene.none"), "overlay-menu-preview-badge");
  intro.append(heading, sceneBadge);

  const card = el("section", "content-card automarker-preset-card");
  const nameLabel = text("label", localizer.t("ui.automarkers.name.label"), "automarker-field");
  const name = document.createElement("input");
  name.type = "text";
  name.maxLength = 80;
  name.placeholder = localizer.t("ui.automarkers.name.placeholder");
  nameLabel.append(name);
  const presetLabel = text("label", localizer.t("ui.automarkers.saved_setup"), "automarker-field");
  const select = document.createElement("select");
  presetLabel.append(select);
  const editor = el("section", "automarker-point-editor");
  const editorHeading = el("div", "automarker-point-editor-heading");
  editorHeading.append(text("strong", localizer.t("ui.automarkers.points.title")), text("span", localizer.t("ui.automarkers.points.description")));
  const pointRows = el("div", "automarker-point-rows");
  const addPoint = button(localizer.t("ui.automarkers.action.add_point"), "quiet-button");
  editor.append(editorHeading, pointRows, addPoint);
  const controls = el("div", "automarker-controls");
  const captureCurrent = button(localizer.t("ui.automarkers.action.capture"), "quiet-button");
  const save = button(localizer.t("ui.automarkers.action.save"), "primary-button");
  const saveAs = button(localizer.t("ui.automarkers.action.save_as"), "quiet-button");
  const load = button(localizer.t("ui.automarkers.action.load"), "primary-button");
  const exportPreset = button(localizer.t("ui.automarkers.action.export"), "quiet-button");
  const importPreset = button(localizer.t("ui.automarkers.action.import"), "quiet-button");
  const importFile = document.createElement("input");
  importFile.type = "file";
  importFile.accept = "application/json,.json";
  importFile.hidden = true;
  importFile.className = "automarker-import-file";
  const preview = button(localizer.t("ui.automarkers.action.preview"), "quiet-button");
  const placeInGame = button(localizer.t("ui.automarkers.action.place"), "primary-button");
  const copyCanary = button(localizer.t("ui.automarkers.action.copy_canary"), "quiet-button");
  const refresh = button(localizer.t("ui.automarkers.action.refresh"), "quiet-button");
  controls.append(captureCurrent, save, saveAs, load, exportPreset, importPreset, preview, placeInGame, copyCanary, refresh, importFile);
  const status = text("p", localizer.t("ui.automarkers.status.connecting"), "card-copy automarker-status");
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
  copyCanary.addEventListener("click", () => void copyOperatorPlacementCanary());
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
      applyCatalog(next, nextObserved);
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
      applyCatalog(next, nextObserved);
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
      status.textContent = localizer.t(saveAsNew ? "ui.automarkers.status.saved_new" : "ui.automarkers.status.saved_update");
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
      status.textContent = localizer.t(points.length === 1
        ? "ui.automarkers.status.preview_one"
        : "ui.automarkers.status.preview_many", { count: localizer.formatNumber(points.length) });
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
      status.textContent = localizer.t("ui.automarkers.status.loaded", { name: result.preset.name });
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
    if (!observedMarkerCaptureAvailability(view, observedMarkers, localizer).enabled || view === null) return;
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
        throw new Error(observedMarkerCaptureAvailability(freshView, snapshot, localizer).reason);
      }
      view = freshView;
      observedMarkers = snapshot;
      setEditorPoints(snapshot.markers);
      if (name.value.trim() === "") {
        name.value = localizer.t("ui.automarkers.default_name", {
          scene: freshView.context?.sceneName ?? localizer.t("ui.automarkers.scene.current"),
        });
      }
      editorDirty = true;
      status.textContent = localizer.t(snapshot.markers.length === 1
        ? "ui.automarkers.status.captured_one"
        : "ui.automarkers.status.captured_many", { count: localizer.formatNumber(snapshot.markers.length) });
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
      status.textContent = localizer.t("ui.automarkers.status.exported", { name: preset.name });
    } finally {
      anchor.remove();
      URL.revokeObjectURL(url);
    }
  }

  async function importSelectedFile(): Promise<void> {
    const file = importFile.files?.[0];
    if (file === undefined || view?.context === null || view === null) return;
    const requestedContextKey = automarkerPresetContextKey(view);
    try {
      if (file.size > AUTOMARKER_EXCHANGE_MAX_BYTES) {
        throw new Error(localizer.t("ui.automarkers.error.file_too_large"));
      }
      const contents = await file.text();
      if (!alive || view?.context === null || view === null) return;
      if (automarkerPresetContextKey(view) !== requestedContextKey) {
        throw new Error(localizer.t("ui.automarkers.error.context_changed_during_import"));
      }
      const imported = parseAutomarkerPresetExchange(contents, view.context.activityFamilyId);
      selectedId = null;
      name.value = imported.name;
      setEditorPoints(imported.points);
      editorDirty = true;
      status.textContent = localizer.t("ui.automarkers.status.imported", { name: imported.name });
    } catch (error) {
      if (alive) status.textContent = message(error);
    } finally {
      importFile.value = "";
      if (alive) render();
    }
  }

  async function copyOperatorPlacementCanary(): Promise<void> {
    const availability = operatorPlacementCanaryCommand(view, selectedId, localizer);
    if (!availability.enabled || availability.command === undefined || dependencies.copyCanaryCommand === undefined) return;
    try {
      await dependencies.copyCanaryCommand(availability.command);
      if (!alive) return;
      status.textContent = localizer.t("ui.automarkers.status.canary_copied");
    } catch (error) {
      if (alive) status.textContent = message(error);
    }
  }

  function render(): void {
    const context = view?.context ?? null;
    sceneBadge.textContent = context === null
      ? localizer.t("ui.automarkers.scene.none")
      : context.sceneName ?? localizer.t("ui.automarkers.scene.id", { id: context.sceneId });
    sceneBadge.dataset.state = context === null ? "waiting" : "ready";
    select.replaceChildren();
    const showUnsavedDraft = context !== null && selectedId === null && editorDirty;
    if (showUnsavedDraft) {
      const option = new Option(localizer.t("ui.automarkers.select.imported"), "");
      option.selected = true;
      select.append(option);
    }
    if (view === null || (view.presets.length === 0 && !showUnsavedDraft)) {
      const option = new Option(localizer.t(context === null
        ? "ui.automarkers.select.enter_scene"
        : "ui.automarkers.select.no_setups"), "");
      select.append(option);
      selectedId = null;
    } else if (view !== null) {
      for (const preset of view.presets) {
        const option = new Option(localizer.t("ui.automarkers.select.preset", {
          name: preset.name, count: localizer.formatNumber(preset.points.length),
        }), preset.presetId);
        option.selected = preset.presetId === selectedId;
        select.append(option);
      }
    }
    const preset = selectedPreset();
    select.disabled = busy || context === null || view?.presets.length === 0;
    save.disabled = busy || context === null || preset === undefined;
    saveAs.disabled = busy || context === null;
    preview.disabled = busy || context === null;
    save.title = localizer.t("ui.automarkers.help.save");
    saveAs.title = localizer.t("ui.automarkers.help.save_as");
    preview.title = localizer.t("ui.automarkers.help.preview");
    refresh.disabled = busy;
    load.disabled = busy || preset === undefined || context === null;
    load.title = localizer.t("ui.automarkers.help.load");
    exportPreset.disabled = busy || preset === undefined;
    exportPreset.title = localizer.t("ui.automarkers.help.export");
    importPreset.disabled = busy || context === null;
    importPreset.title = localizer.t("ui.automarkers.help.import");
    const captureAvailability = observedMarkerCaptureAvailability(view, observedMarkers, localizer);
    captureCurrent.disabled = busy || !captureAvailability.enabled;
    captureCurrent.title = captureAvailability.reason;
    placeInGame.disabled = true;
    placeInGame.title = localizer.t("ui.automarkers.help.native_unavailable", {
      reason: view?.nativeLoadReason ?? "native_waymark_transport_unavailable",
    });
    const canary = operatorPlacementCanaryCommand(view, selectedId, localizer);
    copyCanary.disabled = busy || !canary.enabled || dependencies.copyCanaryCommand === undefined;
    copyCanary.title = dependencies.copyCanaryCommand === undefined
      ? localizer.t("ui.automarkers.help.clipboard_unavailable")
      : canary.reason;
    detail.replaceChildren();
    if (context !== null) {
      detail.append(text("p", localizer.t("ui.automarkers.context", {
        family: context.activityFamilyId, build: context.clientBuild, scene: context.sceneId, map: context.mapId,
      }), "card-copy"));
    }
    if (preset !== undefined) {
      const list = el("ol", "automarker-point-list");
      for (const point of preset.points) {
        list.append(text("li", localizer.t("ui.automarkers.point", {
          number: point.markerNumber, x: format(point.x), y: format(point.y), z: format(point.z),
        })));
      }
      detail.append(list);
    }
    if (view !== null && !view.nativeLoadSupported) {
      detail.append(text("p", localizer.t("ui.automarkers.native_disabled", { reason: view.nativeLoadReason }), "card-copy automarker-safety-note"));
    }
    if (observedMarkers?.captureActive && observedMarkers.requestObserverSupported) {
      const last = observedMarkers.lastVerifiedRequestMarkerNumber === null ||
        observedMarkers.lastVerifiedRequestObservedMicros === null
        ? localizer.t("ui.automarkers.observer.place_marker")
        : localizer.t("ui.automarkers.observer.last", {
            marker: observedMarkers.lastVerifiedRequestMarkerNumber,
            time: formatCaptureTime(observedMarkers.lastVerifiedRequestObservedMicros),
          });
      detail.append(text(
        "p",
        localizer.t("ui.automarkers.observer.active", {
          count: localizer.formatNumber(observedMarkers.verifiedRequestCount), last,
        }),
        "card-copy automarker-request-diagnostic",
      ));
    }
    if (observedMarkers !== null && observedMarkers.reason === "observed_markers_available") {
      const liveSlots = el("ol", "automarker-live-slot-list");
      const requests = new Map(observedMarkers.verifiedRequests.map((request) => [request.markerNumber, request.observedMicros]));
      for (const point of observedMarkers.markers) {
        const requestMicros = requests.get(point.markerNumber) ?? null;
        const row = el("li", "automarker-live-slot");
        row.dataset.markerNumber = String(point.markerNumber);
        row.append(
          text("strong", localizer.t("ui.automarkers.live.marker", { number: point.markerNumber })),
          text("span", observedMarkerFreshnessLabel(point.observedMicros, observedMarkers.observedMicros, localizer)),
          text("span", requestMicros === null
            ? localizer.t("ui.automarkers.live.no_request")
            : localizer.t("ui.automarkers.live.request", { time: formatCaptureTime(requestMicros) }), "automarker-live-request"),
        );
        liveSlots.append(row);
      }
      detail.append(
        text("p", localizer.t("ui.automarkers.live.title"), "card-copy automarker-live-heading"),
        liveSlots,
      );
    }
    if (!captureAvailability.enabled) {
      detail.append(text("p", localizer.t("ui.automarkers.capture_unavailable", {
        reason: captureAvailability.reason,
      }), "card-copy automarker-safety-note"));
    }
  }

  function applyCatalog(next: AutomarkerPresetView, nextObserved: ObservedMarkerSnapshot): void {
    const previousFamily = view?.context?.activityFamilyId ?? null;
    const nextFamily = next.context?.activityFamilyId ?? null;
    const familyChanged = previousFamily !== nextFamily;
    view = next;
    observedMarkers = nextObserved;

    if (familyChanged) {
      // A draft is marker-location data. Never leave it visible or reusable
      // after moving to another dungeon family (or leaving supported scenes).
      selectedId = view.presets[0]?.presetId ?? null;
      const preset = selectedPreset();
      name.value = preset?.name ?? "";
      setEditorPoints(preset?.points ?? [{ markerNumber: 1, x: 0, y: 0, z: 0 }]);
    } else {
      // Scene/map/build provenance can change inside one reviewed activity
      // family. Keep the user's selected setup and unsaved edits in that case.
      if (!view.presets.some((preset) => preset.presetId === selectedId)) {
        selectedId = editorDirty ? null : view.presets[0]?.presetId ?? null;
      }
      const preset = selectedPreset();
      if (!editorDirty) {
        name.value = preset?.name ?? "";
        setEditorPoints(preset?.points ?? [{ markerNumber: 1, x: 0, y: 0, z: 0 }]);
      }
    }

    status.textContent = view.context === null
      ? localizer.t("ui.automarkers.status.enter_scene")
      : view.presets.length === 0
        ? localizer.t("ui.automarkers.status.no_setups")
        : localizer.t(view.presets.length === 1
          ? "ui.automarkers.status.one_setup"
          : "ui.automarkers.status.many_setups", { count: localizer.formatNumber(view.presets.length) });
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
    const remove = button(localizer.t("ui.automarkers.action.remove"), "quiet-button");
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
        throw new Error(localizer.t("ui.automarkers.error.coordinates_required"));
      }
      return { markerNumber: Number(values[0]), x: Number(values[1]), y: Number(values[2]), z: Number(values[3]) };
    });
    if (strict && points.some((point) => !Number.isFinite(point.markerNumber + point.x + point.y + point.z))) {
      throw new Error(localizer.t("ui.automarkers.error.coordinates_finite"));
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
  localizer: UiLocalizer,
): { enabled: boolean; reason: string } {
  if (view?.context === null || view === null) return { enabled: false, reason: localizer.t("ui.automarkers.availability.enter_scene") };
  if (snapshot === null) return { enabled: false, reason: localizer.t("ui.automarkers.availability.waiting_observer") };
  if (!snapshot.captureActive) return { enabled: false, reason: localizer.t("ui.automarkers.availability.monitoring_stopped") };
  if (!snapshot.protocolSupported) {
    return { enabled: false, reason: localizer.t("ui.automarkers.availability.unverified_build", {
      build: snapshot.clientBuild ?? view.context.clientBuild,
    }) };
  }
  if (!observedMarkersMatchPresetView(snapshot, view)) {
    if (snapshot.reason === "no_fully_positioned_markers_observed") {
      return { enabled: false, reason: localizer.t("ui.automarkers.availability.no_positions") };
    }
    if (snapshot.reason === "observed_marker_snapshot_invalid") {
      return { enabled: false, reason: localizer.t("ui.automarkers.availability.invalid_snapshot") };
    }
    return { enabled: false, reason: localizer.t("ui.automarkers.availability.waiting_exact") };
  }
  return { enabled: true, reason: localizer.t("ui.automarkers.availability.ready") };
}

function observedMarkerSnapshotKey(snapshot: ObservedMarkerSnapshot | null): string {
  return snapshot === null ? "none" : `${snapshot.revision}:${snapshot.sessionId ?? ""}:${snapshot.sceneId ?? ""}:${snapshot.mapId ?? ""}`;
}

export function observedMarkerFreshnessLabel(
  markerObservedMicros: number,
  snapshotObservedMicros: number | null,
  localizer: UiLocalizer,
): string {
  if (snapshotObservedMicros === null || markerObservedMicros > snapshotObservedMicros) {
    return localizer.t("ui.automarkers.freshness.observed", { time: formatCaptureTime(markerObservedMicros) });
  }
  const delta = snapshotObservedMicros - markerObservedMicros;
  return delta === 0
    ? localizer.t("ui.automarkers.freshness.latest", { time: formatCaptureTime(markerObservedMicros) })
    : localizer.t("ui.automarkers.freshness.before_latest", {
        time: formatCaptureTime(markerObservedMicros), delta: formatCaptureTime(delta),
      });
}

function formatCaptureTime(micros: number): string {
  return `${(micros / 1_000_000).toFixed(3)}s`;
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
