use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

pub const CURRENT_SUBMISSION_SCHEMA: u16 = 1;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self, DigestError> {
        let value = value.into();
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DigestError::InvalidSha256(value));
        }

        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DigestError {
    #[error("expected a 64-character hexadecimal SHA-256 digest, got {0:?}")]
    InvalidSha256(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionMode {
    Live,
    PostRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportVisibility {
    Private,
    Unlisted,
    Public,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Draft,
    Uploading,
    Finalizing,
    Submitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationTier {
    Uploaded,
    Replayed,
    Corroborated,
    Ranked,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubmissionMetadata {
    pub schema_version: u16,
    pub local_log_id: String,
    pub log_format_version: u16,
    pub capture_session_id: String,
    pub game_region: String,
    pub client_build: String,
    pub protocol_pack_digest: Sha256Digest,
    pub privacy_policy_digest: Sha256Digest,
    pub visibility: ReportVisibility,
}

impl SubmissionMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        local_log_id: impl Into<String>,
        log_format_version: u16,
        capture_session_id: impl Into<String>,
        game_region: impl Into<String>,
        client_build: impl Into<String>,
        protocol_pack_digest: Sha256Digest,
        privacy_policy_digest: Sha256Digest,
        visibility: ReportVisibility,
    ) -> Self {
        Self {
            schema_version: CURRENT_SUBMISSION_SCHEMA,
            local_log_id: local_log_id.into(),
            log_format_version,
            capture_session_id: capture_session_id.into(),
            game_region: game_region.into(),
            client_build: client_build.into(),
            protocol_pack_digest,
            privacy_policy_digest,
            visibility,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogChunkDescriptor {
    pub sequence: u64,
    pub file_offset: u64,
    pub byte_length: u64,
    pub sha256: Sha256Digest,
}

impl LogChunkDescriptor {
    pub fn new(sequence: u64, file_offset: u64, byte_length: u64, sha256: Sha256Digest) -> Self {
        Self {
            sequence,
            file_offset,
            byte_length,
            sha256,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UploadManifest {
    pub metadata: SubmissionMetadata,
    pub chunks: Vec<LogChunkDescriptor>,
    pub sealed_log_digest: Option<Sha256Digest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerReportReceipt {
    pub report_id: String,
    pub accepted_log_digest: Sha256Digest,
    pub verification_tier: VerificationTier,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_deserialization_validates_and_normalizes() {
        let uppercase = format!("\"{}\"", "AB".repeat(32));
        let digest: Sha256Digest = serde_json::from_str(&uppercase).unwrap();
        assert_eq!(digest.as_str(), "ab".repeat(32));

        assert!(serde_json::from_str::<Sha256Digest>("\"not-a-digest\"").is_err());
    }
}
