import { describe, expect, it } from "vitest";

import { parsePluginCatalog } from "./plugin-catalog";

describe("local plug-in catalog", () => {
  it("accepts the version-two installed package and Settings contract", () => {
    const catalog = parsePluginCatalog({
      schemaVersion: 2,
      installedRoot: "C:/rLogs/plugins/installed",
      packages: [
        {
          id: "dev.rlogs.timeline",
          name: "Timeline",
          version: "0.1.0",
          folderName: "timeline",
          runtime: "wasm_component",
          capabilities: ["events_read", "ui_workspace_publish"],
          subscriptions: ["combat"],
          allowedNetworkDomains: [],
          dependencies: [],
          publishesWorkspace: true,
          enabled: true,
          active: true,
          statusDetail: "Enabled and validated.",
        },
      ],
      issues: [],
      settingsTabs: [
        {
          id: "dev.rlogs.timeline:settings",
          label: "Timeline",
          kind: "options",
          entrypoint: "installed://dev.rlogs.timeline/settings",
          contributorPluginId: "dev.rlogs.timeline",
          sectionId: "dev.rlogs.timeline:settings",
          defaultOrder: 200,
        },
      ],
      workspaces: [
        {
          id: "dev.rlogs.timeline",
          name: "Timeline",
          description: "Installed package.",
          version: "0.1.0",
          iconUrl: null,
          iconFallback: "TL",
          defaultOrder: 10,
          tabs: [
            {
              id: "dev.rlogs.timeline:main",
              label: "Main",
              kind: "content",
              entrypoint: "installed://dev.rlogs.timeline/main",
              contributorPluginId: "dev.rlogs.timeline",
              sectionId: "dev.rlogs.timeline:main",
              defaultOrder: 0,
            },
          ],
        },
      ],
    });

    expect(catalog.packages[0]?.id).toBe("dev.rlogs.timeline");
    expect(catalog.workspaces[0]?.tabs[0]?.kind).toBe("content");
    expect(catalog.settingsTabs[0]?.label).toBe("Timeline");
  });

  it("rejects outdated or incomplete catalogs", () => {
    expect(() => parsePluginCatalog({ schemaVersion: 1 })).toThrow(
      "unsupported plug-in catalog",
    );
    expect(() =>
      parsePluginCatalog({
        schemaVersion: 2,
        installedRoot: "plugins",
        packages: [],
        issues: [],
      }),
    ).toThrow("incomplete plug-in catalog");
  });

  it("rejects unknown runtimes and malformed dependencies", () => {
    const packageBase = {
      id: "dev.rlogs.timeline",
      name: "Timeline",
      version: "0.1.0",
      folderName: "timeline",
      capabilities: [],
      subscriptions: [],
      allowedNetworkDomains: [],
      publishesWorkspace: false,
      enabled: false,
      active: false,
      statusDetail: "Disabled by user.",
    };
    const catalogBase = {
      schemaVersion: 2,
      installedRoot: "C:/rLogs/plugins/installed",
      issues: [],
      settingsTabs: [],
      workspaces: [],
    };

    expect(() =>
      parsePluginCatalog({
        ...catalogBase,
        packages: [
          {
            ...packageBase,
            runtime: "unrestricted",
            dependencies: [],
          },
        ],
      }),
    ).toThrow("invalid installed plug-in");
    expect(() =>
      parsePluginCatalog({
        ...catalogBase,
        packages: [
          {
            ...packageBase,
            runtime: "data_only",
            dependencies: [{ pluginId: 7, optional: false }],
          },
        ],
      }),
    ).toThrow("invalid installed plug-in");
  });
});
