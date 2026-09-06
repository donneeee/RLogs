import {
  COMBAT_OVERLAY_TOGGLE_ACTION_ID,
  mountHotkeyBinding,
} from "../../../../../apps/desktop/ui/src/adapters/hotkey-settings";

export type OverlayMetric = "dps" | "edps" | "adps" | "bdps" | "rdps" | "hps" | "tps";
export type OverlayBackgroundMode = "transparent" | "solid" | "custom";
export type OverlayBarColorMode = "random" | "class" | "specialization";
export type OverlayNumberFormat = "compact" | "detailed" | "full";
export type OverlayNumberFormatTarget =
  | "playerMetrics"
  | "percentages"
  | "summaryTotals"
  | "bossHealth"
  | "bossMetrics"
  | "skillValues"
  | "counts";
export type OverlayNumberFormats = Record<OverlayNumberFormatTarget, OverlayNumberFormat>;

const DEFAULT_NUMBER_FORMATS: OverlayNumberFormats = {
  playerMetrics: "detailed",
  percentages: "compact",
  summaryTotals: "detailed",
  bossHealth: "detailed",
  bossMetrics: "detailed",
  skillValues: "detailed",
  counts: "full",
};
export type OverlayHeaderField =
  | "rank"
  | "class_spec"
  | "name"
  | "weapon"
  | "main_imagines"
  | "damage"
  | "effective_damage"
  | "hp_damage"
  | "shield_damage"
  | "dps"
  | "edps"
  | "adps"
  | "bdps"
  | "rdps"
  | "hps"
  | "tps"
  | "healing"
  | "effective_healing"
  | "overheal"
  | "shielding"
  | "damage_taken"
  | "hits"
  | "critical_rate"
  | "casts"
  | "deaths"
  | "revives"
  | "rdps_damage"
  | "contribution_given"
  | "contribution_received"
  | "value"
  | "percent";
export type OverlaySummaryField =
  | "attempt_time"
  | "encounter_time"
  | "run_time"
  | "game_time"
  | "true_time"
  | "scene"
  | "team_dps"
  | "team_damage"
  | "boss_health";
export type OverlayButtonAction =
  | "cycle_metric"
  | "cycle_timer"
  | "cycle_segment"
  | "reset_encounter"
  | "toggle_visibility"
  | "open_history";

const MIN_OVERLAY_HEIGHT = 80;
// The clock label changes between Encounter, Game, True, and Run while the
// overlay is live. Keep its geometry invariant so neither the scene title nor
// adjacent controls move when the selected clock or its digits change.
const FIXED_TIMER_CONTROL_WIDTH = 116;

export interface OverlayButton {
  id: string;
  label: string;
  action: OverlayButtonAction;
  /** Fixed logical width. Zero keeps non-timer controls content-sized. */
  width: number;
}

export interface OverlayLayer {
  id: string;
  title: string;
  metric: OverlayMetric;
  x: number;
  y: number;
  width: number;
  headerFields: OverlayHeaderField[];
  headerWidths: Record<OverlayHeaderField, number>;
  hiddenHeaderLabels: OverlayHeaderField[];
  summaryFields: OverlaySummaryField[];
  summaryFieldWidths?: Partial<Record<OverlaySummaryField, number>>;
  summaryFieldRows: Partial<Record<OverlaySummaryField, number>>;
  /** Unified order for summary values and action controls. */
  summaryItemOrder: string[];
  /** Row placement for each key in `summaryItemOrder`. */
  summaryItemRows: Record<string, number>;
  hiddenSummaryLabels: OverlaySummaryField[];
  showBossDps: boolean;
  buttons: OverlayButton[];
}

export interface CombatOverlaySettings {
  schemaVersion: 1;
  canvasWidth: number;
  canvasHeight: number;
  opacityPercent: number;
  barOpacityPercent: number;
  summaryOpacityPercent: number;
  barColorMode: OverlayBarColorMode;
  barColorOverrides: Record<string, string>;
  /** Legacy fallback retained for settings written before per-category formatting. */
  numberFormat: OverlayNumberFormat;
  numberFormats: OverlayNumberFormats;
  backgroundMode: OverlayBackgroundMode;
  backgroundColor: string;
  backgroundOpacityPercent: number;
  customBackgroundRevision: number | null;
  liveOverlayEnabled: boolean;
  alwaysOnTop: boolean;
  clickThrough: boolean;
  autoHideOutsideCombat: boolean;
  autoHideDelaySeconds: number;
  /** Presentation cadence only; packet capture and persisted history remain lossless. */
  refreshIntervalMillis: number;
  dynamicHeight: boolean;
  allowLiveResize: boolean;
  /** Show the named header-view strip in addition to the Cycle metric control. */
  showViewTabs: boolean;
  maxVisiblePlayers: number;
  scalePercent: number;
  layers: OverlayLayer[];
}

/**
 * Long-poll timeouts return the last published snapshot so renderer health can
 * be confirmed without inventing a new feed revision. Those no-op responses
 * must not rebuild the entire overlay DOM: doing so once per second makes the
 * transparent WebView compositor visibly flash over the game.
 */
export function runtimeOverlayNeedsRender(
  previousRevision: number,
  nextRevision: number,
  settingsChanged: boolean,
  timerSettingsChanged: boolean,
): boolean {
  return nextRevision > previousRevision || settingsChanged || timerSettingsChanged;
}

export function runtimeOverlayRenderDelay(
  lastRenderMillis: number,
  nowMillis: number,
  refreshIntervalMillis: number,
): number {
  if (lastRenderMillis <= 0) return 0;
  return Math.max(0, refreshIntervalMillis - Math.max(0, nowMillis - lastRenderMillis));
}

interface OverlayGlobalTimerSettings {
  pauseOverlayTimersOutsideCombat: boolean;
  overlayTimerInactivitySeconds: number;
}

export function applyOverlayTimerPause(
  snapshot: OverlaySnapshot | null,
  policy: OverlayGlobalTimerSettings,
): OverlaySnapshot | null {
  if (
    snapshot === null ||
    !policy.pauseOverlayTimersOutsideCombat ||
    snapshot.last_hostile_micros == null ||
    snapshot.latest_event_micros == null
  ) return snapshot;
  const pauseAt = snapshot.last_hostile_micros
    + Math.max(0, policy.overlayTimerInactivitySeconds) * 1_000_000;
  const excess = Math.max(0, snapshot.latest_event_micros - pauseAt);
  if (excess === 0) return snapshot;
  const subtract = (value: number | null | undefined): number | null | undefined =>
    value == null ? value : Math.max(0, value - excess);
  return {
    ...snapshot,
    attempt_elapsed_micros: snapshot.encounter_terminal_micros == null
      ? subtract(snapshot.attempt_elapsed_micros)
      : snapshot.attempt_elapsed_micros,
    encounter_elapsed_micros: snapshot.encounter_terminal_micros == null
      ? subtract(snapshot.encounter_elapsed_micros)
      : snapshot.encounter_elapsed_micros,
    run_elapsed_micros: snapshot.run_terminal_micros == null
      ? subtract(snapshot.run_elapsed_micros)
      : snapshot.run_elapsed_micros,
    game_time_micros: subtract(snapshot.game_time_micros),
    true_time_micros: subtract(snapshot.true_time_micros),
  };
}

interface OverlayBadgePresentation {
  slot_id: number | null;
  ability_id: number | null;
  item_id: number | null;
  tier: number | null;
  level: number | null;
  level_min: number | null;
  level_max: number | null;
  badge_kind: string | null;
  label: string;
  icon_asset_path: string | null;
}

interface OverlayActorPresentation {
  character_id: string | null;
  class_id: number | null;
  specialization_id: number | null;
  class_name: string | null;
  specialization_name: string | null;
  class_spec_icon_asset_path: string | null;
  role: "damage" | "healer" | "tank" | null;
  accent: "damage_glow" | null;
  weapon: OverlayBadgePresentation | null;
  primary_imagines: readonly OverlayBadgePresentation[];
}

interface OverlayActor {
  actor_id: string;
  entity_uuid?: string | null;
  display_name: string | null;
  actor_kind?: string | null;
  monster_id?: number | null;
  current_hp?: number | null;
  max_hp?: number | null;
  dps: number;
  edps?: number | null;
  adps?: number | null;
  bdps?: number | null;
  hps: number;
  tps: number;
  rdps: number | null;
  reported_damage?: number;
  damage_during_combat?: number;
  effective_damage?: number;
  hp_damage?: number;
  shield_damage?: number;
  damage_taken?: number;
  rdps_damage?: number | null;
  rdps_contribution_given?: number | null;
  rdps_contribution_received?: number | null;
  reported_healing?: number;
  effective_healing?: number;
  overheal?: number;
  shielding?: number;
  casts?: number;
  hits?: number;
  critical_hits?: number;
  deaths?: number;
  revives?: number;
  rdps_skill_detail_truncated?: boolean;
  abilities?: readonly OverlayAbility[];
  presentation?: OverlayActorPresentation;
}

interface OverlaySnapshotActor extends OverlayActor {
  run_dps?: number | null;
  encounter_dps?: number | null;
  active_dps?: number | null;
}

interface OverlayAbility {
  ability_id: string;
  presentation_name?: string | null;
  icon_asset_path?: string | null;
  presentation_recount_group_id?: string | null;
  presentation_recount_group_name?: string | null;
  casts: number;
  hits: number;
  critical_hits: number;
  reported_damage: number;
  effective_damage: number;
  reported_healing: number;
  effective_healing: number;
  shielding: number;
  rdps_received_damage?: string;
  rdps_received_rate?: number;
  rdps_sources?: readonly OverlayAbilityRdpsSource[];
  rdps_unresolved_relationship_count?: number;
  rdps_given_damage?: string;
  rdps_given_rate?: number;
  rdps_grants?: readonly OverlayAbilityRdpsGrant[];
  rdps_support_effect?: boolean;
  rdps_effect_id?: string;
}

export interface OverlayAbilityRdpsSource {
  provider_actor_id: string;
  provider_name: string;
  effect_id: string;
  effect_name: string;
  attribution_component: string | null;
  attributed_rdps: string;
  rdps: number;
  damage_event_count: number;
}

export interface OverlayAbilityRdpsGrant {
  effect_id: string;
  effect_name: string;
  attribution_component: string | null;
  attributed_rdps: string;
  rdps: number;
  damage_event_count: number;
}

export interface OverlayDamageInfluence {
  effect_id: string;
  attribution_component?: string | null;
  provider_actor_id: string;
  provider_ability_id?: string | null;
  recipient_actor_id: string;
  affected_ability_id: string | null;
  damage_event_count: number;
  attributed_rdps?: string | null;
  damage_context_complete: boolean;
}

interface OverlayRdpsEffectPresentation {
  effect_id: string;
  presentation_name: string;
}

interface OverlaySnapshot {
  rdps_status?: string;
  scene_id?: number | null;
  combat_started_micros?: number | null;
  attempt_elapsed_micros?: number | null;
  attempt_damage_elapsed_micros?: number | null;
  encounter_elapsed_micros?: number | null;
  encounter_terminal_micros?: number | null;
  run_terminal_micros?: number | null;
  active_combat_micros?: number;
  run_elapsed_micros?: number | null;
  game_time_micros?: number | null;
  true_time_micros?: number | null;
  combat_active?: boolean;
  last_hostile_micros?: number | null;
  latest_event_micros?: number | null;
  combat_inactivity_timeout_micros?: number;
  rdps_damage_influences?: readonly OverlayDamageInfluence[];
  rdps_damage_influences_truncated?: boolean;
  rdps_effect_presentations?: readonly OverlayRdpsEffectPresentation[];
  actors: readonly OverlaySnapshotActor[];
}

interface OverlayBossPresentation {
  actor_id: string;
  monster_id: number;
  name: string;
  current_hp: number;
  max_hp: number;
  bdps: number;
  team_damage: number;
}

interface OverlayEncounterPresentation {
  scene_id?: number | null;
  scene_name?: string | null;
  bosses: readonly OverlayBossPresentation[];
  timer_source?: "reviewed_dungeon" | "ambient_inactivity" | string;
  run_projection?: OverlayRunProjection | null;
}

interface OverlayRunProjection {
  rdps_status?: string;
  total_run_time_micros?: number | null;
  game_time_micros?: number | null;
  true_time_micros?: number | null;
  views: readonly OverlayHistoryView[];
}

interface OverlayHistoryView {
  id: string;
  label: string;
  kind: string;
  elapsed_micros: number;
  active_combat_micros: number;
  actors: readonly (OverlayActor | OverlayHistoryActor)[];
  damage_influences?: readonly OverlayDamageInfluence[];
  rdps_effect_presentations?: readonly OverlayRdpsEffectPresentation[];
}

interface OverlayHistoryActor {
  actor_id: string;
  entity_uuid?: string | null;
  monster_id?: string | null;
  character_id?: string | null;
  display_name?: string | null;
  presentation_name?: string | null;
  actor_kind?: string | null;
  class_id?: number | null;
  specialization_id?: number | null;
  presentation_class_name?: string | null;
  presentation_specialization_name?: string | null;
  icon_asset_path?: string | null;
  weapon_icon_asset_path?: string | null;
  presentation_role?: "damage" | "healer" | "tank" | string | null;
  presentation_accent?: string | null;
  weapon_item_id?: number | null;
  weapon_presentation_name?: string | null;
  weapon_level?: number | null;
  weapon_level_min?: number | null;
  weapon_level_max?: number | null;
  weapon_badge_kind?: string | null;
  primary_loadout?: readonly OverlayHistoryLoadoutSlot[];
  damage: number;
  effective_damage: number;
  damage_taken: number;
  healing: number;
  effective_healing: number;
  shielding: number;
  hits: number;
  critical_hits: number;
  deaths: number;
  dps: number;
  encounter_dps: number;
  hps: number;
  tps: number;
  rdps?: number | null;
  rdps_damage?: number | null;
  rdps_contribution_given?: number | null;
  rdps_contribution_received?: number | null;
  observed_cast_events?: number;
  abilities?: readonly OverlayHistoryAbility[];
}

interface OverlayHistoryLoadoutSlot {
  slot_id: number;
  ability_id?: number | null;
  item_id?: number | null;
  tier?: number | null;
  presentation_name?: string | null;
  icon_asset_path?: string | null;
}

interface OverlayHistoryAbility {
  ability_id: string;
  presentation_name?: string | null;
  icon_asset_path?: string | null;
  casts: number;
  hits: number;
  critical_hits: number;
  damage: number;
  effective_damage: number;
  healing: number;
  effective_healing: number;
  shielding: number;
}

interface OverlayLiveUpdate {
  revision: number;
  snapshot: OverlaySnapshot | null;
  actor_presentations?: Readonly<Record<string, OverlayActorPresentation>>;
  encounter_presentation?: OverlayEncounterPresentation;
}

export function overlayActorsFromLiveUpdate(
  update: OverlayLiveUpdate | null | undefined,
): OverlayActor[] {
  const presentations = update?.actor_presentations ?? {};
  return (update?.snapshot?.actors ?? []).map((actor) => ({
    ...actor,
    damage_during_combat: actor.damage_during_combat ?? actor.reported_damage,
    dps: actor.run_dps ?? actor.encounter_dps ?? actor.dps,
    edps: actor.encounter_dps ?? actor.dps,
    adps: actor.active_dps ?? actor.dps,
    presentation: presentations[actor.actor_id],
  })).filter(isOverlayRosterActor);
}

interface MountedSurface {
  dispose(): void;
}

export interface CombatOverlayRuntimeWindow {
  close(): Promise<void>;
  hide(): Promise<void>;
  hideTemporarily(): Promise<void>;
  showIfRequested(): Promise<void>;
  setEnabled(enabled: boolean, automaticallyHidden: boolean): Promise<void>;
  setAutomaticallyHidden(hidden: boolean): Promise<void>;
  setAlwaysOnTop(value: boolean): Promise<void>;
  setSize(width: number, height: number): Promise<void>;
  setIgnoreCursorEvents(value: boolean): Promise<void>;
  startDragging(): Promise<void>;
  startResizeDragging(direction: OverlayResizeDirection): Promise<void>;
  heartbeat(consecutiveFailures: number, lastSuccessfulUpdateUnixMillis: number): Promise<void>;
  onShowRequested(handler: () => void): Promise<() => void>;
  onResized(handler: (width: number, height: number) => void): Promise<() => void>;
}

type OverlayResizeDirection = "East" | "South" | "SouthEast";

interface RenderOptions {
  mode: "preview" | "runtime";
  /** Override the actor-table empty state without changing renderer geometry. */
  emptyMessage?: string;
  selectedLayerId?: string | null;
  onSelectLayer?: (layerId: string) => void;
  onReorderLayers?: (source: string, target: string, placement: ReorderPlacement) => void;
  onMoveLayer?: (layerId: string, x: number, y: number) => void;
  onReorderHeaders?: (
    layerId: string,
    source: OverlayHeaderField,
    target: OverlayHeaderField,
    placement: ReorderPlacement,
  ) => void;
  onReorderButtons?: (
    layerId: string,
    source: string,
    target: string,
    placement: ReorderPlacement,
  ) => void;
  onReorderSummary?: (
    layerId: string,
    source: string,
    targetRow: number,
    target: string | null,
    placement: ReorderPlacement,
  ) => void;
  onResizeHeader?: (layerId: string, field: OverlayHeaderField, width: number) => void;
  onResizeSummary?: (layerId: string, field: OverlaySummaryField, width: number) => void;
  onResizeButton?: (layerId: string, buttonId: string, width: number) => void;
  onContextMenu?: (event: MouseEvent, target: ContextTarget) => void;
  onRuntimeAction?: (layerId: string, action: OverlayButtonAction) => void;
  onStartWindowDrag?: () => void;
  selectedActorByLayer?: ReadonlyMap<string, string>;
  selectedTimerByLayer?: ReadonlyMap<string, OverlaySummaryField>;
  selectedSegmentByLayer?: ReadonlyMap<string, string>;
  onSelectActor?: (layerId: string, actorId: string) => void;
  onCloseActor?: (layerId: string) => void;
  snapshot?: OverlaySnapshot | null;
  encounterPresentation?: OverlayEncounterPresentation | null;
}

type ContextTarget =
  | { kind: "canvas" }
  | { kind: "layer"; layerId: string }
  | { kind: "view"; layerId: string }
  | { kind: "header"; layerId: string; field: OverlayHeaderField }
  | { kind: "summary"; layerId: string }
  | { kind: "summary_item"; layerId: string; field: OverlaySummaryField }
  | { kind: "button"; layerId: string; buttonId: string };

interface ContextMenuEntry {
  label: string;
  action?: () => void;
  children?: readonly ContextMenuEntry[];
  danger?: boolean;
  disabled?: boolean;
  separatorBefore?: boolean;
}

interface OverlayViewPreset {
  title: string;
  metric: OverlayMetric;
  fields: readonly OverlayHeaderField[];
}

type ReorderPlacement = "before" | "after";

interface BarColorIdentity {
  key: string;
  label: string;
  kind: "class" | "specialization";
}

interface BarColorIdentityCatalog {
  classes: readonly { id: number; label: string }[];
  specializations: readonly { id: number; label: string }[];
}

const SAMPLE_ACTORS: readonly OverlayActor[] = [
  { actor_id: "3296036", display_name: "MarieRose", reported_damage: 2_918_531_400, effective_damage: 2_901_443_992, hp_damage: 2_744_993_112, shield_damage: 173_538_288, damage_taken: 7_105_200, dps: 4_864_219, edps: 5_241_801, bdps: 5_812_004, rdps_damage: 3_007_706_400, rdps: 5_012_844, rdps_contribution_given: 121_880_000, rdps_contribution_received: 32_705_000, reported_healing: 28_440_000, effective_healing: 20_521_200, overheal: 7_918_800, shielding: 8_220_000, hps: 34_202, tps: 11_842, casts: 462, hits: 3_824, critical_hits: 1_047, deaths: 1, revives: 2, abilities: sampleAbilities(1), presentation: samplePresentation("Marksman", "Falconry", 2000631, 3948, 5, 3969, 5) },
  { actor_id: "49564002", display_name: "killua", dps: 4_207_914, edps: 4_564_201, bdps: 4_921_330, rdps: 4_384_091, hps: 28_014, tps: 9_443, abilities: sampleAbilities(.84), presentation: samplePresentation("Twin Striker", "Formless", 2001503, 3948, 4, 3969, 3) },
  { actor_id: "26833907", display_name: "Wntr", dps: 2_816_310, edps: 3_010_886, bdps: 3_221_404, rdps: 3_052_771, hps: 1_284_912, tps: 13_208, abilities: sampleAbilities(.58), presentation: samplePresentation("Verdant Oracle", "Smite", 2001505, 3948, 3, 3969, 4) },
  { actor_id: "36458500", display_name: "Yatocchi", dps: 1_392_818, edps: 1_489_542, bdps: 1_623_110, rdps: 1_428_449, hps: 86_102, tps: 71_385, abilities: sampleAbilities(.3), presentation: samplePresentation("Shield Knight", "Shield", 2001508, 3948, 2, 3969, 2) },
  { actor_id: "133943681", display_name: "Umapyoi", dps: 284_770, edps: 303_112, bdps: 318_908, rdps: 301_442, hps: 192_551, tps: 8_614, abilities: sampleAbilities(.12), presentation: samplePresentation("Beat Performer", "Concerto", 2000901, 3948, 1, 3969, 1) },
];

const SAMPLE_SNAPSHOT: OverlaySnapshot = {
  scene_id: 6525,
  combat_active: true,
  combat_started_micros: 0,
  active_combat_micros: 187_000_000,
  attempt_elapsed_micros: 83_000_000,
  encounter_elapsed_micros: 187_000_000,
  run_elapsed_micros: 231_000_000,
  game_time_micros: 204_000_000,
  true_time_micros: 196_000_000,
  actors: SAMPLE_ACTORS,
};

const SAMPLE_ENCOUNTER_PRESENTATION: OverlayEncounterPresentation = {
  scene_id: 6525,
  scene_name: "Chaotic - Mech Facility",
  timer_source: "reviewed_dungeon",
  bosses: [
    { actor_id: "boss-1", monster_id: 1342, name: "Combat Mech 03", current_hp: 81_450_000, max_hp: 120_000_000, bdps: 8_245_118, team_damage: 1_541_836_991 },
    { actor_id: "boss-2", monster_id: 1343, name: "Super Mech 07", current_hp: 39_700_000, max_hp: 95_000_000, bdps: 4_918_442, team_damage: 919_748_654 },
  ],
  run_projection: {
    rdps_status: "partial_packet_proven_rules",
    total_run_time_micros: 231_000_000,
    game_time_micros: 204_000_000,
    true_time_micros: 196_000_000,
    views: [
      { id: "all", label: "Entire run", kind: "all", elapsed_micros: 204_000_000, active_combat_micros: 187_000_000, actors: SAMPLE_ACTORS },
      { id: "true-time", label: "True Time", kind: "true_time", elapsed_micros: 196_000_000, active_combat_micros: 179_000_000, actors: SAMPLE_ACTORS.map((actor) => scaleOverlayActor(actor, 0.97)) },
      { id: "mobbing", label: "Mobbing", kind: "mobbing", elapsed_micros: 121_000_000, active_combat_micros: 109_000_000, actors: SAMPLE_ACTORS.map((actor) => scaleOverlayActor(actor, 0.58)) },
      { id: "boss", label: "Boss", kind: "boss", elapsed_micros: 83_000_000, active_combat_micros: 78_000_000, actors: SAMPLE_ACTORS.map((actor) => scaleOverlayActor(actor, 0.42)) },
    ],
  },
};

const METRICS: readonly OverlayMetric[] = ["dps", "edps", "adps", "bdps", "rdps", "hps", "tps"];
const BAR_COLOR_PALETTE: readonly string[] = [
  "#63e5d6",
  "#62a8ff",
  "#f271b6",
  "#ffbc42",
  "#a984ff",
  "#75d36f",
  "#ff7b72",
  "#5bd6ef",
  "#e78cff",
  "#f09d51",
  "#8bd3c7",
  "#d4c36a",
];
const HEADER_FIELDS: readonly OverlayHeaderField[] = [
  "rank",
  "class_spec",
  "name",
  "weapon",
  "main_imagines",
  "damage",
  "effective_damage",
  "hp_damage",
  "shield_damage",
  "dps",
  "edps",
  "adps",
  "bdps",
  "rdps",
  "hps",
  "tps",
  "healing",
  "effective_healing",
  "overheal",
  "shielding",
  "damage_taken",
  "hits",
  "critical_rate",
  "casts",
  "deaths",
  "revives",
  "rdps_damage",
  "contribution_given",
  "contribution_received",
  "value",
  "percent",
];
const EDITABLE_HEADER_FIELDS = HEADER_FIELDS.filter((field) => field !== "value");
const HEADER_FIELD_GROUPS: ReadonlyArray<readonly [string, readonly OverlayHeaderField[]]> = [
  ["Identity", ["rank", "class_spec", "name", "weapon", "main_imagines"]],
  ["Damage", ["damage", "effective_damage", "hp_damage", "shield_damage", "dps", "edps", "adps", "bdps", "percent"]],
  ["Healing", ["healing", "effective_healing", "overheal", "shielding", "hps"]],
  ["Defense", ["damage_taken", "tps"]],
  ["Activity", ["hits", "critical_rate", "casts", "deaths", "revives"]],
  ["Contribution", ["rdps_damage", "rdps", "contribution_given", "contribution_received"]],
];
const SUMMARY_FIELDS: readonly OverlaySummaryField[] = [
  "attempt_time",
  "encounter_time",
  "run_time",
  "game_time",
  "true_time",
  "scene",
  "team_dps",
  "team_damage",
  "boss_health",
];
const DEFAULT_SUMMARY_FIELDS: readonly OverlaySummaryField[] = [
  "encounter_time",
  "scene",
  "team_dps",
  "team_damage",
  "boss_health",
];
const VIEW_PRESETS: Readonly<Record<string, OverlayViewPreset>> = {
  damage: { title: "Party damage", metric: "dps", fields: ["rank", "class_spec", "weapon", "main_imagines", "name", "damage", "dps", "edps", "adps", "percent"] },
  healing: { title: "Party healing", metric: "hps", fields: ["rank", "class_spec", "name", "healing", "effective_healing", "overheal", "shielding", "hps", "percent"] },
  defense: { title: "Party defense", metric: "tps", fields: ["rank", "class_spec", "name", "damage_taken", "tps", "deaths", "revives", "percent"] },
  contribution: { title: "Party contribution", metric: "rdps", fields: ["rank", "class_spec", "name", "rdps_damage", "rdps", "contribution_given", "contribution_received"] },
  activity: { title: "Party activity", metric: "dps", fields: ["rank", "class_spec", "name", "hits", "critical_rate", "casts", "deaths", "revives"] },
};
const CYCLING_VIEW_PRESETS: readonly OverlayViewPreset[] = [
  VIEW_PRESETS.damage!,
  VIEW_PRESETS.healing!,
  VIEW_PRESETS.defense!,
  VIEW_PRESETS.contribution!,
];
const DEFAULT_HEADER_WIDTHS: Readonly<Record<OverlayHeaderField, number>> = {
  rank: 30,
  class_spec: 32,
  name: 190,
  weapon: 32,
  main_imagines: 54,
  damage: 102,
  effective_damage: 112,
  hp_damage: 102,
  shield_damage: 102,
  dps: 90,
  edps: 90,
  adps: 90,
  bdps: 90,
  rdps: 90,
  hps: 90,
  tps: 90,
  healing: 102,
  effective_healing: 112,
  overheal: 92,
  shielding: 92,
  damage_taken: 108,
  hits: 62,
  critical_rate: 62,
  casts: 62,
  deaths: 62,
  revives: 62,
  rdps_damage: 108,
  contribution_given: 112,
  contribution_received: 112,
  value: 90,
  percent: 48,
};
const ACTIONS: readonly OverlayButtonAction[] = [
  "cycle_metric",
  "cycle_timer",
  "cycle_segment",
  "reset_encounter",
  "toggle_visibility",
  "open_history",
];

const NUMBER_FORMAT = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
let stylesInstalled = false;

export interface CombatOverlayVisibilityPlan {
  showNow: boolean;
  hideAfterMillis: number | null;
}

export function shouldKeepCombatVisibilityTimer(
  currentTimerKey: string | null,
  nextTimerKey: string,
  showNow: boolean,
): boolean {
  if (currentTimerKey === null) return false;
  if (currentTimerKey === nextTimerKey) return true;
  // The active-combat countdown already includes the reducer inactivity
  // timeout plus the user's delay. Preserve that exact deadline when the
  // reducer flips idle instead of adding the delay a second time.
  return !showNow && currentTimerKey.startsWith("combat:");
}

export function shouldIgnoreCombatOverlayCursor(
  _automaticallyHidden: boolean,
  clickThrough: boolean,
): boolean {
  // A natively hidden window cannot receive pointer input. Tying cursor
  // passthrough to the requested hidden state creates a dangerous split state
  // when Windows has not completed (or races) the hide: the overlay remains
  // visible but cannot be moved, hidden, or otherwise interacted with.
  return clickThrough;
}

/**
 * Converts reducer-owned combat state into overlay presentation timing.
 * Damage totals are deliberately ignored: retained totals cannot tell an
 * active pull from downtime between pulls.
 */
export function planCombatOverlayVisibility(
  settings: Pick<CombatOverlaySettings, "autoHideOutsideCombat" | "autoHideDelaySeconds">,
  snapshot: OverlaySnapshot | null,
): CombatOverlayVisibilityPlan {
  if (!settings.autoHideOutsideCombat) {
    return { showNow: true, hideAfterMillis: null };
  }
  if (snapshot?.combat_active === true) {
    const timeoutMicros = safeNonnegativeInteger(snapshot.combat_inactivity_timeout_micros);
    const lastHostileMicros = safeNonnegativeInteger(snapshot.last_hostile_micros);
    const latestEventMicros = safeNonnegativeInteger(snapshot.latest_event_micros);
    const elapsedMicros = lastHostileMicros === null || latestEventMicros === null
      ? 0
      : Math.max(0, latestEventMicros - lastHostileMicros);
    const remainingMicros = timeoutMicros === null || lastHostileMicros === null
      ? null
      : Math.max(0, timeoutMicros - elapsedMicros);
    return {
      showNow: true,
      hideAfterMillis: remainingMicros === null
        ? null
        : Math.ceil(remainingMicros / 1_000) + settings.autoHideDelaySeconds * 1_000,
    };
  }
  return {
    showNow: false,
    hideAfterMillis: settings.autoHideDelaySeconds * 1_000,
  };
}

export function mountCombatOverlayEditorSurface(
  container: HTMLElement,
  openLiveOverlay?: () => Promise<void>,
): MountedSurface {
  installStyles();
  let alive = true;
  let settings: CombatOverlaySettings | null = null;
  let selectedLayerId: string | null = null;
  let previewVisibilityDimmed = false;
  // A designer must always open with deterministic, populated rows. Live data is
  // still available on demand, but it may legitimately be empty between fights.
  let previewDataMode: "live" | "example" = "example";
  let previewLiveUpdate: OverlayLiveUpdate | null = null;
  let previewTimerSettings: OverlayGlobalTimerSettings = {
    pauseOverlayTimersOutsideCombat: true,
    overlayTimerInactivitySeconds: 3,
  };
  const selectedActorByLayer = new Map<string, string>();
  const selectedTimerByLayer = new Map<string, OverlaySummaryField>();
  const selectedSegmentByLayer = new Map<string, string>();

  const root = el("div", "combat-overlay-editor");
  const top = el("section", "content-card combat-overlay-editor-heading");
  const copy = el("div");
  copy.append(
    text("h2", "Combat Overlay designer"),
    text(
      "p",
      "The preview uses the exact renderer, saved layout, and current data used by the native live overlay.",
    ),
  );
  const actions = el("div", "combat-overlay-editor-actions");
  const openOverlay = button("Open live overlay", "primary-button");
  const save = button("Save layout", "secondary-button");
  const reset = button("Reset preview", "secondary-button");
  const addLayerButton = button("Add view", "secondary-button");
  actions.append(openOverlay, addLayerButton, save, reset);
  top.append(copy, actions);

  const status = text("p", "Loading Combat Overlay layout...", "combat-overlay-status");
  const workspace = el("div", "combat-overlay-editor-workspace");
  const previewShell = el("section", "combat-overlay-preview-shell");
  const previewLabel = el("header", "combat-overlay-preview-label");
  const previewScale = el("label", "combat-overlay-scale-control");
  const previewScaleInput = document.createElement("input");
  previewScaleInput.type = "range";
  previewScaleInput.min = "50";
  previewScaleInput.max = "200";
  previewScaleInput.step = "5";
  previewScaleInput.value = "100";
  const previewScaleValue = text("strong", "100%");
  previewScale.append(text("span", "Overlay scale"), previewScaleInput, previewScaleValue);
  const previewDimensions = el("div", "combat-overlay-dimension-controls");
  const previewWidth = inputField("Width", "460", "number");
  previewWidth.label.classList.add("combat-overlay-dimension-control");
  previewWidth.input.min = "320";
  previewWidth.input.max = "2560";
  previewWidth.input.step = "10";
  const previewHeight = inputField("Height", "520", "number");
  previewHeight.label.classList.add("combat-overlay-dimension-control");
  previewHeight.input.min = String(MIN_OVERLAY_HEIGHT);
  previewHeight.input.max = "1440";
  previewHeight.input.step = "10";
  previewDimensions.append(previewWidth.label, previewHeight.label);
  const previewControls = el("div", "combat-overlay-preview-controls");
  const previewData = selectField(
    "Preview data",
    [["live", "Live overlay"], ["example", "Example combat"]],
    "example",
  );
  previewData.label.classList.add("combat-overlay-preview-data-control");
  const refreshPreviewData = button("Refresh", "secondary-button combat-overlay-preview-refresh");
  refreshPreviewData.title = "Reload the current live combat state";
  refreshPreviewData.hidden = true;
  previewControls.append(previewData.label, refreshPreviewData, previewDimensions, previewScale);
  previewLabel.append(
    text("strong", "Inactive preview"),
    text("span", "Drag headers and controls to arrange them."),
    previewControls,
  );
  const canvas = el("div", "combat-overlay-canvas combat-overlay-canvas-preview");
  const previewResizeHandle = el("button", "combat-overlay-preview-resize-handle");
  previewResizeHandle.type = "button";
  previewResizeHandle.title = "Drag to resize the overlay width and height";
  previewResizeHandle.setAttribute("aria-label", "Resize overlay preview");
  previewShell.append(previewLabel, canvas);
  const inspector = el("aside", "content-card combat-overlay-inspector");
  const columnEditor = el("section", "content-card combat-overlay-column-editor-panel");
  columnEditor.hidden = true;
  const editorMain = el("div", "combat-overlay-editor-main");
  editorMain.append(previewShell, columnEditor);
  workspace.append(editorMain, inspector);
  root.append(top, status, workspace);
  container.replaceChildren(root);

  const render = () => {
    if (settings === null) return;
    if (
      selectedLayerId !== null &&
      !settings.layers.some((layer) => layer.id === selectedLayerId)
    ) {
      selectedLayerId = settings.layers[0]?.id ?? null;
    }
    const scale = overlayScale(settings);
    previewScaleInput.value = String(settings.scalePercent);
    previewScaleValue.textContent = `${settings.scalePercent}%`;
    previewWidth.input.value = String(settings.canvasWidth);
    previewHeight.input.value = String(settings.canvasHeight);
    previewHeight.input.disabled = settings.dynamicHeight;
    previewHeight.label.title = settings.dynamicHeight
      ? "Height follows the visible summary, table header, and player rows."
      : "Set a fixed overlay height.";
    previewResizeHandle.title = settings.dynamicHeight
      ? "Drag to resize overlay width"
      : "Drag to resize overlay width and height";
    previewResizeHandle.dataset.widthOnly = String(settings.dynamicHeight);
    canvas.style.width = `${Math.round(settings.canvasWidth * scale)}px`;
    canvas.style.height = `${Math.round(settings.canvasHeight * scale)}px`;
    canvas.style.setProperty("--overlay-opacity", String(settings.opacityPercent / 100));
    canvas.style.setProperty("--bar-opacity", String(settings.barOpacityPercent / 100));
    canvas.style.setProperty("--summary-opacity", String(settings.summaryOpacityPercent / 100));
    canvas.dataset.previewDimmed = String(previewVisibilityDimmed);
    const showingLiveData = previewDataMode === "live";
    const previewActors = showingLiveData
      ? overlayActorsFromLiveUpdate(previewLiveUpdate)
      : SAMPLE_ACTORS;
    const previewSnapshot = showingLiveData
      ? applyOverlayTimerPause(previewLiveUpdate?.snapshot ?? null, previewTimerSettings)
      : SAMPLE_SNAPSHOT;
    const previewPresentation = showingLiveData
      ? previewLiveUpdate?.encounter_presentation ?? null
      : SAMPLE_ENCOUNTER_PRESENTATION;
    renderOverlayCanvas(canvas, settings, previewActors, {
      mode: "preview",
      emptyMessage: showingLiveData ? "Waiting for combat..." : "No example rows",
      snapshot: previewSnapshot,
      encounterPresentation: previewPresentation,
      selectedLayerId,
      onSelectLayer(layerId) {
        selectedLayerId = layerId;
        render();
      },
      onReorderLayers(source, target, placement) {
        if (settings === null) return;
        settings = {
          ...settings,
          layers: moveObjectRelative(settings.layers, source, target, placement),
        };
        render();
      },
      onReorderHeaders(layerId, source, target, placement) {
        updateLayer(layerId, (layer) => ({
          ...layer,
          headerFields: moveRelative(layer.headerFields, source, target, placement),
        }));
      },
      onReorderButtons(layerId, source, target, placement) {
        updateLayer(layerId, (layer) => ({
          ...layer,
          buttons: moveObjectRelative(layer.buttons, source, target, placement),
        }));
      },
      onReorderSummary(layerId, source, targetRow, target, placement) {
        updateLayer(layerId, (layer) =>
          moveSummaryLayoutItem(layer, source, targetRow, target, placement));
      },
      onResizeHeader(layerId, field, width) {
        updateLayer(layerId, (layer) => ({
          ...layer,
          headerWidths: { ...layer.headerWidths, [field]: width },
        }));
      },
      onResizeSummary(layerId, field, width) {
        updateLayer(layerId, (layer) => ({
          ...layer,
          summaryFieldWidths: { ...layer.summaryFieldWidths, [field]: width },
        }));
      },
      onResizeButton(layerId, buttonId, width) {
        updateLayer(layerId, (layer) => ({
          ...layer,
          buttons: layer.buttons.map((value) => value.id === buttonId
            ? { ...value, width }
            : value),
        }));
      },
      onContextMenu: showContextMenu,
      onRuntimeAction(layerId, action) {
        if (action === "cycle_metric") {
          cycleLayerMetric(layerId);
          return;
        }
        if (action === "cycle_timer") {
          cycleSelectedTimer(
            selectedTimerByLayer,
            layerId,
            previewPresentation,
            previewSnapshot,
          );
          render();
          return;
        }
        if (action === "cycle_segment") {
          cycleSelectedSegment(
            selectedSegmentByLayer,
            layerId,
            previewPresentation,
          );
          selectedActorByLayer.delete(layerId);
          render();
          return;
        }
        if (action === "toggle_visibility") {
          previewVisibilityDimmed = !previewVisibilityDimmed;
          status.textContent = previewVisibilityDimmed
            ? "Preview action: the live overlay would now be hidden. Click the button again to restore the preview."
            : "Preview action: the live overlay would now be visible.";
          status.classList.remove("error");
          render();
          return;
        }
        status.textContent = action === "reset_encounter"
          ? "Preview action: Reset encounter is wired. The inactive preview has no live encounter to reset."
          : "Preview action: Open Combat History is wired. Navigation only occurs from the live overlay.";
        status.classList.remove("error");
      },
      selectedActorByLayer,
      selectedTimerByLayer,
      selectedSegmentByLayer,
      onSelectActor(layerId, actorId) {
        selectedActorByLayer.set(layerId, actorId);
        render();
      },
      onCloseActor(layerId) {
        selectedActorByLayer.delete(layerId);
        render();
      },
    });
    canvas.style.height = `${resolvedOverlayHeight(canvas, settings)}px`;
    canvas.append(previewResizeHandle);
    renderInspector();
  };

  const loadPreviewData = async () => {
    refreshPreviewData.disabled = true;
    try {
      const [update, timerSettings] = await Promise.all([
        apiJson<OverlayLiveUpdate>("/api/runtime/live/combat"),
        loadGlobalTimerSettings(),
      ]);
      if (!alive) return;
      previewLiveUpdate = update;
      previewTimerSettings = timerSettings;
      if (previewDataMode === "live") {
        status.textContent = update.snapshot === null
          ? "The live overlay is waiting for combat. Choose Example combat to arrange populated rows."
          : "Live preview refreshed from the current combat overlay state.";
        status.classList.remove("error");
        render();
      }
    } catch (error) {
      if (!alive) return;
      status.textContent = `Could not load live preview data: ${errorMessage(error)}`;
      status.classList.add("error");
    } finally {
      refreshPreviewData.disabled = false;
    }
  };

  const updateLayer = (
    layerId: string,
    update: (layer: OverlayLayer) => OverlayLayer,
  ) => {
    if (settings === null) return;
    settings = {
      ...settings,
      layers: settings.layers.map((layer) =>
        layer.id === layerId ? update(layer) : layer,
      ),
    };
    render();
  };

  const renderInspector = () => {
    if (settings === null) return;
    const layer = settings.layers.find((candidate) => candidate.id === selectedLayerId);
    inspector.replaceChildren();
    columnEditor.replaceChildren();
    columnEditor.hidden = true;
    if (!layer) {
      inspector.append(
        text("h3", "Header views"),
        text("p", "Add a header view to create its selector button and choose which columns it displays."),
      );
    } else {
      inspector.append(text("h3", "Selected header view"));
      const title = inputField("View button label", layer.title);
      const metric = selectField(
        "Sort and percentage metric",
        METRICS.map((value) => [value, metricLabel(value)] as const),
        layer.metric,
      );
      // The inspector is rebuilt after every saved edit, so commit text fields
      // on change rather than replacing the focused input on each keystroke.
      title.input.addEventListener("change", () =>
        updateLayer(layer.id, (value) => ({ ...value, title: title.input.value })),
      );
      metric.select.addEventListener("change", () =>
        setLayerMetric(layer.id, metric.select.value as OverlayMetric),
      );
      const summaryGroup = el("fieldset", "combat-overlay-inspector-group");
      summaryGroup.append(
        text("legend", "Summary section"),
        text(
          "p",
          "Checked items are shown above the player table; unchecked items are hidden. Drag shown items in the preview to reorder them. Width 0 means automatic.",
          "combat-overlay-inspector-hint",
        ),
      );
      const summaryGrid = el("div", "combat-overlay-summary-editor-grid");
      for (const field of SUMMARY_FIELDS) {
        const summaryRow = el("div", "combat-overlay-summary-editor");
        const option = checkbox(`Show ${summaryFieldLabel(field)}`, layer.summaryFields.includes(field));
        option.label.title = `${layer.summaryFields.includes(field) ? "Hide" : "Show"} ${summaryFieldLabel(field)} in this header view.`;
        const summaryWidth = inputField("Width", String(summaryFieldWidthFor(layer, field)), "number");
        summaryWidth.label.classList.add("combat-overlay-width-field");
        summaryWidth.label.title = "Use 0 for automatic width.";
        summaryWidth.input.min = "0";
        summaryWidth.input.max = "480";
        summaryWidth.input.step = "4";
        summaryWidth.input.disabled = !layer.summaryFields.includes(field);
        summaryWidth.input.setAttribute("aria-label", `${summaryFieldLabel(field)} width`);
        option.input.addEventListener("change", () =>
          updateLayer(layer.id, (value) => withNormalizedSummaryLayout({
            ...value,
            summaryFields: option.input.checked
              ? [...value.summaryFields, field]
              : value.summaryFields.filter((candidate) => candidate !== field),
          })),
        );
        summaryWidth.input.addEventListener("change", () =>
          updateLayer(layer.id, (value) => ({
            ...value,
            summaryFieldWidths: {
              ...value.summaryFieldWidths,
              [field]: clamp(Number(summaryWidth.input.value), 0, 480),
            },
          })),
        );
        summaryRow.append(option.label, summaryWidth.label);
        summaryGrid.append(summaryRow);
      }
      summaryGroup.append(summaryGrid);
      const bossDpsOption = checkbox("Show bDPS for each boss", layer.showBossDps);
      bossDpsOption.label.title = "Show each boss's target-specific damage per active-combat second on its HP row.";
      bossDpsOption.input.addEventListener("change", () =>
        updateLayer(layer.id, (value) => ({ ...value, showBossDps: bossDpsOption.input.checked })),
      );
      summaryGroup.append(bossDpsOption.label);
      const headerGroup = el("fieldset", "combat-overlay-inspector-group");
      headerGroup.append(
        text("legend", "Headers and column widths"),
        text(
          "p",
          "Drag the cyan dividers in the preview, or enter an exact width here.",
          "combat-overlay-inspector-hint",
        ),
      );
      const headerEditorGrid = el("div", "combat-overlay-header-editor-grid");
      for (const field of EDITABLE_HEADER_FIELDS) {
        const headerRow = el("div", "combat-overlay-header-editor");
        const option = checkbox(fieldLabel(field), layer.headerFields.includes(field));
        option.label.title = layer.headerFields.includes(field)
          ? `Remove the ${fieldLabel(field)} column. You can also right-click it in the preview.`
          : `Add the ${fieldLabel(field)} column.`;
        const headerWidth = inputField("Width", String(headerWidthFor(layer, field)), "number");
        headerWidth.label.classList.add("combat-overlay-width-field");
        headerWidth.input.min = "0";
        headerWidth.input.max = "480";
        headerWidth.input.step = "4";
        option.input.addEventListener("change", () => {
          if (!option.input.checked && layer.headerFields.length === 1) {
            option.input.checked = true;
            return;
          }
          updateLayer(layer.id, (value) => ({
            ...value,
            headerFields: option.input.checked
              ? insertHeaderField(value.headerFields, field)
              : value.headerFields.filter((candidate) => candidate !== field),
            hiddenHeaderLabels: option.input.checked
              ? value.hiddenHeaderLabels
              : value.hiddenHeaderLabels.filter((candidate) => candidate !== field),
          }));
        });
        headerWidth.input.addEventListener("change", () =>
          updateLayer(layer.id, (value) => ({
            ...value,
            headerWidths: {
              ...value.headerWidths,
              [field]: clamp(Number(headerWidth.input.value), 0, 480),
            },
          })),
        );
        headerRow.append(option.label, headerWidth.label);
        headerEditorGrid.append(headerRow);
      }
      headerGroup.append(headerEditorGrid);
      columnEditor.append(headerGroup);
      columnEditor.hidden = false;
      const buttonGroup = el("div", "combat-overlay-inspector-group");
      buttonGroup.append(
        text("strong", "Buttons"),
        text(
          "p",
          "Add, rename, or remove controls shown in this header view.",
          "combat-overlay-inspector-hint",
        ),
      );
      for (const overlayButton of layer.buttons) {
        const row = el("div", "combat-overlay-button-editor");
        const label = inputField("Label", overlayButton.label);
        const action = selectField(
          "Function",
          ACTIONS.map((value) => [value, actionLabel(value)] as const),
          overlayButton.action,
        );
        const controlWidth = inputField("Width", String(buttonWidthFor(overlayButton)), "number");
        const timerWidthIsFixed = overlayButton.action === "cycle_timer";
        controlWidth.input.min = timerWidthIsFixed
          ? String(FIXED_TIMER_CONTROL_WIDTH)
          : "0";
        controlWidth.input.max = "480";
        controlWidth.input.step = "1";
        controlWidth.input.disabled = timerWidthIsFixed;
        controlWidth.input.title = timerWidthIsFixed
          ? "The timer width is fixed so changing clocks cannot move the rest of the header."
          : "Use 0 for automatic width, or enter a fixed logical-pixel width.";
        label.input.addEventListener("change", () =>
          updateButton(layer.id, overlayButton.id, {
            ...overlayButton,
            label: label.input.value.trim() || overlayButton.label,
          }),
        );
        action.select.addEventListener("change", () =>
          updateButton(layer.id, overlayButton.id, {
            ...overlayButton,
            action: action.select.value as OverlayButtonAction,
            width: action.select.value === "cycle_timer"
              ? FIXED_TIMER_CONTROL_WIDTH
              : overlayButton.action === "cycle_timer"
                ? 0
                : overlayButton.width,
          }),
        );
        controlWidth.input.addEventListener("change", () =>
          updateButton(layer.id, overlayButton.id, {
            ...overlayButton,
            width: clamp(
              Number(controlWidth.input.value),
              0,
              480,
            ),
          }),
        );
        const remove = button("Remove", "secondary-button danger-button combat-overlay-remove-control");
        remove.title = `Remove the ${overlayButton.label} control from this view`;
        remove.addEventListener("click", () => deleteButton(layer.id, overlayButton.id));
        row.append(label.label, action.label, controlWidth.label, remove);
        buttonGroup.append(row);
      }
      const addControlRow = el("div", "combat-overlay-add-control");
      const newControlAction = selectField(
        "New control function",
        ACTIONS.map((value) => [value, actionLabel(value)] as const),
        "open_history",
      );
      const addControl = button("Add control", "secondary-button");
      addControl.addEventListener("click", () => {
        const action = newControlAction.select.value as OverlayButtonAction;
        addButton(layer.id, action, defaultButtonLabel(action));
      });
      addControlRow.append(newControlAction.label, addControl);
      buttonGroup.append(addControlRow);
      const viewActions = el("div", "combat-overlay-view-editor-actions");
      const duplicateView = button("Duplicate view", "secondary-button");
      duplicateView.addEventListener("click", () => duplicateLayer(layer.id));
      const deleteView = button("Delete view", "secondary-button danger-button");
      deleteView.disabled = settings.layers.length <= 1;
      deleteView.title = settings.layers.length <= 1
        ? "The overlay must keep at least one header view."
        : `Delete ${layer.title}`;
      deleteView.addEventListener("click", () => deleteLayer(layer.id));
      viewActions.append(duplicateView, deleteView);
      inspector.append(
        title.label,
        metric.label,
        summaryGroup,
        buttonGroup,
        viewActions,
      );
    }

    const appearanceGroup = el("fieldset", "combat-overlay-inspector-group");
    appearanceGroup.append(
      text("legend", "Overlay appearance"),
      text(
        "p",
        "These controls update the exact preview and live overlay surface.",
        "combat-overlay-inspector-hint",
      ),
    );
    const overlayOpacity = inputField(
      "Overall opacity (20-100%)",
      String(settings.opacityPercent),
      "number",
    );
    overlayOpacity.input.min = "20";
    overlayOpacity.input.max = "100";
    const barOpacity = inputField(
      "Colored bar opacity (0-100%)",
      String(settings.barOpacityPercent),
      "number",
    );
    barOpacity.input.min = "0";
    barOpacity.input.max = "100";
    const summaryOpacity = inputField(
      "Summary background opacity (0-100%)",
      String(settings.summaryOpacityPercent),
      "number",
    );
    summaryOpacity.input.min = "0";
    summaryOpacity.input.max = "100";
    const showViewTabs = checkbox(
      "Show named header-view tabs",
      settings.showViewTabs,
    );
    showViewTabs.label.title = "Turn this off to reclaim the space used by labels such as Party damage. Cycle metric still switches complete header views.";
    overlayOpacity.input.addEventListener("change", () => {
      settings = {
        ...settings!,
        opacityPercent: clamp(Number(overlayOpacity.input.value), 20, 100),
      };
      render();
    });
    barOpacity.input.addEventListener("change", () => {
      settings = {
        ...settings!,
        barOpacityPercent: clamp(Number(barOpacity.input.value), 0, 100),
      };
      render();
    });
    summaryOpacity.input.addEventListener("change", () => {
      settings = {
        ...settings!,
        summaryOpacityPercent: clamp(Number(summaryOpacity.input.value), 0, 100),
      };
      render();
    });
    showViewTabs.input.addEventListener("change", () => {
      settings = { ...settings!, showViewTabs: showViewTabs.input.checked };
      render();
    });
    appearanceGroup.append(
      overlayOpacity.label,
      barOpacity.label,
      summaryOpacity.label,
      showViewTabs.label,
    );

    const backgroundGroup = el("fieldset", "combat-overlay-inspector-group");
    backgroundGroup.append(text("legend", "Overlay background"));
    const backgroundMode = selectField(
      "Style",
      [
        ["transparent", "Transparent"],
        ["solid", "Solid tint"],
        ...(settings.customBackgroundRevision === null
          ? []
          : [["custom", "Custom image"] as const]),
      ],
      settings.backgroundMode,
    );
    const backgroundColor = inputField("Tint color", settings.backgroundColor, "color");
    const backgroundOpacity = inputField(
      "Background opacity (0-100%)",
      String(settings.backgroundOpacityPercent),
      "number",
    );
    backgroundOpacity.input.min = "0";
    backgroundOpacity.input.max = "100";
    backgroundColor.input.disabled = settings.backgroundMode !== "solid";
    backgroundMode.select.addEventListener("change", () => {
      settings = {
        ...settings!,
        backgroundMode: backgroundMode.select.value as OverlayBackgroundMode,
      };
      render();
    });
    backgroundColor.input.addEventListener("change", () => {
      settings = { ...settings!, backgroundColor: backgroundColor.input.value };
      render();
    });
    backgroundOpacity.input.addEventListener("change", () => {
      settings = {
        ...settings!,
        backgroundOpacityPercent: clamp(Number(backgroundOpacity.input.value), 0, 100),
      };
      render();
    });
    const uploadLabel = el("label", "combat-overlay-field");
    uploadLabel.append(text("span", "Custom PNG, JPEG, WebP, or animated GIF"));
    const upload = document.createElement("input");
    upload.type = "file";
    upload.accept = "image/png,image/jpeg,image/webp,image/gif";
    upload.addEventListener("change", () => {
      const file = upload.files?.[0];
      if (!file) return;
      upload.disabled = true;
      void uploadBackground(file)
        .then((revision) => {
          settings = {
            ...settings!,
            backgroundMode: "custom",
            customBackgroundRevision: revision,
          };
          status.textContent = "Custom background loaded into the preview. Save the layout to keep it active.";
          status.classList.remove("error");
          render();
        })
        .catch((error) => {
          status.textContent = errorMessage(error);
          status.classList.add("error");
          upload.disabled = false;
        });
    });
    uploadLabel.append(upload);
    backgroundGroup.append(
      backgroundMode.label,
      backgroundColor.label,
      backgroundOpacity.label,
      uploadLabel,
    );
    inspector.append(appearanceGroup, backgroundGroup);
  };

  const updateButton = (
    layerId: string,
    buttonId: string,
    next: OverlayButton,
  ) => updateLayer(layerId, (layer) => ({
    ...layer,
    buttons: layer.buttons.map((value) => (value.id === buttonId ? next : value)),
  }));

  const setLayerMetric = (layerId: string, metric: OverlayMetric) =>
    updateLayer(layerId, (layer) => ({
      ...layer,
      metric,
      buttons: layer.buttons.map((value) => value.action === "cycle_metric"
        ? { ...value, label: metricLabel(metric) }
        : value),
    }));

  const cycleLayerMetric = (layerId: string) => {
    if (settings === null) return;
    const nextLayerId = nextOverlayHeaderViewId(settings.layers, layerId);
    if (nextLayerId === layerId) return;
    copyLayerRuntimeSelections(
      layerId,
      nextLayerId,
      selectedTimerByLayer,
      selectedSegmentByLayer,
    );
    selectedLayerId = nextLayerId;
    const nextLayer = settings.layers.find((candidate) => candidate.id === nextLayerId)!;
    status.textContent = `Preview action: switched to the ${nextLayer.title} header view.`;
    status.classList.remove("error");
    render();
  };

  const showContextMenu = (event: MouseEvent, target: ContextTarget) => {
    event.preventDefault();
    closeContextMenu();
    const viewEntries = Object.values(VIEW_PRESETS).map((preset) => ({
      label: preset.title,
      action: () => addLayer(preset),
    } satisfies ContextMenuEntry));
    viewEntries.push({ label: "Custom view", action: () => addLayer() });
    const addColumnEntries = (layer: OverlayLayer): ContextMenuEntry[] => HEADER_FIELD_GROUPS
      .map(([label, fields]) => ({
        label,
        children: fields
          .filter((field) => !layer.headerFields.includes(field))
          .map((field) => ({ label: fieldLabel(field), action: () => addHeader(layer.id, field) })),
      }))
      .filter((entry) => entry.children.length > 0);
    const controlEntries = (layerId: string): ContextMenuEntry[] => [
      { label: "Cycle metric", action: () => addButton(layerId, "cycle_metric", "Metric") },
      { label: "Cycle timer", action: () => addButton(layerId, "cycle_timer", "Encounter") },
      { label: "Cycle segment", action: () => addButton(layerId, "cycle_segment", "Entire run") },
      { label: "Reset encounter", action: () => addButton(layerId, "reset_encounter", "Reset") },
      { label: "Hide overlay", action: () => addButton(layerId, "toggle_visibility", "Hide") },
      { label: "Open Combat History", action: () => addButton(layerId, "open_history", "History") },
    ];
    const summaryEntries = (layer: OverlayLayer): ContextMenuEntry[] => SUMMARY_FIELDS
      .filter((field) => !layer.summaryFields.includes(field))
      .map((field) => ({
        label: summaryFieldLabel(field),
        action: () => addSummaryField(layer.id, field),
      }));
    let entries: ContextMenuEntry[] = [];
    if (target.kind === "canvas") {
      entries = [{ label: "Add view", children: viewEntries }];
    } else if (target.kind === "layer" || target.kind === "view") {
      const layer = settings?.layers.find((candidate) => candidate.id === target.layerId);
      if (layer !== undefined) {
        entries.push(
          { label: "Add summary item", children: summaryEntries(layer), disabled: summaryEntries(layer).length === 0 },
          { label: "Add column", children: addColumnEntries(layer), disabled: addColumnEntries(layer).length === 0 },
          { label: "Add control", children: controlEntries(target.layerId) },
          { label: "Add view", children: viewEntries, separatorBefore: true },
          { label: "View options", children: [
            { label: "Duplicate view", action: () => duplicateLayer(target.layerId) },
            ...((settings?.layers.length ?? 0) > 1
              ? [{ label: "Delete view", action: () => deleteLayer(target.layerId), danger: true } satisfies ContextMenuEntry]
              : []),
          ] },
        );
      }
    } else if (target.kind === "summary" || target.kind === "summary_item") {
      const layer = settings?.layers.find((candidate) => candidate.id === target.layerId);
      if (layer !== undefined) {
        entries = [
          { label: "Add summary item", children: summaryEntries(layer), disabled: summaryEntries(layer).length === 0 },
          { label: "Add control", children: controlEntries(target.layerId) },
          ...(target.kind === "summary_item"
            ? [{
                label: "Summary item options",
                children: [
                  {
                    label: layer.hiddenSummaryLabels.includes(target.field) ? "Show title" : "Hide title",
                    action: () => toggleSummaryLabel(target.layerId, target.field),
                  },
                  {
                    label: `Remove ${summaryFieldLabel(target.field)}`,
                    action: () => removeSummaryField(target.layerId, target.field),
                    danger: true,
                  },
                ],
              } satisfies ContextMenuEntry]
            : []),
        ];
      }
    } else if (target.kind === "header") {
      const layer = settings?.layers.find((candidate) => candidate.id === target.layerId);
      if (layer !== undefined) {
        entries = [
          { label: "Add column", children: addColumnEntries(layer), disabled: addColumnEntries(layer).length === 0 },
          { label: "Add control", children: controlEntries(target.layerId) },
          { label: "Header options", children: [
            {
              label: layer.hiddenHeaderLabels.includes(target.field) ? "Show header name" : "Hide header name",
              action: () => toggleHeaderLabel(target.layerId, target.field),
            },
            ...(layer.headerFields.length > 1
              ? [{ label: `Remove ${fieldLabel(target.field)}`, action: () => removeHeader(target.layerId, target.field), danger: true } satisfies ContextMenuEntry]
              : []),
          ] },
          { label: "Add view", children: viewEntries, separatorBefore: true },
        ];
      }
    } else {
      entries = [{ label: "Control options", children: [
        { label: "Delete control", action: () => deleteButton(target.layerId, target.buttonId), danger: true },
      ] }];
    }
    const menu = buildContextMenu(entries, true);
    menu.style.left = `${event.clientX}px`;
    menu.style.top = `${event.clientY}px`;
    document.body.append(menu);
    keepMenuInViewport(menu);
    window.setTimeout(() => document.addEventListener("pointerdown", (pointerEvent) => {
      // Keep the menu mounted through the selected item's click event. Removing
      // the clicked button on pointer-down prevents browsers from dispatching
      // its later click, which made every context action appear inert.
      if (!menu.contains(pointerEvent.target as Node)) closeContextMenu();
    }, { once: true }), 0);
  };

  const addLayer = (preset?: OverlayViewPreset) => {
    if (settings === null) return;
    const source = settings.layers.find((layer) => layer.id === selectedLayerId)
      ?? settings.layers[0];
    const id = uniqueId("view", settings.layers.map((layer) => layer.id));
    const layer: OverlayLayer = {
      id,
      title: preset?.title ?? `View ${settings.layers.length + 1}`,
      metric: preset?.metric ?? source?.metric ?? "dps",
      x: source?.x ?? 18,
      y: source?.y ?? 18,
      width: source?.width ?? 680,
      headerFields: [...(preset?.fields ?? source?.headerFields ?? ["rank", "class_spec", "name", "dps", "percent"])],
      headerWidths: { ...(source?.headerWidths ?? DEFAULT_HEADER_WIDTHS) },
      hiddenHeaderLabels: [...(source?.hiddenHeaderLabels ?? [])],
      summaryFields: [...(source?.summaryFields ?? DEFAULT_SUMMARY_FIELDS)],
      summaryFieldWidths: { ...(source?.summaryFieldWidths ?? {}) },
      summaryFieldRows: { ...(source?.summaryFieldRows ?? defaultSummaryFieldRows(DEFAULT_SUMMARY_FIELDS)) },
      summaryItemOrder: [...(source?.summaryItemOrder ?? [])],
      summaryItemRows: { ...(source?.summaryItemRows ?? {}) },
      hiddenSummaryLabels: [...(source?.hiddenSummaryLabels ?? [])],
      showBossDps: source?.showBossDps ?? true,
      buttons: (source?.buttons ?? []).map((value) => ({ ...value })),
    };
    settings = { ...settings, layers: [...settings.layers, withNormalizedSummaryLayout(layer)] };
    selectedLayerId = id;
    render();
  };

  const duplicateLayer = (layerId: string) => {
    if (settings === null) return;
    const source = settings.layers.find((layer) => layer.id === layerId);
    if (!source) return;
    const id = uniqueId(source.id, settings.layers.map((layer) => layer.id));
    const copy = {
      ...source,
      id,
      title: `${source.title} copy`,
      headerFields: [...source.headerFields],
      headerWidths: { ...source.headerWidths },
      hiddenHeaderLabels: [...source.hiddenHeaderLabels],
      summaryFields: [...source.summaryFields],
      summaryFieldWidths: { ...source.summaryFieldWidths },
      summaryFieldRows: { ...source.summaryFieldRows },
      summaryItemOrder: [...source.summaryItemOrder],
      summaryItemRows: { ...source.summaryItemRows },
      hiddenSummaryLabels: [...source.hiddenSummaryLabels],
      showBossDps: source.showBossDps,
      buttons: source.buttons.map((value) => ({ ...value })),
    };
    settings = { ...settings, layers: [...settings.layers, copy] };
    selectedLayerId = id;
    render();
  };

  const deleteLayer = (layerId: string) => {
    if (settings === null) return;
    if (settings.layers.length <= 1) {
      status.textContent = "The overlay must keep at least one header view.";
      status.classList.add("error");
      return;
    }
    const deletedIndex = settings.layers.findIndex((layer) => layer.id === layerId);
    settings = { ...settings, layers: settings.layers.filter((layer) => layer.id !== layerId) };
    selectedActorByLayer.delete(layerId);
    selectedLayerId = settings.layers[Math.min(Math.max(0, deletedIndex), settings.layers.length - 1)]?.id
      ?? settings.layers[0]?.id
      ?? null;
    status.textContent = "Header view removed from the preview. Save the layout to keep this change.";
    status.classList.remove("error");
    render();
  };

  const addButton = (
    layerId: string,
    action: OverlayButtonAction = "open_history",
    label = "History",
  ) => {
    status.textContent = `${label} control added. Click it in the preview to test its configured action, then edit its label or function in the inspector.`;
    status.classList.remove("error");
    updateLayer(layerId, (layer) => {
      const next = {
        ...layer,
        buttons: [...layer.buttons, {
          id: uniqueId("button", layer.buttons.map((value) => value.id)),
          label,
          action,
          width: defaultButtonWidth(action),
        }],
      };
      return withNormalizedSummaryLayout(next);
    });
  };

  const addHeader = (layerId: string, field: OverlayHeaderField) =>
    updateLayer(layerId, (layer) => withNormalizedSummaryLayout({
      ...layer,
      headerFields: insertHeaderField(layer.headerFields, field),
      hiddenHeaderLabels: layer.hiddenHeaderLabels.filter((candidate) => candidate !== field),
    }));

  const addSummaryField = (layerId: string, field: OverlaySummaryField) =>
    updateLayer(layerId, (layer) => withNormalizedSummaryLayout({
      ...layer,
      summaryFields: layer.summaryFields.includes(field)
        ? layer.summaryFields
        : [...layer.summaryFields, field],
      summaryFieldRows: layer.summaryFields.includes(field)
        ? layer.summaryFieldRows
        : {
            ...layer.summaryFieldRows,
            [field]: defaultSummaryRow(field),
          },
    }));

  const removeSummaryField = (layerId: string, field: OverlaySummaryField) =>
    updateLayer(layerId, (layer) => withNormalizedSummaryLayout({
      ...layer,
      summaryFields: layer.summaryFields.filter((candidate) => candidate !== field),
      summaryFieldRows: Object.fromEntries(
        Object.entries(layer.summaryFieldRows).filter(([candidate]) => candidate !== field),
      ),
      hiddenSummaryLabels: layer.hiddenSummaryLabels.filter((candidate) => candidate !== field),
    }));

  const toggleSummaryLabel = (layerId: string, field: OverlaySummaryField) => {
    const hidden = settings?.layers
      .find((layer) => layer.id === layerId)
      ?.hiddenSummaryLabels.includes(field) ?? false;
    status.textContent = `${summaryFieldLabel(field)} title ${hidden ? "shown" : "hidden"}. Its value remains visible.`;
    status.classList.remove("error");
    updateLayer(layerId, (layer) => ({
      ...layer,
      hiddenSummaryLabels: layer.hiddenSummaryLabels.includes(field)
        ? layer.hiddenSummaryLabels.filter((candidate) => candidate !== field)
        : [...layer.hiddenSummaryLabels, field],
    }));
  };

  const removeHeader = (layerId: string, field: OverlayHeaderField) =>
    updateLayer(layerId, (layer) => layer.headerFields.length <= 1 ? layer : ({
      ...layer,
      headerFields: layer.headerFields.filter((candidate) => candidate !== field),
      hiddenHeaderLabels: layer.hiddenHeaderLabels.filter((candidate) => candidate !== field),
    }));

  const toggleHeaderLabel = (layerId: string, field: OverlayHeaderField) => {
    const hidden = settings?.layers
      .find((layer) => layer.id === layerId)
      ?.hiddenHeaderLabels.includes(field) ?? false;
    status.textContent = `${fieldLabel(field)} header name ${hidden ? "shown" : "hidden"}. The column and its row values remain visible.`;
    status.classList.remove("error");
    updateLayer(layerId, (layer) => ({
      ...layer,
      hiddenHeaderLabels: layer.hiddenHeaderLabels.includes(field)
        ? layer.hiddenHeaderLabels.filter((candidate) => candidate !== field)
        : [...layer.hiddenHeaderLabels, field],
    }));
  };

  const deleteButton = (layerId: string, buttonId: string) =>
    updateLayer(layerId, (layer) => withNormalizedSummaryLayout({
      ...layer,
      buttons: layer.buttons.filter((value) => value.id !== buttonId),
    }));

  addLayerButton.addEventListener("click", () => {
    addLayer();
  });
  previewScaleInput.addEventListener("input", () => {
    if (settings === null) return;
    settings = {
      ...settings,
      scalePercent: clamp(Number(previewScaleInput.value), 50, 200),
    };
    render();
  });
  const updatePreviewDimensions = () => {
    if (settings === null) return;
    const nextHeight = settings.dynamicHeight
      ? settings.canvasHeight
      : clamp(Number(previewHeight.input.value), MIN_OVERLAY_HEIGHT, 1440);
    settings = {
      ...settings,
      canvasWidth: clamp(Number(previewWidth.input.value), 320, 2560),
      canvasHeight: nextHeight,
    };
    render();
  };
  previewWidth.input.addEventListener("change", updatePreviewDimensions);
  previewHeight.input.addEventListener("change", updatePreviewDimensions);
  previewData.select.addEventListener("change", () => {
    previewDataMode = previewData.select.value === "example" ? "example" : "live";
    refreshPreviewData.hidden = previewDataMode !== "live";
    if (previewDataMode === "live" && previewLiveUpdate === null) {
      void loadPreviewData();
      return;
    }
    status.textContent = previewDataMode === "live"
      ? "Showing the current data used by the native live overlay."
      : "Showing stable example combat so populated rows can be arranged.";
    status.classList.remove("error");
    render();
  });
  refreshPreviewData.addEventListener("click", () => void loadPreviewData());
  previewResizeHandle.addEventListener("pointerdown", (event) => {
    if (settings === null || event.button !== 0) return;
    event.preventDefault();
    const startX = event.clientX;
    const startY = event.clientY;
    const startWidth = settings.canvasWidth;
    const startHeight = settings.canvasHeight;
    const scale = overlayScale(settings);
    previewResizeHandle.classList.add("is-resizing");
    const move = (moveEvent: PointerEvent) => {
      if (settings === null) return;
      settings = {
        ...settings,
        canvasWidth: clamp(Math.round(startWidth + (moveEvent.clientX - startX) / scale), 320, 2560),
        canvasHeight: settings.dynamicHeight
          ? startHeight
          : clamp(
              Math.round(startHeight + (moveEvent.clientY - startY) / scale),
              MIN_OVERLAY_HEIGHT,
              1440,
            ),
      };
      render();
    };
    const finish = () => {
      previewResizeHandle.classList.remove("is-resizing");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish, { once: true });
    window.addEventListener("pointercancel", finish, { once: true });
  });

  save.addEventListener("click", async () => {
    if (settings === null) return;
    save.disabled = true;
    try {
      settings = await saveSettings(settings);
      status.textContent = "Combat Overlay layout saved by its plug-in.";
      status.classList.remove("error");
      render();
    } catch (error) {
      status.textContent = errorMessage(error);
      status.classList.add("error");
    } finally {
      save.disabled = false;
    }
  });

  reset.addEventListener("click", async () => {
    try {
      settings = await loadSettings();
      selectedLayerId = settings.layers[0]?.id ?? null;
      status.textContent = "Unsaved preview edits reset.";
      render();
    } catch (error) {
      status.textContent = errorMessage(error);
      status.classList.add("error");
    }
  });

  openOverlay.addEventListener("click", async () => {
    try {
      if (!openLiveOverlay) {
        throw new Error("Open the native rLogs application to launch a live overlay window.");
      }
      if (settings === null) settings = await loadSettings();
      if (!settings.liveOverlayEnabled) {
        settings = await saveSettings({ ...settings, liveOverlayEnabled: true });
        render();
      }
      await openLiveOverlay();
      status.textContent = "Live overlay enabled. rLogs will restore it automatically on future launches.";
      status.classList.remove("error");
    } catch (error) {
      status.textContent = `The live overlay is available in the native app: ${errorMessage(error)}`;
      status.classList.add("error");
    }
  });

  void loadSettings()
    .then((loaded) => {
      if (!alive) return;
      settings = loaded;
      selectedLayerId = loaded.layers[0]?.id ?? null;
      status.textContent = "Showing stable example combat so the layout is always visible. Choose Live overlay to inspect current combat data.";
      render();
      void loadPreviewData();
    })
    .catch((error) => {
      status.textContent = errorMessage(error);
      status.classList.add("error");
    });

  return {
    dispose() {
      alive = false;
      closeContextMenu();
      root.remove();
    },
  };
}

export function mountCombatOverlayOptionsSurface(container: HTMLElement): MountedSurface {
  installStyles();
  let alive = true;
  let settings: CombatOverlaySettings | null = null;
  let autoSaveTimer: number | null = null;
  let autoSaveInFlight = false;
  let autoSavePending = false;
  let observedColorIdentities: readonly BarColorIdentity[] = [];
  let draftBarColorOverrides: Record<string, string> = {};
  const root = el("div", "combat-overlay-options");
  const heading = el("section", "content-card combat-overlay-options-heading");
  heading.append(
    text("h2", "Combat Overlay options"),
    text(
      "p",
      "Behavior that cannot be edited directly on the overlay surface. Size, opacity, backgrounds, columns, and views remain in the Overlay designer.",
    ),
  );
  const form = el("form", "content-card combat-overlay-options-form");

  const overlayHotkey = mountHotkeyBinding(COMBAT_OVERLAY_TOGGLE_ACTION_ID, { compact: true });
  const hotkeys = el("fieldset", "combat-overlay-options-group");
  hotkeys.append(
    text("legend", "Hotkey"),
    text(
      "p",
      "This is the same binding shown in Settings > Hotkeys. Changes in either place update the core registry.",
    ),
    overlayHotkey.element,
  );

  const dynamicHeight = checkbox("Grow and shrink with visible player rows", true);
  const maxVisiblePlayers = inputField("Maximum visible players", "20", "number");
  maxVisiblePlayers.input.min = "1";
  maxVisiblePlayers.input.max = "20";
  const rowBehavior = el("fieldset", "combat-overlay-options-group");
  rowBehavior.append(
    text("legend", "Visible rows"),
    text("p", "Control how the native window responds as combatants appear or disappear."),
    dynamicHeight.label,
    maxVisiblePlayers.label,
  );

  const alwaysOnTop = checkbox("Always on top", true);
  const clickThrough = checkbox("Click-through while live", false);
  const allowLiveResize = checkbox("Allow resizing the live overlay", true);
  const liveOverlayEnabled = checkbox("Enable Combat Overlay when rLogs starts", false);
  const windowBehavior = el("fieldset", "combat-overlay-options-group");
  windowBehavior.append(
    text("legend", "Window behavior"),
    text("p", "These settings affect interaction with the native overlay window."),
    liveOverlayEnabled.label,
    alwaysOnTop.label,
    clickThrough.label,
    allowLiveResize.label,
  );

  const numberFormatOptions: readonly (readonly [OverlayNumberFormat, string])[] = [
    ["compact", "Compact (18M)"],
    ["detailed", "Detailed (18.33M)"],
    ["full", "Full (18,334,123)"],
  ];
  const numberFormatFields: Record<OverlayNumberFormatTarget, ReturnType<typeof selectField<OverlayNumberFormat>>> = {
    playerMetrics: selectField("Player metrics", numberFormatOptions, "detailed"),
    percentages: selectField("Percentages", numberFormatOptions, "compact"),
    summaryTotals: selectField("Team totals", numberFormatOptions, "detailed"),
    bossHealth: selectField("Boss HP", numberFormatOptions, "detailed"),
    bossMetrics: selectField("Boss DPS and damage", numberFormatOptions, "detailed"),
    skillValues: selectField("Skill values", numberFormatOptions, "detailed"),
    counts: selectField("Hits, casts, deaths, and revives", numberFormatOptions, "full"),
  };
  const numberFormatGrid = el("div", "combat-overlay-number-format-grid");
  numberFormatGrid.append(...Object.values(numberFormatFields).map((field) => field.label));
  const numberDisplay = el("fieldset", "combat-overlay-options-group");
  numberDisplay.append(
    text("legend", "Number display"),
    text(
      "p",
      "Choose precision independently so compact rows do not force boss HP, totals, or counts to lose detail.",
    ),
    numberFormatGrid,
  );

  const barColors = el("fieldset", "combat-overlay-options-group combat-overlay-bar-colors");
  const barColorMode = selectField<OverlayBarColorMode>(
    "Color rows by",
    [
      ["random", "Stable random color per player"],
      ["class", "Class"],
      ["specialization", "Specialization"],
    ],
    "random",
  );
  const barColorChoices = el("div", "combat-overlay-bar-color-grid");
  const barColorHint = text(
    "p",
    "Random colors remain stable for each character. Class and specialization modes use identities supplied by the active game plug-in.",
  );
  barColors.append(
    text("legend", "Bar colors"),
    barColorHint,
    barColorMode.label,
    barColorChoices,
  );

  const combatDetection = el("fieldset", "combat-overlay-options-group");
  combatDetection.append(
    text("legend", "Combat detection"),
    text(
      "p",
      "Visibility follows the Combat Meter reducer. It does not guess from retained DPS totals.",
    ),
  );
  const autoHideOutsideCombat = checkbox("Hide overlay outside active combat", false);
  const autoHideDelay = inputField("Hide delay after combat (seconds)", "5", "number");
  autoHideDelay.input.min = "0";
  autoHideDelay.input.max = "300";
  const refreshInterval = inputField(
    "Overlay refresh interval (milliseconds)",
    "250",
    "number",
  );
  refreshInterval.input.min = "50";
  refreshInterval.input.max = "2000";
  refreshInterval.input.step = "50";
  combatDetection.append(
    autoHideOutsideCombat.label,
    autoHideDelay.label,
    refreshInterval.label,
    text(
      "p",
      "This changes only how often the overlay redraws. Capture, decoding, and saved history remain lossless.",
    ),
  );
  const message = text("span", "Loading settings...", "combat-overlay-status");
  const optionsGrid = el("div", "combat-overlay-options-grid");
  const interactionColumn = el("div", "combat-overlay-options-column");
  interactionColumn.append(hotkeys, barColors);
  const visibilityColumn = el("div", "combat-overlay-options-column");
  visibilityColumn.append(rowBehavior, combatDetection);
  const displayColumn = el("div", "combat-overlay-options-column");
  displayColumn.append(windowBehavior, numberDisplay);
  optionsGrid.append(interactionColumn, visibilityColumn, displayColumn);
  heading.append(message);
  form.append(optionsGrid);
  root.append(heading, form);
  container.replaceChildren(root);

  const renderBarColorChoices = () => {
    barColorChoices.replaceChildren();
    const mode = barColorMode.select.value as OverlayBarColorMode;
    if (mode === "random") {
      barColorChoices.append(text(
        "p",
        "Each character receives a deterministic palette color, so sorting and reconnecting do not reshuffle the bars.",
        "combat-overlay-color-empty",
      ));
      return;
    }
    const prefix = `${mode}:`;
    const identities = new Map<string, BarColorIdentity>();
    for (const identity of observedColorIdentities) {
      if (identity.kind === mode) identities.set(identity.key, identity);
    }
    for (const key of Object.keys(draftBarColorOverrides)) {
      if (key.startsWith(prefix) && !identities.has(key)) {
        const id = key.slice(prefix.length);
        identities.set(key, {
          key,
          kind: mode,
          label: mode === "class" ? `Class ${id}` : `Specialization ${id}`,
        });
      }
    }
    if (identities.size === 0) {
      barColorChoices.append(text(
        "p",
        `No ${mode} identities are available in the current live snapshot yet. Automatic colors will still work as soon as the game plug-in reports them.`,
        "combat-overlay-color-empty",
      ));
      return;
    }
    for (const identity of [...identities.values()].sort((left, right) => left.label.localeCompare(right.label))) {
      const row = el("div", "combat-overlay-bar-color-row");
      const copy = el("div", "combat-overlay-bar-color-copy");
      copy.append(text("strong", identity.label), text("small", identity.key));
      const color = document.createElement("input");
      color.type = "color";
      color.value = draftBarColorOverrides[identity.key] ?? automaticBarColor(identity.key);
      color.title = `Choose the bar color for ${identity.label}`;
      color.setAttribute("aria-label", `Bar color for ${identity.label}`);
      color.addEventListener("input", () => {
        draftBarColorOverrides = {
          ...draftBarColorOverrides,
          [identity.key]: color.value,
        };
      });
      const resetColor = button("Auto", "secondary-button combat-overlay-color-reset");
      resetColor.title = `Use the automatic palette color for ${identity.label}`;
      resetColor.addEventListener("click", () => {
        const next = { ...draftBarColorOverrides };
        delete next[identity.key];
        draftBarColorOverrides = next;
        renderBarColorChoices();
        queueAutoSave();
      });
      row.append(copy, color, resetColor);
      barColorChoices.append(row);
    }
  };

  const apply = (value: CombatOverlaySettings, saved = false) => {
    settings = value;
    draftBarColorOverrides = { ...value.barColorOverrides };
    barColorMode.select.value = value.barColorMode;
    dynamicHeight.input.checked = value.dynamicHeight;
    maxVisiblePlayers.input.value = String(value.maxVisiblePlayers);
    maxVisiblePlayers.input.disabled = !value.dynamicHeight;
    liveOverlayEnabled.input.checked = value.liveOverlayEnabled;
    alwaysOnTop.input.checked = value.alwaysOnTop;
    clickThrough.input.checked = value.clickThrough;
    allowLiveResize.input.checked = value.allowLiveResize;
    for (const [target, field] of Object.entries(numberFormatFields) as [OverlayNumberFormatTarget, typeof numberFormatFields[OverlayNumberFormatTarget]][]) {
      field.select.value = value.numberFormats[target];
    }
    autoHideOutsideCombat.input.checked = value.autoHideOutsideCombat;
    autoHideDelay.input.value = String(value.autoHideDelaySeconds);
    autoHideDelay.input.disabled = !value.autoHideOutsideCombat;
    refreshInterval.input.value = String(value.refreshIntervalMillis);
    renderBarColorChoices();
    message.textContent = saved
      ? "Changes saved automatically."
      : "Changes save automatically.";
    message.classList.remove("error");
  };
  const readDraft = (): CombatOverlaySettings | null => settings === null
    ? null
    : {
      ...settings,
      barColorMode: barColorMode.select.value as OverlayBarColorMode,
      barColorOverrides: { ...draftBarColorOverrides },
      dynamicHeight: dynamicHeight.input.checked,
      maxVisiblePlayers: clamp(Number(maxVisiblePlayers.input.value), 1, 20),
      liveOverlayEnabled: liveOverlayEnabled.input.checked,
      alwaysOnTop: alwaysOnTop.input.checked,
      clickThrough: clickThrough.input.checked,
      allowLiveResize: allowLiveResize.input.checked,
      numberFormats: Object.fromEntries(
        (Object.entries(numberFormatFields) as [OverlayNumberFormatTarget, typeof numberFormatFields[OverlayNumberFormatTarget]][])
          .map(([target, field]) => [target, field.select.value as OverlayNumberFormat]),
      ) as OverlayNumberFormats,
      autoHideOutsideCombat: autoHideOutsideCombat.input.checked,
      autoHideDelaySeconds: clamp(Number(autoHideDelay.input.value), 0, 300),
      refreshIntervalMillis: clamp(Number(refreshInterval.input.value), 50, 2_000),
    };
  const flushAutoSave = async () => {
    autoSaveTimer = null;
    if (!alive || settings === null) return;
    if (autoSaveInFlight) {
      autoSavePending = true;
      return;
    }
    const draft = readDraft();
    if (draft === null) return;
    autoSaveInFlight = true;
    autoSavePending = false;
    message.textContent = "Saving changes...";
    message.classList.remove("error");
    try {
      const saved = await saveSettings(draft);
      if (!alive) return;
      settings = saved;
      if (!autoSavePending) apply(saved, true);
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      autoSaveInFlight = false;
      if (alive && autoSavePending) {
        autoSavePending = false;
        autoSaveTimer = window.setTimeout(() => void flushAutoSave(), 0);
      }
    }
  };
  function queueAutoSave() {
    if (!alive || settings === null) return;
    autoSavePending = true;
    message.textContent = "Saving changes...";
    message.classList.remove("error");
    if (autoSaveTimer !== null) window.clearTimeout(autoSaveTimer);
    if (autoSaveInFlight) {
      autoSaveTimer = null;
      return;
    }
    autoSaveTimer = window.setTimeout(() => void flushAutoSave(), 250);
  }
  dynamicHeight.input.addEventListener("change", () => {
    maxVisiblePlayers.input.disabled = !dynamicHeight.input.checked;
  });
  autoHideOutsideCombat.input.addEventListener("change", () => {
    autoHideDelay.input.disabled = !autoHideOutsideCombat.input.checked;
  });
  barColorMode.select.addEventListener("change", renderBarColorChoices);
  form.addEventListener("input", (event) => {
    // Native color pickers emit input continuously while the palette is open.
    // Save their final change instead of replacing the picker mid-selection.
    if (event.target instanceof HTMLInputElement && event.target.type === "color") return;
    queueAutoSave();
  });
  form.addEventListener("change", queueAutoSave);
  form.addEventListener("submit", (event) => event.preventDefault());
  void loadSettings().then((value) => alive && apply(value)).catch((error) => {
    message.textContent = errorMessage(error);
    message.classList.add("error");
  });
  void loadBarColorIdentities().then((identities) => {
    if (!alive) return;
    observedColorIdentities = identities;
    renderBarColorChoices();
  }).catch(() => {
    // The game plug-in may not have produced a live snapshot yet. Automatic
    // colors still work from numeric identities once actors arrive.
  });
  return {
    dispose() {
      alive = false;
      if (autoSaveTimer !== null) window.clearTimeout(autoSaveTimer);
      overlayHotkey.dispose();
      root.remove();
    },
  };
}

export async function mountCombatOverlayRuntimeApp(
  container: HTMLElement,
  appWindow: CombatOverlayRuntimeWindow,
): Promise<void> {
  installStyles();
  const root = el("main", "combat-overlay-runtime");
  let canvas = el("div", "combat-overlay-canvas combat-overlay-canvas-runtime");
  canvas.append(text("p", "Loading live overlay…", "combat-overlay-runtime-loading"));
  const resizeEast = runtimeResizeHandle("East", appWindow);
  const resizeSouth = runtimeResizeHandle("South", appWindow);
  const resizeSouthEast = runtimeResizeHandle("SouthEast", appWindow);
  root.append(canvas, resizeEast, resizeSouth, resizeSouthEast);
  container.replaceChildren(root);
  document.documentElement.classList.add("combat-overlay-runtime-document");

  const [settings, initialTimerSettings] = await Promise.all([
    loadSettings(),
    loadGlobalTimerSettings(),
  ]);
  let runtimeSettings = settings;
  let timerSettings = initialTimerSettings;
  let activeLayerId = settings.layers[0]?.id ?? null;
  let actors: readonly OverlayActor[] = [];
  const selectedActorByLayer = new Map<string, string>();
  const selectedTimerByLayer = new Map<string, OverlaySummaryField>();
  const selectedSegmentByLayer = new Map<string, string>();
  let revision = 0;
  let active = true;
  let latestSnapshot: OverlaySnapshot | null = null;
  let encounterPresentation: OverlayEncounterPresentation | null = null;
  let automaticallyHidden = !settings.liveOverlayEnabled || settings.autoHideOutsideCombat;
  let visibilityTimer: number | null = null;
  let visibilityTimerKey: string | null = null;
  let resizeSaveTimer: number | null = null;
  let resizeSettingsPending = false;
  let forceResetPending = false;
  let stopResizeListener: (() => void) | null = null;
  let stopShowRequestListener: (() => void) | null = null;
  let frameTimer: number | null = null;
  let lastRenderMillis = 0;
  let settingsFingerprint = JSON.stringify(settings);
  let timerSettingsFingerprint = JSON.stringify(initialTimerSettings);
  let lastSettingsRefreshMillis = 0;
  let lastWindowWidth = Math.round(settings.canvasWidth * overlayScale(settings));
  let lastWindowHeight = Math.round(settings.canvasHeight * overlayScale(settings));
  let pendingProgrammaticSize: { width: number; height: number } | null = {
    width: lastWindowWidth,
    height: lastWindowHeight,
  };
  const reportWindowSyncFailure = (operation: string, error: unknown) => {
    root.dataset.windowSyncError = operation;
    root.title = errorMessage(error);
  };
  void appWindow.setEnabled(settings.liveOverlayEnabled, automaticallyHidden)
    .catch((error) => reportWindowSyncFailure("startup visibility", error));
  void appWindow.setAlwaysOnTop(settings.alwaysOnTop)
    .catch((error) => reportWindowSyncFailure("topmost", error));
  void appWindow.setSize(lastWindowWidth, lastWindowHeight)
    .catch((error) => {
      pendingProgrammaticSize = null;
      reportWindowSyncFailure("resize", error);
    });

  const saveResizedWindow = () => {
    if (resizeSaveTimer !== null) window.clearTimeout(resizeSaveTimer);
    resizeSettingsPending = true;
    resizeSaveTimer = window.setTimeout(() => {
      resizeSaveTimer = null;
      const resizedSettings = runtimeSettings;
      void saveSettings(resizedSettings).then((saved) => {
        runtimeSettings = saved;
        settingsFingerprint = JSON.stringify(saved);
        resizeSettingsPending = false;
        render();
      }).catch((error) => {
        resizeSettingsPending = false;
        reportWindowSyncFailure("saving size", error);
      });
    }, 450);
  };

  const clearVisibilityTimer = () => {
    if (visibilityTimer !== null) {
      window.clearTimeout(visibilityTimer);
      visibilityTimer = null;
    }
    visibilityTimerKey = null;
  };
  const setAutomaticallyHidden = (hidden: boolean) => {
    automaticallyHidden = hidden;
    // Keep the last rendered overlay frame ready while the native window is
    // physically hidden. Native damage activity reveals that already-painted
    // frame immediately; CSS-hiding it would add a second WebView wake/render
    // step and briefly expose an empty transparent window.
    root.classList.remove("is-auto-hidden");
    // Publish the host-owned state first. If the physical hide wins a race
    // with focus restoration while the old state is still visible, Windows
    // can immediately reveal the overlay and make Hide appear to need a
    // second click.
    void appWindow.setAutomaticallyHidden(hidden)
      .then(() => hidden ? appWindow.hideTemporarily() : undefined)
      .catch((error) => reportWindowSyncFailure("native automatic visibility", error));
    // Auto-hide owns visibility only. Cursor passthrough is an explicit user
    // option and must never be enabled merely because a hide was requested.
    void appWindow.setIgnoreCursorEvents(runtimeSettings.clickThrough)
      .catch((error) => reportWindowSyncFailure("automatic visibility", error));
  };
  const hideAutomatically = () => {
    visibilityTimer = null;
    visibilityTimerKey = null;
    if (automaticallyHidden || !runtimeSettings.autoHideOutsideCombat) return;
    // Hide the native window itself. CSS-only hiding leaves WebView2's
    // transparent compositor surface visible as a faint rectangle on Windows.
    // A native feed observer wakes the window when reducer combat resumes, so
    // this remains reliable even if the hidden WebView's timers are suspended.
    setAutomaticallyHidden(true);
  };
  const syncCombatVisibility = () => {
    if (!runtimeSettings.liveOverlayEnabled) {
      clearVisibilityTimer();
      if (!automaticallyHidden) setAutomaticallyHidden(true);
      return;
    }
    const plan = planCombatOverlayVisibility(
      runtimeSettings,
      latestSnapshot,
    );
    if (!runtimeSettings.autoHideOutsideCombat || plan.hideAfterMillis === null) {
      clearVisibilityTimer();
    }
    if (plan.showNow && automaticallyHidden) {
      setAutomaticallyHidden(false);
    }
    if (plan.hideAfterMillis !== null) {
      const timerKey = latestSnapshot?.combat_active === true
        ? `combat:${latestSnapshot.last_hostile_micros ?? "unknown"}:${latestSnapshot.combat_inactivity_timeout_micros ?? "unknown"}:${runtimeSettings.autoHideDelaySeconds}`
        : `idle:${runtimeSettings.autoHideDelaySeconds}`;
      if (
        automaticallyHidden ||
        (visibilityTimer !== null && shouldKeepCombatVisibilityTimer(
          visibilityTimerKey,
          timerKey,
          plan.showNow,
        ))
      ) return;
      clearVisibilityTimer();
      visibilityTimerKey = timerKey;
      if (plan.hideAfterMillis === 0) hideAutomatically();
      else visibilityTimer = window.setTimeout(hideAutomatically, plan.hideAfterMillis);
    }
  };
  syncCombatVisibility();
  void appWindow.onShowRequested(() => {
    // The native editor button and global hotkey can reveal the preloaded
    // window while this runtime remains alive. Reconcile that real window
    // transition with combat auto-hide instead of trusting the stale local
    // `automaticallyHidden` flag.
    clearVisibilityTimer();
    setAutomaticallyHidden(false);
    syncCombatVisibility();
  }).then((unlisten) => {
    stopShowRequestListener = unlisten;
  }).catch((error) => reportWindowSyncFailure("show tracking", error));

  const render = () => {
    if (frameTimer !== null) {
      window.clearTimeout(frameTimer);
      frameTimer = null;
    }
    lastRenderMillis = performance.now();
    const scale = overlayScale(runtimeSettings);
    root.dataset.dynamicHeight = String(runtimeSettings.dynamicHeight);
    root.dataset.liveResize = String(runtimeSettings.allowLiveResize);
    resizeEast.hidden = !runtimeSettings.allowLiveResize;
    resizeSouth.hidden = !runtimeSettings.allowLiveResize || runtimeSettings.dynamicHeight;
    resizeSouthEast.hidden = !runtimeSettings.allowLiveResize || runtimeSettings.dynamicHeight;
    // Build the next frame off-DOM. A malformed/oversized update must never
    // clear the last good overlay and leave a permanently transparent WebView.
    const nextCanvas = el("div", "combat-overlay-canvas combat-overlay-canvas-runtime");
    nextCanvas.style.width = `${Math.round(runtimeSettings.canvasWidth * scale)}px`;
    nextCanvas.style.height = `${Math.round(runtimeSettings.canvasHeight * scale)}px`;
    nextCanvas.style.setProperty("--overlay-opacity", String(runtimeSettings.opacityPercent / 100));
    nextCanvas.style.setProperty("--bar-opacity", String(runtimeSettings.barOpacityPercent / 100));
    nextCanvas.style.setProperty("--summary-opacity", String(runtimeSettings.summaryOpacityPercent / 100));
    renderOverlayCanvas(nextCanvas, runtimeSettings, actors, {
      mode: "runtime",
      snapshot: applyOverlayTimerPause(latestSnapshot, timerSettings),
      encounterPresentation,
      selectedLayerId: activeLayerId,
      onSelectLayer(layerId) {
        activeLayerId = layerId;
        render();
      },
      selectedActorByLayer,
      selectedTimerByLayer,
      selectedSegmentByLayer,
      onSelectActor(layerId, actorId) {
        selectedActorByLayer.set(layerId, actorId);
        render();
      },
      onCloseActor(layerId) {
        selectedActorByLayer.delete(layerId);
        render();
      },
      onStartWindowDrag() {
        void appWindow.startDragging().catch((error) =>
          reportWindowSyncFailure("window drag", error));
      },
      onRuntimeAction(layerId, action) {
        if (action === "cycle_metric") {
          const nextLayerId = nextOverlayHeaderViewId(runtimeSettings.layers, layerId);
          copyLayerRuntimeSelections(
            layerId,
            nextLayerId,
            selectedTimerByLayer,
            selectedSegmentByLayer,
          );
          activeLayerId = nextLayerId;
          selectedActorByLayer.delete(layerId);
          render();
        } else if (action === "cycle_timer") {
          cycleSelectedTimer(
            selectedTimerByLayer,
            layerId,
            encounterPresentation,
            latestSnapshot,
          );
          render();
        } else if (action === "cycle_segment") {
          cycleSelectedSegment(
            selectedSegmentByLayer,
            layerId,
            encounterPresentation,
          );
          selectedActorByLayer.delete(layerId);
          render();
        } else if (action === "toggle_visibility") {
          clearVisibilityTimer();
          // A manual Hide is authoritative. Do not clear automatic-hidden
          // state immediately before hiding: that races the native host's
          // show path and can make this button appear to do nothing.
          void appWindow.hide().catch((error) =>
            reportWindowSyncFailure("manual hide", error));
        } else if (action === "reset_encounter") {
          if (forceResetPending) return;
          const confirmed = window.confirm(
            "Force reset the live meter? If a run is active, rLogs will mark it invalid and it cannot be submitted.",
          );
          if (!confirmed) return;
          forceResetPending = true;
          void forceResetLiveCombat().then(() => {
            latestSnapshot = null;
            encounterPresentation = null;
            actors = [];
            selectedActorByLayer.clear();
            render();
          }).catch((error) => reportWindowSyncFailure("force reset", error))
            .finally(() => {
              forceResetPending = false;
            });
        }
      },
    });
    // Dynamic sizing depends on real layout metrics (`scrollHeight`). Detached
    // elements report zero here in WebView2, which collapses the native window
    // to the minimum height and clips every actor row below the header. Keep the
    // last good frame visible while the completed replacement participates in
    // layout invisibly, then swap only after it has been measured.
    nextCanvas.style.visibility = "hidden";
    canvas.after(nextCanvas);
    const desiredHeight = resolvedOverlayHeight(nextCanvas, runtimeSettings);
    const desiredWidth = Math.round(runtimeSettings.canvasWidth * scale);
    nextCanvas.style.height = `${desiredHeight}px`;
    nextCanvas.style.removeProperty("visibility");
    // Replace the measured frame atomically. Removing the visible canvas
    // before adopting its successor can expose one transparent compositor
    // frame on WebView2, especially while the game is presenting at high FPS.
    canvas.replaceWith(nextCanvas);
    canvas = nextCanvas;
    if (desiredWidth !== lastWindowWidth || desiredHeight !== lastWindowHeight) {
      lastWindowWidth = desiredWidth;
      lastWindowHeight = desiredHeight;
      pendingProgrammaticSize = { width: desiredWidth, height: desiredHeight };
      void appWindow.setSize(desiredWidth, desiredHeight).catch((error) => {
        pendingProgrammaticSize = null;
        reportWindowSyncFailure("resize", error);
      });
    }
  };
  const requestFeedRender = () => {
    const delay = runtimeOverlayRenderDelay(
      lastRenderMillis,
      performance.now(),
      runtimeSettings.refreshIntervalMillis,
    );
    if (delay === 0) {
      render();
    } else if (frameTimer === null) {
      frameTimer = window.setTimeout(render, delay);
    }
  };
  render();

  void appWindow.onResized((width, height) => {
    const roundedWidth = Math.round(width);
    const roundedHeight = Math.round(height);
    if (
      pendingProgrammaticSize !== null &&
      Math.abs(roundedWidth - pendingProgrammaticSize.width) <= 3 &&
      Math.abs(roundedHeight - pendingProgrammaticSize.height) <= 3
    ) {
      lastWindowWidth = roundedWidth;
      lastWindowHeight = roundedHeight;
      pendingProgrammaticSize = null;
      return;
    }
    pendingProgrammaticSize = null;
    const widthChanged = Math.abs(roundedWidth - lastWindowWidth) > 1;
    const heightChanged = Math.abs(roundedHeight - lastWindowHeight) > 1;
    if (!widthChanged && !heightChanged) return;
    lastWindowWidth = roundedWidth;
    lastWindowHeight = roundedHeight;
    if (!runtimeSettings.allowLiveResize) {
      render();
      return;
    }
    const widthScale = roundedWidth / runtimeSettings.canvasWidth * 100;
    const heightScale = roundedHeight / runtimeSettings.canvasHeight * 100;
    const nextScale = widthChanged || runtimeSettings.dynamicHeight
      ? widthScale
      : heightScale;
    runtimeSettings = {
      ...runtimeSettings,
      scalePercent: clamp(Math.round(nextScale), 50, 200),
    };
    render();
    saveResizedWindow();
  }).then((unlisten) => {
    stopResizeListener = unlisten;
  }).catch((error) => reportWindowSyncFailure("resize tracking", error));

  let consecutiveRuntimeFailures = 0;
  let lastSuccessfulUpdateUnixMillis = Date.now();
  const heartbeatTimer = window.setInterval(() => {
    void appWindow.heartbeat(consecutiveRuntimeFailures, lastSuccessfulUpdateUnixMillis).catch((error) =>
      reportWindowSyncFailure("renderer heartbeat", error));
  }, 2_000);
  void appWindow.heartbeat(consecutiveRuntimeFailures, lastSuccessfulUpdateUnixMillis).catch((error) =>
    reportWindowSyncFailure("initial renderer heartbeat", error));
  const run = async () => {
    while (active) {
      try {
        const update = await apiJson<OverlayLiveUpdate>(
          "/api/runtime/live/combat/wait",
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ after_revision: revision, timeout_millis: 1_000 }),
          },
          5_000,
        );
        if (!active) return;
        lastSuccessfulUpdateUnixMillis = Date.now();
        const previousRevision = revision;
        revision = Math.max(revision, update.revision);
        let shouldRender = runtimeOverlayNeedsRender(
          previousRevision,
          update.revision,
          false,
          false,
        );
        latestSnapshot = update.snapshot;
        encounterPresentation = update.encounter_presentation ?? null;
        const availableViewIds = new Set(
          [
            "live",
            ...(encounterPresentation?.run_projection?.views.map((view) => view.id) ?? []),
          ],
        );
        for (const [layerId, viewId] of selectedSegmentByLayer) {
          if (!availableViewIds.has(viewId)) selectedSegmentByLayer.delete(layerId);
        }
        actors = overlayActorsFromLiveUpdate(update);
        const nowMillis = Date.now();
        const shouldRefreshSettings = !resizeSettingsPending
          && nowMillis - lastSettingsRefreshMillis >= 1_000;
        const [refreshedSettings, refreshedTimerSettings] = shouldRefreshSettings
          ? await Promise.all([loadSettings(), loadGlobalTimerSettings()])
          : [null, null];
        if (shouldRefreshSettings) lastSettingsRefreshMillis = nowMillis;
        const refreshedFingerprint = refreshedSettings === null
          ? settingsFingerprint
          : JSON.stringify(refreshedSettings);
        if (refreshedSettings !== null && refreshedFingerprint !== settingsFingerprint) {
          shouldRender = true;
          const alwaysOnTopChanged = refreshedSettings.alwaysOnTop !== runtimeSettings.alwaysOnTop;
          const clickThroughChanged = refreshedSettings.clickThrough !== runtimeSettings.clickThrough;
          const enabledChanged = refreshedSettings.liveOverlayEnabled !== runtimeSettings.liveOverlayEnabled;
          const autoHideChanged = refreshedSettings.autoHideOutsideCombat !== runtimeSettings.autoHideOutsideCombat;
          runtimeSettings = refreshedSettings;
          settingsFingerprint = refreshedFingerprint;
          if (!runtimeSettings.layers.some((layer) => layer.id === activeLayerId)) {
            activeLayerId = runtimeSettings.layers[0]?.id ?? null;
          }
          if (alwaysOnTopChanged) await appWindow.setAlwaysOnTop(runtimeSettings.alwaysOnTop);
          if (enabledChanged || autoHideChanged) {
            clearVisibilityTimer();
            automaticallyHidden = !runtimeSettings.liveOverlayEnabled
              || (runtimeSettings.autoHideOutsideCombat && latestSnapshot?.combat_active !== true);
            root.classList.remove("is-auto-hidden");
            await appWindow.setEnabled(runtimeSettings.liveOverlayEnabled, automaticallyHidden);
          }
          if (clickThroughChanged) {
            await appWindow.setIgnoreCursorEvents(
              shouldIgnoreCombatOverlayCursor(automaticallyHidden, runtimeSettings.clickThrough),
            );
          }
        }
        const refreshedTimerFingerprint = refreshedTimerSettings === null
          ? timerSettingsFingerprint
          : JSON.stringify(refreshedTimerSettings);
        if (
          refreshedTimerSettings !== null &&
          refreshedTimerFingerprint !== timerSettingsFingerprint
        ) {
          shouldRender = true;
          timerSettings = refreshedTimerSettings;
          timerSettingsFingerprint = refreshedTimerFingerprint;
        }
        syncCombatVisibility();
        if (shouldRender) requestFeedRender();
        consecutiveRuntimeFailures = 0;
        delete root.dataset.runtimeError;
      } catch (error) {
        consecutiveRuntimeFailures += 1;
        root.dataset.runtimeError = String(consecutiveRuntimeFailures);
        root.title = `Live overlay update failed: ${errorMessage(error)}`;
        if (canvas.querySelector(".combat-overlay-layer") === null) {
          canvas.replaceChildren(
            text(
              "p",
              "Live overlay is reconnecting…",
              "combat-overlay-runtime-loading combat-overlay-runtime-error",
            ),
          );
        }
        const retryMillis = Math.min(
          5_000,
          250 * (2 ** Math.min(4, consecutiveRuntimeFailures - 1)),
        );
        await new Promise((resolve) => window.setTimeout(resolve, retryMillis));
      }
    }
  };
  window.addEventListener("beforeunload", () => {
    active = false;
    clearVisibilityTimer();
    if (frameTimer !== null) window.clearTimeout(frameTimer);
    if (resizeSaveTimer !== null) window.clearTimeout(resizeSaveTimer);
    window.clearInterval(heartbeatTimer);
    stopResizeListener?.();
    stopShowRequestListener?.();
  }, { once: true });
  void run();
  if (settings.clickThrough) {
    window.setTimeout(() => void appWindow.setIgnoreCursorEvents(true), 1_500);
  }
}

function runtimeResizeHandle(
  direction: OverlayResizeDirection,
  appWindow: CombatOverlayRuntimeWindow,
): HTMLButtonElement {
  const handle = el("button", "combat-overlay-runtime-resize-handle") as HTMLButtonElement;
  handle.type = "button";
  handle.dataset.direction = direction;
  handle.title = direction === "East"
    ? "Drag to resize overlay width"
    : direction === "South"
      ? "Drag to resize overlay height"
      : "Drag to resize overlay";
  handle.setAttribute("aria-label", handle.title);
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    void appWindow.startResizeDragging(direction);
  });
  return handle;
}

interface ProjectedOverlayState {
  actors: readonly OverlayActor[];
  snapshot: OverlaySnapshot | null | undefined;
  view: OverlayHistoryView | null;
}

function resolveProjectedOverlayState(
  actorSource: readonly OverlayActor[],
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  selectedViewId: string | undefined,
): ProjectedOverlayState {
  const projection = presentation?.run_projection;
  if (
    selectedViewId === undefined
    || selectedViewId === "live"
    || projection === null
    || projection === undefined
    || projection.views.length === 0
  ) {
    const actors = applyOverlayRdpsSkillDetail(
      actorSource,
      snapshot?.rdps_damage_influences ?? [],
      snapshot?.rdps_effect_presentations ?? [],
      snapshot?.active_combat_micros ?? 0,
      snapshot?.rdps_damage_influences_truncated === true,
    );
    return { actors, snapshot: snapshot === undefined || snapshot === null ? snapshot : { ...snapshot, actors }, view: null };
  }
  const view = projection.views.find((candidate) => candidate.id === selectedViewId)
    ?? projection.views.find((candidate) => candidate.id === "all")
    ?? projection.views[0]
    ?? null;
  if (view === null) return { actors: actorSource, snapshot, view: null };
  const liveActorsById = new Map(actorSource.map((actor) => [actor.actor_id, actor]));
  const projectedActors = view.actors
    .map(projectedActorToOverlayActor)
    .map((actor) => view.id === "all"
      && projection.total_run_time_micros !== null
      && projection.total_run_time_micros !== undefined
      && projection.total_run_time_micros > 0
      ? {
          ...actor,
          dps: actor.dps * view.elapsed_micros / projection.total_run_time_micros,
        }
      : actor)
    .map((actor) => mergeProjectedActorPresentation(actor, liveActorsById.get(actor.actor_id)))
    .filter(isOverlayRosterActor);
  const actors = applyOverlayRdpsSkillDetail(
    projectedActors,
    view.damage_influences ?? [],
    view.rdps_effect_presentations ?? [],
    view.active_combat_micros,
    false,
  );
  return {
    actors,
    view,
    snapshot: {
      ...(snapshot ?? { actors: [] }),
      active_combat_micros: view.active_combat_micros,
      encounter_elapsed_micros: view.elapsed_micros,
      run_elapsed_micros: view.id === "all"
        ? projection.total_run_time_micros
        : view.elapsed_micros,
      game_time_micros: view.id === "all"
        ? projection.game_time_micros
        : view.elapsed_micros,
      true_time_micros: projection.true_time_micros,
      actors,
    },
  };
}

/**
 * Joins exact decimal-string influence rows to the affected recipient skill.
 * This is presentation-only: actor totals and ordinary skill damage are never
 * mutated, and unavailable/truncated attribution remains visibly unresolved.
 */
export function applyOverlayRdpsSkillDetail(
  actors: readonly OverlayActor[],
  influences: readonly OverlayDamageInfluence[],
  effectPresentations: readonly OverlayRdpsEffectPresentation[],
  activeCombatMicros: number,
  truncated: boolean,
): readonly OverlayActor[] {
  const actorNames = new Map(actors.map((actor) => [actor.actor_id, actorName(actor)]));
  const effectNames = new Map(
    effectPresentations.map((effect) => [effect.effect_id, effect.presentation_name]),
  );
  const byRecipientAbility = new Map<string, OverlayDamageInfluence[]>();
  const grantsByProvider = new Map<string, Map<string, OverlayAbilityRdpsGrant & {
    providerAbilityId: string | null;
  }>>();
  for (const influence of influences) {
    if (influence.affected_ability_id !== null) {
      const key = `${influence.recipient_actor_id}\u0000${influence.affected_ability_id}`;
      const rows = byRecipientAbility.get(key) ?? [];
      rows.push(influence);
      byRecipientAbility.set(key, rows);
    }
    if (influence.attributed_rdps === null || influence.attributed_rdps === undefined) continue;
    const component = influence.attribution_component?.trim() || null;
    const providerAbilityId = influence.provider_ability_id?.trim() || null;
    const providerGrants = grantsByProvider.get(influence.provider_actor_id) ?? new Map();
    const grantKey = `${providerAbilityId ?? ""}\u0000${influence.effect_id}\u0000${component ?? ""}`;
    const previous = providerGrants.get(grantKey);
    const amount = (previous === undefined ? 0n : BigInt(previous.attributed_rdps))
      + BigInt(influence.attributed_rdps);
    providerGrants.set(grantKey, {
      providerAbilityId,
      effect_id: influence.effect_id,
      effect_name: effectNames.get(influence.effect_id) ?? "Unlocalized combat effect",
      attribution_component: component,
      attributed_rdps: amount.toString(),
      rdps: 0,
      damage_event_count: (previous?.damage_event_count ?? 0) + influence.damage_event_count,
    });
    grantsByProvider.set(influence.provider_actor_id, providerGrants);
  }
  const duration = influences.length > 0
    ? Math.max(1_000_000, activeCombatMicros)
    : Math.max(0, activeCombatMicros);
  const projected = actors.map((actor) => {
    const abilities = (actor.abilities ?? []).map((ability) => {
      const rows = byRecipientAbility.get(`${actor.actor_id}\u0000${ability.ability_id}`) ?? [];
      const grouped = new Map<string, OverlayAbilityRdpsSource>();
      let unresolved = 0;
      for (const row of rows) {
        if (!row.damage_context_complete || row.attributed_rdps === null || row.attributed_rdps === undefined) {
          unresolved += 1;
          continue;
        }
        const component = row.attribution_component?.trim() || null;
        const key = `${row.provider_actor_id}\u0000${row.effect_id}\u0000${component ?? ""}`;
        const previous = grouped.get(key);
        const amount = (previous === undefined ? 0n : BigInt(previous.attributed_rdps))
          + BigInt(row.attributed_rdps);
        grouped.set(key, {
          provider_actor_id: row.provider_actor_id,
          provider_name: actorNames.get(row.provider_actor_id) ?? "Unidentified participant",
          effect_id: row.effect_id,
          effect_name: effectNames.get(row.effect_id) ?? "Unlocalized combat effect",
          attribution_component: component,
          attributed_rdps: amount.toString(),
          rdps: duration === 0 ? 0 : Number(amount) * 1_000_000 / duration,
          damage_event_count: (previous?.damage_event_count ?? 0) + row.damage_event_count,
        });
      }
      const sources = [...grouped.values()].sort((left, right) => {
        const leftAmount = BigInt(left.attributed_rdps);
        const rightAmount = BigInt(right.attributed_rdps);
        if (leftAmount !== rightAmount) return leftAmount > rightAmount ? -1 : 1;
        return left.provider_actor_id.localeCompare(right.provider_actor_id);
      });
      const received = sources.reduce(
        (sum, source) => sum + BigInt(source.attributed_rdps),
        0n,
      );
      return {
        ...ability,
        rdps_received_damage: received.toString(),
        rdps_received_rate: duration === 0 ? 0 : Number(received) * 1_000_000 / duration,
        rdps_sources: sources,
        rdps_unresolved_relationship_count: unresolved,
      };
    });
    const grants = [...(grantsByProvider.get(actor.actor_id)?.values() ?? [])].map((grant) => ({
      ...grant,
      rdps: duration === 0 ? 0 : Number(BigInt(grant.attributed_rdps)) * 1_000_000 / duration,
    }));
    const supportGrants: OverlayAbilityRdpsGrant[] = [];
    for (const grant of grants) {
      const abilityIndex = grant.providerAbilityId === null
        ? -1
        : abilities.findIndex((ability) => ability.ability_id === grant.providerAbilityId);
      if (abilityIndex < 0) {
        supportGrants.push(grant);
        continue;
      }
      const ability = abilities[abilityIndex]!;
      const abilityGrants = [...(ability.rdps_grants ?? []), grant];
      const given = abilityGrants.reduce(
        (sum, entry) => sum + BigInt(entry.attributed_rdps),
        0n,
      );
      abilities[abilityIndex] = {
        ...ability,
        rdps_given_damage: given.toString(),
        rdps_given_rate: duration === 0 ? 0 : Number(given) * 1_000_000 / duration,
        rdps_grants: abilityGrants,
      };
    }
    for (const grant of supportGrants) {
      abilities.push({
        ability_id: `support-effect:${grant.effect_id}:${grant.attribution_component ?? "complete"}`,
        presentation_name: grant.effect_name,
        casts: 0,
        hits: 0,
        critical_hits: 0,
        reported_damage: 0,
        effective_damage: 0,
        reported_healing: 0,
        effective_healing: 0,
        shielding: 0,
        rdps_received_damage: "0",
        rdps_received_rate: 0,
        rdps_sources: [],
        rdps_unresolved_relationship_count: 0,
        rdps_given_damage: grant.attributed_rdps,
        rdps_given_rate: grant.rdps,
        rdps_grants: [grant],
        rdps_support_effect: true,
        rdps_effect_id: grant.effect_id,
      });
    }
    return {
      ...actor,
      rdps_skill_detail_truncated: truncated,
      abilities,
    };
  });
  return moveOverlayEncoreOwnedSkills(projected, influences);
}

const OVERLAY_ENCORE_EFFECT_ID = "55333";
const OVERLAY_ENCORE_DAMAGE_ACTION_IDS = new Set(["230401", "230501"]);

function moveOverlayEncoreOwnedSkills(
  actors: OverlayActor[],
  influences: readonly OverlayDamageInfluence[],
): OverlayActor[] {
  const byActor = new Map(actors.map((actor) => [actor.actor_id, actor]));
  const movements = new Map<string, Map<string, { damage: bigint; hits: number }>>();
  for (const influence of influences) {
    if (
      influence.effect_id !== OVERLAY_ENCORE_EFFECT_ID ||
      !influence.damage_context_complete ||
      influence.provider_actor_id === influence.recipient_actor_id ||
      !influence.affected_ability_id ||
      !OVERLAY_ENCORE_DAMAGE_ACTION_IDS.has(influence.affected_ability_id) ||
      !influence.attribution_component ||
      humanizeOverlayAttributionComponent(influence.attribution_component).toLocaleLowerCase() !==
        "encore standalone generated damage" ||
      influence.attributed_rdps == null ||
      !/^\d+$/u.test(influence.attributed_rdps)
    ) continue;
    const key = `${influence.recipient_actor_id}\0${influence.affected_ability_id}`;
    const providers = movements.get(key) ?? new Map();
    const prior = providers.get(influence.provider_actor_id) ?? { damage: 0n, hits: 0 };
    providers.set(influence.provider_actor_id, {
      damage: prior.damage + BigInt(influence.attributed_rdps),
      hits: prior.hits + influence.damage_event_count,
    });
    movements.set(key, providers);
  }

  const affectedProviders = new Set<string>();
  for (const [key, providers] of movements) {
    const separator = key.indexOf("\0");
    const recipient = byActor.get(key.slice(0, separator));
    const actionId = key.slice(separator + 1);
    const ability = recipient?.abilities?.find((candidate) => candidate.ability_id === actionId);
    if (!recipient || !ability) continue;
    const moved = [...providers.values()].reduce((sum, value) => sum + value.damage, 0n);
    const movedHits = [...providers.values()].reduce((sum, value) => sum + value.hits, 0);
    const movedDamage = Number(moved);
    if (
      !Number.isSafeInteger(movedDamage) ||
      movedDamage > ability.reported_damage ||
      movedHits > ability.hits ||
      [...providers.keys()].some((providerId) => !byActor.has(providerId))
    ) continue;
    if (movedDamage === ability.reported_damage) {
      recipient.abilities = recipient.abilities?.filter((candidate) => candidate !== ability);
    } else {
      ability.reported_damage -= movedDamage;
      ability.effective_damage = Math.max(0, ability.effective_damage - movedDamage);
      ability.hits -= movedHits;
      ability.critical_hits = Math.min(ability.critical_hits, ability.hits);
      ability.rdps_received_damage = "0";
      ability.rdps_received_rate = 0;
      ability.rdps_sources = [];
    }
    for (const [providerId, value] of providers) {
      const provider = byActor.get(providerId)!;
      const support = provider.abilities?.find((candidate) =>
        candidate.rdps_support_effect === true &&
        candidate.rdps_effect_id === OVERLAY_ENCORE_EFFECT_ID
      );
      if (!support) continue;
      const damage = Number(value.damage);
      if (!Number.isSafeInteger(damage)) continue;
      support.reported_damage += damage;
      support.effective_damage += damage;
      support.hits += value.hits;
      affectedProviders.add(providerId);
    }
  }

  for (const providerId of affectedProviders) {
    const provider = byActor.get(providerId);
    if (!provider?.abilities) continue;
    const support = provider.abilities.find((ability) =>
      ability.rdps_support_effect === true &&
      ability.rdps_effect_id === OVERLAY_ENCORE_EFFECT_ID
    );
    if (!support) continue;
    const nativeEncore = provider.abilities.filter((ability) =>
      OVERLAY_ENCORE_DAMAGE_ACTION_IDS.has(ability.ability_id)
    );
    for (const ability of nativeEncore) {
      support.reported_damage += ability.reported_damage;
      support.effective_damage += ability.effective_damage;
      support.hits += ability.hits;
      support.critical_hits += ability.critical_hits;
    }
    provider.abilities = provider.abilities.filter((ability) => !nativeEncore.includes(ability));
  }
  return actors;
}

/**
 * A reviewed-run projection owns the selected segment's metrics, but the live
 * decoder owns the freshest capture-time identity and loadout presentation.
 * Keep those responsibilities separate so selecting a projected dungeon view
 * cannot make names, weapons, or Imagines wait for run finalization.
 */
export function mergeProjectedActorPresentation(
  projected: OverlayActor,
  live: OverlayActor | undefined,
): OverlayActor {
  if (live === undefined) return projected;

  const projectedPresentation = projected.presentation;
  const livePresentation = live.presentation;
  const presentation = livePresentation === undefined
    ? projectedPresentation
    : {
        character_id: livePresentation.character_id ?? projectedPresentation?.character_id ?? null,
        class_id: livePresentation.class_id ?? projectedPresentation?.class_id ?? null,
        specialization_id: livePresentation.specialization_id
          ?? projectedPresentation?.specialization_id
          ?? null,
        class_name: livePresentation.class_name ?? projectedPresentation?.class_name ?? null,
        specialization_name: livePresentation.specialization_name
          ?? projectedPresentation?.specialization_name
          ?? null,
        class_spec_icon_asset_path: livePresentation.class_spec_icon_asset_path
          ?? projectedPresentation?.class_spec_icon_asset_path
          ?? null,
        role: livePresentation.role ?? projectedPresentation?.role ?? null,
        accent: livePresentation.accent ?? projectedPresentation?.accent ?? null,
        weapon: livePresentation.weapon ?? projectedPresentation?.weapon ?? null,
        primary_imagines: livePresentation.primary_imagines.length > 0
          ? livePresentation.primary_imagines
          : projectedPresentation?.primary_imagines ?? [],
      } satisfies OverlayActorPresentation;

  return {
    ...projected,
    entity_uuid: live.entity_uuid ?? projected.entity_uuid,
    display_name: preferredOverlayDisplayName(live.display_name, projected.display_name),
    actor_kind: live.actor_kind ?? projected.actor_kind,
    monster_id: live.monster_id ?? projected.monster_id,
    presentation,
  };
}

/**
 * The terminal live snapshot can be less complete than the frozen run
 * projection while capture ownership changes hands. A generic player label or
 * public UID is useful as a final fallback, but it must never replace a real
 * name that the completed projection already retained.
 */
export function preferredOverlayDisplayName(
  liveName: string | null | undefined,
  projectedName: string | null | undefined,
): string | null {
  const live = liveName?.trim() || null;
  const projected = projectedName?.trim() || null;
  if (live !== null && !overlayDisplayNameIsPlaceholder(live)) return live;
  if (projected !== null && !overlayDisplayNameIsPlaceholder(projected)) return projected;
  return live ?? projected;
}

function overlayDisplayNameIsPlaceholder(name: string): boolean {
  const folded = name.trim().toLowerCase();
  if (/^\d+$/.test(folded)) return true;
  return ["player", "actor", "uid", "unknown"].some((prefix) =>
    folded === prefix || new RegExp(`^${prefix}\\s+\\d+$`).test(folded));
}

function projectedActorToOverlayActor(actor: OverlayActor | OverlayHistoryActor): OverlayActor {
  if (!("encounter_dps" in actor)) return actor;
  const role = actor.presentation_role === "damage"
    || actor.presentation_role === "healer"
    || actor.presentation_role === "tank"
    ? actor.presentation_role
    : null;
  const accent = actor.presentation_accent === "damage_glow" ? "damage_glow" : null;
  const weaponItemId = actor.weapon_item_id ?? null;
  return {
    actor_id: actor.actor_id,
    entity_uuid: actor.entity_uuid,
    display_name: actor.presentation_name?.trim() || actor.display_name || null,
    actor_kind: actor.actor_kind,
    monster_id: optionalNumber(actor.monster_id),
    dps: actor.dps,
    edps: actor.dps,
    adps: actor.encounter_dps,
    hps: actor.hps,
    tps: actor.tps,
    rdps: actor.rdps ?? null,
    reported_damage: actor.damage,
    effective_damage: actor.effective_damage,
    damage_taken: actor.damage_taken,
    rdps_damage: actor.rdps_damage ?? null,
    rdps_contribution_given: actor.rdps_contribution_given ?? null,
    rdps_contribution_received: actor.rdps_contribution_received ?? null,
    reported_healing: actor.healing,
    effective_healing: actor.effective_healing,
    overheal: Math.max(0, actor.healing - actor.effective_healing),
    shielding: actor.shielding,
    casts: actor.observed_cast_events ?? 0,
    hits: actor.hits,
    critical_hits: actor.critical_hits,
    deaths: actor.deaths,
    revives: 0,
    abilities: (actor.abilities ?? []).map((ability) => ({
      ability_id: ability.ability_id,
      presentation_name: ability.presentation_name,
      icon_asset_path: ability.icon_asset_path,
      casts: ability.casts,
      hits: ability.hits,
      critical_hits: ability.critical_hits,
      reported_damage: ability.damage,
      effective_damage: ability.effective_damage,
      reported_healing: ability.healing,
      effective_healing: ability.effective_healing,
      shielding: ability.shielding,
    })),
    presentation: {
      character_id: actor.character_id ?? null,
      class_id: actor.class_id ?? null,
      specialization_id: actor.specialization_id ?? null,
      class_name: actor.presentation_class_name ?? null,
      specialization_name: actor.presentation_specialization_name ?? null,
      class_spec_icon_asset_path: actor.icon_asset_path ?? null,
      role,
      accent,
      weapon: weaponItemId === null && !actor.weapon_icon_asset_path?.trim()
        ? null
        : {
            slot_id: null,
            ability_id: null,
            item_id: weaponItemId,
            tier: null,
            level: actor.weapon_level ?? null,
            level_min: actor.weapon_level_min ?? null,
            level_max: actor.weapon_level_max ?? null,
            badge_kind: actor.weapon_badge_kind ?? null,
            label: actor.weapon_presentation_name?.trim() || (weaponItemId === null ? "Weapon" : `Weapon ${weaponItemId}`),
            icon_asset_path: actor.weapon_icon_asset_path ?? null,
          },
      primary_imagines: (actor.primary_loadout ?? []).map((slot) => ({
        slot_id: slot.slot_id,
        ability_id: slot.ability_id ?? null,
        item_id: slot.item_id ?? null,
        tier: slot.tier ?? null,
        level: null,
        level_min: null,
        level_max: null,
        badge_kind: null,
        label: slot.presentation_name?.trim() || `Loadout slot ${slot.slot_id}`,
        icon_asset_path: slot.icon_asset_path ?? null,
      })),
    },
  };
}

function optionalNumber(value: string | number | null | undefined): number | null {
  if (value === null || value === undefined || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function availableSegmentViews(
  presentation: OverlayEncounterPresentation | null | undefined,
): readonly OverlayHistoryView[] {
  return presentation?.run_projection?.views ?? [];
}

export function availableTimerFields(
  presentation: OverlayEncounterPresentation | null | undefined,
  snapshot?: OverlaySnapshot | null,
): readonly OverlaySummaryField[] {
  const projection = presentation?.run_projection;
  return [
    "attempt_time" as const,
    "encounter_time" as const,
    ...(projection?.game_time_micros == null && snapshot?.game_time_micros == null
      ? []
      : ["game_time" as const]),
    ...(projection?.true_time_micros == null && snapshot?.true_time_micros == null
      ? []
      : ["true_time" as const]),
    ...(projection?.total_run_time_micros == null && snapshot?.run_elapsed_micros == null
      ? []
      : ["run_time" as const]),
  ];
}

function selectedTimerField(
  selected: ReadonlyMap<string, OverlaySummaryField> | undefined,
  layerId: string,
  presentation: OverlayEncounterPresentation | null | undefined,
  snapshot?: OverlaySnapshot | null,
): OverlaySummaryField {
  const available = availableTimerFields(presentation, snapshot);
  const requested = selected?.get(layerId);
  return requested !== undefined && available.includes(requested)
    ? requested
    : available[0] ?? "encounter_time";
}

function timerDurationMicros(
  field: OverlaySummaryField,
  snapshot: OverlaySnapshot | null | undefined,
): number | null {
  if (field === "attempt_time") return snapshot?.attempt_elapsed_micros ?? null;
  if (field === "run_time") return snapshot?.run_elapsed_micros ?? null;
  if (field === "game_time") return snapshot?.game_time_micros ?? null;
  if (field === "true_time") return snapshot?.true_time_micros ?? null;
  return snapshot?.encounter_elapsed_micros ?? snapshot?.active_combat_micros ?? null;
}

function rateFromAmount(amount: number | null | undefined, durationMicros: number): number | null {
  if (amount === null || amount === undefined) return null;
  return Math.max(0, amount) * 1_000_000 / Math.max(1_000_000, durationMicros);
}

export function projectOverlayRatesForTimer(
  actors: readonly OverlayActor[],
  snapshot: OverlaySnapshot | null | undefined,
  timer: OverlaySummaryField,
): OverlayActor[] {
  const duration = timerDurationMicros(timer, snapshot);
  if (duration === null) return [...actors];
  return actors.map((actor) => {
    const damage = actor.reported_damage;
    const rdps = rateFromAmount(actor.rdps_damage, duration);
    const abilities = actor.abilities?.map((ability) => {
      const received = Number(ability.rdps_received_damage ?? "0");
      const given = Number(ability.rdps_given_damage ?? "0");
      return {
        ...ability,
        rdps_received_rate: rateFromAmount(received, duration) ?? 0,
        rdps_given_rate: rateFromAmount(given, duration) ?? 0,
        rdps_sources: ability.rdps_sources?.map((source) => ({
          ...source,
          rdps: rateFromAmount(Number(source.attributed_rdps), duration) ?? 0,
        })),
        rdps_grants: ability.rdps_grants?.map((grant) => ({
          ...grant,
          rdps: rateFromAmount(Number(grant.attributed_rdps), duration) ?? 0,
        })),
      };
    });
    return {
      ...actor,
      dps: rateFromAmount(damage, duration) ?? actor.dps,
      // eDPS and aDPS are semantic rates with reducer-owned clocks. eDPS uses
      // the paused current-phase combat clock; aDPS freezes at the latest
      // accepted player-damage event. Both restore their boss-entry checkpoint
      // on a retry. The timer selector still controls generic DPS/HPS/TPS/rDPS
      // and their drilldowns.
      edps: actor.edps ?? actor.dps,
      adps: actor.adps ?? actor.edps ?? actor.dps,
      hps: rateFromAmount(actor.reported_healing, duration) ?? actor.hps,
      tps: rateFromAmount(actor.damage_taken, duration) ?? actor.tps,
      rdps,
      abilities,
    };
  });
}

function cycleSelectedTimer(
  selected: Map<string, OverlaySummaryField>,
  layerId: string,
  presentation: OverlayEncounterPresentation | null | undefined,
  snapshot?: OverlaySnapshot | null,
): void {
  const available = availableTimerFields(presentation, snapshot);
  if (available.length === 0) return;
  const current = selectedTimerField(selected, layerId, presentation, snapshot);
  const currentIndex = available.indexOf(current);
  selected.set(layerId, available[(currentIndex + 1 + available.length) % available.length]!);
}

function cycleSelectedSegment(
  selected: Map<string, string>,
  layerId: string,
  presentation: OverlayEncounterPresentation | null | undefined,
): void {
  const available = availableSegmentViews(presentation);
  if (available.length === 0) {
    selected.set(layerId, "live");
    return;
  }
  const viewIds = ["live", ...available.map((view) => view.id)];
  const current = selected.get(layerId) ?? "live";
  const currentIndex = viewIds.indexOf(current);
  selected.set(layerId, viewIds[(currentIndex + 1 + viewIds.length) % viewIds.length]!);
}

function runtimeControlLabel(
  control: OverlayButton,
  layerId: string,
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  selectedTimers: ReadonlyMap<string, OverlaySummaryField> | undefined,
  selectedSegments: ReadonlyMap<string, string> | undefined,
): string {
  if (control.action === "cycle_segment") {
    const views = availableSegmentViews(presentation);
    if (views.length === 0) return "Live";
    const selectedId = selectedSegments?.get(layerId);
    if (selectedId === undefined || selectedId === "live") return "Live";
    return views.find((view) => view.id === selectedId)?.label ?? "Live";
  }
  if (control.action === "cycle_timer") {
    const field = selectedTimerField(selectedTimers, layerId, presentation, snapshot);
    return `${timerFieldLabel(field)} ${summaryTimerValue(field, snapshot)}`;
  }
  return control.label;
}

function timerFieldLabel(field: OverlaySummaryField): string {
  if (field === "attempt_time") return "Attempt";
  if (field === "run_time") return "Run";
  if (field === "game_time") return "Game";
  if (field === "true_time") return "True";
  return "Encounter";
}

function summaryTimerValue(
  field: OverlaySummaryField,
  snapshot: OverlaySnapshot | null | undefined,
): string {
  if (field === "attempt_time") return formatOptionalOverlayTime(snapshot?.attempt_elapsed_micros);
  if (field === "run_time") return formatOptionalOverlayTime(snapshot?.run_elapsed_micros);
  if (field === "game_time") return formatOptionalOverlayTime(snapshot?.game_time_micros);
  if (field === "true_time") return formatOptionalOverlayTime(snapshot?.true_time_micros);
  return formatOptionalOverlayTime(
    snapshot?.encounter_elapsed_micros ?? snapshot?.active_combat_micros,
  );
}

export function renderOverlayCanvas(
  canvas: HTMLElement,
  settings: CombatOverlaySettings,
  actorSource: readonly OverlayActor[],
  options: RenderOptions,
): void {
  canvas.replaceChildren();
  const scale = overlayScale(settings);
  const selectedLayer = settings.layers.find((candidate) => candidate.id === options.selectedLayerId)
    ?? settings.layers[0];
  const projected = resolveProjectedOverlayState(
    actorSource,
    options.snapshot,
    options.encounterPresentation,
    selectedLayer === undefined ? undefined : options.selectedSegmentByLayer?.get(selectedLayer.id),
  );
  const projectedPresentation = projected.view?.kind === "mobbing"
    && options.encounterPresentation !== null
    && options.encounterPresentation !== undefined
    ? { ...options.encounterPresentation, bosses: [] }
    : options.encounterPresentation;
  const rdpsStatus = options.encounterPresentation?.run_projection?.rdps_status
    ?? projected.snapshot?.rdps_status
    ?? null;
  const rdpsAvailability = describeOverlayRdpsAvailability(rdpsStatus);
  const timer = selectedLayer === undefined
    ? "encounter_time"
    : selectedTimerField(
        options.selectedTimerByLayer,
        selectedLayer.id,
        options.encounterPresentation,
        projected.snapshot,
      );
  const actors = projectOverlayRatesForTimer(
    maskUnavailableOverlayRdps(projected.actors, rdpsAvailability),
    projected.snapshot,
    timer,
  )
    .filter((actor) => metricValue(actor, "dps") + metricValue(actor, "hps") + metricValue(actor, "tps") > 0);
  const layer = selectedLayer;
  if (layer !== undefined) {
    const layerElement = el("section", "combat-overlay-layer");
    layerElement.dataset.layerId = layer.id;
    layerElement.dataset.mode = options.mode;
    if (options.selectedLayerId === layer.id) layerElement.dataset.selected = "true";
    layerElement.style.left = "0";
    layerElement.style.top = "0";
    layerElement.style.width = `${settings.canvasWidth}px`;
    layerElement.style.height = settings.dynamicHeight ? "auto" : `${settings.canvasHeight}px`;
    layerElement.style.transform = `scale(${scale})`;
    layerElement.style.transformOrigin = "top left";
    applyOverlayBackground(layerElement, settings);
    layerElement.addEventListener("pointerdown", () => {
      if (options.selectedLayerId !== layer.id) options.onSelectLayer?.(layer.id);
    });
    layerElement.addEventListener("contextmenu", (event) =>
      options.onContextMenu?.(event, { kind: "layer", layerId: layer.id }),
    );

    const selectedActorId = options.selectedActorByLayer?.get(layer.id);
    const selectedActor = selectedActorId === undefined
      ? undefined
      : actors.find((actor) => actor.actor_id === selectedActorId);
    const viewControls = el("div", "combat-overlay-view-controls");
    for (const view of settings.showViewTabs ? settings.layers : []) {
      const viewButton = button(view.title, "combat-overlay-control combat-overlay-view-control");
      viewButton.dataset.viewId = view.id;
      viewButton.dataset.active = String(view.id === layer.id);
      viewButton.title = view.id === layer.id
        ? `${view.title} is the active header view`
        : `Switch to the ${view.title} header view`;
      if (options.mode === "preview") {
        const grip = text("span", "⋮⋮", "combat-overlay-reorder-grip");
        grip.title = `Drag to move the ${view.title} view button`;
        viewButton.prepend(grip);
        wirePointerReorder(
          grip,
          layer.id,
          view.id,
          "viewId",
          (target, placement) => options.onReorderLayers?.(view.id, target, placement),
        );
      }
      viewButton.addEventListener("click", (event) => {
        event.stopPropagation();
        options.onSelectLayer?.(view.id);
      });
      viewButton.addEventListener("contextmenu", (event) => {
        event.stopPropagation();
        options.onContextMenu?.(event, { kind: "view", layerId: view.id });
      });
      viewControls.append(viewButton);
    }
    if (selectedActor !== undefined) {
      const back = button("Back", "combat-overlay-control combat-overlay-back-control");
      back.title = `Return to ${layer.title}`;
      back.addEventListener("click", (event) => {
        event.stopPropagation();
        options.onCloseActor?.(layer.id);
      });
      viewControls.append(back);
    }
    const summary = renderEncounterSummary(
      layer,
      settings.numberFormats,
      actors,
      selectedActor,
      projected.snapshot,
      projectedPresentation,
      viewControls,
      options,
      scale,
    );

    if (selectedActor !== undefined) {
      layerElement.style.setProperty("--meter-color", actorBarColor(selectedActor, settings));
      renderAbilityBreakdown(layerElement, layer, selectedActor, summary, settings.numberFormats);
      canvas.append(layerElement);
      return;
    }

    const header = el("div", "combat-overlay-row combat-overlay-header-row");
    header.style.gridTemplateColumns = gridColumns(layer.headerFields, layer.headerWidths);
    for (const field of layer.headerFields) {
      const label = fieldLabel(field);
      const visibleLabel = overlayHeaderLabel(field, layer.metric);
      const labelHidden = layer.hiddenHeaderLabels.includes(field);
      const cell = text("span", labelHidden ? "" : visibleLabel);
      cell.dataset.headerField = field;
      cell.setAttribute("aria-label", label);
      if (options.mode === "preview") {
        cell.classList.add("combat-overlay-reorder-target");
        cell.title = `Drag the ${label} header to reorder it. Drag its cyan divider to resize it. Right-click for column options.`;
        cell.addEventListener("contextmenu", (event) => {
          event.stopPropagation();
          options.onContextMenu?.(event, { kind: "header", layerId: layer.id, field });
        });
        wirePointerReorder(
          cell,
          layer.id,
          field,
          "headerField",
          (target, placement) => options.onReorderHeaders?.(
            layer.id,
            field,
            target as OverlayHeaderField,
            placement,
          ),
        );
        const resize = text("span", "", "combat-overlay-header-resize");
        resize.dataset.width = `${headerWidthFor(layer, field)} px`;
        resize.title = `Drag to resize the ${fieldLabel(field)} column (${resize.dataset.width})`;
        wireHeaderResize(
          resize,
          layerElement,
          layer,
          field,
          scale,
          (width) => options.onResizeHeader?.(layer.id, field, width),
        );
        cell.append(resize);
      }
      header.append(cell);
    }

    const rows = el("div", "combat-overlay-rows");
    const sorted = [...actors]
      .sort((left, right) => metricValue(right, layer.metric) - metricValue(left, layer.metric))
      .slice(0, settings.maxVisiblePlayers);
    const maximum = Math.max(1, ...sorted.map((actor) => metricValue(actor, layer.metric)));
    for (const [index, actor] of sorted.entries()) {
      const row = el("div", "combat-overlay-row combat-overlay-actor-row");
      row.style.gridTemplateColumns = gridColumns(layer.headerFields, layer.headerWidths);
      row.style.setProperty("--meter-fill", `${Math.max(0, metricValue(actor, layer.metric) / maximum) * 100}%`);
      row.style.setProperty("--meter-color", actorBarColor(actor, settings));
      for (const field of layer.headerFields) {
        if (field === "name") {
          const actorLink = button(actorName(actor), "combat-overlay-actor-link overlay-field-name");
          actorLink.title = `Open ${actorName(actor)} skill breakdown`;
          actorLink.addEventListener("click", (event) => {
            event.stopPropagation();
            options.onSelectActor?.(layer.id, actor.actor_id);
          });
          row.append(actorLink);
        } else if (field === "class_spec" || field === "weapon" || field === "main_imagines") {
          row.append(renderBadgeCell(field, actor));
        } else {
          row.append(text(
            "span",
            fieldValue(field, actor, index, layer.metric, maximum, settings.numberFormats),
            `overlay-field-${field}`,
          ));
        }
      }
      rows.append(row);
    }
    if (rows.childElementCount === 0) {
      rows.append(text(
        "p",
        options.emptyMessage ?? (options.mode === "runtime" ? "Waiting for combat..." : "No example rows"),
        "combat-overlay-empty",
      ));
    }
    layerElement.append(summary);
    if (layerUsesRdps(layer) && !rdpsAvailability.providerCreditEnabled) {
      layerElement.append(text(
        "p",
        rdpsAvailability.message,
        "combat-overlay-rdps-status",
      ));
    }
    layerElement.append(header, rows);
    canvas.append(layerElement);
  }
  canvas.addEventListener("contextmenu", (event) => {
    if (event.target === canvas) options.onContextMenu?.(event, { kind: "canvas" });
  });
}

function renderEncounterSummary(
  layer: OverlayLayer,
  numberFormats: OverlayNumberFormats,
  actors: readonly OverlayActor[],
  selectedActor: OverlayActor | undefined,
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  viewControls: HTMLElement,
  options: RenderOptions,
  scale: number,
): HTMLElement {
  const summary = el("section", "combat-overlay-summary");
  summary.addEventListener("contextmenu", (event) => {
    if ((event.target as HTMLElement).closest("[data-summary-field]")) return;
    event.stopPropagation();
    options.onContextMenu?.(event, { kind: "summary", layerId: layer.id });
  });
  const teamActors = actors.filter((actor) => actor.actor_kind !== "monster");
  const teamDamage = teamActors.reduce(
    (total, actor) => total + Math.max(0, actor.reported_damage ?? 0),
    0,
  );
  const teamDps = teamActors.reduce(
    (total, actor) => total + Math.max(0, metricValue(actor, layer.metric)),
    0,
  );
  return renderSummaryRows(
    summary,
    layer,
    numberFormats,
    teamDps,
    teamDamage,
    selectedActor,
    snapshot,
    presentation,
    viewControls,
    options,
    scale,
  );
}

function renderSummaryRows(
  summary: HTMLElement,
  layer: OverlayLayer,
  numberFormats: OverlayNumberFormats,
  teamDps: number,
  teamDamage: number,
  selectedActor: OverlayActor | undefined,
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  viewControls: HTMLElement,
  options: RenderOptions,
  scale: number,
): HTMLElement {
  const rows = summaryLayoutRows(layer);
  const renderRow = (layoutItems: readonly string[], rowIndex: number): HTMLElement | null => {
    const row = el("div", "combat-overlay-summary-row");
    row.dataset.summaryRow = String(rowIndex);
    row.title = options.mode === "runtime"
      ? "Drag this summary row to move the overlay"
      : "Drag summary items within this row or into another row.";
    if (options.mode === "runtime") {
      row.addEventListener("pointerdown", (event) => {
        if (event.button !== 0) return;
        const target = event.target as HTMLElement;
        if (target.closest("button,input,select,a,[data-no-window-drag]")) return;
        event.preventDefault();
        options.onStartWindowDrag?.();
      });
    }
    const items = el("div", "combat-overlay-summary-items");
    for (const layoutKey of layoutItems) {
      const field = summaryLayoutField(layoutKey);
      const buttonId = summaryLayoutButtonId(layoutKey);
      const control = buttonId === null
        ? undefined
        : layer.buttons.find((candidate) => candidate.id === buttonId);
      const item = field === null
        ? control === undefined
          ? null
          : renderSummaryControl(
              control,
              layer,
              snapshot,
              presentation,
              options,
            )
        : field === "boss_health"
        ? renderBossSummaryItem(presentation, layer.showBossDps, numberFormats)
        : overlaySummaryStat(
          field === "team_dps" ? `Team ${metricLabel(layer.metric)}` : summaryFieldLabel(field),
          summaryFieldValue(
            field,
            teamDps,
            teamDamage,
            selectedActor,
            snapshot,
            presentation,
            numberFormats,
          ),
          layer.hiddenSummaryLabels.includes(field),
        );
      if (item === null) continue;
      item.dataset.summaryItem = layoutKey;
      if (field !== null) {
        item.dataset.summaryField = field;
        const fixedWidth = summaryFieldWidthFor(layer, field);
        if (fixedWidth > 0) {
          item.style.flex = `0 0 ${fixedWidth}px`;
          item.style.width = `${fixedWidth}px`;
        }
        item.addEventListener("contextmenu", (event) => {
          event.stopPropagation();
          options.onContextMenu?.(event, { kind: "summary_item", layerId: layer.id, field });
        });
      } else if (control !== undefined) {
        const fixedWidth = buttonWidthFor(control);
        item.dataset.buttonAction = control.action;
        if (fixedWidth > 0) {
          item.style.flex = `0 0 ${fixedWidth}px`;
          item.style.width = `${fixedWidth}px`;
          item.style.minWidth = `${fixedWidth}px`;
          item.style.maxWidth = `${fixedWidth}px`;
        }
      }
      if (options.mode === "preview") {
        item.classList.add("combat-overlay-reorder-target", "combat-overlay-summary-draggable");
        const grip = text("span", "⋮⋮", "combat-overlay-reorder-grip");
        grip.title = `Drag ${field === null ? control?.label ?? "control" : summaryFieldLabel(field)} within this row or into another row`;
        grip.setAttribute("aria-hidden", "true");
        item.prepend(grip);
        wireSummaryPointerReorder(
          field === null ? grip : item,
          layer.id,
          layoutKey,
          (targetRow, target, placement) => options.onReorderSummary?.(
            layer.id,
            layoutKey,
            targetRow,
            target,
            placement,
          ),
        );
        if (field !== null) {
          const resize = text("span", "", "combat-overlay-summary-resize");
          resize.dataset.width = `${summaryFieldWidthFor(layer, field)} px`;
          resize.title = `Drag to resize ${summaryFieldLabel(field)}. Set its width to 0 for automatic sizing.`;
          resize.dataset.noWindowDrag = "true";
          wireSummaryResize(
            resize,
            item,
            layer,
            field,
            scale,
            (width) => options.onResizeSummary?.(layer.id, field, width),
          );
          item.append(resize);
        } else if (control !== undefined && control.action !== "cycle_timer") {
          const resize = text("span", "", "combat-overlay-summary-resize");
          resize.dataset.width = `${buttonWidthFor(control)} px`;
          resize.title = "Drag to resize this control. Set its width to 0 in the inspector for automatic sizing.";
          resize.dataset.noWindowDrag = "true";
          wireButtonResize(
            resize,
            item,
            control,
            scale,
            (width) => options.onResizeButton?.(layer.id, control.id, width),
          );
          item.append(resize);
        }
      }
      items.append(item);
    }
    if (items.childElementCount === 0 && rowIndex !== 0 && options.mode === "runtime") return null;
    row.append(items);
    if (rowIndex === 0 && viewControls.childElementCount > 0) row.append(viewControls);
    return row;
  };
  const visibleBossCount = Math.min(2, presentation?.bosses.length ?? 0);
  for (let rowIndex = 0; rowIndex < rows.length; rowIndex += 1) {
    const fields = rows[rowIndex]!;
    const nextFields = rows[rowIndex + 1];
    const canMergeBossRows =
      visibleBossCount > 0 &&
      !fields.includes(summaryLayoutFieldKey("boss_health")) &&
      fields.length === 2 &&
      fields.includes(summaryLayoutFieldKey("team_dps")) &&
      fields.includes(summaryLayoutFieldKey("team_damage")) &&
      nextFields?.length === 1 &&
      nextFields[0] === summaryLayoutFieldKey("boss_health");
    if (canMergeBossRows) {
      const metricRow = renderRow(fields, rowIndex);
      const bossRow = renderRow(nextFields, rowIndex + 1);
      if (metricRow !== null && bossRow !== null) {
        const grid = el("div", "combat-overlay-summary-boss-grid");
        grid.dataset.bossRows = String(visibleBossCount);
        metricRow.classList.add("combat-overlay-summary-boss-metrics");
        bossRow.classList.add("combat-overlay-summary-boss-cell");
        grid.append(metricRow, bossRow);
        summary.append(grid);
      } else {
        if (metricRow !== null) summary.append(metricRow);
        if (bossRow !== null) summary.append(bossRow);
      }
      rowIndex += 1;
      continue;
    }
    const row = renderRow(fields, rowIndex);
    if (row !== null) summary.append(row);
  }
  if (options.mode === "preview" && rows.length < 8) {
    const dropRow = text(
      "div",
      "Drop here to create another summary row",
      "combat-overlay-summary-row-drop",
    );
    dropRow.dataset.summaryRow = String(rows.length);
    summary.append(dropRow);
  }
  return summary;
}

function renderSummaryControl(
  control: OverlayButton,
  layer: OverlayLayer,
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  options: RenderOptions,
): HTMLButtonElement {
  const controlButton = button(
    control.action === "cycle_metric" ? metricLabel(layer.metric) : runtimeControlLabel(
      control,
      layer.id,
      snapshot,
      presentation,
      options.selectedTimerByLayer,
      options.selectedSegmentByLayer,
    ),
    "combat-overlay-control combat-overlay-summary-control",
  );
  controlButton.dataset.buttonId = control.id;
  controlButton.dataset.buttonAction = control.action;
  controlButton.title = actionLabel(control.action);
  controlButton.addEventListener("click", (event) => {
    event.stopPropagation();
    options.onRuntimeAction?.(layer.id, control.action);
  });
  controlButton.addEventListener("contextmenu", (event) => {
    event.stopPropagation();
    options.onContextMenu?.(event, {
      kind: "button",
      layerId: layer.id,
      buttonId: control.id,
    });
  });
  return controlButton;
}

function renderBossSummaryItem(
  presentation: OverlayEncounterPresentation | null | undefined,
  showBossDps: boolean,
  numberFormats: OverlayNumberFormats,
): HTMLElement | null {
  const bosses = (presentation?.bosses ?? []).slice(0, 2);
  if (bosses.length === 0) return null;
  const bossList = el("div", "combat-overlay-boss-list");
  bossList.dataset.bossCount = String(bosses.length);
  for (const boss of bosses) {
    const current = Math.max(0, boss.current_hp);
    const maximum = Math.max(1, boss.max_hp);
    const percent = clamp(current / maximum * 100, 0, 100);
    const row = el("div", "combat-overlay-boss-row");
    row.style.setProperty("--boss-hp", `${percent}%`);
    row.title = `${boss.name}: ${NUMBER_FORMAT.format(current)} / ${NUMBER_FORMAT.format(maximum)} HP`;
    row.append(text(
      "strong",
      `${boss.name} [${formatOverlayPercent(percent, numberFormats.percentages)}] - ${formatOverlayNumber(current, numberFormats.bossHealth)} / ${formatOverlayNumber(maximum, numberFormats.bossHealth)}`,
      "combat-overlay-boss-primary",
    ));
    if (showBossDps) {
      const metrics = el("span", "combat-overlay-boss-team-metrics");
      const bdps = text("span", formatOverlayNumber(Math.max(0, boss.bdps), numberFormats.bossMetrics));
      bdps.dataset.metric = "bdps";
      const damage = text("span", formatOverlayNumber(Math.max(0, boss.team_damage), numberFormats.bossMetrics));
      damage.dataset.metric = "damage";
      metrics.title = `Team bDPS ${NUMBER_FORMAT.format(Math.max(0, boss.bdps))}; Team DMG ${NUMBER_FORMAT.format(Math.max(0, boss.team_damage))}`;
      metrics.setAttribute("aria-label", metrics.title);
      metrics.append(bdps, damage);
      row.append(metrics);
    }
    row.dataset.showBdps = String(showBossDps);
    bossList.append(row);
  }
  return bossList;
}

function summaryFieldValue(
  field: OverlaySummaryField,
  teamDps: number,
  teamDamage: number,
  selectedActor: OverlayActor | undefined,
  snapshot: OverlaySnapshot | null | undefined,
  presentation: OverlayEncounterPresentation | null | undefined,
  numberFormats: OverlayNumberFormats,
): string {
  switch (field) {
    case "attempt_time": return formatOptionalOverlayTime(snapshot?.attempt_elapsed_micros);
    case "encounter_time": return formatOptionalOverlayTime(
      snapshot?.encounter_elapsed_micros ?? snapshot?.active_combat_micros,
    );
    case "run_time": return formatOptionalOverlayTime(snapshot?.run_elapsed_micros);
    case "game_time": return formatOptionalOverlayTime(snapshot?.game_time_micros);
    case "true_time": return formatOptionalOverlayTime(snapshot?.true_time_micros);
    case "scene": return selectedActor === undefined
      ? overlaySceneName(presentation, snapshot)
      : `${actorName(selectedActor)} skills`;
    case "team_dps": return formatOverlayNumber(teamDps, numberFormats.summaryTotals);
    case "team_damage": return formatOverlayNumber(teamDamage, numberFormats.summaryTotals);
    case "boss_health": return "";
  }
}

function overlaySummaryStat(label: string, value: string, labelHidden = false): HTMLElement {
  const stat = el("span", "combat-overlay-summary-stat");
  stat.classList.toggle("label-hidden", labelHidden);
  stat.title = labelHidden ? `${label}: ${value}` : "";
  stat.setAttribute("aria-label", `${label}: ${value}`);
  stat.append(text("small", label), text("strong", value));
  return stat;
}

export function overlaySceneName(
  presentation: OverlayEncounterPresentation | null | undefined,
  snapshot: OverlaySnapshot | null | undefined,
): string {
  if (presentation?.scene_name?.trim()) return presentation.scene_name.trim();
  const sceneId = presentation?.scene_id ?? snapshot?.scene_id;
  return sceneId === null || sceneId === undefined ? "Waiting for scene" : `Scene ${sceneId}`;
}

function formatOverlayTime(micros: number): string {
  const totalSeconds = Math.max(0, Math.floor(micros / 1_000_000));
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor(totalSeconds % 3_600 / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatOptionalOverlayTime(micros: number | null | undefined): string {
  return micros === null || micros === undefined ? "—" : formatOverlayTime(micros);
}

function trimFixed(value: string): string {
  return value.replace(/(\.\d*?[1-9])0+$|\.0+$/, "$1");
}

export function formatOverlayNumber(value: number, format: OverlayNumberFormat): string {
  if (!Number.isFinite(value)) return "—";
  if (format === "full") return NUMBER_FORMAT.format(value);
  const absolute = Math.abs(value);
  const digits = format === "compact" ? 0 : 2;
  const shortened = (divisor: number, suffix: string) =>
    `${trimFixed((value / divisor).toFixed(digits))}${suffix}`;
  if (absolute >= 1_000_000_000) return shortened(1_000_000_000, "B");
  if (absolute >= 1_000_000) return shortened(1_000_000, "M");
  if (absolute >= 1_000) return shortened(1_000, "K");
  return NUMBER_FORMAT.format(value);
}

export function formatOverlayPercent(value: number, format: OverlayNumberFormat): string {
  if (!Number.isFinite(value)) return "—";
  const digits = format === "compact" ? 0 : format === "detailed" ? 1 : 2;
  return `${trimFixed(Math.max(0, value).toFixed(digits))}%`;
}

function renderAbilityBreakdown(
  layerElement: HTMLElement,
  layer: OverlayLayer,
  actor: OverlayActor,
  summary: HTMLElement,
  numberFormats: OverlayNumberFormats,
): void {
  const valueLabel = layer.metric === "hps"
    ? "Healing"
    : layer.metric === "tps"
      ? "Shielding"
      : layer.metric === "rdps"
        ? "rDMG gained"
        : "Damage";
  const abilities = [...(actor.abilities ?? [])]
    .filter((ability) => abilityMetricValue(ability, layer.metric) > 0 || ability.casts > 0 || ability.hits > 0)
    .sort((left, right) => abilityMetricValue(right, layer.metric) - abilityMetricValue(left, layer.metric))
    .slice(0, 12);
  const maximum = Math.max(1, ...abilities.map((ability) => abilityMetricValue(ability, layer.metric)));
  const rdpsDetail = layer.metric === "rdps";
  const gridClass = rdpsDetail
    ? "combat-overlay-row combat-overlay-header-row combat-overlay-ability-grid combat-overlay-rdps-ability-grid"
    : "combat-overlay-row combat-overlay-header-row combat-overlay-ability-grid";
  const header = el("div", gridClass);
  header.append(text("span", "Ability"), text("span", rdpsDetail ? "rDMG R/G" : valueLabel));
  if (rdpsDetail) header.append(text("span", "rDPS R/G"));
  header.append(text("span", "Hits"));
  const rows = el("div", "combat-overlay-rows combat-overlay-ability-rows");
  for (const ability of abilities) {
    const row = el(
      "div",
      rdpsDetail
        ? "combat-overlay-row combat-overlay-actor-row combat-overlay-ability-grid combat-overlay-rdps-ability-grid"
        : "combat-overlay-row combat-overlay-actor-row combat-overlay-ability-grid",
    );
    const value = abilityMetricValue(ability, layer.metric);
    row.style.setProperty("--meter-fill", `${Math.max(0, value / maximum) * 100}%`);
    const identity = el("span", "combat-overlay-ability-name");
    if (ability.icon_asset_path?.trim()) {
      const icon = document.createElement("img");
      icon.src = ability.icon_asset_path;
      icon.alt = "";
      identity.append(icon);
    }
    const labels = el("span");
    labels.append(
      text("strong", ability.presentation_name?.trim() || "Unlocalized combat action"),
      text(
        "small",
        ability.rdps_support_effect
          ? "Support contribution"
          : "Observed combat action",
      ),
    );
    identity.append(labels);
    row.append(identity);
    if (rdpsDetail) {
      const sources = ability.rdps_sources ?? [];
      const grants = ability.rdps_grants ?? [];
      const receivedTooltip = sources.map((source) => {
        const component = source.attribution_component === null
          ? "complete effect"
          : humanizeOverlayAttributionComponent(source.attribution_component);
        return `Received from ${source.provider_name} → ${source.effect_name} · ${component}: ${formatDecimalAmount(source.attributed_rdps)} rDMG / ${formatOverlayNumber(source.rdps, numberFormats.skillValues)} rDPS · ${source.damage_event_count} events`;
      });
      const givenTooltip = grants.map((grant) => {
        const component = grant.attribution_component === null
          ? "complete effect"
          : humanizeOverlayAttributionComponent(grant.attribution_component);
        return `Given via ${grant.effect_name} · ${component}: ${formatDecimalAmount(grant.attributed_rdps)} rDMG / ${formatOverlayNumber(grant.rdps, numberFormats.skillValues)} rDPS · ${grant.damage_event_count} events`;
      });
      const tooltip = [...receivedTooltip, ...givenTooltip].join("\n");
      const rdmg = text(
        "span",
        formatReceivedGivenAmounts(
          ability.rdps_received_damage ?? "0",
          ability.rdps_given_damage ?? "0",
          formatDecimalAmount,
        ),
        "combat-overlay-rdps-detail-value",
      );
      const rdps = text(
        "span",
        formatReceivedGivenRates(
          ability.rdps_received_rate ?? 0,
          ability.rdps_given_rate ?? 0,
          numberFormats.skillValues,
        ),
        "combat-overlay-rdps-detail-value",
      );
      if (tooltip) {
        rdmg.title = tooltip;
        rdps.title = tooltip;
      }
      row.append(rdmg, rdps);
    } else {
      row.append(text("span", formatOverlayNumber(value, numberFormats.skillValues)));
    }
    row.append(text("span", formatOverlayNumber(ability.hits, numberFormats.counts)));
    rows.append(row);
    if (rdpsDetail && ((ability.rdps_sources?.length ?? 0) + (ability.rdps_grants?.length ?? 0) > 0)) {
      const details = document.createElement("details");
      details.className = "combat-overlay-rdps-sources";
      const detailsSummary = document.createElement("summary");
      const detailCount = (ability.rdps_sources?.length ?? 0) + (ability.rdps_grants?.length ?? 0);
      detailsSummary.textContent = `${detailCount} contribution source${detailCount === 1 ? "" : "s"}`;
      details.append(detailsSummary);
      for (const source of ability.rdps_sources!) {
        const component = source.attribution_component === null
          ? "complete effect"
          : humanizeOverlayAttributionComponent(source.attribution_component);
        details.append(text(
          "div",
          `Received from ${source.provider_name} → ${source.effect_name} · ${component} · ${formatDecimalAmount(source.attributed_rdps)} rDMG · ${formatOverlayNumber(source.rdps, numberFormats.skillValues)} rDPS · ${source.damage_event_count} events`,
        ));
      }
      for (const grant of ability.rdps_grants ?? []) {
        const component = grant.attribution_component === null
          ? "complete effect"
          : humanizeOverlayAttributionComponent(grant.attribution_component);
        details.append(text(
          "div",
          `Given via ${grant.effect_name} · ${component} · ${formatDecimalAmount(grant.attributed_rdps)} rDMG · ${formatOverlayNumber(grant.rdps, numberFormats.skillValues)} rDPS · ${grant.damage_event_count} events`,
        ));
      }
      rows.append(details);
    }
  }
  if (abilities.length === 0) {
    rows.append(text("p", "No ability activity is available yet.", "combat-overlay-empty"));
  }
  if (layer.metric === "rdps") {
    rows.append(text(
      "p",
      actor.rdps_skill_detail_truncated
        ? "Live skill-source detail reached its safety cap. Actor and party rDPS totals remain exact; omitted rational detail is unavailable."
        : "R/G means received/given. Received rDMG is grouped by the affected skill; outgoing credit uses a proven provider skill when available, otherwise an exact support-effect row.",
      "combat-overlay-breakdown-note",
    ));
  } else if (layer.metric === "tps") {
    rows.append(text(
      "p",
      "Incoming damage has no owned-skill source; these rows show observed shielding.",
      "combat-overlay-breakdown-note",
    ));
  }
  layerElement.append(summary, header, rows);
}

function sampleAbilities(multiplier: number): readonly OverlayAbility[] {
  return [
    ["2233", "Powerdraw", 1_842_330_100, 214, 27],
    ["2203291", "Falcon Strike", 1_206_770_400, 488, 0],
    ["2352", "Celestial Eagle", 734_881_200, 326, 9],
    ["2203521", "Steel Beak", 416_290_500, 181, 0],
    ["55240", "Radiance Barrage", 202_711_100, 94, 0],
  ].map(([abilityId, name, damage, hits, casts]) => ({
    ability_id: String(abilityId),
    presentation_name: String(name),
    icon_asset_path: null,
    casts: Math.round(Number(casts) * multiplier),
    hits: Math.max(1, Math.round(Number(hits) * multiplier)),
    critical_hits: 0,
    reported_damage: Math.round(Number(damage) * multiplier),
    effective_damage: Math.round(Number(damage) * multiplier),
    reported_healing: 0,
    effective_healing: 0,
    shielding: 0,
  }));
}

function scaleOverlayActor(actor: OverlayActor, multiplier: number): OverlayActor {
  const scaleOptional = (value: number | undefined): number | undefined => (
    value === undefined ? undefined : value * multiplier
  );
  const scaleNullable = (value: number | null | undefined): number | null | undefined => (
    value == null ? value : value * multiplier
  );
  return {
    ...actor,
    reported_damage: scaleOptional(actor.reported_damage),
    effective_damage: scaleOptional(actor.effective_damage),
    hp_damage: scaleOptional(actor.hp_damage),
    shield_damage: scaleOptional(actor.shield_damage),
    damage_taken: scaleOptional(actor.damage_taken),
    dps: actor.dps * multiplier,
    edps: scaleNullable(actor.edps),
    adps: scaleNullable(actor.adps),
    bdps: scaleNullable(actor.bdps),
    rdps_damage: scaleNullable(actor.rdps_damage),
    rdps: actor.rdps == null ? actor.rdps : actor.rdps * multiplier,
    rdps_contribution_given: scaleNullable(actor.rdps_contribution_given),
    rdps_contribution_received: scaleNullable(actor.rdps_contribution_received),
    reported_healing: scaleOptional(actor.reported_healing),
    effective_healing: scaleOptional(actor.effective_healing),
    overheal: scaleOptional(actor.overheal),
    shielding: scaleOptional(actor.shielding),
    hps: actor.hps * multiplier,
    tps: actor.tps * multiplier,
    abilities: actor.abilities?.map((ability) => ({
      ...ability,
      reported_damage: ability.reported_damage * multiplier,
      effective_damage: ability.effective_damage * multiplier,
      reported_healing: ability.reported_healing * multiplier,
      effective_healing: ability.effective_healing * multiplier,
      shielding: ability.shielding * multiplier,
    })),
  };
}

function samplePresentation(
  className: string,
  specializationName: string,
  weaponItemId: number,
  firstImagineAbilityId: number,
  firstImagineTier: number,
  secondImagineAbilityId: number,
  secondImagineTier: number,
): OverlayActorPresentation {
  const gameAssets = "/game-assets/blue-protocol-star-resonance/shared";
  const classSpec = sampleClassSpecPresentation(className, specializationName);
  const imagine = (slotId: number, abilityId: number, tier: number): OverlayBadgePresentation => ({
    slot_id: slotId,
    ability_id: abilityId,
    item_id: abilityId === 3948 ? 3000101 : abilityId === 3969 ? 3000121 : null,
    tier,
    level: null,
    level_min: null,
    level_max: null,
    badge_kind: null,
    label: abilityId === 3948 ? "Rorola" : abilityId === 3969 ? "Igoreus" : `Main Imagine ${slotId}`,
    icon_asset_path: abilityId === 3948
      ? `${gameAssets}/icons/imagines/battle/3000101-rorola.png`
      : abilityId === 3969
        ? `${gameAssets}/icons/imagines/battle/3000121-igoreus.png`
        : null,
  });
  return {
    character_id: null,
    class_id: classSpec?.classId ?? null,
    specialization_id: classSpec?.specializationId ?? null,
    class_name: className,
    specialization_name: specializationName,
    class_spec_icon_asset_path: classSpec
      ? `${gameAssets}/${classSpec.specializationIcon}`
      : null,
    role: className === "Verdant Oracle" || className === "Beat Performer"
      ? "healer"
      : className === "Shield Knight" || className === "Heavy Guardian"
        ? "tank"
        : "damage",
    accent: specializationName === "Smite" || specializationName === "Dissonance"
      ? "damage_glow"
      : null,
    weapon: sampleWeaponPresentation(weaponItemId, gameAssets),
    primary_imagines: [
      imagine(1, firstImagineAbilityId, firstImagineTier),
      imagine(2, secondImagineAbilityId, secondImagineTier),
    ],
  };
}

function sampleWeaponPresentation(
  itemId: number,
  gameAssets: string,
): OverlayBadgePresentation {
  const entries: Record<number, {
    label: string;
    icon: string;
    level: number;
    badgeKind: string;
  }> = {
    2000631: {
      label: "Ember - Gaze of the Far Sea",
      icon: "icons/weapons/items/ch_wp_rodri_06_01.png",
      level: 280,
      badgeKind: "ember_far_sea",
    },
    2001503: {
      label: "Ragedream Axe",
      icon: "icons/weapons/items/ch_wp_tata02_01.png",
      level: 250,
      badgeKind: "weapon",
    },
    2001505: {
      label: "Voidforge Ring",
      icon: "icons/weapons/items/ch_wp_iruna_02_01.png",
      level: 250,
      badgeKind: "weapon",
    },
    2001508: {
      label: "Oath of the Immortal Watch",
      icon: "icons/weapons/items/ch_wp_farfara_02_01.png",
      level: 250,
      badgeKind: "weapon",
    },
    2001509: {
      label: "Voidcall Movement",
      icon: "icons/weapons/items/ch_wp_guitar_02_01.png",
      level: 250,
      badgeKind: "weapon",
    },
  };
  const entry = entries[itemId];
  return {
    slot_id: null,
    ability_id: null,
    item_id: itemId,
    tier: null,
    level: entry?.level ?? null,
    level_min: null,
    level_max: null,
    badge_kind: entry?.badgeKind ?? null,
    label: entry?.label ?? `Weapon item ${itemId}`,
    icon_asset_path: entry ? `${gameAssets}/${entry.icon}` : null,
  };
}

function sampleClassSpecPresentation(
  className: string,
  specializationName: string,
): {
  classId: number;
  specializationId: number;
  specializationIcon: string;
  weaponIcon: string;
} | null {
  const entries: Record<string, {
    classId: number;
    specializationId: number;
    specializationIcon: string;
    weaponIcon: string;
  }> = {
    "Marksman/Falconry": {
      classId: 11,
      specializationId: 117,
      specializationIcon: "icons/talents/shared/marksman-falconry-spec-1129-falconry-spec.png",
      weaponIcon: "icons/weapons/classes/marksman.png",
    },
    "Twin Striker/Formless": {
      classId: 3,
      specializationId: 128,
      specializationIcon: "icons/talents/shared/twin-striker-formless-spec-312-formless-expertise-spec.png",
      weaponIcon: "icons/weapons/classes/twin-striker.png",
    },
    "Verdant Oracle/Smite": {
      classId: 5,
      specializationId: 110,
      specializationIcon: "icons/talents/shared/verdant-oracle-smite-spec-510-smite-spec.png",
      weaponIcon: "icons/weapons/classes/verdant-oracle.png",
    },
    "Shield Knight/Shield": {
      classId: 12,
      specializationId: 123,
      specializationIcon: "icons/talents/shared/shield-knight-shield-spec-1218-shield-spec.png",
      weaponIcon: "icons/weapons/classes/shield-knight.png",
    },
    "Beat Performer/Concerto": {
      classId: 13,
      specializationId: 120,
      specializationIcon: "icons/talents/shared/beat-performer-concerto-spec-1317-concerto-spec.png",
      weaponIcon: "icons/weapons/classes/beat-performer.png",
    },
  };
  return entries[`${className}/${specializationName}`] ?? null;
}

function abilityMetricValue(ability: OverlayAbility, metric: OverlayMetric): number {
  if (metric === "hps") return Math.max(0, ability.effective_healing);
  if (metric === "tps") return Math.max(0, ability.shielding);
  if (metric === "rdps") {
    return Math.max(
      0,
      Number(ability.rdps_received_damage ?? "0") + Number(ability.rdps_given_damage ?? "0"),
    );
  }
  return Math.max(0, ability.reported_damage);
}

function formatReceivedGivenAmounts(
  received: string,
  given: string,
  formatter: (value: string) => string,
): string {
  const entries = [];
  if (BigInt(received) !== 0n) entries.push(`R ${formatter(received)}`);
  if (BigInt(given) !== 0n) entries.push(`G ${formatter(given)}`);
  return entries.join(" · ") || "—";
}

function formatReceivedGivenRates(
  received: number,
  given: number,
  format: OverlayNumberFormat,
): string {
  const entries = [];
  if (received !== 0) entries.push(`R ${formatOverlayNumber(received, format)}`);
  if (given !== 0) entries.push(`G ${formatOverlayNumber(given, format)}`);
  return entries.join(" · ") || "—";
}

function formatDecimalAmount(value: string): string {
  try {
    return BigInt(value).toLocaleString("en-US");
  } catch {
    return "—";
  }
}

export function actorName(actor: OverlayActor): string {
  const displayName = actor.display_name?.trim();
  if (displayName) return displayName;
  const characterId = actor.presentation?.character_id?.trim();
  if (characterId) return `UID ${characterId}`;
  return "Unidentified player";
}

export function humanizeOverlayAttributionComponent(component: string): string {
  const cleaned = component
    .replace(/\s*\((?:actions?\s*)?\d+(?:[\s/,]+\d+)*\)/giu, "")
    .replace(/\b(?:effect|action)\s+\d+(?:[\s/,]+\d+)*\b/giu, "")
    .replace(/[-_]+/gu, " ")
    .replace(/\s+/gu, " ")
    .trim();
  if (!cleaned) return "Complete effect";
  return cleaned.replace(/\b\w/gu, (letter) => letter.toUpperCase());
}

export function isOverlayRosterActor(actor: OverlayActor): boolean {
  if (actor.actor_kind === "player") return true;
  // Keep packet-confirmed allied NPC party members when they carry a combat
  // class identity, but never promote unresolved targets into the roster.
  return actor.actor_kind === "npc" && (
    actor.presentation?.class_id != null || actor.presentation?.specialization_id != null
  );
}

function overlayHeaderLabel(field: OverlayHeaderField, metric: OverlayMetric): string {
  if (field === "deaths") return "💀";
  if (field === "revives") return "😇";
  if (field === "percent") return `${metricLabel(metric)}%`;
  return fieldLabel(field);
}

function actorBarColor(actor: OverlayActor, settings: CombatOverlaySettings): string {
  const classId = actor.presentation?.class_id;
  const specializationId = actor.presentation?.specialization_id;
  const classKey = classId === null || classId === undefined ? null : `class:${classId}`;
  const specializationKey = specializationId === null || specializationId === undefined
    ? null
    : `specialization:${specializationId}`;
  const identity = settings.barColorMode === "class"
    ? classKey ?? `actor:${actor.actor_id}`
    : settings.barColorMode === "specialization"
      ? specializationKey ?? classKey ?? `actor:${actor.actor_id}`
      : `actor:${actor.actor_id}`;
  const override = settings.barColorOverrides[identity]
    ?? (settings.barColorMode === "specialization" && classKey !== null
      ? settings.barColorOverrides[classKey]
      : undefined);
  return override ?? automaticBarColor(identity);
}

function automaticBarColor(identity: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < identity.length; index += 1) {
    hash ^= identity.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return BAR_COLOR_PALETTE[(hash >>> 0) % BAR_COLOR_PALETTE.length] ?? "#63e5d6";
}

function renderBadgeCell(
  field: "class_spec" | "weapon" | "main_imagines",
  actor: OverlayActor,
): HTMLElement {
  const cell = el("span", `overlay-badge-cell overlay-field-${field}`);
  const presentation = actor.presentation;
  if (field === "class_spec") {
    if (!presentation || (presentation.class_id === null && presentation.specialization_id === null
      && !presentation.class_name && !presentation.specialization_name)) {
      cell.append(unknownBadge(`Class and specialization were not observed for ${actorName(actor)}.`));
      return cell;
    }
    const identity = [presentation.class_name, presentation.specialization_name]
      .filter((value): value is string => Boolean(value));
    cell.append(overlayRoleBadge(
      presentation.class_spec_icon_asset_path,
      "?",
      identity.join(" • "),
      presentation.role,
      presentation.accent,
    ));
    return cell;
  }
  if (field === "weapon") {
    const weapon = presentation?.weapon;
    if (!weapon) {
      cell.append(unknownBadge(`Weapon was not observed for ${actorName(actor)}.`));
      return cell;
    }
    cell.append(overlayBadge(
      weapon.icon_asset_path,
      "?",
      badgeTooltip(weapon, "Weapon"),
      weapon.tier,
      weapon.badge_kind,
      weapon.level === null
        ? weapon.level_min !== null && weapon.level_max !== null
          ? `${weapon.level_min}-${weapon.level_max}`
          : null
        : String(weapon.level),
    ));
    return cell;
  }
  const imagines = presentation?.primary_imagines ?? [];
  for (let index = 0; index < 2; index += 1) {
    const imagine = imagines[index];
    cell.append(imagine
      ? overlayBadge(
        imagine.icon_asset_path,
        "?",
        badgeTooltip(imagine, `Main Imagine ${index + 1}`),
        imagine.tier,
      )
      : unknownBadge(`Main Imagine slot ${index + 1} was not observed for ${actorName(actor)}.`));
  }
  return cell;
}

function overlayBadge(
  iconAssetPath: string | null,
  fallback: string,
  tooltip: string,
  tier: number | null,
  badgeKind: string | null = null,
  levelLabel: string | null = null,
): HTMLElement {
  const badge = el("span", "combat-overlay-badge");
  badge.title = tooltip;
  if (tier !== null) badge.dataset.tier = String(tier);
  if (badgeKind) badge.dataset.badgeKind = badgeKind;
  if (iconAssetPath?.trim()) {
    badge.dataset.state = "resolved";
    const icon = document.createElement("img");
    icon.src = iconAssetPath;
    icon.alt = "";
    badge.append(icon);
  } else {
    badge.dataset.state = "fallback";
    badge.textContent = fallback;
  }
  if (levelLabel) {
    const level = el("span", "combat-overlay-badge-level");
    level.textContent = levelLabel;
    badge.append(level);
  }
  return badge;
}

function overlayRoleBadge(
  iconAssetPath: string | null,
  fallback: string,
  tooltip: string,
  role: OverlayActorPresentation["role"],
  accent: OverlayActorPresentation["accent"],
): HTMLElement {
  const badge = el("span", "combat-overlay-badge combat-overlay-role-badge");
  badge.title = tooltip;
  if (role) badge.dataset.combatRole = role;
  if (accent) badge.dataset.combatAccent = accent;
  if (iconAssetPath?.trim()) {
    badge.dataset.state = "resolved";
    const icon = el("span", "combat-overlay-role-icon");
    const cssUrl = `url("${iconAssetPath}")`;
    icon.style.maskImage = cssUrl;
    icon.style.webkitMaskImage = cssUrl;
    badge.append(icon);
  } else {
    badge.dataset.state = "fallback";
    badge.textContent = fallback;
  }
  return badge;
}

function unknownBadge(tooltip: string): HTMLElement {
  return overlayBadge(null, "?", tooltip, null);
}

function badgeTooltip(badge: OverlayBadgePresentation, kind: string): string {
  return [
    badge.label || kind,
    badge.tier === null ? null : `Tier ${badge.tier}`,
    badge.level === null
      ? badge.level_min !== null && badge.level_max !== null
        ? `Lv. ${badge.level_min}-${badge.level_max}`
        : null
      : `Lv. ${badge.level}`,
  ].filter((value): value is string => Boolean(value)).join(" • ");
}

function resolvedOverlayHeight(
  canvas: HTMLElement,
  settings: CombatOverlaySettings,
): number {
  const scale = overlayScale(settings);
  if (!settings.dynamicHeight) return Math.round(settings.canvasHeight * scale);
  let contentBottom = 0;
  for (const layer of canvas.querySelectorAll<HTMLElement>(".combat-overlay-layer")) {
    contentBottom = Math.max(contentBottom, (layer.offsetTop + layer.scrollHeight) * scale);
  }
  return clamp(
    Math.ceil(contentBottom),
    Math.round(80 * scale),
    Math.round(1440 * scale),
  );
}

function wirePointerReorder(
  handle: HTMLElement,
  layerId: string,
  source: string,
  dataKey: "headerField" | "buttonId" | "viewId" | "summaryField",
  onReorder: (target: string, placement: ReorderPlacement) => void,
): void {
  const attribute = dataKey === "headerField"
    ? "data-header-field"
    : dataKey === "buttonId"
      ? "data-button-id"
      : dataKey === "summaryField"
        ? "data-summary-field"
        : "data-view-id";
  handle.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
  });
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const layerElement = handle.closest<HTMLElement>(".combat-overlay-layer");
    if (layerElement?.dataset.layerId !== layerId) return;
    const startX = event.clientX;
    const startY = event.clientY;
    let targetId: string | null = null;
    let targetElement: HTMLElement | null = null;
    let targetPlacement: ReorderPlacement = "before";
    let moved = false;
    handle.classList.add("is-dragging");
    const clearTarget = () => {
      targetElement?.classList.remove("is-reorder-target");
      targetElement?.removeAttribute("data-reorder-placement");
      targetElement = null;
      targetId = null;
    };
    const move = (next: PointerEvent) => {
      moved ||= Math.abs(next.clientX - startX) >= 3 || Math.abs(next.clientY - startY) >= 3;
      if (!moved) return;
      const candidate = document.elementFromPoint(next.clientX, next.clientY)?.closest<HTMLElement>(`[${attribute}]`) ?? null;
      const candidateId = candidate?.dataset[dataKey] ?? null;
      const sameLayer = candidate?.closest<HTMLElement>(".combat-overlay-layer")?.dataset.layerId === layerId;
      if (!sameLayer || candidateId === null || candidateId === source) {
        clearTarget();
        return;
      }
      const bounds = candidate.getBoundingClientRect();
      const placement: ReorderPlacement = next.clientX >= bounds.left + bounds.width / 2
        ? "after"
        : "before";
      if (candidate !== targetElement) {
        clearTarget();
        targetElement = candidate;
        targetId = candidateId;
        targetElement.classList.add("is-reorder-target");
      }
      targetPlacement = placement;
      targetElement.dataset.reorderPlacement = placement;
    };
    const stop = () => {
      handle.classList.remove("is-dragging");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      const commitTarget = targetId;
      const commitPlacement = targetPlacement;
      clearTarget();
      if (moved && commitTarget !== null) onReorder(commitTarget, commitPlacement);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
  });
}

function wireSummaryPointerReorder(
  dragSurface: HTMLElement,
  layerId: string,
  source: string,
  onReorder: (
    targetRow: number,
    target: string | null,
    placement: ReorderPlacement,
  ) => void,
): void {
  dragSurface.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
  });
  dragSurface.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const layerElement = dragSurface.closest<HTMLElement>(".combat-overlay-layer");
    if (layerElement?.dataset.layerId !== layerId) return;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    let moved = false;
    let targetRow: number | null = null;
    let targetField: string | null = null;
    let placement: ReorderPlacement = "after";
    let targetElement: HTMLElement | null = null;
    dragSurface.classList.add("is-dragging");
    try {
      dragSurface.setPointerCapture(pointerId);
    } catch {
      // Window listeners below remain as a fallback for older WebViews.
    }
    const clearTarget = () => {
      targetElement?.classList.remove("is-reorder-target");
      targetElement?.removeAttribute("data-reorder-placement");
      targetElement = null;
      targetRow = null;
      targetField = null;
    };
    const move = (next: PointerEvent) => {
      moved ||= Math.abs(next.clientX - startX) >= 3 || Math.abs(next.clientY - startY) >= 3;
      if (!moved) return;
      const rows = [...layerElement.querySelectorAll<HTMLElement>("[data-summary-row]")];
      const row = rows.find((candidate) => {
        const bounds = candidate.getBoundingClientRect();
        return next.clientY >= bounds.top && next.clientY <= bounds.bottom;
      }) ?? rows.reduce<HTMLElement | null>((nearest, candidate) => {
        if (nearest === null) return candidate;
        const bounds = candidate.getBoundingClientRect();
        const nearestBounds = nearest.getBoundingClientRect();
        const distance = Math.abs(next.clientY - (bounds.top + bounds.bottom) / 2);
        const nearestDistance = Math.abs(next.clientY - (nearestBounds.top + nearestBounds.bottom) / 2);
        return distance < nearestDistance ? candidate : nearest;
      }, null);
      const parsedRow = Number(row?.dataset.summaryRow);
      if (row === null || !Number.isInteger(parsedRow)) {
        clearTarget();
        return;
      }
      const candidates = [...row.querySelectorAll<HTMLElement>("[data-summary-item]")]
        .filter((candidate) => candidate.dataset.summaryItem !== source)
        .sort((left, right) => {
          const leftBounds = left.getBoundingClientRect();
          const rightBounds = right.getBoundingClientRect();
          return Math.abs(leftBounds.top - rightBounds.top) >= 4
            ? leftBounds.top - rightBounds.top
            : leftBounds.left - rightBounds.left;
        });
      let candidate: HTMLElement | null = null;
      let nextPlacement: ReorderPlacement = "after";
      const verticalLayout = candidates.some((candidate, index) => {
        if (index === 0) return false;
        const previousBounds = candidates[index - 1]!.getBoundingClientRect();
        const bounds = candidate.getBoundingClientRect();
        return Math.abs(bounds.top - previousBounds.top) >= 4;
      });
      if (verticalLayout) {
        for (const possibleTarget of candidates) {
          const bounds = possibleTarget.getBoundingClientRect();
          if (next.clientY < bounds.top + bounds.height / 2) {
            candidate = possibleTarget;
            nextPlacement = "before";
            break;
          }
        }
      } else {
        for (const possibleTarget of candidates) {
          const bounds = possibleTarget.getBoundingClientRect();
          if (next.clientX < bounds.left + bounds.width / 2) {
            candidate = possibleTarget;
            nextPlacement = "before";
            break;
          }
        }
      }
      if (candidate === null && candidates.length > 0) {
        candidate = candidates.at(-1) ?? null;
        nextPlacement = "after";
      }
      const nextField = candidate?.dataset.summaryItem;
      const nextTarget = candidate ?? row;
      if (nextTarget !== targetElement) {
        clearTarget();
        targetElement = nextTarget;
        nextTarget.classList.add("is-reorder-target");
      }
      targetRow = parsedRow;
      targetField = nextField ?? null;
      placement = nextPlacement;
      nextTarget.dataset.reorderPlacement = placement;
    };
    const stop = () => {
      dragSurface.classList.remove("is-dragging");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      try {
        if (dragSurface.hasPointerCapture(pointerId)) dragSurface.releasePointerCapture(pointerId);
      } catch {
        // The pointer may already have been released by the WebView.
      }
      const commitRow = targetRow;
      const commitField = targetField;
      const commitPlacement = placement;
      clearTarget();
      if (moved && commitRow !== null) onReorder(commitRow, commitField, commitPlacement);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
  });
}

function wireHeaderResize(
  handle: HTMLElement,
  layerElement: HTMLElement,
  layer: OverlayLayer,
  field: OverlayHeaderField,
  scale: number,
  onResize: (width: number) => void,
): void {
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = headerWidthFor(layer, field);
    let nextWidth = startWidth;
    let moved = false;
    handle.classList.add("is-resizing");
    const move = (next: PointerEvent) => {
      nextWidth = Math.round(clamp(startWidth + (next.clientX - startX) / scale, 0, 480));
      moved ||= Math.abs(next.clientX - startX) >= 2;
      handle.dataset.width = `${nextWidth} px`;
      const widths = { ...layer.headerWidths, [field]: nextWidth };
      const columns = gridColumns(layer.headerFields, widths);
      for (const row of layerElement.querySelectorAll<HTMLElement>(".combat-overlay-row")) {
        row.style.gridTemplateColumns = columns;
      }
    };
    const stop = () => {
      handle.classList.remove("is-resizing");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      if (moved) onResize(nextWidth);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
  });
}

function wireSummaryResize(
  handle: HTMLElement,
  item: HTMLElement,
  layer: OverlayLayer,
  field: OverlaySummaryField,
  scale: number,
  onResize: (width: number) => void,
): void {
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const configuredWidth = summaryFieldWidthFor(layer, field);
    const startWidth = configuredWidth > 0
      ? configuredWidth
      : Math.round(item.getBoundingClientRect().width / Math.max(scale, 0.01));
    let nextWidth = startWidth;
    let moved = false;
    handle.classList.add("is-resizing");
    const move = (next: PointerEvent) => {
      nextWidth = Math.round(clamp(startWidth + (next.clientX - startX) / scale, 32, 480));
      moved ||= Math.abs(next.clientX - startX) >= 2;
      handle.dataset.width = `${nextWidth} px`;
      item.style.flex = `0 0 ${nextWidth}px`;
      item.style.width = `${nextWidth}px`;
    };
    const stop = () => {
      handle.classList.remove("is-resizing");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      if (moved) onResize(nextWidth);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
  });
}

function wireButtonResize(
  handle: HTMLElement,
  item: HTMLElement,
  control: OverlayButton,
  scale: number,
  onResize: (width: number) => void,
): void {
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const configuredWidth = buttonWidthFor(control);
    const startWidth = configuredWidth > 0
      ? configuredWidth
      : Math.round(item.getBoundingClientRect().width / Math.max(scale, 0.01));
    let nextWidth = startWidth;
    let moved = false;
    handle.classList.add("is-resizing");
    const move = (next: PointerEvent) => {
      const minimum = control.action === "cycle_timer" ? FIXED_TIMER_CONTROL_WIDTH : 32;
      nextWidth = Math.round(clamp(startWidth + (next.clientX - startX) / scale, minimum, 480));
      moved ||= Math.abs(next.clientX - startX) >= 2;
      handle.dataset.width = `${nextWidth} px`;
      item.style.flex = `0 0 ${nextWidth}px`;
      item.style.width = `${nextWidth}px`;
    };
    const stop = () => {
      handle.classList.remove("is-resizing");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      if (moved) onResize(nextWidth);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
  });
}

export function parseCombatOverlaySettings(value: unknown): CombatOverlaySettings {
  if (!isRecord(value) || value.schemaVersion !== 1 || !Array.isArray(value.layers)) {
    throw new Error("The native host returned invalid Combat Overlay settings.");
  }
  const normalizedLayers = value.layers.map(normalizeLayerValue) as OverlayLayer[];
  const settings = {
    ...value,
    barOpacityPercent: value.barOpacityPercent === undefined
      ? 25
      : value.barOpacityPercent,
    summaryOpacityPercent: value.summaryOpacityPercent === undefined
      ? 85
      : value.summaryOpacityPercent,
    barColorMode: value.barColorMode === undefined
      ? "random"
      : value.barColorMode,
    barColorOverrides: value.barColorOverrides === undefined
      ? {}
      : value.barColorOverrides,
    numberFormat: value.numberFormat === undefined
      ? "detailed"
      : value.numberFormat,
    numberFormats: normalizeNumberFormats(value.numberFormats),
    liveOverlayEnabled: value.liveOverlayEnabled === undefined
      ? false
      : value.liveOverlayEnabled,
    autoHideOutsideCombat: value.autoHideOutsideCombat === undefined
      ? false
      : value.autoHideOutsideCombat,
    autoHideDelaySeconds: value.autoHideDelaySeconds === undefined
      ? 5
      : value.autoHideDelaySeconds,
    refreshIntervalMillis: value.refreshIntervalMillis === undefined
      ? 250
      : value.refreshIntervalMillis,
    allowLiveResize: value.allowLiveResize === undefined
      ? true
      : value.allowLiveResize,
    showViewTabs: value.showViewTabs === undefined
      ? false
      : value.showViewTabs,
    layers: ensureMetricHeaderViews(normalizedLayers),
  } as unknown as CombatOverlaySettings;
  if (
    !Number.isInteger(settings.canvasWidth) ||
    !Number.isInteger(settings.canvasHeight) ||
    !Number.isInteger(settings.opacityPercent) ||
    !Number.isInteger(settings.barOpacityPercent) ||
    settings.barOpacityPercent < 0 ||
    settings.barOpacityPercent > 100 ||
    !Number.isInteger(settings.summaryOpacityPercent) ||
    settings.summaryOpacityPercent < 0 ||
    settings.summaryOpacityPercent > 100 ||
    !(settings.barColorMode === "random"
      || settings.barColorMode === "class"
      || settings.barColorMode === "specialization") ||
    !isBarColorOverrides(settings.barColorOverrides) ||
    !(settings.numberFormat === "compact"
      || settings.numberFormat === "detailed"
      || settings.numberFormat === "full") ||
    !isOverlayNumberFormats(settings.numberFormats) ||
    !(["transparent", "solid", "custom"] as const).includes(settings.backgroundMode) ||
    typeof settings.backgroundColor !== "string" ||
    !/^#[0-9a-f]{6}$/i.test(settings.backgroundColor) ||
    !Number.isInteger(settings.backgroundOpacityPercent) ||
    settings.backgroundOpacityPercent < 0 ||
    settings.backgroundOpacityPercent > 100 ||
    !(settings.customBackgroundRevision === null || (
      Number.isSafeInteger(settings.customBackgroundRevision)
      && settings.customBackgroundRevision >= 0
    )) ||
    (settings.backgroundMode === "custom" && settings.customBackgroundRevision === null) ||
    typeof settings.liveOverlayEnabled !== "boolean" ||
    typeof settings.alwaysOnTop !== "boolean" ||
    typeof settings.clickThrough !== "boolean" ||
    typeof settings.autoHideOutsideCombat !== "boolean" ||
    !Number.isInteger(settings.autoHideDelaySeconds) ||
    settings.autoHideDelaySeconds < 0 ||
    settings.autoHideDelaySeconds > 300 ||
    !Number.isInteger(settings.refreshIntervalMillis) ||
    settings.refreshIntervalMillis < 50 ||
    settings.refreshIntervalMillis > 2_000 ||
    typeof settings.dynamicHeight !== "boolean" ||
    typeof settings.allowLiveResize !== "boolean" ||
    typeof settings.showViewTabs !== "boolean" ||
    !Number.isInteger(settings.maxVisiblePlayers) ||
    settings.maxVisiblePlayers < 1 ||
    settings.maxVisiblePlayers > 20 ||
    !Number.isInteger(settings.scalePercent) ||
    settings.scalePercent < 50 ||
    settings.scalePercent > 200 ||
    settings.layers.length === 0 ||
    !settings.layers.every(isLayer)
  ) {
    throw new Error("The native host returned invalid Combat Overlay settings.");
  }
  return settings;
}

function normalizeNumberFormats(value: unknown): OverlayNumberFormats {
  if (!isRecord(value)) return { ...DEFAULT_NUMBER_FORMATS };
  return Object.fromEntries(
    (Object.keys(DEFAULT_NUMBER_FORMATS) as OverlayNumberFormatTarget[]).map((target) => {
      const candidate = value[target];
      return [target, isOverlayNumberFormat(candidate) ? candidate : DEFAULT_NUMBER_FORMATS[target]];
    }),
  ) as OverlayNumberFormats;
}

function isOverlayNumberFormat(value: unknown): value is OverlayNumberFormat {
  return value === "compact" || value === "detailed" || value === "full";
}

function isOverlayNumberFormats(value: unknown): value is OverlayNumberFormats {
  return isRecord(value) && (Object.keys(DEFAULT_NUMBER_FORMATS) as OverlayNumberFormatTarget[])
    .every((target) => isOverlayNumberFormat(value[target]));
}

function isLayer(value: unknown): value is OverlayLayer {
  return isRecord(value)
    && typeof value.id === "string"
    && typeof value.title === "string"
    && METRICS.includes(value.metric as OverlayMetric)
    && Number.isInteger(value.x)
    && Number.isInteger(value.y)
    && Number.isInteger(value.width)
    && Array.isArray(value.headerFields)
    && value.headerFields.every((field) => HEADER_FIELDS.includes(field as OverlayHeaderField))
    && isHeaderWidths(value.headerWidths)
    && Array.isArray(value.hiddenHeaderLabels)
    && value.hiddenHeaderLabels.every((field) => HEADER_FIELDS.includes(field as OverlayHeaderField))
    && Array.isArray(value.summaryFields)
    && value.summaryFields.length <= SUMMARY_FIELDS.length
    && value.summaryFields.every((field) => SUMMARY_FIELDS.includes(field as OverlaySummaryField))
    && new Set(value.summaryFields).size === value.summaryFields.length
    && isSummaryFieldWidths(value.summaryFieldWidths)
    && isSummaryFieldRows(value.summaryFieldRows, value.summaryFields as OverlaySummaryField[])
    && isSummaryItemLayout(value)
    && Array.isArray(value.hiddenSummaryLabels)
    && value.hiddenSummaryLabels.every((field) => (value.summaryFields as unknown[]).includes(field))
    && new Set(value.hiddenSummaryLabels).size === value.hiddenSummaryLabels.length
    && typeof value.showBossDps === "boolean"
    && Array.isArray(value.buttons)
    && value.buttons.every(isOverlayButtonValue);
}

function isSummaryItemLayout(value: Record<string, unknown>): boolean {
  if (!Array.isArray(value.summaryItemOrder) || !isRecord(value.summaryItemRows)) return false;
  const fields = value.summaryFields as OverlaySummaryField[];
  const buttons = value.buttons as OverlayButton[];
  const expected = new Set([
    ...fields.map(summaryLayoutFieldKey),
    ...buttons.map((button) => summaryLayoutButtonKey(button.id)),
  ]);
  const order = value.summaryItemOrder;
  return order.length === expected.size
    && new Set(order).size === order.length
    && order.every((key) => typeof key === "string" && expected.has(key))
    && Object.keys(value.summaryItemRows).length === expected.size
    && Object.entries(value.summaryItemRows).every(([key, row]) =>
      expected.has(key) && Number.isInteger(row) && Number(row) >= 0 && Number(row) < 8);
}

export function ensureMetricHeaderViews(layers: readonly OverlayLayer[]): OverlayLayer[] {
  let copied: OverlayLayer[] = layers.map((layer) => ({
    ...layer,
    headerFields: [...layer.headerFields],
    headerWidths: { ...layer.headerWidths },
    hiddenHeaderLabels: [...layer.hiddenHeaderLabels],
    summaryFields: [...layer.summaryFields],
    summaryFieldWidths: { ...layer.summaryFieldWidths },
    summaryFieldRows: { ...layer.summaryFieldRows },
    summaryItemOrder: [...layer.summaryItemOrder],
    summaryItemRows: { ...layer.summaryItemRows },
    hiddenSummaryLabels: [...layer.hiddenSummaryLabels],
    buttons: layer.buttons.map((button) => ({ ...button })),
  }));
  const cycleTemplate = copied
    .flatMap((layer) => layer.buttons)
    .find((button) => button.action === "cycle_metric");
  if (cycleTemplate === undefined) return copied;
  copied = copied.map((layer) => layer.buttons.some((button) => button.action === "cycle_metric")
    ? layer
    : withNormalizedSummaryLayout({
        ...layer,
        buttons: [...layer.buttons, {
          ...cycleTemplate,
          id: uniqueId("metric", layer.buttons.map((button) => button.id)),
          label: metricLabel(layer.metric),
        }],
      }));
  if (copied.length !== 1) {
    return copied;
  }
  const source = copied[0]!;
  const usedIds = [source.id];
  for (const preset of CYCLING_VIEW_PRESETS) {
    if (preset.metric === source.metric) continue;
    const id = uniqueId(`metric-${preset.metric}`, usedIds);
    usedIds.push(id);
    copied.push({
      ...source,
      id,
      title: preset.title,
      metric: preset.metric,
      headerFields: [...preset.fields],
      headerWidths: { ...source.headerWidths },
      hiddenHeaderLabels: source.hiddenHeaderLabels.filter((field) => preset.fields.includes(field)),
      summaryFields: [...source.summaryFields],
      summaryFieldWidths: { ...source.summaryFieldWidths },
      summaryFieldRows: { ...source.summaryFieldRows },
      summaryItemOrder: [...source.summaryItemOrder],
      summaryItemRows: { ...source.summaryItemRows },
      hiddenSummaryLabels: [...source.hiddenSummaryLabels],
      buttons: source.buttons.map((button) => button.action === "cycle_metric"
        ? { ...button, label: metricLabel(preset.metric) }
        : { ...button }),
    });
  }
  return copied;
}

export function nextOverlayHeaderViewId(
  layers: readonly OverlayLayer[],
  currentLayerId: string,
): string {
  if (layers.length === 0) return currentLayerId;
  const current = layers.findIndex((layer) => layer.id === currentLayerId);
  return layers[(current + 1 + layers.length) % layers.length]?.id ?? layers[0]!.id;
}

function copyLayerRuntimeSelections(
  sourceLayerId: string,
  targetLayerId: string,
  selectedTimers: Map<string, OverlaySummaryField>,
  selectedSegments: Map<string, string>,
): void {
  const timer = selectedTimers.get(sourceLayerId);
  if (timer !== undefined) selectedTimers.set(targetLayerId, timer);
  const segment = selectedSegments.get(sourceLayerId);
  if (segment !== undefined) selectedSegments.set(targetLayerId, segment);
}

function normalizeLayerValue(value: unknown): unknown {
  if (!isRecord(value)) return value;
  const metric = METRICS.includes(value.metric as OverlayMetric)
    ? value.metric as OverlayMetric
    : "dps";
  const rawFields = Array.isArray(value.headerFields) ? value.headerFields : [];
  const headerFields = [...new Set(rawFields.map((field) => field === "value" ? metric : field))];
  const rawHiddenLabels = Array.isArray(value.hiddenHeaderLabels) ? value.hiddenHeaderLabels : [];
  const hiddenHeaderLabels = [...new Set(rawHiddenLabels.map((field) => field === "value" ? metric : field))]
    .filter((field) => headerFields.includes(field));
  let summaryFields = Array.isArray(value.summaryFields)
    ? [...new Set(value.summaryFields.filter((field) => SUMMARY_FIELDS.includes(field as OverlaySummaryField)))]
    : [...DEFAULT_SUMMARY_FIELDS];
  const buttons = Array.isArray(value.buttons)
    ? value.buttons.filter(isOverlayButtonValue).map((button) => ({
        ...button,
        width: button.action === "cycle_timer"
          ? FIXED_TIMER_CONTROL_WIDTH
          : Number.isInteger(button.width)
            ? Number(button.width)
            : defaultButtonWidth(button.action),
      }))
    : [];
  const isLegacySummaryLayout = !Array.isArray(value.summaryItemOrder);
  const timerButton = buttons.find((button) => button.action === "cycle_timer");
  const legacyTimerRow = isRecord(value.summaryFieldRows)
    && Number.isInteger(value.summaryFieldRows.encounter_time)
    ? Number(value.summaryFieldRows.encounter_time)
    : 0;
  if (isLegacySummaryLayout && timerButton !== undefined) {
    summaryFields = summaryFields.filter((field) => field !== "encounter_time");
  }
  const hiddenSummaryLabels = Array.isArray(value.hiddenSummaryLabels)
    ? [...new Set(value.hiddenSummaryLabels.filter((field) => summaryFields.includes(field)))]
    : [];
  const summaryFieldRows = normalizedSummaryFieldRows(summaryFields as OverlaySummaryField[], value.summaryFieldRows);
  const layout = normalizedSummaryItemLayout(
    summaryFields as OverlaySummaryField[],
    summaryFieldRows,
    buttons,
    value.summaryItemOrder,
    value.summaryItemRows,
    timerButton === undefined ? null : { id: timerButton.id, row: legacyTimerRow },
  );
  return {
    ...value,
    headerFields,
    headerWidths: {
      ...DEFAULT_HEADER_WIDTHS,
      ...(isRecord(value.headerWidths) ? value.headerWidths : {}),
    },
    hiddenHeaderLabels,
    summaryFields,
    summaryFieldWidths: isRecord(value.summaryFieldWidths) ? value.summaryFieldWidths : {},
    summaryFieldRows,
    summaryItemOrder: layout.order,
    summaryItemRows: layout.rows,
    hiddenSummaryLabels,
    showBossDps: typeof value.showBossDps === "boolean" ? value.showBossDps : true,
    buttons,
  };
}

function isOverlayButtonValue(value: unknown): value is OverlayButton {
  return isRecord(value)
    && typeof value.id === "string"
    && typeof value.label === "string"
    && ACTIONS.includes(value.action as OverlayButtonAction)
    && (value.width === undefined || (
      Number.isInteger(value.width)
      && Number(value.width) >= 0
      && Number(value.width) <= 480
    ));
}

function isSummaryFieldWidths(
  value: unknown,
): value is Partial<Record<OverlaySummaryField, number>> {
  if (!isRecord(value)) return false;
  return Object.entries(value).every(([field, width]) =>
    SUMMARY_FIELDS.includes(field as OverlaySummaryField)
    && Number.isInteger(width)
    && Number(width) >= 0
    && Number(width) <= 480);
}

function isSummaryFieldRows(
  value: unknown,
  fields: readonly OverlaySummaryField[],
): value is Partial<Record<OverlaySummaryField, number>> {
  if (!isRecord(value)) return false;
  return Object.entries(value).every(([field, row]) =>
    fields.includes(field as OverlaySummaryField)
    && Number.isInteger(row)
    && Number(row) >= 0
    && Number(row) < 8);
}

function isHeaderWidths(value: unknown): value is Record<OverlayHeaderField, number> {
  return isRecord(value)
    && Object.keys(value).every((field) => HEADER_FIELDS.includes(field as OverlayHeaderField))
    && HEADER_FIELDS.every((field) => Number.isInteger(value[field])
      && Number(value[field]) >= 0
      && Number(value[field]) <= 480);
}

function isBarColorOverrides(value: unknown): value is Record<string, string> {
  if (!isRecord(value) || Object.keys(value).length > 64) return false;
  return Object.entries(value).every(([key, color]) =>
    /^(?:class|specialization):[1-9]\d*$/.test(key)
    && typeof color === "string"
    && /^#[0-9a-f]{6}$/i.test(color));
}

async function loadSettings(): Promise<CombatOverlaySettings> {
  return normalizeHeaderViewGeometry(
    parseCombatOverlaySettings(await apiJson<unknown>("/api/settings/combat-overlay")),
  );
}

async function loadGlobalTimerSettings(): Promise<OverlayGlobalTimerSettings> {
  const value = await apiJson<unknown>("/api/settings/core");
  if (
    !isRecord(value) ||
    typeof value.pauseOverlayTimersOutsideCombat !== "boolean" ||
    !Number.isInteger(value.overlayTimerInactivitySeconds) ||
    Number(value.overlayTimerInactivitySeconds) < 0 ||
    Number(value.overlayTimerInactivitySeconds) > 300
  ) {
    throw new Error("The local host returned invalid global overlay timer settings.");
  }
  return {
    pauseOverlayTimersOutsideCombat: value.pauseOverlayTimersOutsideCombat,
    overlayTimerInactivitySeconds: Number(value.overlayTimerInactivitySeconds),
  };
}

async function forceResetLiveCombat(): Promise<void> {
  await apiJson<unknown>("/api/runtime/live/combat/force-reset", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: "{}",
  });
}

async function loadBarColorIdentities(): Promise<readonly BarColorIdentity[]> {
  const catalog = await apiJson<BarColorIdentityCatalog>(
    "/api/settings/combat-overlay/bar-color-identities",
  );
  const identities = new Map<string, BarColorIdentity>();
  for (const value of catalog.classes) {
    const key = `class:${value.id}`;
    identities.set(key, { key, label: value.label, kind: "class" });
  }
  for (const value of catalog.specializations) {
    const key = `specialization:${value.id}`;
    identities.set(key, {
      key,
      label: value.label.replace(/\s+Spec$/i, "").trim(),
      kind: "specialization",
    });
  }
  const update = await apiJson<OverlayLiveUpdate>("/api/runtime/live/combat").catch(() => null);
  for (const presentation of Object.values(update?.actor_presentations ?? {})) {
    if (presentation.class_id !== null) {
      const key = `class:${presentation.class_id}`;
      identities.set(key, {
        key,
        label: presentation.class_name?.trim() || `Class ${presentation.class_id}`,
        kind: "class",
      });
    }
    if (presentation.specialization_id !== null) {
      const key = `specialization:${presentation.specialization_id}`;
      identities.set(key, {
        key,
        label: presentation.specialization_name?.replace(/\s+Spec$/i, "").trim()
          || `Specialization ${presentation.specialization_id}`,
        kind: "specialization",
      });
    }
  }
  return [...identities.values()].sort((left, right) =>
    left.kind.localeCompare(right.kind) || left.label.localeCompare(right.label));
}

async function saveSettings(settings: CombatOverlaySettings): Promise<CombatOverlaySettings> {
  return normalizeHeaderViewGeometry(parseCombatOverlaySettings(await apiJson<unknown>("/api/settings/combat-overlay", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(normalizeHeaderViewGeometry(settings)),
  })));
}

export function normalizeHeaderViewGeometry(settings: CombatOverlaySettings): CombatOverlaySettings {
  if (settings.layers.every((view) =>
    view.x === 0 && view.y === 0 && view.width === settings.canvasWidth)) {
    return settings;
  }
  return {
    ...settings,
    layers: settings.layers.map((view) => ({
      ...view,
      x: 0,
      y: 0,
      width: settings.canvasWidth,
    })),
  };
}

async function uploadBackground(file: File): Promise<number> {
  if (file.size > 8 * 1024 * 1024) {
    throw new Error("Custom overlay backgrounds must be 8 MiB or smaller.");
  }
  const response = await fetch("/api/settings/combat-overlay/background", {
    method: "POST",
    cache: "no-store",
    headers: { Accept: "application/json" },
    body: file,
  });
  const value: unknown = await response.json();
  if (!response.ok) {
    throw new Error(
      isRecord(value) && typeof value.error === "string"
        ? value.error
        : `HTTP ${response.status}`,
    );
  }
  if (
    !isRecord(value)
    || !Number.isSafeInteger(value.revision)
    || (value.revision as number) < 0
  ) {
    throw new Error("The native host returned an invalid background revision.");
  }
  return value.revision as number;
}

function applyOverlayBackground(
  overlay: HTMLElement,
  settings: CombatOverlaySettings,
): void {
  overlay.dataset.backgroundMode = settings.backgroundMode;
  overlay.style.setProperty(
    "--overlay-background-opacity",
    String(settings.backgroundOpacityPercent / 100),
  );
  overlay.style.setProperty(
    "--summary-opacity",
    String(settings.summaryOpacityPercent / 100),
  );
  overlay.style.setProperty("--overlay-background-color", settings.backgroundColor);
  overlay.style.setProperty(
    "--overlay-background-image",
    settings.customBackgroundRevision === null
      ? "none"
      : `url("/api/settings/combat-overlay/background?v=${settings.customBackgroundRevision}")`,
  );
}

export async function apiJson<T>(route: string, init?: RequestInit, timeoutMillis?: number): Promise<T> {
  const controller = timeoutMillis === undefined ? null : new AbortController();
  const timeout = controller === null
    ? null
    : globalThis.setTimeout(() => controller.abort(), timeoutMillis);
  try {
    const response = await fetch(route, {
      cache: "no-store",
      headers: { Accept: "application/json", ...init?.headers },
      ...init,
      ...(controller === null ? {} : { signal: controller.signal }),
    });
    const value: unknown = await response.json();
    if (!response.ok) {
      throw new Error(isRecord(value) && typeof value.error === "string" ? value.error : `HTTP ${response.status}`);
    }
    return value as T;
  } catch (error) {
    if (controller?.signal.aborted) {
      throw new Error(`Live overlay request timed out after ${timeoutMillis} ms`, { cause: error });
    }
    throw error;
  } finally {
    if (timeout !== null) globalThis.clearTimeout(timeout);
  }
}

function metricValue(actor: OverlayActor, metric: OverlayMetric): number {
  return metricNumber(actor, metric) ?? 0;
}

export interface OverlayRdpsAvailability {
  providerCreditEnabled: boolean;
  blockerCodes: readonly string[];
  message: string;
}

export function describeOverlayRdpsAvailability(
  status: string | null | undefined,
): OverlayRdpsAvailability {
  if (status === "ready" || status === "partial_packet_proven_rules") {
    return { providerCreditEnabled: true, blockerCodes: [], message: "rDPS is active." };
  }
  const blocked = status?.startsWith("formula_pack_blocked:") === true
    || status?.startsWith("formula_runtime_blocked:") === true;
  if (blocked) {
    const blockerCodes = rdpsStatusList(status ?? "", "blockers=");
    const labels = blockerCodes.map(overlayRdpsBlockerLabel);
    return {
      providerCreditEnabled: false,
      blockerCodes,
      message: labels.length === 0
        ? "rDPS unavailable: exact-build proof gates remain open. Ordinary damage remains active."
        : `rDPS unavailable: ${labels.join("; ")}. Ordinary damage remains active. Structurally absent remote-player casts are not required or inferred.`,
    };
  }
  if (status === "waiting_for_client_build") {
    return {
      providerCreditEnabled: false,
      blockerCodes: [],
      message: "rDPS unavailable: waiting for authoritative exact game-build identity. Ordinary damage remains active.",
    };
  }
  return {
    providerCreditEnabled: false,
    blockerCodes: [],
    message: "rDPS unavailable: no authoritative formula projection is active. Ordinary damage remains active.",
  };
}

export function maskUnavailableOverlayRdps(
  actors: readonly OverlayActor[],
  availability: OverlayRdpsAvailability,
): OverlayActor[] {
  if (availability.providerCreditEnabled) return [...actors];
  return actors.map((actor) => ({
    ...actor,
    rdps: null,
    rdps_damage: null,
    rdps_contribution_given: null,
    rdps_contribution_received: null,
  }));
}

function layerUsesRdps(layer: OverlayLayer): boolean {
  return layer.metric === "rdps" || layer.headerFields.some((field) =>
    field === "rdps" || field === "rdps_damage" ||
    field === "contribution_given" || field === "contribution_received");
}

function rdpsStatusList(status: string, prefix: string): string[] {
  const segment = status
    .split(";")
    .map((value) => value.trim())
    .find((value) => value.startsWith(prefix));
  if (segment === undefined) return [];
  return segment
    .slice(prefix.length)
    .split(",")
    .map((value) => value.trim())
    .filter((value, index, values) => value.length > 0 && values.indexOf(value) === index);
}

function overlayRdpsBlockerLabel(code: string): string {
  switch (code) {
    case "protocol-pack-identity": return "current-build protocol-pack identity is missing";
    case "canonical-replay-conservation": return "canonical replay conservation is unproven";
    case "protocol-event-coverage": return "protocol event coverage is unproven";
    case "critical-damage-factor-interpretation-authority":
      return "critical-damage interpretation, operation order, and rounding are unresolved";
    case "party-support-formula-frontier":
      return "party-skill and team-entry provider, recipient-scope, formula, stacking, rounding, and conservation proof is incomplete";
    case "historical-build-runtime-promotion-not-reviewed":
      return "historical-build runtime promotion is not reviewed";
    default: return `proof gate ${code} is unresolved`;
  }
}

function safeNonnegativeInteger(value: unknown): number | null {
  return Number.isSafeInteger(value) && Number(value) >= 0 ? Number(value) : null;
}

function metricNumber(actor: OverlayActor, metric: OverlayMetric): number | null {
  if (metric === "edps") return actor.edps ?? actor.dps;
  if (metric === "adps") return actor.adps ?? actor.edps ?? actor.dps;
  if (metric === "bdps") return actor.bdps ?? null;
  if (metric === "rdps") return actor.rdps ?? null;
  return actor[metric];
}

function fieldValue(
  field: OverlayHeaderField,
  actor: OverlayActor,
  index: number,
  metric: OverlayMetric,
  maximum: number,
  numberFormats: OverlayNumberFormats,
): string {
  const metricFormat = numberFormats.playerMetrics;
  const countFormat = numberFormats.counts;
  switch (field) {
    case "rank": return String(index + 1);
    case "class_spec": return "";
    case "name": return actorName(actor);
    case "weapon": return "";
    case "main_imagines": return "";
    case "damage": return optionalNumberText(actor.reported_damage, metricFormat);
    case "effective_damage": return optionalNumberText(actor.effective_damage, metricFormat);
    case "hp_damage": return optionalNumberText(actor.hp_damage, metricFormat);
    case "shield_damage": return optionalNumberText(actor.shield_damage, metricFormat);
    case "dps": return metricText(actor, "dps", metricFormat);
    case "edps": return metricText(actor, "edps", metricFormat);
    case "adps": return metricText(actor, "adps", metricFormat);
    case "bdps": return metricText(actor, "bdps", metricFormat);
    case "rdps": return metricText(actor, "rdps", metricFormat);
    case "hps": return metricText(actor, "hps", metricFormat);
    case "tps": return metricText(actor, "tps", metricFormat);
    case "healing": return optionalNumberText(actor.reported_healing, metricFormat);
    case "effective_healing": return optionalNumberText(actor.effective_healing, metricFormat);
    case "overheal": return optionalNumberText(actor.overheal, metricFormat);
    case "shielding": return optionalNumberText(actor.shielding, metricFormat);
    case "damage_taken": return optionalNumberText(actor.damage_taken, metricFormat);
    case "hits": return optionalNumberText(actor.hits, countFormat);
    case "critical_rate": return actor.hits && actor.critical_hits !== undefined
      ? formatOverlayPercent((actor.critical_hits / actor.hits) * 100, numberFormats.percentages)
      : "—";
    case "casts": return optionalNumberText(actor.casts, countFormat);
    case "deaths": return optionalNumberText(actor.deaths, countFormat);
    case "revives": return optionalNumberText(actor.revives, countFormat);
    case "rdps_damage": return optionalNumberText(actor.rdps_damage, metricFormat);
    case "contribution_given": return optionalNumberText(actor.rdps_contribution_given, metricFormat);
    case "contribution_received": return optionalNumberText(actor.rdps_contribution_received, metricFormat);
    case "value": return metricText(actor, metric, metricFormat);
    case "percent": return formatOverlayPercent((metricValue(actor, metric) / maximum) * 100, numberFormats.percentages);
  }
}

function metricText(actor: OverlayActor, metric: OverlayMetric, numberFormat: OverlayNumberFormat): string {
  const value = metricNumber(actor, metric);
  return value === null ? "—" : formatOverlayNumber(value, numberFormat);
}

function optionalNumberText(
  value: number | null | undefined,
  numberFormat: OverlayNumberFormat,
): string {
  return value === null || value === undefined ? "—" : formatOverlayNumber(value, numberFormat);
}

function gridColumns(
  fields: readonly OverlayHeaderField[],
  widths: Readonly<Partial<Record<OverlayHeaderField, number>>>,
): string {
  return fields.map((field) => `${headerWidth(widths, field)}px`).join(" ");
}

function headerWidthFor(layer: OverlayLayer, field: OverlayHeaderField): number {
  return headerWidth(layer.headerWidths, field);
}

function summaryFieldWidthFor(layer: OverlayLayer, field: OverlaySummaryField): number {
  return Math.round(clamp(layer.summaryFieldWidths?.[field] ?? 0, 0, 480));
}

export function buttonWidthFor(button: Pick<OverlayButton, "action" | "width">): number {
  if (button.action === "cycle_timer") return FIXED_TIMER_CONTROL_WIDTH;
  return Math.round(clamp(button.width ?? defaultButtonWidth(button.action), 0, 480));
}

function defaultButtonWidth(action: OverlayButtonAction): number {
  return action === "cycle_timer" ? FIXED_TIMER_CONTROL_WIDTH : 0;
}

function headerWidth(
  widths: Readonly<Partial<Record<OverlayHeaderField, number>>>,
  field: OverlayHeaderField,
): number {
  return Math.round(clamp(widths[field] ?? DEFAULT_HEADER_WIDTHS[field], 0, 480));
}

function overlayScale(settings: CombatOverlaySettings): number {
  return clamp(settings.scalePercent, 50, 200) / 100;
}

function metricLabel(metric: OverlayMetric): string {
  return ({ dps: "DPS", edps: "eDPS", adps: "aDPS", bdps: "bDPS", rdps: "rDPS", hps: "HPS", tps: "TPS" } as const)[metric];
}

function fieldLabel(field: OverlayHeaderField): string {
  return ({
    rank: "#",
    class_spec: "Class",
    name: "Player",
    weapon: "Wpn",
    main_imagines: "Imagines",
    damage: "Damage",
    effective_damage: "Effective damage",
    hp_damage: "HP damage",
    shield_damage: "Shield damage",
    dps: "DPS",
    edps: "eDPS",
    adps: "aDPS",
    bdps: "bDPS",
    rdps: "rDPS",
    hps: "HPS",
    tps: "TPS",
    healing: "Healing",
    effective_healing: "Effective healing",
    overheal: "Overheal",
    shielding: "Shielding",
    damage_taken: "Damage taken",
    hits: "Hits",
    critical_rate: "Crit %",
    casts: "Casts",
    deaths: "Deaths",
    revives: "Revives",
    rdps_damage: "rDMG",
    contribution_given: "rDMG granted",
    contribution_received: "rDMG received",
    value: "Value",
    percent: "DMG%",
  } as const)[field];
}

function summaryFieldLabel(field: OverlaySummaryField): string {
  return ({
    attempt_time: "Attempt time",
    encounter_time: "Encounter time",
    run_time: "Run time",
    game_time: "Game time",
    true_time: "True time",
    scene: "Scene",
    team_dps: "Team DPS",
    team_damage: "Team damage",
    boss_health: "Boss health",
  } as const)[field];
}

function defaultSummaryRow(field: OverlaySummaryField): number {
  if (field === "team_dps" || field === "team_damage") return 1;
  if (field === "boss_health") return 2;
  return 0;
}

function defaultSummaryFieldRows(
  fields: readonly OverlaySummaryField[],
): Partial<Record<OverlaySummaryField, number>> {
  return Object.fromEntries(fields.map((field) => [field, defaultSummaryRow(field)]));
}

function summaryLayoutFieldKey(field: OverlaySummaryField): string {
  return `summary:${field}`;
}

function summaryLayoutButtonKey(buttonId: string): string {
  return `button:${buttonId}`;
}

function summaryLayoutField(key: string): OverlaySummaryField | null {
  if (!key.startsWith("summary:")) return null;
  const field = key.slice("summary:".length);
  return SUMMARY_FIELDS.includes(field as OverlaySummaryField)
    ? field as OverlaySummaryField
    : null;
}

function summaryLayoutButtonId(key: string): string | null {
  return key.startsWith("button:") ? key.slice("button:".length) || null : null;
}

function normalizedSummaryItemLayout(
  fields: readonly OverlaySummaryField[],
  fieldRows: Readonly<Partial<Record<OverlaySummaryField, number>>>,
  buttons: readonly OverlayButton[],
  rawOrder: unknown,
  rawRows: unknown,
  migratedTimer: { id: string; row: number } | null = null,
): { order: string[]; rows: Record<string, number> } {
  const expected = [
    ...fields.map(summaryLayoutFieldKey),
    ...buttons.map((button) => summaryLayoutButtonKey(button.id)),
  ];
  const expectedSet = new Set(expected);
  const candidateOrder = Array.isArray(rawOrder)
    ? rawOrder.filter((key): key is string => typeof key === "string")
    : [];
  const orderIsComplete = candidateOrder.length === expected.length
    && new Set(candidateOrder).size === candidateOrder.length
    && candidateOrder.every((key) => expectedSet.has(key));
  const timerKey = migratedTimer === null ? null : summaryLayoutButtonKey(migratedTimer.id);
  const order = orderIsComplete
    ? [...candidateOrder]
    : [
        ...(timerKey === null || !expectedSet.has(timerKey) ? [] : [timerKey]),
        ...fields.map(summaryLayoutFieldKey),
        ...buttons
          .map((button) => summaryLayoutButtonKey(button.id))
          .filter((key) => key !== timerKey),
      ];
  const candidateRows = isRecord(rawRows) ? rawRows : {};
  const rows = Object.fromEntries(order.map((key) => {
    const rawRow = candidateRows[key];
    if (Number.isInteger(rawRow) && Number(rawRow) >= 0 && Number(rawRow) < 8) {
      return [key, Number(rawRow)];
    }
    const field = summaryLayoutField(key);
    if (field !== null) return [key, fieldRows[field] ?? defaultSummaryRow(field)];
    if (key === timerKey) return [key, clamp(migratedTimer?.row ?? 0, 0, 7)];
    return [key, 0];
  }));
  return compactSummaryLayoutRows(order, rows);
}

function withNormalizedSummaryLayout(layer: OverlayLayer): OverlayLayer {
  const layout = normalizedSummaryItemLayout(
    layer.summaryFields,
    layer.summaryFieldRows,
    layer.buttons,
    layer.summaryItemOrder,
    layer.summaryItemRows,
  );
  return {
    ...layer,
    summaryItemOrder: layout.order,
    summaryItemRows: layout.rows,
  };
}

function compactSummaryLayoutRows(
  order: readonly string[],
  rows: Readonly<Record<string, number>>,
): { order: string[]; rows: Record<string, number> } {
  const occupied = [...new Set(order.map((key) => rows[key] ?? 0))]
    .sort((left, right) => left - right);
  const compact = new Map(occupied.map((row, index) => [row, index]));
  return {
    order: [...order],
    rows: Object.fromEntries(order.map((key) => [key, compact.get(rows[key] ?? 0) ?? 0])),
  };
}

function normalizedSummaryFieldRows(
  fields: readonly OverlaySummaryField[],
  value: unknown,
): Partial<Record<OverlaySummaryField, number>> {
  const raw = isRecord(value) ? value : {};
  const assigned = fields.map((field) => {
    const candidate = raw[field];
    return [field, Number.isInteger(candidate) && Number(candidate) >= 0 && Number(candidate) < 8
      ? Number(candidate)
      : defaultSummaryRow(field)] as const;
  });
  const occupiedRows = [...new Set(assigned.map(([, row]) => row))].sort((left, right) => left - right);
  const compactRows = new Map(occupiedRows.map((row, index) => [row, index]));
  return Object.fromEntries(assigned.map(([field, row]) => [field, compactRows.get(row) ?? 0]));
}

function summaryRows(layer: OverlayLayer): OverlaySummaryField[][] {
  const mapping = normalizedSummaryFieldRows(layer.summaryFields, layer.summaryFieldRows);
  const rows: OverlaySummaryField[][] = [];
  for (const field of layer.summaryFields) {
    const row = mapping[field] ?? 0;
    (rows[row] ??= []).push(field);
  }
  return rows.filter((fields) => fields.length > 0);
}

function summaryLayoutRows(layer: OverlayLayer): string[][] {
  const normalized = normalizedSummaryItemLayout(
    layer.summaryFields,
    layer.summaryFieldRows,
    layer.buttons,
    layer.summaryItemOrder,
    layer.summaryItemRows,
  );
  const rows: string[][] = [];
  for (const key of normalized.order) {
    const row = normalized.rows[key] ?? 0;
    (rows[row] ??= []).push(key);
  }
  return rows.filter((items) => items.length > 0);
}

export function moveSummaryLayoutItem(
  layer: OverlayLayer,
  source: string,
  targetRow: number,
  target: string | null,
  placement: ReorderPlacement,
): OverlayLayer {
  const rows = summaryLayoutRows(layer);
  if (!rows.some((row) => row.includes(source))) return layer;
  while (rows.length <= targetRow && rows.length < 8) rows.push([]);
  if (targetRow < 0 || targetRow >= rows.length) return layer;
  for (const row of rows) {
    const sourceIndex = row.indexOf(source);
    if (sourceIndex >= 0) row.splice(sourceIndex, 1);
  }
  const destination = rows[targetRow]!;
  const targetIndex = target === null ? -1 : destination.indexOf(target);
  destination.splice(
    targetIndex < 0 ? destination.length : targetIndex + (placement === "after" ? 1 : 0),
    0,
    source,
  );
  const compacted = rows.filter((row) => row.length > 0);
  const summaryItemOrder = compacted.flat();
  const summaryItemRows = Object.fromEntries(
    compacted.flatMap((row, rowIndex) => row.map((key) => [key, rowIndex])),
  );
  const summaryFieldRows = {
    ...layer.summaryFieldRows,
    ...Object.fromEntries(summaryItemOrder.flatMap((key) => {
      const field = summaryLayoutField(key);
      return field === null ? [] : [[field, summaryItemRows[key] ?? 0]];
    })),
  };
  return { ...layer, summaryItemOrder, summaryItemRows, summaryFieldRows };
}

export function moveSummaryField(
  layer: OverlayLayer,
  source: OverlaySummaryField,
  targetRow: number,
  target: OverlaySummaryField | null,
  placement: ReorderPlacement,
): OverlayLayer {
  if (!layer.summaryFields.includes(source)) return layer;
  const rows = summaryRows(layer);
  while (rows.length <= targetRow && rows.length < 8) rows.push([]);
  if (targetRow < 0 || targetRow >= rows.length) return layer;
  for (const row of rows) {
    const sourceIndex = row.indexOf(source);
    if (sourceIndex >= 0) row.splice(sourceIndex, 1);
  }
  const destination = rows[targetRow]!;
  const targetIndex = target === null ? -1 : destination.indexOf(target);
  const insertionIndex = targetIndex < 0
    ? destination.length
    : targetIndex + (placement === "after" ? 1 : 0);
  destination.splice(insertionIndex, 0, source);
  const compacted = rows.filter((row) => row.length > 0);
  const summaryFields = compacted.flat();
  const summaryFieldRows = Object.fromEntries(
    compacted.flatMap((row, rowIndex) => row.map((field) => [field, rowIndex])),
  );
  return { ...layer, summaryFields, summaryFieldRows };
}

function actionLabel(action: OverlayButtonAction): string {
  return ({
    cycle_metric: "Cycle metric",
    cycle_timer: "Cycle timer",
    cycle_segment: "Cycle segment",
    reset_encounter: "Reset encounter",
    toggle_visibility: "Hide overlay",
    open_history: "Open Combat History",
  } as const)[action];
}

function defaultButtonLabel(action: OverlayButtonAction): string {
  return ({
    cycle_metric: "Metric",
    cycle_timer: "Encounter",
    cycle_segment: "Entire run",
    reset_encounter: "Reset",
    toggle_visibility: "Hide",
    open_history: "History",
  } as const)[action];
}

export function moveRelative<T>(
  values: readonly T[],
  source: T,
  target: T,
  placement: ReorderPlacement,
): T[] {
  if (source === target) return [...values];
  const next = values.filter((value) => value !== source);
  const index = next.indexOf(target);
  const insertionIndex = index < 0
    ? next.length
    : index + (placement === "after" ? 1 : 0);
  next.splice(insertionIndex, 0, source);
  return next;
}

function insertHeaderField(
  fields: readonly OverlayHeaderField[],
  field: OverlayHeaderField,
): OverlayHeaderField[] {
  if (fields.includes(field)) return [...fields];
  const next = [...fields];
  const percentIndex = next.indexOf("percent");
  next.splice(percentIndex < 0 ? next.length : percentIndex, 0, field);
  return next;
}

function moveObjectRelative<T extends { id: string }>(
  values: readonly T[],
  source: string,
  target: string,
  placement: ReorderPlacement,
): T[] {
  if (source === target) return [...values];
  const item = values.find((value) => value.id === source);
  if (!item) return [...values];
  const next = values.filter((value) => value.id !== source);
  const index = next.findIndex((value) => value.id === target);
  const insertionIndex = index < 0
    ? next.length
    : index + (placement === "after" ? 1 : 0);
  next.splice(insertionIndex, 0, item);
  return next;
}

function uniqueId(prefix: string, existing: readonly string[]): string {
  let index = 1;
  let candidate = `${prefix}-${index}`;
  while (existing.includes(candidate)) candidate = `${prefix}-${++index}`;
  return candidate;
}

function closeContextMenu(): void {
  document.querySelector(".combat-overlay-context-menu")?.remove();
}

function buildContextMenu(entries: readonly ContextMenuEntry[], root = false): HTMLDivElement {
  const menu = el("div", root ? "combat-overlay-context-menu" : "combat-overlay-context-submenu");
  menu.setAttribute("role", "menu");
  for (const entry of entries) {
    if (entry.separatorBefore) menu.append(el("div", "combat-overlay-context-separator"));
    const wrapper = el("div", "combat-overlay-context-entry");
    const item = button(entry.label, entry.danger ? "danger" : "");
    item.setAttribute("role", "menuitem");
    item.disabled = entry.disabled ?? false;
    if (entry.children && entry.children.length > 0) {
      item.classList.add("has-submenu");
      item.setAttribute("aria-haspopup", "menu");
      const submenu = buildContextMenu(entry.children);
      wrapper.append(item, submenu);
      const reveal = () => {
        wrapper.parentElement?.querySelectorAll(":scope > .combat-overlay-context-entry.is-open")
          .forEach((candidate) => candidate !== wrapper && candidate.classList.remove("is-open"));
        wrapper.classList.add("is-open");
        submenu.classList.remove("opens-left", "opens-up");
        const bounds = submenu.getBoundingClientRect();
        if (bounds.right > window.innerWidth - 6) submenu.classList.add("opens-left");
        if (bounds.bottom > window.innerHeight - 6) submenu.classList.add("opens-up");
      };
      wrapper.addEventListener("pointerenter", reveal);
      item.addEventListener("focus", reveal);
      item.addEventListener("keydown", (event) => {
        if (event.key === "ArrowRight") {
          event.preventDefault();
          reveal();
          submenu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
        }
      });
    } else {
      item.addEventListener("click", () => {
        closeContextMenu();
        entry.action?.();
      });
      wrapper.append(item);
    }
    menu.append(wrapper);
  }
  return menu;
}

function keepMenuInViewport(menu: HTMLElement): void {
  const bounds = menu.getBoundingClientRect();
  const left = Math.max(6, Math.min(bounds.left, window.innerWidth - bounds.width - 6));
  const top = Math.max(6, Math.min(bounds.top, window.innerHeight - bounds.height - 6));
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
}

function inputField(labelText: string, value: string, type = "text"): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = el("label", "combat-overlay-field");
  const input = document.createElement("input");
  input.type = type;
  input.value = value;
  label.append(text("span", labelText), input);
  return { label, input };
}

function selectField<T extends string>(
  labelText: string,
  values: readonly (readonly [T, string])[],
  selected: T,
): { label: HTMLLabelElement; select: HTMLSelectElement } {
  const label = el("label", "combat-overlay-field");
  const select = document.createElement("select");
  for (const [value, copy] of values) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = copy;
    option.selected = value === selected;
    select.append(option);
  }
  label.append(text("span", labelText), select);
  return { label, select };
}

function checkbox(labelText: string, checked: boolean): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = el("label", "combat-overlay-checkbox");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  label.append(input, text("span", labelText));
  return { label, input };
}

function button(label: string, className = ""): HTMLButtonElement {
  const value = document.createElement("button");
  value.type = "button";
  value.className = className;
  value.textContent = label;
  return value;
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = value;
  return node;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, Number.isFinite(value) ? value : minimum));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function installStyles(): void {
  if (stylesInstalled || document.querySelector("#rlogs-combat-overlay-styles")) return;
  stylesInstalled = true;
  const style = document.createElement("style");
  style.id = "rlogs-combat-overlay-styles";
  style.textContent = `
    .combat-overlay-editor { display:grid; gap:13px; }
    .combat-overlay-editor-heading { position:sticky; z-index:50; top:0; display:flex; min-height:78px; align-items:center; justify-content:space-between; gap:18px; padding:14px 18px; background:color-mix(in srgb,var(--surface-raised) 94%,transparent); box-shadow:0 10px 28px rgb(0 0 0 / 28%); backdrop-filter:blur(22px) saturate(125%); }
    .combat-overlay-editor-heading h2 { margin:0; }
    .combat-overlay-editor-heading p, .combat-overlay-status { margin:4px 0 0; color:var(--muted); }
    .combat-overlay-editor-actions { display:flex; gap:8px; flex-wrap:wrap; justify-content:flex-end; }
    .combat-overlay-editor-workspace { display:grid; grid-template-columns:minmax(0, 1fr) minmax(320px, 360px); gap:13px; align-items:start; }
    .combat-overlay-editor-main { display:grid; min-width:0; gap:13px; align-content:start; }
    .combat-overlay-preview-shell { min-width:0; align-self:start; overflow:auto; padding:12px; border:1px solid var(--line); border-radius:16px; background:#060b12; }
    .combat-overlay-preview-label { display:flex; align-items:center; justify-content:space-between; gap:12px; padding:0 2px 10px; color:var(--muted); font-size:12px; }
    .combat-overlay-preview-label strong { color:var(--text); text-transform:uppercase; letter-spacing:.08em; }
    .combat-overlay-preview-label > span { min-width:120px; flex:1 1 auto; }
    .combat-overlay-preview-controls { display:flex; flex:0 0 auto; align-items:end; justify-content:flex-end; gap:12px; }
    .combat-overlay-preview-data-control { display:grid; gap:2px; color:var(--muted); font-size:9px; }
    .combat-overlay-preview-data-control select { min-width:112px; min-height:28px; border:1px solid var(--line); border-radius:6px; padding:3px 7px; color:var(--text); background:var(--input); font:600 11px/1.2 system-ui; color-scheme:dark; }
    .combat-overlay-preview-data-control option { color:#edf3fb; background:#101927; }
    .combat-overlay-preview-refresh { min-height:28px; padding:3px 9px; }
    .combat-overlay-dimension-controls { display:flex; align-items:end; gap:6px; }
    .combat-overlay-dimension-control { display:grid; gap:2px; color:var(--muted); font-size:9px; }
    .combat-overlay-dimension-control input { width:72px; min-height:28px; border:1px solid var(--line); border-radius:6px; padding:3px 6px; color:var(--text); background:var(--input); font:600 11px/1.2 system-ui; text-align:right; }
    .combat-overlay-scale-control { display:flex; align-items:center; gap:7px; white-space:nowrap; }
    .combat-overlay-scale-control input { width:120px; accent-color:#63e5d6; }
    .combat-overlay-scale-control strong { min-width:42px; color:#63e5d6; text-align:right; }
    .combat-overlay-canvas { position:relative; isolation:isolate; overflow:hidden; color:#f4f8ff; background:transparent; border-radius:12px; }
    .combat-overlay-canvas-preview { min-width:320px; }
    .combat-overlay-preview-resize-handle { position:absolute; z-index:120; right:1px; bottom:1px; width:18px; height:18px; padding:0; border:0; border-radius:4px 0 9px; background:linear-gradient(135deg,transparent 0 38%,#63e5d6 40% 49%,transparent 51% 61%,#63e5d6 63% 72%,transparent 74%); cursor:nwse-resize; touch-action:none; opacity:.72; }
    .combat-overlay-preview-resize-handle[data-width-only='true'] { height:100%; bottom:0; width:8px; border-radius:0 9px 9px 0; background:#63e5d629; cursor:ew-resize; }
    .combat-overlay-preview-resize-handle:hover, .combat-overlay-preview-resize-handle.is-resizing { opacity:1; filter:drop-shadow(0 0 5px #63e5d6); }
    .combat-overlay-layer { position:absolute; z-index:1; isolation:isolate; overflow:hidden; container-type:inline-size; box-sizing:border-box; border:1px solid #91a4bd38; border-radius:9px; background:transparent; box-shadow:0 12px 30px #0008; opacity:var(--overlay-opacity, .92); text-shadow:0 1px 2px #000,0 0 4px #000c; }
    .combat-overlay-layer::before { content:''; position:absolute; z-index:0; inset:0; pointer-events:none; opacity:var(--overlay-background-opacity, .92); background-color:var(--overlay-background-color, #0b1522); }
    .combat-overlay-layer[data-background-mode='transparent']::before { opacity:0; }
    .combat-overlay-layer[data-background-mode='custom']::before { background-image:var(--overlay-background-image, none); background-position:center; background-repeat:no-repeat; background-size:cover; }
    .combat-overlay-layer > * { position:relative; z-index:1; }
    .combat-overlay-layer[data-selected='true'] { outline:2px solid #5eead4; outline-offset:2px; }
    .combat-overlay-summary { border-bottom:1px solid #91a4bd2e; background:rgb(12 21 32 / var(--summary-opacity,.85)); }
    .combat-overlay-summary-row { display:flex; min-height:25px; align-items:stretch; justify-content:space-between; gap:8px; padding:0 7px; border-top:1px solid #91a4bd20; background:rgb(16 27 41 / var(--summary-opacity,.85)); }
    .combat-overlay-summary-row:first-child { min-height:30px; border-top:0; background:transparent; }
    .combat-overlay-summary-row-drop { min-height:13px; display:grid; place-items:center; border-top:1px dashed #63e5d655; color:#7f92aa; font:700 8px/1 system-ui; letter-spacing:.03em; cursor:copy; }
    .combat-overlay-summary-row-drop.is-reorder-target, .combat-overlay-summary-row.is-reorder-target { background:#153a42; color:#63e5d6; box-shadow:inset 0 0 0 1px #63e5d6; }
    .combat-overlay-summary-boss-grid { display:grid; grid-template-columns:minmax(176px,32%) minmax(0,1fr); align-items:stretch; column-gap:0; min-height:36px; border-top:1px solid #91a4bd20; background:rgb(16 27 41 / var(--summary-opacity,.85)); }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-row { min-height:0; padding:0; border-top:0; background:transparent; }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics > .combat-overlay-summary-items { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); width:100%; }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics .combat-overlay-summary-stat { position:relative; display:flex; min-width:0; min-height:36px; flex-direction:column; align-items:flex-end; justify-content:center; gap:1px; padding:2px 7px 2px 18px; border-right:1px solid #91a4bd25; }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics .combat-overlay-summary-stat > .combat-overlay-reorder-grip { position:absolute; left:5px; top:50%; transform:translateY(-50%); }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics .combat-overlay-summary-stat + .combat-overlay-summary-stat { border-top:0; }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics .combat-overlay-summary-stat small,
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-metrics .combat-overlay-summary-stat strong { text-align:right; }
    .combat-overlay-summary-boss-grid > .combat-overlay-summary-boss-cell .combat-overlay-summary-items { width:100%; }
    .combat-overlay-summary-boss-grid .combat-overlay-boss-list { border-top:0; }
    .combat-overlay-summary-primary { display:flex; min-height:30px; align-items:center; justify-content:space-between; gap:8px; padding:2px 6px 2px 7px; }
    .combat-overlay-summary-items { display:flex; min-width:0; flex:1 1 auto; flex-wrap:wrap; align-items:stretch; overflow:hidden; }
    .combat-overlay-summary-team-row { display:flex; min-height:25px; align-items:stretch; border-top:1px solid #91a4bd20; background:rgb(16 27 41 / var(--summary-opacity,.85)); padding:0 7px; }
    .combat-overlay-summary-team-items { flex:0 1 auto; }
    .combat-overlay-summary-stat { position:relative; display:flex; min-width:0; box-sizing:border-box; align-items:baseline; justify-content:space-between; gap:6px; padding:4px 8px; border-right:1px solid #91a4bd25; }
    .combat-overlay-summary-stat:last-child { border-right:0; }
    .combat-overlay-summary-stat small { overflow:hidden; color:#7f93aa; font-size:8px; font-weight:800; letter-spacing:.05em; text-overflow:ellipsis; text-transform:uppercase; white-space:nowrap; }
    .combat-overlay-summary-stat strong { color:#edf5ff; font-size:11px; font-variant-numeric:tabular-nums; white-space:nowrap; }
    .combat-overlay-summary-stat.label-hidden { gap:0; }
    .combat-overlay-summary-stat.label-hidden small { display:none; }
    .combat-overlay-summary-draggable { cursor:grab; touch-action:none; user-select:none; }
    .combat-overlay-summary-draggable.is-dragging { cursor:grabbing; }
    .combat-overlay-summary-stat > .combat-overlay-reorder-grip { pointer-events:none; }
    .combat-overlay-summary-control { position:relative; box-sizing:border-box; min-width:0; flex:0 0 auto; align-self:center; justify-content:center; margin:2px 3px; white-space:nowrap; }
    .combat-overlay-summary-control[data-button-action='cycle_timer'] { width:${FIXED_TIMER_CONTROL_WIDTH}px; min-width:${FIXED_TIMER_CONTROL_WIDTH}px; max-width:${FIXED_TIMER_CONTROL_WIDTH}px; flex-basis:${FIXED_TIMER_CONTROL_WIDTH}px; overflow:hidden; justify-content:center; font-variant-numeric:tabular-nums; text-overflow:ellipsis; }
    .combat-overlay-summary-editor-grid { display:grid; grid-template-columns:1fr; gap:7px; }
    .combat-overlay-summary-editor { display:grid; grid-template-columns:minmax(0,1fr) 76px; min-width:0; min-height:38px; gap:10px; align-items:center; padding:4px 8px; border:1px solid color-mix(in srgb,var(--line) 78%,transparent); border-radius:7px; background:color-mix(in srgb,var(--surface-soft) 72%,transparent); }
    .combat-overlay-summary-editor .combat-overlay-width-field { display:block; }
    .combat-overlay-summary-editor .combat-overlay-width-field > span { position:absolute; width:1px; height:1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); clip-path:inset(50%); border:0; white-space:nowrap; }
    .combat-overlay-summary-editor .combat-overlay-field input { min-height:30px; padding:4px 7px; text-align:right; }
    .combat-overlay-boss-list { position:relative; display:grid; grid-template-rows:repeat(2,22px); flex:1 1 100%; min-width:0; width:100%; border-top:0; }
    .combat-overlay-boss-list > .combat-overlay-reorder-grip { position:absolute; z-index:2; top:50%; left:3px; transform:translateY(-50%); }
    .combat-overlay-boss-row { position:relative; isolation:isolate; display:grid; grid-template-columns:minmax(0,1fr) minmax(140px,auto); align-items:center; gap:7px; min-height:22px; padding:2px 7px 2px 20px; overflow:hidden; color:#dbe6f3; font-size:8.5px; font-variant-numeric:tabular-nums; }
    .combat-overlay-boss-row::before { position:absolute; z-index:-1; inset:0; width:var(--boss-hp,0%); background:linear-gradient(90deg,#a33142,#d85555); content:""; opacity:calc(var(--summary-opacity,.85) * .52); }
    .combat-overlay-boss-row + .combat-overlay-boss-row { border-top:1px solid #91a4bd20; }
    .combat-overlay-boss-primary { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
    .combat-overlay-boss-team-metrics { display:grid; grid-template-columns:repeat(2,minmax(62px,auto)); align-items:center; justify-content:end; gap:6px; overflow:hidden; text-align:right; white-space:nowrap; }
    .combat-overlay-boss-team-metrics > span { overflow:hidden; text-overflow:ellipsis; }
    .combat-overlay-boss-team-metrics [data-metric='bdps']::before { color:#9aaabe; content:'bDPS '; font-size:7.5px; font-weight:800; }
    .combat-overlay-boss-team-metrics [data-metric='damage']::before { color:#9aaabe; content:'DMG '; font-size:7.5px; font-weight:800; }
    .combat-overlay-boss-list[data-boss-count='1'] { grid-template-rows:36px; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-row { grid-template-columns:minmax(0,1fr) minmax(140px,auto); grid-row:auto; gap:7px; min-height:36px; padding-block:2px; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-primary { display:block; min-height:0; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-team-metrics { grid-template-areas:'team team' 'bdps damage'; grid-template-rows:10px 14px; align-self:center; min-height:24px; border-top:0; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-team-metrics::before { grid-area:team; color:#9aaabe; content:'TEAM'; font-size:7px; font-weight:800; letter-spacing:.08em; text-align:center; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-team-metrics [data-metric='bdps'] { grid-area:bdps; }
    .combat-overlay-boss-list[data-boss-count='1'] .combat-overlay-boss-team-metrics [data-metric='damage'] { grid-area:damage; }
    @container (max-width:520px) {
      .combat-overlay-summary-boss-grid { grid-template-columns:minmax(128px,36%) minmax(0,1fr); }
      .combat-overlay-boss-row { grid-template-columns:minmax(0,1fr) minmax(112px,auto); gap:4px; padding-right:4px; font-size:8px; }
      .combat-overlay-boss-team-metrics { grid-template-columns:repeat(2,minmax(52px,auto)); gap:4px; }
      .combat-overlay-boss-team-metrics [data-metric='bdps']::before { content:'bDPS '; }
      .combat-overlay-boss-team-metrics [data-metric='damage']::before { content:'DMG '; }
    }
    .combat-overlay-layer-drag { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:13px; user-select:none; }
    .combat-overlay-layer-controls { display:flex; min-width:0; align-items:center; gap:5px; }
    .combat-overlay-view-controls { display:flex; min-width:0; align-items:center; gap:3px; padding-right:5px; border-right:1px solid #8aa0b82f; }
    .combat-overlay-control { display:inline-flex; min-height:23px; align-items:center; gap:4px; padding:2px 7px; border:1px solid #8aa0b82f; border-radius:5px; color:#bcd0e4; background:#0d1724; font:700 10px/1 system-ui; }
    .combat-overlay-control:hover { color:#63e5d6; border-color:#63e5d688; }
    .combat-overlay-view-control[data-active='true'] { color:#08141d; border-color:#63e5d6; background:#63e5d6; text-shadow:none; }
    .combat-overlay-view-control { position:relative; }
    .combat-overlay-view-control[data-active='true'] .combat-overlay-reorder-grip { color:#17343c; }
    .combat-overlay-reorder-grip { color:#687b91; cursor:grab; touch-action:none; user-select:none; letter-spacing:-2px; }
    .combat-overlay-canvas-preview .combat-overlay-view-control > .combat-overlay-reorder-grip,
    .combat-overlay-canvas-preview .combat-overlay-summary-stat > .combat-overlay-reorder-grip,
    .combat-overlay-canvas-preview .combat-overlay-summary-control > .combat-overlay-reorder-grip { position:absolute; z-index:6; left:2px; top:50%; transform:translateY(-50%); }
    .combat-overlay-reorder-grip:hover, .combat-overlay-reorder-grip.is-dragging { color:#63e5d6; cursor:grabbing; }
    .combat-overlay-row { position:relative; display:grid; align-items:center; min-height:28px; column-gap:7px; padding:0 8px; font:600 11px/1.1 system-ui; }
    .combat-overlay-row > span { position:relative; z-index:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; text-align:right; }
    .combat-overlay-row > .overlay-field-name, .combat-overlay-row > [data-header-field='name'] { text-align:left; }
    .combat-overlay-row > .overlay-badge-cell, .combat-overlay-row > [data-header-field='class_spec'], .combat-overlay-row > [data-header-field='weapon'], .combat-overlay-row > [data-header-field='main_imagines'] { text-align:center; }
    .overlay-badge-cell { z-index:1; display:flex; align-items:center; justify-content:center; gap:2px; overflow:visible !important; }
    .combat-overlay-badge { position:relative; display:inline-flex; width:22px; height:22px; flex:0 0 22px; align-items:center; justify-content:center; border:1px solid #8ba1b83d; border-radius:5px; color:#bcd0e4; background:#101d2b; font:800 9px/1 system-ui; box-sizing:border-box; }
    .combat-overlay-badge img { width:100%; height:100%; object-fit:contain; }
    .combat-overlay-badge[data-state='resolved'] { width:24px; height:24px; flex-basis:24px; overflow:visible; margin:-1px; border-color:transparent; border-radius:0; background:transparent; box-shadow:none; }
    .combat-overlay-badge[data-state='fallback'] { border:1px solid #8ba1b83d; border-radius:5px; background:#101d2b; }
    .combat-overlay-role-badge { --role-color:#bcd0e4; color:var(--role-color); }
    .combat-overlay-role-badge[data-state='fallback'] { border-color:color-mix(in srgb,var(--role-color) 62%,transparent); background:color-mix(in srgb,var(--role-color) 14%,#101d2b); }
    .combat-overlay-role-icon { width:108%; height:108%; background:var(--role-color); -webkit-mask-position:center; mask-position:center; -webkit-mask-repeat:no-repeat; mask-repeat:no-repeat; -webkit-mask-size:contain; mask-size:contain; }
    .combat-overlay-role-badge[data-combat-role='damage'] { --role-color:#d99a97; }
    .combat-overlay-role-badge[data-combat-role='healer'] { --role-color:#9bc9a8; }
    .combat-overlay-role-badge[data-combat-role='tank'] { --role-color:#7ea6c6; }
    .combat-overlay-role-badge[data-combat-accent='damage_glow'] { filter:drop-shadow(0 0 3px #ff4d5fcc) drop-shadow(0 0 6px #ff4d5f66); }
    .combat-overlay-badge[data-state='resolved'][data-badge-kind='far_sea'] img { filter:drop-shadow(0 0 2px #5fd0ffcc) drop-shadow(0 0 5px #5fd0ff77); }
    .combat-overlay-badge[data-state='resolved'][data-badge-kind='ember_far_sea'] img { filter:drop-shadow(0 0 2px #ffb152dd) drop-shadow(0 0 5px #ffb15288); }
    .combat-overlay-badge[data-state='fallback'][data-badge-kind='far_sea'] { background:linear-gradient(145deg,#123c62,#07192b 68%); border-color:#5fd0ff7a; font:14px/1 'Segoe UI Emoji',sans-serif; }
    .combat-overlay-badge[data-state='fallback'][data-badge-kind='ember_far_sea'] { background:linear-gradient(145deg,#53341a,#171326 58%,#07192b); border-color:#ffb15294; font:14px/1 'Segoe UI Emoji',sans-serif; }
    .combat-overlay-badge-level { position:absolute; right:1px; bottom:1px; padding:1px 2px; border-radius:2px; color:#fff; background:#02060ce8; font:800 6px/1 'Cascadia Code',Consolas,monospace; }
    .combat-overlay-badge[data-state='fallback'][data-tier='1'] { border-color:#aab1bd; box-shadow:0 0 5px #aab1bd80; }
    .combat-overlay-badge[data-state='fallback'][data-tier='2'] { border-color:#45d778; box-shadow:0 0 6px #45d7789c; }
    .combat-overlay-badge[data-state='fallback'][data-tier='3'] { border-color:#4ba3ff; box-shadow:0 0 6px #4ba3ff9c; }
    .combat-overlay-badge[data-state='fallback'][data-tier='4'] { border-color:#b06cff; box-shadow:0 0 7px #b06cffad; }
    .combat-overlay-badge[data-state='fallback'][data-tier='5'] { border-color:#f7c84a; box-shadow:0 0 7px #f7c84ab8; }
    .combat-overlay-badge[data-state='resolved'][data-tier='1'] img { filter:drop-shadow(0 0 2px #aab1bd) drop-shadow(0 0 4px #aab1bd80); }
    .combat-overlay-badge[data-state='resolved'][data-tier='2'] img { filter:drop-shadow(0 0 2px #45d778) drop-shadow(0 0 4px #45d7789c); }
    .combat-overlay-badge[data-state='resolved'][data-tier='3'] img { filter:drop-shadow(0 0 2px #4ba3ff) drop-shadow(0 0 4px #4ba3ff9c); }
    .combat-overlay-badge[data-state='resolved'][data-tier='4'] img { filter:drop-shadow(0 0 2px #b06cff) drop-shadow(0 0 4px #b06cffad); }
    .combat-overlay-badge[data-state='resolved'][data-tier='5'] img { filter:drop-shadow(0 0 2px #f7c84a) drop-shadow(0 0 4px #f7c84ab8); }
    .combat-overlay-actor-link { position:relative; z-index:1; overflow:hidden; min-width:0; padding:0; border:0; color:#f4f8ff; background:transparent; text-align:left; text-overflow:ellipsis; text-shadow:0 1px 2px #000,0 0 4px #000,1px 0 2px #000,-1px 0 2px #000; white-space:nowrap; font:inherit; cursor:pointer; }
    .combat-overlay-actor-link:hover { color:#63e5d6; text-decoration:underline; text-underline-offset:2px; }
    .combat-overlay-header-row { min-height:22px; color:#7f93aa; font-size:9px; text-transform:uppercase; letter-spacing:.08em; }
    .combat-overlay-header-row > span { box-sizing:border-box; overflow:visible; padding-right:6px; border-right:1px solid #66809b3d; }
    .combat-overlay-header-row > span:last-child { border-right-color:transparent; }
    .combat-overlay-canvas-preview .combat-overlay-header-row > span { border-right-color:#63e5d66b; }
    .combat-overlay-canvas-preview .combat-overlay-header-row > span:last-child { border-right-color:#63e5d63d; }
    .combat-overlay-canvas-preview[data-preview-dimmed='true'] .combat-overlay-layer { opacity:.28; filter:saturate(.45); }
    .combat-overlay-header-row > .combat-overlay-reorder-target { cursor:grab; touch-action:none; user-select:none; }
    .combat-overlay-reorder-target.is-dragging { cursor:grabbing; color:#63e5d6; }
    .combat-overlay-reorder-target.is-reorder-target, .combat-overlay-control.is-reorder-target { color:#63e5d6; background:#153a42; }
    .combat-overlay-reorder-target.is-reorder-target[data-reorder-placement='before'], .combat-overlay-control.is-reorder-target[data-reorder-placement='before'] { box-shadow:inset 3px 0 #63e5d6; }
    .combat-overlay-reorder-target.is-reorder-target[data-reorder-placement='after'], .combat-overlay-control.is-reorder-target[data-reorder-placement='after'] { box-shadow:inset -3px 0 #63e5d6; }
    .combat-overlay-header-resize, .combat-overlay-summary-resize { position:absolute; z-index:5; inset:-5px -10px -5px auto; width:20px; cursor:ew-resize; touch-action:none; }
    .combat-overlay-summary-resize { inset:0 -8px 0 auto; }
    .combat-overlay-header-resize::before, .combat-overlay-summary-resize::before { content:''; position:absolute; inset:3px auto 3px 9px; width:2px; border-radius:2px; background:#63e5d6; box-shadow:0 0 0 1px #061018, 0 0 7px #63e5d688; opacity:.82; }
    .combat-overlay-header-resize::after, .combat-overlay-summary-resize::after { content:'⋮'; position:absolute; top:50%; left:11px; color:#9af4ea; font:700 10px/1 system-ui; transform:translateY(-52%); opacity:.72; }
    .combat-overlay-header-resize:hover::before, .combat-overlay-header-resize.is-resizing::before, .combat-overlay-summary-resize:hover::before, .combat-overlay-summary-resize.is-resizing::before { width:3px; background:#d5fffa; box-shadow:0 0 0 1px #061018, 0 0 10px #63e5d6; opacity:1; }
    .combat-overlay-header-resize:hover::after, .combat-overlay-header-resize.is-resizing::after, .combat-overlay-summary-resize:hover::after, .combat-overlay-summary-resize.is-resizing::after { color:#fff; opacity:1; }
    .combat-overlay-header-resize.is-resizing::after, .combat-overlay-summary-resize.is-resizing::after { content:attr(data-width); top:-15px; left:50%; min-width:42px; padding:3px 5px; border:1px solid #63e5d6; border-radius:4px; color:#eafffc; background:#07131d; box-shadow:0 5px 14px #000a; text-align:center; transform:translateX(-50%); white-space:nowrap; }
    .combat-overlay-actor-row { border-top:1px solid #8395ab1f; }
    .combat-overlay-actor-row::before { content:''; position:absolute; inset:0 auto 0 0; width:var(--meter-fill); background:color-mix(in srgb,var(--meter-color,#63e5d6) 55%,#0b1522); opacity:var(--bar-opacity, .25); }
    .combat-overlay-ability-grid { grid-template-columns:minmax(112px, 1fr) minmax(68px, auto) 34px !important; }
    .combat-overlay-rdps-ability-grid { grid-template-columns:minmax(104px, 1fr) minmax(62px, auto) minmax(62px, auto) 34px !important; }
    .combat-overlay-ability-name { display:flex; align-items:center; gap:6px; text-align:left !important; }
    .combat-overlay-ability-name img { width:22px; height:22px; flex:0 0 22px; object-fit:contain; }
    .combat-overlay-ability-name > span { display:grid; min-width:0; gap:1px; overflow:hidden; }
    .combat-overlay-ability-name strong, .combat-overlay-ability-name small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
    .combat-overlay-ability-name small { color:#7f93aa; font-size:8px; }
    .combat-overlay-rdps-detail-value { cursor:help; font-variant-numeric:tabular-nums; }
    .combat-overlay-rdps-sources { margin:0; padding:5px 9px 7px 22px; border-top:1px solid #8395ab14; color:#91a4bd; font:9px/1.35 system-ui; }
    .combat-overlay-rdps-sources summary { color:#9af4ea; cursor:pointer; user-select:none; }
    .combat-overlay-rdps-sources div { padding:4px 0 0 10px; overflow-wrap:anywhere; }
    .combat-overlay-breakdown-note { margin:0; padding:7px 9px; border-top:1px solid #8395ab1f; color:#91a4bd; font:9px/1.3 system-ui; }
    .combat-overlay-empty { margin:0; padding:16px; color:#7f93aa; text-align:center; font:12px system-ui; }
    .combat-overlay-rdps-status { margin:0; padding:7px 9px; border-bottom:1px solid #91a4bd2e; color:#f2c879; background:#3a2b1266; font:600 9px/1.35 system-ui; }
    .combat-overlay-inspector { position:sticky; top:48px; display:grid; min-width:0; align-content:start; gap:14px; padding:16px; overflow:visible; }
    .combat-overlay-inspector > * { min-width:0; }
    .combat-overlay-inspector h3 { margin:0; line-height:1.25; }
    .combat-overlay-field { display:grid; min-width:0; gap:5px; color:var(--muted); font-size:11px; line-height:1.3; }
    .combat-overlay-field > span { min-width:0; overflow-wrap:anywhere; }
    .combat-overlay-field input, .combat-overlay-field select { display:block; width:100%; min-width:0; max-width:100%; min-height:36px; border:1px solid var(--line); border-radius:7px; padding:7px 9px; color:var(--text); background:var(--input); font:inherit; line-height:1.25; }
    .combat-overlay-field select { color-scheme:dark; }
    .combat-overlay-field select option, .combat-overlay-field select optgroup { color:#edf3fb; background:#101927; }
    .combat-overlay-field select option:checked { color:#07131d; background:#63e5d6; }
    .combat-overlay-field input[type='file'] { padding:6px; font-size:10px; }
    .combat-overlay-inspector-group { display:grid; min-width:0; gap:9px; margin:0; padding:12px; border:1px solid var(--line); border-radius:9px; }
    .combat-overlay-inspector-group legend { max-width:calc(100% - 12px); padding:0 5px; line-height:1.25; white-space:normal; }
    .combat-overlay-inspector-hint { margin:0; color:var(--muted); font-size:10px; line-height:1.35; }
    .combat-overlay-column-editor-panel { min-width:0; padding:14px 16px 16px; overflow:visible; }
    .combat-overlay-column-editor-panel[hidden] { display:none; }
    .combat-overlay-column-editor-panel .combat-overlay-inspector-group { padding:0; border:0; }
    .combat-overlay-header-editor-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(190px,1fr)); gap:7px 12px; }
    .combat-overlay-header-editor { display:grid; grid-template-columns:minmax(0, 1fr) 76px; min-width:0; min-height:38px; gap:10px; align-items:center; padding:4px 8px; border:1px solid color-mix(in srgb,var(--line) 78%,transparent); border-radius:7px; background:color-mix(in srgb,var(--surface-soft) 72%,transparent); }
    .combat-overlay-header-editor .combat-overlay-width-field { display:block; }
    .combat-overlay-header-editor .combat-overlay-width-field > span { position:absolute; width:1px; height:1px; padding:0; overflow:hidden; clip:rect(0 0 0 0); clip-path:inset(50%); border:0; white-space:nowrap; }
    .combat-overlay-header-editor .combat-overlay-field input { min-height:30px; padding:4px 7px; text-align:right; }
    .combat-overlay-checkbox { display:flex; min-width:0; gap:8px; align-items:center; font-size:12px; line-height:1.25; }
    .combat-overlay-checkbox input { width:16px; height:16px; flex:0 0 16px; margin:0; accent-color:#63e5d6; }
    .combat-overlay-checkbox span { min-width:0; overflow-wrap:anywhere; }
    .combat-overlay-button-editor { display:grid; grid-template-columns:minmax(0, 1fr) minmax(0, 1fr) 70px auto; gap:8px; align-items:end; }
    .combat-overlay-button-editor .combat-overlay-field:nth-child(3) input { text-align:right; }
    .combat-overlay-remove-control { min-height:36px; padding-inline:10px; }
    .combat-overlay-add-control { display:grid; grid-template-columns:minmax(0, 1fr) auto; gap:8px; align-items:end; padding-top:2px; }
    .combat-overlay-add-control > button { min-height:36px; white-space:nowrap; }
    .combat-overlay-view-editor-actions { display:grid; grid-template-columns:1fr 1fr; gap:7px; }
    .danger-button { color:#ffd2d8 !important; border-color:#ff7185a6 !important; background:#5b202a8c !important; font-weight:800; opacity:1; }
    .danger-button:hover:not(:disabled) { color:#fff !important; border-color:#ff7185 !important; background:#7b2430 !important; }
    .combat-overlay-context-menu, .combat-overlay-context-submenu { z-index:2000; display:grid; min-width:190px; padding:5px; border:1px solid #53677f; border-radius:8px; background:#0c1522; box-shadow:0 16px 44px #000b; }
    .combat-overlay-context-menu { position:fixed; }
    .combat-overlay-context-entry { position:relative; display:grid; }
    .combat-overlay-context-submenu { position:absolute; top:-5px; left:calc(100% + 4px); display:none; }
    .combat-overlay-context-entry.is-open > .combat-overlay-context-submenu { display:grid; }
    .combat-overlay-context-submenu.opens-left { right:calc(100% + 4px); left:auto; }
    .combat-overlay-context-submenu.opens-up { top:auto; bottom:-5px; }
    .combat-overlay-context-menu button, .combat-overlay-context-submenu button { position:relative; min-height:30px; padding:7px 28px 7px 10px; border:0; border-radius:5px; color:#dbe8f5; background:transparent; text-align:left; white-space:nowrap; }
    .combat-overlay-context-menu button:hover, .combat-overlay-context-menu button:focus-visible, .combat-overlay-context-submenu button:hover, .combat-overlay-context-submenu button:focus-visible { outline:0; background:#1b2d40; }
    .combat-overlay-context-menu button.has-submenu::after, .combat-overlay-context-submenu button.has-submenu::after { content:'›'; position:absolute; right:9px; color:#9eb2c8; font-size:18px; line-height:1; }
    .combat-overlay-context-menu button.danger, .combat-overlay-context-submenu button.danger { color:#ff9caa; }
    .combat-overlay-context-menu button:disabled, .combat-overlay-context-submenu button:disabled { color:#607084; cursor:default; }
    .combat-overlay-context-separator { height:1px; margin:4px 3px; background:#71849a45; }
    .combat-overlay-options { display:grid; min-width:0; gap:14px; }
    .combat-overlay-options > .content-card { box-sizing:border-box; min-width:0; overflow:hidden; }
    .combat-overlay-options-heading { padding:18px 20px; }
    .combat-overlay-options-heading h2 { margin:0; }
    .combat-overlay-options-heading p { margin:7px 0 0; max-width:900px; color:var(--muted); line-height:1.45; }
    .combat-overlay-options-heading > .combat-overlay-status { display:block; margin-top:9px; }
    .combat-overlay-options-form { display:grid; min-width:0; gap:14px; padding:18px 20px; }
    .combat-overlay-options-grid { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); min-width:0; gap:14px; align-items:start; }
    .combat-overlay-options-column { display:grid; min-width:0; gap:14px; align-content:start; }
    .combat-overlay-options-group { box-sizing:border-box; display:grid; min-width:0; gap:12px; margin:0; padding:15px; border:1px solid var(--line); border-radius:10px; background:color-mix(in srgb,var(--surface-soft) 70%,transparent); }
    .combat-overlay-options-group legend { padding:0 5px; color:var(--text); font-weight:800; }
    .combat-overlay-options-group p { margin:0; color:var(--muted); font-size:11px; line-height:1.4; }
    .combat-overlay-number-format-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(180px,1fr)); gap:9px; }
    .combat-overlay-bar-color-grid { display:grid; min-width:0; max-height:330px; gap:7px; overflow:auto; scrollbar-color:#3d556e #0a111c; }
    .combat-overlay-bar-color-row { display:grid; grid-template-columns:minmax(0,1fr) 34px auto; min-width:0; gap:9px; align-items:center; padding:7px 8px; border:1px solid color-mix(in srgb,var(--line) 75%,transparent); border-radius:8px; background:color-mix(in srgb,var(--input) 76%,transparent); }
    .combat-overlay-bar-color-copy { display:grid; min-width:0; gap:2px; }
    .combat-overlay-bar-color-copy strong, .combat-overlay-bar-color-copy small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
    .combat-overlay-bar-color-copy small { color:var(--muted); font:9px/1.2 'Cascadia Code',Consolas,monospace; }
    .combat-overlay-bar-color-row input[type='color'] { box-sizing:border-box; width:34px; height:30px; padding:2px; border:1px solid var(--line); border-radius:6px; background:var(--input); cursor:pointer; }
    .combat-overlay-color-reset { min-height:30px; padding:5px 9px !important; }
    .combat-overlay-color-empty { padding:8px; border:1px dashed var(--line); border-radius:8px; }
    .combat-overlay-status.error { color:#ff8f9e; }
    .combat-overlay-runtime-document, .combat-overlay-runtime-document body, .combat-overlay-runtime-document #app { margin:0; min-width:0; min-height:0; overflow:hidden; background:transparent !important; }
    .combat-overlay-runtime { position:relative; width:100vw; height:100vh; overflow:hidden; border-radius:9px; background:transparent; clip-path:inset(0 round 9px); }
    .combat-overlay-runtime.is-auto-hidden { visibility:hidden; opacity:0; pointer-events:none; }
    .combat-overlay-runtime-loading { margin:0; padding:12px; color:#9fb1c5; background:#0b1522e8; font:600 11px/1.35 system-ui; }
    .combat-overlay-canvas-runtime { overflow:hidden; border-radius:9px; background:transparent; clip-path:inset(0 round 9px); }
    .combat-overlay-runtime-resize-handle { position:absolute; z-index:110; margin:0; padding:0; border:0; background:transparent; opacity:.35; touch-action:none; }
    .combat-overlay-runtime-resize-handle[hidden] { display:none; }
    .combat-overlay-runtime-resize-handle[data-direction='East'] { top:0; right:0; bottom:10px; width:6px; cursor:ew-resize; }
    .combat-overlay-runtime-resize-handle[data-direction='South'] { right:10px; bottom:0; left:0; height:6px; cursor:ns-resize; }
    .combat-overlay-runtime-resize-handle[data-direction='SouthEast'] { right:0; bottom:0; width:14px; height:14px; border-radius:3px 0 0; background:linear-gradient(135deg,transparent 0 39%,#63e5d6 41% 50%,transparent 52% 62%,#63e5d6 64% 73%,transparent 75%); cursor:nwse-resize; }
    .combat-overlay-runtime:hover .combat-overlay-runtime-resize-handle { opacity:.9; }
    @media (max-width:1120px) { .combat-overlay-editor-workspace { grid-template-columns:1fr; } .combat-overlay-inspector { position:static; } .combat-overlay-preview-label { align-items:flex-start; flex-direction:column; } .combat-overlay-preview-controls { width:100%; flex-wrap:wrap; justify-content:flex-start; } .combat-overlay-options-grid { grid-template-columns:repeat(2,minmax(0,1fr)); } }
    @media (max-width:720px) { .combat-overlay-options-grid { grid-template-columns:1fr; } }
  `;
  document.head.append(style);
}
