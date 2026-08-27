import { describe, expect, it } from "vitest";

import {
  displayShortcut,
  parseHotkeySettings,
  shortcutFromKeyboardEvent,
} from "./hotkey-settings";

function keyEvent(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "o",
    code: "KeyO",
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("Hotkey settings", () => {
  it("captures physical key combinations and requires modifiers for letters", () => {
    expect(shortcutFromKeyboardEvent(keyEvent({ ctrlKey: true, shiftKey: true })))
      .toBe("Ctrl+Shift+KeyO");
    expect(shortcutFromKeyboardEvent(keyEvent({}))).toBeNull();
    expect(shortcutFromKeyboardEvent(keyEvent({ key: "F10", code: "F10" })))
      .toBe("F10");
  });

  it("formats registered combinations for people instead of parser syntax", () => {
    expect(displayShortcut("control+shift+KeyO")).toBe("Ctrl+Shift+O");
    expect(displayShortcut("Ctrl+Alt+Digit7")).toBe("Ctrl+Alt+7");
  });

  it("parses an authoritative catalog and binding map", () => {
    expect(parseHotkeySettings({
      schemaVersion: 1,
      actions: [{
        actionId: "app.rlogs.combat-overlay.toggle-visibility",
        label: "Show/hide Combat Overlay",
        description: "Toggle it.",
        category: "Combat Overlay",
      }],
      bindings: {
        "app.rlogs.combat-overlay.toggle-visibility": "Ctrl+Shift+KeyO",
      },
    }).bindings["app.rlogs.combat-overlay.toggle-visibility"]).toBe("Ctrl+Shift+KeyO");
  });
});
