//! Canonical-event combat timeline reducer.

use std::collections::{BTreeMap, BTreeSet};

use rlogs_events::{
    ActorKind, ActorState, CanonicalEvent, CombatState, EncounterState, EventEnvelope, EventTopic,
    LifeState, TimelineEventKind,
};
use rlogs_log_format::RlogHeader;
use rlogs_plugin_api::PluginCapability;
use rlogs_plugin_runtime::{PluginFailure, PluginOutputSink, ReplayPlugin, ReplayPluginDescriptor};
use serde::{Deserialize, Serialize};

pub const COMBAT_METER_PLUGIN_ID: &str = "app.rlogs.combat-meter";
pub const COMBAT_SNAPSHOT_SCHEMA_ID: &str = "app.rlogs.combat-meter.snapshot";
pub const COMBAT_SNAPSHOT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatTimelineSnapshot {
    pub schema_version: u16,
    pub session_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub encounter_id: Option<String>,
    pub encounter_state: Option<String>,
    pub event_count: u64,
    pub data_gap_count: u64,
    pub combat_window_count: u32,
    pub combat_started_micros: Option<u64>,
    pub combat_ended_micros: Option<u64>,
    pub active_combat_micros: u64,
    pub closed_at_log_end: bool,
    pub actors: Vec<ActorCombatSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorCombatSummary {
    pub actor_id: u64,
    pub entity_uuid: i64,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    pub class_id: Option<i32>,
    pub level: Option<u32>,
    pub reported_damage: i64,
    pub effective_damage: i64,
    pub hp_damage: i64,
    pub shield_damage: i64,
    pub damage_during_combat: i64,
    pub dps: f64,
    pub reported_healing: i64,
    pub effective_healing: i64,
    pub overheal: i64,
    pub shielding: i64,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub deaths: u64,
    pub revives: u64,
    pub position_samples: u64,
    pub path_distance: f64,
    pub abilities: Vec<AbilityCombatSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbilityCombatSummary {
    pub ability_id: i64,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub reported_damage: i64,
    pub effective_damage: i64,
    pub reported_healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
}

#[derive(Debug, Clone, Default)]
struct ActorAccumulator {
    entity_uuid: i64,
    display_name: Option<String>,
    actor_kind: Option<String>,
    class_id: Option<i32>,
    level: Option<u32>,
    reported_damage: i64,
    effective_damage: i64,
    hp_damage: i64,
    shield_damage: i64,
    damage_during_combat: i64,
    reported_healing: i64,
    effective_healing: i64,
    overheal: i64,
    shielding: i64,
    casts: u64,
    hits: u64,
    critical_hits: u64,
    deaths: u64,
    revives: u64,
    position_samples: u64,
    path_distance: f64,
    last_position: Option<(f32, f32, f32)>,
    abilities: BTreeMap<i64, AbilityAccumulator>,
}

#[derive(Debug, Clone, Default)]
struct AbilityAccumulator {
    casts: u64,
    hits: u64,
    critical_hits: u64,
    reported_damage: i64,
    effective_damage: i64,
    reported_healing: i64,
    effective_healing: i64,
    shielding: i64,
}

#[derive(Debug, Default)]
pub struct CombatTimelinePlugin {
    header: Option<RlogHeader>,
    actors: BTreeMap<u64, ActorAccumulator>,
    encounter_id: Option<String>,
    encounter_state: Option<String>,
    active_combat_started: Option<u64>,
    first_combat_started: Option<u64>,
    last_combat_ended: Option<u64>,
    active_combat_micros: u64,
    combat_window_count: u32,
    last_event_micros: Option<u64>,
    event_count: u64,
    data_gap_count: u64,
    closed_at_log_end: bool,
}

impl CombatTimelinePlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn actor_mut(&mut self, actor_id: u64, entity_uuid: i64) -> &mut ActorAccumulator {
        let actor = self.actors.entry(actor_id).or_default();
        actor.entity_uuid = entity_uuid;
        actor
    }

    fn begin_combat(&mut self, observed_micros: u64) {
        if self.active_combat_started.is_none() {
            self.active_combat_started = Some(observed_micros);
            self.first_combat_started.get_or_insert(observed_micros);
            self.combat_window_count = self.combat_window_count.saturating_add(1);
        }
    }

    fn end_combat(&mut self, observed_micros: u64) {
        if let Some(started) = self.active_combat_started.take() {
            self.active_combat_micros = self
                .active_combat_micros
                .saturating_add(observed_micros.saturating_sub(started));
            self.last_combat_ended = Some(observed_micros);
        }
    }

    fn snapshot(&self) -> Result<CombatTimelineSnapshot, PluginFailure> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| PluginFailure::Message("combat plug-in was not initialized".into()))?;
        let duration = self.active_combat_micros;
        let actors = self
            .actors
            .iter()
            .map(|(actor_id, actor)| ActorCombatSummary {
                actor_id: *actor_id,
                entity_uuid: actor.entity_uuid,
                display_name: actor.display_name.clone(),
                actor_kind: actor.actor_kind.clone(),
                class_id: actor.class_id,
                level: actor.level,
                reported_damage: actor.reported_damage,
                effective_damage: actor.effective_damage,
                hp_damage: actor.hp_damage,
                shield_damage: actor.shield_damage,
                damage_during_combat: actor.damage_during_combat,
                dps: if duration == 0 {
                    0.0
                } else {
                    actor.damage_during_combat as f64 * 1_000_000.0 / duration as f64
                },
                reported_healing: actor.reported_healing,
                effective_healing: actor.effective_healing,
                overheal: actor.overheal,
                shielding: actor.shielding,
                casts: actor.casts,
                hits: actor.hits,
                critical_hits: actor.critical_hits,
                deaths: actor.deaths,
                revives: actor.revives,
                position_samples: actor.position_samples,
                path_distance: actor.path_distance,
                abilities: actor
                    .abilities
                    .iter()
                    .map(|(ability_id, ability)| AbilityCombatSummary {
                        ability_id: *ability_id,
                        casts: ability.casts,
                        hits: ability.hits,
                        critical_hits: ability.critical_hits,
                        reported_damage: ability.reported_damage,
                        effective_damage: ability.effective_damage,
                        reported_healing: ability.reported_healing,
                        effective_healing: ability.effective_healing,
                        shielding: ability.shielding,
                    })
                    .collect(),
            })
            .collect();
        Ok(CombatTimelineSnapshot {
            schema_version: COMBAT_SNAPSHOT_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            deployment_id: header.region.identity.deployment_id.clone(),
            region_id: header.region.identity.region_id.clone(),
            world_id: header.region.identity.world_id.clone(),
            client_build: header.region.client_build.clone(),
            protocol_pack_digest: header.region.protocol_pack_digest.clone(),
            encounter_id: self.encounter_id.clone(),
            encounter_state: self.encounter_state.clone(),
            event_count: self.event_count,
            data_gap_count: self.data_gap_count,
            combat_window_count: self.combat_window_count,
            combat_started_micros: self.first_combat_started,
            combat_ended_micros: self.last_combat_ended,
            active_combat_micros: duration,
            closed_at_log_end: self.closed_at_log_end,
            actors,
        })
    }
}

impl ReplayPlugin for CombatTimelinePlugin {
    fn descriptor(&self) -> ReplayPluginDescriptor {
        ReplayPluginDescriptor {
            id: COMBAT_METER_PLUGIN_ID.into(),
            name: "Combat timeline".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::EncountersRead,
            ]),
            subscriptions: BTreeSet::from([
                EventTopic::Actor,
                EventTopic::Combat,
                EventTopic::Encounter,
                EventTopic::DataQuality,
            ]),
        }
    }

    fn begin(
        &mut self,
        header: &RlogHeader,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        *self = Self::default();
        self.header = Some(header.clone());
        Ok(())
    }

    fn on_event(
        &mut self,
        envelope: &EventEnvelope,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.event_count = self.event_count.saturating_add(1);
        self.last_event_micros = Some(envelope.time.observed_micros);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        match &timeline.kind {
            TimelineEventKind::EncounterBoundary {
                state,
                encounter_id,
                ..
            } => {
                self.encounter_id = encounter_id.clone().or_else(|| self.encounter_id.clone());
                self.encounter_state = Some(encounter_state_name(*state).into());
            }
            TimelineEventKind::CombatBoundary { state, .. } => match state {
                CombatState::Started => self.begin_combat(envelope.time.observed_micros),
                CombatState::Ended => self.end_combat(envelope.time.observed_micros),
            },
            TimelineEventKind::Actor(actor) => {
                let accumulator = self.actor_mut(actor.actor.actor_id.0, actor.actor.entity_uuid.0);
                if actor.state != ActorState::Despawned {
                    accumulator.display_name = actor
                        .display_name
                        .clone()
                        .or_else(|| accumulator.display_name.clone());
                    accumulator.actor_kind = Some(actor_kind_name(actor.kind));
                    accumulator.class_id = actor.class_id.or(accumulator.class_id);
                    accumulator.level = actor.level.or(accumulator.level);
                }
            }
            TimelineEventKind::Cast(cast) => {
                let accumulator = self.actor_mut(cast.source.actor_id.0, cast.source.entity_uuid.0);
                accumulator.casts = accumulator.casts.saturating_add(1);
                let ability = accumulator.abilities.entry(cast.ability.0).or_default();
                ability.casts = ability.casts.saturating_add(1);
            }
            TimelineEventKind::Damage(damage) => {
                let during_combat = self.active_combat_started.is_some();
                let accumulator =
                    self.actor_mut(damage.source.actor_id.0, damage.source.entity_uuid.0);
                let reported = nonnegative(damage.amount);
                let effective = nonnegative(
                    damage
                        .actual_amount
                        .or(damage.hp_loss)
                        .unwrap_or(damage.amount),
                );
                accumulator.reported_damage = accumulator.reported_damage.saturating_add(reported);
                accumulator.effective_damage =
                    accumulator.effective_damage.saturating_add(effective);
                accumulator.hp_damage = accumulator
                    .hp_damage
                    .saturating_add(nonnegative(damage.hp_loss.unwrap_or(0)));
                accumulator.shield_damage = accumulator
                    .shield_damage
                    .saturating_add(nonnegative(damage.shield_loss.unwrap_or(0)));
                if during_combat {
                    accumulator.damage_during_combat =
                        accumulator.damage_during_combat.saturating_add(reported);
                }
                accumulator.hits = accumulator.hits.saturating_add(1);
                if damage.flags.critical == Some(true) {
                    accumulator.critical_hits = accumulator.critical_hits.saturating_add(1);
                }
                if let Some(ability_id) = damage.ability {
                    let ability = accumulator.abilities.entry(ability_id.0).or_default();
                    ability.hits = ability.hits.saturating_add(1);
                    ability.reported_damage = ability.reported_damage.saturating_add(reported);
                    ability.effective_damage = ability.effective_damage.saturating_add(effective);
                    if damage.flags.critical == Some(true) {
                        ability.critical_hits = ability.critical_hits.saturating_add(1);
                    }
                }
            }
            TimelineEventKind::Healing(healing) => {
                let accumulator =
                    self.actor_mut(healing.source.actor_id.0, healing.source.entity_uuid.0);
                let reported = nonnegative(healing.amount);
                let effective = nonnegative(healing.effective_amount.unwrap_or(healing.amount));
                accumulator.reported_healing =
                    accumulator.reported_healing.saturating_add(reported);
                accumulator.effective_healing =
                    accumulator.effective_healing.saturating_add(effective);
                accumulator.overheal = accumulator
                    .overheal
                    .saturating_add(nonnegative(healing.overheal.unwrap_or(0)));
                if let Some(ability_id) = healing.ability {
                    let ability = accumulator.abilities.entry(ability_id.0).or_default();
                    ability.reported_healing = ability.reported_healing.saturating_add(reported);
                    ability.effective_healing = ability.effective_healing.saturating_add(effective);
                }
            }
            TimelineEventKind::Shield(shield) => {
                let accumulator =
                    self.actor_mut(shield.source.actor_id.0, shield.source.entity_uuid.0);
                let amount = nonnegative(shield.amount);
                accumulator.shielding = accumulator.shielding.saturating_add(amount);
                let ability = accumulator.abilities.entry(shield.ability.0).or_default();
                ability.shielding = ability.shielding.saturating_add(amount);
            }
            TimelineEventKind::Life { actor, state } => {
                let accumulator = self.actor_mut(actor.actor_id.0, actor.entity_uuid.0);
                match state {
                    LifeState::Died => accumulator.deaths = accumulator.deaths.saturating_add(1),
                    LifeState::Revived => {
                        accumulator.revives = accumulator.revives.saturating_add(1)
                    }
                }
            }
            TimelineEventKind::Position(position) => {
                let accumulator =
                    self.actor_mut(position.actor.actor_id.0, position.actor.entity_uuid.0);
                accumulator.position_samples = accumulator.position_samples.saturating_add(1);
                if let Some((x, y, z)) = accumulator.last_position {
                    let dx = f64::from(position.x - x);
                    let dy = f64::from(position.y - y);
                    let dz = f64::from(position.z - z);
                    accumulator.path_distance += (dx * dx + dy * dy + dz * dz).sqrt();
                }
                accumulator.last_position = Some((position.x, position.y, position.z));
            }
            TimelineEventKind::DataGap(_) => {
                self.data_gap_count = self.data_gap_count.saturating_add(1);
            }
            TimelineEventKind::RunBoundary { .. }
            | TimelineEventKind::EntityAttributes(_)
            | TimelineEventKind::Cooldown(_)
            | TimelineEventKind::RecorderPause(_)
            | TimelineEventKind::Status(_) => {}
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
        if self.active_combat_started.is_some() {
            self.closed_at_log_end = true;
            self.end_combat(self.last_event_micros.unwrap_or_default());
        }
        output.snapshot(
            COMBAT_SNAPSHOT_SCHEMA_ID,
            COMBAT_SNAPSHOT_SCHEMA_VERSION,
            &self.snapshot()?,
        )
    }
}

fn nonnegative(value: i64) -> i64 {
    value.max(0)
}

fn encounter_state_name(state: EncounterState) -> &'static str {
    match state {
        EncounterState::Started => "started",
        EncounterState::Cleared => "cleared",
        EncounterState::Wiped => "wiped",
        EncounterState::Ended => "ended",
    }
}

fn actor_kind_name(kind: ActorKind) -> String {
    match kind {
        ActorKind::Player => "player".into(),
        ActorKind::Monster => "monster".into(),
        ActorKind::Npc => "npc".into(),
        ActorKind::SceneObject => "scene_object".into(),
        ActorKind::Zone => "zone".into(),
        ActorKind::Projectile => "projectile".into(),
        ActorKind::Pet => "pet".into(),
        ActorKind::TrainingDummy => "training_dummy".into(),
        ActorKind::Drop => "drop".into(),
        ActorKind::Field => "field".into(),
        ActorKind::Trap => "trap".into(),
        ActorKind::Collection => "collection".into(),
        ActorKind::StaticObject => "static_object".into(),
        ActorKind::Vehicle => "vehicle".into(),
        ActorKind::Toy => "toy".into(),
        ActorKind::Housing => "housing".into(),
        ActorKind::Unknown(value) => format!("unknown:{value}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;

    use rlogs_log_format::RlogLimits;
    use rlogs_plugin_runtime::{PluginOutput, PluginRunLimits, replay_rlog};

    use super::*;

    #[test]
    fn reference_replay_produces_exact_combat_and_movement_totals() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let report = replay_rlog(
            BufReader::new(File::open(fixture).unwrap()),
            CombatTimelinePlugin::new(),
            RlogLimits::default(),
            PluginRunLimits::default(),
        )
        .unwrap();
        assert_eq!(report.rlog.event_count, 13);
        assert_eq!(report.metrics.events_delivered, 13);
        let PluginOutput::Snapshot {
            schema_id,
            schema_version,
            payload,
        } = &report.outputs[0]
        else {
            panic!("expected combat snapshot");
        };
        assert_eq!(schema_id, COMBAT_SNAPSHOT_SCHEMA_ID);
        assert_eq!(*schema_version, COMBAT_SNAPSHOT_SCHEMA_VERSION);
        let snapshot: CombatTimelineSnapshot = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(snapshot.active_combat_micros, 10_000_000);
        assert_eq!(snapshot.combat_window_count, 1);
        assert_eq!(snapshot.encounter_state.as_deref(), Some("cleared"));
        assert!(!snapshot.closed_at_log_end);

        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == 1)
            .unwrap();
        assert_eq!(player.reported_damage, 20_000);
        assert_eq!(player.effective_damage, 20_000);
        assert_eq!(player.damage_during_combat, 20_000);
        assert_eq!(player.dps, 2_000.0);
        assert_eq!(player.reported_healing, 3_000);
        assert_eq!(player.effective_healing, 2_000);
        assert_eq!(player.overheal, 1_000);
        assert_eq!(player.casts, 1);
        assert_eq!(player.hits, 2);
        assert_eq!(player.critical_hits, 1);
        assert_eq!(player.position_samples, 2);
        assert_eq!(player.path_distance, 5.0);

        let boss = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == 2)
            .unwrap();
        assert_eq!(boss.deaths, 1);
    }
}
