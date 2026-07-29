//! Stable manifests, capabilities, and subscriptions for RLogs plugins.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use rlogs_events::EventTopic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLUGIN_API_VERSION: u16 = 1;
pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const PLUGIN_MANIFEST_FILE_NAME: &str = "plugin.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventsRead,
    EncountersRead,
    CharacterProfilesRead,
    LocalChatRead,
    OverlayPublish,
    ScopedStorage,
    NetworkAccess,
    RawProtocolResearch,
    UnsafeNativeExecution,
    SharedResourcesRead,
    LocalizationTransform,
    SubmissionTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntime {
    DataOnly,
    WasmComponent,
    BrowserOverlay,
    ExternalProcess,
    NativeDeveloper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDependency {
    pub plugin_id: String,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStorage {
    /// The resource ships inside the plug-in package.
    #[default]
    Package,
    /// The resource lives under the host-derived
    /// `assets/<plugin-folder-name>/` namespace.
    PluginAssets,
    /// A provider-owned resource intended for reuse by other plug-ins. It
    /// lives under `assets/shared/<provider-plugin-folder-name>/`.
    SharedAssets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedResourceExport {
    /// Unique within the owning plug-in.
    pub name: String,
    /// Extensible semantic kind, such as `game_data_catalog` or
    /// `localization_aliases`.
    pub kind: String,
    /// Selects the host-controlled root used to resolve `path`.
    #[serde(default)]
    pub storage: ResourceStorage,
    /// File or directory relative to the selected storage root.
    pub path: String,
    pub schema_id: String,
    pub schema_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedResourceImport {
    pub owner_plugin_id: String,
    pub name: String,
    pub schema_id: Option<String>,
    pub minimum_schema_version: Option<u16>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    CanonicalEvent,
    EncounterReduction,
    LocalizationLookup,
    ProfileProjection,
    SubmissionBuild,
}

impl OperationStage {
    pub const ALL: [Self; 5] = [
        Self::CanonicalEvent,
        Self::EncounterReduction,
        Self::LocalizationLookup,
        Self::ProfileProjection,
        Self::SubmissionBuild,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    BeforeCore,
    AfterCore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginHookHandler {
    /// A host-provided declarative transform backed by one exported resource.
    DataResource { resource: String },
    /// An operation exposed by the package entrypoint.
    Entrypoint { operation: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHook {
    pub stage: OperationStage,
    pub phase: HookPhase,
    /// Lower values run first when dependency and before/after edges do not
    /// otherwise constrain the order.
    #[serde(default)]
    pub priority: i32,
    pub handler: PluginHookHandler,
    /// Optional plug-in IDs. Missing targets are ignored so ordering can refer
    /// to optional companion packages.
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub schema_version: u16,
    /// Reverse-domain identifier, for example `app.rlogs.combat-meter`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u16,
    pub runtime: PluginRuntime,
    /// Executable, component, or web entrypoint relative to the package root.
    /// Data-only plug-ins omit this field.
    pub entrypoint: Option<String>,
    pub capabilities: BTreeSet<PluginCapability>,
    pub subscriptions: BTreeSet<EventTopic>,
    /// Empty unless `NetworkAccess` is requested.
    pub allowed_network_domains: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    #[serde(default)]
    pub resource_exports: Vec<SharedResourceExport>,
    #[serde(default)]
    pub resource_imports: Vec<SharedResourceImport>,
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
}

impl PluginManifest {
    pub fn from_toml(bytes: &[u8]) -> Result<Self, PluginManifestLoadError> {
        let text = std::str::from_utf8(bytes)?;
        let manifest: Self = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedManifestSchemaVersion {
                supported: PLUGIN_MANIFEST_SCHEMA_VERSION,
                requested: self.schema_version,
            });
        }
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
        if self.subscriptions.contains(&EventTopic::Chat)
            && !self.capabilities.contains(&PluginCapability::LocalChatRead)
        {
            return Err(ManifestError::ChatSubscriptionWithoutCapability);
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

        match (self.runtime, self.entrypoint.as_deref()) {
            (PluginRuntime::DataOnly, Some(_)) => {
                return Err(ManifestError::DataOnlyEntrypoint);
            }
            (PluginRuntime::DataOnly, None) => {}
            (_, None) => return Err(ManifestError::MissingEntrypoint),
            (_, Some(path)) => validate_relative_path("entrypoint", path)?,
        }

        let mut dependency_ids = BTreeSet::new();
        for dependency in &self.dependencies {
            validate_id(&dependency.plugin_id)?;
            if dependency.plugin_id == self.id {
                return Err(ManifestError::SelfDependency);
            }
            if !dependency_ids.insert(&dependency.plugin_id) {
                return Err(ManifestError::DuplicateDependency {
                    plugin_id: dependency.plugin_id.clone(),
                });
            }
        }

        let mut exported_resources = BTreeMap::new();
        for resource in &self.resource_exports {
            validate_slug("resource export name", &resource.name)?;
            validate_slug("resource kind", &resource.kind)?;
            validate_relative_path("resource export path", &resource.path)?;
            validate_id(&resource.schema_id)?;
            if resource.schema_version == 0 {
                return Err(ManifestError::ZeroResourceSchemaVersion {
                    resource: resource.name.clone(),
                });
            }
            if exported_resources
                .insert(resource.name.as_str(), resource)
                .is_some()
            {
                return Err(ManifestError::DuplicateResourceExport {
                    resource: resource.name.clone(),
                });
            }
        }

        if !self.resource_imports.is_empty()
            && !self
                .capabilities
                .contains(&PluginCapability::SharedResourcesRead)
        {
            return Err(ManifestError::ResourceImportsWithoutCapability);
        }
        let mut imported_resources = BTreeSet::new();
        for resource in &self.resource_imports {
            validate_id(&resource.owner_plugin_id)?;
            validate_slug("resource import name", &resource.name)?;
            if let Some(schema_id) = &resource.schema_id {
                validate_id(schema_id)?;
            }
            if resource.minimum_schema_version == Some(0) {
                return Err(ManifestError::ZeroImportedResourceSchemaVersion {
                    owner_plugin_id: resource.owner_plugin_id.clone(),
                    resource: resource.name.clone(),
                });
            }
            if !imported_resources.insert((&resource.owner_plugin_id, &resource.name)) {
                return Err(ManifestError::DuplicateResourceImport {
                    owner_plugin_id: resource.owner_plugin_id.clone(),
                    resource: resource.name.clone(),
                });
            }
        }

        let mut stages = BTreeSet::new();
        for hook in &self.hooks {
            if !stages.insert(hook.stage) {
                return Err(ManifestError::DuplicateHookStage { stage: hook.stage });
            }
            if hook.stage == OperationStage::LocalizationLookup
                && !self
                    .capabilities
                    .contains(&PluginCapability::LocalizationTransform)
            {
                return Err(ManifestError::LocalizationHookWithoutCapability);
            }
            if hook.stage == OperationStage::SubmissionBuild
                && !self
                    .capabilities
                    .contains(&PluginCapability::SubmissionTransform)
            {
                return Err(ManifestError::SubmissionHookWithoutCapability);
            }
            for plugin_id in hook.before.iter().chain(&hook.after) {
                validate_id(plugin_id)?;
                if plugin_id == &self.id {
                    return Err(ManifestError::SelfHookOrdering);
                }
            }
            match (&self.runtime, &hook.handler) {
                (PluginRuntime::DataOnly, PluginHookHandler::DataResource { resource }) => {
                    if !exported_resources.contains_key(resource.as_str()) {
                        return Err(ManifestError::UnknownHookResource {
                            resource: resource.clone(),
                        });
                    }
                }
                (PluginRuntime::DataOnly, PluginHookHandler::Entrypoint { .. }) => {
                    return Err(ManifestError::DataOnlyEntrypointHook);
                }
                (_, PluginHookHandler::Entrypoint { operation }) => {
                    validate_slug("hook operation", operation)?;
                }
                (_, PluginHookHandler::DataResource { resource }) => {
                    if !exported_resources.contains_key(resource.as_str()) {
                        return Err(ManifestError::UnknownHookResource {
                            resource: resource.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

fn validate_slug(field: &'static str, value: &str) -> Result<(), ManifestError> {
    if value.is_empty()
        || value.len() > 96
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(ManifestError::InvalidSlug {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_relative_path(field: &'static str, value: &str) -> Result<(), ManifestError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManifestError::UnsafeRelativePath {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
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
    #[error(
        "plugin manifest schema version {requested} is unsupported; this host supports {supported}"
    )]
    UnsupportedManifestSchemaVersion { supported: u16, requested: u16 },

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

    #[error("chat subscriptions require the local_chat_read capability")]
    ChatSubscriptionWithoutCapability,

    #[error("native developer plugins must request unsafe_native_execution")]
    NativeRuntimeWithoutCapability,

    #[error("unsafe_native_execution is only valid for a native developer plugin")]
    NativeCapabilityOnSandboxedRuntime,

    #[error("data-only plug-ins cannot declare an executable entrypoint")]
    DataOnlyEntrypoint,

    #[error("executable plug-ins must declare an entrypoint")]
    MissingEntrypoint,

    #[error("unsafe relative package path in {field}: {value}")]
    UnsafeRelativePath { field: &'static str, value: String },

    #[error("invalid {field} slug: {value}")]
    InvalidSlug { field: &'static str, value: String },

    #[error("a plug-in cannot depend on itself")]
    SelfDependency,

    #[error("duplicate dependency on {plugin_id}")]
    DuplicateDependency { plugin_id: String },

    #[error("duplicate exported resource {resource}")]
    DuplicateResourceExport { resource: String },

    #[error("resource {resource} has schema version zero")]
    ZeroResourceSchemaVersion { resource: String },

    #[error("resource imports require shared_resources_read")]
    ResourceImportsWithoutCapability,

    #[error("duplicate import {owner_plugin_id}:{resource}")]
    DuplicateResourceImport {
        owner_plugin_id: String,
        resource: String,
    },

    #[error("import {owner_plugin_id}:{resource} has minimum schema version zero")]
    ZeroImportedResourceSchemaVersion {
        owner_plugin_id: String,
        resource: String,
    },

    #[error("plug-in declares more than one hook for {stage:?}")]
    DuplicateHookStage { stage: OperationStage },

    #[error("localization hooks require localization_transform")]
    LocalizationHookWithoutCapability,

    #[error("submission hooks require submission_transform")]
    SubmissionHookWithoutCapability,

    #[error("a plug-in hook cannot order itself before or after itself")]
    SelfHookOrdering,

    #[error("hook refers to missing exported resource {resource}")]
    UnknownHookResource { resource: String },

    #[error("data-only plug-ins cannot invoke entrypoint hooks")]
    DataOnlyEntrypointHook,
}

#[derive(Debug, Error)]
pub enum PluginManifestLoadError {
    #[error("plugin.toml is not UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("plugin.toml is invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("plugin.toml failed validation: {0}")]
    Manifest(#[from] ManifestError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(runtime: PluginRuntime) -> PluginManifest {
        PluginManifest {
            schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: "app.rlogs.example".into(),
            name: "Example".into(),
            version: "0.1.0".into(),
            api_version: PLUGIN_API_VERSION,
            runtime,
            entrypoint: match runtime {
                PluginRuntime::DataOnly => None,
                PluginRuntime::WasmComponent => Some("bin/plugin.wasm".into()),
                PluginRuntime::BrowserOverlay => Some("web/index.html".into()),
                PluginRuntime::ExternalProcess => Some("bin/plugin".into()),
                PluginRuntime::NativeDeveloper => Some("bin/plugin-native".into()),
            },
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::ScopedStorage,
            ]),
            subscriptions: BTreeSet::from([EventTopic::Combat, EventTopic::Encounter]),
            allowed_network_domains: vec![],
            dependencies: Vec::new(),
            resource_exports: Vec::new(),
            resource_imports: Vec::new(),
            hooks: Vec::new(),
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

    #[test]
    fn chat_events_require_a_separate_local_capability() {
        let mut plugin = manifest(PluginRuntime::WasmComponent);
        plugin.subscriptions.insert(EventTopic::Chat);

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::ChatSubscriptionWithoutCapability)
        );
        plugin.capabilities.insert(PluginCapability::LocalChatRead);
        assert_eq!(plugin.validate(), Ok(()));
    }

    #[test]
    fn a_data_only_locale_alias_is_a_complete_package_manifest() {
        let source = br#"
schema_version = 1
id = "dev.example.bpsr-aliases"
name = "Example BPSR aliases"
version = "0.1.0"
api_version = 1
runtime = "data_only"
capabilities = ["shared_resources_read", "localization_transform"]
subscriptions = []
allowed_network_domains = []

[[resource_imports]]
owner_plugin_id = "app.rlogs.game.blue-protocol-star-resonance"
name = "catalog"
schema_id = "app.rlogs.bpsr.game-data"
minimum_schema_version = 2
required = true

[[resource_exports]]
name = "aliases"
kind = "localization-aliases"
path = "resources/aliases.toml"
schema_id = "dev.example.bpsr-aliases.locale-aliases"
schema_version = 1

[[hooks]]
stage = "localization_lookup"
phase = "after_core"
priority = 100
before = []
after = []

[hooks.handler]
kind = "data_resource"
resource = "aliases"
"#;
        let manifest = PluginManifest::from_toml(source).unwrap();

        assert_eq!(manifest.runtime, PluginRuntime::DataOnly);
        assert_eq!(manifest.resource_exports.len(), 1);
        assert_eq!(manifest.hooks[0].phase, HookPhase::AfterCore);
    }

    #[test]
    fn package_paths_cannot_escape_the_plugin_folder() {
        let mut plugin = manifest(PluginRuntime::WasmComponent);
        plugin.entrypoint = Some("../outside.wasm".into());

        assert!(matches!(
            plugin.validate(),
            Err(ManifestError::UnsafeRelativePath { .. })
        ));
    }
}
