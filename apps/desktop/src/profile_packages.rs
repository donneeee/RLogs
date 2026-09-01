use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rlogs_profiles::LocalProfilePackage;
use serde::{Deserialize, Serialize};

pub const PROFILE_PACKAGE_STORE_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_PROFILE_PACKAGE_BYTES: u64 = 9 * 1024 * 1024;
const MAXIMUM_PROFILE_PACKAGES: usize = 512;
const MAXIMUM_DISCOVERED_ENTRIES: usize = 4_096;
const MAXIMUM_COMPONENT_BYTES: usize = 128;
const PROFILE_PACKAGE_DIRECTORY_DEPTH: usize = 5;
const PROFILE_PUBLICATION_LEDGER_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_PROFILE_PUBLICATION_LEDGER_BYTES: u64 = 512 * 1024;
const MAXIMUM_PROFILE_PUBLICATION_RECORDS: usize = 512;

#[derive(Clone, Debug, Serialize)]
pub struct ProfilePackageStoreView {
    pub schema_version: u16,
    pub package_root: String,
    pub entry_count: usize,
    pub total_package_bytes: u64,
    pub entries: Vec<ProfilePackageView>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfilePackageView {
    pub package_id: String,
    pub created_unix_millis: u64,
    pub local_package_path: String,
    pub package_byte_length: u64,
    pub game_plugin_id: String,
    pub deployment: String,
    pub region: String,
    pub realm: Option<String>,
    pub world: Option<String>,
    pub character_id: String,
    pub display_name: Option<String>,
    pub server_id: Option<String>,
    pub class_id: Option<i64>,
    pub specialization_id: Option<i64>,
    pub level: Option<u64>,
    pub profile_field_count: usize,
    pub source_session_id: String,
    pub source_client_build: String,
    pub source_observation_count: u64,
    pub source_last_event_sequence: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfilePackageInspection {
    pub schema_version: u16,
    pub local_package_path: String,
    pub package_byte_length: u64,
    pub package: LocalProfilePackage,
}

#[derive(Debug)]
pub struct LocalProfilePackageStore {
    root: PathBuf,
    entries: Vec<StoredProfilePackage>,
    issues: Vec<String>,
}

#[derive(Clone, Debug)]
struct StoredProfilePackage {
    path: PathBuf,
    byte_length: u64,
    package: LocalProfilePackage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePublicationRecord {
    pub package_id: String,
    pub profile_id: String,
    pub character_id: String,
    pub published_unix_millis: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProfilePublicationLedger {
    schema_version: u16,
    records: Vec<ProfilePublicationRecord>,
}

#[derive(Debug)]
pub struct ProfilePublicationLedger {
    path: PathBuf,
    records: BTreeMap<String, ProfilePublicationRecord>,
}

impl ProfilePublicationLedger {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or_else(|| "profile publication ledger path has no parent".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create profile publication folder: {error}"))?;
        let stored = match std::fs::metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > MAXIMUM_PROFILE_PUBLICATION_LEDGER_BYTES
                {
                    return Err("profile publication ledger is not a bounded regular file".into());
                }
                let bytes = std::fs::read(&path).map_err(|error| {
                    format!("could not read profile publication ledger: {error}")
                })?;
                Some(
                    serde_json::from_slice::<StoredProfilePublicationLedger>(&bytes).map_err(
                        |error| format!("profile publication ledger JSON is invalid: {error}"),
                    )?,
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "could not inspect profile publication ledger: {error}"
                ));
            }
        };
        let mut records = BTreeMap::new();
        if let Some(stored) = stored {
            if stored.schema_version != PROFILE_PUBLICATION_LEDGER_SCHEMA_VERSION {
                return Err(format!(
                    "profile publication ledger uses unsupported schema {}; expected {PROFILE_PUBLICATION_LEDGER_SCHEMA_VERSION}",
                    stored.schema_version
                ));
            }
            if stored.records.len() > MAXIMUM_PROFILE_PUBLICATION_RECORDS {
                return Err(format!(
                    "profile publication ledger exceeds its {MAXIMUM_PROFILE_PUBLICATION_RECORDS}-record safety limit"
                ));
            }
            for record in stored.records {
                validate_publication_record(&record)?;
                if records.insert(record.package_id.clone(), record).is_some() {
                    return Err("profile publication ledger contains a duplicate package ID".into());
                }
            }
        }
        Ok(Self { path, records })
    }

    #[cfg(test)]
    pub fn is_published(&self, package_id: &str) -> bool {
        self.records.contains_key(package_id)
    }

    /// Returns true only when the recorded publication happened after the
    /// current local observation package was created. A stable profile body
    /// intentionally keeps the same package ID, but a later live session must
    /// still be allowed to refresh the website's "last seen" timestamp.
    pub fn covers_observation(&self, package_id: &str, created_unix_millis: u64) -> bool {
        self.records
            .get(package_id)
            .is_some_and(|record| record.published_unix_millis >= created_unix_millis)
    }

    pub fn latest_for_character(&self, character_id: &str) -> Option<&ProfilePublicationRecord> {
        self.records
            .values()
            .filter(|record| record.character_id == character_id)
            .max_by_key(|record| record.published_unix_millis)
    }

    pub fn reconcile(&mut self, active_package_ids: &BTreeSet<String>) -> Result<(), String> {
        let mut candidate = self.records.clone();
        candidate.retain(|package_id, _| active_package_ids.contains(package_id));
        if candidate == self.records {
            return Ok(());
        }
        self.persist(&candidate)?;
        self.records = candidate;
        Ok(())
    }

    pub fn record(
        &mut self,
        record: ProfilePublicationRecord,
        active_package_ids: &BTreeSet<String>,
    ) -> Result<(), String> {
        validate_publication_record(&record)?;
        if !active_package_ids.contains(&record.package_id) {
            return Err("published profile package is no longer current locally".into());
        }
        let mut candidate = self.records.clone();
        candidate.retain(|package_id, _| active_package_ids.contains(package_id));
        candidate.insert(record.package_id.clone(), record);
        if candidate.len() > MAXIMUM_PROFILE_PUBLICATION_RECORDS {
            return Err(format!(
                "profile publication ledger exceeds its {MAXIMUM_PROFILE_PUBLICATION_RECORDS}-record safety limit"
            ));
        }
        self.persist(&candidate)?;
        self.records = candidate;
        Ok(())
    }

    fn persist(&self, records: &BTreeMap<String, ProfilePublicationRecord>) -> Result<(), String> {
        let stored = StoredProfilePublicationLedger {
            schema_version: PROFILE_PUBLICATION_LEDGER_SCHEMA_VERSION,
            records: records.values().cloned().collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| format!("could not encode profile publication ledger: {error}"))?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAXIMUM_PROFILE_PUBLICATION_LEDGER_BYTES {
            return Err(format!(
                "profile publication ledger exceeds its {MAXIMUM_PROFILE_PUBLICATION_LEDGER_BYTES}-byte safety limit"
            ));
        }
        atomic_write(&self.path, &bytes)
            .map_err(|error| format!("could not persist profile publication ledger: {error}"))
    }
}

fn validate_publication_record(record: &ProfilePublicationRecord) -> Result<(), String> {
    if !is_sha256(&record.package_id) {
        return Err("profile publication record package ID must be a lowercase SHA-256".into());
    }
    if !record.profile_id.starts_with("prf_")
        || record.profile_id.len() != 36
        || !record.profile_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("profile publication record profile ID is invalid".into());
    }
    if record.character_id.is_empty()
        || record.character_id.len() > MAXIMUM_COMPONENT_BYTES
        || record.character_id.contains('\0')
    {
        return Err("profile publication record character ID is invalid".into());
    }
    if record.published_unix_millis == 0 {
        return Err("profile publication record timestamp must be positive".into());
    }
    Ok(())
}

impl LocalProfilePackageStore {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("could not create profile package folder: {error}"))?;
        let mut value = Self {
            root,
            entries: Vec::new(),
            issues: Vec::new(),
        };
        value.reload()?;
        Ok(value)
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let mut files = Vec::new();
        let mut discovered = 0_usize;
        collect_package_files(&self.root, 0, &mut discovered, &mut files)?;
        files.sort();
        self.entries.clear();
        self.issues.clear();
        for path in files {
            if self.entries.len() >= MAXIMUM_PROFILE_PACKAGES {
                self.issues.push(format!(
                    "Profile package limit {MAXIMUM_PROFILE_PACKAGES} reached; remaining files were ignored."
                ));
                break;
            }
            match load_package(&self.root, &path) {
                Ok(entry) => self.entries.push(entry),
                Err(error) => self.issues.push(format!("{}: {error}", path.display())),
            }
        }
        sort_entries(&mut self.entries);
        Ok(())
    }

    pub fn upsert(&mut self, package: LocalProfilePackage) -> Result<ProfilePackageView, String> {
        package
            .validate()
            .map_err(|error| format!("profile package validation failed: {error}"))?;
        let path = package_path(&self.root, &package)?;
        let parent = path
            .parent()
            .ok_or_else(|| "profile package path has no parent".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create profile identity folder: {error}"))?;
        let mut bytes = serde_json::to_vec_pretty(&package)
            .map_err(|error| format!("could not encode profile package: {error}"))?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAXIMUM_PROFILE_PACKAGE_BYTES {
            return Err(format!(
                "profile package is {} bytes; maximum is {MAXIMUM_PROFILE_PACKAGE_BYTES}",
                bytes.len()
            ));
        }
        atomic_write(&path, &bytes)?;
        let stored = StoredProfilePackage {
            path: path.clone(),
            byte_length: bytes.len() as u64,
            package,
        };
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.path == path) {
            *existing = stored;
        } else {
            if self.entries.len() >= MAXIMUM_PROFILE_PACKAGES {
                return Err(format!(
                    "profile package store reached its {MAXIMUM_PROFILE_PACKAGES}-package limit"
                ));
            }
            self.entries.push(stored);
        }
        sort_entries(&mut self.entries);
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .map(profile_view)
            .ok_or_else(|| "stored profile package disappeared".to_owned())
    }

    pub fn snapshot(&self) -> ProfilePackageStoreView {
        ProfilePackageStoreView {
            schema_version: PROFILE_PACKAGE_STORE_SCHEMA_VERSION,
            package_root: self.root.display().to_string(),
            entry_count: self.entries.len(),
            total_package_bytes: self.entries.iter().map(|entry| entry.byte_length).sum(),
            entries: self.entries.iter().map(profile_view).collect(),
            issues: self.issues.clone(),
        }
    }

    pub fn inspect(&self, package_id: &str) -> Result<ProfilePackageInspection, String> {
        if !is_sha256(package_id) {
            return Err("profile package ID must be a lowercase SHA-256".into());
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.package.package_id == package_id)
            .ok_or_else(|| format!("profile package {package_id} was not found"))?;
        let reloaded = load_package(&self.root, &entry.path)?;
        Ok(ProfilePackageInspection {
            schema_version: PROFILE_PACKAGE_STORE_SCHEMA_VERSION,
            local_package_path: reloaded.path.display().to_string(),
            package_byte_length: reloaded.byte_length,
            package: reloaded.package,
        })
    }
}

fn collect_package_files(
    directory: &Path,
    depth: usize,
    discovered: &mut usize,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        format!(
            "could not read profile package folder {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        *discovered = discovered
            .checked_add(1)
            .ok_or_else(|| "profile package entry count overflowed".to_owned())?;
        if *discovered > MAXIMUM_DISCOVERED_ENTRIES {
            return Err(format!(
                "profile package tree exceeds {MAXIMUM_DISCOVERED_ENTRIES} entries"
            ));
        }
        let entry =
            entry.map_err(|error| format!("could not read profile package entry: {error}"))?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "could not inspect profile package entry {}: {error}",
                entry.path().display()
            )
        })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if depth < PROFILE_PACKAGE_DIRECTORY_DEPTH {
                collect_package_files(&entry.path(), depth + 1, discovered, output)?;
            }
        } else if file_type.is_file() && entry.file_name() == "current.profile.json" {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn load_package(root: &Path, path: &Path) -> Result<StoredProfilePackage, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("could not inspect package: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_PROFILE_PACKAGE_BYTES {
        return Err("package is not a bounded regular file".into());
    }
    let mut file = File::open(path).map_err(|error| format!("could not open package: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("could not read package: {error}"))?;
    let package: LocalProfilePackage = serde_json::from_slice(&bytes)
        .map_err(|error| format!("package JSON is invalid: {error}"))?;
    package
        .validate()
        .map_err(|error| format!("package contract is invalid: {error}"))?;
    let expected = package_path(root, &package)?;
    if expected != path {
        return Err(format!("package routing expects {}", expected.display()));
    }
    Ok(StoredProfilePackage {
        path: path.to_owned(),
        byte_length: metadata.len(),
        package,
    })
}

fn package_path(root: &Path, package: &LocalProfilePackage) -> Result<PathBuf, String> {
    let routing = &package.request.payload.routing;
    let game = safe_component("game_plugin_id", &package.request.payload.game_plugin_id)?;
    let deployment = safe_component("deployment", routing_value(routing, "deployment")?)?;
    let region = safe_component("region", routing_value(routing, "region")?)?;
    let server = if let Some(realm) = routing.get("realm") {
        format!("realm-{}", safe_component("realm", realm)?)
    } else if let Some(world) = routing.get("world") {
        format!("world-{}", safe_component("world", world)?)
    } else {
        "server-unresolved".into()
    };
    let character = safe_component("character-id", routing_value(routing, "character-id")?)?;
    Ok(root
        .join(game)
        .join(deployment)
        .join(region)
        .join(server)
        .join(character)
        .join("current.profile.json"))
}

fn routing_value<'a>(
    routing: &'a std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, String> {
    routing
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("profile package is missing routing field {key}"))
}

fn safe_component(field: &str, value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAXIMUM_COMPONENT_BYTES
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "profile package {field} is not safe for a local folder"
        ));
    }
    Ok(value.to_owned())
}

fn profile_view(entry: &StoredProfilePackage) -> ProfilePackageView {
    let payload = &entry.package.request.payload;
    let routing = &payload.routing;
    let body = payload.body.as_object();
    ProfilePackageView {
        package_id: entry.package.package_id.clone(),
        created_unix_millis: entry.package.created_unix_millis,
        local_package_path: entry.path.display().to_string(),
        package_byte_length: entry.byte_length,
        game_plugin_id: payload.game_plugin_id.clone(),
        deployment: routing.get("deployment").cloned().unwrap_or_default(),
        region: routing.get("region").cloned().unwrap_or_default(),
        realm: routing.get("realm").cloned(),
        world: routing.get("world").cloned(),
        character_id: routing.get("character-id").cloned().unwrap_or_default(),
        display_name: body
            .and_then(|body| body.get("display_name"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        server_id: body
            .and_then(|body| body.get("server_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        class_id: body
            .and_then(|body| body.get("class_id"))
            .and_then(serde_json::Value::as_i64),
        specialization_id: body
            .and_then(|body| body.get("specialization_id"))
            .and_then(serde_json::Value::as_i64),
        level: body
            .and_then(|body| body.get("level"))
            .and_then(serde_json::Value::as_u64),
        profile_field_count: body.map_or(0, |body| {
            body.values().filter(|value| !value.is_null()).count()
        }),
        source_session_id: entry.package.source.session_id.clone(),
        source_client_build: entry.package.source.client_build.clone(),
        source_observation_count: entry.package.source.observation_count,
        source_last_event_sequence: entry.package.source.last_event_sequence,
    }
}

fn sort_entries(entries: &mut [StoredProfilePackage]) {
    entries.sort_by(|left, right| {
        right
            .package
            .created_unix_millis
            .cmp(&left.package.created_unix_millis)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("json.partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not replace interrupted profile package partial: {error}"
            ));
        }
    }
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)
        .map_err(|error| format!("could not create profile package partial: {error}"))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(bytes)
        .and_then(|_| writer.flush())
        .and_then(|_| writer.get_ref().sync_all())
        .map_err(|error| format!("could not sync profile package partial: {error}"))?;
    drop(writer);
    if let Err(error) = atomic_replace(&partial, path) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    sync_parent_directory(path)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(format!(
            "could not atomically replace profile package: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination)
        .map_err(|error| format!("could not atomically replace profile package: {error}"))
}

#[cfg(windows)]
fn sync_parent_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "profile package path has no parent".to_owned())?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync profile package folder: {error}"))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rlogs_profiles::{LocalProfilePackage, ProfilePackageSource};
    use rlogs_submission::{WebsitePayloadEnvelope, WebsitePayloadRequest};
    use serde_json::json;

    use super::*;

    fn temporary_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rlogs-profile-package-store-{}-{nonce}",
            std::process::id()
        ))
    }

    fn package(level: u64) -> LocalProfilePackage {
        let request = WebsitePayloadRequest::new(
            "/v1/games/blue-protocol-star-resonance/profiles",
            WebsitePayloadEnvelope::new(
                "app.rlogs.game.blue-protocol-star-resonance",
                "character-profile",
                "app.rlogs.bpsr.character-profile",
                1,
                BTreeMap::from([
                    ("deployment".into(), "global".into()),
                    ("region".into(), "north-america".into()),
                    ("realm".into(), "asteria".into()),
                    ("character-id".into(), "123456".into()),
                ]),
                json!({
                    "display_name": "MarieRose",
                    "level": level,
                    "class_id": 5
                }),
            )
            .unwrap(),
        )
        .unwrap();
        LocalProfilePackage::new(
            level,
            ProfilePackageSource {
                session_id: format!("session-{level}"),
                client_build: "steam-24252055".into(),
                protocol_pack_digest: "sha256:pack".into(),
                canonical_content_sha256: format!("sha256:{}", "a".repeat(64)),
                observation_count: 2,
                last_event_sequence: 10,
                live_capture: None,
            },
            request,
        )
        .unwrap()
    }

    #[test]
    fn current_package_is_atomically_replaced_in_human_readable_folders() {
        let root = temporary_root();
        let mut store = LocalProfilePackageStore::open(root.clone()).unwrap();
        let first = store.upsert(package(59)).unwrap();
        assert!(
            first
                .local_package_path
                .contains("app.rlogs.game.blue-protocol-star-resonance")
        );
        assert!(first.local_package_path.contains("asteria"));
        assert!(first.local_package_path.contains("123456"));
        let second = store.upsert(package(60)).unwrap();
        assert_ne!(first.package_id, second.package_id);
        assert_eq!(store.snapshot().entry_count, 1);
        assert_eq!(store.snapshot().entries[0].level, Some(60));

        let restored = LocalProfilePackageStore::open(root.clone()).unwrap();
        let inspection = restored.inspect(&second.package_id).unwrap();
        assert_eq!(inspection.package.request.payload.body["level"], 60);
        assert!(
            !serde_json::to_string(&inspection)
                .unwrap()
                .contains("password")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_files_are_reported_and_never_become_packages() {
        let root = temporary_root();
        let bad = root
            .join("bad")
            .join("global")
            .join("region")
            .join("server")
            .join("character");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("current.profile.json"), b"not json").unwrap();
        let store = LocalProfilePackageStore::open(root.clone()).unwrap();
        assert_eq!(store.snapshot().entry_count, 0);
        assert_eq!(store.snapshot().issues.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_ledger_persists_success_and_prunes_superseded_packages() {
        let root = temporary_root();
        let ledger_path = root.join("publications.v1.json");
        let first = package(59).package_id;
        let second = package(60).package_id;
        let mut ledger = ProfilePublicationLedger::open(ledger_path.clone()).unwrap();
        ledger
            .record(
                ProfilePublicationRecord {
                    package_id: first.clone(),
                    profile_id: format!("prf_{}", "b".repeat(32)),
                    character_id: "123456".into(),
                    published_unix_millis: 1,
                },
                &BTreeSet::from([first.clone(), second.clone()]),
            )
            .unwrap();
        assert!(ledger.is_published(&first));
        assert!(ledger.covers_observation(&first, 1));
        assert!(!ledger.covers_observation(&first, 2));
        assert!(!ledger.is_published(&second));

        let mut restored = ProfilePublicationLedger::open(ledger_path).unwrap();
        assert!(restored.is_published(&first));
        restored
            .reconcile(&BTreeSet::from([second.clone()]))
            .unwrap();
        assert!(!restored.is_published(&first));

        let restored = ProfilePublicationLedger::open(root.join("publications.v1.json")).unwrap();
        assert!(!restored.is_published(&first));
        std::fs::remove_dir_all(root).unwrap();
    }
}
