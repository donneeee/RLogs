use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const WEBSITE_PAYLOAD_SCHEMA_VERSION: u16 = 1;
const MAX_WEBSITE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

/// A game-neutral, privacy-reviewed unit ready for authenticated website
/// transport. Authentication is host configuration and is never part of the
/// game payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebsitePayloadEnvelope {
    pub schema_version: u16,
    pub game_plugin_id: String,
    pub payload_kind: String,
    pub payload_schema_id: String,
    pub payload_schema_version: u16,
    /// Non-secret routing fields such as deployment, region, world, and public
    /// character ID.
    pub routing: BTreeMap<String, String>,
    pub body: Value,
}

impl WebsitePayloadEnvelope {
    pub fn new(
        game_plugin_id: impl Into<String>,
        payload_kind: impl Into<String>,
        payload_schema_id: impl Into<String>,
        payload_schema_version: u16,
        routing: BTreeMap<String, String>,
        body: Value,
    ) -> Result<Self, WebsitePayloadError> {
        let value = Self {
            schema_version: WEBSITE_PAYLOAD_SCHEMA_VERSION,
            game_plugin_id: game_plugin_id.into(),
            payload_kind: payload_kind.into(),
            payload_schema_id: payload_schema_id.into(),
            payload_schema_version,
            routing,
            body,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), WebsitePayloadError> {
        if self.schema_version != WEBSITE_PAYLOAD_SCHEMA_VERSION {
            return Err(WebsitePayloadError::UnsupportedSchema {
                actual: self.schema_version,
            });
        }
        validate_identifier("game_plugin_id", &self.game_plugin_id, true)?;
        validate_identifier("payload_kind", &self.payload_kind, false)?;
        validate_identifier("payload_schema_id", &self.payload_schema_id, true)?;
        if self.payload_schema_version == 0 {
            return Err(WebsitePayloadError::ZeroPayloadSchemaVersion);
        }
        if !self.body.is_object() {
            return Err(WebsitePayloadError::BodyMustBeObject);
        }
        for (key, value) in &self.routing {
            validate_identifier("routing key", key, false)?;
            if value.trim().is_empty() || value.len() > 256 {
                return Err(WebsitePayloadError::InvalidRoutingValue { key: key.clone() });
            }
            reject_prohibited_key(key)?;
        }
        inspect_value(&self.body)?;
        let size = serde_json::to_vec(self)
            .map_err(WebsitePayloadError::Serialization)?
            .len();
        if size > MAX_WEBSITE_PAYLOAD_BYTES {
            return Err(WebsitePayloadError::PayloadTooLarge {
                actual: size,
                maximum: MAX_WEBSITE_PAYLOAD_BYTES,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebsitePayloadRequest {
    /// Relative to a host-configured website base URL. Game plug-ins are not
    /// allowed to choose credentials, schemes, or hosts.
    pub relative_endpoint: String,
    pub payload: WebsitePayloadEnvelope,
}

impl WebsitePayloadRequest {
    pub fn new(
        relative_endpoint: impl Into<String>,
        payload: WebsitePayloadEnvelope,
    ) -> Result<Self, WebsitePayloadError> {
        let value = Self {
            relative_endpoint: relative_endpoint.into(),
            payload,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), WebsitePayloadError> {
        validate_relative_endpoint(&self.relative_endpoint)?;
        self.payload.validate()
    }
}

fn inspect_value(value: &Value) -> Result<(), WebsitePayloadError> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                reject_prohibited_key(key)?;
                inspect_value(child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                inspect_value(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reject_prohibited_key(key: &str) -> Result<(), WebsitePayloadError> {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if matches!(
        normalized.as_str(),
        "password"
            | "passphrase"
            | "account"
            | "authentication"
            | "credential"
            | "credentials"
            | "login"
            | "secret"
            | "clientsecret"
            | "token"
            | "passwordciphertext"
            | "passwordhash"
            | "accesstoken"
            | "refreshtoken"
            | "authtoken"
            | "sessiontoken"
            | "authorization"
            | "bearer"
            | "cookie"
            | "sessioncookie"
            | "sessionid"
            | "accountid"
            | "platformaccountid"
            | "publisheraccountid"
            | "openid"
            | "loginname"
            | "userid"
            | "discordid"
            | "email"
            | "emailaddress"
            | "phonenumber"
    ) {
        return Err(WebsitePayloadError::ProhibitedField {
            field: key.to_owned(),
        });
    }
    Ok(())
}

fn validate_identifier(
    field: &'static str,
    value: &str,
    dotted: bool,
) -> Result<(), WebsitePayloadError> {
    let parts: Vec<_> = value.split('.').collect();
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 96
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !part.starts_with('-')
            && !part.ends_with('-')
    };
    if value.len() > 192
        || parts.iter().any(|part| !valid_part(part))
        || (dotted && parts.len() < 2)
        || (!dotted && parts.len() != 1)
    {
        return Err(WebsitePayloadError::InvalidIdentifier {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_relative_endpoint(value: &str) -> Result<(), WebsitePayloadError> {
    let Some(path) = value.strip_prefix('/') else {
        return Err(WebsitePayloadError::UnsafeRelativeEndpoint {
            value: value.to_owned(),
        });
    };
    if path.is_empty()
        || value.starts_with("//")
        || value.contains("://")
        || value.contains('?')
        || value.contains('#')
        || value.contains('%')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(WebsitePayloadError::UnsafeRelativeEndpoint {
            value: value.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum WebsitePayloadError {
    #[error("unsupported website payload schema version {actual}")]
    UnsupportedSchema { actual: u16 },

    #[error("invalid {field}: {value}")]
    InvalidIdentifier { field: &'static str, value: String },

    #[error("website profile payload schema version must be greater than zero")]
    ZeroPayloadSchemaVersion,

    #[error("website payload body must be a JSON object")]
    BodyMustBeObject,

    #[error("invalid routing value for {key}")]
    InvalidRoutingValue { key: String },

    #[error("prohibited credential or account field in website payload: {field}")]
    ProhibitedField { field: String },

    #[error("website payload is {actual} bytes; maximum is {maximum}")]
    PayloadTooLarge { actual: usize, maximum: usize },

    #[error("unsafe relative website endpoint: {value}")]
    UnsafeRelativeEndpoint { value: String },

    #[error("could not serialize website payload: {0}")]
    Serialization(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn profile() -> WebsitePayloadEnvelope {
        WebsitePayloadEnvelope::new(
            "app.rlogs.game.example",
            "character-profile",
            "app.rlogs.example.character-profile",
            1,
            BTreeMap::from([
                ("region".into(), "north-america".into()),
                ("character-id".into(), "public-123".into()),
            ]),
            json!({"display_name": "Example", "level": 60}),
        )
        .unwrap()
    }

    #[test]
    fn accepts_public_character_profile_data() {
        WebsitePayloadRequest::new("/v1/games/example/profiles", profile()).unwrap();
    }

    #[test]
    fn rejects_credentials_at_any_payload_depth() {
        let error = WebsitePayloadEnvelope::new(
            "app.rlogs.game.example",
            "character-profile",
            "app.rlogs.example.character-profile",
            1,
            BTreeMap::new(),
            json!({"character": {"password": "must-never-leave"}}),
        )
        .unwrap_err();
        assert!(matches!(error, WebsitePayloadError::ProhibitedField { .. }));
    }

    #[test]
    fn rejects_account_containers_that_try_to_hide_the_identifier() {
        let error = WebsitePayloadEnvelope::new(
            "app.rlogs.game.example",
            "character-profile",
            "app.rlogs.example.character-profile",
            1,
            BTreeMap::new(),
            json!({"account": {"id": "must-never-leave"}}),
        )
        .unwrap_err();
        assert!(matches!(error, WebsitePayloadError::ProhibitedField { .. }));
    }

    #[test]
    fn rejects_game_selected_remote_hosts() {
        let error =
            WebsitePayloadRequest::new("https://example.invalid/upload", profile()).unwrap_err();
        assert!(matches!(
            error,
            WebsitePayloadError::UnsafeRelativeEndpoint { .. }
        ));
    }
}
