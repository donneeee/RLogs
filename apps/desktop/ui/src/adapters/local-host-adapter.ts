import { invoke } from "@tauri-apps/api/core";
import {
  mountCombatOverlayEditorSurface,
  mountCombatOverlayOptionsSurface as mountOwnedCombatOverlayOptionsSurface,
} from "../../../../../plugins/builtin/desktop/combat-overlay/ui/combat-overlay";
import {
  type CombatHistoryCatalog,
  parseCombatHistoryCatalog,
  parseCombatHistoryDeleteResult,
  parseCombatHistorySnapshot,
} from "./combat-history";
import {
  compactSpecializationName,
  mountCombatHistorySurface,
} from "./combat-history-surface";
import {
  type CombatMeterSettings,
  HISTORY_PARTY_COLUMN_IDS,
  cloneHistoryPartyView,
  type HistoryPartyColumnId,
  type HistoryPartyViewSettings,
  historySpecializationFallbackColor,
  parseCombatMeterSettings,
} from "./combat-meter-settings";
import {
  captureInterfaceSummary,
  selectCaptureInterface,
  type CaptureEnvironment,
} from "./capture-interface";
import { engineStateFromRuntime } from "./engine-state";
import {
  loadHotkeySettings,
  mountHotkeyBinding,
} from "./hotkey-settings";
import {
  EVENT_VIEWER_TOPICS,
  type EventViewerFilter,
  type EventViewerPage,
  type EventViewerTopic,
  type LiveEventBatch,
  type LiveEventLine,
  parseEventViewerPage,
  parseLiveEventBatch,
} from "./event-viewer";
import {
  type LocalPluginCatalog,
  parsePluginCatalog,
} from "./plugin-catalog";
import {
  type ProfilePackageStoreView,
  parseProfilePackageInspection,
  parseProfilePackageStore,
  parseProfileProjectionResult,
} from "./profile-packages";
import { projectedRunCount } from "./run-projection";
import { parseRunReport } from "./run-report";
import { mountRunReportSurface } from "./run-report-surface";
import {
  type SubmissionQueueView,
  parseSubmissionImportResult,
  parseSubmissionQueue,
  parseSubmissionVerificationResult,
} from "./submission-queue";
import {
  type SubmissionPolicy,
  type SubmissionPolicyView,
  editableSubmissionPolicy,
  parseMockSubmissionResult,
  parseSubmissionPolicy,
  parseSubmissionTransportResult,
} from "./submission-policy";
import {
  applyThemeSettings,
  type ThemeBackground,
  type ThemeDensity,
  type ThemeFont,
  type ThemePreset,
  type ThemeSettings,
  parseThemeSettings,
} from "./theme-settings";
import type {
  DesktopHostAdapter,
  InstalledPluginDescriptor,
  MountedSurface,
  ShellPreferences,
  WorkspaceDescriptor,
  WorkspaceTabDescriptor,
} from "../shell/types";

const SESSION_RECORDER_PLUGIN_ID = "app.rlogs.session-recorder";
const COMBAT_METER_PLUGIN_ID = "app.rlogs.combat-meter";
const COMBAT_OVERLAY_PLUGIN_ID = "app.rlogs.combat-overlay";
const LOG_UPLOADER_PLUGIN_ID = "app.rlogs.log-uploader";
const PROFILE_SYNC_PLUGIN_ID = "app.rlogs.bpsr.profile-sync";
const THEMES_PLUGIN_ID = "app.rlogs.themes";

interface RuntimeResult {
  session_id: string;
  source_kind: string;
  output_rlog: string;
  coverage_report: string | null;
  frame_count: number | null;
  framed_record_count: number | null;
  canonical_event_count: number;
  known_route_count: number | null;
  unknown_route_count: number | null;
  data_gap_count: number | null;
  private_capture: string | null;
  connection_evidence: string | null;
  combat_plugin: RuntimePluginResult;
  combat_snapshot: unknown;
  encounter_recorder: RuntimePluginResult;
  submission_queue_id: string | null;
  submission_queue_status: string;
  profile_package_count: number;
  profile_sync_status: string;
  upload_artifact: {
    file_byte_length: number;
    file_sha256: string;
    chunk_count: number;
    canonical_content_sha256: string;
  } | null;
}

interface RuntimePluginResult {
  metrics: {
    events_seen: number;
    events_delivered: number;
    outputs_emitted: number;
  };
  outputs: Array<{
    type?: string;
    schema_id?: string;
    payload?: unknown;
  }>;
}

interface RuntimeSnapshot {
  schema_version: number;
  phase: "idle" | "processing" | "complete" | "failed";
  active_session_id: string | null;
  detail: string;
  started_unix_millis: number | null;
  completed_unix_millis: number | null;
  live_capture_can_stop: boolean;
  monitored_frame_count: number;
  decoded_event_count: number;
  saving_run: boolean;
  sealed_run_count: number;
  last_result: RuntimeResult | null;
}

interface ApiError {
  error: string;
}

interface RuntimeEnvironment extends CaptureEnvironment {
  platform: string;
  game_processes: Array<{
    process_id: number;
    executable_name: string;
  }>;
  dumpcap_path: string | null;
}

export async function createLocalHostAdapterIfAvailable(): Promise<DesktopHostAdapter | null> {
  try {
    const response = await fetch("/api/runtime/status", {
      cache: "no-store",
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(1_500),
    });
    if (
      !response.ok ||
      !response.headers.get("content-type")?.includes("application/json")
    ) {
      return null;
    }
    await response.json();
    return createLocalHostAdapter();
  } catch {
    return null;
  }
}

function subscribeLiveEvents(
  onBatch: (batch: LiveEventBatch) => void,
  onError: (error: unknown) => void,
): () => void {
  let active = true;
  let revision = 0;
  const abort = new AbortController();
  const run = async () => {
    while (active) {
      try {
        const batch = parseLiveEventBatch(
          await apiJson<unknown>("/api/runtime/live/events/wait", {
            method: "POST",
            headers: {
              Accept: "application/json",
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              after_revision: revision,
              timeout_millis: 1_000,
              limit: 512,
              tail: revision === 0,
            }),
            signal: abort.signal,
          }),
        );
        if (!active) return;
        if (batch.revision > revision) {
          revision = batch.revision;
          onBatch(batch);
        }
      } catch (error) {
        if (!active || abort.signal.aborted) return;
        onError(error);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      }
    }
  };
  void run();
  return () => {
    active = false;
    abort.abort();
  };
}

interface CombatHistoryRevisionUpdate {
  schema_version: 1;
  revision: number;
}

function parseCombatHistoryRevisionUpdate(value: unknown): CombatHistoryRevisionUpdate {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !Number.isSafeInteger(value.revision) ||
    (value.revision as number) < 0
  ) {
    throw new Error("The native host returned an invalid combat-history revision.");
  }
  return value as unknown as CombatHistoryRevisionUpdate;
}

function subscribeCombatHistoryChanges(
  onChange: () => void,
  onError: (error: unknown) => void,
): () => void {
  let active = true;
  let revision = 0;
  const abort = new AbortController();
  const run = async () => {
    while (active) {
      try {
        const update = parseCombatHistoryRevisionUpdate(
          await apiJson<unknown>("/api/runtime/combat-history/wait", {
            method: "POST",
            headers: {
              Accept: "application/json",
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              after_revision: revision,
              timeout_millis: 5_000,
            }),
            signal: abort.signal,
          }),
        );
        if (!active) return;
        if (update.revision > revision) {
          revision = update.revision;
          onChange();
        }
      } catch (error) {
        if (!active || abort.signal.aborted) return;
        onError(error);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      }
    }
  };
  void run();
  return () => {
    active = false;
    abort.abort();
  };
}

function createLocalHostAdapter(): DesktopHostAdapter {
  let pluginCatalogRequest: Promise<LocalPluginCatalog> | null = null;
  const loadPluginCatalog = (force = false): Promise<LocalPluginCatalog> => {
    if (force || pluginCatalogRequest === null) {
      pluginCatalogRequest = apiJson<unknown>("/api/plugins/catalog").then(
        parsePluginCatalog,
      );
    }
    return pluginCatalogRequest;
  };
  const updatePluginCatalog = async (
    route: string,
    init: RequestInit,
  ): Promise<LocalPluginCatalog> => {
    const catalog = parsePluginCatalog(await apiJson<unknown>(route, init));
    pluginCatalogRequest = Promise.resolve(catalog);
    return catalog;
  };

  return {
    modeLabel: "Active",

    async loadEngineState() {
      return engineStateFromRuntime(
        await apiJson<RuntimeSnapshot>("/api/runtime/status"),
      );
    },

    async loadWorkspaces() {
      const catalog = await loadPluginCatalog();
      return catalog.workspaces;
    },

    async loadPreferences() {
      return parseShellPreferences(
        await apiJson<unknown>("/api/settings/layout"),
      );
    },

    async savePreferences(preferences) {
      await apiJson<ShellPreferences>("/api/settings/layout", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(preferences),
      });
    },

    async mountSurface(workspace, tab, container) {
      container.replaceChildren();
      switch (tab.entrypoint) {
        case `builtin://${SESSION_RECORDER_PLUGIN_ID}/control`:
          return mountControlSurface(container);
        case `builtin://${SESSION_RECORDER_PLUGIN_ID}/sessions`:
          return mountLastSessionSurface(container);
        case `builtin://${SESSION_RECORDER_PLUGIN_ID}/events`:
          return mountEventViewerSurface(container);
        case `builtin://${SESSION_RECORDER_PLUGIN_ID}/runs`:
          return mountRunReportSurface(container, async () =>
            parseRunReport(await apiJson<unknown>("/api/runtime/run-report")),
          );
        case `builtin://${COMBAT_METER_PLUGIN_ID}/history`:
          return mountCombatHistorySurface(
            container,
            async () =>
              parseCombatHistoryCatalog(
                await apiJson<unknown>("/api/runtime/combat-history"),
              ),
            async (sessionId) =>
              parseCombatHistorySnapshot(
                await apiJson<unknown>("/api/runtime/combat-history/detail", {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ sessionId }),
                }),
              ),
            async () =>
              parseCombatMeterSettings(
                await apiJson<unknown>("/api/settings/combat-meter"),
              ),
            subscribeCombatHistoryChanges,
            {
              async setFavorite(historyId, isFavorite) {
                return parseCombatHistoryCatalog(
                  await apiJson<unknown>("/api/runtime/combat-history/favorite", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ historyId, isFavorite }),
                  }),
                );
              },
              async deleteEntries(historyIds) {
                return parseCombatHistoryDeleteResult(
                  await apiJson<unknown>("/api/runtime/combat-history/delete", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ historyIds }),
                  }),
                );
              },
            },
          );
        case `builtin://${COMBAT_METER_PLUGIN_ID}/options`:
          return mountCombatMeterOptionsSurface(container);
        case `builtin://${COMBAT_OVERLAY_PLUGIN_ID}/overlay`:
          return mountCombatOverlaySurface(container);
        case `builtin://${COMBAT_OVERLAY_PLUGIN_ID}/options`:
          return mountCombatOverlayOptionsSurface(container);
        case `builtin://${LOG_UPLOADER_PLUGIN_ID}/submissions`:
          return mountLogUploaderSettingsSurface(container);
        case `builtin://${PROFILE_SYNC_PLUGIN_ID}/profile-sync`:
          return mountProfileSyncSettingsSurface(container);
        case `builtin://${THEMES_PLUGIN_ID}/appearance`:
          return mountThemeSettingsSurface(container);
        case "core://settings/general":
          return mountCoreSettingsSurface(container);
        case "core://settings/network":
          return mountNetworkSettingsSurface(container);
        case "core://settings/hotkeys":
          return mountHotkeySettingsSurface(container);
      }
      const catalog = await loadPluginCatalog();
      return mountInstalledPackageSurface(
        container,
        workspace,
        tab,
        catalog.packages,
      );
    },

    async loadPluginCatalog() {
      return loadPluginCatalog();
    },

    async setPluginEnabled(pluginId, enabled) {
      return updatePluginCatalog("/api/plugins/enablement", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ pluginId, enabled }),
      });
    },

    async refreshPlugins() {
      return updatePluginCatalog("/api/plugins/refresh", {
        method: "POST",
      });
    },
  };
}

interface CoreSettings {
  schemaVersion: 1;
  closeToTray: boolean;
  hideOverlaysWhenUnfocused: boolean;
  captureInterface: string | null;
  dumpcapPath: string | null;
}

function mountCombatMeterOptionsSurface(
  container: HTMLElement,
): MountedSurface {
  let alive = true;
  let current: CombatMeterSettings | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface combat-meter-options-surface";
  const heading = actionCard(
    "Combat Meter options",
    "These controls belong to the Combat Meter plug-in and change presentation only. Captured events and calculated totals stay untouched.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const playerDetails = selectOption(
    "Player details",
    "Choose what happens when you select a player in a saved run.",
    [
      ["in_app_layer", "Open as the next page"],
      ["popover", "Open over the run"],
    ],
  );
  const partyRowHeading = document.createElement("div");
  partyRowHeading.className = "submission-policy-section-heading";
  partyRowHeading.append(
    text("strong", "Party row details"),
    text("small", "Check each captured detail you want shown beneath a party member's name."),
  );
  const showClass = checkboxOption(
    "Show class",
    "Display each observed class name in party rows.",
  );
  const showSpecialization = checkboxOption(
    "Show specialization",
    "Display the independently observed combat specialization when available.",
  );
  const showLevel = checkboxOption(
    "Show level",
    "Display the actor level frozen into the saved run.",
  );
  const showAbilityScore = checkboxOption(
    "Show Ability Score",
    "Display the packet-observed Ability Score when available.",
  );
  const showSeasonalScore = checkboxOption(
    "Show seasonal strength",
    "Display the actor's packet-observed seasonal strength score when available.",
  );
  const showCharacterUid = checkboxOption(
    "Show character UID",
    "Display the stable public character UID in party rows.",
  );
  const showPartyIcons = checkboxOption(
    "Show specialization icons",
    "Display the reviewed talent-tree specialization icon, with a class-icon fallback when its spec mapping is unresolved.",
  );
  const loadoutHeading = document.createElement("div");
  loadoutHeading.className = "submission-policy-section-heading";
  loadoutHeading.append(
    text("strong", "Party loadout row"),
    text("small", "Choose each equipment group shown on the third line of every party member."),
  );
  const showWeapon = checkboxOption(
    "Show weapon",
    "Display the observed equipped weapon; unresolved identity or icon data uses ?.",
  );
  const showPrimaryImagines = checkboxOption(
    "Show two primary Imagines",
    "Display both primary Imagine slots when the team snapshot exposes them.",
  );
  const showRoleLoadout = checkboxOption(
    "Show role skills / extra Imagines",
    "Display all four replaceable role slots without guessing unused equipment.",
  );
  const historyPartyColorMode = selectOption(
    "Party and timeline colors",
    "Use the same stable color for each summary bar, graph line, legend, and graph statistic.",
    [
      ["party_order", "Party order palette"],
      ["randomized", "Randomized per saved run"],
      ["specialization", "Custom by specialization"],
    ],
  );
  const specializationColorControls = new Map<string, HTMLInputElement>();
  const specializationColorList = document.createElement("div");
  specializationColorList.className = "history-specialization-color-list";
  specializationColorList.append(
    text(
      "p",
      "Loading specializations observed in saved runs…",
      "runtime-empty-result",
    ),
  );
  const updateColorModeVisibility = () => {
    specializationColorList.hidden = historyPartyColorMode.select.value !== "specialization";
  };
  historyPartyColorMode.select.addEventListener("change", updateColorModeVisibility);
  const historyColumnsHeading = document.createElement("div");
  historyColumnsHeading.className = "submission-policy-section-heading";
  historyColumnsHeading.append(
    text("strong", "History party columns"),
    text("small", "Show or hide each sortable header and its matching values in saved-run party summaries."),
  );
  const showHistoryPlayerColumn = checkboxOption(
    "Show Player",
    "Display player identity, class, specialization, captured stats, and loadout.",
  );
  const showHistoryDamageColumn = checkboxOption(
    "Show Damage",
    "Display total damage for the selected run segment and target.",
  );
  const showHistoryDpsColumn = checkboxOption(
    "Show DPS",
    "Display damage divided by the selected segment's elapsed time.",
  );
  const showHistoryEncounterDpsColumn = checkboxOption(
    "Show eDPS",
    "Display damage divided by active combat time.",
  );
  const showHistoryHpsColumn = checkboxOption(
    "Show HPS",
    "Display effective healing per second.",
  );
  const showHistoryTpsColumn = checkboxOption(
    "Show TPS",
    "Display damage taken per second.",
  );
  const showHistoryRdpsColumn = checkboxOption(
    "Show rDPS",
    "Display relative DPS when the run contains a resolved contribution model.",
  );
  const showHistoryApmColumn = checkboxOption(
    "Show APM",
    "Display resolved active actions per minute.",
  );
  const showHistoryDeathsColumn = checkboxOption(
    "Show Deaths",
    "Display each party member's death count.",
  );
  let historyPartyViews: HistoryPartyViewSettings[] = [];
  let selectedHistoryPartyViewId = "";
  const historyPartyViewsEditor = document.createElement("div");
  historyPartyViewsEditor.className = "history-party-views-editor";
  const renderHistoryPartyViewsEditor = () => {
    historyPartyViewsEditor.replaceChildren();
    const selected = historyPartyViews.find((view) => view.id === selectedHistoryPartyViewId)
      ?? historyPartyViews[0];
    if (!selected) {
      historyPartyViewsEditor.append(text("p", "No History views are configured.", "runtime-empty-result"));
      return;
    }
    selectedHistoryPartyViewId = selected.id;
    const toolbar = document.createElement("div");
    toolbar.className = "history-party-view-toolbar";
    const viewSelect = document.createElement("select");
    viewSelect.setAttribute("aria-label", "History party view");
    for (const view of historyPartyViews) viewSelect.append(new Option(view.label, view.id));
    viewSelect.value = selected.id;
    viewSelect.addEventListener("change", () => {
      selectedHistoryPartyViewId = viewSelect.value;
      renderHistoryPartyViewsEditor();
    });
    const addView = button("Add view", "quiet-button");
    addView.type = "button";
    addView.disabled = historyPartyViews.length >= 12;
    addView.addEventListener("click", () => {
      const id = uniqueHistoryPartyViewId(historyPartyViews, "view");
      historyPartyViews.push({
        id,
        label: `View ${historyPartyViews.length + 1}`,
        columns: ["player", "damage", "dps"],
        widths: { player: 360, damage: 120, dps: 105 },
        sortKey: "dps",
        sortDirection: "descending",
        detailMode: "damage",
      });
      selectedHistoryPartyViewId = id;
      renderHistoryPartyViewsEditor();
    });
    const duplicateView = button("Duplicate", "quiet-button");
    duplicateView.type = "button";
    duplicateView.disabled = historyPartyViews.length >= 12;
    duplicateView.addEventListener("click", () => {
      const copy = cloneHistoryPartyView(selected);
      copy.id = uniqueHistoryPartyViewId(historyPartyViews, `${selected.id}-copy`);
      copy.label = `${selected.label} copy`.slice(0, 32);
      historyPartyViews.push(copy);
      selectedHistoryPartyViewId = copy.id;
      renderHistoryPartyViewsEditor();
    });
    const deleteView = button("Delete", "quiet-button history-party-view-delete");
    deleteView.type = "button";
    deleteView.disabled = historyPartyViews.length === 1;
    deleteView.addEventListener("click", () => {
      if (historyPartyViews.length === 1) return;
      const index = historyPartyViews.findIndex((view) => view.id === selected.id);
      historyPartyViews.splice(index, 1);
      selectedHistoryPartyViewId = historyPartyViews[Math.max(0, index - 1)]!.id;
      renderHistoryPartyViewsEditor();
    });
    toolbar.append(viewSelect, addView, duplicateView, deleteView);

    const name = field("View name", "text", selected.label, "Damage");
    name.input.maxLength = 32;
    name.input.addEventListener("input", () => {
      selected.label = name.input.value.slice(0, 32);
      const option = [...viewSelect.options].find((candidate) => candidate.value === selected.id);
      if (option) option.textContent = selected.label || "Untitled view";
    });
    const defaults = document.createElement("div");
    defaults.className = "history-party-view-defaults";
    const sort = selectOption(
      "Default sort",
      "This column controls the initial row order when you switch to the view.",
      selected.columns.map((column) => [column, historyPartyColumnLabel(column)] as const),
    );
    sort.select.value = selected.sortKey;
    sort.select.addEventListener("change", () => {
      selected.sortKey = sort.select.value as HistoryPartyColumnId;
    });
    const direction = selectOption(
      "Direction",
      "Choose the initial direction; table headers remain clickable afterward.",
      [["descending", "Highest first"], ["ascending", "Lowest first"]],
    );
    direction.select.value = selected.sortDirection;
    direction.select.addEventListener("change", () => {
      selected.sortDirection = direction.select.value as "ascending" | "descending";
    });
    defaults.append(sort.label, direction.label);
    const detailMode = selectOption(
      "Player drill-down",
      "Choose the summary opened when a party member is selected from this view.",
      [
        ["damage", "Damage and offensive abilities"],
        ["healing", "Healing and shielding sources"],
        ["defense", "Incoming damage by source and ability"],
      ],
    );
    detailMode.select.value = selected.detailMode;
    detailMode.select.addEventListener("change", () => {
      selected.detailMode = detailMode.select.value as "damage" | "healing" | "defense";
    });

    const activeHeading = document.createElement("div");
    activeHeading.className = "submission-policy-section-heading";
    activeHeading.append(
      text("strong", "Visible headers"),
      text("small", "Drag to reorder. Widths are saved independently for this view."),
    );
    const columns = document.createElement("div");
    columns.className = "history-party-view-columns";
    let draggedColumn: HistoryPartyColumnId | null = null;
    for (const column of selected.columns) {
      const row = document.createElement("div");
      row.className = "history-party-view-column";
      row.draggable = true;
      row.dataset.column = column;
      row.addEventListener("dragstart", () => {
        draggedColumn = column;
        row.dataset.dragging = "true";
      });
      row.addEventListener("dragend", () => {
        draggedColumn = null;
        delete row.dataset.dragging;
      });
      row.addEventListener("dragover", (event) => event.preventDefault());
      row.addEventListener("drop", (event) => {
        event.preventDefault();
        if (!draggedColumn || draggedColumn === column) return;
        const from = selected.columns.indexOf(draggedColumn);
        const to = selected.columns.indexOf(column);
        selected.columns.splice(from, 1);
        selected.columns.splice(to, 0, draggedColumn);
        renderHistoryPartyViewsEditor();
      });
      const drag = text("span", "⋮⋮", "history-party-view-drag");
      drag.title = `Drag ${historyPartyColumnLabel(column)} to reorder it`;
      const label = text("strong", historyPartyColumnLabel(column));
      const width = document.createElement("input");
      width.type = "number";
      width.min = "24";
      width.max = "800";
      width.step = "1";
      width.value = String(selected.widths[column] ?? historyPartyDefaultWidth(column));
      width.setAttribute("aria-label", `${historyPartyColumnLabel(column)} width in pixels`);
      width.addEventListener("change", () => {
        selected.widths[column] = Math.max(24, Math.min(800, Number(width.value) || historyPartyDefaultWidth(column)));
        width.value = String(selected.widths[column]);
      });
      const remove = button("Remove", "quiet-button");
      remove.type = "button";
      remove.disabled = selected.columns.length === 1;
      remove.addEventListener("click", () => {
        if (selected.columns.length === 1) return;
        selected.columns = selected.columns.filter((candidate) => candidate !== column);
        if (selected.sortKey === column) {
          selected.sortKey = selected.columns[0]!;
          selected.sortDirection = selected.sortKey === "player" ? "ascending" : "descending";
        }
        renderHistoryPartyViewsEditor();
      });
      row.append(drag, label, width, text("span", "px"), remove);
      columns.append(row);
    }
    const available = HISTORY_PARTY_COLUMN_IDS.filter((column) => !selected.columns.includes(column));
    const addColumnRow = document.createElement("div");
    addColumnRow.className = "history-party-view-add-column";
    const addColumnSelect = document.createElement("select");
    addColumnSelect.setAttribute("aria-label", "Header to add");
    for (const column of available) addColumnSelect.append(new Option(historyPartyColumnLabel(column), column));
    const addColumn = button("Add header", "quiet-button");
    addColumn.type = "button";
    addColumn.disabled = available.length === 0;
    addColumn.addEventListener("click", () => {
      const column = addColumnSelect.value as HistoryPartyColumnId;
      if (!column || selected.columns.includes(column)) return;
      selected.columns.push(column);
      selected.widths[column] = historyPartyDefaultWidth(column);
      renderHistoryPartyViewsEditor();
    });
    addColumnRow.append(addColumnSelect, addColumn);
    historyPartyViewsEditor.append(toolbar, name.label, defaults, detailMode.label, activeHeading, columns, addColumnRow);
  };
  const historyBodyFontSize = numberOption(
    "History body text",
    "Base labels, controls, and descriptive copy throughout History.",
    11,
    24,
  );
  const historyHeadingFontSize = numberOption(
    "History headings",
    "Dungeon, player, graph, and section heading size.",
    16,
    40,
  );
  const historyTableFontSize = numberOption(
    "History tables and rows",
    "Saved-run rows, party rankings, abilities, and entity tables.",
    10,
    24,
  );
  const historyMetadataFontSize = numberOption(
    "History metadata",
    "UIDs, class/spec details, dates, captions, and axis labels.",
    9,
    20,
  );
  const historyMetricFontSize = numberOption(
    "History metrics",
    "Prominent timer, damage, DPS, eDPS, HPS, and TPS values.",
    13,
    36,
  );
  const historyIconSize = numberOption(
    "History specialization icons",
    "Talent-tree icons in party rows and the saved-run browser.",
    20,
    64,
  );
  const historySizingHeading = document.createElement("div");
  historySizingHeading.className = "submission-policy-section-heading";
  historySizingHeading.append(
    text("strong", "History sizing"),
    text("small", "Independent pixel sizes; these affect History only."),
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const save = button("Save Combat Meter options", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const message = text(
    "span",
    "Loading Combat Meter settings…",
    "runtime-action-message",
  );
  actions.append(save, message);
  const navigationGroup = historyOptionsGroup(
    "Navigation",
    "How History opens deeper player and skill information.",
    [playerDetails.label],
    true,
  );
  const partyRowsGroup = historyOptionsGroup(
    "Party rows",
    "Identity, progression, icons, and loadout information shown inside the Player column.",
    [
      partyRowHeading,
      showClass.label,
      showSpecialization.label,
      showLevel.label,
      showAbilityScore.label,
      showSeasonalScore.label,
      showCharacterUid.label,
      showPartyIcons.label,
      loadoutHeading,
      showWeapon.label,
      showPrimaryImagines.label,
      showRoleLoadout.label,
    ],
  );
  const columnsGroup = historyOptionsGroup(
    "Party table views",
    "Create independent named layouts with their own headers, order, widths, and initial sorting.",
    [historyPartyViewsEditor],
  );
  const colorsGroup = historyOptionsGroup(
    "Party colors",
    "Keep summary bars and timeline series visually linked while choosing how identities receive colors.",
    [historyPartyColorMode.label, specializationColorList],
  );
  const sizingGroup = historyOptionsGroup(
    "Sizing",
    "Independent font and icon sizes that affect History only.",
    [
      historySizingHeading,
      historyBodyFontSize.label,
      historyHeadingFontSize.label,
      historyTableFontSize.label,
      historyMetadataFontSize.label,
      historyMetricFontSize.label,
      historyIconSize.label,
    ],
  );
  form.append(
    navigationGroup,
    partyRowsGroup,
    columnsGroup,
    colorsGroup,
    sizingGroup,
    actions,
  );
  root.append(heading, form);
  container.replaceChildren(root);

  void Promise.all([
    apiJson<unknown>("/api/settings/combat-meter").then(parseCombatMeterSettings),
    apiJson<unknown>("/api/runtime/combat-history")
      .then(parseCombatHistoryCatalog)
      .catch(() => null),
  ]).then(([settings, catalog]) => {
      if (!alive) return;
      current = settings;
      playerDetails.select.value = settings.playerDetailPresentation;
      showClass.input.checked = settings.showClass;
      showSpecialization.input.checked = settings.showSpecialization;
      showLevel.input.checked = settings.showLevel;
      showAbilityScore.input.checked = settings.showAbilityScore;
      showSeasonalScore.input.checked = settings.showSeasonalScore;
      showCharacterUid.input.checked = settings.showCharacterUid;
      showPartyIcons.input.checked = settings.showPartyIcons;
      showWeapon.input.checked = settings.showWeapon;
      showPrimaryImagines.input.checked = settings.showPrimaryImagines;
      showRoleLoadout.input.checked = settings.showRoleLoadout;
      showHistoryPlayerColumn.input.checked = settings.showHistoryPlayerColumn;
      showHistoryDamageColumn.input.checked = settings.showHistoryDamageColumn;
      showHistoryDpsColumn.input.checked = settings.showHistoryDpsColumn;
      showHistoryEncounterDpsColumn.input.checked = settings.showHistoryEncounterDpsColumn;
      showHistoryHpsColumn.input.checked = settings.showHistoryHpsColumn;
      showHistoryTpsColumn.input.checked = settings.showHistoryTpsColumn;
      showHistoryRdpsColumn.input.checked = settings.showHistoryRdpsColumn;
      showHistoryApmColumn.input.checked = settings.showHistoryApmColumn;
      showHistoryDeathsColumn.input.checked = settings.showHistoryDeathsColumn;
      historyPartyViews = settings.historyPartyViews.map(cloneHistoryPartyView);
      selectedHistoryPartyViewId = historyPartyViews[0]!.id;
      renderHistoryPartyViewsEditor();
      historyPartyColorMode.select.value = settings.historyPartyColorMode;
      renderHistorySpecializationColorControls(
        specializationColorList,
        specializationColorControls,
        historySpecializationChoices(catalog, settings.historySpecializationColors),
        settings.historySpecializationColors,
      );
      updateColorModeVisibility();
      historyBodyFontSize.input.value = String(settings.historyBodyFontSizePx);
      historyHeadingFontSize.input.value = String(settings.historyHeadingFontSizePx);
      historyTableFontSize.input.value = String(settings.historyTableFontSizePx);
      historyMetadataFontSize.input.value = String(settings.historyMetadataFontSizePx);
      historyMetricFontSize.input.value = String(settings.historyMetricFontSizePx);
      historyIconSize.input.value = String(settings.historyIconSizePx);
      save.disabled = false;
      message.textContent = "History presentation settings are ready.";
    })
    .catch((error: unknown) => {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (current === null) return;
    save.disabled = true;
    message.classList.remove("error");
    message.textContent = "Saving…";
    try {
      const historySpecializationColors = {
        ...current.historySpecializationColors,
      };
      for (const [specializationId, input] of specializationColorControls) {
        historySpecializationColors[specializationId] = input.value;
      }
      current = parseCombatMeterSettings(
        await apiJson<unknown>("/api/settings/combat-meter", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            schemaVersion: 1,
            playerDetailPresentation: playerDetails.select.value,
            showClass: showClass.input.checked,
            showSpecialization: showSpecialization.input.checked,
            showLevel: showLevel.input.checked,
            showAbilityScore: showAbilityScore.input.checked,
            showSeasonalScore: showSeasonalScore.input.checked,
            showCharacterUid: showCharacterUid.input.checked,
            showPartyIcons: showPartyIcons.input.checked,
            showWeapon: showWeapon.input.checked,
            showPrimaryImagines: showPrimaryImagines.input.checked,
            showRoleLoadout: showRoleLoadout.input.checked,
            showHistoryPlayerColumn: showHistoryPlayerColumn.input.checked,
            showHistoryDamageColumn: showHistoryDamageColumn.input.checked,
            showHistoryDpsColumn: showHistoryDpsColumn.input.checked,
            showHistoryEncounterDpsColumn: showHistoryEncounterDpsColumn.input.checked,
            showHistoryHpsColumn: showHistoryHpsColumn.input.checked,
            showHistoryTpsColumn: showHistoryTpsColumn.input.checked,
            showHistoryRdpsColumn: showHistoryRdpsColumn.input.checked,
            showHistoryApmColumn: showHistoryApmColumn.input.checked,
            showHistoryDeathsColumn: showHistoryDeathsColumn.input.checked,
            historyPartyViews,
            historyPartyColorMode: historyPartyColorMode.select.value,
            historySpecializationColors,
            historyBodyFontSizePx: Number(historyBodyFontSize.input.value),
            historyHeadingFontSizePx: Number(historyHeadingFontSize.input.value),
            historyTableFontSizePx: Number(historyTableFontSize.input.value),
            historyMetadataFontSizePx: Number(historyMetadataFontSize.input.value),
            historyMetricFontSizePx: Number(historyMetricFontSize.input.value),
            historyIconSizePx: Number(historyIconSize.input.value),
          }),
        }),
      );
      if (alive) message.textContent = "Combat Meter options saved.";
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      if (alive) save.disabled = false;
    }
  });

  return {
    dispose() {
      alive = false;
      root.remove();
    },
  };
}

function mountCombatOverlaySurface(container: HTMLElement): MountedSurface {
  return mountCombatOverlayEditorSurface(container, async () => {
    await invoke("show_combat_overlay");
  });
}

function mountCombatOverlayOptionsSurface(
  container: HTMLElement,
): MountedSurface {
  return mountOwnedCombatOverlayOptionsSurface(container);
}

function mountLogUploaderSettingsSurface(
  container: HTMLElement,
): MountedSurface {
  return mountCombinedSettingsSurface(container, [
    (target) =>
      mountSubmissionPolicyOptionsSurface(target, "log_uploader"),
    mountSubmissionQueueSurface,
  ]);
}

function mountProfileSyncSettingsSurface(
  container: HTMLElement,
): MountedSurface {
  return mountCombinedSettingsSurface(container, [
    (target) =>
      mountSubmissionPolicyOptionsSurface(target, "bpsr_profile_sync"),
    mountProfileSyncStatusSurface,
  ]);
}

function mountCombinedSettingsSurface(
  container: HTMLElement,
  mounts: readonly ((target: HTMLElement) => MountedSurface)[],
): MountedSurface {
  const root = document.createElement("div");
  root.className = "plugin-surface combined-settings-surface";
  const mounted = mounts.map((mount) => {
    const target = document.createElement("section");
    target.className = "combined-settings-section";
    root.append(target);
    return mount(target);
  });
  container.replaceChildren(root);
  return {
    dispose() {
      mounted.forEach((surface) => surface.dispose());
      root.remove();
    },
  };
}

function mountCoreSettingsSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let coreSettings: CoreSettings | null = null;
  let layoutSettings: ShellPreferences | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface core-settings-surface";
  const heading = actionCard(
    "Core behavior",
    "These controls belong to rLogs itself. Feature-specific controls remain owned by the plug-in that contributes their Settings tab.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const closeToTray = checkboxOption(
    "Close to notification area",
    "The window hides instead of ending rLogs when you press Close. Use the tray menu to reopen or quit.",
  );
  const hideOverlaysWhenUnfocused = checkboxOption(
    "Hide overlays when the game is not active",
    "Hide every rLogs overlay while neither the active game nor rLogs is the foreground app. Each overlay returns only when its own visibility rules allow it.",
  );
  const lockTabs = checkboxOption(
    "Lock tab dragging",
    "Tabs remain selectable, but cannot be reordered inside their own section.",
  );
  const lockSections = checkboxOption(
    "Lock section dragging",
    "Whole tab sections remain fixed while preserving their plug-in-defined membership.",
  );
  const boundary = actionCard(
    "Section boundary",
    "A tab can only move inside the section that declared it. Moving a whole section never changes which tabs belong to it.",
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions submission-policy-actions";
  const save = button("Save Core settings", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const reset = button("Reset saved layout", "quiet-button");
  reset.disabled = true;
  const message = text(
    "span",
    "Loading Core settings…",
    "runtime-action-message",
  );
  actions.append(save, reset, message);
  form.append(
    closeToTray.label,
    hideOverlaysWhenUnfocused.label,
    lockTabs.label,
    lockSections.label,
    actions,
  );
  root.append(heading, form, boundary);
  container.replaceChildren(root);

  const applyViews = (core: CoreSettings, layout: ShellPreferences) => {
    coreSettings = core;
    layoutSettings = layout;
    closeToTray.input.checked = core.closeToTray;
    hideOverlaysWhenUnfocused.input.checked =
      core.hideOverlaysWhenUnfocused;
    lockTabs.input.checked = layout.lockTabDragging;
    lockSections.input.checked = layout.lockSectionDragging;
    save.disabled = false;
    reset.disabled = false;
    message.textContent = "Settings are stored by the native host.";
  };

  void Promise.all([
    apiJson<unknown>("/api/settings/core").then(parseCoreSettings),
    apiJson<unknown>("/api/settings/layout").then(parseShellPreferences),
  ])
    .then(([core, layout]) => {
      if (alive) applyViews(core, layout);
    })
    .catch((error: unknown) => {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (coreSettings === null || layoutSettings === null) return;
    save.disabled = true;
    message.classList.remove("error");
    message.textContent = "Saving atomically…";
    try {
      const [core, layout] = await Promise.all([
        apiJson<unknown>("/api/settings/core", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            ...coreSettings,
            closeToTray: closeToTray.input.checked,
            hideOverlaysWhenUnfocused:
              hideOverlaysWhenUnfocused.input.checked,
          }),
        }).then(parseCoreSettings),
        apiJson<unknown>("/api/settings/layout", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            ...layoutSettings,
            lockTabDragging: lockTabs.input.checked,
            lockSectionDragging: lockSections.input.checked,
          }),
        }).then(parseShellPreferences),
      ]);
      if (!alive) return;
      applyViews(core, layout);
      window.dispatchEvent(
        new CustomEvent<ShellPreferences>("rlogs:layout-settings-changed", {
          detail: layout,
        }),
      );
      message.textContent = "Core settings saved.";
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
      save.disabled = false;
    }
  });

  reset.addEventListener("click", async () => {
    if (layoutSettings === null) return;
    reset.disabled = true;
    message.classList.remove("error");
    message.textContent = "Resetting saved ordering…";
    try {
      const layout = parseShellPreferences(
        await apiJson<unknown>("/api/settings/layout", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            ...layoutSettings,
            workspaceOrder: [],
            activeWorkspaceId: null,
            activeTabs: {},
            tabOrders: {},
            sectionOrders: {},
          }),
        }),
      );
      if (!alive) return;
      layoutSettings = layout;
      window.dispatchEvent(
        new CustomEvent<ShellPreferences>("rlogs:layout-settings-changed", {
          detail: layout,
        }),
      );
      message.textContent = "Saved tab, section, and workspace ordering reset.";
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      if (alive) reset.disabled = false;
    }
  });

  return {
    dispose() {
      alive = false;
      root.remove();
    },
  };
}

function mountHotkeySettingsSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  const mountedBindings: ReturnType<typeof mountHotkeyBinding>[] = [];
  const root = document.createElement("div");
  root.className = "plugin-surface hotkey-settings-surface";
  const heading = actionCard(
    "Hotkeys",
    "Core-owned shortcuts work while the game has focus. Changing a shortcut here updates every feature-specific Hotkey control that points to the same action.",
  );
  const list = document.createElement("section");
  list.className = "content-card hotkey-settings-list";
  list.append(text("p", "Loading hotkey actions...", "runtime-action-message"));
  root.append(heading, list);
  container.replaceChildren(root);

  void loadHotkeySettings().then((settings) => {
    if (!alive) return;
    list.replaceChildren();
    let category: string | null = null;
    for (const action of settings.actions) {
      if (action.category !== category) {
        category = action.category;
        list.append(text("h3", category, "hotkey-category-heading"));
      }
      const binding = mountHotkeyBinding(action.actionId);
      mountedBindings.push(binding);
      list.append(binding.element);
    }
    if (settings.actions.length === 0) {
      list.append(text("p", "No hotkey actions are currently registered."));
    }
  }).catch((error: unknown) => {
    if (!alive) return;
    list.replaceChildren(text("p", errorMessage(error), "runtime-action-message error"));
  });

  return {
    dispose() {
      alive = false;
      mountedBindings.forEach((binding) => binding.dispose());
      root.remove();
    },
  };
}

function mountNetworkSettingsSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let coreSettings: CoreSettings | null = null;
  let runtimeEnvironment: RuntimeEnvironment | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface network-settings-surface";
  const heading = actionCard(
    "Native capture adapter",
    "Choose the interface rLogs should use for future live captures. Capture remains off until you explicitly start it from Session Recorder.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const interfaceLabel = document.createElement("label");
  interfaceLabel.className = "submission-policy-select";
  const interfaceCopy = document.createElement("span");
  interfaceCopy.append(
    text("strong", "Network device"),
    text(
      "small",
      "Shown by name, adapter model, MAC address, and connection state. The leading number is only dumpcap's internal index.",
    ),
  );
  const captureInterface = document.createElement("select");
  const interfaceStatus = text(
    "small",
    "Waiting for adapter discovery.",
    "network-device-status",
  );
  interfaceLabel.append(interfaceCopy, captureInterface, interfaceStatus);
  const dumpcap = field(
    "dumpcap executable",
    "text",
    "",
    "C:\\Program Files\\Wireshark\\dumpcap.exe",
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const save = button("Save network settings", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const refresh = button("Refresh devices", "quiet-button");
  const message = text(
    "span",
    "Detecting capture interfaces…",
    "runtime-action-message",
  );
  actions.append(save, refresh, message);
  form.append(interfaceLabel, dumpcap.label, actions);
  root.append(heading, form);
  container.replaceChildren(root);

  const load = async () => {
    refresh.disabled = true;
    message.classList.remove("error");
    message.textContent = "Detecting capture interfaces…";
    try {
      const [core, environment] = await Promise.all([
        apiJson<unknown>("/api/settings/core").then(parseCoreSettings),
        apiJson<RuntimeEnvironment>("/api/runtime/environment"),
      ]);
      if (!alive) return;
      coreSettings = core;
      runtimeEnvironment = environment;
      captureInterface.replaceChildren();
      if (environment.capture_interfaces.length === 0) {
        const option = document.createElement("option");
        option.value = "";
        option.textContent = "No interfaces detected";
        captureInterface.append(option);
      } else {
        for (const device of environment.capture_interfaces) {
          const option = document.createElement("option");
          option.value = device.value;
          option.textContent = device.label;
          captureInterface.append(option);
        }
      }
      if (
        core.captureInterface !== null &&
        !environment.capture_interfaces.some(
          (device) => device.value === core.captureInterface,
        )
      ) {
        const saved = document.createElement("option");
        saved.value = core.captureInterface;
        saved.textContent = `Saved device (${core.captureInterface})`;
        captureInterface.append(saved);
      }
      const selection = selectCaptureInterface(
        environment,
        core.captureInterface,
      );
      captureInterface.value = selection.device?.value ?? "";
      interfaceStatus.textContent = captureInterfaceSummary(
        selection,
        environment,
        core.captureInterface,
      );
      dumpcap.input.value =
        core.dumpcapPath ?? environment.dumpcap_path ?? "";
      save.disabled = false;
      message.textContent = selection.replacedSavedDevice
        ? "The unusable saved device was replaced in this form. Save to keep the corrected selection."
        : `${environment.capture_interfaces.length} interface${environment.capture_interfaces.length === 1 ? "" : "s"} detected.`;
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      if (alive) refresh.disabled = false;
    }
  };

  captureInterface.addEventListener("change", () => {
    const device = runtimeEnvironment?.capture_interfaces.find(
      (candidate) => candidate.value === captureInterface.value,
    );
    if (device === undefined) {
      interfaceStatus.textContent = "This device was not found in the latest scan.";
      return;
    }
    const details = [
      device.friendly_name,
      device.description,
      device.mac_address === null ? null : `MAC ${device.mac_address}`,
      device.is_up === true
        ? "active"
        : device.is_up === false
          ? "disconnected"
          : null,
      device.is_virtual === true ? "virtual adapter" : null,
    ].filter((value): value is string => value !== null);
    interfaceStatus.textContent = details.join(" — ");
  });
  refresh.addEventListener("click", () => void load());
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (coreSettings === null) return;
    save.disabled = true;
    message.classList.remove("error");
    message.textContent = "Saving network settings…";
    try {
      coreSettings = parseCoreSettings(
        await apiJson<unknown>("/api/settings/core", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            ...coreSettings,
            captureInterface: emptyToNull(captureInterface.value),
            dumpcapPath: emptyToNull(dumpcap.input.value),
          }),
        }),
      );
      if (alive) message.textContent = "Network settings saved.";
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      if (alive) save.disabled = false;
    }
  });
  void load();

  return {
    dispose() {
      alive = false;
      root.remove();
    },
  };
}

function mountThemeSettingsSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let current: ThemeSettings | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface theme-settings-surface";
  const heading = actionCard(
    "Appearance",
    "Themes are supplied by the Themes plug-in. These values style the shared shell without moving feature code into Core.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const preset = selectOption("Theme", "Base color treatment.", [
    ["midnight", "Midnight"],
    ["graphite", "Graphite"],
    ["aurora", "Aurora"],
  ]);
  const background = selectOption(
    "Background",
    "Tinted treatment behind the workspace.",
    [
      ["soft-glow", "Soft glow"],
      ["glass", "Tinted glass"],
      ["aurora", "Aurora"],
      ["none", "None"],
    ],
  );
  const density = selectOption("Density", "Spacing across the shell.", [
    ["comfortable", "Comfortable"],
    ["compact", "Compact"],
  ]);
  const font = selectOption("Font", "Shared application typeface.", [
    ["system", "System"],
    ["humanist", "Humanist"],
    ["mono", "Monospace"],
  ]);
  const scale = field("Font scale (%)", "number", "100", "85-130");
  scale.input.min = "85";
  scale.input.max = "130";
  scale.input.step = "1";
  const accent = field("Accent color", "color", "#64dfd2", "#64dfd2");
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const save = button("Apply appearance", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const message = text(
    "span",
    "Loading Themes settings…",
    "runtime-action-message",
  );
  actions.append(save, message);
  form.append(
    preset.label,
    background.label,
    density.label,
    font.label,
    scale.label,
    accent.label,
    actions,
  );
  root.append(heading, form);
  container.replaceChildren(root);

  const applyView = (settings: ThemeSettings) => {
    current = settings;
    preset.select.value = settings.preset;
    background.select.value = settings.background;
    density.select.value = settings.density;
    font.select.value = settings.font;
    scale.input.value = String(settings.fontScalePercent);
    accent.input.value = settings.accent;
    applyThemeSettings(settings);
    save.disabled = false;
    message.textContent = "Appearance is applied immediately after saving.";
  };
  void apiJson<unknown>("/api/settings/themes")
    .then(parseThemeSettings)
    .then((settings) => {
      if (alive) applyView(settings);
    })
    .catch((error: unknown) => {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
    });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (current === null) return;
    save.disabled = true;
    message.classList.remove("error");
    try {
      const settings = parseThemeSettings(
        await apiJson<unknown>("/api/settings/themes", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            schemaVersion: 1,
            preset: preset.select.value as ThemePreset,
            background: background.select.value as ThemeBackground,
            density: density.select.value as ThemeDensity,
            font: font.select.value as ThemeFont,
            fontScalePercent: Number(scale.input.value),
            accent: accent.input.value,
          }),
        }),
      );
      if (!alive) return;
      applyView(settings);
      message.textContent = "Appearance saved.";
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
      save.disabled = false;
    }
  });

  return {
    dispose() {
      alive = false;
      root.remove();
    },
  };
}

function mountInstalledPackageSurface(
  container: HTMLElement,
  workspace: WorkspaceDescriptor,
  tab: WorkspaceTabDescriptor,
  packages: readonly InstalledPluginDescriptor[],
): MountedSurface {
  container.replaceChildren();
  const owner = packages.find((plugin) => plugin.id === workspace.id);
  const contributor = packages.find(
    (plugin) => plugin.id === tab.contributorPluginId,
  );
  const expectedPrefix = `installed://${tab.contributorPluginId}/`;
  const surfaceId = tab.entrypoint.startsWith(expectedPrefix)
    ? tab.entrypoint.slice(expectedPrefix.length)
    : "";
  if (
    contributor?.active &&
    surfaceId.length > 0 &&
    (contributor.runtime === "browser_overlay" ||
      contributor.runtime === "native_developer")
  ) {
    const frame = document.createElement("iframe");
    frame.className = "installed-plugin-frame";
    frame.title = `${workspace.name} - ${tab.label}`;
    frame.referrerPolicy = "no-referrer";
    frame.setAttribute(
      "sandbox",
      contributor.runtime === "native_developer"
        ? "allow-forms allow-scripts allow-same-origin"
        : "allow-forms allow-scripts",
    );
    frame.src = `/api/plugins/surface/${encodeURIComponent(tab.contributorPluginId)}/${encodeURIComponent(surfaceId)}`;
    container.append(frame);
    return {
      dispose() {
        frame.src = "about:blank";
        frame.remove();
      },
    };
  }
  const surface = document.createElement("div");
  surface.className = "plugin-surface installed-package-surface";
  const status = actionCard(
    `${workspace.name} · ${tab.label}`,
    "This folder package and its workspace declaration passed host validation, but no compatible active surface runtime is available.",
  );
  const details = document.createElement("section");
  details.className = "content-card package-inspection-card";
  details.append(
    fileRow("Workspace owner", owner?.id ?? workspace.id),
    fileRow("Surface contributor", contributor?.id ?? tab.contributorPluginId),
    fileRow("Declared runtime", formatIdentifier(contributor?.runtime ?? "unknown")),
    fileRow(
      "Requested capabilities",
      contributor?.capabilities.length
        ? contributor.capabilities.map(formatIdentifier).join(", ")
        : "None",
    ),
    fileRow(
      "Event subscriptions",
      contributor?.subscriptions.length
        ? contributor.subscriptions.map(formatIdentifier).join(", ")
        : "None",
    ),
  );
  const boundary = actionCard(
    "Execution boundary",
    "Only enabled browser and native-developer packages can mount local HTML surfaces. Other runtimes remain metadata-only until their adapter is available.",
  );
  surface.append(status, details, boundary);
  container.append(surface);
  return {
    dispose() {
      surface.remove();
    },
  };
}

function mountControlSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  const root = document.createElement("div");
  root.className = "plugin-surface runtime-control-surface";

  const status = runtimeStatusCard();
  const safeReplay = actionCard(
    "Safe pipeline check",
    "Replay the sanitized canonical fixture through the bounded Combat Meter plug-in. This does not read packets or start capture.",
  );
  const replayButton = button("Run safe replay", "primary-button");
  const replayMessage = text("span", "Ready.", "runtime-action-message");
  replayButton.addEventListener("click", async () => {
    replayButton.disabled = true;
    replayMessage.textContent = "Running…";
    try {
      await apiJson<RuntimeResult>("/api/runtime/reference-replay", {
        method: "POST",
      });
      replayMessage.textContent = "Replay completed.";
      await refreshStatus(status);
    } catch (error) {
      replayMessage.textContent = errorMessage(error);
    } finally {
      replayButton.disabled = false;
    }
  });
  const safeActions = document.createElement("div");
  safeActions.className = "runtime-card-actions";
  safeActions.append(replayButton, replayMessage);
  safeReplay.append(safeActions);

  const offline = actionCard(
    "Process an existing BPSR capture",
    "Runs PCAP → exact connection filter → TCP reconstruction → BPSR protocol pack → canonical .rlog → Combat Meter. The source PCAP remains private and local.",
  );
  const form = document.createElement("form");
  form.className = "runtime-form";
  const sessionId = field(
    "Session ID",
    "text",
    defaultSessionId(),
    "letters, digits, dots, underscores, or dashes",
  );
  const capturePath = field(
    "Private PCAP or PCAPNG",
    "text",
    "",
    "C:\\path\\to\\capture.pcapng",
  );
  const connectionsPath = field(
    "Exact connection evidence",
    "text",
    "",
    "C:\\path\\to\\capture.connections.json",
  );
  const packPath = field(
    "Protocol pack override",
    "text",
    "",
    "leave blank for the current bundled Global pack",
  );
  const outputDirectory = field(
    "Log output folder",
    "text",
    "",
    "leave blank for RLogs/runtime-data/logs",
  );
  const formGrid = document.createElement("div");
  formGrid.className = "runtime-form-grid";
  formGrid.append(
    sessionId.label,
    capturePath.label,
    connectionsPath.label,
    packPath.label,
    outputDirectory.label,
  );
  const processButton = button("Process capture", "primary-button");
  processButton.type = "submit";
  const processMessage = text(
    "span",
    "No capture is started by this form.",
    "runtime-action-message",
  );
  const formActions = document.createElement("div");
  formActions.className = "runtime-card-actions";
  formActions.append(processButton, processMessage);
  form.append(formGrid, formActions);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    processButton.disabled = true;
    processMessage.textContent = "Starting background session…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/offline", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          session_id: sessionId.input.value.trim(),
          capture_path: capturePath.input.value.trim(),
          connections_path: connectionsPath.input.value.trim(),
          pack_path: emptyToNull(packPath.input.value),
          output_directory: emptyToNull(outputDirectory.input.value),
        }),
      });
      processMessage.textContent =
        "Accepted. The page will keep polling while the capture is decoded.";
      await refreshStatus(status);
    } catch (error) {
      processMessage.textContent = errorMessage(error);
    } finally {
      processButton.disabled = false;
    }
  });
  offline.append(form);

  const live = actionCard(
    "Continuous process-owned monitoring",
    "rLogs automatically monitors exact TCP flows owned by BPSR_STEAM. The BPSR plug-in decodes continuously in memory, opens a dungeon log on an entry packet, seals it on completion, and immediately waits for the next entry.",
  );
  const liveForm = document.createElement("form");
  liveForm.className = "runtime-form";
  const liveSessionId = field(
    "Session ID",
    "text",
    defaultSessionId(),
    "use a new ID for every capture",
  );
  const processId = field(
    "BPSR_STEAM process ID",
    "number",
    "",
    "Task Manager PID",
  );
  processId.input.min = "1";
  processId.input.step = "1";
  const captureInterface = field(
    "dumpcap interface",
    "text",
    "1",
    "interface number or \\\\Device\\\\NPF_…",
  );
  const dumpcapPath = field(
    "dumpcap executable",
    "text",
    "C:\\Program Files\\Wireshark\\dumpcap.exe",
    "absolute path to dumpcap.exe",
  );
  const logOutput = field(
    "Canonical log folder",
    "text",
    "",
    "leave blank for runtime-data/logs",
  );
  const liveGrid = document.createElement("div");
  liveGrid.className = "runtime-form-grid";
  liveGrid.append(
    liveSessionId.label,
    processId.label,
    captureInterface.label,
    dumpcapPath.label,
    logOutput.label,
  );
  const startLive = button("Start monitoring now", "primary-button");
  startLive.type = "submit";
  const stopLive = button("Restart monitoring", "quiet-button");
  stopLive.disabled = true;
  const refreshEnvironment = button("Refresh detection", "quiet-button");
  const liveMessage = text(
    "span",
    "Monitoring is automatic while the game process exists. Login/authentication routes are prohibited, and no dungeon log is persisted before an entry boundary.",
    "runtime-action-message",
  );
  const liveActions = document.createElement("div");
  liveActions.className = "runtime-card-actions";
  liveActions.append(
    startLive,
    stopLive,
    refreshEnvironment,
    liveMessage,
  );
  liveForm.append(liveGrid, liveActions);
  liveForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    startLive.disabled = true;
    liveMessage.textContent = "Starting continuous process-owned monitoring…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/live/start", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          session_id: liveSessionId.input.value.trim(),
          process_id: Number(processId.input.value),
          interface: captureInterface.input.value.trim(),
          dumpcap_path: dumpcapPath.input.value.trim(),
          duration_seconds: 0,
          log_output_directory: emptyToNull(logOutput.input.value),
        }),
      });
      liveMessage.textContent =
        "Monitoring all process-owned BPSR packets; dungeon persistence is waiting for an entry boundary.";
      stopLive.disabled = false;
      await updateRuntimeControls();
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
      startLive.disabled = false;
    }
  });
  stopLive.addEventListener("click", async () => {
    stopLive.disabled = true;
    liveMessage.textContent =
      "Restart requested; draining current decoder state before automatic monitoring resumes…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/live/stop", {
        method: "POST",
      });
      await updateRuntimeControls();
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
      await updateRuntimeControls();
    }
  });
  const detectEnvironment = async () => {
    refreshEnvironment.disabled = true;
    try {
      const [environment, core] = await Promise.all([
        apiJson<RuntimeEnvironment>("/api/runtime/environment"),
        apiJson<unknown>("/api/settings/core").then(parseCoreSettings),
      ]);
      dumpcapPath.input.value =
        core.dumpcapPath ??
        environment.dumpcap_path ??
        dumpcapPath.input.value;
      const process = environment.game_processes[0];
      if (process !== undefined) {
        processId.input.value = String(process.process_id);
      }
      const selection = selectCaptureInterface(
        environment,
        core.captureInterface,
      );
      const captureDevice = selection.device;
      if (captureDevice !== null) {
        captureInterface.input.value = captureDevice.value;
        captureInterface.input.title = captureDevice.label;
      } else if (core.captureInterface !== null) {
        captureInterface.input.value = core.captureInterface;
        captureInterface.input.title =
          "Saved device was not present in the latest dumpcap scan.";
      }
      const processDetail =
        process === undefined
          ? "BPSR_STEAM is not currently detected"
          : environment.game_processes.length === 1
            ? `detected ${process.executable_name} PID ${process.process_id}`
            : `detected ${environment.game_processes.length} matching processes; using PID ${process.process_id}`;
      const interfaceDetail =
        captureDevice === null
          ? core.captureInterface === null
            ? "no dumpcap interface was auto-selected"
            : "using the saved interface, which was not detected in this scan"
          : selection.source === "game_traffic"
            ? `using game-matched device ${captureDevice.label}`
            : core.captureInterface === captureDevice.value
            ? `using saved device ${captureDevice.label}`
            : `using ${captureDevice.label}`;
      liveMessage.textContent = `${processDetail}; ${interfaceDetail}.`;
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
    } finally {
      refreshEnvironment.disabled = false;
    }
  };
  refreshEnvironment.addEventListener("click", () => {
    void detectEnvironment();
  });
  live.append(liveForm);

  root.append(status.card, safeReplay, offline, live);
  container.append(root);
  const updateRuntimeControls = async () => {
    const snapshot = await refreshStatus(status);
    if (snapshot === null) {
      startLive.disabled = false;
      stopLive.disabled = true;
      return;
    }
    const liveActive =
      snapshot.phase === "processing" && snapshot.live_capture_can_stop;
    startLive.disabled = snapshot.phase === "processing";
    stopLive.disabled = !liveActive;
  };
  void detectEnvironment();
  void updateRuntimeControls();
  const interval = window.setInterval(() => {
    if (alive) {
      void updateRuntimeControls();
    }
  }, 1_000);

  return {
    dispose() {
      alive = false;
      window.clearInterval(interval);
    },
  };
}

function mountLastSessionSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  const root = document.createElement("div");
  root.className = "plugin-surface runtime-session-surface";
  const heading = actionCard(
    "Latest completed pipeline",
    "This view reports privacy-safe runtime counts and output locations. It never renders raw packet payloads or connection endpoints.",
  );
  const result = document.createElement("div");
  result.className = "runtime-result";
  root.append(heading, result);
  container.append(root);

  const refresh = async () => {
    try {
      const snapshot = await apiJson<RuntimeSnapshot>("/api/runtime/status");
      if (!alive) {
        return;
      }
      renderLastResult(result, snapshot);
    } catch (error) {
      result.replaceChildren(
        text("p", errorMessage(error), "runtime-action-message error"),
      );
    }
  };
  void refresh();
  const interval = window.setInterval(() => void refresh(), 1_000);
  return {
    dispose() {
      alive = false;
      window.clearInterval(interval);
    },
  };
}

function mountEventViewerSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let queryId: string | null = null;
  let busy = false;
  const root = document.createElement("div");
  root.className = "plugin-surface event-viewer-surface";
  const heading = actionCard(
    "Canonical Event Viewer",
    "Live, privacy-reviewed events after opcode decoding and before localization. IDs and amounts remain exact decimal strings; raw packet bytes and login/account payloads never enter this view.",
  );
  const liveCard = document.createElement("section");
  liveCard.className = "content-card event-live-card";
  const liveHeader = document.createElement("header");
  liveHeader.className = "card-heading";
  liveHeader.append(
    text("h2", "Live decoded log"),
    text("span", "ID-first · bounded to 500 visible lines"),
  );
  const liveActions = document.createElement("div");
  liveActions.className = "runtime-card-actions";
  const pauseLive = button("Pause view", "secondary-button");
  const clearLive = button("Clear visible lines", "quiet-button");
  const liveMessage = text(
    "span",
    "Connecting to the native decoded-event feed…",
    "runtime-action-message",
  );
  liveActions.append(pauseLive, clearLive, liveMessage);
  const liveLog = document.createElement("div");
  liveLog.className = "event-live-log";
  liveLog.setAttribute("role", "log");
  liveLog.setAttribute("aria-live", "off");
  liveCard.append(liveHeader, liveActions, liveLog);
  const controls = document.createElement("section");
  controls.className = "content-card event-viewer-controls";
  const filterGrid = document.createElement("div");
  filterGrid.className = "event-filter-grid";

  const topicLabel = document.createElement("label");
  topicLabel.className = "runtime-field";
  topicLabel.append(text("span", "Topic"));
  const topicSelect = document.createElement("select");
  topicSelect.append(new Option("All canonical topics", ""));
  for (const topic of EVENT_VIEWER_TOPICS) {
    topicSelect.append(new Option(formatIdentifier(topic), topic));
  }
  topicLabel.append(topicSelect);

  const kind = field(
    "Event kind",
    "text",
    "",
    "damage, cast, status, data_gap…",
  );
  const search = field(
    "Canonical ID search",
    "search",
    "",
    "entity UUID, actor, ability, status, scene…",
  );
  const pageSizeLabel = document.createElement("label");
  pageSizeLabel.className = "runtime-field";
  pageSizeLabel.append(text("span", "Rows per page"));
  const pageSize = document.createElement("select");
  for (const size of [50, 100, 200]) {
    pageSize.append(new Option(String(size), String(size), false, size === 100));
  }
  pageSizeLabel.append(pageSize);
  filterGrid.append(topicLabel, kind.label, search.label, pageSizeLabel);

  const actions = document.createElement("div");
  actions.className = "runtime-card-actions event-viewer-actions";
  const apply = button("Apply filters", "primary-button");
  const next = button("Next page", "secondary-button");
  next.disabled = true;
  const message = text(
    "span",
    "Waiting for a completed canonical log.",
    "runtime-action-message",
  );
  actions.append(apply, next, message);
  controls.append(filterGrid, actions);

  const metadata = document.createElement("section");
  metadata.className = "content-card event-viewer-metadata";
  metadata.hidden = true;
  const tableCard = document.createElement("section");
  tableCard.className = "content-card event-viewer-table-card";
  const tableMessage = text(
    "p",
    "Run the safe reference replay or complete a capture to inspect its canonical events.",
    "runtime-empty-result",
  );
  tableCard.append(tableMessage);

  const detail = document.createElement("section");
  detail.className = "content-card event-viewer-detail";
  detail.hidden = true;
  const detailHeader = document.createElement("div");
  detailHeader.className = "event-detail-header";
  const detailTitle = text("h2", "Canonical event");
  const copy = button("Copy exact JSON", "quiet-button");
  const canonical = document.createElement("pre");
  detailHeader.append(detailTitle, copy);
  detail.append(detailHeader, canonical);
  copy.addEventListener("click", () => {
    void navigator.clipboard
      .writeText(canonical.textContent ?? "")
      .then(() => {
        copy.textContent = "Copied";
        window.setTimeout(() => {
          if (alive) {
            copy.textContent = "Copy exact JSON";
          }
        }, 1_200);
      })
      .catch((error: unknown) => {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      });
  });

  root.append(heading, liveCard, controls, metadata, tableCard, detail);
  container.append(root);

  const selectedFilter = (): EventViewerFilter => ({
    topic:
      topicSelect.value === ""
        ? null
        : (topicSelect.value as EventViewerTopic),
    kind: emptyToNull(kind.input.value.toLowerCase()),
    search: emptyToNull(search.input.value),
  });

  let livePaused = false;
  let liveRenderFrame: number | null = null;
  let pendingLiveEvents: LiveEventLine[] = [];
  let pendingDropped = 0;
  let liveSessionId: string | null = null;

  const liveEventMatches = (event: LiveEventLine): boolean => {
    const filter = selectedFilter();
    if (filter.topic !== null && event.topic !== filter.topic) return false;
    if (filter.kind !== null && event.kind !== filter.kind) return false;
    if (filter.search === null) return true;
    const searchValue = filter.search.toLowerCase();
    return (
      event.rawIds.toLowerCase().includes(searchValue) ||
      event.kind.toLowerCase().includes(searchValue) ||
      event.topic.toLowerCase().includes(searchValue) ||
      String(event.sequence).includes(searchValue)
    );
  };

  const renderPendingLiveEvents = () => {
    liveRenderFrame = null;
    if (!alive || livePaused) {
      pendingLiveEvents = [];
      pendingDropped = 0;
      return;
    }
    const fragment = document.createDocumentFragment();
    if (pendingDropped > 0) {
      const gap = text(
        "div",
        `[viewer gap] ${pendingDropped.toLocaleString()} older live line(s) were skipped or left the bounded host ring; the full .rlog remains intact.`,
        "event-live-line event-live-gap",
      );
      fragment.append(gap);
    }
    for (const event of pendingLiveEvents.filter(liveEventMatches).slice(-500)) {
      const line = document.createElement("div");
      line.className = "event-live-line";
      line.append(
        text("span", `#${event.sequence}`, "event-live-sequence"),
        text("span", formatObservedMicros(event.observedMicros), "event-live-time"),
        text(
          "span",
          `${formatIdentifier(event.topic)}/${formatIdentifier(event.kind)}`,
          "event-live-kind",
        ),
        text("code", event.rawIds, "event-live-ids"),
      );
      fragment.append(line);
    }
    pendingLiveEvents = [];
    pendingDropped = 0;
    liveLog.append(fragment);
    while (liveLog.childElementCount > 500) {
      liveLog.firstElementChild?.remove();
    }
    liveLog.scrollTop = liveLog.scrollHeight;
    liveMessage.classList.remove("error");
    liveMessage.textContent = `${liveSessionId ?? "live session"} · following decoded events`;
  };

  const queueLiveBatch = (batch: LiveEventBatch) => {
    liveSessionId = batch.sessionId;
    if (livePaused) return;
    pendingDropped = pendingDropped + batch.droppedBefore;
    pendingLiveEvents.push(...batch.events);
    if (pendingLiveEvents.length > 1_000) {
      pendingDropped += pendingLiveEvents.length - 1_000;
      pendingLiveEvents = pendingLiveEvents.slice(-1_000);
    }
    if (liveRenderFrame === null) {
      liveRenderFrame = window.requestAnimationFrame(renderPendingLiveEvents);
    }
  };

  pauseLive.addEventListener("click", () => {
    livePaused = !livePaused;
    pauseLive.textContent = livePaused ? "Resume view" : "Pause view";
    liveMessage.textContent = livePaused
      ? "View paused; decoded events and the lossless .rlog continue recording."
      : "Live view resumed.";
  });
  clearLive.addEventListener("click", () => {
    liveLog.replaceChildren();
    pendingLiveEvents = [];
    pendingDropped = 0;
  });
  const unsubscribeLive = subscribeLiveEvents(queueLiveBatch, (error) => {
    if (!alive) return;
    liveMessage.textContent = errorMessage(error);
    liveMessage.classList.add("error");
  });

  const renderPage = (page: EventViewerPage) => {
    queryId = page.queryId;
    metadata.hidden = false;
    metadata.replaceChildren(
      fileRow("Session", page.sessionId),
      fileRow(
        "Region / realm",
        [
          page.header.region.identity.deployment_id,
          page.header.region.identity.region_id,
          page.header.region.identity.realm_id,
          page.header.region.identity.world_id,
        ]
          .filter((value): value is string => value !== null && value !== "")
          .join(" / "),
      ),
      fileRow("Client build", page.header.region.client_build),
      fileRow("Protocol pack", page.header.region.protocol_pack_digest),
      fileRow("Sealed digest", page.artifactDigest),
    );

    const tableWrap = document.createElement("div");
    tableWrap.className = "event-table-wrap";
    const table = document.createElement("table");
    table.className = "event-table";
    const header = document.createElement("thead");
    const headerRow = document.createElement("tr");
    for (const label of ["Seq", "Time", "Topic / event", "Canonical IDs", "Amount"]) {
      headerRow.append(text("th", label));
    }
    header.append(headerRow);
    const body = document.createElement("tbody");
    for (const event of page.events) {
      const row = document.createElement("tr");
      const sequenceCell = document.createElement("td");
      const open = button(String(event.sequence), "event-sequence-button");
      open.title = `Open canonical event ${event.sequence}`;
      open.addEventListener("click", () => {
        canonical.textContent = event.canonicalJson;
        detailTitle.textContent =
          `#${event.sequence} · ${formatIdentifier(event.kind)}`;
        detail.hidden = false;
        for (const selected of body.querySelectorAll("tr[data-selected]")) {
          selected.removeAttribute("data-selected");
        }
        row.dataset.selected = "true";
      });
      sequenceCell.append(open);
      const timeCell = text("td", formatObservedMicros(event.observedMicros));
      timeCell.title = `${event.observedMicros} observed microseconds`;
      const kindCell = document.createElement("td");
      kindCell.append(
        text("strong", formatIdentifier(event.kind)),
        text("small", formatIdentifier(event.topic)),
      );
      row.append(
        sequenceCell,
        timeCell,
        kindCell,
        text("td", event.summary),
        text("td", event.amount ?? "—"),
      );
      body.append(row);
    }
    table.append(header, body);
    tableWrap.append(table);

    if (page.events.length === 0) {
      tableCard.replaceChildren(
        text(
          "p",
          page.complete
            ? "No canonical events matched these filters."
            : "No matches in this bounded scan window. Continue to scan later events.",
          "runtime-empty-result",
        ),
      );
    } else {
      tableCard.replaceChildren(tableWrap);
    }
    next.disabled = page.complete;
    next.textContent = page.complete ? "End of log" : "Next page";
    message.classList.remove("error");
    message.textContent =
      `Page ${page.pageIndex} · scanned ${page.scannedTotal.toLocaleString()} · ` +
      `${page.matchedTotal.toLocaleString()} matches discovered` +
      (page.integrityVerified ? " · seal verified" : "");
  };

  const loadPage = async (continuation: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    apply.disabled = true;
    next.disabled = true;
    message.classList.remove("error");
    message.textContent = continuation
      ? "Scanning the next bounded event window…"
      : "Verifying the sealed log and applying filters…";
    try {
      const page = parseEventViewerPage(
        await apiJson<unknown>("/api/runtime/events/page", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(
            continuation
              ? {
                  queryId,
                  limit: Number(pageSize.value),
                }
              : {
                  queryId: null,
                  limit: Number(pageSize.value),
                  filter: selectedFilter(),
                },
          ),
        }),
      );
      if (alive) {
        renderPage(page);
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        next.disabled = queryId === null;
      }
    } finally {
      busy = false;
      apply.disabled = false;
    }
  };

  apply.addEventListener("click", () => {
    queryId = null;
    detail.hidden = true;
    liveLog.replaceChildren();
    void loadPage(false);
  });
  next.addEventListener("click", () => void loadPage(true));

  return {
    dispose() {
      alive = false;
      unsubscribeLive();
      if (liveRenderFrame !== null) {
        window.cancelAnimationFrame(liveRenderFrame);
      }
    },
  };
}

function mountSubmissionPolicyOptionsSurface(
  container: HTMLElement,
  section: "log_uploader" | "bpsr_profile_sync",
): MountedSurface {
  let alive = true;
  let current: SubmissionPolicyView | null = null;
  const isUploader = section === "log_uploader";
  const root = document.createElement("div");
  root.className = "plugin-surface submission-policy-surface";
  const heading = actionCard(
    isUploader ? "Log Uploader consent" : "BPSR Profile Sync consent",
    isUploader
      ? "Controls permission to submit sealed combat artifacts. Local drafts continue to be created while this is off, so capture and recovery never depend on a website."
      : "Controls permission to build and submit Blue Protocol: Star Resonance character-profile packages. This is intentionally separate from combat-log consent.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const enable = checkboxOption(
    isUploader ? "Enable Log Uploader" : "Enable BPSR Profile Sync",
    isUploader
      ? "Permits local dry runs now and external submission only after a website transport is implemented."
      : "Reserves permission for profile projection and submission; neither is connected yet.",
  );
  const automatic = checkboxOption(
    isUploader
      ? "Automatically submit completed combat logs"
      : "Automatically sync character profiles",
    isUploader
      ? "Applies only after a real external transport exists. Sealed local drafts are still created either way."
      : "Applies only after a profile projection and external transport exist.",
  );
  form.append(enable.label, automatic.label);

  let visibility: HTMLSelectElement | null = null;
  let retention: HTMLSelectElement | null = null;
  if (isUploader) {
    const visibilityField = selectOption(
      "Default report visibility",
      "Used when a new local draft is created.",
      [
        ["private", "Private"],
        ["unlisted", "Unlisted"],
        ["public", "Public"],
      ],
    );
    visibility = visibilityField.select;
    const retentionField = selectOption(
      "Successful artifact retention",
      "Removal will occur only after a future server receipt is verified; the local mock never deletes artifacts.",
      [
        ["keep", "Keep local artifact"],
        [
          "remove_after_verified_receipt",
          "Remove after verified receipt",
        ],
      ],
    );
    retention = retentionField.select;
    form.append(visibilityField.label, retentionField.label);
  }

  const details = document.createElement("section");
  details.className = "content-card runtime-file-list submission-policy-details";
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions submission-policy-actions";
  const save = button("Save options", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const message = text(
    "span",
    "Loading fail-closed host settings…",
    "runtime-action-message",
  );
  actions.append(save, message);
  form.append(actions);
  root.append(heading, form, details);
  container.append(root);

  const applyView = (view: SubmissionPolicyView) => {
    current = view;
    if (isUploader) {
      enable.input.checked = view.log_uploader.enabled;
      automatic.input.checked = view.log_uploader.automatic_combat_logs;
      if (visibility !== null) {
        visibility.value = view.log_uploader.default_visibility;
      }
      if (retention !== null) {
        retention.value =
          view.log_uploader.successful_artifact_retention;
      }
    } else {
      enable.input.checked = view.bpsr_profile_sync.enabled;
      automatic.input.checked =
        view.bpsr_profile_sync.automatic_profiles;
    }
    details.replaceChildren(
      fileRow("Settings file", view.settings_path),
      fileRow("External transport", formatIdentifier(view.transport_mode)),
      fileRow("Submission endpoint", view.endpoint_url ?? "Not configured"),
      fileRow(
        "Stored settings",
        view.issue === null ? "Valid" : `Fail-closed: ${view.issue}`,
      ),
    );
    message.classList.toggle("error", view.issue !== null);
    message.textContent =
      view.issue ??
      (isUploader
        ? view.transport_mode === "http"
          ? `Verified uploads will use ${view.endpoint_url}.`
          : "Set RLOGS_SUBMISSION_API_URL and restart rLogs to connect a receiver."
        : "Profile projection and external networking remain unavailable in this build.");
    save.disabled = false;
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (current === null) {
      return;
    }
    save.disabled = true;
    message.classList.remove("error");
    message.textContent = "Writing options atomically…";
    const updated: SubmissionPolicy = editableSubmissionPolicy(current);
    if (isUploader && visibility !== null && retention !== null) {
      updated.log_uploader.enabled = enable.input.checked;
      updated.log_uploader.automatic_combat_logs =
        automatic.input.checked;
      updated.log_uploader.default_visibility =
        visibility.value as SubmissionPolicy["log_uploader"]["default_visibility"];
      updated.log_uploader.successful_artifact_retention =
        retention.value as SubmissionPolicy["log_uploader"]["successful_artifact_retention"];
    } else {
      updated.bpsr_profile_sync.enabled = enable.input.checked;
      updated.bpsr_profile_sync.automatic_profiles =
        automatic.input.checked;
    }
    try {
      const view = parseSubmissionPolicy(
        await apiJson<unknown>("/api/submissions/policy", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(updated),
        }),
      );
      if (alive) {
        applyView(view);
        message.classList.remove("error");
        message.textContent = `${isUploader ? "Log Uploader" : "BPSR Profile Sync"} options saved.`;
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        save.disabled = false;
      }
    }
  });

  void apiJson<unknown>("/api/submissions/policy")
    .then(parseSubmissionPolicy)
    .then((view) => {
      if (alive) {
        applyView(view);
      }
    })
    .catch((error: unknown) => {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      }
    });

  return {
    dispose() {
      alive = false;
    },
  };
}

function mountProfileSyncStatusSurface(
  container: HTMLElement,
): MountedSurface {
  let alive = true;
  let busy = false;
  let currentPolicy: SubmissionPolicyView | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface profile-sync-status-surface";
  const heading = actionCard(
    "Reviewable character-profile packages",
    "Profile Sync merges only personal-gameplay BPSR character observations from a fully sealed log. Public lookups for other characters are excluded. Packages contain no website host, credentials, passwords, login tokens, account containers, or transport state.",
  );
  const headingActions = document.createElement("div");
  headingActions.className = "runtime-card-actions";
  const buildButton = button("Build from last sealed log", "primary-button");
  buildButton.disabled = true;
  const refreshButton = button("Refresh packages", "quiet-button");
  const message = text(
    "span",
    "Loading Profile Sync policy and local packages…",
    "runtime-action-message",
  );
  headingActions.append(buildButton, refreshButton, message);
  heading.append(headingActions);
  const status = document.createElement("section");
  status.className = "content-card runtime-file-list";
  status.append(fileRow("Policy", "Loading…"));
  const content = document.createElement("div");
  content.className = "profile-package-content";
  const inspection = document.createElement("section");
  inspection.className = "content-card profile-package-inspection";
  inspection.hidden = true;
  const boundary = actionCard(
    "Current boundary",
    "Projection and local review are implemented. Device pairing, authentication, automatic external submission, and website transport remain disconnected. Building a package makes zero external requests.",
  );
  root.append(heading, status, content, inspection, boundary);
  container.append(root);

  const render = (
    policy: SubmissionPolicyView,
    packages: ProfilePackageStoreView,
  ) => {
    currentPolicy = policy;
    buildButton.disabled = !policy.bpsr_profile_sync.enabled;
    status.replaceChildren(
      fileRow(
        "Profile Sync",
        policy.bpsr_profile_sync.enabled ? "Enabled" : "Disabled",
      ),
      fileRow(
        "Automatic packages",
        policy.bpsr_profile_sync.enabled &&
          policy.bpsr_profile_sync.automatic_profiles
          ? "Enabled after completed real sessions"
          : "Inactive",
      ),
      fileRow("External transport", formatIdentifier(policy.transport_mode)),
      fileRow("Package folder", packages.package_root),
    );

    const metrics = document.createElement("div");
    metrics.className = "runtime-result-grid profile-package-metrics";
    for (const [value, label] of [
      [packages.entry_count.toLocaleString(), "Current profiles"],
      [formatBytes(packages.total_package_bytes), "Local package storage"],
      [packages.issues.length.toLocaleString(), "Store diagnostics"],
    ] as const) {
      const metric = document.createElement("article");
      metric.append(text("strong", value), text("span", label));
      metrics.append(metric);
    }
    const children: HTMLElement[] = [metrics];
    if (packages.issues.length > 0) {
      const diagnostics = document.createElement("section");
      diagnostics.className = "content-card diagnostic-panel";
      diagnostics.append(
        text("h2", "Package diagnostics"),
        text(
          "p",
          "Invalid or tampered files remain excluded from review and future transport.",
        ),
      );
      const list = document.createElement("div");
      list.className = "diagnostic-list";
      for (const issue of packages.issues) {
        const item = document.createElement("article");
        item.className = "diagnostic-item";
        item.append(text("strong", issue));
        list.append(item);
      }
      diagnostics.append(list);
      children.push(diagnostics);
    }
    if (packages.entries.length === 0) {
      children.push(
        text(
          "p",
          policy.bpsr_profile_sync.enabled
            ? "No local profile package exists yet. Complete a world-load capture with Profile Sync active, or build from the last sealed log."
            : "Profile Sync is disabled. Existing packages would remain reviewable, but no new package will be generated.",
          "runtime-empty-result",
        ),
      );
    } else {
      const list = document.createElement("div");
      list.className = "profile-package-list";
      for (const profile of packages.entries) {
        const card = document.createElement("section");
        card.className = "content-card profile-package-entry";
        const header = document.createElement("header");
        const copy = document.createElement("div");
        copy.append(
          text(
            "strong",
            profile.display_name ?? `Character ${profile.character_id}`,
          ),
          text(
            "small",
            `UID ${profile.character_id} · ${profile.realm ?? profile.world ?? profile.region} · ${new Date(profile.created_unix_millis).toLocaleString()}`,
          ),
        );
        const pill = text(
          "span",
          `${profile.profile_field_count.toLocaleString()} fields`,
          "state-pill",
        );
        pill.dataset.state = "ready";
        header.append(copy, pill);
        const details = document.createElement("div");
        details.className = "submission-queue-entry-details";
        details.append(
          fileRow("Package SHA-256", profile.package_id),
          fileRow(
            "Class / specialization",
            `${profile.class_id ?? "—"} / ${profile.specialization_id ?? "—"}`,
          ),
          fileRow("Level", profile.level?.toLocaleString() ?? "—"),
          fileRow(
            "Region",
            `${profile.deployment} / ${profile.region} / ${profile.realm ?? profile.world ?? "unresolved"}`,
          ),
          fileRow(
            "Source evidence",
            `${profile.source_observation_count.toLocaleString()} observations through event ${profile.source_last_event_sequence.toLocaleString()}`,
          ),
          fileRow("Client build", profile.source_client_build),
          fileRow("Source session", profile.source_session_id),
          fileRow("Local package", profile.local_package_path),
        );
        const actions = document.createElement("div");
        actions.className =
          "runtime-card-actions submission-verification-actions";
        const inspectButton = button("Inspect exact JSON", "quiet-button");
        const inspectMessage = text(
          "span",
          `${formatBytes(profile.package_byte_length)} · local review only`,
          "runtime-action-message",
        );
        inspectButton.addEventListener("click", async () => {
          inspectButton.disabled = true;
          inspectMessage.classList.remove("error");
          inspectMessage.textContent = "Re-reading and validating package…";
          try {
            const result = parseProfilePackageInspection(
              await apiJson<unknown>("/api/profiles/packages/inspect", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ packageId: profile.package_id }),
              }),
            );
            if (alive) {
              renderProfilePackageInspection(inspection, result.package);
              inspectMessage.textContent =
                "Stored contract revalidated before display.";
            }
          } catch (error) {
            if (alive) {
              inspectMessage.textContent = errorMessage(error);
              inspectMessage.classList.add("error");
            }
          } finally {
            inspectButton.disabled = false;
          }
        });
        actions.append(inspectButton, inspectMessage);
        card.append(header, details, actions);
        list.append(card);
      }
      children.push(list);
    }
    content.replaceChildren(...children);
    message.classList.toggle("error", policy.issue !== null);
    message.textContent =
      policy.issue ??
      `${packages.entry_count.toLocaleString()} reviewable package${packages.entry_count === 1 ? "" : "s"} · no network activity`;
  };

  const load = async (rescan: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    buildButton.disabled = true;
    message.classList.remove("error");
    message.textContent = "Reading bounded local package metadata…";
    try {
      const [policy, packages] = await Promise.all([
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
        apiJson<unknown>(
          rescan
            ? "/api/profiles/packages/refresh"
            : "/api/profiles/packages",
          rescan ? { method: "POST" } : undefined,
        ).then(parseProfilePackageStore),
      ]);
      if (alive) {
        render(policy, packages);
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        content.replaceChildren(
          text("p", errorMessage(error), "runtime-empty-result"),
        );
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      buildButton.disabled =
        currentPolicy?.bpsr_profile_sync.enabled !== true;
    }
  };

  buildButton.addEventListener("click", async () => {
    if (busy) {
      return;
    }
    busy = true;
    buildButton.disabled = true;
    refreshButton.disabled = true;
    message.classList.remove("error");
    message.textContent =
      "Verifying the last sealed log and merging personal profile observations…";
    try {
      const result = parseProfileProjectionResult(
        await apiJson<unknown>("/api/profiles/project-last", {
          method: "POST",
        }),
      );
      const [policy, packages] = await Promise.all([
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
        apiJson<unknown>("/api/profiles/packages").then(
          parseProfilePackageStore,
        ),
      ]);
      if (alive) {
        render(policy, packages);
        message.classList.remove("error");
        message.textContent =
          result.projected_package_count === 0
            ? `No personal profile observations were present in ${result.source_session_id}; ${result.external_network_requests} external requests.`
            : `Built ${result.projected_package_count.toLocaleString()} package${result.projected_package_count === 1 ? "" : "s"} from ${result.source_session_id}; ${result.external_network_requests} external requests.`;
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      buildButton.disabled =
        currentPolicy?.bpsr_profile_sync.enabled !== true;
    }
  });
  refreshButton.addEventListener("click", () => void load(true));
  void load(false);

  return {
    dispose() {
      alive = false;
    },
  };
}

function renderProfilePackageInspection(
  container: HTMLElement,
  profilePackage: Record<string, unknown>,
) {
  const json = JSON.stringify(profilePackage, null, 2);
  const header = document.createElement("header");
  const copy = document.createElement("div");
  copy.append(
    text("h2", "Exact local profile package"),
    text(
      "p",
      "Raw IDs and privacy-reviewed field names are shown without localization. This is the exact contract a future authenticated transport would receive.",
    ),
  );
  const copyButton = button("Copy JSON", "quiet-button");
  const copyMessage = text(
    "span",
    "Nothing is transmitted.",
    "runtime-action-message",
  );
  copyButton.addEventListener("click", async () => {
    copyButton.disabled = true;
    copyMessage.classList.remove("error");
    try {
      await navigator.clipboard.writeText(json);
      copyMessage.textContent = "Copied exact JSON.";
    } catch (error) {
      copyMessage.textContent = errorMessage(error);
      copyMessage.classList.add("error");
    } finally {
      copyButton.disabled = false;
    }
  });
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  actions.append(copyButton, copyMessage);
  header.append(copy, actions);
  const pre = document.createElement("pre");
  pre.textContent = json;
  container.replaceChildren(header, pre);
  container.hidden = false;
}

function mountSubmissionQueueSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let busy = false;
  let currentPolicy: SubmissionPolicyView | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface submission-queue-surface";
  const heading = actionCard(
    "Local submission drafts",
    "Every real completed session creates one crash-safe local draft after the sealed .rlog passes integrity verification. The dry run below exercises resumable submission locally; no authentication or external network transport exists yet.",
  );
  const headingActions = document.createElement("div");
  headingActions.className = "runtime-card-actions";
  const refreshButton = button("Refresh local queue", "quiet-button");
  const refreshMessage = text(
    "span",
    "Reference fixtures are never queued.",
    "runtime-action-message",
  );
  headingActions.append(refreshButton, refreshMessage);
  heading.append(headingActions);

  const recovery = actionCard(
    "Recover an existing sealed log",
    "Add a previously completed .rlog without its original PCAP. The host streams the whole file once, validates its canonical seal and EOF, calculates exact file and chunk hashes, and only then creates a local draft using the configured default visibility.",
  );
  const importForm = document.createElement("form");
  importForm.className = "runtime-form submission-import-form";
  const artifactPath = field(
    "Existing sealed .rlog",
    "text",
    "",
    "C:\\path\\to\\completed-session.rlog",
  );
  const importActions = document.createElement("div");
  importActions.className = "runtime-card-actions";
  const importButton = button("Verify and add draft", "primary-button");
  importButton.type = "submit";
  const importMessage = text(
    "span",
    "The source file remains local and is not copied or transmitted.",
    "runtime-action-message",
  );
  importActions.append(importButton, importMessage);
  importForm.append(artifactPath.label, importActions);
  recovery.append(importForm);

  const content = document.createElement("div");
  content.className = "submission-queue-content";
  root.append(heading, recovery, content);
  container.append(root);

  async function verifyArtifact(
    queueId: string,
    verifyButton: HTMLButtonElement,
    verifyMessage: HTMLElement,
  ) {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    verifyButton.disabled = true;
    verifyMessage.classList.remove("error");
    verifyMessage.textContent =
      "Streaming and re-verifying the exact local artifact…";
    try {
      const result = parseSubmissionVerificationResult(
        await apiJson<unknown>("/api/submissions/queue/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ queueId }),
        }),
      );
      if (alive) {
        verifyMessage.textContent =
          `Exact file, seal, canonical digest, and ${result.artifact.chunk_count.toLocaleString()} chunk${result.artifact.chunk_count === 1 ? "" : "s"} verified at ` +
          new Date(result.verified_unix_millis).toLocaleString();
      }
    } catch (error) {
      if (alive) {
        verifyMessage.textContent = errorMessage(error);
        verifyMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
      verifyButton.disabled = false;
    }
  }

  async function dryRunSubmission(
    queueId: string,
    dryRunButton: HTMLButtonElement,
    dryRunMessage: HTMLElement,
  ) {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    dryRunButton.disabled = true;
    dryRunMessage.classList.remove("error");
    dryRunMessage.textContent =
      "Re-verifying, chunking, interrupting, and resuming against the local mock receiver…";
    try {
      const result = parseMockSubmissionResult(
        await apiJson<unknown>("/api/submissions/mock/run", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ queueId }),
        }),
      );
      if (alive) {
        dryRunMessage.textContent =
          `Local receipt ${result.report_id}: ${result.chunk_count.toLocaleString()} chunk${result.chunk_count === 1 ? "" : "s"}, ` +
          `${formatBytes(result.uploaded_bytes)}, restart recovered, ${result.external_network_requests} external requests.`;
      }
    } catch (error) {
      if (alive) {
        dryRunMessage.textContent = errorMessage(error);
        dryRunMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
      dryRunButton.disabled = currentPolicy?.log_uploader.enabled !== true;
    }
  }

  async function uploadSubmission(
    queueId: string,
    uploadButton: HTMLButtonElement,
    uploadMessage: HTMLElement,
  ) {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    uploadButton.disabled = true;
    uploadMessage.classList.remove("error");
    uploadMessage.textContent =
      "Re-verifying the sealed artifact and sending only missing chunks...";
    try {
      const result = parseSubmissionTransportResult(
        await apiJson<unknown>("/api/submissions/queue/upload", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ queueId }),
        }),
      );
      if (alive) {
        const link = document.createElement("a");
        link.href = result.share_url;
        link.target = "_blank";
        link.rel = "noreferrer";
        link.textContent = "Open verified parse";
        uploadMessage.replaceChildren(
          `${result.duplicate ? "Existing" : "New"} replayed report ${result.report_id}; `,
          link,
          `. ${result.uploaded_chunk_count.toLocaleString()} chunk${result.uploaded_chunk_count === 1 ? "" : "s"} sent (${formatBytes(result.uploaded_bytes)}).`,
        );
      }
    } catch (error) {
      if (alive) {
        uploadMessage.textContent = errorMessage(error);
        uploadMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
      uploadButton.disabled =
        currentPolicy?.log_uploader.enabled !== true ||
        currentPolicy.transport_mode !== "http";
    }
  }

  const render = (
    queue: SubmissionQueueView,
    policy: SubmissionPolicyView,
  ) => {
    currentPolicy = policy;
    const metrics = document.createElement("div");
    metrics.className = "runtime-result-grid submission-queue-metrics";
    for (const [value, label] of [
      [queue.entry_count.toLocaleString(), "Local drafts"],
      [formatBytes(queue.total_artifact_bytes), "Artifact storage"],
      [queue.issues.length.toLocaleString(), "Queue diagnostics"],
    ] as const) {
      const metric = document.createElement("article");
      metric.append(text("strong", value), text("span", label));
      metrics.append(metric);
    }

    const location = document.createElement("section");
    location.className = "content-card runtime-file-list";
    location.append(
      fileRow("Queue folder", queue.queue_directory),
      fileRow(
        "Transmission",
        policy.log_uploader.enabled
          ? policy.transport_mode === "http"
            ? "Opted in - verified receiver uploads available"
            : "Opted in - receiver not configured"
          : "Disabled - local queue inspection only",
      ),
      fileRow(
        "Default visibility",
        formatIdentifier(policy.log_uploader.default_visibility),
      ),
      fileRow(
        "Transport",
        policy.endpoint_url ?? "Disconnected - 0 external network requests",
      ),
    );

    const children: HTMLElement[] = [metrics, location];
    if (queue.issues.length > 0) {
      const diagnostics = document.createElement("section");
      diagnostics.className = "content-card diagnostic-panel";
      diagnostics.append(
        text("h2", "Queue diagnostics"),
        text(
          "p",
          "Invalid or unreadable queue files are isolated and are not treated as uploadable drafts.",
        ),
      );
      const list = document.createElement("div");
      list.className = "diagnostic-list";
      for (const issue of queue.issues) {
        const item = document.createElement("article");
        item.className = "diagnostic-item";
        item.append(text("strong", issue));
        list.append(item);
      }
      diagnostics.append(list);
      children.push(diagnostics);
    }

    if (queue.entries.length === 0) {
      children.push(
        text(
          "p",
          "No real completed sessions are queued. A sanitized reference replay deliberately does not create a draft.",
          "runtime-empty-result",
        ),
      );
    } else {
      const entries = document.createElement("div");
      entries.className = "submission-queue-list";
      for (const entry of queue.entries) {
        const card = document.createElement("section");
        card.className = "content-card submission-queue-entry";
        const header = document.createElement("header");
        const copy = document.createElement("div");
        copy.append(
          text("strong", entry.capture_session_id),
          text(
            "small",
            `${formatIdentifier(entry.state)} · ${formatIdentifier(entry.visibility)} · ${new Date(entry.created_unix_millis).toLocaleString()}`,
          ),
        );
        const integrity = text(
          "span",
          entry.artifact_exists && entry.artifact_byte_length_matches
            ? "Artifact present"
            : entry.artifact_exists
              ? "Length changed"
              : "Artifact missing",
          "state-pill",
        );
        integrity.dataset.state =
          entry.artifact_exists && entry.artifact_byte_length_matches
            ? "ready"
            : "blocked";
        header.append(copy, integrity);

        const details = document.createElement("div");
        details.className = "submission-queue-entry-details";
        details.append(
          fileRow("Queue ID / file SHA-256", entry.queue_id),
          fileRow("Canonical SHA-256", entry.canonical_content_sha256),
          fileRow(
            "Artifact",
            `${formatBytes(entry.file_byte_length)} in ${entry.chunk_count.toLocaleString()} chunk${entry.chunk_count === 1 ? "" : "s"}`,
          ),
          fileRow(
            "Game / region",
            `${entry.game_plugin_id} / ${entry.game_region}`,
          ),
          fileRow("Client build", entry.client_build),
          fileRow(
            "Local-only path",
            entry.local_artifact_path,
          ),
        );
        const verificationActions = document.createElement("div");
        verificationActions.className =
          "runtime-card-actions submission-verification-actions";
        const verifyButton = button(
          "Re-verify exact artifact",
          "quiet-button",
        );
        const verifyMessage = text(
          "span",
          "Size is a quick diagnostic; full seal and hash verification runs only when requested or immediately before a future upload.",
          "runtime-action-message",
        );
        verifyButton.addEventListener("click", () => {
          void verifyArtifact(entry.queue_id, verifyButton, verifyMessage);
        });
        const dryRunActions = document.createElement("div");
        dryRunActions.className =
          "runtime-card-actions submission-verification-actions";
        const dryRunButton = button(
          "Dry-run resumable upload",
          "primary-button",
        );
        dryRunButton.disabled = !policy.log_uploader.enabled;
        const dryRunMessage = text(
          "span",
          policy.log_uploader.enabled
            ? "Uses the exact upload state machine against an in-process mock receiver, including a forced restart."
            : "Enable Log Uploader in Options to permit this local-only test.",
          "runtime-action-message",
        );
        dryRunButton.addEventListener("click", () => {
          void dryRunSubmission(
            entry.queue_id,
            dryRunButton,
            dryRunMessage,
          );
        });
        const uploadActions = document.createElement("div");
        uploadActions.className =
          "runtime-card-actions submission-verification-actions";
        const uploadButton = button("Submit verified parse", "primary-button");
        uploadButton.disabled =
          !policy.log_uploader.enabled || policy.transport_mode !== "http";
        const uploadMessage = text(
          "span",
          !policy.log_uploader.enabled
            ? "Enable Log Uploader in Options before submitting."
            : policy.transport_mode !== "http"
              ? "A submission receiver has not been configured."
              : `Uploads the sealed artifact to ${policy.endpoint_url}; the receiver replays it before publication.`,
          "runtime-action-message",
        );
        uploadButton.addEventListener("click", () => {
          void uploadSubmission(entry.queue_id, uploadButton, uploadMessage);
        });
        uploadActions.append(uploadButton, uploadMessage);
        dryRunActions.append(dryRunButton, dryRunMessage);
        verificationActions.append(verifyButton, verifyMessage);
        card.append(
          header,
          details,
          verificationActions,
          uploadActions,
          dryRunActions,
        );
        entries.append(card);
      }
      children.push(entries);
    }
    content.replaceChildren(...children);
    refreshMessage.classList.remove("error");
    refreshMessage.textContent =
      `${queue.entry_count.toLocaleString()} local draft${queue.entry_count === 1 ? "" : "s"} - ${policy.log_uploader.enabled ? "uploader enabled" : "uploader disabled"} - ${policy.transport_mode === "http" ? "receiver connected" : "no receiver"}`;
  };

  importForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    importMessage.classList.remove("error");
    importMessage.textContent =
      "Streaming and verifying the existing sealed log…";
    try {
      const result = parseSubmissionImportResult(
        await apiJson<unknown>("/api/submissions/queue/import", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ artifactPath: artifactPath.input.value }),
        }),
      );
      const [queue, policy] = await Promise.all([
        apiJson<unknown>("/api/submissions/queue").then(parseSubmissionQueue),
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
      ]);
      if (alive) {
        render(queue, policy);
        importMessage.textContent =
          result.outcome === "queued"
            ? `Added ${result.capture_session_id} as a verified local draft.`
            : `${result.capture_session_id} is already queued with the same exact file hash.`;
      }
    } catch (error) {
      if (alive) {
        importMessage.textContent = errorMessage(error);
        importMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
    }
  });

  const refresh = async (rescan: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    refreshMessage.classList.remove("error");
    refreshMessage.textContent = "Reading bounded local queue metadata…";
    try {
      const [queue, policy] = await Promise.all([
        apiJson<unknown>(
          rescan
            ? "/api/submissions/queue/refresh"
            : "/api/submissions/queue",
          rescan ? { method: "POST" } : undefined,
        ).then(parseSubmissionQueue),
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
      ]);
      if (alive) {
        render(queue, policy);
      }
    } catch (error) {
      if (alive) {
        refreshMessage.textContent = errorMessage(error);
        refreshMessage.classList.add("error");
        content.replaceChildren(
          text("p", errorMessage(error), "runtime-empty-result"),
        );
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
    }
  };
  refreshButton.addEventListener("click", () => void refresh(true));
  void refresh(false);

  return {
    dispose() {
      alive = false;
    },
  };
}

function runtimeStatusCard(): {
  card: HTMLElement;
  phase: HTMLElement;
  detail: HTMLElement;
  session: HTMLElement;
} {
  const card = document.createElement("section");
  card.className = "content-card runtime-status-card";
  const dot = document.createElement("span");
  dot.className = "runtime-status-dot";
  dot.setAttribute("aria-hidden", "true");
  const copy = document.createElement("div");
  const phase = text("strong", "Loading runtime…");
  const detail = text("p", "Checking the localhost controller.");
  copy.append(phase, detail);
  const session = text("span", "—", "runtime-session-id");
  card.append(dot, copy, session);
  return { card, phase, detail, session };
}

async function refreshStatus(
  status: ReturnType<typeof runtimeStatusCard>,
): Promise<RuntimeSnapshot | null> {
  try {
    const snapshot = await apiJson<RuntimeSnapshot>("/api/runtime/status");
    const monitoring =
      snapshot.phase === "processing" && snapshot.live_capture_can_stop;
    status.card.dataset.phase = monitoring ? "capturing" : snapshot.phase;
    status.phase.textContent = monitoring
      ? snapshot.saving_run
        ? "Monitoring - recording run"
        : "Monitoring"
      : snapshot.phase === "processing"
        ? "Processing requested file"
        : titleCase(snapshot.phase);
    status.detail.textContent = snapshot.detail;
    status.session.textContent =
      snapshot.active_session_id ??
      snapshot.last_result?.session_id ??
      "No session";
    return snapshot;
  } catch (error) {
    status.card.dataset.phase = "failed";
    status.phase.textContent = "Runtime unavailable";
    status.detail.textContent = errorMessage(error);
    status.session.textContent = "Disconnected";
    return null;
  }
}

function renderLastResult(container: HTMLElement, snapshot: RuntimeSnapshot) {
  const result = snapshot.last_result;
  if (result === null) {
    container.replaceChildren(
      text(
        "p",
        snapshot.phase === "processing" && snapshot.live_capture_can_stop
          ? "Continuous monitoring is active. History appears from capture-time projections as soon as a run seals."
          : snapshot.phase === "processing"
          ? "An explicitly requested offline file is being processed."
          : "No completed session yet. Run the safe replay or process an offline capture.",
        "runtime-empty-result",
      ),
    );
    return;
  }
  const metrics = document.createElement("div");
  metrics.className = "runtime-result-grid";
  const values: readonly (readonly [string, string])[] = [
    [String(result.frame_count ?? "—"), "Capture frames"],
    [String(result.framed_record_count ?? "—"), "BPSR records"],
    [String(result.canonical_event_count), "Canonical events"],
    [String(result.known_route_count ?? "—"), "Known routes"],
    [String(result.unknown_route_count ?? "—"), "Unknown routes"],
    [String(result.data_gap_count ?? "—"), "Data gaps"],
    [
      String(result.combat_plugin.metrics.events_delivered),
      "Events delivered",
    ],
    [
      String(result.combat_plugin.metrics.outputs_emitted),
      "Plug-in outputs",
    ],
    [
      String(projectedRunCount(result.encounter_recorder.outputs)),
      "Projected runs",
    ],
    [String(result.profile_package_count), "Profile packages"],
    [
      String(result.encounter_recorder.metrics.events_delivered),
      "Run evidence events",
    ],
    [
      result.upload_artifact === null
        ? "Not built"
        : String(result.upload_artifact.chunk_count),
      "Upload chunks",
    ],
    [
      result.upload_artifact === null
        ? "Not scanned"
        : String(result.upload_artifact.file_byte_length),
      "Artifact bytes",
    ],
  ];
  for (const [value, label] of values) {
    const metric = document.createElement("article");
    metric.append(text("strong", value), text("span", label));
    metrics.append(metric);
  }
  const files = document.createElement("section");
  files.className = "content-card runtime-file-list";
  files.append(
    fileRow("Sealed rlog", result.output_rlog),
    fileRow(
      "Artifact SHA-256",
      result.upload_artifact?.file_sha256 ?? "Validation not requested",
    ),
    fileRow(
      "Canonical SHA-256",
      result.upload_artifact?.canonical_content_sha256 ??
        "Available from the capture-time seal",
    ),
    fileRow("Queue status", formatIdentifier(result.submission_queue_status)),
    fileRow("Profile Sync", formatIdentifier(result.profile_sync_status)),
    fileRow(
      "Queue ID",
      result.submission_queue_id ?? "Not queued",
    ),
    fileRow("Coverage report", result.coverage_report ?? "Not applicable"),
    fileRow("Private capture", result.private_capture ?? "Not applicable"),
    fileRow(
      "Connection evidence",
      result.connection_evidence ?? "Not applicable",
    ),
  );
  container.replaceChildren(metrics, files);
}

function actionCard(title: string, detail: string): HTMLElement {
  const card = document.createElement("section");
  card.className = "content-card runtime-action-card";
  card.append(text("h2", title), text("p", detail));
  return card;
}

interface HistorySpecializationChoice {
  id: string;
  label: string;
  classLabel: string | null;
}

function historyPartyColumnLabel(column: HistoryPartyColumnId): string {
  return {
    player: "Player",
    damage: "Damage",
    effectiveDamage: "Effective damage",
    damageTaken: "Damage taken",
    healing: "Healing",
    effectiveHealing: "Effective healing",
    shielding: "Shielding",
    hits: "Hits",
    criticalRate: "Crit %",
    dps: "DPS",
    encounterDps: "eDPS",
    hps: "HPS",
    tps: "TPS",
    rdps: "rDPS",
    rdpsGiven: "rDPS granted",
    rdpsReceived: "rDPS received",
    apm: "APM",
    deaths: "Deaths",
  }[column];
}

function historyPartyDefaultWidth(column: HistoryPartyColumnId): number {
  if (column === "player") return 360;
  if (["effectiveDamage", "effectiveHealing", "damageTaken", "rdpsReceived", "rdpsGiven"].includes(column)) return 135;
  if (column === "deaths" || column === "criticalRate") return 82;
  return 105;
}

function uniqueHistoryPartyViewId(
  views: readonly HistoryPartyViewSettings[],
  prefix: string,
): string {
  const safePrefix = prefix.toLocaleLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 32) || "view";
  const ids = new Set(views.map((view) => view.id));
  if (!ids.has(safePrefix)) return safePrefix;
  for (let suffix = 2; suffix <= 999; suffix += 1) {
    const id = `${safePrefix}-${suffix}`.slice(0, 40);
    if (!ids.has(id)) return id;
  }
  return `view-${Date.now().toString(36)}`;
}

function historyOptionsGroup(
  title: string,
  detail: string,
  controls: HTMLElement[],
  open = false,
): HTMLDetailsElement {
  const group = document.createElement("details");
  group.className = "history-options-group";
  group.setAttribute("name", "combat-meter-history-options");
  group.open = open;
  const summary = document.createElement("summary");
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  summary.append(copy, text("span", "›", "history-options-group-chevron"));
  const body = document.createElement("div");
  body.className = "history-options-group-body";
  body.append(...controls);
  group.append(summary, body);
  return group;
}

function historySpecializationChoices(
  catalog: CombatHistoryCatalog | null,
  savedColors: Readonly<Record<string, string>>,
): HistorySpecializationChoice[] {
  const choices = new Map<string, HistorySpecializationChoice>();
  for (const entry of catalog?.entries ?? []) {
    for (const participant of entry.participants) {
      if (participant.specialization_id === null) continue;
      const id = String(participant.specialization_id);
      const specialization = participant.presentation_specialization_name?.trim()
        ? compactSpecializationName(participant.presentation_specialization_name)
        : `Specialization ${id}`;
      const classLabel = participant.presentation_class_name?.trim() || null;
      const candidate = {
        id,
        label: specialization,
        classLabel,
      };
      const existing = choices.get(id);
      if (!existing || (existing.label.startsWith("Specialization ") && !candidate.label.startsWith("Specialization "))) {
        choices.set(id, candidate);
      }
    }
  }
  for (const id of Object.keys(savedColors)) {
    if (!choices.has(id)) {
      choices.set(id, {
        id,
        label: `Specialization ${id}`,
        classLabel: null,
      });
    }
  }
  return [...choices.values()].sort((left, right) =>
    `${left.classLabel ?? ""} ${left.label}`.localeCompare(
      `${right.classLabel ?? ""} ${right.label}`,
      undefined,
      { numeric: true },
    ));
}

function renderHistorySpecializationColorControls(
  container: HTMLElement,
  controls: Map<string, HTMLInputElement>,
  choices: HistorySpecializationChoice[],
  savedColors: Readonly<Record<string, string>>,
): void {
  controls.clear();
  container.replaceChildren();
  if (choices.length === 0) {
    container.append(
      text(
        "p",
        "No specialization IDs have been observed in saved History yet.",
        "runtime-empty-result",
      ),
    );
    return;
  }
  for (const choice of choices) {
    const label = document.createElement("label");
    label.className = "history-specialization-color";
    const copy = document.createElement("span");
    copy.append(
      text("strong", choice.label),
      text(
        "small",
        `${choice.classLabel ? `${choice.classLabel} · ` : ""}Spec ID ${choice.id}`,
      ),
    );
    const input = document.createElement("input");
    input.type = "color";
    input.value = savedColors[choice.id] ?? historySpecializationFallbackColor(choice.id);
    input.setAttribute("aria-label", `${choice.label} color`);
    controls.set(choice.id, input);
    label.append(copy, input);
    container.append(label);
  }
}

function field(
  labelText: string,
  type: string,
  value: string,
  placeholder: string,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = document.createElement("label");
  label.className = "runtime-field";
  const name = text("span", labelText);
  const input = document.createElement("input");
  input.type = type;
  input.value = value;
  input.placeholder = placeholder;
  input.autocomplete = "off";
  input.spellcheck = false;
  label.append(name, input);
  return { label, input };
}

function checkboxOption(
  title: string,
  detail: string,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = document.createElement("label");
  label.className = "submission-policy-toggle";
  const input = document.createElement("input");
  input.type = "checkbox";
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  label.append(input, copy);
  return { label, input };
}

function selectOption(
  title: string,
  detail: string,
  options: readonly (readonly [string, string])[],
): { label: HTMLLabelElement; select: HTMLSelectElement } {
  const label = document.createElement("label");
  label.className = "submission-policy-select";
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  const select = document.createElement("select");
  for (const [value, optionLabel] of options) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = optionLabel;
    select.append(option);
  }
  label.append(copy, select);
  return { label, select };
}

function numberOption(
  title: string,
  detail: string,
  minimum: number,
  maximum: number,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = document.createElement("label");
  label.className = "submission-policy-number";
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  const control = document.createElement("span");
  control.className = "submission-policy-number-control";
  const input = document.createElement("input");
  input.type = "number";
  input.min = String(minimum);
  input.max = String(maximum);
  input.step = "1";
  input.inputMode = "numeric";
  control.append(input, text("span", "px"));
  label.append(copy, control);
  return { label, input };
}

function fileRow(label: string, value: string): HTMLElement {
  const row = document.createElement("div");
  row.append(text("span", label), text("code", value));
  return row;
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  value: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tagName);
  node.textContent = value;
  if (className !== undefined) {
    node.className = className;
  }
  return node;
}

async function apiJson<T>(
  route: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetch(route, {
    cache: "no-store",
    headers: { Accept: "application/json", ...init?.headers },
    ...init,
  });
  const body: unknown = await response.json();
  if (!response.ok) {
    const detail =
      isApiError(body) && body.error.trim() !== ""
        ? body.error
        : `Local runtime returned HTTP ${response.status}`;
    throw new Error(detail);
  }
  return body as T;
}

function parseShellPreferences(value: unknown): ShellPreferences {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    !isStringArray(value.workspaceOrder) ||
    !(
      value.activeWorkspaceId === null ||
      typeof value.activeWorkspaceId === "string"
    ) ||
    !isStringRecord(value.activeTabs) ||
    !isStringArrayRecord(value.tabOrders) ||
    !isStringArrayRecord(value.sectionOrders) ||
    typeof value.lockTabDragging !== "boolean" ||
    typeof value.lockSectionDragging !== "boolean"
  ) {
    throw new Error("The local host returned invalid layout settings.");
  }
  return value as unknown as ShellPreferences;
}

function parseCoreSettings(value: unknown): CoreSettings {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    typeof value.closeToTray !== "boolean" ||
    typeof value.hideOverlaysWhenUnfocused !== "boolean" ||
    !isNullableString(value.captureInterface) ||
    !isNullableString(value.dumpcapPath)
  ) {
    throw new Error("The local host returned invalid Core settings.");
  }
  return value as unknown as CoreSettings;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) &&
    value.every((entry) => typeof entry === "string")
  );
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return (
    isRecord(value) &&
    Object.values(value).every((entry) => typeof entry === "string")
  );
}

function isStringArrayRecord(
  value: unknown,
): value is Record<string, readonly string[]> {
  return (
    isRecord(value) &&
    Object.values(value).every((entry) => isStringArray(entry))
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "error" in value &&
    typeof value.error === "string"
  );
}

function emptyToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function defaultSessionId(): string {
  const now = new Date();
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `session-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
}

function titleCase(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function formatIdentifier(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map(titleCase)
    .join(" ");
}

function formatObservedMicros(value: number): string {
  return `${(value / 1_000_000).toFixed(3)}s`;
}

function formatBytes(value: number): string {
  if (value < 1_024) {
    return `${value.toLocaleString()} B`;
  }
  const units = ["KiB", "MiB", "GiB", "TiB"] as const;
  let amount = value / 1_024;
  let index = 0;
  while (amount >= 1_024 && index < units.length - 1) {
    amount /= 1_024;
    index += 1;
  }
  return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${units[index]}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
