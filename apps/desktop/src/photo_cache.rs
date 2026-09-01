use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use prost::Message;
use reqwest::Url;
use rlogs_game_bpsr::LocalPhotoAssetReference;
use rlogs_profiles::LocalProfilePackage;
use rusqlite::{Connection, OpenFlags};

const MAXIMUM_CACHE_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_CACHE_PROTO_BYTES: usize = 1024 * 1024;
const MAXIMUM_PHOTO_BYTES: u64 = 8 * 1024 * 1024;
const MAXIMUM_CACHE_ROWS: usize = 64;

#[derive(Clone, PartialEq, Message)]
struct HttpCachePhotoInfo {
    #[prost(map = "string, string", tag = "1")]
    entries: HashMap<String, String>,
}

pub(crate) fn package_photo_wall_identity(
    package: &LocalProfilePackage,
) -> Option<(i64, Vec<u32>)> {
    let character_id = package
        .request
        .payload
        .routing
        .get("character-id")?
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)?;
    let collection = package
        .request
        .payload
        .body
        .get("collection_summary")?
        .as_object()?;
    let mut photo_ids = BTreeSet::new();
    if let Some(values) = collection
        .get("photo_ids")
        .and_then(serde_json::Value::as_array)
    {
        photo_ids.extend(values.iter().filter_map(json_photo_id));
    }
    if let Some(wall) = collection
        .get("photo_wall")
        .and_then(serde_json::Value::as_object)
    {
        photo_ids.extend(wall.values().filter_map(json_photo_id));
    }
    (!photo_ids.is_empty()).then(|| (character_id, photo_ids.into_iter().collect()))
}

pub(crate) fn reviewed_cached_photo_asset(
    character_id: i64,
    photo_ids: &[u32],
) -> Option<LocalPhotoAssetReference> {
    let photo_ids = unique_photo_ids(photo_ids);
    let [photo_id] = photo_ids.as_slice() else {
        return None;
    };
    let mut candidates = Vec::new();
    for database in candidate_database_paths() {
        let Some(root) = database.parent().and_then(Path::parent) else {
            continue;
        };
        candidates.extend(read_character_cache(&database, root, character_id));
    }
    candidates.sort();
    candidates.dedup();
    let [(source_url, local_path, declared_size)] = candidates.as_slice() else {
        return None;
    };
    let _ = local_path;
    Some(LocalPhotoAssetReference {
        character_id,
        photo_id: *photo_id,
        picture_type: 2,
        declared_size: *declared_size,
        version: None,
        source_url: source_url.clone(),
    })
}

fn read_character_cache(
    database: &Path,
    persistent_root: &Path,
    character_id: i64,
) -> Vec<(String, PathBuf, Option<u32>)> {
    if character_id <= 0
        || !database.metadata().is_ok_and(|metadata| {
            metadata.is_file() && metadata.len() <= MAXIMUM_CACHE_DATABASE_BYTES
        })
    {
        return Vec::new();
    }
    let Ok(connection) = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let Ok(mut statement) = connection.prepare(
        "SELECT key, value FROM cache_photo_info WHERE typeof(key) = 'integer' AND typeof(value) = 'blob' LIMIT 65",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .take(MAXIMUM_CACHE_ROWS)
        .filter(|(player_uuid, bytes)| {
            *player_uuid > 0
                && (*player_uuid >> 16) == character_id
                && bytes.len() <= MAXIMUM_CACHE_PROTO_BYTES
        })
        .flat_map(|(_, bytes)| {
            HttpCachePhotoInfo::decode(bytes.as_slice())
                .ok()
                .into_iter()
                .flat_map(|cache| cache.entries)
        })
        .filter_map(|(source_url, local_path)| {
            reviewed_cache_entry(persistent_root, source_url, PathBuf::from(local_path))
        })
        .collect()
}

fn reviewed_cache_entry(
    persistent_root: &Path,
    source_url: String,
    local_path: PathBuf,
) -> Option<(String, PathBuf, Option<u32>)> {
    if !reviewed_source_url(&source_url) {
        return None;
    }
    let root = persistent_root.canonicalize().ok()?;
    let local_path = local_path.canonicalize().ok()?;
    if !local_path.starts_with(&root) {
        return None;
    }
    let metadata = local_path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAXIMUM_PHOTO_BYTES {
        return None;
    }
    let mut header = [0_u8; 12];
    let read = File::open(&local_path).ok()?.read(&mut header).ok()?;
    if !is_reviewed_image_header(&header[..read]) {
        return None;
    }
    Some((source_url, local_path, u32::try_from(metadata.len()).ok()))
}

fn reviewed_source_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("photo.playbpsr.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with("/xinghen-prod/")
            && url.fragment().is_none()
    })
}

fn is_reviewed_image_header(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || (bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

fn unique_photo_ids(values: &[u32]) -> Vec<u32> {
    values
        .iter()
        .copied()
        .filter(|value| *value > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn json_photo_id(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn candidate_database_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let Some(profile) = std::env::var_os("USERPROFILE") else {
            return Vec::new();
        };
        return ["BPSR_STEAM", "BPSR"]
            .into_iter()
            .map(|product| {
                PathBuf::from(&profile)
                    .join("AppData/LocalLow/bokura")
                    .join(product)
                    .join("db/brk_panda.db")
            })
            .collect();
    }
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
        let Some(base) = base else {
            return Vec::new();
        };
        return ["BPSR_STEAM", "BPSR"]
            .into_iter()
            .map(|product| {
                base.join("unity3d/bokura")
                    .join(product)
                    .join("db/brk_panda.db")
            })
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var_os("HOME") else {
            return Vec::new();
        };
        return ["BPSR_STEAM", "BPSR"]
            .into_iter()
            .map(|product| {
                PathBuf::from(&home)
                    .join("Library/Application Support/bokura")
                    .join(product)
                    .join("db/brk_panda.db")
            })
            .collect();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rlogs_profiles::ProfilePackageSource;
    use rlogs_submission::{WebsitePayloadEnvelope, WebsitePayloadRequest};
    use serde_json::json;

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("rlogs-photo-cache-{sequence}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (TestDirectory, PathBuf, PathBuf) {
        let directory = TestDirectory::new();
        let persistent_root = directory.0.join("BPSR_STEAM");
        let database = persistent_root.join("db/brk_panda.db");
        let photo = persistent_root.join("RenderPhoto/render.png");
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
        std::fs::write(&photo, b"\x89PNG\r\n\x1a\nreviewed").unwrap();
        (directory, database, photo)
    }

    fn write_cache(database: &Path, character_id: i64, entries: HashMap<String, String>) {
        let connection = Connection::open(database).unwrap();
        connection
            .execute(
                "CREATE TABLE cache_photo_info (key PRIMARY KEY, value blob)",
                [],
            )
            .unwrap();
        let bytes = HttpCachePhotoInfo { entries }.encode_to_vec();
        let player_uuid = (character_id << 16) | 640;
        connection
            .execute(
                "INSERT INTO cache_photo_info (key, value) VALUES (?1, ?2)",
                rusqlite::params![player_uuid, bytes],
            )
            .unwrap();
    }

    fn profile_package(collection_summary: serde_json::Value) -> LocalProfilePackage {
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
                    ("character-id".into(), "3296036".into()),
                ]),
                json!({"collection_summary": collection_summary}),
            )
            .unwrap(),
        )
        .unwrap();
        LocalProfilePackage::new(
            1,
            ProfilePackageSource {
                session_id: "session-photo-wall".into(),
                client_build: "steam-24687926".into(),
                protocol_pack_digest: "sha256:pack".into(),
                canonical_content_sha256: format!("sha256:{}", "a".repeat(64)),
                observation_count: 1,
                last_event_sequence: 1,
                live_capture: None,
            },
            request,
        )
        .unwrap()
    }

    #[test]
    fn profile_identity_combines_wall_slots_and_explicit_photo_ids_exactly() {
        let package = profile_package(json!({
            "photo_ids": [7, 1, 7, 0, "not-an-id"],
            "photo_wall": {"1": 1, "2": 7, "3": 0}
        }));
        assert_eq!(
            package_photo_wall_identity(&package),
            Some((3_296_036, vec![1, 7]))
        );

        let empty = profile_package(json!({"photo_ids": [], "photo_wall": {}}));
        assert_eq!(package_photo_wall_identity(&empty), None);
    }

    #[test]
    fn exact_character_and_single_photo_resolve_the_reviewed_cache_entry() {
        let (_directory, database, photo) = fixture();
        write_cache(
            &database,
            3_296_036,
            HashMap::from([(
                "https://photo.playbpsr.com/xinghen-prod/render.png".into(),
                photo.display().to_string(),
            )]),
        );
        let candidates = read_character_cache(
            &database,
            database.parent().unwrap().parent().unwrap(),
            3_296_036,
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].0,
            "https://photo.playbpsr.com/xinghen-prod/render.png"
        );
        assert_eq!(candidates[0].2, Some(16));
    }

    #[test]
    fn wrong_character_and_outside_paths_are_rejected() {
        let (directory, database, _photo) = fixture();
        let outside = directory.0.join("outside.png");
        std::fs::write(&outside, b"\x89PNG\r\n\x1a\nreviewed").unwrap();
        write_cache(
            &database,
            3_296_036,
            HashMap::from([(
                "https://photo.playbpsr.com/xinghen-prod/render.png".into(),
                outside.display().to_string(),
            )]),
        );
        let root = database.parent().unwrap().parent().unwrap();
        assert!(read_character_cache(&database, root, 3_296_036).is_empty());
        assert!(read_character_cache(&database, root, 9_999_999).is_empty());
    }

    #[test]
    fn multiple_photo_ids_do_not_guess_a_cache_mapping() {
        assert_eq!(unique_photo_ids(&[1, 2]), vec![1, 2]);
        assert_eq!(unique_photo_ids(&[1, 1]), vec![1]);
    }

    #[test]
    #[ignore = "requires an installed BPSR client cache and explicit UID/photo inputs"]
    fn installed_cache_resolves_without_printing_the_private_source_url() {
        let character_id = std::env::var("RLOGS_BPSR_PHOTO_CACHE_UID")
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let photo_id = std::env::var("RLOGS_BPSR_PHOTO_CACHE_PHOTO_ID")
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let asset = reviewed_cached_photo_asset(character_id, &[photo_id]).unwrap();
        assert_eq!(asset.character_id, character_id);
        assert_eq!(asset.photo_id, photo_id);
        assert!(
            asset
                .source_url
                .starts_with("https://photo.playbpsr.com/xinghen-prod/")
        );
        assert!(asset.declared_size.is_some_and(|size| size > 0));
        println!(
            "resolved UID {} photo {} from a {}-byte reviewed local cache entry",
            asset.character_id,
            asset.photo_id,
            asset.declared_size.unwrap()
        );
    }
}
