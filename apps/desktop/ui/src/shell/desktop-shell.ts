import {
  mergeWorkspaceOrder,
  moveWorkspace,
  moveWorkspaceByOffset,
} from "./order";
import {
  moveSection,
  moveTabInsideSection,
  orderTabSections,
  type OrderedTabSection,
} from "./tab-order";
import { wireNativeWindowChrome } from "./native-window";
import {
  readWorkspaceNavigationRequest,
  WORKSPACE_NAVIGATION_EVENT,
} from "./workspace-navigation";
import { displayVersion, releaseNotesUrl } from "./app-version";
import { invoke } from "@tauri-apps/api/core";
import type {
  DesktopHostAdapter,
  EngineState,
  MountedSurface,
  PluginCatalogSnapshot,
  SettingsTabDescriptor,
  ShellPreferences,
  WorkspaceDescriptor,
  WorkspaceTabDescriptor,
} from "./types";

type HostView = "settings";

const SETTINGS_WORKSPACE_ID = "host.rlogs.settings";
const CORE_SETTINGS_TABS: readonly SettingsTabDescriptor[] = [
  {
    id: "host.rlogs.settings:general",
    label: "General",
    kind: "options",
    entrypoint: "core://settings/general",
    contributorPluginId: SETTINGS_WORKSPACE_ID,
    sectionId: "host.rlogs.settings:core",
    defaultOrder: -300,
  },
  {
    id: "host.rlogs.settings:network",
    label: "Network",
    kind: "options",
    entrypoint: "core://settings/network",
    contributorPluginId: SETTINGS_WORKSPACE_ID,
    sectionId: "host.rlogs.settings:core",
    defaultOrder: -299,
  },
  {
    id: "host.rlogs.settings:hotkeys",
    label: "Hotkeys",
    kind: "options",
    entrypoint: "core://settings/hotkeys",
    contributorPluginId: SETTINGS_WORKSPACE_ID,
    sectionId: "host.rlogs.settings:core",
    defaultOrder: -298,
  },
  {
    id: "host.rlogs.settings:plugins",
    label: "Plug-ins",
    kind: "options",
    entrypoint: "core://settings/plugins",
    contributorPluginId: SETTINGS_WORKSPACE_ID,
    sectionId: "host.rlogs.settings:core",
    defaultOrder: -297,
  },
];

type PointerDragState =
  | {
      kind: "workspace";
      sourceId: string;
      targetId: string | null;
      pointerId: number;
      startX: number;
      startY: number;
      moved: boolean;
      sourceElement: HTMLElement;
    }
  | {
      kind: "tab";
      workspaceId: string;
      sectionId: string;
      sourceId: string;
      targetId: string | null;
      pointerId: number;
      startX: number;
      startY: number;
      moved: boolean;
      sourceElement: HTMLElement;
    }
  | {
      kind: "section";
      workspaceId: string;
      sourceId: string;
      targetId: string | null;
      pointerId: number;
      startX: number;
      startY: number;
      moved: boolean;
      sourceElement: HTMLElement;
    };

export class DesktopShell {
  readonly #root: HTMLElement;
  readonly #adapter: DesktopHostAdapter;
  readonly #applicationVersion: string;
  #workspaces = new Map<string, WorkspaceDescriptor>();
  #order: string[] = [];
  #activeWorkspaceId: string | null = null;
  #activeTabs: Record<string, string> = {};
  #tabOrders: Record<string, string[]> = {};
  #sectionOrders: Record<string, string[]> = {};
  #lockTabDragging = false;
  #lockSectionDragging = false;
  #settingsWorkspace = settingsWorkspace([]);
  #activeHostView: HostView | null = null;
  #mountedSurface: MountedSurface | null = null;
  #mountSequence = 0;
  #draggedWorkspaceId: string | null = null;
  #draggedTab:
    | { workspaceId: string; sectionId: string; tabId: string }
    | null = null;
  #draggedSection:
    | { workspaceId: string; sectionId: string }
    | null = null;
  #pointerDrag: PointerDragState | null = null;
  #suppressedTabClick: string | null = null;
  #suppressedWorkspaceClick: string | null = null;
  #pluginCatalog: PluginCatalogSnapshot | null = null;
  #engineStateTimer: number | null = null;
  #engineStateRefreshActive = false;
  #engineState: EngineState = {
    phase: "unavailable",
    label: "Connecting core",
    detail: "Waiting for the native runtime.",
  };

  constructor(
    root: HTMLElement,
    adapter: DesktopHostAdapter,
    applicationVersion: string,
  ) {
    this.#root = root;
    this.#adapter = adapter;
    this.#applicationVersion = applicationVersion;
    window.addEventListener("rlogs:layout-settings-changed", (event) => {
      const preferences = (event as CustomEvent<ShellPreferences>).detail;
      this.#order = mergeWorkspaceOrder(
        [...this.#workspaces.values()],
        preferences.workspaceOrder,
      );
      this.#activeTabs = { ...preferences.activeTabs };
      this.#activeWorkspaceId =
        preferences.activeWorkspaceId !== null &&
        this.#workspaces.has(preferences.activeWorkspaceId)
          ? preferences.activeWorkspaceId
          : (this.#order[0] ?? null);
      this.#applyLayoutPreferences(preferences);
      this.#render();
    });
    window.addEventListener("rlogs:developer-mode-changed", () => {
      void this.#reloadPluginCatalog("refresh", undefined, undefined, true).catch(
        (error: unknown) => {
          this.#setMainNotice(
            `Developer mode changed, but the workspace catalog could not refresh: ${error instanceof Error ? error.message : String(error)}`,
            "error",
          );
        },
      );
    });
    window.addEventListener(WORKSPACE_NAVIGATION_EVENT, (event) => {
      const request = readWorkspaceNavigationRequest(event);
      if (request === null) return;
      const workspace = this.#workspaces.get(request.workspaceId);
      const tab = workspace?.tabs.find(
        (candidate) => candidate.entrypoint === request.entrypoint,
      );
      if (workspace === undefined || tab === undefined) return;
      this.#activeHostView = null;
      this.#activeWorkspaceId = workspace.id;
      this.#activeTabs[workspace.id] = tab.id;
      this.#render();
      void this.#persistPreferences();
    });
  }

  async start(): Promise<void> {
    this.#renderFrame();
    this.#startEngineStatePolling();
    this.#setMainNotice("Loading plug-in workspaces…", "loading");

    try {
      const [workspaces, preferences, pluginCatalog] = await Promise.all([
        this.#adapter.loadWorkspaces(),
        this.#adapter.loadPreferences(),
        this.#adapter.loadPluginCatalog?.() ?? Promise.resolve(null),
      ]);
      this.#pluginCatalog = pluginCatalog;
      this.#settingsWorkspace = settingsWorkspace(
        pluginCatalog?.settingsTabs ?? [],
      );
      this.#applySnapshot(workspaces, preferences);
      this.#render();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      this.#setMainNotice(`The desktop shell could not start: ${message}`, "error");
    }
  }

  #applySnapshot(
    workspaces: readonly WorkspaceDescriptor[],
    preferences: ShellPreferences,
  ): void {
    this.#workspaces = new Map(
      workspaces.map((workspace) => [workspace.id, workspace]),
    );
    this.#order = mergeWorkspaceOrder(workspaces, preferences.workspaceOrder);
    this.#activeTabs = { ...preferences.activeTabs };
    this.#applyLayoutPreferences(preferences);
    this.#activeWorkspaceId =
      preferences.activeWorkspaceId !== null &&
      this.#workspaces.has(preferences.activeWorkspaceId)
        ? preferences.activeWorkspaceId
        : (this.#order[0] ?? null);
    this.#activeHostView = null;
  }

  #applyLayoutPreferences(preferences: ShellPreferences): void {
    this.#tabOrders = Object.fromEntries(
      Object.entries(preferences.tabOrders).map(([key, value]) => [
        key,
        [...value],
      ]),
    );
    this.#sectionOrders = Object.fromEntries(
      Object.entries(preferences.sectionOrders).map(([key, value]) => [
        key,
        [...value],
      ]),
    );
    this.#lockTabDragging = preferences.lockTabDragging;
    this.#lockSectionDragging = preferences.lockSectionDragging;
  }

  #renderFrame(): void {
    this.#root.replaceChildren();

    const shell = element("div", "desktop-shell");
    shell.innerHTML = `
      <header class="native-titlebar">
        <div class="native-drag-region" data-tauri-drag-region>
          <span class="native-titlebar-mark" aria-hidden="true">r/</span>
          <strong data-tauri-drag-region>rLogs</strong>
          <span data-tauri-drag-region>Modular parser host</span>
        </div>
        <div class="native-window-controls" aria-label="Window controls">
          <button type="button" data-window-action="minimize" aria-label="Minimize">—</button>
          <button type="button" data-window-action="maximize" aria-label="Maximize">□</button>
          <button type="button" class="window-close" data-window-action="close" aria-label="Close">×</button>
        </div>
      </header>
      <aside class="workspace-rail" aria-label="rLogs navigation">
        <div class="brand-row">
          <div class="brand-mark" aria-hidden="true">rL</div>
          <div class="brand-copy">
            <strong>rLogs</strong>
            <span class="brand-product-line">Desktop</span>
          </div>
          <span class="development-badge"></span>
        </div>
        <div class="rail-heading">
          <span>Workspaces</span>
          <span class="rail-heading-hint">drag to arrange</span>
        </div>
        <nav class="workspace-navigation" aria-label="Plug-in workspaces">
          <ol class="workspace-list"></ol>
        </nav>
        <div class="rail-spacer"></div>
        <div class="host-navigation" aria-label="Application tools"></div>
        <div class="engine-state" tabindex="0" aria-describedby="engine-state-diagnostics">
          <span class="engine-state-dot" aria-hidden="true"></span>
          <span class="engine-state-summary">
            <strong>Connecting core</strong>
            <small>Waiting for the native runtime.</small>
          </span>
          <div class="engine-state-diagnostics" id="engine-state-diagnostics" role="tooltip">
            <strong>Technical details</strong>
            <p>Waiting for the native runtime.</p>
          </div>
        </div>
      </aside>
      <main class="workspace-main">
        <div class="workspace-content" aria-live="polite"></div>
      </main>
    `;

    const badge = requireElement(shell, ".development-badge");
    badge.textContent = this.#adapter.modeLabel;
    badge.setAttribute(
      "title",
      "rLogs is connected to the app running on this computer.",
    );
    badge.setAttribute(
      "aria-label",
      `${this.#adapter.modeLabel}: rLogs is connected to the app running on this computer.`,
    );
    const productLine = requireElement(shell, ".brand-product-line");
    const version = element("button", "application-version");
    version.type = "button";
    version.textContent = displayVersion(this.#applicationVersion);
    version.title = `Open release notes for rLogs ${displayVersion(this.#applicationVersion)}`;
    version.setAttribute(
      "aria-label",
      `Open GitHub release notes for rLogs ${displayVersion(this.#applicationVersion)}`,
    );
    version.addEventListener("click", () => {
      void invoke("open_release_notes").catch(() => {
        window.open(
          releaseNotesUrl(this.#applicationVersion),
          "_blank",
          "noopener,noreferrer",
        );
      });
    });
    productLine.append(" · ", version);
    this.#root.append(shell);
    wireNativeWindowChrome(shell);
    this.#renderEngineState();
  }

  #startEngineStatePolling(): void {
    if (this.#engineStateTimer !== null) {
      return;
    }
    void this.#refreshEngineState();
    this.#engineStateTimer = window.setInterval(() => {
      void this.#refreshEngineState();
    }, 750);
    window.addEventListener(
      "pagehide",
      () => {
        if (this.#engineStateTimer !== null) {
          window.clearInterval(this.#engineStateTimer);
          this.#engineStateTimer = null;
        }
      },
      { once: true },
    );
  }

  async #refreshEngineState(): Promise<void> {
    if (this.#engineStateRefreshActive) {
      return;
    }
    this.#engineStateRefreshActive = true;
    try {
      this.#engineState =
        (await this.#adapter.loadEngineState?.()) ?? {
          phase: "idle",
          label: "Shell prototype",
          detail: "No native capture runtime is attached.",
        };
    } catch (error) {
      this.#engineState = {
        phase: "unavailable",
        label: "Not connected",
        detail: "rLogs cannot reach its background service.",
        technicalDetail: error instanceof Error ? error.message : String(error),
      };
    } finally {
      this.#engineStateRefreshActive = false;
      this.#renderEngineState();
    }
  }

  #renderEngineState(): void {
    const container = this.#root.querySelector<HTMLElement>(".engine-state");
    const label = container?.querySelector<HTMLElement>(
      ".engine-state-summary strong",
    );
    const detail = container?.querySelector<HTMLElement>(
      ".engine-state-summary small",
    );
    const diagnostics = container?.querySelector<HTMLElement>(
      ".engine-state-diagnostics p",
    );
    if (
      container === null ||
      label == null ||
      detail == null ||
      diagnostics == null
    ) {
      return;
    }
    container.dataset.phase = this.#engineState.phase;
    label.textContent = this.#engineState.label;
    detail.textContent = this.#engineState.detail;
    // Freeze diagnostics while they are being read. Live packet counters update
    // frequently and replacing tooltip text mid-hover makes native tooltips close.
    if (!container.matches(":hover, :focus-within")) {
      diagnostics.textContent =
        this.#engineState.technicalDetail ?? this.#engineState.detail;
    }
  }

  #render(): void {
    this.#renderNavigation();

    if (this.#activeHostView === "settings") {
      this.#renderSettings();
      return;
    }
    const workspace =
      this.#activeWorkspaceId === null
        ? undefined
        : this.#workspaces.get(this.#activeWorkspaceId);
    if (workspace === undefined) {
      this.#renderEmptyState();
      return;
    }
    this.#renderWorkspace(workspace);
  }

  #renderNavigation(): void {
    const list = requireElement<HTMLOListElement>(
      this.#root,
      ".workspace-list",
    );
    list.replaceChildren();

    for (const workspaceId of this.#order) {
      const workspace = this.#workspaces.get(workspaceId);
      if (workspace === undefined) {
        continue;
      }
      const item = element("li", "workspace-item");
      const button = element("button", "workspace-button");
      button.type = "button";
      button.draggable = false;
      button.dataset.workspaceId = workspace.id;
      button.setAttribute(
        "aria-label",
        `${workspace.name}. Drag or press Alt plus Up or Down Arrow to reorder.`,
      );
      button.title = `${workspace.name} · drag to reorder`;
      if (
        this.#activeHostView === null &&
        workspace.id === this.#activeWorkspaceId
      ) {
        button.classList.add("active");
        button.setAttribute("aria-current", "page");
      }

      const icon = element("span", "workspace-icon");
      icon.setAttribute("aria-hidden", "true");
      if (workspace.iconUrl === null) {
        icon.dataset.state = "fallback";
        icon.textContent = workspace.iconFallback;
      } else {
        icon.dataset.state = "resolved";
        const image = document.createElement("img");
        image.alt = "";
        image.src = workspace.iconUrl;
        icon.append(image);
      }

      const copy = element("span", "workspace-button-copy");
      const name = document.createElement("strong");
      name.textContent = workspace.name;
      const detail = document.createElement("small");
      detail.textContent =
        workspace.tabs.length === 1
          ? `v${workspace.version} · ${workspace.tabs[0]?.label ?? "Workspace"}`
          : `v${workspace.version} · ${workspace.tabs.length} tabs`;
      copy.append(name, detail);

      const dragHandle = element("span", "drag-handle");
      dragHandle.textContent = "⠿";
      dragHandle.setAttribute("aria-hidden", "true");
      button.append(icon, copy, dragHandle);

      button.addEventListener("click", () => {
        if (this.#suppressedWorkspaceClick === workspace.id) {
          this.#suppressedWorkspaceClick = null;
          return;
        }
        this.#selectWorkspace(workspace.id);
      });
      button.addEventListener("pointerdown", (event) => {
        if (event.button !== 0) return;
        this.#beginPointerDrag(
          {
            kind: "workspace",
            sourceId: workspace.id,
          },
          button,
          event,
        );
      });
      button.addEventListener("pointermove", (event) => {
        this.#updatePointerDrag(event);
      });
      button.addEventListener("pointerup", (event) => {
        this.#finishPointerDrag(null, event);
      });
      button.addEventListener("pointercancel", () => {
        this.#cancelPointerDrag();
      });
      button.addEventListener("keydown", (event) => {
        if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) {
          return;
        }
        event.preventDefault();
        const offset = event.key === "ArrowUp" ? -1 : 1;
        this.#order = moveWorkspaceByOffset(
          this.#order,
          workspace.id,
          offset,
        );
        this.#renderNavigation();
        void this.#persistPreferences();
        requestAnimationFrame(() => {
          const moved = this.#root.querySelector<HTMLElement>(
            `[data-workspace-id="${CSS.escape(workspace.id)}"]`,
          );
          moved?.focus();
        });
      });
      button.addEventListener("dragstart", (event) => {
        this.#draggedWorkspaceId = workspace.id;
        button.classList.add("dragging");
        event.dataTransfer?.setData("text/plain", workspace.id);
        if (event.dataTransfer !== null) {
          event.dataTransfer.effectAllowed = "move";
        }
      });
      button.addEventListener("dragend", () => {
        this.#draggedWorkspaceId = null;
        button.classList.remove("dragging");
        this.#clearDropTargets();
      });
      button.addEventListener("dragover", (event) => {
        if (
          this.#draggedWorkspaceId === null ||
          this.#draggedWorkspaceId === workspace.id
        ) {
          return;
        }
        event.preventDefault();
        if (event.dataTransfer !== null) {
          event.dataTransfer.dropEffect = "move";
        }
        this.#clearDropTargets();
        button.classList.add("drop-target");
      });
      button.addEventListener("drop", (event) => {
        event.preventDefault();
        const sourceId =
          this.#draggedWorkspaceId ??
          event.dataTransfer?.getData("text/plain") ??
          "";
        this.#clearDropTargets();
        if (sourceId === "") {
          return;
        }
        this.#order = moveWorkspace(this.#order, sourceId, workspace.id);
        this.#draggedWorkspaceId = null;
        this.#renderNavigation();
        void this.#persistPreferences();
      });

      item.append(button);
      list.append(item);
    }

    const hostNavigation = requireElement(
      this.#root,
      ".host-navigation",
    );
    hostNavigation.replaceChildren(
      this.#hostNavigationButton("settings", "Settings", "⚙"),
    );
  }

  #hostNavigationButton(
    view: HostView,
    label: string,
    iconText: string,
  ): HTMLButtonElement {
    const button = element("button", "host-navigation-button");
    button.type = "button";
    button.classList.toggle("active", this.#activeHostView === view);
    const icon = element("span", "host-navigation-icon");
    icon.textContent = iconText;
    icon.setAttribute("aria-hidden", "true");
    const text = document.createElement("span");
    text.textContent = label;
    button.append(icon, text);
    button.addEventListener("click", () => {
      this.#activeHostView = view;
      this.#render();
    });
    return button;
  }

  #selectWorkspace(workspaceId: string): void {
    if (!this.#workspaces.has(workspaceId)) {
      return;
    }
    this.#activeHostView = null;
    this.#activeWorkspaceId = workspaceId;
    this.#render();
    void this.#persistPreferences();
  }

  #renderWorkspace(workspace: WorkspaceDescriptor): void {
    const content = this.#prepareMain(
      workspace.name,
      workspace.description,
      workspace.id === SETTINGS_WORKSPACE_ID
        ? "Host settings"
        : `Plug-in v${workspace.version}`,
    );
    const activeTab = this.#resolveActiveTab(workspace);

    if (workspace.tabs.length > 1) {
      content.append(this.#buildTabList(workspace, activeTab));
    }

    const panel = element("section", "tab-panel");
    panel.id = panelId(workspace.id, activeTab.id);
    panel.setAttribute("role", "tabpanel");
    panel.tabIndex = 0;
    if (workspace.tabs.length > 1) {
      panel.setAttribute(
        "aria-labelledby",
        tabId(workspace.id, activeTab.id),
      );
    } else {
      panel.setAttribute("aria-label", activeTab.label);
      panel.classList.add("single-surface");
    }
    content.append(panel);
    void this.#mountSurface(workspace, activeTab, panel);
  }

  #resolveActiveTab(workspace: WorkspaceDescriptor): WorkspaceTabDescriptor {
    const orderedTabs = this.#orderedSections(workspace).flatMap(
      (section) => section.tabs,
    );
    const savedId = this.#activeTabs[workspace.id];
    const savedTab = orderedTabs.find((tab) => tab.id === savedId);
    const tab = savedTab ?? orderedTabs[0];
    if (tab === undefined) {
      throw new Error(`Workspace ${workspace.id} has no validated tabs`);
    }
    this.#activeTabs[workspace.id] = tab.id;
    return tab;
  }

  #buildTabList(
    workspace: WorkspaceDescriptor,
    activeTab: WorkspaceTabDescriptor,
  ): HTMLElement {
    const tabList = element("div", "workspace-tabs");
    tabList.setAttribute("role", "tablist");
    tabList.setAttribute("aria-label", `${workspace.name} sections`);
    const sections = this.#orderedSections(workspace);
    const orderedTabs = sections.flatMap((section) => section.tabs);

    sections.forEach((section, sectionIndex) => {
      if (sectionIndex > 0) {
        const separator = element("span", "tab-section-separator");
        separator.textContent = "|";
        separator.setAttribute("aria-hidden", "true");
        tabList.append(separator);
      }

      const sectionGroup = element("div", "tab-section");
      sectionGroup.dataset.workspaceId = workspace.id;
      sectionGroup.dataset.sectionId = section.id;
      if (sections.length > 1) {
        const sectionHandle = element("button", "section-drag-handle");
        sectionHandle.type = "button";
        sectionHandle.draggable = false;
        sectionHandle.disabled = this.#lockSectionDragging;
        sectionHandle.textContent = "⠿";
        sectionHandle.title = this.#lockSectionDragging
          ? "Section dragging is locked in Settings"
          : "Drag this whole tab section";
        sectionHandle.setAttribute(
          "aria-label",
          this.#lockSectionDragging
            ? "Section dragging is locked"
            : `Drag ${section.tabs.map((tab) => tab.label).join(", ")} as one section`,
        );
        sectionHandle.addEventListener("pointerdown", (event) => {
          if (this.#lockSectionDragging || event.button !== 0) return;
          this.#beginPointerDrag(
            {
              kind: "section",
              workspaceId: workspace.id,
              sourceId: section.id,
            },
            sectionHandle,
            event,
          );
        });
        sectionHandle.addEventListener("pointermove", (event) => {
          this.#updatePointerDrag(event);
        });
        sectionHandle.addEventListener("pointerup", (event) => {
          this.#finishPointerDrag(workspace, event);
        });
        sectionHandle.addEventListener("pointercancel", () => {
          this.#cancelPointerDrag();
        });
        sectionHandle.addEventListener("dragstart", (event) => {
          if (this.#lockSectionDragging) {
            event.preventDefault();
            return;
          }
          this.#draggedSection = {
            workspaceId: workspace.id,
            sectionId: section.id,
          };
          sectionGroup.classList.add("dragging-section");
          event.dataTransfer?.setData(
            "application/x-rlogs-section",
            section.id,
          );
          if (event.dataTransfer !== null) {
            event.dataTransfer.effectAllowed = "move";
          }
        });
        sectionHandle.addEventListener("dragend", () => {
          this.#draggedSection = null;
          sectionGroup.classList.remove("dragging-section");
          this.#clearDropTargets();
        });
        sectionGroup.append(sectionHandle);
      }

      sectionGroup.addEventListener("dragover", (event) => {
        if (
          this.#lockSectionDragging ||
          this.#draggedSection === null ||
          this.#draggedSection.workspaceId !== workspace.id ||
          this.#draggedSection.sectionId === section.id
        ) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        this.#clearDropTargets();
        sectionGroup.classList.add("section-drop-target");
        if (event.dataTransfer !== null) {
          event.dataTransfer.dropEffect = "move";
        }
      });
      sectionGroup.addEventListener("drop", (event) => {
        if (
          this.#lockSectionDragging ||
          this.#draggedSection === null ||
          this.#draggedSection.workspaceId !== workspace.id
        ) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        this.#sectionOrders[workspace.id] = moveSection(
          sections,
          this.#draggedSection.sectionId,
          section.id,
        );
        this.#draggedSection = null;
        this.#clearDropTargets();
        this.#renderWorkspace(workspace);
        void this.#persistPreferences();
      });

      section.tabs.forEach((tab) => {
        const button = element("button", "workspace-tab");
        button.type = "button";
        button.draggable = false;
        button.dataset.workspaceId = workspace.id;
        button.dataset.tabId = tab.id;
        button.dataset.sectionId = section.id;
        button.classList.toggle("drag-locked", this.#lockTabDragging);
        button.title = this.#lockTabDragging
          ? `${tab.label} · tab dragging is locked in Settings`
          : `${tab.label} · drag left or right inside this section`;
      button.id = tabId(workspace.id, tab.id);
      button.setAttribute("role", "tab");
      button.setAttribute(
        "aria-selected",
        tab.id === activeTab.id ? "true" : "false",
      );
      button.setAttribute("aria-controls", panelId(workspace.id, tab.id));
      button.tabIndex = tab.id === activeTab.id ? 0 : -1;
      if (tab.kind === "options") {
        button.classList.add("options-tab");
      }
      const label = document.createElement("span");
      label.textContent = tab.label;
      button.append(label);
      if (tab.contributorPluginId !== workspace.id) {
        const extensionMark = element("span", "extension-mark");
        extensionMark.textContent = "ADD-ON";
        extensionMark.title = `Contributed by ${tab.contributorPluginId}`;
        button.append(extensionMark);
      }
      button.addEventListener("click", () => {
        if (this.#suppressedTabClick === tab.id) {
          this.#suppressedTabClick = null;
          return;
        }
        this.#selectTab(workspace, tab.id);
      });
      button.addEventListener("pointerdown", (event) => {
        if (this.#lockTabDragging || event.button !== 0) return;
        this.#beginPointerDrag(
          {
            kind: "tab",
            workspaceId: workspace.id,
            sectionId: section.id,
            sourceId: tab.id,
          },
          button,
          event,
        );
      });
      button.addEventListener("pointermove", (event) => {
        this.#updatePointerDrag(event);
      });
      button.addEventListener("pointerup", (event) => {
        this.#finishPointerDrag(workspace, event);
      });
      button.addEventListener("pointercancel", () => {
        this.#cancelPointerDrag();
      });
      button.addEventListener("keydown", (event) => {
        const index = orderedTabs.findIndex((candidate) => candidate.id === tab.id);
        const lastIndex = orderedTabs.length - 1;
        let nextIndex: number | null = null;
        switch (event.key) {
          case "ArrowRight":
            nextIndex = index === lastIndex ? 0 : index + 1;
            break;
          case "ArrowLeft":
            nextIndex = index === 0 ? lastIndex : index - 1;
            break;
          case "Home":
            nextIndex = 0;
            break;
          case "End":
            nextIndex = lastIndex;
            break;
        }
        if (nextIndex === null) {
          return;
        }
        event.preventDefault();
        const nextTab = orderedTabs[nextIndex];
        if (nextTab === undefined) {
          return;
        }
        this.#selectTab(workspace, nextTab.id, true);
      });
      button.addEventListener("dragstart", (event) => {
        if (this.#lockTabDragging) {
          event.preventDefault();
          return;
        }
        event.stopPropagation();
        this.#draggedTab = {
          workspaceId: workspace.id,
          sectionId: section.id,
          tabId: tab.id,
        };
        button.classList.add("dragging-tab");
        event.dataTransfer?.setData("application/x-rlogs-tab", tab.id);
        if (event.dataTransfer !== null) {
          event.dataTransfer.effectAllowed = "move";
        }
      });
      button.addEventListener("dragend", (event) => {
        event.stopPropagation();
        this.#draggedTab = null;
        button.classList.remove("dragging-tab");
        this.#clearDropTargets();
      });
      button.addEventListener("dragover", (event) => {
        const dragged = this.#draggedTab;
        if (
          this.#lockTabDragging ||
          dragged === null ||
          dragged.workspaceId !== workspace.id ||
          dragged.sectionId !== section.id ||
          dragged.tabId === tab.id
        ) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        this.#clearDropTargets();
        button.classList.add("tab-drop-target");
        if (event.dataTransfer !== null) {
          event.dataTransfer.dropEffect = "move";
        }
      });
      button.addEventListener("drop", (event) => {
        const dragged = this.#draggedTab;
        if (
          this.#lockTabDragging ||
          dragged === null ||
          dragged.workspaceId !== workspace.id ||
          dragged.sectionId !== section.id
        ) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        const nextOrder = moveTabInsideSection(
          sections,
          dragged.tabId,
          tab.id,
        );
        if (nextOrder === null) {
          return;
        }
        this.#tabOrders[workspace.id] = nextOrder;
        this.#draggedTab = null;
        this.#clearDropTargets();
        this.#renderWorkspace(workspace);
        void this.#persistPreferences();
      });
      sectionGroup.append(button);
    });
      tabList.append(sectionGroup);
    });
    return tabList;
  }

  #orderedSections(workspace: WorkspaceDescriptor): OrderedTabSection[] {
    return orderTabSections(
      workspace.tabs,
      this.#tabOrders[workspace.id] ?? [],
      this.#sectionOrders[workspace.id] ?? [],
    );
  }

  #beginPointerDrag(
    details:
      | {
          kind: "workspace";
          sourceId: string;
        }
      | {
          kind: "tab";
          workspaceId: string;
          sectionId: string;
          sourceId: string;
        }
      | {
          kind: "section";
          workspaceId: string;
          sourceId: string;
        },
    sourceElement: HTMLElement,
    event: PointerEvent,
  ): void {
    this.#cancelPointerDrag();
    const common = {
      targetId: null,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      moved: false,
      sourceElement,
    };
    this.#pointerDrag =
      details.kind === "workspace"
        ? { ...details, ...common }
        : details.kind === "tab"
          ? { ...details, ...common }
          : { ...details, ...common };
    sourceElement.setPointerCapture(event.pointerId);
  }

  #updatePointerDrag(event: PointerEvent): void {
    const drag = this.#pointerDrag;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    if (
      !drag.moved &&
      Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 5
    ) {
      return;
    }
    drag.moved = true;
    event.preventDefault();
    this.#clearDropTargets();
    drag.sourceElement.classList.remove("invalid-drop");
    if (drag.kind === "workspace") {
      drag.sourceElement.classList.add("dragging");
      const target = document
        .elementFromPoint(event.clientX, event.clientY)
        ?.closest<HTMLElement>(".workspace-button");
      if (
        target?.dataset.workspaceId !== undefined &&
        target.dataset.workspaceId !== drag.sourceId
      ) {
        drag.targetId = target.dataset.workspaceId;
        target.classList.add("drop-target");
      } else {
        drag.targetId = null;
      }
      return;
    }
    if (drag.kind === "tab") {
      drag.sourceElement.classList.add("dragging-tab");
      const pointedElement = document.elementFromPoint(
        event.clientX,
        event.clientY,
      );
      const target =
        pointedElement?.closest<HTMLElement>(".workspace-tab") ?? null;
      const pointedSection =
        pointedElement?.closest<HTMLElement>(".tab-section") ?? null;
      if (
        target?.dataset.workspaceId === drag.workspaceId &&
        target.dataset.sectionId === drag.sectionId &&
        target.dataset.tabId !== undefined &&
        target.dataset.tabId !== drag.sourceId
      ) {
        drag.targetId = target.dataset.tabId;
        target.classList.add("tab-drop-target");
      } else {
        drag.targetId = null;
        if (
          pointedSection?.dataset.workspaceId === drag.workspaceId &&
          pointedSection.dataset.sectionId !== drag.sectionId
        ) {
          drag.sourceElement.classList.add("invalid-drop");
          pointedSection.classList.add("invalid-tab-drop");
        }
      }
      return;
    }

    drag.sourceElement
      .closest<HTMLElement>(".tab-section")
      ?.classList.add("dragging-section");
    const target = document
      .elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>(".tab-section");
    if (
      target?.dataset.workspaceId === drag.workspaceId &&
      target.dataset.sectionId !== undefined &&
      target.dataset.sectionId !== drag.sourceId
    ) {
      drag.targetId = target.dataset.sectionId;
      target.classList.add("section-drop-target");
    } else {
      drag.targetId = null;
    }
  }

  #finishPointerDrag(
    workspace: WorkspaceDescriptor | null,
    event: PointerEvent,
  ): void {
    const drag = this.#pointerDrag;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    let changed = false;
    let workspaceOrderChanged = false;
    if (drag.moved && drag.kind === "workspace") {
      this.#suppressedWorkspaceClick = drag.sourceId;
      window.setTimeout(() => {
        if (this.#suppressedWorkspaceClick === drag.sourceId) {
          this.#suppressedWorkspaceClick = null;
        }
      }, 0);
      if (drag.targetId !== null) {
        this.#order = moveWorkspace(
          this.#order,
          drag.sourceId,
          drag.targetId,
        );
        workspaceOrderChanged = true;
      }
    } else if (
      drag.moved &&
      drag.kind === "tab" &&
      workspace !== null
    ) {
      this.#suppressedTabClick = drag.sourceId;
      window.setTimeout(() => {
        if (this.#suppressedTabClick === drag.sourceId) {
          this.#suppressedTabClick = null;
        }
      }, 0);
      if (drag.targetId !== null) {
        const nextOrder = moveTabInsideSection(
          this.#orderedSections(workspace),
          drag.sourceId,
          drag.targetId,
        );
        if (nextOrder !== null) {
          this.#tabOrders[workspace.id] = nextOrder;
          changed = true;
        }
      }
    } else if (
      drag.moved &&
      drag.kind === "section" &&
      drag.targetId !== null &&
      workspace !== null
    ) {
      this.#sectionOrders[workspace.id] = moveSection(
        this.#orderedSections(workspace),
        drag.sourceId,
        drag.targetId,
      );
      changed = true;
    }
    this.#cancelPointerDrag();
    if (workspaceOrderChanged) {
      this.#renderNavigation();
      void this.#persistPreferences();
    } else if (changed && workspace !== null) {
      this.#renderWorkspace(workspace);
      void this.#persistPreferences();
    }
  }

  #cancelPointerDrag(): void {
    const drag = this.#pointerDrag;
    if (
      drag !== null &&
      drag.sourceElement.hasPointerCapture(drag.pointerId)
    ) {
      drag.sourceElement.releasePointerCapture(drag.pointerId);
    }
    drag?.sourceElement.classList.remove(
      "dragging",
      "dragging-tab",
      "invalid-drop",
    );
    drag?.sourceElement
      .closest<HTMLElement>(".tab-section")
      ?.classList.remove("dragging-section");
    this.#pointerDrag = null;
    this.#clearDropTargets();
  }

  #selectTab(
    workspace: WorkspaceDescriptor,
    tabIdValue: string,
    focusTab = false,
  ): void {
    if (!workspace.tabs.some((tab) => tab.id === tabIdValue)) {
      return;
    }
    this.#activeTabs[workspace.id] = tabIdValue;
    this.#renderWorkspace(workspace);
    void this.#persistPreferences();
    if (focusTab) {
      requestAnimationFrame(() => {
        document.getElementById(tabId(workspace.id, tabIdValue))?.focus();
      });
    }
  }

  async #mountSurface(
    workspace: WorkspaceDescriptor,
    tab: WorkspaceTabDescriptor,
    panel: HTMLElement,
  ): Promise<void> {
    this.#disposeSurface();
    const sequence = ++this.#mountSequence;
    panel.append(this.#surfaceLoadingState());
    try {
      const mounted =
        tab.entrypoint === "core://settings/plugins"
          ? this.#mountPluginManagerSurface(panel)
          : await this.#adapter.mountSurface(workspace, tab, panel);
      if (sequence !== this.#mountSequence) {
        mounted.dispose();
        return;
      }
      this.#mountedSurface = mounted;
    } catch (error) {
      if (sequence !== this.#mountSequence) {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      panel.replaceChildren(
        this.#messageCard(
          "Plug-in surface failed",
          message,
          "surface-error",
        ),
      );
    }
  }

  #surfaceLoadingState(): HTMLElement {
    return this.#messageCard(
      "Opening plug-in surface",
      "The host is preparing the selected tab.",
      "surface-loading",
    );
  }

  #disposeSurface(): void {
    this.#mountSequence += 1;
    this.#mountedSurface?.dispose();
    this.#mountedSurface = null;
  }

  #renderEmptyState(): void {
    this.#disposeSurface();
    const content = this.#prepareMain(
      "Your workspace is ready",
      "Enable a plug-in to add its workspace to the navigation.",
      "0 workspaces",
    );
    const card = this.#messageCard(
      "No workspace plug-ins enabled",
      "rLogs Core stays deliberately blank. Capture, game decoding, combat views, profile sync, and other features arrive as independent plug-ins.",
      "empty-state",
    );
    const action = element("button", "primary-button");
    action.type = "button";
    action.textContent = "Open Plug-in Manager";
    action.addEventListener("click", () => {
      this.#activeHostView = "settings";
      this.#activeTabs[SETTINGS_WORKSPACE_ID] =
        "host.rlogs.settings:plugins";
      this.#render();
    });
    card.append(action);
    content.append(card);
  }

  #mountPluginManagerSurface(container: HTMLElement): MountedSurface {
    const catalog = this.#pluginCatalog;
    const activeCount =
      catalog?.packages.filter((plugin) => plugin.active).length ?? 0;
    const packageCount = catalog?.packages.length ?? 0;
    const root = element("div", "plugin-surface plugin-manager-surface");
    const panel = element("section", "host-panel");
    const toolbar = element("div", "manager-toolbar");
    const toolbarCopy = element("div", "manager-toolbar-copy");
    const heading = document.createElement("h2");
    heading.textContent =
      catalog === null
        ? "Native package manager unavailable"
        : packageCount === 0
          ? "No folder packages installed"
          : "Installed packages";
    const body = document.createElement("p");
    body.textContent =
      catalog === null
        ? "Run the UI through the local Rust host to inspect installed packages."
        : `${activeCount}/${packageCount} active · Folder: ${catalog.installedRoot}`;
    toolbarCopy.append(heading, body);
    toolbar.append(toolbarCopy);
    if (this.#adapter.refreshPlugins !== undefined) {
      const refresh = element("button", "quiet-button");
      refresh.type = "button";
      refresh.textContent = "Rescan folder";
      refresh.addEventListener("click", () => {
        refresh.disabled = true;
        void this.#reloadPluginCatalog("refresh").catch((error: unknown) => {
          body.textContent =
            error instanceof Error ? error.message : String(error);
          refresh.disabled = false;
        });
      });
      toolbar.append(refresh);
    }
    panel.append(toolbar);

    if (catalog !== null && catalog.packages.length > 0) {
      const list = element("div", "manager-list");
      for (const plugin of catalog.packages) {
        const row = element("div", "manager-row");
        const icon = element("span", "workspace-icon");
        icon.textContent = iconFallback(plugin.name);
        const copy = element("div", "manager-row-copy");
        const name = document.createElement("strong");
        name.textContent = plugin.name;
        const detail = document.createElement("small");
        detail.textContent =
          `${plugin.id} · v${plugin.version} · ${formatIdentifier(plugin.runtime)}`;
        const statusDetail = document.createElement("p");
        statusDetail.textContent = plugin.statusDetail;
        const permissions = element("div", "permission-list");
        const requested = [
          ...plugin.capabilities.map(formatIdentifier),
          ...plugin.subscriptions.map(
            (subscription) => `${formatIdentifier(subscription)} events`,
          ),
        ];
        if (requested.length === 0) {
          requested.push("No capabilities");
        }
        for (const permission of requested) {
          const chip = element("span", "permission-chip");
          chip.textContent = permission;
          permissions.append(chip);
        }
        copy.append(name, detail, statusDetail, permissions);
        const actions = element("div", "manager-row-actions");
        const state = element("span", "state-pill");
        state.dataset.state = plugin.active
          ? "active"
          : plugin.enabled
            ? "blocked"
            : "disabled";
        state.textContent = plugin.active
          ? "Active"
          : plugin.enabled
            ? "Blocked"
            : "Disabled";
        actions.append(state);
        if (this.#adapter.setPluginEnabled !== undefined) {
          const toggle = element("button", "quiet-button plugin-toggle");
          toggle.type = "button";
          toggle.textContent = plugin.enabled ? "Disable" : "Enable";
          toggle.addEventListener("click", () => {
            toggle.disabled = true;
            void this.#reloadPluginCatalog(
              "enablement",
              plugin.id,
              !plugin.enabled,
            ).catch((error: unknown) => {
              statusDetail.textContent =
                error instanceof Error ? error.message : String(error);
              statusDetail.classList.add("error");
              toggle.disabled = false;
            });
          });
          actions.append(toggle);
        }
        row.append(icon, copy, actions);
        list.append(row);
      }
      panel.append(list);
    } else if (catalog !== null) {
      panel.append(
        this.#messageCard(
          "Drop one folder per plug-in",
          "Each package needs a validated plugin.toml and all declared files inside its own folder. New packages remain disabled until you review and enable them here.",
          "empty-state",
        ),
      );
    }
    root.append(panel);

    if (catalog !== null && catalog.issues.length > 0) {
      const diagnostics = element("section", "host-panel diagnostic-panel");
      const diagnosticHeading = document.createElement("h2");
      diagnosticHeading.textContent = "Diagnostics";
      const diagnosticIntro = document.createElement("p");
      diagnosticIntro.textContent =
        "Invalid and blocked packages are isolated without preventing independent plug-ins from loading.";
      const list = element("div", "diagnostic-list");
      for (const issue of catalog.issues) {
        const item = element("article", "diagnostic-item");
        const title = document.createElement("strong");
        title.textContent =
          issue.pluginId ?? issue.packagePath ?? formatIdentifier(issue.kind);
        const detail = document.createElement("p");
        detail.textContent = issue.detail;
        item.append(title, detail);
        list.append(item);
      }
      diagnostics.append(diagnosticHeading, diagnosticIntro, list);
      root.append(diagnostics);
    }
    container.replaceChildren(root);
    return {
      dispose() {
        root.remove();
      },
    };
  }

  async #reloadPluginCatalog(
    operation: "refresh" | "enablement",
    pluginId?: string,
    enabled?: boolean,
    preserveCurrentView = false,
  ): Promise<void> {
    const previousHostView = this.#activeHostView;
    const previousWorkspaceId = this.#activeWorkspaceId;
    const previousSettingsTab = this.#activeTabs[SETTINGS_WORKSPACE_ID];
    const catalog =
      operation === "refresh"
        ? await this.#adapter.refreshPlugins?.()
        : pluginId !== undefined && enabled !== undefined
          ? await this.#adapter.setPluginEnabled?.(pluginId, enabled)
          : undefined;
    if (catalog === undefined) {
      throw new Error("The local plug-in manager is unavailable.");
    }
    this.#pluginCatalog = catalog;
    this.#settingsWorkspace = settingsWorkspace(catalog.settingsTabs);
    const [workspaces, preferences] = await Promise.all([
      this.#adapter.loadWorkspaces(),
      this.#adapter.loadPreferences(),
    ]);
    this.#applySnapshot(workspaces, preferences);
    this.#settingsWorkspace = settingsWorkspace(catalog.settingsTabs);
    if (preserveCurrentView) {
      this.#activeHostView = previousHostView;
      if (
        previousWorkspaceId !== null &&
        this.#workspaces.has(previousWorkspaceId)
      ) {
        this.#activeWorkspaceId = previousWorkspaceId;
      }
      if (
        previousSettingsTab !== undefined &&
        this.#settingsWorkspace.tabs.some((tab) => tab.id === previousSettingsTab)
      ) {
        this.#activeTabs[SETTINGS_WORKSPACE_ID] = previousSettingsTab;
      }
    } else {
      this.#activeHostView = "settings";
      this.#activeTabs[SETTINGS_WORKSPACE_ID] =
        "host.rlogs.settings:plugins";
    }
    this.#render();
  }

  #renderSettings(): void {
    this.#renderWorkspace(this.#settingsWorkspace);
  }

  #prepareMain(title: string, description: string, meta: string): HTMLElement {
    const content = requireElement(this.#root, ".workspace-content");
    content.replaceChildren();

    const heading = element("section", "workspace-heading");
    const copy = element("div", "workspace-heading-copy");
    const eyebrow = document.createElement("span");
    eyebrow.textContent = "Selected workspace";
    const headingTitle = document.createElement("h1");
    headingTitle.textContent = title;
    const paragraph = document.createElement("p");
    paragraph.textContent = description;
    copy.append(eyebrow, headingTitle, paragraph);
    const badge = element("span", "workspace-meta");
    badge.textContent = meta;
    heading.append(copy, badge);
    content.append(heading);
    return content;
  }

  #messageCard(title: string, detail: string, className: string): HTMLElement {
    const card = element("div", `message-card ${className}`);
    const mark = element("div", "message-mark");
    mark.textContent = "rL";
    mark.setAttribute("aria-hidden", "true");
    const copy = element("div", "message-copy");
    const heading = document.createElement("h2");
    heading.textContent = title;
    const paragraph = document.createElement("p");
    paragraph.textContent = detail;
    copy.append(heading, paragraph);
    card.append(mark, copy);
    return card;
  }

  #setMainNotice(message: string, kind: "loading" | "error"): void {
    this.#disposeSurface();
    const content = requireElement(this.#root, ".workspace-content");
    content.replaceChildren(
      this.#messageCard(
        kind === "loading" ? "Starting rLogs" : "Unable to start",
        message,
        kind === "loading" ? "surface-loading" : "surface-error",
      ),
    );
  }

  #clearDropTargets(): void {
    this.#root
      .querySelectorAll(
        ".workspace-button.drop-target, .workspace-tab.tab-drop-target, .tab-section.section-drop-target, .tab-section.invalid-tab-drop",
      )
      .forEach((target) =>
        target.classList.remove(
          "drop-target",
          "tab-drop-target",
          "section-drop-target",
          "invalid-tab-drop",
        ),
      );
  }

  async #persistPreferences(): Promise<void> {
    await this.#adapter.savePreferences({
      schemaVersion: 1,
      workspaceOrder: this.#order,
      activeWorkspaceId: this.#activeWorkspaceId,
      activeTabs: this.#activeTabs,
      tabOrders: this.#tabOrders,
      sectionOrders: this.#sectionOrders,
      lockTabDragging: this.#lockTabDragging,
      lockSectionDragging: this.#lockSectionDragging,
    });
  }
}

function element<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tagName);
  node.className = className;
  return node;
}

function requireElement<T extends Element = HTMLElement>(
  root: ParentNode,
  selector: string,
): T {
  const node = root.querySelector<T>(selector);
  if (node === null) {
    throw new Error(`Desktop shell element is missing: ${selector}`);
  }
  return node;
}

function safeDomId(value: string): string {
  return value.replaceAll(/[^a-zA-Z0-9_-]/g, "-");
}

function formatIdentifier(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function iconFallback(name: string): string {
  const initials = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part.charAt(0))
    .join("");
  return (initials.length >= 2 ? initials : name.slice(0, 2)).toUpperCase();
}

function tabId(workspaceId: string, tabIdValue: string): string {
  return `tab-${safeDomId(workspaceId)}-${safeDomId(tabIdValue)}`;
}

function panelId(workspaceId: string, tabIdValue: string): string {
  return `panel-${safeDomId(workspaceId)}-${safeDomId(tabIdValue)}`;
}

function settingsWorkspace(
  pluginTabs: readonly SettingsTabDescriptor[],
): WorkspaceDescriptor {
  return {
    id: SETTINGS_WORKSPACE_ID,
    name: "Settings",
    description:
      "Core controls and plug-in settings, grouped by the feature that owns them.",
    version: "Core",
    iconUrl: null,
    iconFallback: "⚙",
    defaultOrder: Number.MAX_SAFE_INTEGER,
    tabs: [...CORE_SETTINGS_TABS, ...pluginTabs],
  };
}
