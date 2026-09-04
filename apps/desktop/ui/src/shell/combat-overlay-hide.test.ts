import { describe, expect, it, vi } from "vitest";

import { dispatchCombatOverlayHide } from "./combat-overlay-hide";

describe("Combat Overlay manual hide dispatch", () => {
  it("does not latch later hide clicks behind an IPC promise suspended by the hidden WebView", () => {
    const pending = new Promise<unknown>(() => undefined);
    const hostHide = vi.fn(() => pending);
    const nativeHide = vi.fn(() => Promise.resolve());
    const onFailure = vi.fn();

    dispatchCombatOverlayHide(hostHide, nativeHide, onFailure);
    dispatchCombatOverlayHide(hostHide, nativeHide, onFailure);

    expect(hostHide).toHaveBeenCalledTimes(2);
    expect(nativeHide).not.toHaveBeenCalled();
    expect(onFailure).not.toHaveBeenCalled();
  });

  it("uses the direct native hide only when host dispatch fails", async () => {
    const failure = new Error("host IPC unavailable");
    const hostHide = vi.fn(() => Promise.reject(failure));
    const nativeHide = vi.fn(() => Promise.resolve());
    const onFailure = vi.fn();

    dispatchCombatOverlayHide(hostHide, nativeHide, onFailure);
    await vi.waitFor(() => expect(nativeHide).toHaveBeenCalledOnce());

    expect(onFailure).toHaveBeenCalledWith("host hide", failure);
  });
});
