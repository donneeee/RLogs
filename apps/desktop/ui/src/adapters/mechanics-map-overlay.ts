import type { MountedSurface } from "../shell/types";
import {
  actionControlRemainingMillis,
  mechanicSignalRemainingMillis,
  projectCoralMatrixBeam,
  projectCoralPizzaRegions,
  projectCoralWaveRegion,
  projectCursedTombChargeRegion,
  projectMechanicsMapEntities,
  projectMechanicsMapPoint,
  projectRaidFloorRegions,
  projectTinaPizzaRegion,
  targetDebuffRemainingMillis,
  zoomMechanicsMapAt,
  type MechanicsMapProjectedRegion,
  type MechanicsMapSnapshot,
  type MechanicsMapUpdate,
  type MechanicsMapViewPoint,
} from "./mechanics-map";

const PREFERENCES_KEY = "rlogs.mechanics-map-overlay.canvas.v1";

export interface MechanicsMapOverlayDependencies {
  loadSnapshot(): Promise<MechanicsMapUpdate>;
  waitForSnapshot(afterRevision: number): Promise<MechanicsMapUpdate>;
  prepareLocalMaps(): Promise<void>;
  hide(): Promise<void>;
  setInteractive(interactive: boolean): Promise<void>;
  onInteractivity(handler: (interactive: boolean) => void): Promise<() => void>;
  onFocusHeld(handler: (held: boolean) => void): Promise<() => void>;
}

export interface MechanicsMapCanvasPreferences {
  scale: number;
  panX: number;
  panY: number;
  rotateWithPlayer: boolean;
  showMonsters: boolean;
  locked: boolean;
  moduleX: number;
  moduleY: number;
  moduleWidth: number;
  moduleHeight: number;
  targetX: number;
  targetY: number;
  targetWidth: number;
  playerX: number;
  playerY: number;
  playerWidth: number;
  actionsX: number;
  actionsY: number;
  actionsWidth: number;
  partyX: number;
  partyY: number;
  partyWidth: number;
  objectivesX: number;
  objectivesY: number;
  objectivesWidth: number;
  alertsX: number;
  alertsY: number;
  alertsWidth: number;
}

const DEFAULT_PREFERENCES: MechanicsMapCanvasPreferences = {
  scale: 1,
  panX: 0,
  panY: 0,
  rotateWithPlayer: true,
  showMonsters: true,
  locked: false,
  moduleX: 24,
  moduleY: 120,
  moduleWidth: 520,
  moduleHeight: 520,
  targetX: 580,
  targetY: 48,
  targetWidth: 420,
  playerX: 24,
  playerY: 48,
  playerWidth: 420,
  actionsX: 24,
  actionsY: 680,
  actionsWidth: 520,
  partyX: 1040,
  partyY: 48,
  partyWidth: 360,
  objectivesX: 580,
  objectivesY: 360,
  objectivesWidth: 420,
  alertsX: 1040,
  alertsY: 420,
  alertsWidth: 360,
};

export function parseMechanicsMapCanvasPreferences(value: unknown): MechanicsMapCanvasPreferences {
  if (!record(value)) return { ...DEFAULT_PREFERENCES };
  const scale = finitePositive(value.scale) ? value.scale : 1;
  const panX = finiteBounded(value.panX) ? value.panX : 0;
  const panY = finiteBounded(value.panY) ? value.panY : 0;
  return {
    scale,
    panX,
    panY,
    rotateWithPlayer: typeof value.rotateWithPlayer === "boolean" ? value.rotateWithPlayer : true,
    showMonsters: typeof value.showMonsters === "boolean" ? value.showMonsters : true,
    locked: typeof value.locked === "boolean" ? value.locked : false,
    moduleX: finiteBounded(value.moduleX) ? value.moduleX : 24,
    moduleY: finiteBounded(value.moduleY) ? value.moduleY : 120,
    moduleWidth: finitePositive(value.moduleWidth) ? Math.max(260, value.moduleWidth) : 520,
    moduleHeight: finitePositive(value.moduleHeight) ? Math.max(260, value.moduleHeight) : 520,
    targetX: finiteBounded(value.targetX) ? value.targetX : 580,
    targetY: finiteBounded(value.targetY) ? value.targetY : 48,
    targetWidth: finitePositive(value.targetWidth) ? Math.max(280, value.targetWidth) : 420,
    playerX: finiteBounded(value.playerX) ? value.playerX : 24,
    playerY: finiteBounded(value.playerY) ? value.playerY : 48,
    playerWidth: finitePositive(value.playerWidth) ? Math.max(280, value.playerWidth) : 420,
    actionsX: finiteBounded(value.actionsX) ? value.actionsX : 24,
    actionsY: finiteBounded(value.actionsY) ? value.actionsY : 680,
    actionsWidth: finitePositive(value.actionsWidth) ? Math.max(280, value.actionsWidth) : 520,
    partyX: finiteBounded(value.partyX) ? value.partyX : 1040,
    partyY: finiteBounded(value.partyY) ? value.partyY : 48,
    partyWidth: finitePositive(value.partyWidth) ? Math.max(260, value.partyWidth) : 360,
    objectivesX: finiteBounded(value.objectivesX) ? value.objectivesX : 580,
    objectivesY: finiteBounded(value.objectivesY) ? value.objectivesY : 360,
    objectivesWidth: finitePositive(value.objectivesWidth) ? Math.max(280, value.objectivesWidth) : 420,
    alertsX: finiteBounded(value.alertsX) ? value.alertsX : 1040,
    alertsY: finiteBounded(value.alertsY) ? value.alertsY : 420,
    alertsWidth: finitePositive(value.alertsWidth) ? Math.max(260, value.alertsWidth) : 360,
  };
}

export function shouldRenderMechanicsMapUpdate(
  current: Pick<MechanicsMapUpdate, "revision">,
  next: Pick<MechanicsMapUpdate, "revision">,
): boolean {
  return next.revision !== current.revision;
}

export function formatDungeonObjectiveValue(
  value: number | null,
  complete: boolean | null,
  requiredCount: number | null = null,
): string {
  if (value !== null && requiredCount !== null) {
    return `${value.toLocaleString()} / ${requiredCount.toLocaleString()}${complete === true ? " ✓" : ""}`;
  }
  if (value !== null) return `${value.toLocaleString()}${complete === true ? " ✓" : ""}`;
  return complete === true ? "Complete" : "Observed";
}

export function formatDungeonAttemptTime(micros: number): string {
  const totalTenths = Math.max(0, Math.floor(micros / 100_000));
  const hours = Math.floor(totalTenths / 36_000);
  const minutes = Math.floor(totalTenths / 600) % 60;
  const seconds = Math.floor(totalTenths / 10) % 60;
  const tenths = totalTenths % 10;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${tenths}`
    : `${minutes}:${String(seconds).padStart(2, "0")}.${tenths}`;
}

function humanizeDungeonState(value: string): string {
  return value.replaceAll("_", " ").toUpperCase();
}

export function mountMechanicsMapOverlay(
  container: HTMLElement,
  dependencies: MechanicsMapOverlayDependencies,
): MountedSurface {
  let alive = true;
  let update: MechanicsMapUpdate | null = null;
  let preferences = loadPreferences();
  let drag: { pointerId: number; x: number; y: number } | null = null;
  let moduleDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let moduleResize: { pointerId: number; x: number; y: number; width: number; height: number } | null = null;
  let targetDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let targetResize: { pointerId: number; x: number; width: number } | null = null;
  let playerDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let playerResize: { pointerId: number; x: number; width: number } | null = null;
  let actionsDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let actionsResize: { pointerId: number; x: number; width: number } | null = null;
  let partyDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let partyResize: { pointerId: number; x: number; width: number } | null = null;
  let objectivesDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let objectivesResize: { pointerId: number; x: number; width: number } | null = null;
  let alertsDrag: { pointerId: number; x: number; y: number; left: number; top: number } | null = null;
  let alertsResize: { pointerId: number; x: number; width: number } | null = null;
  let frame: number | null = null;
  let image: HTMLImageElement | null = null;
  let imageUrl: string | null = null;
  let imageReady = false;
  let preparingAsset = false;
  let removeInteractivityListener: (() => void) | null = null;
  let removeFocusHeldListener: (() => void) | null = null;
  let targetTimer: number | null = null;
  let targetRenderedAtMillis = 0;
  let playerTimer: number | null = null;
  let playerRenderedAtMillis = 0;
  let actionsTimer: number | null = null;
  let actionsRenderedAtMillis = 0;
  let objectivesTimer: number | null = null;
  let objectivesRenderedAtMillis = 0;
  let alertsTimer: number | null = null;
  let alertsRenderedAtMillis = 0;

  const root = element("main", "overlay-canvas-runtime");
  const panel = element("section", "mechanics-map-overlay-runtime");
  panel.dataset.locked = String(preferences.locked);
  const toolbar = element("header", "mechanics-map-overlay-toolbar");
  const identity = element("div", "mechanics-map-overlay-identity");
  const title = text("strong", "Waiting for scene");
  const status = text("span", "CONNECTING");
  identity.append(title, status);
  const actions = element("div", "mechanics-map-overlay-actions");
  const rotate = button("Rotate", preferences.rotateWithPlayer, () => {
    preferences.rotateWithPlayer = !preferences.rotateWithPlayer;
    rotate.dataset.active = String(preferences.rotateWithPlayer);
    savePreferences();
    scheduleDraw();
  });
  const monsters = button("Mobs", preferences.showMonsters, () => {
    preferences.showMonsters = !preferences.showMonsters;
    monsters.dataset.active = String(preferences.showMonsters);
    savePreferences();
    scheduleDraw();
  });
  const reset = button("Reset", false, () => {
    preferences.scale = 1;
    preferences.panX = 0;
    preferences.panY = 0;
    savePreferences();
    scheduleDraw();
  });
  const lock = button(preferences.locked ? "Unlock" : "Lock", preferences.locked, () => {
    void setLocked(!preferences.locked);
  });
  const hide = button("Hide", false, () => { void dependencies.hide(); });
  actions.append(rotate, monsters, reset, lock, hide);
  toolbar.append(identity, actions);

  const viewport = element("section", "mechanics-map-overlay-viewport");
  const canvas = document.createElement("canvas");
  canvas.className = "mechanics-map-overlay-canvas";
  canvas.setAttribute("aria-label", "Live packet-observed Mechanics Map canvas");
  const notice = text("p", "Waiting for packet-observed position…", "mechanics-map-overlay-notice");
  viewport.append(canvas, notice);
  const resize = element("button", "mechanics-map-overlay-resize");
  resize.type = "button";
  resize.title = "Resize Mechanics Map overlay";
  resize.setAttribute("aria-label", "Resize Mechanics Map overlay");
  resize.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    moduleResize = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      width: preferences.moduleWidth, height: preferences.moduleHeight,
    };
    resize.setPointerCapture(event.pointerId);
  });
  panel.append(toolbar, viewport, resize);
  const playerPanel = element("section", "player-frame-overlay-runtime");
  const playerToolbar = element("header", "player-frame-overlay-toolbar");
  const playerTitle = text("strong", "Player");
  const playerStatus = text("span", "WAITING");
  playerToolbar.append(playerTitle, playerStatus);
  const playerBody = element("section", "player-frame-overlay-body");
  const playerResizeHandle = element("button", "player-frame-overlay-resize");
  playerResizeHandle.type = "button";
  playerResizeHandle.title = "Resize player frame";
  playerResizeHandle.setAttribute("aria-label", "Resize player frame");
  playerPanel.append(playerToolbar, playerBody, playerResizeHandle);
  const actionsPanel = element("section", "action-controls-overlay-runtime");
  const actionsToolbar = element("header", "action-controls-overlay-toolbar");
  const actionsTitle = text("strong", "Action cooldowns");
  const actionsStatus = text("span", "WAITING");
  actionsToolbar.append(actionsTitle, actionsStatus);
  const actionsBody = element("section", "action-controls-overlay-body");
  const actionsResizeHandle = element("button", "action-controls-overlay-resize");
  actionsResizeHandle.type = "button";
  actionsResizeHandle.title = "Resize action cooldowns";
  actionsResizeHandle.setAttribute("aria-label", "Resize action cooldowns");
  actionsPanel.append(actionsToolbar, actionsBody, actionsResizeHandle);
  const partyPanel = element("section", "party-frame-overlay-runtime");
  const partyToolbar = element("header", "party-frame-overlay-toolbar");
  const partyTitle = text("strong", "Party");
  const partyStatus = text("span", "WAITING");
  partyToolbar.append(partyTitle, partyStatus);
  const partyBody = element("section", "party-frame-overlay-body");
  const partyResizeHandle = element("button", "party-frame-overlay-resize");
  partyResizeHandle.type = "button";
  partyResizeHandle.title = "Resize party frames";
  partyResizeHandle.setAttribute("aria-label", "Resize party frames");
  partyPanel.append(partyToolbar, partyBody, partyResizeHandle);
  const targetPanel = element("section", "target-frame-overlay-runtime");
  const targetToolbar = element("header", "target-frame-overlay-toolbar");
  const targetTitle = text("strong", "Current target");
  const targetStatus = text("span", "NO TARGET");
  targetToolbar.append(targetTitle, targetStatus);
  const targetBody = element("section", "target-frame-overlay-body");
  const targetResizeHandle = element("button", "target-frame-overlay-resize");
  targetResizeHandle.type = "button";
  targetResizeHandle.title = "Resize target frame";
  targetResizeHandle.setAttribute("aria-label", "Resize target frame");
  targetPanel.append(targetToolbar, targetBody, targetResizeHandle);
  const objectivesPanel = element("section", "dungeon-objectives-overlay-runtime");
  const objectivesToolbar = element("header", "dungeon-objectives-overlay-toolbar");
  const objectivesTitle = text("strong", "Dungeon objectives");
  const objectivesStatus = text("span", "WAITING");
  objectivesToolbar.append(objectivesTitle, objectivesStatus);
  const objectivesBody = element("section", "dungeon-objectives-overlay-body");
  const objectivesResizeHandle = element("button", "dungeon-objectives-overlay-resize");
  objectivesResizeHandle.type = "button";
  objectivesResizeHandle.title = "Resize dungeon objectives";
  objectivesResizeHandle.setAttribute("aria-label", "Resize dungeon objectives");
  objectivesPanel.append(objectivesToolbar, objectivesBody, objectivesResizeHandle);
  const alertsPanel = element("section", "mechanic-alerts-overlay-runtime");
  const alertsToolbar = element("header", "mechanic-alerts-overlay-toolbar");
  const alertsTitle = text("strong", "Mechanic alerts");
  const alertsStatus = text("span", "WAITING");
  alertsToolbar.append(alertsTitle, alertsStatus);
  const alertsBody = element("section", "mechanic-alerts-overlay-body");
  const alertsResizeHandle = element("button", "mechanic-alerts-overlay-resize");
  alertsResizeHandle.type = "button";
  alertsResizeHandle.title = "Resize mechanic alerts";
  alertsResizeHandle.setAttribute("aria-label", "Resize mechanic alerts");
  alertsPanel.append(alertsToolbar, alertsBody, alertsResizeHandle);
  root.append(panel, playerPanel, actionsPanel, partyPanel, targetPanel, objectivesPanel, alertsPanel);
  container.replaceChildren(root);
  applyModuleGeometry();

  toolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || (event.target as Element).closest("button")) return;
    moduleDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.moduleX, top: preferences.moduleY,
    };
    toolbar.setPointerCapture(event.pointerId);
  });
  toolbar.addEventListener("pointermove", moveModule);
  toolbar.addEventListener("pointerup", endModuleDrag);
  toolbar.addEventListener("pointercancel", endModuleDrag);
  resize.addEventListener("pointermove", resizeModule);
  resize.addEventListener("pointerup", endModuleResize);
  resize.addEventListener("pointercancel", endModuleResize);
  playerToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    playerDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.playerX, top: preferences.playerY,
    };
    playerToolbar.setPointerCapture(event.pointerId);
  });
  playerToolbar.addEventListener("pointermove", movePlayer);
  playerToolbar.addEventListener("pointerup", endPlayerDrag);
  playerToolbar.addEventListener("pointercancel", endPlayerDrag);
  playerResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    playerResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.playerWidth };
    playerResizeHandle.setPointerCapture(event.pointerId);
  });
  playerResizeHandle.addEventListener("pointermove", resizePlayer);
  playerResizeHandle.addEventListener("pointerup", endPlayerResize);
  playerResizeHandle.addEventListener("pointercancel", endPlayerResize);
  actionsToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    actionsDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.actionsX, top: preferences.actionsY,
    };
    actionsToolbar.setPointerCapture(event.pointerId);
  });
  actionsToolbar.addEventListener("pointermove", moveActions);
  actionsToolbar.addEventListener("pointerup", endActionsDrag);
  actionsToolbar.addEventListener("pointercancel", endActionsDrag);
  actionsResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    actionsResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.actionsWidth };
    actionsResizeHandle.setPointerCapture(event.pointerId);
  });
  actionsResizeHandle.addEventListener("pointermove", resizeActions);
  actionsResizeHandle.addEventListener("pointerup", endActionsResize);
  actionsResizeHandle.addEventListener("pointercancel", endActionsResize);
  partyToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    partyDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.partyX, top: preferences.partyY,
    };
    partyToolbar.setPointerCapture(event.pointerId);
  });
  partyToolbar.addEventListener("pointermove", moveParty);
  partyToolbar.addEventListener("pointerup", endPartyDrag);
  partyToolbar.addEventListener("pointercancel", endPartyDrag);
  partyResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    partyResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.partyWidth };
    partyResizeHandle.setPointerCapture(event.pointerId);
  });
  partyResizeHandle.addEventListener("pointermove", resizeParty);
  partyResizeHandle.addEventListener("pointerup", endPartyResize);
  partyResizeHandle.addEventListener("pointercancel", endPartyResize);
  targetToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    targetDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.targetX, top: preferences.targetY,
    };
    targetToolbar.setPointerCapture(event.pointerId);
  });
  targetToolbar.addEventListener("pointermove", moveTarget);
  targetToolbar.addEventListener("pointerup", endTargetDrag);
  targetToolbar.addEventListener("pointercancel", endTargetDrag);
  targetResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    targetResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.targetWidth };
    targetResizeHandle.setPointerCapture(event.pointerId);
  });
  targetResizeHandle.addEventListener("pointermove", resizeTarget);
  targetResizeHandle.addEventListener("pointerup", endTargetResize);
  targetResizeHandle.addEventListener("pointercancel", endTargetResize);
  objectivesToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    objectivesDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.objectivesX, top: preferences.objectivesY,
    };
    objectivesToolbar.setPointerCapture(event.pointerId);
  });
  objectivesToolbar.addEventListener("pointermove", moveObjectives);
  objectivesToolbar.addEventListener("pointerup", endObjectivesDrag);
  objectivesToolbar.addEventListener("pointercancel", endObjectivesDrag);
  objectivesResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    objectivesResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.objectivesWidth };
    objectivesResizeHandle.setPointerCapture(event.pointerId);
  });
  objectivesResizeHandle.addEventListener("pointermove", resizeObjectives);
  objectivesResizeHandle.addEventListener("pointerup", endObjectivesResize);
  objectivesResizeHandle.addEventListener("pointercancel", endObjectivesResize);
  alertsToolbar.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    alertsDrag = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      left: preferences.alertsX, top: preferences.alertsY,
    };
    alertsToolbar.setPointerCapture(event.pointerId);
  });
  alertsToolbar.addEventListener("pointermove", moveAlerts);
  alertsToolbar.addEventListener("pointerup", endAlertsDrag);
  alertsToolbar.addEventListener("pointercancel", endAlertsDrag);
  alertsResizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    alertsResize = { pointerId: event.pointerId, x: event.clientX, width: preferences.alertsWidth };
    alertsResizeHandle.setPointerCapture(event.pointerId);
  });
  alertsResizeHandle.addEventListener("pointermove", resizeAlerts);
  alertsResizeHandle.addEventListener("pointerup", endAlertsResize);
  alertsResizeHandle.addEventListener("pointercancel", endAlertsResize);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("pointerdown", beginPan);
  canvas.addEventListener("pointermove", continuePan);
  canvas.addEventListener("pointerup", endPan);
  canvas.addEventListener("pointercancel", endPan);
  const resizeObserver = new ResizeObserver(scheduleDraw);
  resizeObserver.observe(viewport);
  const handleScreenResize = (): void => {
    applyModuleGeometry();
    scheduleDraw();
  };
  window.addEventListener("resize", handleScreenResize);

  void dependencies.setInteractive(!preferences.locked);
  void dependencies.onInteractivity((interactive) => {
    if (interactive && preferences.locked) void setLocked(false);
  }).then((remove) => { removeInteractivityListener = remove; });
  void dependencies.onFocusHeld((held) => {
    root.dataset.focusHeld = String(held);
    actionsPanel.dataset.focused = String(held);
  }).then((remove) => { removeFocusHeldListener = remove; });
  void connect();

  async function connect(): Promise<void> {
    try {
      update = await dependencies.loadSnapshot();
      if (!alive) return;
      renderState();
      while (alive) {
        const next = await dependencies.waitForSnapshot(update.revision);
        if (!alive) return;
        if (!shouldRenderMechanicsMapUpdate(update, next)) continue;
        update = next;
        renderState();
      }
    } catch (cause) {
      if (!alive) return;
      status.textContent = "UNAVAILABLE";
      status.dataset.state = "error";
      notice.hidden = false;
      notice.textContent = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function renderState(): void {
    const snapshot = update?.snapshot;
    if (!snapshot) return;
    title.textContent = snapshot.scene_name ?? (snapshot.scene_id === null ? "Waiting for scene" : `Scene ${snapshot.scene_id}`);
    status.textContent = snapshot.local_position_observed ? "LIVE" : snapshot.scene_id === null ? "WAITING" : "POSITION NEEDED";
    status.dataset.state = snapshot.local_position_observed ? "live" : "waiting";
    notice.hidden = snapshot.local_position_observed && snapshot.data_gap === null;
    notice.textContent = snapshot.data_gap ?? "Waiting for packet-observed position…";
    loadBackground(snapshot.background_asset_url);
    renderPlayer(snapshot);
    renderActions(snapshot);
    renderParty(snapshot);
    renderTarget(snapshot);
    renderDungeonObjectives(snapshot);
    renderMechanicAlerts(snapshot);
    scheduleDraw();
  }

  function renderMechanicAlerts(snapshot: MechanicsMapSnapshot): void {
    stopAlertsTimer();
    alertsBody.replaceChildren();
    const signals = snapshot.mechanics
      .filter((signal) => signal.mechanic_kind !== null)
      .slice(-8)
      .reverse();
    alertsStatus.textContent = signals.length === 0 ? "WAITING" : `${signals.length} OBSERVED`;
    alertsStatus.dataset.state = signals.length === 0 ? "waiting" : "live";
    if (signals.length === 0) {
      alertsBody.append(text("p", "Waiting for reviewed packet mechanic signals…", "mechanic-alerts-overlay-empty"));
      return;
    }
    alertsRenderedAtMillis = performance.now();
    for (const signal of signals) {
      const row = element("article", "mechanic-alerts-overlay-row");
      row.dataset.effectId = String(signal.effect_id);
      row.dataset.durationMillis = signal.duration_millis === null ? "" : String(signal.duration_millis);
      row.dataset.appliedAtMicros = String(signal.applied_at_micros);
      row.dataset.snapshotObservedMicros = snapshot.last_observed_micros === null
        ? ""
        : String(snapshot.last_observed_micros);
      row.title = `Packet effect ID: ${signal.effect_id}`;
      const identity = element("span", "mechanic-alerts-overlay-identity");
      identity.append(
        text("strong", signal.presentation_name ?? humanizeDungeonState(signal.mechanic_kind!)),
        text("small", humanizeDungeonState(signal.mechanic_kind!)),
      );
      const detail = element("span", "mechanic-alerts-overlay-detail");
      if (signal.stacks !== null && signal.stacks > 1) {
        detail.append(text("small", `×${signal.stacks}`, "mechanic-alerts-overlay-stacks"));
      }
      detail.append(text("b", "OBSERVED", "mechanic-alerts-overlay-time"));
      row.append(identity, detail);
      alertsBody.append(row);
    }
    updateAlertTimers();
    if (signals.some((signal) => mechanicSignalRemainingMillis(signal, snapshot.last_observed_micros, 0) !== null)) {
      alertsTimer = window.setInterval(updateAlertTimers, 100);
    }
  }

  function updateAlertTimers(): void {
    const elapsed = performance.now() - alertsRenderedAtMillis;
    let active = false;
    for (const row of alertsBody.querySelectorAll<HTMLElement>(".mechanic-alerts-overlay-row")) {
      const remaining = mechanicSignalRemainingMillis({
        effect_id: Number(row.dataset.effectId),
        duration_millis: row.dataset.durationMillis === "" ? null : Number(row.dataset.durationMillis),
        applied_at_micros: Number(row.dataset.appliedAtMicros),
      }, row.dataset.snapshotObservedMicros === "" ? null : Number(row.dataset.snapshotObservedMicros), elapsed);
      const timer = row.querySelector<HTMLElement>(".mechanic-alerts-overlay-time");
      if (timer && remaining !== null) timer.textContent = `${(remaining / 1_000).toFixed(1)}s`;
      row.dataset.expired = String(remaining === 0);
      active ||= (remaining ?? 0) > 0;
    }
    if (!active) stopAlertsTimer();
  }

  function renderDungeonObjectives(snapshot: MechanicsMapSnapshot): void {
    stopObjectivesTimer();
    const dungeon = snapshot.dungeon;
    objectivesBody.replaceChildren();
    if (dungeon === null) {
      objectivesTitle.textContent = "Dungeon objectives";
      objectivesStatus.textContent = "WAITING";
      objectivesStatus.dataset.state = "waiting";
      objectivesBody.append(text("p", "Waiting for packet-observed dungeon objectives…", "dungeon-objectives-overlay-empty"));
      return;
    }
    objectivesTitle.textContent = snapshot.scene_name ??
      (dungeon.dungeon_id === null ? "Dungeon objectives" : `Dungeon ${dungeon.dungeon_id}`);
    objectivesStatus.textContent = humanizeDungeonState(dungeon.encounter_state ?? dungeon.flow_phase ?? dungeon.state);
    objectivesStatus.dataset.state = dungeon.encounter_state === "wiped" || dungeon.state === "failed"
      ? "failed"
      : dungeon.encounter_state === "cleared" || dungeon.state === "completed" ? "complete" : "live";
    const metadata = [
      dungeon.dungeon_id === null ? null : `Dungeon ${dungeon.dungeon_id}`,
      dungeon.difficulty_id === null ? null : `Difficulty ${dungeon.difficulty_id}`,
    ].filter((value): value is string => value !== null).join(" · ");
    if (metadata) objectivesBody.append(text("small", metadata, "dungeon-objectives-overlay-meta"));
    if (dungeon.attempt_number > 0) {
      const attempt = element("div", "dungeon-objectives-overlay-attempt");
      const label = text("span", `Attempt ${dungeon.attempt_number}`);
      const timer = text("b", formatDungeonAttemptTime(dungeon.attempt_elapsed_micros));
      const retries = text("small", `${dungeon.retry_count} ${dungeon.retry_count === 1 ? "retry" : "retries"}`);
      attempt.append(label, timer, retries);
      objectivesBody.append(attempt);
      objectivesRenderedAtMillis = performance.now();
      const updateAttemptTimer = () => {
        const localElapsedMillis = dungeon.attempt_running
          ? Math.max(0, performance.now() - objectivesRenderedAtMillis)
          : 0;
        timer.textContent = formatDungeonAttemptTime(
          dungeon.attempt_elapsed_micros + Math.round(localElapsedMillis * 1_000),
        );
      };
      if (dungeon.attempt_running) objectivesTimer = window.setInterval(updateAttemptTimer, 100);
    }
    if (dungeon.objectives.length === 0) {
      objectivesBody.append(text("p", "No packet-observed objectives yet.", "dungeon-objectives-overlay-empty"));
      return;
    }
    for (const objective of dungeon.objectives) {
      const row = element("article", "dungeon-objectives-overlay-row");
      row.dataset.complete = String(objective.complete === true);
      row.dataset.catalogResolution = objective.catalog_resolution;
      row.title = [
        `Packet objective ID: ${objective.objective_id}`,
        objective.objective_map_key === null ? null : `Map key: ${objective.objective_map_key}`,
        `Catalog: ${humanizeDungeonState(objective.catalog_resolution)}`,
      ].filter((value): value is string => value !== null).join("\n");
      const identity = element("span", "dungeon-objectives-overlay-identity");
      identity.append(
        text("strong", objective.presentation_name ?? objective.activity_target_key ?? `Objective ${objective.objective_id}`),
        text("small", objective.complete === true ? "COMPLETE" : "PACKET OBSERVED"),
      );
      row.append(identity, text("b", formatDungeonObjectiveValue(
        objective.value,
        objective.complete,
        objective.required_count,
      )));
      if (objective.value !== null && objective.required_count !== null && objective.required_count > 0) {
        const progress = document.createElement("progress");
        progress.className = "dungeon-objectives-overlay-progress";
        progress.max = objective.required_count;
        progress.value = Math.max(0, Math.min(objective.value, objective.required_count));
        row.append(progress);
      }
      objectivesBody.append(row);
    }
  }

  function renderActions(snapshot: MechanicsMapSnapshot): void {
    stopActionsTimer();
    actionsBody.replaceChildren();
    actionsStatus.textContent = snapshot.action_controls.length === 0
      ? "WAITING"
      : `${snapshot.action_controls.length} OBSERVED`;
    actionsStatus.dataset.state = snapshot.action_controls.length === 0 ? "waiting" : "live";
    if (snapshot.action_controls.length === 0) {
      actionsBody.append(text("p", "Waiting for packet-observed cooldown state…", "action-controls-overlay-empty"));
      return;
    }
    for (const control of snapshot.action_controls) {
      const entry = element("span", "action-controls-overlay-entry");
      entry.dataset.remainingMillis = control.remaining_millis === null ? "" : String(control.remaining_millis);
      entry.dataset.durationMillis = control.duration_millis === null ? "" : String(control.duration_millis);
      entry.title = `${control.presentation_name ?? `Skill ${control.skill_level_id}`}\nPacket SkillLevel ID: ${control.skill_level_id}`;
      const icon = element("span", "action-controls-overlay-icon");
      if (control.icon_asset_path) {
        const image = document.createElement("img");
        image.src = control.icon_asset_path;
        image.alt = "";
        icon.append(image);
      } else {
        icon.append(text("span", "?"));
      }
      icon.append(element("span", "action-controls-overlay-sweep"));
      const timer = text("b", formatActionRemaining(control.remaining_millis), "action-controls-overlay-time");
      icon.append(timer);
      if (control.charge_count !== null) {
        icon.append(text("small", String(control.charge_count), "action-controls-overlay-charges"));
      }
      const label = text("small", control.presentation_name ?? `Skill ${control.skill_level_id}`, "action-controls-overlay-label");
      entry.append(icon, label);
      actionsBody.append(entry);
    }
    actionsRenderedAtMillis = performance.now();
    updateActionTimers();
    if (snapshot.action_controls.some((control) => (control.remaining_millis ?? 0) > 0)) {
      actionsTimer = window.setInterval(updateActionTimers, 100);
    }
  }

  function updateActionTimers(): void {
    const elapsed = performance.now() - actionsRenderedAtMillis;
    let active = false;
    for (const entry of actionsBody.querySelectorAll<HTMLElement>(".action-controls-overlay-entry")) {
      const base = entry.dataset.remainingMillis === "" ? null : Number(entry.dataset.remainingMillis);
      const remaining = actionControlRemainingMillis({ remaining_millis: base }, elapsed);
      const timer = entry.querySelector<HTMLElement>(".action-controls-overlay-time");
      if (timer) timer.textContent = formatActionRemaining(remaining);
      const duration = entry.dataset.durationMillis === "" ? null : Number(entry.dataset.durationMillis);
      const ratio = remaining !== null && duration !== null && duration > 0
        ? Math.min(1, remaining / duration)
        : 0;
      entry.style.setProperty("--cooldown-turn", `${ratio}turn`);
      entry.dataset.ready = String(remaining === 0);
      active ||= (remaining ?? 0) > 0;
    }
    if (!active) stopActionsTimer();
  }

  function stopActionsTimer(): void {
    if (actionsTimer === null) return;
    window.clearInterval(actionsTimer);
    actionsTimer = null;
  }

  function stopObjectivesTimer(): void {
    if (objectivesTimer === null) return;
    window.clearInterval(objectivesTimer);
    objectivesTimer = null;
  }

  function stopAlertsTimer(): void {
    if (alertsTimer === null) return;
    window.clearInterval(alertsTimer);
    alertsTimer = null;
  }

  function renderParty(snapshot: MechanicsMapSnapshot): void {
    partyBody.replaceChildren();
    partyStatus.textContent = snapshot.party.length === 0 ? "WAITING" : `${snapshot.party.length} JOINED`;
    partyStatus.dataset.state = snapshot.party.length === 0 ? "waiting" : "live";
    if (snapshot.party.length === 0) {
      partyBody.append(text("p", "Waiting for rostered party actors…", "party-frame-overlay-empty"));
      return;
    }
    for (const member of snapshot.party) {
      const row = element("article", "party-frame-overlay-member");
      row.dataset.dead = String(member.dead);
      row.dataset.stale = String(member.stale);
      const identity = element("div", "party-frame-overlay-identity");
      identity.append(
        text("strong", member.display_name ?? `Player ${member.actor_id}`),
        text("span", member.dead ? "DEFEATED" : member.stale ? "STALE" : formatTargetHealth(member.current_hp, member.max_hp)),
      );
      const health = element("div", "party-frame-overlay-health");
      health.dataset.observed = String(member.hp_percent !== null);
      const healthFill = element("span");
      healthFill.style.width = `${member.hp_percent ?? 0}%`;
      health.append(healthFill);
      row.append(identity, health);
      if (member.current_shield !== null || member.max_shield !== null) {
        const shield = element("div", "party-frame-overlay-shield");
        shield.dataset.observed = String(member.shield_percent !== null);
        const shieldFill = element("span");
        shieldFill.style.width = `${member.shield_percent ?? 0}%`;
        shield.append(shieldFill);
        row.append(shield);
      }
      partyBody.append(row);
    }
  }

  function renderPlayer(snapshot: MechanicsMapSnapshot): void {
    stopPlayerTimer();
    const player = snapshot.player;
    playerPanel.dataset.stale = String(player?.stale ?? false);
    if (player === null) {
      playerTitle.textContent = "Player";
      playerStatus.textContent = "WAITING";
      playerStatus.dataset.state = "waiting";
      playerBody.replaceChildren(text("p", "Waiting for packet-observed player vitals…", "player-frame-overlay-empty"));
      return;
    }
    playerTitle.textContent = player.display_name ?? `Player ${player.actor_id}`;
    playerStatus.textContent = player.dead ? "DEFEATED" : player.stale ? "STALE" : "LIVE";
    playerStatus.dataset.state = player.dead ? "dead" : player.stale ? "waiting" : "live";
    const identity = element("div", "player-frame-overlay-identity");
    identity.append(
      text("strong", "HP"),
      text("span", formatTargetHealth(player.current_hp, player.max_hp)),
    );
    const vitals = element("div", "player-frame-overlay-vitals");
    const health = element("div", "player-frame-overlay-health");
    const healthFill = element("span");
    healthFill.style.width = `${player.hp_percent ?? 0}%`;
    health.dataset.observed = String(player.hp_percent !== null);
    health.append(healthFill);
    vitals.append(health);
    if (player.current_shield !== null) {
      const shield = element("div", "player-frame-overlay-shield");
      const shieldFill = element("span");
      shieldFill.style.width = `${player.shield_percent ?? (player.current_shield > 0 ? 100 : 0)}%`;
      shield.dataset.observed = String(player.shield_percent !== null);
      shield.title = `Shield ${formatTargetHealth(player.current_shield, player.max_shield)}`;
      shield.append(shieldFill);
      vitals.append(
        shield,
        text("small", `SHIELD ${formatTargetHealth(player.current_shield, player.max_shield)}`, "player-frame-overlay-shield-label"),
      );
    }
    const statuses = element("div", "player-frame-overlay-statuses");
    statuses.setAttribute("aria-label", "Player status effects");
    for (const effect of player.statuses) {
      const entry = element("span", "player-frame-overlay-status-entry");
      const item = element("span", "player-frame-overlay-status");
      const effectName = effect.presentation_name ?? `Effect ${effect.effect_id}`;
      const sourceName = effect.source_display_name ??
        (effect.source_actor_id === null ? "not supplied" : "unresolved actor");
      entry.title = `${effectName}\nSource: ${sourceName}`;
      if (effect.icon_asset_path) {
        const icon = document.createElement("img");
        icon.src = effect.icon_asset_path;
        icon.alt = "";
        item.append(icon);
      } else {
        item.append(text("span", "?"));
      }
      if ((effect.stacks ?? 0) > 1) item.append(text("b", String(effect.stacks)));
      if (effect.remaining_millis !== null) {
        const timer = text("span", formatDebuffRemaining(effect.remaining_millis), "player-frame-overlay-status-time");
        timer.dataset.playerStatusRemaining = String(effect.remaining_millis);
        item.append(timer);
      }
      entry.append(item);
      statuses.append(entry);
    }
    playerBody.replaceChildren(identity, vitals, statuses);
    playerRenderedAtMillis = performance.now();
    updatePlayerTimers();
    if (player.statuses.some((effect) => effect.remaining_millis !== null)) {
      playerTimer = window.setInterval(updatePlayerTimers, 100);
    }
  }

  function updatePlayerTimers(): void {
    const elapsed = performance.now() - playerRenderedAtMillis;
    for (const entry of playerBody.querySelectorAll<HTMLElement>(".player-frame-overlay-status-entry")) {
      const timer = entry.querySelector<HTMLElement>("[data-player-status-remaining]");
      if (timer === null) continue;
      const base = Number(timer.dataset.playerStatusRemaining);
      const remaining = targetDebuffRemainingMillis({ remaining_millis: base }, elapsed) ?? 0;
      entry.hidden = remaining <= 0;
      timer.textContent = formatDebuffRemaining(remaining);
    }
  }

  function stopPlayerTimer(): void {
    if (playerTimer === null) return;
    window.clearInterval(playerTimer);
    playerTimer = null;
  }

  function renderTarget(snapshot: MechanicsMapSnapshot): void {
    stopTargetTimer();
    const target = snapshot.target;
    targetPanel.dataset.stale = String(target?.stale ?? false);
    if (target === null) {
      targetStatus.textContent = "NO TARGET";
      targetStatus.dataset.state = "waiting";
      targetBody.replaceChildren(text("p", "Select a target in game.", "target-frame-overlay-empty"));
      return;
    }
    targetStatus.textContent = target.dead ? "DEFEATED" : target.stale ? "STALE" : "LIVE";
    targetStatus.dataset.state = target.dead ? "dead" : target.stale ? "waiting" : "live";
    const identity = element("div", "target-frame-overlay-identity");
    const name = text("strong", target.display_name ?? (target.monster_id === null ? "Unknown target" : `Monster ${target.monster_id}`));
    const health = text("span", formatTargetHealth(target.current_hp, target.max_hp));
    identity.append(name, health);
    const track = element("div", "target-frame-overlay-health");
    const fill = element("span");
    fill.style.width = `${target.hp_percent ?? 0}%`;
    track.dataset.observed = String(target.hp_percent !== null);
    track.append(fill);
    const vitals = element("div", "target-frame-overlay-vitals");
    vitals.append(track);
    if (target.current_shield !== null) {
      const shield = element("div", "target-frame-overlay-shield");
      const shieldFill = element("span");
      shieldFill.style.width = `${target.shield_percent ?? (target.current_shield > 0 ? 100 : 0)}%`;
      shield.dataset.observed = String(target.shield_percent !== null);
      shield.title = `Shield ${formatTargetHealth(target.current_shield, target.max_shield)}`;
      shield.append(shieldFill);
      const shieldLabel = text("small", `SHIELD ${formatTargetHealth(target.current_shield, target.max_shield)}`, "target-frame-overlay-shield-label");
      vitals.append(shield, shieldLabel);
    }
    if (target.breaking_stage !== null) {
      const breaking = text("small", formatBreakingStage(target.breaking_stage), "target-frame-overlay-breaking");
      breaking.dataset.stage = String(target.breaking_stage);
      vitals.append(breaking);
    }
    const debuffs = element("div", "target-frame-overlay-debuffs");
    debuffs.setAttribute("aria-label", "Target debuffs");
    for (const effect of target.debuffs) {
      const entry = element("span", "target-frame-overlay-debuff-entry");
      const item = element("span", "target-frame-overlay-debuff");
      const effectName = effect.presentation_name ?? `Effect ${effect.effect_id}`;
      const sourceName = effect.source_display_name ?? (effect.source_actor_id === null ? "not supplied" : "unresolved actor");
      entry.title = `${effectName}\nSource: ${sourceName}`;
      if (effect.icon_asset_path) {
        const icon = document.createElement("img");
        icon.src = effect.icon_asset_path;
        icon.alt = "";
        item.append(icon);
      } else {
        item.append(text("span", "?"));
      }
      if ((effect.stacks ?? 0) > 1) item.append(text("b", String(effect.stacks)));
      if (effect.remaining_millis !== null) {
        const timer = text("span", formatDebuffRemaining(effect.remaining_millis), "target-frame-overlay-debuff-time");
        timer.dataset.targetDebuffRemaining = String(effect.remaining_millis);
        item.append(timer);
      }
      const owner = text("small", effect.source_display_name ?? "—", "target-frame-overlay-debuff-owner");
      entry.append(item, owner);
      debuffs.append(entry);
    }
    const noDebuffs = text("span", "No packet-classified debuffs", "target-frame-overlay-no-debuffs");
    noDebuffs.hidden = target.debuffs.length > 0;
    debuffs.append(noDebuffs);
    targetBody.replaceChildren(identity, vitals, debuffs);
    targetRenderedAtMillis = performance.now();
    updateTargetTimers();
    if (target.debuffs.some((effect) => effect.remaining_millis !== null)) {
      targetTimer = window.setInterval(updateTargetTimers, 100);
    }
  }

  function formatBreakingStage(stage: number): string {
    if (stage === 0) return "BREAKING";
    if (stage === 1) return "BREAK ENDED";
    return `BREAK STAGE ${stage}`;
  }

  function updateTargetTimers(): void {
    const elapsed = performance.now() - targetRenderedAtMillis;
    let visible = 0;
    for (const entry of targetBody.querySelectorAll<HTMLElement>(".target-frame-overlay-debuff-entry")) {
      const timer = entry.querySelector<HTMLElement>("[data-target-debuff-remaining]");
      if (timer === null) {
        entry.hidden = false;
        visible += 1;
        continue;
      }
      const base = Number(timer.dataset.targetDebuffRemaining);
      const remaining = targetDebuffRemainingMillis({ remaining_millis: base }, elapsed) ?? 0;
      entry.hidden = remaining <= 0;
      if (!entry.hidden) visible += 1;
      timer.textContent = formatDebuffRemaining(remaining);
    }
    const empty = targetBody.querySelector<HTMLElement>(".target-frame-overlay-no-debuffs");
    if (empty) empty.hidden = visible > 0;
  }

  function stopTargetTimer(): void {
    if (targetTimer === null) return;
    window.clearInterval(targetTimer);
    targetTimer = null;
  }

  function loadBackground(url: string | null): void {
    if (url === imageUrl) return;
    imageUrl = url;
    imageReady = false;
    image = null;
    if (url === null) return;
    const next = new Image();
    next.decoding = "async";
    next.onload = () => {
      if (!alive || imageUrl !== url) return;
      image = next;
      imageReady = true;
      scheduleDraw();
    };
    next.onerror = () => {
      if (!alive || imageUrl !== url || preparingAsset) return;
      preparingAsset = true;
      void dependencies.prepareLocalMaps().then(() => {
        if (!alive || imageUrl !== url) return;
        imageUrl = null;
        loadBackground(url);
      }).catch(() => undefined).finally(() => { preparingAsset = false; });
    };
    next.src = url;
  }

  function scheduleDraw(): void {
    if (!alive || frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      draw();
    });
  }

  function draw(): void {
    const snapshot = update?.snapshot;
    const rect = viewport.getBoundingClientRect();
    if (!snapshot || rect.width <= 0 || rect.height <= 0) return;
    const dpr = Math.max(1, window.devicePixelRatio || 1);
    const width = Math.max(1, Math.round(rect.width));
    const height = Math.max(1, Math.round(rect.height));
    const pixelWidth = Math.round(width * dpr);
    const pixelHeight = Math.round(height * dpr);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    const context = canvas.getContext("2d");
    if (!context) return;
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    context.clearRect(0, 0, width, height);
    context.save();
    context.translate(width / 2 + preferences.panX, height / 2 + preferences.panY);
    context.scale(preferences.scale, preferences.scale);
    context.translate(-width / 2, -height / 2);
    drawBackdrop(context, snapshot, width, height, imageReady ? image : null);
    drawArena(context, snapshot, width, height);
    drawRegions(context, snapshot, width, height);
    drawEntities(context, snapshot, width, height, preferences);
    context.restore();
  }

  function onWheel(event: WheelEvent): void {
    event.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const next = zoomMechanicsMapAt(
      preferences,
      event.clientX - rect.left - rect.width / 2,
      event.clientY - rect.top - rect.height / 2,
      event.deltaY,
    );
    preferences.scale = next.scale;
    preferences.panX = next.panX;
    preferences.panY = next.panY;
    savePreferences();
    scheduleDraw();
  }

  function beginPan(event: PointerEvent): void {
    if (event.button !== 0) return;
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    canvas.setPointerCapture(event.pointerId);
    root.dataset.panning = "true";
  }

  function continuePan(event: PointerEvent): void {
    if (drag?.pointerId !== event.pointerId) return;
    preferences.panX += event.clientX - drag.x;
    preferences.panY += event.clientY - drag.y;
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    savePreferences();
    scheduleDraw();
  }

  function endPan(event: PointerEvent): void {
    if (drag?.pointerId !== event.pointerId) return;
    drag = null;
    delete root.dataset.panning;
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
  }

  async function setLocked(value: boolean): Promise<void> {
    preferences.locked = value;
    panel.dataset.locked = String(value);
    lock.textContent = value ? "Unlock" : "Lock";
    lock.dataset.active = String(value);
    savePreferences();
    await dependencies.setInteractive(!value);
  }

  function savePreferences(): void {
    localStorage.setItem(PREFERENCES_KEY, JSON.stringify(preferences));
  }

  function applyModuleGeometry(): void {
    const maximumX = Math.max(0, window.innerWidth - preferences.moduleWidth);
    const maximumY = Math.max(0, window.innerHeight - preferences.moduleHeight);
    preferences.moduleX = Math.min(maximumX, Math.max(0, preferences.moduleX));
    preferences.moduleY = Math.min(maximumY, Math.max(0, preferences.moduleY));
    panel.style.left = `${preferences.moduleX}px`;
    panel.style.top = `${preferences.moduleY}px`;
    panel.style.width = `${Math.min(window.innerWidth, preferences.moduleWidth)}px`;
    panel.style.height = `${Math.min(window.innerHeight, preferences.moduleHeight)}px`;
    const targetWidth = Math.min(window.innerWidth, preferences.targetWidth);
    preferences.targetX = Math.min(Math.max(0, window.innerWidth - targetWidth), Math.max(0, preferences.targetX));
    preferences.targetY = Math.min(Math.max(0, window.innerHeight - 96), Math.max(0, preferences.targetY));
    targetPanel.style.left = `${preferences.targetX}px`;
    targetPanel.style.top = `${preferences.targetY}px`;
    targetPanel.style.width = `${targetWidth}px`;
    const playerWidth = Math.min(window.innerWidth, preferences.playerWidth);
    preferences.playerX = Math.min(Math.max(0, window.innerWidth - playerWidth), Math.max(0, preferences.playerX));
    preferences.playerY = Math.min(Math.max(0, window.innerHeight - 80), Math.max(0, preferences.playerY));
    playerPanel.style.left = `${preferences.playerX}px`;
    playerPanel.style.top = `${preferences.playerY}px`;
    playerPanel.style.width = `${playerWidth}px`;
    const actionsWidth = Math.min(window.innerWidth, preferences.actionsWidth);
    preferences.actionsX = Math.min(Math.max(0, window.innerWidth - actionsWidth), Math.max(0, preferences.actionsX));
    preferences.actionsY = Math.min(Math.max(0, window.innerHeight - 96), Math.max(0, preferences.actionsY));
    actionsPanel.style.left = `${preferences.actionsX}px`;
    actionsPanel.style.top = `${preferences.actionsY}px`;
    actionsPanel.style.width = `${actionsWidth}px`;
    const actionsHorizontalAnchor = preferences.actionsX + actionsWidth / 2 > window.innerWidth / 2 ? "right" : "left";
    const actionsVerticalAnchor = preferences.actionsY + actionsPanel.offsetHeight / 2 > window.innerHeight / 2 ? "bottom" : "top";
    actionsPanel.style.transformOrigin = `${actionsHorizontalAnchor} ${actionsVerticalAnchor}`;
    const partyWidth = Math.min(window.innerWidth, preferences.partyWidth);
    preferences.partyX = Math.min(Math.max(0, window.innerWidth - partyWidth), Math.max(0, preferences.partyX));
    preferences.partyY = Math.min(Math.max(0, window.innerHeight - 96), Math.max(0, preferences.partyY));
    partyPanel.style.left = `${preferences.partyX}px`;
    partyPanel.style.top = `${preferences.partyY}px`;
    partyPanel.style.width = `${partyWidth}px`;
    const objectivesWidth = Math.min(window.innerWidth, preferences.objectivesWidth);
    preferences.objectivesX = Math.min(Math.max(0, window.innerWidth - objectivesWidth), Math.max(0, preferences.objectivesX));
    preferences.objectivesY = Math.min(Math.max(0, window.innerHeight - 96), Math.max(0, preferences.objectivesY));
    objectivesPanel.style.left = `${preferences.objectivesX}px`;
    objectivesPanel.style.top = `${preferences.objectivesY}px`;
    objectivesPanel.style.width = `${objectivesWidth}px`;
    const alertsWidth = Math.min(window.innerWidth, preferences.alertsWidth);
    preferences.alertsX = Math.min(Math.max(0, window.innerWidth - alertsWidth), Math.max(0, preferences.alertsX));
    preferences.alertsY = Math.min(Math.max(0, window.innerHeight - 96), Math.max(0, preferences.alertsY));
    alertsPanel.style.left = `${preferences.alertsX}px`;
    alertsPanel.style.top = `${preferences.alertsY}px`;
    alertsPanel.style.width = `${alertsWidth}px`;
  }

  function moveModule(event: PointerEvent): void {
    if (moduleDrag?.pointerId !== event.pointerId) return;
    preferences.moduleX = moduleDrag.left + event.clientX - moduleDrag.x;
    preferences.moduleY = moduleDrag.top + event.clientY - moduleDrag.y;
    applyModuleGeometry();
  }

  function endModuleDrag(event: PointerEvent): void {
    if (moduleDrag?.pointerId !== event.pointerId) return;
    moduleDrag = null;
    savePreferences();
  }

  function resizeModule(event: PointerEvent): void {
    if (moduleResize?.pointerId !== event.pointerId) return;
    preferences.moduleWidth = Math.max(260, moduleResize.width + event.clientX - moduleResize.x);
    preferences.moduleHeight = Math.max(260, moduleResize.height + event.clientY - moduleResize.y);
    applyModuleGeometry();
  }

  function endModuleResize(event: PointerEvent): void {
    if (moduleResize?.pointerId !== event.pointerId) return;
    moduleResize = null;
    savePreferences();
  }

  function movePlayer(event: PointerEvent): void {
    if (playerDrag?.pointerId !== event.pointerId) return;
    preferences.playerX = playerDrag.left + event.clientX - playerDrag.x;
    preferences.playerY = playerDrag.top + event.clientY - playerDrag.y;
    applyModuleGeometry();
  }

  function endPlayerDrag(event: PointerEvent): void {
    if (playerDrag?.pointerId !== event.pointerId) return;
    playerDrag = null;
    savePreferences();
  }

  function resizePlayer(event: PointerEvent): void {
    if (playerResize?.pointerId !== event.pointerId) return;
    preferences.playerWidth = Math.max(280, playerResize.width + event.clientX - playerResize.x);
    applyModuleGeometry();
  }

  function endPlayerResize(event: PointerEvent): void {
    if (playerResize?.pointerId !== event.pointerId) return;
    playerResize = null;
    savePreferences();
  }

  function moveActions(event: PointerEvent): void {
    if (actionsDrag?.pointerId !== event.pointerId) return;
    preferences.actionsX = actionsDrag.left + event.clientX - actionsDrag.x;
    preferences.actionsY = actionsDrag.top + event.clientY - actionsDrag.y;
    applyModuleGeometry();
  }

  function endActionsDrag(event: PointerEvent): void {
    if (actionsDrag?.pointerId !== event.pointerId) return;
    actionsDrag = null;
    savePreferences();
  }

  function resizeActions(event: PointerEvent): void {
    if (actionsResize?.pointerId !== event.pointerId) return;
    preferences.actionsWidth = Math.max(280, actionsResize.width + event.clientX - actionsResize.x);
    applyModuleGeometry();
  }

  function endActionsResize(event: PointerEvent): void {
    if (actionsResize?.pointerId !== event.pointerId) return;
    actionsResize = null;
    savePreferences();
  }

  function moveParty(event: PointerEvent): void {
    if (partyDrag?.pointerId !== event.pointerId) return;
    preferences.partyX = partyDrag.left + event.clientX - partyDrag.x;
    preferences.partyY = partyDrag.top + event.clientY - partyDrag.y;
    applyModuleGeometry();
  }

  function endPartyDrag(event: PointerEvent): void {
    if (partyDrag?.pointerId !== event.pointerId) return;
    partyDrag = null;
    savePreferences();
  }

  function resizeParty(event: PointerEvent): void {
    if (partyResize?.pointerId !== event.pointerId) return;
    preferences.partyWidth = Math.max(260, partyResize.width + event.clientX - partyResize.x);
    applyModuleGeometry();
  }

  function endPartyResize(event: PointerEvent): void {
    if (partyResize?.pointerId !== event.pointerId) return;
    partyResize = null;
    savePreferences();
  }

  function moveTarget(event: PointerEvent): void {
    if (targetDrag?.pointerId !== event.pointerId) return;
    preferences.targetX = targetDrag.left + event.clientX - targetDrag.x;
    preferences.targetY = targetDrag.top + event.clientY - targetDrag.y;
    applyModuleGeometry();
  }

  function endTargetDrag(event: PointerEvent): void {
    if (targetDrag?.pointerId !== event.pointerId) return;
    targetDrag = null;
    savePreferences();
  }

  function resizeTarget(event: PointerEvent): void {
    if (targetResize?.pointerId !== event.pointerId) return;
    preferences.targetWidth = Math.max(280, targetResize.width + event.clientX - targetResize.x);
    applyModuleGeometry();
  }

  function endTargetResize(event: PointerEvent): void {
    if (targetResize?.pointerId !== event.pointerId) return;
    targetResize = null;
    savePreferences();
  }

  function moveObjectives(event: PointerEvent): void {
    if (objectivesDrag?.pointerId !== event.pointerId) return;
    preferences.objectivesX = objectivesDrag.left + event.clientX - objectivesDrag.x;
    preferences.objectivesY = objectivesDrag.top + event.clientY - objectivesDrag.y;
    applyModuleGeometry();
  }

  function endObjectivesDrag(event: PointerEvent): void {
    if (objectivesDrag?.pointerId !== event.pointerId) return;
    objectivesDrag = null;
    savePreferences();
  }

  function resizeObjectives(event: PointerEvent): void {
    if (objectivesResize?.pointerId !== event.pointerId) return;
    preferences.objectivesWidth = Math.max(280, objectivesResize.width + event.clientX - objectivesResize.x);
    applyModuleGeometry();
  }

  function endObjectivesResize(event: PointerEvent): void {
    if (objectivesResize?.pointerId !== event.pointerId) return;
    objectivesResize = null;
    savePreferences();
  }

  function moveAlerts(event: PointerEvent): void {
    if (alertsDrag?.pointerId !== event.pointerId) return;
    preferences.alertsX = alertsDrag.left + event.clientX - alertsDrag.x;
    preferences.alertsY = alertsDrag.top + event.clientY - alertsDrag.y;
    applyModuleGeometry();
  }

  function endAlertsDrag(event: PointerEvent): void {
    if (alertsDrag?.pointerId !== event.pointerId) return;
    alertsDrag = null;
    savePreferences();
  }

  function resizeAlerts(event: PointerEvent): void {
    if (alertsResize?.pointerId !== event.pointerId) return;
    preferences.alertsWidth = Math.max(260, alertsResize.width + event.clientX - alertsResize.x);
    applyModuleGeometry();
  }

  function endAlertsResize(event: PointerEvent): void {
    if (alertsResize?.pointerId !== event.pointerId) return;
    alertsResize = null;
    savePreferences();
  }

  return {
    dispose() {
      alive = false;
      if (frame !== null) cancelAnimationFrame(frame);
      stopTargetTimer();
      stopPlayerTimer();
      stopActionsTimer();
      stopObjectivesTimer();
      stopAlertsTimer();
      resizeObserver.disconnect();
      window.removeEventListener("resize", handleScreenResize);
      removeInteractivityListener?.();
      removeFocusHeldListener?.();
      image = null;
      root.remove();
    },
  };
}

function drawBackdrop(
  context: CanvasRenderingContext2D,
  snapshot: MechanicsMapSnapshot,
  width: number,
  height: number,
  activeImage: HTMLImageElement | null,
): void {
  context.fillStyle = "rgba(5, 13, 23, 0.92)";
  context.fillRect(0, 0, width, height);
  if (snapshot.map_model === "absolute_scene_map" && activeImage) {
    const ratio = Math.min(width / activeImage.naturalWidth, height / activeImage.naturalHeight);
    const imageWidth = activeImage.naturalWidth * ratio;
    const imageHeight = activeImage.naturalHeight * ratio;
    context.globalAlpha = 0.9;
    context.drawImage(activeImage, (width - imageWidth) / 2, (height - imageHeight) / 2, imageWidth, imageHeight);
    context.globalAlpha = 1;
    context.fillStyle = "rgba(3, 11, 19, 0.16)";
    context.fillRect(0, 0, width, height);
  } else {
    const gradient = context.createRadialGradient(width / 2, height / 2, 0, width / 2, height / 2, Math.max(width, height) * 0.7);
    gradient.addColorStop(0, "rgba(49, 84, 96, 0.35)");
    gradient.addColorStop(1, "rgba(5, 13, 23, 0.98)");
    context.fillStyle = gradient;
    context.fillRect(0, 0, width, height);
  }
}

function drawArena(context: CanvasRenderingContext2D, snapshot: MechanicsMapSnapshot, width: number, height: number): void {
  context.save();
  context.strokeStyle = "rgba(92, 228, 212, 0.35)";
  context.lineWidth = 1;
  if (snapshot.map_layout === "raid_ring") {
    const center = projectMechanicsMapPoint(snapshot, 0, 0, false);
    if (center && snapshot.map_span_x && snapshot.map_span_z) {
      for (const radius of [11.5, 12.5, 17.5, 18.5, 30]) {
        context.beginPath();
        context.ellipse(center.mapX / 100 * width, center.mapY / 100 * height,
          radius / snapshot.map_span_x * width, radius / snapshot.map_span_z * height, 0, 0, Math.PI * 2);
        context.stroke();
      }
    }
  } else if (snapshot.map_layout === "raid_grid") {
    for (const x of [-30, -10, 10, 30]) drawWorldLine(context, snapshot, width, height, x, 22.5, x, -22.5);
    for (const z of [-22.5, -7.5, 7.5, 22.5]) drawWorldLine(context, snapshot, width, height, -30, z, 30, z);
  } else {
    context.beginPath();
    context.arc(width / 2, height / 2, Math.min(width, height) * 0.25, 0, Math.PI * 2);
    context.stroke();
    context.beginPath();
    context.arc(width / 2, height / 2, Math.min(width, height) * 0.44, 0, Math.PI * 2);
    context.stroke();
  }
  context.restore();
}

function drawRegions(context: CanvasRenderingContext2D, snapshot: MechanicsMapSnapshot, width: number, height: number): void {
  for (const signal of snapshot.mechanics) {
    drawPolygon(context, projectCursedTombChargeRegion(snapshot, signal), width, height,
      signal.mechanic_kind === "clone_charge_right" ? "rgba(179,138,255,.28)" : "rgba(255,91,111,.28)");
    const beam = projectCoralMatrixBeam(snapshot, signal);
    if (beam.length === 2) drawLine(context, beam[0]!, beam[1]!, width, height, "rgba(242,195,107,.95)", 3);
  }
  for (const entity of snapshot.entities) {
    drawPolygon(context, projectTinaPizzaRegion(snapshot, entity), width, height,
      entity.mechanic_role === "pizza_fast" ? "rgba(255,157,92,.32)" : "rgba(255,91,111,.32)");
    drawPolygon(context, projectCoralWaveRegion(snapshot, entity), width, height, "rgba(92,228,212,.24)");
  }
  for (const region of projectCoralPizzaRegions(snapshot)) {
    drawPolygon(context, region.points, width, height,
      region.kind === "pizza_purple" ? "rgba(179,138,255,.3)" : "rgba(255,157,92,.3)");
  }
  for (const region of projectRaidFloorRegions(snapshot)) drawFloorRegion(context, region, width, height);
  drawActiveRaidRings(context, snapshot, width, height);
}

function drawEntities(
  context: CanvasRenderingContext2D,
  snapshot: MechanicsMapSnapshot,
  width: number,
  height: number,
  preferences: MechanicsMapCanvasPreferences,
): void {
  const sceneMap = snapshot.map_model === "absolute_scene_map";
  const entities = projectMechanicsMapEntities(snapshot, sceneMap ? false : preferences.rotateWithPlayer)
    .filter((entity) => entity.visible && (preferences.showMonsters || !["monster", "npc", "object"].includes(entity.kind)));
  const colors: Record<string, string> = {
    local: "#5ce4d4", party: "#6da9ff", boss: "#ff6f83", player: "#8aa2ba",
    monster: "#f2c36b", pet: "#b38aff", npc: "#8aa2ba", object: "#8aa2ba",
  };
  for (const entity of entities) {
    const x = entity.mapX / 100 * width;
    const y = entity.mapY / 100 * height;
    const radius = entity.kind === "boss" ? 8 : entity.kind === "local" ? 7 : 5;
    context.save();
    context.globalAlpha = entity.stale ? 0.32 : entity.dead ? 0.48 : 1;
    context.shadowBlur = 10;
    context.shadowColor = colors[entity.kind] ?? "#8aa2ba";
    context.fillStyle = colors[entity.kind] ?? "#8aa2ba";
    context.strokeStyle = "rgba(4,12,20,.95)";
    context.lineWidth = 2;
    context.beginPath();
    if (entity.kind === "local" && entity.facing_radians !== null) {
      context.translate(x, y);
      context.rotate(entity.facing_radians);
      context.moveTo(0, -radius - 3);
      context.lineTo(radius, radius);
      context.lineTo(-radius, radius);
      context.closePath();
    } else if (entity.kind === "boss") {
      context.rect(x - radius, y - radius, radius * 2, radius * 2);
    } else {
      context.arc(x, y, radius, 0, Math.PI * 2);
    }
    context.fill();
    context.stroke();
    context.shadowBlur = 0;
    if (entity.display_name && ["party", "boss"].includes(entity.kind)) {
      context.fillStyle = "rgba(242,246,251,.92)";
      context.font = "600 10px system-ui";
      context.textAlign = "center";
      context.fillText(entity.display_name, entity.kind === "local" ? 0 : x, entity.kind === "local" ? radius + 14 : y + radius + 13);
    }
    context.restore();
  }
  for (const marker of snapshot.markers) {
    if (marker.x === null || marker.z === null) continue;
    const point = projectMechanicsMapPoint(snapshot, marker.x, marker.z, sceneMap ? false : preferences.rotateWithPlayer);
    if (!point?.visible) continue;
    const x = point.mapX / 100 * width;
    const y = point.mapY / 100 * height;
    context.fillStyle = "#f4d76b";
    context.beginPath();
    context.arc(x, y, 8, 0, Math.PI * 2);
    context.fill();
    if (marker.marker_id !== null) {
      context.fillStyle = "#061018";
      context.font = "800 9px system-ui";
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.fillText(String(marker.marker_id), x, y);
    }
  }
}

function drawFloorRegion(context: CanvasRenderingContext2D, region: MechanicsMapProjectedRegion, width: number, height: number): void {
  const danger = region.kind === "phase_edge" || region.kind === "phase_corner";
  drawPolygon(context, region.points, width, height, danger ? "rgba(255,91,111,.26)" : "rgba(179,138,255,.24)");
  if (!region.label || region.points.length === 0) return;
  const x = region.points.reduce((sum, point) => sum + point.mapX, 0) / region.points.length / 100 * width;
  const y = region.points.reduce((sum, point) => sum + point.mapY, 0) / region.points.length / 100 * height;
  context.fillStyle = "#f2f6fb";
  context.font = "800 12px system-ui";
  context.textAlign = "center";
  context.fillText(region.label, x, y);
}

function drawActiveRaidRings(context: CanvasRenderingContext2D, snapshot: MechanicsMapSnapshot, width: number, height: number): void {
  if (snapshot.map_layout !== "raid_ring" || !snapshot.map_span_x || !snapshot.map_span_z) return;
  const center = projectMechanicsMapPoint(snapshot, 0, 0, false);
  if (!center) return;
  const bands = { ring_inner: [0, 12.5], ring_middle: [12.5, 17.5], ring_outer: [18.5, 30] } as const;
  for (const signal of snapshot.mechanics.slice(-8)) {
    if (!signal.mechanic_kind || !(signal.mechanic_kind in bands)) continue;
    const [inner, outer] = bands[signal.mechanic_kind as keyof typeof bands];
    const radius = inner === 0 ? outer : (inner + outer) / 2;
    context.save();
    context.strokeStyle = mechanicColor(signal.effect_id, signal.mechanic_kind);
    context.globalAlpha = 0.55;
    context.lineWidth = Math.max(2, (outer - inner) / snapshot.map_span_x * width);
    context.beginPath();
    context.ellipse(center.mapX / 100 * width, center.mapY / 100 * height,
      radius / snapshot.map_span_x * width, radius / snapshot.map_span_z * height, 0, 0, Math.PI * 2);
    context.stroke();
    context.restore();
  }
}

function drawPolygon(context: CanvasRenderingContext2D, points: readonly MechanicsMapViewPoint[], width: number, height: number, fill: string): void {
  if (points.length < 3) return;
  context.beginPath();
  context.moveTo(points[0]!.mapX / 100 * width, points[0]!.mapY / 100 * height);
  for (const point of points.slice(1)) context.lineTo(point.mapX / 100 * width, point.mapY / 100 * height);
  context.closePath();
  context.fillStyle = fill;
  context.fill();
  context.strokeStyle = fill;
  context.lineWidth = 1.5;
  context.stroke();
}

function drawLine(context: CanvasRenderingContext2D, start: MechanicsMapViewPoint, end: MechanicsMapViewPoint, width: number, height: number, stroke: string, lineWidth: number): void {
  context.beginPath();
  context.moveTo(start.mapX / 100 * width, start.mapY / 100 * height);
  context.lineTo(end.mapX / 100 * width, end.mapY / 100 * height);
  context.strokeStyle = stroke;
  context.lineWidth = lineWidth;
  context.stroke();
}

function drawWorldLine(context: CanvasRenderingContext2D, snapshot: MechanicsMapSnapshot, width: number, height: number, x1: number, z1: number, x2: number, z2: number): void {
  const start = projectMechanicsMapPoint(snapshot, x1, z1, false);
  const end = projectMechanicsMapPoint(snapshot, x2, z2, false);
  if (start && end) drawLine(context, start, end, width, height, "rgba(92,228,212,.35)", 1);
}

function mechanicColor(effectId: number, kind: string | null): string {
  if (kind?.includes("ice")) return "#6da9ff";
  if (kind?.includes("water")) return "#5ce4d4";
  if (kind?.includes("orange") || kind?.includes("pinball")) return "#ff9d5c";
  if (kind?.includes("purple") || kind?.includes("mirage")) return "#b38aff";
  return Math.abs(effectId) % 2 === 0 ? "#ff6f83" : "#f2c36b";
}

function loadPreferences(): MechanicsMapCanvasPreferences {
  try {
    return parseMechanicsMapCanvasPreferences(JSON.parse(localStorage.getItem(PREFERENCES_KEY) ?? "null"));
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
}

function formatTargetHealth(current: number | null, maximum: number | null): string {
  const format = (value: number): string => Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 }).format(value);
  if (current !== null && maximum !== null) return `${format(current)} / ${format(maximum)}`;
  if (current !== null) return format(current);
  if (maximum !== null) return `— / ${format(maximum)}`;
  return "HP not observed";
}

function formatDebuffRemaining(value: number): string {
  const seconds = Math.max(0, value) / 1_000;
  return seconds >= 10 ? `${Math.ceil(seconds)}s` : `${Math.ceil(seconds * 10) / 10}s`;
}

function formatActionRemaining(value: number | null): string {
  if (value === null) return "•";
  if (value <= 0) return "";
  const seconds = value / 1_000;
  return seconds >= 10 ? String(Math.ceil(seconds)) : (Math.ceil(seconds * 10) / 10).toFixed(1);
}

function button(label: string, active: boolean, action: () => void): HTMLButtonElement {
  const value = text("button", label);
  value.type = "button";
  value.dataset.active = String(active);
  value.addEventListener("click", action);
  return value;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const value = document.createElement(tag);
  if (className) value.className = className;
  return value;
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = value;
  return node;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function finitePositive(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value > 0;
}

function finiteBounded(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 10_000_000;
}
