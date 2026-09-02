import { describeRdpsStatus } from "./rdps-status";
import type {
  HistoryDamageInfluenceSummary,
  HistoryRdpsEffectPresentation,
} from "./combat-history";

export const COMBAT_SNAPSHOT_SCHEMA_VERSION = 5;

export interface CombatAbilitySummary {
  ability_id: string;
  casts: number;
  hits: number;
  critical_hits: number;
  reported_damage: number;
  effective_damage: number;
  reported_healing: number;
  effective_healing: number;
  shielding: number;
}

export interface CombatActorSummary {
  actor_id: string;
  entity_uuid: string;
  display_name: string | null;
  actor_kind: string | null;
  class_id: number | null;
  specialization_id: number | null;
  level: number | null;
  seasonal_score: number | null;
  reported_damage: number;
  effective_damage: number;
  hp_damage: number;
  shield_damage: number;
  damage_during_combat: number;
  damage_taken: number;
  dps: number;
  run_dps: number;
  encounter_dps: number;
  active_dps: number;
  hps: number;
  tps: number;
  rdps_damage: number | null;
  rdps: number | null;
  rdps_contribution_given: number | null;
  rdps_contribution_received: number | null;
  rdps_incomplete: boolean;
  reported_healing: number;
  effective_healing: number;
  overheal: number;
  shielding: number;
  casts: number;
  hits: number;
  critical_hits: number;
  deaths: number;
  revives: number;
  position_samples: number;
  path_distance: number;
  abilities: readonly CombatAbilitySummary[];
}

export interface CombatTimelineSnapshot {
  schema_version: typeof COMBAT_SNAPSHOT_SCHEMA_VERSION;
  session_id: string;
  deployment_id: string;
  region_id: string;
  world_id: string | null;
  client_build: string;
  protocol_pack_digest: string;
  rdps_status: string;
  encounter_id: string | null;
  encounter_state: string | null;
  event_count: number;
  data_gap_count: number;
  combat_window_count: number;
  combat_started_micros: number | null;
  combat_ended_micros: number | null;
  active_combat_micros: number;
  encounter_elapsed_micros: number | null;
  encounter_terminal_micros: number | null;
  run_terminal_micros: number | null;
  run_elapsed_micros: number | null;
  game_time_micros: number | null;
  true_time_micros: number | null;
  closed_at_log_end: boolean;
  rdps_damage_influences: readonly HistoryDamageInfluenceSummary[];
  rdps_damage_influences_truncated: boolean;
  rdps_effect_presentations: readonly HistoryRdpsEffectPresentation[];
  actors: readonly CombatActorSummary[];
}

export type CombatActorSortKey =
  | "run_dps"
  | "encounter_dps"
  | "active_dps"
  | "hps"
  | "tps"
  | "rdps_damage"
  | "rdps"
  | "rdps_contribution_given"
  | "rdps_contribution_received"
  | "reported_damage"
  | "effective_damage"
  | "effective_healing"
  | "deaths";

export function parseCombatTimelineSnapshot(
  value: unknown,
): CombatTimelineSnapshot {
  if (
    !isRecord(value) ||
    value.schema_version !== COMBAT_SNAPSHOT_SCHEMA_VERSION ||
    typeof value.session_id !== "string" ||
    typeof value.deployment_id !== "string" ||
    typeof value.region_id !== "string" ||
    !isOptionalString(value.world_id) ||
    typeof value.client_build !== "string" ||
    typeof value.protocol_pack_digest !== "string" ||
    typeof value.rdps_status !== "string" ||
    !isOptionalString(value.encounter_id) ||
    !isOptionalString(value.encounter_state) ||
    !isSafeCounter(value.event_count) ||
    !isSafeCounter(value.data_gap_count) ||
    !isSafeCounter(value.combat_window_count) ||
    !isOptionalCounter(value.combat_started_micros) ||
    !isOptionalCounter(value.combat_ended_micros) ||
    !isSafeCounter(value.active_combat_micros) ||
    (value.encounter_elapsed_micros !== undefined &&
      !isOptionalCounter(value.encounter_elapsed_micros)) ||
    (value.encounter_terminal_micros !== undefined &&
      !isOptionalCounter(value.encounter_terminal_micros)) ||
    (value.run_terminal_micros !== undefined &&
      !isOptionalCounter(value.run_terminal_micros)) ||
    !isOptionalCounter(value.run_elapsed_micros) ||
    !isOptionalCounter(value.game_time_micros) ||
    !isOptionalCounter(value.true_time_micros) ||
    typeof value.closed_at_log_end !== "boolean" ||
    (value.rdps_damage_influences !== undefined &&
      (!Array.isArray(value.rdps_damage_influences) ||
        value.rdps_damage_influences.length > 4_096 ||
        !value.rdps_damage_influences.every(isLiveDamageInfluence))) ||
    (value.rdps_damage_influences_truncated !== undefined &&
      typeof value.rdps_damage_influences_truncated !== "boolean") ||
    (value.rdps_effect_presentations !== undefined &&
      (!Array.isArray(value.rdps_effect_presentations) ||
        value.rdps_effect_presentations.length > 4_096 ||
        !value.rdps_effect_presentations.every(isLiveRdpsEffectPresentation))) ||
    !Array.isArray(value.actors) ||
    !value.actors.every(isCombatActor)
  ) {
    throw new Error(
      "The local host returned an invalid or unsupported Combat Meter snapshot.",
    );
  }
  const snapshot = {
    ...value,
    rdps_damage_influences: (value.rdps_damage_influences ?? []).map(
      (influence) => ({
        ...influence,
        attributed_rdps: influence.attributed_rdps ?? null,
        attribution_component: influence.attribution_component ?? null,
        complete_effect: influence.complete_effect !== false,
      }),
    ),
    rdps_damage_influences_truncated:
      value.rdps_damage_influences_truncated === true,
    rdps_effect_presentations: value.rdps_effect_presentations ?? [],
    actors: value.actors.map((actor) => ({
      ...actor,
      run_dps: isFiniteNumber(actor.run_dps) ? actor.run_dps : actor.dps,
      encounter_dps: isFiniteNumber(actor.encounter_dps)
        ? actor.encounter_dps
        : actor.dps,
      active_dps: isFiniteNumber(actor.active_dps) ? actor.active_dps : actor.dps,
      rdps_incomplete: actor.rdps_incomplete === true,
    })),
    encounter_elapsed_micros: value.encounter_elapsed_micros ?? null,
    encounter_terminal_micros: value.encounter_terminal_micros ?? null,
    run_terminal_micros: value.run_terminal_micros ?? null,
  } as unknown as CombatTimelineSnapshot;
  if (describeRdpsStatus(snapshot.rdps_status).providerCreditEnabled) {
    return snapshot;
  }
  return {
    ...snapshot,
    rdps_damage_influences: [],
    rdps_damage_influences_truncated: false,
    rdps_effect_presentations: [],
    actors: snapshot.actors.map((actor) => ({
      ...actor,
      rdps_damage: null,
      rdps: null,
      rdps_contribution_given: null,
      rdps_contribution_received: null,
      rdps_incomplete: false,
    })),
  };
}

function isLiveRdpsEffectPresentation(
  value: unknown,
): value is HistoryRdpsEffectPresentation {
  return (
    isRecord(value) &&
    isDecimalIdentifier(value.effect_id, true) &&
    typeof value.presentation_name === "string" &&
    typeof value.presentation_kind === "string" &&
    typeof value.presentation_resolution === "string" &&
    isOptionalString(value.icon_asset_path)
  );
}

function isLiveDamageInfluence(value: unknown): value is HistoryDamageInfluenceSummary {
  return (
    isRecord(value) &&
    isDecimalIdentifier(value.effect_id, true) &&
    (value.attribution_component === undefined ||
      isOptionalString(value.attribution_component)) &&
    (value.complete_effect === undefined || typeof value.complete_effect === "boolean") &&
    isDecimalIdentifier(value.provider_actor_id, false) &&
    isDecimalIdentifier(value.provider_entity_uuid, true) &&
    isDecimalIdentifier(value.recipient_actor_id, false) &&
    isDecimalIdentifier(value.recipient_entity_uuid, true) &&
    isOptionalDecimalIdentifier(value.affected_ability_id, true) &&
    isOptionalDecimalIdentifier(value.target_actor_id, false) &&
    isOptionalDecimalIdentifier(value.target_entity_uuid, true) &&
    isSafeCounter(value.first_observed_micros) &&
    isSafeCounter(value.last_observed_micros) &&
    isSafeCounter(value.damage_event_count) &&
    isDecimalIdentifier(value.observed_damage, true) &&
    isDecimalIdentifier(value.exact_integer_delta, true) &&
    Array.isArray(value.exact_rational_deltas) &&
    value.exact_rational_deltas.every(
      (delta) =>
        isRecord(delta) &&
        isDecimalIdentifier(delta.numerator, true) &&
        isDecimalIdentifier(delta.denominator, false) &&
        delta.denominator !== "0" &&
        isSafeCounter(delta.contribution_count),
    ) &&
    (value.attributed_rdps === undefined ||
      isOptionalDecimalIdentifier(value.attributed_rdps, true)) &&
    typeof value.damage_context_complete === "boolean"
  );
}

export function sortCombatActors(
  actors: readonly CombatActorSummary[],
  key: CombatActorSortKey,
  direction: "ascending" | "descending",
): CombatActorSummary[] {
  const factor = direction === "ascending" ? 1 : -1;
  return [...actors].sort((left, right) => {
    const numeric = ((left[key] ?? Number.NEGATIVE_INFINITY) -
      (right[key] ?? Number.NEGATIVE_INFINITY)) * factor;
    if (numeric !== 0) return numeric;
    const name = actorLabel(left).localeCompare(actorLabel(right));
    if (name !== 0) return name;
    return compareDecimalIdentifiers(left.actor_id, right.actor_id);
  });
}

export function actorLabel(actor: CombatActorSummary): string {
  return actor.display_name?.trim() || `Entity UUID ${actor.entity_uuid}`;
}

function isCombatActor(value: unknown): value is CombatActorSummary {
  return (
    isRecord(value) &&
    isDecimalIdentifier(value.actor_id, false) &&
    isDecimalIdentifier(value.entity_uuid, true) &&
    isOptionalString(value.display_name) &&
    isOptionalString(value.actor_kind) &&
    isOptionalSafeInteger(value.class_id) &&
    isOptionalSafeInteger(value.specialization_id) &&
    isOptionalCounter(value.level) &&
    isOptionalSafeInteger(value.seasonal_score) &&
    isSafeInteger(value.reported_damage) &&
    isSafeInteger(value.effective_damage) &&
    isSafeInteger(value.hp_damage) &&
    isSafeInteger(value.shield_damage) &&
    isSafeInteger(value.damage_during_combat) &&
    isSafeInteger(value.damage_taken) &&
    isFiniteNumber(value.dps) &&
    (value.run_dps === undefined || isFiniteNumber(value.run_dps)) &&
    (value.encounter_dps === undefined || isFiniteNumber(value.encounter_dps)) &&
    (value.active_dps === undefined || isFiniteNumber(value.active_dps)) &&
    isFiniteNumber(value.hps) &&
    isFiniteNumber(value.tps) &&
    isOptionalSafeInteger(value.rdps_damage) &&
    isOptionalFiniteNumber(value.rdps) &&
    isOptionalSafeInteger(value.rdps_contribution_given) &&
    isOptionalSafeInteger(value.rdps_contribution_received) &&
    (value.rdps_incomplete === undefined || typeof value.rdps_incomplete === "boolean") &&
    isSafeInteger(value.reported_healing) &&
    isSafeInteger(value.effective_healing) &&
    isSafeInteger(value.overheal) &&
    isSafeInteger(value.shielding) &&
    isSafeCounter(value.casts) &&
    isSafeCounter(value.hits) &&
    isSafeCounter(value.critical_hits) &&
    isSafeCounter(value.deaths) &&
    isSafeCounter(value.revives) &&
    isSafeCounter(value.position_samples) &&
    isFiniteNumber(value.path_distance) &&
    Array.isArray(value.abilities) &&
    value.abilities.every(isCombatAbility)
  );
}

function isCombatAbility(value: unknown): value is CombatAbilitySummary {
  return (
    isRecord(value) &&
    isDecimalIdentifier(value.ability_id, true) &&
    isSafeCounter(value.casts) &&
    isSafeCounter(value.hits) &&
    isSafeCounter(value.critical_hits) &&
    isSafeInteger(value.reported_damage) &&
    isSafeInteger(value.effective_damage) &&
    isSafeInteger(value.reported_healing) &&
    isSafeInteger(value.effective_healing) &&
    isSafeInteger(value.shielding)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isOptionalString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value);
}

function isSafeCounter(value: unknown): value is number {
  return isSafeInteger(value) && value >= 0;
}

function isOptionalSafeInteger(value: unknown): value is number | null {
  return value === null || isSafeInteger(value);
}

function isOptionalCounter(value: unknown): value is number | null {
  return value === null || isSafeCounter(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isOptionalFiniteNumber(value: unknown): value is number | null {
  return value === null || isFiniteNumber(value);
}

function isDecimalIdentifier(value: unknown, signed: boolean): value is string {
  if (typeof value !== "string") return false;
  return signed
    ? /^(?:0|-?[1-9]\d*)$/.test(value)
    : /^(?:0|[1-9]\d*)$/.test(value);
}

function isOptionalDecimalIdentifier(
  value: unknown,
  signed: boolean,
): value is string | null {
  return value === null || isDecimalIdentifier(value, signed);
}

function compareDecimalIdentifiers(left: string, right: string): number {
  const leftValue = BigInt(left);
  const rightValue = BigInt(right);
  return leftValue < rightValue ? -1 : leftValue > rightValue ? 1 : 0;
}
