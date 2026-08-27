import { invoke } from "@tauri-apps/api/core";

export const COMBAT_OVERLAY_TOGGLE_ACTION_ID =
  "app.rlogs.combat-overlay.toggle-visibility";

export interface HotkeyActionDefinition {
  actionId: string;
  label: string;
  description: string;
  category: string;
}

export interface HotkeySettingsView {
  schemaVersion: 1;
  actions: readonly HotkeyActionDefinition[];
  bindings: Readonly<Record<string, string>>;
}

interface HotkeyAssignmentResult {
  settings: HotkeySettingsView;
  displacedActionId: string | null;
}

export interface MountedHotkeyBinding {
  element: HTMLElement;
  dispose(): void;
}

const HOTKEY_CHANGE_EVENT = "rlogs:hotkeys-changed";

export async function loadHotkeySettings(): Promise<HotkeySettingsView> {
  return parseHotkeySettings(await apiJson<unknown>("/api/settings/hotkeys"));
}

export function mountHotkeyBinding(
  actionId: string,
  options: { compact?: boolean } = {},
): MountedHotkeyBinding {
  let alive = true;
  let settings: HotkeySettingsView | null = null;
  let capturing = false;
  const root = document.createElement("section");
  root.className = options.compact
    ? "hotkey-binding hotkey-binding-compact"
    : "hotkey-binding";
  const copy = document.createElement("div");
  copy.className = "hotkey-binding-copy";
  const label = document.createElement("strong");
  label.textContent = "Loading hotkey...";
  const description = document.createElement("small");
  copy.append(label, description);
  const controls = document.createElement("div");
  controls.className = "hotkey-binding-controls";
  const capture = document.createElement("button");
  capture.type = "button";
  capture.className = "hotkey-capture-button";
  capture.textContent = "Loading...";
  capture.disabled = true;
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "hotkey-clear-button";
  clear.textContent = "Clear";
  clear.disabled = true;
  controls.append(capture, clear);
  const message = document.createElement("small");
  message.className = "hotkey-binding-message";
  root.append(copy, controls, message);

  const action = () => settings?.actions.find((candidate) => candidate.actionId === actionId);
  const render = () => {
    const definition = action();
    label.textContent = definition?.label ?? actionId;
    description.textContent = definition?.description ?? "This hotkey action is unavailable.";
    const shortcut = settings?.bindings[actionId] ?? null;
    capture.textContent = capturing
      ? "Press a shortcut..."
      : shortcut === null
        ? "Not assigned"
        : displayShortcut(shortcut);
    capture.disabled = settings === null || definition === undefined;
    clear.disabled = shortcut === null;
    capture.classList.toggle("is-capturing", capturing);
  };

  const setMessage = (value: string, error = false) => {
    message.textContent = value;
    message.classList.toggle("error", error);
  };

  const apply = (next: HotkeySettingsView) => {
    settings = next;
    render();
  };

  const reload = () => {
    void loadHotkeySettings().then((next) => {
      if (alive) apply(next);
    }).catch((error: unknown) => {
      if (!alive) return;
      setMessage(errorMessage(error), true);
    });
  };

  const assign = async (shortcut: string | null) => {
    capture.disabled = true;
    clear.disabled = true;
    setMessage(shortcut === null ? "Clearing shortcut..." : "Registering with the operating system...");
    try {
      const result = await assignHotkey(actionId, shortcut);
      if (!alive) return;
      apply(result.settings);
      const displaced = result.displacedActionId === null
        ? null
        : result.settings.actions.find(
            (candidate) => candidate.actionId === result.displacedActionId,
          );
      setMessage(displaced
        ? `${displayShortcut(shortcut ?? "")} moved here. ${displaced.label} was cleared to prevent a conflict.`
        : shortcut === null
          ? "Shortcut cleared."
          : `${displayShortcut(shortcut)} is active.`);
      window.dispatchEvent(new CustomEvent<HotkeySettingsView>(HOTKEY_CHANGE_EVENT, {
        detail: result.settings,
      }));
    } catch (error) {
      if (!alive) return;
      setMessage(errorMessage(error), true);
      reload();
    }
  };

  const stopCapture = () => {
    capturing = false;
    window.removeEventListener("keydown", onKeyDown, true);
    render();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (!capturing) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.key === "Escape") {
      stopCapture();
      setMessage("Shortcut change canceled.");
      return;
    }
    if (event.key === "Backspace" || event.key === "Delete") {
      stopCapture();
      void assign(null);
      return;
    }
    const shortcut = shortcutFromKeyboardEvent(event);
    if (shortcut === null) {
      setMessage("Keep holding the modifier, then press a letter, number, function key, or navigation key.");
      return;
    }
    stopCapture();
    void assign(shortcut);
  };

  capture.addEventListener("click", () => {
    if (capturing) {
      stopCapture();
      setMessage("Shortcut change canceled.");
      return;
    }
    capturing = true;
    setMessage("Press the new shortcut. Esc cancels; Backspace or Delete clears it.");
    window.addEventListener("keydown", onKeyDown, true);
    render();
  });
  clear.addEventListener("click", () => void assign(null));
  const onChanged = (event: Event) => {
    const next = (event as CustomEvent<HotkeySettingsView>).detail;
    if (next) apply(next);
    else reload();
  };
  window.addEventListener(HOTKEY_CHANGE_EVENT, onChanged);
  reload();

  return {
    element: root,
    dispose() {
      alive = false;
      stopCapture();
      window.removeEventListener(HOTKEY_CHANGE_EVENT, onChanged);
      root.remove();
    },
  };
}

export function shortcutFromKeyboardEvent(event: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return null;
  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");
  const key = event.code || event.key;
  if (!key || key === "Unidentified") return null;
  const standalone = /^F(?:[1-9]|1[0-9]|2[0-4])$/.test(key)
    || ["Pause", "PrintScreen", "ScrollLock"].includes(key);
  if (modifiers.length === 0 && !standalone) return null;
  return [...modifiers, key].join("+");
}

export function displayShortcut(shortcut: string): string {
  return shortcut.split("+").map((part) => {
    const lower = part.toLowerCase();
    if (lower === "control" || lower === "ctrl") return "Ctrl";
    if (lower === "shift") return "Shift";
    if (lower === "alt" || lower === "option") return "Alt";
    if (lower === "super" || lower === "meta" || lower === "command") return "Win";
    return part
      .replace(/^Key([A-Z])$/, "$1")
      .replace(/^Digit([0-9])$/, "$1")
      .replace(/^Numpad([0-9])$/, "Num $1")
      .replace(/^Arrow(Up|Down|Left|Right)$/, "$1");
  }).join("+");
}

export function parseHotkeySettings(value: unknown): HotkeySettingsView {
  if (!isRecord(value) || value.schemaVersion !== 1 || !Array.isArray(value.actions)
    || !isRecord(value.bindings)) {
    throw new Error("The native host returned invalid Hotkey settings.");
  }
  const actions = value.actions.map((entry) => {
    if (!isRecord(entry) || typeof entry.actionId !== "string"
      || typeof entry.label !== "string" || typeof entry.description !== "string"
      || typeof entry.category !== "string") {
      throw new Error("The native host returned an invalid Hotkey action.");
    }
    return {
      actionId: entry.actionId,
      label: entry.label,
      description: entry.description,
      category: entry.category,
    };
  });
  const bindings: Record<string, string> = {};
  for (const [actionId, shortcut] of Object.entries(value.bindings)) {
    if (typeof shortcut !== "string") {
      throw new Error("The native host returned an invalid Hotkey binding.");
    }
    bindings[actionId] = shortcut;
  }
  return { schemaVersion: 1, actions, bindings };
}

async function assignHotkey(
  actionId: string,
  shortcut: string | null,
): Promise<HotkeyAssignmentResult> {
  const assignment = { actionId, shortcut };
  const value = isTauriRuntime()
    ? await invoke<unknown>("assign_hotkey", { assignment })
    : await apiJson<unknown>("/api/settings/hotkeys/assign", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(assignment),
      });
  if (!isRecord(value)) throw new Error("The native host returned an invalid Hotkey result.");
  return {
    settings: parseHotkeySettings(value.settings),
    displacedActionId: typeof value.displacedActionId === "string"
      ? value.displacedActionId
      : null,
  };
}

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

async function apiJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const body = await response.json() as { error?: unknown };
      if (typeof body.error === "string") message = body.error;
    } catch {
      // Preserve the HTTP status when the response is not JSON.
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
