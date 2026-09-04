export type CombatOverlayHideFailure = (
  operation: "host hide" | "native hide fallback",
  error: unknown,
) => void;

/**
 * Dispatches a manual Combat Overlay hide without retaining an in-flight
 * promise in the WebView.
 *
 * A successful native hide can suspend WebView2 before the IPC promise settles.
 * Keeping that promise as a click guard would therefore leave Hide permanently
 * latched after the window is shown again. The host command owns both the
 * requested-visible flag and the physical window transition; the direct window
 * API is used only if command dispatch fails while the WebView is still awake.
 */
export function dispatchCombatOverlayHide(
  hideThroughHost: () => Promise<unknown>,
  hideNativeFallback: () => Promise<void>,
  onFailure: CombatOverlayHideFailure,
): void {
  void hideThroughHost().catch((hostError) => {
    onFailure("host hide", hostError);
    void hideNativeFallback().catch((nativeError) => {
      onFailure("native hide fallback", nativeError);
    });
  });
}
