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
    /// Publishes a host-rendered workspace with one or more packaged web
    /// surfaces. This does not grant access to any engine data by itself.
    UiWorkspacePublish,
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
pub enum PluginWorkspaceTabKind {
    #[default]
    Content,
    Options,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginWorkspaceTab {
    /// Stable, human-readable slug used for tab state and deep links.
    pub id: String,
    pub label: String,
    /// Browser surface relative to the plug-in package root.
    pub entrypoint: String,
    #[serde(default)]
    pub kind: PluginWorkspaceTabKind,
    /// Stable section slug. Tabs in the same section stay adjacent and can be
    /// moved as one block. Omitted tabs belong to the owner's `main` section.
    #[serde(default)]
    pub section: Option<String>,
    /// Keeps an unfinished surface out of the ordinary catalog. The desktop
    /// host must also reject direct surface requests unless Developer Mode is
    /// enabled.
    #[serde(default)]
    pub developer_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginWorkspace {
    /// Optional navigation icon relative to the plug-in package root. The host
    /// treats this as an image, never executable markup.
    pub icon: Option<String>,
    /// Bundled default only. A user's saved drag order always wins.
    #[serde(default)]
    pub default_order: i32,
    pub tabs: Vec<PluginWorkspaceTab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginWorkspaceTabContribution {
    /// Plug-in whose left-side workspace will host this tab.
    pub target_plugin_id: String,
    /// Stable only within the contributing plug-in. The host namespaces it by
    /// contributor ID so two add-ons cannot collide.
    pub id: String,
    pub label: String,
    /// Browser surface relative to the contributing plug-in's package root.
    pub entrypoint: String,
    #[serde(default)]
    pub kind: PluginWorkspaceTabKind,
    /// Stable section slug within the contributing plug-in. Contributions
    /// default to that plug-in's `main` section.
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub developer_only: bool,
    /// Owner tabs occupy their declared order starting at zero. Contributions
    /// default after them but can request a deliberate relative position.
    #[serde(default = "default_workspace_tab_contribution_order")]
    pub default_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSettingsTabContribution {
    /// Stable only within the contributing plug-in. The host namespaces it by
    /// contributor ID so independent packages cannot collide.
    pub id: String,
    pub label: String,
    /// Browser surface relative to the contributing plug-in's package root.
    pub entrypoint: String,
    /// Stable section slug within the contributing plug-in.
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub developer_only: bool,
    /// Settings is the only host-owned UI target. Plug-ins choose their own
    /// placement within it instead of being named by Core.
    #[serde(default = "default_workspace_tab_contribution_order")]
    pub default_order: i32,
}

fn default_workspace_tab_contribution_order() -> i32 {
    1_000
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStorage {
    /// The resource ships inside the plug-in package.
    #[default]
    Package,
    /// The resource lives under the host-derived
    /// `assets/rlogs/plugins/<plugin-folder-name>/` namespace.
    PluginAssets,
    /// A provider-owned resource intended for reuse by other plug-ins. It
    /// lives under `assets/rlogs/shared/<provider-plugin-folder-name>/`.
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
    /// Optional top-level desktop workspace. The selected plug-in owns these
    /// real tabs; the rLogs host owns only navigation and containment.
    #[serde(default)]
    pub workspace: Option<PluginWorkspace>,
    /// Additional tabs contributed to another plug-in's workspace. The target
    /// must also be declared as a dependency.
    #[serde(default)]
    pub workspace_tab_contributions: Vec<PluginWorkspaceTabContribution>,
    /// Tabs contributed to the host-owned Settings destination. Core knows the
    /// destination exists, but does not know which feature plug-ins will appear
    /// there.
    #[serde(default)]
    pub settings_tab_contributions: Vec<PluginSettingsTabContribution>,
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

        if let Some(workspace) = &self.workspace {
            if !self
                .capabilities
                .contains(&PluginCapability::UiWorkspacePublish)
            {
                return Err(ManifestError::WorkspaceWithoutCapability);
            }
            if self.runtime == PluginRuntime::DataOnly {
                return Err(ManifestError::DataOnlyWorkspace);
            }
            if let Some(icon) = &workspace.icon {
                validate_relative_path("workspace icon", icon)?;
            }
            if workspace.tabs.is_empty() {
                return Err(ManifestError::WorkspaceWithoutTabs);
            }
            if workspace.tabs.len() > 16 {
                return Err(ManifestError::TooManyWorkspaceTabs { maximum: 16 });
            }

            let mut tab_ids = BTreeSet::new();
            let mut has_options = false;
            for tab in &workspace.tabs {
                validate_slug("workspace tab id", &tab.id)?;
                if !tab_ids.insert(tab.id.as_str()) {
                    return Err(ManifestError::DuplicateWorkspaceTab {
                        tab: tab.id.clone(),
                    });
                }
                let label = tab.label.trim();
                if label.is_empty() {
                    return Err(ManifestError::EmptyWorkspaceTabLabel {
                        tab: tab.id.clone(),
                    });
                }
                if label.chars().count() > 48 {
                    return Err(ManifestError::WorkspaceTabLabelTooLong {
                        tab: tab.id.clone(),
                    });
                }
                validate_relative_path("workspace tab entrypoint", &tab.entrypoint)?;
                if let Some(section) = &tab.section {
                    validate_slug("workspace tab section", section)?;
                }
                if tab.kind == PluginWorkspaceTabKind::Options {
                    if has_options {
                        return Err(ManifestError::DuplicateOptionsTab);
                    }
                    has_options = true;
                }
            }
        }

        if !self.workspace_tab_contributions.is_empty() {
            if !self
                .capabilities
                .contains(&PluginCapability::UiWorkspacePublish)
            {
                return Err(ManifestError::WorkspaceWithoutCapability);
            }
            if self.runtime == PluginRuntime::DataOnly {
                return Err(ManifestError::DataOnlyWorkspace);
            }
        }
        if !self.settings_tab_contributions.is_empty() {
            if !self
                .capabilities
                .contains(&PluginCapability::UiWorkspacePublish)
            {
                return Err(ManifestError::WorkspaceWithoutCapability);
            }
            if self.runtime == PluginRuntime::DataOnly {
                return Err(ManifestError::DataOnlyWorkspace);
            }
        }
        let mut contributed_tabs = BTreeSet::new();
        for contribution in &self.workspace_tab_contributions {
            validate_id(&contribution.target_plugin_id)?;
            if contribution.target_plugin_id == self.id {
                return Err(ManifestError::SelfWorkspaceTabContribution);
            }
            validate_slug("workspace tab contribution id", &contribution.id)?;
            let label = contribution.label.trim();
            if label.is_empty() {
                return Err(ManifestError::EmptyWorkspaceTabLabel {
                    tab: contribution.id.clone(),
                });
            }
            if label.chars().count() > 48 {
                return Err(ManifestError::WorkspaceTabLabelTooLong {
                    tab: contribution.id.clone(),
                });
            }
            validate_relative_path(
                "workspace tab contribution entrypoint",
                &contribution.entrypoint,
            )?;
            if let Some(section) = &contribution.section {
                validate_slug("workspace tab contribution section", section)?;
            }
            if !contributed_tabs.insert((
                contribution.target_plugin_id.as_str(),
                contribution.id.as_str(),
            )) {
                return Err(ManifestError::DuplicateWorkspaceTabContribution {
                    target_plugin_id: contribution.target_plugin_id.clone(),
                    tab: contribution.id.clone(),
                });
            }
        }
        let mut contributed_settings_tabs = BTreeSet::new();
        for contribution in &self.settings_tab_contributions {
            validate_slug("settings tab contribution id", &contribution.id)?;
            let label = contribution.label.trim();
            if label.is_empty() {
                return Err(ManifestError::EmptyWorkspaceTabLabel {
                    tab: contribution.id.clone(),
                });
            }
            if label.chars().count() > 48 {
                return Err(ManifestError::WorkspaceTabLabelTooLong {
                    tab: contribution.id.clone(),
                });
            }
            validate_relative_path(
                "settings tab contribution entrypoint",
                &contribution.entrypoint,
            )?;
            if let Some(section) = &contribution.section {
                validate_slug("settings tab contribution section", section)?;
            }
            if !contributed_settings_tabs.insert(contribution.id.as_str()) {
                return Err(ManifestError::DuplicateSettingsTabContribution {
                    tab: contribution.id.clone(),
                });
            }
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
        for contribution in &self.workspace_tab_contributions {
            if !dependency_ids.contains(&contribution.target_plugin_id) {
                return Err(ManifestError::WorkspaceTabContributionWithoutDependency {
                    target_plugin_id: contribution.target_plugin_id.clone(),
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

    #[error("a workspace requires the ui_workspace_publish capability")]
    WorkspaceWithoutCapability,

    #[error("data-only plug-ins cannot publish interactive workspaces")]
    DataOnlyWorkspace,

    #[error("a plug-in workspace must declare at least one tab")]
    WorkspaceWithoutTabs,

    #[error("a plug-in workspace cannot declare more than {maximum} tabs")]
    TooManyWorkspaceTabs { maximum: usize },

    #[error("duplicate workspace tab {tab}")]
    DuplicateWorkspaceTab { tab: String },

    #[error("workspace tab {tab} has an empty label")]
    EmptyWorkspaceTabLabel { tab: String },

    #[error("workspace tab {tab} label exceeds 48 characters")]
    WorkspaceTabLabelTooLong { tab: String },

    #[error("a plug-in workspace cannot declare more than one options tab")]
    DuplicateOptionsTab,

    #[error("a plug-in should declare its own tabs in its workspace")]
    SelfWorkspaceTabContribution,

    #[error("duplicate tab contribution {tab} for {target_plugin_id}")]
    DuplicateWorkspaceTabContribution {
        target_plugin_id: String,
        tab: String,
    },

    #[error("duplicate Settings tab contribution {tab}")]
    DuplicateSettingsTabContribution { tab: String },

    #[error("a tab contribution to {target_plugin_id} requires a declared dependency")]
    WorkspaceTabContributionWithoutDependency { target_plugin_id: String },

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
            workspace: None,
            workspace_tab_contributions: Vec::new(),
            settings_tab_contributions: Vec::new(),
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

    #[test]
    fn a_workspace_publishes_real_packaged_tabs() {
        let mut plugin = manifest(PluginRuntime::WasmComponent);
        plugin
            .capabilities
            .insert(PluginCapability::UiWorkspacePublish);
        plugin.workspace = Some(PluginWorkspace {
            icon: Some("ui/profile.svg".into()),
            default_order: 20,
            tabs: vec![
                PluginWorkspaceTab {
                    id: "profile".into(),
                    label: "Profile".into(),
                    entrypoint: "ui/profile.html".into(),
                    kind: PluginWorkspaceTabKind::Content,
                    section: None,
                    developer_only: false,
                },
                PluginWorkspaceTab {
                    id: "options".into(),
                    label: "Options".into(),
                    entrypoint: "ui/options.html".into(),
                    kind: PluginWorkspaceTabKind::Options,
                    section: None,
                    developer_only: false,
                },
            ],
        });

        assert_eq!(plugin.validate(), Ok(()));
    }

    #[test]
    fn a_workspace_requires_explicit_ui_permission() {
        let mut plugin = manifest(PluginRuntime::BrowserOverlay);
        plugin.workspace = Some(PluginWorkspace {
            icon: None,
            default_order: 0,
            tabs: vec![PluginWorkspaceTab {
                id: "live".into(),
                label: "Live".into(),
                entrypoint: "web/live.html".into(),
                kind: PluginWorkspaceTabKind::Content,
                section: None,
                developer_only: false,
            }],
        });

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::WorkspaceWithoutCapability)
        );
    }

    #[test]
    fn workspace_tab_ids_are_unique_and_paths_stay_in_the_package() {
        let mut plugin = manifest(PluginRuntime::BrowserOverlay);
        plugin
            .capabilities
            .insert(PluginCapability::UiWorkspacePublish);
        plugin.workspace = Some(PluginWorkspace {
            icon: None,
            default_order: 0,
            tabs: vec![
                PluginWorkspaceTab {
                    id: "live".into(),
                    label: "Live".into(),
                    entrypoint: "web/live.html".into(),
                    kind: PluginWorkspaceTabKind::Content,
                    section: None,
                    developer_only: false,
                },
                PluginWorkspaceTab {
                    id: "live".into(),
                    label: "Options".into(),
                    entrypoint: "../options.html".into(),
                    kind: PluginWorkspaceTabKind::Options,
                    section: None,
                    developer_only: false,
                },
            ],
        });

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::DuplicateWorkspaceTab { tab: "live".into() })
        );

        plugin.workspace.as_mut().unwrap().tabs[1].id = "options".into();
        assert!(matches!(
            plugin.validate(),
            Err(ManifestError::UnsafeRelativePath {
                field: "workspace tab entrypoint",
                ..
            })
        ));
    }

    #[test]
    fn an_add_on_can_contribute_a_namespaced_tab_to_another_workspace() {
        let mut plugin = manifest(PluginRuntime::WasmComponent);
        plugin.id = "app.rlogs.module-optimizer".into();
        plugin
            .capabilities
            .insert(PluginCapability::UiWorkspacePublish);
        plugin.dependencies.push(PluginDependency {
            plugin_id: "app.rlogs.character-profiles".into(),
            optional: true,
        });
        plugin
            .workspace_tab_contributions
            .push(PluginWorkspaceTabContribution {
                target_plugin_id: "app.rlogs.character-profiles".into(),
                id: "modules".into(),
                label: "Modules".into(),
                entrypoint: "ui/profile-modules.html".into(),
                kind: PluginWorkspaceTabKind::Content,
                section: None,
                developer_only: false,
                default_order: 200,
            });

        assert_eq!(plugin.validate(), Ok(()));
    }

    #[test]
    fn a_tab_contribution_requires_an_explicit_dependency() {
        let mut plugin = manifest(PluginRuntime::BrowserOverlay);
        plugin
            .capabilities
            .insert(PluginCapability::UiWorkspacePublish);
        plugin
            .workspace_tab_contributions
            .push(PluginWorkspaceTabContribution {
                target_plugin_id: "app.rlogs.character-profiles".into(),
                id: "extra".into(),
                label: "Extra".into(),
                entrypoint: "web/extra.html".into(),
                kind: PluginWorkspaceTabKind::Content,
                section: None,
                developer_only: false,
                default_order: 1_000,
            });

        assert_eq!(
            plugin.validate(),
            Err(ManifestError::WorkspaceTabContributionWithoutDependency {
                target_plugin_id: "app.rlogs.character-profiles".into(),
            },)
        );
    }

    #[test]
    fn a_settings_tab_is_owned_and_ordered_by_its_plugin_manifest() {
        let mut plugin = manifest(PluginRuntime::BrowserOverlay);
        plugin.id = "app.rlogs.themes".into();
        plugin
            .capabilities
            .insert(PluginCapability::UiWorkspacePublish);
        plugin
            .settings_tab_contributions
            .push(PluginSettingsTabContribution {
                id: "appearance".into(),
                label: "Appearance".into(),
                entrypoint: "ui/settings.html".into(),
                section: Some("theme".into()),
                developer_only: false,
                default_order: 200,
            });

        assert_eq!(plugin.validate(), Ok(()));
        plugin
            .settings_tab_contributions
            .push(plugin.settings_tab_contributions[0].clone());
        assert_eq!(
            plugin.validate(),
            Err(ManifestError::DuplicateSettingsTabContribution {
                tab: "appearance".into(),
            })
        );
    }
}
