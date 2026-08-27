import { describe, expect, it } from "vitest";

import { parseThemeSettings } from "./theme-settings";

describe("Themes settings", () => {
  it("accepts the bounded host contract", () => {
    expect(
      parseThemeSettings({
        schemaVersion: 1,
        preset: "midnight",
        density: "comfortable",
        font: "system",
        fontScalePercent: 100,
        accent: "#64dfd2",
        background: "soft-glow",
      }).accent,
    ).toBe("#64dfd2");
  });

  it("rejects arbitrary CSS and invalid scale values", () => {
    expect(() =>
      parseThemeSettings({
        schemaVersion: 1,
        preset: "midnight",
        density: "comfortable",
        font: "system",
        fontScalePercent: 500,
        accent: "url(file:///secret)",
        background: "custom-css",
      }),
    ).toThrow("invalid Themes settings");
  });
});
