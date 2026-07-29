//! Trusted Blue Protocol: Star Resonance integration for rLogs.

mod catalog;
mod coverage;
mod decoder;
mod framer_set;
mod framing;
mod game_schema_v1;
mod journal;
mod pack;
mod packet;
mod pipeline;
mod privacy;
mod profile;
mod region;
mod route;
mod stream;
mod website;

pub const BPSR_GAME_PLUGIN_ID: &str = "app.rlogs.game.blue-protocol-star-resonance";
pub const BPSR_PROFILE_SCHEMA_ID: &str = "app.rlogs.bpsr.character-profile";
pub const BPSR_PROFILE_SCHEMA_VERSION: u16 = 1;

pub fn bundled_manifest()
-> Result<rlogs_game_plugin_api::GamePluginManifest, rlogs_game_plugin_api::GamePluginManifestError>
{
    rlogs_game_plugin_api::GamePluginManifest::from_toml(include_bytes!("../plugin.toml"))
}

pub use catalog::{
    MappingConfidence, MappingProvenance, RouteCatalog, RouteCatalogError, RouteDefinition,
};
pub use coverage::{
    CoverageReport, CoverageSummary, FragmentCoverage, ProtocolFeatureCoverage,
    ProtocolPackCoverageSummary, RouteCoverage,
};
pub use decoder::{
    DecoderKind, ProtocolDecodeBatch, ProtocolDecodeStatus, ProtocolRuntime, ProtocolRuntimeConfig,
    ProtocolRuntimeError,
};
pub use framer_set::{
    BpsrFramerSet, BpsrFramerSetConfig, BpsrFramerSetConfigError, BpsrFramerSetMetrics,
};
pub use framing::{
    BpsrCallLayout, BpsrFrame, BpsrFrameUpLayout, BpsrFramingConfig, BpsrFramingConfigError,
    BpsrFramingEvent, BpsrFramingIssue, BpsrFramingIssueReason, BpsrFramingMetrics,
    BpsrReturnLayout, BpsrStreamFramer,
};
pub use journal::{CaptureSession, GameBuild, JournalError, ProtocolJournal};
pub use pack::{
    PROTOCOL_PACK_SCHEMA_VERSION, ProtocolFeature, ProtocolPack, ProtocolPackDefinition,
    ProtocolPackError, ProtocolPackRegistry, ProtocolPackRegistryError, ProtocolPackRoute,
    ProtocolPackRouteDisposition, ProtocolPackTarget,
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
    CharacterAppearance, CharacterProfilePatch, CharacterProgression, CollectionSummary,
    CombatProfessionProfile, CosmeticOwnership, EquipmentItem, ImagineOwnership,
    LifeProfessionProfile, ProfileEventError, RgbColor, SeasonProfile, SkillLevel, SocialDisplay,
    TalentLevel,
};
pub use region::{RegionEndpointRule, RegionResolver, RegionResolverError, ResolvedRegion};
pub use route::{FragmentKind, PacketDirection, RouteKey, RoutedMessage};
pub use stream::{JsonlJournalError, JsonlJournalReader, JsonlJournalSummary, JsonlJournalWriter};
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
                    .join("blue-protocol-star-resonance"),
                ResourceStorage::SharedAssets => root
                    .join("../../..")
                    .join("assets")
                    .join("shared")
                    .join("blue-protocol-star-resonance"),
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
            .join("blue-protocol-star-resonance");
        let shared_assets = install_root
            .join("assets")
            .join("shared")
            .join("blue-protocol-star-resonance");
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
