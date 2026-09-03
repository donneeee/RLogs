import { describeRdpsStatus } from "./rdps-status";

export const COMBAT_HISTORY_SCHEMA_VERSION = 1;

export interface CombatHistoryCatalog {
  schema_version: 1;
  entries: CombatHistoryCatalogEntry[];
}

export interface CombatHistoryCatalogEntry {
  history_id: string;
  is_favorite: boolean;
  session_id: string;
  run_index: number;
  captured_unix_millis: number;
  activity_id: string | null;
  activity_family_id: string | null;
  scene_id: number | null;
  presentation_scene_name: string | null;
  difficulty_family: string | null;
  difficulty_tier: number | null;
  terminal_state: string;
  game_time_micros: number | null;
  total_run_time_micros: number | null;
  active_combat_micros: number;
  player_count: number;
  deployment_id: string;
  region_id: string;
  world_id: string | null;
  team_damage: number;
  team_dps: number;
  team_encounter_dps: number;
  true_time_micros: number | null;
  retry_count: number;
  boss_retry_count: number;
  wipe_count: number;
  cleared_encounter_count: number;
  last_encounter_terminal_state: string | null;
  participants: CombatHistoryParticipant[];
}

export interface CombatHistoryDeleteResult {
  requested_count: number;
  deleted_count: number;
  preserved_favorite_count: number;
  unknown_history_id_count: number;
  cleanup_warnings: string[];
}

export interface CombatHistoryParticipant {
  actor_id: string;
  entity_uuid: string;
  display_name: string | null;
  actor_kind: string | null;
  class_id: number | null;
  specialization_id: number | null;
  presentation_class_name: string | null;
  presentation_specialization_name: string | null;
  level: number | null;
  ability_score: number | null;
  weapon_item_id: number | null;
  weapon_breakthrough_count: number | null;
  weapon_icon_asset_path: string | null;
  weapon_presentation_name: string | null;
  weapon_level: number | null;
  weapon_level_min: number | null;
  weapon_level_max: number | null;
  weapon_badge_kind: string | null;
  seasonal_score: number | null;
  primary_loadout: HistoryLoadoutSlot[];
  auxiliary_loadout: HistoryLoadoutSlot[];
  damage: number;
  dps: number;
  encounter_dps: number;
  character_id: string | null;
  presentation_name: string | null;
  presentation_kind: string | null;
  icon_asset_path: string | null;
  presentation_role: string | null;
  presentation_accent: string | null;
}

export interface CombatHistorySnapshot {
  schema_version: 1;
  session_id: string;
  deployment_id: string;
  region_id: string;
  world_id: string | null;
  client_build: string;
  protocol_pack_digest: string;
  rdps_formula_identity: string | null;
  runs: CombatRunHistory[];
}

export interface CombatRunHistory {
  run_index: number;
  activity_id: string | null;
  activity_family_id: string | null;
  scene_id: number | null;
  presentation_scene_name: string | null;
  instance_id: string | null;
  difficulty_family: string | null;
  difficulty_tier: number | null;
  terminal_state: string;
  entered_micros: number | null;
  started_micros: number;
  first_combat_micros: number | null;
  ended_micros: number | null;
  load_time_micros: number | null;
  precombat_time_micros: number | null;
  total_run_time_micros: number | null;
  game_time_micros: number | null;
  true_time_micros: number | null;
  retry_count: number;
  boss_retry_count: number;
  wipe_count: number;
  cleared_encounter_count: number;
  last_encounter_terminal_state: string | null;
  rdps_status: string;
  apm_status: string;
  views: CombatHistoryView[];
}

export interface CombatHistoryView {
  id: string;
  label: string;
  kind: string;
  segment_indices: number[];
  elapsed_micros: number;
  active_combat_micros: number;
  actors: HistoryActorSummary[];
  targets: HistoryTargetIdentity[];
  damage_influences: HistoryDamageInfluenceSummary[];
  rdps_effect_presentations: HistoryRdpsEffectPresentation[];
}

export interface HistoryRdpsEffectPresentation {
  effect_id: string;
  presentation_name: string;
  presentation_kind: string;
  presentation_resolution: string;
  icon_asset_path: string | null;
}

export interface HistoryRationalDamageDelta {
  numerator: string;
  denominator: string;
  contribution_count: number;
}

export interface HistoryDamageInfluenceSummary {
  effect_id: string;
  attribution_component?: string | null;
  complete_effect?: boolean;
  provider_actor_id: string;
  provider_entity_uuid: string;
  recipient_actor_id: string;
  recipient_entity_uuid: string;
  affected_ability_id: string | null;
  target_actor_id: string | null;
  target_entity_uuid: string | null;
  first_observed_micros: number;
  last_observed_micros: number;
  damage_event_count: number;
  critical_hit_count?: number | null;
  observed_damage: string;
  exact_integer_delta: string;
  exact_rational_deltas: HistoryRationalDamageDelta[];
  attributed_rdps: string | null;
  damage_context_complete: boolean;
}

export interface HistoryActorSummary {
  actor_id: string;
  entity_uuid: string;
  monster_id: string | null;
  character_id: string | null;
  display_name: string | null;
  actor_kind: string | null;
  presentation_name: string | null;
  presentation_kind: string | null;
  class_id: number | null;
  specialization_id: number | null;
  presentation_class_name: string | null;
  presentation_specialization_name: string | null;
  icon_asset_path: string | null;
  presentation_role: string | null;
  presentation_accent: string | null;
  level: number | null;
  ability_score: number | null;
  weapon_item_id: number | null;
  weapon_breakthrough_count: number | null;
  weapon_icon_asset_path: string | null;
  weapon_presentation_name: string | null;
  weapon_level: number | null;
  weapon_level_min: number | null;
  weapon_level_max: number | null;
  weapon_badge_kind: string | null;
  seasonal_score: number | null;
  primary_loadout: HistoryLoadoutSlot[];
  auxiliary_loadout: HistoryLoadoutSlot[];
  damage: number;
  effective_damage: number;
  damage_taken: number;
  healing: number;
  effective_healing: number;
  shielding: number;
  hits: number;
  critical_hits: number;
  deaths: number;
  death_seconds: number[];
  dps: number;
  encounter_dps: number;
  hps: number;
  tps: number;
  rdps: number | null;
  rdps_damage: number | null;
  rdps_contribution_given: number | null;
  rdps_contribution_received: number | null;
  rdps_incomplete: boolean;
  apm: number | null;
  observed_cast_events: number;
  abilities: HistoryAbilitySummary[];
  targets: HistoryTargetSummary[];
  effects: HistoryEffectSummary[];
  series: HistorySeriesPoint[];
}

export interface HistoryLoadoutSlot {
  slot_id: number;
  ability_id: number | null;
  item_id: number | null;
  tier: number | null;
  presentation_name: string | null;
  icon_asset_path: string | null;
  item_tier: number | null;
  maximum_tier: number | null;
}

export interface HistoryAbilitySummary {
  ability_id: string;
  presentation_name: string | null;
  presentation_kind: string | null;
  presentation_resolution: string | null;
  icon_asset_path: string | null;
  presentation_recount_group_id: string | null;
  presentation_recount_group_name: string | null;
  casts: number;
  hits: number;
  critical_hits: number;
  /** False only for synthesized rows whose legacy evidence omitted crit flags. */
  critical_hits_observed?: boolean;
  damage: number;
  effective_damage: number;
  healing: number;
  effective_healing: number;
  shielding: number;
  dps: number;
  encounter_dps: number;
  hps: number;
  targets: HistoryAbilityTargetSummary[];
}

export interface HistoryAbilityTargetSummary {
  actor_id: string;
  entity_uuid: string;
  damage: number;
  effective_damage: number;
  healing: number;
  effective_healing: number;
  shielding: number;
  hits: number;
  critical_hits: number;
  critical_hits_observed?: boolean;
}

export interface HistoryTargetSummary {
  actor_id: string;
  entity_uuid: string;
  damage: number;
  effective_damage: number;
  hits: number;
  critical_hits: number;
  effect_events: number;
  series: HistorySeriesPoint[];
}

export interface HistoryTargetIdentity {
  actor_id: string;
  entity_uuid: string;
  monster_id: string | null;
  display_name: string | null;
  actor_kind: string | null;
  presentation_name: string | null;
}

export interface HistoryEffectSummary {
  effect_id: string;
  presentation_name: string | null;
  presentation_kind: string | null;
  presentation_resolution: string | null;
  icon_asset_path: string | null;
  target_actor_id: string;
  target_entity_uuid: string;
  applied: number;
  refreshed: number;
  stacked: number;
  consumed: number;
  removed: number;
}

export interface HistorySeriesPoint {
  second: number;
  damage: number;
  effective_healing: number;
  damage_taken: number;
}

export function parseCombatHistoryCatalog(value: unknown): CombatHistoryCatalog {
  const catalog = record(value, "combat history catalog");
  if (catalog.schema_version !== COMBAT_HISTORY_SCHEMA_VERSION) {
    throw new Error("The combat history catalog uses an unsupported schema.");
  }
  const entries = array(catalog.entries, "combat history entries", 2_048);
  for (const [index, value] of entries.entries()) {
    const entry = record(value, `combat history entry ${index}`);
    text(entry.history_id, "history ID");
    if (entry.is_favorite === undefined) entry.is_favorite = false;
    boolean(entry.is_favorite, "favorite state");
    text(entry.session_id, "session ID");
    counter(entry.run_index, "run index");
    counter(entry.captured_unix_millis, "capture time");
    optionalText(entry.activity_id, "activity ID");
    optionalText(entry.activity_family_id, "activity family");
    optionalInteger(entry.scene_id, "scene ID");
    if (entry.presentation_scene_name === undefined) entry.presentation_scene_name = null;
    optionalText(entry.presentation_scene_name, "scene name");
    optionalText(entry.difficulty_family, "difficulty");
    optionalCounter(entry.difficulty_tier, "difficulty tier");
    text(entry.terminal_state, "terminal state");
    optionalCounter(entry.game_time_micros, "game time");
    optionalCounter(entry.total_run_time_micros, "total run time");
    counter(entry.active_combat_micros, "active combat time");
    counter(entry.player_count, "player count");
    text(entry.deployment_id, "deployment ID");
    text(entry.region_id, "region ID");
    optionalText(entry.world_id, "world ID");
    integer(entry.team_damage, "team damage");
    finiteNumber(entry.team_dps, "team DPS");
    finiteNumber(entry.team_encounter_dps, "team encounter DPS");
    optionalCounter(entry.true_time_micros, "true time");
    counter(entry.retry_count, "retry count");
    counter(entry.boss_retry_count, "boss retry count");
    if (entry.wipe_count === undefined) entry.wipe_count = 0;
    if (entry.cleared_encounter_count === undefined) entry.cleared_encounter_count = 0;
    if (entry.last_encounter_terminal_state === undefined) {
      entry.last_encounter_terminal_state = null;
    }
    counter(entry.wipe_count, "wipe count");
    counter(entry.cleared_encounter_count, "cleared encounter count");
    optionalText(entry.last_encounter_terminal_state, "last encounter terminal state");
    array(entry.participants, "history participants", 40).forEach((value, participantIndex) => {
      const participant = record(value, `history participant ${participantIndex}`);
      text(participant.actor_id, "participant actor ID");
      text(participant.entity_uuid, "participant entity UUID");
      optionalText(participant.display_name, "participant display name");
      optionalText(participant.actor_kind, "participant actor kind");
      optionalInteger(participant.class_id, "participant class ID");
      optionalInteger(participant.specialization_id, "participant specialization ID");
      optionalText(participant.presentation_class_name, "participant class name");
      optionalText(
        participant.presentation_specialization_name,
        "participant specialization name",
      );
      optionalCounter(participant.level, "participant level");
      if (participant.ability_score === undefined) participant.ability_score = null;
      optionalInteger(participant.ability_score, "participant ability score");
      if (participant.weapon_item_id === undefined) participant.weapon_item_id = null;
      optionalInteger(participant.weapon_item_id, "participant weapon item ID");
      if (participant.weapon_breakthrough_count === undefined) {
        participant.weapon_breakthrough_count = null;
      }
      optionalCounter(
        participant.weapon_breakthrough_count,
        "participant weapon breakthrough count",
      );
      if (participant.weapon_icon_asset_path === undefined) {
        participant.weapon_icon_asset_path = null;
      }
      optionalText(participant.weapon_icon_asset_path, "participant weapon icon path");
      if (participant.weapon_presentation_name === undefined) participant.weapon_presentation_name = null;
      optionalText(participant.weapon_presentation_name, "participant weapon name");
      for (const field of ["weapon_level", "weapon_level_min", "weapon_level_max"] as const) {
        if (participant[field] === undefined) participant[field] = null;
        optionalCounter(participant[field], `participant ${field.replaceAll("_", " ")}`);
      }
      if (participant.weapon_badge_kind === undefined) participant.weapon_badge_kind = null;
      optionalText(participant.weapon_badge_kind, "participant weapon badge kind");
      optionalInteger(participant.seasonal_score, "participant seasonal score");
      normalizeLoadout(participant, "primary_loadout", "participant primary loadout");
      normalizeLoadout(participant, "auxiliary_loadout", "participant auxiliary loadout");
      integer(participant.damage, "participant damage");
      finiteNumber(participant.dps, "participant DPS");
      finiteNumber(participant.encounter_dps, "participant encounter DPS");
      optionalText(participant.character_id, "participant character ID");
      optionalText(participant.presentation_name, "participant presentation name");
      optionalText(participant.presentation_kind, "participant presentation kind");
      optionalText(participant.icon_asset_path, "participant icon path");
      if (participant.presentation_role === undefined) participant.presentation_role = null;
      if (participant.presentation_accent === undefined) participant.presentation_accent = null;
      optionalText(participant.presentation_role, "participant presentation role");
      optionalText(participant.presentation_accent, "participant presentation accent");
    });
  }
  return catalog as unknown as CombatHistoryCatalog;
}

export function parseCombatHistoryDeleteResult(
  value: unknown,
): CombatHistoryDeleteResult {
  const result = record(value, "combat history deletion result");
  counter(result.requested_count, "requested deletion count");
  counter(result.deleted_count, "deleted history count");
  counter(result.preserved_favorite_count, "preserved favorite count");
  counter(result.unknown_history_id_count, "unknown history count");
  array(result.cleanup_warnings, "history cleanup warnings", 2_048).forEach(
    (warning) => text(warning, "history cleanup warning"),
  );
  return result as unknown as CombatHistoryDeleteResult;
}

export function parseCombatHistorySnapshot(value: unknown): CombatHistorySnapshot {
  const snapshot = record(value, "combat history");
  if (snapshot.schema_version !== COMBAT_HISTORY_SCHEMA_VERSION) {
    throw new Error("The combat history detail uses an unsupported schema.");
  }
  text(snapshot.session_id, "session ID");
  text(snapshot.deployment_id, "deployment ID");
  text(snapshot.region_id, "region ID");
  optionalText(snapshot.world_id, "world ID");
  text(snapshot.client_build, "client build");
  text(snapshot.protocol_pack_digest, "protocol pack digest");
  if (snapshot.rdps_formula_identity === undefined) {
    snapshot.rdps_formula_identity = null;
  }
  optionalText(snapshot.rdps_formula_identity, "rDPS formula identity");
  const runs = array(snapshot.runs, "combat history runs", 64);
  for (const [runIndex, value] of runs.entries()) {
    const run = record(value, `run ${runIndex}`);
    counter(run.run_index, "run index");
    text(run.rdps_status, "run rDPS status");
    const providerCreditEnabled =
      snapshot.rdps_formula_identity !== null &&
      describeRdpsStatus(run.rdps_status as string).providerCreditEnabled;
    if (run.wipe_count === undefined) run.wipe_count = 0;
    if (run.cleared_encounter_count === undefined) run.cleared_encounter_count = 0;
    if (run.last_encounter_terminal_state === undefined) {
      run.last_encounter_terminal_state = null;
    }
    counter(run.wipe_count, "wipe count");
    counter(run.cleared_encounter_count, "cleared encounter count");
    optionalText(run.last_encounter_terminal_state, "last encounter terminal state");
    if (run.presentation_scene_name === undefined) run.presentation_scene_name = null;
    optionalText(run.presentation_scene_name, "scene name");
    array(run.views, "history views", 128).forEach((view, viewIndex) => {
      const parsed = record(view, `run ${runIndex} view ${viewIndex}`);
      text(parsed.id, "view ID");
      text(parsed.label, "view label");
      counter(parsed.elapsed_micros, "elapsed time");
      counter(parsed.active_combat_micros, "active combat time");
      array(parsed.actors, "view actors", 100_000).forEach((actor, actorIndex) => {
        const parsedActor = record(
          actor,
          `run ${runIndex} view ${viewIndex} actor ${actorIndex}`,
        );
        if (parsedActor.death_seconds === undefined) {
          parsedActor.death_seconds = [];
        }
        if (parsedActor.character_id === undefined) {
          parsedActor.character_id = null;
        }
        if (parsedActor.monster_id === undefined) {
          parsedActor.monster_id = null;
        }
        optionalText(parsedActor.monster_id, "actor monster ID");
        if (parsedActor.presentation_name === undefined) {
          parsedActor.presentation_name = null;
        }
        if (parsedActor.presentation_kind === undefined) {
          parsedActor.presentation_kind = null;
        }
        if (parsedActor.specialization_id === undefined) {
          parsedActor.specialization_id = null;
        }
        if (parsedActor.presentation_class_name === undefined) {
          parsedActor.presentation_class_name = null;
        }
        if (parsedActor.presentation_specialization_name === undefined) {
          parsedActor.presentation_specialization_name = null;
        }
        if (parsedActor.icon_asset_path === undefined) {
          parsedActor.icon_asset_path = null;
        }
        if (parsedActor.presentation_role === undefined) {
          parsedActor.presentation_role = null;
        }
        if (parsedActor.presentation_accent === undefined) {
          parsedActor.presentation_accent = null;
        }
        optionalText(parsedActor.presentation_role, "actor presentation role");
        optionalText(parsedActor.presentation_accent, "actor presentation accent");
        if (parsedActor.seasonal_score === undefined) {
          parsedActor.seasonal_score = null;
        }
        if (parsedActor.ability_score === undefined) {
          parsedActor.ability_score = null;
        }
        optionalInteger(parsedActor.ability_score, "actor ability score");
        if (parsedActor.weapon_item_id === undefined) {
          parsedActor.weapon_item_id = null;
        }
        optionalInteger(parsedActor.weapon_item_id, "actor weapon item ID");
        if (parsedActor.weapon_breakthrough_count === undefined) {
          parsedActor.weapon_breakthrough_count = null;
        }
        optionalCounter(parsedActor.weapon_breakthrough_count, "actor weapon breakthrough count");
        if (parsedActor.weapon_icon_asset_path === undefined) {
          parsedActor.weapon_icon_asset_path = null;
        }
        optionalText(parsedActor.weapon_icon_asset_path, "actor weapon icon path");
        if (parsedActor.weapon_presentation_name === undefined) parsedActor.weapon_presentation_name = null;
        optionalText(parsedActor.weapon_presentation_name, "actor weapon name");
        for (const field of ["weapon_level", "weapon_level_min", "weapon_level_max"] as const) {
          if (parsedActor[field] === undefined) parsedActor[field] = null;
          optionalCounter(parsedActor[field], `actor ${field.replaceAll("_", " ")}`);
        }
        if (parsedActor.weapon_badge_kind === undefined) parsedActor.weapon_badge_kind = null;
        optionalText(parsedActor.weapon_badge_kind, "actor weapon badge kind");
        if (parsedActor.rdps_damage === undefined) parsedActor.rdps_damage = null;
        if (parsedActor.rdps === undefined) parsedActor.rdps = null;
        if (parsedActor.rdps_contribution_given === undefined) {
          parsedActor.rdps_contribution_given = null;
        }
        if (parsedActor.rdps_contribution_received === undefined) {
          parsedActor.rdps_contribution_received = null;
        }
        if (parsedActor.rdps_incomplete === undefined) parsedActor.rdps_incomplete = false;
        if (typeof parsedActor.rdps_incomplete !== "boolean") {
          throw new Error("actor rDPS incomplete marker must be boolean");
        }
        optionalInteger(parsedActor.rdps_damage, "actor rDPS damage");
        if (parsedActor.rdps !== null) finiteNumber(parsedActor.rdps, "actor rDPS");
        optionalInteger(
          parsedActor.rdps_contribution_given,
          "actor rDPS contribution given",
        );
        optionalInteger(
          parsedActor.rdps_contribution_received,
          "actor rDPS contribution received",
        );
        if (!providerCreditEnabled) {
          parsedActor.rdps = null;
          parsedActor.rdps_damage = null;
          parsedActor.rdps_contribution_given = null;
          parsedActor.rdps_contribution_received = null;
          parsedActor.rdps_incomplete = false;
        }
        normalizeLoadout(parsedActor, "primary_loadout", "actor primary loadout");
        normalizeLoadout(parsedActor, "auxiliary_loadout", "actor auxiliary loadout");
        normalizeCombatPresentationRows(
          parsedActor,
          "abilities",
          "actor ability",
        );
        normalizeCombatPresentationRows(
          parsedActor,
          "effects",
          "actor effect",
        );
        if (parsedActor.targets === undefined) parsedActor.targets = [];
        array(parsedActor.targets, "actor target rows", 100_000).forEach(
          (target, targetIndex) => {
            const parsedTarget = record(target, `actor target ${targetIndex}`);
            if (parsedTarget.series === undefined) parsedTarget.series = [];
            normalizeHistorySeries(parsedTarget.series, `actor target ${targetIndex} series`);
          },
        );
        if (parsedActor.series === undefined) parsedActor.series = [];
        normalizeHistorySeries(parsedActor.series, "actor series");
        array(parsedActor.death_seconds, "actor death seconds", 10_000).forEach(
          (second) => counter(second, "actor death second"),
        );
      });
      const parsedTargets = array(parsed.targets, "view targets", 100_000);
      parsedTargets.forEach((target, targetIndex) => {
        const parsedTarget = record(
          target,
          `run ${runIndex} view ${viewIndex} target ${targetIndex}`,
        );
        text(parsedTarget.actor_id, "target actor ID");
        text(parsedTarget.entity_uuid, "target entity UUID");
        if (parsedTarget.monster_id === undefined) parsedTarget.monster_id = null;
        if (parsedTarget.presentation_name === undefined) {
          parsedTarget.presentation_name = null;
        }
        optionalText(parsedTarget.monster_id, "target monster ID");
        optionalText(parsedTarget.display_name, "target display name");
        optionalText(parsedTarget.actor_kind, "target actor kind");
        optionalText(parsedTarget.presentation_name, "target presentation name");
      });
      // Current formal-run projections omit owned pets and ambient training
      // dummies. Apply the same boundary while reading saved projections made
      // before that rule. Actor rows and direct-source evidence remain intact;
      // damageable projectile mechanics intentionally remain selectable.
      parsed.targets = parsedTargets.filter((target) => {
        const parsedTarget = target as Record<string, unknown>;
        return (
          parsedTarget.actor_kind !== "pet" &&
          parsedTarget.actor_kind !== "training_dummy"
        );
      });
      if (parsed.rdps_effect_presentations === undefined) {
        parsed.rdps_effect_presentations = [];
      }
      array(
        parsed.rdps_effect_presentations,
        "view rDPS effect presentations",
        2_000,
      ).forEach((presentation, presentationIndex) => {
        const parsedPresentation = record(
          presentation,
          `run ${runIndex} view ${viewIndex} rDPS effect presentation ${presentationIndex}`,
        );
        text(parsedPresentation.effect_id, "rDPS effect presentation ID");
        text(parsedPresentation.presentation_name, "rDPS effect presentation name");
        text(parsedPresentation.presentation_kind, "rDPS effect presentation kind");
        text(
          parsedPresentation.presentation_resolution,
          "rDPS effect presentation resolution",
        );
        if (parsedPresentation.icon_asset_path === undefined) {
          parsedPresentation.icon_asset_path = null;
        }
        optionalText(parsedPresentation.icon_asset_path, "rDPS effect presentation icon");
      });
      if (parsed.damage_influences === undefined) parsed.damage_influences = [];
      array(parsed.damage_influences, "view damage influences", 1_000_000).forEach(
        (influence, influenceIndex) => {
          const parsedInfluence = record(
            influence,
            `run ${runIndex} view ${viewIndex} damage influence ${influenceIndex}`,
          );
          text(parsedInfluence.effect_id, "influence effect ID");
          if (parsedInfluence.attribution_component === undefined) {
            parsedInfluence.attribution_component = null;
          }
          if (parsedInfluence.complete_effect === undefined) {
            parsedInfluence.complete_effect = true;
          }
          optionalText(
            parsedInfluence.attribution_component,
            "influence attribution component",
          );
          boolean(parsedInfluence.complete_effect, "influence complete effect state");
          text(parsedInfluence.provider_actor_id, "influence provider actor ID");
          text(parsedInfluence.provider_entity_uuid, "influence provider entity UUID");
          text(parsedInfluence.recipient_actor_id, "influence recipient actor ID");
          text(parsedInfluence.recipient_entity_uuid, "influence recipient entity UUID");
          optionalText(parsedInfluence.affected_ability_id, "influence ability ID");
          optionalText(parsedInfluence.target_actor_id, "influence target actor ID");
          optionalText(parsedInfluence.target_entity_uuid, "influence target entity UUID");
          counter(parsedInfluence.first_observed_micros, "influence first timestamp");
          counter(parsedInfluence.last_observed_micros, "influence last timestamp");
          counter(parsedInfluence.damage_event_count, "influence damage event count");
          if (parsedInfluence.critical_hit_count === undefined) {
            parsedInfluence.critical_hit_count = null;
          }
          optionalCounter(parsedInfluence.critical_hit_count, "influence critical hit count");
          text(parsedInfluence.observed_damage, "influence observed damage");
          text(parsedInfluence.exact_integer_delta, "influence exact integer delta");
          if (parsedInfluence.attributed_rdps === undefined) {
            parsedInfluence.attributed_rdps = null;
          }
          optionalText(parsedInfluence.attributed_rdps, "influence attributed rDPS");
          boolean(parsedInfluence.damage_context_complete, "influence damage context state");
          array(
            parsedInfluence.exact_rational_deltas,
            "influence exact rational deltas",
            100_000,
          ).forEach((term, termIndex) => {
            const parsedTerm = record(term, `influence rational delta ${termIndex}`);
            text(parsedTerm.numerator, "influence rational numerator");
            text(parsedTerm.denominator, "influence rational denominator");
            counter(parsedTerm.contribution_count, "influence rational contribution count");
          });
        },
      );
      if (!providerCreditEnabled) {
        parsed.damage_influences = [];
        parsed.rdps_effect_presentations = [];
      }
    });
  }
  return snapshot as unknown as CombatHistorySnapshot;
}

function normalizeHistorySeries(value: unknown, label: string): void {
  array(value, label, 100_000).forEach((point, index) => {
    const parsed = record(point, `${label} point ${index}`);
    counter(parsed.second, `${label} second`);
    integer(parsed.damage, `${label} damage`);
    integer(parsed.effective_healing, `${label} effective healing`);
    integer(parsed.damage_taken, `${label} damage taken`);
  });
}

function normalizeCombatPresentationRows(
  owner: Record<string, unknown>,
  key: string,
  label: string,
): void {
  if (owner[key] === undefined) owner[key] = [];
  array(owner[key], `${label} rows`, 100_000).forEach((value, index) => {
    const row = record(value, `${label} ${index}`);
    for (const field of [
      "presentation_name",
      "presentation_kind",
      "presentation_resolution",
      "icon_asset_path",
      "presentation_recount_group_id",
      "presentation_recount_group_name",
    ]) {
      if (row[field] === undefined) row[field] = null;
      optionalText(row[field], `${label} ${field}`);
    }
  });
}

function normalizeLoadout(
  owner: Record<string, unknown>,
  key: string,
  label: string,
): void {
  if (owner[key] === undefined) owner[key] = [];
  array(owner[key], label, 16).forEach((value, index) => {
    const slot = record(value, `${label} slot ${index}`);
    integer(slot.slot_id, `${label} slot ID`);
    for (const field of ["ability_id", "item_id", "tier", "item_tier", "maximum_tier"]) {
      if (slot[field] === undefined) slot[field] = null;
      optionalInteger(slot[field], `${label} ${field}`);
    }
    for (const field of ["presentation_name", "icon_asset_path"]) {
      if (slot[field] === undefined) slot[field] = null;
      optionalText(slot[field], `${label} ${field}`);
    }
  });
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object.`);
  }
  return value as Record<string, unknown>;
}

function array(value: unknown, label: string, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) {
    throw new Error(`${label} must be an array with at most ${maximum} entries.`);
  }
  return value;
}

function text(value: unknown, label: string): void {
  if (typeof value !== "string" || value.length > 4_096) {
    throw new Error(`${label} must be text.`);
  }
}

function boolean(value: unknown, label: string): void {
  if (typeof value !== "boolean") {
    throw new Error(`${label} must be a boolean.`);
  }
}

function optionalText(value: unknown, label: string): void {
  if (value !== null) text(value, label);
}

function counter(value: unknown, label: string): void {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a non-negative safe integer.`);
  }
}

function optionalCounter(value: unknown, label: string): void {
  if (value !== null) counter(value, label);
}

function optionalInteger(value: unknown, label: string): void {
  if (value !== null && (typeof value !== "number" || !Number.isSafeInteger(value))) {
    throw new Error(`${label} must be a safe integer.`);
  }
}

function integer(value: unknown, label: string): void {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error(`${label} must be a safe integer.`);
  }
}

function finiteNumber(value: unknown, label: string): void {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${label} must be a finite number.`);
  }
}
