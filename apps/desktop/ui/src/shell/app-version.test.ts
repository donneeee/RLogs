import { describe, expect, it } from "vitest";

import { displayVersion, releaseNotesUrl } from "./app-version";

describe("desktop app version", () => {
  it("links an installed version to its exact GitHub release notes", () => {
    expect(displayVersion("0.1.45")).toBe("v0.1.45");
    expect(releaseNotesUrl("0.1.45")).toBe(
      "https://github.com/donneeee/RLogs/releases/tag/v0.1.45",
    );
  });

  it("does not interpolate an invalid version into an external URL", () => {
    expect(releaseNotesUrl("development")).toBe(
      "https://github.com/donneeee/RLogs/releases",
    );
  });
});
