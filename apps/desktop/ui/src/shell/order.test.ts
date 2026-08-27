import { describe, expect, it } from "vitest";

import type { WorkspaceDescriptor } from "./types";
import {
  mergeWorkspaceOrder,
  moveWorkspace,
  moveWorkspaceByOffset,
} from "./order";

function workspace(
  id: string,
  name: string,
  defaultOrder: number,
): WorkspaceDescriptor {
  return {
    id,
    name,
    defaultOrder,
    description: "",
    version: "0.1.0",
    iconUrl: null,
    iconFallback: name.slice(0, 2),
    tabs: [
      {
        id: "main",
        label: "Main",
        kind: "content",
        entrypoint: "ui/main.html",
        contributorPluginId: id,
        sectionId: `${id}:main`,
        defaultOrder: 0,
      },
    ],
  };
}

describe("mergeWorkspaceOrder", () => {
  it("preserves saved IDs and appends newly installed plug-ins deterministically", () => {
    const workspaces = [
      workspace("app.third", "Third", 30),
      workspace("app.first", "First", 10),
      workspace("app.second", "Second", 20),
    ];

    expect(mergeWorkspaceOrder(workspaces, ["app.second"])).toEqual([
      "app.second",
      "app.first",
      "app.third",
    ]);
  });

  it("removes stale and duplicate IDs", () => {
    const workspaces = [workspace("app.first", "First", 10)];

    expect(
      mergeWorkspaceOrder(workspaces, [
        "missing.plugin",
        "app.first",
        "app.first",
      ]),
    ).toEqual(["app.first"]);
  });
});

describe("workspace movement", () => {
  it("moves a dragged workspace to the target index", () => {
    expect(moveWorkspace(["a", "b", "c"], "a", "c")).toEqual(["b", "c", "a"]);
  });

  it("supports bounded keyboard movement", () => {
    expect(moveWorkspaceByOffset(["a", "b", "c"], "b", -1)).toEqual([
      "b",
      "a",
      "c",
    ]);
    expect(moveWorkspaceByOffset(["a", "b", "c"], "a", -1)).toEqual([
      "a",
      "b",
      "c",
    ]);
  });
});
