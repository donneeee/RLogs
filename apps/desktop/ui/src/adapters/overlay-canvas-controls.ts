import type { UiLocalizer } from "../localization/ui-locale";

export interface OverlayCanvasControlDependencies {
  moduleControls: readonly HTMLElement[];
  locked: boolean;
  toggleLock(): void | Promise<void>;
  hide(): void | Promise<void>;
  done(): void | Promise<void>;
}

export interface MountedOverlayCanvasControls {
  element: HTMLElement;
  setLocked(locked: boolean): void;
  dispose(): void;
}

/**
 * Owns controls for the shared native canvas itself. Individual overlay
 * modules supply visibility buttons, but no module owns Hide, Done, Lock, or
 * the window-level Escape gesture.
 */
export function mountOverlayCanvasControls(
  dependencies: OverlayCanvasControlDependencies,
  localizer: UiLocalizer,
): MountedOverlayCanvasControls {
  let locked = dependencies.locked;
  let exitPending = false;
  const bar = element("header", "overlay-canvas-editor-bar");
  bar.setAttribute("aria-label", localizer.t("ui.mechanics_map.canvas_editor.aria"));
  const identity = element("div", "overlay-canvas-editor-identity");
  identity.append(
    text("strong", localizer.t("ui.mechanics_map.canvas_editor.label")),
    text("span", localizer.t("ui.mechanics_map.canvas_editor.mode")),
  );
  const actions = element("div", "overlay-canvas-editor-actions");
  const lock = button("", () => { void invoke(dependencies.toggleLock, "toggle canvas lock"); });
  lock.title = localizer.t("ui.mechanics_map.canvas_editor.lock_help");
  const hide = button(localizer.t("ui.mechanics_map.canvas_editor.hide"), () => {
    void invoke(dependencies.hide, "hide overlay canvas");
  });
  hide.title = localizer.t("ui.mechanics_map.canvas_editor.hide_help");
  const done = button(localizer.t("ui.mechanics_map.canvas_editor.done"), requestDone);
  done.title = localizer.t("ui.mechanics_map.canvas_editor.done_help");
  actions.append(...dependencies.moduleControls, lock, hide, done);
  bar.append(identity, actions);

  const setLocked = (value: boolean): void => {
    locked = value;
    lock.textContent = localizer.t(value
      ? "ui.mechanics_map.canvas_editor.unlock"
      : "ui.mechanics_map.canvas_editor.lock");
    lock.dataset.active = String(value);
    lock.setAttribute("aria-pressed", String(value));
  };
  setLocked(locked);

  function requestDone(): void {
    if (locked || exitPending) return;
    exitPending = true;
    let result: void | Promise<void>;
    try { result = dependencies.done(); }
    catch (error) {
      console.error("Could not exit overlay canvas editing", error);
      exitPending = false;
      return;
    }
    void Promise.resolve(result)
      .catch((error: unknown) => console.error("Could not exit overlay canvas editing", error))
      .finally(() => { exitPending = false; });
  }

  const handleEscape = (event: KeyboardEvent): void => {
    if (event.key !== "Escape" || event.defaultPrevented || event.isComposing || locked || exitPending) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("input, textarea, select, [contenteditable]:not([contenteditable='false']), [role='textbox'], [role='combobox'], [role='listbox']")) return;
    event.preventDefault();
    event.stopPropagation();
    requestDone();
  };
  window.addEventListener("keydown", handleEscape);

  return {
    element: bar,
    setLocked,
    dispose() { window.removeEventListener("keydown", handleEscape); },
  };
}

async function invoke(action: () => void | Promise<void>, label: string): Promise<void> {
  try { await action(); }
  catch (error) { console.error(`Could not ${label}`, error); }
}

function button(label: string, action: () => void): HTMLButtonElement {
  const value = text("button", label);
  value.type = "button";
  value.dataset.active = "false";
  value.addEventListener("click", action);
  return value;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const value = document.createElement(tag);
  if (className) value.className = className;
  return value;
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = value;
  return node;
}
