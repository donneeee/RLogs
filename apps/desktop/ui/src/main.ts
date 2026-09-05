import "./styles/shell.css";

import { createDevelopmentAdapter } from "./adapters/development-adapter";
import {
  createLocalHostAdapterIfAvailable,
  mountStandaloneEventInspector,
} from "./adapters/local-host-adapter";
import { loadAndApplyThemeSettings } from "./adapters/theme-settings";
import { DesktopShell } from "./shell/desktop-shell";
import { installInterfaceZoom } from "./shell/ui-zoom";
import { dispatchCombatOverlayHide } from "./shell/combat-overlay-hide";
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
    const hideOverlayWindow = (): Promise<void> => {
      // Keep the host visibility state and the physical Tauri window in sync.
      // Do not retain/await the IPC promise here. A successful hide suspends
      // this WebView before its promise continuation is guaranteed to run; a
      // pending click guard would then make Hide inert after the next show.
      dispatchCombatOverlayHide(
        () => invoke("hide_combat_overlay"),
        () => appWindow.hide(),
        (operation, error) => console.error(`Combat Overlay ${operation} failed`, error),
      );
      return Promise.resolve();
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
      heartbeat: (consecutiveFailures) => invoke(
        "combat_overlay_heartbeat",
        { consecutiveFailures },
      ),
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
  const closeInspector = document.createElement("button");
  closeInspector.type = "button";
  closeInspector.textContent = "Close Event Inspector";
  closeInspector.setAttribute("aria-label", "Close Event Inspector window");
  closeInspector.style.cssText =
    "position:fixed;z-index:10000;top:12px;right:18px;padding:8px 12px;border:1px solid #53677f;border-radius:8px;color:#e8f1fa;background:#132235;font:600 12px/1.2 system-ui;cursor:pointer";
  closeInspector.addEventListener("click", () => {
    void getCurrentWindow().close().catch(() => window.close());
  });
  document.body.append(closeInspector);
  const catalog = await fetch("/api/plugins/catalog", { cache: "no-store" })
    .then(async (response) => response.ok ? await response.json() as unknown : null)
    .catch(() => null);
  const customTriggersAvailable =
    typeof catalog === "object" &&
    catalog !== null &&
    Array.isArray((catalog as { workspaces?: unknown }).workspaces) &&
    (catalog as { workspaces: Array<{ id?: unknown }> }).workspaces.some(
      (workspace) => workspace.id === "app.rlogs.custom-triggers",
    );
  if (customTriggersAvailable) {
    mountStandaloneEventInspector(root);
  } else {
    const unavailable = document.createElement("main");
    unavailable.style.cssText =
      "box-sizing:border-box;width:100vw;min-height:100vh;padding:28px;color:#f2f6fb;background:#0b1522;font:14px/1.5 system-ui";
    const title = document.createElement("h1");
    title.textContent = "Event Inspector is disabled";
    const detail = document.createElement("p");
    detail.textContent =
      "This unfinished feature is available only when Developer mode is enabled in rLogs Settings.";
    unavailable.append(title, detail);
    root.replaceChildren(unavailable);
  }
} else {
  await loadAndApplyThemeSettings();
  installInterfaceZoom();
  const adapter =
    (await createLocalHostAdapterIfAvailable()) ?? createDevelopmentAdapter();
  const applicationVersion = await getVersion().catch(() => "development");
  const shell = new DesktopShell(root, adapter, applicationVersion);
  void shell.start();
}
