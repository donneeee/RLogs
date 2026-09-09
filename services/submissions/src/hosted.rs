//! Internal, stateless replay endpoint used by the Cloudflare verifier Worker.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rlogs_submission::{
    DEFAULT_MAXIMUM_LOG_BYTES, ReportVisibility, Sha256Digest, SubmissionPurpose, UploadManifest,
    VerificationTier,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    HostedRunArtifact, HostedVerificationResult, PUBLIC_PARSE_PROJECTION_REVISION,
    PUBLIC_PARSE_SCHEMA_VERSION, PublicParseReport, PublicRunReconciliation,
    PublicSubmissionProvenance, ServiceError, reconcile_hosted_run_group,
    verify_hosted_submission_path, verify_hosted_training_dummy_path,
};

const MAXIMUM_JOB_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_CHUNK_BYTES: u64 = 16 * 1024 * 1024;
const MAXIMUM_RECONCILIATION_SOURCES: usize = 64;
const MAXIMUM_RECONCILIATION_OBJECT_KEY_BYTES: usize = 512;
const MAXIMUM_RECONCILIATION_CHUNKS: usize = 16_384;
const MAXIMUM_PROJECTION_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_RECONCILIATION_PROJECTION_BYTES: u64 = 256 * 1024 * 1024;
const MAXIMUM_RECONCILIATION_SOURCE_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAXIMUM_RECONCILIATION_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct HostedVerifierState {
    client: reqwest::Client,
    artifact_origin: String,
    staging_root: PathBuf,
}

impl HostedVerifierState {
    pub fn from_environment() -> Result<Self, ServiceError> {
        let artifact_origin = std::env::var("RLOGS_HOSTED_ARTIFACT_ORIGIN")
            .unwrap_or_else(|_| "http://rlogs-artifacts.r2".into());
        let parsed = reqwest::Url::parse(&artifact_origin).map_err(|error| {
            ServiceError::InvalidConfiguration(format!(
                "hosted artifact origin is invalid: {error}"
            ))
        })?;
        if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ServiceError::InvalidConfiguration(
                "hosted artifact origin must contain only a scheme and authority".into(),
            ));
        }
        let staging_root = std::env::var_os("RLOGS_HOSTED_STAGING")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/rlogs-hosted-verifier"));
        std::fs::create_dir_all(&staging_root)?;
        let staging_root = std::fs::canonicalize(staging_root)?;
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(60))
                .user_agent(concat!("rLogs-hosted-verifier/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|error| ServiceError::InvalidConfiguration(error.to_string()))?,
            artifact_origin: artifact_origin.trim_end_matches('/').to_owned(),
            staging_root,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedVerificationJob {
    pub schema_version: u16,
    pub upload_id: String,
    pub artifact_sha256: Sha256Digest,
    pub expected_report_id: String,
    pub created_unix_millis: u64,
    pub manifest: UploadManifest,
    pub chunks: Vec<HostedChunk>,
    pub submission_provenance: PublicSubmissionProvenance,
    #[serde(default)]
    pub verified_names_by_character: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedChunk {
    pub sequence: u64,
    pub object_key: String,
    pub byte_length: u64,
    pub sha256: Sha256Digest,
}

/// Internal manifest for reconciling immutable R2 products. The Worker
/// supplies content-addressed public projections and the original sealed-log
/// chunk commitments; it never supplies attribution or formula results.
/// A projection embedded as `Public` is necessary but not sufficient: this
/// stateless endpoint cannot observe a later visibility change, so the Worker
/// must recheck current D1 visibility immediately before persistence.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedReconciliationJob {
    pub schema_version: u16,
    pub run_group_id: String,
    pub sources: Vec<HostedReconciliationSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedReconciliationSource {
    pub report_id: String,
    pub upload_id: String,
    pub run_index: u32,
    pub artifact_sha256: Sha256Digest,
    pub projection: HostedObject,
    pub chunks: Vec<HostedChunk>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedObject {
    pub object_key: String,
    pub byte_length: u64,
    pub sha256: Sha256Digest,
}

#[derive(Debug, Serialize)]
struct HostedErrorBody {
    error: String,
}

pub fn router(state: HostedVerifierState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/internal/v1/verify", post(verify))
        .route("/internal/v1/reconcile", post(reconcile))
        .layer(DefaultBodyLimit::max(MAXIMUM_JOB_BYTES))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "rlogs-hosted-verifier",
        "schema_version": 1,
    }))
}

async fn verify(
    State(state): State<HostedVerifierState>,
    Json(job): Json<HostedVerificationJob>,
) -> Result<Json<HostedVerificationResult>, HostedVerifierError> {
    validate_job(&job)?;
    let path = state.staging_root.join(format!(
        "{}-{}.rlog",
        job.upload_id,
        Uuid::new_v4().simple()
    ));
    let result = download_and_verify(&state, &job, &path).await;
    let _ = std::fs::remove_file(&path);
    result.map(Json)
}

async fn reconcile(
    State(state): State<HostedVerifierState>,
    Json(job): Json<HostedReconciliationJob>,
) -> Result<Json<PublicRunReconciliation>, HostedVerifierError> {
    validate_reconciliation_job(&job)?;
    let run_group_id = job.run_group_id.clone();
    let materialized = download_reconciliation_sources(&state, &job).await?;
    tokio::task::spawn_blocking(move || materialized.reconcile(&run_group_id))
        .await
        .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?
        .map(Json)
}

struct MaterializedReconciliation {
    sources: Vec<(PublicParseReport, u32, PathBuf)>,
    _temporary_files: TemporaryFiles,
}

impl MaterializedReconciliation {
    fn reconcile(
        &self,
        run_group_id: &str,
    ) -> Result<PublicRunReconciliation, HostedVerifierError> {
        let artifacts = self
            .sources
            .iter()
            .map(|(report, run_index, artifact_path)| HostedRunArtifact {
                report,
                run_index: *run_index,
                artifact_path,
            })
            .collect::<Vec<_>>();
        reconcile_hosted_run_group(run_group_id, &artifacts).map_err(HostedVerifierError::from)
    }
}

#[derive(Default)]
struct TemporaryFiles(Vec<PathBuf>);

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn download_reconciliation_sources(
    state: &HostedVerifierState,
    job: &HostedReconciliationJob,
) -> Result<MaterializedReconciliation, HostedVerifierError> {
    let mut sources = Vec::with_capacity(job.sources.len());
    let mut temporary_files = TemporaryFiles::default();
    let mut total_projection_bytes = 0_u64;
    for (source_index, source) in job.sources.iter().enumerate() {
        total_projection_bytes = total_projection_bytes
            .checked_add(source.projection.byte_length)
            .ok_or_else(|| {
                HostedVerifierError::Rejected("projection byte count overflowed".into())
            })?;
        if total_projection_bytes > MAXIMUM_RECONCILIATION_PROJECTION_BYTES {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation projections exceed {MAXIMUM_RECONCILIATION_PROJECTION_BYTES} aggregate bytes"
            )));
        }
        let projection_bytes = download_committed_object(
            state,
            &source.projection.object_key,
            source.projection.byte_length,
            &source.projection.sha256,
            MAXIMUM_PROJECTION_BYTES,
            "projection",
        )
        .await?;
        let projection: PublicParseReport =
            serde_json::from_slice(&projection_bytes).map_err(|error| {
                HostedVerifierError::Rejected(format!("hosted projection is invalid: {error}"))
            })?;
        if projection.report_id != source.report_id {
            return Err(HostedVerifierError::Rejected(format!(
                "projection report {} does not match manifest report {}",
                projection.report_id, source.report_id
            )));
        }
        if projection.visibility != ReportVisibility::Public
            || projection.schema_version != PUBLIC_PARSE_SCHEMA_VERSION
            || projection.projection_revision != PUBLIC_PARSE_PROJECTION_REVISION
            || projection.verification.tier != VerificationTier::Replayed
        {
            return Err(HostedVerifierError::Rejected(format!(
                "projection report {} is not a current public replay",
                source.report_id
            )));
        }

        let artifact_path = state.staging_root.join(format!(
            "reconcile-{}-{source_index}.rlog",
            Uuid::new_v4().simple()
        ));
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&artifact_path)?;
        temporary_files.0.push(artifact_path.clone());
        let mut artifact_hasher = Sha256::new();
        for chunk in &source.chunks {
            let bytes = download_committed_object(
                state,
                &chunk.object_key,
                chunk.byte_length,
                &chunk.sha256,
                MAXIMUM_CHUNK_BYTES,
                "artifact chunk",
            )
            .await?;
            output.write_all(&bytes)?;
            artifact_hasher.update(&bytes);
        }
        output.flush()?;
        output.sync_all()?;
        drop(output);
        if format!("{:x}", artifact_hasher.finalize()) != source.artifact_sha256.as_str() {
            return Err(HostedVerifierError::Rejected(format!(
                "assembled artifact for report {} does not match its commitment",
                source.report_id
            )));
        }
        sources.push((projection, source.run_index, artifact_path));
    }
    Ok(MaterializedReconciliation {
        sources,
        _temporary_files: temporary_files,
    })
}

async fn download_committed_object(
    state: &HostedVerifierState,
    object_key: &str,
    expected_bytes: u64,
    expected_digest: &Sha256Digest,
    hard_maximum_bytes: u64,
    label: &'static str,
) -> Result<Vec<u8>, HostedVerifierError> {
    let encoded = percent_encode_object_key(object_key);
    let mut response = state
        .client
        .get(format!("{}{encoded}", state.artifact_origin))
        .send()
        .await
        .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?;
    if !response.status().is_success() {
        return Err(HostedVerifierError::Retryable(format!(
            "hosted {label} returned HTTP {}",
            response.status()
        )));
    }
    if expected_bytes == 0 || expected_bytes > hard_maximum_bytes {
        return Err(HostedVerifierError::Rejected(format!(
            "hosted {label} declared length is outside its allowed bound"
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length != expected_bytes)
    {
        return Err(HostedVerifierError::Rejected(format!(
            "hosted {label} length does not match its commitment"
        )));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(expected_bytes).unwrap_or(0));
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?
    {
        let next_length = u64::try_from(bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if next_length > expected_bytes || next_length > hard_maximum_bytes {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted {label} exceeded its committed length"
            )));
        }
        hasher.update(&chunk);
        bytes.extend_from_slice(&chunk);
    }
    if u64::try_from(bytes.len()).ok() != Some(expected_bytes) {
        return Err(HostedVerifierError::Rejected(format!(
            "hosted {label} length does not match its commitment"
        )));
    }
    let actual_digest = format!("{:x}", hasher.finalize());
    if actual_digest != expected_digest.as_str() {
        return Err(HostedVerifierError::Rejected(format!(
            "hosted {label} digest does not match its commitment"
        )));
    }
    Ok(bytes)
}

fn validate_reconciliation_job(job: &HostedReconciliationJob) -> Result<(), HostedVerifierError> {
    if job.schema_version != 1
        || !is_identifier(&job.run_group_id)
        || job.run_group_id.len() > 96
        || job.sources.is_empty()
        || job.sources.len() > MAXIMUM_RECONCILIATION_SOURCES
    {
        return Err(HostedVerifierError::Rejected(
            "hosted reconciliation job metadata is invalid".into(),
        ));
    }
    let mut total_chunks = 0_usize;
    let mut total_artifact_bytes = 0_u64;
    let mut total_projection_bytes = 0_u64;
    let mut report_ids = BTreeSet::new();
    let mut artifact_digests = BTreeSet::new();
    let mut object_keys = BTreeSet::new();
    for (source_index, source) in job.sources.iter().enumerate() {
        if !is_identifier(&source.report_id)
            || !is_identifier(&source.upload_id)
            || source.report_id != expected_report_id(&source.artifact_sha256)
            || source.upload_id != expected_upload_id(&source.artifact_sha256)
            || !report_ids.insert(&source.report_id)
            || !artifact_digests.insert(&source.artifact_sha256)
            || source.projection.byte_length == 0
            || source.projection.byte_length > MAXIMUM_PROJECTION_BYTES
            || !valid_projection_object_key(
                &source.report_id,
                &source.projection.sha256,
                &source.projection.object_key,
            )
            || !object_keys.insert(&source.projection.object_key)
            || source.chunks.is_empty()
        {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation source {source_index} metadata is invalid"
            )));
        }
        total_chunks = total_chunks
            .checked_add(source.chunks.len())
            .ok_or_else(|| {
                HostedVerifierError::Rejected("hosted reconciliation chunk count overflowed".into())
            })?;
        if total_chunks > MAXIMUM_RECONCILIATION_CHUNKS {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation exceeds {MAXIMUM_RECONCILIATION_CHUNKS} chunks"
            )));
        }
        total_projection_bytes = total_projection_bytes
            .checked_add(source.projection.byte_length)
            .ok_or_else(|| {
                HostedVerifierError::Rejected("projection byte count overflowed".into())
            })?;
        if total_projection_bytes > MAXIMUM_RECONCILIATION_PROJECTION_BYTES {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation projections exceed {MAXIMUM_RECONCILIATION_PROJECTION_BYTES} aggregate bytes"
            )));
        }
        let mut source_bytes = 0_u64;
        for (chunk_index, chunk) in source.chunks.iter().enumerate() {
            if chunk.sequence != u64::try_from(chunk_index).unwrap_or(u64::MAX)
                || chunk.byte_length == 0
                || chunk.byte_length > MAXIMUM_CHUNK_BYTES
                || !valid_reconciliation_chunk_key(&source.upload_id, chunk)
                || !object_keys.insert(&chunk.object_key)
            {
                return Err(HostedVerifierError::Rejected(format!(
                    "hosted reconciliation source {source_index} chunk {chunk_index} metadata is invalid"
                )));
            }
            source_bytes = source_bytes.checked_add(chunk.byte_length).ok_or_else(|| {
                HostedVerifierError::Rejected("artifact byte count overflowed".into())
            })?;
        }
        if source_bytes == 0
            || source_bytes > DEFAULT_MAXIMUM_LOG_BYTES
            || source_bytes > MAXIMUM_RECONCILIATION_SOURCE_ARTIFACT_BYTES
        {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation source {source_index} artifact size exceeds the {MAXIMUM_RECONCILIATION_SOURCE_ARTIFACT_BYTES}-byte container budget"
            )));
        }
        total_artifact_bytes = total_artifact_bytes
            .checked_add(source_bytes)
            .ok_or_else(|| {
                HostedVerifierError::Rejected("artifact byte count overflowed".into())
            })?;
        if total_artifact_bytes > MAXIMUM_RECONCILIATION_ARTIFACT_BYTES {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted reconciliation artifacts exceed {MAXIMUM_RECONCILIATION_ARTIFACT_BYTES} aggregate bytes"
            )));
        }
    }
    Ok(())
}

fn valid_projection_object_key(report_id: &str, digest: &Sha256Digest, value: &str) -> bool {
    value.len() <= MAXIMUM_RECONCILIATION_OBJECT_KEY_BYTES
        && value == format!("reports/{report_id}/projection-{digest}.json")
}

fn valid_reconciliation_chunk_key(upload_id: &str, chunk: &HostedChunk) -> bool {
    chunk.object_key.len() <= MAXIMUM_RECONCILIATION_OBJECT_KEY_BYTES
        && chunk.object_key
            == format!(
                "uploads/{upload_id}/chunks/{:08}-{}.bin",
                chunk.sequence, chunk.sha256
            )
}

fn expected_report_id(digest: &Sha256Digest) -> String {
    format!("rpt_{}", &digest.as_str()[..32])
}

fn expected_upload_id(digest: &Sha256Digest) -> String {
    format!("up_{}", &digest.as_str()[..32])
}

async fn download_and_verify(
    state: &HostedVerifierState,
    job: &HostedVerificationJob,
    path: &PathBuf,
) -> Result<HostedVerificationResult, HostedVerifierError> {
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    let mut artifact_hasher = Sha256::new();
    for chunk in &job.chunks {
        let bytes = download_committed_object(
            state,
            &chunk.object_key,
            chunk.byte_length,
            &chunk.sha256,
            MAXIMUM_CHUNK_BYTES,
            "artifact chunk",
        )
        .await?;
        output.write_all(&bytes)?;
        artifact_hasher.update(&bytes);
    }
    output.flush()?;
    output.sync_all()?;
    drop(output);
    if format!("{:x}", artifact_hasher.finalize()) != job.artifact_sha256.as_str() {
        return Err(HostedVerifierError::Rejected(
            "assembled artifact digest does not match its commitment".into(),
        ));
    }

    let path = path.clone();
    let manifest = job.manifest.clone();
    let report_id = job.expected_report_id.clone();
    let created = job.created_unix_millis;
    let provenance = job.submission_provenance.clone();
    let names = job.verified_names_by_character.clone();
    tokio::task::spawn_blocking(move || match manifest.metadata.purpose {
        SubmissionPurpose::CombatRun => {
            verify_hosted_submission_path(&path, &manifest, &report_id, created, provenance, &names)
                .map(HostedVerificationResult::Combat)
        }
        SubmissionPurpose::TrainingDummy => verify_hosted_training_dummy_path(
            &path, &manifest, &report_id, created, provenance, &names,
        )
        .map(HostedVerificationResult::TrainingDummy),
    })
    .await
    .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?
    .map_err(|error| HostedVerifierError::Rejected(error.to_string()))
}

fn validate_job(job: &HostedVerificationJob) -> Result<(), HostedVerifierError> {
    if job.schema_version != 1
        || !is_identifier(&job.upload_id)
        || !is_identifier(&job.expected_report_id)
        || job.created_unix_millis == 0
        || job.manifest.sealed_log_digest.as_ref() != Some(&job.artifact_sha256)
        || job.chunks.len() != job.manifest.chunks.len()
    {
        return Err(HostedVerifierError::Rejected(
            "hosted verification job metadata is invalid".into(),
        ));
    }
    for (index, (chunk, descriptor)) in job.chunks.iter().zip(&job.manifest.chunks).enumerate() {
        if chunk.sequence != u64::try_from(index).unwrap_or(u64::MAX)
            || chunk.sequence != descriptor.sequence
            || chunk.byte_length == 0
            || chunk.byte_length > MAXIMUM_CHUNK_BYTES
            || chunk.byte_length != descriptor.byte_length
            || chunk.sha256 != descriptor.sha256
            || !valid_object_key(&job.upload_id, &chunk.object_key)
        {
            return Err(HostedVerifierError::Rejected(format!(
                "hosted artifact chunk {index} metadata is invalid"
            )));
        }
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_object_key(upload_id: &str, value: &str) -> bool {
    let prefix = format!("uploads/{upload_id}/chunks/");
    let Some(name) = value.strip_prefix(&prefix) else {
        return false;
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && value.len() <= 512
}

fn percent_encode_object_key(value: &str) -> String {
    let mut encoded = String::from("/");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[derive(Debug)]
enum HostedVerifierError {
    Rejected(String),
    Retryable(String),
}

impl From<ServiceError> for HostedVerifierError {
    fn from(value: ServiceError) -> Self {
        match value {
            error @ (ServiceError::Io(_)
            | ServiceError::InvalidConfiguration(_)
            | ServiceError::ClockBeforeEpoch) => Self::Retryable(error.to_string()),
            error => Self::Rejected(error.to_string()),
        }
    }
}

impl From<std::io::Error> for HostedVerifierError {
    fn from(value: std::io::Error) -> Self {
        Self::Retryable(value.to_string())
    }
}

impl IntoResponse for HostedVerifierError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Rejected(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            Self::Retryable(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
        };
        (status, Json(HostedErrorBody { error: message })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::path::Path;
    use tower::ServiceExt;

    fn digest(value: &[u8]) -> Sha256Digest {
        Sha256Digest::parse(format!("{:x}", Sha256::digest(value))).unwrap()
    }

    fn test_state(staging_root: &Path) -> HostedVerifierState {
        HostedVerifierState {
            client: reqwest::Client::new(),
            artifact_origin: "http://rlogs-artifacts.r2".into(),
            staging_root: std::fs::canonicalize(staging_root).unwrap(),
        }
    }

    fn reconciliation_source(seed: &[u8], projection_bytes: u64) -> HostedReconciliationSource {
        let artifact_digest = digest(seed);
        let report_id = expected_report_id(&artifact_digest);
        let upload_id = expected_upload_id(&artifact_digest);
        let mut projection_seed = seed.to_vec();
        projection_seed.extend_from_slice(b"-projection");
        let projection_digest = digest(&projection_seed);
        HostedReconciliationSource {
            report_id: report_id.clone(),
            upload_id: upload_id.clone(),
            run_index: 0,
            artifact_sha256: artifact_digest.clone(),
            projection: HostedObject {
                object_key: format!("reports/{report_id}/projection-{projection_digest}.json"),
                byte_length: projection_bytes,
                sha256: projection_digest,
            },
            chunks: vec![HostedChunk {
                sequence: 0,
                object_key: format!("uploads/{upload_id}/chunks/00000000-{artifact_digest}.bin"),
                byte_length: u64::try_from(seed.len()).unwrap(),
                sha256: artifact_digest,
            }],
        }
    }

    fn reconciliation_source_with_artifact_bytes(
        seed: &[u8],
        artifact_bytes: u64,
    ) -> HostedReconciliationSource {
        let mut source = reconciliation_source(seed, 2);
        source.chunks.clear();
        let mut remaining = artifact_bytes;
        let mut sequence = 0_u64;
        while remaining > 0 {
            let byte_length = remaining.min(MAXIMUM_CHUNK_BYTES);
            let chunk_digest =
                digest(format!("{}-{sequence}", String::from_utf8_lossy(seed)).as_bytes());
            source.chunks.push(HostedChunk {
                sequence,
                object_key: format!(
                    "uploads/{}/chunks/{sequence:08}-{chunk_digest}.bin",
                    source.upload_id
                ),
                byte_length,
                sha256: chunk_digest,
            });
            remaining -= byte_length;
            sequence += 1;
        }
        source
    }

    #[test]
    fn object_keys_are_scoped_to_the_job() {
        assert!(valid_object_key(
            "up_abc",
            "uploads/up_abc/chunks/00000000-deadbeef.bin"
        ));
        assert!(!valid_object_key(
            "up_abc",
            "uploads/up_other/chunks/00000000-deadbeef.bin"
        ));
        assert!(!valid_object_key(
            "up_abc",
            "uploads/up_abc/chunks/../secret"
        ));
        assert!(!valid_object_key(
            "up_abc",
            "uploads/up_abc/chunks/chunk.bin?version=other"
        ));
        assert!(!valid_object_key(
            "up_abc",
            "uploads/up_abc/chunks/nested/chunk.bin"
        ));
    }

    #[test]
    fn object_key_encoding_preserves_paths_without_query_injection() {
        assert_eq!(
            percent_encode_object_key("uploads/up_a/chunks/a b?#.bin"),
            "/uploads/up_a/chunks/a%20b%3F%23.bin"
        );
    }

    #[test]
    fn reconciliation_projection_keys_are_content_addressed_and_scoped() {
        let projection_digest = digest(b"{}");
        let report_id = "rpt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(valid_projection_object_key(
            report_id,
            &projection_digest,
            &format!("reports/{report_id}/projection-{projection_digest}.json")
        ));
        assert!(!valid_projection_object_key(
            report_id,
            &projection_digest,
            &format!(
                "reports/rpt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/projection-{projection_digest}.json"
            )
        ));
        assert!(!valid_projection_object_key(
            report_id,
            &projection_digest,
            &format!("reports/{report_id}/projection-{projection_digest}.json?raw=1")
        ));
    }

    #[test]
    fn reconciliation_chunk_keys_are_content_addressed_and_upload_scoped() {
        let sha256 = digest(b"chunk");
        let chunk = HostedChunk {
            sequence: 7,
            object_key: format!(
                "uploads/up_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/chunks/00000007-{sha256}.bin"
            ),
            byte_length: 5,
            sha256,
        };
        assert!(valid_reconciliation_chunk_key(
            "up_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &chunk
        ));
        assert!(!valid_reconciliation_chunk_key(
            "up_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &chunk
        ));
    }

    #[test]
    fn reconciliation_manifest_bounds_sources_and_identifiers() {
        let source = reconciliation_source(b"artifact", 2);
        let valid = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: vec![source.clone()],
        };
        assert!(validate_reconciliation_job(&valid).is_ok());

        let mut empty = valid.clone();
        empty.sources.clear();
        assert!(validate_reconciliation_job(&empty).is_err());

        let mut too_many = valid.clone();
        too_many.sources = vec![source; MAXIMUM_RECONCILIATION_SOURCES + 1];
        assert!(validate_reconciliation_job(&too_many).is_err());

        let mut invalid_group = valid;
        invalid_group.run_group_id = "run/escape".into();
        assert!(validate_reconciliation_job(&invalid_group).is_err());
    }

    #[test]
    fn reconciliation_projection_bounds_allow_raids_but_cap_container_memory() {
        let at_limit = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: vec![reconciliation_source(
                b"at-projection-limit",
                MAXIMUM_PROJECTION_BYTES,
            )],
        };
        assert!(validate_reconciliation_job(&at_limit).is_ok());

        let per_projection = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: vec![reconciliation_source(
                b"oversized-projection",
                MAXIMUM_PROJECTION_BYTES + 1,
            )],
        };
        assert!(validate_reconciliation_job(&per_projection).is_err());

        let aggregate = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: (0..5)
                .map(|index| {
                    reconciliation_source(
                        format!("aggregate-{index}").as_bytes(),
                        MAXIMUM_PROJECTION_BYTES,
                    )
                })
                .collect(),
        };
        assert!(validate_reconciliation_job(&aggregate).is_err());
    }

    #[test]
    fn reconciliation_artifact_bounds_fit_the_standard_container_pool() {
        let at_source_limit = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: vec![reconciliation_source_with_artifact_bytes(
                b"source-limit",
                MAXIMUM_RECONCILIATION_SOURCE_ARTIFACT_BYTES,
            )],
        };
        assert!(validate_reconciliation_job(&at_source_limit).is_ok());

        let over_source_limit = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: vec![reconciliation_source_with_artifact_bytes(
                b"over-source-limit",
                MAXIMUM_RECONCILIATION_SOURCE_ARTIFACT_BYTES + 1,
            )],
        };
        let error = validate_reconciliation_job(&over_source_limit).unwrap_err();
        assert!(
            matches!(error, HostedVerifierError::Rejected(message) if message.contains("container budget"))
        );

        let three_gib = 3 * 1024 * 1024 * 1024;
        let over_aggregate = HostedReconciliationJob {
            schema_version: 1,
            run_group_id: "run_exact000000000000000000000000000".into(),
            sources: (0..3)
                .map(|index| {
                    reconciliation_source_with_artifact_bytes(
                        format!("aggregate-artifact-{index}").as_bytes(),
                        three_gib,
                    )
                })
                .collect(),
        };
        let error = validate_reconciliation_job(&over_aggregate).unwrap_err();
        assert!(
            matches!(error, HostedVerifierError::Rejected(message) if message.contains("aggregate bytes"))
        );
    }

    #[test]
    fn operational_service_errors_are_retryable_but_evidence_failures_are_rejected() {
        assert!(matches!(
            HostedVerifierError::from(ServiceError::Io(std::io::Error::other("disk unavailable"))),
            HostedVerifierError::Retryable(_)
        ));
        assert!(matches!(
            HostedVerifierError::from(ServiceError::InvalidConfiguration("missing pack".into())),
            HostedVerifierError::Retryable(_)
        ));
        assert!(matches!(
            HostedVerifierError::from(ServiceError::CrossVantageReplay("bad witness".into())),
            HostedVerifierError::Rejected(_)
        ));
    }

    #[test]
    fn temporary_reconciliation_artifacts_are_cleaned_up() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("artifact.rlog");
        std::fs::write(&path, b"artifact").unwrap();
        drop(TemporaryFiles(vec![path.clone()]));
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn committed_object_download_rejects_an_oversized_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(|| async { vec![0_u8; 32] }),
            )
            .await
            .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let mut state = test_state(root.path());
        state.artifact_origin = format!("http://{address}");

        let result = download_committed_object(
            &state,
            "reports/rpt_a/projection.json",
            8,
            &digest(&[0_u8; 8]),
            8,
            "projection",
        )
        .await;
        assert!(matches!(result, Err(HostedVerifierError::Rejected(_))));
        server.abort();
    }

    #[tokio::test]
    async fn reconciliation_endpoint_fails_closed_before_opening_paths() {
        let root = tempfile::tempdir().unwrap();
        let response = router(test_state(root.path()))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v1/reconcile")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"schema_version":1,"run_group_id":"run_exact000000000000000000000000000","sources":[]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
