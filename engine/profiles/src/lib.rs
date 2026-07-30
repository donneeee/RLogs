//! Game-neutral local character-profile packages.

use rlogs_submission::{WebsitePayloadError, WebsitePayloadRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const LOCAL_PROFILE_PACKAGE_SCHEMA_VERSION: u16 = 1;
pub const MAXIMUM_PROFILE_SOURCE_ID_BYTES: usize = 256;

/// Exact sealed-log evidence from which a trusted game plug-in projected a
/// character profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePackageSource {
    pub session_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub canonical_content_sha256: String,
    pub observation_count: u64,
    pub last_event_sequence: u64,
}

/// A local, reviewable website payload. This contains no host, credentials, or
/// authorization and is not itself permission to transmit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalProfilePackage {
    pub schema_version: u16,
    /// SHA-256 of the canonical, recursively key-sorted JSON representation of
    /// the game-neutral website request. This makes the seal reproducible in
    /// Rust, browser JavaScript, and future backends.
    pub package_id: String,
    pub created_unix_millis: u64,
    pub source: ProfilePackageSource,
    pub request: WebsitePayloadRequest,
}

impl LocalProfilePackage {
    pub fn new(
        created_unix_millis: u64,
        source: ProfilePackageSource,
        request: WebsitePayloadRequest,
    ) -> Result<Self, ProfilePackageError> {
        let value = Self {
            schema_version: LOCAL_PROFILE_PACKAGE_SCHEMA_VERSION,
            package_id: request_digest(&request)?,
            created_unix_millis,
            source,
            request,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ProfilePackageError> {
        if self.schema_version != LOCAL_PROFILE_PACKAGE_SCHEMA_VERSION {
            return Err(ProfilePackageError::UnsupportedSchema {
                actual: self.schema_version,
            });
        }
        if self.created_unix_millis == 0 {
            return Err(ProfilePackageError::MissingCreationTime);
        }
        self.request.validate()?;
        if self.request.payload.payload_kind != "character-profile" {
            return Err(ProfilePackageError::WrongPayloadKind);
        }
        for key in ["deployment", "region", "character-id"] {
            if !self.request.payload.routing.contains_key(key) {
                return Err(ProfilePackageError::MissingRoutingField { key });
            }
        }
        validate_source_text("session_id", &self.source.session_id)?;
        validate_source_text("client_build", &self.source.client_build)?;
        validate_source_text("protocol_pack_digest", &self.source.protocol_pack_digest)?;
        if !is_prefixed_sha256(&self.source.canonical_content_sha256) {
            return Err(ProfilePackageError::InvalidCanonicalDigest);
        }
        if self.source.observation_count == 0 || self.source.last_event_sequence == 0 {
            return Err(ProfilePackageError::MissingObservationEvidence);
        }
        let expected = request_digest(&self.request)?;
        if self.package_id != expected {
            return Err(ProfilePackageError::PackageDigestMismatch {
                expected,
                actual: self.package_id.clone(),
            });
        }
        Ok(())
    }
}

fn validate_source_text(field: &'static str, value: &str) -> Result<(), ProfilePackageError> {
    if value.trim().is_empty() || value.len() > MAXIMUM_PROFILE_SOURCE_ID_BYTES {
        return Err(ProfilePackageError::InvalidSourceField { field });
    }
    Ok(())
}

fn request_digest(request: &WebsitePayloadRequest) -> Result<String, ProfilePackageError> {
    // `serde_json::Value` objects use sorted keys without `preserve_order`.
    // Converting the typed request first therefore removes Rust struct-field
    // ordering from the digest contract and gives other runtimes a small,
    // deterministic canonicalization rule to reproduce.
    let canonical = serde_json::to_value(request).map_err(ProfilePackageError::Serialization)?;
    let bytes = serde_json::to_vec(&canonical).map_err(ProfilePackageError::Serialization)?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[derive(Debug, Error)]
pub enum ProfilePackageError {
    #[error("unsupported local profile package schema {actual}")]
    UnsupportedSchema { actual: u16 },

    #[error("local profile package has no creation time")]
    MissingCreationTime,

    #[error("profile package payload kind is not character-profile")]
    WrongPayloadKind,

    #[error("profile package is missing routing field {key}")]
    MissingRoutingField { key: &'static str },

    #[error("profile package source field {field} is invalid")]
    InvalidSourceField { field: &'static str },

    #[error("profile package canonical-content digest is invalid")]
    InvalidCanonicalDigest,

    #[error("profile package has no observation evidence")]
    MissingObservationEvidence,

    #[error("profile package ID does not match the request: expected {expected}, got {actual}")]
    PackageDigestMismatch { expected: String, actual: String },

    #[error("profile package website request is invalid: {0}")]
    Website(#[from] WebsitePayloadError),

    #[error("could not encode profile package request: {0}")]
    Serialization(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rlogs_submission::{WebsitePayloadEnvelope, WebsitePayloadRequest};
    use serde_json::json;

    use super::*;

    fn package() -> LocalProfilePackage {
        let payload = WebsitePayloadEnvelope::new(
            "app.rlogs.game.example",
            "character-profile",
            "app.rlogs.example.character-profile",
            1,
            BTreeMap::from([
                ("deployment".into(), "global".into()),
                ("region".into(), "north-america".into()),
                ("character-id".into(), "123456".into()),
            ]),
            json!({"display_name": "Example", "level": 60}),
        )
        .unwrap();
        LocalProfilePackage::new(
            1,
            ProfilePackageSource {
                session_id: "session-1".into(),
                client_build: "build-1".into(),
                protocol_pack_digest: "sha256:pack".into(),
                canonical_content_sha256: format!("sha256:{}", "a".repeat(64)),
                observation_count: 2,
                last_event_sequence: 9,
            },
            WebsitePayloadRequest::new("/v1/games/example/profiles", payload).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn package_round_trips_and_revalidates_exact_request_digest() {
        let package = package();
        let restored: LocalProfilePackage =
            serde_json::from_slice(&serde_json::to_vec(&package).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.package_id.len(), 64);
    }

    #[test]
    fn request_or_source_tampering_is_rejected() {
        let mut body_tampered = package();
        body_tampered.request.payload.body["level"] = json!(61);
        assert!(matches!(
            body_tampered.validate().unwrap_err(),
            ProfilePackageError::PackageDigestMismatch { .. }
        ));

        let mut source_tampered = package();
        source_tampered.source.canonical_content_sha256 = "not-a-digest".into();
        assert!(matches!(
            source_tampered.validate().unwrap_err(),
            ProfilePackageError::InvalidCanonicalDigest
        ));
    }

    #[test]
    fn request_digest_uses_recursively_sorted_json_keys() {
        let request = package().request;
        let typed_digest = request_digest(&request).unwrap();
        let deliberately_reordered = serde_json::json!({
            "payload": {
                "routing": {
                    "region": "north-america",
                    "deployment": "global",
                    "character-id": "123456"
                },
                "schema_version": 1,
                "payload_schema_version": 1,
                "payload_schema_id": "app.rlogs.example.character-profile",
                "payload_kind": "character-profile",
                "game_plugin_id": "app.rlogs.game.example",
                "body": {
                    "level": 60,
                    "display_name": "Example"
                }
            },
            "relative_endpoint": "/v1/games/example/profiles"
        });
        let bytes = serde_json::to_vec(&deliberately_reordered).unwrap();
        let reordered_digest = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        assert_eq!(typed_digest, reordered_digest);
        assert_eq!(
            typed_digest,
            "9e4ccb06bb416aef8630df13fb4fffa8d1cd9b79ff84fd8c23414c87b9cdd287"
        );
    }
}
