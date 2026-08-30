//! Trusted Blue Protocol: Star Resonance integration for rLogs.

mod action_correlation;
mod actor_presentation;
mod auxiliary_action_presentation;
mod battle_imagine_presentation;
mod behavior;
mod boss_presentation;
mod catalog;
mod class_localization;
mod combat_presentation;
mod continuous_recording;
mod coverage;
mod damage_protocol;
mod damage_stage;
mod decoder;
mod dirty_blob_v1;
mod dreamscope_build_inference;
mod dungeon_dirty_v1;
mod effect_fingerprint;
mod factor_attribution;
mod factor_correlation;
mod fight_source;
mod framer_set;
mod framing;
mod game_schema_v1;
mod install;
mod journal;
mod loadout;
mod module_effect_resolution;
mod monster_localization;
mod offline_recording;
mod pack;
mod packet;
mod pipeline;
mod privacy;
mod profile;
mod profile_projection;
mod rdps;
mod rdps_audit;
mod rdps_runtime;
mod rdps_validation;
mod region;
mod route;
mod run_rules;
mod run_segmentation;
mod scene_localization;
mod segmented_recording;
mod shield_state;
mod skill_speed;
mod specialization_detection;
mod state_formula;
mod state_rdps;
mod stream;
mod use_skill_attr;
mod weapon_presentation;
mod website;

pub const BPSR_GAME_PLUGIN_ID: &str = "app.rlogs.game.blue-protocol-star-resonance";
pub const BPSR_PROFILE_SCHEMA_ID: &str = "app.rlogs.bpsr.character-profile";
pub const BPSR_PROFILE_SCHEMA_VERSION: u16 = 1;

pub fn bundled_manifest()
-> Result<rlogs_game_plugin_api::GamePluginManifest, rlogs_game_plugin_api::GamePluginManifestError>
{
    rlogs_game_plugin_api::GamePluginManifest::from_toml(include_bytes!("../plugin.toml"))
}

pub use action_correlation::{
    ACTION_CORRELATION_SCHEMA_VERSION, ActionCorrelationAudit, ActionCorrelationReport,
    DamageCorrelationCandidate, PendingActionEvidence,
};
pub use actor_presentation::{
    ActorCombatIdentity, ActorCombatPresentation, resolve_actor_combat_identity,
    resolve_actor_combat_presentation,
};
pub use auxiliary_action_presentation::{
    AuxiliaryActionPresentation, auxiliary_action_presentation, localized_auxiliary_action_name,
};
pub use battle_imagine_presentation::{
    BattleImaginePresentation, battle_imagine_presentation, localized_battle_imagine_name,
};
pub use behavior::{GameDataObjectiveCatalog, ObjectiveCatalogError, ObjectiveCatalogResolver};
pub use boss_presentation::{is_boss_monster, scene_boss_monster_ids};
pub use catalog::{
    MappingConfidence, MappingProvenance, RouteCatalog, RouteCatalogError, RouteDefinition,
};
pub use class_localization::{
    character_id_from_entity_uuid, class_icon_path, class_role, class_weapon_icon_path,
    is_localized_class_name, localized_class_identities, localized_class_name,
    localized_specialization_identities, localized_specialization_name, specialization_accent,
    specialization_class_id, specialization_icon_path, specialization_role,
};
pub use combat_presentation::{
    CombatActionPresentation, RdpsAttributionEffectPresentation, StatusEffectPresentation,
    combat_action_presentation, localized_combat_action_name, localized_recount_group_name,
    localized_status_effect_name, rdps_attribution_effect_presentation, status_effect_presentation,
};
pub use continuous_recording::{
    ContinuousBpsrRecorder, ContinuousRecordingConfig, ContinuousRecordingError,
    ContinuousRecordingMetrics, ContinuousResearchJournalConfig,
};
pub use coverage::{
    CoverageReport, CoverageSummary, FragmentCoverage, ProtocolFeatureCoverage,
    ProtocolPackCoverageSummary, RouteCoverage,
};
pub use damage_protocol::{BpsrDamageProperty, BpsrDamageSourceKind, BpsrDamageType};
pub use decoder::{
    AnnouncedServerEndpoint, DecoderKind, ProtocolDecodeBatch, ProtocolDecodeStatus,
    ProtocolRuntime, ProtocolRuntimeConfig, ProtocolRuntimeError, ServerClockObservation,
    decode_known_entity_attribute_value,
};
pub use dirty_blob_v1::{DirtyLuckyValueEntry, DirtyLuckyValueUpdate, decode_lucky_value_update};
pub use dreamscope_build_inference::{
    DreamscopeBuildEvidence, DreamscopeBuildInference, DreamscopeBuildInferenceAnalyzer,
    DreamscopeBuildInferenceError, DreamscopeBuildInferenceReport, DreamscopeBuildResolution,
    DreamscopeCatalogSummary, DreamscopeEvidenceKind, DreamscopeEvidenceMatch,
    DreamscopeEvidenceResolution, DreamscopeFactorItemIdentity, DreamscopeSourceCandidate,
    DreamscopeSourceKind, UnresolvedDreamscopeProviderEvidence,
    dreamscope_candidates_for_terminal_effect, dreamscope_catalog_game_build,
    dreamscope_catalog_summary, dreamscope_factor_item_by_id, dreamscope_observed_effect_match,
    dreamscope_terminal_effect_match, infer_dreamscope_build,
};
pub use effect_fingerprint::{
    DreamscopeEffectSelector, EffectDreamscopeSourceKind, EffectFingerprintCatalogSummary,
    EffectFingerprintMatchKind, EffectFingerprintResolution, EffectSourceCandidate,
    EquipmentSuitSelector, ExactDreamscopeLoadout, ResolvedEquippedEffectOwner,
    ResolvedStatusEffectFingerprint, effect_fingerprint_catalog_game_build,
    effect_fingerprint_catalog_summary, resolve_dreamscope_effect_owner,
    resolve_effect_origin_fingerprint, resolve_equipped_effect_owner,
    resolve_status_effect_fingerprint,
};
pub use factor_attribution::{
    PsychoscopeActionCondition, PsychoscopeActionRelation, PsychoscopeActionRelationKind,
    PsychoscopeDamageDomain, PsychoscopeEnergyDirection, PsychoscopeEnergyRelation,
    PsychoscopeEnergyResource, PsychoscopeFactorCategory, PsychoscopeFactorEvidence,
    PsychoscopeFactorRule, PsychoscopeFactorStat, PsychoscopeFactorStatModifier,
    PsychoscopeFactorValueUnit, PsychoscopeFormulaInputRole, PsychoscopeRecountParent,
    PsychoscopeStatCreditPolicy, psychoscope_factor_by_item_id, psychoscope_factor_rules,
    psychoscope_factor_runtime_rules_enabled, psychoscope_recount_parent,
};
pub use factor_correlation::{
    FACTOR_CORRELATION_SCHEMA_VERSION, FactorActionDamageAggregate, FactorActionDamageRole,
    FactorCorrelationError, FactorCorrelationWindow, FactorDamageActorRelation, FactorDamageTotals,
    FactorLifecycleSample, FactorResourceActorRelation, FactorResourceBaseline,
    FactorResourceTransitionSample, FactorResourceWireState, FactorRuleCorrelationSummary,
    FactorSelectionEvidence, FactorSelectionObservation, FactorWindowCloseReason,
    PsychoscopeFactorCorrelationAnalyzer, PsychoscopeFactorCorrelationReport,
    UnmatchedFactorLifecycleEvent,
};
pub use fight_source::BpsrFightSourceKind;
pub use framer_set::{
    BpsrFramerSet, BpsrFramerSetConfig, BpsrFramerSetConfigError, BpsrFramerSetMetrics,
};
pub use framing::{
    BpsrCallLayout, BpsrFrame, BpsrFrameUpLayout, BpsrFramingConfig, BpsrFramingConfigError,
    BpsrFramingEvent, BpsrFramingIssue, BpsrFramingIssueReason, BpsrFramingMetrics,
    BpsrReturnLayout, BpsrStreamFramer,
};
pub use install::{
    BPSR_STEAM_APP_ID, LiveProtocolPackKind, LiveProtocolPackSelection,
    LiveProtocolPackSelectionError, resolve_live_steam_protocol_pack,
    steam_manifest_for_executable,
};
pub use journal::{CaptureSession, GameBuild, JournalError, ProtocolJournal};
pub use loadout::{normalize_auxiliary_imagine_tier, project_actor_loadouts};
pub use module_effect_resolution::{
    ActiveModuleEffect, ActiveModuleEffectSnapshot, ModuleEffectCatalog, ModuleEffectCatalogError,
    ModuleEffectLevel, ModuleEffectResolutionIssue, ModuleEffectSource,
};
pub use monster_localization::localized_monster_name;
pub use offline_recording::{
    CaptureCoverageSummary, DecodeCoverageSummary, EventTopicCoverage, FeatureRecordingCoverage,
    GapRecordingCoverage, JournalTailPolicy, OFFLINE_RECORDING_REPORT_SCHEMA_VERSION,
    OfflineRecordingConfig, OfflineRecordingError, OfflineRecordingLimits, OfflineRecordingReport,
    OfflineRecordingResult, ProtocolPackTransitionRecording, RouteRecordingCoverage,
    RouteRecordingDisposition, record_offline_capture, record_offline_journal,
    record_offline_journal_transition_with_tail_policy, record_offline_journal_with_tail_policy,
};
pub use pack::{
    PROTOCOL_PACK_SCHEMA_VERSION, ProtocolFeature, ProtocolPack, ProtocolPackAcquisition,
    ProtocolPackDefinition, ProtocolPackError, ProtocolPackRegistry, ProtocolPackRegistryError,
    ProtocolPackRoute, ProtocolPackRouteDisposition, ProtocolPackTarget,
};
pub use packet::{
    CaptureAdapter, CaptureGap, CaptureGapKind, CaptureRecord, CaptureRecordDraft,
    CaptureRecordKind, CompressionState, NetworkEndpoint, PacketEnvelope, PacketPayload,
};
pub use pipeline::ResearchPipeline;
pub use privacy::{
    AllowedDataDomain, DecodeDisposition, PrivacyPolicyError, ProhibitedDataClass,
    ProtocolPrivacyPolicy,
};
pub use profile::{
    ActivityProgress, BattleImagineSkill, CharacterAppearance, CharacterProfilePatch,
    CharacterProgression, CollectionSummary, CombatPowerBreakdown, CombatPowerComponent,
    CombatPowerSubcomponent, CombatProfessionProfile, CosmeticOwnership, CultivationAreaProfile,
    CultivationLineProfile, DungeonProgress, DungeonTargetProgress, EquipmentAttributeProfile,
    EquipmentEnchantmentProfile, EquipmentItem, EquipmentSuitEntryProfile, EquippedActionSlot,
    HandbookProgress, ImagineOwnership, LifeProfessionProfile, MasterModeDungeonProgress,
    ModuleItemProfile, ModulePartProfile, ModuleProfile, ModuleUpgradeRecord, ProfileEventError,
    ReputationProgress, RgbColor, SeasonCultivationProfile, SeasonMedalHole, SeasonMedalNode,
    SeasonMedalProfile, SeasonProfile, SkillLevel, SocialDisplay, TalentLevel,
    TalentProgressProfile, WeeklyTowerProgress,
};
pub use profile_projection::{
    BpsrProfileProjectionError, MAXIMUM_LOCAL_PROFILE_CHARACTERS, project_local_profile_packages,
};
pub use rdps::{
    RdpsContributionKind, RdpsEffectLookup, RdpsEffectRule, RdpsReviewState, RdpsSourceScope,
    RdpsStackingRule, RdpsTargetScope, classify_rdps_effect,
    confirmed_damage_contribution_deployment_id, confirmed_damage_contribution_game_build,
    confirmed_damage_contribution_rules,
};
pub use rdps_audit::{
    RDPS_AUDIT_SCHEMA_VERSION, RdpsAuditAbilityCorrelation, RdpsAuditActionKind,
    RdpsAuditDamageTotals, RdpsAuditError, RdpsAuditPacketOrigin, RdpsAuditProviderClass,
    RdpsAuditProviderRecipientExample, RdpsAuditProviderRecipientExampleClass,
    RdpsAuditProviderRecipientMatrix, RdpsAuditRecipientClass, RdpsAuditReport, RdpsEffectAudit,
    RdpsEffectAuditAnalyzer,
};
pub use rdps_validation::{
    RDPS_VALIDATION_REPORT_SCHEMA_VERSION, RdpsValidationAnalyzer, RdpsValidationCapturePreflight,
    RdpsValidationDomainSummary, RdpsValidationDreamscopeSourceObservation,
    RdpsValidationDreamscopeTerminalEffectReport, RdpsValidationError,
    RdpsValidationObligationReport, RdpsValidationObservedProviderScope, RdpsValidationProgress,
    RdpsValidationProjectedProviderRecipientObservation, RdpsValidationProjectionSummary,
    RdpsValidationProviderRecipientObservation, RdpsValidationRationalContributionTotal,
    RdpsValidationRemoteCalculationReadiness, RdpsValidationRemoteEffectReadiness,
    RdpsValidationRemoteReadinessLedger, RdpsValidationRemoteReadinessSummary,
    RdpsValidationRemoteScalarResolution, RdpsValidationReport, RdpsValidationSummary,
    RdpsValidationUnmatchedProjectedEffect,
};
pub use region::{
    RegionEndpointRule, RegionResolver, RegionResolverError, ResolvedRegion,
    SERVER_REALM_CATALOG_SCHEMA_VERSION, ServerRealmCatalog, ServerRealmCatalogDefinition,
    ServerRealmCatalogError, ServerRealmDefinition,
};
pub use route::{FragmentKind, PacketDirection, RouteKey, RoutedMessage};
pub use run_rules::{
    BpsrRunRuleError, BpsrSceneRunIdentity, bundled_run_reducer_config, bundled_run_rule_catalogs,
    bundled_scene_run_identities,
};
pub use run_segmentation::{
    DungeonRunSegmenter, DungeonSegmentAction, DungeonSegmentBoundary, DungeonSegmentEndReason,
    DungeonSegmentStartReason,
};
pub use scene_localization::{ScenePresentation, localized_scene_name, scene_presentation};
pub use segmented_recording::{
    SealedDungeonRunLog, SegmentedDungeonLogWriter, SegmentedRecordingError,
};
pub use shield_state::{ShieldInstanceSnapshot, ShieldListSnapshot, decode_shield_list};
pub use skill_speed::{
    ExactSkillSpeedRatio, ExactSkillStageDuration, ExactSkillStageDurationDelta,
    ExactSkillStageTimingCounterfactual, SkillStageSpeedFamily, SkillStageSpeedInputs,
    exact_external_speed_capacity_fraction, exact_skill_stage_timing_counterfactual,
    singing_stage_speed, skill_stage_speed,
};
pub use specialization_detection::{
    specialization_from_observed_abilities, specialization_identity_from_observed_abilities,
    specialization_talent_node_ids,
};
pub use state_formula::{
    AdditiveFixedPointPairCandidate, BPSR_FIXED_POINT_SCALE, CriticalDamageFactorInterpretation,
    PacketDamageScriptFamily, PositiveFixedPointRounding, additive_fixed_point_pair_candidates,
    exact_additive_fixed_point_marginal_from_observed_output,
    exact_external_attack_and_damage_bonus_fraction, exact_external_attack_and_factors_fraction,
    exact_external_attack_coefficient_stage_fraction, exact_external_attack_ordered_stage_fraction,
    exact_external_capped_critical_chance_and_damage_fraction,
    exact_external_combined_critical_lucky_chance_and_damage_fraction,
    exact_external_combined_critical_lucky_chance_fraction,
    exact_external_combined_critical_lucky_damage_fraction,
    exact_external_composite_damage_fraction, exact_external_critical_chance_and_damage_fraction,
    exact_external_critical_chance_fraction, exact_external_critical_damage_fraction,
    exact_external_damage_bonus_fraction, exact_external_lucky_chance_and_damage_fraction,
    exact_external_lucky_chance_fraction, exact_external_lucky_damage_fraction,
    exact_joint_critical_cold_team_luck_fractions, exact_positive_linear_conversion_delta,
    fixed_point_percent_input_marginal, linear_state_scaled_damage_marginal,
    packet_attack_coefficient_stage_provider_marginal, packet_attribute_family_provider_marginal,
    packet_attribute_family_value, two_stage_percent_input_marginal,
};
pub use state_rdps::{
    BpsrRemoteFactorLearner, BpsrRemoteFactorTimeline, BpsrStateDamageContributionProjector,
    HarmonyGraceFamilyRoundingDiagnostic, HarmonyGraceFormulaTrace,
    InspirationCombinedFormulaTrace, InspirationCombinedPipelineAudit, RemoteRdpsEvidencePolicy,
    proven_state_damage_contribution_effect_ids, remote_rdps_evidence_policy,
    state_damage_contribution_deployment_id, state_damage_contribution_formula_identity,
    state_damage_contribution_formula_target_matches, state_damage_contribution_game_build,
    state_damage_contribution_protocol_pack_digest, state_damage_contribution_target_matches,
    target_vulnerability_candidate_effect_ids,
};
pub use stream::{
    JsonlJournalError, JsonlJournalReader, JsonlJournalRecordStream, JsonlJournalSummary,
    JsonlJournalWriter,
};
pub use use_skill_attr::{
    BPSR_USE_SKILL_ATTR_BUILD, ClientSkillStageEndSnapshot, ClientSkillStageTriggerSnapshot,
    ServerSkillStageEndSnapshot, UseSkillActionDecodeError, UseSkillActionSnapshot,
    UseSkillAttrDecodeError, UseSkillAttributes, UseSkillParamSnapshot, UseSkillPosition,
    decode_client_skill_stage_end, decode_client_skill_stage_trigger,
    decode_server_skill_stage_end, decode_use_skill_attr_into,
    decode_world_use_slot_skill_action_into,
};
pub use weapon_presentation::{
    WeaponLevelPresentation, WeaponPresentation, weapon_level_presentation, weapon_presentation,
};
pub use website::{BpsrWebsiteProfileError, website_profile_request};

#[cfg(test)]
mod manifest_tests {
    use std::path::Path;

    use rlogs_game_plugin_api::{GamePluginCapability, ResourceStorage};
    use rlogs_plugin_host::SharedResourceRegistry;

    use super::*;

    #[test]
    fn bundled_manifest_matches_the_compiled_game_integration() {
        let manifest = bundled_manifest().unwrap();
        assert_eq!(manifest.id, BPSR_GAME_PLUGIN_ID);
        assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
        assert!(
            manifest
                .capabilities
                .contains(&GamePluginCapability::PacketDecoding)
        );
        assert!(
            manifest
                .capabilities
                .contains(&GamePluginCapability::WebsiteProfiles)
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for resource in [
            manifest.resources.protocol_packs.as_deref(),
            manifest.resources.game_data_catalog.as_deref(),
            manifest.resources.research_inventory.as_deref(),
            manifest.resources.localization_staging.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            assert!(root.join(resource).exists(), "missing resource {resource}");
        }
        for export in &manifest.resource_exports {
            let export_root = match export.storage {
                ResourceStorage::Package => root.to_owned(),
                ResourceStorage::PluginAssets => root
                    .join("../../..")
                    .join("assets")
                    .join("blue-protocol-star-resonance")
                    .join("plugins")
                    .join("blue-protocol-star-resonance"),
                ResourceStorage::SharedAssets => root
                    .join("../../..")
                    .join("assets")
                    .join("blue-protocol-star-resonance")
                    .join("shared"),
            };
            assert!(
                export_root.join(&export.path).exists(),
                "missing shared export {} at {}",
                export.name,
                export.path
            );
        }
        let install_root = root.join("../../..");
        let plugin_assets = install_root
            .join("assets")
            .join("blue-protocol-star-resonance")
            .join("plugins")
            .join("blue-protocol-star-resonance");
        let shared_assets = install_root
            .join("assets")
            .join("blue-protocol-star-resonance")
            .join("shared");
        let mut registry = SharedResourceRegistry::default();
        registry
            .register_exports_with_asset_roots(
                &manifest.id,
                root,
                &plugin_assets,
                &shared_assets,
                &manifest.resource_exports,
            )
            .unwrap();
        let catalog = registry.get(&manifest.id, "catalog").unwrap();
        assert_eq!(catalog.schema_id(), "app.rlogs.bpsr.game-data");
        assert_eq!(catalog.schema_version(), 2);
        assert!(catalog.resolve_read_path(None).unwrap().is_dir());
        assert_eq!(
            manifest.website_profiles.unwrap().relative_endpoint,
            crate::website::BPSR_PROFILE_ENDPOINT
        );
    }
}
