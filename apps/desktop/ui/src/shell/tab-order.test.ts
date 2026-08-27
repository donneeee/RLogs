import { describe, expect, it } from "vitest";

import {
  moveSection,
  moveTabInsideSection,
  orderTabSections,
} from "./tab-order";
import type { WorkspaceTabDescriptor } from "./types";

const tabs: WorkspaceTabDescriptor[] = [
  tab("meter:history", "meter", 0),
  tab("meter:options", "meter", 1),
  tab("overlay:live", "overlay", 100),
  tab("overlay:options", "overlay", 101),
];

describe("tab section ordering", () => {
  it("keeps manifest membership while restoring section and tab order", () => {
    const sections = orderTabSections(
      tabs,
      ["meter:options", "meter:history"],
      ["overlay", "meter"],
    );
    expect(sections.map((section) => section.id)).toEqual([
      "overlay",
      "meter",
    ]);
    expect(sections[1]?.tabs.map((tab) => tab.id)).toEqual([
      "meter:options",
      "meter:history",
    ]);
  });

  it("rejects a tab drop into a different section", () => {
    const sections = orderTabSections(tabs, [], []);
    expect(
      moveTabInsideSection(sections, "meter:options", "overlay:live"),
    ).toBeNull();
  });

  it("moves tabs inside one section and sections as whole blocks", () => {
    const sections = orderTabSections(tabs, [], []);
    expect(
      moveTabInsideSection(sections, "meter:options", "meter:history"),
    ).toEqual([
      "meter:options",
      "meter:history",
      "overlay:live",
      "overlay:options",
    ]);
    expect(moveSection(sections, "overlay", "meter")).toEqual([
      "overlay",
      "meter",
    ]);
    expect(moveSection(sections, "meter", "overlay")).toEqual([
      "overlay",
      "meter",
    ]);
  });
});

function tab(
  id: string,
  sectionId: string,
  defaultOrder: number,
): WorkspaceTabDescriptor {
  return {
    id,
    label: id,
    kind: id.endsWith("options") ? "options" : "content",
    entrypoint: `builtin://${id}`,
    contributorPluginId: id.split(":")[0] ?? "plugin",
    sectionId,
    defaultOrder,
  };
}
