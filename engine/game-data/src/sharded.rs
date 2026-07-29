use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    AssetRecord, COMPILED_BUNDLE_SCHEMA_VERSION, GAME_DATA_SCHEMA_VERSION, GameDataBuild,
    GameDataError, GameDataManifest, GameDataRecord, LocalizationEntry, SymbolKind,
    validate_source_data,
};

const MAXIMUM_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShardKind {
    Records,
    Localization,
    RecordKeys,
    Assets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledShardDescriptor {
    pub kind: ShardKind,
    pub symbol_kind: Option<SymbolKind>,
    pub locale: Option<String>,
    pub bucket: u16,
    pub relative_path: String,
    pub entries: u32,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub compressed_sha256: String,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledBundleManifest {
    pub schema_version: u16,
    pub content_digest: String,
    pub game_data: GameDataManifest,
    pub shard_bits: u8,
    pub shards: Vec<CompiledShardDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordKey {
    pub stable_key: String,
    pub kind: SymbolKind,
    pub id: i64,
}

pub fn build_bundle_manifest(
    game_data: GameDataManifest,
    shard_bits: u8,
    mut shards: Vec<CompiledShardDescriptor>,
) -> Result<CompiledBundleManifest, GameDataError> {
    validate_source_data(&game_data, &[], &[], &[])?;
    validate_shard_bits(shard_bits)?;
    shards.sort_by(|left, right| {
        (left.kind as u8, left.symbol_kind, &left.locale, left.bucket).cmp(&(
            right.kind as u8,
            right.symbol_kind,
            &right.locale,
            right.bucket,
        ))
    });
    validate_descriptors(&shards, shard_bits)?;
    let content_digest = manifest_digest(&game_data, shard_bits, &shards)?;
    Ok(CompiledBundleManifest {
        schema_version: COMPILED_BUNDLE_SCHEMA_VERSION,
        content_digest,
        game_data,
        shard_bits,
        shards,
    })
}

pub fn encode_json_shard<T: Serialize>(
    records: &[T],
) -> Result<(Vec<u8>, u64, String), GameDataError> {
    let uncompressed = serde_json::to_vec(records)?;
    let content_sha256 = format!("sha256:{:x}", Sha256::digest(&uncompressed));
    let compressed = zstd::stream::encode_all(Cursor::new(&uncompressed), 9)?;
    Ok((compressed, uncompressed.len() as u64, content_sha256))
}

pub fn numeric_id_bucket(id: i64, shard_bits: u8) -> u16 {
    let mask = (1_u64 << shard_bits) - 1;
    ((id as u64) & mask) as u16
}

pub fn stable_key_bucket(key: &str, shard_bits: u8) -> u16 {
    digest_bucket(key.as_bytes(), shard_bits)
}

pub fn localization_bucket(key: &str, shard_bits: u8) -> u16 {
    digest_bucket(key.as_bytes(), shard_bits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub maximum_resident_shards: usize,
    pub maximum_resident_bytes: usize,
    pub maximum_compressed_shard_bytes: usize,
    pub maximum_uncompressed_shard_bytes: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            maximum_resident_shards: 128,
            maximum_resident_bytes: 64 * 1024 * 1024,
            maximum_compressed_shard_bytes: 8 * 1024 * 1024,
            maximum_uncompressed_shard_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub resident_shards: usize,
    pub resident_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
pub struct GameDataStore {
    root: PathBuf,
    manifest: CompiledBundleManifest,
    descriptors: HashMap<LookupKey, CompiledShardDescriptor>,
    cache: Mutex<ShardCache>,
    policy: CachePolicy,
}

impl GameDataStore {
    pub fn open(root: impl AsRef<Path>, policy: CachePolicy) -> Result<Self, GameDataError> {
        if policy.maximum_resident_shards == 0
            || policy.maximum_resident_bytes == 0
            || policy.maximum_compressed_shard_bytes == 0
            || policy.maximum_uncompressed_shard_bytes == 0
        {
            return Err(GameDataError::InvalidCompiledManifest(
                "cache budgets must be greater than zero".into(),
            ));
        }
        let root = fs::canonicalize(root)?;
        let manifest_path = root.join("manifest.json");
        if fs::metadata(&manifest_path)?.len() > MAXIMUM_MANIFEST_BYTES {
            return Err(GameDataError::InvalidCompiledManifest(
                "manifest exceeds the 4 MiB safety budget".into(),
            ));
        }
        let manifest: CompiledBundleManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
        validate_compiled_manifest(&manifest, &policy)?;
        let descriptors = manifest
            .shards
            .iter()
            .cloned()
            .map(|descriptor| Ok((LookupKey::try_from(&descriptor)?, descriptor)))
            .collect::<Result<HashMap<_, _>, GameDataError>>()?;
        Ok(Self {
            root,
            manifest,
            descriptors,
            cache: Mutex::new(ShardCache::default()),
            policy,
        })
    }

    pub fn manifest(&self) -> &CompiledBundleManifest {
        &self.manifest
    }

    pub fn record(
        &self,
        kind: SymbolKind,
        id: i64,
    ) -> Result<Option<Arc<GameDataRecord>>, GameDataError> {
        let key = LookupKey::Records {
            kind,
            bucket: numeric_id_bucket(id, self.manifest.shard_bits),
        };
        let Some(shard) = self.shard(&key)? else {
            return Ok(None);
        };
        match shard.as_ref() {
            CachedShard::Records(records) => Ok(records.get(&id).cloned()),
            _ => Err(GameDataError::InvalidShard(
                "record lookup resolved to the wrong shard type".into(),
            )),
        }
    }

    pub fn record_for_build(
        &self,
        kind: SymbolKind,
        id: i64,
        build: &GameDataBuild,
    ) -> Result<Option<Arc<GameDataRecord>>, GameDataError> {
        Ok(self
            .record(kind, id)?
            .filter(|record| record.is_available_in(build)))
    }

    pub fn record_by_key(
        &self,
        stable_key: &str,
    ) -> Result<Option<Arc<GameDataRecord>>, GameDataError> {
        let key = LookupKey::RecordKeys {
            bucket: stable_key_bucket(stable_key, self.manifest.shard_bits),
        };
        let Some(shard) = self.shard(&key)? else {
            return Ok(None);
        };
        let target = match shard.as_ref() {
            CachedShard::RecordKeys(keys) => keys.get(stable_key).copied(),
            _ => {
                return Err(GameDataError::InvalidShard(
                    "key lookup resolved to the wrong shard type".into(),
                ));
            }
        };
        match target {
            Some((kind, id)) => self.record(kind, id),
            None => Ok(None),
        }
    }

    pub fn record_by_key_for_build(
        &self,
        stable_key: &str,
        build: &GameDataBuild,
    ) -> Result<Option<Arc<GameDataRecord>>, GameDataError> {
        Ok(self
            .record_by_key(stable_key)?
            .filter(|record| record.is_available_in(build)))
    }

    pub fn localized(&self, locale: &str, key: &str) -> Result<Option<Arc<str>>, GameDataError> {
        Ok(self
            .localization_entry(locale, key)?
            .map(|entry| Arc::<str>::from(entry.text.as_str())))
    }

    pub fn localized_for_build(
        &self,
        locale: &str,
        key: &str,
        build: &GameDataBuild,
    ) -> Result<Option<Arc<str>>, GameDataError> {
        Ok(self
            .localization_entry(locale, key)?
            .filter(|entry| entry.is_available_in(build))
            .map(|entry| Arc::<str>::from(entry.text.as_str())))
    }

    pub fn localization_entry(
        &self,
        locale: &str,
        key: &str,
    ) -> Result<Option<Arc<LocalizationEntry>>, GameDataError> {
        let lookup = LookupKey::Localization {
            locale: locale.to_owned(),
            bucket: localization_bucket(key, self.manifest.shard_bits),
        };
        let Some(shard) = self.shard(&lookup)? else {
            return Ok(None);
        };
        match shard.as_ref() {
            CachedShard::Localization(entries) => Ok(entries.get(key).cloned()),
            _ => Err(GameDataError::InvalidShard(
                "localization lookup resolved to the wrong shard type".into(),
            )),
        }
    }

    pub fn asset(&self, key: &str) -> Result<Option<Arc<AssetRecord>>, GameDataError> {
        let lookup = LookupKey::Assets {
            bucket: stable_key_bucket(key, self.manifest.shard_bits),
        };
        let Some(shard) = self.shard(&lookup)? else {
            return Ok(None);
        };
        match shard.as_ref() {
            CachedShard::Assets(entries) => Ok(entries.get(key).cloned()),
            _ => Err(GameDataError::InvalidShard(
                "asset lookup resolved to the wrong shard type".into(),
            )),
        }
    }

    pub fn cache_stats(&self) -> Result<CacheStats, GameDataError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| GameDataError::CachePoisoned)?;
        Ok(cache.stats())
    }

    fn shard(&self, key: &LookupKey) -> Result<Option<Arc<CachedShard>>, GameDataError> {
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| GameDataError::CachePoisoned)?;
            if let Some(shard) = cache.get(key) {
                return Ok(Some(shard));
            }
            cache.misses = cache.misses.saturating_add(1);
        }
        let Some(descriptor) = self.descriptors.get(key) else {
            return Ok(None);
        };
        let (shard, weight) = self.load_shard(descriptor)?;
        let shard = Arc::new(shard);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| GameDataError::CachePoisoned)?;
        cache.insert(key.clone(), Arc::clone(&shard), weight, self.policy);
        Ok(Some(shard))
    }

    fn load_shard(
        &self,
        descriptor: &CompiledShardDescriptor,
    ) -> Result<(CachedShard, usize), GameDataError> {
        let path = checked_join(&self.root, &descriptor.relative_path)?;
        let compressed_length = fs::metadata(&path)?.len();
        if compressed_length > self.policy.maximum_compressed_shard_bytes as u64 {
            return Err(GameDataError::InvalidShard(format!(
                "{} exceeds the compressed shard budget",
                descriptor.relative_path
            )));
        }
        let compressed = fs::read(path)?;
        if compressed.len() as u64 != descriptor.compressed_bytes {
            return Err(GameDataError::InvalidShard(format!(
                "{} compressed length changed",
                descriptor.relative_path
            )));
        }
        verify_digest(
            &compressed,
            &descriptor.compressed_sha256,
            &descriptor.relative_path,
        )?;
        let maximum = descriptor.uncompressed_bytes as usize;
        if maximum > self.policy.maximum_uncompressed_shard_bytes {
            return Err(GameDataError::InvalidShard(format!(
                "{} exceeds the uncompressed shard budget",
                descriptor.relative_path
            )));
        }
        let decoder = zstd::stream::read::Decoder::new(Cursor::new(compressed))?;
        let mut decoded = Vec::with_capacity(maximum);
        decoder
            .take((maximum as u64).saturating_add(1))
            .read_to_end(&mut decoded)?;
        if decoded.len() != maximum {
            return Err(GameDataError::InvalidShard(format!(
                "{} decoded to {} bytes, expected {}",
                descriptor.relative_path,
                decoded.len(),
                maximum
            )));
        }
        verify_digest(
            &decoded,
            &descriptor.content_sha256,
            &descriptor.relative_path,
        )?;
        let shard = decode_shard(descriptor, &decoded, self.manifest.shard_bits)?;
        let weight = decoded
            .len()
            .saturating_add((descriptor.entries as usize).saturating_mul(128));
        Ok((shard, weight))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LookupKey {
    Records { kind: SymbolKind, bucket: u16 },
    Localization { locale: String, bucket: u16 },
    RecordKeys { bucket: u16 },
    Assets { bucket: u16 },
}

impl TryFrom<&CompiledShardDescriptor> for LookupKey {
    type Error = GameDataError;

    fn try_from(descriptor: &CompiledShardDescriptor) -> Result<Self, Self::Error> {
        match descriptor.kind {
            ShardKind::Records => Ok(Self::Records {
                kind: descriptor.symbol_kind.ok_or_else(|| {
                    GameDataError::InvalidCompiledManifest(format!(
                        "{} lacks a symbol kind",
                        descriptor.relative_path
                    ))
                })?,
                bucket: descriptor.bucket,
            }),
            ShardKind::Localization => Ok(Self::Localization {
                locale: descriptor.locale.clone().ok_or_else(|| {
                    GameDataError::InvalidCompiledManifest(format!(
                        "{} lacks a locale",
                        descriptor.relative_path
                    ))
                })?,
                bucket: descriptor.bucket,
            }),
            ShardKind::RecordKeys => Ok(Self::RecordKeys {
                bucket: descriptor.bucket,
            }),
            ShardKind::Assets => Ok(Self::Assets {
                bucket: descriptor.bucket,
            }),
        }
    }
}

#[derive(Debug)]
enum CachedShard {
    Records(HashMap<i64, Arc<GameDataRecord>>),
    Localization(HashMap<String, Arc<LocalizationEntry>>),
    RecordKeys(HashMap<String, (SymbolKind, i64)>),
    Assets(HashMap<String, Arc<AssetRecord>>),
}

#[derive(Debug)]
struct CacheEntry {
    shard: Arc<CachedShard>,
    weight: usize,
    last_access: u64,
}

#[derive(Debug, Default)]
struct ShardCache {
    entries: HashMap<LookupKey, CacheEntry>,
    resident_bytes: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl ShardCache {
    fn get(&mut self, key: &LookupKey) -> Option<Arc<CachedShard>> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(key)?;
        self.hits = self.hits.saturating_add(1);
        entry.last_access = self.clock;
        Some(Arc::clone(&entry.shard))
    }

    fn insert(
        &mut self,
        key: LookupKey,
        shard: Arc<CachedShard>,
        weight: usize,
        policy: CachePolicy,
    ) {
        if weight > policy.maximum_resident_bytes {
            return;
        }
        if let Some(existing) = self.entries.remove(&key) {
            self.resident_bytes = self.resident_bytes.saturating_sub(existing.weight);
        }
        while !self.entries.is_empty()
            && (self.entries.len() >= policy.maximum_resident_shards
                || self.resident_bytes.saturating_add(weight) > policy.maximum_resident_bytes)
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.resident_bytes = self.resident_bytes.saturating_sub(removed.weight);
                self.evictions = self.evictions.saturating_add(1);
            }
        }
        self.clock = self.clock.saturating_add(1);
        self.resident_bytes = self.resident_bytes.saturating_add(weight);
        self.entries.insert(
            key,
            CacheEntry {
                shard,
                weight,
                last_access: self.clock,
            },
        );
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            resident_shards: self.entries.len(),
            resident_bytes: self.resident_bytes,
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
        }
    }
}

fn decode_shard(
    descriptor: &CompiledShardDescriptor,
    decoded: &[u8],
    shard_bits: u8,
) -> Result<CachedShard, GameDataError> {
    match descriptor.kind {
        ShardKind::Records => {
            let records: Vec<GameDataRecord> = decode_records(decoded)?;
            if records.len() != descriptor.entries as usize {
                return Err(entry_count_error(descriptor, records.len()));
            }
            let expected_kind = descriptor.symbol_kind.ok_or_else(|| {
                GameDataError::InvalidShard(format!(
                    "{} has no record kind",
                    descriptor.relative_path
                ))
            })?;
            let mut by_id = HashMap::with_capacity(records.len());
            for record in records {
                if record.schema_version != GAME_DATA_SCHEMA_VERSION
                    || record.stable_key.trim().is_empty()
                    || record.availability.is_empty()
                {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains an invalid game-data record",
                        descriptor.relative_path
                    )));
                }
                if record.kind != expected_kind
                    || numeric_id_bucket(record.id, shard_bits) != descriptor.bucket
                {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains a record assigned to another shard",
                        descriptor.relative_path
                    )));
                }
                if by_id.insert(record.id, Arc::new(record)).is_some() {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains duplicate record IDs",
                        descriptor.relative_path
                    )));
                }
            }
            Ok(CachedShard::Records(by_id))
        }
        ShardKind::Localization => {
            let entries: Vec<LocalizationEntry> = decode_records(decoded)?;
            if entries.len() != descriptor.entries as usize {
                return Err(entry_count_error(descriptor, entries.len()));
            }
            let expected_locale = descriptor.locale.as_deref().ok_or_else(|| {
                GameDataError::InvalidShard(format!("{} has no locale", descriptor.relative_path))
            })?;
            let mut by_key = HashMap::with_capacity(entries.len());
            for entry in entries {
                if entry.schema_version != GAME_DATA_SCHEMA_VERSION
                    || entry.key.trim().is_empty()
                    || entry.availability.is_empty()
                {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains an invalid localization entry",
                        descriptor.relative_path
                    )));
                }
                if entry.locale != expected_locale {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains locale {}",
                        descriptor.relative_path, entry.locale
                    )));
                }
                if localization_bucket(&entry.key, shard_bits) != descriptor.bucket {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains a localization key assigned to another shard",
                        descriptor.relative_path
                    )));
                }
                if by_key.insert(entry.key.clone(), Arc::new(entry)).is_some() {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains duplicate localization keys",
                        descriptor.relative_path
                    )));
                }
            }
            Ok(CachedShard::Localization(by_key))
        }
        ShardKind::RecordKeys => {
            let entries: Vec<RecordKey> = decode_records(decoded)?;
            if entries.len() != descriptor.entries as usize {
                return Err(entry_count_error(descriptor, entries.len()));
            }
            let mut by_key = HashMap::with_capacity(entries.len());
            for entry in entries {
                if entry.stable_key.trim().is_empty() {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains an empty stable key",
                        descriptor.relative_path
                    )));
                }
                if stable_key_bucket(&entry.stable_key, shard_bits) != descriptor.bucket {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains a stable key assigned to another shard",
                        descriptor.relative_path
                    )));
                }
                if by_key
                    .insert(entry.stable_key, (entry.kind, entry.id))
                    .is_some()
                {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains duplicate stable keys",
                        descriptor.relative_path
                    )));
                }
            }
            Ok(CachedShard::RecordKeys(by_key))
        }
        ShardKind::Assets => {
            let entries: Vec<AssetRecord> = decode_records(decoded)?;
            if entries.len() != descriptor.entries as usize {
                return Err(entry_count_error(descriptor, entries.len()));
            }
            let mut by_key = HashMap::with_capacity(entries.len());
            for entry in entries {
                if entry.key.trim().is_empty() || !entry.sha256.starts_with("sha256:") {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains an invalid asset record",
                        descriptor.relative_path
                    )));
                }
                if stable_key_bucket(&entry.key, shard_bits) != descriptor.bucket {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains an asset key assigned to another shard",
                        descriptor.relative_path
                    )));
                }
                if by_key.insert(entry.key.clone(), Arc::new(entry)).is_some() {
                    return Err(GameDataError::InvalidShard(format!(
                        "{} contains duplicate asset keys",
                        descriptor.relative_path
                    )));
                }
            }
            Ok(CachedShard::Assets(by_key))
        }
    }
}

fn decode_records<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, GameDataError> {
    Ok(serde_json::from_slice(bytes)?)
}

fn validate_compiled_manifest(
    manifest: &CompiledBundleManifest,
    policy: &CachePolicy,
) -> Result<(), GameDataError> {
    if manifest.schema_version != COMPILED_BUNDLE_SCHEMA_VERSION {
        return Err(GameDataError::InvalidCompiledManifest(format!(
            "unsupported schema {}",
            manifest.schema_version
        )));
    }
    validate_source_data(&manifest.game_data, &[], &[], &[])?;
    validate_shard_bits(manifest.shard_bits)?;
    validate_descriptors(&manifest.shards, manifest.shard_bits)?;
    let expected = manifest_digest(&manifest.game_data, manifest.shard_bits, &manifest.shards)?;
    if expected != manifest.content_digest {
        return Err(GameDataError::InvalidCompiledManifest(
            "content digest mismatch".into(),
        ));
    }
    for descriptor in &manifest.shards {
        if descriptor.compressed_bytes > policy.maximum_compressed_shard_bytes as u64 {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "{} exceeds the configured compressed shard budget",
                descriptor.relative_path
            )));
        }
        if descriptor.uncompressed_bytes > policy.maximum_uncompressed_shard_bytes as u64 {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "{} exceeds the configured shard budget",
                descriptor.relative_path
            )));
        }
    }
    Ok(())
}

fn validate_descriptors(
    descriptors: &[CompiledShardDescriptor],
    shard_bits: u8,
) -> Result<(), GameDataError> {
    let mut paths = HashSet::with_capacity(descriptors.len());
    let mut lookups = HashSet::with_capacity(descriptors.len());
    let maximum_bucket = (1_u16 << shard_bits) - 1;
    for descriptor in descriptors {
        validate_relative_path(&descriptor.relative_path)?;
        if descriptor.bucket > maximum_bucket {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "{} has bucket {} outside {} bits",
                descriptor.relative_path, descriptor.bucket, shard_bits
            )));
        }
        if descriptor.entries == 0
            || descriptor.compressed_bytes == 0
            || descriptor.uncompressed_bytes == 0
            || !descriptor.compressed_sha256.starts_with("sha256:")
            || !descriptor.content_sha256.starts_with("sha256:")
        {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "{} has incomplete metadata",
                descriptor.relative_path
            )));
        }
        if !paths.insert(descriptor.relative_path.clone()) {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "duplicate path {}",
                descriptor.relative_path
            )));
        }
        let lookup = LookupKey::try_from(descriptor)?;
        if !lookups.insert(lookup) {
            return Err(GameDataError::InvalidCompiledManifest(format!(
                "duplicate lookup for {}",
                descriptor.relative_path
            )));
        }
    }
    Ok(())
}

fn validate_shard_bits(shard_bits: u8) -> Result<(), GameDataError> {
    if !(4..=8).contains(&shard_bits) {
        return Err(GameDataError::InvalidCompiledManifest(format!(
            "shard_bits must be in 4..=8, got {shard_bits}"
        )));
    }
    Ok(())
}

fn manifest_digest(
    game_data: &GameDataManifest,
    shard_bits: u8,
    shards: &[CompiledShardDescriptor],
) -> Result<String, GameDataError> {
    let bytes = serde_json::to_vec(&(game_data, shard_bits, shards))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn digest_bucket(bytes: &[u8], shard_bits: u8) -> u16 {
    let digest = Sha256::digest(bytes);
    let mask = (1_u16 << shard_bits) - 1;
    u16::from(digest[0]) & mask
}

fn checked_join(root: &Path, relative: &str) -> Result<PathBuf, GameDataError> {
    validate_relative_path(relative)?;
    let resolved = fs::canonicalize(root.join(relative))?;
    if !resolved.starts_with(root) {
        return Err(GameDataError::UnsafePath(relative.to_owned()));
    }
    Ok(resolved)
}

fn validate_relative_path(relative: &str) -> Result<(), GameDataError> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GameDataError::UnsafePath(relative.to_owned()));
    }
    Ok(())
}

fn verify_digest(bytes: &[u8], expected: &str, label: &str) -> Result<(), GameDataError> {
    let actual = format!("sha256:{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(GameDataError::InvalidShard(format!(
            "{label} digest mismatch"
        )))
    }
}

fn entry_count_error(descriptor: &CompiledShardDescriptor, actual: usize) -> GameDataError {
    GameDataError::InvalidShard(format!(
        "{} contains {} entries, expected {}",
        descriptor.relative_path, actual, descriptor.entries
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_functions_are_deterministic_and_bounded() {
        assert_eq!(
            stable_key_bucket("skill.stormblade.1714", 8),
            stable_key_bucket("skill.stormblade.1714", 8)
        );
        assert!(numeric_id_bucket(-1, 8) <= 255);
        assert!(localization_bucket("game.skill.1714", 4) <= 15);
    }

    #[test]
    fn cache_evicts_to_both_limits() {
        let mut cache = ShardCache::default();
        let policy = CachePolicy {
            maximum_resident_shards: 2,
            maximum_resident_bytes: 25,
            maximum_compressed_shard_bytes: 1024,
            maximum_uncompressed_shard_bytes: 1024,
        };
        for bucket in 0..3 {
            cache.insert(
                LookupKey::RecordKeys { bucket },
                Arc::new(CachedShard::RecordKeys(HashMap::new())),
                10,
                policy,
            );
        }
        let stats = cache.stats();
        assert_eq!(stats.resident_shards, 2);
        assert_eq!(stats.resident_bytes, 20);
        assert_eq!(stats.evictions, 1);
    }
}
