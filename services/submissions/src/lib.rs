//! Private artifact ingestion and public parse projections for rLogs.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, ETAG},
    },
    response::{IntoResponse, Redirect, Response},
    routing::{get, patch, post, put},
};
use reqwest::Url;
use rlogs_combat::{
    ActivityKind, RaidRouteKind, RunAnalysis, RunSegmentKind, RunSubmissionDisposition,
};
use rlogs_events::{
    ActorKind, CanonicalEvent, EntityAttributeUpdateKind, EntityRef, EventEnvelope,
    EventProvenance, EventSensitivity, EvidenceSource, GameProfileEvent, RunState, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BpsrLifeWaveTriggerLearner, BpsrRemoteFactorLearner,
    BpsrStatResonanceTransitionLearner, BpsrStateDamageContributionProjector,
    CharacterProfilePatch, SwiftVortexCandidateAuditAnalyzer, SwiftVortexCandidateAuditReport,
    bundled_run_reducer_config, character_id_from_entity_uuid, combat_action_presentation,
    combat_breakdown_ability_id, combat_recount_group_id, confirmed_damage_contribution_rules,
    is_stat_resonance_status, localized_class_name, localized_combat_action_name,
    localized_recount_group_name, localized_scene_name, localized_specialization_name,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use rlogs_plugin_combat_meter::{
    CombatHistorySnapshot, CombatHistoryView, CombatTimelinePlugin, HistoryActorSummary,
};
use rlogs_plugin_encounter_recorder::EncounterRecorderPlugin;
use rlogs_submission::{
    ArtifactBuildLimits, LocalLogArtifact, ReportVisibility, Sha256Digest, SubmissionMetadata,
    SubmissionSession, UploadManifest, VerificationTier,
    build_privacy_verified_submission_artifact, submission_privacy_policy_digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

mod accounts;
mod profiles;

use accounts::{
    AccountError, AccountStore, AccountView, AppTokenReceipt, DiscordConfiguration,
    PublicAccountIdentity, WebSessionReceipt,
};
use profiles::{
    PhotoAssetContent, PhotoAssetReceipt, PhotoCatalogQuery, PhotoLikeReceipt,
    ProfilePublishReceipt, ProfileRegistry, ProfileRegistryError, PublicPhotoCatalog,
    PublicProfile, PublicProfileCatalog, PublicProfileCatalogEntry, PublicProfileLoadout,
};
use rlogs_profiles::LocalProfilePackage;

pub const PUBLIC_PARSE_SCHEMA_VERSION: u16 = 12;
pub const PUBLIC_PARSE_PROJECTION_REVISION: u16 = 1;
pub const PUBLIC_CATALOG_SCHEMA_VERSION: u16 = 6;
pub const PUBLIC_RECONCILIATION_SCHEMA_VERSION: u16 = 12;
pub const UPLOAD_RESPONSE_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_CATALOG_ENTRIES: usize = 100_000;
const MAXIMUM_QUERY_LIMIT: usize = 250;
const MAXIMUM_UPLOAD_CHUNKS: usize = 16_384;
const UPLOAD_OWNER_SCHEMA_VERSION: u16 = 1;
const AUTH_INTROSPECTION_SCHEMA_VERSION: u16 = 1;
const PRIVATE_PARSE_MEMBERSHIP_SCHEMA_VERSION: u16 = 1;
const MY_PARSE_CATALOG_SCHEMA_VERSION: u16 = 1;
const COMMUNITY_MILESTONE_CATALOG_SCHEMA_VERSION: u16 = 1;
const LIFE_WAVE_SOURCE_TYPE_ID: i32 = 1;
const LIFE_WAVE_SOURCE_CONFIG_ID: i64 = 2_302_420;
const LIFE_WAVE_EFFECT_ID: i64 = 2_302_421;
const LIFE_WAVE_DURATION_MILLIS: u64 = 5_000;

#[derive(Clone)]
pub enum SubmissionAuthentication {
    UnauthenticatedDevelopment,
    SharedIngestKey {
        key: String,
        submitter_id: String,
    },
    Introspection {
        endpoint: Url,
        client: reqwest::Client,
    },
}

impl SubmissionAuthentication {
    pub fn shared_ingest_key(key: String) -> Result<Self, ServiceError> {
        if key.is_empty() {
            return Err(ServiceError::InvalidConfiguration(
                "shared ingest key cannot be empty".into(),
            ));
        }
        Ok(Self::SharedIngestKey {
            submitter_id: pseudonymous_identifier("sub", key.as_bytes()),
            key,
        })
    }

    pub fn introspection(endpoint: &str) -> Result<Self, ServiceError> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|error| {
            ServiceError::InvalidConfiguration(format!(
                "authentication introspection URL is invalid: {error}"
            ))
        })?;
        if endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err(ServiceError::InvalidConfiguration(
                "authentication introspection URL cannot contain credentials, a query, or a fragment"
                    .into(),
            ));
        }
        let secure = endpoint.scheme() == "https";
        let loopback = endpoint.scheme() == "http"
            && endpoint
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if !secure && !loopback {
            return Err(ServiceError::InvalidConfiguration(
                "authentication introspection must use HTTPS, except for a loopback development bridge"
                    .into(),
            ));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .user_agent(concat!("rLogs-submissions/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                ServiceError::InvalidConfiguration(format!(
                    "could not initialize authentication client: {error}"
                ))
            })?;
        Ok(Self::Introspection { endpoint, client })
    }

    pub fn is_required(&self) -> bool {
        !matches!(self, Self::UnauthenticatedDevelopment)
    }
}

#[derive(Clone)]
pub struct SubmissionService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    root: PathBuf,
    public_site_url: String,
    authentication: SubmissionAuthentication,
    accounts: AccountStore,
    profiles: ProfileRegistry,
    writes: Mutex<()>,
}

struct CatalogRunGroup {
    representative: PublicParseCatalogEntry,
    representative_quality: CanonicalSpineQuality,
    submitters: BTreeSet<String>,
    local_profile_witnesses: BTreeSet<String>,
    reconciliation_sources: Vec<ReconciliationRunSource>,
    milestone_source: MilestoneSource,
}

#[derive(Debug, Clone)]
struct MilestoneSource {
    entry: PublicParseCatalogEntry,
    authoritative_completion: bool,
    participants: Vec<PublicParticipant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalSpineQuality {
    authoritative_start: bool,
    authoritative_completion: bool,
    data_gap_count: u64,
    event_count: u64,
    report_id: String,
}

#[derive(Debug, Clone)]
struct LocalProfileObservation {
    character_id: String,
    event_sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    payload_sha256: String,
    loadout: ProfileLoadoutObservation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProfileLoadoutObservation {
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    equipped_skill_ids: Vec<String>,
    equipped_imagines: Vec<PublicCombatImagineLoadout>,
    equipment_count: Option<usize>,
    equipped_module_count: Option<usize>,
    talent_count: Option<usize>,
}

impl ProfileLoadoutObservation {
    fn is_empty(&self) -> bool {
        self.class_id.is_none()
            && self.specialization_id.is_none()
            && self.equipped_skill_ids.is_empty()
            && self.equipped_imagines.is_empty()
            && self.equipment_count.is_none()
            && self.equipped_module_count.is_none()
            && self.talent_count.is_none()
    }
}

#[derive(Debug, Clone)]
struct RawLocalStateObservation {
    actor_id: u64,
    entity_uuid: i64,
    kind: LocalStateWitnessKind,
    update_kind: EntityAttributeUpdateKind,
    event_sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    payload_sha256: String,
    wire: Option<(u64, u64, u64)>,
    related_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone)]
struct LocalStateObservation {
    character_id: String,
    related_character_id: Option<String>,
    raw: RawLocalStateObservation,
}

#[derive(Debug, Clone)]
struct VerifiedCrossVantageStateEvent {
    report_id: String,
    character_id: String,
    related_character_id: Option<String>,
    placement: LocalStateWitnessPlacement,
    game_time_millis: Option<i64>,
    envelope: EventEnvelope,
}

#[derive(Debug, Default)]
struct SelectedArtifactWitnesses {
    profiles: Vec<PublicLocalProfileWitness>,
    states: Vec<(String, PublicLocalStateWitness)>,
}

/// Private server-side participant membership derived from the sealed replay.
/// Character UIDs in this file are never added to the public parse catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateParseMembership {
    schema_version: u16,
    report_id: String,
    artifact_sha256: String,
    /// Exact sealed actor joins, retained privately for display-name recovery.
    /// None denotes a legacy index that has not cached these joins yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    character_by_actor: Option<BTreeMap<String, String>>,
    runs: Vec<PrivateRunMembership>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateRunMembership {
    run_index: u32,
    character_ids: Vec<String>,
}

struct CrossVantageReplayResult {
    participants: Vec<PublicReconciledParticipant>,
    conservation: PublicAttributionConservation,
    rdps_effects: Vec<PublicRdpsEffectPresentation>,
    rdps_influences: Vec<PublicRdpsInfluence>,
    swift_vortex_candidate_audit: Option<SwiftVortexCandidateAuditReport>,
}

#[derive(Debug, Clone)]
struct ReconciliationRunSource {
    report_id: String,
    run_index: u32,
    artifact_sha256: String,
    protocol_pack_digest: String,
    created_unix_millis: u64,
    quality: CanonicalSpineQuality,
    local_profile_witnesses: Vec<PublicLocalProfileWitness>,
    local_state_witnesses: Vec<PublicLocalStateWitness>,
    /// Stable participant identities that were already present in the public
    /// projection. Only this subset may be exposed by the public
    /// reconciliation manifest.
    public_participant_character_ids: Vec<String>,
    /// Exact participant membership recovered from the sealed artifact. This
    /// is used only for coverage and witness matching; remote UIDs that were
    /// redacted from the public projection stay private.
    participant_character_ids: Vec<String>,
}

impl ReconciliationRunSource {
    fn from_report(
        report: &PublicParseReport,
        run: &PublicRun,
        private_membership: Option<&PrivateRunMembership>,
    ) -> Self {
        let public_participant_character_ids = run
            .participants
            .iter()
            .filter_map(|participant| participant.character_id.clone())
            .collect::<BTreeSet<_>>();
        let mut participant_character_ids = public_participant_character_ids.clone();
        if let Some(private_membership) = private_membership {
            participant_character_ids.extend(private_membership.character_ids.iter().cloned());
        }
        Self {
            report_id: report.report_id.clone(),
            run_index: run.run_index,
            artifact_sha256: report.verification.artifact_sha256.clone(),
            protocol_pack_digest: report.protocol_pack_digest.clone(),
            created_unix_millis: report.created_unix_millis,
            quality: CanonicalSpineQuality::from_report(report, run),
            local_profile_witnesses: run.local_profile_witnesses.clone(),
            local_state_witnesses: run.local_state_witnesses.clone(),
            public_participant_character_ids: public_participant_character_ids
                .into_iter()
                .collect(),
            participant_character_ids: participant_character_ids.into_iter().collect(),
        }
    }
}

impl CanonicalSpineQuality {
    fn from_report(report: &PublicParseReport, run: &PublicRun) -> Self {
        Self {
            authoritative_start: run.authoritative_start,
            authoritative_completion: run.authoritative_completion,
            data_gap_count: run.data_gap_count,
            event_count: report.verification.event_count,
            report_id: report.report_id.clone(),
        }
    }

    fn is_better_than(&self, other: &Self) -> bool {
        (
            self.authoritative_start && self.authoritative_completion,
            self.authoritative_start,
            self.authoritative_completion,
            std::cmp::Reverse(self.data_gap_count),
            self.event_count,
            std::cmp::Reverse(self.report_id.as_str()),
        ) > (
            other.authoritative_start && other.authoritative_completion,
            other.authoritative_start,
            other.authoritative_completion,
            std::cmp::Reverse(other.data_gap_count),
            other.event_count,
            std::cmp::Reverse(other.report_id.as_str()),
        )
    }
}

impl SubmissionService {
    pub fn open(
        root: PathBuf,
        public_site_url: String,
        ingest_key: Option<String>,
    ) -> Result<Self, ServiceError> {
        let authentication = match ingest_key {
            Some(key) => SubmissionAuthentication::shared_ingest_key(key)?,
            None => SubmissionAuthentication::UnauthenticatedDevelopment,
        };
        Self::open_with_authentication(root, public_site_url, authentication)
    }

    pub fn open_with_authentication(
        root: PathBuf,
        public_site_url: String,
        authentication: SubmissionAuthentication,
    ) -> Result<Self, ServiceError> {
        if public_site_url.trim().is_empty() {
            return Err(ServiceError::InvalidConfiguration(
                "public site URL cannot be empty".into(),
            ));
        }
        for relative in [
            "uploads",
            "artifacts/sha256",
            "projections",
            "memberships",
            "reconciliations",
            "profiles",
        ] {
            std::fs::create_dir_all(root.join(relative))?;
        }
        let public_site_url = public_site_url.trim_end_matches('/').to_owned();
        let discord_configuration = DiscordConfiguration::from_environment(&public_site_url)?;
        let accounts = AccountStore::open(root.join("accounts"), discord_configuration)?;
        let profiles = ProfileRegistry::open(root.join("profiles"), public_site_url.clone())?;
        let service = Self {
            inner: Arc::new(ServiceInner {
                root,
                public_site_url,
                authentication,
                accounts,
                profiles,
                writes: Mutex::new(()),
            }),
        };
        let memberships_changed = service.ensure_membership_indexes()?;
        if memberships_changed {
            let _write = service.write_guard();
            service.rebuild_catalog_locked()?;
        } else {
            service.ensure_catalog()?;
        }
        Ok(service)
    }

    /// Rebuild every stale derived report from its immutable artifact.
    ///
    /// This is an explicit maintenance operation: normal reads still refresh
    /// one stale report lazily, while deployments can migrate a complete
    /// projection store before exporting its public read model.
    pub fn refresh_all_projections(&self) -> Result<ProjectionRefreshSummary, ServiceError> {
        let _write = self.write_guard();
        let projection_root = self.inner.root.join("projections");
        let mut paths = std::fs::read_dir(&projection_root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();

        let mut summary = ProjectionRefreshSummary {
            inspected: 0,
            refreshed: 0,
            already_current: 0,
        };
        for path in paths {
            let report: PublicParseReport = read_json(&path)?;
            summary.inspected += 1;
            if report_projection_is_current(&report) {
                summary.already_current += 1;
                continue;
            }
            self.refresh_projection_locked(report)?;
            summary.refreshed += 1;
        }
        if summary.refreshed > 0 {
            self.rebuild_catalog_locked()?;
        }
        Ok(summary)
    }

    fn begin_upload(
        &self,
        manifest: UploadManifest,
        owner: &UploadOwner,
    ) -> Result<BeginUploadResponse, ServiceError> {
        validate_manifest(&manifest)?;
        let digest = manifest
            .sealed_log_digest
            .as_ref()
            .ok_or(ServiceError::MissingSealedDigest)?;
        if let Some(report_id) = self.report_id_for_digest(digest)? {
            return Ok(BeginUploadResponse {
                schema_version: UPLOAD_RESPONSE_SCHEMA_VERSION,
                upload_id: None,
                missing_chunks: Vec::new(),
                existing_report_id: Some(report_id.clone()),
                share_url: Some(self.share_url(&report_id)),
            });
        }

        let _write = self.write_guard();
        if let Some(report_id) = self.report_id_for_digest(digest)? {
            return Ok(BeginUploadResponse {
                schema_version: UPLOAD_RESPONSE_SCHEMA_VERSION,
                upload_id: None,
                missing_chunks: Vec::new(),
                existing_report_id: Some(report_id.clone()),
                share_url: Some(self.share_url(&report_id)),
            });
        }
        let upload_id = upload_id(digest);
        let directory = self.upload_directory(&upload_id)?;
        if directory.exists() {
            self.require_upload_owner(&upload_id, owner)?;
            let stored: UploadManifest = read_json(&directory.join("manifest.json"))?;
            if stored != manifest {
                return Err(ServiceError::UploadManifestConflict);
            }
        } else {
            std::fs::create_dir(&directory)?;
            write_json_atomic(&directory.join("owner.json"), owner)?;
            write_json_atomic(&directory.join("manifest.json"), &manifest)?;
        }
        let missing_chunks = manifest
            .chunks
            .iter()
            .filter(|chunk| !chunk_file_is_valid(&directory, chunk))
            .map(|chunk| chunk.sequence)
            .collect();
        Ok(BeginUploadResponse {
            schema_version: UPLOAD_RESPONSE_SCHEMA_VERSION,
            upload_id: Some(upload_id),
            missing_chunks,
            existing_report_id: None,
            share_url: None,
        })
    }

    fn receive_chunk(
        &self,
        upload_id: &str,
        sequence: u64,
        bytes: &[u8],
        owner: &UploadOwner,
    ) -> Result<ChunkUploadResponse, ServiceError> {
        validate_identifier(upload_id, "upload ID")?;
        let _write = self.write_guard();
        self.require_upload_owner(upload_id, owner)?;
        let manifest = self.load_manifest(upload_id)?;
        let descriptor = manifest
            .chunks
            .iter()
            .find(|chunk| chunk.sequence == sequence)
            .ok_or(ServiceError::UnknownChunk(sequence))?;
        let actual_length = u64::try_from(bytes.len()).map_err(|_| ServiceError::SizeOverflow)?;
        if descriptor.byte_length != actual_length {
            return Err(ServiceError::ChunkLengthMismatch {
                expected: descriptor.byte_length,
                actual: actual_length,
            });
        }
        let digest = digest_bytes(bytes)?;
        if digest != descriptor.sha256 {
            return Err(ServiceError::ChunkDigestMismatch(sequence));
        }

        let path = self.chunk_path(upload_id, sequence)?;
        let duplicate = path.exists();
        if duplicate {
            let existing = std::fs::read(&path)?;
            if existing != bytes {
                return Err(ServiceError::ConflictingChunk(sequence));
            }
        } else {
            write_bytes_atomic(&path, bytes)?;
        }
        Ok(ChunkUploadResponse {
            schema_version: UPLOAD_RESPONSE_SCHEMA_VERSION,
            sequence,
            sha256: digest,
            duplicate,
        })
    }

    fn finalize_upload(
        &self,
        upload_id: &str,
        owner: &UploadOwner,
    ) -> Result<FinalizeUploadResponse, ServiceError> {
        validate_identifier(upload_id, "upload ID")?;
        let _write = self.write_guard();
        self.require_upload_owner(upload_id, owner)?;
        let manifest = self.load_manifest(upload_id)?;
        let sealed_digest = manifest
            .sealed_log_digest
            .as_ref()
            .ok_or(ServiceError::MissingSealedDigest)?
            .clone();
        if let Some(report_id) = self.report_id_for_digest(&sealed_digest)? {
            let _ = std::fs::remove_dir_all(self.upload_directory(upload_id)?);
            return Ok(self.finalize_response(&report_id, &sealed_digest, true));
        }

        let assembled = self.assemble_upload(upload_id, &manifest)?;
        let artifact = File::open(&assembled).and_then(|file| {
            build_privacy_verified_submission_artifact(
                file,
                ArtifactBuildLimits::default(),
                RlogLimits::default(),
            )
            .map_err(std::io::Error::other)
        })?;
        verify_artifact_metadata(&manifest, &artifact)?;

        let report_id = report_id(&sealed_digest);
        let created_unix_millis = unix_millis()?;
        let mut report = build_public_report(
            &assembled,
            &manifest,
            &artifact,
            &report_id,
            created_unix_millis,
            owner.public_provenance(),
        )?;
        // Account consent is evaluated only after the immutable artifact has
        // passed privacy verification and server replay. It can promote this
        // verified projection to public, but it never weakens those gates.
        if let Some(submitter_id) = owner.submitter_id.as_deref() {
            if self
                .inner
                .accounts
                .publishes_verified_parses(submitter_id)?
            {
                report.visibility = verified_report_visibility(report.visibility, true);
            }
        }

        let artifact_path = self.artifact_path(&sealed_digest)?;
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !artifact_path.exists() {
            std::fs::rename(&assembled, &artifact_path)?;
        } else {
            std::fs::remove_file(&assembled)?;
        }
        let membership = build_private_parse_membership(&artifact_path, &report)?;
        write_json_atomic(&self.membership_path(&report_id)?, &membership)?;
        apply_verified_character_keys(
            &mut report,
            membership
                .character_by_actor
                .as_ref()
                .expect("new memberships always include sealed actor identities"),
        )?;
        self.restore_submitter_name(&artifact_path, &mut report)?;
        write_json_atomic(&self.projection_path(&report_id)?, &report)?;
        self.rebuild_catalog_locked()?;
        let _ = std::fs::remove_dir_all(self.upload_directory(upload_id)?);
        Ok(self.finalize_response(&report_id, &sealed_digest, false))
    }

    pub fn report(&self, report_id: &str) -> Result<PublicParseReport, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        let report: PublicParseReport = read_json(&self.projection_path(report_id)?)?;
        if report.visibility == ReportVisibility::Private {
            return Err(ServiceError::NotFound);
        }
        self.refresh_projection_if_needed(report)
    }

    fn account_report(
        &self,
        report_id: &str,
        submitter_id: &str,
    ) -> Result<PublicParseReport, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        let report: PublicParseReport = read_json(&self.projection_path(report_id)?)?;
        let submitted_by_account =
            report.submission_provenance.submitter_id.as_deref() == Some(submitter_id);
        if submitted_by_account {
            return self.refresh_projection_if_needed(report);
        }
        if report.visibility == ReportVisibility::Private {
            return Err(ServiceError::NotFound);
        }
        let claimed_character_ids = self
            .owned_profile_catalog(submitter_id)?
            .profiles
            .into_iter()
            .map(|profile| profile.character_id)
            .collect::<BTreeSet<_>>();
        let membership = self.read_membership(&report)?;
        if membership.runs.iter().any(|run| {
            run.character_ids
                .iter()
                .any(|character_id| claimed_character_ids.contains(character_id))
        }) {
            self.refresh_projection_if_needed(report)
        } else {
            Err(ServiceError::NotFound)
        }
    }

    /// Replays a legacy public projection from its immutable, privacy-verified
    /// artifact when a user first opens it. This keeps old My Parses entries
    /// useful without trusting a client resubmission or paying the replay cost
    /// on every receiver startup.
    fn refresh_projection_if_needed(
        &self,
        report: PublicParseReport,
    ) -> Result<PublicParseReport, ServiceError> {
        if report_projection_is_current(&report) {
            if !report
                .runs
                .iter()
                .flat_map(|run| &run.participants)
                .any(|p| {
                    p.display_name
                        .as_deref()
                        .is_none_or(|name| name.trim().is_empty())
                })
            {
                return Ok(report);
            }
            let _write = self.write_guard();
            let mut current: PublicParseReport =
                read_json(&self.projection_path(&report.report_id)?)?;
            let Some(submitter) = current.submission_provenance.submitter_id.as_deref() else {
                return Ok(current);
            };
            if self.owned_profile_catalog(submitter)?.profiles.is_empty() {
                return Ok(current);
            }
            let digest = Sha256Digest::parse(current.verification.artifact_sha256.clone())?;
            if self.restore_submitter_name(&self.artifact_path(&digest)?, &mut current)? {
                write_json_atomic(&self.projection_path(&current.report_id)?, &current)?;
            }
            return Ok(current);
        }
        let _write = self.write_guard();
        let current: PublicParseReport = read_json(&self.projection_path(&report.report_id)?)?;
        if report_projection_is_current(&current) {
            return Ok(current);
        }
        let refreshed = self.refresh_projection_locked(current)?;
        if refreshed.visibility == ReportVisibility::Public {
            self.rebuild_catalog_locked()?;
        }
        Ok(refreshed)
    }

    fn refresh_projection_locked(
        &self,
        report: PublicParseReport,
    ) -> Result<PublicParseReport, ServiceError> {
        let artifact_digest = Sha256Digest::parse(report.verification.artifact_sha256.clone())?;
        let artifact_path = self.artifact_path(&artifact_digest)?;
        let artifact_file = File::open(&artifact_path)?;
        let artifact = build_privacy_verified_submission_artifact(
            artifact_file,
            ArtifactBuildLimits::default(),
            RlogLimits::default(),
        )
        .map_err(std::io::Error::other)?;
        let protocol_digest = report
            .protocol_pack_digest
            .strip_prefix("sha256:")
            .unwrap_or(&report.protocol_pack_digest);
        let metadata = SubmissionMetadata::new(
            report.game_plugin_id.clone(),
            report.report_id.clone(),
            0,
            report.report_id.clone(),
            report.region_id.clone(),
            report.client_build.clone(),
            Sha256Digest::parse(protocol_digest.to_owned())?,
            Sha256Digest::parse(report.verification.privacy_policy_digest.clone())?,
            report.visibility,
        );
        let manifest = UploadManifest {
            metadata,
            chunks: Vec::new(),
            sealed_log_digest: Some(artifact_digest),
        };
        let mut refreshed = build_public_report(
            &artifact_path,
            &manifest,
            &artifact,
            &report.report_id,
            report.created_unix_millis,
            report.submission_provenance,
        )?;
        let membership = build_private_parse_membership(&artifact_path, &refreshed)?;
        write_json_atomic(&self.membership_path(&refreshed.report_id)?, &membership)?;
        apply_verified_character_keys(
            &mut refreshed,
            membership
                .character_by_actor
                .as_ref()
                .expect("new memberships always include sealed actor identities"),
        )?;
        self.restore_submitter_name(&artifact_path, &mut refreshed)?;
        write_json_atomic(&self.projection_path(&refreshed.report_id)?, &refreshed)?;
        Ok(refreshed)
    }

    /// Resolve a missing label using the sealed actor-to-UID join and the
    /// submitter's verified profile. Actor numbers alone never identify users.
    fn restore_submitter_name(
        &self,
        artifact_path: &Path,
        report: &mut PublicParseReport,
    ) -> Result<bool, ServiceError> {
        if !report
            .runs
            .iter()
            .flat_map(|run| &run.participants)
            .any(|participant| {
                participant
                    .display_name
                    .as_deref()
                    .is_none_or(|name| name.trim().is_empty())
            })
        {
            return Ok(false);
        }
        let Some(submitter) = report.submission_provenance.submitter_id.as_deref() else {
            return Ok(false);
        };
        let profiles = self.owned_profile_catalog(submitter)?;
        if profiles.profiles.is_empty() {
            return Ok(false);
        }
        let mut membership = self.read_membership(report)?;
        if membership.character_by_actor.is_none() {
            membership.character_by_actor = Some(sealed_character_identities(artifact_path)?);
            write_json_atomic(&self.membership_path(&report.report_id)?, &membership)?;
        }
        let identities = membership
            .character_by_actor
            .as_ref()
            .expect("initialized above");
        Ok(restore_verified_names(
            report,
            identities,
            &profiles.profiles,
        ))
    }

    fn update_report_visibility(
        &self,
        report_id: &str,
        submitter_id: &str,
        visibility: ReportVisibility,
    ) -> Result<UpdateParseVisibilityResponse, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        let _write = self.write_guard();
        let path = self.projection_path(report_id)?;
        let mut report: PublicParseReport = read_json(&path)?;
        if report.submission_provenance.submitter_id.as_deref() != Some(submitter_id) {
            // Do not disclose whether a report exists to a different account.
            return Err(ServiceError::NotFound);
        }
        if !report_projection_is_current(&report) {
            report = self.refresh_projection_locked(report)?;
        }
        report.visibility = visibility;
        write_json_atomic(&path, &report)?;
        self.rebuild_catalog_locked()?;
        Ok(UpdateParseVisibilityResponse {
            schema_version: 1,
            report_id: report.report_id,
            visibility,
            share_url: (visibility != ReportVisibility::Private).then(|| self.share_url(report_id)),
        })
    }

    fn my_parse_catalog(
        &self,
        submitter_id: &str,
        query: &MyParsesQuery,
    ) -> Result<MyParseCatalog, ServiceError> {
        let claimed_character_ids = self
            .owned_profile_catalog(submitter_id)?
            .profiles
            .into_iter()
            .map(|profile| profile.character_id)
            .collect::<BTreeSet<_>>();
        self.my_parse_catalog_for_character_ids(submitter_id, &claimed_character_ids, query)
    }

    fn my_parse_catalog_for_character_ids(
        &self,
        submitter_id: &str,
        claimed_character_ids: &BTreeSet<String>,
        query: &MyParsesQuery,
    ) -> Result<MyParseCatalog, ServiceError> {
        let mut entries = Vec::new();
        for file in std::fs::read_dir(self.inner.root.join("projections"))? {
            let file = file?;
            if file.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let report: PublicParseReport = read_json(&file.path())?;
            let submitted_by_you =
                report.submission_provenance.submitter_id.as_deref() == Some(submitter_id);
            if report.visibility == ReportVisibility::Private && !submitted_by_you {
                continue;
            }
            let membership = self.read_membership(&report)?;
            for run in &report.runs {
                let matched_character_ids = membership
                    .runs
                    .iter()
                    .find(|membership| membership.run_index == run.run_index)
                    .into_iter()
                    .flat_map(|membership| &membership.character_ids)
                    .filter(|character_id| claimed_character_ids.contains(*character_id))
                    .cloned()
                    .collect::<Vec<_>>();
                if !submitted_by_you && matched_character_ids.is_empty() {
                    continue;
                }
                entries.push(MyParseCatalogEntry {
                    parse: PublicParseCatalogEntry::from_report(&report, run),
                    visibility: report.visibility,
                    submitted_by_you,
                    matched_character_ids,
                });
                if entries.len() > MAXIMUM_CATALOG_ENTRIES {
                    return Err(ServiceError::CatalogTooLarge);
                }
            }
        }
        entries.sort_by(|left, right| {
            right
                .parse
                .created_unix_millis
                .cmp(&left.parse.created_unix_millis)
                .then_with(|| left.parse.report_id.cmp(&right.parse.report_id))
                .then_with(|| left.parse.run_index.cmp(&right.parse.run_index))
        });
        let total_entries = entries.len();
        let offset = query.offset.unwrap_or(0).min(total_entries);
        let limit = query.limit.unwrap_or(100).clamp(1, MAXIMUM_QUERY_LIMIT);
        let end = offset.saturating_add(limit).min(total_entries);
        let next_offset = (end < total_entries).then_some(end);
        Ok(MyParseCatalog {
            schema_version: MY_PARSE_CATALOG_SCHEMA_VERSION,
            total_entries,
            offset,
            next_offset,
            claimed_character_ids: claimed_character_ids.iter().cloned().collect(),
            entries: entries[offset..end].to_vec(),
        })
    }

    fn publish_profile(
        &self,
        package: LocalProfilePackage,
        owner: &UploadOwner,
        device_token: &str,
    ) -> Result<ProfilePublishReceipt, ServiceError> {
        let _write = self.write_guard();
        Ok(self.inner.profiles.publish(
            package,
            owner.submitter_id.as_deref(),
            owner.device_id.as_deref(),
            device_token,
            unix_millis()?,
        )?)
    }

    fn profile(&self, profile_id: &str) -> Result<PublicProfile, ServiceError> {
        Ok(self.inner.profiles.get(profile_id)?)
    }

    fn profile_loadout(
        &self,
        profile_id: &str,
        project_id: i32,
    ) -> Result<PublicProfileLoadout, ServiceError> {
        Ok(self.inner.profiles.get_loadout(profile_id, project_id)?)
    }

    fn publish_profile_photo(
        &self,
        profile_id: &str,
        photo_id: u32,
        bytes: &[u8],
        owner: &UploadOwner,
    ) -> Result<PhotoAssetReceipt, ServiceError> {
        let _write = self.write_guard();
        Ok(self.inner.profiles.publish_photo_asset(
            profile_id,
            photo_id,
            bytes,
            owner.submitter_id.as_deref(),
            unix_millis()?,
        )?)
    }

    fn profile_photo(
        &self,
        profile_id: &str,
        photo_id: u32,
    ) -> Result<PhotoAssetContent, ServiceError> {
        Ok(self.inner.profiles.photo_asset(profile_id, photo_id)?)
    }

    fn profile_catalog(
        &self,
        query: &ProfileCatalogQuery,
    ) -> Result<PublicProfileCatalog, ServiceError> {
        Ok(self.inner.profiles.catalog(query.character_id.as_deref())?)
    }

    fn photo_catalog(
        &self,
        query: &PhotoCatalogQuery,
        viewer_submitter_id: Option<&str>,
    ) -> Result<PublicPhotoCatalog, ServiceError> {
        Ok(self
            .inner
            .profiles
            .photo_catalog(query, viewer_submitter_id)?)
    }

    fn set_profile_photo_like(
        &self,
        profile_id: &str,
        photo_id: u32,
        submitter_id: &str,
        liked: bool,
    ) -> Result<PhotoLikeReceipt, ServiceError> {
        let _write = self.write_guard();
        Ok(self.inner.profiles.set_photo_like(
            profile_id,
            photo_id,
            submitter_id,
            liked,
            unix_millis()?,
        )?)
    }

    fn owned_profile_catalog(
        &self,
        submitter_id: &str,
    ) -> Result<PublicProfileCatalog, ServiceError> {
        Ok(self.inner.profiles.owned_catalog(submitter_id)?)
    }

    pub fn reconciliation(
        &self,
        run_group_id: &str,
    ) -> Result<PublicRunReconciliation, ServiceError> {
        read_json(&self.reconciliation_path(run_group_id)?)
    }

    fn load_verified_cross_vantage_state_events(
        &self,
        reconciliation: &PublicRunReconciliation,
    ) -> Result<Vec<VerifiedCrossVantageStateEvent>, ServiceError> {
        let mut selected_by_report = BTreeMap::<String, SelectedArtifactWitnesses>::new();
        for character in &reconciliation.characters {
            let Some(selected_report_id) = character.selected_report_id.as_deref() else {
                continue;
            };
            if selected_report_id == reconciliation.canonical_spine.report_id {
                continue;
            }
            let source = character
                .witnesses
                .iter()
                .find(|source| source.report_id == selected_report_id)
                .ok_or_else(|| ServiceError::CrossVantageReplay(format!(
                    "selected report {selected_report_id} has no witness source for character {}",
                    character.character_id
                )))?;
            let selected = selected_by_report
                .entry(selected_report_id.to_owned())
                .or_default();
            selected.profiles.extend(source.snapshots.clone());
            selected.states.extend(
                source
                    .state_snapshots
                    .iter()
                    .cloned()
                    .map(|witness| (character.character_id.clone(), witness)),
            );
        }

        let mut verified = Vec::new();
        for (report_id, mut selected) in selected_by_report {
            selected
                .profiles
                .sort_by_key(|witness| witness.event_sequence);
            selected
                .profiles
                .dedup_by_key(|witness| witness.event_sequence);
            selected
                .states
                .sort_by_key(|(_, witness)| witness.event_sequence);
            selected
                .states
                .dedup_by_key(|(_, witness)| witness.event_sequence);
            let report = reconciliation
                .reports
                .iter()
                .find(|report| report.report_id == report_id)
                .ok_or_else(|| {
                    ServiceError::CrossVantageReplay(format!(
                        "selected report {report_id} is absent from the reconciliation manifest"
                    ))
                })?;
            let digest = Sha256Digest::parse(report.artifact_sha256.clone())?;
            let path = self.artifact_path(&digest)?;
            let character_id_by_entity_uuid = artifact_character_id_by_entity_uuid(&path)?;
            let file = File::open(&path).map_err(|error| {
                ServiceError::CrossVantageReplay(format!(
                    "could not open selected artifact {} for report {report_id}: {error}",
                    path.display()
                ))
            })?;
            let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
            let mut seen_profiles = BTreeSet::new();
            let mut seen_states = BTreeSet::new();
            reader.replay(|envelope| {
                if let Ok(index) = selected
                    .profiles
                    .binary_search_by_key(&envelope.sequence, |witness| witness.event_sequence)
                {
                    let witness = &selected.profiles[index];
                    verify_profile_witness_event(&report_id, witness, envelope)
                        .map_err(|error| error.to_string())?;
                    seen_profiles.insert(witness.event_sequence);
                    verified.push(VerifiedCrossVantageStateEvent {
                        report_id: report_id.clone(),
                        character_id: witness.character_id.clone(),
                        related_character_id: None,
                        placement: witness.placement,
                        game_time_millis: witness.game_time_millis,
                        envelope: envelope.clone(),
                    });
                }
                if let Ok(index) = selected
                    .states
                    .binary_search_by_key(&envelope.sequence, |(_, witness)| witness.event_sequence)
                {
                    let (character_id, witness) = &selected.states[index];
                    verify_state_witness_event(&report_id, witness, envelope)
                        .map_err(|error| error.to_string())?;
                    verify_state_witness_characters(
                        &report_id,
                        character_id,
                        witness,
                        envelope,
                        &character_id_by_entity_uuid,
                    )
                    .map_err(|error| error.to_string())?;
                    seen_states.insert(witness.event_sequence);
                    verified.push(VerifiedCrossVantageStateEvent {
                        report_id: report_id.clone(),
                        character_id: character_id.clone(),
                        related_character_id: witness.related_character_id.clone(),
                        placement: witness.placement,
                        game_time_millis: witness.game_time_millis,
                        envelope: envelope.clone(),
                    });
                }
                Ok(())
            })?;
            if let Some(sequence) = selected
                .profiles
                .iter()
                .map(|witness| witness.event_sequence)
                .filter(|sequence| !seen_profiles.contains(sequence))
                .chain(
                    selected
                        .states
                        .iter()
                        .map(|(_, witness)| witness.event_sequence)
                        .filter(|sequence| !seen_states.contains(sequence)),
                )
                .next()
            {
                return Err(ServiceError::CrossVantageWitnessMismatch {
                    report_id,
                    event_sequence: sequence,
                });
            }
        }
        verified.sort_by(|left, right| {
            (
                left.placement,
                left.game_time_millis,
                &left.report_id,
                left.envelope.sequence,
            )
                .cmp(&(
                    right.placement,
                    right.game_time_millis,
                    &right.report_id,
                    right.envelope.sequence,
                ))
        });
        Ok(verified)
    }

    fn replay_cross_vantage_attribution(
        &self,
        reconciliation: &PublicRunReconciliation,
        mut imported_events: Vec<VerifiedCrossVantageStateEvent>,
    ) -> Result<CrossVantageReplayResult, ServiceError> {
        let canonical_digest =
            Sha256Digest::parse(reconciliation.canonical_spine.artifact_sha256.clone())?;
        let canonical_path = self.artifact_path(&canonical_digest)?;
        let canonical_entities = canonical_character_entities(&canonical_path)?;
        remap_cross_vantage_state_entities(&mut imported_events, &canonical_entities)?;

        let mut remote_factor_learner =
            BpsrRemoteFactorLearner::new().map_err(ServiceError::Replay)?;
        let mut life_wave_trigger_learner = BpsrLifeWaveTriggerLearner::default();
        let mut stat_resonance_learner =
            BpsrStatResonanceTransitionLearner::new().map_err(ServiceError::Replay)?;
        replay_canonical_with_cross_vantage_state(
            &canonical_path,
            reconciliation.canonical_spine.run_index,
            &imported_events,
            |envelope, imported| {
                if imported {
                    stat_resonance_learner.observe(envelope);
                }
                if imported && life_wave_trigger_learner.observe(envelope) {
                    return Ok(());
                }
                remote_factor_learner.observe(envelope);
                Ok(())
            },
        )?;
        let remote_factors = remote_factor_learner.finish();
        let life_wave_triggers = life_wave_trigger_learner.finish();
        let stat_resonance_transitions = stat_resonance_learner.finish();

        let mut meter = CombatTimelinePlugin::with_damage_contribution_projection(
            confirmed_damage_contribution_rules().map_err(ServiceError::Replay)?,
            Some(Box::new(
                BpsrStateDamageContributionProjector::new_with_cross_vantage_timelines(
                    remote_factors,
                    life_wave_triggers,
                    stat_resonance_transitions,
                )
                .map_err(ServiceError::Replay)?,
            )),
        )
        .map_err(ServiceError::Replay)?
        .with_ability_breakdown_resolver(combat_breakdown_ability_id);
        let mut encounter = EncounterRecorderPlugin::new(
            bundled_run_reducer_config()
                .map_err(|error| ServiceError::Replay(error.to_string()))?,
        );
        let header_file = File::open(&canonical_path)?;
        let header_reader = RlogReader::new(BufReader::new(header_file), RlogLimits::default())?;
        let header = header_reader.header().clone();
        meter.begin_live(&header);
        encounter.begin_live(&header);
        let mut swift_vortex_audit = SwiftVortexCandidateAuditAnalyzer::new();
        replay_canonical_with_cross_vantage_state(
            &canonical_path,
            reconciliation.canonical_spine.run_index,
            &imported_events,
            |envelope, imported| {
                swift_vortex_audit.observe(envelope);
                if imported
                    && matches!(
                        &envelope.event,
                        CanonicalEvent::Timeline(timeline)
                            if matches!(
                                timeline.kind,
                                TimelineEventKind::Status(_) | TimelineEventKind::Healing(_)
                            )
                    )
                {
                    return Ok(());
                }
                meter.observe_live(envelope);
                if !imported {
                    encounter
                        .observe_live(envelope)
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
        )?;
        let run_projection = encounter
            .live_snapshot()
            .map_err(|error| ServiceError::Replay(error.to_string()))?;
        let mut history = meter
            .history_snapshot(&run_projection.runs)
            .map_err(|error| ServiceError::Replay(error.to_string()))?;
        enrich_bpsr_history_ability_presentation(&mut history)?;
        let run = history
            .runs
            .iter()
            .find(|run| run.run_index == reconciliation.canonical_spine.run_index)
            .ok_or_else(|| {
                ServiceError::CrossVantageReplay(format!(
                    "canonical history has no run index {}",
                    reconciliation.canonical_spine.run_index
                ))
            })?;
        let view = run
            .views
            .iter()
            .find(|view| view.kind == "all")
            .or_else(|| run.views.first())
            .ok_or_else(|| ServiceError::CrossVantageReplay("canonical run has no view".into()))?;

        let canonical_report: PublicParseReport =
            read_json(&self.projection_path(&reconciliation.canonical_spine.report_id)?)?;
        let canonical_run = canonical_report
            .runs
            .iter()
            .find(|run| run.run_index == reconciliation.canonical_spine.run_index)
            .ok_or_else(|| {
                ServiceError::CrossVantageReplay(
                    "canonical public projection is missing the selected run".into(),
                )
            })?;
        let canonical_damage = canonical_run
            .participants
            .iter()
            .map(|participant| (public_participant_key(participant), participant.damage))
            .collect::<BTreeMap<_, _>>();
        let replay_damage = view
            .actors
            .iter()
            .filter(|actor| is_public_participant(actor))
            .map(|actor| {
                let participant = public_participant(actor);
                (public_participant_key(&participant), participant.damage)
            })
            .collect::<BTreeMap<_, _>>();
        if canonical_damage != replay_damage {
            return Err(ServiceError::CrossVantageReplay(format!(
                "state injection changed ordinary participant damage: canonical={canonical_damage:?}, replay={replay_damage:?}"
            )));
        }

        let participants = view
            .actors
            .iter()
            .filter(|actor| is_public_participant(actor))
            .map(|actor| PublicReconciledParticipant {
                participant: public_participant(actor),
                rdps_damage: actor.rdps_damage,
                contribution_given: actor.rdps_contribution_given,
                contribution_received: actor.rdps_contribution_received,
                rdps_incomplete: actor.rdps_incomplete,
            })
            .collect::<Vec<_>>();
        let raw_damage = checked_i64_sum(
            participants
                .iter()
                .map(|participant| participant.participant.damage),
        )?;
        let rdps_damage = checked_optional_i64_sum(
            participants
                .iter()
                .map(|participant| participant.rdps_damage),
            "rDPS damage",
        )?;
        let contribution_given = checked_optional_i64_sum(
            participants
                .iter()
                .map(|participant| participant.contribution_given),
            "contribution given",
        )?;
        let contribution_received = checked_optional_i64_sum(
            participants
                .iter()
                .map(|participant| participant.contribution_received),
            "contribution received",
        )?;
        let conservation = PublicAttributionConservation {
            raw_damage,
            rdps_damage,
            contribution_given,
            contribution_received,
            conserved: contribution_given == contribution_received && raw_damage == rdps_damage,
        };
        if !conservation.conserved {
            return Err(ServiceError::CrossVantageReplay(format!(
                "party conservation failed: raw={raw_damage}, rdps={rdps_damage}, given={contribution_given}, received={contribution_received}"
            )));
        }
        let swift_vortex_candidate_audit = swift_vortex_audit.report();
        Ok(CrossVantageReplayResult {
            participants,
            conservation,
            rdps_effects: public_rdps_effects(view),
            rdps_influences: public_rdps_influences(view),
            swift_vortex_candidate_audit: (swift_vortex_candidate_audit
                .candidate_status_event_count
                > 0)
            .then_some(swift_vortex_candidate_audit),
        })
    }

    pub fn catalog(&self, query: &CatalogQuery) -> Result<PublicParseCatalog, ServiceError> {
        let mut catalog: PublicParseCatalog = read_json(&self.catalog_path())?;
        catalog.entries.retain(|entry| query.matches(entry));
        catalog.facets = CatalogFacets::from_entries(&catalog.entries);
        catalog.total_entries = catalog.entries.len();
        let limit = query.limit.unwrap_or(50).clamp(1, MAXIMUM_QUERY_LIMIT);
        let offset = query.offset.unwrap_or_default().min(catalog.total_entries);
        catalog.entries = catalog
            .entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();
        catalog.offset = offset;
        let consumed = offset.saturating_add(catalog.entries.len());
        catalog.next_offset = (consumed < catalog.total_entries).then_some(consumed);
        Ok(catalog)
    }

    pub fn community_milestones(
        &self,
        query: &CommunityMilestoneQuery,
    ) -> Result<PublicCommunityMilestoneCatalog, ServiceError> {
        let mut catalog: PublicCommunityMilestoneCatalog =
            read_json(&self.community_milestone_catalog_path())?;
        let limit = query.limit.unwrap_or(12).clamp(1, 50);
        catalog.entries.truncate(limit);
        Ok(catalog)
    }

    pub fn observed_characters(&self) -> Result<PublicObservedCharacterCatalog, ServiceError> {
        read_json(&self.observed_character_catalog_path())
    }

    fn finalize_response(
        &self,
        report_id: &str,
        digest: &Sha256Digest,
        duplicate: bool,
    ) -> FinalizeUploadResponse {
        FinalizeUploadResponse {
            schema_version: UPLOAD_RESPONSE_SCHEMA_VERSION,
            report_id: report_id.into(),
            accepted_log_digest: digest.clone(),
            verification_tier: VerificationTier::Replayed,
            share_url: self.share_url(report_id),
            duplicate,
        }
    }

    fn share_url(&self, report_id: &str) -> String {
        format!(
            "{}/parses/?parse={report_id}#parse",
            self.inner.public_site_url
        )
    }

    fn require_upload_owner(
        &self,
        upload_id: &str,
        actual: &UploadOwner,
    ) -> Result<(), ServiceError> {
        let expected: UploadOwner =
            read_json(&self.upload_directory(upload_id)?.join("owner.json"))?;
        if expected == *actual {
            Ok(())
        } else {
            Err(ServiceError::UploadOwnerMismatch)
        }
    }

    fn write_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.inner
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn ensure_catalog(&self) -> Result<(), ServiceError> {
        let _write = self.write_guard();
        let path = self.catalog_path();
        let current = path
            .is_file()
            .then(|| read_json::<PublicParseCatalog>(&path).ok())
            .flatten();
        if !current
            .as_ref()
            .is_some_and(|catalog| catalog.schema_version == PUBLIC_CATALOG_SCHEMA_VERSION)
            || !self.community_milestone_catalog_path().is_file()
            || !self.observed_character_catalog_path().is_file()
        {
            self.rebuild_catalog_locked()?;
        }
        Ok(())
    }

    fn ensure_membership_indexes(&self) -> Result<bool, ServiceError> {
        let _write = self.write_guard();
        let mut changed = false;
        for file in std::fs::read_dir(self.inner.root.join("projections"))? {
            let file = file?;
            if file.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let report: PublicParseReport = read_json(&file.path())?;
            let path = self.membership_path(&report.report_id)?;
            let current = path
                .is_file()
                .then(|| read_json::<PrivateParseMembership>(&path).ok())
                .flatten();
            let valid = current.as_ref().is_some_and(|membership| {
                membership.schema_version == PRIVATE_PARSE_MEMBERSHIP_SCHEMA_VERSION
                    && membership.report_id == report.report_id
                    && membership.artifact_sha256 == report.verification.artifact_sha256
            });
            if valid {
                continue;
            }
            let digest = Sha256Digest::parse(report.verification.artifact_sha256.clone())?;
            let artifact_path = self.artifact_path(&digest)?;
            if !artifact_path.is_file() {
                // A submitted-by-me report remains discoverable from its
                // provenance, but participant access fails closed when the
                // sealed evidence needed to rebuild membership is absent.
                continue;
            }
            let membership = build_private_parse_membership(&artifact_path, &report)?;
            write_json_atomic(&path, &membership)?;
            changed = true;
        }
        Ok(changed)
    }

    fn read_membership(
        &self,
        report: &PublicParseReport,
    ) -> Result<PrivateParseMembership, ServiceError> {
        let membership: PrivateParseMembership =
            read_json(&self.membership_path(&report.report_id)?)?;
        if membership.schema_version != PRIVATE_PARSE_MEMBERSHIP_SCHEMA_VERSION
            || membership.report_id != report.report_id
            || membership.artifact_sha256 != report.verification.artifact_sha256
        {
            return Err(ServiceError::InvalidMembershipIndex);
        }
        Ok(membership)
    }

    fn rebuild_catalog_locked(&self) -> Result<(), ServiceError> {
        let mut grouped = BTreeMap::<String, CatalogRunGroup>::new();
        for file in std::fs::read_dir(self.inner.root.join("projections"))? {
            let file = file?;
            if file.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let report: PublicParseReport = read_json(&file.path())?;
            if report.visibility != ReportVisibility::Public {
                continue;
            }
            let membership_path = self.membership_path(&report.report_id)?;
            let membership = membership_path
                .is_file()
                .then(|| self.read_membership(&report))
                .transpose()?;
            for run in &report.runs {
                let entry = PublicParseCatalogEntry::from_report(&report, run);
                let quality = CanonicalSpineQuality::from_report(&report, run);
                let private_run_membership = membership.as_ref().and_then(|membership| {
                    membership
                        .runs
                        .iter()
                        .find(|membership| membership.run_index == run.run_index)
                });
                let reconciliation_source =
                    ReconciliationRunSource::from_report(&report, run, private_run_membership);
                let milestone_source = MilestoneSource {
                    entry: entry.clone(),
                    authoritative_completion: run.authoritative_completion,
                    participants: run.participants.clone(),
                };
                let submitter = report.submission_provenance.submitter_id.clone();
                match grouped.entry(entry.run_group_id.clone()) {
                    std::collections::btree_map::Entry::Vacant(vacant) => {
                        let mut submitters = BTreeSet::new();
                        if let Some(submitter) = submitter {
                            submitters.insert(submitter);
                        }
                        let local_profile_witnesses =
                            run.local_profile_character_ids.iter().cloned().collect();
                        vacant.insert(CatalogRunGroup {
                            representative: entry,
                            representative_quality: quality,
                            submitters,
                            local_profile_witnesses,
                            reconciliation_sources: vec![reconciliation_source],
                            milestone_source,
                        });
                    }
                    std::collections::btree_map::Entry::Occupied(mut occupied) => {
                        let group = occupied.get_mut();
                        let mut report_ids = group.representative.report_ids.clone();
                        report_ids.push(report.report_id.clone());
                        report_ids.sort();
                        report_ids.dedup();
                        if let Some(submitter) = submitter {
                            group.submitters.insert(submitter);
                        }
                        group
                            .local_profile_witnesses
                            .extend(run.local_profile_character_ids.iter().cloned());
                        group.reconciliation_sources.push(reconciliation_source);
                        if quality.is_better_than(&group.representative_quality) {
                            group.representative = entry;
                            group.representative_quality = quality;
                            group.milestone_source = milestone_source;
                        }
                        group.representative.report_ids = report_ids;
                        group.representative.contribution_count =
                            group.representative.report_ids.len();
                        group.representative.distinct_submitter_count = group.submitters.len();
                    }
                }
            }
            if grouped.len() > MAXIMUM_CATALOG_ENTRIES {
                return Err(ServiceError::CatalogTooLarge);
            }
        }
        let mut entries = Vec::with_capacity(grouped.len());
        let mut milestone_sources = Vec::with_capacity(grouped.len());
        for group in grouped.into_values() {
            let mut reconciliation = build_public_reconciliation(&group);
            let replay_ready = matches!(
                reconciliation.state_replay_readiness,
                CrossVantageStateReplayReadiness::PartialCoverageReady
                    | CrossVantageStateReplayReadiness::FullCoverageReady
            );
            let artifacts_available = reconciliation.reports.iter().all(|report| {
                Sha256Digest::parse(report.artifact_sha256.clone())
                    .ok()
                    .and_then(|digest| self.artifact_path(&digest).ok())
                    .is_some_and(|path| path.is_file())
            });
            if replay_ready && artifacts_available {
                match self.load_verified_cross_vantage_state_events(&reconciliation) {
                    Ok(events) => {
                        reconciliation.verified_state_input_sha256 =
                            Some(verified_state_input_digest(&reconciliation, &events)?);
                        match self.replay_cross_vantage_attribution(&reconciliation, events) {
                            Ok(result) => {
                                reconciliation.status =
                                    RunAttributionReconciliationStatus::Reconciled;
                                reconciliation.reconciled_participants = result.participants;
                                reconciliation.conservation = Some(result.conservation);
                                reconciliation.rdps_effects = result.rdps_effects;
                                reconciliation.rdps_influences = result.rdps_influences;
                                reconciliation.swift_vortex_candidate_audit =
                                    result.swift_vortex_candidate_audit;
                                reconciliation.attribution_replay_completed = true;
                            }
                            Err(error) => {
                                reconciliation.state_replay_readiness =
                                    CrossVantageStateReplayReadiness::Blocked;
                                reconciliation
                                    .state_replay_blockers
                                    .push(format!("conserved_replay_failed:{error}"));
                            }
                        }
                    }
                    Err(error) => {
                        reconciliation.state_replay_readiness =
                            CrossVantageStateReplayReadiness::Blocked;
                        reconciliation
                            .state_replay_blockers
                            .push(format!("sealed_witness_verification_failed:{error}"));
                    }
                }
            }
            write_json_atomic(
                &self.reconciliation_path(&reconciliation.run_group_id)?,
                &reconciliation,
            )?;
            let mut entry = group.representative;
            entry.distinct_submitter_count = group.submitters.len();
            entry.local_profile_witness_character_count = group.local_profile_witnesses.len();
            entry.attribution_reconciliation_status = reconciliation.status;
            entries.push(entry);
            milestone_sources.push(group.milestone_source);
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_unix_millis));
        let facets = CatalogFacets::from_entries(&entries);
        write_json_atomic(
            &self.catalog_path(),
            &PublicParseCatalog {
                schema_version: PUBLIC_CATALOG_SCHEMA_VERSION,
                total_entries: entries.len(),
                offset: 0,
                next_offset: None,
                entries,
                facets,
            },
        )?;
        let observed_characters = build_observed_character_catalog(
            &milestone_sources,
            &self.inner.profiles.catalog(None)?.profiles,
        );
        std::fs::create_dir_all(self.inner.root.join("characters"))?;
        write_json_atomic(
            &self.observed_character_catalog_path(),
            &observed_characters,
        )?;
        write_json_atomic(
            &self.community_milestone_catalog_path(),
            &build_community_milestone_catalog(milestone_sources),
        )
    }

    fn report_id_for_digest(&self, digest: &Sha256Digest) -> Result<Option<String>, ServiceError> {
        let id = report_id(digest);
        Ok(self.projection_path(&id)?.exists().then_some(id))
    }

    fn load_manifest(&self, upload_id: &str) -> Result<UploadManifest, ServiceError> {
        read_json(&self.upload_directory(upload_id)?.join("manifest.json"))
    }

    fn assemble_upload(
        &self,
        upload_id: &str,
        manifest: &UploadManifest,
    ) -> Result<PathBuf, ServiceError> {
        let directory = self.upload_directory(upload_id)?;
        let partial = directory.join(format!(
            "assembled-{}.partial.rlog",
            Uuid::new_v4().simple()
        ));
        let mut output = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)?,
        );
        let mut file_hasher = Sha256::new();
        for descriptor in &manifest.chunks {
            let bytes = std::fs::read(self.chunk_path(upload_id, descriptor.sequence)?).map_err(
                |error| match error.kind() {
                    std::io::ErrorKind::NotFound => ServiceError::MissingChunk(descriptor.sequence),
                    _ => ServiceError::Io(error),
                },
            )?;
            let actual_length =
                u64::try_from(bytes.len()).map_err(|_| ServiceError::SizeOverflow)?;
            if actual_length != descriptor.byte_length || digest_bytes(&bytes)? != descriptor.sha256
            {
                return Err(ServiceError::ChunkVerificationFailed(descriptor.sequence));
            }
            file_hasher.update(&bytes);
            output.write_all(&bytes)?;
        }
        output.flush()?;
        drop(output);
        let actual = Sha256Digest::parse(format!("{:x}", file_hasher.finalize()))?;
        let expected = manifest
            .sealed_log_digest
            .as_ref()
            .ok_or(ServiceError::MissingSealedDigest)?;
        if &actual != expected {
            let _ = std::fs::remove_file(&partial);
            return Err(ServiceError::SealedDigestMismatch);
        }
        Ok(partial)
    }

    fn catalog_path(&self) -> PathBuf {
        self.inner.root.join("catalog.v1.json")
    }

    fn community_milestone_catalog_path(&self) -> PathBuf {
        self.inner.root.join("community-milestones.v1.json")
    }

    fn observed_character_catalog_path(&self) -> PathBuf {
        self.inner.root.join("characters").join("catalog.v1.json")
    }

    fn upload_directory(&self, upload_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(upload_id, "upload ID")?;
        Ok(self.inner.root.join("uploads").join(upload_id))
    }

    fn chunk_path(&self, upload_id: &str, sequence: u64) -> Result<PathBuf, ServiceError> {
        Ok(self
            .upload_directory(upload_id)?
            .join(format!("chunk-{sequence:08}.bin")))
    }

    fn projection_path(&self, report_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        Ok(self
            .inner
            .root
            .join("projections")
            .join(format!("{report_id}.json")))
    }

    fn membership_path(&self, report_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        Ok(self
            .inner
            .root
            .join("memberships")
            .join(format!("{report_id}.json")))
    }

    fn artifact_path(&self, digest: &Sha256Digest) -> Result<PathBuf, ServiceError> {
        let value = digest.as_str();
        Ok(self
            .inner
            .root
            .join("artifacts/sha256")
            .join(&value[..2])
            .join(format!("{value}.rlog")))
    }

    fn reconciliation_path(&self, run_group_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(run_group_id, "run group ID")?;
        Ok(self
            .inner
            .root
            .join("reconciliations")
            .join(format!("{run_group_id}.json")))
    }
}

fn verified_report_visibility(
    requested: ReportVisibility,
    publish_verified_parses: bool,
) -> ReportVisibility {
    if publish_verified_parses {
        ReportVisibility::Public
    } else {
        requested
    }
}

pub fn router(service: SubmissionService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/auth/config", get(auth_config))
        .route("/v1/auth/discord/start", get(begin_discord_auth))
        .route("/v1/auth/discord/callback", get(complete_discord_auth))
        .route(
            "/v1/auth/discord/complete",
            post(complete_discord_auth_from_site),
        )
        .route("/v1/auth/session/exchange", post(exchange_auth_code))
        .route("/v1/auth/me", get(get_account).patch(update_account))
        .route(
            "/v1/auth/me/parse-publication",
            patch(update_parse_publication_preference),
        )
        .route("/v1/auth/device", get(get_device_account))
        .route("/v1/auth/profiles", get(get_account_profiles))
        .route("/v1/auth/parses", get(get_account_parses))
        .route("/v1/auth/parses/{report_id}", get(get_account_parse))
        .route(
            "/v1/auth/parses/{report_id}/visibility",
            patch(update_account_parse_visibility),
        )
        .route("/v1/auth/app-tokens", post(issue_app_token))
        .route("/v1/uploads", post(begin_upload))
        .route(
            "/v1/uploads/{upload_id}/chunks/{sequence}",
            put(receive_chunk),
        )
        .route("/v1/uploads/{upload_id}/finalize", post(finalize_upload))
        .route("/v1/parses", get(list_parses))
        .route("/v1/activity/milestones", get(list_community_milestones))
        .route("/v1/characters", get(list_observed_characters))
        .route("/v1/parses/{report_id}", get(get_parse))
        .route(
            "/v1/games/blue-protocol-star-resonance/profiles",
            post(publish_bpsr_profile),
        )
        .route(
            "/v1/games/blue-protocol-star-resonance/profiles/{profile_id}/photo-wall/{photo_id}",
            put(publish_bpsr_profile_photo),
        )
        .route("/v1/profiles", get(list_profiles))
        .route("/v1/photos", get(list_profile_photos))
        .route("/v1/profiles/{profile_id}", get(get_profile))
        .route(
            "/v1/profiles/{profile_id}/loadouts/{project_id}",
            get(get_profile_loadout),
        )
        .route("/v1/users/{account_id}", get(get_public_account))
        .route(
            "/v1/profiles/{profile_id}/photo-wall/{photo_id}",
            get(get_profile_photo),
        )
        .route(
            "/v1/profiles/{profile_id}/photo-wall/{photo_id}/like",
            put(like_profile_photo).delete(unlike_profile_photo),
        )
        .route(
            "/v1/run-groups/{run_group_id}/reconciliation",
            get(get_run_reconciliation),
        )
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(service)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok","service":"rlogs-submissions","schema_version":1}))
}

async fn auth_config(State(service): State<SubmissionService>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "schema_version": 1,
        "discord_enabled": service.inner.accounts.configured(),
        "desktop_authentication": "bearer_app_token"
    }))
}

async fn begin_discord_auth(
    State(service): State<SubmissionService>,
) -> Result<Redirect, ApiError> {
    let url = service.inner.accounts.begin_discord_login(unix_millis()?)?;
    Ok(Redirect::temporary(&url))
}

async fn complete_discord_auth(
    State(service): State<SubmissionService>,
    Query(query): Query<DiscordCallbackQuery>,
) -> Result<Redirect, ApiError> {
    let url = service
        .inner
        .accounts
        .complete_discord_login(&query.code, &query.state, unix_millis()?)
        .await?;
    Ok(Redirect::temporary(&url))
}

async fn complete_discord_auth_from_site(
    State(service): State<SubmissionService>,
    Json(request): Json<DiscordCallbackQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let login_code = service
        .inner
        .accounts
        .complete_discord_login_code(&request.code, &request.state, unix_millis()?)
        .await?;
    Ok(Json(serde_json::json!({
        "schema_version": 1,
        "login_code": login_code
    })))
}

async fn exchange_auth_code(
    State(service): State<SubmissionService>,
    Json(request): Json<LoginCodeExchangeRequest>,
) -> Result<Json<WebSessionReceipt>, ApiError> {
    Ok(Json(
        service
            .inner
            .accounts
            .exchange_login_code(&request.code, unix_millis()?)?,
    ))
}

async fn get_account(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
) -> Result<Json<AccountView>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    Ok(Json(
        service
            .inner
            .accounts
            .authenticate_web(token, unix_millis()?)?,
    ))
}

async fn update_account(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<AccountView>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    Ok(Json(service.inner.accounts.update_username(
        token,
        &request.username,
        unix_millis()?,
    )?))
}

async fn update_parse_publication_preference(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Json(request): Json<UpdateParsePublicationPreferenceRequest>,
) -> Result<Json<AccountView>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    Ok(Json(
        service.inner.accounts.update_publish_verified_parses(
            token,
            request.publish_verified_parses,
            unix_millis()?,
        )?,
    ))
}

async fn get_device_account(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = bearer_token(&headers)
        .filter(|token| token.starts_with("rld_"))
        .ok_or(ApiError::Unauthorized)?;
    let identity =
        service
            .inner
            .accounts
            .authenticate_device(token)
            .map_err(|error| match error {
                AccountError::NotConfigured | AccountError::Unauthorized => ApiError::Unauthorized,
                other => ApiError::Account(other),
            })?;
    Ok(Json(serde_json::json!({
        "schema_version": 1,
        "submitter_id": identity.submitter_id,
        "device_id": identity.device_id,
        "authentication": "device_token"
    })))
}

async fn issue_app_token(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
) -> Result<Json<AppTokenReceipt>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    Ok(Json(
        service
            .inner
            .accounts
            .issue_device_token(token, unix_millis()?)?,
    ))
}

async fn get_account_profiles(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
) -> Result<Json<PublicProfileCatalog>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let account = service
        .inner
        .accounts
        .authenticate_web(token, unix_millis()?)?;
    Ok(Json(service.owned_profile_catalog(&account.submitter_id)?))
}

async fn get_public_account(
    State(service): State<SubmissionService>,
    AxumPath(account_id): AxumPath<u64>,
) -> Result<Json<PublicAccountProfileCatalog>, ApiError> {
    let Some((submitter_id, account)) = service.inner.accounts.public_identity(account_id)? else {
        return Err(ServiceError::NotFound.into());
    };
    let profiles = service.owned_profile_catalog(&submitter_id)?;
    Ok(Json(PublicAccountProfileCatalog {
        schema_version: 1,
        account,
        profiles: profiles.profiles,
    }))
}

async fn get_account_parses(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Query(query): Query<MyParsesQuery>,
) -> Result<Json<MyParseCatalog>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let account = service
        .inner
        .accounts
        .authenticate_web(token, unix_millis()?)?;
    Ok(Json(
        service.my_parse_catalog(&account.submitter_id, &query)?,
    ))
}

async fn get_account_parse(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath(report_id): AxumPath<String>,
) -> Result<Json<PublicParseReport>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let account = service
        .inner
        .accounts
        .authenticate_web(token, unix_millis()?)?;
    Ok(Json(
        service.account_report(&report_id, &account.submitter_id)?,
    ))
}

async fn update_account_parse_visibility(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath(report_id): AxumPath<String>,
    Json(request): Json<UpdateParseVisibilityRequest>,
) -> Result<Json<UpdateParseVisibilityResponse>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let account = service
        .inner
        .accounts
        .authenticate_web(token, unix_millis()?)?;
    Ok(Json(service.update_report_visibility(
        &report_id,
        &account.submitter_id,
        request.visibility,
    )?))
}

async fn begin_upload(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Json(manifest): Json<UploadManifest>,
) -> Result<Json<BeginUploadResponse>, ApiError> {
    let owner = authorize(&service, &headers).await?;
    Ok(Json(service.begin_upload(manifest, &owner)?))
}

async fn receive_chunk(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath((upload_id, sequence)): AxumPath<(String, u64)>,
    bytes: Bytes,
) -> Result<Json<ChunkUploadResponse>, ApiError> {
    let owner = authorize(&service, &headers).await?;
    Ok(Json(
        service.receive_chunk(&upload_id, sequence, &bytes, &owner)?,
    ))
}

async fn finalize_upload(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath(upload_id): AxumPath<String>,
) -> Result<Json<FinalizeUploadResponse>, ApiError> {
    let owner = authorize(&service, &headers).await?;
    Ok(Json(service.finalize_upload(&upload_id, &owner)?))
}

async fn list_parses(
    State(service): State<SubmissionService>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<PublicParseCatalog>, ApiError> {
    Ok(Json(service.catalog(&query)?))
}

async fn list_community_milestones(
    State(service): State<SubmissionService>,
    Query(query): Query<CommunityMilestoneQuery>,
) -> Result<Json<PublicCommunityMilestoneCatalog>, ApiError> {
    Ok(Json(service.community_milestones(&query)?))
}

async fn list_observed_characters(
    State(service): State<SubmissionService>,
) -> Result<Json<PublicObservedCharacterCatalog>, ApiError> {
    Ok(Json(service.observed_characters()?))
}

async fn get_parse(
    State(service): State<SubmissionService>,
    AxumPath(report_id): AxumPath<String>,
) -> Result<Json<PublicParseReport>, ApiError> {
    Ok(Json(service.report(&report_id)?))
}

async fn publish_bpsr_profile(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Json(package): Json<LocalProfilePackage>,
) -> Result<Json<ProfilePublishReceipt>, ApiError> {
    let device_token = bearer_token(&headers)
        .filter(|token| token.starts_with("rld_"))
        .ok_or(ApiError::Unauthorized)?
        .to_owned();
    let owner = authorize(&service, &headers).await?;
    if owner.submitter_id.is_none() || owner.device_id.is_none() {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(service.publish_profile(
        package,
        &owner,
        &device_token,
    )?))
}

async fn publish_bpsr_profile_photo(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath((profile_id, photo_id)): AxumPath<(String, u32)>,
    bytes: Bytes,
) -> Result<Json<PhotoAssetReceipt>, ApiError> {
    let owner = authorize(&service, &headers).await?;
    if owner.submitter_id.is_none() || owner.device_id.is_none() {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(service.publish_profile_photo(
        &profile_id,
        photo_id,
        &bytes,
        &owner,
    )?))
}

async fn list_profiles(
    State(service): State<SubmissionService>,
    Query(query): Query<ProfileCatalogQuery>,
) -> Result<Json<PublicProfileCatalog>, ApiError> {
    Ok(Json(service.profile_catalog(&query)?))
}

async fn list_profile_photos(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    Query(query): Query<PhotoCatalogQuery>,
) -> Result<Json<PublicPhotoCatalog>, ApiError> {
    let viewer = if let Some(token) = bearer_token(&headers) {
        Some(
            service
                .inner
                .accounts
                .authenticate_web(token, unix_millis()?)?,
        )
    } else {
        None
    };
    Ok(Json(service.photo_catalog(
        &query,
        viewer.as_ref().map(|account| account.submitter_id.as_str()),
    )?))
}

async fn get_profile(
    State(service): State<SubmissionService>,
    AxumPath(profile_id): AxumPath<String>,
) -> Result<Json<PublicProfile>, ApiError> {
    Ok(Json(service.profile(&profile_id)?))
}

async fn get_profile_loadout(
    State(service): State<SubmissionService>,
    AxumPath((profile_id, project_id)): AxumPath<(String, i32)>,
) -> Result<Json<PublicProfileLoadout>, ApiError> {
    Ok(Json(service.profile_loadout(&profile_id, project_id)?))
}

async fn get_profile_photo(
    State(service): State<SubmissionService>,
    AxumPath((profile_id, photo_id)): AxumPath<(String, u32)>,
) -> Result<Response, ApiError> {
    let asset = service.profile_photo(&profile_id, photo_id)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, asset.media_type)
        .header(CACHE_CONTROL, "public, max-age=300")
        .header(ETAG, format!("\"{}\"", asset.sha256))
        .body(Body::from(asset.bytes))
        .map_err(|_| {
            ApiError::Service(ServiceError::InvalidConfiguration(
                "could not build Photo Wall response".into(),
            ))
        })
}

async fn like_profile_photo(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath((profile_id, photo_id)): AxumPath<(String, u32)>,
) -> Result<Json<PhotoLikeReceipt>, ApiError> {
    set_profile_photo_like(service, headers, profile_id, photo_id, true).await
}

async fn unlike_profile_photo(
    State(service): State<SubmissionService>,
    headers: HeaderMap,
    AxumPath((profile_id, photo_id)): AxumPath<(String, u32)>,
) -> Result<Json<PhotoLikeReceipt>, ApiError> {
    set_profile_photo_like(service, headers, profile_id, photo_id, false).await
}

async fn set_profile_photo_like(
    service: SubmissionService,
    headers: HeaderMap,
    profile_id: String,
    photo_id: u32,
    liked: bool,
) -> Result<Json<PhotoLikeReceipt>, ApiError> {
    let token = bearer_token(&headers).ok_or(ApiError::Unauthorized)?;
    let account = service
        .inner
        .accounts
        .authenticate_web(token, unix_millis()?)?;
    Ok(Json(service.set_profile_photo_like(
        &profile_id,
        photo_id,
        &account.submitter_id,
        liked,
    )?))
}

async fn get_run_reconciliation(
    State(service): State<SubmissionService>,
    AxumPath(run_group_id): AxumPath<String>,
) -> Result<Json<PublicRunReconciliation>, ApiError> {
    Ok(Json(service.reconciliation(&run_group_id)?))
}

async fn authorize(
    service: &SubmissionService,
    headers: &HeaderMap,
) -> Result<UploadOwner, ApiError> {
    if let Some(token) = bearer_token(headers).filter(|token| token.starts_with("rld_")) {
        let identity = service
            .inner
            .accounts
            .authenticate_device(token)
            .map_err(|error| match error {
                AccountError::NotConfigured | AccountError::Unauthorized => ApiError::Unauthorized,
                other => ApiError::Account(other),
            })?;
        return Ok(UploadOwner {
            schema_version: UPLOAD_OWNER_SCHEMA_VERSION,
            submitter_id: Some(identity.submitter_id),
            device_id: Some(identity.device_id),
            authentication: "device_token".into(),
        });
    }
    match &service.inner.authentication {
        SubmissionAuthentication::UnauthenticatedDevelopment => Ok(UploadOwner {
            schema_version: UPLOAD_OWNER_SCHEMA_VERSION,
            submitter_id: None,
            device_id: None,
            authentication: "unauthenticated_development".into(),
        }),
        SubmissionAuthentication::SharedIngestKey { key, submitter_id } => {
            let actual = bearer_token(headers).ok_or(ApiError::Unauthorized)?;
            if !constant_time_equal(actual.as_bytes(), key.as_bytes()) {
                return Err(ApiError::Unauthorized);
            }
            Ok(UploadOwner {
                schema_version: UPLOAD_OWNER_SCHEMA_VERSION,
                submitter_id: Some(submitter_id.clone()),
                device_id: Some(pseudonymous_identifier("device", key.as_bytes())),
                authentication: "shared_ingest_key".into(),
            })
        }
        SubmissionAuthentication::Introspection { endpoint, client } => {
            let token = bearer_token(headers).ok_or(ApiError::Unauthorized)?;
            let response = client
                .post(endpoint.clone())
                .bearer_auth(token)
                .send()
                .await
                .map_err(|_| ApiError::AuthenticationUnavailable)?;
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return Err(ApiError::Unauthorized);
            }
            if !response.status().is_success() {
                return Err(ApiError::AuthenticationUnavailable);
            }
            let identity: IntrospectionResponse = response
                .json()
                .await
                .map_err(|_| ApiError::AuthenticationUnavailable)?;
            if identity.schema_version != AUTH_INTROSPECTION_SCHEMA_VERSION
                || !identity.active
                || identity.authentication != "device_token"
            {
                return Err(ApiError::Unauthorized);
            }
            let submitter_id = identity.submitter_id.ok_or(ApiError::Unauthorized)?;
            let device_id = identity.device_id.ok_or(ApiError::Unauthorized)?;
            validate_identity_value(&submitter_id)
                .map_err(|_| ApiError::AuthenticationUnavailable)?;
            validate_identity_value(&device_id).map_err(|_| ApiError::AuthenticationUnavailable)?;
            Ok(UploadOwner {
                schema_version: UPLOAD_OWNER_SCHEMA_VERSION,
                submitter_id: Some(submitter_id),
                device_id: Some(device_id),
                authentication: identity.authentication,
            })
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn validate_identity_value(value: &str) -> Result<(), ServiceError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(ServiceError::InvalidAuthenticationIdentity);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct IntrospectionResponse {
    schema_version: u16,
    active: bool,
    submitter_id: Option<String>,
    device_id: Option<String>,
    authentication: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscordCallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginCodeExchangeRequest {
    code: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateAccountRequest {
    username: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateParsePublicationPreferenceRequest {
    publish_verified_parses: bool,
}

#[derive(Debug, Serialize)]
struct PublicAccountProfileCatalog {
    schema_version: u16,
    account: PublicAccountIdentity,
    profiles: Vec<PublicProfileCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UploadOwner {
    schema_version: u16,
    submitter_id: Option<String>,
    device_id: Option<String>,
    authentication: String,
}

impl UploadOwner {
    fn public_provenance(&self) -> PublicSubmissionProvenance {
        PublicSubmissionProvenance {
            submitter_id: self.submitter_id.clone(),
            authentication: self.authentication.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BeginUploadResponse {
    pub schema_version: u16,
    pub upload_id: Option<String>,
    pub missing_chunks: Vec<u64>,
    pub existing_report_id: Option<String>,
    pub share_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChunkUploadResponse {
    pub schema_version: u16,
    pub sequence: u64,
    pub sha256: Sha256Digest,
    pub duplicate: bool,
}

#[derive(Debug, Serialize)]
pub struct FinalizeUploadResponse {
    pub schema_version: u16,
    pub report_id: String,
    pub accepted_log_digest: Sha256Digest,
    pub verification_tier: VerificationTier,
    pub share_url: String,
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicParseReport {
    pub schema_version: u16,
    /// Revision of the replay/projection semantics used to derive this report.
    /// This advances independently from the JSON contract schema so stored
    /// reports can be rebuilt after correctness fixes without client churn.
    #[serde(default)]
    pub projection_revision: u16,
    pub report_id: String,
    pub visibility: ReportVisibility,
    pub created_unix_millis: u64,
    pub game_plugin_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub verification: PublicVerification,
    #[serde(default)]
    pub submission_provenance: PublicSubmissionProvenance,
    pub runs: Vec<PublicRun>,
}

fn report_projection_is_current(report: &PublicParseReport) -> bool {
    report.schema_version >= PUBLIC_PARSE_SCHEMA_VERSION
        && report.projection_revision >= PUBLIC_PARSE_PROJECTION_REVISION
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProjectionRefreshSummary {
    pub inspected: usize,
    pub refreshed: usize,
    pub already_current: usize,
}

/// Identifies the uploader independently from every character in the log.
/// Authentication providers return a submitter identity, while the receiver
/// deliberately keeps device identity in its private in-progress upload state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSubmissionProvenance {
    pub submitter_id: Option<String>,
    pub authentication: String,
}

impl Default for PublicSubmissionProvenance {
    fn default() -> Self {
        Self {
            submitter_id: None,
            authentication: "legacy_unknown".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicVerification {
    pub tier: VerificationTier,
    pub artifact_sha256: String,
    pub canonical_content_sha256: String,
    pub event_count: u64,
    pub privacy_policy_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRun {
    pub run_index: u32,
    #[serde(default)]
    pub run_group_id: String,
    #[serde(default)]
    pub correlation_method: RunCorrelationMethod,
    pub activity_id: Option<String>,
    pub activity_family_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_category_id: Option<String>,
    pub scene_id: Option<i32>,
    pub scene_name: Option<String>,
    pub difficulty_family: Option<String>,
    pub difficulty_tier: Option<u32>,
    pub terminal_state: String,
    pub total_run_time_micros: Option<u64>,
    pub game_time_micros: Option<u64>,
    pub active_combat_micros: u64,
    pub true_time_micros: Option<u64>,
    pub retry_count: u32,
    pub boss_retry_count: u32,
    pub rdps_status: String,
    pub data_gap_count: u64,
    pub authoritative_start: bool,
    pub authoritative_completion: bool,
    pub submission_disposition: RunSubmissionDisposition,
    /// Stable characters for which this observer's artifact contains a
    /// privacy-reviewed local profile witness. A same-instance run group may
    /// use these witnesses to replace another report's remote inference, but
    /// never to duplicate that report's combat events.
    #[serde(default)]
    pub local_profile_character_ids: Vec<String>,
    /// Run-scoped, personal-gameplay profile snapshots. The digest commits to
    /// the exact trusted plug-in payload without publishing that payload in
    /// the parse. Cross-vantage replay uses these references to retrieve and
    /// verify the corresponding event from the private sealed artifact.
    #[serde(default)]
    pub local_profile_witnesses: Vec<PublicLocalProfileWitness>,
    /// Exact state events for the character proven local by a personal profile
    /// observation. These are commitments and provenance only; values stay in
    /// the private sealed artifact until a joint replay verifies and consumes
    /// the referenced canonical event.
    #[serde(default)]
    pub local_state_witnesses: Vec<PublicLocalStateWitness>,
    /// Human-readable, privacy-reviewed combat loadout phases. These are
    /// emitted only from profile snapshots observed after combat began and
    /// through the authoritative run completion, so lobby configuration can
    /// never backfill a parse.
    #[serde(default)]
    pub combat_loadout_phases: Vec<PublicCombatLoadoutPhase>,
    pub segments: Vec<PublicRunSegment>,
    pub participants: Vec<PublicParticipant>,
    /// Exact packet-derived provider/recipient relationships for this replay.
    /// The rows retain decimal strings and reduced fractions so browsers never
    /// lose precision while explaining an rDPS total.
    #[serde(default)]
    pub rdps_influences: Vec<PublicRdpsInfluence>,
    #[serde(default)]
    pub rdps_effects: Vec<PublicRdpsEffectPresentation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCombatLoadoutPhase {
    pub character_id: String,
    pub display_name: Option<String>,
    pub observed_micros: u64,
    pub run_elapsed_micros: u64,
    pub game_time_millis: Option<i64>,
    pub segment_index: Option<u32>,
    pub encounter_index: Option<u32>,
    pub attempt_number: Option<u32>,
    pub in_active_combat: bool,
    pub class_id: Option<i32>,
    pub class_name: Option<String>,
    pub specialization_id: Option<i32>,
    pub specialization_name: Option<String>,
    #[serde(default)]
    pub equipped_skill_ids: Vec<String>,
    #[serde(default)]
    pub equipped_imagines: Vec<PublicCombatImagineLoadout>,
    pub equipment_count: Option<usize>,
    pub equipped_module_count: Option<usize>,
    pub talent_count: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicCombatImagineLoadout {
    pub skill_id: String,
    pub tier: Option<u32>,
    pub equipped_slot: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicLocalProfileWitness {
    pub character_id: String,
    #[serde(default)]
    pub placement: LocalStateWitnessPlacement,
    pub event_sequence: u64,
    pub observed_micros: u64,
    #[serde(default)]
    pub game_time_millis: Option<i64>,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStateWitnessKind {
    EntityAttributes,
    TemporaryAttributes,
    /// Exact local combat-resource snapshots/deltas. These remain committed
    /// private-artifact evidence until a matched cross-vantage replay verifies
    /// and imports them; clients never submit a derived opportunity claim.
    Resource,
    LifeWaveTriggerStatus,
    LifeWaveTriggerHealing,
    StatResonanceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicLocalStateWitness {
    pub character_id: String,
    /// Stable source character for a directional evidence row such as the
    /// healing half of a Life Wave trigger pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_character_id: Option<String>,
    pub actor_id: u64,
    pub entity_uuid: i64,
    pub kind: LocalStateWitnessKind,
    pub update_kind: String,
    #[serde(default)]
    pub placement: LocalStateWitnessPlacement,
    pub event_sequence: u64,
    pub observed_micros: u64,
    pub game_time_millis: Option<i64>,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStateWitnessPlacement {
    #[default]
    Unspecified,
    PreRunBaseline,
    InRun,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCorrelationMethod {
    ExactInstanceId,
    #[default]
    IsolatedArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRunSegment {
    pub index: u32,
    pub kind: RunSegmentKind,
    pub wall_time_micros: u64,
    pub active_combat_micros: u64,
    pub attempt_count: u32,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicParticipant {
    pub actor_id: String,
    pub character_id: Option<String>,
    /// Stable, non-reversible identity derived by the verifier from the sealed
    /// character UID. This links the same observed player across public parses
    /// without exposing a remote participant's private UID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_character_key: Option<String>,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    pub class_id: Option<i32>,
    pub class_name: Option<String>,
    pub specialization_id: Option<i32>,
    pub specialization_name: Option<String>,
    pub damage: i64,
    pub dps: f64,
    pub encounter_dps: f64,
    pub hps: f64,
    pub tps: f64,
    pub rdps: Option<f64>,
    pub deaths: u64,
    #[serde(default)]
    pub death_seconds: Vec<u32>,
    #[serde(default)]
    pub abilities: Vec<PublicAbilitySummary>,
    /// Sparse one-second points. Missing seconds are zero and do not increase
    /// the public projection or Cloudflare transfer size.
    #[serde(default)]
    pub series: Vec<PublicSeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicAbilitySummary {
    pub ability_id: String,
    pub presentation_name: Option<String>,
    pub presentation_kind: Option<String>,
    pub icon_asset_path: Option<String>,
    /// Stable game presentation family used to associate an action request
    /// with the exact child damage rows it produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_recount_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_recount_group_name: Option<String>,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub damage: i64,
    pub effective_damage: i64,
    pub healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSeriesPoint {
    pub second: u32,
    pub damage: i64,
    pub effective_healing: i64,
    pub damage_taken: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRdpsEffectPresentation {
    pub effect_id: String,
    pub presentation_name: String,
    pub presentation_kind: String,
    pub icon_asset_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRationalDamageDelta {
    pub numerator: String,
    pub denominator: String,
    pub contribution_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRdpsInfluence {
    pub effect_id: String,
    pub attribution_component: Option<String>,
    pub complete_effect: bool,
    pub provider_actor_id: String,
    pub recipient_actor_id: String,
    pub affected_ability_id: Option<String>,
    pub target_actor_id: Option<String>,
    pub first_observed_micros: u64,
    pub last_observed_micros: u64,
    pub damage_event_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_hit_count: Option<u64>,
    pub observed_damage: String,
    pub exact_integer_delta: String,
    pub exact_rational_deltas: Vec<PublicRationalDamageDelta>,
    pub attributed_rdps: Option<String>,
    pub damage_context_complete: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogQuery {
    pub deployment: Option<String>,
    pub region: Option<String>,
    pub activity: Option<String>,
    pub scene: Option<i32>,
    pub difficulty: Option<String>,
    pub terminal: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunityMilestoneQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCommunityMilestoneCatalog {
    pub schema_version: u16,
    pub total_entries: usize,
    pub entries: Vec<PublicCommunityMilestone>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityMilestoneKind {
    MasterTwentyDungeon,
    NightmareRaid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCommunityMilestone {
    pub kind: CommunityMilestoneKind,
    pub character_id: String,
    pub display_name: Option<String>,
    pub report_id: String,
    pub run_index: u32,
    pub completed_unix_millis: u64,
    pub scene_id: Option<i32>,
    pub scene_name: Option<String>,
    pub difficulty_family: String,
    pub difficulty_tier: Option<u32>,
    pub total_run_time_micros: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MyParsesQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyParseCatalog {
    pub schema_version: u16,
    pub total_entries: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub claimed_character_ids: Vec<String>,
    pub entries: Vec<MyParseCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyParseCatalogEntry {
    #[serde(flatten)]
    pub parse: PublicParseCatalogEntry,
    pub visibility: ReportVisibility,
    pub submitted_by_you: bool,
    pub matched_character_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParseVisibilityRequest {
    pub visibility: ReportVisibility,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateParseVisibilityResponse {
    pub schema_version: u16,
    pub report_id: String,
    pub visibility: ReportVisibility,
    pub share_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProfileCatalogQuery {
    pub character_id: Option<String>,
}

impl CatalogQuery {
    fn matches(&self, entry: &PublicParseCatalogEntry) -> bool {
        optional_matches(&self.deployment, &entry.deployment_id)
            && optional_matches(&self.region, &entry.region_id)
            && self
                .activity
                .as_deref()
                .is_none_or(|value| catalog_activity_category(entry) == Some(value))
            && self.scene.is_none_or(|value| entry.scene_id == Some(value))
            && optional_matches_option(&self.difficulty, &entry.difficulty_family)
            && optional_matches(&self.terminal, &entry.terminal_state)
    }
}

fn public_activity_category_id(analysis: &RunAnalysis) -> Option<&'static str> {
    if analysis.identity.activity_family_id.as_deref() == Some("stimen-vaults") {
        return Some("stimens");
    }
    match analysis.identity.activity_kind {
        ActivityKind::Dungeon => Some("dungeons"),
        ActivityKind::Raid
            if analysis.identity.raid_route_kind == Some(RaidRouteKind::Gauntlet) =>
        {
            Some("gauntlets")
        }
        ActivityKind::Raid => Some("raids"),
        ActivityKind::Unknown => None,
    }
}

fn catalog_activity_category(entry: &PublicParseCatalogEntry) -> Option<&str> {
    entry.activity_category_id.as_deref().or_else(|| {
        // Compatibility for reports written before activity categories were
        // persisted. The only old public BPSR families were dungeon rules;
        // Stimen is the one category that must not inherit that default.
        if entry.activity_family_id.as_deref() == Some("stimen-vaults")
            || entry.activity_family_id.as_deref() == Some("stimen-remains")
            || entry
                .scene_id
                .is_some_and(|scene_id| (32_101..=32_160).contains(&scene_id))
        {
            Some("stimens")
        } else if entry.activity_family_id.is_some() {
            Some("dungeons")
        } else {
            None
        }
    })
}

fn build_observed_character_catalog(
    sources: &[MilestoneSource],
    profiles: &[PublicProfileCatalogEntry],
) -> PublicObservedCharacterCatalog {
    let mut profiles_by_uid = BTreeMap::<String, &PublicProfileCatalogEntry>::new();
    let mut profiles_by_key = BTreeMap::<String, &PublicProfileCatalogEntry>::new();
    let mut profiles_by_name = BTreeMap::<String, Vec<&PublicProfileCatalogEntry>>::new();
    for profile in profiles {
        profiles_by_uid.insert(profile.character_id.clone(), profile);
        profiles_by_key.insert(
            pseudonymous_identifier("chr", profile.character_id.as_bytes()),
            profile,
        );
        if let Some(name) = profile.display_name.as_deref() {
            profiles_by_name
                .entry(observed_name_identity(
                    &profile.deployment,
                    &profile.region,
                    name,
                ))
                .or_default()
                .push(profile);
        }
    }

    let mut characters = BTreeMap::<String, PublicObservedCharacter>::new();
    for source in sources {
        for participant in &source.participants {
            if participant.actor_kind.as_deref() != Some("player") {
                continue;
            }
            let Some(display_name) = participant
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let name_identity = observed_name_identity(
                &source.entry.deployment_id,
                &source.entry.region_id,
                display_name,
            );
            let unique_name_profile = profiles_by_name
                .get(&name_identity)
                .filter(|matches| matches.len() == 1)
                .map(|matches| matches[0]);
            let claimed = participant
                .character_id
                .as_ref()
                .and_then(|uid| profiles_by_uid.get(uid).copied())
                .or_else(|| {
                    participant
                        .observed_character_key
                        .as_ref()
                        .and_then(|key| profiles_by_key.get(key).copied())
                })
                .or(unique_name_profile);
            let character_id = participant
                .character_id
                .clone()
                .or_else(|| claimed.map(|profile| profile.character_id.clone()));
            let observed_character_key = participant
                .observed_character_key
                .clone()
                .or_else(|| {
                    character_id
                        .as_deref()
                        .map(|uid| pseudonymous_identifier("chr", uid.as_bytes()))
                })
                .unwrap_or_else(|| legacy_observed_character_key(&name_identity));
            let identity_kind =
                if character_id.is_some() || participant.observed_character_key.is_some() {
                    ObservedCharacterIdentityKind::VerifiedUid
                } else {
                    ObservedCharacterIdentityKind::LegacyNameObservation
                };
            let reference = PublicObservedCharacterReportReference {
                report_id: source.entry.report_id.clone(),
                run_index: source.entry.run_index,
                created_unix_millis: source.entry.created_unix_millis,
                scene_id: source.entry.scene_id,
                scene_name: source.entry.scene_name.clone(),
                terminal_state: source.entry.terminal_state.clone(),
            };
            let character = characters
                .entry(observed_character_key.clone())
                .or_insert_with(|| PublicObservedCharacter {
                    observed_character_key,
                    identity_kind,
                    character_id: character_id.clone(),
                    claimed_profile_id: claimed.map(|profile| profile.profile_id.clone()),
                    display_name: display_name.to_owned(),
                    deployment: source.entry.deployment_id.clone(),
                    region: source.entry.region_id.clone(),
                    class_id: participant.class_id,
                    class_name: participant.class_name.clone(),
                    specialization_id: participant.specialization_id,
                    specialization_name: participant.specialization_name.clone(),
                    first_seen_unix_millis: source.entry.created_unix_millis,
                    last_seen_unix_millis: source.entry.created_unix_millis,
                    report_count: 0,
                    reports: Vec::new(),
                });
            let latest = source.entry.created_unix_millis >= character.last_seen_unix_millis;
            character.first_seen_unix_millis = character
                .first_seen_unix_millis
                .min(source.entry.created_unix_millis);
            character.last_seen_unix_millis = character
                .last_seen_unix_millis
                .max(source.entry.created_unix_millis);
            if latest {
                character.display_name = display_name.to_owned();
                character.class_id = participant.class_id.or(character.class_id);
                character.class_name = participant
                    .class_name
                    .clone()
                    .or_else(|| character.class_name.clone());
                character.specialization_id = participant
                    .specialization_id
                    .or(character.specialization_id);
                character.specialization_name = participant
                    .specialization_name
                    .clone()
                    .or_else(|| character.specialization_name.clone());
            }
            if !character.reports.iter().any(|existing| {
                existing.report_id == reference.report_id
                    && existing.run_index == reference.run_index
            }) {
                character.reports.push(reference);
            }
        }
    }

    let mut characters = characters.into_values().collect::<Vec<_>>();
    for character in &mut characters {
        character
            .reports
            .sort_by_key(|reference| std::cmp::Reverse(reference.created_unix_millis));
        character.report_count = character.reports.len();
    }
    characters.sort_by(|left, right| {
        right
            .last_seen_unix_millis
            .cmp(&left.last_seen_unix_millis)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    PublicObservedCharacterCatalog {
        schema_version: 1,
        generated_unix_millis: characters
            .iter()
            .map(|character| character.last_seen_unix_millis)
            .max()
            .unwrap_or(1),
        total_characters: characters.len(),
        characters,
    }
}

fn observed_name_identity(deployment: &str, region: &str, display_name: &str) -> String {
    format!(
        "{}\0{}\0{}",
        deployment,
        region,
        display_name.to_lowercase()
    )
}

fn legacy_observed_character_key(name_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-observed-character-name-v1\0");
    hasher.update(name_identity.as_bytes());
    format!("obs_{:x}", hasher.finalize())[..36].to_owned()
}

fn build_community_milestone_catalog(
    sources: Vec<MilestoneSource>,
) -> PublicCommunityMilestoneCatalog {
    let mut firsts =
        BTreeMap::<(CommunityMilestoneKind, String, String), PublicCommunityMilestone>::new();
    for source in sources {
        if source.entry.terminal_state != "completed" || !source.authoritative_completion {
            continue;
        }
        let kind = match (
            catalog_activity_category(&source.entry),
            source.entry.difficulty_family.as_deref(),
            source.entry.difficulty_tier,
        ) {
            (Some("dungeons"), Some("master"), Some(tier)) if tier >= 20 => {
                CommunityMilestoneKind::MasterTwentyDungeon
            }
            (Some("raids"), Some("nightmare"), _) => CommunityMilestoneKind::NightmareRaid,
            _ => continue,
        };
        let scene_key = source
            .entry
            .scene_id
            .map(|scene_id| format!("scene:{scene_id}"))
            .or_else(|| {
                source
                    .entry
                    .activity_id
                    .as_ref()
                    .map(|activity_id| format!("activity:{activity_id}"))
            })
            .unwrap_or_else(|| "unknown".into());
        for participant in source.participants {
            let Some(character_id) = participant.character_id.filter(|value| !value.is_empty())
            else {
                continue;
            };
            let candidate = PublicCommunityMilestone {
                kind,
                character_id: character_id.clone(),
                display_name: participant.display_name,
                report_id: source.entry.report_id.clone(),
                run_index: source.entry.run_index,
                completed_unix_millis: source.entry.created_unix_millis,
                scene_id: source.entry.scene_id,
                scene_name: source.entry.scene_name.clone(),
                difficulty_family: source.entry.difficulty_family.clone().unwrap_or_default(),
                difficulty_tier: source.entry.difficulty_tier,
                total_run_time_micros: source.entry.total_run_time_micros,
            };
            let key = (kind, character_id, scene_key.clone());
            match firsts.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if candidate.completed_unix_millis < entry.get().completed_unix_millis =>
                {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
    }
    let mut entries = firsts.into_values().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .completed_unix_millis
            .cmp(&left.completed_unix_millis)
            .then_with(|| left.character_id.cmp(&right.character_id))
            .then_with(|| left.report_id.cmp(&right.report_id))
    });
    PublicCommunityMilestoneCatalog {
        schema_version: COMMUNITY_MILESTONE_CATALOG_SCHEMA_VERSION,
        total_entries: entries.len(),
        entries,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicParseCatalog {
    pub schema_version: u16,
    #[serde(default)]
    pub total_entries: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub next_offset: Option<usize>,
    pub entries: Vec<PublicParseCatalogEntry>,
    pub facets: CatalogFacets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicObservedCharacterCatalog {
    pub schema_version: u16,
    pub generated_unix_millis: u64,
    pub total_characters: usize,
    pub characters: Vec<PublicObservedCharacter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedCharacterIdentityKind {
    VerifiedUid,
    LegacyNameObservation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicObservedCharacter {
    pub observed_character_key: String,
    pub identity_kind: ObservedCharacterIdentityKind,
    pub character_id: Option<String>,
    pub claimed_profile_id: Option<String>,
    pub display_name: String,
    pub deployment: String,
    pub region: String,
    pub class_id: Option<i32>,
    pub class_name: Option<String>,
    pub specialization_id: Option<i32>,
    pub specialization_name: Option<String>,
    pub first_seen_unix_millis: u64,
    pub last_seen_unix_millis: u64,
    pub report_count: usize,
    pub reports: Vec<PublicObservedCharacterReportReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicObservedCharacterReportReference {
    pub report_id: String,
    pub run_index: u32,
    pub created_unix_millis: u64,
    pub scene_id: Option<i32>,
    pub scene_name: Option<String>,
    pub terminal_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicParseCatalogEntry {
    pub report_id: String,
    #[serde(default)]
    pub report_ids: Vec<String>,
    pub run_index: u32,
    #[serde(default)]
    pub run_group_id: String,
    #[serde(default = "one_usize")]
    pub contribution_count: usize,
    #[serde(default)]
    pub distinct_submitter_count: usize,
    #[serde(default)]
    pub local_profile_witness_character_count: usize,
    #[serde(default)]
    pub attribution_reconciliation_status: RunAttributionReconciliationStatus,
    pub created_unix_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitter_id: Option<String>,
    pub deployment_id: String,
    pub region_id: String,
    pub activity_id: Option<String>,
    pub activity_family_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_category_id: Option<String>,
    pub scene_id: Option<i32>,
    pub scene_name: Option<String>,
    pub difficulty_family: Option<String>,
    pub difficulty_tier: Option<u32>,
    pub terminal_state: String,
    pub total_run_time_micros: Option<u64>,
    pub participant_count: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAttributionReconciliationStatus {
    #[default]
    SingleVantage,
    MultipleReportsNoAdditionalVantage,
    CrossVantageEvidenceAvailable,
    Reconciled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRunReconciliation {
    pub schema_version: u16,
    pub reconciliation_id: String,
    pub run_group_id: String,
    pub status: RunAttributionReconciliationStatus,
    pub canonical_spine: PublicCanonicalSpine,
    pub reports: Vec<PublicReconciliationReport>,
    pub characters: Vec<PublicReconciliationCharacter>,
    pub participant_character_count: usize,
    pub local_vantage_character_count: usize,
    pub complete_local_vantage_coverage: bool,
    pub state_replay_readiness: CrossVantageStateReplayReadiness,
    pub state_replay_blockers: Vec<String>,
    /// Deterministic digest of the canonical artifact plus every exact state
    /// witness successfully re-read and verified from its sealed artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_state_input_sha256: Option<String>,
    /// Reconciled actor totals derived from one canonical combat spine. Source
    /// reports remain immutable and are never summed together.
    #[serde(default)]
    pub reconciled_participants: Vec<PublicReconciledParticipant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conservation: Option<PublicAttributionConservation>,
    /// These rows come from the conserved replay that consumed all verified
    /// cross-vantage state. They are empty until that replay completes.
    #[serde(default)]
    pub rdps_influences: Vec<PublicRdpsInfluence>,
    #[serde(default)]
    pub rdps_effects: Vec<PublicRdpsEffectPresentation>,
    /// Audit-only exact paired lifecycle evidence for the unpromoted Swift
    /// Vortex candidate. This can satisfy the magnitude review gate but can
    /// never enable production attribution by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swift_vortex_candidate_audit: Option<SwiftVortexCandidateAuditReport>,
    /// This product inventories and selects evidence. It does not claim that
    /// the conserved counterfactual replay has already consumed it.
    pub attribution_replay_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicReconciledParticipant {
    #[serde(flatten)]
    pub participant: PublicParticipant,
    pub rdps_damage: Option<i64>,
    pub contribution_given: Option<i64>,
    pub contribution_received: Option<i64>,
    pub rdps_incomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicAttributionConservation {
    pub raw_damage: i64,
    pub rdps_damage: i64,
    pub contribution_given: i64,
    pub contribution_received: i64,
    pub conserved: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossVantageStateReplayReadiness {
    #[default]
    SingleVantage,
    MultipleReportsNoAdditionalVantage,
    Blocked,
    PartialCoverageReady,
    FullCoverageReady,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCanonicalSpine {
    pub report_id: String,
    pub run_index: u32,
    pub artifact_sha256: String,
    pub authoritative_start: bool,
    pub authoritative_completion: bool,
    pub data_gap_count: u64,
    pub event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicReconciliationReport {
    pub report_id: String,
    pub run_index: u32,
    pub artifact_sha256: String,
    /// Decoder/protocol identity is replay compatibility evidence, not game
    /// run identity. Reports from the same exact game instance remain grouped
    /// when this differs, but their state cannot be mixed automatically.
    #[serde(default)]
    pub protocol_pack_digest: String,
    pub created_unix_millis: u64,
    pub canonical_spine: bool,
    pub local_profile_witnesses: Vec<PublicLocalProfileWitness>,
    pub local_state_witnesses: Vec<PublicLocalStateWitness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicReconciliationCharacter {
    pub character_id: String,
    pub participant_report_count: usize,
    pub disposition: ProfileWitnessDisposition,
    pub selected_report_id: Option<String>,
    pub state_witness_count: usize,
    pub game_time_aligned_state_witness_count: usize,
    pub witnesses: Vec<PublicCharacterWitnessSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCharacterWitnessSource {
    pub report_id: String,
    pub run_index: u32,
    pub artifact_sha256: String,
    pub snapshots: Vec<PublicLocalProfileWitness>,
    pub state_snapshots: Vec<PublicLocalStateWitness>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileWitnessDisposition {
    #[default]
    Missing,
    SingleReportExact,
    MultipleReportsIdentical,
    MultipleReportsRequireOrdering,
}

impl PublicParseCatalogEntry {
    fn from_report(report: &PublicParseReport, run: &PublicRun) -> Self {
        let run_group_id = if run.run_group_id.is_empty() {
            format!("legacy_{}_{}", report.report_id, run.run_index)
        } else {
            run.run_group_id.clone()
        };
        Self {
            report_id: report.report_id.clone(),
            report_ids: vec![report.report_id.clone()],
            run_index: run.run_index,
            run_group_id,
            contribution_count: 1,
            distinct_submitter_count: usize::from(
                report.submission_provenance.submitter_id.is_some(),
            ),
            local_profile_witness_character_count: run.local_profile_character_ids.len(),
            attribution_reconciliation_status: RunAttributionReconciliationStatus::SingleVantage,
            created_unix_millis: report.created_unix_millis,
            submitter_id: report.submission_provenance.submitter_id.clone(),
            deployment_id: report.deployment_id.clone(),
            region_id: report.region_id.clone(),
            activity_id: run.activity_id.clone(),
            activity_family_id: run.activity_family_id.clone(),
            activity_category_id: run.activity_category_id.clone(),
            scene_id: run.scene_id,
            scene_name: run.scene_name.clone(),
            difficulty_family: run.difficulty_family.clone(),
            difficulty_tier: run.difficulty_tier,
            terminal_state: run.terminal_state.clone(),
            total_run_time_micros: run.total_run_time_micros,
            participant_count: run.participants.len(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogFacets {
    pub deployments: Vec<FacetValue>,
    pub regions: Vec<FacetValue>,
    pub activities: Vec<FacetValue>,
    pub scenes: Vec<SceneFacetValue>,
    pub difficulties: Vec<FacetValue>,
    pub terminal_states: Vec<FacetValue>,
}

impl CatalogFacets {
    fn from_entries(entries: &[PublicParseCatalogEntry]) -> Self {
        let mut deployments = BTreeMap::new();
        let mut regions = BTreeMap::new();
        let mut activities = BTreeMap::new();
        let mut scenes = BTreeMap::new();
        let mut difficulties = BTreeMap::new();
        let mut terminal_states = BTreeMap::new();
        for entry in entries {
            increment(&mut deployments, entry.deployment_id.clone());
            increment(&mut regions, entry.region_id.clone());
            // The public Activity facet is the broad playable category.
            // Exact scene and family identities remain separate internal
            // dimensions and are never exposed as an Activity label.
            if let Some(value) = catalog_activity_category(entry) {
                increment(&mut activities, value.to_owned());
            }
            if let Some(value) = entry.difficulty_family.as_ref() {
                increment(&mut difficulties, value.clone());
            }
            increment(&mut terminal_states, entry.terminal_state.clone());
            if let Some(scene_id) = entry.scene_id {
                let value = scenes
                    .entry(scene_id)
                    .or_insert_with(|| (entry.scene_name.clone(), 0_usize));
                value.1 += 1;
            }
        }
        Self {
            deployments: facet_values(deployments),
            regions: facet_values(regions),
            activities: facet_values(activities),
            scenes: scenes
                .into_iter()
                .map(|(id, (label, count))| SceneFacetValue { id, label, count })
                .collect(),
            difficulties: facet_values(difficulties),
            terminal_states: facet_values(terminal_states),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetValue {
    pub id: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneFacetValue {
    pub id: i32,
    pub label: Option<String>,
    pub count: usize,
}

fn build_public_report(
    path: &Path,
    manifest: &UploadManifest,
    artifact: &LocalLogArtifact,
    report_id: &str,
    created_unix_millis: u64,
    submission_provenance: PublicSubmissionProvenance,
) -> Result<PublicParseReport, ServiceError> {
    let first_pass_file = File::open(path)?;
    let replay_file = File::open(path)?;
    build_public_report_from_readers(
        BufReader::new(first_pass_file),
        BufReader::new(replay_file),
        manifest,
        artifact,
        report_id,
        created_unix_millis,
        submission_provenance,
    )
}

/// Reconstructs the authoritative public report from two independent reads of
/// the same sealed artifact. The first pass learns remote attribution factors;
/// the second performs the complete combat and encounter replay. Keeping this
/// boundary free of filesystem access lets hosted verifiers supply immutable
/// object-storage bytes without trusting a client-produced projection.
pub fn build_public_report_from_readers<FirstPass, ReplayPass>(
    first_pass: FirstPass,
    replay_pass: ReplayPass,
    manifest: &UploadManifest,
    artifact: &LocalLogArtifact,
    report_id: &str,
    created_unix_millis: u64,
    submission_provenance: PublicSubmissionProvenance,
) -> Result<PublicParseReport, ServiceError>
where
    FirstPass: BufRead,
    ReplayPass: BufRead,
{
    if manifest.metadata.game_plugin_id != BPSR_GAME_PLUGIN_ID {
        return Err(ServiceError::UnsupportedGamePlugin(
            manifest.metadata.game_plugin_id.clone(),
        ));
    }
    let first_pass_reader = RlogReader::new(first_pass, RlogLimits::default())?;
    let mut remote_factor_learner = BpsrRemoteFactorLearner::new().map_err(ServiceError::Replay)?;
    first_pass_reader.replay(|event| {
        remote_factor_learner.observe(event);
        Ok(())
    })?;
    let remote_factors = remote_factor_learner.finish();

    let mut meter = CombatTimelinePlugin::with_damage_contribution_projection(
        confirmed_damage_contribution_rules().map_err(ServiceError::Replay)?,
        Some(Box::new(
            BpsrStateDamageContributionProjector::new_with_remote_factor_timeline(remote_factors)
                .map_err(ServiceError::Replay)?,
        )),
    )
    .map_err(ServiceError::Replay)?
    .with_ability_breakdown_resolver(combat_breakdown_ability_id);
    let mut encounter = EncounterRecorderPlugin::new(
        bundled_run_reducer_config().map_err(|error| ServiceError::Replay(error.to_string()))?,
    );
    let reader = RlogReader::new(replay_pass, RlogLimits::default())?;
    let header = reader.header().clone();
    meter.begin_live(&header);
    encounter.begin_live(&header);
    let mut local_profile_observations = Vec::new();
    let mut character_id_by_entity_uuid = BTreeMap::<i64, Option<String>>::new();
    let mut raw_local_state_observations = Vec::new();
    let mut stat_resonance_confounder_keys = BTreeSet::new();
    let replay = reader.replay(|event| {
        if event.sensitivity == EventSensitivity::PersonalGameplay
            && let CanonicalEvent::CharacterProfileObserved { profile } = &event.event
        {
            let loadout = profile_loadout_observation(profile)?;
            local_profile_observations.push(LocalProfileObservation {
                character_id: profile.character.character_id.clone(),
                event_sequence: event.sequence,
                observed_micros: event.time.observed_micros,
                game_time_millis: event.time.game_time_millis,
                payload_sha256: local_profile_payload_digest(profile)
                    .map_err(|error| error.to_string())?,
                loadout,
            });
        }
        if let CanonicalEvent::Timeline(timeline) = &event.event {
            match &timeline.kind {
                TimelineEventKind::Status(status) if !is_stat_resonance_status(status) => {
                    if let Some(wire) = event_wire_identity(event) {
                        stat_resonance_confounder_keys.insert((
                            wire,
                            status.target.actor_id.0,
                            status.target.entity_uuid.0,
                        ));
                    }
                }
                TimelineEventKind::UnresolvedStatus(status) => {
                    if let Some(wire) = event_wire_identity(event) {
                        stat_resonance_confounder_keys.insert((
                            wire,
                            status.target.actor_id.0,
                            status.target.entity_uuid.0,
                        ));
                    }
                }
                _ => {}
            }
            match &timeline.kind {
                TimelineEventKind::Actor(actor) => {
                    if let Some(character_id) = actor.character_id.as_ref() {
                        character_id_by_entity_uuid
                            .entry(actor.actor.entity_uuid.0)
                            .and_modify(|known| {
                                if known.as_deref() != Some(character_id.as_str()) {
                                    *known = None;
                                }
                            })
                            .or_insert_with(|| Some(character_id.clone()));
                    }
                }
                TimelineEventKind::EntityAttributes(attributes) => {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: attributes.actor.actor_id.0,
                        entity_uuid: attributes.actor.entity_uuid.0,
                        kind: LocalStateWitnessKind::EntityAttributes,
                        update_kind: attributes.update_kind,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: None,
                    });
                }
                TimelineEventKind::TemporaryAttributes(attributes) => {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: attributes.actor.actor_id.0,
                        entity_uuid: attributes.actor.entity_uuid.0,
                        kind: LocalStateWitnessKind::TemporaryAttributes,
                        update_kind: attributes.update_kind,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: None,
                    });
                }
                TimelineEventKind::Resource(resource) => {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: resource.actor.actor_id.0,
                        entity_uuid: resource.actor.entity_uuid.0,
                        kind: LocalStateWitnessKind::Resource,
                        update_kind: resource.update_kind,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: None,
                    });
                }
                TimelineEventKind::Status(status) if is_stat_resonance_status(status) => {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: status.target.actor_id.0,
                        entity_uuid: status.target.entity_uuid.0,
                        kind: LocalStateWitnessKind::StatResonanceStatus,
                        update_kind: EntityAttributeUpdateKind::Unknown,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                    });
                }
                TimelineEventKind::Status(status)
                    if status.effect.0 == LIFE_WAVE_EFFECT_ID
                        && status
                            .origin
                            .map(|origin| (origin.source_type_id, origin.source_config_id))
                            == Some((LIFE_WAVE_SOURCE_TYPE_ID, LIFE_WAVE_SOURCE_CONFIG_ID))
                        && status.duration_millis == Some(LIFE_WAVE_DURATION_MILLIS)
                        && status.instance_id.is_some()
                        && matches!(
                            status.state,
                            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
                        ) =>
                {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: status.target.actor_id.0,
                        entity_uuid: status.target.entity_uuid.0,
                        kind: LocalStateWitnessKind::LifeWaveTriggerStatus,
                        update_kind: EntityAttributeUpdateKind::Unknown,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: None,
                    });
                }
                TimelineEventKind::Healing(healing)
                    if healing.amount > 0 && healing.effective_amount != Some(0) =>
                {
                    let payload =
                        serde_json::to_vec(&timeline.kind).map_err(|error| error.to_string())?;
                    raw_local_state_observations.push(RawLocalStateObservation {
                        actor_id: healing.target.actor_id.0,
                        entity_uuid: healing.target.entity_uuid.0,
                        kind: LocalStateWitnessKind::LifeWaveTriggerHealing,
                        update_kind: EntityAttributeUpdateKind::Unknown,
                        event_sequence: event.sequence,
                        observed_micros: event.time.observed_micros,
                        game_time_millis: event.time.game_time_millis,
                        payload_sha256: local_state_payload_digest(&payload),
                        wire: event_wire_identity(event),
                        related_entity_uuid: Some(healing.source.entity_uuid.0),
                    });
                }
                _ => {}
            }
        }
        meter.observe_live(event);
        encounter
            .observe_live(event)
            .map_err(|error| error.to_string())
    })?;
    let run_projection = encounter
        .live_snapshot()
        .map_err(|error| ServiceError::Replay(error.to_string()))?;
    let mut history = meter
        .history_snapshot(&run_projection.runs)
        .map_err(|error| ServiceError::Replay(error.to_string()))?;
    enrich_bpsr_history_ability_presentation(&mut history)?;
    if !run_projection
        .runs
        .iter()
        .any(RunAnalysis::is_completed_submission)
    {
        return Err(ServiceError::NoCompletedRun);
    }
    let local_character_ids = local_profile_observations
        .iter()
        .map(|observation| observation.character_id.as_str())
        .collect::<BTreeSet<_>>();
    let life_wave_status_keys = raw_local_state_observations
        .iter()
        .filter(|raw| raw.kind == LocalStateWitnessKind::LifeWaveTriggerStatus)
        .filter_map(|raw| raw.wire.map(|wire| (wire, raw.actor_id, raw.entity_uuid)))
        .collect::<BTreeSet<_>>();
    let life_wave_healing_keys = raw_local_state_observations
        .iter()
        .filter(|raw| raw.kind == LocalStateWitnessKind::LifeWaveTriggerHealing)
        .filter_map(|raw| raw.wire.map(|wire| (wire, raw.actor_id, raw.entity_uuid)))
        .collect::<BTreeSet<_>>();
    let life_wave_pair_keys = life_wave_status_keys
        .intersection(&life_wave_healing_keys)
        .copied()
        .collect::<BTreeSet<_>>();
    let stat_resonance_status_keys = raw_local_state_observations
        .iter()
        .filter(|raw| raw.kind == LocalStateWitnessKind::StatResonanceStatus)
        .filter_map(|raw| raw.wire.map(|wire| (wire, raw.actor_id, raw.entity_uuid)))
        .collect::<BTreeSet<_>>();
    let entity_attribute_keys = raw_local_state_observations
        .iter()
        .filter(|raw| raw.kind == LocalStateWitnessKind::EntityAttributes)
        .filter_map(|raw| raw.wire.map(|wire| (wire, raw.actor_id, raw.entity_uuid)))
        .collect::<BTreeSet<_>>();
    let exact_stat_resonance_keys = stat_resonance_status_keys
        .intersection(&entity_attribute_keys)
        .filter(|key| !stat_resonance_confounder_keys.contains(key))
        .copied()
        .collect::<BTreeSet<_>>();
    let local_state_observations = raw_local_state_observations
        .into_iter()
        .filter_map(|raw| {
            if matches!(
                raw.kind,
                LocalStateWitnessKind::LifeWaveTriggerStatus
                    | LocalStateWitnessKind::LifeWaveTriggerHealing
            ) && !raw.wire.is_some_and(|wire| {
                life_wave_pair_keys.contains(&(wire, raw.actor_id, raw.entity_uuid))
            }) {
                return None;
            }
            if raw.kind == LocalStateWitnessKind::StatResonanceStatus
                && !raw.wire.is_some_and(|wire| {
                    exact_stat_resonance_keys.contains(&(wire, raw.actor_id, raw.entity_uuid))
                })
            {
                return None;
            }
            let character_id = character_id_by_entity_uuid
                .get(&raw.entity_uuid)
                .and_then(Option::as_ref)?;
            let related_character_id = match raw.related_entity_uuid {
                Some(entity_uuid) => Some(
                    character_id_by_entity_uuid
                        .get(&entity_uuid)
                        .and_then(Option::as_ref)?
                        .clone(),
                ),
                None => None,
            };
            local_character_ids
                .contains(character_id.as_str())
                .then(|| LocalStateObservation {
                    character_id: character_id.clone(),
                    related_character_id,
                    raw,
                })
        })
        .collect::<Vec<_>>();
    let runs = public_runs(
        &history,
        &run_projection.runs,
        &local_profile_observations,
        &local_state_observations,
    );
    if runs.is_empty() {
        return Err(ServiceError::NoCompletedRun);
    }
    Ok(PublicParseReport {
        schema_version: PUBLIC_PARSE_SCHEMA_VERSION,
        projection_revision: PUBLIC_PARSE_PROJECTION_REVISION,
        report_id: report_id.into(),
        visibility: manifest.metadata.visibility,
        created_unix_millis,
        game_plugin_id: manifest.metadata.game_plugin_id.clone(),
        deployment_id: history.deployment_id,
        region_id: history.region_id,
        world_id: history.world_id,
        client_build: history.client_build,
        protocol_pack_digest: history.protocol_pack_digest,
        verification: PublicVerification {
            tier: VerificationTier::Replayed,
            artifact_sha256: artifact.file_sha256.to_string(),
            canonical_content_sha256: replay.content_sha256,
            event_count: replay.event_count,
            privacy_policy_digest: manifest.metadata.privacy_policy_digest.to_string(),
        },
        submission_provenance,
        runs,
    })
}

fn public_runs(
    history: &CombatHistorySnapshot,
    analyses: &[RunAnalysis],
    local_profile_observations: &[LocalProfileObservation],
    local_state_observations: &[LocalStateObservation],
) -> Vec<PublicRun> {
    history
        .runs
        .iter()
        .filter_map(|run| {
            let analysis = analyses.get(run.run_index as usize)?;
            if !analysis.is_completed_submission() {
                return None;
            }
            let view = run
                .views
                .iter()
                .find(|view| view.kind == "all")
                .or_else(|| run.views.first());
            let participant_character_ids = view
                .into_iter()
                .flat_map(|view| &view.actors)
                .filter_map(|actor| actor.character_id.clone())
                .collect::<BTreeSet<_>>();
            let local_profile_witnesses = run_scoped_profile_witnesses(
                analysis,
                &participant_character_ids,
                local_profile_observations,
            );
            let combat_loadout_phases = run_scoped_combat_loadout_phases(
                analysis,
                &participant_character_ids,
                local_profile_observations,
            );
            let local_profile_character_ids = local_profile_witnesses
                .iter()
                .map(|witness| witness.character_id.clone())
                .collect::<BTreeSet<_>>();
            let local_state_witnesses = run_scoped_state_witnesses(
                analysis,
                &local_profile_character_ids,
                local_state_observations,
            );
            Some(PublicRun {
                run_index: run.run_index,
                run_group_id: run_group_id(history, analysis, run.run_index),
                correlation_method: if analysis.identity.instance_id.is_some() {
                    RunCorrelationMethod::ExactInstanceId
                } else {
                    RunCorrelationMethod::IsolatedArtifact
                },
                activity_id: run.activity_id.clone(),
                activity_family_id: run.activity_family_id.clone(),
                activity_category_id: public_activity_category_id(analysis).map(str::to_owned),
                scene_id: run.scene_id,
                scene_name: run
                    .scene_id
                    .and_then(|scene_id| {
                        localized_scene_name(i64::from(scene_id), "en-US")
                            .ok()
                            .flatten()
                    })
                    .map(str::to_owned)
                    .or_else(|| run.presentation_scene_name.clone()),
                difficulty_family: run.difficulty_family.clone(),
                difficulty_tier: run.difficulty_tier,
                terminal_state: run.terminal_state.clone(),
                total_run_time_micros: run.total_run_time_micros,
                game_time_micros: run.game_time_micros,
                active_combat_micros: view.map_or(0, |view| view.active_combat_micros),
                true_time_micros: run.true_time_micros,
                retry_count: run.retry_count,
                boss_retry_count: run.boss_retry_count,
                rdps_status: run.rdps_status.clone(),
                data_gap_count: analysis.data_gap_count,
                authoritative_start: analysis.authoritative_start,
                authoritative_completion: analysis.authoritative_completion,
                submission_disposition: analysis.submission_disposition,
                local_profile_character_ids: local_profile_character_ids.iter().cloned().collect(),
                local_profile_witnesses,
                local_state_witnesses,
                combat_loadout_phases,
                segments: analysis
                    .segments
                    .iter()
                    .map(|segment| PublicRunSegment {
                        index: segment.index,
                        kind: segment.kind,
                        wall_time_micros: segment.wall_time_micros,
                        active_combat_micros: segment.active_combat_micros,
                        attempt_count: segment.attempt_count,
                        retry_count: segment.retry_count,
                    })
                    .collect(),
                participants: view
                    .map(|view| {
                        view.actors
                            .iter()
                            .filter(|actor| is_public_participant(actor))
                            .map(public_participant)
                            .collect()
                    })
                    .unwrap_or_default(),
                rdps_influences: view.map(public_rdps_influences).unwrap_or_default(),
                rdps_effects: view.map(public_rdps_effects).unwrap_or_default(),
            })
        })
        .collect()
}

fn enrich_bpsr_history_ability_presentation(
    history: &mut CombatHistorySnapshot,
) -> Result<(), ServiceError> {
    for run in &mut history.runs {
        for view in &mut run.views {
            for actor in &mut view.actors {
                for ability in &mut actor.abilities {
                    let Ok(ability_id) = ability.ability_id.parse::<i64>() else {
                        continue;
                    };
                    if let Some(presentation) =
                        combat_action_presentation(ability_id).map_err(ServiceError::Replay)?
                    {
                        ability.presentation_name =
                            localized_combat_action_name(ability_id, "en-US")
                                .map_err(ServiceError::Replay)?
                                .map(str::to_owned);
                        ability.presentation_kind = Some(presentation.kind.clone());
                        ability.presentation_resolution = Some(presentation.resolution.clone());
                        ability.icon_asset_path = presentation.icon.as_ref().map(|path| {
                            format!("/game-assets/blue-protocol-star-resonance/shared/{path}")
                        });
                        ability.presentation_recount_group_name =
                            localized_recount_group_name(ability_id, "en-US")
                                .map_err(ServiceError::Replay)?
                                .map(str::to_owned);
                    }
                    ability.presentation_recount_group_id = combat_recount_group_id(ability_id)
                        .map_err(ServiceError::Replay)?
                        .map(|group_id| group_id.to_string());
                }
            }
        }
    }
    Ok(())
}

fn local_profile_payload_digest(profile: &GameProfileEvent) -> Result<String, serde_json::Error> {
    let payload = serde_json::to_vec(&profile.payload)?;
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-local-profile-witness-v1\0");
    hasher.update(profile.game_plugin_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(profile.payload_schema_id.as_bytes());
    hasher.update(profile.payload_schema_version.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(&payload);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn profile_loadout_observation(
    profile: &GameProfileEvent,
) -> Result<ProfileLoadoutObservation, String> {
    let patch = CharacterProfilePatch::from_game_event(profile)
        .map_err(|error| format!("could not decode privacy-reviewed combat loadout: {error}"))?;
    let mut equipped_skill_ids = patch
        .equipped_action_slots
        .as_ref()
        .map(|slots| {
            slots
                .iter()
                .map(|slot| slot.skill_id.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    equipped_skill_ids.sort();
    equipped_skill_ids.dedup();
    let mut equipped_imagines = patch
        .battle_imagine_skills
        .as_ref()
        .into_iter()
        .flatten()
        .filter_map(|imagine| {
            Some(PublicCombatImagineLoadout {
                skill_id: imagine.skill_id.to_string(),
                tier: imagine.remodel_level,
                equipped_slot: imagine.equipped_slot?,
            })
        })
        .collect::<Vec<_>>();
    equipped_imagines.sort_by_key(|imagine| imagine.equipped_slot);
    Ok(ProfileLoadoutObservation {
        display_name: patch.display_name,
        class_id: patch.class_id,
        specialization_id: patch.specialization_id,
        equipped_skill_ids,
        equipped_imagines,
        equipment_count: patch.equipment.as_ref().map(Vec::len),
        equipped_module_count: patch
            .modules
            .as_ref()
            .map(|modules| modules.equipped_slots.len()),
        talent_count: patch.talents.as_ref().map(Vec::len),
    })
}

fn run_scoped_profile_witnesses(
    analysis: &RunAnalysis,
    participant_character_ids: &BTreeSet<String>,
    observations: &[LocalProfileObservation],
) -> Vec<PublicLocalProfileWitness> {
    run_scoped_profile_observations(analysis, participant_character_ids, observations)
        .into_iter()
        .map(|observation| PublicLocalProfileWitness {
            character_id: observation.character_id.clone(),
            placement: LocalStateWitnessPlacement::InRun,
            event_sequence: observation.event_sequence,
            observed_micros: observation.observed_micros,
            game_time_millis: observation.game_time_millis,
            payload_sha256: observation.payload_sha256.clone(),
        })
        .collect()
}

fn run_scoped_profile_observations<'a>(
    analysis: &RunAnalysis,
    participant_character_ids: &BTreeSet<String>,
    observations: &'a [LocalProfileObservation],
) -> Vec<&'a LocalProfileObservation> {
    // A lobby/entry snapshot is not authoritative combat-loadout evidence:
    // players can change class, specialization, modules, skills, or Imagines
    // after entering and before the pull. Only snapshots strictly after the
    // first observed combat window may fill a submitted run's state gaps.
    // Every later snapshot remains ordered so mid-run and boss-specific swaps
    // are replayed at their actual time instead of rewriting the whole run.
    let Some(first_combat_started_micros) = analysis
        .encounters
        .iter()
        .flat_map(|encounter| &encounter.combat_windows)
        .map(|window| window.started_micros)
        .min()
    else {
        return Vec::new();
    };
    let ended_micros = analysis
        .timing
        .ended_micros
        .unwrap_or(analysis.timing.observed_until_micros);
    let mut selected = observations
        .iter()
        .filter(|observation| {
            participant_character_ids.contains(observation.character_id.as_str())
                && observation.observed_micros > first_combat_started_micros
                && observation.observed_micros <= ended_micros
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|observation| (observation.observed_micros, observation.event_sequence));
    selected.dedup_by_key(|observation| observation.event_sequence);
    selected
}

fn run_scoped_combat_loadout_phases(
    analysis: &RunAnalysis,
    participant_character_ids: &BTreeSet<String>,
    observations: &[LocalProfileObservation],
) -> Vec<PublicCombatLoadoutPhase> {
    let mut last_loadout_by_character = BTreeMap::<String, ProfileLoadoutObservation>::new();
    run_scoped_profile_observations(analysis, participant_character_ids, observations)
        .into_iter()
        .filter_map(|observation| {
            if observation.loadout.is_empty()
                || last_loadout_by_character
                    .get(&observation.character_id)
                    .is_some_and(|previous| previous == &observation.loadout)
            {
                return None;
            }
            last_loadout_by_character.insert(
                observation.character_id.clone(),
                observation.loadout.clone(),
            );
            let segment = analysis.segments.iter().find(|segment| {
                observation.observed_micros >= segment.started_micros
                    && observation.observed_micros <= segment.ended_micros
            });
            let encounter = analysis.encounters.iter().find(|encounter| {
                observation.observed_micros >= encounter.started_micros
                    && observation.observed_micros <= encounter.ended_micros
            });
            let in_active_combat = encounter.is_some_and(|encounter| {
                encounter.combat_windows.iter().any(|window| {
                    observation.observed_micros >= window.started_micros
                        && observation.observed_micros <= window.ended_micros
                })
            });
            Some(PublicCombatLoadoutPhase {
                character_id: observation.character_id.clone(),
                display_name: observation.loadout.display_name.clone(),
                observed_micros: observation.observed_micros,
                run_elapsed_micros: observation
                    .observed_micros
                    .saturating_sub(analysis.timing.started_micros),
                game_time_millis: observation.game_time_millis,
                segment_index: segment.map(|segment| segment.index),
                encounter_index: encounter.map(|encounter| encounter.index),
                attempt_number: encounter.map(|encounter| encounter.attempt_number),
                in_active_combat,
                class_id: observation.loadout.class_id,
                class_name: observation
                    .loadout
                    .class_id
                    .and_then(|id| localized_class_name(id, "en-US").ok().flatten())
                    .map(str::to_owned),
                specialization_id: observation.loadout.specialization_id,
                specialization_name: observation
                    .loadout
                    .specialization_id
                    .and_then(|id| localized_specialization_name(id, "en-US").ok().flatten())
                    .map(str::to_owned),
                equipped_skill_ids: observation.loadout.equipped_skill_ids.clone(),
                equipped_imagines: observation.loadout.equipped_imagines.clone(),
                equipment_count: observation.loadout.equipment_count,
                equipped_module_count: observation.loadout.equipped_module_count,
                talent_count: observation.loadout.talent_count,
            })
        })
        .collect()
}

fn local_state_payload_digest(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-local-state-witness-v1\0");
    hasher.update(payload);
    format!("sha256:{:x}", hasher.finalize())
}

fn event_wire_identity(envelope: &EventEnvelope) -> Option<(u64, u64, u64)> {
    match envelope.provenance.source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some((capture_sequence, connection_id, stream_id)),
        _ => None,
    }
}

fn verified_state_input_digest(
    reconciliation: &PublicRunReconciliation,
    events: &[VerifiedCrossVantageStateEvent],
) -> Result<String, serde_json::Error> {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-cross-vantage-verified-state-profile-trigger-resource-v5\0");
    hasher.update(reconciliation.canonical_spine.artifact_sha256.as_bytes());
    hasher.update(reconciliation.canonical_spine.run_index.to_le_bytes());
    for event in events {
        hasher.update(b"\0");
        hasher.update(event.report_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(event.character_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(
            event
                .related_character_id
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update(event.envelope.sequence.to_le_bytes());
        hasher.update(event.game_time_millis.unwrap_or(i64::MIN).to_le_bytes());
        hasher.update(match event.placement {
            LocalStateWitnessPlacement::Unspecified => b"unspecified".as_slice(),
            LocalStateWitnessPlacement::PreRunBaseline => b"pre-run-baseline".as_slice(),
            LocalStateWitnessPlacement::InRun => b"in-run".as_slice(),
        });
        hasher.update(serde_json::to_vec(&event.envelope.event)?);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn canonical_character_entities(path: &Path) -> Result<BTreeMap<String, EntityRef>, ServiceError> {
    let file = File::open(path)?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
    let mut entities = BTreeMap::<String, EntityRef>::new();
    reader.replay(|envelope| {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        let TimelineEventKind::Actor(actor) = &timeline.kind else {
            return Ok(());
        };
        let Some(character_id) = actor.character_id.as_ref() else {
            return Ok(());
        };
        match entities.entry(character_id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(actor.actor);
            }
            std::collections::btree_map::Entry::Occupied(entry) if *entry.get() != actor.actor => {
                return Err(format!(
                    "canonical character {character_id} changes runtime entity from {:?} to {:?}",
                    entry.get(),
                    actor.actor
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
        Ok(())
    })?;
    Ok(entities)
}

fn artifact_character_id_by_entity_uuid(
    path: &Path,
) -> Result<BTreeMap<i64, String>, ServiceError> {
    let file = File::open(path)?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
    let mut characters = BTreeMap::<i64, String>::new();
    reader.replay(|envelope| {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        let TimelineEventKind::Actor(actor) = &timeline.kind else {
            return Ok(());
        };
        let Some(character_id) = actor.character_id.as_ref() else {
            return Ok(());
        };
        match characters.entry(actor.actor.entity_uuid.0) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(character_id.clone());
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() != character_id => {
                return Err(format!(
                    "artifact entity {} changes stable character from {} to {character_id}",
                    actor.actor.entity_uuid.0,
                    entry.get()
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
        Ok(())
    })?;
    Ok(characters)
}

fn remap_cross_vantage_state_entities(
    events: &mut [VerifiedCrossVantageStateEvent],
    canonical_entities: &BTreeMap<String, EntityRef>,
) -> Result<(), ServiceError> {
    for event in events {
        if let CanonicalEvent::CharacterProfileObserved { profile } = &event.envelope.event {
            if profile.character.character_id != event.character_id {
                return Err(ServiceError::CrossVantageReplay(format!(
                    "verified profile event {} changed character identity",
                    event.envelope.sequence
                )));
            }
            continue;
        }
        let canonical = canonical_entities
            .get(&event.character_id)
            .copied()
            .ok_or_else(|| {
                ServiceError::CrossVantageReplay(format!(
                    "canonical spine has no runtime entity for selected character {}",
                    event.character_id
                ))
            })?;
        let CanonicalEvent::Timeline(timeline) = &mut event.envelope.event else {
            return Err(ServiceError::CrossVantageReplay(format!(
                "verified evidence event {} is neither a profile nor timeline event",
                event.envelope.sequence
            )));
        };
        match &mut timeline.kind {
            TimelineEventKind::EntityAttributes(attributes) => attributes.actor = canonical,
            TimelineEventKind::TemporaryAttributes(attributes) => attributes.actor = canonical,
            TimelineEventKind::Resource(resource) => resource.actor = canonical,
            TimelineEventKind::Status(status) => {
                status.target = canonical;
                if is_stat_resonance_status(status) {
                    let related_character_id =
                        event.related_character_id.as_deref().ok_or_else(|| {
                            ServiceError::CrossVantageReplay(format!(
                                "verified Stat Resonance status event {} has no stable provider character",
                                event.envelope.sequence
                            ))
                        })?;
                    status.source = Some(
                        canonical_entities
                            .get(related_character_id)
                            .copied()
                            .ok_or_else(|| {
                                ServiceError::CrossVantageReplay(format!(
                                    "canonical spine has no runtime entity for Stat Resonance provider character {related_character_id}"
                                ))
                            })?,
                    );
                }
            }
            TimelineEventKind::Healing(healing) => {
                healing.target = canonical;
                let related_character_id =
                    event.related_character_id.as_deref().ok_or_else(|| {
                        ServiceError::CrossVantageReplay(format!(
                            "verified Life Wave healing event {} has no stable source character",
                            event.envelope.sequence
                        ))
                    })?;
                healing.source = canonical_entities
                    .get(related_character_id)
                    .copied()
                    .ok_or_else(|| {
                        ServiceError::CrossVantageReplay(format!(
                            "canonical spine has no runtime entity for Life Wave source character {related_character_id}"
                        ))
                    })?;
            }
            _ => {
                return Err(ServiceError::CrossVantageReplay(format!(
                    "verified evidence event {} is no longer a supported state or trigger event",
                    event.envelope.sequence
                )));
            }
        }
    }
    Ok(())
}

fn imported_wire_namespace(report_id: &str, character_id: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-cross-vantage-wire-v1\0");
    hasher.update(report_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(character_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes) | (1_u64 << 63)
}

fn aligned_cross_vantage_envelope(
    imported: &VerifiedCrossVantageStateEvent,
    observed_micros: u64,
    region: &rlogs_events::RegionContext,
) -> EventEnvelope {
    let mut envelope = imported.envelope.clone();
    envelope.region = region.clone();
    envelope.time.observed_micros = observed_micros;
    let (capture_sequence, stream_id) = match imported.envelope.provenance.source {
        EvidenceSource::Wire {
            capture_sequence,
            stream_id,
            ..
        } => (capture_sequence, stream_id),
        _ => (imported.envelope.sequence, imported.envelope.sequence),
    };
    let provenance = EventProvenance::wire(
        capture_sequence,
        imported_wire_namespace(&imported.report_id, &imported.character_id),
        stream_id,
    );
    envelope.provenance = provenance.clone();
    if let CanonicalEvent::Timeline(timeline) = &mut envelope.event {
        timeline.time = envelope.time;
        timeline.provenance = provenance;
    }
    envelope
}

fn replay_canonical_with_cross_vantage_state(
    path: &Path,
    target_run_index: u32,
    imported_events: &[VerifiedCrossVantageStateEvent],
    mut observe: impl FnMut(&EventEnvelope, bool) -> Result<(), String>,
) -> Result<rlogs_log_format::RlogReplaySummary, ServiceError> {
    let file = File::open(path)?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
    let region = reader.header().region.clone();
    let mut baselines = imported_events
        .iter()
        .filter(|event| event.placement == LocalStateWitnessPlacement::PreRunBaseline)
        .cloned()
        .collect::<VecDeque<_>>();
    let mut in_run = imported_events
        .iter()
        .filter(|event| event.placement == LocalStateWitnessPlacement::InRun)
        .cloned()
        .collect::<Vec<_>>();
    in_run.sort_by(|left, right| {
        (
            left.game_time_millis,
            &left.report_id,
            left.envelope.sequence,
        )
            .cmp(&(
                right.game_time_millis,
                &right.report_id,
                right.envelope.sequence,
            ))
    });
    let mut in_run = VecDeque::from(in_run);
    let mut next_run_index = 0_u32;
    let mut active_run_index = None;
    let mut target_seen = false;

    let summary = reader.replay(|envelope| {
        let run_state = match &envelope.event {
            CanonicalEvent::Timeline(timeline) => match timeline.kind {
                TimelineEventKind::RunBoundary { state, .. } => Some(state),
                _ => None,
            },
            _ => None,
        };
        let begins_run = match run_state {
            Some(RunState::Entered) => true,
            Some(RunState::Started) => active_run_index.is_none(),
            _ => false,
        };
        if begins_run {
            active_run_index = Some(next_run_index);
            next_run_index = next_run_index.saturating_add(1);
            target_seen |= active_run_index == Some(target_run_index);
            observe(envelope, false)?;
            if active_run_index == Some(target_run_index) {
                while let Some(imported) = baselines.pop_front() {
                    let aligned = aligned_cross_vantage_envelope(
                        &imported,
                        envelope.time.observed_micros,
                        &region,
                    );
                    observe(&aligned, true)?;
                }
            }
            return Ok(());
        }

        if active_run_index == Some(target_run_index)
            && let Some(canonical_game_time) = envelope.time.game_time_millis
        {
            while in_run.front().is_some_and(|imported| {
                imported
                    .game_time_millis
                    .is_some_and(|game_time| game_time < canonical_game_time)
            }) {
                let imported = in_run.pop_front().expect("front was present");
                let imported_game_time = imported
                    .game_time_millis
                    .expect("in-run evidence without game time is blocked before replay");
                let delta_millis = canonical_game_time.saturating_sub(imported_game_time);
                let delta_micros = u64::try_from(delta_millis)
                    .unwrap_or(u64::MAX)
                    .saturating_mul(1_000);
                let aligned = aligned_cross_vantage_envelope(
                    &imported,
                    envelope.time.observed_micros.saturating_sub(delta_micros),
                    &region,
                );
                observe(&aligned, true)?;
            }
        }
        observe(envelope, false)?;
        if matches!(
            run_state,
            Some(RunState::Ended | RunState::Completed | RunState::Failed | RunState::Exited)
        ) {
            active_run_index = None;
        }
        Ok(())
    })?;
    if !target_seen {
        return Err(ServiceError::CrossVantageReplay(format!(
            "canonical artifact has no run index {target_run_index}"
        )));
    }
    if !baselines.is_empty() || !in_run.is_empty() {
        return Err(ServiceError::CrossVantageReplay(format!(
            "{} selected state witnesses could not be conservatively aligned to canonical server time",
            baselines.len().saturating_add(in_run.len())
        )));
    }
    Ok(summary)
}

fn verify_profile_witness_event(
    report_id: &str,
    witness: &PublicLocalProfileWitness,
    envelope: &EventEnvelope,
) -> Result<(), ServiceError> {
    if envelope.sequence != witness.event_sequence
        || envelope.time.observed_micros != witness.observed_micros
        || envelope.time.game_time_millis != witness.game_time_millis
        || envelope.sensitivity != EventSensitivity::PersonalGameplay
    {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    }
    let CanonicalEvent::CharacterProfileObserved { profile } = &envelope.event else {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    };
    if profile.character.character_id != witness.character_id
        || local_profile_payload_digest(profile)? != witness.payload_sha256
    {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    }
    Ok(())
}

fn verify_state_witness_event(
    report_id: &str,
    witness: &PublicLocalStateWitness,
    envelope: &EventEnvelope,
) -> Result<(), ServiceError> {
    if envelope.sequence != witness.event_sequence
        || envelope.time.observed_micros != witness.observed_micros
        || envelope.time.game_time_millis != witness.game_time_millis
    {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    }
    let CanonicalEvent::Timeline(timeline) = &envelope.event else {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    };
    let (actor_id, entity_uuid, update_kind, actual_kind) = match &timeline.kind {
        TimelineEventKind::EntityAttributes(attributes) => (
            attributes.actor.actor_id.0,
            attributes.actor.entity_uuid.0,
            attributes.update_kind,
            LocalStateWitnessKind::EntityAttributes,
        ),
        TimelineEventKind::TemporaryAttributes(attributes) => (
            attributes.actor.actor_id.0,
            attributes.actor.entity_uuid.0,
            attributes.update_kind,
            LocalStateWitnessKind::TemporaryAttributes,
        ),
        TimelineEventKind::Resource(resource) => (
            resource.actor.actor_id.0,
            resource.actor.entity_uuid.0,
            resource.update_kind,
            LocalStateWitnessKind::Resource,
        ),
        TimelineEventKind::Status(status) => (
            status.target.actor_id.0,
            status.target.entity_uuid.0,
            EntityAttributeUpdateKind::Unknown,
            if is_stat_resonance_status(status) {
                LocalStateWitnessKind::StatResonanceStatus
            } else {
                LocalStateWitnessKind::LifeWaveTriggerStatus
            },
        ),
        TimelineEventKind::Healing(healing) => (
            healing.target.actor_id.0,
            healing.target.entity_uuid.0,
            EntityAttributeUpdateKind::Unknown,
            LocalStateWitnessKind::LifeWaveTriggerHealing,
        ),
        _ => {
            return Err(ServiceError::CrossVantageWitnessMismatch {
                report_id: report_id.into(),
                event_sequence: witness.event_sequence,
            });
        }
    };
    let expected_update_kind = match update_kind {
        EntityAttributeUpdateKind::Unknown => "unknown",
        EntityAttributeUpdateKind::Snapshot => "snapshot",
        EntityAttributeUpdateKind::Delta => "delta",
    };
    let exact_life_wave_evidence = match (&timeline.kind, actual_kind) {
        (TimelineEventKind::Status(status), LocalStateWitnessKind::LifeWaveTriggerStatus) => {
            status.effect.0 == LIFE_WAVE_EFFECT_ID
                && status
                    .origin
                    .map(|origin| (origin.source_type_id, origin.source_config_id))
                    == Some((LIFE_WAVE_SOURCE_TYPE_ID, LIFE_WAVE_SOURCE_CONFIG_ID))
                && status.duration_millis == Some(LIFE_WAVE_DURATION_MILLIS)
                && status.instance_id.is_some()
                && matches!(
                    status.state,
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
                )
        }
        (TimelineEventKind::Healing(healing), LocalStateWitnessKind::LifeWaveTriggerHealing) => {
            healing.amount > 0
                && healing.effective_amount != Some(0)
                && witness.related_character_id.is_some()
        }
        (TimelineEventKind::Status(status), LocalStateWitnessKind::StatResonanceStatus) => {
            is_stat_resonance_status(status)
                && status.source.is_some()
                && status.instance_id.is_some()
                && witness.related_character_id.is_some()
                && matches!(status.state, StatusState::Applied | StatusState::Removed)
        }
        (
            _,
            LocalStateWitnessKind::LifeWaveTriggerStatus
            | LocalStateWitnessKind::LifeWaveTriggerHealing
            | LocalStateWitnessKind::StatResonanceStatus,
        ) => false,
        _ => true,
    };
    let payload = serde_json::to_vec(&timeline.kind)?;
    if actual_kind != witness.kind
        || actor_id != witness.actor_id
        || entity_uuid != witness.entity_uuid
        || expected_update_kind != witness.update_kind
        || !exact_life_wave_evidence
        || local_state_payload_digest(&payload) != witness.payload_sha256
    {
        return Err(ServiceError::CrossVantageWitnessMismatch {
            report_id: report_id.into(),
            event_sequence: witness.event_sequence,
        });
    }
    Ok(())
}

fn verify_state_witness_characters(
    report_id: &str,
    character_id: &str,
    witness: &PublicLocalStateWitness,
    envelope: &EventEnvelope,
    character_id_by_entity_uuid: &BTreeMap<i64, String>,
) -> Result<(), ServiceError> {
    let target_character_matches = character_id_by_entity_uuid
        .get(&witness.entity_uuid)
        .is_some_and(|observed| observed == character_id);
    let related_character_matches = match (&witness.related_character_id, &envelope.event) {
        (None, _) => true,
        (Some(related), CanonicalEvent::Timeline(timeline)) => match &timeline.kind {
            TimelineEventKind::Healing(healing) => character_id_by_entity_uuid
                .get(&healing.source.entity_uuid.0)
                .is_some_and(|observed| observed == related),
            TimelineEventKind::Status(status) if is_stat_resonance_status(status) => status
                .source
                .and_then(|source| character_id_by_entity_uuid.get(&source.entity_uuid.0))
                .is_some_and(|observed| observed == related),
            _ => false,
        },
        (Some(_), _) => false,
    };
    if target_character_matches && related_character_matches {
        return Ok(());
    }
    Err(ServiceError::CrossVantageWitnessMismatch {
        report_id: report_id.into(),
        event_sequence: witness.event_sequence,
    })
}

fn run_scoped_state_witnesses(
    analysis: &RunAnalysis,
    local_character_ids: &BTreeSet<String>,
    observations: &[LocalStateObservation],
) -> Vec<PublicLocalStateWitness> {
    let started_micros = analysis.timing.started_micros;
    let ended_micros = analysis
        .timing
        .ended_micros
        .unwrap_or(analysis.timing.observed_until_micros);
    let mut groups =
        BTreeMap::<(String, LocalStateWitnessKind), Vec<&LocalStateObservation>>::new();
    for observation in observations.iter().filter(|observation| {
        local_character_ids.contains(&observation.character_id)
            && observation.raw.observed_micros <= ended_micros
    }) {
        groups
            .entry((observation.character_id.clone(), observation.raw.kind))
            .or_default()
            .push(observation);
    }

    let mut selected = Vec::new();
    for (_, mut observations) in groups {
        observations.sort_by_key(|observation| {
            (
                observation.raw.observed_micros,
                observation.raw.event_sequence,
            )
        });
        let first_relevant = observations
            .iter()
            .enumerate()
            .filter(|(_, observation)| {
                observation.raw.observed_micros <= started_micros
                    && observation.raw.update_kind == EntityAttributeUpdateKind::Snapshot
            })
            .map(|(index, _)| index)
            .next_back()
            .unwrap_or_default();
        selected.extend(
            observations
                .into_iter()
                .skip(first_relevant)
                .map(|observation| PublicLocalStateWitness {
                    character_id: observation.character_id.clone(),
                    related_character_id: observation.related_character_id.clone(),
                    actor_id: observation.raw.actor_id,
                    entity_uuid: observation.raw.entity_uuid,
                    kind: observation.raw.kind,
                    update_kind: match observation.raw.update_kind {
                        EntityAttributeUpdateKind::Unknown => "unknown",
                        EntityAttributeUpdateKind::Snapshot => "snapshot",
                        EntityAttributeUpdateKind::Delta => "delta",
                    }
                    .into(),
                    placement: if observation.raw.observed_micros <= started_micros {
                        LocalStateWitnessPlacement::PreRunBaseline
                    } else {
                        LocalStateWitnessPlacement::InRun
                    },
                    event_sequence: observation.raw.event_sequence,
                    observed_micros: observation.raw.observed_micros,
                    game_time_millis: observation.raw.game_time_millis,
                    payload_sha256: observation.raw.payload_sha256.clone(),
                }),
        );
    }
    selected.sort_by_key(|witness| (witness.observed_micros, witness.event_sequence));
    selected.dedup_by_key(|witness| witness.event_sequence);
    selected
}

fn build_public_reconciliation(group: &CatalogRunGroup) -> PublicRunReconciliation {
    let canonical_report_id = group.representative.report_id.as_str();
    let canonical_run_index = group.representative.run_index;
    let canonical = group
        .reconciliation_sources
        .iter()
        .find(|source| {
            source.report_id == canonical_report_id && source.run_index == canonical_run_index
        })
        .expect("catalog representative must have a reconciliation source");

    let mut sources = group.reconciliation_sources.clone();
    sources.sort_by(|left, right| {
        (&left.report_id, left.run_index).cmp(&(&right.report_id, right.run_index))
    });
    let participant_character_ids = sources
        .iter()
        .flat_map(|source| source.participant_character_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let participant_character_count = participant_character_ids.len();
    let mut character_ids = sources
        .iter()
        .flat_map(|source| source.public_participant_character_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    character_ids.extend(
        sources
            .iter()
            .flat_map(|source| source.local_profile_witnesses.iter())
            .map(|witness| witness.character_id.clone()),
    );

    let characters = character_ids
        .into_iter()
        .map(|character_id| {
            let participant_report_count = sources
                .iter()
                .filter(|source| source.participant_character_ids.contains(&character_id))
                .count();
            let witnesses = sources
                .iter()
                .filter_map(|source| {
                    let mut snapshots = source
                        .local_profile_witnesses
                        .iter()
                        .filter(|witness| witness.character_id == character_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    let mut state_snapshots = source
                        .local_state_witnesses
                        .iter()
                        .filter(|witness| witness.character_id == character_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    if snapshots.is_empty() && state_snapshots.is_empty() {
                        return None;
                    }
                    snapshots
                        .sort_by_key(|witness| (witness.observed_micros, witness.event_sequence));
                    state_snapshots
                        .sort_by_key(|witness| (witness.observed_micros, witness.event_sequence));
                    Some(PublicCharacterWitnessSource {
                        report_id: source.report_id.clone(),
                        run_index: source.run_index,
                        artifact_sha256: source.artifact_sha256.clone(),
                        snapshots,
                        state_snapshots,
                    })
                })
                .collect::<Vec<_>>();
            let (disposition, selected_report_id) =
                profile_witness_disposition(&witnesses, canonical_report_id);
            PublicReconciliationCharacter {
                character_id,
                participant_report_count,
                disposition,
                selected_report_id,
                state_witness_count: witnesses
                    .iter()
                    .map(|source| source.state_snapshots.len())
                    .sum(),
                game_time_aligned_state_witness_count: witnesses
                    .iter()
                    .flat_map(|source| &source.state_snapshots)
                    .filter(|snapshot| snapshot.game_time_millis.is_some())
                    .count(),
                witnesses,
            }
        })
        .collect::<Vec<_>>();

    let reports = sources
        .iter()
        .map(|source| PublicReconciliationReport {
            report_id: source.report_id.clone(),
            run_index: source.run_index,
            artifact_sha256: source.artifact_sha256.clone(),
            protocol_pack_digest: source.protocol_pack_digest.clone(),
            created_unix_millis: source.created_unix_millis,
            canonical_spine: source.report_id == canonical_report_id
                && source.run_index == canonical_run_index,
            local_profile_witnesses: source.local_profile_witnesses.clone(),
            local_state_witnesses: source.local_state_witnesses.clone(),
        })
        .collect::<Vec<_>>();
    let local_vantage_character_count = reports
        .iter()
        .flat_map(|report| &report.local_profile_witnesses)
        .map(|witness| witness.character_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let complete_local_vantage_coverage = participant_character_count > 0
        && local_vantage_character_count == participant_character_count;
    let (state_replay_readiness, state_replay_blockers) = cross_vantage_state_readiness(
        &reports,
        &characters,
        canonical_report_id,
        complete_local_vantage_coverage,
    );

    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-cross-vantage-reconciliation-v1\0");
    hasher.update(group.representative.run_group_id.as_bytes());
    hasher.update(b"\0");
    for report in &reports {
        hasher.update(report.report_id.as_bytes());
        hasher.update(report.run_index.to_le_bytes());
        hasher.update(report.artifact_sha256.as_bytes());
        hasher.update(report.protocol_pack_digest.as_bytes());
        for witness in &report.local_profile_witnesses {
            hasher.update(witness.character_id.as_bytes());
            hasher.update(match witness.placement {
                LocalStateWitnessPlacement::Unspecified => b"unspecified".as_slice(),
                LocalStateWitnessPlacement::PreRunBaseline => b"pre-run-baseline".as_slice(),
                LocalStateWitnessPlacement::InRun => b"in-run".as_slice(),
            });
            hasher.update(witness.event_sequence.to_le_bytes());
            hasher.update(witness.observed_micros.to_le_bytes());
            hasher.update(witness.game_time_millis.unwrap_or(i64::MIN).to_le_bytes());
            hasher.update(witness.payload_sha256.as_bytes());
        }
        for witness in &report.local_state_witnesses {
            hasher.update(witness.character_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(
                witness
                    .related_character_id
                    .as_deref()
                    .unwrap_or("")
                    .as_bytes(),
            );
            hasher.update(witness.actor_id.to_le_bytes());
            hasher.update(witness.entity_uuid.to_le_bytes());
            hasher.update(match witness.kind {
                LocalStateWitnessKind::EntityAttributes => b"entity-attributes".as_slice(),
                LocalStateWitnessKind::TemporaryAttributes => b"temporary-attributes".as_slice(),
                LocalStateWitnessKind::Resource => b"resource".as_slice(),
                LocalStateWitnessKind::LifeWaveTriggerStatus => {
                    b"life-wave-trigger-status".as_slice()
                }
                LocalStateWitnessKind::LifeWaveTriggerHealing => {
                    b"life-wave-trigger-healing".as_slice()
                }
                LocalStateWitnessKind::StatResonanceStatus => b"stat-resonance-status".as_slice(),
            });
            hasher.update(witness.update_kind.as_bytes());
            hasher.update(match witness.placement {
                LocalStateWitnessPlacement::Unspecified => b"unspecified".as_slice(),
                LocalStateWitnessPlacement::PreRunBaseline => b"pre-run-baseline".as_slice(),
                LocalStateWitnessPlacement::InRun => b"in-run".as_slice(),
            });
            hasher.update(witness.event_sequence.to_le_bytes());
            hasher.update(witness.observed_micros.to_le_bytes());
            hasher.update(witness.game_time_millis.unwrap_or(i64::MIN).to_le_bytes());
            hasher.update(witness.payload_sha256.as_bytes());
        }
    }
    let reconciliation_id = format!("rec_{:x}", hasher.finalize())[..36].to_owned();

    PublicRunReconciliation {
        schema_version: PUBLIC_RECONCILIATION_SCHEMA_VERSION,
        reconciliation_id,
        run_group_id: group.representative.run_group_id.clone(),
        status: reconciliation_status(&reports),
        canonical_spine: PublicCanonicalSpine {
            report_id: canonical.report_id.clone(),
            run_index: canonical.run_index,
            artifact_sha256: canonical.artifact_sha256.clone(),
            authoritative_start: canonical.quality.authoritative_start,
            authoritative_completion: canonical.quality.authoritative_completion,
            data_gap_count: canonical.quality.data_gap_count,
            event_count: canonical.quality.event_count,
        },
        reports,
        characters,
        participant_character_count,
        local_vantage_character_count,
        complete_local_vantage_coverage,
        state_replay_readiness,
        state_replay_blockers,
        verified_state_input_sha256: None,
        reconciled_participants: Vec::new(),
        conservation: None,
        rdps_influences: Vec::new(),
        rdps_effects: Vec::new(),
        swift_vortex_candidate_audit: None,
        attribution_replay_completed: false,
    }
}

fn cross_vantage_state_readiness(
    reports: &[PublicReconciliationReport],
    characters: &[PublicReconciliationCharacter],
    canonical_report_id: &str,
    complete_local_vantage_coverage: bool,
) -> (CrossVantageStateReplayReadiness, Vec<String>) {
    if reports.len() <= 1 {
        return (CrossVantageStateReplayReadiness::SingleVantage, Vec::new());
    }
    let canonical_local_characters = reports
        .iter()
        .find(|report| report.canonical_spine && report.report_id == canonical_report_id)
        .into_iter()
        .flat_map(|report| &report.local_profile_witnesses)
        .map(|witness| witness.character_id.as_str())
        .collect::<BTreeSet<_>>();
    let additional_local_characters = reports
        .iter()
        .filter(|report| report.report_id != canonical_report_id)
        .flat_map(|report| &report.local_profile_witnesses)
        .map(|witness| witness.character_id.as_str())
        .filter(|character_id| !canonical_local_characters.contains(character_id))
        .collect::<BTreeSet<_>>();
    if additional_local_characters.is_empty() {
        return (
            CrossVantageStateReplayReadiness::MultipleReportsNoAdditionalVantage,
            vec!["no_additional_local_vantage".into()],
        );
    }

    let protocol_pack_digests = reports
        .iter()
        .map(|report| report.protocol_pack_digest.as_str())
        .collect::<BTreeSet<_>>();
    if protocol_pack_digests.len() != 1 || protocol_pack_digests.contains("") {
        return (
            CrossVantageStateReplayReadiness::Blocked,
            vec![format!(
                "protocol_pack_digest_mismatch:{}",
                protocol_pack_digests.len()
            )],
        );
    }

    let mut blockers = Vec::new();
    for character_id in additional_local_characters {
        let Some(character) = characters
            .iter()
            .find(|character| character.character_id == character_id)
        else {
            blockers.push(format!("character:{character_id}:missing_manifest_row"));
            continue;
        };
        if character.disposition == ProfileWitnessDisposition::MultipleReportsRequireOrdering {
            blockers.push(format!(
                "character:{character_id}:profile_snapshots_require_ordering"
            ));
            continue;
        }
        let Some(selected_report_id) = character.selected_report_id.as_deref() else {
            blockers.push(format!(
                "character:{character_id}:no_selected_profile_witness"
            ));
            continue;
        };
        let Some(source) = character
            .witnesses
            .iter()
            .find(|source| source.report_id == selected_report_id)
        else {
            blockers.push(format!("character:{character_id}:selected_report_missing"));
            continue;
        };
        if source.state_snapshots.is_empty() {
            blockers.push(format!("character:{character_id}:no_local_state_witness"));
            continue;
        }
        let unspecified_profiles = source
            .snapshots
            .iter()
            .filter(|snapshot| snapshot.placement == LocalStateWitnessPlacement::Unspecified)
            .count();
        if unspecified_profiles > 0 {
            blockers.push(format!(
                "character:{character_id}:unspecified_profile_placement:{unspecified_profiles}"
            ));
        }
        let profiles_missing_game_time = source
            .snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.placement == LocalStateWitnessPlacement::InRun
                    && snapshot.game_time_millis.is_none()
            })
            .count();
        if profiles_missing_game_time > 0 {
            blockers.push(format!(
                "character:{character_id}:in_run_profile_missing_game_time:{profiles_missing_game_time}"
            ));
        }
        let unspecified = source
            .state_snapshots
            .iter()
            .filter(|snapshot| snapshot.placement == LocalStateWitnessPlacement::Unspecified)
            .count();
        if unspecified > 0 {
            blockers.push(format!(
                "character:{character_id}:unspecified_state_placement:{unspecified}"
            ));
        }
        let missing_game_time = source
            .state_snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.placement == LocalStateWitnessPlacement::InRun
                    && snapshot.game_time_millis.is_none()
            })
            .count();
        if missing_game_time > 0 {
            blockers.push(format!(
                "character:{character_id}:in_run_state_missing_game_time:{missing_game_time}"
            ));
        }
    }
    if !blockers.is_empty() {
        return (CrossVantageStateReplayReadiness::Blocked, blockers);
    }
    (
        if complete_local_vantage_coverage {
            CrossVantageStateReplayReadiness::FullCoverageReady
        } else {
            CrossVantageStateReplayReadiness::PartialCoverageReady
        },
        Vec::new(),
    )
}

fn reconciliation_status(
    reports: &[PublicReconciliationReport],
) -> RunAttributionReconciliationStatus {
    if reports.len() <= 1 {
        return RunAttributionReconciliationStatus::SingleVantage;
    }
    let distinct_local_characters = reports
        .iter()
        .flat_map(|report| &report.local_profile_witnesses)
        .map(|witness| witness.character_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    if distinct_local_characters > 1 {
        RunAttributionReconciliationStatus::CrossVantageEvidenceAvailable
    } else {
        RunAttributionReconciliationStatus::MultipleReportsNoAdditionalVantage
    }
}

fn profile_witness_disposition(
    witnesses: &[PublicCharacterWitnessSource],
    canonical_report_id: &str,
) -> (ProfileWitnessDisposition, Option<String>) {
    match witnesses {
        [] => (ProfileWitnessDisposition::Missing, None),
        [only] => (
            ProfileWitnessDisposition::SingleReportExact,
            Some(only.report_id.clone()),
        ),
        _ => {
            let payload_sets = witnesses
                .iter()
                .map(|source| {
                    source
                        .snapshots
                        .iter()
                        .map(|snapshot| {
                            (
                                snapshot.payload_sha256.as_str(),
                                snapshot.placement,
                                snapshot.game_time_millis,
                            )
                        })
                        .collect::<BTreeSet<_>>()
                })
                .collect::<Vec<_>>();
            let identical = payload_sets.windows(2).all(|pair| pair[0] == pair[1]);
            if !identical {
                return (
                    ProfileWitnessDisposition::MultipleReportsRequireOrdering,
                    None,
                );
            }
            let selected = witnesses
                .iter()
                .find(|source| source.report_id == canonical_report_id)
                .unwrap_or(&witnesses[0]);
            (
                ProfileWitnessDisposition::MultipleReportsIdentical,
                Some(selected.report_id.clone()),
            )
        }
    }
}

fn run_group_id(history: &CombatHistorySnapshot, analysis: &RunAnalysis, run_index: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-run-group-v3\0");
    hasher.update(history.deployment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(history.region_id.as_bytes());
    hasher.update(b"\0");
    // An instance number is not assumed to be globally unique forever. The
    // game build prevents historical reuse across patches from collapsing
    // unrelated runs into one evidence group. Protocol-pack identity belongs
    // to replay compatibility: two RLogs versions can observe the same game
    // instance and must still be discoverable as the same run.
    hasher.update(history.client_build.as_bytes());
    hasher.update(b"\0");
    // Exact instance identity is authoritative. Optional world visibility can
    // differ between observers, so including it would split the same run.
    if let Some(scene_id) = analysis.identity.scene_id {
        hasher.update(scene_id.to_le_bytes());
    }
    hasher.update(b"\0");
    if let Some(instance_id) = analysis.identity.instance_id.as_deref() {
        hasher.update(b"exact-instance\0");
        hasher.update(instance_id.as_bytes());
    } else {
        hasher.update(b"isolated-artifact\0");
        hasher.update(analysis.source_session_id.as_bytes());
        hasher.update(run_index.to_le_bytes());
    }
    format!("run_{:x}", hasher.finalize())[..36].to_owned()
}

fn pseudonymous_identifier(prefix: &str, value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-submit-provenance-v1\0");
    hasher.update(value);
    format!("{prefix}_{:x}", hasher.finalize())[..prefix.len() + 33].to_owned()
}

const fn one_usize() -> usize {
    1
}

fn is_public_participant(actor: &HistoryActorSummary) -> bool {
    actor
        .presentation_kind
        .as_deref()
        .or(actor.actor_kind.as_deref())
        .is_some_and(|kind| kind == "player")
}

fn build_private_parse_membership(
    artifact_path: &Path,
    report: &PublicParseReport,
) -> Result<PrivateParseMembership, ServiceError> {
    let character_by_actor = sealed_character_identities(artifact_path)?;
    private_parse_membership(report, &character_by_actor)
}

fn sealed_character_identities(
    artifact_path: &Path,
) -> Result<BTreeMap<String, String>, ServiceError> {
    let file = File::open(artifact_path)?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
    let mut character_by_actor = BTreeMap::<String, String>::new();
    reader.replay(|event| {
        let CanonicalEvent::Timeline(timeline) = &event.event else {
            return Ok(());
        };
        let TimelineEventKind::Actor(actor) = &timeline.kind else {
            return Ok(());
        };
        if actor.kind != ActorKind::Player {
            return Ok(());
        }
        let explicit = actor.character_id.as_deref().filter(|value| !value.is_empty());
        let derived = character_id_from_entity_uuid(actor.actor.entity_uuid.0);
        let character_id = match (explicit, derived.as_deref()) {
            (Some(explicit), Some(derived)) if explicit != derived => {
                return Err(format!(
                    "player actor {} character UID {explicit} disagrees with entity UUID UID {derived}",
                    actor.actor.actor_id.0
                ));
            }
            (Some(explicit), _) => explicit.to_owned(),
            (None, Some(derived)) => derived.to_owned(),
            (None, None) => return Ok(()),
        };
        let actor_id = actor.actor.actor_id.0.to_string();
        match character_by_actor.entry(actor_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(character_id);
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if entry.get() != &character_id =>
            {
                return Err(format!(
                    "runtime actor {} changed stable character UID from {} to {character_id}",
                    entry.key(),
                    entry.get()
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
        Ok(())
    })?;

    Ok(character_by_actor)
}

fn restore_verified_names(
    report: &mut PublicParseReport,
    identities: &BTreeMap<String, String>,
    profiles: &[PublicProfileCatalogEntry],
) -> bool {
    let mut changed = false;
    for participant in report.runs.iter_mut().flat_map(|run| &mut run.participants) {
        if participant
            .display_name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
        {
            continue;
        }
        let Some(uid) = identities.get(&participant.actor_id) else {
            continue;
        };
        if participant
            .character_id
            .as_ref()
            .is_some_and(|id| id != uid)
        {
            continue;
        }
        let names = profiles
            .iter()
            .filter(|profile| {
                profile.claimed
                    && profile.character_id == *uid
                    && profile.deployment == report.deployment_id
                    && (profile.region == report.region_id
                        || report.region_id == "global"
                        || report.region_id == "unknown")
            })
            .filter_map(|profile| profile.display_name.as_deref())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<_>>();
        if names.len() == 1 {
            participant.display_name = names.first().map(|name| (*name).to_owned());
            changed = true;
        }
    }
    changed
}

fn apply_verified_character_keys(
    report: &mut PublicParseReport,
    identities: &BTreeMap<String, String>,
) -> Result<bool, ServiceError> {
    let mut changed = false;
    for participant in report.runs.iter_mut().flat_map(|run| &mut run.participants) {
        let Some(sealed_uid) = identities.get(&participant.actor_id) else {
            continue;
        };
        match participant.character_id.as_deref() {
            Some(public_uid) if public_uid != sealed_uid => {
                return Err(ServiceError::Replay(format!(
                    "report participant {} character UID {public_uid} disagrees with sealed actor UID {sealed_uid}",
                    participant.actor_id
                )));
            }
            Some(_) => {}
            None => {}
        }
        let observed_key = pseudonymous_identifier("chr", sealed_uid.as_bytes());
        if participant.observed_character_key.as_deref() != Some(&observed_key) {
            participant.observed_character_key = Some(observed_key);
            changed = true;
        }
    }
    Ok(changed)
}

fn private_parse_membership(
    report: &PublicParseReport,
    character_by_actor: &BTreeMap<String, String>,
) -> Result<PrivateParseMembership, ServiceError> {
    let runs = report
        .runs
        .iter()
        .map(|run| {
            let mut character_ids = BTreeSet::new();
            for participant in &run.participants {
                let indexed = character_by_actor.get(&participant.actor_id);
                if let (Some(explicit), Some(indexed)) =
                    (participant.character_id.as_deref(), indexed)
                    && explicit != indexed
                {
                    return Err(ServiceError::Replay(format!(
                        "report participant {} character UID {explicit} disagrees with sealed actor UID {indexed}",
                        participant.actor_id
                    )));
                }
                if let Some(character_id) = participant.character_id.as_ref().or(indexed) {
                    character_ids.insert(character_id.clone());
                }
            }
            Ok(PrivateRunMembership {
                run_index: run.run_index,
                character_ids: character_ids.into_iter().collect(),
            })
        })
        .collect::<Result<Vec<_>, ServiceError>>()?;
    Ok(PrivateParseMembership {
        schema_version: PRIVATE_PARSE_MEMBERSHIP_SCHEMA_VERSION,
        report_id: report.report_id.clone(),
        artifact_sha256: report.verification.artifact_sha256.clone(),
        character_by_actor: Some(character_by_actor.clone()),
        runs,
    })
}

fn public_participant(actor: &HistoryActorSummary) -> PublicParticipant {
    PublicParticipant {
        actor_id: actor.actor_id.clone(),
        character_id: actor.character_id.clone(),
        observed_character_key: None,
        display_name: actor
            .presentation_name
            .clone()
            .or_else(|| actor.display_name.clone()),
        actor_kind: actor
            .presentation_kind
            .clone()
            .or_else(|| actor.actor_kind.clone()),
        class_id: actor.class_id,
        class_name: actor
            .class_id
            .and_then(|id| localized_class_name(id, "en-US").ok().flatten())
            .map(str::to_owned)
            .or_else(|| actor.presentation_class_name.clone()),
        specialization_id: actor.specialization_id,
        specialization_name: actor
            .specialization_id
            .and_then(|id| localized_specialization_name(id, "en-US").ok().flatten())
            .map(str::to_owned)
            .or_else(|| actor.presentation_specialization_name.clone()),
        damage: actor.damage,
        dps: actor.dps,
        encounter_dps: actor.encounter_dps,
        hps: actor.hps,
        tps: actor.tps,
        rdps: actor.rdps,
        deaths: actor.deaths,
        death_seconds: actor.death_seconds.clone(),
        abilities: actor
            .abilities
            .iter()
            .map(|ability| PublicAbilitySummary {
                ability_id: ability.ability_id.clone(),
                presentation_name: ability.presentation_name.clone(),
                presentation_kind: ability.presentation_kind.clone(),
                icon_asset_path: ability.icon_asset_path.clone(),
                presentation_recount_group_id: ability.presentation_recount_group_id.clone(),
                presentation_recount_group_name: ability.presentation_recount_group_name.clone(),
                casts: ability.casts,
                hits: ability.hits,
                critical_hits: ability.critical_hits,
                damage: ability.damage,
                effective_damage: ability.effective_damage,
                healing: ability.healing,
                effective_healing: ability.effective_healing,
                shielding: ability.shielding,
            })
            .collect(),
        series: actor
            .series
            .iter()
            .map(|point| PublicSeriesPoint {
                second: point.second,
                damage: point.damage,
                effective_healing: point.effective_healing,
                damage_taken: point.damage_taken,
            })
            .collect(),
    }
}

fn public_rdps_effects(view: &CombatHistoryView) -> Vec<PublicRdpsEffectPresentation> {
    view.rdps_effect_presentations
        .iter()
        .map(|effect| PublicRdpsEffectPresentation {
            effect_id: effect.effect_id.clone(),
            presentation_name: effect.presentation_name.clone(),
            presentation_kind: effect.presentation_kind.clone(),
            icon_asset_path: effect.icon_asset_path.clone(),
        })
        .collect()
}

fn public_rdps_influences(view: &CombatHistoryView) -> Vec<PublicRdpsInfluence> {
    view.damage_influences
        .iter()
        .map(|influence| PublicRdpsInfluence {
            effect_id: influence.effect_id.clone(),
            attribution_component: influence.attribution_component.clone(),
            complete_effect: influence.complete_effect,
            provider_actor_id: influence.provider_actor_id.clone(),
            recipient_actor_id: influence.recipient_actor_id.clone(),
            affected_ability_id: influence.affected_ability_id.clone(),
            target_actor_id: influence.target_actor_id.clone(),
            first_observed_micros: influence.first_observed_micros,
            last_observed_micros: influence.last_observed_micros,
            damage_event_count: influence.damage_event_count,
            critical_hit_count: influence.critical_hit_count,
            observed_damage: influence.observed_damage.clone(),
            exact_integer_delta: influence.exact_integer_delta.clone(),
            exact_rational_deltas: influence
                .exact_rational_deltas
                .iter()
                .map(|delta| PublicRationalDamageDelta {
                    numerator: delta.numerator.clone(),
                    denominator: delta.denominator.clone(),
                    contribution_count: delta.contribution_count,
                })
                .collect(),
            attributed_rdps: influence.attributed_rdps.clone(),
            damage_context_complete: influence.damage_context_complete,
        })
        .collect()
}

fn public_participant_key(participant: &PublicParticipant) -> String {
    // Both sides of this invariant are projections of the same canonical
    // combat spine, so its exact runtime actor ID is the strongest key. A
    // presentation enrichment may attach a character ID to only one side and
    // must not turn that harmless metadata difference into a replay failure.
    format!("actor:{}", participant.actor_id)
}

fn checked_i64_sum(values: impl IntoIterator<Item = i64>) -> Result<i64, ServiceError> {
    values.into_iter().try_fold(0_i64, |total, value| {
        total.checked_add(value).ok_or(ServiceError::SizeOverflow)
    })
}

fn checked_optional_i64_sum(
    values: impl IntoIterator<Item = Option<i64>>,
    field: &str,
) -> Result<i64, ServiceError> {
    let mut total = 0_i64;
    for value in values {
        let value = value.ok_or_else(|| {
            ServiceError::CrossVantageReplay(format!("reconciled participant is missing {field}"))
        })?;
        total = total.checked_add(value).ok_or(ServiceError::SizeOverflow)?;
    }
    Ok(total)
}

fn validate_manifest(manifest: &UploadManifest) -> Result<(), ServiceError> {
    if manifest.chunks.len() > MAXIMUM_UPLOAD_CHUNKS {
        return Err(ServiceError::TooManyChunks {
            actual: manifest.chunks.len(),
            maximum: MAXIMUM_UPLOAD_CHUNKS,
        });
    }
    let digest = manifest
        .sealed_log_digest
        .clone()
        .ok_or(ServiceError::MissingSealedDigest)?;
    let expected_privacy_policy = submission_privacy_policy_digest();
    if manifest.metadata.privacy_policy_digest != expected_privacy_policy {
        return Err(ServiceError::UnsupportedPrivacyPolicy {
            expected: expected_privacy_policy.to_string(),
            actual: manifest.metadata.privacy_policy_digest.to_string(),
        });
    }
    SubmissionSession::new_post_run(manifest.metadata.clone(), manifest.chunks.clone(), digest)?;
    Ok(())
}

fn verify_artifact_metadata(
    manifest: &UploadManifest,
    artifact: &LocalLogArtifact,
) -> Result<(), ServiceError> {
    let metadata = &manifest.metadata;
    if artifact.file_sha256
        != *manifest
            .sealed_log_digest
            .as_ref()
            .ok_or(ServiceError::MissingSealedDigest)?
        || artifact.header.session_id != metadata.capture_session_id
        || artifact.header.schema_version != metadata.log_format_version
        || artifact.header.region.identity.region_id != metadata.game_region
        || artifact.header.region.client_build != metadata.client_build
        || strip_sha256(&artifact.header.region.protocol_pack_digest)
            != metadata.protocol_pack_digest.as_str()
    {
        return Err(ServiceError::ArtifactMetadataMismatch);
    }
    Ok(())
}

fn strip_sha256(value: &str) -> &str {
    value.strip_prefix("sha256:").unwrap_or(value)
}

fn report_id(digest: &Sha256Digest) -> String {
    format!("rpt_{}", &digest.as_str()[..32])
}

fn chunk_file_is_valid(
    directory: &Path,
    descriptor: &rlogs_submission::LogChunkDescriptor,
) -> bool {
    let path = directory.join(format!("chunk-{:08}.bin", descriptor.sequence));
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    u64::try_from(bytes.len()).ok() == Some(descriptor.byte_length)
        && digest_bytes(&bytes).ok().as_ref() == Some(&descriptor.sha256)
}

fn upload_id(digest: &Sha256Digest) -> String {
    format!("up_{}", &digest.as_str()[..32])
}

fn digest_bytes(bytes: &[u8]) -> Result<Sha256Digest, ServiceError> {
    Ok(Sha256Digest::parse(format!("{:x}", Sha256::digest(bytes)))?)
}

fn unix_millis() -> Result<u64, ServiceError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServiceError::ClockBeforeEpoch)?
        .as_millis()
        .try_into()
        .map_err(|_| ServiceError::SizeOverflow)
}

fn validate_identifier(value: &str, label: &'static str) -> Result<(), ServiceError> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ServiceError::InvalidIdentifier(label, value.into()));
    }
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), ServiceError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), ServiceError> {
    let partial = path.with_extension(format!("partial-{}", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(partial, path)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ServiceError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ServiceError::NotFound,
            _ => ServiceError::Io(error),
        })?
        .read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn increment(values: &mut BTreeMap<String, usize>, key: String) {
    *values.entry(key).or_default() += 1;
}

fn facet_values(values: BTreeMap<String, usize>) -> Vec<FacetValue> {
    values
        .into_iter()
        .map(|(id, count)| FacetValue { id, count })
        .collect()
}

fn optional_matches(filter: &Option<String>, actual: &str) -> bool {
    filter.as_ref().is_none_or(|filter| filter == actual)
}

fn optional_matches_option(filter: &Option<String>, actual: &Option<String>) -> bool {
    filter
        .as_ref()
        .is_none_or(|filter| actual.as_ref() == Some(filter))
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("invalid service configuration: {0}")]
    InvalidConfiguration(String),
    #[error("upload manifest is missing its sealed digest")]
    MissingSealedDigest,
    #[error("an interrupted upload with this digest has different metadata")]
    UploadManifestConflict,
    #[error("this interrupted upload belongs to a different authenticated device")]
    UploadOwnerMismatch,
    #[error("authentication returned an invalid submitter or device identity")]
    InvalidAuthenticationIdentity,
    #[error("unknown upload chunk {0}")]
    UnknownChunk(u64),
    #[error("upload chunk length is {actual}; expected {expected}")]
    ChunkLengthMismatch { expected: u64, actual: u64 },
    #[error("upload chunk {0} digest does not match")]
    ChunkDigestMismatch(u64),
    #[error("upload chunk {0} conflicts with an already stored chunk")]
    ConflictingChunk(u64),
    #[error("upload chunk {0} is missing")]
    MissingChunk(u64),
    #[error("upload chunk {0} failed final verification")]
    ChunkVerificationFailed(u64),
    #[error("assembled artifact digest does not match the sealed digest")]
    SealedDigestMismatch,
    #[error("artifact header does not match submission metadata")]
    ArtifactMetadataMismatch,
    #[error("submission privacy policy {actual} is unsupported; expected {expected}")]
    UnsupportedPrivacyPolicy { expected: String, actual: String },
    #[error("game plug-in {0:?} is not supported by this replay worker")]
    UnsupportedGamePlugin(String),
    #[error("server replay failed: {0}")]
    Replay(String),
    #[error(
        "cross-vantage witness {event_sequence} in report {report_id} does not match its sealed artifact commitment"
    )]
    CrossVantageWitnessMismatch {
        report_id: String,
        event_sequence: u64,
    },
    #[error("cross-vantage replay failed: {0}")]
    CrossVantageReplay(String),
    #[error("the sealed log does not contain a completed run")]
    NoCompletedRun,
    #[error("upload manifest contains {actual} chunks; maximum is {maximum}")]
    TooManyChunks { actual: usize, maximum: usize },
    #[error("invalid {0} {1:?}")]
    InvalidIdentifier(&'static str, String),
    #[error("catalog exceeded its safety limit")]
    CatalogTooLarge,
    #[error("private parse membership index failed integrity validation")]
    InvalidMembershipIndex,
    #[error(transparent)]
    Account(#[from] AccountError),
    #[error(transparent)]
    Profile(#[from] ProfileRegistryError),
    #[error("requested resource was not found")]
    NotFound,
    #[error("system clock is before the Unix epoch")]
    ClockBeforeEpoch,
    #[error("byte or time value overflowed")]
    SizeOverflow,
    #[error(transparent)]
    Submission(#[from] rlogs_submission::SubmissionError),
    #[error(transparent)]
    Digest(#[from] rlogs_submission::DigestError),
    #[error(transparent)]
    Rlog(#[from] rlogs_log_format::RlogError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
enum ApiError {
    Unauthorized,
    AuthenticationUnavailable,
    Account(AccountError),
    Service(ServiceError),
}

impl From<ServiceError> for ApiError {
    fn from(value: ServiceError) -> Self {
        Self::Service(value)
    }
}

impl From<AccountError> for ApiError {
    fn from(value: AccountError) -> Self {
        Self::Account(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "write authorization failed".into(),
            ),
            Self::AuthenticationUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "write authentication is temporarily unavailable".into(),
            ),
            Self::Account(AccountError::NotConfigured) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "account authentication is not configured".into(),
            ),
            Self::Account(AccountError::Unauthorized) => (
                StatusCode::UNAUTHORIZED,
                "account authentication failed".into(),
            ),
            Self::Account(AccountError::InvalidOrExpiredCode) => (
                StatusCode::BAD_REQUEST,
                "login code is invalid or expired".into(),
            ),
            Self::Account(AccountError::DiscordUnavailable) => (
                StatusCode::BAD_GATEWAY,
                "Discord authentication is temporarily unavailable".into(),
            ),
            Self::Account(AccountError::InvalidUsername) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "username must be 3-24 letters, numbers, hyphens, or underscores".into(),
            ),
            Self::Account(AccountError::UsernameUnavailable) => (
                StatusCode::CONFLICT,
                "that username is already in use".into(),
            ),
            Self::Account(error) => (StatusCode::BAD_REQUEST, error.to_string()),
            Self::Service(ServiceError::NotFound) => {
                (StatusCode::NOT_FOUND, "resource not found".into())
            }
            Self::Service(ServiceError::Profile(ProfileRegistryError::NotFound)) => {
                (StatusCode::NOT_FOUND, "profile not found".into())
            }
            Self::Service(ServiceError::Profile(ProfileRegistryError::AuthenticationRequired)) => (
                StatusCode::UNAUTHORIZED,
                "profile publication requires authentication".into(),
            ),
            Self::Service(ServiceError::Profile(ProfileRegistryError::ClaimConflict {
                character_id,
            })) => (
                StatusCode::CONFLICT,
                format!("UID {character_id} is already claimed by another user"),
            ),
            Self::Service(ServiceError::Profile(ProfileRegistryError::StalePackage)) => (
                StatusCode::CONFLICT,
                "a newer profile is already published".into(),
            ),
            Self::Service(ServiceError::Profile(ProfileRegistryError::PhotoNotObserved {
                photo_id,
            })) => (
                StatusCode::CONFLICT,
                format!("photo {photo_id} has not been observed on this profile"),
            ),
            Self::Service(ServiceError::Profile(ProfileRegistryError::InvalidPhotoAsset(
                message,
            ))) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            Self::Service(error) => (StatusCode::BAD_REQUEST, error.to_string()),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_report_without_projection_revision_is_stale() {
        let report =
            fixture_public_report("rpt_11111111111111111111111111111111", "character-a", 0);
        let mut encoded = serde_json::to_value(&report).unwrap();
        encoded
            .as_object_mut()
            .unwrap()
            .remove("projection_revision");

        let legacy: PublicParseReport = serde_json::from_value(encoded).unwrap();

        assert_eq!(legacy.projection_revision, 0);
        assert!(!report_projection_is_current(&legacy));
        assert!(report_projection_is_current(&report));
    }

    #[test]
    fn account_opt_in_promotes_only_verified_projection_visibility() {
        assert_eq!(
            verified_report_visibility(ReportVisibility::Unlisted, true),
            ReportVisibility::Public
        );
        assert_eq!(
            verified_report_visibility(ReportVisibility::Private, false),
            ReportVisibility::Private
        );
    }

    #[test]
    fn interrupted_upload_is_bound_to_one_device_identity() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let upload_id = "up_11111111111111111111111111111111";
        let directory = service.upload_directory(upload_id).unwrap();
        std::fs::create_dir(&directory).unwrap();
        let owner = UploadOwner {
            schema_version: UPLOAD_OWNER_SCHEMA_VERSION,
            submitter_id: Some("submitter-one".into()),
            device_id: Some("device-one".into()),
            authentication: "device_token".into(),
        };
        write_json_atomic(&directory.join("owner.json"), &owner).unwrap();

        service.require_upload_owner(upload_id, &owner).unwrap();
        let mut other_device = owner.clone();
        other_device.device_id = Some("device-two".into());
        assert!(matches!(
            service.require_upload_owner(upload_id, &other_device),
            Err(ServiceError::UploadOwnerMismatch)
        ));
    }

    #[test]
    fn introspection_rejects_plain_http_outside_loopback() {
        assert!(matches!(
            SubmissionAuthentication::introspection("http://example.test/v1/auth/introspect"),
            Err(ServiceError::InvalidConfiguration(_))
        ));
    }

    #[tokio::test]
    async fn introspection_returns_token_free_upload_identity() {
        async fn mock_introspection(headers: HeaderMap) -> impl IntoResponse {
            if bearer_token(&headers) != Some("test-device-token") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            Json(serde_json::json!({
                "schema_version": 1,
                "active": true,
                "submitter_id": "submitter-one",
                "device_id": "device-one",
                "authentication": "device_token"
            }))
            .into_response()
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/auth/introspect", post(mock_introspection)),
            )
            .await
            .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let authentication = SubmissionAuthentication::introspection(&format!(
            "http://{address}/v1/auth/introspect"
        ))
        .unwrap();
        let service = SubmissionService::open_with_authentication(
            root.path().into(),
            "https://example.test".into(),
            authentication,
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer test-device-token".parse().unwrap());

        let owner = authorize(&service, &headers).await.unwrap();
        assert_eq!(owner.submitter_id.as_deref(), Some("submitter-one"));
        assert_eq!(owner.device_id.as_deref(), Some("device-one"));
        assert_eq!(owner.authentication, "device_token");
        server.abort();
    }

    #[test]
    fn catalog_facets_are_derived_from_entries() {
        let entries = vec![PublicParseCatalogEntry {
            report_id: "rpt_a".into(),
            report_ids: vec!["rpt_a".into()],
            run_index: 0,
            run_group_id: "run_a".into(),
            contribution_count: 1,
            distinct_submitter_count: 1,
            local_profile_witness_character_count: 1,
            attribution_reconciliation_status: RunAttributionReconciliationStatus::SingleVantage,
            created_unix_millis: 1,
            submitter_id: None,
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: Some("chaotic".into()),
            activity_category_id: Some("dungeons".into()),
            scene_id: Some(6565),
            scene_name: Some("Chaotic - Sea-Ringed Reef".into()),
            difficulty_family: Some("master".into()),
            difficulty_tier: Some(5),
            terminal_state: "completed".into(),
            total_run_time_micros: Some(10),
            participant_count: 5,
        }];
        let facets = CatalogFacets::from_entries(&entries);
        assert_eq!(facets.regions[0].id, "north-america");
        assert_eq!(facets.activities[0].id, "dungeons");
        assert_eq!(facets.scenes[0].id, 6565);
        assert_eq!(facets.difficulties[0].id, "master");
    }

    #[test]
    fn catalog_activity_filter_uses_the_broad_category_not_the_scene_key() {
        let entry = PublicParseCatalogEntry {
            report_id: "rpt_test".into(),
            report_ids: vec!["rpt_test".into()],
            run_index: 0,
            run_group_id: "run_test".into(),
            contribution_count: 1,
            distinct_submitter_count: 1,
            local_profile_witness_character_count: 0,
            attribution_reconciliation_status: RunAttributionReconciliationStatus::SingleVantage,
            created_unix_millis: 1,
            submitter_id: None,
            deployment_id: "global".into(),
            region_id: "global".into(),
            activity_id: Some("scene.32160".into()),
            activity_family_id: Some("stimen-vaults".into()),
            activity_category_id: None,
            scene_id: Some(32160),
            scene_name: Some("Floor 60".into()),
            difficulty_family: None,
            difficulty_tier: None,
            terminal_state: "completed".into(),
            total_run_time_micros: Some(10),
            participant_count: 5,
        };

        let facets = CatalogFacets::from_entries(std::slice::from_ref(&entry));
        assert_eq!(facets.activities.len(), 1);
        assert_eq!(facets.activities[0].id, "stimens");
        assert!(
            CatalogQuery {
                activity: Some("stimens".into()),
                ..CatalogQuery::default()
            }
            .matches(&entry)
        );
        assert!(
            !CatalogQuery {
                activity: Some("scene.32160".into()),
                ..CatalogQuery::default()
            }
            .matches(&entry)
        );
    }

    #[test]
    fn community_milestones_keep_the_first_authoritative_clear_per_character_and_scene() {
        let source = |report: &str,
                      created: u64,
                      category: &str,
                      difficulty: &str,
                      tier: Option<u32>,
                      authoritative_completion: bool| {
            MilestoneSource {
                entry: PublicParseCatalogEntry {
                    report_id: report.into(),
                    report_ids: vec![report.into()],
                    run_index: 0,
                    run_group_id: format!("run_{report}"),
                    contribution_count: 1,
                    distinct_submitter_count: 1,
                    local_profile_witness_character_count: 1,
                    attribution_reconciliation_status:
                        RunAttributionReconciliationStatus::SingleVantage,
                    created_unix_millis: created,
                    submitter_id: None,
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    activity_id: Some("scene.6500".into()),
                    activity_family_id: Some("test".into()),
                    activity_category_id: Some(category.into()),
                    scene_id: Some(6500),
                    scene_name: Some("Test activity".into()),
                    difficulty_family: Some(difficulty.into()),
                    difficulty_tier: tier,
                    terminal_state: "completed".into(),
                    total_run_time_micros: Some(90_000_000),
                    participant_count: 1,
                },
                authoritative_completion,
                participants: vec![PublicParticipant {
                    actor_id: "1".into(),
                    character_id: Some("3296036".into()),
                    observed_character_key: None,
                    display_name: Some("MarieRose".into()),
                    actor_kind: Some("player".into()),
                    class_id: None,
                    class_name: None,
                    specialization_id: None,
                    specialization_name: None,
                    damage: 1,
                    dps: 1.0,
                    encounter_dps: 1.0,
                    hps: 0.0,
                    tps: 0.0,
                    rdps: None,
                    deaths: 0,
                    death_seconds: Vec::new(),
                    abilities: Vec::new(),
                    series: Vec::new(),
                }],
            }
        };
        let catalog = build_community_milestone_catalog(vec![
            source("rpt_later", 200, "dungeons", "master", Some(20), true),
            source("rpt_first", 100, "dungeons", "master", Some(20), true),
            source("rpt_unverified", 50, "dungeons", "master", Some(20), false),
            source("rpt_nightmare", 300, "raids", "nightmare", None, true),
            source("rpt_not_m20", 400, "dungeons", "master", Some(19), true),
        ]);

        assert_eq!(catalog.total_entries, 2);
        assert_eq!(
            catalog.entries[0].kind,
            CommunityMilestoneKind::NightmareRaid
        );
        assert_eq!(catalog.entries[1].report_id, "rpt_first");
        assert_eq!(catalog.entries[1].completed_unix_millis, 100);
    }

    #[test]
    fn query_filters_without_a_hand_authored_dungeon_list() {
        let entry = PublicParseCatalogEntry {
            report_id: "rpt_a".into(),
            report_ids: vec!["rpt_a".into()],
            run_index: 0,
            run_group_id: "run_a".into(),
            contribution_count: 1,
            distinct_submitter_count: 1,
            local_profile_witness_character_count: 1,
            attribution_reconciliation_status: RunAttributionReconciliationStatus::SingleVantage,
            created_unix_millis: 1,
            submitter_id: None,
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: None,
            activity_category_id: None,
            scene_id: Some(6565),
            scene_name: None,
            difficulty_family: Some("master".into()),
            difficulty_tier: Some(5),
            terminal_state: "completed".into(),
            total_run_time_micros: None,
            participant_count: 5,
        };
        assert!(
            CatalogQuery {
                region: Some("north-america".into()),
                scene: Some(6565),
                ..CatalogQuery::default()
            }
            .matches(&entry)
        );
        assert!(
            !CatalogQuery {
                region: Some("asia".into()),
                ..CatalogQuery::default()
            }
            .matches(&entry)
        );
    }

    #[test]
    fn report_ids_are_deterministic_and_non_path_like() {
        let digest = Sha256Digest::parse("ab".repeat(32)).unwrap();
        assert_eq!(report_id(&digest), "rpt_abababababababababababababababab");
        assert_eq!(upload_id(&digest), "up_abababababababababababababababab");
    }

    #[test]
    fn run_groups_use_exact_game_instance_identity_not_parser_version() {
        let mut history = CombatHistorySnapshot {
            schema_version: 1,
            session_id: "history".into(),
            deployment_id: "global".into(),
            region_id: "asteria".into(),
            world_id: Some("world-1".into()),
            client_build: "test".into(),
            protocol_pack_digest: "test".into(),
            rdps_formula_identity: None,
            runs: Vec::new(),
        };
        let mut analysis = fixture_analysis("capture-a", Some("instance-42"));
        let exact_a = run_group_id(&history, &analysis, 0);
        analysis.source_session_id = "capture-b".into();
        history.world_id = None;
        let exact_b = run_group_id(&history, &analysis, 0);
        assert_eq!(exact_a, exact_b);

        history.client_build = "next-build".into();
        let other_build = run_group_id(&history, &analysis, 0);
        assert_ne!(exact_a, other_build);
        history.client_build = "test".into();

        history.protocol_pack_digest = "next-protocol".into();
        let other_protocol = run_group_id(&history, &analysis, 0);
        assert_eq!(exact_a, other_protocol);
        history.protocol_pack_digest = "test".into();

        analysis.identity.scene_id = Some(6566);
        let other_scene = run_group_id(&history, &analysis, 0);
        assert_ne!(exact_a, other_scene);
        analysis.identity.scene_id = Some(6565);

        analysis.identity.instance_id = Some("instance-43".into());
        let other_instance = run_group_id(&history, &analysis, 0);
        assert_ne!(exact_a, other_instance);
        analysis.identity.instance_id = Some("instance-42".into());

        history.region_id = "other-region".into();
        let other_region = run_group_id(&history, &analysis, 0);
        assert_ne!(exact_a, other_region);
        history.region_id = "asteria".into();

        history.deployment_id = "other-deployment".into();
        let other_deployment = run_group_id(&history, &analysis, 0);
        assert_ne!(exact_a, other_deployment);
        history.deployment_id = "global".into();

        analysis.identity.instance_id = None;
        let isolated_b = run_group_id(&history, &analysis, 0);
        analysis.source_session_id = "capture-c".into();
        let isolated_c = run_group_id(&history, &analysis, 0);
        assert_ne!(isolated_b, isolated_c);
    }

    fn fixture_analysis(source_session_id: &str, instance_id: Option<&str>) -> RunAnalysis {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "source_session_id": source_session_id,
            "identity": {
                "activity_kind": "dungeon",
                "scene_id": 6565,
                "instance_id": instance_id
            },
            "terminal_state": "completed",
            "authoritative_start": true,
            "authoritative_completion": true,
            "timing": {
                "started_micros": 1,
                "ended_micros": 2,
                "observed_until_micros": 2,
                "wall_time_micros": 1,
                "active_combat_micros": 1,
                "noncombat_micros": 0,
                "manual_pause_micros": 0
            },
            "segments": [],
            "encounters": [],
            "manual_pauses": [],
            "data_gap_count": 0,
            "findings": [],
            "submission_disposition": "rank_candidate"
        }))
        .unwrap()
    }

    #[test]
    fn run_scoped_profile_witnesses_keep_only_personal_state_relevant_to_the_run() {
        let mut analysis = fixture_analysis("capture-a", Some("instance-42"));
        analysis.timing.started_micros = 10;
        analysis.timing.ended_micros = Some(40);
        analysis.timing.observed_until_micros = 40;
        analysis.encounters.push(
            serde_json::from_value(serde_json::json!({
                "index": 0,
                "encounter_id": "boss-1",
                "kind": "boss",
                "segment_index": 0,
                "attempt_number": 1,
                "is_retry": false,
                "is_successful_attempt": true,
                "terminal_state": "cleared",
                "started_micros": 20,
                "ended_micros": 40,
                "wall_time_micros": 20,
                "active_combat_micros": 20,
                "combat_windows": [{
                    "started_micros": 20,
                    "ended_micros": 40,
                    "duration_micros": 20,
                    "closed_at_boundary": true
                }],
                "closed_at_run_end": true
            }))
            .unwrap(),
        );
        let participants = ["character-a".to_owned()].into_iter().collect();
        let observations = vec![
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 1,
                observed_micros: 0,
                game_time_millis: None,
                payload_sha256: "sha256:old".into(),
                loadout: ProfileLoadoutObservation::default(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 2,
                observed_micros: 15,
                game_time_millis: None,
                payload_sha256: "sha256:pre-pull".into(),
                loadout: ProfileLoadoutObservation::default(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 3,
                observed_micros: 25,
                game_time_millis: Some(25),
                payload_sha256: "sha256:first-combat-loadout".into(),
                loadout: ProfileLoadoutObservation {
                    display_name: Some("Player".into()),
                    class_id: Some(5),
                    specialization_id: Some(2),
                    equipped_skill_ids: vec!["2203291".into()],
                    equipment_count: Some(11),
                    equipped_module_count: Some(8),
                    talent_count: Some(12),
                    ..Default::default()
                },
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 4,
                observed_micros: 35,
                game_time_millis: Some(35),
                payload_sha256: "sha256:boss-swap".into(),
                loadout: ProfileLoadoutObservation {
                    display_name: Some("Player".into()),
                    class_id: Some(2),
                    specialization_id: Some(1),
                    equipped_skill_ids: vec!["1714".into()],
                    equipment_count: Some(11),
                    equipped_module_count: Some(8),
                    talent_count: Some(12),
                    ..Default::default()
                },
            },
            LocalProfileObservation {
                character_id: "not-a-participant".into(),
                event_sequence: 5,
                observed_micros: 30,
                game_time_millis: Some(30),
                payload_sha256: "sha256:unrelated".into(),
                loadout: ProfileLoadoutObservation::default(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 6,
                observed_micros: 45,
                game_time_millis: Some(45),
                payload_sha256: "sha256:after-completion".into(),
                loadout: ProfileLoadoutObservation::default(),
            },
        ];

        let selected = run_scoped_profile_witnesses(&analysis, &participants, &observations);
        assert_eq!(
            selected
                .iter()
                .map(|witness| witness.event_sequence)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert_eq!(
            selected
                .iter()
                .map(|witness| witness.placement)
                .collect::<Vec<_>>(),
            vec![LocalStateWitnessPlacement::InRun; 2]
        );
        assert!(selected.iter().all(|witness| witness.event_sequence != 2));
        assert!(selected.iter().all(|witness| witness.event_sequence != 6));

        let phases = run_scoped_combat_loadout_phases(&analysis, &participants, &observations);
        assert_eq!(phases.len(), 2);
        assert_eq!(
            phases
                .iter()
                .map(|phase| phase.class_id)
                .collect::<Vec<_>>(),
            vec![Some(5), Some(2)]
        );
        assert_eq!(
            phases
                .iter()
                .map(|phase| phase.run_elapsed_micros)
                .collect::<Vec<_>>(),
            vec![15, 25]
        );
        assert!(phases.iter().all(|phase| phase.in_active_combat));
        assert_eq!(phases[0].equipped_skill_ids, vec!["2203291"]);
        assert_eq!(phases[1].equipped_skill_ids, vec!["1714"]);
    }

    #[test]
    fn run_scoped_attribute_witnesses_replay_from_latest_pre_run_snapshot() {
        let analysis = fixture_analysis("capture-a", Some("instance-42"));
        let local_characters = ["character-a".to_owned()].into_iter().collect();
        let observation = |sequence: u64,
                           observed_micros: u64,
                           update_kind: EntityAttributeUpdateKind|
         -> LocalStateObservation {
            LocalStateObservation {
                character_id: "character-a".into(),
                related_character_id: None,
                raw: RawLocalStateObservation {
                    actor_id: 8,
                    entity_uuid: 80,
                    kind: LocalStateWitnessKind::EntityAttributes,
                    update_kind,
                    event_sequence: sequence,
                    observed_micros,
                    game_time_millis: Some(observed_micros as i64),
                    payload_sha256: format!("sha256:{sequence}"),
                    wire: None,
                    related_entity_uuid: None,
                },
            }
        };
        let observations = vec![
            observation(1, 0, EntityAttributeUpdateKind::Snapshot),
            observation(2, 1, EntityAttributeUpdateKind::Snapshot),
            observation(3, 2, EntityAttributeUpdateKind::Delta),
            observation(4, 3, EntityAttributeUpdateKind::Delta),
        ];

        let selected = run_scoped_state_witnesses(&analysis, &local_characters, &observations);
        assert_eq!(
            selected
                .iter()
                .map(|witness| witness.event_sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(
            selected
                .iter()
                .all(|witness| witness.game_time_millis.is_some())
        );
    }

    #[test]
    fn profile_witness_selection_is_fail_closed_across_reports() {
        let source = |report_id: &str, payloads: &[&str]| PublicCharacterWitnessSource {
            report_id: report_id.into(),
            run_index: 0,
            artifact_sha256: format!("sha256:{report_id}"),
            snapshots: payloads
                .iter()
                .enumerate()
                .map(|(index, payload)| PublicLocalProfileWitness {
                    character_id: "character-a".into(),
                    placement: LocalStateWitnessPlacement::PreRunBaseline,
                    event_sequence: index as u64 + 1,
                    observed_micros: index as u64 + 1,
                    game_time_millis: None,
                    payload_sha256: (*payload).into(),
                })
                .collect(),
            state_snapshots: Vec::new(),
        };

        assert_eq!(
            profile_witness_disposition(&[], "rpt_a"),
            (ProfileWitnessDisposition::Missing, None)
        );
        assert_eq!(
            profile_witness_disposition(&[source("rpt_a", &["sha256:one"])], "rpt_a"),
            (
                ProfileWitnessDisposition::SingleReportExact,
                Some("rpt_a".into())
            )
        );
        assert_eq!(
            profile_witness_disposition(
                &[
                    source("rpt_a", &["sha256:one"]),
                    source("rpt_b", &["sha256:one"]),
                ],
                "rpt_b",
            ),
            (
                ProfileWitnessDisposition::MultipleReportsIdentical,
                Some("rpt_b".into())
            )
        );
        assert_eq!(
            profile_witness_disposition(
                &[
                    source("rpt_a", &["sha256:one"]),
                    source("rpt_b", &["sha256:two"]),
                ],
                "rpt_a",
            ),
            (
                ProfileWitnessDisposition::MultipleReportsRequireOrdering,
                None
            )
        );
    }

    #[test]
    fn catalog_rebuild_materializes_cross_vantage_reconciliation_without_double_counting() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let report_a =
            fixture_public_report("rpt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "character-a", 2);
        let report_b =
            fixture_public_report("rpt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "character-b", 0);
        write_json_atomic(
            &service.projection_path(&report_a.report_id).unwrap(),
            &report_a,
        )
        .unwrap();
        write_json_atomic(
            &service.projection_path(&report_b.report_id).unwrap(),
            &report_b,
        )
        .unwrap();

        service.rebuild_catalog_locked().unwrap();
        let catalog = service.catalog(&CatalogQuery::default()).unwrap();
        assert_eq!(catalog.entries.len(), 1);
        let entry = &catalog.entries[0];
        assert_eq!(entry.contribution_count, 2);
        assert_eq!(entry.local_profile_witness_character_count, 2);
        assert_eq!(
            entry.attribution_reconciliation_status,
            RunAttributionReconciliationStatus::CrossVantageEvidenceAvailable
        );

        let reconciliation = service
            .reconciliation("run_exact000000000000000000000000000")
            .unwrap();
        assert_eq!(
            reconciliation.canonical_spine.report_id,
            "rpt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(reconciliation.reports.len(), 2);
        assert_eq!(reconciliation.characters.len(), 2);
        assert_eq!(reconciliation.participant_character_count, 2);
        assert_eq!(reconciliation.local_vantage_character_count, 2);
        assert!(reconciliation.complete_local_vantage_coverage);
        assert_eq!(
            reconciliation.state_replay_readiness,
            CrossVantageStateReplayReadiness::FullCoverageReady
        );
        assert!(reconciliation.state_replay_blockers.is_empty());
        assert!(reconciliation.characters.iter().all(|character| {
            character.disposition == ProfileWitnessDisposition::SingleReportExact
                && character.selected_report_id.is_some()
        }));
        assert!(!reconciliation.attribution_replay_completed);
    }

    #[test]
    fn sealed_membership_counts_redacted_participants_without_publishing_remote_uids() {
        let mut report =
            fixture_public_report("rpt_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "character-a", 0);
        for participant in &mut report.runs[0].participants {
            participant.character_id = None;
        }
        let private_membership = PrivateRunMembership {
            run_index: 0,
            character_ids: vec!["character-a".into(), "character-b".into()],
        };
        let group = CatalogRunGroup {
            representative: PublicParseCatalogEntry::from_report(&report, &report.runs[0]),
            representative_quality: CanonicalSpineQuality::from_report(&report, &report.runs[0]),
            submitters: BTreeSet::new(),
            local_profile_witnesses: ["character-a".to_owned()].into_iter().collect(),
            reconciliation_sources: vec![ReconciliationRunSource::from_report(
                &report,
                &report.runs[0],
                Some(&private_membership),
            )],
            milestone_source: MilestoneSource {
                entry: PublicParseCatalogEntry::from_report(&report, &report.runs[0]),
                authoritative_completion: report.runs[0].authoritative_completion,
                participants: report.runs[0].participants.clone(),
            },
        };

        let reconciliation = build_public_reconciliation(&group);

        assert_eq!(reconciliation.participant_character_count, 2);
        assert_eq!(reconciliation.local_vantage_character_count, 1);
        assert!(!reconciliation.complete_local_vantage_coverage);
        assert_eq!(reconciliation.characters.len(), 1);
        assert_eq!(reconciliation.characters[0].character_id, "character-a");
        assert!(
            reconciliation
                .characters
                .iter()
                .all(|character| character.character_id != "character-b")
        );
    }

    #[test]
    fn startup_rebuilds_a_stale_derived_catalog_schema() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        write_json_atomic(
            &service.catalog_path(),
            &PublicParseCatalog {
                schema_version: PUBLIC_CATALOG_SCHEMA_VERSION - 1,
                total_entries: 0,
                offset: 0,
                next_offset: None,
                entries: Vec::new(),
                facets: CatalogFacets::default(),
            },
        )
        .unwrap();
        drop(service);

        let reopened =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let catalog: PublicParseCatalog = read_json(&reopened.catalog_path()).unwrap();

        assert_eq!(catalog.schema_version, PUBLIC_CATALOG_SCHEMA_VERSION);
    }

    #[test]
    fn duplicate_reports_from_one_local_character_are_not_cross_vantage() {
        let report = |report_id: &str| PublicReconciliationReport {
            report_id: report_id.into(),
            run_index: 0,
            artifact_sha256: format!("sha256:{report_id}"),
            protocol_pack_digest: "sha256:pack".into(),
            created_unix_millis: 1,
            canonical_spine: report_id.ends_with('a'),
            local_profile_witnesses: vec![PublicLocalProfileWitness {
                character_id: "same-character".into(),
                placement: LocalStateWitnessPlacement::PreRunBaseline,
                event_sequence: 1,
                observed_micros: 1,
                game_time_millis: None,
                payload_sha256: "sha256:profile".into(),
            }],
            local_state_witnesses: Vec::new(),
        };
        assert_eq!(
            reconciliation_status(&[report("rpt_a"), report("rpt_b")]),
            RunAttributionReconciliationStatus::MultipleReportsNoAdditionalVantage
        );
    }

    #[test]
    fn in_run_cross_vantage_state_without_game_time_blocks_replay() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let report_a =
            fixture_public_report("rpt_cccccccccccccccccccccccccccccccc", "character-a", 0);
        let mut report_b =
            fixture_public_report("rpt_dddddddddddddddddddddddddddddddd", "character-b", 0);
        let state = &mut report_b.runs[0].local_state_witnesses[0];
        state.placement = LocalStateWitnessPlacement::InRun;
        state.game_time_millis = None;
        write_json_atomic(
            &service.projection_path(&report_a.report_id).unwrap(),
            &report_a,
        )
        .unwrap();
        write_json_atomic(
            &service.projection_path(&report_b.report_id).unwrap(),
            &report_b,
        )
        .unwrap();

        service.rebuild_catalog_locked().unwrap();
        let reconciliation = service
            .reconciliation("run_exact000000000000000000000000000")
            .unwrap();
        assert_eq!(
            reconciliation.state_replay_readiness,
            CrossVantageStateReplayReadiness::Blocked
        );
        assert_eq!(
            reconciliation.state_replay_blockers,
            vec!["character:character-b:in_run_state_missing_game_time:1"]
        );
    }

    #[test]
    fn same_game_run_with_different_protocol_packs_groups_but_blocks_joint_replay() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let report_a =
            fixture_public_report("rpt_12121212121212121212121212121212", "character-a", 0);
        let mut report_b =
            fixture_public_report("rpt_34343434343434343434343434343434", "character-b", 0);
        report_b.protocol_pack_digest = "sha256:different-pack".into();
        write_json_atomic(
            &service.projection_path(&report_a.report_id).unwrap(),
            &report_a,
        )
        .unwrap();
        write_json_atomic(
            &service.projection_path(&report_b.report_id).unwrap(),
            &report_b,
        )
        .unwrap();

        service.rebuild_catalog_locked().unwrap();
        let catalog = service.catalog(&CatalogQuery::default()).unwrap();
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].contribution_count, 2);
        let reconciliation = service
            .reconciliation("run_exact000000000000000000000000000")
            .unwrap();
        assert_eq!(
            reconciliation.state_replay_readiness,
            CrossVantageStateReplayReadiness::Blocked
        );
        assert_eq!(
            reconciliation.state_replay_blockers,
            vec!["protocol_pack_digest_mismatch:2"]
        );
        assert!(!reconciliation.attribution_replay_completed);
    }

    fn cross_vantage_test_region() -> rlogs_events::RegionContext {
        rlogs_events::RegionContext {
            identity: rlogs_events::RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: Some("world-1".into()),
            },
            client_build: "24687926".into(),
            protocol_pack_digest: "sha256:test-pack".into(),
            evidence: Vec::new(),
        }
    }

    fn cross_vantage_timeline_envelope(
        sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        kind: TimelineEventKind,
    ) -> EventEnvelope {
        let time = rlogs_events::EventTime {
            observed_micros,
            game_time_millis,
        };
        let provenance = EventProvenance::wire(sequence, 1, 1);
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "canonical-session".into(),
            sequence,
            region: cross_vantage_test_region(),
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(rlogs_events::TimelineEvent {
                sequence,
                time,
                provenance,
                kind,
            }),
        }
    }

    fn cross_vantage_attribute_envelope(
        sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        value: i64,
    ) -> EventEnvelope {
        cross_vantage_attributes_envelope(
            sequence,
            observed_micros,
            game_time_millis,
            &[(11320, value)],
        )
    }

    fn cross_vantage_attributes_envelope(
        sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        values: &[(i32, i64)],
    ) -> EventEnvelope {
        cross_vantage_timeline_envelope(
            sequence,
            observed_micros,
            game_time_millis,
            TimelineEventKind::EntityAttributes(rlogs_events::EntityAttributeEvent {
                actor: EntityRef {
                    actor_id: rlogs_events::ActorId(22),
                    entity_uuid: rlogs_events::EntityUuid(222),
                },
                update_kind: EntityAttributeUpdateKind::Snapshot,
                ownership: None,
                attributes: values
                    .iter()
                    .map(|(attribute_id, value)| rlogs_events::EntityAttribute {
                        attribute_id: *attribute_id,
                        raw_value: value.to_le_bytes().to_vec(),
                        decoded: Some(rlogs_events::EntityAttributeValue::Integer(*value)),
                    })
                    .collect(),
            }),
        )
    }

    fn cross_vantage_resource_envelope(
        sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        resource_value: u32,
    ) -> EventEnvelope {
        cross_vantage_timeline_envelope(
            sequence,
            observed_micros,
            game_time_millis,
            TimelineEventKind::Resource(rlogs_events::ResourceEvent {
                actor: EntityRef {
                    actor_id: rlogs_events::ActorId(22),
                    entity_uuid: rlogs_events::EntityUuid(222),
                },
                update_kind: EntityAttributeUpdateKind::Snapshot,
                origin_energy_raw_bits: Some(100.0_f32.to_bits()),
                resource_ids: vec![7],
                resource_values: vec![resource_value],
                cooldowns: vec![rlogs_events::ResourceCooldown {
                    resource_id: Some(7),
                    begin_time_millis: Some(650),
                    duration_millis: Some(1_500),
                    valid_cooldown_time_millis: Some(1_500),
                    existence_time_millis: Some(2_000),
                }],
            }),
        )
    }

    fn cross_vantage_profile_envelope(
        sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        character_id: &str,
    ) -> EventEnvelope {
        let region = cross_vantage_test_region();
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "secondary-session".into(),
            sequence,
            region: region.clone(),
            time: rlogs_events::EventTime {
                observed_micros,
                game_time_millis,
            },
            provenance: EventProvenance::wire(sequence, 2, 2),
            sensitivity: EventSensitivity::PersonalGameplay,
            event: CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                    payload_schema_id: rlogs_game_bpsr::BPSR_PROFILE_SCHEMA_ID.into(),
                    payload_schema_version: rlogs_game_bpsr::BPSR_PROFILE_SCHEMA_VERSION,
                    character: rlogs_events::CharacterIdentity {
                        region: region.identity,
                        character_id: character_id.into(),
                    },
                    payload: serde_json::json!({}),
                }),
            },
        }
    }

    fn cross_vantage_life_wave_profile_envelope(
        sequence: u64,
        observed_micros: u64,
        character_id: &str,
    ) -> EventEnvelope {
        let mut envelope =
            cross_vantage_profile_envelope(sequence, observed_micros, None, character_id);
        let CanonicalEvent::CharacterProfileObserved { profile } = &mut envelope.event else {
            unreachable!();
        };
        profile.payload = serde_json::json!({
            "character": profile.character.clone(),
            "modules": {
                "equipped_slots": { "1": "life-wave-module" },
                "inventory": [{
                    "instance_id": "life-wave-module",
                    "config_id": 5500101,
                    "count": 1,
                    "quality": null,
                    "load_flag": null,
                    "module_type": null,
                    "level": null,
                    "parts": [{
                        "part_id": 2404,
                        "initial_link_points": 20
                    }],
                    "upgrade_records": [],
                    "success_rate": null
                }]
            }
        });
        envelope
    }

    fn cross_vantage_actor_envelope(
        sequence: u64,
        observed_micros: u64,
        actor_id: u64,
        entity_uuid: i64,
        character_id: &str,
    ) -> EventEnvelope {
        cross_vantage_timeline_envelope(
            sequence,
            observed_micros,
            Some(observed_micros as i64),
            TimelineEventKind::Actor(rlogs_events::ActorEvent {
                actor: EntityRef {
                    actor_id: rlogs_events::ActorId(actor_id),
                    entity_uuid: rlogs_events::EntityUuid(entity_uuid),
                },
                state: rlogs_events::ActorState::Spawned,
                entity_type_id: 1,
                kind: rlogs_events::ActorKind::Player,
                monster_id: None,
                character_id: Some(character_id.into()),
                display_name: Some(character_id.into()),
                class_id: Some(4),
                specialization_id: Some(2),
                level: Some(80),
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_score: None,
                primary_loadout: Vec::new(),
                auxiliary_loadout: Vec::new(),
                loadout_observation: rlogs_events::ActorLoadoutObservation::default(),
            }),
        )
    }

    fn cross_vantage_dungeon_envelope(
        sequence: u64,
        observed_micros: u64,
        kind: rlogs_events::DungeonEventKind,
    ) -> EventEnvelope {
        let time = rlogs_events::EventTime {
            observed_micros,
            game_time_millis: Some(observed_micros as i64),
        };
        let provenance = EventProvenance::wire(sequence, 1, 1);
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "canonical-session".into(),
            sequence,
            region: cross_vantage_test_region(),
            time,
            provenance,
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Dungeon(rlogs_events::DungeonEvent {
                kind,
                dungeon_id: Some(rlogs_events::DungeonId(7152)),
                instance_id: Some("instance-test".into()),
                difficulty_id: None,
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        }
    }

    fn cross_vantage_monster_envelope(sequence: u64, observed_micros: u64) -> EventEnvelope {
        let mut envelope = cross_vantage_actor_envelope(
            sequence,
            observed_micros,
            99,
            999,
            "temporary-monster-key",
        );
        let CanonicalEvent::Timeline(timeline) = &mut envelope.event else {
            unreachable!();
        };
        let TimelineEventKind::Actor(actor) = &mut timeline.kind else {
            unreachable!();
        };
        actor.kind = rlogs_events::ActorKind::Monster;
        actor.monster_id = Some(rlogs_events::MonsterId(9001));
        actor.character_id = None;
        actor.display_name = Some("fixture monster".into());
        actor.class_id = None;
        actor.specialization_id = None;
        envelope
    }

    fn cross_vantage_damage_envelope(
        sequence: u64,
        observed_micros: u64,
        amount: i64,
    ) -> EventEnvelope {
        cross_vantage_timeline_envelope(
            sequence,
            observed_micros,
            Some(i64::try_from(observed_micros / 1_000).unwrap()),
            TimelineEventKind::Damage(rlogs_events::DamageEvent {
                source: EntityRef {
                    actor_id: rlogs_events::ActorId(22),
                    entity_uuid: rlogs_events::EntityUuid(222),
                },
                direct_source: None,
                target: EntityRef {
                    actor_id: rlogs_events::ActorId(99),
                    entity_uuid: rlogs_events::EntityUuid(999),
                },
                ability: Some(rlogs_events::AbilityId(1001)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: Some(0),
                hit_event_id: Some(1),
                damage_source: Some(1),
                damage_type: Some(1),
                flags: rlogs_events::DamageFlags {
                    critical: Some(true),
                    ..Default::default()
                },
                packet: rlogs_events::DamagePacketDetail::default(),
            }),
        )
    }

    #[test]
    fn sealed_state_witness_commitment_is_verified_before_import() {
        let envelope = cross_vantage_attribute_envelope(7, 70, Some(700), 1234);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            unreachable!();
        };
        let payload = serde_json::to_vec(&timeline.kind).unwrap();
        let witness = PublicLocalStateWitness {
            character_id: "character-b".into(),
            related_character_id: None,
            actor_id: 22,
            entity_uuid: 222,
            kind: LocalStateWitnessKind::EntityAttributes,
            update_kind: "snapshot".into(),
            placement: LocalStateWitnessPlacement::InRun,
            event_sequence: 7,
            observed_micros: 70,
            game_time_millis: Some(700),
            payload_sha256: local_state_payload_digest(&payload),
        };
        verify_state_witness_event("rpt_b", &witness, &envelope).unwrap();

        let mut tampered = envelope;
        let CanonicalEvent::Timeline(timeline) = &mut tampered.event else {
            unreachable!();
        };
        let TimelineEventKind::EntityAttributes(attributes) = &mut timeline.kind else {
            unreachable!();
        };
        attributes.attributes[0].decoded = Some(rlogs_events::EntityAttributeValue::Integer(1235));
        assert!(matches!(
            verify_state_witness_event("rpt_b", &witness, &tampered),
            Err(ServiceError::CrossVantageWitnessMismatch { .. })
        ));
    }

    #[test]
    fn sealed_resource_witness_requires_the_exact_local_resource_payload() {
        let envelope = cross_vantage_resource_envelope(9, 90, Some(900), 80);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            unreachable!();
        };
        let witness = PublicLocalStateWitness {
            character_id: "character-b".into(),
            related_character_id: None,
            actor_id: 22,
            entity_uuid: 222,
            kind: LocalStateWitnessKind::Resource,
            update_kind: "snapshot".into(),
            placement: LocalStateWitnessPlacement::InRun,
            event_sequence: 9,
            observed_micros: 90,
            game_time_millis: Some(900),
            payload_sha256: local_state_payload_digest(
                &serde_json::to_vec(&timeline.kind).unwrap(),
            ),
        };
        verify_state_witness_event("rpt_b", &witness, &envelope).unwrap();

        let mut tampered = envelope;
        let CanonicalEvent::Timeline(timeline) = &mut tampered.event else {
            unreachable!();
        };
        let TimelineEventKind::Resource(resource) = &mut timeline.kind else {
            unreachable!();
        };
        resource.resource_values[0] = 81;
        assert!(matches!(
            verify_state_witness_event("rpt_b", &witness, &tampered),
            Err(ServiceError::CrossVantageWitnessMismatch { .. })
        ));
    }

    #[test]
    fn sealed_life_wave_healing_witness_requires_exact_event_and_stable_healer_identity() {
        let target = EntityRef {
            actor_id: rlogs_events::ActorId(22),
            entity_uuid: rlogs_events::EntityUuid(222),
        };
        let source = EntityRef {
            actor_id: rlogs_events::ActorId(33),
            entity_uuid: rlogs_events::EntityUuid(333),
        };
        let envelope = cross_vantage_timeline_envelope(
            8,
            80,
            Some(800),
            TimelineEventKind::Healing(rlogs_events::HealingEvent {
                source,
                direct_source: None,
                target,
                ability: Some(rlogs_events::AbilityId(123)),
                amount: 500,
                actual_amount: None,
                hp_loss: None,
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                effective_amount: Some(400),
                overheal: Some(100),
                critical: None,
                periodic: None,
                packet: rlogs_events::DamagePacketDetail::default(),
            }),
        );
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            unreachable!();
        };
        let witness = PublicLocalStateWitness {
            character_id: "character-b".into(),
            related_character_id: Some("character-a".into()),
            actor_id: 22,
            entity_uuid: 222,
            kind: LocalStateWitnessKind::LifeWaveTriggerHealing,
            update_kind: "unknown".into(),
            placement: LocalStateWitnessPlacement::InRun,
            event_sequence: 8,
            observed_micros: 80,
            game_time_millis: Some(800),
            payload_sha256: local_state_payload_digest(
                &serde_json::to_vec(&timeline.kind).unwrap(),
            ),
        };
        verify_state_witness_event("rpt_b", &witness, &envelope).unwrap();
        let characters = BTreeMap::from([(222, "character-b".into()), (333, "character-a".into())]);
        verify_state_witness_characters("rpt_b", "character-b", &witness, &envelope, &characters)
            .unwrap();

        let mut wrong_healer = witness.clone();
        wrong_healer.related_character_id = Some("character-c".into());
        assert!(matches!(
            verify_state_witness_characters(
                "rpt_b",
                "character-b",
                &wrong_healer,
                &envelope,
                &characters,
            ),
            Err(ServiceError::CrossVantageWitnessMismatch { .. })
        ));
    }

    #[test]
    fn cross_vantage_state_uses_baseline_then_strict_server_time_ordering() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("canonical.rlog");
        let header = rlogs_log_format::RlogHeader::new(
            "canonical-session",
            cross_vantage_test_region(),
            "unit-test",
        );
        let mut writer = rlogs_log_format::RlogWriter::new(Vec::new(), header).unwrap();
        let events = [
            cross_vantage_timeline_envelope(
                1,
                10_000,
                Some(90),
                TimelineEventKind::RunBoundary {
                    state: RunState::Entered,
                    scene_id: Some(rlogs_events::SceneId(7152)),
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_timeline_envelope(
                2,
                20_000,
                Some(100),
                TimelineEventKind::CombatBoundary {
                    state: rlogs_events::CombatState::Started,
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_timeline_envelope(
                3,
                30_000,
                Some(101),
                TimelineEventKind::CombatBoundary {
                    state: rlogs_events::CombatState::Ended,
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_timeline_envelope(
                4,
                40_000,
                Some(102),
                TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    scene_id: Some(rlogs_events::SceneId(7152)),
                    reason: rlogs_events::BoundaryReason::Completion,
                },
            ),
        ];
        for event in &events {
            writer.push(event).unwrap();
        }
        std::fs::write(&path, writer.finish().unwrap()).unwrap();

        let imported = vec![
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_b".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::PreRunBaseline,
                game_time_millis: Some(70),
                envelope: cross_vantage_profile_envelope(89, 4, Some(70), "character-b"),
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_b".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::PreRunBaseline,
                game_time_millis: Some(80),
                envelope: cross_vantage_attribute_envelope(90, 5, Some(80), 1000),
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_b".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::InRun,
                game_time_millis: Some(100),
                envelope: cross_vantage_attribute_envelope(91, 15, Some(100), 1100),
            },
        ];
        let mut imported = imported;
        remap_cross_vantage_state_entities(
            &mut imported,
            &BTreeMap::from([(
                "character-b".into(),
                EntityRef {
                    actor_id: rlogs_events::ActorId(22),
                    entity_uuid: rlogs_events::EntityUuid(222),
                },
            )]),
        )
        .unwrap();
        let mut order = Vec::new();
        let mut imported_observed = BTreeMap::new();
        replay_canonical_with_cross_vantage_state(&path, 0, &imported, |event, imported| {
            if imported {
                imported_observed.insert(event.sequence, event.time.observed_micros);
            }
            order.push(format!(
                "{}{}",
                if imported { "I" } else { "C" },
                event.sequence
            ));
            Ok(())
        })
        .unwrap();
        assert_eq!(order, vec!["C1", "I89", "I90", "C2", "I91", "C3", "C4"]);
        assert_eq!(imported_observed.get(&91), Some(&29_000));
    }

    #[test]
    fn joint_replay_attributes_life_wave_and_preserves_party_conservation() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let mut region = cross_vantage_test_region();
        region.protocol_pack_digest =
            "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395".into();
        let header =
            rlogs_log_format::RlogHeader::new("canonical-session", region.clone(), "unit-test");
        let mut writer = rlogs_log_format::RlogWriter::new(Vec::new(), header).unwrap();
        let mut events = vec![
            cross_vantage_dungeon_envelope(1, 5, rlogs_events::DungeonEventKind::Started),
            cross_vantage_timeline_envelope(
                2,
                10,
                Some(10),
                TimelineEventKind::RunBoundary {
                    state: RunState::Entered,
                    scene_id: Some(rlogs_events::SceneId(7152)),
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_actor_envelope(3, 20, 11, 111, "character-a"),
            cross_vantage_actor_envelope(4, 30, 22, 222, "character-b"),
            cross_vantage_monster_envelope(5, 35),
            cross_vantage_timeline_envelope(
                6,
                40,
                Some(40),
                TimelineEventKind::CombatBoundary {
                    state: rlogs_events::CombatState::Started,
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_damage_envelope(7, 45_000, 100),
            cross_vantage_timeline_envelope(
                8,
                50_000,
                Some(50),
                TimelineEventKind::CombatBoundary {
                    state: rlogs_events::CombatState::Ended,
                    reason: rlogs_events::BoundaryReason::AuthoritativePacket,
                },
            ),
            cross_vantage_timeline_envelope(
                9,
                60_000,
                Some(60),
                TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    scene_id: Some(rlogs_events::SceneId(7152)),
                    reason: rlogs_events::BoundaryReason::Completion,
                },
            ),
            cross_vantage_dungeon_envelope(10, 65_000, rlogs_events::DungeonEventKind::Completed),
        ];
        let mut timeline_sequence = 0_u64;
        for event in &mut events {
            event.region = region.clone();
            if let CanonicalEvent::Timeline(timeline) = &mut event.event {
                timeline_sequence += 1;
                timeline.sequence = timeline_sequence;
            }
            writer.push(event).unwrap();
        }
        let bytes = writer.finish().unwrap();
        let digest = digest_bytes(&bytes).unwrap();
        let artifact_path = service.artifact_path(&digest).unwrap();
        std::fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        std::fs::write(&artifact_path, bytes).unwrap();

        let report_id = "rpt_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let mut report = fixture_public_report(report_id, "character-a", 0);
        report.protocol_pack_digest = region.protocol_pack_digest.clone();
        report.verification.artifact_sha256 = digest.to_string();
        report.verification.event_count = 10;
        report.runs[0].local_state_witnesses.clear();
        for (actor_id, character_id) in [("11", "character-a"), ("22", "character-b")] {
            let participant = report.runs[0]
                .participants
                .iter_mut()
                .find(|participant| participant.character_id.as_deref() == Some(character_id))
                .unwrap();
            participant.actor_id = actor_id.into();
        }
        report.runs[0]
            .participants
            .iter_mut()
            .find(|participant| participant.character_id.as_deref() == Some("character-b"))
            .unwrap()
            .damage = 100;
        write_json_atomic(&service.projection_path(report_id).unwrap(), &report).unwrap();
        let group = CatalogRunGroup {
            representative: PublicParseCatalogEntry::from_report(&report, &report.runs[0]),
            representative_quality: CanonicalSpineQuality::from_report(&report, &report.runs[0]),
            submitters: BTreeSet::new(),
            local_profile_witnesses: ["character-a".to_owned()].into_iter().collect(),
            reconciliation_sources: vec![ReconciliationRunSource::from_report(
                &report,
                &report.runs[0],
                None,
            )],
            milestone_source: MilestoneSource {
                entry: PublicParseCatalogEntry::from_report(&report, &report.runs[0]),
                authoritative_completion: report.runs[0].authoritative_completion,
                participants: report.runs[0].participants.clone(),
            },
        };
        let reconciliation = build_public_reconciliation(&group);
        let target = EntityRef {
            actor_id: rlogs_events::ActorId(220),
            entity_uuid: rlogs_events::EntityUuid(222),
        };
        let source = EntityRef {
            actor_id: rlogs_events::ActorId(110),
            entity_uuid: rlogs_events::EntityUuid(111),
        };
        let mut trigger_status = cross_vantage_timeline_envelope(
            100,
            42,
            Some(42),
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(target),
                target,
                effect: rlogs_events::StatusEffectId(LIFE_WAVE_EFFECT_ID),
                instance_id: Some(rlogs_events::StatusEffectInstanceId(700)),
                origin: Some(rlogs_events::StatusOrigin {
                    source_type_id: LIFE_WAVE_SOURCE_TYPE_ID,
                    source_config_id: LIFE_WAVE_SOURCE_CONFIG_ID,
                }),
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: Some(LIFE_WAVE_DURATION_MILLIS),
                level: Some(1),
                part_id: None,
                count: Some(1),
                created_at_millis: None,
            }),
        );
        let mut trigger_healing = cross_vantage_timeline_envelope(
            101,
            42,
            Some(42),
            TimelineEventKind::Healing(rlogs_events::HealingEvent {
                source,
                direct_source: None,
                target,
                ability: Some(rlogs_events::AbilityId(123)),
                amount: 500,
                actual_amount: None,
                hp_loss: None,
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                effective_amount: Some(500),
                overheal: Some(0),
                critical: None,
                periodic: None,
                packet: rlogs_events::DamagePacketDetail::default(),
            }),
        );
        for envelope in [&mut trigger_status, &mut trigger_healing] {
            let provenance = EventProvenance::wire(500, 9, 9);
            envelope.provenance = provenance.clone();
            let CanonicalEvent::Timeline(timeline) = &mut envelope.event else {
                unreachable!();
            };
            timeline.provenance = provenance;
        }
        let imported = vec![
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_secondary".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::PreRunBaseline,
                game_time_millis: None,
                envelope: cross_vantage_life_wave_profile_envelope(1, 1, "character-b"),
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_secondary".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::InRun,
                game_time_millis: Some(41),
                envelope: cross_vantage_attributes_envelope(
                    2,
                    41,
                    Some(41),
                    &[
                        (11710, 5_000),
                        (11712, 5_000),
                        (11780, 2_000),
                        (11782, 2_000),
                        (11940, 2_000),
                        (11942, 2_000),
                        (11950, 2_000),
                        (11952, 2_000),
                        (11840, 700),
                        (11930, 2_000),
                        (12510, 5_000),
                        (12530, 3_000),
                    ],
                ),
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_secondary".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::InRun,
                game_time_millis: Some(42),
                envelope: trigger_status,
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_secondary".into(),
                character_id: "character-b".into(),
                related_character_id: Some("character-a".into()),
                placement: LocalStateWitnessPlacement::InRun,
                game_time_millis: Some(42),
                envelope: trigger_healing,
            },
            VerifiedCrossVantageStateEvent {
                report_id: "rpt_secondary".into(),
                character_id: "character-b".into(),
                related_character_id: None,
                placement: LocalStateWitnessPlacement::InRun,
                game_time_millis: Some(43),
                envelope: cross_vantage_attributes_envelope(
                    102,
                    43,
                    Some(43),
                    &[
                        (11710, 6_000),
                        (11712, 6_000),
                        (11780, 2_000),
                        (11782, 2_000),
                        (11940, 2_000),
                        (11942, 2_000),
                        (11950, 2_000),
                        (11952, 2_000),
                        (11840, 700),
                        (11930, 2_000),
                        (12510, 5_000),
                        (12530, 3_000),
                    ],
                ),
            },
        ];
        let result = service
            .replay_cross_vantage_attribution(&reconciliation, imported)
            .unwrap();
        assert!(result.conservation.conserved);
        assert_eq!(result.conservation.raw_damage, 100);
        assert_eq!(result.conservation.rdps_damage, 100);
        assert!(result.conservation.contribution_given > 0);
        assert_eq!(
            result.conservation.contribution_given,
            result.conservation.contribution_received
        );
        assert_eq!(result.participants.len(), 2);
        let provider = result
            .participants
            .iter()
            .find(|participant| participant.participant.actor_id == "11")
            .unwrap();
        let recipient = result
            .participants
            .iter()
            .find(|participant| participant.participant.actor_id == "22")
            .unwrap();
        assert_eq!(
            provider.contribution_given,
            Some(result.conservation.contribution_given)
        );
        assert_eq!(
            provider.rdps_damage,
            Some(result.conservation.contribution_given)
        );
        assert_eq!(
            recipient.contribution_received,
            Some(result.conservation.contribution_received)
        );
        assert_eq!(
            recipient.rdps_damage,
            Some(100 - result.conservation.contribution_received)
        );
        assert!(!provider.rdps_incomplete);
        assert!(!recipient.rdps_incomplete);
    }

    #[test]
    fn my_parses_include_unlisted_participation_and_account_owned_private_reports() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let mut unlisted =
            fixture_public_report("rpt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "3296036", 0);
        unlisted.visibility = ReportVisibility::Unlisted;
        unlisted.submission_provenance.submitter_id = Some("someone-else".into());
        let mut private_other =
            fixture_public_report("rpt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "3296036", 0);
        private_other.visibility = ReportVisibility::Private;
        private_other.submission_provenance.submitter_id = Some("someone-else".into());
        let mut private_owned =
            fixture_public_report("rpt_cccccccccccccccccccccccccccccccc", "other-character", 0);
        private_owned.visibility = ReportVisibility::Private;
        private_owned.submission_provenance.submitter_id = Some("account-one".into());

        for report in [&unlisted, &private_other, &private_owned] {
            write_json_atomic(&service.projection_path(&report.report_id).unwrap(), report)
                .unwrap();
            let character_ids = report.runs[0]
                .participants
                .iter()
                .filter_map(|participant| participant.character_id.clone())
                .collect();
            write_json_atomic(
                &service.membership_path(&report.report_id).unwrap(),
                &PrivateParseMembership {
                    schema_version: PRIVATE_PARSE_MEMBERSHIP_SCHEMA_VERSION,
                    report_id: report.report_id.clone(),
                    artifact_sha256: report.verification.artifact_sha256.clone(),
                    character_by_actor: None,
                    runs: vec![PrivateRunMembership {
                        run_index: 0,
                        character_ids,
                    }],
                },
            )
            .unwrap();
        }

        let catalog = service
            .my_parse_catalog_for_character_ids(
                "account-one",
                &["character-a".to_owned()].into_iter().collect(),
                &MyParsesQuery::default(),
            )
            .unwrap();
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.parse.report_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                unlisted.report_id.as_str(),
                private_owned.report_id.as_str()
            ]
        );
        assert!(catalog.entries.iter().any(|entry| {
            entry.parse.report_id == unlisted.report_id
                && !entry.submitted_by_you
                && entry.matched_character_ids == ["character-a"]
        }));
        assert!(catalog.entries.iter().any(|entry| {
            entry.parse.report_id == private_owned.report_id && entry.submitted_by_you
        }));
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| entry.parse.report_id != private_other.report_id)
        );
        assert!(matches!(
            service.account_report(&private_other.report_id, "account-one"),
            Err(ServiceError::NotFound)
        ));
        assert_eq!(
            service
                .account_report(&private_owned.report_id, "account-one")
                .unwrap()
                .report_id,
            private_owned.report_id
        );
    }

    #[test]
    fn report_owner_can_change_visibility_and_public_catalog_updates_atomically() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let mut report =
            fixture_public_report("rpt_dddddddddddddddddddddddddddddddd", "3296036", 0);
        report.visibility = ReportVisibility::Unlisted;
        report.submission_provenance.submitter_id = Some("account-one".into());
        write_json_atomic(
            &service.projection_path(&report.report_id).unwrap(),
            &report,
        )
        .unwrap();

        assert!(matches!(
            service.update_report_visibility(
                &report.report_id,
                "account-two",
                ReportVisibility::Public,
            ),
            Err(ServiceError::NotFound)
        ));
        let published = service
            .update_report_visibility(&report.report_id, "account-one", ReportVisibility::Public)
            .unwrap();
        assert_eq!(published.visibility, ReportVisibility::Public);
        assert!(published.share_url.is_some());
        assert_eq!(
            service
                .catalog(&CatalogQuery::default())
                .unwrap()
                .total_entries,
            1
        );

        let private = service
            .update_report_visibility(&report.report_id, "account-one", ReportVisibility::Private)
            .unwrap();
        assert_eq!(private.visibility, ReportVisibility::Private);
        assert!(private.share_url.is_none());
        assert_eq!(
            service
                .catalog(&CatalogQuery::default())
                .unwrap()
                .total_entries,
            0
        );
        assert!(matches!(
            service.report(&report.report_id),
            Err(ServiceError::NotFound)
        ));
        assert_eq!(
            service
                .account_report(&report.report_id, "account-one")
                .unwrap()
                .visibility,
            ReportVisibility::Private
        );
    }

    #[test]
    fn missing_name_uses_sealed_uid_and_verified_owner_profile() {
        let mut report = fixture_public_report("identity", "5", 0);
        report.region_id = "global".into();
        report.runs[0].participants.truncate(1);
        report.runs[0].participants[0].actor_id = "5".into();
        report.runs[0].participants[0].character_id = None;
        let identities = BTreeMap::from([("5".into(), "3296036".into())]);
        let mut profile = PublicProfileCatalogEntry {
            profile_id: "profile".into(),
            claimed: true,
            package_id: "package".into(),
            updated_unix_millis: 1,
            source_client_build: "build".into(),
            deployment: "global".into(),
            region: "north-america".into(),
            realm: None,
            world: None,
            character_id: "999".into(),
            display_name: Some("MarieRose".into()),
            module_inventory_count: 0,
            equipped_module_count: 0,
        };
        assert!(!restore_verified_names(
            &mut report,
            &identities,
            &[profile.clone()]
        ));
        profile.character_id = "3296036".into();
        let mut conflicting = profile.clone();
        conflicting.display_name = Some("Another name".into());
        assert!(!restore_verified_names(
            &mut report,
            &identities,
            &[profile.clone(), conflicting]
        ));
        assert!(restore_verified_names(
            &mut report,
            &identities,
            &[profile.clone()]
        ));
        let participant = &report.runs[0].participants[0];
        assert_eq!(participant.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(participant.character_id, None);
        profile.display_name = Some("Newer name".into());
        assert!(!restore_verified_names(
            &mut report,
            &identities,
            &[profile]
        ));
        assert_eq!(
            report.runs[0].participants[0].display_name.as_deref(),
            Some("MarieRose")
        );
    }

    #[test]
    fn sealed_actor_identity_publishes_only_an_opaque_participant_key() {
        let mut report = fixture_public_report("identity", "3296036", 0);
        report.runs[0].participants.truncate(1);
        report.runs[0].participants[0].actor_id = "5".into();
        report.runs[0].participants[0].character_id = None;
        let identities = BTreeMap::from([("5".into(), "3296036".into())]);

        assert!(apply_verified_character_keys(&mut report, &identities).unwrap());
        let participant = &report.runs[0].participants[0];
        assert_eq!(participant.character_id, None);
        assert_eq!(
            participant.observed_character_key.as_deref(),
            Some(pseudonymous_identifier("chr", b"3296036").as_str())
        );
        assert!(!apply_verified_character_keys(&mut report, &identities).unwrap());
    }

    #[test]
    fn conflicting_public_and_sealed_character_ids_fail_closed() {
        let mut report = fixture_public_report("identity-conflict", "3296036", 0);
        report.runs[0].participants.truncate(1);
        report.runs[0].participants[0].actor_id = "5".into();
        report.runs[0].participants[0].character_id = Some("not-marie".into());
        let identities = BTreeMap::from([("5".into(), "3296036".into())]);

        assert!(matches!(
            apply_verified_character_keys(&mut report, &identities),
            Err(ServiceError::Replay(_))
        ));
    }

    fn fixture_public_report(
        report_id: &str,
        local_character_id: &str,
        data_gap_count: u64,
    ) -> PublicParseReport {
        let participant = |character_id: &str| PublicParticipant {
            actor_id: character_id.into(),
            character_id: Some(character_id.into()),
            observed_character_key: None,
            display_name: None,
            actor_kind: Some("player".into()),
            class_id: None,
            class_name: None,
            specialization_id: None,
            specialization_name: None,
            damage: 0,
            dps: 0.0,
            encounter_dps: 0.0,
            hps: 0.0,
            tps: 0.0,
            rdps: None,
            deaths: 0,
            death_seconds: Vec::new(),
            abilities: Vec::new(),
            series: Vec::new(),
        };
        PublicParseReport {
            schema_version: PUBLIC_PARSE_SCHEMA_VERSION,
            projection_revision: PUBLIC_PARSE_PROJECTION_REVISION,
            report_id: report_id.into(),
            visibility: ReportVisibility::Public,
            created_unix_millis: 1,
            game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            world_id: None,
            client_build: "24687926".into(),
            protocol_pack_digest: "sha256:pack".into(),
            verification: PublicVerification {
                tier: VerificationTier::Replayed,
                artifact_sha256: format!("sha256:artifact-{report_id}"),
                canonical_content_sha256: format!("sha256:content-{report_id}"),
                event_count: 100,
                privacy_policy_digest: "sha256:privacy".into(),
            },
            submission_provenance: PublicSubmissionProvenance {
                submitter_id: Some(format!("submitter-{local_character_id}")),
                authentication: "device_token".into(),
            },
            runs: vec![PublicRun {
                run_index: 0,
                run_group_id: "run_exact000000000000000000000000000".into(),
                correlation_method: RunCorrelationMethod::ExactInstanceId,
                activity_id: None,
                activity_family_id: None,
                activity_category_id: None,
                scene_id: Some(7152),
                scene_name: None,
                difficulty_family: None,
                difficulty_tier: None,
                terminal_state: "completed".into(),
                total_run_time_micros: Some(10),
                game_time_micros: Some(10),
                active_combat_micros: 10,
                true_time_micros: None,
                retry_count: 0,
                boss_retry_count: 0,
                rdps_status: "pending_cross_vantage".into(),
                data_gap_count,
                authoritative_start: true,
                authoritative_completion: true,
                submission_disposition: RunSubmissionDisposition::RankCandidate,
                local_profile_character_ids: vec![local_character_id.into()],
                local_profile_witnesses: vec![PublicLocalProfileWitness {
                    character_id: local_character_id.into(),
                    placement: LocalStateWitnessPlacement::PreRunBaseline,
                    event_sequence: 1,
                    observed_micros: 1,
                    game_time_millis: None,
                    payload_sha256: format!("sha256:profile-{local_character_id}"),
                }],
                local_state_witnesses: vec![PublicLocalStateWitness {
                    character_id: local_character_id.into(),
                    related_character_id: None,
                    actor_id: 8,
                    entity_uuid: 216_009_015_936,
                    kind: LocalStateWitnessKind::EntityAttributes,
                    update_kind: "snapshot".into(),
                    placement: LocalStateWitnessPlacement::PreRunBaseline,
                    event_sequence: 2,
                    observed_micros: 1,
                    game_time_millis: Some(100),
                    payload_sha256: format!("sha256:state-{local_character_id}"),
                }],
                combat_loadout_phases: Vec::new(),
                segments: Vec::new(),
                participants: vec![participant("character-a"), participant("character-b")],
                rdps_influences: Vec::new(),
                rdps_effects: Vec::new(),
            }],
        }
    }

    #[test]
    fn observed_character_catalog_merges_opaque_identity_and_claims_profile() {
        let character_id = "3296036";
        let observed_key = pseudonymous_identifier("chr", character_id.as_bytes());
        let participant = |name: &str| PublicParticipant {
            actor_id: "actor-1".into(),
            character_id: None,
            observed_character_key: Some(observed_key.clone()),
            display_name: Some(name.into()),
            actor_kind: Some("player".into()),
            class_id: Some(4),
            class_name: Some("Marksman".into()),
            specialization_id: Some(2),
            specialization_name: Some("Falconry Spec".into()),
            damage: 1,
            dps: 1.0,
            encounter_dps: 1.0,
            hps: 0.0,
            tps: 0.0,
            rdps: None,
            deaths: 0,
            death_seconds: Vec::new(),
            abilities: Vec::new(),
            series: Vec::new(),
        };
        let source = |index: u32, created: u64, name: &str| MilestoneSource {
            entry: PublicParseCatalogEntry {
                report_id: format!("rpt_{index:032x}"),
                report_ids: vec![format!("rpt_{index:032x}")],
                run_index: index,
                run_group_id: format!("run_{index:032x}"),
                contribution_count: 1,
                distinct_submitter_count: 1,
                local_profile_witness_character_count: 0,
                attribution_reconciliation_status:
                    RunAttributionReconciliationStatus::SingleVantage,
                created_unix_millis: created,
                submitter_id: None,
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                activity_id: None,
                activity_family_id: None,
                activity_category_id: None,
                scene_id: Some(1),
                scene_name: Some("Test scene".into()),
                difficulty_family: None,
                difficulty_tier: None,
                terminal_state: "completed".into(),
                total_run_time_micros: Some(1),
                participant_count: 1,
            },
            authoritative_completion: true,
            participants: vec![participant(name)],
        };
        let profile = PublicProfileCatalogEntry {
            profile_id: "3296036".into(),
            claimed: true,
            package_id: "pkg_test".into(),
            updated_unix_millis: 3,
            source_client_build: "test".into(),
            deployment: "global".into(),
            region: "north-america".into(),
            realm: None,
            world: None,
            character_id: character_id.into(),
            display_name: Some("MarieRose".into()),
            module_inventory_count: 0,
            equipped_module_count: 0,
        };

        let catalog = build_observed_character_catalog(
            &[source(1, 10, "MarieRose"), source(2, 20, "MarieRose")],
            &[profile],
        );

        assert_eq!(catalog.total_characters, 1);
        let character = &catalog.characters[0];
        assert_eq!(character.observed_character_key, observed_key);
        assert_eq!(character.character_id.as_deref(), Some(character_id));
        assert_eq!(character.claimed_profile_id.as_deref(), Some("3296036"));
        assert_eq!(
            character.identity_kind,
            ObservedCharacterIdentityKind::VerifiedUid
        );
        assert_eq!(character.report_count, 2);
        assert_eq!(character.reports[0].created_unix_millis, 20);
    }

    #[test]
    fn catalog_paginates_after_filtering_and_keeps_full_facets() {
        let root = tempfile::tempdir().unwrap();
        let service =
            SubmissionService::open(root.path().into(), "https://example.test".into(), None)
                .unwrap();
        let make_entry = |index: u32, region: &str| PublicParseCatalogEntry {
            report_id: format!("rpt_{index:032x}"),
            report_ids: vec![format!("rpt_{index:032x}")],
            run_index: index,
            run_group_id: format!("run_{index:032x}"),
            contribution_count: 1,
            distinct_submitter_count: 1,
            local_profile_witness_character_count: 1,
            attribution_reconciliation_status: RunAttributionReconciliationStatus::SingleVantage,
            created_unix_millis: u64::from(index),
            submitter_id: None,
            deployment_id: "global".into(),
            region_id: region.into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: Some("chaotic".into()),
            activity_category_id: Some("dungeons".into()),
            scene_id: Some(6565),
            scene_name: Some("Chaotic - Sea-Ringed Reef".into()),
            difficulty_family: Some("master".into()),
            difficulty_tier: Some(5),
            terminal_state: "completed".into(),
            total_run_time_micros: Some(10),
            participant_count: 5,
        };
        let entries = vec![
            make_entry(3, "global"),
            make_entry(2, "global"),
            make_entry(1, "cn"),
        ];
        write_json_atomic(
            &service.catalog_path(),
            &PublicParseCatalog {
                schema_version: PUBLIC_CATALOG_SCHEMA_VERSION,
                total_entries: entries.len(),
                offset: 0,
                next_offset: None,
                facets: CatalogFacets::from_entries(&entries),
                entries,
            },
        )
        .unwrap();

        let page = service
            .catalog(&CatalogQuery {
                region: Some("global".into()),
                limit: Some(1),
                offset: Some(1),
                ..CatalogQuery::default()
            })
            .unwrap();
        assert_eq!(page.total_entries, 2);
        assert_eq!(page.offset, 1);
        assert_eq!(page.next_offset, None);
        assert_eq!(page.entries[0].run_index, 2);
        assert_eq!(page.facets.regions[0].count, 2);
    }
}
