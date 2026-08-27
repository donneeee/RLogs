import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface CoreWindowSettings {
  schemaVersion: 1;
  closeToTray: boolean;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function wireNativeWindowChrome(root: ParentNode): void {
  const titlebar = root.querySelector<HTMLElement>(".native-titlebar");
  if (titlebar === null) return;

  if (window.__TAURI_INTERNALS__ === undefined) {
    titlebar.dataset.runtime = "browser-preview";
    root
      .querySelectorAll<HTMLButtonElement>("[data-window-action]")
      .forEach((button) => {
        button.disabled = true;
        button.title = "Available in the native rLogs application";
      });
    return;
  }

  titlebar.dataset.runtime = "native";
  const appWindow = getCurrentWindow();
  let closeRequestInFlight = false;
  const requestApplicationClose = async (): Promise<void> => {
    if (closeRequestInFlight) return;
    closeRequestInFlight = true;

    try {
      if (await closeToTrayEnabled()) {
        await appWindow.hide();
        closeRequestInFlight = false;
        return;
      }
      await invoke("quit_rlogs");
    } catch (error) {
      closeRequestInFlight = false;
      console.error("rLogs could not complete the requested window close", error);
    }
  };
  const dragRegion = root.querySelector<HTMLElement>(".native-drag-region");
  let pendingDrag: { clientX: number; clientY: number } | null = null;
  dragRegion?.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    pendingDrag = { clientX: event.clientX, clientY: event.clientY };
  });
  dragRegion?.addEventListener("mousemove", (event) => {
    if (pendingDrag === null) return;
    if ((event.buttons & 1) === 0) {
      pendingDrag = null;
      return;
    }
    const moved = Math.hypot(
      event.clientX - pendingDrag.clientX,
      event.clientY - pendingDrag.clientY,
    );
    if (moved < 5) return;
    pendingDrag = null;
    void appWindow.startDragging();
  });
  dragRegion?.addEventListener("dblclick", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    pendingDrag = null;
    void appWindow.toggleMaximize();
  });
  window.addEventListener("mouseup", () => {
    pendingDrag = null;
  });
  root
    .querySelectorAll<HTMLButtonElement>("[data-window-action]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const action = button.dataset.windowAction;
        switch (action) {
          case "minimize":
            void appWindow.minimize();
            break;
          case "maximize":
            void appWindow.toggleMaximize();
            break;
          case "close":
            void requestApplicationClose();
            break;
        }
      });
    });

  void appWindow.onCloseRequested((event) => {
    event.preventDefault();
    void requestApplicationClose();
  });
}

async function closeToTrayEnabled(): Promise<boolean> {
  try {
    const response = await fetch("/api/settings/core", {
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) return false;
    const value: unknown = await response.json();
    return isCoreWindowSettings(value) && value.closeToTray;
  } catch {
    return false;
  }
}

function isCoreWindowSettings(value: unknown): value is CoreWindowSettings {
  return (
    typeof value === "object" &&
    value !== null &&
    "schemaVersion" in value &&
    value.schemaVersion === 1 &&
    "closeToTray" in value &&
    typeof value.closeToTray === "boolean"
  );
}
