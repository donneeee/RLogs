import type { MountedSurface } from "../shell/types";
import type {
  CombatHistoryCatalog,
  CombatHistoryCatalogEntry,
  CombatHistoryDeleteResult,
  CombatHistoryParticipant,
  CombatHistorySnapshot,
  CombatHistoryView,
  CombatRunHistory,
  HistoryDamageInfluenceSummary,
  HistoryAbilitySummary,
  HistoryActorSummary,
  HistoryTargetIdentity,
} from "./combat-history";
import {
  DEFAULT_COMBAT_METER_SETTINGS,
  HISTORY_PARTY_PALETTE,
  type HistoryPartyColumnId,
  type CombatMeterSettings,
  historySeededPaletteColor,
  historySpecializationFallbackColor,
} from "./combat-meter-settings";
import { describeRdpsStatus } from "./rdps-status";

const NUMBER = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
const INTEGER = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const COMPACT = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});
export type GraphMetric = "damage" | "effective_healing" | "damage_taken";
type HistorySort = "newest" | "oldest" | "fastest" | "team_dps" | "team_edps";
type PartySortKey = HistoryPartyColumnId;
type PartySortDirection = "ascending" | "descending";
type AbilitySortKey = "ability" | "damage" | "rdmgReceived" | "rdpsReceived" | "hits" | "casts" | "criticals" | "dps" | "encounterDps" | "healing" | "effectiveHealing" | "shielding" | "hps";
type AbilitySortDirection = "ascending" | "descending";
const HISTORY_PAGE_SIZE = 50;

const PARTY_SORT_COLUMNS: ReadonlyArray<{
  key: PartySortKey;
  label: string;
  numeric: boolean;
}> = [
  { key: "player", label: "Player", numeric: false },
  { key: "damage", label: "Damage", numeric: true },
  { key: "effectiveDamage", label: "Effective damage", numeric: true },
  { key: "damageTaken", label: "Damage taken", numeric: true },
  { key: "healing", label: "Healing", numeric: true },
  { key: "effectiveHealing", label: "Effective healing", numeric: true },
  { key: "shielding", label: "Shielding", numeric: true },
  { key: "hits", label: "Hits", numeric: true },
  { key: "criticalRate", label: "Crit %", numeric: true },
  { key: "dps", label: "DPS", numeric: true },
  { key: "encounterDps", label: "eDPS", numeric: true },
  { key: "hps", label: "HPS", numeric: true },
  { key: "tps", label: "TPS", numeric: true },
  { key: "rdmg", label: "rDMG", numeric: true },
  { key: "rdps", label: "rDPS", numeric: true },
  { key: "rdpsGiven", label: "rDMG granted", numeric: true },
  { key: "rdpsReceived", label: "rDMG received", numeric: true },
  { key: "apm", label: "APM", numeric: true },
  { key: "deaths", label: "Deaths", numeric: true },
];

const ABILITY_SORT_COLUMNS: ReadonlyArray<{
  key: AbilitySortKey;
  label: string;
  numeric: boolean;
}> = [
  { key: "ability", label: "Ability", numeric: false },
  { key: "damage", label: "Damage", numeric: true },
  { key: "rdmgReceived", label: "rDMG gained", numeric: true },
  { key: "rdpsReceived", label: "rDPS gained", numeric: true },
  { key: "hits", label: "Hits", numeric: true },
  { key: "casts", label: "Casts", numeric: true },
  { key: "criticals", label: "Crits", numeric: true },
  { key: "dps", label: "DPS", numeric: true },
  { key: "encounterDps", label: "eDPS", numeric: true },
  { key: "healing", label: "Healing", numeric: true },
  { key: "hps", label: "HPS", numeric: true },
];

const HEALING_ABILITY_SORT_COLUMNS: typeof ABILITY_SORT_COLUMNS = [
  { key: "ability", label: "Ability", numeric: false },
  { key: "healing", label: "Healing", numeric: true },
  { key: "effectiveHealing", label: "Effective healing", numeric: true },
  { key: "shielding", label: "Shielding", numeric: true },
  { key: "casts", label: "Casts", numeric: true },
  { key: "hps", label: "HPS", numeric: true },
];

const GRAPH_METRICS: readonly GraphDefinition[] = [
  {
    metric: "damage",
    title: "Damage timeline",
    rateLabel: "DPS",
    description: "Five-second moving damage rate",
  },
  {
    metric: "effective_healing",
    title: "Healing timeline",
    rateLabel: "HPS",
    description: "Five-second moving effective-healing rate",
  },
  {
    metric: "damage_taken",
    title: "Damage taken timeline",
    rateLabel: "TPS",
    description: "Five-second moving damage-taken rate",
  },
];

interface GraphDefinition {
  metric: GraphMetric;
  title: string;
  rateLabel: string;
  description: string;
}

interface ActorGraphSeries {
  actor: HistoryActorSummary;
  color: string;
  values: number[];
  average: number;
  peak: number;
}

export type CombatHistoryChangeSubscriber = (
  onChange: (update?: CombatHistoryChangeUpdate) => void,
  onError: (error: unknown) => void,
) => () => void;

export type HistoryRdpsRefreshStage =
  | "queued"
  | "waiting_for_live_capture"
  | "replaying"
  | "validating_and_saving"
  | "failed";

export interface HistoryRdpsRefreshProgress {
  session_id: string;
  stage: HistoryRdpsRefreshStage;
  processed_events: number;
  processed_bytes: number;
  total_bytes: number;
  detail?: string;
}

export interface CombatHistoryChangeUpdate {
  catalog_changed: boolean;
  rdps_refreshes: HistoryRdpsRefreshProgress[];
}

export interface CombatHistoryActions {
  setFavorite(
    historyId: string,
    isFavorite: boolean,
  ): Promise<CombatHistoryCatalog>;
  deleteEntries(historyIds: string[]): Promise<CombatHistoryDeleteResult>;
}

export interface IncomingDamageAbilityRow {
  ability: HistoryAbilitySummary;
  damage: number;
  hits: number;
}

export interface IncomingDamageSourceGroup {
  source: HistoryActorSummary | undefined;
  sourceActorId: string;
  sourceEntityUuid: string;
  total: number;
  abilities: IncomingDamageAbilityRow[];
  unattributed: number;
}

export interface RdpsReceivedSourceSummary {
  providerActorId: string;
  providerEntityUuid: string;
  effectId: string;
  attributionComponent: string | null;
  attributedRdps: string | null;
  damageEventCount: number;
  unresolvedRelationshipCount: number;
}

export interface RdpsReceivedSkillSummary {
  abilityId: string | null;
  attributedRdps: string | null;
  damageEventCount: number;
  unresolvedRelationshipCount: number;
  sources: RdpsReceivedSourceSummary[];
}

export interface RdpsGrantedEffectSummary {
  effectId: string;
  attributionComponent: string | null;
  attributedRdps: string | null;
  damageEventCount: number;
  unresolvedRelationshipCount: number;
}

export interface ActorRdpsBreakdown {
  receivedSkills: RdpsReceivedSkillSummary[];
  grantedEffects: RdpsGrantedEffectSummary[];
}

function historyActorByIdentity(
  view: CombatHistoryView,
  actorId: string,
  entityUuid: string,
): HistoryActorSummary | undefined {
  const exact = view.actors.find(
    (candidate) => candidate.actor_id === actorId && candidate.entity_uuid === entityUuid,
  );
  if (exact) return exact;
  const actorIdMatches = view.actors.filter((candidate) => candidate.actor_id === actorId);
  return actorIdMatches.length === 1 ? actorIdMatches[0] : undefined;
}

export function incomingDamageSourceGroups(
  view: CombatHistoryView,
  victim: HistoryActorSummary,
  sourceActorId: string | null,
): IncomingDamageSourceGroup[] {
  return victim.targets
    .filter((target) => sourceActorId === null || target.actor_id === sourceActorId)
    .map((target) => {
      const source = view.actors.find((candidate) => candidate.actor_id === target.actor_id);
      const total = target.series.reduce((sum, point) => sum + point.damage_taken, 0);
      const abilities = (source?.abilities ?? []).flatMap((ability) => {
        const contribution = ability.targets.find(
          (candidate) => candidate.actor_id === victim.actor_id,
        );
        const damage = contribution?.effective_damage ?? 0;
        return damage > 0
          ? [{ ability, damage, hits: contribution?.hits ?? 0 }]
          : [];
      }).sort((left, right) => right.damage - left.damage);
      const attributed = abilities.reduce((sum, entry) => sum + entry.damage, 0);
      return {
        source,
        sourceActorId: target.actor_id,
        sourceEntityUuid: target.entity_uuid,
        total,
        abilities,
        unattributed: Math.max(0, total - attributed),
      };
    })
    .filter((source) => source.total > 0 || source.abilities.length > 0)
    .sort((left, right) => right.total - left.total);
}

export function historyDamageInfluenceMatchesQuery(
  view: CombatHistoryView,
  influence: HistoryDamageInfluenceSummary,
  query: string,
): boolean {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  if (terms.length === 0) return true;

  const provider = historyActorByIdentity(
    view,
    influence.provider_actor_id,
    influence.provider_entity_uuid,
  );
  const recipient = historyActorByIdentity(
    view,
    influence.recipient_actor_id,
    influence.recipient_entity_uuid,
  );
  const target = view.targets.find(
    (candidate) => candidate.actor_id === influence.target_actor_id,
  );
  const effect = historyRdpsEffectPresentation(view, influence.effect_id);
  const ability = recipient?.abilities.find(
    (candidate) => candidate.ability_id === influence.affected_ability_id,
  );
  const searchable = [
    "effect",
    influence.effect_id,
    effect?.presentation_name,
    "component",
    influence.attribution_component,
    "provider",
    influence.provider_actor_id,
    influence.provider_entity_uuid,
    provider?.character_id,
    provider?.display_name,
    provider?.presentation_name,
    provider?.presentation_class_name,
    provider?.presentation_specialization_name,
    "recipient",
    influence.recipient_actor_id,
    influence.recipient_entity_uuid,
    recipient?.character_id,
    recipient?.display_name,
    recipient?.presentation_name,
    recipient?.presentation_class_name,
    recipient?.presentation_specialization_name,
    "skill ability",
    influence.affected_ability_id,
    ability?.presentation_name,
    "target",
    influence.target_actor_id,
    influence.target_entity_uuid,
    target?.monster_id,
    target?.display_name,
    target?.presentation_name,
  ].filter((value): value is string => value !== null && value !== undefined)
    .join(" ")
    .toLocaleLowerCase();
  return terms.every((term) => searchable.includes(term));
}

export function historyRdpsEffectPresentation(
  view: CombatHistoryView,
  effectId: string,
) {
  return view.rdps_effect_presentations?.find(
    (candidate) => candidate.effect_id === effectId,
  ) ?? view.actors
    .flatMap((candidate) => candidate.effects)
    .find((candidate) => candidate.effect_id === effectId);
}

export function actorRdpsBreakdown(
  view: CombatHistoryView,
  actorId: string,
  targetActorId: string | null = null,
): ActorRdpsBreakdown {
  type ExactAccumulator = {
    exactTotal: bigint;
    hasExact: boolean;
    damageEventCount: number;
    unresolvedRelationshipCount: number;
  };
  type ReceivedSkillAccumulator = ExactAccumulator & {
    abilityId: string | null;
    sources: Map<string, RdpsReceivedSourceSummary & { exactTotal: bigint; hasExact: boolean }>;
  };
  type GrantedEffectAccumulator = ExactAccumulator & {
    effectId: string;
    attributionComponent: string | null;
  };

  const received = new Map<string, ReceivedSkillAccumulator>();
  const granted = new Map<string, GrantedEffectAccumulator>();
  const addExact = (accumulator: ExactAccumulator, influence: HistoryDamageInfluenceSummary) => {
    accumulator.damageEventCount += influence.damage_event_count;
    if (influence.attributed_rdps === null) {
      accumulator.unresolvedRelationshipCount += 1;
      return;
    }
    accumulator.exactTotal += BigInt(influence.attributed_rdps);
    accumulator.hasExact = true;
  };

  for (const influence of view.damage_influences ?? []) {
    if (targetActorId !== null && influence.target_actor_id !== targetActorId) continue;
    if (influence.recipient_actor_id === actorId) {
      const abilityKey = influence.affected_ability_id ?? "\u0000";
      let skill = received.get(abilityKey);
      if (!skill) {
        skill = {
          abilityId: influence.affected_ability_id,
          exactTotal: 0n,
          hasExact: false,
          damageEventCount: 0,
          unresolvedRelationshipCount: 0,
          sources: new Map(),
        };
        received.set(abilityKey, skill);
      }
      addExact(skill, influence);
      const sourceKey = [
        influence.provider_actor_id,
        influence.provider_entity_uuid,
        influence.effect_id,
        influence.attribution_component ?? "",
      ].join("\u001f");
      let source = skill.sources.get(sourceKey);
      if (!source) {
        source = {
          providerActorId: influence.provider_actor_id,
          providerEntityUuid: influence.provider_entity_uuid,
          effectId: influence.effect_id,
          attributionComponent: influence.attribution_component ?? null,
          attributedRdps: null,
          exactTotal: 0n,
          hasExact: false,
          damageEventCount: 0,
          unresolvedRelationshipCount: 0,
        };
        skill.sources.set(sourceKey, source);
      }
      addExact(source, influence);
    }
    if (influence.provider_actor_id === actorId) {
      const component = influence.attribution_component ?? null;
      const effectKey = `${influence.effect_id}\u001f${component ?? ""}`;
      let effect = granted.get(effectKey);
      if (!effect) {
        effect = {
          effectId: influence.effect_id,
          attributionComponent: component,
          exactTotal: 0n,
          hasExact: false,
          damageEventCount: 0,
          unresolvedRelationshipCount: 0,
        };
        granted.set(effectKey, effect);
      }
      addExact(effect, influence);
    }
  }

  const exactValue = (entry: ExactAccumulator): string | null =>
    entry.hasExact ? entry.exactTotal.toString() : null;
  const compareExact = (left: ExactAccumulator, right: ExactAccumulator): number => {
    if (left.hasExact !== right.hasExact) return left.hasExact ? -1 : 1;
    if (left.exactTotal === right.exactTotal) return 0;
    return left.exactTotal > right.exactTotal ? -1 : 1;
  };
  const receivedSkills = [...received.values()]
    .sort((left, right) => compareExact(left, right) ||
      (left.abilityId ?? "").localeCompare(right.abilityId ?? "", undefined, { numeric: true }))
    .map((skill): RdpsReceivedSkillSummary => ({
      abilityId: skill.abilityId,
      attributedRdps: exactValue(skill),
      damageEventCount: skill.damageEventCount,
      unresolvedRelationshipCount: skill.unresolvedRelationshipCount,
      sources: [...skill.sources.values()]
        .sort((left, right) => compareExact(left, right) ||
          left.providerActorId.localeCompare(right.providerActorId, undefined, { numeric: true }))
        .map((source) => ({
          providerActorId: source.providerActorId,
          providerEntityUuid: source.providerEntityUuid,
          effectId: source.effectId,
          attributionComponent: source.attributionComponent,
          attributedRdps: exactValue(source),
          damageEventCount: source.damageEventCount,
          unresolvedRelationshipCount: source.unresolvedRelationshipCount,
        })),
    }));
  const grantedEffects = [...granted.values()]
    .sort((left, right) => compareExact(left, right) ||
      left.effectId.localeCompare(right.effectId, undefined, { numeric: true }))
    .map((effect): RdpsGrantedEffectSummary => ({
      effectId: effect.effectId,
      attributionComponent: effect.attributionComponent,
      attributedRdps: exactValue(effect),
      damageEventCount: effect.damageEventCount,
      unresolvedRelationshipCount: effect.unresolvedRelationshipCount,
    }));
  return { receivedSkills, grantedEffects };
}

export function mountCombatHistorySurface(
  container: HTMLElement,
  loadCatalog: () => Promise<CombatHistoryCatalog>,
  loadDetail: (sessionId: string) => Promise<CombatHistorySnapshot>,
  loadSettings: () => Promise<CombatMeterSettings> = async () =>
    DEFAULT_COMBAT_METER_SETTINGS,
  subscribeCatalogChanges?: CombatHistoryChangeSubscriber,
  actions?: CombatHistoryActions,
): MountedSurface {
  let alive = true;
  let catalog: CombatHistoryCatalog | null = null;
  let selectedEntry: CombatHistoryCatalogEntry | null = null;
  let detail: CombatHistorySnapshot | null = null;
  let viewId = "all";
  let targetActorId: string | null = null;
  let detailActorId: string | null = null;
  let hiddenGraphActors = new Set<string>();
  let graphMetric: GraphMetric = "damage";
  let settings = DEFAULT_COMBAT_METER_SETTINGS;
  let browserQuery = "";
  let browserDifficulty = "all";
  let browserSort: HistorySort = "newest";
  let browserPage = 0;
  let browserFavoritesOnly = false;
  let selectedHistoryIds = new Set<string>();
  let deleteConfirmationOpen = false;
  let historyMutationInFlight = false;
  let partySortKey: PartySortKey = "encounterDps";
  let partySortDirection: PartySortDirection = "descending";
  let historyPartyViewId = DEFAULT_COMBAT_METER_SETTINGS.historyPartyViews[0]!.id;
  let abilitySortKey: AbilitySortKey = "damage";
  let abilitySortDirection: AbilitySortDirection = "descending";
  let influenceQuery = "";
  let expandedInfluenceActorId: string | null = null;
  let collapsedRecountGroups = new Set<string>();
  let loadInFlight: Promise<void> | null = null;
  let reloadAfterCurrent = false;
  let reloadDetailAfterCurrent = false;
  let rdpsRefreshes = new Map<string, HistoryRdpsRefreshProgress>();
  let unsubscribeCatalogChanges = () => {};

  const root = element("div", "plugin-surface combat-history-surface");
  applyHistorySizing(root, settings);
  const status = element("span", "combat-history-live-status", "Loading run history…");
  status.setAttribute("aria-live", "polite");
  const content = element("div", "combat-history-content");
  content.append(element("p", "runtime-empty-result", "Loading saved dungeon runs…"));
  root.append(status, content);
  container.append(root);

  const load = (
    includeSettings = false,
    reloadSelectedDetail = false,
  ): Promise<void> => {
    if (loadInFlight) {
      reloadAfterCurrent = true;
      reloadDetailAfterCurrent ||= reloadSelectedDetail;
      return loadInFlight;
    }
    loadInFlight = (async () => {
      status.textContent = "Reading the lightweight history index…";
      try {
        const loadedCatalog = await loadCatalog();
        const loadedSettings = includeSettings ? await loadSettings() : settings;
        if (!alive) return;
        const retained = selectedEntry
          ? loadedCatalog.entries.find((entry) => entry.history_id === selectedEntry?.history_id)
          : null;
        catalog = loadedCatalog;
        const availableHistoryIds = new Set(
          loadedCatalog.entries.map((entry) => entry.history_id),
        );
        selectedHistoryIds = new Set(
          [...selectedHistoryIds].filter((historyId) =>
            availableHistoryIds.has(historyId),
          ),
        );
        settings = loadedSettings;
        if (!settings.historyPartyViews.some((view) => view.id === historyPartyViewId)) {
          historyPartyViewId = settings.historyPartyViews[0]!.id;
        }
        applyHistorySizing(root, settings);
        if (catalog.entries.length === 0) {
          selectedEntry = null;
          detail = null;
          detailActorId = null;
        } else if (retained) {
          selectedEntry = retained;
          if (reloadSelectedDetail) {
            detail = await loadDetail(retained.session_id);
            if (!alive) return;
          }
        } else {
          selectedEntry = null;
          detail = null;
          detailActorId = null;
        }
        render();
        status.textContent = catalog.entries.length === 0
          ? "No completed run history is indexed yet."
          : `${catalog.entries.length.toLocaleString()} run(s) indexed`;
      } catch (error) {
        if (!alive) return;
        status.textContent = errorMessage(error);
        if (catalog === null) {
          content.replaceChildren(
            element("p", "runtime-empty-result", "Combat history could not be loaded."),
          );
        }
      } finally {
        loadInFlight = null;
        if (reloadAfterCurrent && alive) {
          const reloadDetail = reloadDetailAfterCurrent;
          reloadAfterCurrent = false;
          reloadDetailAfterCurrent = false;
          void load(false, reloadDetail);
        }
      }
    })();
    return loadInFlight;
  };

  const selectEntry = async (entry: CombatHistoryCatalogEntry) => {
    selectedEntry = entry;
    viewId = "all";
    targetActorId = null;
    detailActorId = null;
    hiddenGraphActors = new Set();
    detail = await loadDetail(entry.session_id);
    if (alive) render();
  };

  const render = () => {
    if (!catalog || catalog.entries.length === 0) {
      content.replaceChildren(
        element(
          "p",
          "runtime-empty-result",
          "Complete a dungeon after this history version is installed. The run will appear here immediately when its completion packet seals it.",
        ),
      );
      return;
    }
    if (detailActorId !== null && settings.playerDetailPresentation === "in_app_layer") {
      content.replaceChildren(renderPlayerLayer());
      return;
    }
    if (selectedEntry && detail) {
      content.replaceChildren(renderSelectedRun());
      return;
    }

    const entries = filterAndSortHistoryEntries(
      catalog.entries,
      browserQuery,
      browserDifficulty,
      browserSort,
      browserFavoritesOnly,
    );
    const pageCount = Math.max(1, Math.ceil(entries.length / HISTORY_PAGE_SIZE));
    browserPage = Math.min(browserPage, pageCount - 1);
    const pageEntries = entries.slice(
      browserPage * HISTORY_PAGE_SIZE,
      (browserPage + 1) * HISTORY_PAGE_SIZE,
    );
    const runBrowser = element("section", "combat-history-run-browser");
    runBrowser.append(
      element(
        "div",
        "card-heading combat-history-browser-heading",
        element(
          "div",
          "",
          element("h2", "", "Past dungeon runs"),
          element("p", "card-copy", "Search the compact index, then open one run for its complete breakdown."),
        ),
        element("span", "state-pill", `${entries.length.toLocaleString()} of ${catalog.entries.length.toLocaleString()}`),
      ),
    );

    const toolbar = element("div", "combat-history-browser-toolbar");
    const search = document.createElement("input");
    search.type = "search";
    search.value = browserQuery;
    search.placeholder = "Search dungeon, player, UID, region…";
    search.setAttribute("aria-label", "Search saved dungeon runs");
    search.addEventListener("input", () => {
      browserQuery = search.value;
      browserPage = 0;
      render();
      requestAnimationFrame(() => {
        const next = content.querySelector<HTMLInputElement>(
          ".combat-history-browser-toolbar input[type='search']",
        );
        next?.focus();
        next?.setSelectionRange(browserQuery.length, browserQuery.length);
      });
    });
    const difficulty = selectControl(
      "Difficulty",
      [["all", "All difficulties"], ...uniqueDifficultyFilters(catalog.entries)],
      browserDifficulty,
      (value) => {
        browserDifficulty = value;
        browserPage = 0;
        render();
      },
    );
    const sort = selectControl(
      "Sort runs",
      [
        ["newest", "Newest first"],
        ["oldest", "Oldest first"],
        ["fastest", "Fastest run"],
        ["team_dps", "Highest team DPS"],
        ["team_edps", "Highest team eDPS"],
      ],
      browserSort,
      (value) => {
        browserSort = value as HistorySort;
        browserPage = 0;
        render();
      },
    );
    const favoriteFilter = button(
      browserFavoritesOnly ? "★ Favorites" : "☆ Favorites",
      "quiet-button combat-history-favorite-filter",
    );
    favoriteFilter.dataset.selected = String(browserFavoritesOnly);
    favoriteFilter.setAttribute("aria-pressed", String(browserFavoritesOnly));
    favoriteFilter.addEventListener("click", () => {
      browserFavoritesOnly = !browserFavoritesOnly;
      browserPage = 0;
      render();
    });
    toolbar.append(search, difficulty, sort, favoriteFilter);
    runBrowser.append(toolbar);

    const selectedEntries = catalog.entries.filter((entry) =>
      selectedHistoryIds.has(entry.history_id),
    );
    if (selectedEntries.length > 0) {
      const protectedCount = selectedEntries.filter((entry) => entry.is_favorite).length;
      const deletableCount = selectedEntries.length - protectedCount;
      const clearSelection = button("Clear selection", "quiet-button");
      clearSelection.disabled = historyMutationInFlight;
      clearSelection.addEventListener("click", () => {
        selectedHistoryIds = new Set();
        deleteConfirmationOpen = false;
        render();
      });
      const deleteSelected = button(
        `Delete selected${deletableCount > 0 ? ` (${deletableCount})` : ""}`,
        "quiet-button combat-history-delete-selected",
      );
      deleteSelected.disabled =
        historyMutationInFlight || deletableCount === 0 || !actions;
      deleteSelected.addEventListener("click", () => {
        deleteConfirmationOpen = true;
        render();
      });
      runBrowser.append(
        element(
          "div",
          "combat-history-selection-bar",
          element(
            "div",
            "",
            element("strong", "", `${selectedEntries.length} selected`),
            protectedCount > 0
              ? element(
                  "small",
                  "",
                  `${protectedCount} favorite${protectedCount === 1 ? " is" : "s are"} protected from deletion.`,
                )
              : element("small", "", "Select runs across pages, then delete them together."),
          ),
          element("div", "combat-history-selection-actions", clearSelection, deleteSelected),
        ),
      );
    }

    if (pageEntries.length === 0) {
      runBrowser.append(element("p", "runtime-empty-result", "No saved runs match these filters."));
      appendDeleteConfirmation(runBrowser);
      content.replaceChildren(runBrowser);
      return;
    }

    const tableScroll = element("div", "combat-history-run-table-scroll");
    const runList = element("div", "combat-history-run-list");
    runList.setAttribute("role", "table");
    const pageHistoryIds = pageEntries.map((entry) => entry.history_id);
    const selectedOnPage = pageHistoryIds.filter((historyId) =>
      selectedHistoryIds.has(historyId),
    ).length;
    const selectPage = document.createElement("input");
    selectPage.type = "checkbox";
    selectPage.className = "combat-history-selection-checkbox";
    selectPage.checked = pageEntries.length > 0 && selectedOnPage === pageEntries.length;
    selectPage.indeterminate = selectedOnPage > 0 && selectedOnPage < pageEntries.length;
    selectPage.setAttribute("aria-label", "Select all runs on this page");
    selectPage.addEventListener("change", () => {
      const next = new Set(selectedHistoryIds);
      for (const historyId of pageHistoryIds) {
        if (selectPage.checked) next.add(historyId);
        else next.delete(historyId);
      }
      selectedHistoryIds = next;
      render();
    });
    runList.append(
      element(
        "div",
        "combat-history-run-row combat-history-run-row-heading",
        element("span", "combat-history-run-column-select", selectPage),
        element("span", "combat-history-run-column-favorite", "Fav."),
        element("span", "combat-history-run-column-dungeon", "Dungeon"),
        element("span", "combat-history-run-column-party", "Party"),
        element("span", "combat-history-run-column-metric", "Team DPS"),
        element("span", "combat-history-run-column-metric", "Team eDPS"),
        element("span", "combat-history-run-column-metric", "Run time"),
        element("span", "combat-history-run-column-recorded", "Recorded"),
        element("span", "combat-history-run-column-open", ""),
      ),
    );
    for (const entry of pageEntries) {
      const item = element("div", "combat-history-run-row combat-history-run-button");
      item.setAttribute("role", "row");
      item.tabIndex = 0;
      item.dataset.selected = String(selectedHistoryIds.has(entry.history_id));
      item.dataset.favorite = String(entry.is_favorite);
      item.dataset.terminalState = entry.terminal_state;
      const selection = document.createElement("input");
      selection.type = "checkbox";
      selection.className = "combat-history-selection-checkbox";
      selection.checked = selectedHistoryIds.has(entry.history_id);
      selection.setAttribute("aria-label", `Select ${activityLabel(entry)}`);
      selection.addEventListener("click", (event) => event.stopPropagation());
      selection.addEventListener("change", () => {
        const next = new Set(selectedHistoryIds);
        if (selection.checked) next.add(entry.history_id);
        else next.delete(entry.history_id);
        selectedHistoryIds = next;
        render();
      });
      const favorite = button(
        entry.is_favorite ? "★" : "☆",
        "combat-history-favorite-button",
      );
      favorite.dataset.favorite = String(entry.is_favorite);
      favorite.setAttribute("aria-pressed", String(entry.is_favorite));
      favorite.setAttribute(
        "aria-label",
        `${entry.is_favorite ? "Remove" : "Add"} ${activityLabel(entry)} ${
          entry.is_favorite ? "from" : "to"
        } favorites`,
      );
      favorite.disabled = historyMutationInFlight || !actions;
      favorite.addEventListener("click", (event) => {
        event.stopPropagation();
        if (!actions || historyMutationInFlight) return;
        historyMutationInFlight = true;
        status.classList.remove("error");
        status.textContent = entry.is_favorite
          ? "Removing favorite…"
          : "Saving favorite…";
        render();
        void actions
          .setFavorite(entry.history_id, !entry.is_favorite)
          .then((updatedCatalog) => {
            if (!alive) return;
            catalog = updatedCatalog;
            status.textContent = `${updatedCatalog.entries.length.toLocaleString()} run(s) indexed`;
          })
          .catch((error) => {
            if (!alive) return;
            status.classList.add("error");
            status.textContent = errorMessage(error);
          })
          .finally(() => {
            historyMutationInFlight = false;
            if (alive) render();
          });
      });
      const runTime = element(
        "span",
        "combat-history-run-metric",
        formatDuration(entry.total_run_time_micros ?? entry.game_time_micros),
      );
      runTime.title = entry.total_run_time_micros === null
        ? "Legacy index: pruned game time"
        : "Total run time: instance entry to boss completion";
      item.append(
        element("span", "combat-history-run-column-select", selection),
        element("span", "combat-history-run-column-favorite", favorite),
        element(
          "span",
          "combat-history-run-identity",
          element("strong", "", activityLabel(entry)),
          element(
            "small",
            "",
            runStatusLabel(entry, entry.terminal_state, entry.retry_count),
          ),
        ),
        renderCatalogParty(entry.participants, entry.player_count, settings),
        metricValue(entry.team_dps),
        metricValue(entry.team_encounter_dps),
        runTime,
        element(
          "span",
          "combat-history-run-recorded",
          element("strong", "", formatCalendarDate(entry.captured_unix_millis)),
          element("small", "", formatTimestamp(entry.captured_unix_millis)),
        ),
        element("span", "combat-history-run-open", "›"),
      );
      item.addEventListener("click", () => {
        status.textContent = "Loading saved run detail…";
        void selectEntry(entry)
          .then(() => {
            status.textContent = `${catalog?.entries.length ?? 0} run(s) indexed`;
          })
          .catch((error) => {
            status.textContent = errorMessage(error);
            status.classList.add("error");
          });
      });
      item.addEventListener("keydown", (event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        item.click();
      });
      runList.append(item);
    }
    tableScroll.append(runList);
    runBrowser.append(tableScroll);
    if (pageCount > 1) {
      const pagination = element("div", "combat-history-pagination");
      const previous = button("Previous", "quiet-button");
      previous.disabled = browserPage === 0;
      previous.addEventListener("click", () => {
        browserPage = Math.max(0, browserPage - 1);
        render();
      });
      const next = button("Next", "quiet-button");
      next.disabled = browserPage + 1 >= pageCount;
      next.addEventListener("click", () => {
        browserPage = Math.min(pageCount - 1, browserPage + 1);
        render();
      });
      pagination.append(
        previous,
        element("span", "", `Page ${browserPage + 1} of ${pageCount}`),
        next,
      );
      runBrowser.append(pagination);
    }
    appendDeleteConfirmation(runBrowser);
    content.replaceChildren(runBrowser);
  };

  function appendDeleteConfirmation(owner: HTMLElement): void {
    if (!deleteConfirmationOpen || !catalog) return;
    const selectedEntries = catalog.entries.filter((entry) =>
      selectedHistoryIds.has(entry.history_id),
    );
    const protectedCount = selectedEntries.filter((entry) => entry.is_favorite).length;
    const deletableCount = selectedEntries.length - protectedCount;
    if (selectedEntries.length === 0) {
      deleteConfirmationOpen = false;
      return;
    }

    const cancel = button("Cancel", "quiet-button");
    cancel.disabled = historyMutationInFlight;
    cancel.addEventListener("click", () => {
      deleteConfirmationOpen = false;
      render();
    });
    const confirm = button(
      `Delete ${deletableCount} run${deletableCount === 1 ? "" : "s"}`,
      "quiet-button combat-history-delete-selected",
    );
    confirm.disabled = historyMutationInFlight || deletableCount === 0 || !actions;
    confirm.addEventListener("click", () => {
      if (!actions || historyMutationInFlight || deletableCount === 0) return;
      const requestedIds = [...selectedHistoryIds];
      historyMutationInFlight = true;
      status.classList.remove("error");
      status.textContent = `Deleting ${deletableCount} saved run${deletableCount === 1 ? "" : "s"}…`;
      render();
      void actions
        .deleteEntries(requestedIds)
        .then(async (result) => {
          const updatedCatalog = await loadCatalog();
          if (!alive) return;
          catalog = updatedCatalog;
          selectedHistoryIds = new Set();
          deleteConfirmationOpen = false;
          const notes = [
            `${result.deleted_count} run${result.deleted_count === 1 ? "" : "s"} deleted`,
          ];
          if (result.preserved_favorite_count > 0) {
            notes.push(
              `${result.preserved_favorite_count} favorite${
                result.preserved_favorite_count === 1 ? "" : "s"
              } preserved`,
            );
          }
          if (result.cleanup_warnings.length > 0) {
            notes.push(`${result.cleanup_warnings.length} storage cleanup warning${
              result.cleanup_warnings.length === 1 ? "" : "s"
            }`);
          }
          status.textContent = notes.join(" · ");
        })
        .catch((error) => {
          if (!alive) return;
          status.classList.add("error");
          status.textContent = errorMessage(error);
        })
        .finally(() => {
          historyMutationInFlight = false;
          if (alive) render();
        });
    });

    const backdrop = element("div", "combat-history-delete-backdrop");
    backdrop.addEventListener("click", (event) => {
      if (event.target !== backdrop || historyMutationInFlight) return;
      deleteConfirmationOpen = false;
      render();
    });
    const dialog = element(
      "section",
      "combat-history-delete-dialog",
      element("h2", "", "Delete selected history?"),
      element(
        "p",
        "",
        `${deletableCount} saved run${deletableCount === 1 ? "" : "s"} will be permanently removed from this device.`,
      ),
    );
    if (protectedCount > 0) {
      dialog.append(
        element(
          "p",
          "combat-history-delete-protected",
          `${protectedCount} favorited run${protectedCount === 1 ? " is" : "s are"} protected and will remain in history.`,
        ),
      );
    }
    dialog.append(
      element("div", "combat-history-delete-actions", cancel, confirm),
    );
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");
    dialog.setAttribute("aria-label", "Confirm deletion of selected combat history");
    backdrop.append(dialog);
    owner.append(backdrop);
  }

  const renderPageNavigation = (
    backLabel: string,
    currentLabel: string,
    onBack: () => void,
    metadata: ReadonlyArray<readonly [string, string]> = [],
  ): HTMLElement => {
    const back = button(backLabel, "quiet-button combat-history-back-button");
    back.addEventListener("click", onBack);
    const context = element(
      "div",
      "combat-history-page-context",
      element("span", "combat-history-page-current", currentLabel),
    );
    if (metadata.length > 0) {
      context.append(
        element(
          "div",
          "combat-history-page-metadata",
          ...metadata.map(([label, value]) =>
            element(
              "span",
              "combat-history-page-metric",
              element("small", "", label),
              element("strong", "", value),
            ),
          ),
        ),
      );
    }
    const navigation = element(
      "nav",
      "combat-history-page-navigation",
      back,
      context,
    );
    navigation.dataset.sticky = String(metadata.length > 0);
    navigation.setAttribute("aria-label", "Combat history navigation");
    return navigation;
  };

  const renderHistoryFilters = (
    run: CombatRunHistory,
    view: CombatHistoryView,
  ): HTMLElement => {
    const filters = element("section", "content-card combat-history-filters");
    const viewButtons = element("div", "combat-history-filter-tabs");
    const primaryViews = run.views.filter((candidate) => candidate.kind !== "retry");
    const retryViews = run.views.filter((candidate) => candidate.kind === "retry");
    for (const candidate of primaryViews) {
      const tab = button(candidate.label, "combat-history-filter-button");
      tab.dataset.selected = String(candidate.id === view.id);
      tab.addEventListener("click", () => {
        viewId = candidate.id;
        render();
      });
      viewButtons.append(tab);
    }
    if (retryViews.length > 0) {
      const retrySelect = document.createElement("select");
      retrySelect.className = "combat-history-retry-select";
      retrySelect.setAttribute("aria-label", "Boss retry attempt");
      retrySelect.append(new Option("Retries", ""));
      for (const retryView of retryViews) {
        retrySelect.append(new Option(retryView.label, retryView.id));
      }
      retrySelect.value = view.kind === "retry" ? view.id : "";
      retrySelect.addEventListener("change", () => {
        if (!retrySelect.value) {
          return;
        }
        viewId = retrySelect.value;
        render();
      });
      viewButtons.append(retrySelect);
    }
    const targetSelect = document.createElement("select");
    targetSelect.className = "combat-history-target-select";
    targetSelect.append(new Option("All targets", ""));
    for (const target of view.targets) {
      targetSelect.append(new Option(historyTargetLabel(target), target.actor_id));
    }
    targetSelect.value = targetActorId ?? "";
    targetSelect.addEventListener("change", () => {
      targetActorId = targetSelect.value || null;
      render();
    });
    filters.append(
      element("div", "combat-history-segment-controls", element("span", "field-label", "Segment"), viewButtons),
      element("label", "combat-history-target-filter", element("span", "field-label", "Target entity"), targetSelect),
    );
    return filters;
  };

  const renderStickyHistoryContext = (
    navigation: HTMLElement,
    filters: HTMLElement,
  ): HTMLElement => element(
    "section",
    "combat-history-sticky-context",
    navigation,
    filters,
  );

  const renderSelectedRun = (): HTMLElement => {
    const pane = element("div", "combat-history-detail");
    if (!selectedEntry || !detail) {
      pane.append(element("p", "runtime-empty-result", "Select a dungeon run."));
      return pane;
    }
    const run = detail.runs.find((run) => run.run_index === selectedEntry?.run_index);
    if (!run) {
      pane.append(element("p", "runtime-empty-result", "The selected run is missing from its detail artifact."));
      return pane;
    }
    const view = run.views.find((view) => view.id === viewId) ?? run.views[0];
    if (!view) {
      pane.append(element("p", "runtime-empty-result", "This run has no combat views."));
      return pane;
    }
    viewId = view.id;
    if (targetActorId && !view.targets.some((target) => target.actor_id === targetActorId)) {
      targetActorId = null;
    }
    const entireRun = run.views.find((candidate) => candidate.id === "all") ?? view;
    const trueTime = run.views.find((candidate) => candidate.id === "true_time");

    const summary = element("section", "content-card combat-history-run-summary");
    summary.append(
      element("div", "combat-history-title",
        element("div", "",
          element("span", "run-report-kicker", `${detail.region_id} · Scene ${run.scene_id ?? "?"}`),
          element("h2", "", activityLabel(run)),
          element("p", "", runStatusLabel(run, run.terminal_state)),
        ),
        element("span", "state-pill", "Saved locally"),
      ),
      metricGrid([
        [
          formatDuration(run.total_run_time_micros ?? totalRunTime(run)),
          "Total run: entry → completion",
        ],
        [formatDuration(run.game_time_micros), "Game time: reviewed intervals"],
        [formatDuration(entireRun.active_combat_micros), "Active combat / eDPS"],
        [formatDuration(run.true_time_micros ?? trueTime?.elapsed_micros ?? null), "True Time: projected best"],
        [
          `${run.retry_count} total · ${run.boss_retry_count} boss`,
          "Retries",
        ],
      ]),
    );

    const navigation = renderPageNavigation(
        "← Past runs",
        activityContextLabel(run),
        () => {
          selectedEntry = null;
          detail = null;
          detailActorId = null;
          render();
        },
        playerLayerTimeMetadata(run),
      );
    pane.append(
      renderStickyHistoryContext(navigation, renderHistoryFilters(run, view)),
    );
    const rdpsRefresh = rdpsRefreshes.get(selectedEntry.session_id);
    if (rdpsRefresh || run.rdps_status.startsWith("formula_refresh_queued:")) {
      pane.append(renderHistoryRdpsProgress(rdpsRefresh));
    }

    const participants = participantRows(view);
    if (
      detailActorId !== null &&
      !participants.some((actor) => actor.actor_id === detailActorId)
    ) {
      detailActorId = null;
    }
    const actorColors = historyActorColors(
      participants,
      settings,
      selectedEntry.history_id,
    );
    const table = renderPlayerTable(view, participants, actorColors);
    pane.append(summary, table, renderGraphGallery(view, participants, actorColors));
    const selected = participants.find((actor) => actor.actor_id === detailActorId);
    if (selected && settings.playerDetailPresentation === "popover") {
      pane.append(renderPlayerDetails(run, view, selected, "popover"));
    }
    return pane;
  };

  const renderPlayerLayer = (): HTMLElement => {
    const pane = element("div", "combat-history-detail combat-history-player-layer");
    if (!selectedEntry || !detail || detailActorId === null) {
      detailActorId = null;
      pane.append(element("p", "runtime-empty-result", "Player details are unavailable."));
      return pane;
    }
    const run = detail.runs.find((candidate) => candidate.run_index === selectedEntry?.run_index);
    const view = run?.views.find((candidate) => candidate.id === viewId) ?? run?.views[0];
    if (view && targetActorId && !view.targets.some((target) => target.actor_id === targetActorId)) {
      targetActorId = null;
    }
    const actor = view?.actors.find((candidate) => candidate.actor_id === detailActorId);
    if (!run || !view || !actor) {
      detailActorId = null;
      render();
      return pane;
    }
    const navigation = renderPageNavigation(
        "← Run summary",
        `${activityContextLabel(run)} · ${actorLabel(actor)} skills`,
        () => {
          detailActorId = null;
          render();
        },
        playerLayerTimeMetadata(run),
      );
    pane.append(
      renderStickyHistoryContext(navigation, renderHistoryFilters(run, view)),
    );
    pane.append(renderPlayerDetails(run, view, actor, "layer"));
    return pane;
  };

  const renderPlayerTable = (
    view: CombatHistoryView,
    participants: HistoryActorSummary[],
    actorColors: ReadonlyMap<string, string>,
  ): HTMLElement => {
    const card = element("section", "content-card combat-history-player-card");
    const partyView = settings.historyPartyViews.find(
      (candidate) => candidate.id === historyPartyViewId,
    ) ?? settings.historyPartyViews[0]!;
    historyPartyViewId = partyView.id;
    const viewButtons = element("div", "combat-history-party-view-buttons");
    viewButtons.setAttribute("aria-label", "Party table view");
    for (const candidate of settings.historyPartyViews) {
      const viewButton = button(candidate.label, "combat-history-party-view-button");
      viewButton.dataset.selected = String(candidate.id === partyView.id);
      viewButton.setAttribute("aria-pressed", String(candidate.id === partyView.id));
      viewButton.addEventListener("click", () => {
        historyPartyViewId = candidate.id;
        partySortKey = candidate.sortKey;
        partySortDirection = candidate.sortDirection;
        render();
      });
      viewButtons.append(viewButton);
    }
    card.append(
      element("div", "card-heading",
        element("div", "", element("h2", "", "Party"), viewButtons),
        element(
          "span", "",
          targetActorId ? "Damage filtered to one entity" : participantCountLabel(participants),
        ),
      ),
    );
    const visibleColumns = partyView.columns.map(
      (key) => PARTY_SORT_COLUMNS.find((column) => column.key === key)!,
    );
    if (!visibleColumns.some((column) => column.key === partySortKey)) {
      partySortKey = partyView.sortKey;
      partySortDirection = partyView.sortDirection;
    }
    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-player-table";
    const playerColumnVisible = visibleColumns.some((column) => column.key === "player");
    const numericColumnCount = visibleColumns.filter((column) => column.numeric).length;
    const minimumTableWidth = Math.max(
      160,
      (playerColumnVisible ? 310 : 0) + numericColumnCount * 92,
    );
    table.style.setProperty("--history-player-table-min-width", `${minimumTableWidth}px`);
    const columnGroup = document.createElement("colgroup");
    for (const column of visibleColumns) {
      const col = document.createElement("col");
      const width = partyView.widths[column.key];
      if (width !== undefined) col.style.width = `${width}px`;
      columnGroup.append(col);
    }
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    for (const column of visibleColumns) {
      const cell = document.createElement("th");
      if (column.numeric) cell.className = "meter-number";
      const active = column.key === partySortKey;
      cell.dataset.sortActive = String(active);
      cell.setAttribute("aria-sort", active ? partySortDirection : "none");
      const indicator = active
        ? partySortDirection === "descending" ? "↓" : "↑"
        : "↕";
      const sort = button(
        `${column.label} ${indicator}`,
        `meter-sort-button combat-history-player-sort${column.numeric ? " meter-number" : ""}`,
      );
      sort.type = "button";
      sort.title = active
        ? `Sort ${column.label} ${partySortDirection === "descending" ? "lowest to highest" : "highest to lowest"}`
        : `Sort by ${column.label}`;
      sort.addEventListener("click", () => {
        if (partySortKey === column.key) {
          partySortDirection = partySortDirection === "descending" ? "ascending" : "descending";
        } else {
          partySortKey = column.key;
          partySortDirection = column.key === "player" ? "ascending" : "descending";
        }
        render();
      });
      cell.append(sort);
      heading.append(cell);
    }
    head.append(heading);
    const body = document.createElement("tbody");
    const sortedParticipants = sortPartyParticipants(
      participants,
      view,
      targetActorId,
      partySortKey,
      partySortDirection,
    );
    const barMaximum = partySortMaximum(participants, view, targetActorId, partySortKey);
    for (const actor of sortedParticipants) {
      const metrics = displayedMetrics(actor, view, targetActorId);
      const row = document.createElement("tr");
      row.tabIndex = 0;
      row.dataset.actorKind = actor.actor_kind ?? "unknown";
      row.dataset.presentationRole = actor.presentation_role ?? "unknown";
      const sortValue = partySortValue(actor, view, targetActorId, partySortKey);
      const barWidth = partyBarPercentage(sortValue, barMaximum);
      row.dataset.barActive = String(barWidth > 0);
      row.style.setProperty(
        "--combat-history-row-bar-color",
        actorColors.get(actor.actor_id) ?? graphColor(0),
      );
      row.style.setProperty("--combat-history-row-bar-width", `${barWidth}%`);
      row.setAttribute("aria-label", `Open ${actorLabel(actor)} skill details`);
      for (const column of visibleColumns) {
        switch (column.key) {
          case "player":
            row.append(identityCell(actor, settings));
            break;
          case "damage":
            row.append(numeric(metrics.damage));
            break;
          case "effectiveDamage":
            row.append(numeric(metrics.effectiveDamage, true));
            break;
          case "damageTaken":
            row.append(numeric(metrics.damageTaken, true));
            break;
          case "healing":
            row.append(numeric(metrics.healing, true));
            break;
          case "effectiveHealing":
            row.append(numeric(metrics.effectiveHealing, true));
            break;
          case "shielding":
            row.append(numeric(metrics.shielding, true));
            break;
          case "hits":
            row.append(numeric(metrics.hits, true));
            break;
          case "criticalRate":
            row.append(percentageCell(metrics.hits === 0 ? null : metrics.criticalHits / metrics.hits));
            break;
          case "dps":
            row.append(numeric(metrics.dps));
            break;
          case "encounterDps":
            row.append(numeric(metrics.encounterDps));
            break;
          case "hps":
            row.append(numeric(metrics.hps));
            break;
          case "tps":
            row.append(numeric(metrics.tps));
            break;
          case "rdmg":
            row.append(rdpsNumeric(actor.rdps_damage, true, targetActorId === null, actor.rdps_incomplete));
            break;
          case "rdps":
            row.append(rdpsNumeric(actor.rdps, false, targetActorId === null, actor.rdps_incomplete));
            break;
          case "rdpsGiven":
            row.append(rdpsNumeric(actor.rdps_contribution_given, true, targetActorId === null, actor.rdps_incomplete));
            break;
          case "rdpsReceived":
            row.append(rdpsNumeric(actor.rdps_contribution_received, true, targetActorId === null, actor.rdps_incomplete));
            break;
          case "apm":
            row.append(numeric(targetActorId ? null : actor.apm));
            break;
          case "deaths":
            row.append(numeric(actor.deaths, true));
            break;
        }
      }
      const select = () => {
        detailActorId = actor.actor_id;
        render();
      };
      row.addEventListener("click", select);
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") select();
      });
      body.append(row);
    }
    table.append(columnGroup, head, body);
    scroller.append(table);
    card.append(scroller);
    return card;
  };

  const playerDetailBackdrop = (dialog: HTMLElement): HTMLElement => {
    const backdrop = element("div", "combat-history-modal-backdrop", dialog);
    backdrop.addEventListener("mousedown", (event) => {
      if (event.target !== backdrop) return;
      detailActorId = null;
      render();
    });
    requestAnimationFrame(() => dialog.focus());
    return backdrop;
  };

  const renderIncomingDamageCard = (
    view: CombatHistoryView,
    victim: HistoryActorSummary,
  ): HTMLElement => {
    const card = element("section", "content-card combat-history-skill-card");
    card.append(
      element(
        "div",
        "card-heading",
        element("h2", "", "Incoming damage"),
        element("span", "", "Exact packet total by source and ability"),
      ),
    );

    const sources = incomingDamageSourceGroups(view, victim, targetActorId);

    if (sources.length === 0) {
      card.append(
        element(
          "p",
          "runtime-empty-result",
          targetActorId === null
            ? "No incoming damage was recorded for this player in the selected segment."
            : "No incoming damage from the selected entity was recorded for this player.",
        ),
      );
      return card;
    }

    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-skill-table combat-history-defense-table";
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    for (const [label, numericColumn] of [
      ["", false],
      ["Source", false],
      ["Ability", false],
      ["Damage taken", true],
      ["Hits", true],
      ["TPS", true],
    ] as const) {
      const cell = document.createElement("th");
      if (numericColumn) cell.className = "meter-number";
      cell.textContent = label;
      heading.append(cell);
    }
    head.append(heading);
    const body = document.createElement("tbody");
    const maximum = sources.reduce((value, source) => Math.max(value, source.total), 0);

    for (const sourceEntry of sources) {
      const groupKey = `incoming:${victim.actor_id}:${sourceEntry.sourceActorId}`;
      const collapsed = collapsedRecountGroups.has(groupKey);
      const parent = document.createElement("tr");
      parent.dataset.rowKind = "recount-parent";
      parent.dataset.barActive = String(sourceEntry.total > 0);
      parent.style.setProperty(
        "--combat-history-row-bar-width",
        `${partyBarPercentage(sourceEntry.total, maximum)}%`,
      );
      parent.style.setProperty(
        "--combat-history-row-bar-color",
        historyActorColor(
          sourceEntry.source ?? victim,
          0,
          settings,
          selectedEntry?.history_id ?? "history-defense",
        ),
      );
      const treeCell = document.createElement("td");
      treeCell.className = "combat-history-tree-cell";
      const toggle = button(collapsed ? "\u25b6" : "\u25bc", "combat-history-tree-toggle");
      toggle.type = "button";
      toggle.setAttribute("aria-expanded", String(!collapsed));
      toggle.setAttribute(
        "aria-label",
        `${collapsed ? "Expand" : "Collapse"} incoming damage from ${sourceEntry.source ? actorLabel(sourceEntry.source) : sourceEntry.sourceActorId}`,
      );
      toggle.addEventListener("click", () => {
        const next = new Set(collapsedRecountGroups);
        if (next.has(groupKey)) next.delete(groupKey);
        else next.add(groupKey);
        collapsedRecountGroups = next;
        render();
      });
      treeCell.append(toggle);
      const sourceCell = document.createElement("td");
      sourceCell.className = "meter-actor";
      sourceCell.append(
        element(
          "span",
          "combat-history-combat-copy",
          element(
            "strong",
            "",
            sourceEntry.source
              ? actorLabel(sourceEntry.source)
              : `Source actor ${sourceEntry.sourceActorId}`,
          ),
          element("small", "", `Entity ${sourceEntry.sourceEntityUuid}`),
        ),
      );
      parent.append(
        treeCell,
        sourceCell,
        element("td", "", `${sourceEntry.abilities.length} mapped abilities`),
        numeric(sourceEntry.total, true),
        numeric(sourceEntry.abilities.reduce((sum, entry) => sum + entry.hits, 0), true),
        numeric(perSecond(sourceEntry.total, view.elapsed_micros)),
      );
      body.append(parent);
      if (collapsed) continue;

      const children = [
        ...sourceEntry.abilities.map((entry) => ({
          ability: entry.ability,
          damage: entry.damage,
          hits: entry.hits,
          unattributed: false,
        })),
        ...(sourceEntry.unattributed > 0
          ? [{ ability: null, damage: sourceEntry.unattributed, hits: 0, unattributed: true }]
          : []),
      ];
      for (const [index, child] of children.entries()) {
        const row = document.createElement("tr");
        row.dataset.rowKind = "recount-child";
        row.dataset.lastChild = String(index === children.length - 1);
        const branchCell = document.createElement("td");
        branchCell.className = "combat-history-tree-cell";
        const branch = element("span", "combat-history-tree-branch", "");
        branch.setAttribute("aria-hidden", "true");
        branchCell.append(branch);
        const blankSource = document.createElement("td");
        let abilityCell: HTMLTableCellElement;
        if (child.ability) {
          abilityCell = combatPresentationCell(
            child.ability.ability_id,
            child.ability.presentation_name,
            child.ability.presentation_kind,
            child.ability.presentation_resolution,
            child.ability.icon_asset_path,
            "ability",
          );
        } else {
          abilityCell = document.createElement("td");
          abilityCell.className = "meter-actor combat-history-combat-presentation-cell";
          abilityCell.append(
            element(
              "span",
              "combat-history-combat-copy",
              element("strong", "", "Unattributed packet damage"),
              element("small", "", "Preserved difference between incoming and mapped ability totals"),
            ),
          );
        }
        row.append(
          branchCell,
          blankSource,
          abilityCell,
          numeric(child.damage, true),
          numeric(child.hits, true),
          numeric(perSecond(child.damage, view.elapsed_micros)),
        );
        body.append(row);
      }
    }
    table.append(head, body);
    scroller.append(table);
    card.append(scroller);
    return card;
  };

  const renderPlayerDetails = (
    run: CombatRunHistory,
    view: CombatHistoryView,
    actor: HistoryActorSummary,
    presentation: "layer" | "popover",
  ): HTMLElement => {
    const dialog = element(
      "section",
      presentation === "popover"
        ? "combat-history-player-dialog"
        : "combat-history-player-page",
    );
    if (presentation === "popover") {
      dialog.setAttribute("role", "dialog");
      dialog.setAttribute("aria-modal", "true");
      dialog.setAttribute("aria-label", `${actorLabel(actor)} skill details`);
      dialog.tabIndex = -1;
    }
    const dialogHeader = element(
      "header",
      "combat-history-dialog-header",
      element(
        "div",
        "",
        element("span", "run-report-kicker", actorIdentityLabel(actor)),
        element("h2", "", actorLabel(actor)),
        element(
          "p",
          "card-copy",
          `${actor.character_id ? `UID ${actor.character_id} · ` : ""}Entity ${actor.entity_uuid} · ${view.label}`,
        ),
      ),
    );
    if (presentation === "popover") {
      const close = button("Close", "quiet-button combat-history-dialog-close");
      close.setAttribute("aria-label", "Close player details");
      close.addEventListener("click", () => {
        detailActorId = null;
        render();
      });
      dialogHeader.append(close);
    }
    const actorMetrics = displayedMetrics(actor, view, targetActorId);
    const overview = metricGrid([
      [INTEGER.format(actorMetrics.damage), "Damage"],
      [NUMBER.format(actorMetrics.dps), "DPS"],
      [NUMBER.format(actorMetrics.encounterDps), "eDPS"],
      [rdpsDisplay(actor.rdps_damage, true, actor.rdps_incomplete), "rDMG"],
      [rdpsDisplay(actor.rdps, false, actor.rdps_incomplete), "rDPS"],
      [
        rdpsDisplay(actor.rdps_contribution_given, true, actor.rdps_incomplete),
        "rDMG granted",
      ],
      [
        rdpsDisplay(actor.rdps_contribution_received, true, actor.rdps_incomplete),
        "rDMG received",
      ],
      [NUMBER.format(actorMetrics.hps), "HPS"],
      [NUMBER.format(actorMetrics.tps), "TPS"],
      [INTEGER.format(actor.deaths), "Deaths"],
    ]);
    const partyView = settings.historyPartyViews.find(
      (candidate) => candidate.id === historyPartyViewId,
    ) ?? settings.historyPartyViews[0]!;
    const detailMode = partyView.detailMode;
    if (detailMode === "defense") {
      const defenseCard = renderIncomingDamageCard(view, actor);
      const rdpsBreakdown = renderRdpsBreakdown(view, actor);
      const influences = renderDamageInfluences(view, actor);
      const effects = renderEffects(actor);
      dialog.append(dialogHeader, overview, defenseCard, rdpsBreakdown, influences, effects);
      return presentation === "layer"
        ? dialog
        : playerDetailBackdrop(dialog);
    }
    const abilityColumns = detailMode === "healing"
      ? HEALING_ABILITY_SORT_COLUMNS
      : ABILITY_SORT_COLUMNS;
    if (!abilityColumns.some((column) => column.key === abilitySortKey)) {
      abilitySortKey = detailMode === "healing" ? "hps" : "damage";
      abilitySortDirection = "descending";
    }
    const card = element("section", "content-card combat-history-skill-card");
    card.append(
      element("div", "card-heading",
        element("h2", "", detailMode === "healing" ? "Healing and shielding" : "Skills"),
        element("span", "", targetActorId ? "Target-filtered" : `${actor.abilities.length} observed abilities`),
      ),
    );
    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-skill-table";
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    const treeHeading = document.createElement("th");
    treeHeading.className = "combat-history-tree-column";
    treeHeading.setAttribute("aria-label", "Recount tree");
    heading.append(treeHeading);
    for (const column of abilityColumns) {
      const cell = document.createElement("th");
      if (column.numeric) cell.className = "meter-number";
      const active = column.key === abilitySortKey;
      cell.dataset.sortActive = String(active);
      cell.setAttribute("aria-sort", active ? abilitySortDirection : "none");
      const indicator = active
        ? abilitySortDirection === "descending" ? "↓" : "↑"
        : "↕";
      const sort = button(
        `${column.label} ${indicator}`,
        `meter-sort-button combat-history-ability-sort${column.numeric ? " meter-number" : ""}`,
      );
      sort.type = "button";
      sort.title = active
        ? `Sort ${column.label} ${abilitySortDirection === "descending" ? "lowest to highest" : "highest to lowest"}`
        : `Sort by ${column.label}`;
      sort.addEventListener("click", () => {
        if (abilitySortKey === column.key) {
          abilitySortDirection = abilitySortDirection === "descending" ? "ascending" : "descending";
        } else {
          abilitySortKey = column.key;
          abilitySortDirection = column.key === "ability" ? "ascending" : "descending";
        }
        render();
      });
      cell.append(sort);
      heading.append(cell);
    }
    head.append(heading);
    const rdpsBreakdown = actorRdpsBreakdown(view, actor.actor_id, targetActorId);
    const rdpsByAbilityId = new Map(
      rdpsBreakdown.receivedSkills.flatMap((skill) =>
        skill.abilityId === null ? [] : [[skill.abilityId, skill] as const]
      ),
    );
    const unmappedRdps = rdpsBreakdown.receivedSkills.find((skill) => skill.abilityId === null);
    const displayedAbilities = [
      ...actor.abilities.map((ability) => displayedAbility(
        ability,
        view,
        targetActorId,
        rdpsByAbilityId.get(ability.ability_id),
      )),
      ...(unmappedRdps ? [displayedUnmappedRdpsSkill(unmappedRdps, view)] : []),
    ];
    const abilities = groupDisplayedAbilities(
      displayedAbilities,
      abilitySortKey,
      abilitySortDirection,
    );
    const barMaximum = abilitySortMaximum(
      abilities.map((entry) => entry.ability),
      abilitySortKey,
    );
    const barColor = historyActorColor(
      actor,
      0,
      settings,
      selectedEntry?.history_id ?? `run-${run.run_index}`,
    );
    const body = document.createElement("tbody");
    for (const entry of abilities) {
      const ability = entry.ability;
      const groupId = entry.kind === "recount-parent"
        ? ability.abilityId
        : ability.recountGroupId;
      const groupKey = groupId ? `${actor.actor_id}:${groupId}` : null;
      const collapsed = groupKey ? collapsedRecountGroups.has(groupKey) : false;
      if (entry.kind === "recount-child" && collapsed) continue;
      const row = document.createElement("tr");
      row.dataset.rowKind = entry.kind;
      row.dataset.lastChild = String(entry.isLastChild);
      const sortValue = abilitySortValue(ability, abilitySortKey);
      const barWidth = partyBarPercentage(sortValue, barMaximum);
      row.dataset.barActive = String(barWidth > 0);
      row.style.setProperty("--combat-history-row-bar-color", barColor);
      row.style.setProperty("--combat-history-row-bar-width", `${barWidth}%`);
      const presentationCell = combatPresentationCell(
          ability.abilityId,
          ability.presentationName,
          ability.presentationKind,
          ability.presentationResolution,
          ability.iconAssetPath,
          "ability",
        );
      const treeCell = document.createElement("td");
      treeCell.className = "combat-history-tree-cell";
      if (entry.kind === "recount-parent" && groupKey) {
        const toggle = button(collapsed ? "\u25b6" : "\u25bc", "combat-history-tree-toggle");
        toggle.type = "button";
        toggle.setAttribute("aria-expanded", String(!collapsed));
        toggle.setAttribute(
          "aria-label",
          `${collapsed ? "Expand" : "Collapse"} Recount ${ability.abilityId}`,
        );
        toggle.title = `${entry.childCount} child action${entry.childCount === 1 ? "" : "s"}`;
        toggle.addEventListener("click", () => {
          const next = new Set(collapsedRecountGroups);
          if (next.has(groupKey)) next.delete(groupKey);
          else next.add(groupKey);
          collapsedRecountGroups = next;
          render();
        });
        treeCell.append(toggle);
      } else if (entry.kind === "recount-child") {
        const branch = element("span", "combat-history-tree-branch", "");
        branch.setAttribute("aria-hidden", "true");
        treeCell.append(branch);
      }
      row.append(treeCell);
      for (const column of abilityColumns) {
        switch (column.key) {
          case "ability": row.append(presentationCell); break;
          case "damage": row.append(numeric(ability.damage, true)); break;
          case "rdmgReceived": row.append(relativeDamageSkillCell(ability, view, "damage")); break;
          case "rdpsReceived": row.append(relativeDamageSkillCell(ability, view, "rate")); break;
          case "hits": row.append(numeric(ability.hits, true)); break;
          case "casts": row.append(numeric(ability.casts, true)); break;
          case "criticals": row.append(numeric(ability.criticals, true)); break;
          case "dps": row.append(numeric(ability.dps)); break;
          case "encounterDps": row.append(numeric(ability.encounterDps)); break;
          case "healing": row.append(numeric(ability.healing, true)); break;
          case "effectiveHealing": row.append(numeric(ability.effectiveHealing, true)); break;
          case "shielding": row.append(numeric(ability.shielding, true)); break;
          case "hps": row.append(numeric(ability.hps)); break;
        }
      }
      body.append(row);
    }
    table.append(head, body);
    scroller.append(table);
    card.append(scroller);

    const rdpsSummary = renderRdpsBreakdown(view, actor);
    const influences = renderDamageInfluences(view, actor);
    const effects = renderEffects(actor);
    dialog.append(dialogHeader, overview, card, rdpsSummary, influences, effects);
    const pendingFeatures: string[] = [];
    const rdpsStatus = describeRdpsStatus(run.rdps_status);
    if (rdpsStatus.historyMessage !== null) {
      pendingFeatures.push(rdpsStatus.historyMessage);
    }
    if (run.apm_status !== "ready") {
      pendingFeatures.push(
        "APM waits for reviewed active-skill, role-skill, and Imagine action classification; passive effects and hit packets will not be counted.",
      );
    }
    if (pendingFeatures.length > 0) {
      dialog.append(
        element(
          "p",
          "combat-history-pending-note",
          pendingFeatures.join(" "),
        ),
      );
    }
    return presentation === "layer" ? dialog : playerDetailBackdrop(dialog);
  };

  const renderGraphGallery = (
    view: CombatHistoryView,
    participants: HistoryActorSummary[],
    actorColors: ReadonlyMap<string, string>,
  ): HTMLElement => {
    const gallery = element("section", "content-card combat-history-graph-gallery");
    gallery.append(
      element(
        "div",
        "card-heading",
        element("h2", "", "Party timelines"),
        element("span", "", "One-second source buckets · five-second moving rate"),
      ),
    );
    const legend = element("div", "combat-history-graph-legend");
    for (const [index, actor] of participants.entries()) {
      const control = button("", "combat-history-series-toggle");
      const hidden = hiddenGraphActors.has(actor.actor_id);
      control.dataset.hidden = String(hidden);
      control.dataset.actorKind = graphActorKind(actor);
      control.style.setProperty(
        "--series-color",
        actorColors.get(actor.actor_id) ?? graphColor(index),
      );
      control.setAttribute(
        "aria-label",
        `${hidden ? "Show" : "Hide"} ${actorLabel(actor)} in timelines`,
      );
      control.append(
        element("span", "combat-history-series-swatch"),
        element("strong", "", actorLabel(actor)),
      );
      if (graphActorKind(actor) === "npc") {
        control.append(element("span", "combat-history-legend-npc", "NPC"));
      }
      control.addEventListener("click", () => {
        const next = new Set(hiddenGraphActors);
        if (next.has(actor.actor_id)) next.delete(actor.actor_id);
        else next.add(actor.actor_id);
        hiddenGraphActors = next;
        render();
      });
      legend.append(control);
    }
    gallery.append(legend);
    const definition = GRAPH_METRICS.find(
      (candidate) => candidate.metric === graphMetric,
    ) ?? GRAPH_METRICS[0]!;
    gallery.append(
      renderMetricGraph(
        participants,
        definition,
        view.elapsed_micros,
        hiddenGraphActors,
        actorColors,
        targetActorId,
        (metric) => {
          graphMetric = metric;
          render();
        },
      ),
    );
    return gallery;
  };

  const renderEffects = (actor: HistoryActorSummary): HTMLElement => {
    const card = element("section", "content-card combat-history-effects-card");
    const effects = actor.effects.filter(
      (effect) => targetActorId === null || effect.target_actor_id === targetActorId,
    );
    card.append(
      element("div", "card-heading", element("h2", "", "Status effects"), element("span", "", `${effects.length} effect IDs`)),
    );
    if (effects.length === 0) {
      card.append(element("p", "runtime-empty-result", "No attributed status events in this filter."));
      return card;
    }
    const list = element("div", "combat-history-effect-list");
    for (const effect of effects) {
      list.append(
        element("div", "combat-history-effect-row",
          combatPresentationIdentity(
            effect.effect_id,
            effect.presentation_name,
            effect.presentation_kind,
            effect.presentation_resolution,
            effect.icon_asset_path,
            "effect",
          ),
          element(
            "span",
            "",
            `Applied ${effect.applied} · Refreshed ${effect.refreshed} · Stacked ${effect.stacked} · Consumed ${effect.consumed} · Removed ${effect.removed}`,
          ),
        ),
      );
    }
    card.append(list);
    return card;
  };

  const renderRdpsBreakdown = (
    view: CombatHistoryView,
    actor: HistoryActorSummary,
  ): HTMLElement => {
    const card = element("section", "content-card combat-history-rdps-breakdown-card");
    const breakdown = actorRdpsBreakdown(view, actor.actor_id, targetActorId);
    const relationshipCount = breakdown.receivedSkills.reduce(
      (sum, skill) => sum + skill.sources.length,
      0,
    ) + breakdown.grantedEffects.length;
    card.append(
      element(
        "div",
        "card-heading",
        element("h2", "", "Relative damage sources"),
        element(
          "span",
          "",
          targetActorId === null
            ? `${relationshipCount} summarized relationship${relationshipCount === 1 ? "" : "s"}`
            : "Target-filtered",
        ),
      ),
    );

    if (breakdown.receivedSkills.length === 0 && breakdown.grantedEffects.length === 0) {
      card.append(
        element(
          "p",
          "runtime-empty-result",
          "No conserved rDPS relationship has been calculated for this player in this view. This is not proof that no party support affected them.",
        ),
      );
      return card;
    }

    if (breakdown.receivedSkills.length > 0) {
      card.append(
        element(
          "p",
          "card-copy",
          "Received rDMG and rDPS are nested into the normal skill table. Hover either value to see the contributing players, effects, components, and event totals.",
        ),
      );
    }

    if (breakdown.grantedEffects.length > 0) {
      const section = element("section", "combat-history-rdps-summary-section");
      section.append(
        element("h3", "", "Granted by support effect"),
        element(
          "p",
          "card-copy",
          "Outgoing relative damage is grouped by the support effect that earned the credit.",
        ),
      );
      const scroller = element("div", "meter-table-scroll");
      const table = document.createElement("table");
      table.className = "meter-table combat-history-rdps-summary-table";
      const head = document.createElement("thead");
      const heading = document.createElement("tr");
      for (const label of ["Support effect", "Component", "rDMG granted", "rDPS granted", "Events"]) {
        const cell = document.createElement("th");
        if (["rDMG granted", "rDPS granted", "Events"].includes(label)) cell.className = "meter-number";
        cell.textContent = label;
        heading.append(cell);
      }
      head.append(heading);
      const body = document.createElement("tbody");
      for (const granted of breakdown.grantedEffects) {
        const effect = historyRdpsEffectPresentation(view, granted.effectId);
        const row = document.createElement("tr");
        const effectCell = document.createElement("td");
        effectCell.append(
          combatPresentationIdentity(
            granted.effectId,
            effect?.presentation_name ?? null,
            effect?.presentation_kind ?? null,
            effect?.presentation_resolution ?? null,
            effect?.icon_asset_path ?? null,
            "effect",
          ),
        );
        row.append(
          effectCell,
          textTableCell(
            granted.attributionComponent
              ? attributionComponentLabel(granted.attributionComponent)
              : "Complete effect",
          ),
          rdpsSummaryExactCell(granted.attributedRdps, granted.unresolvedRelationshipCount),
          relativeDamageRateCell(granted.attributedRdps, view.elapsed_micros),
          numeric(granted.damageEventCount, true),
        );
        body.append(row);
      }
      table.append(head, body);
      scroller.append(table);
      section.append(scroller);
      card.append(section);
    }
    return card;
  };

  const renderDamageInfluences = (
    view: CombatHistoryView,
    actor: HistoryActorSummary,
  ): HTMLElement => {
    const card = document.createElement("details");
    card.className = "content-card combat-history-influence-card";
    card.open = expandedInfluenceActorId === actor.actor_id;
    const actorInfluences = (view.damage_influences ?? []).filter((influence) =>
      (influence.provider_actor_id === actor.actor_id ||
        influence.recipient_actor_id === actor.actor_id) &&
      (targetActorId === null || influence.target_actor_id === targetActorId)
    );
    const influences = actorInfluences.filter((influence) =>
      historyDamageInfluenceMatchesQuery(view, influence, influenceQuery)
    );
    card.append(
      element(
        "summary",
        "card-heading combat-history-influence-summary",
        element("h2", "", "Exact influence audit ledger"),
        element(
          "span",
          "",
          influenceQuery.trim()
            ? `${influences.length} of ${actorInfluences.length} exact relationships`
            : `${influences.length} exact relationship${influences.length === 1 ? "" : "s"}`,
        ),
      ),
    );
    card.addEventListener("toggle", () => {
      const next = card.open ? actor.actor_id : null;
      if (expandedInfluenceActorId === next) return;
      expandedInfluenceActorId = next;
      render();
    });
    if (!card.open) return card;
    const toolbar = element("div", "combat-history-influence-toolbar");
    const search = document.createElement("input");
    search.type = "search";
    search.className = "combat-history-influence-search";
    search.value = influenceQuery;
    search.placeholder = "Filter UID, effect ID, skill ID, player, or target…";
    search.setAttribute("aria-label", "Filter damage influences");
    search.addEventListener("input", () => {
      influenceQuery = search.value;
      render();
      requestAnimationFrame(() => {
        const next = content.querySelector<HTMLInputElement>(
          ".combat-history-influence-search",
        );
        next?.focus();
        next?.setSelectionRange(influenceQuery.length, influenceQuery.length);
      });
    });
    toolbar.append(search);
    card.append(toolbar);
    if (influences.length === 0) {
      card.append(
        element(
          "p",
          "runtime-empty-result",
          "No matching-build packet-proven damage influence is available in this filter.",
        ),
      );
      return card;
    }

    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-influence-table";
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    for (const label of [
      "Effect",
      "Component",
      "Provider",
      "Recipient",
      "Affected damage ID",
      "Target",
      "Events",
      "Observed damage",
      "Attributed rDMG",
    ]) {
      const cell = document.createElement("th");
      if (["Events", "Observed damage", "Attributed rDMG"].includes(label)) {
        cell.className = "meter-number";
      }
      cell.textContent = label;
      heading.append(cell);
    }
    head.append(heading);
    const body = document.createElement("tbody");
    for (const influence of influences) {
      const provider = historyActorByIdentity(
        view,
        influence.provider_actor_id,
        influence.provider_entity_uuid,
      );
      const recipient = historyActorByIdentity(
        view,
        influence.recipient_actor_id,
        influence.recipient_entity_uuid,
      );
      const target = view.targets.find(
        (candidate) => candidate.actor_id === influence.target_actor_id,
      );
      const effect = historyRdpsEffectPresentation(view, influence.effect_id);
      const ability = recipient?.abilities.find(
        (candidate) => candidate.ability_id === influence.affected_ability_id,
      );
      const row = document.createElement("tr");
      const effectCell = document.createElement("td");
      effectCell.append(
        combatPresentationIdentity(
          influence.effect_id,
          effect?.presentation_name ?? null,
          effect?.presentation_kind ?? null,
          effect?.presentation_resolution ?? null,
          effect?.icon_asset_path ?? null,
          "effect",
        ),
      );
      const abilityCell = document.createElement("td");
      if (influence.affected_ability_id) {
        abilityCell.append(
          combatPresentationIdentity(
            influence.affected_ability_id,
            ability?.presentation_name ?? null,
            ability?.presentation_kind ?? null,
            ability?.presentation_resolution ?? null,
            ability?.icon_asset_path ?? null,
            "ability",
          ),
        );
      } else {
        abilityCell.textContent = "Context unresolved";
        abilityCell.dataset.contextComplete = "false";
      }
      row.dataset.contextComplete = String(influence.damage_context_complete);
      row.append(
        effectCell,
        textTableCell(
          influence.attribution_component
            ? attributionComponentLabel(influence.attribution_component)
            : "Complete effect",
        ),
        textTableCell(provider ? actorLabel(provider) : `Actor ${influence.provider_actor_id}`),
        textTableCell(recipient ? actorLabel(recipient) : `Actor ${influence.recipient_actor_id}`),
        abilityCell,
        textTableCell(
          target
            ? target.presentation_name?.trim() || target.display_name?.trim() || `Entity ${target.entity_uuid}`
            : influence.target_entity_uuid
              ? `Entity ${influence.target_entity_uuid}`
              : "Context unresolved",
        ),
        numeric(influence.damage_event_count, true),
        exactIntegerCell(influence.observed_damage),
        exactInfluenceCell(
          influence.attributed_rdps,
          influence.exact_integer_delta,
          influence.exact_rational_deltas,
        ),
      );
      body.append(row);
    }
    table.append(head, body);
    scroller.append(table);
    card.append(scroller);
    return card;
  };

  const closeDetailOnEscape = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    if (deleteConfirmationOpen && !historyMutationInFlight) {
      event.preventDefault();
      deleteConfirmationOpen = false;
      render();
      return;
    }
    if (detailActorId === null) return;
    event.preventDefault();
    detailActorId = null;
    render();
  };
  const refreshWhenVisible = () => {
    if (document.visibilityState === "visible") void load();
  };
  window.addEventListener("keydown", closeDetailOnEscape);
  document.addEventListener("visibilitychange", refreshWhenVisible);
  void load(true).then(() => {
    if (!alive || !subscribeCatalogChanges) return;
    unsubscribeCatalogChanges = subscribeCatalogChanges(
      (update) => {
        if (update === undefined) {
          void load(false, true);
          return;
        }
        const selectedSessionId = selectedEntry?.session_id ?? null;
        const previouslyRefreshing = selectedSessionId !== null && rdpsRefreshes.has(selectedSessionId);
        rdpsRefreshes = new Map(
          update.rdps_refreshes.map((progress) => [progress.session_id, progress]),
        );
        const currentlyRefreshing = selectedSessionId !== null && rdpsRefreshes.has(selectedSessionId);
        if (update.catalog_changed || (previouslyRefreshing && !currentlyRefreshing)) {
          void load(false, true);
        } else {
          render();
        }
      },
      (error) => {
        if (alive) status.textContent = `Automatic history refresh is reconnecting: ${errorMessage(error)}`;
      },
    );
  });
  return {
    dispose() {
      alive = false;
      unsubscribeCatalogChanges();
      window.removeEventListener("keydown", closeDetailOnEscape);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    },
  };
}

export function participantRows(view: CombatHistoryView): HistoryActorSummary[] {
  return view.actors
    .filter((actor) =>
      actor.actor_kind === "player" ||
      actor.actor_kind === "npc" ||
      actor.presentation_kind === "party_npc",
    )
    .filter((actor) =>
      actor.damage > 0 ||
      actor.healing > 0 ||
      actor.damage_taken > 0 ||
      actor.deaths > 0 ||
      (actor.rdps_contribution_given !== null && actor.rdps_contribution_given !== 0) ||
      (actor.rdps_contribution_received !== null && actor.rdps_contribution_received !== 0),
    )
    .sort((left, right) => right.encounter_dps - left.encounter_dps || actorLabel(left).localeCompare(actorLabel(right)));
}

function participantCountLabel(participants: HistoryActorSummary[]): string {
  const npcCount = participants.filter((actor) => graphActorKind(actor) === "npc").length;
  const playerCount = participants.length - npcCount;
  if (npcCount === 0) return `${playerCount} combatants`;
  return `${playerCount} players · ${npcCount} party NPCs`;
}

function displayedMetrics(
  actor: HistoryActorSummary,
  view: CombatHistoryView,
  targetActorId: string | null,
): {
  damage: number;
  effectiveDamage: number;
  damageTaken: number;
  healing: number;
  effectiveHealing: number;
  shielding: number;
  hits: number;
  criticalHits: number;
  dps: number;
  encounterDps: number;
  hps: number;
  tps: number;
} {
  if (targetActorId === null) {
    return {
      damage: actor.damage,
      effectiveDamage: actor.effective_damage,
      damageTaken: actor.damage_taken,
      healing: actor.healing,
      effectiveHealing: actor.effective_healing,
      shielding: actor.shielding,
      hits: actor.hits,
      criticalHits: actor.critical_hits,
      dps: actor.dps,
      encounterDps: actor.encounter_dps,
      hps: actor.hps,
      tps: actor.tps,
    };
  }
  const target = actor.targets.find((target) => target.actor_id === targetActorId);
  const damage = target?.damage ?? 0;
  const effectiveDamage = target?.effective_damage ?? 0;
  const damageTaken = target?.series.reduce((sum, point) => sum + point.damage_taken, 0) ?? 0;
  const abilityTargets = actor.abilities.flatMap((ability) => {
    const summary = ability.targets.find((candidate) => candidate.actor_id === targetActorId);
    return summary ? [summary] : [];
  });
  const total = (select: (summary: (typeof abilityTargets)[number]) => number) =>
    abilityTargets.reduce((sum, summary) => sum + select(summary), 0);
  const healing = total((summary) => summary.healing);
  const effectiveHealing = total((summary) => summary.effective_healing);
  const shielding = total((summary) => summary.shielding);
  return {
    damage,
    effectiveDamage,
    damageTaken,
    healing,
    effectiveHealing,
    shielding,
    hits: target?.hits ?? 0,
    criticalHits: target?.critical_hits ?? 0,
    dps: perSecond(damage, view.elapsed_micros),
    encounterDps: perSecond(damage, view.active_combat_micros),
    hps: perSecond(effectiveHealing, view.elapsed_micros),
    tps: perSecond(damageTaken, view.elapsed_micros),
  };
}

type PartySortValue = number | string | null;

function partySortValue(
  actor: HistoryActorSummary,
  view: CombatHistoryView,
  targetActorId: string | null,
  key: PartySortKey,
): PartySortValue {
  const metrics = displayedMetrics(actor, view, targetActorId);
  switch (key) {
    case "player": return actorLabel(actor);
    case "damage": return metrics.damage;
    case "effectiveDamage": return metrics.effectiveDamage;
    case "damageTaken": return metrics.damageTaken;
    case "healing": return metrics.healing;
    case "effectiveHealing": return metrics.effectiveHealing;
    case "shielding": return metrics.shielding;
    case "hits": return metrics.hits;
    case "criticalRate": return metrics.hits > 0
      ? metrics.criticalHits / metrics.hits
      : null;
    case "dps": return metrics.dps;
    case "encounterDps": return metrics.encounterDps;
    case "hps": return metrics.hps;
    case "tps": return metrics.tps;
    case "rdmg": return targetActorId === null ? actor.rdps_damage : null;
    case "rdps": return targetActorId === null ? actor.rdps : null;
    case "rdpsGiven": return targetActorId === null ? actor.rdps_contribution_given : null;
    case "rdpsReceived": return targetActorId === null ? actor.rdps_contribution_received : null;
    case "apm": return targetActorId === null ? actor.apm : null;
    case "deaths": return actor.deaths;
  }
}

function sortPartyParticipants(
  participants: readonly HistoryActorSummary[],
  view: CombatHistoryView,
  targetActorId: string | null,
  key: PartySortKey,
  direction: PartySortDirection,
): HistoryActorSummary[] {
  return [...participants].sort((left, right) => {
    const compared = comparePartySortValues(
      partySortValue(left, view, targetActorId, key),
      partySortValue(right, view, targetActorId, key),
      direction,
    );
    return compared || actorLabel(left).localeCompare(actorLabel(right));
  });
}

function partySortMaximum(
  participants: readonly HistoryActorSummary[],
  view: CombatHistoryView,
  targetActorId: string | null,
  key: PartySortKey,
): number {
  return participants.reduce((maximum, actor) => {
    const value = partySortValue(actor, view, targetActorId, key);
    return typeof value === "number" && Number.isFinite(value)
      ? Math.max(maximum, value)
      : maximum;
  }, 0);
}

export function comparePartySortValues(
  left: PartySortValue,
  right: PartySortValue,
  direction: PartySortDirection,
): number {
  const leftMissing = left === null || (typeof left === "number" && !Number.isFinite(left));
  const rightMissing = right === null || (typeof right === "number" && !Number.isFinite(right));
  if (leftMissing || rightMissing) {
    if (leftMissing && rightMissing) return 0;
    return leftMissing ? 1 : -1;
  }
  const comparison = typeof left === "string" && typeof right === "string"
    ? left.localeCompare(right, undefined, { sensitivity: "base", numeric: true })
    : Number(left) - Number(right);
  return direction === "descending" ? -comparison : comparison;
}

export function partyBarPercentage(value: PartySortValue, maximum: number): number {
  if (typeof value !== "number" || !Number.isFinite(value) || maximum <= 0) return 0;
  return Math.max(0, Math.min(100, (value / maximum) * 100));
}

function displayedAbility(
  ability: HistoryAbilitySummary,
  view: CombatHistoryView,
  targetActorId: string | null,
  rdps?: RdpsReceivedSkillSummary,
) {
  const receivedRdmgExact = rdps?.attributedRdps ?? null;
  const receivedRdmg = receivedRdmgExact === null ? null : Number(receivedRdmgExact);
  const relativeDamage = {
    receivedRdmgExact,
    receivedRdmg: receivedRdmg !== null && Number.isFinite(receivedRdmg)
      ? receivedRdmg
      : null,
    receivedRdps: receivedRdmg !== null && Number.isFinite(receivedRdmg)
      ? perSecond(receivedRdmg, view.elapsed_micros)
      : null,
    rdpsSources: rdps?.sources ?? [],
    rdpsDamageEventCount: rdps?.damageEventCount ?? 0,
    rdpsUnresolvedRelationshipCount: rdps?.unresolvedRelationshipCount ?? 0,
    hasRdpsRelationship: rdps !== undefined,
  };
  if (targetActorId === null) {
    return {
      abilityId: ability.ability_id,
      presentationName: ability.presentation_name,
      presentationKind: ability.presentation_kind,
      presentationResolution: ability.presentation_resolution,
      iconAssetPath: ability.icon_asset_path,
      recountGroupId: ability.presentation_recount_group_id,
      recountGroupName: ability.presentation_recount_group_name,
      damage: ability.damage,
      hits: ability.hits,
      casts: ability.casts,
      criticals: ability.critical_hits,
      dps: ability.dps,
      encounterDps: ability.encounter_dps,
      healing: ability.healing,
      effectiveHealing: ability.effective_healing,
      shielding: ability.shielding,
      hps: ability.hps,
      ...relativeDamage,
    };
  }
  const target = ability.targets.find((target) => target.actor_id === targetActorId);
  const damage = target?.damage ?? 0;
  const healing = target?.healing ?? 0;
  const effectiveHealing = target?.effective_healing ?? 0;
  const shielding = target?.shielding ?? 0;
  return {
    abilityId: ability.ability_id,
    presentationName: ability.presentation_name,
    presentationKind: ability.presentation_kind,
    presentationResolution: ability.presentation_resolution,
    iconAssetPath: ability.icon_asset_path,
    recountGroupId: ability.presentation_recount_group_id,
    recountGroupName: ability.presentation_recount_group_name,
    damage,
    hits: target?.hits ?? 0,
    casts: ability.casts,
    criticals: target?.critical_hits ?? 0,
    dps: perSecond(damage, view.elapsed_micros),
    encounterDps: perSecond(damage, view.active_combat_micros),
    healing,
    effectiveHealing,
    shielding,
    hps: perSecond(effectiveHealing, view.elapsed_micros),
    ...relativeDamage,
  };
}

export type DisplayedAbility = ReturnType<typeof displayedAbility>;

export function displayedUnmappedRdpsSkill(
  rdps: RdpsReceivedSkillSummary,
  view: CombatHistoryView,
): DisplayedAbility {
  const receivedRdmgExact = rdps.attributedRdps;
  const receivedRdmg = receivedRdmgExact === null ? null : Number(receivedRdmgExact);
  return {
    abilityId: "not observed",
    presentationName: "Unmapped damage actions",
    presentationKind: "unresolved",
    presentationResolution: "unresolved",
    iconAssetPath: null,
    recountGroupId: null,
    recountGroupName: null,
    damage: 0,
    hits: 0,
    casts: 0,
    criticals: 0,
    dps: 0,
    encounterDps: 0,
    healing: 0,
    effectiveHealing: 0,
    shielding: 0,
    hps: 0,
    receivedRdmgExact,
    receivedRdmg: receivedRdmg !== null && Number.isFinite(receivedRdmg)
      ? receivedRdmg
      : null,
    receivedRdps: receivedRdmg !== null && Number.isFinite(receivedRdmg)
      ? perSecond(receivedRdmg, view.elapsed_micros)
      : null,
    rdpsSources: rdps.sources,
    rdpsDamageEventCount: rdps.damageEventCount,
    rdpsUnresolvedRelationshipCount: rdps.unresolvedRelationshipCount,
    hasRdpsRelationship: true,
  };
}

function sumExactRelativeDamage(
  abilities: readonly DisplayedAbility[],
): string | null {
  const values = abilities.flatMap((ability) =>
    ability.receivedRdmgExact === null ? [] : [BigInt(ability.receivedRdmgExact)]
  );
  return values.length === 0
    ? null
    : values.reduce((sum, value) => sum + value, 0n).toString();
}

function combineRelativeDamageSources(
  abilities: readonly DisplayedAbility[],
): RdpsReceivedSourceSummary[] {
  type SourceAccumulator = RdpsReceivedSourceSummary & {
    exactTotal: bigint;
    hasExact: boolean;
  };
  const combined = new Map<string, SourceAccumulator>();
  for (const ability of abilities) {
    for (const source of ability.rdpsSources) {
      const key = [
        source.providerActorId,
        source.providerEntityUuid,
        source.effectId,
        source.attributionComponent ?? "",
      ].join("\u001f");
      let accumulator = combined.get(key);
      if (!accumulator) {
        accumulator = {
          ...source,
          attributedRdps: null,
          damageEventCount: 0,
          unresolvedRelationshipCount: 0,
          exactTotal: 0n,
          hasExact: false,
        };
        combined.set(key, accumulator);
      }
      accumulator.damageEventCount += source.damageEventCount;
      accumulator.unresolvedRelationshipCount += source.unresolvedRelationshipCount;
      if (source.attributedRdps !== null) {
        accumulator.exactTotal += BigInt(source.attributedRdps);
        accumulator.hasExact = true;
      }
    }
  }
  return [...combined.values()]
    .sort((left, right) => {
      if (left.hasExact !== right.hasExact) return left.hasExact ? -1 : 1;
      if (left.exactTotal !== right.exactTotal) {
        return left.exactTotal > right.exactTotal ? -1 : 1;
      }
      return left.providerActorId.localeCompare(
        right.providerActorId,
        undefined,
        { numeric: true },
      );
    })
    .map(({ exactTotal, hasExact, ...source }) => ({
      ...source,
      attributedRdps: hasExact ? exactTotal.toString() : null,
    }));
}

export interface DisplayedAbilityRow {
  ability: DisplayedAbility;
  kind: "standalone" | "recount-parent" | "recount-child";
  childCount: number;
  isLastChild: boolean;
}

function recountParentAbility(
  groupId: string,
  children: readonly DisplayedAbility[],
): DisplayedAbility {
  const total = (select: (ability: DisplayedAbility) => number) =>
    children.reduce((sum, ability) => sum + select(ability), 0);
  const totalNullable = (select: (ability: DisplayedAbility) => number | null) => {
    const values = children.flatMap((ability) => {
      const value = select(ability);
      return value === null ? [] : [value];
    });
    return values.length === 0 ? null : values.reduce((sum, value) => sum + value, 0);
  };
  const groupName = children.find((ability) => ability.recountGroupName?.trim())
    ?.recountGroupName?.trim() ?? null;
  const receivedRdmgExact = sumExactRelativeDamage(children);
  const receivedRdmg = receivedRdmgExact === null ? null : Number(receivedRdmgExact);
  return {
    abilityId: groupId,
    presentationName: groupName ?? `Recount group ${groupId}`,
    presentationKind: "recount-parent",
    presentationResolution: groupName ? "localized" : "unresolved",
    iconAssetPath: children.find((ability) => ability.iconAssetPath)?.iconAssetPath ?? null,
    recountGroupId: null,
    recountGroupName: groupName,
    damage: total((ability) => ability.damage),
    hits: total((ability) => ability.hits),
    casts: total((ability) => ability.casts),
    criticals: total((ability) => ability.criticals),
    dps: total((ability) => ability.dps),
    encounterDps: total((ability) => ability.encounterDps),
    healing: total((ability) => ability.healing),
    effectiveHealing: total((ability) => ability.effectiveHealing),
    shielding: total((ability) => ability.shielding),
    hps: total((ability) => ability.hps),
    receivedRdmgExact,
    receivedRdmg: receivedRdmg !== null && Number.isFinite(receivedRdmg)
      ? receivedRdmg
      : null,
    receivedRdps: totalNullable((ability) => ability.receivedRdps),
    rdpsSources: combineRelativeDamageSources(children),
    rdpsDamageEventCount: total((ability) => ability.rdpsDamageEventCount),
    rdpsUnresolvedRelationshipCount: total(
      (ability) => ability.rdpsUnresolvedRelationshipCount,
    ),
    hasRdpsRelationship: children.some((ability) => ability.hasRdpsRelationship),
  };
}

export function groupDisplayedAbilities(
  abilities: readonly DisplayedAbility[],
  key: AbilitySortKey,
  direction: AbilitySortDirection,
): DisplayedAbilityRow[] {
  const standalone: DisplayedAbility[] = [];
  const groups = new Map<string, DisplayedAbility[]>();
  for (const ability of abilities) {
    const groupId = ability.recountGroupId?.trim();
    if (!groupId) {
      standalone.push(ability);
      continue;
    }
    const children = groups.get(groupId) ?? [];
    children.push(ability);
    groups.set(groupId, children);
  }

  const parents = new Map<string, DisplayedAbility>();
  const parentGroupIds = new Map<DisplayedAbility, string>();
  for (const [groupId, children] of groups) {
    const parent = recountParentAbility(groupId, children);
    parents.set(groupId, parent);
    parentGroupIds.set(parent, groupId);
  }
  const topLevel = sortDisplayedAbilities(
    [...standalone, ...parents.values()],
    key,
    direction,
  );
  const rows: DisplayedAbilityRow[] = [];
  for (const ability of topLevel) {
    const parentGroupId = parentGroupIds.get(ability);
    const children = parentGroupId ? groups.get(parentGroupId) : undefined;
    if (!children) {
      rows.push({ ability, kind: "standalone", childCount: 0, isLastChild: false });
      continue;
    }
    rows.push({
      ability,
      kind: "recount-parent",
      childCount: children.length,
      isLastChild: false,
    });
    const sortedChildren = sortDisplayedAbilities(children, key, direction);
    rows.push(...sortedChildren.map((child, index) => ({
      ability: child,
      kind: "recount-child" as const,
      childCount: 0,
      isLastChild: index === sortedChildren.length - 1,
    })));
  }
  return rows;
}

function abilitySortValue(
  ability: DisplayedAbility,
  key: AbilitySortKey,
): PartySortValue {
  switch (key) {
    case "ability": return ability.presentationName?.trim() || ability.abilityId;
    case "damage": return ability.damage;
    case "rdmgReceived": return ability.receivedRdmg;
    case "rdpsReceived": return ability.receivedRdps;
    case "hits": return ability.hits;
    case "casts": return ability.casts;
    case "criticals": return ability.criticals;
    case "dps": return ability.dps;
    case "encounterDps": return ability.encounterDps;
    case "healing": return ability.healing;
    case "effectiveHealing": return ability.effectiveHealing;
    case "shielding": return ability.shielding;
    case "hps": return ability.hps;
  }
}

export function sortDisplayedAbilities(
  abilities: readonly DisplayedAbility[],
  key: AbilitySortKey,
  direction: AbilitySortDirection,
): DisplayedAbility[] {
  return [...abilities].sort((left, right) => {
    const compared = comparePartySortValues(
      abilitySortValue(left, key),
      abilitySortValue(right, key),
      direction,
    );
    return compared || left.abilityId.localeCompare(right.abilityId, undefined, { numeric: true });
  });
}

export function abilitySortMaximum(
  abilities: readonly DisplayedAbility[],
  key: AbilitySortKey,
): number {
  return abilities.reduce((maximum, ability) => {
    const value = abilitySortValue(ability, key);
    return typeof value === "number" && Number.isFinite(value)
      ? Math.max(maximum, value)
      : maximum;
  }, 0);
}

function playerLayerTimeMetadata(
  run: CombatRunHistory,
): ReadonlyArray<readonly [string, string]> {
  const entireRun = run.views.find((candidate) => candidate.id === "all") ?? run.views[0];
  const trueTime = run.views.find((candidate) => candidate.id === "true_time");
  return [
    ["Run", formatDuration(run.total_run_time_micros ?? totalRunTime(run))],
    ["Game", formatDuration(run.game_time_micros)],
    ["Active", formatDuration(entireRun?.active_combat_micros ?? null)],
    ["True", formatDuration(run.true_time_micros ?? trueTime?.elapsed_micros ?? null)],
    ["Retries", `${run.retry_count} / ${run.boss_retry_count} boss`],
  ];
}

function combatPresentationCell(
  id: string,
  name: string | null,
  kind: string | null,
  resolution: string | null,
  iconPath: string | null,
  namespace: "ability" | "effect",
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-actor combat-history-combat-presentation-cell";
  cell.append(combatPresentationIdentity(id, name, kind, resolution, iconPath, namespace));
  return cell;
}

function combatPresentationIdentity(
  id: string,
  name: string | null,
  kind: string | null,
  resolution: string | null,
  iconPath: string | null,
  namespace: "ability" | "effect",
): HTMLElement {
  const identity = element("span", "combat-history-combat-presentation");
  const icon = element("span", "combat-history-combat-icon", iconPath ? "" : "?");
  icon.dataset.state = iconPath ? "resolved" : "unresolved";
  if (iconPath) {
    const image = document.createElement("img");
    image.src = iconPath;
    image.alt = "";
    image.draggable = false;
    icon.append(image);
  }
  const kindLabel = kind ? humanizePresentationKind(kind) : null;
  const unresolvedLabel = namespace === "ability" ? "Unresolved action" : "Unresolved effect";
  const copy = element(
    "span",
    "combat-history-combat-copy",
    element("strong", "", name?.trim() || unresolvedLabel),
    element(
      "span",
      "combat-history-combat-metadata",
      `${namespace === "ability" ? "ID" : "Effect"} ${id}${kindLabel ? ` · ${kindLabel}` : ""}`,
    ),
  );
  identity.dataset.resolution = resolution ?? "unresolved";
  identity.append(icon, copy);
  return identity;
}

function humanizePresentationKind(kind: string): string {
  const words = kind.replace(/[_-]+/g, " ").trim();
  return words ? words[0]!.toUpperCase() + words.slice(1) : "";
}

function renderMetricGraph(
  actors: HistoryActorSummary[],
  definition: GraphDefinition,
  elapsedMicros: number,
  hiddenActorIds: ReadonlySet<string>,
  actorColors: ReadonlyMap<string, string>,
  targetActorId: string | null,
  selectMetric: (metric: GraphMetric) => void,
): HTMLElement {
  const card = element("section", "combat-history-metric-graph");
  const durationSeconds = Math.max(1, Math.ceil(elapsedMicros / 1_000_000));
  const allSeries = actors
    .map((actor) =>
      buildActorGraphSeries(
        actor,
        definition.metric,
        durationSeconds,
        actorColors.get(actor.actor_id) ?? graphColor(actors.indexOf(actor)),
        targetActorId,
      ),
    )
    .filter((entry) =>
      entry.peak > 0 || (targetActorId === null && entry.actor.death_seconds.length > 0),
    );
  const visibleSeries = allSeries.filter(
    (entry) => !hiddenActorIds.has(entry.actor.actor_id),
  );
  const scaleMaximum = graphScaleMaximum(
    allSeries.map((entry) => entry.values),
  );
  card.append(
    element(
      "div",
      "combat-history-graph-heading",
      element("div", "", element("h3", "", definition.title), element("p", "", definition.description)),
      renderGraphMetricToggle(definition.metric, selectMetric),
    ),
  );
  if (allSeries.length === 0) {
    card.append(
      element(
        "p",
        "runtime-empty-result",
        `No ${definition.rateLabel} values are available in this segment.`,
      ),
    );
    return card;
  }
  if (visibleSeries.length === 0) {
    card.append(
      element(
        "p",
        "combat-history-graph-note",
        "Every party line is hidden. The run-scale axes remain fixed so re-enabling a line does not change the measurements.",
      ),
    );
  }
  card.append(
    partyLineChart(
      visibleSeries,
      definition,
      durationSeconds,
      scaleMaximum,
      targetActorId === null,
    ),
  );
  const stats = element("div", "combat-history-graph-stats");
  for (const entry of visibleSeries) {
    const item = element("div", "");
    item.style.setProperty("--series-color", entry.color);
    item.dataset.actorKind = graphActorKind(entry.actor);
    item.append(
      element("strong", "combat-history-graph-stat-name", actorLabel(entry.actor)),
      element(
        "span",
        "combat-history-graph-stat-metric",
        element("span", "combat-history-graph-stat-label", "Avg."),
        element("span", "combat-history-graph-stat-value", COMPACT.format(entry.average)),
      ),
      element(
        "span",
        "combat-history-graph-stat-metric",
        element("span", "combat-history-graph-stat-label", "Peak"),
        element("span", "combat-history-graph-stat-value", COMPACT.format(entry.peak)),
      ),
    );
    stats.append(item);
  }
  card.append(stats);
  return card;
}

function renderGraphMetricToggle(
  selectedMetric: GraphMetric,
  selectMetric: (metric: GraphMetric) => void,
): HTMLElement {
  const toggle = element("div", "combat-history-graph-metric-toggle");
  toggle.setAttribute("role", "group");
  toggle.setAttribute("aria-label", "Timeline metric");
  for (const definition of GRAPH_METRICS) {
    const option = button(definition.rateLabel, "");
    const selected = definition.metric === selectedMetric;
    option.dataset.selected = String(selected);
    option.setAttribute("aria-pressed", String(selected));
    option.addEventListener("click", () => selectMetric(definition.metric));
    toggle.append(option);
  }
  return toggle;
}

export function buildActorGraphSeries(
  actor: HistoryActorSummary,
  metric: GraphMetric,
  durationSeconds: number,
  color: string,
  targetActorId: string | null,
): ActorGraphSeries {
  const raw = Array.from({ length: durationSeconds + 1 }, () => 0);
  const points = targetActorId === null
    ? actor.series
    : actor.targets.find((target) => target.actor_id === targetActorId)?.series ?? [];
  for (const point of points) {
    const second = Math.min(durationSeconds, Math.max(0, point.second));
    raw[second] = (raw[second] ?? 0) + point[metric];
  }
  const values = movingAverage(raw, 5);
  const total = raw.reduce((sum, value) => sum + value, 0);
  return {
    actor,
    color,
    values,
    average: total / durationSeconds,
    peak: values.reduce((maximum, value) => Math.max(maximum, value), 0),
  };
}

function movingAverage(values: number[], windowSeconds: number): number[] {
  let running = 0;
  return values.map((value, index) => {
    running += value;
    const expired = index - windowSeconds;
    if (expired >= 0) running -= values[expired] ?? 0;
    return running / Math.min(index + 1, windowSeconds);
  });
}

function partyLineChart(
  series: ActorGraphSeries[],
  definition: GraphDefinition,
  durationSeconds: number,
  scaleMaximum: number,
  showDeathMarkers: boolean,
): SVGSVGElement {
  const width = 1_120;
  const height = 330;
  const left = 78;
  const right = 24;
  const top = 22;
  const bottom = 48;
  const plotWidth = width - left - right;
  const plotHeight = height - top - bottom;
  const scale = niceScale(scaleMaximum, 4);
  const timeTicks = graphTimeTicks(durationSeconds);
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.classList.add("combat-history-chart");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("role", "img");
  svg.setAttribute(
    "aria-label",
    `${definition.title}, ${definition.rateLabel} by character over ${formatGraphTime(durationSeconds)}.`,
  );

  const xFor = (second: number) =>
    left + (Math.min(durationSeconds, Math.max(0, second)) / durationSeconds) * plotWidth;
  const yFor = (value: number) =>
    top + plotHeight - (Math.min(scale.maximum, Math.max(0, value)) / scale.maximum) * plotHeight;

  for (const tick of scale.ticks) {
    const y = yFor(tick);
    svg.append(
      svgNode("line", "combat-history-grid-line", {
        x1: left,
        x2: width - right,
        y1: y,
        y2: y,
      }),
      svgText(left - 10, y + 4, COMPACT.format(tick), "combat-history-y-label", "end"),
    );
  }
  for (const tick of timeTicks) {
    const x = xFor(tick);
    svg.append(
      svgNode("line", "combat-history-grid-line combat-history-grid-line-time", {
        x1: x,
        x2: x,
        y1: top,
        y2: top + plotHeight,
      }),
      svgText(x, height - 24, formatGraphTime(tick), "combat-history-x-label", "middle"),
    );
  }
  svg.append(
    svgText(width / 2, height - 5, "Run time", "combat-history-axis-title", "middle"),
  );
  const yTitle = svgText(15, top + plotHeight / 2, definition.rateLabel, "combat-history-axis-title", "middle");
  yTitle.setAttribute("transform", `rotate(-90 15 ${top + plotHeight / 2})`);
  svg.append(yTitle);

  for (const entry of series) {
    const points = entry.values
      .map((value, second) => `${xFor(second).toFixed(2)},${yFor(value).toFixed(2)}`)
      .join(" ");
    const polyline = svgNode("polyline", "combat-history-character-line", {
      points,
      fill: "none",
      stroke: entry.color,
      "stroke-width": graphActorKind(entry.actor) === "npc" ? 2.7 : 2.35,
      "vector-effect": "non-scaling-stroke",
    });
    if (graphActorKind(entry.actor) === "npc") {
      polyline.setAttribute("stroke-dasharray", "10 7");
    }
    polyline.append(
      svgTitle(
        `${actorLabel(entry.actor)} · avg ${NUMBER.format(entry.average)} · peak ${NUMBER.format(entry.peak)} ${definition.rateLabel}`,
      ),
    );
    svg.append(polyline);
    if (!showDeathMarkers) continue;
    for (const deathSecond of entry.actor.death_seconds) {
      const second = Math.min(durationSeconds, deathSecond);
      const value = entry.values[Math.round(second)] ?? 0;
      svg.append(deathMarker(xFor(second), yFor(value), actorLabel(entry.actor), second));
    }
  }
  return svg;
}

export function graphScaleMaximum(
  valueSeries: readonly (readonly number[])[],
): number {
  let maximum = 1;
  for (const values of valueSeries) {
    for (const value of values) maximum = Math.max(maximum, value);
  }
  return maximum;
}

export function historyTargetLabel(target: HistoryTargetIdentity): string {
  const localizedName = target.presentation_name?.trim();
  const packetName = target.display_name?.trim();
  if (localizedName || packetName) {
    return `${localizedName || packetName} · Entity ${target.entity_uuid}`;
  }
  const kind = formatIdentifier(target.actor_kind?.trim() || "entity");
  return target.monster_id
    ? `${kind} ${target.monster_id} · Entity ${target.entity_uuid}`
    : `${kind} · Entity ${target.entity_uuid}`;
}

function deathMarker(x: number, lineY: number, actorName: string, second: number): SVGGElement {
  const y = Math.min(268, Math.max(16, lineY));
  const group = svgNode("g", "combat-history-death-marker", {
    transform: `translate(${x.toFixed(2)} ${y.toFixed(2)})`,
  });
  group.append(
    svgNode("circle", "combat-history-death-marker-halo", { cx: 0, cy: 0, r: 10 }),
    svgText(0, 5.5, "☠", "combat-history-death-marker-skull", "middle"),
    svgTitle(`${actorName} died at ${formatGraphTime(second)}`),
  );
  return group;
}

function graphTimeTicks(durationSeconds: number): number[] {
  const choices = [1, 2, 5, 10, 15, 30, 60, 120, 180, 300, 600, 900];
  const desired = durationSeconds / 6;
  const step = choices.find((candidate) => candidate >= desired) ?? choices.at(-1)!;
  const ticks = [0];
  for (let second = step; second < durationSeconds; second += step) ticks.push(second);
  if (ticks.at(-1) !== durationSeconds) ticks.push(durationSeconds);
  return ticks;
}

function niceScale(maximum: number, desiredSteps: number): { maximum: number; ticks: number[] } {
  const roughStep = maximum / Math.max(1, desiredSteps);
  const magnitude = 10 ** Math.floor(Math.log10(Math.max(roughStep, 1)));
  const normalized = roughStep / magnitude;
  const step = (normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10) * magnitude;
  const scaledMaximum = Math.max(step, Math.ceil(maximum / step) * step);
  const ticks: number[] = [];
  for (let value = 0; value <= scaledMaximum + step * 0.001; value += step) ticks.push(value);
  return { maximum: scaledMaximum, ticks };
}

function formatGraphTime(second: number): string {
  const totalSeconds = Math.max(0, Math.round(second));
  const minutes = Math.floor(totalSeconds / 60);
  return `${minutes}:${(totalSeconds % 60).toString().padStart(2, "0")}`;
}

function graphColor(index: number): string {
  return HISTORY_PARTY_PALETTE[
    ((index % HISTORY_PARTY_PALETTE.length) + HISTORY_PARTY_PALETTE.length) %
      HISTORY_PARTY_PALETTE.length
  ]!;
}

export function historyActorColor(
  actor: Pick<HistoryActorSummary, "actor_id" | "specialization_id">,
  index: number,
  settings: Pick<
    CombatMeterSettings,
    "historyPartyColorMode" | "historySpecializationColors"
  >,
  runSeed: string,
): string {
  if (settings.historyPartyColorMode === "randomized") {
    return historySeededPaletteColor(`run:${runSeed}`, index);
  }
  if (settings.historyPartyColorMode === "specialization") {
    if (actor.specialization_id !== null) {
      const key = String(actor.specialization_id);
      return settings.historySpecializationColors[key] ??
        historySpecializationFallbackColor(key);
    }
    return historySeededPaletteColor(`run:${runSeed}:unresolved`, index);
  }
  return graphColor(index);
}

function historyActorColors(
  actors: HistoryActorSummary[],
  settings: CombatMeterSettings,
  runSeed: string,
): ReadonlyMap<string, string> {
  return new Map(
    actors.map((actor, index) => [
      actor.actor_id,
      historyActorColor(actor, index, settings, runSeed),
    ]),
  );
}

function graphActorKind(actor: HistoryActorSummary): "player" | "npc" {
  return actor.presentation_kind === "party_npc" || actor.actor_kind === "npc"
    ? "npc"
    : "player";
}

function actorIdentityLabel(actor: HistoryActorSummary): string {
  const identity = [
    actor.presentation_class_name,
    actor.presentation_specialization_name
      ? compactSpecializationName(actor.presentation_specialization_name)
      : null,
  ].filter((value): value is string => Boolean(value?.trim()));
  if (identity.length > 0) return identity.join(" · ");
  return graphActorKind(actor) === "npc" ? "Party NPC" : "Party member";
}

function svgNode<K extends keyof SVGElementTagNameMap>(
  tag: K,
  className: string,
  attributes: Record<string, string | number>,
): SVGElementTagNameMap[K] {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  if (className) node.setAttribute("class", className);
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, String(value));
  return node;
}

function svgText(
  x: number,
  y: number,
  value: string,
  className: string,
  anchor: "start" | "middle" | "end",
): SVGTextElement {
  const node = svgNode("text", className, { x, y, "text-anchor": anchor });
  node.textContent = value;
  return node;
}

function svgTitle(value: string): SVGTitleElement {
  const node = document.createElementNS("http://www.w3.org/2000/svg", "title");
  node.textContent = value;
  return node;
}

export function filterAndSortHistoryEntries(
  entries: readonly CombatHistoryCatalogEntry[],
  query: string,
  difficulty: string,
  sort: HistorySort,
  favoritesOnly = false,
): CombatHistoryCatalogEntry[] {
  const needle = query.trim().toLocaleLowerCase();
  const filtered = entries.filter((entry) => {
    if (favoritesOnly && !entry.is_favorite) return false;
    if (difficulty !== "all" && difficultyFilterKey(entry) !== difficulty) return false;
    if (!needle) return true;
    const participantText = entry.participants
      .flatMap((participant) => [
        participant.presentation_name,
        participant.display_name,
        participant.character_id,
        participant.entity_uuid,
        participant.class_id?.toString() ?? null,
        participant.specialization_id?.toString() ?? null,
      ])
      .filter((value): value is string => value !== null)
      .join(" ");
    return [
      activityLabel(entry),
      entry.activity_id,
      entry.activity_family_id,
      entry.scene_id?.toString() ?? null,
      difficultyLabel(entry.difficulty_family, entry.difficulty_tier),
      entry.terminal_state,
      entry.deployment_id,
      entry.region_id,
      entry.world_id,
      participantText,
    ]
      .filter((value): value is string => value !== null)
      .join(" ")
      .toLocaleLowerCase()
      .includes(needle);
  });
  return filtered.sort((left, right) => {
    switch (sort) {
      case "oldest":
        return left.captured_unix_millis - right.captured_unix_millis;
      case "fastest":
        return historyRunTime(left) - historyRunTime(right);
      case "team_dps":
        return right.team_dps - left.team_dps;
      case "team_edps":
        return right.team_encounter_dps - left.team_encounter_dps;
      case "newest":
      default:
        return right.captured_unix_millis - left.captured_unix_millis;
    }
  });
}

function historyRunTime(entry: CombatHistoryCatalogEntry): number {
  return entry.total_run_time_micros ?? entry.game_time_micros ?? Number.MAX_SAFE_INTEGER;
}

function difficultyFilterKey(
  entry: Pick<CombatHistoryCatalogEntry, "difficulty_family" | "difficulty_tier">,
): string {
  return `${entry.difficulty_family ?? "unresolved"}:${entry.difficulty_tier ?? ""}`;
}

function uniqueDifficultyFilters(
  entries: readonly CombatHistoryCatalogEntry[],
): Array<[string, string]> {
  const options = new Map<string, string>();
  for (const entry of entries) {
    options.set(
      difficultyFilterKey(entry),
      difficultyLabel(entry.difficulty_family, entry.difficulty_tier),
    );
  }
  return [...options.entries()].sort((left, right) => left[1].localeCompare(right[1]));
}

function selectControl(
  accessibleLabel: string,
  options: ReadonlyArray<readonly [string, string]>,
  selected: string,
  onChange: (value: string) => void,
): HTMLLabelElement {
  const label = element("label", "combat-history-browser-select");
  const select = document.createElement("select");
  select.setAttribute("aria-label", accessibleLabel);
  for (const [value, text] of options) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = text;
    option.selected = value === selected;
    select.append(option);
  }
  select.addEventListener("change", () => onChange(select.value));
  label.append(select);
  return label;
}

function renderCatalogParty(
  participants: readonly CombatHistoryParticipant[],
  playerCount: number,
  settings: CombatMeterSettings,
): HTMLElement {
  const party = element("span", "combat-history-run-party");
  party.setAttribute("aria-label", `${playerCount} party members`);
  if (!settings.showPartyIcons) {
    party.append(element("span", "combat-history-run-party-count", `${playerCount} players`));
    return party;
  }
  const visibleParticipants = participants.slice(0, 5);
  party.title = participants.map(catalogParticipantTooltip).join("\n");
  for (const participant of visibleParticipants) {
    const icon = renderHistoryIcon(participant, "combat-history-run-party-icon");
    icon.dataset.actorKind = participant.presentation_kind ?? participant.actor_kind ?? "player";
    icon.title = catalogParticipantTooltip(participant);
    party.append(icon);
  }
  if (participants.length === 0) {
    party.append(element("span", "combat-history-run-party-count", `${playerCount} players`));
  } else if (Math.max(playerCount, participants.length) > visibleParticipants.length) {
    party.append(
      element(
        "span",
        "combat-history-run-party-count",
        `+${Math.max(playerCount, participants.length) - visibleParticipants.length}`,
      ),
    );
  }
  return party;
}

export function catalogParticipantLabel(participant: CombatHistoryParticipant): string {
  const displayName = participant.display_name?.trim();
  const presentationName = participant.presentation_name?.trim();
  const kind = participant.presentation_kind ?? participant.actor_kind ?? "player";
  return kind === "player"
    ? displayName || presentationName || `Player ${participant.actor_id}`
    : presentationName || displayName || `Actor ${participant.actor_id}`;
}

export function catalogParticipantTooltip(participant: CombatHistoryParticipant): string {
  const identity = participant.character_id
    ? `UID ${participant.character_id}`
    : `Entity UUID ${participant.entity_uuid}`;
  const className =
    participant.presentation_class_name?.trim() ||
    (participant.class_id == null ? undefined : `Class ${participant.class_id}`);
  const specializationName =
    participant.presentation_specialization_name?.trim() ||
    (participant.specialization_id == null
      ? undefined
      : `Spec ${participant.specialization_id}`);
  return [catalogParticipantLabel(participant), identity, className, specializationName]
    .filter((value): value is string => Boolean(value))
    .join(" \u00b7 ");
}

function metricValue(value: number): HTMLElement {
  const metric = element("span", "combat-history-run-metric", NUMBER.format(value));
  metric.title = INTEGER.format(value);
  return metric;
}

export function activityLabel(entry: Pick<CombatHistoryCatalogEntry, "activity_id" | "activity_family_id" | "scene_id" | "presentation_scene_name">): string {
  const localizedScene = entry.presentation_scene_name?.trim();
  if (localizedScene) return localizedScene;
  const source = entry.activity_family_id ?? entry.activity_id;
  if (source) return formatIdentifier(source);
  return entry.scene_id === null ? "Unresolved dungeon" : `Scene ${entry.scene_id}`;
}

type ActivityDifficultyIdentity = Pick<
  CombatHistoryCatalogEntry,
  | "activity_id"
  | "activity_family_id"
  | "scene_id"
  | "presentation_scene_name"
  | "difficulty_family"
  | "difficulty_tier"
>;

type RunStatusIdentity = ActivityDifficultyIdentity & {
  wipe_count?: number;
  cleared_encounter_count?: number;
  last_encounter_terminal_state?: string | null;
};

const DIFFICULTY_LESS_ACTIVITY_FAMILIES = new Set(["stimen-vaults"]);

export function supplementalDifficultyLabel(
  entry: ActivityDifficultyIdentity,
): string | null {
  if (
    entry.difficulty_family === null &&
    entry.difficulty_tier === null &&
    entry.activity_family_id !== null &&
    DIFFICULTY_LESS_ACTIVITY_FAMILIES.has(entry.activity_family_id)
  ) {
    return null;
  }
  const activity = normalizedLabel(activityLabel(entry));
  const difficulty = normalizedLabel(
    difficultyLabel(entry.difficulty_family, entry.difficulty_tier),
  );
  if (
    activity.length > 0 &&
    difficulty.length > 0 &&
    ` ${activity} `.includes(` ${difficulty} `)
  ) {
    return null;
  }
  return difficultyLabel(entry.difficulty_family, entry.difficulty_tier);
}

function activityContextLabel(entry: ActivityDifficultyIdentity): string {
  return [activityLabel(entry), supplementalDifficultyLabel(entry)]
    .filter((value): value is string => value !== null)
    .join(" · ");
}

function runStatusLabel(
  entry: RunStatusIdentity,
  terminalState: string,
  retryCount = 0,
): string {
  const retryLabel = retryCount === 0
    ? null
    : `${retryCount} retr${retryCount === 1 ? "y" : "ies"}`;
  const terminalLabel =
    (entry.wipe_count ?? 0) > 0 &&
    (entry.cleared_encounter_count ?? 0) === 0 &&
    terminalState !== "completed"
      ? "Wiped"
      : terminalPresentationLabel(terminalState);
  return [
    supplementalDifficultyLabel(entry),
    terminalLabel,
    retryLabel,
  ]
    .filter((value): value is string => value !== null)
    .join(" · ");
}

export function terminalPresentationLabel(terminalState: string): string {
  if (terminalState === "exited") return "Failed (Exited)";
  return formatIdentifier(terminalState);
}

function normalizedLabel(value: string): string {
  return value
    .normalize("NFKC")
    .toLocaleLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

function difficultyLabel(family: string | null, tier: number | null): string {
    if (family === "master") {
      return tier === null ? "Master (tier unresolved)" : `Master ${tier}`;
    }
  return family ? formatIdentifier(family) : "Difficulty unresolved";
}

function actorLabel(actor: HistoryActorSummary): string {
  return (
    actor.presentation_name?.trim() ||
    actor.display_name?.trim() ||
    `${graphActorKind(actor) === "npc" ? "Party NPC" : "Player"} ${actor.actor_id}`
  );
}

function identityCell(
  actor: HistoryActorSummary,
  settings: CombatMeterSettings,
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-actor";
  cell.title = actor.character_id
    ? `Character UID ${actor.character_id} | Entity UUID ${actor.entity_uuid}`
    : `Entity UUID ${actor.entity_uuid}`;
  const name = element(
    "span",
    "combat-history-actor-name",
    element("strong", "", actorLabel(actor)),
  );
  if (graphActorKind(actor) === "npc") {
    const kind = element("span", "combat-history-actor-kind", "NPC");
    kind.dataset.actorKind = "npc";
    name.append(kind);
  }

  const metadata: string[] = [];
  if (settings.showClass && actor.class_id !== null) {
    metadata.push(actor.presentation_class_name?.trim() || `Class ${actor.class_id}`);
  }
  if (settings.showSpecialization && actor.specialization_id !== null) {
    metadata.push(compactSpecializationName(
      actor.presentation_specialization_name?.trim() || String(actor.specialization_id),
    ));
  }
  if (settings.showLevel) {
    metadata.push(actor.level === null ? "Lv. ?" : `Lv. ${actor.level}`);
  }
  if (settings.showAbilityScore) {
    metadata.push(
      actor.ability_score === null
        ? "AS ?"
        : `AS ${INTEGER.format(actor.ability_score)}`,
    );
  }
  if (settings.showSeasonalScore) {
    metadata.push(
      actor.seasonal_score === null
        ? "SS ?"
        : `SS ${INTEGER.format(actor.seasonal_score)}`,
    );
  }
  if (settings.showCharacterUid) {
    metadata.push(actor.character_id ? `UID ${actor.character_id}` : "UID ?");
  }

  const copy = element("div", "combat-history-actor-copy", name);
  if (metadata.length > 0) {
    copy.append(element("span", "combat-history-actor-metadata", metadata.join(" | ")));
  }
  const loadout = renderPartyLoadout(actor, settings);
  if (loadout) copy.append(loadout);
  const identity = element("div", "combat-history-actor-identity");
  if (settings.showPartyIcons) {
    identity.append(renderHistoryIcon(actor, "combat-history-player-icon"));
  }
  identity.append(copy);
  cell.append(identity);
  return cell;
}

export function compactSpecializationName(value: string): string {
  return value.replace(/\s+Spec$/i, "").trim();
}

function renderPartyLoadout(
  actor: HistoryActorSummary,
  settings: CombatMeterSettings,
): HTMLElement | null {
  const groups: HTMLElement[] = [];
  if (settings.showWeapon) {
    const weapon = actor.weapon_item_id === null
      ? unresolvedLoadoutSlot("Weapon not observed in this team snapshot", "weapon")
      : observedWeaponSlot(actor);
    groups.push(element("span", "combat-history-loadout-group", weapon));
  }
  if (settings.showPrimaryImagines) {
    const primarySlots = [...actor.primary_loadout].sort(
      (left, right) => left.slot_id - right.slot_id,
    );
    groups.push(
      element(
        "span",
        "combat-history-loadout-group",
        ...Array.from({ length: 2 }, (_, index) => {
          const observed = primarySlots[index];
          return observed
            ? renderObservedLoadoutSlot(observed, "imagine")
            : unresolvedLoadoutSlot(
                `Primary Imagine ${index + 1} not observed in this team snapshot`,
                "imagine",
              );
        }),
      ),
    );
  }
  if (settings.showRoleLoadout) {
    const observedRoleSlots = [...actor.auxiliary_loadout].sort(
      (left, right) => left.slot_id - right.slot_id,
    );
    const roleSlots = Array.from({ length: 4 }, (_, index) => {
      const observed = observedRoleSlots[index];
      return observed
        ? renderObservedLoadoutSlot(observed, "role_slot")
        : unresolvedLoadoutSlot(
            `Role slot ${index + 1} not observed in this team snapshot`,
            "role_slot",
          );
    });
    groups.push(element("span", "combat-history-loadout-group", ...roleSlots));
  }
  if (groups.length === 0) return null;

  const row = element("span", "combat-history-loadout-row");
  groups.forEach((group, index) => {
    if (index > 0) {
      row.append(element("span", "combat-history-loadout-separator", "|"));
    }
    row.append(group);
  });
  return row;
}

function observedWeaponSlot(actor: HistoryActorSummary): HTMLElement {
  const itemId = actor.weapon_item_id!;
  const levelLabel = actor.weapon_level !== null
    ? `Lv. ${actor.weapon_level}`
    : actor.weapon_level_min !== null && actor.weapon_level_max !== null
      ? `Lv. ${actor.weapon_level_min}-${actor.weapon_level_max}`
      : "Level not observed";
  const weaponName = actor.weapon_presentation_name ?? `Equipped weapon item ${itemId}`;
  const tooltip = `${weaponName} | ${levelLabel} | Item ${itemId}`;
  if (!actor.weapon_icon_asset_path) {
    const unresolved = unresolvedLoadoutSlot(
      `${tooltip}; exact equipped-item artwork not mapped yet`,
      "weapon",
    );
    unresolved.dataset.itemId = String(itemId);
    unresolved.dataset.state = "observed_icon_unresolved";
    return unresolved;
  }

  const weapon = element("span", "combat-history-loadout-slot");
  weapon.dataset.kind = "weapon";
  weapon.dataset.itemId = String(itemId);
  weapon.dataset.state = "resolved";
  if (actor.weapon_badge_kind) weapon.dataset.badgeKind = actor.weapon_badge_kind;
  weapon.title = tooltip;
  weapon.setAttribute("aria-label", `${weaponName}, ${levelLabel}`);
  const icon = document.createElement("img");
  icon.src = actor.weapon_icon_asset_path;
  icon.alt = "";
  weapon.append(icon);
  if (actor.weapon_level !== null) {
    weapon.append(element("span", "combat-history-loadout-tier", String(actor.weapon_level)));
  } else if (actor.weapon_level_min !== null && actor.weapon_level_max !== null) {
    weapon.append(element("span", "combat-history-weapon-range", `${actor.weapon_level_min}-${actor.weapon_level_max}`));
  }
  return weapon;
}

function renderObservedLoadoutSlot(
  slot: HistoryActorSummary["primary_loadout"][number],
  kind: string,
): HTMLElement {
  if (!slot.icon_asset_path) {
    const unresolved = unresolvedLoadoutSlot(
      `${slot.presentation_name ?? "Equipped item"} observed; icon not mapped yet`,
      kind,
    );
    unresolved.dataset.state = "observed_icon_unresolved";
    if (slot.ability_id !== null) unresolved.dataset.abilityId = String(slot.ability_id);
    if (slot.item_id !== null) unresolved.dataset.itemId = String(slot.item_id);
    return unresolved;
  }

  const name = slot.presentation_name?.trim() || `Equipped item ${slot.item_id ?? "?"}`;
  const presentedTier = loadoutTierForPresentation(slot, kind);
  const tier =
    kind === "role_slot" && slot.item_id === null
      ? "Native role skill"
      : presentedTier === null
      ? "Tier not observed"
      : presentedTier === 0
        ? "Base (no tier)"
        : `Tier ${presentedTier}`;
  const rendered = element("span", "combat-history-loadout-slot");
  rendered.dataset.kind = kind;
  rendered.dataset.state = "resolved";
  if (presentedTier !== null) rendered.dataset.tier = String(presentedTier);
  if (slot.ability_id !== null) rendered.dataset.abilityId = String(slot.ability_id);
  if (slot.item_id !== null) rendered.dataset.itemId = String(slot.item_id);
  rendered.title = `${name} | ${tier}`;
  rendered.setAttribute("aria-label", `${name}, ${tier}`);

  const image = document.createElement("img");
  image.src = slot.icon_asset_path;
  image.alt = "";
  image.draggable = false;
  rendered.append(image);
  if (presentedTier !== null && presentedTier > 0) {
    rendered.append(element("span", "combat-history-loadout-tier", String(presentedTier)));
  }
  return rendered;
}

export function loadoutTierForPresentation(
  slot: HistoryActorSummary["primary_loadout"][number],
  kind: string,
): number | null {
  if (kind !== "role_slot") return slot.tier;
  if (slot.item_id === null) return null;
  return slot.tier !== null && slot.tier >= 1 && slot.tier <= 4 ? slot.tier : null;
}

function unresolvedLoadoutSlot(title: string, kind: string): HTMLElement {
  const slot = element("span", "combat-history-loadout-slot", "?");
  slot.dataset.kind = kind;
  slot.dataset.state = "unresolved";
  slot.title = title;
  slot.setAttribute("aria-label", title);
  return slot;
}

interface HistoryIconPresentation {
  icon_asset_path: string | null;
  presentation_role: string | null;
  presentation_accent: string | null;
  class_id: number | null;
}

function renderHistoryIcon(
  actor: HistoryIconPresentation,
  className: string,
): HTMLElement {
  const icon = element("span", className);
  icon.dataset.state = actor.icon_asset_path ? "resolved" : "fallback";
  icon.dataset.presentationRole = actor.presentation_role ?? "unresolved";
  icon.dataset.presentationAccent = actor.presentation_accent ?? "none";
  if (actor.icon_asset_path) {
    const glyph = element("span", "combat-history-icon-glyph");
    const image = `url("${actor.icon_asset_path}")`;
    glyph.style.setProperty("mask-image", image);
    glyph.style.setProperty("-webkit-mask-image", image);
    icon.append(glyph);
  } else {
    icon.title = "Icon not resolved from the captured data";
    icon.setAttribute("aria-label", "Icon unresolved");
    icon.append(element("span", "combat-history-icon-fallback", "?"));
  }
  return icon;
}

function applyHistorySizing(root: HTMLElement, settings: CombatMeterSettings): void {
  root.style.setProperty("--history-body-font-size", `${settings.historyBodyFontSizePx}px`);
  root.style.setProperty("--history-heading-font-size", `${settings.historyHeadingFontSizePx}px`);
  root.style.setProperty("--history-table-font-size", `${settings.historyTableFontSizePx}px`);
  root.style.setProperty("--history-metadata-font-size", `${settings.historyMetadataFontSizePx}px`);
  root.style.setProperty("--history-metric-font-size", `${settings.historyMetricFontSizePx}px`);
  root.style.setProperty("--history-icon-size", `${settings.historyIconSizePx}px`);
}

function metricGrid(items: Array<[string, string]>): HTMLElement {
  const grid = element("div", "metric-grid combat-history-timers");
  for (const [value, label] of items) {
    grid.append(element("article", "", element("span", "", label), element("strong", "", value)));
  }
  return grid;
}

function renderHistoryRdpsProgress(
  progress: HistoryRdpsRefreshProgress | undefined,
): HTMLElement {
  const presentation = historyRdpsProgressPresentation(progress);
  const { stageLabel, percent, details } = presentation;
  const stage = progress?.stage ?? "queued";
  const card = element(
    "section",
    "content-card combat-history-rdps-progress",
    element(
      "div",
      "card-heading",
      element(
        "div",
        "",
        element("span", "run-report-kicker", "One-time saved rDPS update"),
        element("h2", "", stageLabel),
      ),
      element(
        "span",
        "state-pill",
        percent === null ? stageLabel : `${Math.floor(percent)}%`,
      ),
    ),
  );
  const meter = document.createElement("progress");
  meter.className = "combat-history-rdps-progress-meter";
  meter.max = 100;
  if (percent !== null && stage !== "waiting_for_live_capture") meter.value = percent;
  meter.setAttribute("aria-label", "Archived rDPS calculation progress");
  card.append(meter, element("p", "card-copy", details));
  return card;
}

export function historyRdpsProgressPresentation(
  progress: HistoryRdpsRefreshProgress | undefined,
): { stageLabel: string; percent: number | null; details: string } {
  const stage = progress?.stage ?? "queued";
  const stageLabel = (() => {
    switch (stage) {
      case "queued": return "Queued for this run";
      case "waiting_for_live_capture": return "Paused while live capture is active";
      case "replaying": return "Replaying sealed combat events";
      case "validating_and_saving": return "Validating conservation and saving";
      case "failed": return "Could not refresh this run";
    }
  })();
  const totalBytes = progress?.total_bytes ?? 0;
  const processedBytes = Math.min(progress?.processed_bytes ?? 0, totalBytes);
  const percent = totalBytes > 0
    ? Math.min(100, Math.max(0, (processedBytes / totalBytes) * 100))
    : null;
  const details: string[] = [];
  if ((progress?.processed_events ?? 0) > 0) {
    details.push(`${INTEGER.format(progress!.processed_events)} canonical events processed`);
  }
  if (totalBytes > 0) {
    details.push(`${formatByteCount(processedBytes)} of ${formatByteCount(totalBytes)} read`);
  }
  if (stage === "waiting_for_live_capture") {
    details.push("This saved-log calculation will resume after capture stops");
  } else if (stage === "failed") {
    details.push(progress?.detail ?? "The sealed log could not be replayed and validated");
  } else {
    details.push("The result is written back once and later opens use the saved projection");
  }
  return { stageLabel, percent, details: details.join(" · ") };
}

function formatByteCount(value: number): string {
  if (value < 1_024) return `${INTEGER.format(value)} B`;
  if (value < 1_048_576) return `${NUMBER.format(value / 1_024)} KiB`;
  if (value < 1_073_741_824) return `${NUMBER.format(value / 1_048_576)} MiB`;
  return `${NUMBER.format(value / 1_073_741_824)} GiB`;
}

function numeric(value: number | null, integer = false): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number";
  cell.textContent = value === null ? "—" : integer ? INTEGER.format(value) : NUMBER.format(value);
  return cell;
}

function rdpsNumeric(
  value: number | null,
  integer = false,
  applicable = true,
  incomplete = false,
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number";
  if (!applicable) {
    cell.textContent = "—";
  } else if (value === null) {
    cell.textContent = "Unresolved";
    cell.title = "The exact rDPS total is incomplete; ordinary damage remains available.";
  } else {
    cell.textContent = rdpsDisplay(value, integer, incomplete);
    if (incomplete) {
      cell.title = "Reconstructed packet-proven subtotal; one or more remote formula inputs remain unresolved.";
    }
  }
  return cell;
}

function rdpsDisplay(value: number | null, integer: boolean, incomplete: boolean): string {
  if (value === null) return "Unresolved";
  const formatted = integer ? INTEGER.format(value) : NUMBER.format(value);
  return incomplete ? `≈${formatted}` : formatted;
}

function relativeDamageSkillCell(
  ability: DisplayedAbility,
  view: CombatHistoryView,
  metric: "damage" | "rate",
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number combat-history-relative-damage-cell";
  if (!ability.hasRdpsRelationship) {
    cell.textContent = "—";
    cell.title = "No conserved rDPS relationship has been calculated for this skill.";
    return cell;
  }
  if (ability.receivedRdmgExact === null) {
    cell.textContent = "Unresolved";
    cell.title = "A support relationship was observed, but its exact relative damage is unresolved.";
    return cell;
  }

  cell.textContent = metric === "damage"
    ? formatExactInteger(ability.receivedRdmgExact)
    : ability.receivedRdps === null ? "Unresolved" : NUMBER.format(ability.receivedRdps);
  const totalRate = ability.receivedRdps === null
    ? "Unresolved"
    : NUMBER.format(ability.receivedRdps);
  const lines = [
    `rDMG gained: ${formatExactInteger(ability.receivedRdmgExact)}`,
    `rDPS gained: ${totalRate}`,
    `${INTEGER.format(ability.rdpsDamageEventCount)} attributed damage event${ability.rdpsDamageEventCount === 1 ? "" : "s"}`,
  ];
  for (const source of ability.rdpsSources) {
    const provider = historyActorByIdentity(
      view,
      source.providerActorId,
      source.providerEntityUuid,
    );
    const effect = historyRdpsEffectPresentation(view, source.effectId);
    const providerName = provider ? actorLabel(provider) : `Actor ${source.providerActorId}`;
    const effectName = effect?.presentation_name?.trim() || `Effect ${source.effectId}`;
    const component = source.attributionComponent
      ? ` · ${attributionComponentLabel(source.attributionComponent)}`
      : "";
    const sourceDamage = source.attributedRdps === null
      ? "Unresolved"
      : formatExactInteger(source.attributedRdps);
    const sourceDamageNumber = source.attributedRdps === null
      ? null
      : Number(source.attributedRdps);
    const sourceRate = sourceDamageNumber === null || !Number.isFinite(sourceDamageNumber)
      ? "Unresolved"
      : NUMBER.format(perSecond(sourceDamageNumber, view.elapsed_micros));
    lines.push(
      `${providerName} → ${effectName} (${source.effectId})${component}: ${sourceDamage} rDMG · ${sourceRate} rDPS · ${INTEGER.format(source.damageEventCount)} events`,
    );
  }
  if (ability.rdpsUnresolvedRelationshipCount > 0) {
    lines.push(
      `${INTEGER.format(ability.rdpsUnresolvedRelationshipCount)} additional relationship${ability.rdpsUnresolvedRelationshipCount === 1 ? " is" : "s are"} unresolved`,
    );
  }
  cell.title = lines.join("\n");
  cell.setAttribute("aria-label", lines.join(". "));
  return cell;
}

function percentageCell(value: number | null): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number";
  cell.textContent = value === null ? "—" : `${NUMBER.format(value * 100)}%`;
  return cell;
}

function textTableCell(value: string): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.textContent = value;
  return cell;
}

function exactIntegerCell(value: string): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number combat-history-exact-number";
  cell.textContent = formatExactInteger(value);
  cell.title = value;
  return cell;
}

function rdpsSummaryExactCell(
  value: string | null,
  unresolvedRelationshipCount: number,
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number combat-history-exact-number";
  cell.textContent = value === null ? "Unresolved" : formatExactInteger(value);
  cell.title = [
    value === null
      ? "No conserved integer attribution is available"
      : `conserved integer attribution ${value}`,
    unresolvedRelationshipCount > 0
      ? `${unresolvedRelationshipCount} unresolved relationship${unresolvedRelationshipCount === 1 ? "" : "s"} excluded from this total`
      : null,
  ].filter((part): part is string => part !== null).join("; ");
  return cell;
}

function relativeDamageRateCell(
  attributedDamage: string | null,
  elapsedMicros: number,
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-number";
  if (attributedDamage === null) {
    cell.textContent = "Unresolved";
    return cell;
  }
  const damage = Number(attributedDamage);
  if (!Number.isFinite(damage)) {
    cell.textContent = "Unresolved";
    cell.title = "The exact rDMG is retained, but its display rate exceeds the numeric UI range.";
    return cell;
  }
  const rate = perSecond(damage, elapsedMicros);
  cell.textContent = NUMBER.format(rate);
  cell.title = `${attributedDamage} rDMG over ${formatDuration(elapsedMicros)} = ${rate} rDPS`;
  return cell;
}

function attributionComponentLabel(value: string): string {
  return value
    .split("-")
    .filter(Boolean)
    .map((part) => part.length <= 4 && part === part.toUpperCase()
      ? part
      : `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

function exactInfluenceCell(
  attributedRdps: string | null,
  integerDelta: string,
  rationalDeltas: CombatHistoryView["damage_influences"][number]["exact_rational_deltas"],
): HTMLTableCellElement {
  const terms: string[] = [];
  if (integerDelta !== "0") terms.push(formatSignedExactInteger(integerDelta));
  for (const rational of rationalDeltas) {
    const numerator = formatSignedExactInteger(rational.numerator);
    terms.push(rational.denominator === "1"
      ? numerator
      : `${numerator}/${formatExactInteger(rational.denominator)}`);
  }
  const cell = document.createElement("td");
  cell.className = "meter-number combat-history-exact-number";
  cell.textContent = attributedRdps === null
    ? "Unresolved"
    : formatExactInteger(attributedRdps);
  cell.title = [
    attributedRdps === null
      ? "No conserved integer allocation is available for this row"
      : `conserved integer rDMG attribution ${attributedRdps}`,
    integerDelta !== "0" ? `integer ${integerDelta}` : null,
    ...rationalDeltas.map((term) =>
      `rational ${term.numerator}/${term.denominator} across ${term.contribution_count} event${term.contribution_count === 1 ? "" : "s"}`
    ),
    terms.length > 0 ? `exact source terms ${terms.join(" + ").replaceAll("+ -", "- ")}` : null,
  ].filter((term): term is string => term !== null).join("; ");
  return cell;
}

function formatSignedExactInteger(value: string): string {
  if (value.startsWith("-")) return `-${formatExactInteger(value.slice(1))}`;
  return `+${formatExactInteger(value)}`;
}

function formatExactInteger(value: string): string {
  const negative = value.startsWith("-");
  const digits = negative ? value.slice(1) : value;
  const grouped = digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return negative ? `-${grouped}` : grouped;
}

function perSecond(value: number, micros: number): number {
  return micros <= 0 ? 0 : (value * 1_000_000) / micros;
}

function formatDuration(micros: number | null): string {
  if (micros === null) return "Unresolved";
  const totalMillis = Math.floor(micros / 1_000);
  const minutes = Math.floor(totalMillis / 60_000);
  const seconds = Math.floor((totalMillis % 60_000) / 1_000);
  const millis = totalMillis % 1_000;
  return `${minutes}:${seconds.toString().padStart(2, "0")}.${millis.toString().padStart(3, "0")}`;
}

function totalRunTime(run: CombatRunHistory): number | null {
  if (run.entered_micros === null || run.ended_micros === null) return null;
  return Math.max(0, run.ended_micros - run.entered_micros);
}

function formatCalendarDate(unixMillis: number): string {
  return new Date(unixMillis).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

function formatTimestamp(unixMillis: number): string {
  return new Date(unixMillis).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatIdentifier(value: string): string {
  return value
    .replaceAll("-", " ")
    .replaceAll("_", " ")
    .split(" ")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  ...children: Array<Node | string>
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  for (const child of children) node.append(child);
  return node;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
