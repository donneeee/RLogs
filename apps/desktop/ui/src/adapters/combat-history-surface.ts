import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
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
  HistoryDeathEvent,
  HistoryDeathHit,
  HistoryHostileCast,
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
export type GraphMetric = "damage" | "rdps" | "effective_healing" | "damage_taken";
type HistorySort = "newest" | "oldest" | "fastest" | "team_dps" | "team_edps";
type PartySortKey = HistoryPartyColumnId;
type PartySortDirection = "ascending" | "descending";
type AbilitySortKey = "ability" | "damage" | "rdmgReceived" | "rdpsReceived" | "hits" | "casts" | "criticals" | "dps" | "encounterDps" | "healing" | "effectiveHealing" | "shielding" | "hps";
type AbilitySortDirection = "ascending" | "descending";
const HISTORY_PAGE_SIZE = 50;

interface PartySortColumn {
  key: PartySortKey;
  label: string;
  numeric: boolean;
}

interface AbilitySortColumn {
  key: AbilitySortKey;
  label: string;
  numeric: boolean;
}

function partySortColumns(localizer: UiLocalizer): readonly PartySortColumn[] {
  return [
    ["player", "ui.combat_history.column.player", false], ["damage", "ui.combat_history.column.damage", true],
    ["effectiveDamage", "ui.combat_history.column.effective_damage", true], ["damageTaken", "ui.combat_history.column.damage_taken", true],
    ["healing", "ui.combat_history.column.healing", true], ["effectiveHealing", "ui.combat_history.column.effective_healing", true],
    ["shielding", "ui.combat_history.column.shielding", true], ["hits", "ui.combat_history.column.hits", true],
    ["criticalRate", "ui.combat_history.column.critical_percent", true], ["dps", "ui.combat_history.column.edps", true],
    ["encounterDps", "ui.combat_history.column.adps", true], ["hps", "ui.combat_history.column.hps", true],
    ["tps", "ui.combat_history.column.tps", true], ["rdmg", "ui.combat_history.column.rdmg", true],
    ["rdps", "ui.combat_history.column.rdps", true], ["rdpsGiven", "ui.combat_history.column.rdmg_granted", true],
    ["rdpsReceived", "ui.combat_history.column.rdmg_received", true], ["apm", "ui.combat_history.column.apm", true],
    ["deaths", "ui.combat_history.column.deaths", true],
  ].map(([key, labelKey, numeric]) => ({
    key: key as PartySortKey,
    label: localizer.t(labelKey as string),
    numeric: numeric as boolean,
  }));
}

function abilitySortColumns(localizer: UiLocalizer, healing: boolean): readonly AbilitySortColumn[] {
  const columns: ReadonlyArray<readonly [AbilitySortKey, string, boolean]> = healing
    ? [["ability", "ui.combat_history.column.ability", false], ["healing", "ui.combat_history.column.healing", true],
        ["effectiveHealing", "ui.combat_history.column.effective_healing", true], ["shielding", "ui.combat_history.column.shielding", true],
        ["casts", "ui.combat_history.column.casts", true], ["hps", "ui.combat_history.column.hps", true]]
    : [["ability", "ui.combat_history.column.ability", false], ["damage", "ui.combat_history.column.damage", true],
        ["rdmgReceived", "ui.combat_history.column.rdmg_gained", true], ["rdpsReceived", "ui.combat_history.column.rdps_gained", true],
        ["hits", "ui.combat_history.column.hits", true], ["casts", "ui.combat_history.column.casts", true],
        ["criticals", "ui.combat_history.column.crits", true], ["dps", "ui.combat_history.column.edps", true],
        ["encounterDps", "ui.combat_history.column.adps", true], ["healing", "ui.combat_history.column.healing", true],
        ["hps", "ui.combat_history.column.hps", true]];
  return columns.map(([key, labelKey, numeric]) => ({
    key,
    label: localizer.t(labelKey),
    numeric,
  }));
}

interface GraphDefinition {
  metric: GraphMetric;
  title: string;
  rateLabel: string;
  description: string;
}

function graphDefinitions(localizer: UiLocalizer): readonly GraphDefinition[] {
  return [
    {
      metric: "damage",
      title: localizer.t("ui.combat_history.graph.damage_title"),
      rateLabel: localizer.t("ui.combat_history.graph.dps"),
      description: localizer.t("ui.combat_history.graph.damage_description"),
    },
    {
      metric: "rdps",
      title: localizer.t("ui.combat_history.graph.rdps_title"),
      rateLabel: localizer.t("ui.combat_history.graph.rdps"),
      description: localizer.t("ui.combat_history.graph.rdps_description"),
    },
    {
      metric: "effective_healing",
      title: localizer.t("ui.combat_history.graph.healing_title"),
      rateLabel: localizer.t("ui.combat_history.graph.hps"),
      description: localizer.t("ui.combat_history.graph.healing_description"),
    },
    {
      metric: "damage_taken",
      title: localizer.t("ui.combat_history.graph.damage_taken_title"),
      rateLabel: localizer.t("ui.combat_history.graph.tps"),
      description: localizer.t("ui.combat_history.graph.damage_taken_description"),
    },
  ];
}

export interface ActorGraphSeries {
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

const ENCORE_EFFECT_ID = "55333";
const ENCORE_DAMAGE_ACTION_IDS = new Set(["230401", "230501"]);

/** Builds the skill-ownership view without changing the immutable raw totals. */
export function historyOwnedSkillActors(view: CombatHistoryView): HistoryActorSummary[] {
  const actors = view.actors.map((actor) => ({
    ...actor,
    abilities: actor.abilities.map((ability) => ({
      ...ability,
      targets: ability.targets.map((target) => ({ ...target })),
    })),
  }));
  const exactActor = (actorId: string, entityUuid: string) => actors.find(
    (actor) => actor.actor_id === actorId && actor.entity_uuid === entityUuid,
  );
  type Movement = {
    providerActorId: string;
    providerEntityUuid: string;
    damage: bigint;
    events: number;
    criticalHits: number | null;
    targets: Map<string, {
      actorId: string;
      entityUuid: string;
      damage: bigint;
      events: number;
      criticalHits: number | null;
    }>;
  };
  const movements = new Map<string, Map<string, Movement>>();
  for (const influence of view.damage_influences) {
    if (
      influence.effect_id !== ENCORE_EFFECT_ID ||
      !influence.damage_context_complete ||
      influence.provider_actor_id === influence.recipient_actor_id ||
      !influence.affected_ability_id ||
      !ENCORE_DAMAGE_ACTION_IDS.has(influence.affected_ability_id) ||
      !isExactEncoreDamageComponent(influence.attribution_component) ||
      influence.attributed_rdps === null ||
      !/^\d+$/u.test(influence.attributed_rdps) ||
      influence.observed_damage !== influence.attributed_rdps ||
      influence.exact_integer_delta !== influence.attributed_rdps ||
      influence.exact_rational_deltas.length > 0
    ) continue;
    const amount = BigInt(influence.attributed_rdps);
    if (amount <= 0n) continue;
    const recipientKey = [
      influence.recipient_actor_id,
      influence.recipient_entity_uuid,
      influence.affected_ability_id,
    ].join("\0");
    const providerKey = [influence.provider_actor_id, influence.provider_entity_uuid].join("\0");
    const providers = movements.get(recipientKey) ?? new Map<string, Movement>();
    const movement = providers.get(providerKey) ?? {
      providerActorId: influence.provider_actor_id,
      providerEntityUuid: influence.provider_entity_uuid,
      damage: 0n,
      events: 0,
      criticalHits: 0,
      targets: new Map(),
    };
    movement.damage += amount;
    movement.events += influence.damage_event_count;
    movement.criticalHits = movement.criticalHits === null || influence.critical_hit_count == null
      ? null
      : movement.criticalHits + influence.critical_hit_count;
    if (influence.target_actor_id && influence.target_entity_uuid) {
      const targetKey = `${influence.target_actor_id}\0${influence.target_entity_uuid}`;
      const target = movement.targets.get(targetKey) ?? {
        actorId: influence.target_actor_id,
        entityUuid: influence.target_entity_uuid,
        damage: 0n,
        events: 0,
        criticalHits: 0,
      };
      target.damage += amount;
      target.events += influence.damage_event_count;
      target.criticalHits = target.criticalHits === null || influence.critical_hit_count == null
        ? null
        : target.criticalHits + influence.critical_hit_count;
      movement.targets.set(targetKey, target);
    }
    providers.set(providerKey, movement);
    movements.set(recipientKey, providers);
  }

  const providerMovements = new Map<string, Movement[]>();
  for (const [recipientKey, providers] of movements) {
    const [recipientActorId, recipientEntityUuid, actionId] = recipientKey.split("\0");
    if (!recipientActorId || recipientEntityUuid === undefined || !actionId) continue;
    const recipient = exactActor(recipientActorId, recipientEntityUuid);
    const ability = recipient?.abilities.find((candidate) => candidate.ability_id === actionId);
    if (!ability || !Number.isSafeInteger(ability.damage)) continue;
    const moved = [...providers.values()].reduce((sum, movement) => sum + movement.damage, 0n);
    if (moved > BigInt(ability.damage)) continue;
    const movedEvents = [...providers.values()].reduce((sum, movement) => sum + movement.events, 0);
    if (movedEvents > ability.hits) continue;
    const movedDamage = Number(moved);
    if (!Number.isSafeInteger(movedDamage)) continue;
    if (movedDamage === ability.damage) {
      recipient!.abilities = recipient!.abilities.filter((candidate) => candidate !== ability);
    } else {
      const exactCriticals = [...providers.values()].every((movement) => movement.criticalHits !== null);
      const movedCriticals = exactCriticals
        ? [...providers.values()].reduce((sum, movement) => sum + (movement.criticalHits ?? 0), 0)
        : 0;
      ability.damage -= movedDamage;
      ability.effective_damage = Math.max(0, ability.effective_damage - movedDamage);
      ability.hits -= movedEvents;
      if (exactCriticals && movedCriticals <= ability.critical_hits) {
        ability.critical_hits -= movedCriticals;
      } else {
        ability.critical_hits = 0;
        ability.critical_hits_observed = false;
      }
      ability.dps = perSecond(ability.damage, view.elapsed_micros);
      ability.encounter_dps = perSecond(ability.damage, view.active_combat_micros);
      for (const movement of providers.values()) for (const movedTarget of movement.targets.values()) {
        const target = ability.targets.find((candidate) =>
          candidate.actor_id === movedTarget.actorId &&
          candidate.entity_uuid === movedTarget.entityUuid
        );
        if (!target) continue;
        const targetDamage = Number(movedTarget.damage);
        if (!Number.isSafeInteger(targetDamage) || targetDamage > target.damage || movedTarget.events > target.hits) continue;
        target.damage -= targetDamage;
        target.effective_damage = Math.max(0, target.effective_damage - targetDamage);
        target.hits -= movedTarget.events;
        if (movedTarget.criticalHits !== null && movedTarget.criticalHits <= target.critical_hits) {
          target.critical_hits -= movedTarget.criticalHits;
        } else {
          target.critical_hits = 0;
          target.critical_hits_observed = false;
        }
      }
      ability.targets = ability.targets.filter((target) => target.damage > 0 || target.hits > 0);
    }
    for (const movement of providers.values()) {
      const key = `${movement.providerActorId}\0${movement.providerEntityUuid}`;
      const rows = providerMovements.get(key) ?? [];
      rows.push(movement);
      providerMovements.set(key, rows);
    }
  }

  const effect = historyRdpsEffectPresentation(view, ENCORE_EFFECT_ID);
  for (const [providerKey, rows] of providerMovements) {
    const [providerActorId, providerEntityUuid] = providerKey.split("\0");
    if (!providerActorId || providerEntityUuid === undefined) continue;
    const provider = exactActor(providerActorId, providerEntityUuid);
    if (!provider) continue;
    const existingEncore = provider.abilities.filter((ability) =>
      ENCORE_DAMAGE_ACTION_IDS.has(ability.ability_id) &&
      ability.presentation_kind === "support-generated-damage"
    );
    const damage = rows.reduce((sum, row) => sum + row.damage, 0n) +
      BigInt(existingEncore.reduce((sum, ability) => sum + ability.damage, 0));
    const events = rows.reduce((sum, row) => sum + row.events, 0) +
      existingEncore.reduce((sum, ability) => sum + ability.hits, 0);
    const criticalHits = rows.reduce<number | null>(
      (sum, row) => sum === null || row.criticalHits === null ? null : sum + row.criticalHits,
      0,
    );
    const existingCriticalsObserved = existingEncore.every((ability) => ability.critical_hits_observed !== false);
    const combinedCriticalHits = criticalHits === null || !existingCriticalsObserved
      ? null
      : criticalHits + existingEncore.reduce((sum, ability) => sum + ability.critical_hits, 0);
    const numericDamage = Number(damage);
    if (!Number.isSafeInteger(numericDamage)) continue;
    const targets = new Map<string, {
      actorId: string;
      entityUuid: string;
      damage: bigint;
      events: number;
      criticalHits: number | null;
    }>();
    for (const row of rows) for (const [key, incoming] of row.targets) {
      const target = targets.get(key) ?? { ...incoming, damage: 0n, events: 0 };
      target.damage += incoming.damage;
      target.events += incoming.events;
      target.criticalHits = target.criticalHits === null || incoming.criticalHits === null
        ? null
        : target.criticalHits + incoming.criticalHits;
      targets.set(key, target);
    }
    for (const ability of existingEncore) for (const incoming of ability.targets) {
      const key = `${incoming.actor_id}\0${incoming.entity_uuid}`;
      const target = targets.get(key) ?? {
        actorId: incoming.actor_id,
        entityUuid: incoming.entity_uuid,
        damage: 0n,
        events: 0,
        criticalHits: 0,
      };
      target.damage += BigInt(incoming.damage);
      target.events += incoming.hits;
      target.criticalHits = target.criticalHits === null || incoming.critical_hits_observed === false
        ? null
        : target.criticalHits + incoming.critical_hits;
      targets.set(key, target);
    }
    provider.abilities = provider.abilities.filter((ability) => !existingEncore.includes(ability));
    provider.abilities.push({
      ability_id: `support-effect:${ENCORE_EFFECT_ID}`,
      presentation_name: effect?.presentation_name ?? "Encore",
      presentation_kind: "support-generated-damage",
      presentation_resolution: effect?.presentation_resolution ?? "reviewed-source-name",
      icon_asset_path: effect?.icon_asset_path ?? null,
      presentation_recount_group_id: null,
      presentation_recount_group_name: null,
      casts: 0,
      hits: events,
      critical_hits: combinedCriticalHits ?? 0,
      critical_hits_observed: combinedCriticalHits !== null,
      damage: numericDamage,
      effective_damage: numericDamage,
      healing: 0,
      effective_healing: 0,
      shielding: 0,
      dps: perSecond(numericDamage, view.elapsed_micros),
      encounter_dps: perSecond(numericDamage, view.active_combat_micros),
      hps: 0,
      targets: [...targets.values()].flatMap((target) => {
        const targetDamage = Number(target.damage);
        return Number.isSafeInteger(targetDamage) ? [{
          actor_id: target.actorId,
          entity_uuid: target.entityUuid,
          damage: targetDamage,
          effective_damage: targetDamage,
          healing: 0,
          effective_healing: 0,
          shielding: 0,
          hits: target.events,
          critical_hits: target.criticalHits ?? 0,
          critical_hits_observed: target.criticalHits !== null,
        }] : [];
      }),
    });
  }
  return actors;
}

function isExactEncoreDamageComponent(component: string | null | undefined): boolean {
  if (!component) return false;
  return component
    .replace(/\s*\((?:actions?\s*)?\d+(?:[\s/,]+\d+)*\)/giu, "")
    .replace(/\b(?:effect|action)\s+\d+(?:[\s/,]+\d+)*\b/giu, "")
    .replace(/[-_]+/gu, " ")
    .replace(/\s+/gu, " ")
    .trim()
    .toLocaleLowerCase() === "encore standalone generated damage";
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
  localizer: UiLocalizer,
  loadSettings: () => Promise<CombatMeterSettings> = async () =>
    DEFAULT_COMBAT_METER_SETTINGS,
  subscribeCatalogChanges?: CombatHistoryChangeSubscriber,
  actions?: CombatHistoryActions,
): MountedSurface {
  const ui = localizer;
  let alive = true;
  let catalog: CombatHistoryCatalog | null = null;
  let selectedEntry: CombatHistoryCatalogEntry | null = null;
  let detail: CombatHistorySnapshot | null = null;
  let viewId = "all";
  let targetActorId: string | null = null;
  let detailActorId: string | null = null;
  let hiddenGraphActors = new Set<string>();
  let showHostileGraphEvents = true;
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
  const status = element("span", "combat-history-live-status", ui.t("ui.combat_history.status.loading_history"));
  status.setAttribute("aria-live", "polite");
  const content = element("div", "combat-history-content");
  content.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.status.loading_runs")));
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
      status.textContent = ui.t("ui.combat_history.status.reading_index");
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
          ? ui.t("ui.combat_history.status.no_indexed_runs")
          : ui.t("ui.combat_history.status.indexed_runs", { count: ui.formatNumber(catalog.entries.length) });
      } catch (error) {
        if (!alive) return;
        status.textContent = errorMessage(error);
        if (catalog === null) {
          content.replaceChildren(
            element("p", "runtime-empty-result", ui.t("ui.combat_history.error.load_failed")),
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
    showHostileGraphEvents = true;
    detail = await loadDetail(entry.session_id);
    if (alive) render();
  };

  const render = () => {
    if (!catalog || catalog.entries.length === 0) {
      content.replaceChildren(
        element(
          "p",
          "runtime-empty-result",
          ui.t("ui.combat_history.empty.complete_dungeon"),
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
          element("h2", "", ui.t("ui.combat_history.browser.title")),
          element("p", "card-copy", ui.t("ui.combat_history.browser.description")),
        ),
        element("span", "state-pill", ui.t("ui.combat_history.browser.result_count", {
          visible: ui.formatNumber(entries.length),
          total: ui.formatNumber(catalog.entries.length),
        })),
      ),
    );

    const toolbar = element("div", "combat-history-browser-toolbar");
    const search = document.createElement("input");
    search.type = "search";
    search.value = browserQuery;
    search.placeholder = ui.t("ui.combat_history.browser.search_placeholder");
    search.setAttribute("aria-label", ui.t("ui.combat_history.browser.search_aria"));
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
      ui.t("ui.combat_history.browser.difficulty"),
      [["all", ui.t("ui.combat_history.browser.all_difficulties")], ...uniqueDifficultyFilters(catalog.entries)],
      browserDifficulty,
      (value) => {
        browserDifficulty = value;
        browserPage = 0;
        render();
      },
    );
    const sort = selectControl(
      ui.t("ui.combat_history.browser.sort_runs"),
      [
        ["newest", ui.t("ui.combat_history.browser.sort_newest")],
        ["oldest", ui.t("ui.combat_history.browser.sort_oldest")],
        ["fastest", ui.t("ui.combat_history.browser.sort_fastest")],
        ["team_dps", ui.t("ui.combat_history.browser.sort_team_edps")],
        ["team_edps", ui.t("ui.combat_history.browser.sort_team_adps")],
      ],
      browserSort,
      (value) => {
        browserSort = value as HistorySort;
        browserPage = 0;
        render();
      },
    );
    const favoriteFilter = button(
      `${browserFavoritesOnly ? "★" : "☆"} ${ui.t("ui.combat_history.browser.favorites")}`,
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
      const clearSelection = button(ui.t("ui.combat_history.mutation.clear_selection"), "quiet-button");
      clearSelection.disabled = historyMutationInFlight;
      clearSelection.addEventListener("click", () => {
        selectedHistoryIds = new Set();
        deleteConfirmationOpen = false;
        render();
      });
      const deleteSelected = button(
        deletableCount > 0
          ? ui.t("ui.combat_history.mutation.delete_selected_count", {
              count: ui.formatNumber(deletableCount),
            })
          : ui.t("ui.combat_history.mutation.delete_selected"),
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
            element("strong", "", ui.t("ui.combat_history.mutation.selected_count", {
              count: ui.formatNumber(selectedEntries.length),
            })),
            protectedCount > 0
              ? element(
                  "small",
                  "",
                  ui.t(protectedCount === 1
                    ? "ui.combat_history.mutation.favorite_protected"
                    : "ui.combat_history.mutation.favorites_protected", {
                    count: ui.formatNumber(protectedCount),
                  }),
                )
              : element("small", "", ui.t("ui.combat_history.mutation.selection_help")),
          ),
          element("div", "combat-history-selection-actions", clearSelection, deleteSelected),
        ),
      );
    }

    if (pageEntries.length === 0) {
      runBrowser.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.browser.no_matches")));
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
    selectPage.setAttribute("aria-label", ui.t("ui.combat_history.mutation.select_page_aria"));
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
        element("span", "combat-history-run-column-favorite", ui.t("ui.combat_history.column.favorite_short")),
        element("span", "combat-history-run-column-dungeon", ui.t("ui.combat_history.column.dungeon")),
        element("span", "combat-history-run-column-party", ui.t("ui.combat_history.column.party")),
        element("span", "combat-history-run-column-metric", ui.t("ui.combat_history.column.team_edps")),
        element("span", "combat-history-run-column-metric", ui.t("ui.combat_history.column.team_adps")),
        element("span", "combat-history-run-column-metric", ui.t("ui.combat_history.column.run_time")),
        element("span", "combat-history-run-column-recorded", ui.t("ui.combat_history.column.recorded")),
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
      selection.setAttribute("aria-label", ui.t("ui.combat_history.mutation.select_run_aria", {
        run: activityLabel(entry),
      }));
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
        ui.t(entry.is_favorite
          ? "ui.combat_history.mutation.remove_favorite_aria"
          : "ui.combat_history.mutation.add_favorite_aria", { run: activityLabel(entry) }),
      );
      favorite.disabled = historyMutationInFlight || !actions;
      favorite.addEventListener("click", (event) => {
        event.stopPropagation();
        if (!actions || historyMutationInFlight) return;
        historyMutationInFlight = true;
        status.classList.remove("error");
        status.textContent = entry.is_favorite
          ? ui.t("ui.combat_history.mutation.removing_favorite")
          : ui.t("ui.combat_history.mutation.saving_favorite");
        render();
        void actions
          .setFavorite(entry.history_id, !entry.is_favorite)
          .then((updatedCatalog) => {
            if (!alive) return;
            catalog = updatedCatalog;
            status.textContent = ui.t("ui.combat_history.status.indexed_runs", {
              count: ui.formatNumber(updatedCatalog.entries.length),
            });
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
        ? ui.t("ui.combat_history.browser.legacy_run_time_help")
        : ui.t("ui.combat_history.browser.total_run_time_help");
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
        status.textContent = ui.t("ui.combat_history.status.loading_detail");
        void selectEntry(entry)
          .then(() => {
            status.textContent = ui.t("ui.combat_history.status.indexed_runs", {
              count: ui.formatNumber(catalog?.entries.length ?? 0),
            });
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
      const previous = button(ui.t("ui.combat_history.browser.previous"), "quiet-button");
      previous.disabled = browserPage === 0;
      previous.addEventListener("click", () => {
        browserPage = Math.max(0, browserPage - 1);
        render();
      });
      const next = button(ui.t("ui.combat_history.browser.next"), "quiet-button");
      next.disabled = browserPage + 1 >= pageCount;
      next.addEventListener("click", () => {
        browserPage = Math.min(pageCount - 1, browserPage + 1);
        render();
      });
      pagination.append(
        previous,
        element("span", "", ui.t("ui.combat_history.browser.page", {
          current: ui.formatNumber(browserPage + 1),
          total: ui.formatNumber(pageCount),
        })),
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

    const cancel = button(ui.t("ui.combat_history.mutation.cancel"), "quiet-button");
    cancel.disabled = historyMutationInFlight;
    cancel.addEventListener("click", () => {
      deleteConfirmationOpen = false;
      render();
    });
    const confirm = button(
      ui.t(deletableCount === 1
        ? "ui.combat_history.mutation.delete_run"
        : "ui.combat_history.mutation.delete_runs", {
        count: ui.formatNumber(deletableCount),
      }),
      "quiet-button combat-history-delete-selected",
    );
    confirm.disabled = historyMutationInFlight || deletableCount === 0 || !actions;
    confirm.addEventListener("click", () => {
      if (!actions || historyMutationInFlight || deletableCount === 0) return;
      const requestedIds = [...selectedHistoryIds];
      historyMutationInFlight = true;
      status.classList.remove("error");
      status.textContent = ui.t(deletableCount === 1
        ? "ui.combat_history.mutation.deleting_run"
        : "ui.combat_history.mutation.deleting_runs", {
        count: ui.formatNumber(deletableCount),
      });
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
            ui.t(result.deleted_count === 1
              ? "ui.combat_history.mutation.run_deleted"
              : "ui.combat_history.mutation.runs_deleted", {
              count: ui.formatNumber(result.deleted_count),
            }),
          ];
          if (result.preserved_favorite_count > 0) {
            notes.push(
              ui.t(result.preserved_favorite_count === 1
                ? "ui.combat_history.mutation.favorite_preserved"
                : "ui.combat_history.mutation.favorites_preserved", {
                count: ui.formatNumber(result.preserved_favorite_count),
              }),
            );
          }
          if (result.cleanup_warnings.length > 0) {
            notes.push(ui.t(result.cleanup_warnings.length === 1
              ? "ui.combat_history.mutation.cleanup_warning"
              : "ui.combat_history.mutation.cleanup_warnings", {
              count: ui.formatNumber(result.cleanup_warnings.length),
            }));
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
      element("h2", "", ui.t("ui.combat_history.mutation.delete_dialog_title")),
      element(
        "p",
        "",
        ui.t(deletableCount === 1
          ? "ui.combat_history.mutation.delete_dialog_body_one"
          : "ui.combat_history.mutation.delete_dialog_body_many", {
          count: ui.formatNumber(deletableCount),
        }),
      ),
    );
    if (protectedCount > 0) {
      dialog.append(
        element(
          "p",
          "combat-history-delete-protected",
          ui.t(protectedCount === 1
            ? "ui.combat_history.mutation.delete_dialog_protected_one"
            : "ui.combat_history.mutation.delete_dialog_protected_many", {
            count: ui.formatNumber(protectedCount),
          }),
        ),
      );
    }
    dialog.append(
      element("div", "combat-history-delete-actions", cancel, confirm),
    );
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");
    dialog.setAttribute("aria-label", ui.t("ui.combat_history.mutation.delete_dialog_aria"));
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
    navigation.setAttribute("aria-label", ui.t("ui.combat_history.navigation.aria"));
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
      retrySelect.setAttribute("aria-label", ui.t("ui.combat_history.filter.retry_aria"));
      retrySelect.append(new Option(ui.t("ui.combat_history.filter.retries"), ""));
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
    targetSelect.append(new Option(ui.t("ui.combat_history.filter.all_targets"), ""));
    for (const target of view.targets) {
      targetSelect.append(new Option(historyTargetLabel(target), target.actor_id));
    }
    targetSelect.value = targetActorId ?? "";
    targetSelect.addEventListener("change", () => {
      targetActorId = targetSelect.value || null;
      render();
    });
    filters.append(
      element("div", "combat-history-segment-controls", element("span", "field-label", ui.t("ui.combat_history.filter.segment")), viewButtons),
      element("label", "combat-history-target-filter", element("span", "field-label", ui.t("ui.combat_history.filter.target_entity")), targetSelect),
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
      pane.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.empty.select_run")));
      return pane;
    }
    const run = detail.runs.find((run) => run.run_index === selectedEntry?.run_index);
    if (!run) {
      pane.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.empty.missing_detail")));
      return pane;
    }
    const view = run.views.find((view) => view.id === viewId) ?? run.views[0];
    if (!view) {
      pane.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.empty.no_views")));
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
        element("span", "state-pill", ui.t("ui.combat_history.summary.saved_locally")),
      ),
      metricGrid([
        [
          formatDuration(run.total_run_time_micros ?? totalRunTime(run)),
          ui.t("ui.combat_history.summary.total_run"),
        ],
        [formatDuration(run.game_time_micros), ui.t("ui.combat_history.summary.game_time")],
        [formatDuration(entireRun.active_combat_micros), ui.t("ui.combat_history.summary.active_combat")],
        [formatDuration(run.true_time_micros ?? trueTime?.elapsed_micros ?? null), ui.t("ui.combat_history.summary.true_time")],
        [
          ui.t("ui.combat_history.summary.retry_count", {
            total: ui.formatNumber(run.retry_count),
            boss: ui.formatNumber(run.boss_retry_count),
          }),
          ui.t("ui.combat_history.summary.retries"),
        ],
      ]),
    );

    const navigation = renderPageNavigation(
        ui.t("ui.combat_history.navigation.past_runs"),
        activityContextLabel(run),
        () => {
          selectedEntry = null;
          detail = null;
          detailActorId = null;
          render();
        },
        playerLayerTimeMetadata(run, ui),
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
      pane.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.empty.player_unavailable")));
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
        ui.t("ui.combat_history.navigation.run_summary"),
        ui.t("ui.combat_history.navigation.actor_skills", {
          run: activityContextLabel(run),
          actor: actorLabel(actor),
        }),
        () => {
          detailActorId = null;
          render();
        },
        playerLayerTimeMetadata(run, ui),
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
    viewButtons.setAttribute("aria-label", ui.t("ui.combat_history.breakdown.party_view_aria"));
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
        element("div", "", element("h2", "", ui.t("ui.combat_history.breakdown.party")), viewButtons),
        element(
          "span", "",
          targetActorId
            ? ui.t("ui.combat_history.filter.damage_one_entity")
            : participantCountLabel(participants, ui),
        ),
      ),
    );
    const availablePartyColumns = partySortColumns(ui);
    const visibleColumns = partyView.columns.map(
      (key) => availablePartyColumns.find((column) => column.key === key)!,
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
        ? ui.t(partySortDirection === "descending"
          ? "ui.combat_history.sort.lowest_first"
          : "ui.combat_history.sort.highest_first", { column: column.label })
        : ui.t("ui.combat_history.sort.by_column", { column: column.label });
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
      row.setAttribute("aria-label", ui.t("ui.combat_history.breakdown.open_skills_aria", {
        actor: actorLabel(actor),
      }));
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
        element("h2", "", ui.t("ui.combat_history.breakdown.incoming_damage")),
        element("span", "", ui.t("ui.combat_history.breakdown.incoming_damage_description")),
      ),
    );

    const sources = incomingDamageSourceGroups(view, victim, targetActorId);

    if (sources.length === 0) {
      card.append(
        element(
          "p",
          "runtime-empty-result",
          targetActorId === null
            ? ui.t("ui.combat_history.empty.no_incoming_damage")
            : ui.t("ui.combat_history.empty.no_target_incoming_damage"),
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
      [ui.t("ui.combat_history.column.source"), false],
      [ui.t("ui.combat_history.column.ability"), false],
      [ui.t("ui.combat_history.column.damage_taken"), true],
      [ui.t("ui.combat_history.column.hits"), true],
      [ui.t("ui.combat_history.column.tps"), true],
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
        ui.t(collapsed
          ? "ui.combat_history.breakdown.expand_incoming_aria"
          : "ui.combat_history.breakdown.collapse_incoming_aria", {
            source: sourceEntry.source ? actorLabel(sourceEntry.source) : sourceEntry.sourceActorId,
          }),
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
              : ui.t("ui.combat_history.breakdown.source_actor", { actor: sourceEntry.sourceActorId }),
          ),
          element("small", "", ui.t("ui.combat_history.identity.entity", {
            entity: sourceEntry.sourceEntityUuid,
          })),
        ),
      );
      parent.append(
        treeCell,
        sourceCell,
        element("td", "", ui.t("ui.combat_history.count.mapped_abilities", {
          count: ui.formatNumber(sourceEntry.abilities.length),
        })),
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
            ui,
          );
        } else {
          abilityCell = document.createElement("td");
          abilityCell.className = "meter-actor combat-history-combat-presentation-cell";
          abilityCell.append(
            element(
              "span",
              "combat-history-combat-copy",
              element("strong", "", ui.t("ui.combat_history.breakdown.unattributed_damage")),
              element("small", "", ui.t("ui.combat_history.breakdown.unattributed_damage_description")),
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
      dialog.setAttribute("aria-label", ui.t("ui.combat_history.breakdown.skill_details_aria", {
        actor: actorLabel(actor),
      }));
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
          ui.t(actor.character_id
            ? "ui.combat_history.identity.player_context_uid"
            : "ui.combat_history.identity.player_context", {
              uid: actor.character_id ?? "",
              entity: actor.entity_uuid,
              view: view.label,
            }),
        ),
      ),
    );
    if (presentation === "popover") {
      const close = button(ui.t("ui.combat_history.breakdown.close"), "quiet-button combat-history-dialog-close");
      close.setAttribute("aria-label", ui.t("ui.combat_history.breakdown.close_details_aria"));
      close.addEventListener("click", () => {
        detailActorId = null;
        render();
      });
      dialogHeader.append(close);
    }
    const actorMetrics = displayedMetrics(actor, view, targetActorId);
    const overview = metricGrid([
      [INTEGER.format(actorMetrics.damage), ui.t("ui.combat_history.column.damage")],
      [NUMBER.format(actorMetrics.dps), ui.t("ui.combat_history.column.edps")],
      [NUMBER.format(actorMetrics.encounterDps), ui.t("ui.combat_history.column.adps")],
      [rdpsDisplay(actor.rdps_damage, true, actor.rdps_incomplete), ui.t("ui.combat_history.column.rdmg")],
      [rdpsDisplay(actor.rdps, false, actor.rdps_incomplete), ui.t("ui.combat_history.column.rdps")],
      [
        rdpsDisplay(actor.rdps_contribution_given, true, actor.rdps_incomplete),
        ui.t("ui.combat_history.column.rdmg_granted"),
      ],
      [
        rdpsDisplay(actor.rdps_contribution_received, true, actor.rdps_incomplete),
        ui.t("ui.combat_history.column.rdmg_received"),
      ],
      [NUMBER.format(actorMetrics.hps), ui.t("ui.combat_history.column.hps")],
      [NUMBER.format(actorMetrics.tps), ui.t("ui.combat_history.column.tps")],
      [INTEGER.format(actor.deaths), ui.t("ui.combat_history.column.deaths")],
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
    const abilityColumns = abilitySortColumns(ui, detailMode === "healing");
    if (!abilityColumns.some((column) => column.key === abilitySortKey)) {
      abilitySortKey = detailMode === "healing" ? "hps" : "damage";
      abilitySortDirection = "descending";
    }
    const skillActor = detailMode === "damage"
      ? historyOwnedSkillActors(view).find((candidate) =>
        candidate.actor_id === actor.actor_id && candidate.entity_uuid === actor.entity_uuid
      ) ?? actor
      : actor;
    const card = element("section", "content-card combat-history-skill-card");
    card.append(
      element("div", "card-heading",
        element("h2", "", detailMode === "healing"
          ? ui.t("ui.combat_history.breakdown.healing_and_shielding")
          : ui.t("ui.combat_history.breakdown.skills")),
        element("span", "", targetActorId
          ? ui.t("ui.combat_history.filter.target_filtered")
          : ui.t("ui.combat_history.count.owned_abilities", {
              count: ui.formatNumber(skillActor.abilities.length),
            })),
      ),
    );
    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-skill-table";
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    const treeHeading = document.createElement("th");
    treeHeading.className = "combat-history-tree-column";
    treeHeading.setAttribute("aria-label", ui.t("ui.combat_history.breakdown.recount_tree_aria"));
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
        ? ui.t(abilitySortDirection === "descending"
          ? "ui.combat_history.sort.lowest_first"
          : "ui.combat_history.sort.highest_first", { column: column.label })
        : ui.t("ui.combat_history.sort.by_column", { column: column.label });
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
      ...skillActor.abilities.map((ability) => displayedAbility(
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
          ui,
        );
      const treeCell = document.createElement("td");
      treeCell.className = "combat-history-tree-cell";
      if (entry.kind === "recount-parent" && groupKey) {
        const toggle = button(collapsed ? "\u25b6" : "\u25bc", "combat-history-tree-toggle");
        toggle.type = "button";
        toggle.setAttribute("aria-expanded", String(!collapsed));
        toggle.setAttribute(
          "aria-label",
          ui.t(collapsed
            ? "ui.combat_history.breakdown.expand_recount_aria"
            : "ui.combat_history.breakdown.collapse_recount_aria", { ability: ability.abilityId }),
        );
        toggle.title = ui.t(entry.childCount === 1
          ? "ui.combat_history.count.child_action_one"
          : "ui.combat_history.count.child_actions", { count: ui.formatNumber(entry.childCount) });
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
          case "criticals": row.append(
            ability.criticalsObserved !== false
              ? numeric(ability.criticals, true)
              : textTableCell("—"),
          ); break;
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
        ui.t("ui.combat_history.breakdown.apm_pending"),
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
        element("h2", "", ui.t("ui.combat_history.graph.gallery_title")),
        element("span", "", ui.t("ui.combat_history.graph.gallery_description")),
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
        ui.t(hidden
          ? "ui.combat_history.graph.show_actor_aria"
          : "ui.combat_history.graph.hide_actor_aria", { actor: actorLabel(actor) }),
      );
      control.append(
        element("span", "combat-history-series-swatch"),
        element("strong", "", actorLabel(actor)),
      );
      if (graphActorKind(actor) === "npc") {
        control.append(element("span", "combat-history-legend-npc", ui.t("ui.combat_history.graph.npc")));
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
    if ((view.hostile_casts?.length ?? 0) > 0) {
      const control = button("", "combat-history-series-toggle");
      control.dataset.hidden = String(!showHostileGraphEvents);
      control.dataset.actorKind = "hostile";
      control.style.setProperty("--series-color", "var(--amber)");
      control.setAttribute("aria-pressed", String(showHostileGraphEvents));
      control.setAttribute(
        "aria-label",
        ui.t(showHostileGraphEvents
          ? "ui.combat_history.graph.hide_hostile_aria"
          : "ui.combat_history.graph.show_hostile_aria"),
      );
      control.append(
        element("span", "combat-history-series-swatch"),
        element("strong", "", ui.t("ui.combat_history.graph.hostile_mechanics")),
      );
      control.addEventListener("click", () => {
        showHostileGraphEvents = !showHostileGraphEvents;
        render();
      });
      legend.append(control);
    }
    gallery.append(legend);
    const definitions = graphDefinitions(ui);
    const definition = definitions.find(
      (candidate) => candidate.metric === graphMetric,
    ) ?? definitions[0]!;
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
        ui,
        view,
        showHostileGraphEvents,
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
      element("div", "card-heading", element("h2", "", ui.t("ui.combat_history.breakdown.status_effects")), element("span", "", ui.t("ui.combat_history.breakdown.effect_count", {
        count: ui.formatNumber(effects.length),
      }))),
    );
    if (effects.length === 0) {
      card.append(element("p", "runtime-empty-result", ui.t("ui.combat_history.empty.no_status_events")));
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
            ui,
          ),
          element(
            "span",
            "",
            ui.t("ui.combat_history.breakdown.effect_lifecycle", {
              applied: ui.formatNumber(effect.applied),
              refreshed: ui.formatNumber(effect.refreshed),
              stacked: ui.formatNumber(effect.stacked),
              consumed: ui.formatNumber(effect.consumed),
              removed: ui.formatNumber(effect.removed),
            }),
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
        element("h2", "", ui.t("ui.combat_history.breakdown.relative_damage_sources")),
        element(
          "span",
          "",
          targetActorId === null
            ? ui.t(relationshipCount === 1
              ? "ui.combat_history.count.relationship_one"
              : "ui.combat_history.count.relationships", {
                count: ui.formatNumber(relationshipCount),
              })
            : ui.t("ui.combat_history.filter.target_filtered"),
        ),
      ),
    );

    if (breakdown.receivedSkills.length === 0 && breakdown.grantedEffects.length === 0) {
      card.append(
        element(
          "p",
          "runtime-empty-result",
          ui.t("ui.combat_history.rdps.no_relationship"),
        ),
      );
      return card;
    }

    if (breakdown.receivedSkills.length > 0) {
      card.append(
        element(
          "p",
          "card-copy",
          ui.t("ui.combat_history.rdps.received_explanation"),
        ),
      );
    }

    if (breakdown.grantedEffects.length > 0) {
      const section = element("section", "combat-history-rdps-summary-section");
      section.append(
        element("h3", "", ui.t("ui.combat_history.breakdown.granted_by_support_effect")),
        element(
          "p",
          "card-copy",
          ui.t("ui.combat_history.rdps.granted_explanation"),
        ),
      );
      const scroller = element("div", "meter-table-scroll");
      const table = document.createElement("table");
      table.className = "meter-table combat-history-rdps-summary-table";
      const head = document.createElement("thead");
      const heading = document.createElement("tr");
      const labels = [
        ui.t("ui.combat_history.column.support_effect"),
        ui.t("ui.combat_history.column.component"),
        ui.t("ui.combat_history.column.rdmg_granted"),
        ui.t("ui.combat_history.column.rdps_granted"),
        ui.t("ui.combat_history.column.events"),
      ];
      for (const label of labels) {
        const cell = document.createElement("th");
        if (labels.indexOf(label) >= 2) cell.className = "meter-number";
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
            ui,
          ),
        );
        row.append(
          effectCell,
          textTableCell(
            granted.attributionComponent
              ? attributionComponentLabel(granted.attributionComponent)
              : ui.t("ui.combat_history.breakdown.complete_effect"),
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
        element("h2", "", ui.t("ui.combat_history.breakdown.influence_ledger")),
        element(
          "span",
          "",
          influenceQuery.trim()
            ? ui.t("ui.combat_history.count.filtered_relationships", {
                visible: ui.formatNumber(influences.length),
                total: ui.formatNumber(actorInfluences.length),
              })
            : ui.t(influences.length === 1
              ? "ui.combat_history.count.exact_relationship_one"
              : "ui.combat_history.count.exact_relationships", {
                count: ui.formatNumber(influences.length),
              }),
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
    search.placeholder = ui.t("ui.combat_history.filter.influence_placeholder");
    search.setAttribute("aria-label", ui.t("ui.combat_history.filter.influence_aria"));
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
          ui.t("ui.combat_history.empty.no_influences"),
        ),
      );
      return card;
    }

    const scroller = element("div", "meter-table-scroll");
    const table = document.createElement("table");
    table.className = "meter-table combat-history-influence-table";
    const head = document.createElement("thead");
    const heading = document.createElement("tr");
    const influenceLabels = [
      ui.t("ui.combat_history.column.effect"), ui.t("ui.combat_history.column.component"),
      ui.t("ui.combat_history.column.provider"), ui.t("ui.combat_history.column.recipient"),
      ui.t("ui.combat_history.column.affected_damage_id"), ui.t("ui.combat_history.column.target"),
      ui.t("ui.combat_history.column.events"), ui.t("ui.combat_history.column.observed_damage"),
      ui.t("ui.combat_history.column.attributed_rdmg"),
    ];
    for (const [index, label] of influenceLabels.entries()) {
      const cell = document.createElement("th");
      if (index >= 6) {
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
          ui,
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
            ui,
          ),
        );
      } else {
        abilityCell.textContent = ui.t("ui.combat_history.breakdown.context_unresolved");
        abilityCell.dataset.contextComplete = "false";
      }
      row.dataset.contextComplete = String(influence.damage_context_complete);
      row.append(
        effectCell,
        textTableCell(
          influence.attribution_component
            ? attributionComponentLabel(influence.attribution_component)
            : ui.t("ui.combat_history.breakdown.complete_effect"),
        ),
        textTableCell(provider ? actorLabel(provider) : ui.t("ui.combat_history.identity.actor", {
          actor: influence.provider_actor_id,
        })),
        textTableCell(recipient ? actorLabel(recipient) : ui.t("ui.combat_history.identity.actor", {
          actor: influence.recipient_actor_id,
        })),
        abilityCell,
        textTableCell(
          target
            ? target.presentation_name?.trim() || target.display_name?.trim() || ui.t("ui.combat_history.identity.entity", {
                entity: target.entity_uuid,
              })
            : influence.target_entity_uuid
              ? ui.t("ui.combat_history.identity.entity", { entity: influence.target_entity_uuid })
              : ui.t("ui.combat_history.breakdown.context_unresolved"),
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
        if (alive) status.textContent = ui.t("ui.combat_history.status.refresh_reconnecting", {
          error: errorMessage(error),
        });
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

function participantCountLabel(participants: HistoryActorSummary[], localizer: UiLocalizer): string {
  const npcCount = participants.filter((actor) => graphActorKind(actor) === "npc").length;
  const playerCount = participants.length - npcCount;
  if (npcCount === 0) {
    return localizer.t("ui.combat_history.count.combatants", {
      count: localizer.formatNumber(playerCount),
    });
  }
  return localizer.t("ui.combat_history.count.players_and_npcs", {
    players: localizer.formatNumber(playerCount),
    npcs: localizer.formatNumber(npcCount),
  });
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
      criticalsObserved: ability.critical_hits_observed !== false,
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
    criticalsObserved: target?.critical_hits_observed !== false,
    dps: perSecond(damage, view.elapsed_micros),
    encounterDps: perSecond(damage, view.active_combat_micros),
    healing,
    effectiveHealing,
    shielding,
    hps: perSecond(effectiveHealing, view.elapsed_micros),
    ...relativeDamage,
  };
}

export type DisplayedAbility = Omit<ReturnType<typeof displayedAbility>, "criticalsObserved"> & {
  criticalsObserved?: boolean;
};

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
    criticalsObserved: false,
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
    criticalsObserved: children.every((ability) => ability.criticalsObserved !== false),
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
    case "criticals": return ability.criticalsObserved !== false ? ability.criticals : null;
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
  localizer: UiLocalizer,
): ReadonlyArray<readonly [string, string]> {
  const entireRun = run.views.find((candidate) => candidate.id === "all") ?? run.views[0];
  const trueTime = run.views.find((candidate) => candidate.id === "true_time");
  return [
    [localizer.t("ui.combat_history.summary.run_short"), formatDuration(run.total_run_time_micros ?? totalRunTime(run))],
    [localizer.t("ui.combat_history.summary.game_short"), formatDuration(run.game_time_micros)],
    [localizer.t("ui.combat_history.summary.active_short"), formatDuration(entireRun?.active_combat_micros ?? null)],
    [localizer.t("ui.combat_history.summary.true_short"), formatDuration(run.true_time_micros ?? trueTime?.elapsed_micros ?? null)],
    [localizer.t("ui.combat_history.summary.retries"), localizer.t("ui.combat_history.summary.retry_short", {
      total: localizer.formatNumber(run.retry_count),
      boss: localizer.formatNumber(run.boss_retry_count),
    })],
  ];
}

function combatPresentationCell(
  id: string,
  name: string | null,
  kind: string | null,
  resolution: string | null,
  iconPath: string | null,
  namespace: "ability" | "effect",
  localizer: UiLocalizer,
): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "meter-actor combat-history-combat-presentation-cell";
  cell.append(combatPresentationIdentity(id, name, kind, resolution, iconPath, namespace, localizer));
  return cell;
}

export function combatPresentationDisplayName(
  name: string | null,
  namespace: "ability" | "effect",
  localizer: UiLocalizer,
): string {
  return name?.trim() || localizer.t(namespace === "ability"
    ? "ui.combat_history.breakdown.unresolved_action"
    : "ui.combat_history.breakdown.unresolved_effect");
}

function combatPresentationIdentity(
  id: string,
  name: string | null,
  kind: string | null,
  resolution: string | null,
  iconPath: string | null,
  namespace: "ability" | "effect",
  localizer: UiLocalizer,
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
  const copy = element(
    "span",
    "combat-history-combat-copy",
    element("strong", "", combatPresentationDisplayName(name, namespace, localizer)),
    element(
      "span",
      "combat-history-combat-metadata",
      localizer.t(namespace === "ability"
        ? "ui.combat_history.breakdown.action_identity"
        : "ui.combat_history.breakdown.effect_identity", {
          id,
          kind: kindLabel ? ` · ${kindLabel}` : "",
        }),
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

export function renderMetricGraph(
  actors: HistoryActorSummary[],
  definition: GraphDefinition,
  elapsedMicros: number,
  hiddenActorIds: ReadonlySet<string>,
  actorColors: ReadonlyMap<string, string>,
  targetActorId: string | null,
  selectMetric: (metric: GraphMetric) => void,
  localizer: UiLocalizer,
  historyView?: CombatHistoryView,
  showHostileEvents = true,
): HTMLElement {
  const card = element("section", "combat-history-metric-graph");
  const durationSeconds = definition.metric === "rdps"
    ? Math.max(1, historyView?.rate_clock?.length ?? 0)
    : Math.max(1, Math.ceil(elapsedMicros / 1_000_000));
  const allSeries = actors
    .map((actor) =>
      buildActorGraphSeries(
        actor,
        definition.metric,
        durationSeconds,
        actorColors.get(actor.actor_id) ?? graphColor(actors.indexOf(actor)),
        targetActorId,
        historyView,
      ),
    )
    .filter((entry) =>
      entry.peak > 0 || (definition.metric !== "rdps" && targetActorId === null &&
        ((entry.actor.death_events?.length ?? 0) > 0 || entry.actor.death_seconds.length > 0 ||
          (entry.actor.skill_events?.length ?? 0) > 0 ||
          (entry.actor.status_events?.length ?? 0) > 0)),
    );
  const visibleSeries = allSeries.filter(
    (entry) => !hiddenActorIds.has(entry.actor.actor_id),
  );
  const hasHostileCasts = showHostileEvents && (historyView?.hostile_casts?.length ?? 0) > 0;
  const scaleMaximum = graphScaleMaximum(
    allSeries.map((entry) => entry.values),
  );
  card.append(
    element(
      "div",
      "combat-history-graph-heading",
      element("div", "", element("h3", "", definition.title), element("p", "", definition.description)),
      renderGraphMetricToggle(definition.metric, selectMetric, localizer),
    ),
  );
  if (allSeries.length === 0 && !hasHostileCasts) {
    card.append(
      element(
        "p",
        "runtime-empty-result",
        localizer.t("ui.combat_history.graph.no_values", { rate: definition.rateLabel }),
      ),
    );
    return card;
  }
  if (visibleSeries.length === 0) {
    card.append(
      element(
        "p",
        "combat-history-graph-note",
        localizer.t("ui.combat_history.graph.all_hidden"),
      ),
    );
  }
  card.append(
    partyLineChart(
      visibleSeries,
      definition,
      durationSeconds,
      elapsedMicros,
      scaleMaximum,
      targetActorId === null,
      localizer,
    ),
  );
  const eventLanes = recordedEventLanes(
    visibleSeries,
    durationSeconds,
    elapsedMicros,
    localizer,
    historyView,
    showHostileEvents,
  );
  if (eventLanes) card.append(eventLanes);
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
        element("span", "combat-history-graph-stat-label", localizer.t("ui.combat_history.graph.average")),
        element("span", "combat-history-graph-stat-value", COMPACT.format(entry.average)),
      ),
      element(
        "span",
        "combat-history-graph-stat-metric",
        element("span", "combat-history-graph-stat-label", localizer.t("ui.combat_history.graph.peak")),
        element("span", "combat-history-graph-stat-value", COMPACT.format(entry.peak)),
      ),
    );
    stats.append(item);
  }
  card.append(stats);
  return card;
}

function recordedEventLanes(
  series: readonly ActorGraphSeries[],
  durationSeconds: number,
  durationMicros: number,
  localizer: UiLocalizer,
  historyView?: CombatHistoryView,
  showHostileEvents = true,
): HTMLElement | null {
  const playerLanes = series.filter(({ actor }) => graphActorKind(actor) === "player" && (
    (actor.skill_events?.length ?? 0) > 0 || (actor.death_events?.length ?? 0) > 0 ||
    (actor.status_events?.length ?? 0) > 0 || actor.death_seconds.length > 0));
  const hostileLanes = showHostileEvents
    ? groupHostileCastsBySource(historyView?.hostile_casts ?? [])
    : [];
  if (hostileLanes.length === 0 && playerLanes.length === 0) return null;
  const width = 1_120, left = 78, right = 24, laneHeight = 38;
  const plotWidth = width - left - right;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.classList.add("combat-history-event-lanes");
  svg.setAttribute("viewBox", `0 0 ${width} ${(hostileLanes.length + playerLanes.length) * laneHeight}`);
  svg.setAttribute("role", "group");
  svg.setAttribute("aria-label", localizer.t("ui.combat_history.graph.recorded_events_aria"));
  const deathMarkers: SVGGElement[] = [];
  const skillClusterDisclosures: Array<{
    marker: SVGGElement;
    actor: string;
    eventLabels: string[];
  }> = [];
  const xFor = (micros: number) => left +
    (Math.min(durationSeconds, Math.max(0, micros / 1_000_000)) / durationSeconds) * plotWidth;
  hostileLanes.forEach(({ sourceActorId, casts }, laneIndex) => {
    const y = laneIndex * laneHeight + laneHeight / 2;
    const sourceActor = historyView?.actors.find((actor) => actor.actor_id === sourceActorId);
    const source = hostileSourceLabel(sourceActorId, sourceActor, localizer);
    const row = svgNode("g", "combat-history-event-lane combat-history-hostile-event-lane", {
      "data-lane-kind": "hostile",
      "data-source-actor-id": sourceActorId,
    });
    row.style.setProperty("--series-color", "var(--amber)");
    row.append(
      svgNode("line", "combat-history-event-lane-line", { x1: left, x2: width - right, y1: y, y2: y }),
      svgNode("circle", "combat-history-event-lane-swatch", { cx: 10, cy: y, r: 3 }),
      svgText(18, y + 4, compactEventLaneLabel(source), "combat-history-event-lane-label", "start"),
    );
    for (const cluster of clusterHistoryHostileCasts(casts, xFor)) {
      const first = cluster.events[0]!;
      const presentations = cluster.events.map((cast) =>
        hostileActionPresentation(sourceActor, cast.action_id, localizer));
      const actionNames = [...new Set(presentations.map((presentation) => presentation.name))];
      const target = first.target_actor_id
        ? localizer.t("ui.combat_history.graph.hostile_cast_target", {
          target: hostileTargetLabel(first.target_actor_id, historyView, localizer),
        })
        : "";
      const summary = cluster.events.length === 1
        ? localizer.t("ui.combat_history.graph.hostile_cast", {
          source, action: actionNames[0]!, time: formatExactGraphTime(first.at_micros), target,
        })
        : localizer.t("ui.combat_history.graph.hostile_cast_cluster", {
          source,
          count: localizer.formatNumber(cluster.events.length),
          start: formatExactGraphTime(first.at_micros),
          end: formatExactGraphTime(cluster.events.at(-1)!.at_micros),
          actions: actionNames.join(", "),
        });
      const actionIds = new Set(cluster.events.map((cast) => cast.action_id));
      const iconAssetPath = actionIds.size === 1 && presentations[0]!.trusted
        ? presentations[0]!.iconAssetPath
        : null;
      const marker = svgNode("g", "combat-history-skill-event combat-history-hostile-cast-event", {
        transform: `translate(${cluster.x.toFixed(2)} ${y.toFixed(2)})`,
        role: "img",
        tabindex: 0,
        "aria-label": summary,
        "data-event-count": cluster.events.length,
        "data-action-id": first.action_id,
      });
      marker.style.setProperty("--series-color", "var(--amber)");
      marker.append(svgNode("circle", "combat-history-skill-event-hitbox", { cx: 0, cy: 0, r: 12 }));
      if (iconAssetPath) {
        marker.append(
          svgNode("circle", "combat-history-skill-event-icon-ring", { cx: 0, cy: 0, r: 9 }),
          svgNode("image", "combat-history-skill-event-icon", {
            href: iconAssetPath, x: -8, y: -8, width: 16, height: 16,
            preserveAspectRatio: "xMidYMid slice",
          }),
        );
      } else {
        marker.append(svgNode("path", "combat-history-hostile-cast-event-glyph", {
          d: "M-6-5H6V1L0 7L-6 1Z",
        }));
      }
      marker.append(svgTitle(summary));
      if (cluster.events.length > 1) {
        marker.append(
          svgNode("circle", "combat-history-skill-event-badge", { cx: 8, cy: -8, r: 7 }),
          svgText(8, -5, String(cluster.events.length), "combat-history-skill-event-badge-text", "middle"),
        );
      }
      row.append(marker);
    }
    svg.append(row);
  });
  playerLanes.forEach(({ actor, color }, playerLaneIndex) => {
    const laneIndex = hostileLanes.length + playerLaneIndex;
    const y = laneIndex * laneHeight + laneHeight / 2;
    const row = svgNode("g", "combat-history-event-lane combat-history-player-event-lane", {
      "data-lane-kind": "player",
    });
    row.style.setProperty("--series-color", color);
    row.append(
      svgNode("line", "combat-history-event-lane-line", { x1: left, x2: width - right, y1: y, y2: y }),
      svgNode("circle", "combat-history-event-lane-swatch", { cx: 10, cy: y, r: 3 }),
      svgText(18, y + 4, compactEventLaneLabel(actorLabel(actor)), "combat-history-event-lane-label", "start"),
    );
    for (const span of completeHistoryStatusSpans(actor.status_events ?? [])) {
      const presentation = historyView?.status_effect_presentations?.find(
        (candidate) => candidate.effect_id === span.applied.effect_id &&
          Boolean(candidate.presentation_name.trim()) &&
          Boolean(candidate.presentation_resolution.trim()),
      );
      const effect = presentation?.presentation_name.trim() || localizer.t(
        "ui.combat_history.graph.status_effect_fallback",
        { id: span.applied.effect_id },
      );
      const summary = localizer.t(
        span.terminal.state === "consumed"
          ? "ui.combat_history.graph.status_span_consumed"
          : "ui.combat_history.graph.status_span_removed",
        {
          actor: actorLabel(actor), effect,
          start: formatExactGraphTime(span.applied.at_micros),
          end: formatExactGraphTime(span.terminal.at_micros),
        },
      );
      const startX = xFor(span.applied.at_micros);
      const endX = xFor(span.terminal.at_micros);
      const marker = svgNode("g", "combat-history-status-span", {
        role: "img", tabindex: 0, "aria-label": summary,
        "data-effect-id": span.applied.effect_id,
        "data-terminal-state": span.terminal.state,
      });
      marker.append(svgNode("rect", "combat-history-status-span-bar", {
        x: startX, y: y - 5, width: Math.max(1, endX - startX), height: 10, rx: 5,
      }));
      marker.append(svgTitle(summary));
      row.append(marker);
    }
    for (const cluster of clusterHistorySkillEvents(actor.skill_events ?? [], xFor)) {
      const first = cluster.events[0]!;
      const abilityNames = [...new Set(cluster.events.map((event) => {
        const ability = actor.abilities.find((candidate) => candidate.ability_id === event.ability_id);
        return ability?.presentation_name?.trim() ||
          localizer.t("ui.combat_history.graph.ability_fallback", { id: event.ability_id });
      }))];
      const summary = cluster.events.length === 1
        ? localizer.t("ui.combat_history.graph.skill_event", {
          actor: actorLabel(actor), ability: abilityNames[0]!, time: formatExactGraphTime(first.at_micros),
        })
        : localizer.t("ui.combat_history.graph.skill_event_cluster", {
          actor: actorLabel(actor), count: localizer.formatNumber(cluster.events.length),
          start: formatExactGraphTime(first.at_micros),
          end: formatExactGraphTime(cluster.events.at(-1)!.at_micros),
          abilities: abilityNames.join(", "),
        });
      const clusteredAbilityIds = new Set(cluster.events.map((event) => event.ability_id));
      const iconAssetPath = clusteredAbilityIds.size === 1
        ? actor.abilities.find((ability) => ability.ability_id === first.ability_id)
          ?.icon_asset_path?.trim() || null
        : null;
      const marker = svgNode("g", "combat-history-skill-event", {
        transform: `translate(${cluster.x.toFixed(2)} ${y.toFixed(2)})`,
        role: "img",
        tabindex: 0,
        "aria-label": summary,
        "data-event-count": cluster.events.length,
      });
      marker.style.setProperty("--series-color", color);
      marker.append(svgNode("circle", "combat-history-skill-event-hitbox", { cx: 0, cy: 0, r: 12 }));
      if (iconAssetPath) {
        marker.append(
          svgNode("circle", "combat-history-skill-event-icon-ring", { cx: 0, cy: 0, r: 9 }),
          svgNode("image", "combat-history-skill-event-icon", {
            href: iconAssetPath, x: -8, y: -8, width: 16, height: 16,
            preserveAspectRatio: "xMidYMid slice",
          }),
        );
      } else {
        marker.append(svgNode("path", "combat-history-skill-event-glyph", { d: "M1-7L-4 1H0L-1 7L5-2H1Z" }));
      }
      marker.append(svgTitle(summary));
      if (cluster.events.length > 1) {
        const eventLabels = cluster.events.map((event) => {
          const ability = actor.abilities.find((candidate) => candidate.ability_id === event.ability_id);
          const abilityName = ability?.presentation_name?.trim() ||
            localizer.t("ui.combat_history.graph.ability_fallback", { id: event.ability_id });
          return localizer.t("ui.combat_history.graph.skill_event", {
            actor: actorLabel(actor), ability: abilityName, time: formatExactGraphTime(event.at_micros),
          });
        });
        marker.setAttribute("role", "button");
        marker.setAttribute("aria-expanded", "false");
        marker.setAttribute("aria-label", localizer.t("ui.combat_history.graph.skill_cluster_disclosure", {
          summary,
        }));
        marker.append(
          svgNode("circle", "combat-history-skill-event-badge", { cx: 8, cy: -8, r: 7 }),
          svgText(8, -5, String(cluster.events.length), "combat-history-skill-event-badge-text", "middle"),
        );
        skillClusterDisclosures.push({ marker, actor: actorLabel(actor), eventLabels });
      }
      row.append(marker);
    }
    const deaths = (actor.death_events?.length ?? 0) > 0
      ? actor.death_events.map((death) => ({ death, precision: "exact_microsecond" as const }))
      : actor.death_seconds.map((second) => ({
        death: { at_micros: second * 1_000_000, cause: null } satisfies HistoryDeathEvent,
        precision: "one_second_bucket" as const,
      }));
    for (const { death, precision } of deaths) {
      const marker = historyDeathMarker(
        xFor(death.at_micros), y,
        historyDeathSummary(actorLabel(actor), death, localizer, precision, durationMicros),
        color, false,
      );
      deathMarkers.push(marker);
      row.append(marker);
    }
    svg.append(row);
  });
  const frame = element(
    "section",
    "combat-history-event-lanes-frame",
    element("strong", "combat-history-event-lanes-title", localizer.t("ui.combat_history.graph.recorded_events")),
    svg,
  );
  if (skillClusterDisclosures.length > 0) {
    const disclosure = element("section", "combat-history-skill-disclosure");
    disclosure.id = `combat-history-skill-disclosure-${historySkillDisclosureSequence++}`;
    disclosure.hidden = true;
    let activeMarker: SVGGElement | null = null;
    const close = () => {
      disclosure.hidden = true;
      disclosure.replaceChildren();
      if (activeMarker) {
        activeMarker.setAttribute("aria-expanded", "false");
        activeMarker.classList.remove("is-disclosed", "is-event-selected");
        delete activeMarker.dataset.selectedSkillEvent;
      }
      activeMarker = null;
    };
    for (const cluster of skillClusterDisclosures) {
      cluster.marker.setAttribute("aria-controls", disclosure.id);
      const open = () => {
        close();
        cluster.marker.setAttribute("aria-expanded", "true");
        cluster.marker.classList.add("is-disclosed");
        activeMarker = cluster.marker;
        const heading = element("strong", "", localizer.t(
          "ui.combat_history.graph.skill_disclosure_title", { actor: cluster.actor },
        ));
        const list = document.createElement("ol");
        const selected = element("output", "combat-history-skill-disclosure-selected");
        selected.setAttribute("aria-live", "polite");
        cluster.eventLabels.forEach((label, eventIndex) => {
          const item = document.createElement("li");
          const eventButton = button(label, "");
          eventButton.dataset.skillDisclosureEvent = String(eventIndex);
          eventButton.setAttribute("aria-pressed", "false");
          eventButton.addEventListener("click", () => {
            list.querySelectorAll<HTMLButtonElement>("button").forEach((candidate) =>
              candidate.setAttribute("aria-pressed", String(candidate === eventButton)));
            cluster.marker.dataset.selectedSkillEvent = String(eventIndex);
            cluster.marker.classList.add("is-event-selected");
            selected.textContent = label;
          });
          item.append(eventButton);
          list.append(item);
        });
        disclosure.replaceChildren(heading, list, selected);
        disclosure.hidden = false;
      };
      cluster.marker.addEventListener("click", () => {
        if (cluster.marker.getAttribute("aria-expanded") === "true") close();
        else open();
      });
      cluster.marker.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && cluster.marker.getAttribute("aria-expanded") === "true") {
          event.preventDefault();
          close();
        } else if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          if (cluster.marker.getAttribute("aria-expanded") === "true") close();
          else open();
        }
      });
    }
    disclosure.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" || disclosure.hidden) return;
      event.preventDefault();
      const marker = activeMarker;
      close();
      marker?.focus();
    });
    frame.append(disclosure);
  }
  const summary = element("div", "combat-history-death-summary");
  summary.id = `combat-history-event-death-summary-${historyDeathSummarySequence++}`;
  summary.setAttribute("role", "tooltip");
  summary.hidden = true;
  wireHistoryDeathSummaries(deathMarkers, summary);
  frame.append(summary);
  return frame;
}

export function completeHistoryStatusSpans(
  events: readonly HistoryActorSummary["status_events"][number][],
): Array<{
  applied: HistoryActorSummary["status_events"][number];
  terminal: HistoryActorSummary["status_events"][number];
}> {
  type Event = HistoryActorSummary["status_events"][number];
  const grouped = new Map<string, Event[]>();
  const spans: Array<{ applied: Event; terminal: Event }> = [];
  for (const event of events) {
    if (!event.instance_id) continue;
    const key = `${event.effect_id}\u0000${event.instance_id}`;
    const group = grouped.get(key) ?? [];
    group.push(event);
    grouped.set(key, group);
  }
  for (const group of grouped.values()) {
    const applied = group.filter((event) => event.state === "applied");
    const terminal = group.filter((event) =>
      event.state === "removed" || event.state === "consumed");
    if (applied.length !== 1 || terminal.length !== 1) continue;
    const startIndex = group.indexOf(applied[0]!);
    const endIndex = group.indexOf(terminal[0]!);
    if (startIndex !== 0 || endIndex !== group.length - 1 || endIndex <= startIndex) continue;
    if (group.slice(1, -1).some((event) =>
      event.state !== "refreshed" && event.state !== "stacked")) continue;
    if (terminal[0]!.at_micros < applied[0]!.at_micros) continue;
    spans.push({ applied: applied[0]!, terminal: terminal[0]! });
  }
  return spans.sort((left, right) => left.applied.at_micros - right.applied.at_micros);
}

function groupHostileCastsBySource(
  casts: readonly HistoryHostileCast[],
): Array<{ sourceActorId: string; casts: HistoryHostileCast[] }> {
  const grouped = new Map<string, HistoryHostileCast[]>();
  for (const cast of casts) {
    const events = grouped.get(cast.source_actor_id) ?? [];
    events.push(cast);
    grouped.set(cast.source_actor_id, events);
  }
  return [...grouped].map(([sourceActorId, events]) => ({ sourceActorId, casts: events }));
}

function clusterHistoryHostileCasts(
  events: readonly HistoryHostileCast[],
  xFor: (micros: number) => number,
): Array<{ x: number; events: HistoryHostileCast[] }> {
  const clusters = new Map<number, HistoryHostileCast[]>();
  for (const event of events) {
    const pixelBucket = Math.round(xFor(event.at_micros) / 6);
    const cluster = clusters.get(pixelBucket) ?? [];
    cluster.push(event);
    clusters.set(pixelBucket, cluster);
  }
  return [...clusters.entries()].map(([bucket, clustered]) => ({ x: bucket * 6, events: clustered }));
}

function hostileSourceLabel(
  sourceActorId: string,
  actor: HistoryActorSummary | undefined,
  localizer: UiLocalizer,
): string {
  const trustedName = actor?.monster_id && actor.presentation_name?.trim() &&
    actor.presentation_name.trim() !== actor.display_name?.trim()
    ? actor.presentation_name.trim()
    : null;
  return trustedName ?? localizer.t("ui.combat_history.graph.hostile_source_fallback", {
    id: sourceActorId,
  });
}

function hostileTargetLabel(
  targetActorId: string,
  view: CombatHistoryView | undefined,
  localizer: UiLocalizer,
): string {
  const actor = view?.actors.find((candidate) => candidate.actor_id === targetActorId);
  const trustedName = actor?.actor_kind === "player"
    ? actor.presentation_name?.trim() || actor.display_name?.trim()
    : null;
  return trustedName ?? localizer.t("ui.combat_history.graph.actor_id_fallback", {
    id: targetActorId,
  });
}

function hostileActionPresentation(
  sourceActor: HistoryActorSummary | undefined,
  actionId: string,
  localizer: UiLocalizer,
): { name: string; iconAssetPath: string | null; trusted: boolean } {
  const ability = sourceActor?.abilities.find((candidate) => candidate.ability_id === actionId);
  const trusted = Boolean(ability?.presentation_name?.trim() && ability.presentation_resolution?.trim());
  return {
    name: trusted
      ? ability!.presentation_name!.trim()
      : localizer.t("ui.combat_history.graph.hostile_action_fallback", { id: actionId }),
    iconAssetPath: trusted ? ability?.icon_asset_path?.trim() || null : null,
    trusted,
  };
}

function clusterHistorySkillEvents(
  events: readonly HistoryActorSummary["skill_events"][number][],
  xFor: (micros: number) => number,
): Array<{ x: number; events: HistoryActorSummary["skill_events"] }> {
  const clusters = new Map<number, HistoryActorSummary["skill_events"]>();
  for (const event of events) {
    const pixelBucket = Math.round(xFor(event.at_micros) / 6);
    const cluster = clusters.get(pixelBucket) ?? [];
    cluster.push(event);
    clusters.set(pixelBucket, cluster);
  }
  return [...clusters.entries()].map(([bucket, clustered]) => ({ x: bucket * 6, events: clustered }));
}

function compactEventLaneLabel(label: string): string {
  return label.length > 9 ? `${label.slice(0, 8)}…` : label;
}

function renderGraphMetricToggle(
  selectedMetric: GraphMetric,
  selectMetric: (metric: GraphMetric) => void,
  localizer: UiLocalizer,
): HTMLElement {
  const toggle = element("div", "combat-history-graph-metric-toggle");
  toggle.setAttribute("role", "group");
  toggle.setAttribute("aria-label", localizer.t("ui.combat_history.graph.metric_aria"));
  for (const definition of graphDefinitions(localizer)) {
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
  historyView?: CombatHistoryView,
): ActorGraphSeries {
  if (metric === "rdps") {
    return buildActorRdpsGraphSeries(actor, historyView, color, targetActorId) ?? {
      actor,
      color,
      values: Array.from({ length: durationSeconds + 1 }, () => 0),
      average: 0,
      peak: 0,
    };
  }
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

/**
 * Projects reducer-authored adjusted-damage buckets through the reducer's
 * cumulative eDPS clock. A missing/partial clock, inconsistent terminal
 * scalar, or damage in a zero-time window makes the curve unavailable; this
 * function never substitutes wall time or ordinary damage.
 */
export function buildActorRdpsGraphSeries(
  actor: HistoryActorSummary,
  view: CombatHistoryView | undefined,
  color: string,
  targetActorId: string | null = null,
  windowSeconds = 5,
): ActorGraphSeries | null {
  const clock = view?.rate_clock;
  if (targetActorId !== null || view?.rate_clock_complete !== true || !clock?.length ||
      actor.rdps_incomplete || actor.rdps_damage === null || actor.rdps === null ||
      actor.rdps_contribution_given === null || actor.rdps_contribution_received === null ||
      !Number.isFinite(actor.rdps) || windowSeconds < 1) return null;
  const amounts = Array.from({ length: clock.length }, () => 0);
  let priorEdps = 0;
  let priorAdps = 0;
  for (const [index, point] of clock.entries()) {
    if (point.second !== index || point.edps_elapsed_micros < priorEdps ||
        point.adps_elapsed_micros < priorAdps ||
        point.edps_elapsed_micros - priorEdps > 1_000_000 ||
        point.adps_elapsed_micros - priorAdps > 1_000_000 ||
        point.adps_elapsed_micros > point.edps_elapsed_micros) return null;
    priorEdps = point.edps_elapsed_micros;
    priorAdps = point.adps_elapsed_micros;
  }
  let damage = 0;
  let given = 0;
  let received = 0;
  let previousSecond = -1;
  for (const point of actor.series) {
    if (point.second <= previousSecond || point.second >= clock.length ||
        point.rdps_damage === null || point.rdps_contribution_given === null ||
        point.rdps_contribution_received === null || point.rdps_damage < 0 ||
        point.rdps_contribution_given < 0 || point.rdps_contribution_received < 0 ||
        point.damage + point.rdps_contribution_given - point.rdps_contribution_received !==
          point.rdps_damage) return null;
    previousSecond = point.second;
    amounts[point.second] = point.rdps_damage;
    damage += point.rdps_damage;
    given += point.rdps_contribution_given;
    received += point.rdps_contribution_received;
  }
  const finalClock = clock.at(-1)!;
  const expectedRate = finalClock.edps_elapsed_micros === 0
    ? 0
    : actor.rdps_damage * 1_000_000 / finalClock.edps_elapsed_micros;
  if (finalClock.edps_elapsed_micros !== view.elapsed_micros ||
      finalClock.adps_elapsed_micros !== view.active_combat_micros ||
      damage !== actor.rdps_damage || given !== actor.rdps_contribution_given ||
      received !== actor.rdps_contribution_received ||
      Math.abs(actor.rdps - expectedRate) > Math.max(1, Math.abs(expectedRate)) * 1e-12) return null;

  const prefix = [0];
  for (const amount of amounts) prefix.push(prefix.at(-1)! + amount);
  const values: number[] = [0];
  for (let index = 0; index < clock.length; index += 1) {
    const start = Math.max(0, index - windowSeconds + 1);
    const elapsedStart = start === 0 ? 0 : clock[start - 1]!.edps_elapsed_micros;
    const elapsed = clock[index]!.edps_elapsed_micros - elapsedStart;
    const amount = prefix[index + 1]! - prefix[start]!;
    if (elapsed === 0 && amount !== 0) return null;
    values.push(elapsed === 0 ? 0 : amount * 1_000_000 / elapsed);
  }
  return {
    actor,
    color,
    values,
    average: actor.rdps,
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

export function graphInspectionAtSecond(
  series: readonly ActorGraphSeries[],
  requestedSecond: number,
  durationSeconds: number,
): { second: number; values: Array<{ actorId: string; label: string; color: string; value: number }> } {
  const second = Math.min(
    Math.max(1, Math.floor(durationSeconds)),
    Math.max(0, Math.round(requestedSecond)),
  );
  return {
    second,
    values: series.map((entry) => ({
      actorId: entry.actor.actor_id,
      label: actorLabel(entry.actor),
      color: entry.color,
      value: entry.values[second] ?? 0,
    })),
  };
}

function partyLineChart(
  series: ActorGraphSeries[],
  definition: GraphDefinition,
  durationSeconds: number,
  durationMicros: number,
  scaleMaximum: number,
  showDeathMarkers: boolean,
  localizer: UiLocalizer,
): HTMLElement {
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
    localizer.t("ui.combat_history.graph.aria", {
      title: definition.title,
      rate: definition.rateLabel,
      duration: formatGraphTime(durationSeconds),
    }),
  );
  svg.tabIndex = 0;

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
    svgText(width / 2, height - 5, localizer.t("ui.combat_history.graph.run_time"), "combat-history-axis-title", "middle"),
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
        localizer.t("ui.combat_history.graph.series_summary", {
          actor: actorLabel(entry.actor),
          average: localizer.formatNumber(entry.average, { maximumFractionDigits: 1 }),
          peak: localizer.formatNumber(entry.peak, { maximumFractionDigits: 1 }),
          rate: definition.rateLabel,
        }),
      ),
    );
    svg.append(polyline);
    if (!showDeathMarkers) continue;
    const deaths = (entry.actor.death_events?.length ?? 0) > 0
      ? entry.actor.death_events.map((death) => ({ death, precision: "exact_microsecond" as const }))
      : entry.actor.death_seconds.map((second) => ({
        death: { at_micros: second * 1_000_000, cause: null } satisfies HistoryDeathEvent,
        precision: "one_second_bucket" as const,
      }));
    for (const { death, precision } of deaths) {
      const second = Math.min(durationSeconds, death.at_micros / 1_000_000);
      const value = entry.values[Math.round(second)] ?? 0;
      svg.append(historyDeathMarker(
        xFor(second),
        yFor(value),
        historyDeathSummary(actorLabel(entry.actor), death, localizer, precision, durationMicros),
        entry.color,
      ));
    }
  }

  const inspection = svgNode("g", "combat-history-graph-inspection", {});
  inspection.setAttribute("hidden", "");
  const inspectionLine = svgNode("line", "combat-history-graph-inspection-line", {
    x1: left,
    x2: left,
    y1: top,
    y2: top + plotHeight,
  });
  const inspectionPoints = series.map((entry) => {
    const point = svgNode("circle", "combat-history-graph-inspection-point", {
      cx: left,
      cy: yFor(0),
      r: 4,
      fill: entry.color,
    });
    inspection.append(point);
    return point;
  });
  inspection.prepend(inspectionLine);
  svg.append(inspection);

  const frame = element("div", "combat-history-chart-frame");
  const readout = element(
    "div",
    "combat-history-graph-inspection-readout",
    localizer.t("ui.combat_history.graph.inspect_help"),
  );
  readout.setAttribute("aria-live", "polite");
  let inspectedSecond: number | null = null;
  const renderInspection = (requestedSecond: number) => {
    const snapshot = graphInspectionAtSecond(series, requestedSecond, durationSeconds);
    inspectedSecond = snapshot.second;
    const x = xFor(snapshot.second);
    inspection.removeAttribute("hidden");
    inspectionLine.setAttribute("x1", x.toFixed(2));
    inspectionLine.setAttribute("x2", x.toFixed(2));
    snapshot.values.forEach((value, index) => {
      const point = inspectionPoints[index];
      if (!point) return;
      point.setAttribute("cx", x.toFixed(2));
      point.setAttribute("cy", yFor(value.value).toFixed(2));
    });
    readout.replaceChildren(
      element("strong", "", formatGraphTime(snapshot.second)),
      ...snapshot.values.map((value) => {
        const item = element(
          "span",
          "combat-history-graph-inspection-value",
          localizer.t("ui.combat_history.graph.inspect_value", {
            actor: value.label,
            value: localizer.formatNumber(value.value, { maximumFractionDigits: 1 }),
            rate: definition.rateLabel,
          }),
        );
        item.style.setProperty("--series-color", value.color);
        return item;
      }),
    );
  };
  const clearInspection = () => {
    inspectedSecond = null;
    inspection.setAttribute("hidden", "");
    readout.textContent = localizer.t("ui.combat_history.graph.inspect_help");
  };
  svg.addEventListener("pointermove", (event) => {
    const bounds = svg.getBoundingClientRect();
    if (bounds.width <= 0) return;
    const viewX = ((event.clientX - bounds.left) / bounds.width) * width;
    renderInspection(((viewX - left) / plotWidth) * durationSeconds);
  });
  svg.addEventListener("pointerleave", clearInspection);
  svg.addEventListener("focus", () => renderInspection(inspectedSecond ?? 0));
  svg.addEventListener("blur", clearInspection);
  svg.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === "Home"
      ? 0
      : event.key === "End"
        ? durationSeconds
        : (inspectedSecond ?? 0) + (event.key === "ArrowLeft" ? -1 : 1);
    renderInspection(next);
  });
  const deathSummary = element("div", "combat-history-death-summary");
  deathSummary.id = `combat-history-death-summary-${historyDeathSummarySequence++}`;
  deathSummary.setAttribute("role", "tooltip");
  deathSummary.hidden = true;
  wireHistoryDeathSummaries(
    [...svg.querySelectorAll<SVGGElement>(".combat-history-death-marker")],
    deathSummary,
  );
  frame.append(svg, readout, deathSummary);
  return frame;
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

let historyDeathSummarySequence = 0;
let historySkillDisclosureSequence = 0;

export function historyDeathMarker(
  x: number,
  lineY: number,
  summary: string,
  participantColor: string | null = null,
  clampToGraph = true,
): SVGGElement {
  const y = clampToGraph ? Math.min(268, Math.max(16, lineY)) : lineY;
  const group = svgNode("g", "combat-history-death-marker", {
    transform: `translate(${x.toFixed(2)} ${y.toFixed(2)})`,
    role: "img",
    tabindex: 0,
    "aria-label": summary,
  });
  group.dataset.deathSummary = summary;
  if (participantColor !== null) group.style.setProperty("--death-marker-color", participantColor);
  group.append(
    svgNode("circle", "combat-history-death-marker-hitbox", { cx: 0, cy: 0, r: 12 }),
    svgNode("path", "combat-history-death-marker-bones", {
      d: "M-7-6L7 7M7-6L-7 7",
    }),
    svgNode("path", "combat-history-death-marker-skull", {
      d: "M-5-3A5 5 0 1 1 5-3C5 0 3 2 2 2V6H-2V2C-3 2-5 0-5-3Z",
    }),
    svgTitle(summary),
  );
  return group;
}

export function wireHistoryDeathSummaries(
  markers: readonly SVGGElement[],
  summary: HTMLElement,
): void {
  let hovered: SVGGElement | null = null;
  let focused: SVGGElement | null = null;
  const show = (marker: SVGGElement) => {
    markers.forEach((candidate) => candidate.removeAttribute("aria-describedby"));
    summary.textContent = marker.dataset.deathSummary ?? marker.getAttribute("aria-label") ?? "";
    summary.hidden = false;
    marker.setAttribute("aria-describedby", summary.id);
  };
  const hide = () => {
    summary.hidden = true;
    summary.textContent = "";
    markers.forEach((marker) => marker.removeAttribute("aria-describedby"));
  };
  for (const marker of markers) {
    marker.addEventListener("pointerenter", () => {
      hovered = marker;
      show(marker);
    });
    marker.addEventListener("pointerleave", () => {
      if (hovered === marker) hovered = null;
      if (focused) show(focused);
      else hide();
    });
    marker.addEventListener("focus", () => {
      focused = marker;
      show(marker);
    });
    marker.addEventListener("blur", () => {
      if (focused === marker) focused = null;
      if (hovered) show(hovered);
      else hide();
    });
  }
}

export function historyDeathSummary(
  actorName: string,
  death: HistoryDeathEvent,
  localizer: UiLocalizer,
  precision: "exact_microsecond" | "one_second_bucket" = "exact_microsecond",
  durationMicros = death.at_micros + 1_000_000,
): string {
  const occurrence = precision === "one_second_bucket"
    ? localizer.t("ui.combat_history.death.bucket", {
      actor: actorName,
      start: formatExactGraphTime(death.at_micros),
      end: formatExactGraphTime(
        Math.min(Math.max(death.at_micros, durationMicros), death.at_micros + 1_000_000),
      ),
    })
    : localizer.t("ui.combat_history.death.exact", {
      actor: actorName,
      time: formatExactGraphTime(death.at_micros),
    });
  if (!death.cause) {
    return localizer.t("ui.combat_history.death.cause_unavailable", { occurrence });
  }
  const final = historyDeathHitSummary(death.cause.final_hit, localizer);
  const recent = death.cause.prior_hits.length === 0
    ? localizer.t("ui.combat_history.death.no_earlier_hits")
    : localizer.t(
      death.cause.prior_hits.length === 1
        ? "ui.combat_history.death.one_earlier_hit"
        : "ui.combat_history.death.earlier_hits",
      { count: localizer.formatNumber(death.cause.prior_hits.length) },
    );
  const truncated = death.cause.prior_hits_truncated
    ? localizer.t("ui.combat_history.death.earlier_hits_truncated")
    : "";
  return localizer.t("ui.combat_history.death.with_cause", { occurrence, final, recent, truncated });
}

function historyDeathHitSummary(hit: HistoryDeathHit, localizer: UiLocalizer): string {
  const source = hit.source_presentation?.name?.trim() ||
    localizer.t("ui.combat_history.death.actor_fallback", { id: hit.source_actor_id });
  const abilityId = hit.ability_presentation?.ability_id ??
    hit.breakdown_ability_id ?? hit.ability_id;
  const ability = hit.ability_presentation?.name?.trim() ||
    (abilityId
      ? localizer.t("ui.combat_history.death.ability_fallback", { id: abilityId })
      : localizer.t("ui.combat_history.death.unknown_ability"));
  const direct = hit.direct_source_actor_id
    ? localizer.t("ui.combat_history.death.direct_source", {
      source: hit.direct_source_presentation?.name?.trim() ||
        localizer.t("ui.combat_history.death.actor_fallback", { id: hit.direct_source_actor_id }),
      actor_id: hit.direct_source_actor_id,
    })
    : "";
  return localizer.t("ui.combat_history.death.hit", {
    ability,
    ability_id: abilityId ?? localizer.t("ui.combat_history.death.unknown_id"),
    source,
    source_actor_id: hit.source_actor_id,
    direct,
    reported: localizer.formatNumber(hit.reported_damage, { maximumFractionDigits: 0 }),
    effective: localizer.formatNumber(hit.effective_damage, { maximumFractionDigits: 0 }),
    critical: hit.critical ? localizer.t("ui.combat_history.death.critical") : "",
  });
}

function formatExactGraphTime(micros: number): string {
  const totalMillis = Math.max(0, Math.round(micros / 1_000));
  const minutes = Math.floor(totalMillis / 60_000);
  const seconds = Math.floor(totalMillis / 1_000) % 60;
  const millis = totalMillis % 1_000;
  return `${minutes}:${seconds.toString().padStart(2, "0")}.${millis.toString().padStart(3, "0")}`;
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
  const weaponName = actor.weapon_presentation_name ?? `Unlocalized weapon item #${itemId}`;
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
