//! Private artifact ingestion and public parse projections for rLogs.

use std::{
    collections::{BTreeMap, BTreeSet},
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
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use reqwest::Url;
use rlogs_combat::{RunAnalysis, RunSegmentKind, RunSubmissionDisposition};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BpsrStateDamageContributionProjector, bundled_run_reducer_config,
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

mod github_archive;

use github_archive::{ArchiveJob, GithubArchive};

pub const PUBLIC_PARSE_SCHEMA_VERSION: u16 = 1;
pub const PUBLIC_CATALOG_SCHEMA_VERSION: u16 = 1;
pub const UPLOAD_RESPONSE_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_CATALOG_ENTRIES: usize = 100_000;
const MAXIMUM_QUERY_LIMIT: usize = 250;
const MAXIMUM_UPLOAD_CHUNKS: usize = 16_384;
const UPLOAD_OWNER_SCHEMA_VERSION: u16 = 1;
const AUTH_INTROSPECTION_SCHEMA_VERSION: u16 = 1;

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
    writes: Mutex<()>,
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
            "archive-outbox",
            "archive-receipts",
        ] {
            std::fs::create_dir_all(root.join(relative))?;
        }
        let service = Self {
            inner: Arc::new(ServiceInner {
                root,
                public_site_url: public_site_url.trim_end_matches('/').into(),
                authentication,
                github_archive,
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
        let mut grouped = BTreeMap::<String, (PublicParseCatalogEntry, BTreeSet<String>)>::new();
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
                let submitter = report.submission_provenance.submitter_id.clone();
                match grouped.entry(entry.run_group_id.clone()) {
                    std::collections::btree_map::Entry::Vacant(vacant) => {
                        let mut submitters = BTreeSet::new();
                        if let Some(submitter) = submitter {
                            submitters.insert(submitter);
                        }
                        vacant.insert((entry, submitters));
                    }
                    std::collections::btree_map::Entry::Occupied(mut occupied) => {
                        let (representative, submitters) = occupied.get_mut();
                        representative.report_ids.push(report.report_id.clone());
                        representative.report_ids.sort();
                        representative.report_ids.dedup();
                        representative.contribution_count = representative.report_ids.len();
                        if let Some(submitter) = submitter {
                            submitters.insert(submitter);
                        }
                        representative.distinct_submitter_count = submitters.len();
                        if report.created_unix_millis > representative.created_unix_millis {
                            representative.report_id = report.report_id.clone();
                            representative.run_index = run.run_index;
                            representative.created_unix_millis = report.created_unix_millis;
                        }
                    }
                }
            }
            if grouped.len() > MAXIMUM_CATALOG_ENTRIES {
                return Err(ServiceError::CatalogTooLarge);
            }
        }
        let mut entries = grouped
            .into_values()
            .map(|(mut entry, submitters)| {
                entry.distinct_submitter_count = submitters.len();
                entry
            })
            .collect::<Vec<_>>();
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
        .route("/v1/uploads", post(begin_upload))
        .route(
            "/v1/uploads/{upload_id}/chunks/{sequence}",
            put(receive_chunk),
        )
        .route("/v1/uploads/{upload_id}/finalize", post(finalize_upload))
        .route("/v1/parses", get(list_parses))
        .route("/v1/parses/{report_id}", get(get_parse))
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(service)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok","service":"rlogs-submissions","schema_version":1}))
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

async fn authorize(
    service: &SubmissionService,
    headers: &HeaderMap,
) -> Result<UploadOwner, ApiError> {
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
    pub segments: Vec<PublicRunSegment>,
    pub participants: Vec<PublicParticipant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCorrelationMethod {
    ExactInstanceId,
    IsolatedArtifact,
}

impl Default for RunCorrelationMethod {
    fn default() -> Self {
        Self::IsolatedArtifact
    }
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
    let mut meter = CombatTimelinePlugin::with_damage_contribution_projection(
        confirmed_damage_contribution_rules().map_err(ServiceError::Replay)?,
        Some(Box::new(
            BpsrStateDamageContributionProjector::new().map_err(ServiceError::Replay)?,
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
    let replay = reader.replay(|event| {
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
    let runs = public_runs(&history, &run_projection.runs);
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

fn public_runs(history: &CombatHistorySnapshot, analyses: &[RunAnalysis]) -> Vec<PublicRun> {
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

fn run_group_id(history: &CombatHistorySnapshot, analysis: &RunAnalysis, run_index: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-run-group-v1\0");
    hasher.update(history.deployment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(history.region_id.as_bytes());
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
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServiceError::ClockBeforeEpoch)?
        .as_millis()
        .try_into()
        .map_err(|_| ServiceError::SizeOverflow)?)
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
    Service(ServiceError),
}

impl From<ServiceError> for ApiError {
    fn from(value: ServiceError) -> Self {
        Self::Service(value)
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
            Self::Service(ServiceError::NotFound) => {
                (StatusCode::NOT_FOUND, "resource not found".into())
            }
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
    fn run_groups_require_exact_instance_evidence() {
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
