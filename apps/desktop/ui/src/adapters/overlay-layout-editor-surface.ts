import type { MountedSurface } from "../shell/types";
import { LocalHostHttpError } from "../shell/local-host-http";
import { activeOverlaySetup, OVERLAY_MODULE_IDS, safeDefaultOverlayLayout, type OverlayLayoutSettings, type OverlayModuleId } from "./overlay-layout";

export interface OverlayLayoutEditorDependencies {
  load(): Promise<OverlayLayoutSettings>;
  save(settings: OverlayLayoutSettings): Promise<OverlayLayoutSettings>;
  showOverlay(): Promise<void>;
  editOverlay(): Promise<void>;
  hideOverlay(): Promise<void>;
}

const LABELS: Record<OverlayModuleId, string> = { map: "Mechanics Map", player: "Player", actions: "Actions",
  party: "Party", target: "Target", objectives: "Objectives", alerts: "Alerts" };

export function mountOverlayLayoutEditorSurface(container: HTMLElement, dependencies: OverlayLayoutEditorDependencies): MountedSurface {
  let alive = true; let settings: OverlayLayoutSettings | null = null; let saving = false;
  let pendingEdits: Array<(value: OverlayLayoutSettings) => void> = [];
  let persistInFlight: Promise<void> | null = null;
  let pollTimer: number | null = null;
  let errorMessage: string | null = null;
  let failureStatus: number | null = null;
  const root = element("div", "plugin-surface overlay-workspace-surface overlay-layout-editor");
  const header = element("section", "content-card overlay-workspace-intro");
  const heading = element("div", "overlay-workspace-heading");
  heading.append(text("span", "LIVE LAYOUT", "eyebrow"), text("h2", "Overlay Editor"),
    text("p", "Arrange the same seven modules rendered by the game overlay. Values are saved to the selected setup.", "card-copy"));
  const status = text("span", "LOADING", "overlay-menu-preview-badge"); header.append(heading, status);
  const controls = element("section", "content-card overlay-layout-editor-controls");
  const setupSelect = document.createElement("select");
  const lock = input("checkbox"); const lockLabel = element("label"); lockLabel.append(lock, document.createTextNode(" Click-through play mode"));
  const show = text("button", "Show overlays", "primary-button"); show.type = "button";
  const editLayout = text("button", "Edit layout", "quiet-button"); editLayout.type = "button";
  const hide = text("button", "Hide overlays", "quiet-button"); hide.type = "button";
  const reset = text("button", "Reset safe layout", "quiet-button"); reset.type = "button";
  controls.append(text("strong", "Selected setup"), setupSelect, lockLabel, show, editLayout, hide, reset);
  const grid = element("section", "overlay-layout-editor-grid"); root.append(header, controls, grid); container.replaceChildren(root);

  show.addEventListener("click", () => void runLifecycleAction("show"));
  editLayout.addEventListener("click", () => void runLifecycleAction("edit"));
  hide.addEventListener("click", () => void runLifecycleAction("hide"));
  reset.addEventListener("click", () => { if (!settings || !confirm("Reset all overlay setups to the safe default layout?")) return;
    edit((value) => {
      const canvasEnabled = value.canvasEnabled;
      Object.assign(value, safeDefaultOverlayLayout(value.revision));
      value.canvasEnabled = canvasEnabled;
    }); });
  setupSelect.addEventListener("change", () => { if (!settings || !(setupSelect.value in settings.setups)) return;
    const selected = setupSelect.value; edit((value) => { value.selectedSetupId = selected; }); });
  lock.addEventListener("change", () => { const checked = lock.checked; edit((value) => { activeOverlaySetup(value).locked = checked; }); });
  void dependencies.load().then(apply).catch(showError);
  pollTimer = window.setInterval(() => { void poll(); }, 1_000);

  function apply(value: OverlayLayoutSettings): void { if (!alive) return; settings = value; errorMessage = null; failureStatus = null; render(); }
  function render(): void {
    if (!settings) return;
    status.textContent = errorMessage !== null ? "UNAVAILABLE" : saving ? "SAVING" : "SAVED";
    status.dataset.state = errorMessage !== null ? "error" : saving ? "waiting" : "live";
    status.title = errorMessage ?? "";
    setupSelect.replaceChildren(...Object.entries(settings.setups).map(([id, setup]) => new Option(setup.name, id)));
    setupSelect.value = settings.selectedSetupId; const setup = activeOverlaySetup(settings); lock.checked = setup.locked; grid.replaceChildren();
    for (const id of OVERLAY_MODULE_IDS) {
      const module = setup.modules[id]; const card = element("article", "content-card overlay-layout-module-card");
      const visible = input("checkbox"); visible.checked = module.visible; const title = element("label"); title.append(visible, document.createTextNode(` ${LABELS[id]}`));
      visible.addEventListener("change", () => { const checked = visible.checked; edit((value) => { activeOverlaySetup(value).modules[id].visible = checked; }); }); card.append(title);
      for (const [key, label, min, max, step] of [
        ["x", "X %", 0, 100, 1], ["y", "Y %", 0, 100, 1], ["width", "Width %", 8, 100, 1], ["height", "Height %", 6, 100, 1],
        ["opacity", "Opacity %", 20, 100, 5], ["scale", "Scale %", 50, 200, 5], ["zOrder", "Layer", 0, 1000, 1],
      ] as const) {
        const field = input("number"); field.min = String(min); field.max = String(max); field.step = String(step);
        field.value = String(key === "zOrder" ? module[key] : Math.round(module[key] * 100));
        field.addEventListener("change", () => { const raw = Number(field.value); if (!Number.isFinite(raw)) return;
          edit((value) => { const target = activeOverlaySetup(value).modules[id];
            if (key === "zOrder") target.zOrder = Math.max(0, Math.min(1000, Math.round(raw)));
            else target[key] = Math.max(min, Math.min(max, raw)) / 100;
          }); });
        const row = element("label", "overlay-layout-field"); row.append(text("span", label), field); card.append(row);
      }
      grid.append(card);
    }
  }
  function edit(operation: (value: OverlayLayoutSettings) => void): void {
    if (!settings) return;
    operation(settings); pendingEdits.push(operation); errorMessage = null; failureStatus = null; render();
    if (!saving) void persist();
  }
  function persist(): Promise<void> {
    if (persistInFlight !== null) return persistInFlight;
    if (!settings || pendingEdits.length === 0) return Promise.resolve();
    persistInFlight = flushPendingEdits().finally(() => { persistInFlight = null; });
    return persistInFlight;
  }
  async function flushPendingEdits(): Promise<void> {
    if (!settings || saving || pendingEdits.length === 0) return;
    saving = true; render(); let rebased = false;
    try {
      while (alive && settings && pendingEdits.length > 0) {
        const count = pendingEdits.length;
        const snapshot = structuredClone(settings);
        try {
          const saved = await dependencies.save(snapshot);
          pendingEdits.splice(0, count); settings = saved;
          for (const operation of pendingEdits) operation(settings);
          rebased = false;
        } catch (error) {
          if (!(error instanceof LocalHostHttpError) || error.status !== 409) throw error;
          if (rebased) throw error;
          const fresh = await dependencies.load();
          for (const operation of pendingEdits) operation(fresh);
          settings = fresh; rebased = true;
        }
      }
    } catch (error) { failureStatus = error instanceof LocalHostHttpError ? error.status : null; showError(error); }
    finally { saving = false; if (alive) render(); }
  }
  async function runLifecycleAction(action: "show" | "edit" | "hide"): Promise<void> {
    if (!settings) return;
    try {
      if (action === "show" && !activeOverlaySetup(settings).locked) {
        edit((value) => { activeOverlaySetup(value).locked = true; });
      }
      await persist();
      if (pendingEdits.length > 0 || errorMessage !== null) return;
      if (action === "show") await dependencies.showOverlay();
      else if (action === "edit") await dependencies.editOverlay();
      else await dependencies.hideOverlay();
      apply(await dependencies.load());
    } catch (error) { showError(error); }
  }
  async function poll(): Promise<void> {
    if (!alive || saving) return;
    if (pendingEdits.length > 0) {
      if (failureStatus !== 500) return;
      try {
        const fresh = await dependencies.load();
        for (const operation of pendingEdits) operation(fresh);
        settings = fresh; errorMessage = null; failureStatus = null; void persist();
      } catch { /* retain the dirty draft and visible error */ }
      return;
    }
    try { const fresh = await dependencies.load(); if (fresh.revision !== settings?.revision) apply(fresh); } catch (error) { showError(error); }
  }
  function showError(error: unknown): void {
    errorMessage = error instanceof Error ? error.message : String(error);
    status.textContent = "UNAVAILABLE"; status.dataset.state = "error"; status.title = errorMessage;
  }
  return { dispose() { alive = false; if (pollTimer !== null) window.clearInterval(pollTimer); root.remove(); } };
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const node = document.createElement(tag); if (className) node.className = className; return node; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = element(tag, className); node.textContent = value; return node; }
function input(type: string): HTMLInputElement { const node = document.createElement("input"); node.type = type; return node; }
