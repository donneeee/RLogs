import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
import { LocalHostHttpError } from "../shell/local-host-http";
import { activeOverlaySetup, OVERLAY_MODULE_IDS, safeDefaultOverlayLayout, type OverlayLayoutSettings } from "./overlay-layout";

export interface OverlayLayoutEditorDependencies {
  load(): Promise<OverlayLayoutSettings>;
  save(settings: OverlayLayoutSettings): Promise<OverlayLayoutSettings>;
  showOverlay(): Promise<void>;
  editOverlay(): Promise<void>;
  hideOverlay(): Promise<void>;
}

export function mountOverlayLayoutEditorSurface(
  container: HTMLElement,
  dependencies: OverlayLayoutEditorDependencies,
  localizer: UiLocalizer,
): MountedSurface {
  let alive = true; let settings: OverlayLayoutSettings | null = null; let saving = false;
  let pendingEdits: Array<(value: OverlayLayoutSettings) => void> = [];
  let persistInFlight: Promise<void> | null = null;
  let pollTimer: number | null = null;
  let errorMessage: string | null = null;
  let failureStatus: number | null = null;
  const root = element("div", "plugin-surface overlay-workspace-surface overlay-layout-editor");
  const header = element("section", "content-card overlay-workspace-intro");
  const heading = element("div", "overlay-workspace-heading");
  heading.append(text("span", localizer.t("ui.overlay_layout.eyebrow"), "eyebrow"), text("h2", localizer.t("ui.overlay_layout.title")),
    text("p", localizer.t("ui.overlay_layout.description"), "card-copy"));
  const status = text("span", localizer.t("ui.overlay_layout.status.loading"), "overlay-menu-preview-badge"); header.append(heading, status);
  const controls = element("section", "content-card overlay-layout-editor-controls");
  const setupSelect = document.createElement("select");
  const lock = input("checkbox"); const lockLabel = element("label"); lockLabel.append(lock, document.createTextNode(` ${localizer.t("ui.overlay_layout.click_through")}`));
  const show = text("button", localizer.t("ui.overlay_layout.show"), "primary-button"); show.type = "button";
  const editLayout = text("button", localizer.t("ui.overlay_layout.edit"), "quiet-button"); editLayout.type = "button";
  const hide = text("button", localizer.t("ui.overlay_layout.hide"), "quiet-button"); hide.type = "button";
  const reset = text("button", localizer.t("ui.overlay_layout.reset"), "quiet-button"); reset.type = "button";
  controls.append(text("strong", localizer.t("ui.overlay_layout.selected_setup")), setupSelect, lockLabel, show, editLayout, hide, reset);
  const grid = element("section", "overlay-layout-editor-grid"); root.append(header, controls, grid); container.replaceChildren(root);

  show.addEventListener("click", () => void runLifecycleAction("show"));
  editLayout.addEventListener("click", () => void runLifecycleAction("edit"));
  hide.addEventListener("click", () => void runLifecycleAction("hide"));
  reset.addEventListener("click", () => { if (!settings || !confirm(localizer.t("ui.overlay_layout.reset_confirm"))) return;
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
    status.textContent = localizer.t(errorMessage !== null
      ? "ui.overlay_layout.status.unavailable"
      : saving ? "ui.overlay_layout.status.saving" : "ui.overlay_layout.status.saved");
    status.dataset.state = errorMessage !== null ? "error" : saving ? "waiting" : "live";
    status.title = errorMessage ?? "";
    setupSelect.replaceChildren(...Object.entries(settings.setups).map(([id, setup]) => new Option(setup.name, id)));
    setupSelect.value = settings.selectedSetupId; const setup = activeOverlaySetup(settings); lock.checked = setup.locked; grid.replaceChildren();
    for (const id of OVERLAY_MODULE_IDS) {
      const module = setup.modules[id]; const card = element("article", "content-card overlay-layout-module-card");
      const visible = input("checkbox"); visible.checked = module.visible; const title = element("label"); title.append(visible, document.createTextNode(` ${localizer.t(`ui.overlay_layout.module.${id}`)}`));
      visible.addEventListener("change", () => { const checked = visible.checked; edit((value) => { activeOverlaySetup(value).modules[id].visible = checked; }); }); card.append(title);
      for (const [key, labelKey, min, max, step] of [
        ["x", "x", 0, 100, 1], ["y", "y", 0, 100, 1], ["width", "width", 8, 100, 1], ["height", "height", 6, 100, 1],
        ["opacity", "opacity", 20, 100, 5], ["backgroundOpacity", "background_opacity", 0, 100, 5],
        ["scale", "scale", 50, 200, 5], ["zOrder", "layer", 0, 1000, 1],
      ] as const) {
        const field = input("number"); field.min = String(min); field.max = String(max); field.step = String(step);
        field.value = String(key === "zOrder" ? module[key] : Math.round(module[key] * 100));
        field.addEventListener("change", () => { const raw = Number(field.value); if (!Number.isFinite(raw)) return;
          edit((value) => { const target = activeOverlaySetup(value).modules[id];
            if (key === "zOrder") target.zOrder = Math.max(0, Math.min(1000, Math.round(raw)));
            else target[key] = Math.max(min, Math.min(max, raw)) / 100;
          }); });
        const row = element("label", "overlay-layout-field"); row.append(text("span", localizer.t(`ui.overlay_layout.field.${labelKey}`)), field); card.append(row);
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
    status.textContent = localizer.t("ui.overlay_layout.status.unavailable"); status.dataset.state = "error"; status.title = errorMessage;
  }
  return { dispose() { alive = false; if (pollTimer !== null) window.clearInterval(pollTimer); root.remove(); } };
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const node = document.createElement(tag); if (className) node.className = className; return node; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = element(tag, className); node.textContent = value; return node; }
function input(type: string): HTMLInputElement { const node = document.createElement("input"); node.type = type; return node; }
