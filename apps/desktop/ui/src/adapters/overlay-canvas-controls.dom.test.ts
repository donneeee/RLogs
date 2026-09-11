// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UiLocalizer } from "../localization/ui-locale";
import { mountOverlayCanvasControls } from "./overlay-canvas-controls";

const messages: Record<string, string> = {
  "ui.mechanics_map.canvas_editor.aria": "Canvas layout controls",
  "ui.mechanics_map.canvas_editor.label": "Canvas layout",
  "ui.mechanics_map.canvas_editor.mode": "EDIT MODE",
  "ui.mechanics_map.canvas_editor.lock": "Lock",
  "ui.mechanics_map.canvas_editor.unlock": "Unlock",
  "ui.mechanics_map.canvas_editor.lock_help": "Lock help",
  "ui.mechanics_map.canvas_editor.hide": "Hide",
  "ui.mechanics_map.canvas_editor.hide_help": "Hide help",
  "ui.mechanics_map.canvas_editor.done": "Done",
  "ui.mechanics_map.canvas_editor.done_help": "Done help",
};
const localizer: UiLocalizer = {
  locale: "en-US",
  loadedLocales: ["en-US"],
  t: (key) => messages[key] ?? key,
  formatNumber: (value) => String(value),
};

afterEach(() => { document.body.replaceChildren(); });

describe("overlay canvas controls", () => {
  it("owns Hide, Done, Lock, and Escape independently of supplied overlay modules", async () => {
    const player = document.createElement("button"); player.textContent = "Player";
    const actions = document.createElement("button"); actions.textContent = "Actions";
    const toggleLock = vi.fn();
    const hide = vi.fn();
    let resolveDone!: () => void;
    const donePending = new Promise<void>((resolve) => { resolveDone = resolve; });
    const done = vi.fn(() => donePending);
    const mounted = mountOverlayCanvasControls({
      moduleControls: [player, actions], locked: false, toggleLock, hide, done,
    }, localizer);
    document.body.append(mounted.element);

    expect(mounted.element.parentElement).toBe(document.body);
    expect(mounted.element.textContent).toContain("Player");
    expect(mounted.element.textContent).toContain("Actions");
    action("Lock").click();
    action("Hide").click();
    await Promise.resolve();
    expect(toggleLock).toHaveBeenCalledOnce();
    expect(hide).toHaveBeenCalledOnce();
    expect(done).not.toHaveBeenCalled();

    const input = document.createElement("input"); mounted.element.append(input);
    const inputEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    input.dispatchEvent(inputEscape);
    expect(inputEscape.defaultPrevented).toBe(false);
    expect(done).not.toHaveBeenCalled();

    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    window.dispatchEvent(escape);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await Promise.resolve();
    expect(escape.defaultPrevented).toBe(true);
    expect(done).toHaveBeenCalledOnce();
    resolveDone();
    await donePending;

    mounted.setLocked(true);
    expect(action("Unlock").getAttribute("aria-pressed")).toBe("true");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(done).toHaveBeenCalledOnce();
    mounted.dispose();
  });

  function action(label: string): HTMLButtonElement {
    const match = [...document.querySelectorAll("button")].find((button) => button.textContent === label);
    if (!(match instanceof HTMLButtonElement)) throw new Error(`Missing ${label} control`);
    return match;
  }
});
