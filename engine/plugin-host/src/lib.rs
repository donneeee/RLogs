//! Folder discovery, shared resources, and deterministic operation ordering
//! for self-contained rLogs plug-in packages.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use rlogs_plugin_api::{
    HookPhase, OperationStage, PLUGIN_MANIFEST_FILE_NAME, PluginHook, PluginManifest,
    PluginManifestLoadError, ResourceStorage, SharedResourceExport, SharedResourceImport,
};
use thiserror::Error;

const CORE_HOOK_ANCHOR: &str = "\0rlogs-core";

#[derive(Debug, Clone)]
pub struct PluginPackage {
    root: PathBuf,
    folder_name: String,
    asset_root: PathBuf,
    shared_asset_root: PathBuf,
    manifest: PluginManifest,
}

impl PluginPackage {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn folder_name(&self) -> &str {
        &self.folder_name
    }

    /// Host-derived namespace for external assets. The manifest cannot choose
    /// or override this path.
    pub fn asset_root(&self) -> &Path {
        &self.asset_root
    }

    /// Provider-owned namespace for assets deliberately shared with other
    /// plug-ins through resource imports.
    pub fn shared_asset_root(&self) -> &Path {
        &self.shared_asset_root
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

#[derive(Debug)]
pub struct PluginDiscoveryIssue {
    pub package_path: PathBuf,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct PluginDiscoveryReport {
    pub packages: Vec<PluginPackage>,
    pub issues: Vec<PluginDiscoveryIssue>,
}

/// Discovers one directory package per child of `installed_root`.
///
/// Files such as `PUT_PLUGINS_HERE.md` are ignored. A malformed package is
/// reported and disabled without preventing independent valid packages from
/// loading.
pub fn discover_installed_plugins(
    installed_root: &Path,
) -> Result<PluginDiscoveryReport, PluginDiscoveryError> {
    let install_root = installed_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| PluginDiscoveryError::MissingInstallRoot {
            path: installed_root.to_owned(),
        })?;
    discover_plugin_packages(installed_root, install_root)
}

/// Discovers directory packages with an explicit rLogs install root. This is
/// useful for bundled packages whose source folders are not laid out exactly
/// like `plugins/installed/`.
pub fn discover_plugin_packages(
    packages_root: &Path,
    install_root: &Path,
) -> Result<PluginDiscoveryReport, PluginDiscoveryError> {
    let install_root =
        fs::canonicalize(install_root).map_err(|source| PluginDiscoveryError::ReadInstallRoot {
            path: install_root.to_owned(),
            source,
        })?;
    let root = fs::canonicalize(packages_root).map_err(|source| {
        PluginDiscoveryError::ReadInstalledRoot {
            path: packages_root.to_owned(),
            source,
        }
    })?;
    let mut entries = fs::read_dir(&root)
        .map_err(|source| PluginDiscoveryError::ReadInstalledRoot {
            path: root.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| PluginDiscoveryError::ReadInstalledRoot {
            path: root.clone(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    let mut report = PluginDiscoveryReport::default();
    let mut packages = BTreeMap::<String, PluginPackage>::new();
    let mut blocked_ids = BTreeSet::new();

    for entry in entries {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(source) => {
                report.issues.push(PluginDiscoveryIssue {
                    package_path: path,
                    detail: source.to_string(),
                });
                continue;
            }
        };
        if !file_type.is_dir() {
            continue;
        }
        if file_type.is_symlink() {
            report.issues.push(PluginDiscoveryIssue {
                package_path: path,
                detail: "plug-in package directories cannot be symbolic links".into(),
            });
            continue;
        }

        match load_package(&root, &install_root, &path) {
            Ok(package) => {
                let id = package.manifest.id.clone();
                if blocked_ids.contains(&id) {
                    report.issues.push(duplicate_issue(&path, &id));
                    continue;
                }
                if let Some(previous) = packages.remove(&id) {
                    report.issues.push(duplicate_issue(previous.root(), &id));
                    report.issues.push(duplicate_issue(&path, &id));
                    blocked_ids.insert(id);
                    continue;
                }
                packages.insert(id, package);
            }
            Err(error) => report.issues.push(PluginDiscoveryIssue {
                package_path: path,
                detail: error.to_string(),
            }),
        }
    }

    report.packages = packages.into_values().collect();
    Ok(report)
}

fn duplicate_issue(path: &Path, plugin_id: &str) -> PluginDiscoveryIssue {
    PluginDiscoveryIssue {
        package_path: path.to_owned(),
        detail: format!("duplicate plug-in ID {plugin_id}; every conflicting package is disabled"),
    }
}

fn load_package(
    packages_root: &Path,
    install_root: &Path,
    package_path: &Path,
) -> Result<PluginPackage, PluginPackageError> {
    let root = fs::canonicalize(package_path).map_err(|source| PluginPackageError::ReadPath {
        path: package_path.to_owned(),
        source,
    })?;
    if !root.starts_with(packages_root) {
        return Err(PluginPackageError::EscapesInstalledRoot { path: root });
    }

    let manifest_path = root.join(PLUGIN_MANIFEST_FILE_NAME);
    let bytes = fs::read(&manifest_path).map_err(|source| PluginPackageError::ReadPath {
        path: manifest_path,
        source,
    })?;
    let manifest = PluginManifest::from_toml(&bytes)?;
    let folder_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PluginPackageError::InvalidPackageFolderName { path: root.clone() })?
        .to_owned();
    let asset_root = install_root.join("assets").join(&folder_name);
    let shared_asset_root = install_root
        .join("assets")
        .join("shared")
        .join(&folder_name);

    if let Some(entrypoint) = &manifest.entrypoint {
        resolve_existing_package_path(&root, entrypoint)?;
    }
    for resource in &manifest.resource_exports {
        resolve_existing_export_path(&root, &asset_root, &shared_asset_root, resource)?;
    }

    Ok(PluginPackage {
        root,
        folder_name,
        asset_root,
        shared_asset_root,
        manifest,
    })
}

fn resolve_existing_export_path(
    package_root: &Path,
    asset_root: &Path,
    shared_asset_root: &Path,
    export: &SharedResourceExport,
) -> Result<PathBuf, PluginPackageError> {
    match export.storage {
        ResourceStorage::Package => resolve_existing_rooted_path(
            package_root,
            &export.path,
            PluginPackageError::ResourceEscapesPackage {
                relative: export.path.clone(),
            },
        ),
        ResourceStorage::PluginAssets => resolve_existing_rooted_path(
            asset_root,
            &export.path,
            PluginPackageError::ResourceEscapesAssetNamespace {
                relative: export.path.clone(),
            },
        ),
        ResourceStorage::SharedAssets => resolve_existing_rooted_path(
            shared_asset_root,
            &export.path,
            PluginPackageError::ResourceEscapesSharedAssetNamespace {
                relative: export.path.clone(),
            },
        ),
    }
}

fn resolve_existing_package_path(
    package_root: &Path,
    relative: &str,
) -> Result<PathBuf, PluginPackageError> {
    resolve_existing_rooted_path(
        package_root,
        relative,
        PluginPackageError::ResourceEscapesPackage {
            relative: relative.to_owned(),
        },
    )
}

fn resolve_existing_rooted_path(
    root: &Path,
    relative: &str,
    escape_error: PluginPackageError,
) -> Result<PathBuf, PluginPackageError> {
    validate_relative_path(relative)?;
    let canonical_root = fs::canonicalize(root).map_err(|source| PluginPackageError::ReadPath {
        path: root.to_owned(),
        source,
    })?;
    let joined = canonical_root.join(relative);
    let resolved = fs::canonicalize(&joined).map_err(|source| PluginPackageError::ReadPath {
        path: joined,
        source,
    })?;
    if !resolved.starts_with(&canonical_root) {
        return Err(escape_error);
    }
    Ok(resolved)
}

fn validate_relative_path(relative: &str) -> Result<(), PluginPackageError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PluginPackageError::UnsafeRelativePath {
            relative: relative.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PluginDiscoveryError {
    #[error("could not read installed plug-ins folder {path}: {source}")]
    ReadInstalledRoot {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not derive the rLogs install root from {path}")]
    MissingInstallRoot { path: PathBuf },

    #[error("could not read rLogs install root {path}: {source}")]
    ReadInstallRoot {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Error)]
pub enum PluginPackageError {
    #[error("could not read package path {path}: {source}")]
    ReadPath {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("package resolves outside the installed plug-ins folder: {path}")]
    EscapesInstalledRoot { path: PathBuf },

    #[error("plug-in package folder name is not valid UTF-8: {path}")]
    InvalidPackageFolderName { path: PathBuf },

    #[error("unsafe relative package path: {relative}")]
    UnsafeRelativePath { relative: String },

    #[error("package resource resolves outside its package: {relative}")]
    ResourceEscapesPackage { relative: String },

    #[error("asset resolves outside the plug-in asset namespace: {relative}")]
    ResourceEscapesAssetNamespace { relative: String },

    #[error("asset resolves outside the provider's shared asset namespace: {relative}")]
    ResourceEscapesSharedAssetNamespace { relative: String },

    #[error(transparent)]
    Manifest(#[from] PluginManifestLoadError),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SharedResourceId {
    pub owner_plugin_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SharedResourceHandle {
    id: SharedResourceId,
    kind: String,
    path: PathBuf,
    schema_id: String,
    schema_version: u16,
}

impl SharedResourceHandle {
    pub fn id(&self) -> &SharedResourceId {
        &self.id
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn schema_id(&self) -> &str {
        &self.schema_id
    }

    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Resolves a file for read access while retaining the export-root
    /// boundary. Sandboxed runtimes should receive bytes or handles from the
    /// host, not this host filesystem path.
    pub fn resolve_read_path(&self, relative: Option<&Path>) -> Result<PathBuf, ResourceError> {
        let requested = match relative {
            None => self.path.clone(),
            Some(relative) => {
                if relative.as_os_str().is_empty()
                    || relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| !matches!(component, Component::Normal(_)))
                {
                    return Err(ResourceError::UnsafeChildPath {
                        relative: relative.to_owned(),
                    });
                }
                self.path.join(relative)
            }
        };
        let resolved =
            fs::canonicalize(&requested).map_err(|source| ResourceError::ReadResource {
                path: requested,
                source,
            })?;
        if self.path.is_dir() {
            if !resolved.starts_with(&self.path) {
                return Err(ResourceError::EscapesResourceRoot { path: resolved });
            }
        } else if resolved != self.path {
            return Err(ResourceError::EscapesResourceRoot { path: resolved });
        }
        Ok(resolved)
    }
}

#[derive(Debug, Default)]
pub struct SharedResourceRegistry {
    resources: BTreeMap<SharedResourceId, SharedResourceHandle>,
}

impl SharedResourceRegistry {
    pub fn register_package(&mut self, package: &PluginPackage) -> Result<(), ResourceError> {
        self.register_exports_with_asset_roots(
            &package.manifest.id,
            &package.root,
            &package.asset_root,
            &package.shared_asset_root,
            &package.manifest.resource_exports,
        )
    }

    /// Registers resources from either an ordinary package or a trusted game
    /// plug-in. Files remain owned by their package and are never copied.
    pub fn register_exports(
        &mut self,
        owner_plugin_id: &str,
        package_root: &Path,
        exports: &[SharedResourceExport],
    ) -> Result<(), ResourceError> {
        if exports
            .iter()
            .any(|export| export.storage != ResourceStorage::Package)
        {
            return Err(ResourceError::MissingExternalAssetRoots {
                plugin_id: owner_plugin_id.to_owned(),
            });
        }
        self.register_exports_with_asset_roots(
            owner_plugin_id,
            package_root,
            package_root,
            package_root,
            exports,
        )
    }

    /// Registers exports using distinct package and external asset roots. The
    /// caller derives both asset namespaces; manifests only choose `storage`
    /// and a safe relative path.
    pub fn register_exports_with_asset_roots(
        &mut self,
        owner_plugin_id: &str,
        package_root: &Path,
        asset_root: &Path,
        shared_asset_root: &Path,
        exports: &[SharedResourceExport],
    ) -> Result<(), ResourceError> {
        let mut pending = Vec::with_capacity(exports.len());
        for export in exports {
            let path =
                resolve_existing_export_path(package_root, asset_root, shared_asset_root, export)
                    .map_err(ResourceError::InvalidExportPath)?;
            let id = SharedResourceId {
                owner_plugin_id: owner_plugin_id.to_owned(),
                name: export.name.clone(),
            };
            if self.resources.contains_key(&id) {
                return Err(ResourceError::DuplicateResource { id });
            }
            if pending
                .iter()
                .any(|(pending_id, _): &(SharedResourceId, SharedResourceHandle)| pending_id == &id)
            {
                return Err(ResourceError::DuplicateResource { id });
            }
            pending.push((
                id.clone(),
                SharedResourceHandle {
                    id,
                    kind: export.kind.clone(),
                    path,
                    schema_id: export.schema_id.clone(),
                    schema_version: export.schema_version,
                },
            ));
        }
        for (id, resource) in pending {
            self.resources.insert(id, resource);
        }
        Ok(())
    }

    pub fn get(&self, owner_plugin_id: &str, name: &str) -> Option<&SharedResourceHandle> {
        self.resources.get(&SharedResourceId {
            owner_plugin_id: owner_plugin_id.to_owned(),
            name: name.to_owned(),
        })
    }

    pub fn validate_imports(&self, package: &PluginPackage) -> Result<(), ResourceError> {
        for import in &package.manifest.resource_imports {
            let resource = self.get(&import.owner_plugin_id, &import.name);
            let Some(resource) = resource else {
                if import.required {
                    return Err(ResourceError::MissingRequiredImport {
                        plugin_id: package.manifest.id.clone(),
                        owner_plugin_id: import.owner_plugin_id.clone(),
                        resource: import.name.clone(),
                    });
                }
                continue;
            };
            validate_import_schema(package, import, resource)?;
        }
        Ok(())
    }
}

fn validate_import_schema(
    package: &PluginPackage,
    import: &SharedResourceImport,
    resource: &SharedResourceHandle,
) -> Result<(), ResourceError> {
    if let Some(expected) = &import.schema_id {
        if resource.schema_id != *expected {
            return Err(ResourceError::ImportSchemaMismatch {
                plugin_id: package.manifest.id.clone(),
                resource: import.name.clone(),
                expected: expected.clone(),
                actual: resource.schema_id.clone(),
            });
        }
    }
    if let Some(minimum) = import.minimum_schema_version {
        if resource.schema_version < minimum {
            return Err(ResourceError::ImportSchemaVersionTooOld {
                plugin_id: package.manifest.id.clone(),
                resource: import.name.clone(),
                minimum,
                actual: resource.schema_version,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("could not read shared resource path {path}: {source}")]
    ReadResource {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid exported resource path: {0}")]
    InvalidExportPath(PluginPackageError),

    #[error("{plugin_id} exports external assets but no host asset namespaces were supplied")]
    MissingExternalAssetRoots { plugin_id: String },

    #[error("duplicate shared resource {id:?}")]
    DuplicateResource { id: SharedResourceId },

    #[error("{plugin_id} requires missing shared resource {owner_plugin_id}:{resource}")]
    MissingRequiredImport {
        plugin_id: String,
        owner_plugin_id: String,
        resource: String,
    },

    #[error(
        "{plugin_id} expected schema {expected} for {resource}, but the owner publishes {actual}"
    )]
    ImportSchemaMismatch {
        plugin_id: String,
        resource: String,
        expected: String,
        actual: String,
    },

    #[error("{plugin_id} requires {resource} schema version >= {minimum}, but found {actual}")]
    ImportSchemaVersionTooOld {
        plugin_id: String,
        resource: String,
        minimum: u16,
        actual: u16,
    },

    #[error("unsafe child resource path: {relative}")]
    UnsafeChildPath { relative: PathBuf },

    #[error("resolved path escapes the shared resource root: {path}")]
    EscapesResourceRoot { path: PathBuf },
}

pub fn resolve_plugin_load_order(
    packages: &[PluginPackage],
) -> Result<Vec<String>, PluginOrderError> {
    let package_ids: BTreeSet<_> = packages
        .iter()
        .map(|package| package.manifest.id.as_str())
        .collect();
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    let mut indegree = BTreeMap::<String, usize>::new();
    for package in packages {
        edges.entry(package.manifest.id.clone()).or_default();
        indegree.entry(package.manifest.id.clone()).or_default();
    }
    for package in packages {
        for dependency in &package.manifest.dependencies {
            if !package_ids.contains(dependency.plugin_id.as_str()) {
                if dependency.optional {
                    continue;
                }
                return Err(PluginOrderError::MissingRequiredDependency {
                    plugin_id: package.manifest.id.clone(),
                    dependency: dependency.plugin_id.clone(),
                });
            }
            add_edge(
                &mut edges,
                &mut indegree,
                &dependency.plugin_id,
                &package.manifest.id,
            );
        }
    }
    topological_ids(edges, indegree, BTreeMap::new())
        .map_err(|cycle| PluginOrderError::DependencyCycle { plugin_ids: cycle })
}

#[derive(Debug, Clone)]
pub struct ResolvedHook {
    pub plugin_id: String,
    pub hook: PluginHook,
}

#[derive(Debug, Clone, Default)]
pub struct HookPlan {
    pub before_core: Vec<ResolvedHook>,
    pub after_core: Vec<ResolvedHook>,
}

pub fn resolve_hook_plan(
    packages: &[PluginPackage],
    stage: OperationStage,
) -> Result<HookPlan, PluginOrderError> {
    let hooks: BTreeMap<_, _> = packages
        .iter()
        .filter_map(|package| {
            package
                .manifest
                .hooks
                .iter()
                .find(|hook| hook.stage == stage)
                .map(|hook| (package.manifest.id.clone(), (package, hook)))
        })
        .collect();
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    let mut indegree = BTreeMap::<String, usize>::new();
    let mut priorities = BTreeMap::<String, i32>::new();
    edges.entry(CORE_HOOK_ANCHOR.into()).or_default();
    indegree.entry(CORE_HOOK_ANCHOR.into()).or_default();
    priorities.insert(CORE_HOOK_ANCHOR.into(), 0);

    for (plugin_id, (_, hook)) in &hooks {
        edges.entry(plugin_id.clone()).or_default();
        indegree.entry(plugin_id.clone()).or_default();
        priorities.insert(plugin_id.clone(), hook.priority);
        match hook.phase {
            HookPhase::BeforeCore => {
                add_edge(&mut edges, &mut indegree, plugin_id, CORE_HOOK_ANCHOR);
            }
            HookPhase::AfterCore => {
                add_edge(&mut edges, &mut indegree, CORE_HOOK_ANCHOR, plugin_id);
            }
        }
    }

    for (plugin_id, (package, hook)) in &hooks {
        for dependency in &package.manifest.dependencies {
            if hooks.contains_key(&dependency.plugin_id) {
                add_edge(&mut edges, &mut indegree, &dependency.plugin_id, plugin_id);
            }
        }
        for target in &hook.before {
            if hooks.contains_key(target) {
                add_edge(&mut edges, &mut indegree, plugin_id, target);
            }
        }
        for target in &hook.after {
            if hooks.contains_key(target) {
                add_edge(&mut edges, &mut indegree, target, plugin_id);
            }
        }
    }

    let order = topological_ids(edges, indegree, priorities)
        .map_err(|plugin_ids| PluginOrderError::HookCycle { stage, plugin_ids })?;
    let mut plan = HookPlan::default();
    for plugin_id in order {
        if plugin_id == CORE_HOOK_ANCHOR {
            continue;
        }
        let (_, hook) = hooks
            .get(&plugin_id)
            .expect("topological output contains only registered nodes");
        let resolved = ResolvedHook {
            plugin_id,
            hook: (*hook).clone(),
        };
        match hook.phase {
            HookPhase::BeforeCore => plan.before_core.push(resolved),
            HookPhase::AfterCore => plan.after_core.push(resolved),
        }
    }
    Ok(plan)
}

fn add_edge(
    edges: &mut BTreeMap<String, BTreeSet<String>>,
    indegree: &mut BTreeMap<String, usize>,
    from: &str,
    to: &str,
) {
    if edges
        .entry(from.to_owned())
        .or_default()
        .insert(to.to_owned())
    {
        *indegree.entry(to.to_owned()).or_default() += 1;
    }
}

fn topological_ids(
    edges: BTreeMap<String, BTreeSet<String>>,
    mut indegree: BTreeMap<String, usize>,
    priorities: BTreeMap<String, i32>,
) -> Result<Vec<String>, Vec<String>> {
    let mut ready = BTreeSet::<(i32, String)>::new();
    for (id, count) in &indegree {
        if *count == 0 {
            ready.insert((*priorities.get(id).unwrap_or(&0), id.clone()));
        }
    }
    let mut order = Vec::with_capacity(indegree.len());
    while let Some((_, id)) = ready.pop_first() {
        order.push(id.clone());
        if let Some(targets) = edges.get(&id) {
            for target in targets {
                let count = indegree
                    .get_mut(target)
                    .expect("every edge target is registered");
                *count -= 1;
                if *count == 0 {
                    ready.insert((*priorities.get(target).unwrap_or(&0), target.clone()));
                }
            }
        }
    }
    if order.len() == indegree.len() {
        Ok(order)
    } else {
        Err(indegree
            .into_iter()
            .filter_map(|(id, count)| (count != 0).then_some(id))
            .collect())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginOrderError {
    #[error("{plugin_id} requires missing plug-in {dependency}")]
    MissingRequiredDependency {
        plugin_id: String,
        dependency: String,
    },

    #[error("plug-in dependency cycle: {plugin_ids:?}")]
    DependencyCycle { plugin_ids: Vec<String> },

    #[error("{stage:?} hook ordering cycle: {plugin_ids:?}")]
    HookCycle {
        stage: OperationStage,
        plugin_ids: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use rlogs_plugin_api::{ResourceStorage, SharedResourceExport};

    use super::*;

    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples")
    }

    fn builtin_localization_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/builtin/localization")
    }

    fn temporary_install_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rlogs-plugin-host-{}-{suffix}", std::process::id()))
    }

    #[test]
    fn discovers_a_self_contained_data_plugin_folder() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.packages.len(), 1);
        let package = &report.packages[0];
        assert_eq!(package.manifest.id, "dev.rlogs.example.bpsr-uid-aliases");

        let mut resources = SharedResourceRegistry::default();
        resources.register_package(package).unwrap();
        let aliases = resources
            .get(&package.manifest.id, "aliases")
            .expect("alias export");
        assert!(
            aliases
                .resolve_read_path(None)
                .unwrap()
                .ends_with("aliases.toml")
        );
    }

    #[test]
    fn bundled_localization_uses_the_same_folder_manifest_contract() {
        let report = discover_installed_plugins(&builtin_localization_root()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.packages.len(), 1);
        assert_eq!(
            report.packages[0].manifest.id,
            "app.rlogs.localization.en-us"
        );
    }

    #[test]
    fn locale_alias_runs_after_canonical_localization() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        let plan = resolve_hook_plan(&report.packages, OperationStage::LocalizationLookup).unwrap();
        assert!(plan.before_core.is_empty());
        assert_eq!(plan.after_core.len(), 1);
        assert_eq!(
            plan.after_core[0].plugin_id,
            "dev.rlogs.example.bpsr-uid-aliases"
        );
    }

    #[test]
    fn load_order_is_stable_for_the_example_package() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        assert_eq!(
            resolve_plugin_load_order(&report.packages).unwrap(),
            vec!["dev.rlogs.example.bpsr-uid-aliases"]
        );
    }

    #[test]
    fn imports_the_owner_catalog_without_copying_it_into_the_alias_plugin() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        let package = &report.packages[0];
        let mut resources = SharedResourceRegistry::default();
        resources
            .register_exports(
                "app.rlogs.game.blue-protocol-star-resonance",
                package.root(),
                &[SharedResourceExport {
                    name: "catalog".into(),
                    kind: "game-data-catalog".into(),
                    storage: ResourceStorage::Package,
                    path: "resources".into(),
                    schema_id: "app.rlogs.bpsr.game-data".into(),
                    schema_version: 2,
                }],
            )
            .unwrap();
        resources.register_package(package).unwrap();

        resources.validate_imports(package).unwrap();
        assert!(
            package
                .root()
                .join("game-data")
                .try_exists()
                .is_ok_and(|exists| !exists)
        );
    }

    #[test]
    fn shared_assets_are_confined_to_the_provider_folder_namespace() {
        let install_root = temporary_install_root();
        let package_root = install_root.join("plugins/installed/example-assets");
        let shared_root = install_root.join("assets/shared/example-assets/icons");
        fs::create_dir_all(&package_root).unwrap();
        fs::create_dir_all(&shared_root).unwrap();
        fs::write(shared_root.join("sample.svg"), "<svg/>").unwrap();
        fs::write(
            package_root.join(PLUGIN_MANIFEST_FILE_NAME),
            br#"
schema_version = 1
id = "dev.rlogs.example.assets"
name = "Example assets"
version = "0.1.0"
api_version = 1
runtime = "data_only"
capabilities = []
subscriptions = []
allowed_network_domains = []

[[resource_exports]]
name = "icons"
kind = "game-assets"
storage = "shared_assets"
path = "icons"
schema_id = "dev.rlogs.example.icons"
schema_version = 1
"#,
        )
        .unwrap();

        let report = discover_installed_plugins(&install_root.join("plugins/installed")).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let package = &report.packages[0];
        let canonical_install_root = fs::canonicalize(&install_root).unwrap();
        let canonical_shared_root = fs::canonicalize(&shared_root).unwrap();
        assert_eq!(
            package.shared_asset_root(),
            canonical_install_root.join("assets/shared/example-assets")
        );
        let mut registry = SharedResourceRegistry::default();
        registry.register_package(package).unwrap();
        assert!(
            registry
                .get(&package.manifest().id, "icons")
                .unwrap()
                .resolve_read_path(Some(Path::new("sample.svg")))
                .unwrap()
                .starts_with(&canonical_shared_root)
        );

        fs::remove_dir_all(install_root).unwrap();
    }

    #[test]
    fn explicit_before_edges_override_priority_deterministically() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        let mut first = report.packages[0].clone();
        first.manifest.id = "dev.rlogs.example.first".into();
        first.manifest.hooks[0].priority = 100;
        first.manifest.hooks[0].before = vec!["dev.rlogs.example.second".into()];
        let mut second = report.packages[0].clone();
        second.manifest.id = "dev.rlogs.example.second".into();
        second.manifest.hooks[0].priority = -100;

        let plan = resolve_hook_plan(&[second, first], OperationStage::LocalizationLookup).unwrap();
        assert_eq!(
            plan.after_core
                .iter()
                .map(|hook| hook.plugin_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dev.rlogs.example.first", "dev.rlogs.example.second"]
        );
    }

    #[test]
    fn contradictory_hook_order_is_rejected_as_a_cycle() {
        let report = discover_installed_plugins(&example_root()).unwrap();
        let mut first = report.packages[0].clone();
        first.manifest.id = "dev.rlogs.example.first".into();
        first.manifest.hooks[0].after = vec!["dev.rlogs.example.second".into()];
        let mut second = report.packages[0].clone();
        second.manifest.id = "dev.rlogs.example.second".into();
        second.manifest.hooks[0].after = vec!["dev.rlogs.example.first".into()];

        assert!(matches!(
            resolve_hook_plan(&[first, second], OperationStage::LocalizationLookup),
            Err(PluginOrderError::HookCycle { .. })
        ));
    }
}
