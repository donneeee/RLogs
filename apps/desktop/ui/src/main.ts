import "./styles/shell.css";

import { createDevelopmentAdapter } from "./adapters/development-adapter";
import {
  createLocalHostAdapterIfAvailable,
  mountStandaloneEventInspector,
} from "./adapters/local-host-adapter";
import { loadAndApplyThemeSettings } from "./adapters/theme-settings";
import { DesktopShell } from "./shell/desktop-shell";
import { installInterfaceZoom } from "./shell/ui-zoom";
import { mountCombatOverlayRuntimeApp } from "../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";

const root = document.querySelector<HTMLElement>("#app");
if (root === null) {
  throw new Error("rLogs desktop shell requires an #app element");
}

const query = new URLSearchParams(window.location.search);
const isCombatOverlayRuntime =
  window.location.pathname === "/combat-overlay-runtime" ||
  query.get("surface") === "combat-overlay";
const isEventInspectorRuntime =
  window.location.pathname === "/event-inspector-runtime" ||
  query.get("surface") === "event-inspector";

if (isCombatOverlayRuntime) {
  const appWindow = getCurrentWindow();
  try {
    let hideOverlayPending: Promise<void> | null = null;
    const hideOverlayWindow = (): Promise<void> => {
      if (hideOverlayPending !== null) return hideOverlayPending;
      // Keep the host visibility state and the physical Tauri window in sync.
      // Calling the window API directly is an intentional fallback: it avoids
      // leaving a visible WebView compositor surface if the host command is
      // delayed while the overlay runtime is waking or suspending.
      // The host must clear its requested-visible bit before the physical
      // window disappears. Running these in parallel lets focus restoration
      // observe the old bit and immediately show the overlay again, which
      // made Hide require a second click.
      hideOverlayPending = (async () => {
        let hostFailure: unknown = null;
        try {
          await invoke("hide_combat_overlay");
        } catch (error) {
          hostFailure = error;
        }
        // The direct window API remains a fallback even if command dispatch
        // fails, so Hide can never leave the visible overlay trapping clicks.
        await appWindow.hide();
        if (hostFailure !== null) throw hostFailure;
      })().finally(() => {
        hideOverlayPending = null;
      });
      return hideOverlayPending;
    };
    const mounted = mountCombatOverlayRuntimeApp(root, {
      // Keep the preloaded native window alive. Hiding makes the next open
      // instant and avoids reconstructing WebView2 from a button command.
      // Route user/runtime hides through the native host so its requested
      // visibility state changes atomically with the actual window.
      close: hideOverlayWindow,
      hide: hideOverlayWindow,
      // Native automatic visibility physically hides the window. The desktop
      // host observes decoded damage directly, so it can wake the window even
      // while WebView2 is hidden without leaving a compositor rectangle.
      hideTemporarily: () => appWindow.hide(),
      showIfRequested: () => invoke("show_combat_overlay_if_requested"),
      setEnabled: (enabled, automaticallyHidden) => invoke(
        "set_combat_overlay_enabled",
        { enabled, automaticallyHidden },
      ),
      setAutomaticallyHidden: (hidden) => invoke(
        "set_combat_overlay_automatically_hidden",
        { hidden },
      ),
      setAlwaysOnTop: (value) => appWindow.setAlwaysOnTop(value),
      setSize: (width, height) => appWindow.setSize(new LogicalSize(width, height)),
      setIgnoreCursorEvents: (value) => appWindow.setIgnoreCursorEvents(value),
      startDragging: () => appWindow.startDragging(),
      startResizeDragging: (direction) => appWindow.startResizeDragging(direction),
      onShowRequested: async (handler) => appWindow.listen(
        "combat-overlay-show-requested",
        handler,
      ),
      onResized: async (handler) => {
        const scaleFactor = await appWindow.scaleFactor();
        return appWindow.onResized(({ payload }) => {
          handler(payload.width / scaleFactor, payload.height / scaleFactor);
        });
      },
    });
    // mountCombatOverlayRuntimeApp paints its loading surface synchronously,
    // before its first settings request. Reveal it immediately afterward.
    // requestAnimationFrame cannot be used here: WebView2 may suspend animation
    // frames while the native window is hidden, which would leave the overlay
    // loaded but permanently invisible.
    await invoke("combat_overlay_ready");
    await mounted;
  } catch (error) {
    const failure = document.createElement("main");
    failure.style.cssText = "box-sizing:border-box;width:100vw;min-height:100vh;padding:14px;color:#f2f6fb;background:#0b1522;font:12px/1.4 system-ui";
    const title = document.createElement("strong");
    title.textContent = "Combat Overlay could not start";
    const detail = document.createElement("p");
    detail.textContent = error instanceof Error ? error.message : String(error);
    detail.style.color = "#ff9ca9";
    const close = document.createElement("button");
    close.textContent = "Close overlay";
    close.style.cssText = "padding:7px 10px;border:1px solid #53677f;border-radius:6px;color:#e8f1fa;background:#132235";
    close.addEventListener("click", () => void appWindow.close());
    failure.append(title, detail, close);
    root.replaceChildren(failure);
    // A readable in-window error is still preferable to a permanently hidden
    // native surface if initialization fails after navigation succeeds.
    await invoke("combat_overlay_ready").catch(() => undefined);
  }
} else if (isEventInspectorRuntime) {
  await loadAndApplyThemeSettings();
  installInterfaceZoom();
  document.body.dataset.surface = "event-inspector";
  mountStandaloneEventInspector(root);
} else {
  await loadAndApplyThemeSettings();
  installInterfaceZoom();
  const adapter =
    (await createLocalHostAdapterIfAvailable()) ?? createDevelopmentAdapter();
  const applicationVersion = await getVersion().catch(() => "development");
  const shell = new DesktopShell(root, adapter, applicationVersion);
  void shell.start();
}
