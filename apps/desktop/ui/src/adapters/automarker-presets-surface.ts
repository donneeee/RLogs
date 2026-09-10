import type { MountedSurface } from "../shell/types";
import type {
  AutomarkerLoadResult,
  AutomarkerPresetView,
  SaveAutomarkerPresetRequest,
} from "./automarker-presets";
import { automarkerSaveRequest, newlyCreatedPresetId } from "./automarker-presets";

export interface AutomarkerPresetDependencies {
  loadPresets(): Promise<AutomarkerPresetView>;
  saveCurrent(request: SaveAutomarkerPresetRequest): Promise<AutomarkerPresetView>;
  loadPreset(presetId: string): Promise<AutomarkerLoadResult>;
}

export function mountAutomarkerPresetsSurface(
  container: HTMLElement,
  dependencies: AutomarkerPresetDependencies,
): MountedSurface {
  let alive = true;
  let view: AutomarkerPresetView | null = null;
  let selectedId: string | null = null;
  let busy = false;
  let contextRefresh: number | null = null;

  const root = el("div", "plugin-surface overlay-workspace-surface automarker-presets-surface");
  const intro = el("section", "content-card overlay-workspace-intro");
  const heading = el("div", "overlay-workspace-heading");
  heading.append(
    text("span", "AUTOMARKERS", "eyebrow"),
    text("h2", "Marker presets"),
    text("p", "Save numbered ground markers exactly where they are now, then choose a saved setup for the current scene.", "card-copy"),
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
  const controls = el("div", "automarker-controls");
  const save = button("Save", "primary-button");
  const saveAs = button("Save As…", "quiet-button");
  const load = button("Load", "primary-button");
  const refresh = button("Refresh scene", "quiet-button");
  controls.append(save, saveAs, load, refresh);
  const status = text("p", "Connecting to the local marker store…", "card-copy automarker-status");
  const detail = el("div", "automarker-preset-detail");
  card.append(nameLabel, presetLabel, controls, status, detail);
  root.append(intro, card);
  container.replaceChildren(root);

  select.addEventListener("change", () => {
    selectedId = select.value || null;
    const preset = selectedPreset();
    if (preset !== undefined) name.value = preset.name;
    render();
  });
  save.addEventListener("click", () => void persist(false));
  saveAs.addEventListener("click", () => void persist(true));
  load.addEventListener("click", () => void requestLoad());
  refresh.addEventListener("click", () => void refreshView());
  void refreshView();
  contextRefresh = window.setInterval(() => { void refreshContext(); }, 2_000);

  async function refreshContext(): Promise<void> {
    if (!alive || busy) return;
    try {
      const next = await dependencies.loadPresets();
      if (!alive || contextKey(next) === contextKey(view)) return;
      view = next;
      selectedId = view.presets[0]?.presetId ?? null;
      name.value = view.presets[0]?.name ?? "";
      status.textContent = view.context === null
        ? "Enter a scene and wait for its build/map identity before saving or selecting presets."
        : view.presets.length === 0
          ? "No marker setups are saved for this scene and map yet."
          : `${view.presets.length} compatible setup${view.presets.length === 1 ? "" : "s"} available for this scene.`;
      render();
    } catch {
      // Keep the last verified scene catalog visible. Explicit Refresh reports errors.
    }
  }

  async function refreshView(): Promise<void> {
    busy = true;
    render();
    try {
      view = await dependencies.loadPresets();
      if (!alive) return;
      if (!view.presets.some((preset) => preset.presetId === selectedId)) selectedId = view.presets[0]?.presetId ?? null;
      const preset = selectedPreset();
      if (preset !== undefined && name.value.trim() === "") name.value = preset.name;
      status.textContent = view.context === null
        ? "Enter a scene and wait for its build/map identity before saving or selecting presets."
        : view.presets.length === 0
          ? "No marker setups are saved for this scene and map yet."
          : `${view.presets.length} compatible setup${view.presets.length === 1 ? "" : "s"} available for this scene.`;
    } catch (error) {
      status.textContent = message(error);
    } finally {
      busy = false;
      if (alive) render();
    }
  }

  async function persist(saveAsNew: boolean): Promise<void> {
    if (view?.context === null || view === null) return;
    let request: SaveAutomarkerPresetRequest;
    try {
      request = automarkerSaveRequest(saveAsNew ? "save-as" : "save", selectedId, name.value);
    } catch (error) {
      status.textContent = message(error);
      return;
    }
    busy = true;
    render();
    try {
      const priorIds = new Set(view.presets.map((preset) => preset.presetId));
      view = await dependencies.saveCurrent(request);
      if (!alive) return;
      if (saveAsNew) selectedId = newlyCreatedPresetId(priorIds, view) ?? view.presets[0]?.presetId ?? null;
      status.textContent = saveAsNew ? "Saved a new marker setup on this computer." : "Updated the selected marker setup on this computer.";
    } catch (error) {
      status.textContent = message(error);
    } finally {
      busy = false;
      if (alive) render();
    }
  }

  async function requestLoad(): Promise<void> {
    if (selectedId === null || view?.nativeLoadSupported !== true) return;
    busy = true;
    render();
    try {
      const result = await dependencies.loadPreset(selectedId);
      status.textContent = result.supported ? "Marker setup loaded." : "Native placement remains locked until its outbound protocol is verified.";
    } catch (error) {
      status.textContent = message(error);
    } finally {
      busy = false;
      if (alive) render();
    }
  }

  function render(): void {
    const context = view?.context ?? null;
    sceneBadge.textContent = context === null ? "NO SCENE" : context.sceneName ?? `SCENE ${context.sceneId}`;
    sceneBadge.dataset.state = context === null ? "waiting" : "ready";
    select.replaceChildren();
    if (view === null || view.presets.length === 0) {
      const option = new Option(context === null ? "Enter a scene" : "No saved setups for this scene", "");
      select.append(option);
      selectedId = null;
    } else {
      for (const preset of view.presets) {
        const option = new Option(`${preset.name} · ${preset.points.length} marks`, preset.presetId);
        option.selected = preset.presetId === selectedId;
        select.append(option);
      }
    }
    const preset = selectedPreset();
    select.disabled = busy || context === null || view?.presets.length === 0;
    save.disabled = busy || context === null || preset === undefined || view?.captureSupported !== true;
    saveAs.disabled = busy || context === null || view?.captureSupported !== true;
    save.title = view?.captureSupported === true ? "Overwrite the selected setup with current markers" : "Unavailable until native marker observation is protocol-verified";
    saveAs.title = view?.captureSupported === true ? "Save current markers as a new setup" : "Unavailable until native marker observation is protocol-verified";
    refresh.disabled = busy;
    load.disabled = busy || preset === undefined || view?.nativeLoadSupported !== true;
    load.title = view?.nativeLoadSupported === true
      ? "Place this setup at its saved coordinates"
      : "Unavailable until native party-visible marker placement is protocol-verified";
    detail.replaceChildren();
    if (context !== null) {
      detail.append(text("p", `Build ${context.clientBuild} · Scene ${context.sceneId} · Map ${context.mapId}`, "card-copy"));
    }
    if (preset !== undefined) {
      const list = el("ol", "automarker-point-list");
      for (const point of preset.points) {
        list.append(text("li", `${point.markerNumber}: X ${format(point.x)} · Y ${format(point.y)} · Z ${format(point.z)}`));
      }
      detail.append(list);
    }
    if (view !== null && !view.nativeLoadSupported) {
      detail.append(text("p", "Load is visible but disabled: rLogs will not emit a guessed game packet.", "card-copy automarker-safety-note"));
    }
    if (view !== null && !view.captureSupported) {
      detail.append(text("p", "Save and Save As are visible but disabled: the current capture does not yet prove native waymark state.", "card-copy automarker-safety-note"));
    }
  }

  function selectedPreset() {
    return view?.presets.find((preset) => preset.presetId === selectedId);
  }

  return { dispose() { alive = false; if (contextRefresh !== null) window.clearInterval(contextRefresh); } };
}

function contextKey(view: AutomarkerPresetView | null): string {
  const context = view?.context;
  return context === null || context === undefined ? "none" : `${context.clientBuild}:${context.sceneId}:${context.mapId}`;
}

function format(value: number): string { return value.toFixed(3).replace(/\.0+$/, "").replace(/(\.\d*?)0+$/, "$1"); }
function message(error: unknown): string { return error instanceof Error ? error.message : String(error); }
function button(label: string, className: string): HTMLButtonElement { const node = text("button", label, className); node.type = "button"; return node; }
function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const node = document.createElement(tag); if (className) node.className = className; return node; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = el(tag, className); node.textContent = value; return node; }
