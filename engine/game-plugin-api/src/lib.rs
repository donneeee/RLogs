//! Versioned declarations for trusted native game integrations.

use std::collections::BTreeSet;
use std::path::{Component, Path};

pub use rlogs_plugin_api::ResourceStorage;
use rlogs_plugin_api::SharedResourceExport;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GAME_PLUGIN_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const GAME_PLUGIN_API_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamePluginRuntime {
    TrustedNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamePluginCapability {
    ProcessDiscovery,
    PacketFraming,
    PacketDecoding,
    RegionResolution,
    GameData,
    CharacterProfiles,
    WebsiteProfiles,
    Localization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSelector {
    #[serde(default)]
    pub windows_executable_names: Vec<String>,
    #[serde(default)]
    pub linux_process_names: Vec<String>,
}

impl ProcessSelector {
    fn is_empty(&self) -> bool {
        self.windows_executable_names.is_empty() && self.linux_process_names.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePluginResources {
    pub protocol_packs: Option<String>,
    pub game_data_catalog: Option<String>,
    pub research_inventory: Option<String>,
    pub localization_staging: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsiteProfileContract {
    pub payload_schema_id: String,
    pub payload_schema_version: u16,
    /// Relative to the website base URL configured by the user or distributor.
    pub relative_endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePluginManifest {
    pub schema_version: u16,
    pub id: String,
    pub game_id: String,
    pub name: String,
    pub version: String,
    pub api_version: u16,
    pub runtime: GamePluginRuntime,
    pub capabilities: BTreeSet<GamePluginCapability>,
    pub process_selector: Option<ProcessSelector>,
    pub resources: GamePluginResources,
    #[serde(default)]
    pub resource_exports: Vec<SharedResourceExport>,
    pub website_profiles: Option<WebsiteProfileContract>,
}

impl GamePluginManifest {
    pub fn from_toml(bytes: &[u8]) -> Result<Self, GamePluginManifestError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|source| GamePluginManifestError::InvalidUtf8 { source })?;
        let manifest: Self = toml::from_str(text)
            .map_err(|source| GamePluginManifestError::InvalidToml { source })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), GamePluginManifestError> {
        if self.schema_version != GAME_PLUGIN_MANIFEST_SCHEMA_VERSION {
            return Err(GamePluginManifestError::UnsupportedManifestSchema {
                actual: self.schema_version,
            });
        }
        if self.api_version != GAME_PLUGIN_API_VERSION {
            return Err(GamePluginManifestError::UnsupportedApiVersion {
                actual: self.api_version,
            });
        }
        validate_reverse_domain_id(&self.id)?;
        validate_slug("game_id", &self.game_id)?;
        validate_nonempty("name", &self.name)?;
        validate_nonempty("version", &self.version)?;

        if self
            .capabilities
            .contains(&GamePluginCapability::ProcessDiscovery)
            && self
                .process_selector
                .as_ref()
                .is_none_or(ProcessSelector::is_empty)
        {
            return Err(GamePluginManifestError::MissingProcessSelector);
        }
        if let Some(selector) = &self.process_selector {
            for executable in selector
                .windows_executable_names
                .iter()
                .chain(&selector.linux_process_names)
            {
                validate_file_name(executable)?;
            }
        }

        validate_optional_relative_path(
            "resources.protocol_packs",
            self.resources.protocol_packs.as_deref(),
        )?;
        validate_optional_relative_path(
            "resources.game_data_catalog",
            self.resources.game_data_catalog.as_deref(),
        )?;
        validate_optional_relative_path(
            "resources.research_inventory",
            self.resources.research_inventory.as_deref(),
        )?;
        validate_optional_relative_path(
            "resources.localization_staging",
            self.resources.localization_staging.as_deref(),
        )?;

        validate_capability_resource(
            &self.capabilities,
            GamePluginCapability::PacketDecoding,
            "resources.protocol_packs",
            self.resources.protocol_packs.as_deref(),
        )?;

        let mut export_names = BTreeSet::new();
        for export in &self.resource_exports {
            validate_slug("resource export name", &export.name)?;
            validate_slug("resource kind", &export.kind)?;
            validate_relative_path("resource export path", &export.path)?;
            validate_reverse_domain_id(&export.schema_id)?;
            if export.schema_version == 0 {
                return Err(GamePluginManifestError::ZeroResourceSchemaVersion {
                    resource: export.name.clone(),
                });
            }
            if !export_names.insert(&export.name) {
                return Err(GamePluginManifestError::DuplicateResourceExport {
                    resource: export.name.clone(),
                });
            }
        }
        validate_capability_resource(
            &self.capabilities,
            GamePluginCapability::GameData,
            "resources.game_data_catalog",
            self.resources.game_data_catalog.as_deref(),
        )?;

        match (
            self.capabilities
                .contains(&GamePluginCapability::WebsiteProfiles),
            &self.website_profiles,
        ) {
            (true, None) => return Err(GamePluginManifestError::MissingWebsiteProfileContract),
            (false, Some(_)) => {
                return Err(GamePluginManifestError::UndeclaredWebsiteProfileCapability);
            }
            (_, None) => {}
            (_, Some(contract)) => {
                validate_reverse_domain_id(&contract.payload_schema_id)?;
                if contract.payload_schema_version == 0 {
                    return Err(GamePluginManifestError::ZeroPayloadSchemaVersion);
                }
                validate_relative_endpoint(&contract.relative_endpoint)?;
            }
        }

        Ok(())
    }
}

fn validate_capability_resource(
    capabilities: &BTreeSet<GamePluginCapability>,
    capability: GamePluginCapability,
    field: &'static str,
    value: Option<&str>,
) -> Result<(), GamePluginManifestError> {
    if capabilities.contains(&capability) && value.is_none() {
        return Err(GamePluginManifestError::MissingResource { field });
    }
    Ok(())
}

fn validate_optional_relative_path(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), GamePluginManifestError> {
    if let Some(value) = value {
        validate_relative_path(field, value)?;
    }
    Ok(())
}

fn validate_relative_path(field: &'static str, value: &str) -> Result<(), GamePluginManifestError> {
    if value.is_empty()
        || value.contains('\\')
        || Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GamePluginManifestError::UnsafeRelativePath {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_relative_endpoint(value: &str) -> Result<(), GamePluginManifestError> {
    let path =
        value
            .strip_prefix('/')
            .ok_or_else(|| GamePluginManifestError::UnsafeRelativeEndpoint {
                value: value.to_owned(),
            })?;
    if path.is_empty()
        || value.starts_with("//")
        || value.contains('?')
        || value.contains('#')
        || value.contains("://")
        || value.contains('%')
    {
        return Err(GamePluginManifestError::UnsafeRelativeEndpoint {
            value: value.to_owned(),
        });
    }
    validate_relative_path("website_profiles.relative_endpoint", path).map_err(|_| {
        GamePluginManifestError::UnsafeRelativeEndpoint {
            value: value.to_owned(),
        }
    })
}

fn validate_reverse_domain_id(value: &str) -> Result<(), GamePluginManifestError> {
    if value.len() > 160
        || !value.contains('.')
        || value
            .split('.')
            .any(|part| part.is_empty() || !is_slug(part))
    {
        return Err(GamePluginManifestError::InvalidIdentifier {
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_slug(field: &'static str, value: &str) -> Result<(), GamePluginManifestError> {
    if !is_slug(value) {
        return Err(GamePluginManifestError::InvalidSlug {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), GamePluginManifestError> {
    if value.trim().is_empty() || value.len() > 160 {
        return Err(GamePluginManifestError::EmptyOrOversized { field });
    }
    Ok(())
}

fn validate_file_name(value: &str) -> Result<(), GamePluginManifestError> {
    if value.is_empty()
        || value.len() > 255
        || value.contains('/')
        || value.contains('\\')
        || matches!(value, "." | "..")
    {
        return Err(GamePluginManifestError::InvalidProcessName {
            value: value.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum GamePluginManifestError {
    #[error("game plug-in manifest is not UTF-8: {source}")]
    InvalidUtf8 { source: std::str::Utf8Error },

    #[error("game plug-in manifest is not valid TOML: {source}")]
    InvalidToml { source: toml::de::Error },

    #[error("unsupported game plug-in manifest schema version {actual}")]
    UnsupportedManifestSchema { actual: u16 },

    #[error("unsupported game plug-in API version {actual}")]
    UnsupportedApiVersion { actual: u16 },

    #[error("invalid reverse-domain identifier: {value}")]
    InvalidIdentifier { value: String },

    #[error("{field} must be a lowercase ASCII slug: {value}")]
    InvalidSlug { field: &'static str, value: String },

    #[error("{field} must be nonempty and at most 160 bytes")]
    EmptyOrOversized { field: &'static str },

    #[error("process discovery requires at least one process selector")]
    MissingProcessSelector,

    #[error("invalid process or executable name: {value}")]
    InvalidProcessName { value: String },

    #[error("unsafe relative plug-in path in {field}: {value}")]
    UnsafeRelativePath { field: &'static str, value: String },

    #[error("{field} is required by a declared capability")]
    MissingResource { field: &'static str },

    #[error("website_profiles is required by the website_profiles capability")]
    MissingWebsiteProfileContract,

    #[error("website_profiles was supplied without the website_profiles capability")]
    UndeclaredWebsiteProfileCapability,

    #[error("website profile payload schema version must be greater than zero")]
    ZeroPayloadSchemaVersion,

    #[error("duplicate shared resource export {resource}")]
    DuplicateResourceExport { resource: String },

    #[error("shared resource {resource} has schema version zero")]
    ZeroResourceSchemaVersion { resource: String },

    #[error("unsafe relative website endpoint: {value}")]
    UnsafeRelativeEndpoint { value: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> GamePluginManifest {
        GamePluginManifest {
            schema_version: GAME_PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: "app.rlogs.game.example".into(),
            game_id: "example-game".into(),
            name: "Example Game".into(),
            version: "0.1.0".into(),
            api_version: GAME_PLUGIN_API_VERSION,
            runtime: GamePluginRuntime::TrustedNative,
            capabilities: [
                GamePluginCapability::ProcessDiscovery,
                GamePluginCapability::PacketDecoding,
                GamePluginCapability::GameData,
                GamePluginCapability::WebsiteProfiles,
            ]
            .into_iter()
            .collect(),
            process_selector: Some(ProcessSelector {
                windows_executable_names: vec!["Example.exe".into()],
                linux_process_names: vec!["example-game".into()],
            }),
            resources: GamePluginResources {
                protocol_packs: Some("protocol-packs".into()),
                game_data_catalog: Some("game-data/catalog".into()),
                research_inventory: None,
                localization_staging: None,
            },
            resource_exports: vec![SharedResourceExport {
                name: "catalog".into(),
                kind: "game-data-catalog".into(),
                storage: ResourceStorage::Package,
                path: "game-data/catalog".into(),
                schema_id: "app.rlogs.example.game-data".into(),
                schema_version: 2,
            }],
            website_profiles: Some(WebsiteProfileContract {
                payload_schema_id: "app.rlogs.example.character-profile".into(),
                payload_schema_version: 1,
                relative_endpoint: "/v1/games/example-game/profiles".into(),
            }),
        }
    }

    #[test]
    fn accepts_a_complete_trusted_game_plugin() {
        manifest().validate().unwrap();
    }

    #[test]
    fn rejects_paths_that_escape_the_plugin_root() {
        let mut value = manifest();
        value.resources.protocol_packs = Some("../private".into());
        assert!(matches!(
            value.validate(),
            Err(GamePluginManifestError::UnsafeRelativePath { .. })
        ));
    }

    #[test]
    fn rejects_absolute_or_remote_upload_endpoints() {
        let mut value = manifest();
        value.website_profiles.as_mut().unwrap().relative_endpoint =
            "https://example.invalid/profile".into();
        assert!(matches!(
            value.validate(),
            Err(GamePluginManifestError::UnsafeRelativeEndpoint { .. })
        ));
    }
}
