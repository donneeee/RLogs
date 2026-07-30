use serde::{Deserialize, Serialize};

pub const RUN_ANALYSIS_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Dungeon,
    Raid,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaidRouteKind {
    SingleBoss,
    Gauntlet,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSegmentKind {
    Mobbing,
    Boss,
    RaidBoss,
    Gauntlet,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterKind {
    Mobbing,
    Boss,
    RaidBoss,
    GauntletBoss,
    Unknown,
}

impl From<RunSegmentKind> for EncounterKind {
    fn from(value: RunSegmentKind) -> Self {
        match value {
            RunSegmentKind::Mobbing => Self::Mobbing,
            RunSegmentKind::Boss => Self::Boss,
            RunSegmentKind::RaidBoss => Self::RaidBoss,
            RunSegmentKind::Gauntlet => Self::GauntletBoss,
            RunSegmentKind::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTerminalState {
    Open,
    Completed,
    Failed,
    Ended,
    Exited,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterTerminalState {
    Open,
    Cleared,
    Wiped,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSubmissionDisposition {
    NotCompleted,
    CompletedNeedsReview,
    RankCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionLifecycle {
    Active,
    Frozen,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardPartitionKey {
    pub season_id: String,
    pub activity_id: String,
    pub difficulty_id: String,
    pub route_id: Option<String>,
    pub encounter_ruleset_id: String,
    pub encounter_ruleset_version: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdentity {
    pub activity_kind: ActivityKind,
    pub activity_id: Option<String>,
    pub observed_dungeon_id: Option<String>,
    pub instance_id: Option<String>,
    pub difficulty_id: Option<String>,
    pub route_id: Option<String>,
    pub raid_route_kind: Option<RaidRouteKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTiming {
    pub started_micros: u64,
    pub ended_micros: Option<u64>,
    pub observed_until_micros: u64,
    /// Start-to-completion time. Cutscenes, transitions, loading, and manual
    /// recorder pauses are never subtracted.
    pub wall_time_micros: Option<u64>,
    pub active_combat_micros: u64,
    pub noncombat_micros: Option<u64>,
    pub manual_pause_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatWindowSummary {
    pub started_micros: u64,
    pub ended_micros: u64,
    pub duration_micros: u64,
    pub closed_at_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterSummary {
    pub index: u32,
    pub encounter_id: Option<String>,
    pub kind: EncounterKind,
    pub segment_index: u32,
    /// One-based pull number for this encounter ID inside the segment.
    pub attempt_number: u32,
    pub is_retry: bool,
    pub is_successful_attempt: bool,
    pub terminal_state: EncounterTerminalState,
    pub started_micros: u64,
    pub ended_micros: u64,
    pub wall_time_micros: u64,
    pub active_combat_micros: u64,
    pub combat_windows: Vec<CombatWindowSummary>,
    pub closed_at_run_end: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSegmentSummary {
    pub index: u32,
    pub kind: RunSegmentKind,
    pub started_micros: u64,
    pub ended_micros: u64,
    pub wall_time_micros: u64,
    pub active_combat_micros: u64,
    /// Number of bounded encounter pulls in this segment.
    pub attempt_count: u32,
    /// Pulls after the first pull for the same encounter ID.
    pub retry_count: u32,
    /// Sum of pull durations. Recovery, repositioning, and cutscenes between
    /// pulls are excluded.
    pub total_attempt_wall_time_micros: u64,
    pub total_attempt_active_combat_micros: u64,
    /// First pull start through the final observed pull end. This includes
    /// recovery and repositioning between pulls.
    pub elapsed_trying_micros: u64,
    pub between_attempts_micros: u64,
    /// Successful pulls remain individually addressable for multi-wave
    /// mobbing segments.
    pub successful_attempt_indices: Vec<u32>,
    pub successful_attempt_wall_time_micros: u64,
    pub successful_attempt_active_combat_micros: u64,
    /// The final cleared pull. For a boss segment this is the winning pull.
    pub winning_attempt_index: Option<u32>,
    pub winning_attempt_wall_time_micros: Option<u64>,
    pub winning_attempt_active_combat_micros: Option<u64>,
    pub encounter_indices: Vec<u32>,
    pub closed_at_run_end: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualPauseSummary {
    pub started_micros: u64,
    pub resumed_micros: u64,
    pub duration_micros: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "finding", content = "data", rename_all = "snake_case")]
pub enum RunEvidenceFinding {
    DataGaps { count: u64 },
    ManualRecorderPause { count: u32, duration_micros: u64 },
    ManualBoundary,
    StartNotAuthoritative,
    CompletionNotAuthoritative,
    CombatClosedAtRunEnd,
    EncounterClosedAtRunEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAnalysis {
    pub schema_version: u16,
    pub source_session_id: String,
    pub identity: RunIdentity,
    pub partition: Option<LeaderboardPartitionKey>,
    pub terminal_state: RunTerminalState,
    pub authoritative_start: bool,
    pub authoritative_completion: bool,
    pub timing: RunTiming,
    pub segments: Vec<RunSegmentSummary>,
    pub encounters: Vec<EncounterSummary>,
    pub manual_pauses: Vec<ManualPauseSummary>,
    pub data_gap_count: u64,
    pub findings: Vec<RunEvidenceFinding>,
    pub submission_disposition: RunSubmissionDisposition,
}

impl RunAnalysis {
    pub fn is_completed_submission(&self) -> bool {
        self.terminal_state == RunTerminalState::Completed
    }

    pub fn is_rank_candidate(&self) -> bool {
        self.submission_disposition == RunSubmissionDisposition::RankCandidate
    }
}
