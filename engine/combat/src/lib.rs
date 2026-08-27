//! Deterministic, game-neutral run and encounter projections.

mod ancestry;
mod attribution;
mod model;
mod reducer;
mod rules;

pub use ancestry::{ActorAncestryResolver, ActorOwnershipEvidence};
pub use attribution::{
    ActorDamageContribution, ContributionDamageEvent, ContributionStatusEvent,
    ContributionStatusState, DamageContributionKind, DamageContributionReducer,
    DamageContributionRule, DamageContributionRuleError, DamageContributionStacking,
    DamageContributionSummary, EffectDamageContribution, ExactDamageContributionEvent,
    ExactDamageContributionProjector, ExactRationalDamageContributionEvent,
    RationalEffectDamageContribution,
};
pub use model::{
    ActivityKind, CombatWindowSummary, EncounterKind, EncounterSummary, EncounterTerminalState,
    LeaderboardPartitionKey, ManualPauseSummary, PartitionLifecycle, RUN_ANALYSIS_SCHEMA_VERSION,
    RaidRouteKind, RunAnalysis, RunEvidenceFinding, RunIdentity, RunSegmentKind, RunSegmentSummary,
    RunSubmissionDisposition, RunTerminalState, RunTiming,
};
pub use reducer::{RunEventSequencePolicy, RunReducerConfig, RunReducerError, RunSessionReducer};
pub use rules::{
    CompletedObjectiveAction, DifficultyTierRange, DungeonObjectiveRole, DungeonObjectiveRule,
    RUN_RULE_CATALOG_SCHEMA_VERSION, RunRuleCatalog, RunRuleCatalogError, RunRuleConfidence,
    RunRuleEvidence, RunRuleTarget, SceneRunRule,
};
