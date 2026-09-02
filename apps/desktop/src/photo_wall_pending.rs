use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use reqwest::Url;
use rlogs_game_bpsr::LocalPhotoAssetReference;
use serde::{Deserialize, Serialize};

const PHOTO_WALL_PENDING_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_PENDING_REFERENCES: usize = 256;
const MAXIMUM_PENDING_LEDGER_BYTES: u64 = 512 * 1024;
const MAXIMUM_PHOTO_WALL_IMAGE_BYTES: u32 = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct PhotoWallPendingStore {
    path: PathBuf,
    entries: BTreeMap<(i64, u32), StoredPhotoWallReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPhotoWallLedger {
    schema_version: u16,
    entries: Vec<StoredPhotoWallReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPhotoWallReference {
    character_id: i64,
    photo_id: u32,
    picture_type: i32,
    declared_size: Option<u32>,
    version: Option<u32>,
    source_url: String,
}

impl PhotoWallPendingStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or_else(|| "Photo Wall pending ledger path has no parent".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create Photo Wall pending folder: {error}"))?;
        let stored = match std::fs::metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > MAXIMUM_PENDING_LEDGER_BYTES {
                    return Err("Photo Wall pending ledger is not a bounded regular file".into());
                }
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                File::open(&path)
                    .and_then(|mut file| file.read_to_end(&mut bytes))
                    .map_err(|error| {
                        format!("could not read Photo Wall pending ledger: {error}")
                    })?;
                Some(
                    serde_json::from_slice::<StoredPhotoWallLedger>(&bytes).map_err(|error| {
                        format!("Photo Wall pending ledger JSON is invalid: {error}")
                    })?,
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "could not inspect Photo Wall pending ledger: {error}"
                ));
            }
        };
        let mut entries = BTreeMap::new();
        if let Some(stored) = stored {
            if stored.schema_version != PHOTO_WALL_PENDING_SCHEMA_VERSION {
                return Err(format!(
                    "unsupported Photo Wall pending ledger schema {}",
                    stored.schema_version
                ));
            }
            if stored.entries.len() > MAXIMUM_PENDING_REFERENCES {
                return Err(format!(
                    "Photo Wall pending ledger exceeds {MAXIMUM_PENDING_REFERENCES} entries"
                ));
            }
            for entry in stored.entries {
                validate_stored_reference(&entry)?;
                let key = (entry.character_id, entry.photo_id);
                if entries.insert(key, entry).is_some() {
                    return Err("Photo Wall pending ledger contains a duplicate identity".into());
                }
            }
        }
        Ok(Self { path, entries })
    }

    pub fn references(&self) -> Vec<LocalPhotoAssetReference> {
        self.entries
            .values()
            .cloned()
            .map(LocalPhotoAssetReference::from)
            .collect()
    }

    pub fn upsert(&mut self, reference: &LocalPhotoAssetReference) -> Result<bool, String> {
        let stored = StoredPhotoWallReference::try_from(reference)?;
        let key = (stored.character_id, stored.photo_id);
        if self.entries.get(&key).is_some_and(|current| {
            current == &stored
                || current.version.unwrap_or_default() > stored.version.unwrap_or_default()
        }) {
            return Ok(false);
        }
        if self.entries.len() >= MAXIMUM_PENDING_REFERENCES && !self.entries.contains_key(&key) {
            return Err(format!(
                "Photo Wall pending ledger reached its {MAXIMUM_PENDING_REFERENCES}-entry limit"
            ));
        }
        self.entries.insert(key, stored);
        self.persist()?;
        Ok(true)
    }

    pub fn remove_if_matches(
        &mut self,
        reference: &LocalPhotoAssetReference,
    ) -> Result<bool, String> {
        let expected = StoredPhotoWallReference::try_from(reference)?;
        let key = (expected.character_id, expected.photo_id);
        if self.entries.get(&key) != Some(&expected) {
            return Ok(false);
        }
        self.entries.remove(&key);
        self.persist()?;
        Ok(true)
    }

    fn persist(&self) -> Result<(), String> {
        let stored = StoredPhotoWallLedger {
            schema_version: PHOTO_WALL_PENDING_SCHEMA_VERSION,
            entries: self.entries.values().cloned().collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| format!("could not encode Photo Wall pending ledger: {error}"))?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAXIMUM_PENDING_LEDGER_BYTES {
            return Err("Photo Wall pending ledger exceeds its byte limit".into());
        }
        atomic_write(&self.path, &bytes)
    }
}

impl TryFrom<&LocalPhotoAssetReference> for StoredPhotoWallReference {
    type Error = String;

    fn try_from(reference: &LocalPhotoAssetReference) -> Result<Self, Self::Error> {
        let stored = Self {
            character_id: reference.character_id,
            photo_id: reference.photo_id,
            picture_type: reference.picture_type,
            declared_size: reference.declared_size,
            version: reference.version,
            source_url: reference.source_url.clone(),
        };
        validate_stored_reference(&stored)?;
        Ok(stored)
    }
}

impl From<StoredPhotoWallReference> for LocalPhotoAssetReference {
    fn from(reference: StoredPhotoWallReference) -> Self {
        Self {
            character_id: reference.character_id,
            photo_id: reference.photo_id,
            picture_type: reference.picture_type,
            declared_size: reference.declared_size,
            version: reference.version,
            source_url: reference.source_url,
        }
    }
}

fn validate_stored_reference(reference: &StoredPhotoWallReference) -> Result<(), String> {
    if reference.character_id <= 0 || reference.photo_id == 0 {
        return Err("Photo Wall pending reference has an invalid identity".into());
    }
    if !matches!(reference.picture_type, 2 | 3) {
        return Err("Photo Wall pending reference is not a reviewed wall image type".into());
    }
    if reference
        .declared_size
        .is_some_and(|size| size == 0 || size > MAXIMUM_PHOTO_WALL_IMAGE_BYTES)
    {
        return Err("Photo Wall pending reference has an invalid image size".into());
    }
    let url = Url::parse(&reference.source_url)
        .map_err(|_| "Photo Wall pending reference URL is invalid".to_owned())?;
    if url.scheme() != "https"
        || url.host_str() != Some("photo.playbpsr.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/xinghen-prod/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Photo Wall pending reference is outside the reviewed image origin".into());
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("json.partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not replace interrupted Photo Wall pending partial: {error}"
            ));
        }
    }
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)
        .map_err(|error| format!("could not create Photo Wall pending partial: {error}"))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(bytes)
        .and_then(|_| writer.flush())
        .and_then(|_| writer.get_ref().sync_all())
        .map_err(|error| format!("could not sync Photo Wall pending partial: {error}"))?;
    drop(writer);
    if let Err(error) = atomic_replace(&partial, path) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    Ok(())
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
            "could not atomically replace Photo Wall pending ledger: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination)
        .map_err(|error| format!("could not replace Photo Wall pending ledger: {error}"))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rlogs-photo-wall-pending-{}-{nonce}.json",
            std::process::id()
        ))
    }

    fn reference(version: u32) -> LocalPhotoAssetReference {
        LocalPhotoAssetReference {
            character_id: 3_296_036,
            photo_id: 1,
            picture_type: 2,
            declared_size: Some(3_144_767),
            version: Some(version),
            source_url: format!(
                "https://photo.playbpsr.com/xinghen-prod/1/3296036/{version}/photo.png"
            ),
        }
    }

    #[test]
    fn exact_references_survive_restart_and_are_removed_only_after_matching_publication() {
        let path = temporary_path();
        let mut store = PhotoWallPendingStore::open(path.clone()).unwrap();
        let first = reference(4);
        let newer = reference(5);
        assert!(store.upsert(&first).unwrap());
        assert!(store.upsert(&newer).unwrap());
        assert!(!store.upsert(&first).unwrap());

        let mut restored = PhotoWallPendingStore::open(path.clone()).unwrap();
        assert_eq!(restored.references(), vec![newer.clone()]);
        assert!(!restored.remove_if_matches(&first).unwrap());
        assert!(restored.remove_if_matches(&newer).unwrap());
        assert!(restored.references().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unreviewed_or_credential_shaped_urls_never_enter_the_ledger() {
        let path = temporary_path();
        let mut store = PhotoWallPendingStore::open(path.clone()).unwrap();
        let mut invalid = reference(1);
        invalid.source_url =
            "https://photo.playbpsr.com/xinghen-prod/photo.png?token=secret".into();
        assert!(store.upsert(&invalid).is_err());
        assert!(!path.exists());
    }
}
