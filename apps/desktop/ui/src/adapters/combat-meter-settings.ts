export type PlayerDetailPresentation = "in_app_layer" | "popover";
export type HistoryPartyColorMode = "party_order" | "randomized" | "specialization";
export type HistoryPartyColumnId =
  | "player"
  | "damage"
  | "effectiveDamage"
  | "damageTaken"
  | "healing"
  | "effectiveHealing"
  | "shielding"
  | "hits"
  | "criticalRate"
  | "dps"
  | "encounterDps"
  | "hps"
  | "tps"
  | "rdps"
  | "rdpsGiven"
  | "rdpsReceived"
  | "apm"
  | "deaths";
export type HistoryPartySortDirection = "ascending" | "descending";
export type HistoryPlayerDetailMode = "damage" | "healing" | "defense";

export interface HistoryPartyViewSettings {
  id: string;
  label: string;
  columns: HistoryPartyColumnId[];
  widths: Partial<Record<HistoryPartyColumnId, number>>;
  sortKey: HistoryPartyColumnId;
  sortDirection: HistoryPartySortDirection;
  detailMode: HistoryPlayerDetailMode;
}

export const HISTORY_PARTY_COLUMN_IDS: readonly HistoryPartyColumnId[] = [
  "player", "damage", "effectiveDamage", "damageTaken", "healing",
  "effectiveHealing", "shielding", "hits", "criticalRate", "dps",
  "encounterDps", "hps", "tps", "rdps", "rdpsGiven", "rdpsReceived",
  "apm", "deaths",
] as const;

export const DEFAULT_HISTORY_PARTY_VIEWS: readonly HistoryPartyViewSettings[] = [
  {
    id: "damage",
    label: "Damage",
    columns: ["player", "damage", "dps", "encounterDps", "rdps", "deaths"],
    widths: { player: 360, damage: 120, dps: 105, encounterDps: 105, rdps: 105, deaths: 82 },
    sortKey: "encounterDps",
    sortDirection: "descending",
    detailMode: "damage",
  },
  {
    id: "rdps",
    label: "rDPS",
    columns: ["player", "damage", "rdps", "rdpsGiven", "rdpsReceived"],
    widths: {
      player: 360,
      damage: 120,
      rdps: 110,
      rdpsGiven: 130,
      rdpsReceived: 135,
    },
    sortKey: "rdps",
    sortDirection: "descending",
    detailMode: "damage",
  },
  {
    id: "healing",
    label: "Healing",
    columns: ["player", "effectiveHealing", "healing", "shielding", "hps", "deaths"],
    widths: { player: 360, effectiveHealing: 135, healing: 120, shielding: 110, hps: 105, deaths: 82 },
    sortKey: "hps",
    sortDirection: "descending",
    detailMode: "healing",
  },
  {
    id: "defense",
    label: "Defense",
    columns: ["player", "damageTaken", "tps", "deaths"],
    widths: { player: 360, damageTaken: 135, tps: 105, deaths: 82 },
    sortKey: "tps",
    sortDirection: "descending",
    detailMode: "defense",
  },
] as const;

export const HISTORY_PARTY_PALETTE = [
  "#5eead4", "#60a5fa", "#f472b6", "#fbbf24", "#a78bfa",
  "#4ade80", "#fb7185", "#22d3ee", "#f97316", "#c084fc",
  "#2dd4bf", "#e879f9", "#84cc16", "#38bdf8", "#facc15",
  "#34d399", "#818cf8", "#f43f5e", "#14b8a6", "#d946ef",
] as const;

export function historySeededPaletteColor(seed: string, index: number): string {
  const steps = [1, 3, 7, 9, 11, 13, 17, 19] as const;
  const hash = historyColorHash(seed);
  const offset = hash % HISTORY_PARTY_PALETTE.length;
  const step = steps[(hash >>> 8) % steps.length]!;
  return HISTORY_PARTY_PALETTE[(offset + Math.max(0, index) * step) % HISTORY_PARTY_PALETTE.length]!;
}

export function historySpecializationFallbackColor(specializationId: string | number): string {
  return historySeededPaletteColor(`specialization:${specializationId}`, 0);
}

function historyColorHash(value: string): number {
  let hash = 2_166_136_261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16_777_619);
  }
  return hash >>> 0;
}

export interface CombatMeterSettings {
  schemaVersion: 1;
  playerDetailPresentation: PlayerDetailPresentation;
  showClass: boolean;
  showSpecialization: boolean;
  showLevel: boolean;
  showAbilityScore: boolean;
  showSeasonalScore: boolean;
  showCharacterUid: boolean;
  showPartyIcons: boolean;
  showWeapon: boolean;
  showPrimaryImagines: boolean;
  showRoleLoadout: boolean;
  showHistoryPlayerColumn: boolean;
  showHistoryDamageColumn: boolean;
  showHistoryDpsColumn: boolean;
  showHistoryEncounterDpsColumn: boolean;
  showHistoryHpsColumn: boolean;
  showHistoryTpsColumn: boolean;
  showHistoryRdpsColumn: boolean;
  showHistoryApmColumn: boolean;
  showHistoryDeathsColumn: boolean;
  historyPartyViews: HistoryPartyViewSettings[];
  historyPartyColorMode: HistoryPartyColorMode;
  historySpecializationColors: Record<string, string>;
  historyBodyFontSizePx: number;
  historyHeadingFontSizePx: number;
  historyTableFontSizePx: number;
  historyMetadataFontSizePx: number;
  historyMetricFontSizePx: number;
  historyIconSizePx: number;
}

export const DEFAULT_COMBAT_METER_SETTINGS: CombatMeterSettings = {
  schemaVersion: 1,
  playerDetailPresentation: "in_app_layer",
  showClass: true,
  showSpecialization: true,
  showLevel: true,
  showAbilityScore: true,
  showSeasonalScore: true,
  showCharacterUid: true,
  showPartyIcons: true,
  showWeapon: true,
  showPrimaryImagines: true,
  showRoleLoadout: true,
  showHistoryPlayerColumn: true,
  showHistoryDamageColumn: true,
  showHistoryDpsColumn: true,
  showHistoryEncounterDpsColumn: true,
  showHistoryHpsColumn: true,
  showHistoryTpsColumn: true,
  showHistoryRdpsColumn: true,
  showHistoryApmColumn: true,
  showHistoryDeathsColumn: true,
  historyPartyViews: DEFAULT_HISTORY_PARTY_VIEWS.map(cloneHistoryPartyView),
  historyPartyColorMode: "party_order",
  historySpecializationColors: {},
  historyBodyFontSizePx: 15,
  historyHeadingFontSizePx: 24,
  historyTableFontSizePx: 13,
  historyMetadataFontSizePx: 11,
  historyMetricFontSizePx: 18,
  historyIconSizePx: 48,
};

export function parseCombatMeterSettings(value: unknown): CombatMeterSettings {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Combat Meter settings must be an object.");
  }
  const settings = value as Record<string, unknown>;
  if (settings.schemaVersion !== 1) {
    throw new Error("Combat Meter settings use an unsupported schema.");
  }
  if (
    settings.playerDetailPresentation !== "in_app_layer" &&
    settings.playerDetailPresentation !== "popover"
  ) {
    throw new Error("Combat Meter player detail presentation is invalid.");
  }
  if (settings.showAbilityScore === undefined) {
    settings.showAbilityScore = DEFAULT_COMBAT_METER_SETTINGS.showAbilityScore;
  }
  if (settings.showCharacterUid === undefined) {
    settings.showCharacterUid = DEFAULT_COMBAT_METER_SETTINGS.showCharacterUid;
  }
  if (settings.showWeapon === undefined) {
    settings.showWeapon = DEFAULT_COMBAT_METER_SETTINGS.showWeapon;
  }
  if (settings.showPrimaryImagines === undefined) {
    settings.showPrimaryImagines = DEFAULT_COMBAT_METER_SETTINGS.showPrimaryImagines;
  }
  if (settings.showRoleLoadout === undefined) {
    settings.showRoleLoadout = DEFAULT_COMBAT_METER_SETTINGS.showRoleLoadout;
  }
  for (const key of [
    "showHistoryPlayerColumn",
    "showHistoryDamageColumn",
    "showHistoryDpsColumn",
    "showHistoryEncounterDpsColumn",
    "showHistoryHpsColumn",
    "showHistoryTpsColumn",
    "showHistoryRdpsColumn",
    "showHistoryApmColumn",
    "showHistoryDeathsColumn",
  ] as const) {
    if (settings[key] === undefined) settings[key] = DEFAULT_COMBAT_METER_SETTINGS[key];
  }
  if (settings.historyPartyViews === undefined) {
    settings.historyPartyViews = legacyHistoryPartyViews(settings);
  }
  settings.historyPartyViews = parseHistoryPartyViews(settings.historyPartyViews);
  for (const [key, label] of [
    ["showClass", "class visibility"],
    ["showSpecialization", "specialization visibility"],
    ["showLevel", "level visibility"],
    ["showAbilityScore", "ability score visibility"],
    ["showSeasonalScore", "seasonal score visibility"],
    ["showCharacterUid", "character UID visibility"],
    ["showPartyIcons", "party icon visibility"],
    ["showWeapon", "weapon visibility"],
    ["showPrimaryImagines", "primary Imagine visibility"],
    ["showRoleLoadout", "role loadout visibility"],
    ["showHistoryPlayerColumn", "History Player column visibility"],
    ["showHistoryDamageColumn", "History Damage column visibility"],
    ["showHistoryDpsColumn", "History DPS column visibility"],
    ["showHistoryEncounterDpsColumn", "History eDPS column visibility"],
    ["showHistoryHpsColumn", "History HPS column visibility"],
    ["showHistoryTpsColumn", "History TPS column visibility"],
    ["showHistoryRdpsColumn", "History rDPS column visibility"],
    ["showHistoryApmColumn", "History APM column visibility"],
    ["showHistoryDeathsColumn", "History Deaths column visibility"],
  ] as const) {
    if (typeof settings[key] !== "boolean") {
      throw new Error(`Combat Meter ${label} is invalid.`);
    }
  }
  if (settings.historyPartyColorMode === undefined) {
    settings.historyPartyColorMode = DEFAULT_COMBAT_METER_SETTINGS.historyPartyColorMode;
  }
  if (
    settings.historyPartyColorMode !== "party_order" &&
    settings.historyPartyColorMode !== "randomized" &&
    settings.historyPartyColorMode !== "specialization"
  ) {
    throw new Error("Combat Meter History party color mode is invalid.");
  }
  if (settings.historySpecializationColors === undefined) {
    settings.historySpecializationColors = {};
  }
  if (
    typeof settings.historySpecializationColors !== "object" ||
    settings.historySpecializationColors === null ||
    Array.isArray(settings.historySpecializationColors)
  ) {
    throw new Error("Combat Meter History specialization colors are invalid.");
  }
  const specializationColors = Object.entries(
    settings.historySpecializationColors as Record<string, unknown>,
  );
  if (specializationColors.length > 256) {
    throw new Error("Combat Meter History specialization colors exceed 256 entries.");
  }
  for (const [key, color] of specializationColors) {
    if (!/^[A-Za-z0-9._:-]{1,64}$/.test(key) || typeof color !== "string" || !/^#[0-9A-Fa-f]{6}$/.test(color)) {
      throw new Error(`Combat Meter History specialization color ${key || "key"} is invalid.`);
    }
  }
  settings.historySpecializationColors = Object.fromEntries(
    specializationColors.map(([key, color]) => [key, (color as string).toLowerCase()]),
  );
  for (const [key, label, minimum, maximum] of [
    ["historyBodyFontSizePx", "History body font size", 11, 24],
    ["historyHeadingFontSizePx", "History heading font size", 16, 40],
    ["historyTableFontSizePx", "History table font size", 10, 24],
    ["historyMetadataFontSizePx", "History metadata font size", 9, 20],
    ["historyMetricFontSizePx", "History metric font size", 13, 36],
    ["historyIconSizePx", "History icon size", 20, 64],
  ] as const) {
    if (settings[key] === undefined) settings[key] = DEFAULT_COMBAT_METER_SETTINGS[key];
    const value = settings[key];
    if (typeof value !== "number" || !Number.isInteger(value) || value < minimum || value > maximum) {
      throw new Error(`${label} must be between ${minimum} and ${maximum} px.`);
    }
  }
  return settings as unknown as CombatMeterSettings;
}

export function cloneHistoryPartyView(view: HistoryPartyViewSettings): HistoryPartyViewSettings {
  return {
    ...view,
    columns: [...view.columns],
    widths: { ...view.widths },
  };
}

function legacyHistoryPartyViews(settings: Record<string, unknown>): HistoryPartyViewSettings[] {
  const legacyKeys: ReadonlyArray<readonly [HistoryPartyColumnId, string]> = [
    ["player", "showHistoryPlayerColumn"],
    ["damage", "showHistoryDamageColumn"],
    ["dps", "showHistoryDpsColumn"],
    ["encounterDps", "showHistoryEncounterDpsColumn"],
    ["hps", "showHistoryHpsColumn"],
    ["tps", "showHistoryTpsColumn"],
    ["rdps", "showHistoryRdpsColumn"],
    ["apm", "showHistoryApmColumn"],
    ["deaths", "showHistoryDeathsColumn"],
  ];
  const columns = legacyKeys
    .filter(([, key]) => settings[key] !== false)
    .map(([column]) => column);
  const view = cloneHistoryPartyView(DEFAULT_HISTORY_PARTY_VIEWS[0]!);
  view.columns = columns.length > 0 ? columns : ["player"];
  if (!view.columns.includes(view.sortKey)) {
    view.sortKey = view.columns[0]!;
    view.sortDirection = view.sortKey === "player" ? "ascending" : "descending";
  }
  return [view, ...DEFAULT_HISTORY_PARTY_VIEWS.slice(1).map(cloneHistoryPartyView)];
}

function parseHistoryPartyViews(value: unknown): HistoryPartyViewSettings[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > 12) {
    throw new Error("Combat Meter History must contain between 1 and 12 party views.");
  }
  const ids = new Set<string>();
  return value.map((candidate, viewIndex) => {
    if (typeof candidate !== "object" || candidate === null || Array.isArray(candidate)) {
      throw new Error(`Combat Meter History party view ${viewIndex + 1} is invalid.`);
    }
    const view = candidate as Record<string, unknown>;
    if (typeof view.id !== "string" || !/^[A-Za-z0-9_-]{1,40}$/.test(view.id) || ids.has(view.id)) {
      throw new Error(`Combat Meter History party view ${viewIndex + 1} has an invalid or duplicate ID.`);
    }
    ids.add(view.id);
    if (typeof view.label !== "string" || view.label.trim().length < 1 || view.label.trim().length > 32) {
      throw new Error(`Combat Meter History party view ${view.id} has an invalid label.`);
    }
    if (!Array.isArray(view.columns) || view.columns.length < 1 || view.columns.length > HISTORY_PARTY_COLUMN_IDS.length) {
      throw new Error(`Combat Meter History party view ${view.id} has invalid columns.`);
    }
    const columns = view.columns as unknown[];
    const columnSet = new Set<HistoryPartyColumnId>();
    for (const column of columns) {
      if (typeof column !== "string" || !HISTORY_PARTY_COLUMN_IDS.includes(column as HistoryPartyColumnId) || columnSet.has(column as HistoryPartyColumnId)) {
        throw new Error(`Combat Meter History party view ${view.id} contains an invalid or duplicate column.`);
      }
      columnSet.add(column as HistoryPartyColumnId);
    }
    if (typeof view.sortKey !== "string" || !columnSet.has(view.sortKey as HistoryPartyColumnId)) {
      throw new Error(`Combat Meter History party view ${view.id} sort column must be visible.`);
    }
    if (view.sortDirection !== "ascending" && view.sortDirection !== "descending") {
      throw new Error(`Combat Meter History party view ${view.id} sort direction is invalid.`);
    }
    const detailMode = view.detailMode ?? (
      view.id === "healing" ? "healing" : view.id === "defense" ? "defense" : "damage"
    );
    if (detailMode !== "damage" && detailMode !== "healing" && detailMode !== "defense") {
      throw new Error(`Combat Meter History party view ${view.id} detail mode is invalid.`);
    }
    const widthsValue = view.widths ?? {};
    if (typeof widthsValue !== "object" || widthsValue === null || Array.isArray(widthsValue)) {
      throw new Error(`Combat Meter History party view ${view.id} widths are invalid.`);
    }
    const widths: Partial<Record<HistoryPartyColumnId, number>> = {};
    for (const [column, width] of Object.entries(widthsValue as Record<string, unknown>)) {
      if (!HISTORY_PARTY_COLUMN_IDS.includes(column as HistoryPartyColumnId) || typeof width !== "number" || !Number.isInteger(width) || width < 24 || width > 800) {
        throw new Error(`Combat Meter History party view ${view.id} width for ${column} is invalid.`);
      }
      widths[column as HistoryPartyColumnId] = width;
    }
    return {
      id: view.id,
      label: view.label.trim(),
      columns: [...columnSet],
      widths,
      sortKey: view.sortKey as HistoryPartyColumnId,
      sortDirection: view.sortDirection,
      detailMode,
    };
  });
}
