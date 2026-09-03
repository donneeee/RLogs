import { afterEach, describe, expect, it, vi } from "vitest";

import { createDevelopmentAdapter } from "./development-adapter";
import { customTriggerRuleMenuGroups } from "./custom-triggers-workspace-surface";

describe("development desktop workspaces", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps the design-only Overlay and Custom Triggers menus locally testable", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const workspaces = await createDevelopmentAdapter().loadWorkspaces();
    const overlay = workspaces.find(({ id }) => id === "app.rlogs.overlay");
    const triggers = workspaces.find(({ id }) => id === "app.rlogs.custom-triggers");

    expect(overlay?.tabs.map(({ label }) => label)).toEqual([
      "Overview",
      "Setups",
      "Editor",
      "Trackers",
      "Mechanics Map",
      "Settings",
    ]);
    expect(triggers?.tabs.map(({ label }) => label)).toEqual([
      "Overview",
      "Rules",
      "Event Inspector",
      "Library",
      "Settings",
    ]);
    expect(overlay?.version).toBe("0.1.1");
    expect(triggers?.version).toBe("0.1.2");
  });

  it("keeps beginner rule steps visible while advanced controls stay grouped", () => {
    expect(customTriggerRuleMenuGroups()).toEqual({
      management: ["My Rules & Folders"],
      flow: ["When", "If", "Then"],
      advanced: ["Timing & Repeat", "State & Variables", "Advanced"],
      review: ["Test & Review"],
    });
  });
});
