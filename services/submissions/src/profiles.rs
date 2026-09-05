use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_ENDPOINT, BPSR_PROFILE_SCHEMA_ID,
    BPSR_PROFILE_SCHEMA_VERSION, CharacterProfilePatch, merge_profile_patches,
    website_profile_request,
};
use rlogs_profiles::LocalProfilePackage;
use rlogs_submission::WebsitePayloadEnvelope;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PUBLIC_PROFILE_SCHEMA_VERSION: u16 = 1;
pub const PUBLIC_PROFILE_CATALOG_SCHEMA_VERSION: u16 = 1;
const PROFILE_CLAIM_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_PROFILE_COUNT: usize = 100_000;
pub const MAXIMUM_PHOTO_WALL_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAXIMUM_PUBLIC_PHOTO_FEED_ENTRIES: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhotoAssetReceipt {
    pub schema_version: u16,
    pub profile_id: String,
    pub photo_id: u32,
    pub byte_length: usize,
    pub sha256: String,
    pub media_type: String,
    pub image_path: String,
    pub uploaded_unix_millis: u64,
}

#[derive(Debug)]
pub struct PhotoAssetContent {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPhotoAsset {
    schema_version: u16,
    profile_id: String,
    photo_id: u32,
    byte_length: usize,
    sha256: String,
    media_type: String,
    file_name: String,
    image_path: String,
    #[serde(default)]
    uploaded_unix_millis: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhotoCatalogSort {
    #[default]
    Newest,
    Popular,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoCatalogQuery {
    pub sort: Option<PhotoCatalogSort>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicPhotoCatalog {
    pub schema_version: u16,
    pub total_entries: usize,
    pub entries: Vec<PublicPhotoCatalogEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicPhotoCatalogEntry {
    pub profile_id: String,
    pub character_id: String,
    pub display_name: Option<String>,
    pub photo_id: u32,
    pub image_path: String,
    pub uploaded_unix_millis: u64,
    pub like_count: usize,
    pub viewer_liked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhotoLikeReceipt {
    pub schema_version: u16,
    pub profile_id: String,
    pub photo_id: u32,
    pub liked: bool,
    pub like_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPhotoLike {
    schema_version: u16,
    submitter_digest: String,
    liked_unix_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProfile {
    pub schema_version: u16,
    pub profile_id: String,
    pub claimed: bool,
    pub package_id: String,
    pub created_unix_millis: u64,
    pub updated_unix_millis: u64,
    pub source_client_build: String,
    pub source_observation_count: u64,
    pub source_last_event_sequence: u64,
    pub deployment: String,
    pub region: String,
    pub realm: Option<String>,
    pub world: Option<String>,
    pub character_id: String,
    pub display_name: Option<String>,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
    #[serde(default)]
    pub loadouts: Vec<PublicProfileLoadoutSummary>,
    pub envelope: WebsitePayloadEnvelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProfileLoadoutSummary {
    pub project_id: i32,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub profession_id: Option<i32>,
    #[serde(default = "default_true")]
    pub snapshot_available: bool,
    pub updated_unix_millis: u64,
    pub source_client_build: String,
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProfileLoadout {
    pub schema_version: u16,
    pub profile_id: String,
    pub project_id: i32,
    pub updated_unix_millis: u64,
    pub source_client_build: String,
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
    pub envelope: WebsitePayloadEnvelope,
}

impl PublicProfileLoadout {
    fn summary(&self) -> PublicProfileLoadoutSummary {
        PublicProfileLoadoutSummary {
            project_id: self.project_id,
            project_name: None,
            profession_id: self.class_id,
            snapshot_available: true,
            updated_unix_millis: self.updated_unix_millis,
            source_client_build: self.source_client_build.clone(),
            class_id: self.class_id,
            specialization_id: self.specialization_id,
            module_inventory_count: self.module_inventory_count,
            equipped_module_count: self.equipped_module_count,
        }
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProfileCatalogEntry {
    pub profile_id: String,
    pub claimed: bool,
    pub package_id: String,
    pub updated_unix_millis: u64,
    pub source_client_build: String,
    pub deployment: String,
    pub region: String,
    pub realm: Option<String>,
    pub world: Option<String>,
    pub character_id: String,
    pub display_name: Option<String>,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicProfileCatalog {
    pub schema_version: u16,
    pub profiles: Vec<PublicProfileCatalogEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProfilePublishReceipt {
    pub schema_version: u16,
    pub profile_id: String,
    pub character_id: String,
    pub package_id: String,
    pub claimed: bool,
    pub duplicate: bool,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
    pub profile_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileClaim {
    schema_version: u16,
    profile_id: String,
    submitter_id: String,
    character_id: String,
    claimed_unix_millis: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileRegistryError {
    #[error("profile publication requires an authenticated user identity")]
    AuthenticationRequired,
    #[error("profile package is invalid: {0}")]
    InvalidPackage(String),
    #[error("UID {character_id} is already claimed by another authenticated user")]
    ClaimConflict { character_id: String },
    #[error("profile package is older than the currently published profile")]
    StalePackage,
    #[error("profile catalog exceeded its safety limit")]
    CatalogTooLarge,
    #[error("profile was not found")]
    NotFound,
    #[error("Photo Wall image is invalid: {0}")]
    InvalidPhotoAsset(String),
    #[error("photo {photo_id} was not observed on this claimed profile")]
    PhotoNotObserved { photo_id: u32 },
    #[error("could not encode or decode profile JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("profile storage failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct ProfileRegistry {
    root: PathBuf,
    public_site_url: String,
}

impl ProfileRegistry {
    pub fn open(root: PathBuf, public_site_url: String) -> Result<Self, ProfileRegistryError> {
        std::fs::create_dir_all(&root)?;
        let value = Self {
            root,
            public_site_url: public_site_url.trim_end_matches('/').to_owned(),
        };
        value.rebuild_catalog()?;
        Ok(value)
    }

    pub fn publish(
        &self,
        package: LocalProfilePackage,
        submitter_id: Option<&str>,
        device_id: Option<&str>,
        device_token: &str,
        accepted_unix_millis: u64,
    ) -> Result<ProfilePublishReceipt, ProfileRegistryError> {
        let submitter_id = submitter_id.ok_or(ProfileRegistryError::AuthenticationRequired)?;
        let device_id = device_id.ok_or(ProfileRegistryError::AuthenticationRequired)?;
        package
            .validate()
            .map_err(|error| ProfileRegistryError::InvalidPackage(error.to_string()))?;
        if !package.verifies_live_capture(device_id, device_token) {
            return Err(ProfileRegistryError::InvalidPackage(
                "UID claims require device-bound proof from a live process-owned capture".into(),
            ));
        }
        let mut profile = validate_bpsr_package(&package)?;
        let profile_id = self
            .existing_profile_id_for_character(&profile.character.character_id, submitter_id)?
            .unwrap_or_else(|| profile_id(&package.request.payload));
        let directory = self.root.join(&profile_id);
        std::fs::create_dir_all(&directory)?;
        let claim_path = directory.join("claim.json");
        let current_path = directory.join("public.json");
        let package_path = directory.join("current.profile.json");

        let existing_claim = read_optional_json::<ProfileClaim>(&claim_path)?;
        if let Some(existing_claim) = &existing_claim {
            if existing_claim.submitter_id != submitter_id {
                return Err(ProfileRegistryError::ClaimConflict {
                    character_id: profile.character.character_id.clone(),
                });
            }
        }
        let existing_profile = read_optional_json::<PublicProfile>(&current_path)?;
        let duplicate = existing_profile
            .as_ref()
            .is_some_and(|existing| existing.package_id == package.package_id);
        if let Some(existing) = &existing_profile {
            if !duplicate && package.created_unix_millis <= existing.created_unix_millis {
                return Err(ProfileRegistryError::StalePackage);
            }
        }

        // A project is an in-game saved loadout. Keep its verified snapshot isolated
        // from the character-wide merge below so equipment and raw town stats from
        // different builds can never bleed into one another.
        let loadout = if let Some(project_id) = profile
            .current_profession_project_id
            .filter(|project_id| *project_id > 0)
        {
            let loadout_directory = directory.join("loadouts");
            std::fs::create_dir_all(&loadout_directory)?;
            let loadout_path = loadout_directory.join(format!("{project_id}.json"));
            let existing_loadout = read_optional_json::<PublicProfileLoadout>(&loadout_path)?;
            let mut loadout_profile = profile.clone();
            if let Some(existing) = &existing_loadout {
                let mut accumulated: CharacterProfilePatch =
                    serde_json::from_value(existing.envelope.body.clone())?;
                refine_profile_routing_identity(&mut accumulated, &loadout_profile)?;
                merge_profile_patches(&mut accumulated, loadout_profile).map_err(|error| {
                    ProfileRegistryError::InvalidPackage(format!(
                        "could not merge the newer verified loadout patch: {error}"
                    ))
                })?;
                loadout_profile = accumulated;
            }
            let modules = loadout_profile.modules.as_ref();
            let mut envelope = package.request.payload.clone();
            envelope.body = website_profile_request(&loadout_profile)
                .map_err(|error| ProfileRegistryError::InvalidPackage(error.to_string()))?
                .payload
                .body;
            prefer_observed_photo_wall_identity(&package.request.payload.body, &mut envelope.body);
            Some((
                loadout_path,
                PublicProfileLoadout {
                    schema_version: PUBLIC_PROFILE_SCHEMA_VERSION,
                    profile_id: profile_id.clone(),
                    project_id,
                    updated_unix_millis: accepted_unix_millis,
                    source_client_build: package.source.client_build.clone(),
                    class_id: loadout_profile.class_id,
                    specialization_id: loadout_profile.specialization_id,
                    module_inventory_count: modules.map_or(0, |value| value.inventory.len()),
                    equipped_module_count: modules.map_or(0, |value| value.equipped_slots.len()),
                    envelope,
                },
            ))
        } else {
            None
        };

        if let Some(existing) = &existing_profile {
            let mut accumulated: CharacterProfilePatch =
                serde_json::from_value(existing.envelope.body.clone())?;
            refine_profile_routing_identity(&mut accumulated, &profile)?;
            merge_profile_patches(&mut accumulated, profile).map_err(|error| {
                ProfileRegistryError::InvalidPackage(format!(
                    "could not merge the newer verified profile patch: {error}"
                ))
            })?;
            profile = accumulated;
        }

        let claim = existing_claim.unwrap_or(ProfileClaim {
            schema_version: PROFILE_CLAIM_SCHEMA_VERSION,
            profile_id: profile_id.clone(),
            submitter_id: submitter_id.to_owned(),
            character_id: profile.character.character_id.clone(),
            claimed_unix_millis: accepted_unix_millis,
        });
        let modules = profile.modules.as_ref();
        let mut envelope = package.request.payload.clone();
        envelope.body = website_profile_request(&profile)
            .map_err(|error| ProfileRegistryError::InvalidPackage(error.to_string()))?
            .payload
            .body;
        prefer_observed_photo_wall_identity(&package.request.payload.body, &mut envelope.body);
        preserve_current_photo_assets(existing_profile.as_ref(), &mut envelope);
        let mut loadouts = existing_profile
            .as_ref()
            .map_or_else(Vec::new, |existing| existing.loadouts.clone());
        if let Some((_, loadout)) = &loadout {
            loadouts.retain(|summary| summary.project_id != loadout.project_id);
            loadouts.push(loadout.summary());
        }
        if let Some(projects) = profile.profession_projects.as_ref() {
            for project in projects {
                if let Some(summary) = loadouts
                    .iter_mut()
                    .find(|summary| summary.project_id == project.project_id)
                {
                    summary.project_name = Some(project.project_name.clone());
                    summary.profession_id = project.profession_id;
                    if summary.class_id.is_none() {
                        summary.class_id = project.profession_id;
                    }
                } else {
                    loadouts.push(PublicProfileLoadoutSummary {
                        project_id: project.project_id,
                        project_name: Some(project.project_name.clone()),
                        profession_id: project.profession_id,
                        snapshot_available: false,
                        updated_unix_millis: accepted_unix_millis,
                        source_client_build: package.source.client_build.clone(),
                        class_id: project.profession_id,
                        specialization_id: None,
                        module_inventory_count: 0,
                        equipped_module_count: 0,
                    });
                }
            }
        }
        loadouts.sort_by_key(|summary| summary.project_id);
        let published = PublicProfile {
            schema_version: PUBLIC_PROFILE_SCHEMA_VERSION,
            profile_id,
            claimed: true,
            package_id: package.package_id.clone(),
            created_unix_millis: existing_profile.as_ref().map_or(
                package.created_unix_millis,
                |existing| {
                    existing
                        .created_unix_millis
                        .max(package.created_unix_millis)
                },
            ),
            updated_unix_millis: accepted_unix_millis,
            source_client_build: package.source.client_build.clone(),
            source_observation_count: package.source.observation_count,
            source_last_event_sequence: package.source.last_event_sequence,
            deployment: package.request.payload.routing["deployment"].clone(),
            region: package.request.payload.routing["region"].clone(),
            realm: package.request.payload.routing.get("realm").cloned(),
            world: package.request.payload.routing.get("world").cloned(),
            character_id: profile.character.character_id.clone(),
            display_name: profile.display_name.clone(),
            module_inventory_count: modules.map_or(0, |value| value.inventory.len()),
            equipped_module_count: modules.map_or(0, |value| value.equipped_slots.len()),
            loadouts,
            envelope,
        };

        // The claim is written first and never replaced by a later submission.
        // A crash can therefore leave a claimed-but-unpublished UID, but can
        // never make an already claimed UID available to a second account.
        if !claim_path.exists() {
            write_json_new(&claim_path, &claim)?;
        }
        write_json_atomic(&package_path, &package)?;
        if let Some((path, loadout)) = &loadout {
            write_json_atomic(path, loadout)?;
        }
        write_json_atomic(&current_path, &published)?;
        self.rebuild_catalog()?;
        Ok(self.receipt(&published, duplicate))
    }

    pub fn get(&self, profile_id: &str) -> Result<PublicProfile, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        read_json(&self.root.join(profile_id).join("public.json"))
    }

    pub fn get_loadout(
        &self,
        profile_id: &str,
        project_id: i32,
    ) -> Result<PublicProfileLoadout, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        if project_id <= 0 {
            return Err(ProfileRegistryError::NotFound);
        }
        read_json(
            &self
                .root
                .join(profile_id)
                .join("loadouts")
                .join(format!("{project_id}.json")),
        )
    }

    pub fn publish_photo_asset(
        &self,
        profile_id: &str,
        photo_id: u32,
        bytes: &[u8],
        submitter_id: Option<&str>,
        accepted_unix_millis: u64,
    ) -> Result<PhotoAssetReceipt, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        let submitter_id = submitter_id.ok_or(ProfileRegistryError::AuthenticationRequired)?;
        if photo_id == 0 {
            return Err(ProfileRegistryError::InvalidPhotoAsset(
                "photo ID must be positive".into(),
            ));
        }
        if bytes.is_empty() || bytes.len() > MAXIMUM_PHOTO_WALL_IMAGE_BYTES {
            return Err(ProfileRegistryError::InvalidPhotoAsset(format!(
                "image must contain 1 to {MAXIMUM_PHOTO_WALL_IMAGE_BYTES} bytes"
            )));
        }
        let (media_type, extension) = reviewed_image_format(bytes).ok_or_else(|| {
            ProfileRegistryError::InvalidPhotoAsset(
                "only JPEG, PNG, and WebP raster images are accepted".into(),
            )
        })?;
        let directory = self.root.join(profile_id);
        let claim: ProfileClaim = read_json(&directory.join("claim.json"))?;
        if claim.submitter_id != submitter_id {
            return Err(ProfileRegistryError::ClaimConflict {
                character_id: claim.character_id,
            });
        }
        let current_path = directory.join("public.json");
        let mut profile: PublicProfile = read_json(&current_path)?;
        if !profile_contains_photo_id(&profile, photo_id) {
            return Err(ProfileRegistryError::PhotoNotObserved { photo_id });
        }

        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let assets = directory.join("photo-wall");
        std::fs::create_dir_all(&assets)?;
        let metadata_path = assets.join(format!("photo-{photo_id}.json"));
        let previous: Option<StoredPhotoAsset> = read_optional_json(&metadata_path)?;
        let file_name = format!("photo-{photo_id}-{sha256}.{extension}");
        let file_path = assets.join(&file_name);
        if !file_path.exists() {
            write_bytes_new(&file_path, bytes)?;
        }
        let image_path = format!("/v1/profiles/{profile_id}/photo-wall/{photo_id}");
        let uploaded_unix_millis = previous
            .as_ref()
            .filter(|asset| asset.sha256 == sha256)
            .map_or(accepted_unix_millis, |asset| asset.uploaded_unix_millis);
        let stored = StoredPhotoAsset {
            schema_version: 1,
            profile_id: profile_id.to_owned(),
            photo_id,
            byte_length: bytes.len(),
            sha256: sha256.clone(),
            media_type: media_type.into(),
            file_name,
            image_path: image_path.clone(),
            uploaded_unix_millis,
        };
        write_json_atomic(&metadata_path, &stored)?;
        upsert_public_photo_asset(&mut profile, &stored)?;
        profile.updated_unix_millis = accepted_unix_millis;
        write_json_atomic(&current_path, &profile)?;
        self.rebuild_catalog()?;
        if let Some(previous) = previous.filter(|previous| previous.file_name != stored.file_name)
            && stored_photo_file_name_is_safe(&previous)
        {
            match std::fs::remove_file(assets.join(previous.file_name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(PhotoAssetReceipt {
            schema_version: 1,
            profile_id: profile_id.to_owned(),
            photo_id,
            byte_length: bytes.len(),
            sha256,
            media_type: media_type.into(),
            image_path,
            uploaded_unix_millis,
        })
    }

    pub fn photo_catalog(
        &self,
        query: &PhotoCatalogQuery,
        viewer_submitter_id: Option<&str>,
    ) -> Result<PublicPhotoCatalog, ProfileRegistryError> {
        let mut entries = Vec::new();
        for directory in std::fs::read_dir(&self.root)? {
            let directory = directory?;
            if !directory.file_type()?.is_dir() {
                continue;
            }
            let profile_path = directory.path().join("public.json");
            if !profile_path.is_file() {
                continue;
            }
            let profile: PublicProfile = read_json(&profile_path)?;
            let assets = directory.path().join("photo-wall");
            if !assets.is_dir() {
                continue;
            }
            for metadata in std::fs::read_dir(&assets)? {
                let metadata = metadata?;
                let file_name = metadata.file_name();
                let Some(file_name) = file_name.to_str() else {
                    continue;
                };
                if !metadata.file_type()?.is_file()
                    || !file_name.starts_with("photo-")
                    || !file_name.ends_with(".json")
                {
                    continue;
                }
                let stored: StoredPhotoAsset = read_json(&metadata.path())?;
                if stored.profile_id != profile.profile_id
                    || !profile_contains_photo_id(&profile, stored.photo_id)
                    || !stored_photo_file_name_is_safe(&stored)
                {
                    continue;
                }
                let uploaded_unix_millis = if stored.uploaded_unix_millis > 0 {
                    stored.uploaded_unix_millis
                } else {
                    file_modified_unix_millis(&metadata.path())
                        .unwrap_or(profile.updated_unix_millis)
                };
                entries.push(PublicPhotoCatalogEntry {
                    profile_id: profile.profile_id.clone(),
                    character_id: profile.character_id.clone(),
                    display_name: profile.display_name.clone(),
                    photo_id: stored.photo_id,
                    image_path: stored.image_path,
                    uploaded_unix_millis,
                    like_count: self.photo_like_count(&profile.profile_id, stored.photo_id)?,
                    viewer_liked: viewer_submitter_id.is_some_and(|submitter_id| {
                        self.photo_like_path(&profile.profile_id, stored.photo_id, submitter_id)
                            .is_file()
                    }),
                });
            }
        }
        match query.sort.unwrap_or_default() {
            PhotoCatalogSort::Newest => entries.sort_by(|left, right| {
                right
                    .uploaded_unix_millis
                    .cmp(&left.uploaded_unix_millis)
                    .then_with(|| right.like_count.cmp(&left.like_count))
                    .then_with(|| left.profile_id.cmp(&right.profile_id))
                    .then_with(|| left.photo_id.cmp(&right.photo_id))
            }),
            PhotoCatalogSort::Popular => entries.sort_by(|left, right| {
                right
                    .like_count
                    .cmp(&left.like_count)
                    .then_with(|| right.uploaded_unix_millis.cmp(&left.uploaded_unix_millis))
                    .then_with(|| left.profile_id.cmp(&right.profile_id))
                    .then_with(|| left.photo_id.cmp(&right.photo_id))
            }),
        }
        let total_entries = entries.len();
        entries.truncate(
            query
                .limit
                .unwrap_or(24)
                .clamp(1, MAXIMUM_PUBLIC_PHOTO_FEED_ENTRIES),
        );
        Ok(PublicPhotoCatalog {
            schema_version: 1,
            total_entries,
            entries,
        })
    }

    pub fn set_photo_like(
        &self,
        profile_id: &str,
        photo_id: u32,
        submitter_id: &str,
        liked: bool,
        accepted_unix_millis: u64,
    ) -> Result<PhotoLikeReceipt, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        if submitter_id.is_empty()
            || submitter_id.len() > 128
            || submitter_id.chars().any(char::is_control)
        {
            return Err(ProfileRegistryError::AuthenticationRequired);
        }
        let profile = self.get(profile_id)?;
        if !profile_contains_photo_id(&profile, photo_id) {
            return Err(ProfileRegistryError::NotFound);
        }
        let metadata_path = self
            .root
            .join(profile_id)
            .join("photo-wall")
            .join(format!("photo-{photo_id}.json"));
        if !metadata_path.is_file() {
            return Err(ProfileRegistryError::NotFound);
        }
        let path = self.photo_like_path(profile_id, photo_id, submitter_id);
        if liked {
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let submitter_digest = photo_like_submitter_digest(submitter_id);
                write_json_new(
                    &path,
                    &StoredPhotoLike {
                        schema_version: 1,
                        submitter_digest,
                        liked_unix_millis: accepted_unix_millis,
                    },
                )?;
            }
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(PhotoLikeReceipt {
            schema_version: 1,
            profile_id: profile_id.to_owned(),
            photo_id,
            liked,
            like_count: self.photo_like_count(profile_id, photo_id)?,
        })
    }

    fn photo_like_path(&self, profile_id: &str, photo_id: u32, submitter_id: &str) -> PathBuf {
        self.root
            .join(profile_id)
            .join("photo-wall")
            .join("likes")
            .join(format!("photo-{photo_id}"))
            .join(format!(
                "{}.json",
                photo_like_submitter_digest(submitter_id)
            ))
    }

    fn photo_like_count(
        &self,
        profile_id: &str,
        photo_id: u32,
    ) -> Result<usize, ProfileRegistryError> {
        let directory = self
            .root
            .join(profile_id)
            .join("photo-wall")
            .join("likes")
            .join(format!("photo-{photo_id}"));
        if !directory.is_dir() {
            return Ok(0);
        }
        Ok(std::fs::read_dir(directory)?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count())
    }

    pub fn photo_asset(
        &self,
        profile_id: &str,
        photo_id: u32,
    ) -> Result<PhotoAssetContent, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        if photo_id == 0 {
            return Err(ProfileRegistryError::NotFound);
        }
        let directory = self.root.join(profile_id).join("photo-wall");
        let stored: StoredPhotoAsset =
            read_json(&directory.join(format!("photo-{photo_id}.json")))?;
        if stored.profile_id != profile_id || stored.photo_id != photo_id {
            return Err(ProfileRegistryError::NotFound);
        }
        if !stored_photo_file_name_is_safe(&stored)
            || stored.image_path != format!("/v1/profiles/{profile_id}/photo-wall/{photo_id}")
        {
            return Err(ProfileRegistryError::InvalidPhotoAsset(
                "stored image metadata failed integrity verification".into(),
            ));
        }
        let bytes = std::fs::read(directory.join(&stored.file_name))?;
        if bytes.len() != stored.byte_length
            || format!("{:x}", Sha256::digest(&bytes)) != stored.sha256
            || reviewed_image_format(&bytes).map(|value| value.0)
                != Some(stored.media_type.as_str())
        {
            return Err(ProfileRegistryError::InvalidPhotoAsset(
                "stored image failed integrity verification".into(),
            ));
        }
        Ok(PhotoAssetContent {
            bytes,
            media_type: stored.media_type,
            sha256: stored.sha256,
        })
    }

    pub fn catalog(
        &self,
        character_id: Option<&str>,
    ) -> Result<PublicProfileCatalog, ProfileRegistryError> {
        let mut catalog: PublicProfileCatalog = read_json(&self.root.join("catalog.v1.json"))?;
        if let Some(character_id) = character_id {
            catalog
                .profiles
                .retain(|entry| entry.character_id == character_id);
        }
        Ok(catalog)
    }

    pub fn owned_catalog(
        &self,
        submitter_id: &str,
    ) -> Result<PublicProfileCatalog, ProfileRegistryError> {
        let mut catalog = self.catalog(None)?;
        let mut owned = Vec::new();
        for profile in catalog.profiles {
            let claim = read_optional_json::<ProfileClaim>(
                &self.root.join(&profile.profile_id).join("claim.json"),
            )?;
            if claim.is_some_and(|claim| claim.submitter_id == submitter_id) {
                owned.push(profile);
            }
        }
        catalog.profiles = owned;
        Ok(catalog)
    }

    /// Returns the newest already-claimed profile directory for a stable game
    /// character UID. Geographic region is mutable routing metadata learned
    /// from packets; it must not fork a second claimed character when an early
    /// capture was still labelled `global`/`unknown`.
    fn existing_profile_id_for_character(
        &self,
        character_id: &str,
        submitter_id: &str,
    ) -> Result<Option<String>, ProfileRegistryError> {
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let public_path = entry.path().join("public.json");
            let claim_path = entry.path().join("claim.json");
            if !public_path.is_file() || !claim_path.is_file() {
                continue;
            }
            let public: PublicProfile = read_json(&public_path)?;
            if public.character_id != character_id {
                continue;
            }
            let claim: ProfileClaim = read_json(&claim_path)?;
            if claim.character_id != character_id || claim.submitter_id != submitter_id {
                return Err(ProfileRegistryError::ClaimConflict {
                    character_id: character_id.to_owned(),
                });
            }
            candidates.push((public.updated_unix_millis, public.profile_id));
        }
        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        Ok(candidates
            .into_iter()
            .next()
            .map(|(_, profile_id)| profile_id))
    }

    fn receipt(&self, profile: &PublicProfile, duplicate: bool) -> ProfilePublishReceipt {
        ProfilePublishReceipt {
            schema_version: 1,
            profile_id: profile.profile_id.clone(),
            character_id: profile.character_id.clone(),
            package_id: profile.package_id.clone(),
            claimed: profile.claimed,
            duplicate,
            module_inventory_count: profile.module_inventory_count,
            equipped_module_count: profile.equipped_module_count,
            profile_url: format!(
                "{}/profiles/{}/",
                self.public_site_url, profile.character_id
            ),
        }
    }

    fn rebuild_catalog(&self) -> Result<(), ProfileRegistryError> {
        let mut profiles_by_character = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if profiles_by_character.len() >= MAXIMUM_PROFILE_COUNT {
                return Err(ProfileRegistryError::CatalogTooLarge);
            }
            let path = entry.path().join("public.json");
            if !path.is_file() {
                continue;
            }
            let profile: PublicProfile = read_json(&path)?;
            let candidate = catalog_entry(profile);
            match profiles_by_character.get(&candidate.character_id) {
                Some(existing) if profile_catalog_entry_precedes(existing, &candidate) => {}
                _ => {
                    profiles_by_character.insert(candidate.character_id.clone(), candidate);
                }
            }
        }
        let mut profiles = profiles_by_character.into_values().collect::<Vec<_>>();
        profiles.sort_by(|left, right| {
            right
                .updated_unix_millis
                .cmp(&left.updated_unix_millis)
                .then_with(|| left.profile_id.cmp(&right.profile_id))
        });
        write_json_atomic(
            &self.root.join("catalog.v1.json"),
            &PublicProfileCatalog {
                schema_version: PUBLIC_PROFILE_CATALOG_SCHEMA_VERSION,
                profiles,
            },
        )
    }
}

fn profile_catalog_entry_precedes(
    left: &PublicProfileCatalogEntry,
    right: &PublicProfileCatalogEntry,
) -> bool {
    left.updated_unix_millis > right.updated_unix_millis
        || (left.updated_unix_millis == right.updated_unix_millis
            && left.profile_id <= right.profile_id)
}

fn validate_bpsr_package(
    package: &LocalProfilePackage,
) -> Result<CharacterProfilePatch, ProfileRegistryError> {
    let request = &package.request;
    if request.relative_endpoint != BPSR_PROFILE_ENDPOINT
        || request.payload.game_plugin_id != BPSR_GAME_PLUGIN_ID
        || request.payload.payload_schema_id != BPSR_PROFILE_SCHEMA_ID
        || request.payload.payload_schema_version != BPSR_PROFILE_SCHEMA_VERSION
    {
        return Err(ProfileRegistryError::InvalidPackage(
            "the endpoint or BPSR profile schema identity does not match".into(),
        ));
    }
    let profile: CharacterProfilePatch = serde_json::from_value(request.payload.body.clone())?;
    let routing = &request.payload.routing;
    let character = &profile.character;
    let matches = routing.get("character-id") == Some(&character.character_id)
        && routing.get("deployment") == Some(&character.region.deployment_id)
        && routing.get("region") == Some(&character.region.region_id)
        && routing.get("realm") == character.region.realm_id.as_ref()
        && routing.get("world") == character.region.world_id.as_ref();
    if !matches {
        return Err(ProfileRegistryError::InvalidPackage(
            "the profile body identity does not match its routing identity".into(),
        ));
    }
    Ok(profile)
}

fn refine_profile_routing_identity(
    accumulated: &mut CharacterProfilePatch,
    newer: &CharacterProfilePatch,
) -> Result<(), ProfileRegistryError> {
    if accumulated.character.character_id != newer.character.character_id {
        return Err(ProfileRegistryError::InvalidPackage(
            "one profile accumulator received two character UIDs".into(),
        ));
    }
    // Region/realm/world are routing observations rather than character
    // identity. A later live, device-bound profile may refine them without
    // discarding the previously accumulated character facts.
    accumulated.character = newer.character.clone();
    Ok(())
}

fn preserve_current_photo_assets(
    existing: Option<&PublicProfile>,
    newer: &mut WebsitePayloadEnvelope,
) {
    let Some(assets) = existing
        .and_then(|profile| profile.envelope.body.get("collection_summary"))
        .and_then(|collection| collection.get("photo_assets"))
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let retained = assets
        .iter()
        .filter(|asset| {
            asset
                .get("photo_id")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .is_some_and(|photo_id| profile_body_contains_photo_id(&newer.body, photo_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return;
    }
    if let Some(collection) = newer
        .body
        .get_mut("collection_summary")
        .and_then(serde_json::Value::as_object_mut)
    {
        collection.insert("photo_assets".into(), serde_json::Value::Array(retained));
    }
}

fn prefer_observed_photo_wall_identity(
    observed: &serde_json::Value,
    accumulated: &mut serde_json::Value,
) {
    let Some(observed_collection) = observed
        .get("collection_summary")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let has_observed_photo = observed_collection
        .get("photo_ids")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .any(|value| value.as_u64().is_some_and(|id| id > 0))
        })
        || observed_collection
            .get("photo_wall")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|wall| {
                wall.values()
                    .any(|value| value.as_u64().is_some_and(|id| id > 0))
            });
    if !has_observed_photo {
        return;
    }
    let Some(accumulated_collection) = accumulated
        .get_mut("collection_summary")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for key in ["photo_ids", "photo_wall"] {
        if let Some(value) = observed_collection.get(key) {
            accumulated_collection.insert(key.into(), value.clone());
        }
    }
}

fn profile_contains_photo_id(profile: &PublicProfile, photo_id: u32) -> bool {
    profile_body_contains_photo_id(&profile.envelope.body, photo_id)
}

fn profile_body_contains_photo_id(body: &serde_json::Value, photo_id: u32) -> bool {
    let Some(collection) = body.get("collection_summary") else {
        return false;
    };
    collection
        .get("photo_ids")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|photos| {
            photos
                .iter()
                .any(|value| value.as_u64() == Some(u64::from(photo_id)))
        })
        || collection
            .get("photo_wall")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|wall| {
                wall.values()
                    .any(|value| value.as_u64() == Some(u64::from(photo_id)))
            })
}

fn upsert_public_photo_asset(
    profile: &mut PublicProfile,
    stored: &StoredPhotoAsset,
) -> Result<(), ProfileRegistryError> {
    let collection = profile
        .envelope
        .body
        .get_mut("collection_summary")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(ProfileRegistryError::PhotoNotObserved {
            photo_id: stored.photo_id,
        })?;
    let assets = collection
        .entry("photo_assets")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            ProfileRegistryError::InvalidPhotoAsset(
                "published Photo Wall asset collection is malformed".into(),
            )
        })?;
    assets.retain(|asset| {
        asset.get("photo_id").and_then(serde_json::Value::as_u64)
            != Some(u64::from(stored.photo_id))
    });
    assets.push(serde_json::json!({
        "photo_id": stored.photo_id,
        "image_path": stored.image_path,
        "sha256": stored.sha256,
        "media_type": stored.media_type,
        "byte_length": stored.byte_length,
        "uploaded_unix_millis": stored.uploaded_unix_millis,
    }));
    assets.sort_by_key(|asset| {
        asset
            .get("photo_id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX)
    });
    Ok(())
}

fn photo_like_submitter_digest(submitter_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-photo-like-v1\0");
    hasher.update(submitter_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn file_modified_unix_millis(path: &Path) -> Option<u64> {
    path.metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn reviewed_image_format(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.len() >= 45
        && bytes[..8] == [137, 80, 78, 71, 13, 10, 26, 10]
        && bytes[8..12] == [0, 0, 0, 13]
        && &bytes[12..16] == b"IHDR"
        && bytes.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82])
    {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        if (1..=16_384).contains(&width) && (1..=16_384).contains(&height) {
            return Some(("image/png", "png"));
        }
    }
    if bytes.len() >= 16
        && bytes.starts_with(&[0xff, 0xd8, 0xff])
        && bytes.ends_with(&[0xff, 0xd9])
        && bytes.windows(2).any(|marker| {
            marker[0] == 0xff
                && matches!(marker[1], 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf)
        })
    {
        return Some(("image/jpeg", "jpg"));
    }
    if bytes.len() >= 20 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let declared = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize + 8;
        if declared == bytes.len() && matches!(&bytes[12..16], b"VP8 " | b"VP8L" | b"VP8X") {
            return Some(("image/webp", "webp"));
        }
    }
    None
}

fn stored_photo_file_name_is_safe(stored: &StoredPhotoAsset) -> bool {
    let extension = match stored.media_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => return false,
    };
    stored.sha256.len() == 64
        && stored
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && stored.file_name == format!("photo-{}-{}.{}", stored.photo_id, stored.sha256, extension)
}

fn profile_id(envelope: &WebsitePayloadEnvelope) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-profile-character-identity-v2\0");
    hasher.update(
        envelope
            .routing
            .get("character-id")
            .map_or(&[][..], String::as_bytes),
    );
    format!("prf_{:x}", hasher.finalize())[..36].to_owned()
}

fn validate_profile_id(value: &str) -> Result<(), ProfileRegistryError> {
    if value.len() != 36
        || !value.starts_with("prf_")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ProfileRegistryError::NotFound);
    }
    Ok(())
}

fn catalog_entry(profile: PublicProfile) -> PublicProfileCatalogEntry {
    PublicProfileCatalogEntry {
        profile_id: profile.profile_id,
        claimed: profile.claimed,
        package_id: profile.package_id,
        updated_unix_millis: profile.updated_unix_millis,
        source_client_build: profile.source_client_build,
        deployment: profile.deployment,
        region: profile.region,
        realm: profile.realm,
        world: profile.world,
        character_id: profile.character_id,
        display_name: profile.display_name,
        module_inventory_count: profile.module_inventory_count,
        equipped_module_count: profile.equipped_module_count,
    }
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, ProfileRegistryError> {
    match File::open(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(Some(serde_json::from_slice(&bytes)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProfileRegistryError> {
    read_optional_json(path)?.ok_or(ProfileRegistryError::NotFound)
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), ProfileRegistryError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_bytes_new(path: &Path, bytes: &[u8]) -> Result<(), ProfileRegistryError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), ProfileRegistryError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let partial = path.with_extension(format!("partial-{}", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        #[cfg(windows)]
        std::fs::remove_file(path)?;
    }
    std::fs::rename(partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rlogs_events::{CharacterIdentity, RegionIdentity};
    use rlogs_profiles::ProfilePackageSource;
    use rlogs_submission::WebsitePayloadRequest;

    use super::*;

    const TEST_DEVICE_ID: &str = "dev_test";
    const TEST_DEVICE_TOKEN: &str = "rld_test-secret";

    fn package(created: u64, character_id: &str, modules: usize) -> LocalProfilePackage {
        package_with_photos(created, character_id, modules, &[])
    }

    fn package_with_photos(
        created: u64,
        character_id: &str,
        modules: usize,
        photo_ids: &[i64],
    ) -> LocalProfilePackage {
        let profile = CharacterProfilePatch {
            character: CharacterIdentity {
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: Some("asteria".into()),
                    world_id: None,
                },
                character_id: character_id.into(),
            },
            display_name: Some("Example".into()),
            display_id: None,
            server_id: None,
            class_id: Some(2),
            specialization_id: Some(1),
            level: Some(60),
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            combat_stats: None,
            season_strength: None,
            master_score: None,
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: Some(rlogs_game_bpsr::ModuleProfile {
                equipped_slots: BTreeMap::new(),
                inventory: (0..modules)
                    .map(|index| rlogs_game_bpsr::ModuleItemProfile {
                        instance_id: format!("module-{index}"),
                        config_id: 20_001,
                        count: Some(1),
                        quality: Some(5),
                        load_flag: None,
                        module_type: Some(1),
                        level: Some(1),
                        parts: Vec::new(),
                        upgrade_records: Vec::new(),
                        success_rate: None,
                    })
                    .collect(),
            }),
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions: None,
            life_professions: None,
            cosmetics: None,
            collection_summary: (!photo_ids.is_empty()).then(|| {
                rlogs_game_bpsr::CollectionSummary {
                    observed_sections: rlogs_game_bpsr::CollectionObservationSections {
                        personal_zone: true,
                        ..Default::default()
                    },
                    fashion_points: None,
                    mount_points: None,
                    weapon_skin_points: None,
                    equipped_fashion_ids: BTreeMap::new(),
                    owned_fashion_ids: Vec::new(),
                    owned_mount_ids: Vec::new(),
                    owned_weapon_skin_ids: Vec::new(),
                    owned_dye_ids: Vec::new(),
                    unlocked_module_ids: Vec::new(),
                    ride_ids: Vec::new(),
                    ride_skin_ids: Vec::new(),
                    unlocked_emoji_ids: Vec::new(),
                    vanity_pet_ids: Vec::new(),
                    summoned_vanity_pet_id: None,
                    fantasy_atlas_stages: BTreeMap::new(),
                    handbook: None,
                    photo_ids: photo_ids.to_vec(),
                    photo_wall: photo_ids
                        .iter()
                        .enumerate()
                        .map(|(index, photo_id)| (i32::try_from(index).unwrap(), *photo_id))
                        .collect(),
                    achievements: None,
                }
            }),
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            profession_projects: None,
            social_display: None,
        };
        let request = rlogs_game_bpsr::website_profile_request(&profile).unwrap();
        let mut package = LocalProfilePackage::new(
            created,
            ProfilePackageSource {
                session_id: "session-1".into(),
                client_build: "steam-24687926".into(),
                protocol_pack_digest: "sha256:pack".into(),
                canonical_content_sha256: format!("sha256:{}", "a".repeat(64)),
                observation_count: 2,
                last_event_sequence: 9,
                live_capture: None,
            },
            request,
        )
        .unwrap();
        package
            .bind_live_capture(TEST_DEVICE_ID, TEST_DEVICE_TOKEN)
            .unwrap();
        package
    }

    fn mutate_package(
        package: LocalProfilePackage,
        update: impl FnOnce(&mut CharacterProfilePatch),
    ) -> LocalProfilePackage {
        let created_unix_millis = package.created_unix_millis;
        let mut source = package.source;
        source.live_capture = None;
        let mut profile: CharacterProfilePatch =
            serde_json::from_value(package.request.payload.body).unwrap();
        update(&mut profile);
        let request = website_profile_request(&profile).unwrap();
        let mut rebuilt = LocalProfilePackage::new(created_unix_millis, source, request).unwrap();
        rebuilt
            .bind_live_capture(TEST_DEVICE_ID, TEST_DEVICE_TOKEN)
            .unwrap();
        rebuilt
    }

    fn one_pixel_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99,
            0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn first_authenticated_package_claims_uid_and_preserves_modules() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let receipt = registry
            .publish(
                package(10, "1000001", 3),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        assert!(receipt.claimed);
        assert_eq!(receipt.module_inventory_count, 3);
        assert_eq!(receipt.profile_url, "https://site.test/profiles/1000001/");
        let published = registry.get(&receipt.profile_id).unwrap();
        assert_eq!(published.character_id, "1000001");
        assert_eq!(
            published.envelope.body["modules"]["inventory"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn verified_saved_projects_are_published_as_isolated_loadout_snapshots() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = mutate_package(package(10, "1000001", 3), |profile| {
            profile.current_profession_project_id = Some(5);
            profile.profession_projects = Some(vec![
                rlogs_game_bpsr::ProfessionProjectProfile {
                    project_id: 5,
                    project_name: "Daily".into(),
                    profession_id: Some(11),
                },
                rlogs_game_bpsr::ProfessionProjectProfile {
                    project_id: 8,
                    project_name: "Bossing".into(),
                    profession_id: Some(4),
                },
            ]);
            profile.class_id = Some(11);
            profile.specialization_id = Some(2);
        });
        let receipt = registry
            .publish(
                first,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let directory_only = registry.get(&receipt.profile_id).unwrap();
        assert_eq!(directory_only.loadouts.len(), 2);
        assert_eq!(
            directory_only.loadouts[1].project_name.as_deref(),
            Some("Bossing")
        );
        assert!(!directory_only.loadouts[1].snapshot_available);
        let second = mutate_package(package(30, "1000001", 7), |profile| {
            profile.current_profession_project_id = Some(8);
            profile.class_id = Some(4);
            profile.specialization_id = Some(1);
        });
        registry
            .publish(
                second,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            )
            .unwrap();

        let public = registry.get(&receipt.profile_id).unwrap();
        assert_eq!(
            public
                .loadouts
                .iter()
                .map(|loadout| loadout.project_id)
                .collect::<Vec<_>>(),
            vec![5, 8]
        );
        assert_eq!(public.loadouts[0].project_name.as_deref(), Some("Daily"));
        assert!(public.loadouts[0].snapshot_available);
        assert_eq!(public.loadouts[1].project_name.as_deref(), Some("Bossing"));
        assert!(public.loadouts[1].snapshot_available);
        let loadout_five = registry.get_loadout(&receipt.profile_id, 5).unwrap();
        let loadout_eight = registry.get_loadout(&receipt.profile_id, 8).unwrap();
        assert_eq!(loadout_five.class_id, Some(11));
        assert_eq!(loadout_five.specialization_id, Some(2));
        assert_eq!(loadout_five.module_inventory_count, 3);
        assert_eq!(
            loadout_five.envelope.body["current_profession_project_id"],
            5
        );
        assert_eq!(loadout_eight.class_id, Some(4));
        assert_eq!(loadout_eight.specialization_id, Some(1));
        assert_eq!(loadout_eight.module_inventory_count, 7);
        assert_eq!(
            loadout_eight.envelope.body["current_profession_project_id"],
            8
        );
    }

    #[test]
    fn later_duplicate_sync_backfills_loadout_storage_after_server_upgrade() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let package_at = |created| {
            mutate_package(package(created, "1000001", 3), |profile| {
                profile.current_profession_project_id = Some(5);
                profile.class_id = Some(11);
                profile.specialization_id = Some(117);
            })
        };
        let receipt = registry
            .publish(
                package_at(10),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();

        let profile_directory = root.path().join(&receipt.profile_id);
        std::fs::remove_file(profile_directory.join("loadouts").join("5.json")).unwrap();
        let mut legacy_public = registry.get(&receipt.profile_id).unwrap();
        legacy_public.loadouts.clear();
        write_json_atomic(&profile_directory.join("public.json"), &legacy_public).unwrap();

        let refreshed = registry
            .publish(
                package_at(30),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            )
            .unwrap();
        assert!(refreshed.duplicate);
        assert_eq!(registry.get(&receipt.profile_id).unwrap().loadouts.len(), 1);
        assert_eq!(
            registry
                .get_loadout(&receipt.profile_id, 5)
                .unwrap()
                .module_inventory_count,
            3
        );
    }

    #[test]
    fn newer_sparse_live_package_preserves_prior_verified_profile_fields() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = mutate_package(package(10, "1000001", 3), |profile| {
            profile.social_display = Some(rlogs_game_bpsr::SocialDisplay {
                guild_id: Some(77_088),
                guild_name: Some("Sheep".into()),
                equipped_title_id: None,
                equipped_title_level: None,
                title_ids: vec![9_060_001],
                medal_ids: Vec::new(),
                medal_slots: BTreeMap::new(),
                profile_theme_id: None,
            });
        });
        let first = registry
            .publish(
                first,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();

        let second = mutate_package(package(30, "1000001", 0), |profile| {
            profile.display_name = None;
            profile.modules = None;
            profile.social_display = Some(rlogs_game_bpsr::SocialDisplay {
                guild_id: None,
                guild_name: None,
                equipped_title_id: Some(9_061_163),
                equipped_title_level: Some(4),
                title_ids: vec![9_061_163],
                medal_ids: Vec::new(),
                medal_slots: BTreeMap::new(),
                profile_theme_id: None,
            });
        });
        let second = registry
            .publish(
                second,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            )
            .unwrap();

        assert_eq!(first.profile_id, second.profile_id);
        assert!(!second.duplicate);
        assert_eq!(second.module_inventory_count, 3);
        let public = registry.get(&second.profile_id).unwrap();
        assert_eq!(public.display_name.as_deref(), Some("Example"));
        assert_eq!(public.envelope.body["social_display"]["guild_id"], 77_088);
        assert_eq!(
            public.envelope.body["social_display"]["guild_name"],
            "Sheep"
        );
        assert_eq!(
            public.envelope.body["social_display"]["title_ids"],
            serde_json::json!([9_060_001, 9_061_163])
        );
        assert_eq!(
            public.envelope.body["social_display"]["equipped_title_id"],
            9_061_163
        );
    }

    #[test]
    fn an_identical_profile_from_a_later_live_session_refreshes_last_seen() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = registry
            .publish(
                package(10, "1000001", 3),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let refreshed = registry
            .publish(
                package(30, "1000001", 3),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            )
            .unwrap();
        assert_eq!(first.package_id, refreshed.package_id);
        assert!(refreshed.duplicate);
        let public = registry.get(&refreshed.profile_id).unwrap();
        assert_eq!(public.created_unix_millis, 30);
        assert_eq!(public.updated_unix_millis, 40);
    }

    #[test]
    fn packet_resolved_region_refines_one_stable_claim_instead_of_forking_it() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let initially_unresolved = mutate_package(package(10, "1000001", 1), |profile| {
            profile.character.region.region_id = "global".into();
            profile.character.region.realm_id = None;
        });
        let first = registry
            .publish(
                initially_unresolved,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let refined = registry
            .publish(
                package(30, "1000001", 2),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            )
            .unwrap();

        assert_eq!(first.profile_id, refined.profile_id);
        let catalog = registry.catalog(None).unwrap();
        assert_eq!(catalog.profiles.len(), 1);
        assert_eq!(catalog.profiles[0].region, "north-america");
    }

    #[test]
    fn a_second_account_cannot_take_over_claimed_uid() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        registry
            .publish(
                package(10, "1000001", 1),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let error = registry
            .publish(
                package(11, "1000001", 2),
                Some("user-two"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                30,
            )
            .unwrap_err();
        assert!(matches!(error, ProfileRegistryError::ClaimConflict { .. }));
    }

    #[test]
    fn account_catalog_contains_only_that_accounts_claimed_uids() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        registry
            .publish(
                package(10, "1000001", 1),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        registry
            .publish(
                package(11, "2000002", 2),
                Some("user-two"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                30,
            )
            .unwrap();

        let owned = registry.owned_catalog("user-one").unwrap();
        assert_eq!(owned.profiles.len(), 1);
        assert_eq!(owned.profiles[0].character_id, "1000001");
    }

    #[test]
    fn same_owner_can_publish_newer_state_but_not_roll_back() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = registry
            .publish(
                package(10, "1000001", 1),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let second = registry
            .publish(
                package(11, "1000001", 4),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                30,
            )
            .unwrap();
        assert_eq!(first.profile_id, second.profile_id);
        assert_eq!(
            registry
                .get(&second.profile_id)
                .unwrap()
                .module_inventory_count,
            4
        );
        assert!(matches!(
            registry.publish(
                package(9, "1000001", 2),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                40,
            ),
            Err(ProfileRegistryError::StalePackage)
        ));
    }

    #[test]
    fn copied_or_unbound_packages_cannot_claim_a_uid() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();

        let copied = registry
            .publish(
                package(10, "1000001", 1),
                Some("user-two"),
                Some("dev_other"),
                "rld_other-secret",
                20,
            )
            .unwrap_err();
        assert!(matches!(copied, ProfileRegistryError::InvalidPackage(_)));

        let mut unbound = package(11, "1000001", 1);
        unbound.source.live_capture = None;
        let unbound = registry
            .publish(
                unbound,
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                30,
            )
            .unwrap_err();
        assert!(matches!(unbound, ProfileRegistryError::InvalidPackage(_)));
    }

    #[test]
    fn routing_tampering_is_rejected_even_with_a_resealed_request() {
        let mut package = package(10, "1000001", 1);
        package
            .request
            .payload
            .routing
            .insert("character-id".into(), "other".into());
        package = LocalProfilePackage::new(
            package.created_unix_millis,
            package.source,
            WebsitePayloadRequest::new(BPSR_PROFILE_ENDPOINT, package.request.payload).unwrap(),
        )
        .unwrap();
        let error = validate_bpsr_package(&package).unwrap_err();
        assert!(matches!(error, ProfileRegistryError::InvalidPackage(_)));
    }

    #[test]
    fn claimed_owner_can_publish_and_read_an_observed_photo() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let published = registry
            .publish(
                package_with_photos(10, "1000001", 1, &[42]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let png = one_pixel_png();
        let receipt = registry
            .publish_photo_asset(&published.profile_id, 42, &png, Some("user-one"), 30)
            .unwrap();
        assert_eq!(receipt.media_type, "image/png");
        assert_eq!(
            receipt.image_path,
            format!("/v1/profiles/{}/photo-wall/42", published.profile_id)
        );
        let public = registry.get(&published.profile_id).unwrap();
        let asset = &public.envelope.body["collection_summary"]["photo_assets"][0];
        assert_eq!(asset["photo_id"], 42);
        assert_eq!(asset["image_path"], receipt.image_path);
        assert!(asset.get("source_url").is_none());
        let loaded = registry.photo_asset(&published.profile_id, 42).unwrap();
        assert_eq!(loaded.bytes, png);
        assert_eq!(loaded.sha256, receipt.sha256);
    }

    #[test]
    fn photo_feed_uses_asset_upload_time_and_idempotent_account_likes() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = registry
            .publish(
                package_with_photos(10, "1000001", 1, &[42]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let second = registry
            .publish(
                package_with_photos(11, "2000002", 1, &[84]),
                Some("user-two"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                21,
            )
            .unwrap();
        let png = one_pixel_png();
        registry
            .publish_photo_asset(&first.profile_id, 42, &png, Some("user-one"), 30)
            .unwrap();
        registry
            .publish_photo_asset(&second.profile_id, 84, &png, Some("user-two"), 40)
            .unwrap();

        let newest = registry
            .photo_catalog(
                &PhotoCatalogQuery {
                    sort: Some(PhotoCatalogSort::Newest),
                    limit: None,
                },
                None,
            )
            .unwrap();
        assert_eq!(newest.total_entries, 2);
        assert_eq!(newest.entries[0].photo_id, 84);
        assert_eq!(newest.entries[1].uploaded_unix_millis, 30);

        let liked = registry
            .set_photo_like(&first.profile_id, 42, "viewer-one", true, 50)
            .unwrap();
        assert_eq!(liked.like_count, 1);
        let duplicate = registry
            .set_photo_like(&first.profile_id, 42, "viewer-one", true, 51)
            .unwrap();
        assert_eq!(duplicate.like_count, 1);
        registry
            .set_photo_like(&first.profile_id, 42, "viewer-two", true, 52)
            .unwrap();

        let popular = registry
            .photo_catalog(
                &PhotoCatalogQuery {
                    sort: Some(PhotoCatalogSort::Popular),
                    limit: Some(1),
                },
                Some("viewer-one"),
            )
            .unwrap();
        assert_eq!(popular.total_entries, 2);
        assert_eq!(popular.entries.len(), 1);
        assert_eq!(popular.entries[0].photo_id, 42);
        assert_eq!(popular.entries[0].like_count, 2);
        assert!(popular.entries[0].viewer_liked);

        let unliked = registry
            .set_photo_like(&first.profile_id, 42, "viewer-one", false, 60)
            .unwrap();
        assert_eq!(unliked.like_count, 1);
        assert!(!unliked.liked);
    }

    #[test]
    fn photo_publication_rejects_other_owners_unobserved_ids_and_non_rasters() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let published = registry
            .publish(
                package_with_photos(10, "1000001", 1, &[42]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        let png = one_pixel_png();
        assert!(matches!(
            registry.publish_photo_asset(&published.profile_id, 42, &png, Some("user-two"), 30),
            Err(ProfileRegistryError::ClaimConflict { .. })
        ));
        assert!(matches!(
            registry.publish_photo_asset(&published.profile_id, 99, &png, Some("user-one"), 30),
            Err(ProfileRegistryError::PhotoNotObserved { photo_id: 99 })
        ));
        assert!(matches!(
            registry.publish_photo_asset(
                &published.profile_id,
                42,
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                Some("user-one"),
                30,
            ),
            Err(ProfileRegistryError::InvalidPhotoAsset(_))
        ));
    }

    #[test]
    fn later_profile_sync_preserves_only_still_observed_photo_assets() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = registry
            .publish(
                package_with_photos(10, "1000001", 1, &[42]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                20,
            )
            .unwrap();
        registry
            .publish_photo_asset(
                &first.profile_id,
                42,
                &one_pixel_png(),
                Some("user-one"),
                30,
            )
            .unwrap();
        registry
            .publish(
                package_with_photos(40, "1000001", 2, &[42]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                50,
            )
            .unwrap();
        assert_eq!(
            registry.get(&first.profile_id).unwrap().envelope.body["collection_summary"]
                ["photo_assets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        registry
            .publish(
                package_with_photos(60, "1000001", 2, &[43]),
                Some("user-one"),
                Some(TEST_DEVICE_ID),
                TEST_DEVICE_TOKEN,
                70,
            )
            .unwrap();
        assert!(
            registry.get(&first.profile_id).unwrap().envelope.body["collection_summary"]
                .get("photo_assets")
                .is_none()
        );
    }
}
