import {
  mergeWorkspaceOrder,
  moveWorkspace,
  moveWorkspaceByOffset,
} from "./order";
import type {
  DesktopHostAdapter,
  MountedSurface,
  ShellPreferences,
  WorkspaceDescriptor,
  WorkspaceTabDescriptor,
} from "./types";

type HostView = "plugins" | "settings";

export class DesktopShell {
  readonly #root: HTMLElement;
  readonly #adapter: DesktopHostAdapter;
  #workspaces = new Map<string, WorkspaceDescriptor>();
  #order: string[] = [];
  #activeWorkspaceId: string | null = null;
  #activeTabs: Record<string, string> = {};
  #activeHostView: HostView | null = null;
  #mountedSurface: MountedSurface | null = null;
  #mountSequence = 0;
  #draggedWorkspaceId: string | null = null;

  constructor(root: HTMLElement, adapter: DesktopHostAdapter) {
    this.#root = root;
    this.#adapter = adapter;
  }

  async start(): Promise<void> {
    this.#renderFrame();
    this.#setMainNotice("Loading plug-in workspaces…", "loading");

    try {
      const [workspaces, preferences] = await Promise.all([
        this.#adapter.loadWorkspaces(),
        this.#adapter.loadPreferences(),
      ]);
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
    this.#activeWorkspaceId =
      preferences.activeWorkspaceId !== null &&
      this.#workspaces.has(preferences.activeWorkspaceId)
        ? preferences.activeWorkspaceId
        : (this.#order[0] ?? null);
    this.#activeHostView = null;
  }

  #renderFrame(): void {
    this.#root.replaceChildren();

    const shell = element("div", "desktop-shell");
    shell.innerHTML = `
      <aside class="workspace-rail" aria-label="rLogs navigation">
        <div class="brand-row">
          <div class="brand-mark" aria-hidden="true">rL</div>
          <div class="brand-copy">
            <strong>rLogs</strong>
            <span>Desktop</span>
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
        <div class="engine-state">
          <span class="engine-state-dot" aria-hidden="true"></span>
          <span>
            <strong>Core idle</strong>
            <small>No capture adapter attached</small>
          </span>
        </div>
      </aside>
      <main class="workspace-main">
        <header class="shell-topbar">
          <div class="topbar-copy">
            <span class="topbar-kicker">Modular parser host</span>
            <strong class="topbar-title">Workspace</strong>
          </div>
          <div class="topbar-actions"></div>
        </header>
        <div class="workspace-content" aria-live="polite"></div>
      </main>
    `;

    const badge = requireElement(shell, ".development-badge");
    badge.textContent = this.#adapter.modeLabel;
    this.#root.append(shell);
  }

  #render(): void {
    this.#renderNavigation();
    this.#renderTopbarActions();

    if (this.#activeHostView === "plugins") {
      this.#renderPluginManager();
      return;
    }
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
      button.draggable = true;
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
        icon.textContent = workspace.iconFallback;
      } else {
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
          ? workspace.tabs[0]?.label ?? "Workspace"
          : `${workspace.tabs.length} tabs`;
      copy.append(name, detail);

      const dragHandle = element("span", "drag-handle");
      dragHandle.textContent = "⠿";
      dragHandle.setAttribute("aria-hidden", "true");
      button.append(icon, copy, dragHandle);

      button.addEventListener("click", () => {
        this.#selectWorkspace(workspace.id);
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
      this.#hostNavigationButton("plugins", "Plug-in Manager", "＋"),
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

  #renderTopbarActions(): void {
    const actions = requireElement(this.#root, ".topbar-actions");
    actions.replaceChildren();
    if (this.#adapter.setExampleWorkspacesEnabled === undefined) {
      return;
    }

    const button = element("button", "quiet-button");
    button.type = "button";
    button.textContent =
      this.#workspaces.size === 0
        ? "Load sample plug-ins"
        : "Preview blank shell";
    button.addEventListener("click", async () => {
      button.disabled = true;
      try {
        await this.#adapter.setExampleWorkspacesEnabled?.(
          this.#workspaces.size === 0,
        );
        const [workspaces, preferences] = await Promise.all([
          this.#adapter.loadWorkspaces(),
          this.#adapter.loadPreferences(),
        ]);
        this.#applySnapshot(workspaces, preferences);
        this.#render();
      } finally {
        button.disabled = false;
      }
    });
    actions.append(button);
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
      `v${workspace.version}`,
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
    const savedId = this.#activeTabs[workspace.id];
    const savedTab = workspace.tabs.find((tab) => tab.id === savedId);
    const tab = savedTab ?? workspace.tabs[0];
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

    workspace.tabs.forEach((tab, index) => {
      const button = element("button", "workspace-tab");
      button.type = "button";
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
        this.#selectTab(workspace, tab.id);
      });
      button.addEventListener("keydown", (event) => {
        const lastIndex = workspace.tabs.length - 1;
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
        const nextTab = workspace.tabs[nextIndex];
        if (nextTab === undefined) {
          return;
        }
        this.#selectTab(workspace, nextTab.id, true);
      });
      tabList.append(button);
    });
    return tabList;
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
      const mounted = await this.#adapter.mountSurface(workspace, tab, panel);
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
      this.#activeHostView = "plugins";
      this.#render();
    });
    card.append(action);
    content.append(card);
  }

  #renderPluginManager(): void {
    this.#disposeSurface();
    const content = this.#prepareMain(
      "Plug-in Manager",
      "Install, enable, and inspect isolated features.",
      `${this.#workspaces.size} enabled`,
    );
    const panel = element("section", "host-panel");
    const heading = document.createElement("h2");
    heading.textContent =
      this.#workspaces.size === 0 ? "No plug-ins enabled" : "Enabled workspaces";
    const body = document.createElement("p");
    body.textContent =
      this.#workspaces.size === 0
        ? "The native package installer is not connected to this UI prototype yet."
        : "These development descriptors exercise the same shell boundary that installed plug-ins will use.";
    panel.append(heading, body);

    if (this.#workspaces.size > 0) {
      const list = element("div", "manager-list");
      for (const id of this.#order) {
        const workspace = this.#workspaces.get(id);
        if (workspace === undefined) {
          continue;
        }
        const row = element("div", "manager-row");
        const icon = element("span", "workspace-icon");
        icon.textContent = workspace.iconFallback;
        const copy = element("span", "manager-row-copy");
        const name = document.createElement("strong");
        name.textContent = workspace.name;
        const detail = document.createElement("small");
        detail.textContent = `${workspace.id} · v${workspace.version}`;
        copy.append(name, detail);
        const state = element("span", "state-pill");
        state.textContent = "Enabled";
        row.append(icon, copy, state);
        list.append(row);
      }
      panel.append(list);
    }
    content.append(panel);
  }

  #renderSettings(): void {
    this.#disposeSurface();
    const content = this.#prepareMain(
      "Settings",
      "Host behavior that applies across every plug-in.",
      "Core",
    );
    const panel = element("section", "host-panel settings-panel");
    const heading = document.createElement("h2");
    heading.textContent = "Desktop shell";
    const body = document.createElement("p");
    body.textContent =
      "Capture devices, storage limits, appearance, and global permissions will live here. Feature-specific settings belong in each plug-in's Options tab.";
    const rule = element("div", "settings-rule");
    const ruleCopy = element("span", "settings-rule-copy");
    const title = document.createElement("strong");
    title.textContent = "Workspace ordering";
    const detail = document.createElement("small");
    detail.textContent =
      "Drag items in the left rail, or focus one and press Alt + Up/Down Arrow.";
    ruleCopy.append(title, detail);
    const reset = element("button", "quiet-button");
    reset.type = "button";
    reset.textContent = "Reset order";
    reset.disabled = this.#workspaces.size < 2;
    reset.addEventListener("click", () => {
      this.#order = mergeWorkspaceOrder([...this.#workspaces.values()], []);
      this.#renderNavigation();
      void this.#persistPreferences();
    });
    rule.append(ruleCopy, reset);
    panel.append(heading, body, rule);
    content.append(panel);
  }

  #prepareMain(title: string, description: string, meta: string): HTMLElement {
    const content = requireElement(this.#root, ".workspace-content");
    content.replaceChildren();
    const titleElement = requireElement(this.#root, ".topbar-title");
    titleElement.textContent = title;

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
      .querySelectorAll(".workspace-button.drop-target")
      .forEach((target) => target.classList.remove("drop-target"));
  }

  async #persistPreferences(): Promise<void> {
    await this.#adapter.savePreferences({
      workspaceOrder: this.#order,
      activeWorkspaceId: this.#activeWorkspaceId,
      activeTabs: this.#activeTabs,
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

function tabId(workspaceId: string, tabIdValue: string): string {
  return `tab-${safeDomId(workspaceId)}-${safeDomId(tabIdValue)}`;
}

function panelId(workspaceId: string, tabIdValue: string): string {
  return `panel-${safeDomId(workspaceId)}-${safeDomId(tabIdValue)}`;
}
