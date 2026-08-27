import { describe, expect, it } from "vitest";

import {
  clampUiZoomPercent,
  keyboardZoomAction,
  steppedUiZoomPercent,
} from "./ui-zoom";

describe("interface zoom", () => {
  it("clamps and steps through safe zoom levels", () => {
    expect(clampUiZoomPercent(25)).toBe(50);
    expect(clampUiZoomPercent(250)).toBe(200);
    expect(clampUiZoomPercent(Number.NaN)).toBe(100);
    expect(steppedUiZoomPercent(100, 1)).toBe(110);
    expect(steppedUiZoomPercent(105, 1)).toBe(110);
    expect(steppedUiZoomPercent(105, -1)).toBe(100);
  });

  it("recognizes browser-style keyboard shortcuts", () => {
    const event = (
      key: string,
      code = "",
      ctrlKey = true,
    ): Parameters<typeof keyboardZoomAction>[0] => ({
      altKey: false,
      code,
      ctrlKey,
      key,
      metaKey: false,
    });
    expect(keyboardZoomAction(event("+"))).toBe(1);
    expect(keyboardZoomAction(event("="))).toBe(1);
    expect(keyboardZoomAction(event("-"))).toBe(-1);
    expect(keyboardZoomAction(event("0"))).toBe("reset");
    expect(keyboardZoomAction(event("+", "", false))).toBeNull();
  });
});
