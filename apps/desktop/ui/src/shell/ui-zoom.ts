const STORAGE_KEY = "rlogs.interface-zoom.v1";

export const DEFAULT_UI_ZOOM_PERCENT = 100;
export const MIN_UI_ZOOM_PERCENT = 50;
export const MAX_UI_ZOOM_PERCENT = 200;
export const UI_ZOOM_STEP_PERCENT = 10;

export function clampUiZoomPercent(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_UI_ZOOM_PERCENT;
  return Math.min(
    MAX_UI_ZOOM_PERCENT,
    Math.max(MIN_UI_ZOOM_PERCENT, Math.round(value)),
  );
}

export function steppedUiZoomPercent(
  current: number,
  direction: -1 | 1,
): number {
  const normalized = clampUiZoomPercent(current);
  const next =
    direction > 0
      ? Math.floor(normalized / UI_ZOOM_STEP_PERCENT) * UI_ZOOM_STEP_PERCENT +
        UI_ZOOM_STEP_PERCENT
      : Math.ceil(normalized / UI_ZOOM_STEP_PERCENT) * UI_ZOOM_STEP_PERCENT -
        UI_ZOOM_STEP_PERCENT;
  return clampUiZoomPercent(next);
}

export function keyboardZoomAction(
  event: Pick<KeyboardEvent, "altKey" | "code" | "ctrlKey" | "key" | "metaKey">,
): -1 | 1 | "reset" | null {
  if (!event.ctrlKey || event.metaKey || event.altKey) return null;
  if (event.key === "0" || event.code === "Numpad0") return "reset";
  if (
    event.key === "+" ||
    event.key === "=" ||
    event.code === "NumpadAdd"
  ) {
    return 1;
  }
  if (
    event.key === "-" ||
    event.key === "_" ||
    event.code === "NumpadSubtract"
  ) {
    return -1;
  }
  return null;
}

function loadZoomPercent(): number {
  try {
    const stored = Number.parseInt(localStorage.getItem(STORAGE_KEY) ?? "", 10);
    return clampUiZoomPercent(stored);
  } catch {
    return DEFAULT_UI_ZOOM_PERCENT;
  }
}

function storeZoomPercent(value: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(value));
  } catch {
    // Zoom remains available for this session when storage is unavailable.
  }
}

export function installInterfaceZoom(): () => void {
  let zoomPercent = loadZoomPercent();
  let wheelDelta = 0;
  let statusTimer: number | null = null;
  const status = document.createElement("div");
  status.className = "ui-zoom-status";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  document.body.append(status);

  const apply = (next: number, announce: boolean) => {
    zoomPercent = clampUiZoomPercent(next);
    const scale = zoomPercent / 100;
    document.documentElement.style.setProperty("--rlogs-ui-zoom", String(scale));
    document.documentElement.style.setProperty(
      "--rlogs-ui-zoom-inverse",
      String(1 / scale),
    );
    document.documentElement.dataset.uiZoom = String(zoomPercent);
    storeZoomPercent(zoomPercent);
    if (!announce) return;
    status.textContent = `Interface zoom: ${zoomPercent}%`;
    status.dataset.visible = "true";
    if (statusTimer !== null) window.clearTimeout(statusTimer);
    statusTimer = window.setTimeout(() => {
      status.dataset.visible = "false";
      statusTimer = null;
    }, 1_200);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const action = keyboardZoomAction(event);
    if (action === null) return;
    event.preventDefault();
    apply(
      action === "reset"
        ? DEFAULT_UI_ZOOM_PERCENT
        : steppedUiZoomPercent(zoomPercent, action),
      true,
    );
  };

  const onWheel = (event: WheelEvent) => {
    if (!event.ctrlKey || event.deltaY === 0) return;
    event.preventDefault();
    wheelDelta += event.deltaY;
    if (Math.abs(wheelDelta) < 50) return;
    apply(steppedUiZoomPercent(zoomPercent, wheelDelta < 0 ? 1 : -1), true);
    wheelDelta = 0;
  };

  apply(zoomPercent, false);
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("wheel", onWheel, { passive: false });
  return () => {
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("wheel", onWheel);
    if (statusTimer !== null) window.clearTimeout(statusTimer);
    status.remove();
  };
}
