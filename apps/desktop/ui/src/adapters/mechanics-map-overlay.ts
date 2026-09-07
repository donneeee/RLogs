import type { MountedSurface } from "../shell/types";
import {
  projectCoralMatrixBeam,
  projectCoralPizzaRegions,
  projectCoralWaveRegion,
  projectCursedTombChargeRegion,
  projectMechanicsMapEntities,
  projectMechanicsMapPoint,
  projectRaidFloorRegions,
  projectTinaPizzaRegion,
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
  };
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
  let frame: number | null = null;
  let image: HTMLImageElement | null = null;
  let imageUrl: string | null = null;
  let imageReady = false;
  let preparingAsset = false;
  let removeInteractivityListener: (() => void) | null = null;

  const root = element("main", "overlay-canvas-runtime");
  const panel = element("section", "mechanics-map-overlay-runtime");
  panel.dataset.locked = String(preferences.locked);
  applyModuleGeometry();
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
  root.append(panel);
  container.replaceChildren(root);

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
  void connect();

  async function connect(): Promise<void> {
    try {
      update = await dependencies.loadSnapshot();
      if (!alive) return;
      renderState();
      while (alive) {
        update = await dependencies.waitForSnapshot(update.revision);
        if (!alive) return;
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
    scheduleDraw();
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

  return {
    dispose() {
      alive = false;
      if (frame !== null) cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      window.removeEventListener("resize", handleScreenResize);
      removeInteractivityListener?.();
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
