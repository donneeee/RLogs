use std::collections::{BTreeMap, BTreeSet};

use rlogs_events::{
    ActorKind, ActorState, BoundaryReason, CanonicalEvent, CombatState, DungeonEvent,
    DungeonEventKind, EncounterState, EventEnvelope, EventProvenance, EvidenceSource, RunState,
    TimelineEventKind, WorldContext,
};
use thiserror::Error;

use crate::{
    ActivityKind, CombatWindowSummary, CompletedObjectiveAction, EncounterKind, EncounterSummary,
    EncounterTerminalState, LeaderboardPartitionKey, ManualPauseSummary,
    RUN_ANALYSIS_SCHEMA_VERSION, RaidRouteKind, RunAnalysis, RunEvidenceFinding, RunIdentity,
    RunSegmentKind, RunSegmentSummary, RunSubmissionDisposition, RunTerminalState, RunTiming,
    SceneRunRule,
};

const MAXIMUM_RULE_ACTORS: usize = 65_536;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunEventSequencePolicy {
    #[default]
    Contiguous,
    /// Used by a replay plug-in that receives a topic-filtered view of an
    /// already verified canonical log.
    MonotonicFiltered,
}

#[derive(Debug, Clone, Default)]
pub struct RunReducerConfig {
    pub encounter_ruleset_id: Option<String>,
    pub encounter_ruleset_version: Option<u32>,
    pub activity_kind: ActivityKind,
    pub activity_id: Option<String>,
    pub route_id: Option<String>,
    pub raid_route_kind: Option<RaidRouteKind>,
    pub partition: Option<LeaderboardPartitionKey>,
    pub sequence_policy: RunEventSequencePolicy,
    /// Encounter IDs are classified by a versioned game-owned ruleset.
    pub encounter_kinds: BTreeMap<String, EncounterKind>,
    /// Segment changes are explicit rules. Actor or monster spawns never
    /// change the current segment, which keeps boss adds inside the boss log.
    pub encounter_segments: BTreeMap<String, RunSegmentKind>,
    /// Exact-build game rules keyed by observed scene ID. Only the active
    /// scene's small rule is consulted; static game-data catalogs remain lazy.
    pub scene_rules: BTreeMap<i32, SceneRunRule>,
}

#[derive(Debug, Clone, Default)]
pub struct RunSessionReducer {
    config: RunReducerConfig,
    source_session_id: Option<String>,
    previous_sequence: Option<u64>,
    previous_observed_micros: Option<u64>,
    pending_identity: RunIdentity,
    active_scene_id: Option<i32>,
    rule_actors: BTreeMap<i64, RuleActor>,
    current: Option<RunAccumulator>,
    runs: Vec<RunAnalysis>,
}

impl RunSessionReducer {
    pub fn new(config: RunReducerConfig) -> Self {
        let pending_identity = RunIdentity {
            activity_kind: config.activity_kind,
            activity_id: config.activity_id.clone(),
            route_id: config.route_id.clone(),
            raid_route_kind: config.raid_route_kind,
            ..RunIdentity::default()
        };
        Self {
            config,
            pending_identity,
            ..Self::default()
        }
    }

    pub fn on_event(&mut self, envelope: &EventEnvelope) -> Result<(), RunReducerError> {
        self.validate_envelope(envelope)?;
        match &envelope.event {
            CanonicalEvent::WorldChanged(world) => {
                self.on_world_changed(envelope.time.observed_micros, world)
            }
            CanonicalEvent::Dungeon(event) => self.on_dungeon_event(
                &envelope.session_id,
                envelope.sequence,
                envelope.time.observed_micros,
                &envelope.provenance,
                event,
            )?,
            CanonicalEvent::Timeline(timeline) => self.on_timeline_event(
                &envelope.session_id,
                envelope.sequence,
                envelope.time.observed_micros,
                &envelope.provenance,
                &timeline.kind,
            )?,
            _ => {}
        }
        Ok(())
    }

    fn on_world_changed(&mut self, observed_micros: u64, world: &WorldContext) {
        let next_scene_id = world.scene_id.map(|scene| scene.0);
        let departed_active_run = self
            .current
            .as_ref()
            .and_then(|run| run.identity.scene_id)
            .zip(next_scene_id)
            .is_some_and(|(run_scene_id, next_scene_id)| run_scene_id != next_scene_id);
        if departed_active_run {
            // A world transition proves only that this local run view ended.
            // It does not prove that the player chose Exit: a completed floor,
            // reconnect, or capture gap can all be followed by the same scene
            // change. Reserve `Exited` for an explicit dungeon/run-boundary
            // packet and keep this fail-closed for local history.
            self.close_run(RunTerminalState::Ended, Some(observed_micros), false);
        }
        if self.active_scene_id != next_scene_id {
            self.rule_actors.clear();
        }
        self.active_scene_id = next_scene_id;
        self.pending_identity.scene_id = next_scene_id;

        let Some(rule) = next_scene_id.and_then(|scene_id| self.config.scene_rules.get(&scene_id))
        else {
            return;
        };
        self.pending_identity.activity_kind = rule.activity_kind;
        self.pending_identity.activity_id = Some(rule.activity_id.clone());
        self.pending_identity.activity_family_id = rule.activity_family_id.clone();
        self.pending_identity.difficulty_family = rule.difficulty_family.clone();
        self.pending_identity.route_id = rule.route_id.clone();
        self.pending_identity.raid_route_kind = rule.raid_route_kind;
        if let Some(encounter_id) = &rule.mobbing_encounter_id {
            if let Some(kind) = rule.encounter_kind(encounter_id) {
                self.config
                    .encounter_kinds
                    .insert(encounter_id.clone(), kind);
            }
            if let Some(segment) = rule.encounter_segment(encounter_id) {
                self.config
                    .encounter_segments
                    .insert(encounter_id.clone(), segment);
            }
        }
        if let Some(encounter_id) = &rule.boss_encounter_id {
            if let Some(kind) = rule.encounter_kind(encounter_id) {
                self.config
                    .encounter_kinds
                    .insert(encounter_id.clone(), kind);
            }
            if let Some(segment) = rule.encounter_segment(encounter_id) {
                self.config
                    .encounter_segments
                    .insert(encounter_id.clone(), segment);
            }
        }
        self.config.partition = rule.partition.clone();
    }

    /// Records host evidence for a user-requested recorder pause. The interval
    /// remains inside wall-clock run time and requires server review.
    ///
    /// A future canonical recorder-control event will call this same method
    /// during local and server replay.
    pub fn record_manual_pause(
        &mut self,
        started_micros: u64,
        resumed_micros: u64,
        reason: impl Into<String>,
    ) -> Result<(), RunReducerError> {
        if resumed_micros < started_micros {
            return Err(RunReducerError::PauseMovedBackward {
                started_micros,
                resumed_micros,
            });
        }
        let run = self
            .current
            .as_mut()
            .ok_or(RunReducerError::PauseOutsideRun)?;
        if started_micros < run.started_micros {
            return Err(RunReducerError::PauseBeforeRun {
                run_started_micros: run.started_micros,
                pause_started_micros: started_micros,
            });
        }
        run.manual_pauses.push(ManualPauseSummary {
            started_micros,
            resumed_micros,
            duration_micros: resumed_micros.saturating_sub(started_micros),
            reason: reason.into(),
        });
        run.observed_until_micros = run.observed_until_micros.max(resumed_micros);
        Ok(())
    }

    pub fn finish(mut self) -> Vec<RunAnalysis> {
        if let Some(run) = self.current.take() {
            self.runs
                .push(run.finish(RunTerminalState::Open, None, false, &self.config));
        }
        self.runs
    }

    fn validate_envelope(&mut self, envelope: &EventEnvelope) -> Result<(), RunReducerError> {
        if let Some(session_id) = &self.source_session_id {
            if session_id != &envelope.session_id {
                return Err(RunReducerError::SessionChanged {
                    expected: session_id.clone(),
                    actual: envelope.session_id.clone(),
                });
            }
        } else {
            self.source_session_id = Some(envelope.session_id.clone());
        }
        let expected_sequence = self
            .previous_sequence
            .map_or(1, |sequence| sequence.saturating_add(1));
        let sequence_is_invalid = match self.config.sequence_policy {
            RunEventSequencePolicy::Contiguous => envelope.sequence != expected_sequence,
            RunEventSequencePolicy::MonotonicFiltered => {
                self.previous_sequence
                    .is_some_and(|sequence| envelope.sequence <= sequence)
                    || envelope.sequence == 0
            }
        };
        if sequence_is_invalid {
            return Err(RunReducerError::UnexpectedSequence {
                expected: expected_sequence,
                actual: envelope.sequence,
            });
        }
        if let Some(previous) = self.previous_observed_micros
            && envelope.time.observed_micros < previous
        {
            return Err(RunReducerError::ObservedTimeMovedBackward {
                previous,
                next: envelope.time.observed_micros,
            });
        }
        self.previous_sequence = Some(envelope.sequence);
        self.previous_observed_micros = Some(envelope.time.observed_micros);
        if let Some(run) = &mut self.current {
            run.observed_until_micros = envelope.time.observed_micros;
        }
        Ok(())
    }

    fn on_dungeon_event(
        &mut self,
        session_id: &str,
        sequence: u64,
        observed_micros: u64,
        provenance: &EventProvenance,
        event: &DungeonEvent,
    ) -> Result<(), RunReducerError> {
        self.update_dungeon_identity(event);
        match event.kind {
            DungeonEventKind::Entered
            | DungeonEventKind::FlowUpdated
            | DungeonEventKind::ObjectiveRemoved => {}
            DungeonEventKind::ObjectiveUpdated => self.apply_objective_rule(observed_micros, event),
            DungeonEventKind::Started => {
                self.start_run(session_id, sequence, observed_micros, true)?
            }
            DungeonEventKind::BossEngaged => {
                if let Some(run) = &mut self.current {
                    run.switch_segment(RunSegmentKind::Boss, observed_micros, false);
                }
            }
            DungeonEventKind::BossDefeated => {
                if let Some(run) = &mut self.current {
                    run.end_encounter(EncounterTerminalState::Cleared, observed_micros, false);
                }
            }
            DungeonEventKind::Completed => {
                self.close_run(RunTerminalState::Completed, Some(observed_micros), true)
            }
            DungeonEventKind::Failed => {
                self.close_run(RunTerminalState::Failed, Some(observed_micros), true)
            }
            DungeonEventKind::Ended => {
                self.close_run(RunTerminalState::Ended, Some(observed_micros), true)
            }
            DungeonEventKind::Exited => {
                self.close_run(RunTerminalState::Exited, Some(observed_micros), true)
            }
        }
        self.observe_relevant_connection(provenance);
        Ok(())
    }

    fn apply_objective_rule(&mut self, observed_micros: u64, event: &DungeonEvent) {
        if event.objective_complete != Some(true) {
            return;
        }
        let Some(objective_id) = event
            .objective_id
            .or_else(|| event.objective_map_key.map(i64::from))
        else {
            return;
        };
        let Some(action) = self
            .active_scene_id
            .and_then(|scene_id| self.config.scene_rules.get(&scene_id))
            .and_then(|rule| rule.objective_rules.get(&objective_id))
            .map(|rule| rule.on_complete)
        else {
            return;
        };
        let Some(run) = &mut self.current else {
            return;
        };
        match action {
            CompletedObjectiveAction::ClearMobbing => {
                if run.active_segment_kind() == Some(RunSegmentKind::Mobbing) {
                    run.end_encounter(EncounterTerminalState::Cleared, observed_micros, false);
                    run.close_active_segment(observed_micros, false);
                }
                run.mobbing_cleared = true;
                run.boss_phase_armed = false;
            }
            CompletedObjectiveAction::EnterBossSegment => {
                // This gate ends the transition phase, but the leaderboard
                // game clock resumes only when the boss is actually engaged.
                // BossEngaged, an exact boss encounter, or reviewed boss
                // damage opens the segment at that later timestamp.
                if run.active_segment_kind() == Some(RunSegmentKind::Mobbing) {
                    run.close_active_segment(observed_micros, false);
                }
                run.mobbing_cleared = true;
                run.boss_phase_armed = true;
            }
            CompletedObjectiveAction::FinalObjective => {
                run.end_encounter(EncounterTerminalState::Cleared, observed_micros, false)
            }
            CompletedObjectiveAction::None => {}
        }
    }

    fn on_timeline_event(
        &mut self,
        session_id: &str,
        sequence: u64,
        observed_micros: u64,
        provenance: &EventProvenance,
        event: &TimelineEventKind,
    ) -> Result<(), RunReducerError> {
        match event {
            TimelineEventKind::RunBoundary { state, reason, .. } => {
                if *reason == BoundaryReason::Manual
                    && let Some(run) = &mut self.current
                {
                    run.manual_boundary = true;
                }
                let authoritative = *reason == BoundaryReason::AuthoritativePacket;
                match state {
                    RunState::Entered => {}
                    RunState::Started => {
                        self.start_run(session_id, sequence, observed_micros, authoritative)?;
                        self.observe_relevant_connection(provenance);
                    }
                    RunState::Completed => self.close_run(
                        RunTerminalState::Completed,
                        Some(observed_micros),
                        authoritative,
                    ),
                    RunState::Failed => self.close_run(
                        RunTerminalState::Failed,
                        Some(observed_micros),
                        authoritative,
                    ),
                    RunState::Ended => self.close_run(
                        RunTerminalState::Ended,
                        Some(observed_micros),
                        authoritative,
                    ),
                    RunState::Exited => self.close_run(
                        RunTerminalState::Exited,
                        Some(observed_micros),
                        authoritative,
                    ),
                }
            }
            TimelineEventKind::EncounterBoundary {
                state,
                encounter_id,
                reason,
            } => {
                self.observe_relevant_connection(provenance);
                let Some(run) = &mut self.current else {
                    return Ok(());
                };
                if *reason == BoundaryReason::Manual {
                    run.manual_boundary = true;
                }
                match state {
                    EncounterState::Started => {
                        run.start_encounter(encounter_id.clone(), observed_micros, &self.config)
                    }
                    EncounterState::Cleared => {
                        run.end_encounter(EncounterTerminalState::Cleared, observed_micros, false)
                    }
                    EncounterState::Wiped => {
                        run.end_encounter(EncounterTerminalState::Wiped, observed_micros, false)
                    }
                    EncounterState::Ended => {
                        run.end_encounter(EncounterTerminalState::Ended, observed_micros, false)
                    }
                }
            }
            TimelineEventKind::CombatBoundary { state, reason } => {
                self.observe_relevant_connection(provenance);
                let Some(run) = &mut self.current else {
                    return Ok(());
                };
                if *reason == BoundaryReason::Manual {
                    run.manual_boundary = true;
                }
                match state {
                    CombatState::Started => run.start_combat(observed_micros, &self.config),
                    CombatState::Ended => run.end_combat(observed_micros, false),
                }
            }
            TimelineEventKind::DataGap(gap) => {
                if let Some(run) = &mut self.current {
                    let relevant = gap.connection_id.is_none()
                        || gap
                            .connection_id
                            .is_some_and(|id| run.relevant_connection_ids.contains(&id));
                    if relevant {
                        run.data_gap_count = run.data_gap_count.saturating_add(1);
                    }
                }
            }
            TimelineEventKind::RecorderPause(pause) => {
                if pause.resumed_micros != observed_micros {
                    return Err(RunReducerError::PauseResumeTimeMismatch {
                        event_observed_micros: observed_micros,
                        resumed_micros: pause.resumed_micros,
                    });
                }
                if self.current.is_some() {
                    self.record_manual_pause(
                        pause.started_micros,
                        pause.resumed_micros,
                        "user_requested",
                    )?;
                }
            }
            TimelineEventKind::Actor(actor) => self.observe_rule_actor(actor),
            TimelineEventKind::Damage(damage) => {
                self.observe_relevant_connection(provenance);
                self.infer_hostile_damage(
                    observed_micros,
                    damage.source.entity_uuid.0,
                    damage.target.entity_uuid.0,
                );
            }
            TimelineEventKind::Life { actor, state } => {
                if *state == rlogs_events::LifeState::Died {
                    self.infer_actor_death(observed_micros, actor.entity_uuid.0);
                }
            }
            TimelineEventKind::EntityAttributes(_)
            | TimelineEventKind::TemporaryAttributes(_)
            | TimelineEventKind::Cast(_)
            | TimelineEventKind::Cooldown(_)
            | TimelineEventKind::Resource(_)
            | TimelineEventKind::Healing(_)
            | TimelineEventKind::Shield(_)
            | TimelineEventKind::Status(_)
            | TimelineEventKind::UnresolvedStatus(_)
            | TimelineEventKind::UnresolvedAction(_)
            | TimelineEventKind::Position(_) => {
                // Combatants and boss adds never define segment boundaries.
            }
        }
        Ok(())
    }

    fn observe_rule_actor(&mut self, event: &rlogs_events::ActorEvent) {
        let entity_uuid = event.actor.entity_uuid.0;
        if event.state == ActorState::Despawned {
            self.rule_actors.remove(&entity_uuid);
            return;
        }
        if !self.rule_actors.contains_key(&entity_uuid)
            && self.rule_actors.len() >= MAXIMUM_RULE_ACTORS
        {
            return;
        }
        let observed = self.rule_actors.entry(entity_uuid).or_insert(RuleActor {
            kind: event.kind,
            monster_id: None,
        });
        observed.kind = event.kind;
        if let Some(monster_id) = event.monster_id {
            observed.monster_id = Some(monster_id.0);
        }
    }

    fn observe_relevant_connection(&mut self, provenance: &EventProvenance) {
        let Some(run) = &mut self.current else {
            return;
        };
        if let Some(connection_id) = wire_connection_id(provenance) {
            run.relevant_connection_ids.insert(connection_id);
        }
    }

    fn infer_hostile_damage(&mut self, observed_micros: u64, source_uuid: i64, target_uuid: i64) {
        let Some(rule) = self.active_rule().cloned() else {
            return;
        };
        let source = self.rule_actors.get(&source_uuid).copied();
        let target = self.rule_actors.get(&target_uuid).copied();
        let boss_involved = [source, target]
            .into_iter()
            .flatten()
            .filter_map(|actor| actor.monster_id)
            .any(|monster_id| rule.boss_monster_ids.contains(&monster_id));
        let hostile_pair = [source, target]
            .into_iter()
            .flatten()
            .any(|actor| actor.kind == ActorKind::Player)
            && [source, target]
                .into_iter()
                .flatten()
                .any(|actor| actor.kind == ActorKind::Monster);
        if !boss_involved && !hostile_pair {
            return;
        }
        let Some(run) = &mut self.current else {
            return;
        };
        if boss_involved || run.boss_phase_armed {
            run.switch_segment(RunSegmentKind::Boss, observed_micros, false);
            run.ensure_encounter(
                rule.boss_encounter_id.clone(),
                observed_micros,
                &self.config,
            );
        } else if run.active_segment_kind() == Some(RunSegmentKind::Boss) {
            run.ensure_encounter(
                rule.boss_encounter_id.clone(),
                observed_micros,
                &self.config,
            );
        } else if run.active_segment_kind() != Some(RunSegmentKind::Boss) {
            run.ensure_encounter(
                rule.mobbing_encounter_id.clone(),
                observed_micros,
                &self.config,
            );
        }
        run.start_combat(observed_micros, &self.config);
    }

    fn infer_actor_death(&mut self, observed_micros: u64, entity_uuid: i64) {
        let Some(monster_id) = self
            .rule_actors
            .get(&entity_uuid)
            .and_then(|actor| actor.monster_id)
        else {
            return;
        };
        let boss_died = self
            .active_rule()
            .is_some_and(|rule| rule.boss_monster_ids.contains(&monster_id));
        if boss_died
            && let Some(run) = &mut self.current
            && run.active_segment_kind() == Some(RunSegmentKind::Boss)
        {
            run.end_encounter(EncounterTerminalState::Cleared, observed_micros, false);
        }
    }

    fn active_rule(&self) -> Option<&SceneRunRule> {
        self.active_scene_id
            .and_then(|scene_id| self.config.scene_rules.get(&scene_id))
    }

    fn update_dungeon_identity(&mut self, event: &DungeonEvent) {
        if self.pending_identity.activity_kind == ActivityKind::Unknown {
            self.pending_identity.activity_kind = ActivityKind::Dungeon;
        }
        if let Some(dungeon_id) = event.dungeon_id {
            self.pending_identity.observed_dungeon_id = Some(dungeon_id.0.to_string());
        }
        if let Some(instance_id) = &event.instance_id {
            self.pending_identity.instance_id = Some(instance_id.clone());
        }
        if let Some(difficulty_id) = event.difficulty_id {
            self.pending_identity.difficulty_id = Some(difficulty_id.to_string());
            self.pending_identity.difficulty_tier = self
                .active_rule()
                .and_then(|rule| rule.difficulty_tier_range)
                .and_then(|range| {
                    let tier = u32::try_from(difficulty_id).ok()?;
                    (tier >= range.minimum && tier <= range.maximum).then_some(tier)
                });
        }
        if let Some(run) = &mut self.current {
            run.identity = merge_identity(run.identity.clone(), &self.pending_identity);
        }
    }

    fn start_run(
        &mut self,
        session_id: &str,
        sequence: u64,
        observed_micros: u64,
        authoritative: bool,
    ) -> Result<(), RunReducerError> {
        if let Some(current) = &mut self.current {
            let same_instance = self.pending_identity.instance_id.is_none()
                || current.identity.instance_id == self.pending_identity.instance_id;
            if same_instance {
                current.authoritative_start |= authoritative;
                current.identity = merge_identity(current.identity.clone(), &self.pending_identity);
                return Ok(());
            }
        }
        if let Some(previous) = self.current.take() {
            self.runs.push(previous.finish(
                RunTerminalState::Superseded,
                None,
                false,
                &self.config,
            ));
        }
        self.current = Some(RunAccumulator::new(
            session_id.to_owned(),
            sequence,
            observed_micros,
            authoritative,
            self.pending_identity.clone(),
            self.config.encounter_ruleset_id.clone(),
            self.config.encounter_ruleset_version,
        )?);
        Ok(())
    }

    fn close_run(
        &mut self,
        state: RunTerminalState,
        ended_micros: Option<u64>,
        authoritative: bool,
    ) {
        if let Some(run) = self.current.take() {
            self.runs
                .push(run.finish(state, ended_micros, authoritative, &self.config));
        }
    }
}

#[derive(Debug, Clone)]
struct RunAccumulator {
    source_session_id: String,
    identity: RunIdentity,
    started_micros: u64,
    observed_until_micros: u64,
    authoritative_start: bool,
    encounter_ruleset_id: Option<String>,
    encounter_ruleset_version: Option<u32>,
    segments: Vec<RunSegmentAccumulator>,
    active_segment: Option<usize>,
    encounters: Vec<EncounterSummary>,
    active_encounter: Option<EncounterAccumulator>,
    manual_pauses: Vec<ManualPauseSummary>,
    data_gap_count: u64,
    relevant_connection_ids: BTreeSet<u64>,
    manual_boundary: bool,
    mobbing_cleared: bool,
    boss_phase_armed: bool,
}

impl RunAccumulator {
    fn new(
        source_session_id: String,
        _: u64,
        started_micros: u64,
        authoritative_start: bool,
        identity: RunIdentity,
        encounter_ruleset_id: Option<String>,
        encounter_ruleset_version: Option<u32>,
    ) -> Result<Self, RunReducerError> {
        let initial_segment = match identity.activity_kind {
            ActivityKind::Dungeon => RunSegmentKind::Mobbing,
            ActivityKind::Raid | ActivityKind::Unknown => RunSegmentKind::Unknown,
        };
        let mut run = Self {
            source_session_id,
            identity,
            started_micros,
            observed_until_micros: started_micros,
            authoritative_start,
            encounter_ruleset_id,
            encounter_ruleset_version,
            segments: Vec::new(),
            active_segment: None,
            encounters: Vec::new(),
            active_encounter: None,
            manual_pauses: Vec::new(),
            data_gap_count: 0,
            relevant_connection_ids: BTreeSet::new(),
            manual_boundary: false,
            mobbing_cleared: false,
            boss_phase_armed: false,
        };
        run.switch_segment(initial_segment, started_micros, false);
        Ok(run)
    }

    fn switch_segment(&mut self, kind: RunSegmentKind, observed_micros: u64, at_run_end: bool) {
        if self
            .active_segment
            .is_some_and(|index| self.segments[index].kind == kind)
        {
            return;
        }
        self.close_active_segment(observed_micros, at_run_end);
        let index = self.segments.len();
        self.segments
            .push(RunSegmentAccumulator::new(index, kind, observed_micros));
        self.active_segment = Some(index);
        if matches!(
            kind,
            RunSegmentKind::Boss | RunSegmentKind::RaidBoss | RunSegmentKind::Gauntlet
        ) {
            self.mobbing_cleared = false;
            self.boss_phase_armed = false;
        }
    }

    fn close_active_segment(&mut self, observed_micros: u64, at_run_end: bool) {
        if self.active_encounter.is_some() {
            self.end_encounter(EncounterTerminalState::Ended, observed_micros, at_run_end);
        }
        if let Some(index) = self.active_segment.take() {
            self.segments[index].ended_micros = Some(observed_micros);
            self.segments[index].closed_at_run_end = at_run_end;
        }
    }

    fn active_segment_kind(&self) -> Option<RunSegmentKind> {
        self.active_segment.map(|index| self.segments[index].kind)
    }

    fn ensure_encounter(
        &mut self,
        encounter_id: Option<String>,
        observed_micros: u64,
        config: &RunReducerConfig,
    ) {
        if self
            .active_encounter
            .as_ref()
            .is_some_and(|encounter| encounter.encounter_id == encounter_id)
        {
            return;
        }
        self.start_encounter(encounter_id, observed_micros, config);
    }

    fn start_encounter(
        &mut self,
        encounter_id: Option<String>,
        observed_micros: u64,
        config: &RunReducerConfig,
    ) {
        let configured_segment = encounter_id
            .as_ref()
            .and_then(|id| config.encounter_segments.get(id))
            .copied();
        if self.mobbing_cleared
            && !matches!(
                configured_segment,
                Some(RunSegmentKind::Boss | RunSegmentKind::RaidBoss | RunSegmentKind::Gauntlet)
            )
        {
            return;
        }
        if self.active_encounter.is_some() {
            self.end_encounter(EncounterTerminalState::Ended, observed_micros, false);
        }
        if let Some(segment) = configured_segment {
            self.switch_segment(segment, observed_micros, false);
        }
        let segment_index = self.active_segment.unwrap_or_default() as u32;
        let kind = encounter_id
            .as_ref()
            .and_then(|id| config.encounter_kinds.get(id))
            .copied()
            .unwrap_or_else(|| {
                self.active_segment
                    .map(|index| self.segments[index].kind.into())
                    .unwrap_or(EncounterKind::Unknown)
            });
        let attempt_number = self
            .encounters
            .iter()
            .filter(|encounter| {
                encounter.segment_index == segment_index && encounter.encounter_id == encounter_id
            })
            .count()
            .saturating_add(1) as u32;
        self.active_encounter = Some(EncounterAccumulator::new(
            self.encounters.len() as u32,
            encounter_id,
            kind,
            segment_index,
            attempt_number,
            observed_micros,
        ));
    }

    fn start_combat(&mut self, observed_micros: u64, config: &RunReducerConfig) {
        if self.mobbing_cleared && self.active_encounter.is_none() {
            return;
        }
        if self.active_encounter.is_none() {
            self.start_encounter(None, observed_micros, config);
        }
        if let Some(encounter) = &mut self.active_encounter {
            encounter.start_combat(observed_micros);
        }
    }

    fn end_combat(&mut self, observed_micros: u64, at_boundary: bool) {
        if let Some(encounter) = &mut self.active_encounter {
            encounter.end_combat(observed_micros, at_boundary);
        }
    }

    fn end_encounter(
        &mut self,
        terminal_state: EncounterTerminalState,
        observed_micros: u64,
        at_run_end: bool,
    ) {
        let Some(mut encounter) = self.active_encounter.take() else {
            return;
        };
        encounter.end_combat(observed_micros, true);
        let summary = encounter.finish(terminal_state, observed_micros, at_run_end);
        if let Some(segment) = self.segments.get_mut(summary.segment_index as usize) {
            segment.encounter_indices.push(summary.index);
        }
        self.encounters.push(summary);
    }

    fn finish(
        mut self,
        terminal_state: RunTerminalState,
        ended_micros: Option<u64>,
        authoritative_completion: bool,
        config: &RunReducerConfig,
    ) -> RunAnalysis {
        let boundary_micros = ended_micros.unwrap_or(self.observed_until_micros);
        let had_open_encounter = self.active_encounter.is_some();
        let had_open_combat = self
            .active_encounter
            .as_ref()
            .is_some_and(|encounter| encounter.active_combat_started.is_some());
        self.end_encounter(
            EncounterTerminalState::Ended,
            boundary_micros,
            ended_micros.is_some(),
        );
        if let Some(index) = self.active_segment.take() {
            self.segments[index].ended_micros = Some(boundary_micros);
            self.segments[index].closed_at_run_end = ended_micros.is_some();
        }
        let active_combat_micros = self
            .encounters
            .iter()
            .map(|encounter| encounter.active_combat_micros)
            .sum::<u64>();
        let manual_pause_micros = self
            .manual_pauses
            .iter()
            .map(|pause| pause.duration_micros)
            .sum::<u64>();
        let wall_time_micros = ended_micros.map(|ended| ended.saturating_sub(self.started_micros));
        let noncombat_micros =
            wall_time_micros.map(|wall| wall.saturating_sub(active_combat_micros));
        let segments = self
            .segments
            .into_iter()
            .map(|segment| segment.finish(&self.encounters))
            .collect::<Vec<_>>();
        let mut findings = Vec::new();
        if self.data_gap_count != 0 {
            findings.push(RunEvidenceFinding::DataGaps {
                count: self.data_gap_count,
            });
        }
        if !self.manual_pauses.is_empty() {
            findings.push(RunEvidenceFinding::ManualRecorderPause {
                count: self.manual_pauses.len() as u32,
                duration_micros: manual_pause_micros,
            });
        }
        if self.manual_boundary {
            findings.push(RunEvidenceFinding::ManualBoundary);
        }
        if !self.authoritative_start {
            findings.push(RunEvidenceFinding::StartNotAuthoritative);
        }
        if terminal_state == RunTerminalState::Completed && !authoritative_completion {
            findings.push(RunEvidenceFinding::CompletionNotAuthoritative);
        }
        if config.partition.is_none() {
            findings.push(RunEvidenceFinding::LeaderboardPartitionUnresolved);
        }
        if had_open_combat {
            findings.push(RunEvidenceFinding::CombatClosedAtRunEnd);
        }
        if had_open_encounter {
            findings.push(RunEvidenceFinding::EncounterClosedAtRunEnd);
        }
        let submission_disposition = if terminal_state != RunTerminalState::Completed {
            RunSubmissionDisposition::NotCompleted
        } else if !self.authoritative_start || !authoritative_completion || !findings.is_empty() {
            RunSubmissionDisposition::CompletedNeedsReview
        } else {
            RunSubmissionDisposition::RankCandidate
        };
        RunAnalysis {
            schema_version: RUN_ANALYSIS_SCHEMA_VERSION,
            source_session_id: self.source_session_id,
            encounter_ruleset_id: self.encounter_ruleset_id,
            encounter_ruleset_version: self.encounter_ruleset_version,
            identity: self.identity,
            partition: config.partition.clone(),
            terminal_state,
            authoritative_start: self.authoritative_start,
            authoritative_completion,
            timing: RunTiming {
                started_micros: self.started_micros,
                ended_micros,
                observed_until_micros: self.observed_until_micros.max(boundary_micros),
                wall_time_micros,
                active_combat_micros,
                noncombat_micros,
                manual_pause_micros,
            },
            segments,
            encounters: self.encounters,
            manual_pauses: self.manual_pauses,
            data_gap_count: self.data_gap_count,
            findings,
            submission_disposition,
        }
    }
}

#[derive(Debug, Clone)]
struct RunSegmentAccumulator {
    index: usize,
    kind: RunSegmentKind,
    started_micros: u64,
    ended_micros: Option<u64>,
    encounter_indices: Vec<u32>,
    closed_at_run_end: bool,
}

impl RunSegmentAccumulator {
    fn new(index: usize, kind: RunSegmentKind, started_micros: u64) -> Self {
        Self {
            index,
            kind,
            started_micros,
            ended_micros: None,
            encounter_indices: Vec::new(),
            closed_at_run_end: false,
        }
    }

    fn finish(self, encounters: &[EncounterSummary]) -> RunSegmentSummary {
        let ended_micros = self.ended_micros.unwrap_or(self.started_micros);
        let segment_encounters = self
            .encounter_indices
            .iter()
            .filter_map(|index| encounters.get(*index as usize))
            .collect::<Vec<_>>();
        let active_combat_micros = segment_encounters
            .iter()
            .map(|encounter| encounter.active_combat_micros)
            .sum();
        let total_attempt_wall_time_micros = segment_encounters
            .iter()
            .map(|encounter| encounter.wall_time_micros)
            .sum();
        let first_attempt_started_micros = segment_encounters
            .first()
            .map(|encounter| encounter.started_micros);
        let final_attempt_ended_micros = segment_encounters
            .last()
            .map(|encounter| encounter.ended_micros);
        let elapsed_trying_micros = first_attempt_started_micros
            .zip(final_attempt_ended_micros)
            .map_or(0, |(started, ended)| ended.saturating_sub(started));
        let successful_attempts = segment_encounters
            .iter()
            .copied()
            .filter(|encounter| encounter.is_successful_attempt)
            .collect::<Vec<_>>();
        let successful_attempt_indices = successful_attempts
            .iter()
            .map(|encounter| encounter.index)
            .collect::<Vec<_>>();
        let successful_attempt_wall_time_micros = successful_attempts
            .iter()
            .map(|encounter| encounter.wall_time_micros)
            .sum();
        let successful_attempt_active_combat_micros = successful_attempts
            .iter()
            .map(|encounter| encounter.active_combat_micros)
            .sum();
        let winning_attempt = successful_attempts.last().copied();
        RunSegmentSummary {
            index: self.index as u32,
            kind: self.kind,
            started_micros: self.started_micros,
            ended_micros,
            wall_time_micros: ended_micros.saturating_sub(self.started_micros),
            active_combat_micros,
            attempt_count: segment_encounters.len() as u32,
            retry_count: segment_encounters
                .iter()
                .filter(|encounter| encounter.is_retry)
                .count() as u32,
            total_attempt_wall_time_micros,
            total_attempt_active_combat_micros: active_combat_micros,
            elapsed_trying_micros,
            between_attempts_micros: elapsed_trying_micros
                .saturating_sub(total_attempt_wall_time_micros),
            successful_attempt_indices,
            successful_attempt_wall_time_micros,
            successful_attempt_active_combat_micros,
            winning_attempt_index: winning_attempt.map(|encounter| encounter.index),
            winning_attempt_wall_time_micros: winning_attempt
                .map(|encounter| encounter.wall_time_micros),
            winning_attempt_active_combat_micros: winning_attempt
                .map(|encounter| encounter.active_combat_micros),
            encounter_indices: self.encounter_indices,
            closed_at_run_end: self.closed_at_run_end,
        }
    }
}

#[derive(Debug, Clone)]
struct EncounterAccumulator {
    index: u32,
    encounter_id: Option<String>,
    kind: EncounterKind,
    segment_index: u32,
    attempt_number: u32,
    started_micros: u64,
    active_combat_started: Option<u64>,
    combat_windows: Vec<CombatWindowSummary>,
}

impl EncounterAccumulator {
    fn new(
        index: u32,
        encounter_id: Option<String>,
        kind: EncounterKind,
        segment_index: u32,
        attempt_number: u32,
        started_micros: u64,
    ) -> Self {
        Self {
            index,
            encounter_id,
            kind,
            segment_index,
            attempt_number,
            started_micros,
            active_combat_started: None,
            combat_windows: Vec::new(),
        }
    }

    fn start_combat(&mut self, observed_micros: u64) {
        self.active_combat_started.get_or_insert(observed_micros);
    }

    fn end_combat(&mut self, observed_micros: u64, at_boundary: bool) {
        if let Some(started_micros) = self.active_combat_started.take() {
            self.combat_windows.push(CombatWindowSummary {
                started_micros,
                ended_micros: observed_micros,
                duration_micros: observed_micros.saturating_sub(started_micros),
                closed_at_boundary: at_boundary,
            });
        }
    }

    fn finish(
        self,
        terminal_state: EncounterTerminalState,
        ended_micros: u64,
        closed_at_run_end: bool,
    ) -> EncounterSummary {
        let active_combat_micros = self
            .combat_windows
            .iter()
            .map(|window| window.duration_micros)
            .sum();
        EncounterSummary {
            index: self.index,
            encounter_id: self.encounter_id,
            kind: self.kind,
            segment_index: self.segment_index,
            attempt_number: self.attempt_number,
            is_retry: self.attempt_number > 1,
            is_successful_attempt: terminal_state == EncounterTerminalState::Cleared,
            terminal_state,
            started_micros: self.started_micros,
            ended_micros,
            wall_time_micros: ended_micros.saturating_sub(self.started_micros),
            active_combat_micros,
            combat_windows: self.combat_windows,
            closed_at_run_end,
        }
    }
}

fn merge_identity(mut current: RunIdentity, observed: &RunIdentity) -> RunIdentity {
    current.activity_kind = observed.activity_kind;
    current.activity_id = observed.activity_id.clone().or(current.activity_id);
    current.activity_family_id = observed
        .activity_family_id
        .clone()
        .or(current.activity_family_id);
    current.scene_id = observed.scene_id.or(current.scene_id);
    current.observed_dungeon_id = observed
        .observed_dungeon_id
        .clone()
        .or(current.observed_dungeon_id);
    current.instance_id = observed.instance_id.clone().or(current.instance_id);
    current.difficulty_family = observed
        .difficulty_family
        .clone()
        .or(current.difficulty_family);
    current.difficulty_id = observed.difficulty_id.clone().or(current.difficulty_id);
    current.difficulty_tier = observed.difficulty_tier.or(current.difficulty_tier);
    current.route_id = observed.route_id.clone().or(current.route_id);
    current.raid_route_kind = observed.raid_route_kind.or(current.raid_route_kind);
    current
}

fn wire_connection_id(provenance: &EventProvenance) -> Option<u64> {
    match &provenance.source {
        EvidenceSource::Wire { connection_id, .. } => Some(*connection_id),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct RuleActor {
    kind: ActorKind,
    monster_id: Option<i64>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RunReducerError {
    #[error("run reducer session changed from {expected} to {actual}")]
    SessionChanged { expected: String, actual: String },
    #[error("run reducer expected event sequence {expected}, got {actual}")]
    UnexpectedSequence { expected: u64, actual: u64 },
    #[error("run reducer time moved backward from {previous}us to {next}us")]
    ObservedTimeMovedBackward { previous: u64, next: u64 },
    #[error(
        "manual recorder pause ended at {resumed_micros}us before it started at {started_micros}us"
    )]
    PauseMovedBackward {
        started_micros: u64,
        resumed_micros: u64,
    },
    #[error(
        "manual recorder pause resumed at {resumed_micros}us but its event was observed at {event_observed_micros}us"
    )]
    PauseResumeTimeMismatch {
        event_observed_micros: u64,
        resumed_micros: u64,
    },
    #[error("manual recorder pause was recorded outside an active run")]
    PauseOutsideRun,
    #[error(
        "manual recorder pause started at {pause_started_micros}us before the run started at {run_started_micros}us"
    )]
    PauseBeforeRun {
        run_started_micros: u64,
        pause_started_micros: u64,
    },
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        ActorEvent, ActorId, ActorKind, ActorState, CanonicalEventDraft, CanonicalEventDraftKind,
        DamageEvent, DamageFlags, DataGapEvent, DataGapKind, DungeonId, EntityRef, EntityUuid,
        EventEnvelopeFactory, EventProvenance, EventSensitivity, EventTime, MonsterId,
        RegionContext, RegionIdentity, SceneId, TimelineEventKind, WorldContext,
    };

    use super::*;

    fn factory() -> EventEnvelopeFactory {
        EventEnvelopeFactory::new(
            "session-1",
            RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: Some("asteria".into()),
                },
                client_build: "test-build".into(),
                protocol_pack_digest: "sha256:test-pack".into(),
                evidence: Vec::new(),
            },
        )
    }

    fn emit(
        reducer: &mut RunSessionReducer,
        factory: &mut EventEnvelopeFactory,
        observed_micros: u64,
        kind: CanonicalEventDraftKind,
    ) {
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(observed_micros, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind,
            })
            .unwrap();
        reducer.on_event(&envelope).unwrap();
    }

    fn dungeon(kind: DungeonEventKind, instance_id: &str) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Dungeon(DungeonEvent {
            kind,
            dungeon_id: Some(DungeonId(7001)),
            instance_id: Some(instance_id.into()),
            difficulty_id: Some(3),
            objective_map_key: None,
            objective_id: None,
            objective_value: None,
            objective_complete: None,
            objective_catalog: None,
            flow: None,
        })
    }

    fn encounter(state: EncounterState, encounter_id: &str) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::EncounterBoundary {
            state,
            encounter_id: Some(encounter_id.into()),
            reason: match state {
                EncounterState::Wiped => BoundaryReason::Wipe,
                EncounterState::Cleared => BoundaryReason::Completion,
                EncounterState::Started | EncounterState::Ended => {
                    BoundaryReason::AuthoritativePacket
                }
            },
        })
    }

    fn combat(state: CombatState) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::CombatBoundary {
            state,
            reason: BoundaryReason::HostileAction,
        })
    }

    fn actor_spawn() -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
            actor: EntityRef {
                actor_id: ActorId(50),
                entity_uuid: EntityUuid(500),
            },
            state: ActorState::Spawned,
            entity_type_id: 2,
            kind: ActorKind::Monster,
            character_id: None,
            monster_id: None,
            display_name: None,
            class_id: None,
            specialization_id: None,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }))
    }

    fn actor(
        actor_id: u64,
        entity_uuid: i64,
        kind: ActorKind,
        monster_id: Option<i64>,
    ) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
            actor: EntityRef {
                actor_id: ActorId(actor_id),
                entity_uuid: EntityUuid(entity_uuid),
            },
            state: ActorState::Spawned,
            entity_type_id: 1,
            kind,
            character_id: None,
            monster_id: monster_id.map(MonsterId),
            display_name: None,
            class_id: None,
            specialization_id: None,
            level: Some(60),
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }))
    }

    fn damage(source: EntityRef, target: EntityRef) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
            source,
            direct_source: None,
            target,
            ability: None,
            amount: 100,
            actual_amount: None,
            hp_loss: Some(100),
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: Default::default(),
        }))
    }

    fn dungeon_config() -> RunReducerConfig {
        RunReducerConfig {
            activity_kind: ActivityKind::Dungeon,
            activity_id: Some("dungeon-7001".into()),
            partition: Some(LeaderboardPartitionKey {
                season_id: "season-3".into(),
                activity_id: "dungeon-7001".into(),
                difficulty_id: "3".into(),
                route_id: None,
                encounter_ruleset_id: "bpsr-dungeons".into(),
                encounter_ruleset_version: 1,
            }),
            ..RunReducerConfig::default()
        }
    }

    #[test]
    fn completed_dungeon_is_one_run_with_mobbing_and_boss_projections() {
        let mut reducer = RunSessionReducer::new(dungeon_config());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            encounter(EncounterState::Started, "trash"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            combat(CombatState::Started),
        );
        emit(
            &mut reducer,
            &mut events,
            4_000_000,
            combat(CombatState::Ended),
        );
        emit(
            &mut reducer,
            &mut events,
            4_000_000,
            encounter(EncounterState::Cleared, "trash"),
        );
        // Four seconds without combat represent traversal or a cutscene.
        emit(
            &mut reducer,
            &mut events,
            8_000_000,
            dungeon(DungeonEventKind::BossEngaged, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            8_000_000,
            encounter(EncounterState::Started, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            8_000_000,
            combat(CombatState::Started),
        );
        // A boss add spawning must not create a mobbing segment or pull.
        emit(&mut reducer, &mut events, 9_000_000, actor_spawn());
        emit(
            &mut reducer,
            &mut events,
            12_000_000,
            combat(CombatState::Ended),
        );
        emit(
            &mut reducer,
            &mut events,
            12_000_000,
            encounter(EncounterState::Cleared, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            12_000_000,
            dungeon(DungeonEventKind::BossDefeated, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            13_000_000,
            dungeon(DungeonEventKind::Completed, "run-1"),
        );

        let runs = reducer.finish();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.terminal_state, RunTerminalState::Completed);
        assert_eq!(
            run.submission_disposition,
            RunSubmissionDisposition::RankCandidate
        );
        assert_eq!(run.timing.wall_time_micros, Some(12_000_000));
        assert_eq!(run.timing.active_combat_micros, 6_000_000);
        assert_eq!(run.timing.noncombat_micros, Some(6_000_000));
        assert_eq!(run.segments.len(), 2);
        assert_eq!(run.segments[0].kind, RunSegmentKind::Mobbing);
        assert_eq!(run.segments[1].kind, RunSegmentKind::Boss);
        assert_eq!(run.segments[0].encounter_indices, vec![0]);
        assert_eq!(run.segments[1].encounter_indices, vec![1]);
        assert_eq!(run.segments[1].attempt_count, 1);
    }

    #[test]
    fn observed_dungeon_without_partition_is_completed_but_not_rankable() {
        let mut reducer = RunSessionReducer::new(RunReducerConfig::default());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-unmapped"),
        );
        emit(
            &mut reducer,
            &mut events,
            9_000_000,
            dungeon(DungeonEventKind::Completed, "run-unmapped"),
        );

        let runs = reducer.finish();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.identity.activity_kind, ActivityKind::Dungeon);
        assert_eq!(run.segments[0].kind, RunSegmentKind::Mobbing);
        assert_eq!(run.terminal_state, RunTerminalState::Completed);
        assert_eq!(run.partition, None);
        assert!(
            run.findings
                .contains(&RunEvidenceFinding::LeaderboardPartitionUnresolved)
        );
        assert_eq!(
            run.submission_disposition,
            RunSubmissionDisposition::CompletedNeedsReview
        );
    }

    #[test]
    fn world_departure_closes_an_open_run_without_claiming_an_exit() {
        let mut reducer = RunSessionReducer::new(RunReducerConfig::default());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            0,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: Some(SceneId(6_525)),
                map_id: Some(6_525),
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        );
        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "abandoned-run"),
        );
        emit(
            &mut reducer,
            &mut events,
            10_000_000,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: Some(SceneId(8)),
                map_id: Some(8),
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        );

        let runs = reducer.finish();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].identity.scene_id, Some(6_525));
        assert_eq!(runs[0].terminal_state, RunTerminalState::Ended);
        assert_eq!(runs[0].timing.ended_micros, Some(10_000_000));
        assert!(!runs[0].authoritative_completion);
        assert_ne!(
            runs[0].submission_disposition,
            RunSubmissionDisposition::RankCandidate
        );
    }

    #[test]
    fn exact_build_rules_derive_mobbing_and_boss_windows_without_actor_heuristics() {
        let scene_rule = SceneRunRule {
            scene_id: 1631,
            runtime_enabled: true,
            activity_kind: ActivityKind::Dungeon,
            activity_id: "scene.1631".into(),
            activity_family_id: Some("tina-mindrealm".into()),
            activity_localization_key: Some("scene.1631.name".into()),
            difficulty_family: Some("normal".into()),
            difficulty_localization_key: None,
            difficulty_tier_range: None,
            route_id: None,
            raid_route_kind: None,
            partition: None,
            candidate_dungeon_ids: [1_031, 1_631].into_iter().collect(),
            mobbing_encounter_id: Some("scene.1631.mobbing".into()),
            boss_encounter_id: Some("monster.33701".into()),
            boss_monster_ids: [33_701].into_iter().collect(),
            objective_rules: BTreeMap::from([
                (
                    100_178,
                    crate::DungeonObjectiveRule {
                        role: crate::DungeonObjectiveRole::MobbingCompletion,
                        on_complete: CompletedObjectiveAction::ClearMobbing,
                    },
                ),
                (
                    100_176,
                    crate::DungeonObjectiveRule {
                        role: crate::DungeonObjectiveRole::BossPhaseGate,
                        on_complete: CompletedObjectiveAction::EnterBossSegment,
                    },
                ),
                (
                    100_164,
                    crate::DungeonObjectiveRule {
                        role: crate::DungeonObjectiveRole::RunCompletion,
                        on_complete: CompletedObjectiveAction::FinalObjective,
                    },
                ),
            ]),
            evidence: Vec::new(),
        };
        let mut reducer = RunSessionReducer::new(RunReducerConfig {
            encounter_ruleset_id: Some("fixture-rules".into()),
            encounter_ruleset_version: Some(1),
            scene_rules: BTreeMap::from([(1631, scene_rule)]),
            ..RunReducerConfig::default()
        });
        let mut events = factory();
        let player = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(100),
        };
        let mob = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(200),
        };
        let boss = EntityRef {
            actor_id: ActorId(3),
            entity_uuid: EntityUuid(300),
        };

        emit(
            &mut reducer,
            &mut events,
            0,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: Some(SceneId(1631)),
                map_id: None,
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        );
        emit(
            &mut reducer,
            &mut events,
            0,
            actor(1, 100, ActorKind::Player, None),
        );
        emit(
            &mut reducer,
            &mut events,
            0,
            actor(2, 200, ActorKind::Monster, None),
        );
        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "rule-run"),
        );
        emit(&mut reducer, &mut events, 2_000_000, damage(mob, player));
        let mut mobbing_complete = match dungeon(DungeonEventKind::ObjectiveUpdated, "rule-run") {
            CanonicalEventDraftKind::Dungeon(event) => event,
            _ => unreachable!(),
        };
        // Some current-build routes expose the stable objective only as the
        // objective-map key while leaving the nested objective ID absent.
        mobbing_complete.objective_map_key = Some(100_178);
        mobbing_complete.objective_id = None;
        mobbing_complete.objective_complete = Some(true);
        emit(
            &mut reducer,
            &mut events,
            4_000_000,
            CanonicalEventDraftKind::Dungeon(mobbing_complete),
        );
        let mut boss_gate = match dungeon(DungeonEventKind::ObjectiveUpdated, "rule-run") {
            CanonicalEventDraftKind::Dungeon(event) => event,
            _ => unreachable!(),
        };
        boss_gate.objective_id = Some(100_176);
        boss_gate.objective_complete = Some(true);
        emit(
            &mut reducer,
            &mut events,
            6_000_000,
            CanonicalEventDraftKind::Dungeon(boss_gate),
        );
        emit(
            &mut reducer,
            &mut events,
            6_000_000,
            actor(3, 300, ActorKind::Monster, None),
        );
        emit(&mut reducer, &mut events, 8_000_000, damage(player, boss));
        let mut final_objective = match dungeon(DungeonEventKind::ObjectiveUpdated, "rule-run") {
            CanonicalEventDraftKind::Dungeon(event) => event,
            _ => unreachable!(),
        };
        final_objective.objective_id = Some(100_164);
        final_objective.objective_complete = Some(true);
        emit(
            &mut reducer,
            &mut events,
            12_000_000,
            CanonicalEventDraftKind::Dungeon(final_objective),
        );
        emit(
            &mut reducer,
            &mut events,
            13_000_000,
            dungeon(DungeonEventKind::Completed, "rule-run"),
        );

        let run = &reducer.finish()[0];
        assert_eq!(run.identity.scene_id, Some(1631));
        assert_eq!(
            run.identity.activity_family_id.as_deref(),
            Some("tina-mindrealm")
        );
        assert_eq!(run.identity.difficulty_family.as_deref(), Some("normal"));
        assert_eq!(run.encounter_ruleset_id.as_deref(), Some("fixture-rules"));
        assert_eq!(run.segments.len(), 2);
        assert_eq!(run.segments[0].kind, RunSegmentKind::Mobbing);
        assert_eq!(run.segments[0].started_micros, 1_000_000);
        assert_eq!(run.segments[0].ended_micros, 4_000_000);
        assert_eq!(run.segments[1].kind, RunSegmentKind::Boss);
        assert_eq!(run.segments[1].started_micros, 8_000_000);
        assert_eq!(run.encounters.len(), 2);
        assert_eq!(
            run.encounters[0].encounter_id.as_deref(),
            Some("scene.1631.mobbing")
        );
        assert_eq!(
            run.encounters[0].terminal_state,
            EncounterTerminalState::Cleared
        );
        assert_eq!(run.encounters[0].active_combat_micros, 2_000_000);
        assert_eq!(
            run.encounters[1].encounter_id.as_deref(),
            Some("monster.33701")
        );
        assert_eq!(
            run.encounters[1].terminal_state,
            EncounterTerminalState::Cleared
        );
        assert_eq!(run.encounters[1].active_combat_micros, 4_000_000);
        assert_eq!(run.timing.active_combat_micros, 6_000_000);
    }

    #[test]
    fn boss_repull_reports_total_trying_and_winning_attempt_time() {
        let mut reducer = RunSessionReducer::new(dungeon_config());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            dungeon(DungeonEventKind::BossEngaged, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            encounter(EncounterState::Started, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            combat(CombatState::Started),
        );
        emit(
            &mut reducer,
            &mut events,
            5_000_000,
            combat(CombatState::Ended),
        );
        emit(
            &mut reducer,
            &mut events,
            5_000_000,
            encounter(EncounterState::Wiped, "boss"),
        );
        // Recovery and repositioning before the repull.
        emit(
            &mut reducer,
            &mut events,
            7_000_000,
            encounter(EncounterState::Started, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            7_000_000,
            combat(CombatState::Started),
        );
        emit(
            &mut reducer,
            &mut events,
            11_000_000,
            combat(CombatState::Ended),
        );
        emit(
            &mut reducer,
            &mut events,
            11_000_000,
            encounter(EncounterState::Cleared, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            12_000_000,
            dungeon(DungeonEventKind::Completed, "run-1"),
        );

        let run = &reducer.finish()[0];
        let boss = run
            .segments
            .iter()
            .find(|segment| segment.kind == RunSegmentKind::Boss)
            .unwrap();
        assert_eq!(run.timing.wall_time_micros, Some(11_000_000));
        assert_eq!(boss.attempt_count, 2);
        assert_eq!(boss.retry_count, 1);
        assert_eq!(boss.total_attempt_wall_time_micros, 7_000_000);
        assert_eq!(boss.elapsed_trying_micros, 9_000_000);
        assert_eq!(boss.between_attempts_micros, 2_000_000);
        assert_eq!(boss.successful_attempt_indices, vec![1]);
        assert_eq!(boss.winning_attempt_index, Some(1));
        assert_eq!(boss.winning_attempt_wall_time_micros, Some(4_000_000));
        assert_eq!(boss.winning_attempt_active_combat_micros, Some(4_000_000));
        assert_eq!(run.encounters[0].attempt_number, 1);
        assert!(!run.encounters[0].is_retry);
        assert!(!run.encounters[0].is_successful_attempt);
        assert_eq!(run.encounters[1].attempt_number, 2);
        assert!(run.encounters[1].is_retry);
        assert!(run.encounters[1].is_successful_attempt);
    }

    #[test]
    fn exiting_and_reentering_creates_a_new_run_not_a_retry() {
        let mut reducer = RunSessionReducer::new(dungeon_config());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            encounter(EncounterState::Started, "boss"),
        );
        emit(
            &mut reducer,
            &mut events,
            5_000_000,
            dungeon(DungeonEventKind::Exited, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            10_000_000,
            dungeon(DungeonEventKind::Started, "run-2"),
        );
        emit(
            &mut reducer,
            &mut events,
            20_000_000,
            dungeon(DungeonEventKind::Completed, "run-2"),
        );

        let runs = reducer.finish();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].terminal_state, RunTerminalState::Exited);
        assert_eq!(
            runs[0].submission_disposition,
            RunSubmissionDisposition::NotCompleted
        );
        assert_eq!(runs[0].identity.instance_id.as_deref(), Some("run-1"));
        assert_eq!(runs[1].terminal_state, RunTerminalState::Completed);
        assert_eq!(runs[1].identity.instance_id.as_deref(), Some("run-2"));
        assert!(
            runs[1]
                .segments
                .iter()
                .all(|segment| segment.retry_count == 0)
        );
    }

    #[test]
    fn recorder_pause_remains_in_wall_time_and_requires_review() {
        let mut reducer = RunSessionReducer::new(dungeon_config());
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-1"),
        );
        reducer
            .record_manual_pause(4_000_000, 6_000_000, "user paused capture")
            .unwrap();
        emit(
            &mut reducer,
            &mut events,
            10_000_000,
            dungeon(DungeonEventKind::Completed, "run-1"),
        );

        let run = &reducer.finish()[0];
        assert_eq!(run.timing.wall_time_micros, Some(9_000_000));
        assert_eq!(run.timing.manual_pause_micros, 2_000_000);
        assert_eq!(
            run.submission_disposition,
            RunSubmissionDisposition::CompletedNeedsReview
        );
    }

    #[test]
    fn only_run_connection_or_global_data_gaps_affect_submission_evidence() {
        let mut reducer = RunSessionReducer::new(dungeon_config());
        let mut events = factory();
        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "run-1"),
        );
        emit(
            &mut reducer,
            &mut events,
            2_000_000,
            CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(DataGapEvent {
                kind: DataGapKind::DecodeFailure,
                connection_id: Some(2),
                stream_id: Some(1),
                detail: "unrelated idle stream".into(),
            })),
        );
        emit(
            &mut reducer,
            &mut events,
            3_000_000,
            CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(DataGapEvent {
                kind: DataGapKind::CaptureDrop,
                connection_id: None,
                stream_id: None,
                detail: "global capture loss".into(),
            })),
        );
        emit(
            &mut reducer,
            &mut events,
            4_000_000,
            dungeon(DungeonEventKind::Completed, "run-1"),
        );

        let run = &reducer.finish()[0];
        assert_eq!(run.data_gap_count, 1);
        assert!(
            run.findings
                .contains(&RunEvidenceFinding::DataGaps { count: 1 })
        );
    }

    #[test]
    fn raid_gauntlet_uses_data_driven_segments_and_keeps_season_partition() {
        let mut config = RunReducerConfig {
            activity_kind: ActivityKind::Raid,
            activity_id: Some("raid-portal-zone".into()),
            route_id: Some("gauntlet".into()),
            raid_route_kind: Some(RaidRouteKind::Gauntlet),
            partition: Some(LeaderboardPartitionKey {
                season_id: "season-3".into(),
                activity_id: "raid-portal-zone".into(),
                difficulty_id: "hard".into(),
                route_id: Some("gauntlet".into()),
                encounter_ruleset_id: "bpsr-raids".into(),
                encounter_ruleset_version: 4,
            }),
            ..RunReducerConfig::default()
        };
        for boss in ["raid-boss-1", "raid-boss-2", "raid-boss-3"] {
            config
                .encounter_kinds
                .insert(boss.into(), EncounterKind::GauntletBoss);
            config
                .encounter_segments
                .insert(boss.into(), RunSegmentKind::Gauntlet);
        }
        let mut reducer = RunSessionReducer::new(config);
        let mut events = factory();

        emit(
            &mut reducer,
            &mut events,
            1_000_000,
            dungeon(DungeonEventKind::Started, "raid-run-1"),
        );
        let mut observed = 2_000_000;
        for boss in ["raid-boss-1", "raid-boss-2", "raid-boss-3"] {
            emit(
                &mut reducer,
                &mut events,
                observed,
                encounter(EncounterState::Started, boss),
            );
            emit(
                &mut reducer,
                &mut events,
                observed,
                combat(CombatState::Started),
            );
            observed += 2_000_000;
            emit(
                &mut reducer,
                &mut events,
                observed,
                combat(CombatState::Ended),
            );
            emit(
                &mut reducer,
                &mut events,
                observed,
                encounter(EncounterState::Cleared, boss),
            );
            observed += 1_000_000;
        }
        emit(
            &mut reducer,
            &mut events,
            observed,
            dungeon(DungeonEventKind::Completed, "raid-run-1"),
        );

        let run = &reducer.finish()[0];
        assert_eq!(run.identity.activity_kind, ActivityKind::Raid);
        assert_eq!(run.identity.raid_route_kind, Some(RaidRouteKind::Gauntlet));
        assert_eq!(run.partition.as_ref().unwrap().season_id, "season-3");
        assert_eq!(
            run.partition.as_ref().unwrap().route_id.as_deref(),
            Some("gauntlet")
        );
        let gauntlet = run
            .segments
            .iter()
            .find(|segment| segment.kind == RunSegmentKind::Gauntlet)
            .unwrap();
        assert_eq!(gauntlet.attempt_count, 3);
        assert_eq!(gauntlet.retry_count, 0);
        assert_eq!(gauntlet.successful_attempt_indices, vec![0, 1, 2]);
    }
}
