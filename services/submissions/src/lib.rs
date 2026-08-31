//! Private artifact ingestion and public parse projections for rLogs.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post, put},
};
use reqwest::Url;
use rlogs_combat::{RunAnalysis, RunSegmentKind, RunSubmissionDisposition};
use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityRef, EventEnvelope, EventProvenance,
    EventSensitivity, EvidenceSource, GameProfileEvent, RunState, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BpsrLifeWaveTriggerLearner, BpsrRemoteFactorLearner,
    BpsrStateDamageContributionProjector, bundled_run_reducer_config,
    confirmed_damage_contribution_rules, localized_class_name, localized_scene_name,
    localized_specialization_name,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use rlogs_plugin_combat_meter::{CombatHistorySnapshot, CombatTimelinePlugin, HistoryActorSummary};
use rlogs_plugin_encounter_recorder::EncounterRecorderPlugin;
use rlogs_submission::{
    ArtifactBuildLimits, LocalLogArtifact, ReportVisibility, Sha256Digest, SubmissionSession,
    UploadManifest, VerificationTier, build_privacy_verified_submission_artifact,
    submission_privacy_policy_digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

mod accounts;
mod github_archive;
mod profiles;

use accounts::{
    AccountError, AccountStore, AccountView, AppTokenReceipt, DiscordConfiguration,
    WebSessionReceipt,
};
use github_archive::{ArchiveJob, GithubArchive};
use profiles::{
    ProfilePublishReceipt, ProfileRegistry, ProfileRegistryError, PublicProfile,
    PublicProfileCatalog,
};
use rlogs_profiles::LocalProfilePackage;

pub const PUBLIC_PARSE_SCHEMA_VERSION: u16 = 6;
pub const PUBLIC_CATALOG_SCHEMA_VERSION: u16 = 5;
pub const PUBLIC_RECONCILIATION_SCHEMA_VERSION: u16 = 5;
pub const UPLOAD_RESPONSE_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_CATALOG_ENTRIES: usize = 100_000;
const MAXIMUM_QUERY_LIMIT: usize = 250;
const MAXIMUM_UPLOAD_CHUNKS: usize = 16_384;
const UPLOAD_OWNER_SCHEMA_VERSION: u16 = 1;
const AUTH_INTROSPECTION_SCHEMA_VERSION: u16 = 1;
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
    github_archive: Option<GithubArchive>,
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

struct CrossVantageReplayResult {
    participants: Vec<PublicReconciledParticipant>,
    conservation: PublicAttributionConservation,
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
    participant_character_ids: Vec<String>,
}

impl ReconciliationRunSource {
    fn from_report(report: &PublicParseReport, run: &PublicRun) -> Self {
        Self {
            report_id: report.report_id.clone(),
            run_index: run.run_index,
            artifact_sha256: report.verification.artifact_sha256.clone(),
            protocol_pack_digest: report.protocol_pack_digest.clone(),
            created_unix_millis: report.created_unix_millis,
            quality: CanonicalSpineQuality::from_report(report, run),
            local_profile_witnesses: run.local_profile_witnesses.clone(),
            local_state_witnesses: run.local_state_witnesses.clone(),
            participant_character_ids: run
                .participants
                .iter()
                .filter_map(|participant| participant.character_id.clone())
                .collect(),
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
        Self::open_with_optional_github_archive(root, public_site_url, authentication, None)
    }

    /// Opens the receiver and enables the optional private GitHub research
    /// archive when `RLOGS_GITHUB_ARCHIVE_REPOSITORY` is configured. The
    /// repository token remains process-only and is never written to an
    /// outbox job, public projection, or API response.
    pub fn open_with_environment_github_archive(
        root: PathBuf,
        public_site_url: String,
        authentication: SubmissionAuthentication,
    ) -> Result<Self, ServiceError> {
        let github_archive =
            GithubArchive::from_environment().map_err(ServiceError::InvalidConfiguration)?;
        Self::open_with_optional_github_archive(
            root,
            public_site_url,
            authentication,
            github_archive,
        )
    }

    fn open_with_optional_github_archive(
        root: PathBuf,
        public_site_url: String,
        authentication: SubmissionAuthentication,
        github_archive: Option<GithubArchive>,
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
            "reconciliations",
            "archive-outbox",
            "archive-receipts",
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
                github_archive,
                accounts,
                profiles,
                writes: Mutex::new(()),
            }),
        };
        service.ensure_catalog()?;
        service.reconcile_github_archive_outbox()?;
        Ok(service)
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
        let report = build_public_report(
            &assembled,
            &manifest,
            &artifact,
            &report_id,
            created_unix_millis,
            owner.public_provenance(),
        )?;

        let artifact_path = self.artifact_path(&sealed_digest)?;
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !artifact_path.exists() {
            std::fs::rename(&assembled, &artifact_path)?;
        } else {
            std::fs::remove_file(&assembled)?;
        }
        write_json_atomic(&self.projection_path(&report_id)?, &report)?;
        self.enqueue_github_archive_locked(&report)?;
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
        Ok(report)
    }

    fn publish_profile(
        &self,
        package: LocalProfilePackage,
        owner: &UploadOwner,
    ) -> Result<ProfilePublishReceipt, ServiceError> {
        let _write = self.write_guard();
        Ok(self
            .inner
            .profiles
            .publish(package, owner.submitter_id.as_deref(), unix_millis()?)?)
    }

    fn profile(&self, profile_id: &str) -> Result<PublicProfile, ServiceError> {
        Ok(self.inner.profiles.get(profile_id)?)
    }

    fn profile_catalog(
        &self,
        query: &ProfileCatalogQuery,
    ) -> Result<PublicProfileCatalog, ServiceError> {
        Ok(self.inner.profiles.catalog(query.character_id.as_deref())?)
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
        replay_canonical_with_cross_vantage_state(
            &canonical_path,
            reconciliation.canonical_spine.run_index,
            &imported_events,
            |envelope, imported| {
                if imported && life_wave_trigger_learner.observe(envelope) {
                    return Ok(());
                }
                remote_factor_learner.observe(envelope);
                Ok(())
            },
        )?;
        let remote_factors = remote_factor_learner.finish();
        let life_wave_triggers = life_wave_trigger_learner.finish();

        let mut meter = CombatTimelinePlugin::with_damage_contribution_projection(
            confirmed_damage_contribution_rules().map_err(ServiceError::Replay)?,
            Some(Box::new(
                BpsrStateDamageContributionProjector::new_with_remote_factor_and_life_wave_timelines(
                    remote_factors,
                    life_wave_triggers,
                )
                .map_err(ServiceError::Replay)?,
            )),
        )
        .map_err(ServiceError::Replay)?;
        let mut encounter = EncounterRecorderPlugin::new(
            bundled_run_reducer_config()
                .map_err(|error| ServiceError::Replay(error.to_string()))?,
        );
        let header_file = File::open(&canonical_path)?;
        let header_reader = RlogReader::new(BufReader::new(header_file), RlogLimits::default())?;
        let header = header_reader.header().clone();
        meter.begin_live(&header);
        encounter.begin_live(&header);
        replay_canonical_with_cross_vantage_state(
            &canonical_path,
            reconciliation.canonical_spine.run_index,
            &imported_events,
            |envelope, imported| {
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
        let history = meter
            .history_snapshot(&run_projection.runs)
            .map_err(|error| ServiceError::Replay(error.to_string()))?;
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
        Ok(CrossVantageReplayResult {
            participants,
            conservation,
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
        format!("{}/?parse={report_id}#parse", self.inner.public_site_url)
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
        if !self.catalog_path().exists() {
            self.rebuild_catalog_locked()?;
        }
        Ok(())
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
            for run in &report.runs {
                let entry = PublicParseCatalogEntry::from_report(&report, run);
                let quality = CanonicalSpineQuality::from_report(&report, run);
                let reconciliation_source = ReconciliationRunSource::from_report(&report, run);
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
        )
    }

    /// Uploads all currently pending evidence jobs. A failure leaves the job
    /// intact for a later retry and never rolls back an already accepted log.
    pub fn drain_github_archive_once(&self) -> Result<usize, ServiceError> {
        let Some(archive) = self.inner.github_archive.as_ref() else {
            return Ok(0);
        };
        let mut paths = std::fs::read_dir(self.github_archive_outbox_path())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut archived = 0_usize;
        for path in paths {
            let job: ArchiveJob = read_json(&path)?;
            validate_identifier(&job.report_id, "report ID")?;
            let digest = Sha256Digest::parse(job.artifact_sha256.clone())?;
            let receipt_path = self.github_archive_receipt_path(&job.report_id)?;
            if receipt_path.exists() {
                std::fs::remove_file(&path)?;
                continue;
            }
            let receipt = archive
                .archive(
                    &job,
                    &self.artifact_path(&digest)?,
                    &self.projection_path(&job.report_id)?,
                    unix_millis()?,
                )
                .map_err(ServiceError::GithubArchive)?;
            write_json_atomic(&receipt_path, &receipt)?;
            std::fs::remove_file(&path)?;
            archived += 1;
        }
        Ok(archived)
    }

    pub fn github_archive_repository(&self) -> Option<&str> {
        self.inner
            .github_archive
            .as_ref()
            .map(GithubArchive::repository)
    }

    fn reconcile_github_archive_outbox(&self) -> Result<(), ServiceError> {
        if self.inner.github_archive.is_none() {
            return Ok(());
        }
        let _write = self.write_guard();
        for file in std::fs::read_dir(self.inner.root.join("projections"))? {
            let path = file?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let report: PublicParseReport = read_json(&path)?;
            self.enqueue_github_archive_locked(&report)?;
        }
        Ok(())
    }

    fn enqueue_github_archive_locked(
        &self,
        report: &PublicParseReport,
    ) -> Result<(), ServiceError> {
        if self.inner.github_archive.is_none() {
            return Ok(());
        }
        let receipt = self.github_archive_receipt_path(&report.report_id)?;
        let outbox = self.github_archive_outbox_job_path(&report.report_id)?;
        if receipt.exists() || outbox.exists() {
            return Ok(());
        }
        write_json_atomic(
            &outbox,
            &ArchiveJob {
                schema_version: 1,
                report_id: report.report_id.clone(),
                artifact_sha256: report.verification.artifact_sha256.clone(),
                created_unix_millis: report.created_unix_millis,
            },
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

    fn github_archive_outbox_path(&self) -> PathBuf {
        self.inner.root.join("archive-outbox")
    }

    fn github_archive_outbox_job_path(&self, report_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        Ok(self
            .github_archive_outbox_path()
            .join(format!("{report_id}.json")))
    }

    fn github_archive_receipt_path(&self, report_id: &str) -> Result<PathBuf, ServiceError> {
        validate_identifier(report_id, "report ID")?;
        Ok(self
            .inner
            .root
            .join("archive-receipts")
            .join(format!("{report_id}.json")))
    }
}

pub fn router(service: SubmissionService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/auth/config", get(auth_config))
        .route("/v1/auth/discord/start", get(begin_discord_auth))
        .route("/v1/auth/discord/callback", get(complete_discord_auth))
        .route("/v1/auth/session/exchange", post(exchange_auth_code))
        .route("/v1/auth/me", get(get_account))
        .route("/v1/auth/app-tokens", post(issue_app_token))
        .route("/v1/uploads", post(begin_upload))
        .route(
            "/v1/uploads/{upload_id}/chunks/{sequence}",
            put(receive_chunk),
        )
        .route("/v1/uploads/{upload_id}/finalize", post(finalize_upload))
        .route("/v1/parses", get(list_parses))
        .route("/v1/parses/{report_id}", get(get_parse))
        .route(
            "/v1/games/blue-protocol-star-resonance/profiles",
            post(publish_bpsr_profile),
        )
        .route("/v1/profiles", get(list_profiles))
        .route("/v1/profiles/{profile_id}", get(get_profile))
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
    let owner = authorize(&service, &headers).await?;
    if owner.submitter_id.is_none() {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(service.publish_profile(package, &owner)?))
}

async fn list_profiles(
    State(service): State<SubmissionService>,
    Query(query): Query<ProfileCatalogQuery>,
) -> Result<Json<PublicProfileCatalog>, ApiError> {
    Ok(Json(service.profile_catalog(&query)?))
}

async fn get_profile(
    State(service): State<SubmissionService>,
    AxumPath(profile_id): AxumPath<String>,
) -> Result<Json<PublicProfile>, ApiError> {
    Ok(Json(service.profile(&profile_id)?))
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
struct DiscordCallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginCodeExchangeRequest {
    code: String,
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
    pub segments: Vec<PublicRunSegment>,
    pub participants: Vec<PublicParticipant>,
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
    LifeWaveTriggerStatus,
    LifeWaveTriggerHealing,
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

#[derive(Debug, Default, Deserialize)]
pub struct ProfileCatalogQuery {
    pub character_id: Option<String>,
}

impl CatalogQuery {
    fn matches(&self, entry: &PublicParseCatalogEntry) -> bool {
        optional_matches(&self.deployment, &entry.deployment_id)
            && optional_matches(&self.region, &entry.region_id)
            && optional_matches_option(&self.activity, &entry.activity_id)
            && self.scene.is_none_or(|value| entry.scene_id == Some(value))
            && optional_matches_option(&self.difficulty, &entry.difficulty_family)
            && optional_matches(&self.terminal, &entry.terminal_state)
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
    pub deployment_id: String,
    pub region_id: String,
    pub activity_id: Option<String>,
    pub activity_family_id: Option<String>,
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
            deployment_id: report.deployment_id.clone(),
            region_id: report.region_id.clone(),
            activity_id: run.activity_id.clone(),
            activity_family_id: run.activity_family_id.clone(),
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
            if let Some(value) = entry.activity_id.as_ref() {
                increment(&mut activities, value.clone());
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
    if manifest.metadata.game_plugin_id != BPSR_GAME_PLUGIN_ID {
        return Err(ServiceError::UnsupportedGamePlugin(
            manifest.metadata.game_plugin_id.clone(),
        ));
    }
    let first_pass_file = File::open(path)?;
    let first_pass_reader =
        RlogReader::new(BufReader::new(first_pass_file), RlogLimits::default())?;
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
    .map_err(ServiceError::Replay)?;
    let mut encounter = EncounterRecorderPlugin::new(
        bundled_run_reducer_config().map_err(|error| ServiceError::Replay(error.to_string()))?,
    );
    let file = File::open(path)?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
    let header = reader.header().clone();
    meter.begin_live(&header);
    encounter.begin_live(&header);
    let mut local_profile_observations = Vec::new();
    let mut character_id_by_entity_uuid = BTreeMap::<i64, Option<String>>::new();
    let mut raw_local_state_observations = Vec::new();
    let replay = reader.replay(|event| {
        if event.sensitivity == EventSensitivity::PersonalGameplay
            && let CanonicalEvent::CharacterProfileObserved { profile } = &event.event
        {
            local_profile_observations.push(LocalProfileObservation {
                character_id: profile.character.character_id.clone(),
                event_sequence: event.sequence,
                observed_micros: event.time.observed_micros,
                game_time_millis: event.time.game_time_millis,
                payload_sha256: local_profile_payload_digest(profile)
                    .map_err(|error| error.to_string())?,
            });
        }
        if let CanonicalEvent::Timeline(timeline) = &event.event {
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
    let history = meter
        .history_snapshot(&run_projection.runs)
        .map_err(|error| ServiceError::Replay(error.to_string()))?;
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
            })
        })
        .collect()
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

fn run_scoped_profile_witnesses(
    analysis: &RunAnalysis,
    participant_character_ids: &BTreeSet<String>,
    observations: &[LocalProfileObservation],
) -> Vec<PublicLocalProfileWitness> {
    let started_micros = analysis.timing.started_micros;
    let ended_micros = analysis
        .timing
        .ended_micros
        .unwrap_or(analysis.timing.observed_until_micros);
    let mut latest_before = BTreeMap::<&str, &LocalProfileObservation>::new();
    for observation in observations.iter().filter(|observation| {
        participant_character_ids.contains(observation.character_id.as_str())
            && observation.observed_micros <= started_micros
    }) {
        latest_before
            .entry(observation.character_id.as_str())
            .and_modify(|existing| {
                if (observation.observed_micros, observation.event_sequence)
                    > (existing.observed_micros, existing.event_sequence)
                {
                    *existing = observation;
                }
            })
            .or_insert(observation);
    }

    let mut selected = latest_before
        .into_values()
        .map(|observation| (observation, LocalStateWitnessPlacement::PreRunBaseline))
        .chain(
            observations
                .iter()
                .filter(|observation| {
                    participant_character_ids.contains(observation.character_id.as_str())
                        && observation.observed_micros > started_micros
                        && observation.observed_micros <= ended_micros
                })
                .map(|observation| (observation, LocalStateWitnessPlacement::InRun)),
        )
        .map(|(observation, placement)| PublicLocalProfileWitness {
            character_id: observation.character_id.clone(),
            placement,
            event_sequence: observation.event_sequence,
            observed_micros: observation.observed_micros,
            game_time_millis: observation.game_time_millis,
            payload_sha256: observation.payload_sha256.clone(),
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|witness| (witness.observed_micros, witness.event_sequence));
    selected.dedup_by_key(|witness| witness.event_sequence);
    selected
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
    hasher.update(b"rlogs-cross-vantage-verified-state-profile-and-trigger-v3\0");
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
            TimelineEventKind::Status(status) => status.target = canonical,
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
        TimelineEventKind::Status(status) => (
            status.target.actor_id.0,
            status.target.entity_uuid.0,
            EntityAttributeUpdateKind::Unknown,
            LocalStateWitnessKind::LifeWaveTriggerStatus,
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
        (
            _,
            LocalStateWitnessKind::LifeWaveTriggerStatus
            | LocalStateWitnessKind::LifeWaveTriggerHealing,
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
    let mut character_ids = sources
        .iter()
        .flat_map(|source| source.participant_character_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    character_ids.extend(
        sources
            .iter()
            .flat_map(|source| source.local_profile_witnesses.iter())
            .map(|witness| witness.character_id.clone()),
    );
    let participant_character_count = character_ids.len();

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
                LocalStateWitnessKind::LifeWaveTriggerStatus => {
                    b"life-wave-trigger-status".as_slice()
                }
                LocalStateWitnessKind::LifeWaveTriggerHealing => {
                    b"life-wave-trigger-healing".as_slice()
                }
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

fn public_participant(actor: &HistoryActorSummary) -> PublicParticipant {
    PublicParticipant {
        actor_id: actor.actor_id.clone(),
        character_id: actor.character_id.clone(),
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
    }
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
    #[error("private GitHub research archive failed: {0}")]
    GithubArchive(String),
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
            Self::Service(error) => (StatusCode::BAD_REQUEST, error.to_string()),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: Some("chaotic".into()),
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
        assert_eq!(facets.scenes[0].id, 6565);
        assert_eq!(facets.difficulties[0].id, "master");
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
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: None,
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
        let analysis = fixture_analysis("capture-a", Some("instance-42"));
        let participants = ["character-a".to_owned()].into_iter().collect();
        let observations = vec![
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 1,
                observed_micros: 0,
                game_time_millis: None,
                payload_sha256: "sha256:old".into(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 2,
                observed_micros: 1,
                game_time_millis: None,
                payload_sha256: "sha256:start".into(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 3,
                observed_micros: 2,
                game_time_millis: Some(2),
                payload_sha256: "sha256:during".into(),
            },
            LocalProfileObservation {
                character_id: "character-a".into(),
                event_sequence: 4,
                observed_micros: 3,
                game_time_millis: Some(3),
                payload_sha256: "sha256:after".into(),
            },
            LocalProfileObservation {
                character_id: "not-a-participant".into(),
                event_sequence: 5,
                observed_micros: 2,
                game_time_millis: Some(2),
                payload_sha256: "sha256:unrelated".into(),
            },
        ];

        let selected = run_scoped_profile_witnesses(&analysis, &participants, &observations);
        assert_eq!(
            selected
                .iter()
                .map(|witness| witness.event_sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
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
            )],
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

    fn fixture_public_report(
        report_id: &str,
        local_character_id: &str,
        data_gap_count: u64,
    ) -> PublicParseReport {
        let participant = |character_id: &str| PublicParticipant {
            actor_id: character_id.into(),
            character_id: Some(character_id.into()),
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
        };
        PublicParseReport {
            schema_version: PUBLIC_PARSE_SCHEMA_VERSION,
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
                segments: Vec::new(),
                participants: vec![participant("character-a"), participant("character-b")],
            }],
        }
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
            deployment_id: "global".into(),
            region_id: region.into(),
            activity_id: Some("chaotic".into()),
            activity_family_id: Some("chaotic".into()),
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
