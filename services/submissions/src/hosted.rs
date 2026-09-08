//! Internal, stateless replay endpoint used by the Cloudflare verifier Worker.

use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::PathBuf};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rlogs_submission::{Sha256Digest, SubmissionPurpose, UploadManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    HostedVerificationResult, PublicSubmissionProvenance, ServiceError,
    verify_hosted_submission_path, verify_hosted_training_dummy_path,
};

const MAXIMUM_JOB_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_CHUNK_BYTES: u64 = 16 * 1024 * 1024;

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

#[derive(Debug, Serialize)]
struct HostedErrorBody {
    error: String,
}

pub fn router(state: HostedVerifierState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/internal/v1/verify", post(verify))
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

async fn download_and_verify(
    state: &HostedVerifierState,
    job: &HostedVerificationJob,
    path: &PathBuf,
) -> Result<HostedVerificationResult, HostedVerifierError> {
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    let mut artifact_hasher = Sha256::new();
    for chunk in &job.chunks {
        let encoded = percent_encode_object_key(&chunk.object_key);
        let response = state
            .client
            .get(format!("{}{encoded}", state.artifact_origin))
            .send()
            .await
            .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?;
        if !response.status().is_success() {
            return Err(HostedVerifierError::Retryable(format!(
                "artifact chunk {} returned HTTP {}",
                chunk.sequence,
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| HostedVerifierError::Retryable(error.to_string()))?;
        if u64::try_from(bytes.len()).ok() != Some(chunk.byte_length) {
            return Err(HostedVerifierError::Rejected(format!(
                "artifact chunk {} length does not match its commitment",
                chunk.sequence
            )));
        }
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != chunk.sha256.as_str() {
            return Err(HostedVerifierError::Rejected(format!(
                "artifact chunk {} digest does not match its commitment",
                chunk.sequence
            )));
        }
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
    value.starts_with(&format!("uploads/{upload_id}/chunks/"))
        && !value.contains("..")
        && !value.contains('\\')
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
        Self::Rejected(value.to_string())
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
    }

    #[test]
    fn object_key_encoding_preserves_paths_without_query_injection() {
        assert_eq!(
            percent_encode_object_key("uploads/up_a/chunks/a b?#.bin"),
            "/uploads/up_a/chunks/a%20b%3F%23.bin"
        );
    }
}
