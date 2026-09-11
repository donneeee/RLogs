export const OVERLAY_MODULE_IDS = ["map", "player", "actions", "party", "target", "objectives", "alerts"] as const;
export type OverlayModuleId = typeof OVERLAY_MODULE_IDS[number];

export interface OverlayModuleLayout {
  x: number; y: number; width: number; height: number;
  visible: boolean; zOrder: number; opacity: number; backgroundOpacity: number; scale: number;
}

export interface OverlaySetupLayout {
  name: string;
  locked: boolean;
  modules: Record<OverlayModuleId, OverlayModuleLayout>;
}

export interface OverlayLayoutSettings {
  schemaVersion: 2;
  revision: number;
  canvasEnabled: boolean;
  selectedSetupId: string;
  legacyMigrationComplete: boolean;
  setups: Record<string, OverlaySetupLayout>;
}

export function parseOverlayLayoutSettings(value: unknown): OverlayLayoutSettings {
  if (!record(value) || value.schemaVersion !== 2 || !safeInteger(value.revision) || typeof value.canvasEnabled !== "boolean" ||
      typeof value.selectedSetupId !== "string" || typeof value.legacyMigrationComplete !== "boolean" || !record(value.setups)) {
    throw new Error("The local host returned invalid overlay layout settings.");
  }
  const setups: Record<string, OverlaySetupLayout> = {};
  for (const [id, candidate] of Object.entries(value.setups)) {
    if (id.length < 1 || id.length > 64 || !record(candidate) || typeof candidate.name !== "string" ||
        typeof candidate.locked !== "boolean" || !record(candidate.modules)) throw new Error("Invalid overlay setup.");
    const modules = {} as Record<OverlayModuleId, OverlayModuleLayout>;
    for (const moduleId of OVERLAY_MODULE_IDS) modules[moduleId] = parseModule(candidate.modules[moduleId]);
    if (Object.keys(candidate.modules).length !== OVERLAY_MODULE_IDS.length) throw new Error("Invalid overlay module set.");
    setups[id] = { name: candidate.name, locked: candidate.locked, modules };
  }
  if (!(value.selectedSetupId in setups)) throw new Error("Selected overlay setup is unavailable.");
  return { schemaVersion: 2, revision: value.revision, canvasEnabled: value.canvasEnabled, selectedSetupId: value.selectedSetupId,
    legacyMigrationComplete: value.legacyMigrationComplete, setups };
}

export function activeOverlaySetup(settings: OverlayLayoutSettings): OverlaySetupLayout {
  return settings.setups[settings.selectedSetupId]!;
}

export function safeDefaultOverlayLayout(revision: number): OverlayLayoutSettings {
  const module = (x: number, y: number, width: number, height: number, zOrder: number): OverlayModuleLayout =>
    ({ x, y, width, height, visible: true, zOrder, opacity: 1, backgroundOpacity: 0, scale: 1 });
  return { schemaVersion: 2, revision, canvasEnabled: false, selectedSetupId: "default", legacyMigrationComplete: true, setups: { default: {
    name: "Default HUD", locked: false, modules: {
      map: module(.015, .15, .325, .65, 1), player: module(.015, .06, .263, .16, 2),
      actions: module(.015, .79, .325, .16, 3), party: module(.65, .06, .225, .48, 4),
      target: module(.363, .06, .263, .18, 5), objectives: module(.363, .45, .263, .28, 6),
      alerts: module(.65, .53, .225, .28, 7),
    },
  } } };
}

export function normalizedModuleGeometry(
  pixels: { x: number; y: number; width: number; height: number }, viewportWidth: number, viewportHeight: number,
  scale = 1,
): Pick<OverlayModuleLayout, "x" | "y" | "width" | "height"> {
  const vw = Math.max(1, viewportWidth); const vh = Math.max(1, viewportHeight);
  const moduleScale = clamp(scale, 0.5, 2);
  const width = clamp(pixels.width / vw, 0.08, 1 / moduleScale);
  const height = clamp(pixels.height / vh, 0.06, 1 / moduleScale);
  return {
    x: clamp(pixels.x / vw, 0, 1 - width * moduleScale),
    y: clamp(pixels.y / vh, 0, 1 - height * moduleScale),
    width,
    height,
  };
}

export function clampOverlayModulePixelGeometry(
  pixels: { x: number; y: number; width: number; height: number }, viewportWidth: number, viewportHeight: number,
  moduleScale = 1,
): { x: number; y: number; width: number; height: number } {
  const vw = Math.max(1, viewportWidth); const vh = Math.max(1, viewportHeight);
  const geometry = normalizedModuleGeometry(pixels, vw, vh, moduleScale);
  return {
    x: geometry.x * vw,
    y: geometry.y * vh,
    width: geometry.width * vw,
    height: geometry.height * vh,
  };
}

export function clampOverlayModule(module: OverlayModuleLayout): OverlayModuleLayout {
  const scale = clamp(module.scale, 0.5, 2);
  const width = clamp(module.width, 0.08, 1 / scale); const height = clamp(module.height, 0.06, 1 / scale);
  return { ...module, x: clamp(module.x, 0, 1 - width * scale), y: clamp(module.y, 0, 1 - height * scale), width, height,
    opacity: clamp(module.opacity, 0.2, 1), backgroundOpacity: clamp(module.backgroundOpacity, 0, 1),
    scale, zOrder: clamp(Math.round(module.zOrder), 0, 1000) };
}

export function raiseOverlayModule(setup: OverlaySetupLayout, selected: OverlayModuleId): number {
  let maximum = Math.max(...OVERLAY_MODULE_IDS.map((id) => setup.modules[id].zOrder));
  if (maximum >= 1000) {
    const ordered = [...OVERLAY_MODULE_IDS].sort((left, right) => setup.modules[left].zOrder - setup.modules[right].zOrder);
    let next = 0;
    for (const id of ordered) if (id !== selected) setup.modules[id].zOrder = next++;
    maximum = next - 1;
  }
  setup.modules[selected].zOrder = maximum + 1;
  return setup.modules[selected].zOrder;
}

function parseModule(value: unknown): OverlayModuleLayout {
  if (!record(value) || !finite(value.x) || !finite(value.y) || !finite(value.width) || !finite(value.height) ||
      typeof value.visible !== "boolean" || !safeInteger(value.zOrder) || !finite(value.opacity) ||
      (value.backgroundOpacity !== undefined && !finite(value.backgroundOpacity)) || !finite(value.scale)) {
    throw new Error("Invalid overlay module layout.");
  }
  return clampOverlayModule({ x: value.x, y: value.y, width: value.width, height: value.height,
    visible: value.visible, zOrder: value.zOrder, opacity: value.opacity,
    backgroundOpacity: value.backgroundOpacity === undefined ? 0 : value.backgroundOpacity, scale: value.scale });
}
function clamp(value: number, minimum: number, maximum: number): number { return Math.min(maximum, Math.max(minimum, value)); }
function finite(value: unknown): value is number { return typeof value === "number" && Number.isFinite(value); }
function safeInteger(value: unknown): value is number { return Number.isSafeInteger(value) && Number(value) >= 0; }
function record(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
