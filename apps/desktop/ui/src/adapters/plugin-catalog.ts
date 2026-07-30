import type {
  InstalledPluginDescriptor,
  PluginCatalogSnapshot,
  PluginIssueDescriptor,
  WorkspaceDescriptor,
} from "../shell/types";

export interface LocalPluginCatalog extends PluginCatalogSnapshot {
  workspaces: readonly WorkspaceDescriptor[];
}

export function parsePluginCatalog(value: unknown): LocalPluginCatalog {
  if (!isRecord(value) || value.schemaVersion !== 1) {
    throw new Error("The local host returned an unsupported plug-in catalog.");
  }
  if (
    typeof value.installedRoot !== "string" ||
    !Array.isArray(value.packages) ||
    !Array.isArray(value.issues) ||
    !Array.isArray(value.workspaces)
  ) {
    throw new Error("The local host returned an incomplete plug-in catalog.");
  }
  if (!value.packages.every(isInstalledPlugin)) {
    throw new Error("The local host returned an invalid installed plug-in.");
  }
  if (!value.issues.every(isPluginIssue)) {
    throw new Error("The local host returned an invalid plug-in diagnostic.");
  }
  if (!value.workspaces.every(isWorkspace)) {
    throw new Error("The local host returned an invalid plug-in workspace.");
  }
  return value as unknown as LocalPluginCatalog;
}

function isInstalledPlugin(value: unknown): value is InstalledPluginDescriptor {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    typeof value.version === "string" &&
    typeof value.folderName === "string" &&
    isPluginRuntime(value.runtime) &&
    Array.isArray(value.capabilities) &&
    value.capabilities.every((entry) => typeof entry === "string") &&
    Array.isArray(value.subscriptions) &&
    value.subscriptions.every((entry) => typeof entry === "string") &&
    Array.isArray(value.allowedNetworkDomains) &&
    value.allowedNetworkDomains.every((entry) => typeof entry === "string") &&
    Array.isArray(value.dependencies) &&
    value.dependencies.every(isPluginDependency) &&
    typeof value.publishesWorkspace === "boolean" &&
    typeof value.enabled === "boolean" &&
    typeof value.active === "boolean" &&
    typeof value.statusDetail === "string"
  );
}

function isPluginRuntime(value: unknown): boolean {
  return (
    value === "data_only" ||
    value === "wasm_component" ||
    value === "browser_overlay" ||
    value === "external_process" ||
    value === "native_developer"
  );
}

function isPluginDependency(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.pluginId === "string" &&
    typeof value.optional === "boolean"
  );
}

function isPluginIssue(value: unknown): value is PluginIssueDescriptor {
  return (
    isRecord(value) &&
    typeof value.kind === "string" &&
    (value.pluginId === null || typeof value.pluginId === "string") &&
    (value.packagePath === null || typeof value.packagePath === "string") &&
    typeof value.detail === "string"
  );
}

function isWorkspace(value: unknown): value is WorkspaceDescriptor {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    typeof value.description === "string" &&
    typeof value.version === "string" &&
    (value.iconUrl === null || typeof value.iconUrl === "string") &&
    typeof value.iconFallback === "string" &&
    typeof value.defaultOrder === "number" &&
    Array.isArray(value.tabs) &&
    value.tabs.length > 0 &&
    value.tabs.every(
      (tab) =>
        isRecord(tab) &&
        typeof tab.id === "string" &&
        typeof tab.label === "string" &&
        (tab.kind === "content" || tab.kind === "options") &&
        typeof tab.entrypoint === "string" &&
        typeof tab.contributorPluginId === "string",
    )
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
