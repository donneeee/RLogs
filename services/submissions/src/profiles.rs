use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_ENDPOINT, BPSR_PROFILE_SCHEMA_ID,
    BPSR_PROFILE_SCHEMA_VERSION, CharacterProfilePatch,
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
    pub envelope: WebsitePayloadEnvelope,
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
        accepted_unix_millis: u64,
    ) -> Result<ProfilePublishReceipt, ProfileRegistryError> {
        let submitter_id = submitter_id.ok_or(ProfileRegistryError::AuthenticationRequired)?;
        package
            .validate()
            .map_err(|error| ProfileRegistryError::InvalidPackage(error.to_string()))?;
        let profile = validate_bpsr_package(&package)?;
        let profile_id = profile_id(&package.request.payload);
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
        if let Some(existing) = &existing_profile {
            if existing.package_id == package.package_id {
                return Ok(self.receipt(existing, true));
            }
            if package.created_unix_millis <= existing.created_unix_millis {
                return Err(ProfileRegistryError::StalePackage);
            }
        }

        let claim = existing_claim.unwrap_or(ProfileClaim {
            schema_version: PROFILE_CLAIM_SCHEMA_VERSION,
            profile_id: profile_id.clone(),
            submitter_id: submitter_id.to_owned(),
            character_id: profile.character.character_id.clone(),
            claimed_unix_millis: accepted_unix_millis,
        });
        let modules = profile.modules.as_ref();
        let published = PublicProfile {
            schema_version: PUBLIC_PROFILE_SCHEMA_VERSION,
            profile_id,
            claimed: true,
            package_id: package.package_id.clone(),
            created_unix_millis: package.created_unix_millis,
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
            envelope: package.request.payload.clone(),
        };

        // The claim is written first and never replaced by a later submission.
        // A crash can therefore leave a claimed-but-unpublished UID, but can
        // never make an already claimed UID available to a second account.
        if !claim_path.exists() {
            write_json_new(&claim_path, &claim)?;
        }
        write_json_atomic(&package_path, &package)?;
        write_json_atomic(&current_path, &published)?;
        self.rebuild_catalog()?;
        Ok(self.receipt(&published, false))
    }

    pub fn get(&self, profile_id: &str) -> Result<PublicProfile, ProfileRegistryError> {
        validate_profile_id(profile_id)?;
        read_json(&self.root.join(profile_id).join("public.json"))
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
                "{}/?profile={}#profile-lab",
                self.public_site_url, profile.profile_id
            ),
        }
    }

    fn rebuild_catalog(&self) -> Result<(), ProfileRegistryError> {
        let mut profiles = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if profiles.len() >= MAXIMUM_PROFILE_COUNT {
                return Err(ProfileRegistryError::CatalogTooLarge);
            }
            let path = entry.path().join("public.json");
            if !path.is_file() {
                continue;
            }
            let profile: PublicProfile = read_json(&path)?;
            profiles.push(catalog_entry(profile));
        }
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

fn profile_id(envelope: &WebsitePayloadEnvelope) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-profile-identity-v1\0");
    for key in ["deployment", "region", "realm", "world", "character-id"] {
        hasher.update(key.as_bytes());
        hasher.update(b"\0");
        if let Some(value) = envelope.routing.get(key) {
            hasher.update(value.as_bytes());
        }
        hasher.update(b"\0");
    }
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

    fn package(created: u64, character_id: &str, modules: usize) -> LocalProfilePackage {
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
            season_strength: None,
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
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };
        let request = rlogs_game_bpsr::website_profile_request(&profile).unwrap();
        LocalProfilePackage::new(
            created,
            ProfilePackageSource {
                session_id: "session-1".into(),
                client_build: "steam-24687926".into(),
                protocol_pack_digest: "sha256:pack".into(),
                canonical_content_sha256: format!("sha256:{}", "a".repeat(64)),
                observation_count: 2,
                last_event_sequence: 9,
            },
            request,
        )
        .unwrap()
    }

    #[test]
    fn first_authenticated_package_claims_uid_and_preserves_modules() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let receipt = registry
            .publish(package(10, "1000001", 3), Some("user-one"), 20)
            .unwrap();
        assert!(receipt.claimed);
        assert_eq!(receipt.module_inventory_count, 3);
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
    fn a_second_account_cannot_take_over_claimed_uid() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        registry
            .publish(package(10, "1000001", 1), Some("user-one"), 20)
            .unwrap();
        let error = registry
            .publish(package(11, "1000001", 2), Some("user-two"), 30)
            .unwrap_err();
        assert!(matches!(error, ProfileRegistryError::ClaimConflict { .. }));
    }

    #[test]
    fn same_owner_can_publish_newer_state_but_not_roll_back() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            ProfileRegistry::open(root.path().into(), "https://site.test".into()).unwrap();
        let first = registry
            .publish(package(10, "1000001", 1), Some("user-one"), 20)
            .unwrap();
        let second = registry
            .publish(package(11, "1000001", 4), Some("user-one"), 30)
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
            registry.publish(package(9, "1000001", 2), Some("user-one"), 40),
            Err(ProfileRegistryError::StalePackage)
        ));
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
}
