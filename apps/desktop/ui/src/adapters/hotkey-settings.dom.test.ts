// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  mountHotkeyBinding,
  OVERLAY_CANVAS_TOGGLE_ACTION_ID,
} from "./hotkey-settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
  document.body.replaceChildren();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.resetAllMocks();
});

describe("mounted Hotkey settings", () => {
  it("loads a native startup conflict and lets the user clear it", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const conflicted = {
      schemaVersion: 2,
      actions: [{
        actionId: OVERLAY_CANVAS_TOGGLE_ACTION_ID,
        label: "Show/hide shared overlays",
        description: "Toggle the shared Overlay canvas.",
        category: "Overlay",
      }],
      bindings: { [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock" },
      registrationErrors: {
        [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock is already used by another application.",
      },
    };
    const cleared = {
      ...conflicted,
      bindings: {},
      registrationErrors: {},
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "load_hotkey_settings") return conflicted;
      if (command === "assign_hotkey") {
        return { settings: cleared, displacedActionId: null };
      }
      throw new Error(`Unexpected native command: ${command}`);
    });

    const mounted = mountHotkeyBinding(OVERLAY_CANVAS_TOGGLE_ACTION_ID);
    document.body.append(mounted.element);
    await vi.waitFor(() => expect(mounted.element.textContent).toContain("ScrollLock"));

    expect(invoke).toHaveBeenCalledWith("load_hotkey_settings");
    const registrationError = mounted.element.querySelector<HTMLElement>(
      ".hotkey-registration-error",
    );
    expect(registrationError?.hidden).toBe(false);
    expect(registrationError?.textContent).toContain("already used by another application");
    const capture = mounted.element.querySelector<HTMLButtonElement>(".hotkey-capture-button");
    const clear = mounted.element.querySelector<HTMLButtonElement>(".hotkey-clear-button");
    expect(capture?.disabled).toBe(false);
    expect(clear?.disabled).toBe(false);

    capture?.click();
    expect(capture?.textContent).toBe("Press a shortcut...");
    clear?.click();
    await vi.waitFor(() => expect(capture?.textContent).toBe("Not assigned"));
    expect(registrationError?.hidden).toBe(true);
    expect(invoke).toHaveBeenCalledWith("assign_hotkey", {
      assignment: {
        actionId: OVERLAY_CANVAS_TOGGLE_ACTION_ID,
        shortcut: null,
      },
    });

    window.dispatchEvent(new KeyboardEvent("keydown", {
      code: "F8",
      key: "F8",
      bubbles: true,
    }));
    expect(invoke).toHaveBeenCalledTimes(2);
    mounted.dispose();
  });

  it("labels browser assignments as pending a desktop restart", async () => {
    const view = {
      schemaVersion: 2,
      actions: [{
        actionId: OVERLAY_CANVAS_TOGGLE_ACTION_ID,
        label: "Show/hide shared overlays",
        description: "Toggle the shared Overlay canvas.",
        category: "Overlay",
      }],
      bindings: { [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock" },
    };
    vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const body = init?.method === "POST"
        ? { settings: { ...view, bindings: {} }, displacedActionId: null }
        : view;
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }));

    const mounted = mountHotkeyBinding(OVERLAY_CANVAS_TOGGLE_ACTION_ID);
    document.body.append(mounted.element);
    await vi.waitFor(() => {
      expect(mounted.element.textContent).toContain("activate after it restarts");
    });
    expect(mounted.element.textContent).toContain("Native registration status is unavailable");
    mounted.element.querySelector<HTMLButtonElement>(".hotkey-clear-button")?.click();
    await vi.waitFor(() => {
      expect(mounted.element.textContent).toContain(
        "Shortcut removal was saved and takes effect after desktop rLogs restarts.",
      );
    });
    mounted.dispose();
    vi.unstubAllGlobals();
  });

  it("labels browser conflict displacement as pending a desktop restart", async () => {
    const combatActionId = "app.rlogs.combat-overlay.toggle-visibility";
    const actions = [{
      actionId: OVERLAY_CANVAS_TOGGLE_ACTION_ID,
      label: "Show/hide shared overlays",
      description: "Toggle the shared Overlay canvas.",
      category: "Overlay",
    }, {
      actionId: combatActionId,
      label: "Show/hide Combat Overlay",
      description: "Toggle the Combat Overlay.",
      category: "Combat Overlay",
    }];
    const initial = {
      schemaVersion: 2,
      actions,
      bindings: {
        [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "ScrollLock",
        [combatActionId]: "F9",
      },
    };
    const assigned = {
      schemaVersion: 2,
      actions,
      bindings: { [OVERLAY_CANVAS_TOGGLE_ACTION_ID]: "F9" },
    };
    vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const body = init?.method === "POST"
        ? { settings: assigned, displacedActionId: combatActionId }
        : initial;
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }));

    const mounted = mountHotkeyBinding(OVERLAY_CANVAS_TOGGLE_ACTION_ID);
    document.body.append(mounted.element);
    const capture = mounted.element.querySelector<HTMLButtonElement>(".hotkey-capture-button");
    await vi.waitFor(() => expect(capture?.textContent).toBe("ScrollLock"));
    capture?.click();
    window.dispatchEvent(new KeyboardEvent("keydown", {
      code: "F9",
      key: "F9",
      bubbles: true,
    }));
    await vi.waitFor(() => {
      expect(mounted.element.textContent).toContain(
        "Show/hide Combat Overlay was cleared. Both changes take effect after desktop rLogs restarts.",
      );
    });
    mounted.dispose();
    vi.unstubAllGlobals();
  });
});
