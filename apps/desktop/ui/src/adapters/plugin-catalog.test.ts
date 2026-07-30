import { describe, expect, it } from "vitest";

import { parsePluginCatalog } from "./plugin-catalog";

describe("local plug-in catalog", () => {
  it("accepts the version-one installed package contract", () => {
    const catalog = parsePluginCatalog({
      schemaVersion: 1,
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
            },
          ],
        },
      ],
    });

    expect(catalog.packages[0]?.id).toBe("dev.rlogs.timeline");
    expect(catalog.workspaces[0]?.tabs[0]?.kind).toBe("content");
  });

  it("rejects outdated or incomplete catalogs", () => {
    expect(() => parsePluginCatalog({ schemaVersion: 2 })).toThrow(
      "unsupported plug-in catalog",
    );
    expect(() =>
      parsePluginCatalog({
        schemaVersion: 1,
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
      schemaVersion: 1,
      installedRoot: "C:/rLogs/plugins/installed",
      issues: [],
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
