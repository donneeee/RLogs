import "./styles/shell.css";

import { createDevelopmentAdapter } from "./adapters/development-adapter";
import {
  createLocalHostAdapterIfAvailable,
  mountStandaloneEventInspector,
} from "./adapters/local-host-adapter";
import { loadAndApplyThemeSettings } from "./adapters/theme-settings";
import { DesktopShell } from "./shell/desktop-shell";
import { localHostJson } from "./shell/local-host-http";
import { installInterfaceZoom } from "./shell/ui-zoom";
import { dispatchCombatOverlayHide } from "./shell/combat-overlay-hide";
import { mountCombatOverlayRuntimeApp } from "../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";
import { mountMechanicsMapOverlay } from "./adapters/mechanics-map-overlay";
import { parseOverlayLayoutSettings } from "./adapters/overlay-layout";
import { parseMechanicsMapUpdate } from "./adapters/mechanics-map";
import { parseAutomarkerLoadResult, parseAutomarkerPresetView } from "./adapters/automarker-presets";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";
import { loadUiLocalizer } from "./localization/ui-locale";

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
const isOverlayCanvasRuntime =
  window.location.pathname === "/overlay-canvas-runtime" ||
  query.get("surface") === "overlay-canvas";

if (isCombatOverlayRuntime) {
  const appWindow = getCurrentWindow();
  try {
    await loadAndApplyThemeSettings();
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
    await mountCombatOverlayRuntimeApp(root, {
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
      heartbeat: (consecutiveFailures, lastSuccessfulUpdateUnixMillis) => invoke(
        "combat_overlay_heartbeat",
        { consecutiveFailures, lastSuccessfulUpdateUnixMillis },
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
    // Reveal only after the persisted layout has replaced the temporary
    // loading surface. This avoids exposing a dark default rectangle while the
    // transparent WebView is starting.
    await invoke("combat_overlay_ready");
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
} else if (isOverlayCanvasRuntime) {
  // The shared canvas is a transparent native surface. Mark both document
  // layers before applying the user's shell theme so the ordinary opaque
  // `:root` and themed body backgrounds can never become its compositor
  // backdrop.
  document.documentElement.dataset.surface = "overlay-canvas";
  document.body.dataset.surface = "overlay-canvas";
  await loadAndApplyThemeSettings();
  const appWindow = getCurrentWindow();
  try {
    const localizer = await loadUiLocalizer(navigator.languages[0] ?? navigator.language);
    let interactivityHandler: ((interactive: boolean) => void) | undefined;
    let layoutRefreshHandler: ((revision: number) => void) | undefined;
    let focusHeldHandler: ((held: boolean) => void) | undefined;
    let resolveLayoutInitialized!: () => void;
    let rejectLayoutInitialized!: (error: unknown) => void;
    const layoutInitialized = new Promise<void>((resolve, reject) => {
      resolveLayoutInitialized = resolve; rejectLayoutInitialized = reject;
    });
    // Register native listeners before acknowledging runtime readiness. A
    // recreated WebView otherwise loses the one-shot pending Edit request.
    await appWindow.listen<boolean>("overlay-canvas-interactivity", ({ payload }) => {
      interactivityHandler?.(payload);
    });
    await appWindow.listen<number>("overlay-canvas-layout-refresh-requested", ({ payload }) => {
      layoutRefreshHandler?.(payload);
    });
    await appWindow.listen<boolean>("overlay-canvas-focus-held", ({ payload }) => {
      focusHeldHandler?.(payload);
    });
    mountMechanicsMapOverlay(root, {
      loadSnapshot: async () => parseMechanicsMapUpdate(await runtimeJson("/api/runtime/live/mechanics-map")),
      waitForSnapshot: async (afterRevision) => parseMechanicsMapUpdate(await runtimeJson(
        "/api/runtime/live/mechanics-map/wait",
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ after_revision: afterRevision, timeout_millis: 30_000 }),
        },
      )),
      prepareLocalMaps: async () => { await runtimeJson("/api/runtime/local-game-assets/prepare", { method: "POST" }); },
      hideOverlay: async () => { await invoke("hide_overlay_canvas"); },
      setInteractive: async (interactive) => {
        await invoke("set_overlay_canvas_interactive", { interactive });
      },
      acknowledgeInteractivity: async (interactive) => {
        await invoke("acknowledge_overlay_canvas_interactivity", { interactive });
      },
      onLayoutInitialized: (revision, error) => {
        if (revision === undefined) { rejectLayoutInitialized(error); return; }
        void invoke("overlay_canvas_layout_initialized", { revision })
          .then(() => resolveLayoutInitialized())
          .catch(rejectLayoutInitialized);
      },
      onLayoutRefresh: async (handler) => {
        layoutRefreshHandler = handler;
        return () => { if (layoutRefreshHandler === handler) layoutRefreshHandler = undefined; };
      },
      onInteractivity: async (handler) => {
        interactivityHandler = handler;
        return () => { if (interactivityHandler === handler) interactivityHandler = undefined; };
      },
      onFocusHeld: async (handler) => {
        focusHeldHandler = handler;
        return () => { if (focusHeldHandler === handler) focusHeldHandler = undefined; };
      },
      loadAutomarkerPresets: async () => parseAutomarkerPresetView(
        await runtimeJson("/api/automarkers/presets"),
      ),
      loadAutomarkerPreset: async (presetId) => parseAutomarkerLoadResult(
        await runtimeJson("/api/automarkers/presets/load", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ presetId }),
        }),
      ),
      loadLayout: async () => parseOverlayLayoutSettings(await runtimeJson("/api/settings/overlay-layout")),
      saveLayout: async (settings) => parseOverlayLayoutSettings(await runtimeJson("/api/settings/overlay-layout", {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(settings),
      })),
    }, localizer);
    await invoke("overlay_canvas_ready");
    await layoutInitialized;
  } catch (error) {
    const failure = document.createElement("main");
    failure.className = "mechanics-map-overlay-failure";
    failure.textContent = `Map overlay could not start: ${error instanceof Error ? error.message : String(error)}`;
    root.replaceChildren(failure);
    await invoke("overlay_canvas_ready").catch(() => undefined);
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
  const localizer = await loadUiLocalizer(navigator.languages[0] ?? navigator.language);
  const adapter =
    (await createLocalHostAdapterIfAvailable(localizer)) ?? createDevelopmentAdapter();
  const applicationVersion = await getVersion().catch(() => "development");
  const shell = new DesktopShell(root, adapter, applicationVersion);
  void shell.start();
}

async function runtimeJson(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(path, { cache: "no-store", ...init });
  return localHostJson(response, `rLogs host returned HTTP ${response.status} for ${path}`);
}
