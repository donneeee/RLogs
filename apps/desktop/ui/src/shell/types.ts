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
  setExampleWorkspacesEnabled?(enabled: boolean): Promise<void>;
}
