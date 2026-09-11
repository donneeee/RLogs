import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
import type { AutomarkerLoadResult, AutomarkerPoint, AutomarkerPresetView, AutomarkerPreview } from "./automarker-presets";
import { AUTOMARKER_PREVIEW_STORAGE_KEY, automarkerResponseIsCurrent, publishAutomarkerPreview, readActiveAutomarkerPreview } from "./automarker-presets";
import { activeOverlaySetup, normalizedModuleGeometry, raiseOverlayModule, type OverlayLayoutSettings, type OverlayModuleId } from "./overlay-layout";
import { LocalHostHttpError } from "../shell/local-host-http";
import {
  actionControlRemainingMillis,
  fitMechanicsMapCanvasRect,
  mechanicSignalRemainingMillis,
  projectCoralMatrixBeam,
  projectCoralPizzaRegions,
  projectCursedTombChargeRegion,
  projectMechanicsMapEntities,
  projectMechanicsMapPoint,
  projectRaidFloorRegions,
  projectVoidTowerMapAnnotations,
  targetDebuffRemainingMillis,
  zoomMechanicsMapAt,
  type MechanicsMapProjectedRegion,
  type MechanicsMapCanvasRect,
  type MechanicsMapSnapshot,
  type MechanicsMapUpdate,
  type MechanicsMapViewPoint,
} from "./mechanics-map";

const PREFERENCES_KEY = "rlogs.mechanics-map-overlay.canvas.v1";

export interface MechanicsMapOverlayDependencies {
  loadSnapshot(): Promise<MechanicsMapUpdate>;
  waitForSnapshot(afterRevision: number): Promise<MechanicsMapUpdate>;
  prepareLocalMaps(): Promise<void>;
  setInteractive(interactive: boolean): Promise<void>;
  acknowledgeInteractivity?(interactive: boolean): Promise<void>;
  onInteractivity(handler: (interactive: boolean) => void): Promise<() => void>;
  onLayoutInitialized?(revision: number | undefined, error?: unknown): void;
  onLayoutRefresh?(handler: (revision: number) => void): Promise<() => void>;
  onFocusHeld(handler: (held: boolean) => void): Promise<() => void>;
  loadAutomarkerPresets(): Promise<AutomarkerPresetView>;
  loadAutomarkerPreset(presetId: string): Promise<AutomarkerLoadResult>;
  loadLayout(): Promise<OverlayLayoutSettings>;
  saveLayout(settings: OverlayLayoutSettings): Promise<OverlayLayoutSettings>;
}

export interface MechanicsMapCanvasPreferences {
  scale: number;
  panX: number;
  panY: number;
  rotateWithPlayer: boolean;
  showMonsters: boolean;
  mapDim: number;
  highContrastMechanics: boolean;
  showPlayer: boolean;
  showActions: boolean;
  showParty: boolean;
  showTarget: boolean;
  showObjectives: boolean;
  showAlerts: boolean;
  locked: boolean;
  expanded: boolean;
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
  mapDim: 0.32,
  highContrastMechanics: true,
  showPlayer: true,
  showActions: true,
  showParty: true,
  showTarget: true,
  showObjectives: true,
  showAlerts: true,
  locked: false,
  expanded: false,
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
    mapDim: finiteUnitInterval(value.mapDim) ? Math.min(0.8, value.mapDim) : 0.32,
    highContrastMechanics: typeof value.highContrastMechanics === "boolean" ? value.highContrastMechanics : true,
    showPlayer: typeof value.showPlayer === "boolean" ? value.showPlayer : true,
    showActions: typeof value.showActions === "boolean" ? value.showActions : true,
    showParty: typeof value.showParty === "boolean" ? value.showParty : true,
    showTarget: typeof value.showTarget === "boolean" ? value.showTarget : true,
    showObjectives: typeof value.showObjectives === "boolean" ? value.showObjectives : true,
    showAlerts: typeof value.showAlerts === "boolean" ? value.showAlerts : true,
    locked: typeof value.locked === "boolean" ? value.locked : false,
    expanded: typeof value.expanded === "boolean" ? value.expanded : false,
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
  localizer?: UiLocalizer,
): string {
  const format = (number: number): string => localizer?.formatNumber(number) ?? number.toLocaleString();
  if (value !== null && requiredCount !== null) {
    return `${format(value)} / ${format(requiredCount)}${complete === true ? " ✓" : ""}`;
  }
  if (value !== null) return `${format(value)}${complete === true ? " ✓" : ""}`;
  return complete === true
    ? localizer?.t("ui.mechanics_map.objectives.complete_title") ?? "Complete"
    : localizer?.t("ui.mechanics_map.objectives.observed_title") ?? "Observed";
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

function localizedDungeonState(value: string, localizer: UiLocalizer): string {
  const key = ({
    started: "ui.mechanics_map.objectives.state_started",
    cleared: "ui.mechanics_map.objectives.state_cleared",
    wiped: "ui.mechanics_map.objectives.state_wiped",
    ended: "ui.mechanics_map.objectives.state_ended",
  } as const)[value as "started" | "cleared" | "wiped" | "ended"];
  return key === undefined ? humanizeDungeonState(value) : localizer.t(key);
}

export function mountMechanicsMapOverlay(
  container: HTMLElement,
  dependencies: MechanicsMapOverlayDependencies,
  localizer: UiLocalizer,
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
  let removeLayoutRefreshListener: (() => void) | null = null;
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
  let automarkerView: AutomarkerPresetView | null = null;
  let automarkerSceneKey = "";
  let automarkerPreview: AutomarkerPreview | null = null;
  let automarkerPreviewTimer: number | null = null;
  let automarkerRequestGeneration = 0;
  let layoutSettings: OverlayLayoutSettings | null = null;
  let layoutAcknowledged: OverlayLayoutSettings | null = null;
  let layoutPollTimer: number | null = null;
  let layoutSaving = false;
  type LayoutOperation = (settings: OverlayLayoutSettings) => void;
  const layoutOperations: LayoutOperation[] = [];

  const root = element("main", "overlay-canvas-runtime");
  root.dataset.locked = String(preferences.locked);
  const panel = element("section", "mechanics-map-overlay-runtime");
  panel.dataset.locked = String(preferences.locked);
  panel.dataset.expanded = String(preferences.expanded);
  const toolbar = element("header", "mechanics-map-overlay-toolbar");
  const identity = element("div", "mechanics-map-overlay-identity");
  const title = text("strong", localizer.t("ui.mechanics_map.status.waiting_for_scene"));
  const status = text("span", localizer.t("ui.mechanics_map.status.connecting"));
  identity.append(title, status);
  const actions = element("div", "mechanics-map-overlay-actions");
  const rotate = button(localizer.t("ui.mechanics_map.toolbar.rotate"), preferences.rotateWithPlayer, () => {
    preferences.rotateWithPlayer = !preferences.rotateWithPlayer;
    rotate.dataset.active = String(preferences.rotateWithPlayer);
    savePreferences();
    scheduleDraw();
  });
  const monsters = button(localizer.t("ui.mechanics_map.toolbar.mobs"), preferences.showMonsters, () => {
    preferences.showMonsters = !preferences.showMonsters;
    monsters.dataset.active = String(preferences.showMonsters);
    savePreferences();
    scheduleDraw();
  });
  const dimLabel = (): string => localizer.t("ui.mechanics_map.toolbar.dim_percent", {
    percent: localizer.formatNumber(Math.round(preferences.mapDim * 100)),
  });
  const dim = button(dimLabel(), preferences.mapDim > 0, () => {
    preferences.mapDim = nextMapDim(preferences.mapDim);
    dim.textContent = dimLabel();
    dim.dataset.active = String(preferences.mapDim > 0);
    dim.title = localizer.t("ui.mechanics_map.toolbar.dim_help");
    savePreferences();
    scheduleDraw();
  });
  dim.title = localizer.t("ui.mechanics_map.toolbar.dim_help");
  const contrast = button(localizer.t("ui.mechanics_map.toolbar.contrast"), preferences.highContrastMechanics, () => {
    preferences.highContrastMechanics = !preferences.highContrastMechanics;
    contrast.dataset.active = String(preferences.highContrastMechanics);
    contrast.setAttribute("aria-pressed", String(preferences.highContrastMechanics));
    savePreferences();
    scheduleDraw();
  });
  contrast.title = localizer.t("ui.mechanics_map.toolbar.contrast_help");
  const fit = button(localizer.t("ui.mechanics_map.toolbar.fit"), false, () => {
    preferences.scale = 1;
    preferences.panX = 0;
    preferences.panY = 0;
    savePreferences();
    scheduleDraw();
  });
  const center = button(localizer.t("ui.mechanics_map.toolbar.center"), false, centerOnPlayer);
  const markerPresets = button("Marker presets", false, () => {
    automarkerPanel.hidden = !automarkerPanel.hidden;
    markerPresets.dataset.active = String(!automarkerPanel.hidden);
    if (!automarkerPanel.hidden) void refreshAutomarkerPresets();
  });
  const expand = button(preferences.expanded ? "Window" : "Full map", preferences.expanded, () => {
    setExpanded(!preferences.expanded);
  });
  const lock = button(preferences.locked ? "Unlock" : "Lock", preferences.locked, () => {
    void setLocked(!preferences.locked);
  });
  const done = button("Done", false, () => { void exitEditing(); });
  done.title = "Exit editing and keep overlays visible";
  actions.append(rotate, monsters, dim, contrast, fit, center, markerPresets, expand, lock, done);
  toolbar.append(identity, actions);

  const viewport = element("section", "mechanics-map-overlay-viewport");
  const canvas = document.createElement("canvas");
  canvas.className = "mechanics-map-overlay-canvas";
  canvas.setAttribute("aria-label", "Live packet-observed Mechanics Map canvas");
  const notice = text("p", localizer.t("ui.mechanics_map.notice.waiting_for_position"), "mechanics-map-overlay-notice");
  viewport.append(canvas, notice);
  const footer = element("footer", "mechanics-map-overlay-footer");
  const mapSource = text("span", localizer.t("ui.mechanics_map.source.radar_fallback"));
  const mapCoordinates = text("span", "X — · Z —");
  const mapMetrics = text("span", "1× · 0 entities");
  footer.append(mapSource, mapCoordinates, mapMetrics);
  const resize = element("button", "mechanics-map-overlay-resize");
  resize.type = "button";
  resize.title = "Resize Mechanics Map overlay";
  resize.setAttribute("aria-label", "Resize Mechanics Map overlay");
  resize.addEventListener("pointerdown", (event) => {
    if (preferences.expanded) return;
    event.preventDefault();
    moduleResize = {
      pointerId: event.pointerId, x: event.clientX, y: event.clientY,
      width: preferences.moduleWidth, height: preferences.moduleHeight,
    };
    resize.setPointerCapture(event.pointerId);
  });
  panel.append(toolbar, viewport, footer, resize);
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
  const objectivesTitle = text("strong", localizer.t("ui.mechanics_map.objectives.title"));
  const objectivesStatus = text("span", localizer.t("ui.mechanics_map.status.waiting"));
  objectivesToolbar.append(objectivesTitle, objectivesStatus);
  const objectivesBody = element("section", "dungeon-objectives-overlay-body");
  const objectivesResizeHandle = element("button", "dungeon-objectives-overlay-resize");
  objectivesResizeHandle.type = "button";
  objectivesResizeHandle.title = localizer.t("ui.mechanics_map.objectives.resize");
  objectivesResizeHandle.setAttribute("aria-label", localizer.t("ui.mechanics_map.objectives.resize"));
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
  const automarkerPanel = element("section", "automarker-overlay-picker");
  automarkerPanel.hidden = true;
  const automarkerTitle = text("strong", "Marker presets");
  const automarkerSelect = document.createElement("select");
  const automarkerPreviewButton = button("Preview locally", false, previewSelectedAutomarkerPreset);
  const automarkerNote = text("small", "Current dungeon-family presets only");
  automarkerPanel.append(automarkerTitle, automarkerSelect, automarkerPreviewButton, automarkerNote);
  const moduleToggles = [
    moduleVisibilityButton("Player", "showPlayer", playerPanel),
    moduleVisibilityButton("Actions", "showActions", actionsPanel),
    moduleVisibilityButton("Party", "showParty", partyPanel),
    moduleVisibilityButton("Target", "showTarget", targetPanel),
    moduleVisibilityButton(localizer.t("ui.mechanics_map.objectives.toggle"), "showObjectives", objectivesPanel),
    moduleVisibilityButton("Alerts", "showAlerts", alertsPanel),
  ];
  actions.prepend(...moduleToggles);
  root.append(panel, playerPanel, actionsPanel, partyPanel, targetPanel, objectivesPanel, alertsPanel, automarkerPanel);
  container.replaceChildren(root);
  applyModuleGeometry();

  toolbar.addEventListener("pointerdown", (event) => {
    if (preferences.expanded || event.button !== 0 || (event.target as Element).closest("button")) return;
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
  for (const [id, handle, module] of [
    ["map", toolbar, panel], ["player", playerToolbar, playerPanel], ["actions", actionsToolbar, actionsPanel],
    ["party", partyToolbar, partyPanel], ["target", targetToolbar, targetPanel],
    ["objectives", objectivesToolbar, objectivesPanel], ["alerts", alertsToolbar, alertsPanel],
  ] as const) handle.addEventListener("pointerdown", () => bringToFront(id, module));
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
  const handlePreviewStorage = (event: StorageEvent): void => {
    if (event.key === AUTOMARKER_PREVIEW_STORAGE_KEY) refreshAutomarkerPreview();
  };
  window.addEventListener("storage", handlePreviewStorage);
  let escapeLockPending = false;
  const handleEscape = (event: KeyboardEvent): void => {
    if (
      event.key !== "Escape"
      || event.defaultPrevented
      || event.isComposing
      || preferences.locked
      || escapeLockPending
    ) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("input, textarea, select, [contenteditable]:not([contenteditable='false']), [role='textbox'], [role='combobox'], [role='listbox']")) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    void exitEditing();
  };
  window.addEventListener("keydown", handleEscape);
  automarkerPreviewTimer = window.setInterval(refreshAutomarkerPreview, 500);

  void initializeLayout();
  layoutPollTimer = window.setInterval(() => { void refreshLayout(); }, 1_000);
  void dependencies.onInteractivity((interactive) => {
    void (async () => {
      if (interactive && preferences.locked) await setLocked(false);
      await dependencies.acknowledgeInteractivity?.(interactive);
    })();
  }).then((remove) => { removeInteractivityListener = remove; });
  void dependencies.onLayoutRefresh?.((revision) => { void refreshLayout(revision); })
    .then((remove) => { removeLayoutRefreshListener = remove; });
  void dependencies.onFocusHeld((held) => {
    root.dataset.focusHeld = String(held);
    actionsPanel.dataset.focused = String(held);
    playerPanel.dataset.focused = String(held);
  }).then((remove) => { removeFocusHeldListener = remove; });
  void connect();

  function moduleVisibilityButton(
    label: string,
    preference: "showPlayer" | "showActions" | "showParty" | "showTarget" | "showObjectives" | "showAlerts",
    module: HTMLElement,
  ): HTMLButtonElement {
    const control = button(label, preferences[preference], () => {
      preferences[preference] = !preferences[preference];
      control.dataset.active = String(preferences[preference]);
      control.setAttribute("aria-pressed", String(preferences[preference]));
      control.title = localizer.t(preferences[preference]
        ? "ui.mechanics_map.module.hide"
        : "ui.mechanics_map.module.show", { module: label });
      module.hidden = !preferences[preference];
      savePreferences();
      persistSharedLayout();
    });
    control.title = localizer.t(preferences[preference]
      ? "ui.mechanics_map.module.hide"
      : "ui.mechanics_map.module.show", { module: label });
    control.setAttribute("aria-pressed", String(preferences[preference]));
    module.hidden = !preferences[preference];
    return control;
  }

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

  async function refreshAutomarkerPresets(): Promise<void> {
    const requestGeneration = ++automarkerRequestGeneration;
    const requestedSnapshotKey = mechanicsMapAutomarkerSnapshotKey(update?.snapshot);
    try {
      const next = await dependencies.loadAutomarkerPresets();
      if (!alive || !automarkerResponseIsCurrent(
        requestGeneration,
        automarkerRequestGeneration,
        requestedSnapshotKey,
        mechanicsMapAutomarkerSnapshotKey(update?.snapshot),
      ) || automarkerPresetViewSnapshotKey(next) !== requestedSnapshotKey) return;
      automarkerView = next;
      renderAutomarkerPresets();
      refreshAutomarkerPreview();
    } catch (error) {
      if (!automarkerResponseIsCurrent(
        requestGeneration,
        automarkerRequestGeneration,
        requestedSnapshotKey,
        mechanicsMapAutomarkerSnapshotKey(update?.snapshot),
      )) return;
      automarkerView = null;
      automarkerSelect.replaceChildren(new Option("Preset catalog unavailable", ""));
      automarkerSelect.disabled = true;
      automarkerPreviewButton.disabled = true;
      automarkerNote.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  function refreshAutomarkerPreview(): void {
    let next: AutomarkerPreview | null = null;
    const context = automarkerView?.context;
    if (context !== null && context !== undefined) {
      next = readActiveAutomarkerPreview(
        window.localStorage,
        automarkerView!.previewSessionId,
        context,
      );
    }
    if (JSON.stringify(next) === JSON.stringify(automarkerPreview)) return;
    automarkerPreview = next;
    if (next !== null) {
      automarkerNote.textContent = `${next.name}: ${next.points.length} local preview marker${next.points.length === 1 ? "" : "s"}. No game transmission.`;
    }
    scheduleDraw();
  }

  function renderAutomarkerPresets(): void {
    automarkerSelect.replaceChildren();
    const presets = automarkerView?.presets ?? [];
    if (presets.length === 0) {
      automarkerSelect.append(new Option(automarkerView?.context === null ? "Enter a scene" : "No setups for this scene", ""));
    } else {
      for (const preset of presets) automarkerSelect.append(new Option(`${preset.name} · ${preset.points.length} marks`, preset.presetId));
    }
    automarkerSelect.disabled = presets.length === 0;
    automarkerPreviewButton.disabled = presets.length === 0 || automarkerView?.context === null;
    automarkerPreviewButton.title = "Draw the selected setup on this local Mechanics Map only";
    automarkerNote.textContent = "Local preview only. Nothing is sent to the game; native placement remains locked.";
  }

  function previewSelectedAutomarkerPreset(): void {
    const context = automarkerView?.context;
    const preset = automarkerView?.presets.find((candidate) => candidate.presetId === automarkerSelect.value);
    if (!alive || context === null || context === undefined || preset === undefined) return;
    try {
      publishAutomarkerPreview(
        window.localStorage,
        context,
        preset.name,
        preset.points,
        automarkerView!.previewSessionId,
      );
      refreshAutomarkerPreview();
    } catch (error) {
      automarkerNote.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  function renderState(): void {
    const snapshot = update?.snapshot;
    if (!snapshot) return;
    const nextAutomarkerSceneKey = mechanicsMapAutomarkerSnapshotKey(snapshot);
    if (nextAutomarkerSceneKey !== automarkerSceneKey) {
      if (automarkerSceneKey !== "") window.localStorage.removeItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
      automarkerSceneKey = nextAutomarkerSceneKey;
      automarkerView = null;
      automarkerPreview = null;
      renderAutomarkerPresets();
      void refreshAutomarkerPresets();
    }
    title.textContent = snapshot.scene_name ?? (snapshot.scene_id === null
      ? localizer.t("ui.mechanics_map.status.waiting_for_scene")
      : `Scene ${snapshot.scene_id}`);
    status.textContent = snapshot.local_position_observed
      ? localizer.t("ui.mechanics_map.status.live")
      : snapshot.scene_id === null
        ? localizer.t("ui.mechanics_map.status.waiting")
        : localizer.t("ui.mechanics_map.status.position_needed");
    status.dataset.state = snapshot.local_position_observed ? "live" : "waiting";
    loadBackground(snapshot.background_asset_url);
    renderMapAvailability(snapshot);
    renderPlayer(snapshot);
    renderActions(snapshot);
    renderParty(snapshot);
    renderTarget(snapshot);
    renderDungeonObjectives(snapshot);
    renderMechanicAlerts(snapshot);
    renderMapFooter(snapshot);
    scheduleDraw();
  }

  function renderMapAvailability(snapshot: MechanicsMapSnapshot): void {
    const availability = mechanicsMapAssetAvailability(snapshot, imageReady);
    if (availability === "unsupported") {
      notice.hidden = false;
      notice.textContent = "Map unavailable: this identified scene has no reviewed game-map asset.";
      return;
    }
    if (availability === "asset_pending") {
      notice.hidden = false;
      notice.textContent = preparingAsset
        ? "Map unavailable while the reviewed game-map asset is prepared."
        : "Map unavailable: the reviewed game-map asset is not available locally.";
      return;
    }
    notice.hidden = snapshot.local_position_observed && snapshot.data_gap === null;
    notice.textContent = snapshot.data_gap ?? localizer.t("ui.mechanics_map.notice.waiting_for_position");
  }

  function renderMapFooter(snapshot: MechanicsMapSnapshot): void {
    mapSource.textContent = snapshot.map_model === "absolute_scene_map"
      ? imageReady
        ? localizer.t("ui.mechanics_map.source.game_map")
        : preparingAsset
          ? localizer.t("ui.mechanics_map.source.preparing_map")
          : localizer.t("ui.mechanics_map.source.map_asset_pending")
      : localizer.t("ui.mechanics_map.source.radar_fallback");
    mapSource.dataset.state = snapshot.map_model === "absolute_scene_map" && imageReady ? "live" : "fallback";
    const local = snapshot.entities.find((entity) => entity.actor_id === snapshot.local_actor_id);
    mapCoordinates.textContent = local
      ? `X ${formatMechanicsMapCoordinate(local.x, localizer)} · Z ${formatMechanicsMapCoordinate(local.z, localizer)}`
      : "X — · Z —";
    const mechanicCount = snapshot.mechanics.filter((signal) => signal.mechanic_kind !== null).length;
    const objectiveCount = snapshot.dungeon?.objectives.length ?? 0;
    mapMetrics.textContent = `${formatMechanicsMapZoom(preferences.scale)}× · ${localizer.t("ui.mechanics_map.metrics.summary", {
      entities: localizer.formatNumber(snapshot.entities.length),
      mechanics: localizer.formatNumber(mechanicCount),
      objectives: localizer.formatNumber(objectiveCount),
    })}`;
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
      objectivesTitle.textContent = localizer.t("ui.mechanics_map.objectives.title");
      objectivesStatus.textContent = localizer.t("ui.mechanics_map.status.waiting");
      objectivesStatus.dataset.state = "waiting";
      objectivesBody.append(text("p", localizer.t("ui.mechanics_map.objectives.waiting"), "dungeon-objectives-overlay-empty"));
      return;
    }
    objectivesTitle.textContent = snapshot.scene_name ??
      (dungeon.dungeon_id === null
        ? localizer.t("ui.mechanics_map.objectives.title")
        : localizer.t("ui.mechanics_map.objectives.dungeon", { id: dungeon.dungeon_id }));
    objectivesStatus.textContent = localizedDungeonState(
      dungeon.encounter_state ?? dungeon.flow_phase ?? dungeon.state,
      localizer,
    );
    objectivesStatus.dataset.state = dungeon.encounter_state === "wiped" || dungeon.state === "failed"
      ? "failed"
      : dungeon.encounter_state === "cleared" || dungeon.state === "completed" ? "complete" : "live";
    const metadata = [
      dungeon.dungeon_id === null ? null : localizer.t("ui.mechanics_map.objectives.dungeon", { id: dungeon.dungeon_id }),
      dungeon.difficulty_id === null ? null : localizer.t("ui.mechanics_map.objectives.difficulty", { id: dungeon.difficulty_id }),
    ].filter((value): value is string => value !== null).join(" · ");
    if (metadata) objectivesBody.append(text("small", metadata, "dungeon-objectives-overlay-meta"));
    if (dungeon.attempt_number > 0) {
      const attempt = element("div", "dungeon-objectives-overlay-attempt");
      const label = text("span", localizer.t("ui.mechanics_map.objectives.attempt", {
        number: localizer.formatNumber(dungeon.attempt_number),
      }));
      const timer = text("b", formatDungeonAttemptTime(dungeon.attempt_elapsed_micros));
      const retries = text("small", localizer.t(dungeon.retry_count === 1
        ? "ui.mechanics_map.objectives.retry"
        : "ui.mechanics_map.objectives.retries", { count: localizer.formatNumber(dungeon.retry_count) }));
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
      objectivesBody.append(text("p", localizer.t("ui.mechanics_map.objectives.empty"), "dungeon-objectives-overlay-empty"));
      return;
    }
    for (const objective of dungeon.objectives) {
      const row = element("article", "dungeon-objectives-overlay-row");
      row.dataset.complete = String(objective.complete === true);
      row.dataset.catalogResolution = objective.catalog_resolution;
      row.title = [
        localizer.t("ui.mechanics_map.objectives.packet_id", { id: objective.objective_id }),
        objective.objective_map_key === null ? null : localizer.t("ui.mechanics_map.objectives.map_key", { key: objective.objective_map_key }),
        localizer.t("ui.mechanics_map.objectives.catalog", { resolution: humanizeDungeonState(objective.catalog_resolution) }),
      ].filter((value): value is string => value !== null).join("\n");
      const identity = element("span", "dungeon-objectives-overlay-identity");
      identity.append(
        text("strong", objective.presentation_name ?? objective.activity_target_key ?? localizer.t("ui.mechanics_map.objectives.objective", { id: objective.objective_id })),
        text("small", objective.complete === true
          ? localizer.t("ui.mechanics_map.objectives.complete")
          : localizer.t("ui.mechanics_map.objectives.packet_observed")),
      );
      row.append(identity, text("b", formatDungeonObjectiveValue(
        objective.value,
        objective.complete,
        objective.required_count,
        localizer,
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
      const name = element("div", "party-frame-overlay-name");
      name.append(text("strong", member.display_name ?? `Player ${member.actor_id}`));
      identity.append(
        name,
        text("span", member.dead ? "DEFEATED" : member.stale ? "STALE" : formatTargetHealth(member.current_hp, member.max_hp, localizer)),
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
      text("span", formatTargetHealth(player.current_hp, player.max_hp, localizer)),
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
      shield.title = `Shield ${formatTargetHealth(player.current_shield, player.max_shield, localizer)}`;
      shield.append(shieldFill);
      vitals.append(
        shield,
        text("small", `SHIELD ${formatTargetHealth(player.current_shield, player.max_shield, localizer)}`, "player-frame-overlay-shield-label"),
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
    const resources = element("div", "player-resource-overlay-list");
    resources.setAttribute("aria-label", "Packet-observed class resources");
    for (const resource of snapshot.resources) {
      const resourceRow = element("section", "player-resource-overlay-row");
      resourceRow.dataset.kind = resource.kind;
      const resourceIdentity = element("div", "player-resource-overlay-identity");
      resourceIdentity.append(
        text("strong", resource.label),
        text("span", `${resource.current.toLocaleString()} / ${resource.max.toLocaleString()}`),
      );
      const resourceTrack = element("div", "player-resource-overlay-track");
      const resourceFill = element("span");
      resourceFill.style.width = `${Math.max(0, Math.min(100, resource.percent ?? 0))}%`;
      resourceTrack.dataset.observed = String(resource.percent !== null);
      resourceTrack.append(resourceFill);
      resourceRow.append(resourceIdentity, resourceTrack);
      resources.append(resourceRow);
    }
    resources.hidden = snapshot.resources.length === 0;
    playerBody.replaceChildren(identity, vitals, statuses, resources);
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
    const health = text("span", formatTargetHealth(target.current_hp, target.max_hp, localizer));
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
      shield.title = `Shield ${formatTargetHealth(target.current_shield, target.max_shield, localizer)}`;
      shield.append(shieldFill);
      const shieldLabel = text("small", `SHIELD ${formatTargetHealth(target.current_shield, target.max_shield, localizer)}`, "target-frame-overlay-shield-label");
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
      entry.dataset.localOwned = String(effect.owned_by_local_player);
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
      if (effect.owned_by_local_player) {
        const localOwner = text("i", "YOU", "target-frame-overlay-debuff-local-owner");
        localOwner.setAttribute("aria-label", "Applied by you or your Battle Imagine");
        item.append(localOwner);
      }
      if (effect.remaining_millis !== null) {
        const timer = text("span", formatDebuffRemaining(effect.remaining_millis), "target-frame-overlay-debuff-time");
        timer.dataset.targetDebuffRemaining = String(effect.remaining_millis);
        item.append(timer);
      }
      const owner = text(
        "small",
        effect.owned_by_local_player ? "Yours" : effect.source_display_name ?? "—",
        "target-frame-overlay-debuff-owner",
      );
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
      if (update?.snapshot) {
        renderMapAvailability(update.snapshot);
        renderMapFooter(update.snapshot);
      }
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
    if (mechanicsMapAssetAvailability(snapshot, imageReady) !== "ready" || image === null) return;
    context.fillStyle = "rgba(3, 9, 16, 0.94)";
    context.fillRect(0, 0, width, height);
    context.imageSmoothingEnabled = true;
    context.save();
    context.translate(width / 2 + preferences.panX, height / 2 + preferences.panY);
    context.scale(preferences.scale, preferences.scale);
    context.translate(-width / 2, -height / 2);
    const activeImage = image;
    const content = mechanicsMapContentRect(snapshot, width, height, activeImage);
    const readability = mechanicsMapReadabilityProfile(preferences.mapDim, preferences.highContrastMechanics);
    drawBackdrop(context, snapshot, content, activeImage, readability.mapDim);
    context.save();
    context.translate(content.x, content.y);
    context.scale(content.width / width, content.height / height);
    drawRegions(context, snapshot, width, height, preferences.highContrastMechanics);
    drawEntities(context, snapshot, width, height, preferences, automarkerPreview?.points ?? []);
    context.restore();
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
    if (update?.snapshot) renderMapFooter(update.snapshot);
    scheduleDraw();
  }

  function centerOnPlayer(): void {
    const snapshot = update?.snapshot;
    if (!snapshot) return;
    if (snapshot.map_model !== "absolute_scene_map") {
      preferences.panX = 0;
      preferences.panY = 0;
    } else {
      const local = snapshot.entities.find((entity) => entity.actor_id === snapshot.local_actor_id);
      const point = local ? projectMechanicsMapPoint(snapshot, local.x, local.z, false) : null;
      const rect = viewport.getBoundingClientRect();
      if (!point || rect.width <= 0 || rect.height <= 0) return;
      const width = Math.max(1, Math.round(rect.width));
      const height = Math.max(1, Math.round(rect.height));
      const content = mechanicsMapContentRect(snapshot, width, height, imageReady ? image : null);
      const pointX = content.x + point.mapX / 100 * content.width;
      const pointY = content.y + point.mapY / 100 * content.height;
      preferences.panX = -preferences.scale * (pointX - width / 2);
      preferences.panY = -preferences.scale * (pointY - height / 2);
    }
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
    root.dataset.locked = String(value);
    panel.dataset.locked = String(value);
    lock.textContent = value ? "Unlock" : "Lock";
    lock.dataset.active = String(value);
    savePreferences();
    persistSharedLayout();
    await dependencies.setInteractive(!value);
  }

  async function exitEditing(): Promise<void> {
    if (preferences.locked || escapeLockPending) return;
    escapeLockPending = true;
    try {
      // User input proves the editable WebView is initialized. Clear the
      // host-owned forced-edit request before restoring native click-through.
      await dependencies.acknowledgeInteractivity?.(true);
      await setLocked(true);
    } catch { /* Leave the visible canvas available for an explicit retry. */ }
    finally { escapeLockPending = false; }
  }

  function setExpanded(value: boolean): void {
    preferences.expanded = value;
    panel.dataset.expanded = String(value);
    expand.textContent = value ? "Window" : "Full map";
    expand.dataset.active = String(value);
    moduleDrag = null;
    moduleResize = null;
    applyModuleGeometry();
    savePreferences();
    scheduleDraw();
  }

  function savePreferences(): void {
    localStorage.setItem(PREFERENCES_KEY, JSON.stringify(preferences));
  }

  async function initializeLayout(): Promise<void> {
    try {
      let loaded = await dependencies.loadLayout();
      if (!alive) return;
      for (let attempt = 0; alive && !loaded.legacyMigrationComplete && attempt < 4; attempt += 1) {
        try {
          loaded = await dependencies.saveLayout(migrateLegacyLayout(structuredClone(loaded)));
        } catch {
          loaded = await dependencies.loadLayout();
        }
      }
      if (!loaded.legacyMigrationComplete) return;
      if (!alive) return;
      if (layoutSettings !== null && loaded.revision < layoutSettings.revision) return;
      await applySharedLayout(loaded, true);
      dependencies.onLayoutInitialized?.(loaded.revision);
    } catch (error) {
      console.error("Could not load shared overlay layout", error);
      dependencies.onLayoutInitialized?.(undefined, error);
    }
  }

  async function refreshLayout(requiredRevision?: number): Promise<void> {
    if (layoutSaving || layoutOperations.length > 0) {
      if (!layoutSaving) void flushSharedLayout();
      return;
    }
    try {
      const loaded = await dependencies.loadLayout();
      if (requiredRevision !== undefined && loaded.revision < requiredRevision) return;
      if (!loaded.legacyMigrationComplete) { await initializeLayout(); return; }
      if (!alive) return;
      if (layoutSettings !== null && loaded.revision < layoutSettings.revision) return;
      if (loaded.revision !== layoutSettings?.revision) await applySharedLayout(loaded, true);
      dependencies.onLayoutInitialized?.(loaded.revision);
    } catch { /* retain the last safe host-owned layout */ }
  }

  function migrateLegacyLayout(settings: OverlayLayoutSettings): OverlayLayoutSettings {
    const setup = activeOverlaySetup(settings);
    const legacy = window.localStorage.getItem(PREFERENCES_KEY);
    if (legacy !== null) {
      setGeometry(setup.modules.map, preferences.moduleX, preferences.moduleY, preferences.moduleWidth, preferences.moduleHeight);
      setGeometry(setup.modules.player, preferences.playerX, preferences.playerY, preferences.playerWidth, 160);
      setGeometry(setup.modules.actions, preferences.actionsX, preferences.actionsY, preferences.actionsWidth, 170);
      setGeometry(setup.modules.party, preferences.partyX, preferences.partyY, preferences.partyWidth, 480);
      setGeometry(setup.modules.target, preferences.targetX, preferences.targetY, preferences.targetWidth, 190);
      setGeometry(setup.modules.objectives, preferences.objectivesX, preferences.objectivesY, preferences.objectivesWidth, 300);
      setGeometry(setup.modules.alerts, preferences.alertsX, preferences.alertsY, preferences.alertsWidth, 300);
      setup.modules.player.visible = preferences.showPlayer; setup.modules.actions.visible = preferences.showActions;
      setup.modules.party.visible = preferences.showParty; setup.modules.target.visible = preferences.showTarget;
      setup.modules.objectives.visible = preferences.showObjectives; setup.modules.alerts.visible = preferences.showAlerts;
      setup.locked = preferences.locked;
    }
    settings.legacyMigrationComplete = true;
    return settings;
  }

  function setGeometry(module: ReturnType<typeof activeOverlaySetup>["modules"][OverlayModuleId], x: number, y: number, width: number, height: number): void {
    Object.assign(module, normalizedModuleGeometry({ x, y, width, height }, window.innerWidth, window.innerHeight));
  }

  async function applySharedLayout(settings: OverlayLayoutSettings, acknowledge = false): Promise<void> {
    layoutSettings = structuredClone(settings);
    if (acknowledge) layoutAcknowledged = structuredClone(settings);
    const setup = activeOverlaySetup(layoutSettings);
    const map = setup.modules.map;
    preferences.moduleX = map.x * window.innerWidth; preferences.moduleY = map.y * window.innerHeight;
    preferences.moduleWidth = map.width * window.innerWidth; preferences.moduleHeight = map.height * window.innerHeight;
    const bindings: Array<[OverlayModuleId, HTMLElement, "showPlayer" | "showActions" | "showParty" | "showTarget" | "showObjectives" | "showAlerts" | null]> = [
      ["map", panel, null], ["player", playerPanel, "showPlayer"], ["actions", actionsPanel, "showActions"],
      ["party", partyPanel, "showParty"], ["target", targetPanel, "showTarget"],
      ["objectives", objectivesPanel, "showObjectives"], ["alerts", alertsPanel, "showAlerts"],
    ];
    for (const [id, element, visibility] of bindings) {
      const module = setup.modules[id];
      if (id !== "map") {
        const key = id as Exclude<OverlayModuleId, "map">;
        (preferences as unknown as Record<string, number>)[`${key}X`] = module.x * window.innerWidth;
        (preferences as unknown as Record<string, number>)[`${key}Y`] = module.y * window.innerHeight;
        (preferences as unknown as Record<string, number>)[`${key}Width`] = module.width * window.innerWidth;
        element.style.height = `${module.height * window.innerHeight}px`;
      } else {
        element.hidden = !module.visible;
      }
      if (visibility !== null) { preferences[visibility] = module.visible; element.hidden = !module.visible; }
      element.style.zIndex = String(module.zOrder);
      element.style.opacity = String(module.opacity);
      element.style.scale = String(module.scale);
      element.style.transformOrigin = "top left";
    }
    preferences.locked = setup.locked;
    root.dataset.locked = String(setup.locked); panel.dataset.locked = String(setup.locked);
    lock.textContent = setup.locked ? "Unlock" : "Lock"; lock.dataset.active = String(setup.locked);
    applyModuleGeometry(); scheduleDraw();
    await dependencies.setInteractive(!setup.locked);
  }

  function persistSharedLayout(): void {
    if (layoutSettings === null) return;
    const before = structuredClone(layoutSettings);
    const setup = activeOverlaySetup(layoutSettings);
    setGeometry(setup.modules.map, preferences.moduleX, preferences.moduleY, preferences.moduleWidth, preferences.moduleHeight);
    for (const id of ["player", "actions", "party", "target", "objectives", "alerts"] as const) {
      const values = preferences as unknown as Record<string, number>;
      const element = { player: playerPanel, actions: actionsPanel, party: partyPanel, target: targetPanel, objectives: objectivesPanel, alerts: alertsPanel }[id];
      setGeometry(setup.modules[id], values[`${id}X`]!, values[`${id}Y`]!, values[`${id}Width`]!, element.offsetHeight || parseFloat(element.style.height) || 120);
    }
    setup.modules.player.visible = preferences.showPlayer; setup.modules.actions.visible = preferences.showActions;
    setup.modules.party.visible = preferences.showParty; setup.modules.target.visible = preferences.showTarget;
    setup.modules.objectives.visible = preferences.showObjectives; setup.modules.alerts.visible = preferences.showAlerts;
    setup.locked = preferences.locked;
    enqueueLayoutChanges(before, layoutSettings);
    if (!layoutSaving) void flushSharedLayout();
  }

  function enqueueLayoutChanges(before: OverlayLayoutSettings, after: OverlayLayoutSettings): void {
    const setupId = after.selectedSetupId;
    const beforeSetup = before.setups[setupId];
    const afterSetup = after.setups[setupId];
    if (beforeSetup === undefined || afterSetup === undefined) return;
    if (beforeSetup.locked !== afterSetup.locked) {
      const locked = afterSetup.locked;
      layoutOperations.push((settings) => { const setup = settings.setups[setupId]; if (setup) setup.locked = locked; });
    }
    for (const id of ["map", "player", "actions", "party", "target", "objectives", "alerts"] as const) {
      for (const key of ["x", "y", "width", "height", "visible", "zOrder", "opacity", "scale"] as const) {
        if (beforeSetup.modules[id][key] === afterSetup.modules[id][key]) continue;
        const value = afterSetup.modules[id][key];
        layoutOperations.push((settings) => {
          const setup = settings.setups[setupId];
          if (setup) Object.assign(setup.modules[id], { [key]: value });
        });
      }
    }
  }

  function rebasedLayout(): OverlayLayoutSettings | null {
    if (layoutAcknowledged === null) return null;
    const draft = structuredClone(layoutAcknowledged);
    for (const operation of layoutOperations) operation(draft);
    return draft;
  }

  async function flushSharedLayout(): Promise<void> {
    if (layoutSaving) return;
    layoutSaving = true;
    try {
      while (alive && layoutAcknowledged !== null && layoutOperations.length > 0) {
        const operationCount = layoutOperations.length;
        const candidate = rebasedLayout();
        if (candidate === null) break;
        try {
          const saved = await dependencies.saveLayout(candidate);
          layoutOperations.splice(0, operationCount);
          layoutAcknowledged = structuredClone(saved);
          const draft = rebasedLayout() ?? saved;
          await applySharedLayout(draft, layoutOperations.length === 0);
        } catch (error) {
          if (!(error instanceof LocalHostHttpError) || error.status !== 409) throw error;
          const fresh = await dependencies.loadLayout();
          layoutAcknowledged = structuredClone(fresh);
          const draft = rebasedLayout();
          if (draft !== null) await applySharedLayout(draft);
        }
      }
    } catch (error) {
      console.error("Could not save shared overlay layout", error);
    } finally {
      layoutSaving = false;
    }
  }

  function bringToFront(id: OverlayModuleId, element: HTMLElement): void {
    if (layoutSettings === null || preferences.locked) return;
    const before = structuredClone(layoutSettings);
    const setup = activeOverlaySetup(layoutSettings);
    const next = raiseOverlayModule(setup, id);
    element.style.zIndex = String(next);
    enqueueLayoutChanges(before, layoutSettings);
    persistSharedLayout();
  }

  function applyModuleGeometry(): void {
    if (preferences.expanded) {
      panel.style.left = "0";
      panel.style.top = "0";
      panel.style.width = `${Math.max(1, window.innerWidth)}px`;
      panel.style.height = `${Math.max(1, window.innerHeight)}px`;
    } else {
      const maximumX = Math.max(0, window.innerWidth - preferences.moduleWidth);
      const maximumY = Math.max(0, window.innerHeight - preferences.moduleHeight);
      preferences.moduleX = Math.min(maximumX, Math.max(0, preferences.moduleX));
      preferences.moduleY = Math.min(maximumY, Math.max(0, preferences.moduleY));
      panel.style.left = `${preferences.moduleX}px`;
      panel.style.top = `${preferences.moduleY}px`;
      panel.style.width = `${Math.min(window.innerWidth, preferences.moduleWidth)}px`;
      panel.style.height = `${Math.min(window.innerHeight, preferences.moduleHeight)}px`;
    }
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
    if (preferences.expanded || moduleDrag?.pointerId !== event.pointerId) return;
    preferences.moduleX = moduleDrag.left + event.clientX - moduleDrag.x;
    preferences.moduleY = moduleDrag.top + event.clientY - moduleDrag.y;
    applyModuleGeometry();
  }

  function endModuleDrag(event: PointerEvent): void {
    if (moduleDrag?.pointerId !== event.pointerId) return;
    moduleDrag = null;
    savePreferences();
    persistSharedLayout();
  }

  function resizeModule(event: PointerEvent): void {
    if (preferences.expanded || moduleResize?.pointerId !== event.pointerId) return;
    preferences.moduleWidth = Math.max(260, moduleResize.width + event.clientX - moduleResize.x);
    preferences.moduleHeight = Math.max(260, moduleResize.height + event.clientY - moduleResize.y);
    applyModuleGeometry();
  }

  function endModuleResize(event: PointerEvent): void {
    if (moduleResize?.pointerId !== event.pointerId) return;
    moduleResize = null;
    savePreferences();
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
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
    persistSharedLayout();
  }

  return {
    dispose() {
      alive = false;
      automarkerRequestGeneration += 1;
      if (frame !== null) cancelAnimationFrame(frame);
      stopTargetTimer();
      stopPlayerTimer();
      stopActionsTimer();
      stopObjectivesTimer();
      stopAlertsTimer();
      resizeObserver.disconnect();
      window.removeEventListener("resize", handleScreenResize);
      window.removeEventListener("storage", handlePreviewStorage);
      window.removeEventListener("keydown", handleEscape);
      if (automarkerPreviewTimer !== null) window.clearInterval(automarkerPreviewTimer);
      if (layoutPollTimer !== null) window.clearInterval(layoutPollTimer);
      window.localStorage.removeItem(AUTOMARKER_PREVIEW_STORAGE_KEY);
      removeInteractivityListener?.();
      removeLayoutRefreshListener?.();
      removeFocusHeldListener?.();
      image = null;
      root.remove();
    },
  };
}

export function mechanicsMapAutomarkerSnapshotKey(
  snapshot: Pick<MechanicsMapSnapshot, "client_build" | "scene_id" | "map_id"> | null | undefined,
): string {
  return snapshot?.client_build === null || snapshot?.client_build === undefined ||
    snapshot.scene_id === null || snapshot.scene_id === undefined ||
    snapshot.map_id === null || snapshot.map_id === undefined
    ? "none"
    : `${snapshot.client_build}:${snapshot.scene_id}:${snapshot.map_id}`;
}

export function automarkerPresetViewSnapshotKey(view: AutomarkerPresetView): string {
  const context = view.context;
  return context === null
    ? "none"
    : `${context.clientBuild}:${context.sceneId}:${context.mapId}`;
}

function drawBackdrop(
  context: CanvasRenderingContext2D,
  snapshot: MechanicsMapSnapshot,
  rect: MechanicsMapCanvasRect,
  activeImage: HTMLImageElement | null,
  mapDim: number,
): void {
  context.fillStyle = "rgba(5, 13, 23, 0.92)";
  context.fillRect(rect.x, rect.y, rect.width, rect.height);
  if (snapshot.map_model === "absolute_scene_map" && activeImage) {
    context.globalAlpha = 0.9;
    context.drawImage(activeImage, rect.x, rect.y, rect.width, rect.height);
    context.globalAlpha = 1;
    context.fillStyle = `rgba(3, 11, 19, ${mapDim})`;
    context.fillRect(rect.x, rect.y, rect.width, rect.height);
  }
}

export function mechanicsMapAssetAvailability(
  snapshot: Pick<MechanicsMapSnapshot, "map_model" | "background_asset_url">,
  imageReady: boolean,
): "ready" | "asset_pending" | "unsupported" {
  if (snapshot.map_model !== "absolute_scene_map" || snapshot.background_asset_url === null) {
    return "unsupported";
  }
  return imageReady ? "ready" : "asset_pending";
}

function mechanicsMapContentRect(
  snapshot: MechanicsMapSnapshot,
  width: number,
  height: number,
  activeImage: HTMLImageElement | null,
): MechanicsMapCanvasRect {
  if (snapshot.map_model !== "absolute_scene_map") return { x: 0, y: 0, width, height };
  return fitMechanicsMapCanvasRect(
    width,
    height,
    activeImage?.naturalWidth ?? 1,
    activeImage?.naturalHeight ?? 1,
  );
}

function drawRegions(
  context: CanvasRenderingContext2D,
  snapshot: MechanicsMapSnapshot,
  width: number,
  height: number,
  highContrast: boolean,
): void {
  const outline = highContrast ? "rgba(255,255,255,.96)" : null;
  for (const signal of snapshot.mechanics) {
    drawPolygon(context, projectCursedTombChargeRegion(snapshot, signal), width, height,
      signal.mechanic_kind === "clone_charge_right" ? "rgba(179,138,255,.38)" : "rgba(255,91,111,.38)", outline);
    const beam = projectCoralMatrixBeam(snapshot, signal);
    if (beam.length === 2) drawLine(context, beam[0]!, beam[1]!, width, height, "rgba(242,195,107,.98)", highContrast ? 5 : 3, highContrast);
  }
  for (const region of projectCoralPizzaRegions(snapshot)) {
    drawPolygon(context, region.points, width, height,
      region.kind === "pizza_purple" ? "rgba(179,138,255,.4)" : "rgba(255,157,92,.4)", outline);
  }
  for (const region of projectRaidFloorRegions(snapshot)) drawFloorRegion(context, region, width, height, highContrast);
  drawActiveRaidRings(context, snapshot, width, height, highContrast);
}

function drawEntities(
  context: CanvasRenderingContext2D,
  snapshot: MechanicsMapSnapshot,
  width: number,
  height: number,
  preferences: MechanicsMapCanvasPreferences,
  previewPoints: readonly AutomarkerPoint[],
): void {
  const sceneMap = snapshot.map_model === "absolute_scene_map";
  const entities = projectMechanicsMapEntities(snapshot, sceneMap ? false : preferences.rotateWithPlayer)
    .filter((entity) => entity.visible && (preferences.showMonsters || !["monster", "npc", "object"].includes(entity.kind)));
  const colors: Record<string, string> = {
    local: "#5ce4d4", party: "#6da9ff", boss: "#ff6f83", player: "#8aa2ba",
    monster: "#f2c36b", pet: "#b38aff", npc: "#8aa2ba", object: "#8aa2ba",
  };
  const readability = mechanicsMapReadabilityProfile(preferences.mapDim, preferences.highContrastMechanics);
  for (const entity of entities) {
    const x = entity.mapX / 100 * width;
    const y = entity.mapY / 100 * height;
    const radius = entity.kind === "boss" ? 8 : entity.kind === "local" ? 7 : 5;
    context.save();
    context.globalAlpha = entity.stale ? 0.32 : entity.dead ? 0.48 : 1;
    context.shadowBlur = readability.entityGlowBlur;
    context.shadowColor = colors[entity.kind] ?? "#8aa2ba";
    context.fillStyle = colors[entity.kind] ?? "#8aa2ba";
    context.strokeStyle = "rgba(4,12,20,.95)";
    context.lineWidth = readability.entityOutlineWidth;
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
      context.textBaseline = "alphabetic";
      drawOutlinedText(
        context,
        entity.display_name,
        entity.kind === "local" ? 0 : x,
        entity.kind === "local" ? radius + 14 : y + radius + 13,
        readability.labelHaloWidth,
      );
    }
    context.restore();
  }
  for (const marker of snapshot.markers) {
    if (marker.x === null || marker.z === null) continue;
    const point = projectMechanicsMapPoint(snapshot, marker.x, marker.z, sceneMap ? false : preferences.rotateWithPlayer);
    if (!point?.visible) continue;
    const x = point.mapX / 100 * width;
    const y = point.mapY / 100 * height;
    context.save();
    context.shadowBlur = readability.entityGlowBlur;
    context.shadowColor = "#f4d76b";
    context.fillStyle = "#f4d76b";
    context.strokeStyle = "rgba(4,12,20,.98)";
    context.lineWidth = readability.entityOutlineWidth;
    context.beginPath();
    context.arc(x, y, 11, 0, Math.PI * 2);
    context.fill();
    context.stroke();
    context.shadowBlur = 0;
    const markerLabel = mechanicsMapMarkerLabel(marker);
    if (markerLabel !== null) {
      context.fillStyle = "#ffffff";
      context.font = "950 13px system-ui";
      context.textAlign = "center";
      context.textBaseline = "middle";
      drawOutlinedText(context, markerLabel, x, y, 4);
    }
    context.restore();
  }
  for (const marker of projectAutomarkerPreviewMarkers(snapshot, previewPoints, sceneMap ? false : preferences.rotateWithPlayer)) {
    const x = marker.mapX / 100 * width;
    const y = marker.mapY / 100 * height;
    context.save();
    context.shadowBlur = 12;
    context.shadowColor = "#5ce4d4";
    context.fillStyle = "rgba(5, 24, 34, .94)";
    context.strokeStyle = "#5ce4d4";
    context.lineWidth = 4;
    context.beginPath();
    context.arc(x, y, 14, 0, Math.PI * 2);
    context.fill();
    context.stroke();
    context.shadowBlur = 0;
    context.fillStyle = "#ffffff";
    context.font = "950 15px system-ui";
    context.textAlign = "center";
    context.textBaseline = "middle";
    drawOutlinedText(context, marker.label, x, y, 5);
    context.font = "800 9px system-ui";
    drawOutlinedText(context, "PREVIEW", x, y + 23, 4);
    context.restore();
  }
  for (const annotation of projectVoidTowerMapAnnotations(snapshot)) {
    const x = annotation.mapX / 100 * width;
    const y = annotation.mapY / 100 * height;
    context.save();
    context.translate(x, y);
    context.lineWidth = 2;
    context.textAlign = "center";
    context.textBaseline = "middle";
    if (annotation.kind === "sticky_bomb_target") {
      context.fillStyle = "#ff765f";
      context.strokeStyle = "rgba(4, 12, 20, .95)";
      context.beginPath();
      context.arc(0, 0, 11, 0, Math.PI * 2);
      context.fill();
      context.stroke();
      context.fillStyle = "#071019";
      context.font = "950 15px system-ui";
      context.fillText("!", 0, 1);
    } else {
      const correct = annotation.kind === "correct_portal";
      context.fillStyle = correct ? "#61e69a" : "#9b8abd";
      context.strokeStyle = "rgba(4, 12, 20, .95)";
      context.rotate(Math.PI / 4);
      const halfSize = correct ? 10 : 9;
      context.fillRect(-halfSize, -halfSize, halfSize * 2, halfSize * 2);
      context.strokeRect(-halfSize, -halfSize, halfSize * 2, halfSize * 2);
      context.rotate(-Math.PI / 4);
      context.fillStyle = "#071019";
      context.font = "950 14px system-ui";
      context.fillText(correct ? "✓" : "×", 0, 1);
    }
    context.restore();
  }
}

export function projectAutomarkerPreviewMarkers(
  snapshot: MechanicsMapSnapshot,
  points: readonly AutomarkerPoint[],
  rotateWithPlayer: boolean,
): Array<{ markerNumber: number; label: string; mapX: number; mapY: number }> {
  return points.flatMap((point) => {
    const projected = projectMechanicsMapPoint(snapshot, point.x, point.z, rotateWithPlayer);
    return projected?.visible
      ? [{ markerNumber: point.markerNumber, label: String(point.markerNumber), mapX: projected.mapX, mapY: projected.mapY }]
      : [];
  });
}

export function mechanicsMapMarkerLabel(
  marker: Pick<MechanicsMapSnapshot["markers"][number], "marker_id" | "marker_number">,
): string | null {
  if (Number.isSafeInteger(marker.marker_number) && marker.marker_number! >= 1 && marker.marker_number! <= 6) {
    return String(marker.marker_number);
  }
  return marker.marker_id === null ? null : String(marker.marker_id);
}

function drawFloorRegion(context: CanvasRenderingContext2D, region: MechanicsMapProjectedRegion, width: number, height: number, highContrast: boolean): void {
  const danger = region.kind === "phase_edge" || region.kind === "phase_corner";
  drawPolygon(context, region.points, width, height, danger ? "rgba(255,91,111,.38)" : "rgba(179,138,255,.36)", highContrast ? "rgba(255,255,255,.96)" : null);
  if (!region.label || region.points.length === 0) return;
  const x = region.points.reduce((sum, point) => sum + point.mapX, 0) / region.points.length / 100 * width;
  const y = region.points.reduce((sum, point) => sum + point.mapY, 0) / region.points.length / 100 * height;
  context.fillStyle = "#f2f6fb";
  context.font = "800 12px system-ui";
  context.textAlign = "center";
  context.textBaseline = "middle";
  drawOutlinedText(context, region.label, x, y, highContrast ? 5 : 3);
}

function drawActiveRaidRings(context: CanvasRenderingContext2D, snapshot: MechanicsMapSnapshot, width: number, height: number, highContrast: boolean): void {
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
    context.globalAlpha = highContrast ? 0.82 : 0.55;
    context.lineWidth = Math.max(2, (outer - inner) / snapshot.map_span_x * width);
    context.beginPath();
    context.ellipse(center.mapX / 100 * width, center.mapY / 100 * height,
      radius / snapshot.map_span_x * width, radius / snapshot.map_span_z * height, 0, 0, Math.PI * 2);
    context.stroke();
    context.restore();
  }
}

function drawPolygon(context: CanvasRenderingContext2D, points: readonly MechanicsMapViewPoint[], width: number, height: number, fill: string, outline: string | null = null): void {
  if (points.length < 3) return;
  context.beginPath();
  context.moveTo(points[0]!.mapX / 100 * width, points[0]!.mapY / 100 * height);
  for (const point of points.slice(1)) context.lineTo(point.mapX / 100 * width, point.mapY / 100 * height);
  context.closePath();
  context.fillStyle = fill;
  context.fill();
  if (outline) {
    context.strokeStyle = "rgba(3,9,16,.96)";
    context.lineWidth = 7;
    context.stroke();
  }
  context.strokeStyle = outline ?? fill;
  context.lineWidth = outline ? 2.5 : 1.5;
  context.stroke();
}

function drawLine(context: CanvasRenderingContext2D, start: MechanicsMapViewPoint, end: MechanicsMapViewPoint, width: number, height: number, stroke: string, lineWidth: number, halo = false): void {
  context.beginPath();
  context.moveTo(start.mapX / 100 * width, start.mapY / 100 * height);
  context.lineTo(end.mapX / 100 * width, end.mapY / 100 * height);
  if (halo) {
    context.strokeStyle = "rgba(3,9,16,.96)";
    context.lineWidth = lineWidth + 6;
    context.stroke();
  }
  context.strokeStyle = stroke;
  context.lineWidth = lineWidth;
  context.stroke();
}

function drawOutlinedText(
  context: CanvasRenderingContext2D,
  value: string,
  x: number,
  y: number,
  haloWidth: number,
): void {
  const fill = context.fillStyle;
  context.strokeStyle = "rgba(3,9,16,.98)";
  context.lineWidth = haloWidth;
  context.lineJoin = "round";
  context.strokeText(value, x, y);
  context.fillStyle = fill;
  context.fillText(value, x, y);
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

function formatMechanicsMapCoordinate(value: number, localizer: UiLocalizer): string {
  return localizer.formatNumber(value, Number.isInteger(value) ? undefined : { maximumFractionDigits: 1 });
}

function formatMechanicsMapZoom(value: number): string {
  return value >= 10 ? value.toFixed(0) : value.toFixed(value < 2 ? 2 : 1).replace(/\.0+$/, "");
}

export function nextMapDim(value: number): number {
  const levels = [0, 0.16, 0.32, 0.48, 0.64] as const;
  return levels.find((level) => level > value + 0.001) ?? levels[0];
}

export interface MechanicsMapReadabilityProfile {
  mapDim: number;
  entityOutlineWidth: number;
  entityGlowBlur: number;
  labelHaloWidth: number;
}

export function mechanicsMapReadabilityProfile(
  mapDim: number,
  highContrastMechanics: boolean,
): MechanicsMapReadabilityProfile {
  return {
    mapDim: Math.min(0.8, Math.max(0, Number.isFinite(mapDim) ? mapDim : 0.32)),
    entityOutlineWidth: highContrastMechanics ? 4 : 3,
    entityGlowBlur: highContrastMechanics ? 14 : 10,
    labelHaloWidth: highContrastMechanics ? 5 : 3,
  };
}

function formatTargetHealth(current: number | null, maximum: number | null, localizer: UiLocalizer): string {
  const format = (value: number): string => localizer.formatNumber(value, { notation: "compact", maximumFractionDigits: 2 });
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

function finiteUnitInterval(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
}

function finiteBounded(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 10_000_000;
}
