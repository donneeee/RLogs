import { describe, expect, it } from "vitest";

import {
  displayShortcut,
  OVERLAY_CANVAS_TOGGLE_ACTION_ID,
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
    expect(shortcutFromKeyboardEvent(keyEvent({ key: "ScrollLock", code: "ScrollLock" })))
      .toBe("ScrollLock");
  });

  it("formats registered combinations for people instead of parser syntax", () => {
    expect(displayShortcut("control+shift+KeyO")).toBe("Ctrl+Shift+O");
    expect(displayShortcut("Ctrl+Alt+Digit7")).toBe("Ctrl+Alt+7");
    expect(displayShortcut("ScrollLock")).toBe("ScrollLock");
  });

  it("parses the schema-two action catalog and default Overlay Canvas binding", () => {
    const settings = parseHotkeySettings({
      schemaVersion: 2,
      actions: [{
        actionId: "app.rlogs.combat-overlay.toggle-visibility",
        label: "Show/hide Combat Overlay",
        description: "Toggle it.",
        category: "Combat Overlay",
      }, {
        actionId: OVERLAY_CANVAS_TOGGLE_ACTION_ID,
        label: "Show/hide canvas overlays",
        description: "Toggle the shared Overlay Canvas.",
        category: "Overlay",
      }],
      bindings: {
        "app.rlogs.combat-overlay.toggle-visibility": "Ctrl+Shift+KeyO",
        [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock",
      },
    });
    expect(settings.bindings["app.rlogs.combat-overlay.toggle-visibility"])
      .toBe("Ctrl+Shift+KeyO");
    expect(settings.bindings[OVERLAY_CANVAS_TOGGLE_ACTION_ID]).toBe("ScrollLock");
    expect(settings.actions.map((action) => action.actionId)).toContain(
      OVERLAY_CANVAS_TOGGLE_ACTION_ID,
    );
    expect(settings.registrationErrors).toEqual({});

    expect(parseHotkeySettings({
      ...settings,
      registrationErrors: {
        [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock is already in use.",
      },
    }).registrationErrors[OVERLAY_CANVAS_TOGGLE_ACTION_ID])
      .toBe("ScrollLock is already in use.");
  });
});
