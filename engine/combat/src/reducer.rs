use std::collections::BTreeMap;

use rlogs_events::{
    BoundaryReason, CanonicalEvent, CombatState, DungeonEvent, DungeonEventKind, EncounterState,
    EventEnvelope, RunState, TimelineEventKind,
};
use thiserror::Error;

use crate::{
    ActivityKind, CombatWindowSummary, EncounterKind, EncounterSummary, EncounterTerminalState,
    LeaderboardPartitionKey, ManualPauseSummary, RUN_ANALYSIS_SCHEMA_VERSION, RaidRouteKind,
    RunAnalysis, RunEvidenceFinding, RunIdentity, RunSegmentKind, RunSegmentSummary,
    RunSubmissionDisposition, RunTerminalState, RunTiming,
};

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
}

#[derive(Debug, Default)]
pub struct RunSessionReducer {
    config: RunReducerConfig,
    source_session_id: Option<String>,
    previous_sequence: Option<u64>,
    previous_observed_micros: Option<u64>,
    pending_identity: RunIdentity,
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
            CanonicalEvent::Dungeon(event) => self.on_dungeon_event(
                &envelope.session_id,
                envelope.sequence,
                envelope.time.observed_micros,
                event,
            )?,
            CanonicalEvent::Timeline(timeline) => self.on_timeline_event(
                &envelope.session_id,
                envelope.sequence,
                envelope.time.observed_micros,
                &timeline.kind,
            )?,
            _ => {}
        }
        Ok(())
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
        event: &DungeonEvent,
    ) -> Result<(), RunReducerError> {
        self.update_dungeon_identity(event);
        match event.kind {
            DungeonEventKind::Entered
            | DungeonEventKind::FlowUpdated
            | DungeonEventKind::ObjectiveUpdated
            | DungeonEventKind::ObjectiveRemoved => {}
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
        Ok(())
    }

    fn on_timeline_event(
        &mut self,
        session_id: &str,
        sequence: u64,
        observed_micros: u64,
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
                        self.start_run(session_id, sequence, observed_micros, authoritative)?
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
            TimelineEventKind::DataGap(_) => {
                if let Some(run) = &mut self.current {
                    run.data_gap_count = run.data_gap_count.saturating_add(1);
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
            TimelineEventKind::Actor(_)
            | TimelineEventKind::EntityAttributes(_)
            | TimelineEventKind::Cast(_)
            | TimelineEventKind::Cooldown(_)
            | TimelineEventKind::Damage(_)
            | TimelineEventKind::Healing(_)
            | TimelineEventKind::Shield(_)
            | TimelineEventKind::Life { .. }
            | TimelineEventKind::Status(_)
            | TimelineEventKind::Position(_) => {
                // Combatants and boss adds never define segment boundaries.
            }
        }
        Ok(())
    }

    fn update_dungeon_identity(&mut self, event: &DungeonEvent) {
        if let Some(dungeon_id) = event.dungeon_id {
            self.pending_identity.observed_dungeon_id = Some(dungeon_id.0.to_string());
        }
        if let Some(instance_id) = &event.instance_id {
            self.pending_identity.instance_id = Some(instance_id.clone());
        }
        if let Some(difficulty_id) = event.difficulty_id {
            self.pending_identity.difficulty_id = Some(difficulty_id.to_string());
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

#[derive(Debug)]
struct RunAccumulator {
    source_session_id: String,
    identity: RunIdentity,
    started_micros: u64,
    observed_until_micros: u64,
    authoritative_start: bool,
    segments: Vec<RunSegmentAccumulator>,
    active_segment: Option<usize>,
    encounters: Vec<EncounterSummary>,
    active_encounter: Option<EncounterAccumulator>,
    manual_pauses: Vec<ManualPauseSummary>,
    data_gap_count: u64,
    manual_boundary: bool,
}

impl RunAccumulator {
    fn new(
        source_session_id: String,
        _: u64,
        started_micros: u64,
        authoritative_start: bool,
        identity: RunIdentity,
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
            segments: Vec::new(),
            active_segment: None,
            encounters: Vec::new(),
            active_encounter: None,
            manual_pauses: Vec::new(),
            data_gap_count: 0,
            manual_boundary: false,
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
        if self.active_encounter.is_some() {
            self.end_encounter(EncounterTerminalState::Ended, observed_micros, at_run_end);
        }
        if let Some(index) = self.active_segment.take() {
            self.segments[index].ended_micros = Some(observed_micros);
            self.segments[index].closed_at_run_end = at_run_end;
        }
        let index = self.segments.len();
        self.segments
            .push(RunSegmentAccumulator::new(index, kind, observed_micros));
        self.active_segment = Some(index);
    }

    fn start_encounter(
        &mut self,
        encounter_id: Option<String>,
        observed_micros: u64,
        config: &RunReducerConfig,
    ) {
        if self.active_encounter.is_some() {
            self.end_encounter(EncounterTerminalState::Ended, observed_micros, false);
        }
        if let Some(segment) = encounter_id
            .as_ref()
            .and_then(|id| config.encounter_segments.get(id))
            .copied()
        {
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

#[derive(Debug)]
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

#[derive(Debug)]
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
    current.observed_dungeon_id = observed
        .observed_dungeon_id
        .clone()
        .or(current.observed_dungeon_id);
    current.instance_id = observed.instance_id.clone().or(current.instance_id);
    current.difficulty_id = observed.difficulty_id.clone().or(current.difficulty_id);
    current.route_id = observed.route_id.clone().or(current.route_id);
    current.raid_route_kind = observed.raid_route_kind.or(current.raid_route_kind);
    current
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
        DungeonId, EntityRef, EntityUuid, EventEnvelopeFactory, EventProvenance, EventSensitivity,
        EventTime, RegionContext, RegionIdentity, TimelineEventKind,
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
            monster_id: None,
            display_name: None,
            class_id: None,
            level: None,
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
