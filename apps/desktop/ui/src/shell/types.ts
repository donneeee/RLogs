export type WorkspaceTabKind = "content" | "options";

export interface WorkspaceTabDescriptor {
  id: string;
  label: string;
  kind: WorkspaceTabKind;
  entrypoint: string;
  /// Package that owns the surface. This can differ from the workspace owner
  /// when an add-on contributes a namespaced tab.
  contributorPluginId: string;
}

export interface WorkspaceDescriptor {
  id: string;
  name: string;
  description: string;
  version: string;
  iconUrl: string | null;
  iconFallback: string;
  defaultOrder: number;
  tabs: readonly WorkspaceTabDescriptor[];
}

export type PluginRuntimeKind =
  | "data_only"
  | "wasm_component"
  | "browser_overlay"
  | "external_process"
  | "native_developer";

export interface PluginDependencyDescriptor {
  pluginId: string;
  optional: boolean;
}

export interface InstalledPluginDescriptor {
  id: string;
  name: string;
  version: string;
  folderName: string;
  runtime: PluginRuntimeKind;
  capabilities: readonly string[];
  subscriptions: readonly string[];
  allowedNetworkDomains: readonly string[];
  dependencies: readonly PluginDependencyDescriptor[];
  publishesWorkspace: boolean;
  enabled: boolean;
  active: boolean;
  statusDetail: string;
}

export interface PluginIssueDescriptor {
  kind: string;
  pluginId: string | null;
  packagePath: string | null;
  detail: string;
}

export interface PluginCatalogSnapshot {
  schemaVersion: number;
  installedRoot: string;
  packages: readonly InstalledPluginDescriptor[];
  issues: readonly PluginIssueDescriptor[];
}

export interface ShellPreferences {
  workspaceOrder: readonly string[];
  activeWorkspaceId: string | null;
  activeTabs: Readonly<Record<string, string>>;
}

export interface MountedSurface {
  dispose(): void;
}

export interface DesktopHostAdapter {
  readonly modeLabel: string;
  loadWorkspaces(): Promise<readonly WorkspaceDescriptor[]>;
  loadPreferences(): Promise<ShellPreferences>;
  savePreferences(preferences: ShellPreferences): Promise<void>;
  mountSurface(
    workspace: WorkspaceDescriptor,
    tab: WorkspaceTabDescriptor,
    container: HTMLElement,
  ): Promise<MountedSurface>;
  loadPluginCatalog?(): Promise<PluginCatalogSnapshot>;
  setPluginEnabled?(
    pluginId: string,
    enabled: boolean,
  ): Promise<PluginCatalogSnapshot>;
  refreshPlugins?(): Promise<PluginCatalogSnapshot>;
  setExampleWorkspacesEnabled?(enabled: boolean): Promise<void>;
}
