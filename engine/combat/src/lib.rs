//! Deterministic, game-neutral run and encounter projections.

mod model;
mod reducer;

pub use model::{
    ActivityKind, CombatWindowSummary, EncounterKind, EncounterSummary, EncounterTerminalState,
    LeaderboardPartitionKey, ManualPauseSummary, PartitionLifecycle, RUN_ANALYSIS_SCHEMA_VERSION,
    RaidRouteKind, RunAnalysis, RunEvidenceFinding, RunIdentity, RunSegmentKind, RunSegmentSummary,
    RunSubmissionDisposition, RunTerminalState, RunTiming,
};
pub use reducer::{RunEventSequencePolicy, RunReducerConfig, RunReducerError, RunSessionReducer};
