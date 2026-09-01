use std::collections::BTreeMap;

use rlogs_submission::{WebsitePayloadEnvelope, WebsitePayloadError, WebsitePayloadRequest};
use thiserror::Error;

use crate::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION, CharacterProfilePatch,
};

pub const BPSR_PROFILE_ENDPOINT: &str = "/v1/games/blue-protocol-star-resonance/profiles";

/// Projects a decoded BPSR character observation into Core's game-neutral
/// website request. The host supplies the website base URL and authentication.
pub fn website_profile_request(
    profile: &CharacterProfilePatch,
) -> Result<WebsitePayloadRequest, BpsrWebsiteProfileError> {
    let mut routing = BTreeMap::from([
        (
            "deployment".into(),
            profile.character.region.deployment_id.clone(),
        ),
        ("region".into(), profile.character.region.region_id.clone()),
        (
            "character-id".into(),
            profile.character.character_id.clone(),
        ),
    ]);
    if let Some(realm) = &profile.character.region.realm_id {
        routing.insert("realm".into(), realm.clone());
    }
    if let Some(world) = &profile.character.region.world_id {
        routing.insert("world".into(), world.clone());
    }

    let mut body = serde_json::to_value(profile)?;
    // Collection subsection provenance is required for correct local merging,
    // but is an implementation detail rather than public profile content.
    if let Some(collection) = body
        .get_mut("collection_summary")
        .and_then(serde_json::Value::as_object_mut)
    {
        collection.remove("observed_sections");
    }
    let payload = WebsitePayloadEnvelope::new(
        BPSR_GAME_PLUGIN_ID,
        "character-profile",
        BPSR_PROFILE_SCHEMA_ID,
        BPSR_PROFILE_SCHEMA_VERSION,
        routing,
        body,
    )?;
    Ok(WebsitePayloadRequest::new(BPSR_PROFILE_ENDPOINT, payload)?)
}

#[derive(Debug, Error)]
pub enum BpsrWebsiteProfileError {
    #[error("could not encode the BPSR character profile: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("BPSR character profile failed website payload validation: {0}")]
    Validation(#[from] WebsitePayloadError),
}

#[cfg(test)]
mod tests {
    use rlogs_events::{CharacterIdentity, RegionIdentity};

    use super::*;

    #[test]
    fn region_and_public_character_id_are_explicit_routing_fields() {
        let profile = CharacterProfilePatch {
            character: CharacterIdentity {
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: Some("world-7".into()),
                },
                character_id: "public-character-123".into(),
            },
            display_name: Some("Example".into()),
            display_id: None,
            server_id: Some("7".into()),
            class_id: Some(3),
            specialization_id: Some(2),
            level: Some(60),
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            season_strength: None,
            master_score: None,
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: Some(crate::ModuleProfile {
                equipped_slots: [(1, "9007199254740993".into())].into_iter().collect(),
                inventory: vec![crate::ModuleItemProfile {
                    instance_id: "9007199254740993".into(),
                    config_id: 20_001,
                    count: Some(1),
                    quality: Some(5),
                    load_flag: Some(1),
                    module_type: Some(2),
                    level: Some(4),
                    parts: vec![crate::ModulePartProfile {
                        part_id: 301,
                        initial_link_points: Some(2),
                    }],
                    upgrade_records: vec![crate::ModuleUpgradeRecord {
                        part_id: 301,
                        succeeded: Some(true),
                    }],
                    success_rate: Some(75),
                }],
            }),
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: Some(crate::TalentProgressProfile {
                total_points: Some(10),
                total_reset_count: Some(1),
            }),
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

        let request = website_profile_request(&profile).unwrap();
        assert_eq!(request.relative_endpoint, BPSR_PROFILE_ENDPOINT);
        assert_eq!(request.payload.routing.get("deployment").unwrap(), "global");
        assert_eq!(
            request.payload.routing.get("character-id").unwrap(),
            "public-character-123"
        );
        assert_eq!(
            request.payload.body["modules"]["inventory"][0]["instance_id"],
            "9007199254740993"
        );
        assert_eq!(request.payload.body["talent_progress"]["total_points"], 10);
        assert!(request.payload.body.get("account_id").is_none());
    }
}
