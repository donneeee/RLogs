//! Stable manifests, capabilities, and subscriptions for RLogs plugins.

use std::collections::BTreeSet;

use rlogs_events::EventTopic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLUGIN_API_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventsRead,
    EncountersRead,
    CharacterProfilesRead,
    OverlayPublish,
    ScopedStorage,
    NetworkAccess,
    RawProtocolResearch,
    UnsafeNativeExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntime {
    WasmComponent,
    BrowserOverlay,
    ExternalProcess,
    NativeDeveloper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Reverse-domain identifier, for example `app.rlogs.combat-meter`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u16,
    pub runtime: PluginRuntime,
    pub capabilities: BTreeSet<PluginCapability>,
    pub subscriptions: BTreeSet<EventTopic>,
    /// Empty unless `NetworkAccess` is requested.
    pub allowed_network_domains: Vec<String>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_id(&self.id)?;

        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }
        if self.version.trim().is_empty() {
            return Err(ManifestError::EmptyVersion);
        }
        if self.api_version != PLUGIN_API_VERSION {
            return Err(ManifestError::UnsupportedApiVersion {
                supported: PLUGIN_API_VERSION,
                requested: self.api_version,
            });
        }

        let requests_network = self.capabilities.contains(&PluginCapability::NetworkAccess);
        if !requests_network && !self.allowed_network_domains.is_empty() {
            return Err(ManifestError::NetworkDomainsWithoutCapability);
        }

        let requests_native = self
            .capabilities
            .contains(&PluginCapability::UnsafeNativeExecution);
        match (self.runtime, requests_native) {
            (PluginRuntime::NativeDeveloper, false) => {
                return Err(ManifestError::NativeRuntimeWithoutCapability);
            }
            (PluginRuntime::NativeDeveloper, true) => {}
            (_, true) => return Err(ManifestError::NativeCapabilityOnSandboxedRuntime),
            (_, false) => {}
        }

        Ok(())
    }
}

fn validate_id(id: &str) -> Result<(), ManifestError> {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return Err(ManifestError::InvalidId);
    };

    if !first.is_ascii_lowercase()
        || !characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '-')
        })
        || !id.contains('.')
    {
        return Err(ManifestError::InvalidId);
    }

    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("plugin id must be a lowercase reverse-domain identifier")]
    InvalidId,

    #[error("plugin name cannot be empty")]
    EmptyName,

    #[error("plugin version cannot be empty")]
    EmptyVersion,

    #[error("plugin API version {requested} is unsupported; this host supports {supported}")]
    UnsupportedApiVersion { supported: u16, requested: u16 },

    #[error("network domains require the network_access capability")]
    NetworkDomainsWithoutCapability,

    #[error("native developer plugins must request unsafe_native_execution")]
    NativeRuntimeWithoutCapability,

    #[error("unsafe_native_execution is only valid for a native developer plugin")]
    NativeCapabilityOnSandboxedRuntime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(runtime: PluginRuntime) -> PluginManifest {
        PluginManifest {
            id: "app.rlogs.example".into(),
            name: "Example".into(),
            version: "0.1.0".into(),
            api_version: PLUGIN_API_VERSION,
            runtime,
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::ScopedStorage,
            ]),
            subscriptions: BTreeSet::from([EventTopic::Combat, EventTopic::Encounter]),
            allowed_network_domains: vec![],
        }
    }

    #[test]
    fn a_sandboxed_analyzer_uses_only_public_events() {
        let plugin = manifest(PluginRuntime::WasmComponent);

        assert_eq!(plugin.validate(), Ok(()));
        assert!(plugin.capabilities.contains(&PluginCapability::EventsRead));
        assert!(
            !plugin
                .capabilities
                .contains(&PluginCapability::RawProtocolResearch)
        );
    }

    #[test]
    fn network_domains_require_an_explicit_capability() {
        let mut plugin = manifest(PluginRuntime::ExternalProcess);
        plugin.allowed_network_domains = vec!["example.invalid".into()];

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::NetworkDomainsWithoutCapability)
        );
    }

    #[test]
    fn unrestricted_native_execution_cannot_be_hidden_in_a_normal_plugin() {
        let mut plugin = manifest(PluginRuntime::WasmComponent);
        plugin
            .capabilities
            .insert(PluginCapability::UnsafeNativeExecution);

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::NativeCapabilityOnSandboxedRuntime)
        );
    }

    #[test]
    fn a_native_developer_plugin_is_explicitly_unsafe() {
        let mut plugin = manifest(PluginRuntime::NativeDeveloper);
        assert_eq!(
            plugin.validate(),
            Err(ManifestError::NativeRuntimeWithoutCapability)
        );

        plugin
            .capabilities
            .insert(PluginCapability::UnsafeNativeExecution);
        assert_eq!(plugin.validate(), Ok(()));
    }

    #[test]
    fn manifests_round_trip_for_language_neutral_transport() {
        let plugin = manifest(PluginRuntime::BrowserOverlay);
        let json = serde_json::to_string(&plugin).unwrap();
        let decoded: PluginManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, plugin);
    }
}
