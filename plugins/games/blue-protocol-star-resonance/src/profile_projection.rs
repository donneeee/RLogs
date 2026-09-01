use std::io::BufRead;

use rlogs_events::{CanonicalEvent, EventEnvelope, EventSensitivity};
use rlogs_log_format::{RlogError, RlogLimits, RlogReader};
use rlogs_profiles::{LocalProfilePackage, ProfilePackageError, ProfilePackageSource};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AchievementProgress, AchievementProgressProfile, BpsrWebsiteProfileError,
    CharacterProfilePatch, CharacterProgression, CollectionSummary, HandbookProgress,
    ProfileEventError, SeasonAchievementProgress, SeasonProfile, SocialDisplay,
    website_profile_request,
};

pub const MAXIMUM_LOCAL_PROFILE_CHARACTERS: usize = 8;
const MAXIMUM_PENDING_PUBLIC_PROFILE_CHARACTERS: usize = 64;

/// Applies a newer privacy-reviewed profile patch to an accumulated profile.
///
/// This is also used by the submission registry when a verified live package
/// follows an earlier verified package for the same character. Keeping that
/// boundary on the plug-in's canonical merge rules prevents sparse sessions
/// from erasing fields that their packets did not carry.
pub fn merge_profile_patches(
    accumulated: &mut CharacterProfilePatch,
    newer: CharacterProfilePatch,
) -> Result<(), BpsrProfileProjectionError> {
    accumulated.merge_from(newer)
}

/// Incrementally accumulates the local player's privacy-reviewed profile while
/// continuous capture is running. A character identity must first be observed
/// in a `PersonalGameplay` event. Later public social snapshots may enrich only
/// that already-proven identity; profiles belonging solely to teammates or
/// social lookups can never become claimable packages.
#[derive(Debug, Default)]
pub struct LiveProfileProjection {
    accumulators: Vec<ProfileAccumulator>,
    pending_public: Vec<CharacterProfilePatch>,
}

impl LiveProfileProjection {
    /// Merges one canonical observation and reports whether a claimable local
    /// profile changed.
    pub fn observe(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<bool, BpsrProfileProjectionError> {
        let CanonicalEvent::CharacterProfileObserved { profile } = &envelope.event else {
            return Ok(false);
        };
        let patch = CharacterProfilePatch::from_game_event(profile)?;
        if envelope.sensitivity == EventSensitivity::PersonalGameplay {
            if let Some(existing) = self
                .accumulators
                .iter_mut()
                .find(|existing| existing.profile.character == patch.character)
            {
                let before = existing.profile.clone();
                existing.profile.merge_from(patch)?;
                existing.observation_count = existing
                    .observation_count
                    .checked_add(1)
                    .ok_or(BpsrProfileProjectionError::ObservationOverflow)?;
                existing.last_event_sequence = envelope.sequence;
                return Ok(existing.profile != before);
            }
            if self.accumulators.len() >= MAXIMUM_LOCAL_PROFILE_CHARACTERS {
                return Err(BpsrProfileProjectionError::TooManyLocalCharacters {
                    maximum: MAXIMUM_LOCAL_PROFILE_CHARACTERS,
                });
            }
            let mut patch = patch;
            if let Some(index) = self
                .pending_public
                .iter()
                .position(|pending| pending.character == patch.character)
            {
                patch.merge_public_from(self.pending_public.swap_remove(index))?;
            }
            self.accumulators.push(ProfileAccumulator {
                profile: patch,
                observation_count: 1,
                last_event_sequence: envelope.sequence,
            });
            return Ok(true);
        }

        if envelope.sensitivity == EventSensitivity::PublicGameplay
            && let Some(existing) = self
                .accumulators
                .iter_mut()
                .find(|existing| existing.profile.character == patch.character)
        {
            let before = existing.profile.clone();
            existing.profile.merge_public_from(patch)?;
            existing.observation_count = existing
                .observation_count
                .checked_add(1)
                .ok_or(BpsrProfileProjectionError::ObservationOverflow)?;
            existing.last_event_sequence = envelope.sequence;
            return Ok(existing.profile != before);
        }
        if envelope.sensitivity == EventSensitivity::PublicGameplay {
            if let Some(existing) = self
                .pending_public
                .iter_mut()
                .find(|existing| existing.character == patch.character)
            {
                existing.merge_public_from(patch)?;
            } else {
                if self.pending_public.len() >= MAXIMUM_PENDING_PUBLIC_PROFILE_CHARACTERS {
                    self.pending_public.remove(0);
                }
                self.pending_public.push(patch);
            }
        }
        Ok(false)
    }

    /// Builds reviewable packages from the current live state without waiting
    /// for a dungeon log to seal. The digest covers the canonical merged BPSR
    /// profile body and its observation ledger; transport binding still adds
    /// the device-token HMAC before publication.
    pub fn packages(
        &self,
        session_id: &str,
        client_build: &str,
        protocol_pack_digest: &str,
        created_unix_millis: u64,
    ) -> Result<Vec<LocalProfilePackage>, BpsrProfileProjectionError> {
        let mut packages = Vec::with_capacity(self.accumulators.len());
        for accumulator in &self.accumulators {
            let request = website_profile_request(&accumulator.profile)?;
            let digest_input = serde_json::to_vec(&(
                &accumulator.profile,
                accumulator.observation_count,
                accumulator.last_event_sequence,
            ))?;
            packages.push(LocalProfilePackage::new(
                created_unix_millis,
                ProfilePackageSource {
                    session_id: session_id.to_owned(),
                    client_build: client_build.to_owned(),
                    protocol_pack_digest: protocol_pack_digest.to_owned(),
                    canonical_content_sha256: format!("sha256:{:x}", Sha256::digest(digest_input)),
                    observation_count: accumulator.observation_count,
                    last_event_sequence: accumulator.last_event_sequence,
                    live_capture: None,
                },
                request,
            )?);
        }
        Ok(packages)
    }
}

/// Replays one sealed canonical log and projects only personal-gameplay BPSR
/// character observations. Public social lookups for other characters are
/// deliberately excluded.
pub fn project_local_profile_packages<R: BufRead>(
    input: R,
    limits: RlogLimits,
    created_unix_millis: u64,
) -> Result<Vec<LocalProfilePackage>, BpsrProfileProjectionError> {
    let mut reader = RlogReader::new(input, limits)?;
    let header = reader.header().clone();
    let mut accumulators: Vec<ProfileAccumulator> = Vec::new();

    while let Some(envelope) = reader.next_event()? {
        observe_profile(&mut accumulators, &envelope)?;
    }
    let summary = reader
        .summary()
        .ok_or(BpsrProfileProjectionError::MissingSeal)?;
    let mut packages = Vec::with_capacity(accumulators.len());
    for accumulator in accumulators {
        let request = website_profile_request(&accumulator.profile)?;
        packages.push(LocalProfilePackage::new(
            created_unix_millis,
            ProfilePackageSource {
                session_id: header.session_id.clone(),
                client_build: header.region.client_build.clone(),
                protocol_pack_digest: header.region.protocol_pack_digest.clone(),
                canonical_content_sha256: summary.content_sha256.clone(),
                observation_count: accumulator.observation_count,
                last_event_sequence: accumulator.last_event_sequence,
                live_capture: None,
            },
            request,
        )?);
    }
    Ok(packages)
}

fn observe_profile(
    accumulators: &mut Vec<ProfileAccumulator>,
    envelope: &EventEnvelope,
) -> Result<(), BpsrProfileProjectionError> {
    if envelope.sensitivity != EventSensitivity::PersonalGameplay {
        return Ok(());
    }
    let CanonicalEvent::CharacterProfileObserved { profile } = &envelope.event else {
        return Ok(());
    };
    let patch = CharacterProfilePatch::from_game_event(profile)?;
    if let Some(existing) = accumulators
        .iter_mut()
        .find(|existing| existing.profile.character == patch.character)
    {
        existing.profile.merge_from(patch)?;
        existing.observation_count = existing
            .observation_count
            .checked_add(1)
            .ok_or(BpsrProfileProjectionError::ObservationOverflow)?;
        existing.last_event_sequence = envelope.sequence;
        return Ok(());
    }
    if accumulators.len() >= MAXIMUM_LOCAL_PROFILE_CHARACTERS {
        return Err(BpsrProfileProjectionError::TooManyLocalCharacters {
            maximum: MAXIMUM_LOCAL_PROFILE_CHARACTERS,
        });
    }
    accumulators.push(ProfileAccumulator {
        profile: patch,
        observation_count: 1,
        last_event_sequence: envelope.sequence,
    });
    Ok(())
}

#[derive(Debug)]
struct ProfileAccumulator {
    profile: CharacterProfilePatch,
    observation_count: u64,
    last_event_sequence: u64,
}

impl CharacterProfilePatch {
    fn merge_from(
        &mut self,
        newer: CharacterProfilePatch,
    ) -> Result<(), BpsrProfileProjectionError> {
        if self.character != newer.character {
            return Err(BpsrProfileProjectionError::CharacterMismatch);
        }
        replace_if_some(&mut self.display_name, newer.display_name);
        replace_if_some(&mut self.display_id, newer.display_id);
        replace_if_some(&mut self.server_id, newer.server_id);
        replace_if_some(&mut self.class_id, newer.class_id);
        replace_if_some(&mut self.specialization_id, newer.specialization_id);
        replace_if_some(&mut self.level, newer.level);
        merge_progression(&mut self.progression, newer.progression);
        replace_if_some(&mut self.combat_power, newer.combat_power);
        replace_if_some(
            &mut self.combat_power_breakdown,
            newer.combat_power_breakdown,
        );
        replace_if_some(&mut self.season_strength, newer.season_strength);
        replace_if_some(&mut self.master_score, newer.master_score);
        merge_season(&mut self.season, newer.season);
        replace_if_some(&mut self.appearance, newer.appearance);
        replace_if_some(&mut self.equipment, newer.equipment);
        replace_if_some(
            &mut self.equipment_suit_entries,
            newer.equipment_suit_entries,
        );
        replace_if_some(&mut self.modules, newer.modules);
        replace_if_some(&mut self.owned_imagines, newer.owned_imagines);
        replace_if_some(&mut self.battle_imagine_skills, newer.battle_imagine_skills);
        replace_if_some(&mut self.equipped_action_slots, newer.equipped_action_slots);
        replace_if_some(&mut self.active_skills, newer.active_skills);
        replace_if_some(&mut self.talents, newer.talents);
        replace_if_some(&mut self.talent_progress, newer.talent_progress);
        replace_if_some(&mut self.combat_professions, newer.combat_professions);
        replace_if_some(&mut self.life_professions, newer.life_professions);
        replace_if_some(&mut self.cosmetics, newer.cosmetics);
        merge_collection_summary(&mut self.collection_summary, newer.collection_summary);
        replace_if_some(&mut self.activity_progress, newer.activity_progress);
        replace_if_some(&mut self.season_medals, newer.season_medals);
        replace_if_some(&mut self.season_cultivation, newer.season_cultivation);
        replace_if_some(&mut self.reputations, newer.reputations);
        replace_if_some(
            &mut self.current_profession_project_id,
            newer.current_profession_project_id,
        );
        merge_social_display(&mut self.social_display, newer.social_display);
        Ok(())
    }

    fn merge_public_from(
        &mut self,
        newer: CharacterProfilePatch,
    ) -> Result<(), BpsrProfileProjectionError> {
        if self.character != newer.character {
            return Err(BpsrProfileProjectionError::CharacterMismatch);
        }
        replace_if_some(&mut self.display_name, newer.display_name);
        replace_if_some(&mut self.display_id, newer.display_id);
        replace_if_some(&mut self.server_id, newer.server_id);
        replace_if_some(&mut self.class_id, newer.class_id);
        replace_if_some(&mut self.specialization_id, newer.specialization_id);
        replace_if_some(&mut self.level, newer.level);
        replace_if_some(&mut self.combat_power, newer.combat_power);
        replace_if_some(&mut self.season_strength, newer.season_strength);
        replace_if_some(&mut self.master_score, newer.master_score);
        merge_season(&mut self.season, newer.season);
        merge_public_appearance(&mut self.appearance, newer.appearance);
        if self.equipment.is_none() {
            self.equipment = newer.equipment;
        }
        if self.combat_professions.is_none() {
            self.combat_professions = newer.combat_professions;
        }
        merge_social_display(&mut self.social_display, newer.social_display);
        Ok(())
    }
}

fn replace_if_some<T>(target: &mut Option<T>, newer: Option<T>) {
    if newer.is_some() {
        *target = newer;
    }
}

fn merge_social_display(target: &mut Option<SocialDisplay>, newer: Option<SocialDisplay>) {
    let Some(newer) = newer else {
        return;
    };
    let Some(target) = target.as_mut() else {
        *target = Some(newer);
        return;
    };
    replace_if_some(&mut target.guild_id, newer.guild_id);
    replace_if_some(&mut target.guild_name, newer.guild_name);
    replace_if_some(&mut target.equipped_title_id, newer.equipped_title_id);
    replace_if_some(&mut target.equipped_title_level, newer.equipped_title_level);
    target.title_ids.extend(newer.title_ids);
    target.title_ids.sort_unstable();
    target.title_ids.dedup();
    target.medal_ids.extend(newer.medal_ids);
    target.medal_ids.sort_unstable();
    target.medal_ids.dedup();
    target.medal_slots.extend(newer.medal_slots);
    replace_if_some(&mut target.profile_theme_id, newer.profile_theme_id);
}

fn merge_collection_summary(
    target: &mut Option<CollectionSummary>,
    newer: Option<CollectionSummary>,
) {
    let Some(newer) = newer else {
        return;
    };
    let Some(target) = target.as_mut() else {
        *target = Some(newer);
        return;
    };

    let CollectionSummary {
        observed_sections,
        fashion_points,
        mount_points,
        weapon_skin_points,
        equipped_fashion_ids,
        owned_fashion_ids,
        owned_mount_ids,
        owned_weapon_skin_ids,
        owned_dye_ids,
        unlocked_module_ids,
        ride_ids,
        ride_skin_ids,
        unlocked_emoji_ids,
        vanity_pet_ids,
        summoned_vanity_pet_id,
        fantasy_atlas_stages,
        handbook,
        photo_ids,
        photo_wall,
        achievements,
    } = newer;

    // Older schema-compatible events have no subsection marker. Merge those
    // conservatively so a partial legacy observation can enrich a profile but
    // cannot erase independent collection state.
    if observed_sections.is_empty() {
        replace_if_some(&mut target.fashion_points, fashion_points);
        replace_if_some(&mut target.mount_points, mount_points);
        replace_if_some(&mut target.weapon_skin_points, weapon_skin_points);
        target.equipped_fashion_ids.extend(equipped_fashion_ids);
        merge_unique(&mut target.owned_fashion_ids, owned_fashion_ids);
        merge_unique(&mut target.owned_mount_ids, owned_mount_ids);
        merge_unique(&mut target.owned_weapon_skin_ids, owned_weapon_skin_ids);
        merge_unique(&mut target.owned_dye_ids, owned_dye_ids);
        merge_unique(&mut target.unlocked_module_ids, unlocked_module_ids);
        merge_unique(&mut target.ride_ids, ride_ids);
        merge_unique(&mut target.ride_skin_ids, ride_skin_ids);
        merge_unique(&mut target.unlocked_emoji_ids, unlocked_emoji_ids);
        merge_unique(&mut target.vanity_pet_ids, vanity_pet_ids);
        replace_if_some(&mut target.summoned_vanity_pet_id, summoned_vanity_pet_id);
        target.fantasy_atlas_stages.extend(fantasy_atlas_stages);
        merge_handbook(&mut target.handbook, handbook);
        merge_unique(&mut target.photo_ids, photo_ids);
        target.photo_wall.extend(photo_wall);
        merge_achievements(&mut target.achievements, achievements);
        return;
    }

    if observed_sections.fashion {
        replace_if_some(&mut target.fashion_points, fashion_points);
        replace_if_some(&mut target.mount_points, mount_points);
        replace_if_some(&mut target.weapon_skin_points, weapon_skin_points);
        target.equipped_fashion_ids = equipped_fashion_ids;
        target.owned_fashion_ids = owned_fashion_ids;
        target.owned_mount_ids = owned_mount_ids;
        target.owned_weapon_skin_ids = owned_weapon_skin_ids;
        target.owned_dye_ids = owned_dye_ids;
    }
    if observed_sections.collection_book {
        target.unlocked_module_ids = unlocked_module_ids;
    }
    if observed_sections.personal_zone {
        replace_if_some(&mut target.fashion_points, fashion_points);
        replace_if_some(&mut target.mount_points, mount_points);
        replace_if_some(&mut target.weapon_skin_points, weapon_skin_points);
        target.photo_ids = photo_ids;
        target.photo_wall = photo_wall;
    }
    if observed_sections.rides {
        target.ride_ids = ride_ids;
        target.ride_skin_ids = ride_skin_ids;
    }
    if observed_sections.emojis {
        target.unlocked_emoji_ids = unlocked_emoji_ids;
    }
    if observed_sections.handbook {
        target.handbook = handbook;
    }
    if observed_sections.vanity_pets {
        target.vanity_pet_ids = vanity_pet_ids;
        target.summoned_vanity_pet_id = summoned_vanity_pet_id;
    }
    if observed_sections.fantasy_atlas {
        target.fantasy_atlas_stages = fantasy_atlas_stages;
    }
    if observed_sections.achievements {
        target.achievements = achievements;
    }
    target.observed_sections.merge(observed_sections);
}

fn merge_unique<T: Ord>(target: &mut Vec<T>, newer: Vec<T>) {
    target.extend(newer);
    target.sort_unstable();
    target.dedup();
}

fn merge_handbook(target: &mut Option<HandbookProgress>, newer: Option<HandbookProgress>) {
    let Some(newer) = newer else {
        return;
    };
    let Some(target) = target.as_mut() else {
        *target = Some(newer);
        return;
    };
    merge_unique(&mut target.important_people_ids, newer.important_people_ids);
    merge_unique(&mut target.reading_book_ids, newer.reading_book_ids);
    merge_unique(&mut target.dictionary_entry_ids, newer.dictionary_entry_ids);
    merge_unique(&mut target.postcard_ids, newer.postcard_ids);
    merge_unique(&mut target.monthly_card_ids, newer.monthly_card_ids);
}

fn merge_achievements(
    target: &mut Option<AchievementProgressProfile>,
    newer: Option<AchievementProgressProfile>,
) {
    let Some(newer) = newer else {
        return;
    };
    let Some(target) = target.as_mut() else {
        *target = Some(newer);
        return;
    };
    merge_achievement_entries(&mut target.general, newer.general);
    for newer_season in newer.seasons {
        if let Some(target_season) = target
            .seasons
            .iter_mut()
            .find(|season| season.season_id == newer_season.season_id)
        {
            merge_achievement_entries(&mut target_season.achievements, newer_season.achievements);
        } else {
            target.seasons.push(SeasonAchievementProgress {
                season_id: newer_season.season_id,
                achievements: newer_season.achievements,
            });
        }
    }
    target
        .seasons
        .sort_unstable_by_key(|season| season.season_id);
    merge_unique(
        &mut target.initialized_season_ids,
        newer.initialized_season_ids,
    );
    replace_if_some(&mut target.version, newer.version);
}

fn merge_achievement_entries(
    target: &mut Vec<AchievementProgress>,
    newer: Vec<AchievementProgress>,
) {
    for newer in newer {
        if let Some(target) = target
            .iter_mut()
            .find(|entry| entry.achievement_id == newer.achievement_id)
        {
            replace_if_some(&mut target.finish_count, newer.finish_count);
            replace_if_some(&mut target.reward_claimed, newer.reward_claimed);
            replace_if_some(&mut target.begin_progress, newer.begin_progress);
        } else {
            target.push(newer);
        }
    }
    target.sort_unstable_by_key(|entry| entry.achievement_id);
}

fn merge_progression(
    target: &mut Option<CharacterProgression>,
    newer: Option<CharacterProgression>,
) {
    let Some(newer) = newer else {
        return;
    };
    if let Some(target) = target {
        replace_if_some(&mut target.current_experience, newer.current_experience);
        replace_if_some(
            &mut target.previous_season_max_level,
            newer.previous_season_max_level,
        );
    } else {
        *target = Some(newer);
    }
}

fn merge_season(target: &mut Option<SeasonProfile>, newer: Option<SeasonProfile>) {
    let Some(newer) = newer else {
        return;
    };
    if let Some(target) = target {
        replace_if_some(&mut target.season_id, newer.season_id);
        replace_if_some(&mut target.level, newer.level);
        replace_if_some(&mut target.experience, newer.experience);
        replace_if_some(&mut target.power, newer.power);
        replace_if_some(&mut target.strength, newer.strength);
    } else {
        *target = Some(newer);
    }
}

fn merge_public_appearance(
    target: &mut Option<crate::CharacterAppearance>,
    newer: Option<crate::CharacterAppearance>,
) {
    let Some(newer) = newer else {
        return;
    };
    let Some(target) = target else {
        *target = Some(newer);
        return;
    };
    replace_if_some(&mut target.gender_id, newer.gender_id);
    replace_if_some(&mut target.body_size_id, newer.body_size_id);
    replace_if_some(&mut target.height, newer.height);
    replace_if_some(&mut target.voice_id, newer.voice_id);
    replace_if_some(&mut target.avatar_id, newer.avatar_id);
    replace_if_some(&mut target.profile_image_url, newer.profile_image_url);
    replace_if_some(&mut target.half_body_image_url, newer.half_body_image_url);
    replace_if_some(
        &mut target.business_card_style_id,
        newer.business_card_style_id,
    );
    replace_if_some(&mut target.avatar_frame_id, newer.avatar_frame_id);
}

#[derive(Debug, Error)]
pub enum BpsrProfileProjectionError {
    #[error("sealed log could not be replayed for profile projection: {0}")]
    Rlog(#[from] RlogError),

    #[error("sealed log ended without a verified integrity summary")]
    MissingSeal,

    #[error("BPSR profile event is invalid: {0}")]
    Profile(#[from] ProfileEventError),

    #[error("BPSR profile website request is invalid: {0}")]
    Website(#[from] BpsrWebsiteProfileError),

    #[error("local profile package is invalid: {0}")]
    Package(#[from] ProfilePackageError),

    #[error("live profile projection could not encode canonical content: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("one profile accumulator received two character identities")]
    CharacterMismatch,

    #[error("profile observation count overflowed")]
    ObservationOverflow,

    #[error("sealed log contains more than {maximum} local characters")]
    TooManyLocalCharacters { maximum: usize },
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{BufReader, Cursor},
    };

    use rlogs_events::{
        CharacterIdentity, EVENT_SCHEMA_VERSION, EventEnvelope, EventProvenance, EventSensitivity,
        EventTime, RegionContext, RegionEvidence, RegionEvidenceKind, RegionIdentity,
    };
    use rlogs_log_format::{RlogHeader, RlogWriter};

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: Some("asteria".into()),
                world_id: None,
            },
            client_build: "steam-24252055".into(),
            protocol_pack_digest: "sha256:pack".into(),
            evidence: vec![RegionEvidence {
                kind: RegionEvidenceKind::ProtocolPack,
                reference: "test".into(),
            }],
        }
    }

    fn profile() -> CharacterProfilePatch {
        CharacterProfilePatch {
            character: CharacterIdentity {
                region: region().identity,
                character_id: "123456".into(),
            },
            display_name: None,
            display_id: None,
            server_id: None,
            class_id: None,
            specialization_id: None,
            level: None,
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            season_strength: None,
            master_score: None,
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: None,
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
        }
    }

    fn envelope(
        sequence: u64,
        sensitivity: EventSensitivity,
        profile: CharacterProfilePatch,
    ) -> EventEnvelope {
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "profile-session".into(),
            sequence,
            region: region(),
            time: EventTime {
                observed_micros: sequence,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(sequence, 1, 1),
            sensitivity,
            event: CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event().unwrap()),
            },
        }
    }

    fn collection(
        observed_sections: crate::CollectionObservationSections,
    ) -> crate::CollectionSummary {
        crate::CollectionSummary {
            observed_sections,
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
            photo_ids: Vec::new(),
            photo_wall: BTreeMap::new(),
            achievements: None,
        }
    }

    #[test]
    fn independent_collection_packets_preserve_other_sections_and_allow_exact_clears() {
        let mut accumulated = profile();
        let mut photos = collection(crate::CollectionObservationSections {
            personal_zone: true,
            ..Default::default()
        });
        photos.photo_ids = vec![41, 42];
        photos.photo_wall = BTreeMap::from([(0, 42)]);
        accumulated.collection_summary = Some(photos);

        let mut achievement_patch = profile();
        let mut achievements = collection(crate::CollectionObservationSections {
            achievements: true,
            ..Default::default()
        });
        achievements.achievements = Some(crate::AchievementProgressProfile {
            general: vec![crate::AchievementProgress {
                achievement_id: 9,
                finish_count: Some(1),
                reward_claimed: Some(true),
                begin_progress: None,
            }],
            seasons: Vec::new(),
            initialized_season_ids: vec![3],
            version: Some(7),
        });
        achievement_patch.collection_summary = Some(achievements);
        accumulated.merge_from(achievement_patch).unwrap();

        let merged = accumulated.collection_summary.as_ref().unwrap();
        assert_eq!(merged.photo_ids, vec![41, 42]);
        assert_eq!(merged.photo_wall, BTreeMap::from([(0, 42)]));
        assert_eq!(
            merged.achievements.as_ref().unwrap().general[0].achievement_id,
            9
        );

        let mut clear_photos = profile();
        clear_photos.collection_summary = Some(collection(crate::CollectionObservationSections {
            personal_zone: true,
            ..Default::default()
        }));
        accumulated.merge_from(clear_photos).unwrap();
        let cleared = accumulated.collection_summary.as_ref().unwrap();
        assert!(cleared.photo_ids.is_empty());
        assert!(cleared.photo_wall.is_empty());
        assert_eq!(
            cleared.achievements.as_ref().unwrap().general[0].achievement_id,
            9
        );

        let request = website_profile_request(&accumulated).unwrap();
        assert!(
            request.payload.body["collection_summary"]
                .get("observed_sections")
                .is_none()
        );
    }

    #[test]
    fn personal_patches_merge_and_public_social_profiles_are_excluded() {
        let header = RlogHeader::new("profile-session", region(), "profile-test");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        let mut initial = profile();
        initial.display_name = Some("MarieRose".into());
        initial.level = Some(59);
        initial.season = Some(SeasonProfile {
            season_id: Some(3),
            level: Some(12),
            experience: Some(100),
            power: Some(200),
            strength: Some(300),
        });
        writer
            .push(&envelope(1, EventSensitivity::PersonalGameplay, initial))
            .unwrap();
        let mut social = profile();
        social.character.character_id = "999999".into();
        social.display_name = Some("SomeoneElse".into());
        writer
            .push(&envelope(2, EventSensitivity::PublicGameplay, social))
            .unwrap();
        let mut update = profile();
        update.level = Some(60);
        update.season = Some(SeasonProfile {
            season_id: Some(4),
            level: None,
            experience: None,
            power: None,
            strength: None,
        });
        writer
            .push(&envelope(3, EventSensitivity::PersonalGameplay, update))
            .unwrap();
        let bytes = writer.finish().unwrap();

        let packages = project_local_profile_packages(
            BufReader::new(Cursor::new(bytes)),
            RlogLimits::default(),
            10,
        )
        .unwrap();
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert_eq!(package.request.payload.routing["character-id"], "123456");
        assert_eq!(package.request.payload.body["display_name"], "MarieRose");
        assert_eq!(package.request.payload.body["level"], 60);
        assert_eq!(package.request.payload.body["season"]["season_id"], 4);
        assert_eq!(package.request.payload.body["season"]["level"], 12);
        assert_eq!(package.source.observation_count, 2);
        assert_eq!(package.source.last_event_sequence, 3);
        let json = serde_json::to_string(package).unwrap();
        assert!(!json.contains("SomeoneElse"));
        assert!(!json.contains("password"));
        assert!(!json.contains("account"));
    }

    #[test]
    fn a_log_without_personal_profiles_returns_no_package() {
        let header = RlogHeader::new("profile-session", region(), "profile-test");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer
            .push(&envelope(1, EventSensitivity::PublicGameplay, profile()))
            .unwrap();
        let packages = project_local_profile_packages(
            BufReader::new(Cursor::new(writer.finish().unwrap())),
            RlogLimits::default(),
            10,
        )
        .unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn live_projection_requires_personal_identity_then_safely_enriches_it() {
        let mut live = LiveProfileProjection::default();

        let mut unrelated = profile();
        unrelated.character.character_id = "999999".into();
        unrelated.display_name = Some("SomeoneElse".into());
        assert!(
            !live
                .observe(&envelope(1, EventSensitivity::PublicGameplay, unrelated,))
                .unwrap()
        );
        assert!(
            live.packages("live-session", "steam-24252055", "sha256:pack", 10)
                .unwrap()
                .is_empty()
        );

        let mut early_public_self = profile();
        early_public_self.display_name = Some("MarieRose".into());
        early_public_self.appearance = Some(crate::CharacterAppearance {
            gender_id: None,
            body_size_id: None,
            height: None,
            voice_id: None,
            face_options: BTreeMap::new(),
            color_options: BTreeMap::new(),
            avatar_id: None,
            profile_image_url: Some("https://images.example/profile.webp".into()),
            half_body_image_url: None,
            business_card_style_id: None,
            avatar_frame_id: None,
            unlocked_profile_image_ids: Vec::new(),
            unlocked_face_item_ids: Vec::new(),
            unlocked_voice_ids: Vec::new(),
        });
        assert!(
            !live
                .observe(&envelope(
                    2,
                    EventSensitivity::PublicGameplay,
                    early_public_self,
                ))
                .unwrap()
        );

        let mut personal = profile();
        personal.modules = Some(crate::ModuleProfile {
            equipped_slots: BTreeMap::from([(1, "9007199254740993".into())]),
            inventory: vec![crate::ModuleItemProfile {
                instance_id: "9007199254740993".into(),
                config_id: 314,
                count: Some(1),
                quality: Some(5),
                load_flag: Some(1),
                module_type: Some(2),
                level: Some(12),
                parts: Vec::new(),
                upgrade_records: Vec::new(),
                success_rate: None,
            }],
        });
        personal.appearance = Some(crate::CharacterAppearance {
            gender_id: Some(2),
            body_size_id: None,
            height: None,
            voice_id: None,
            face_options: BTreeMap::new(),
            color_options: BTreeMap::new(),
            avatar_id: Some(44),
            profile_image_url: None,
            half_body_image_url: None,
            business_card_style_id: None,
            avatar_frame_id: None,
            unlocked_profile_image_ids: vec![44, 45],
            unlocked_face_item_ids: Vec::new(),
            unlocked_voice_ids: Vec::new(),
        });
        assert!(
            live.observe(&envelope(3, EventSensitivity::PersonalGameplay, personal,))
                .unwrap()
        );

        let mut public_self = profile();
        public_self.master_score = Some(1_234_567);
        public_self.appearance = Some(crate::CharacterAppearance {
            gender_id: None,
            body_size_id: None,
            height: None,
            voice_id: None,
            face_options: BTreeMap::new(),
            color_options: BTreeMap::new(),
            avatar_id: None,
            profile_image_url: None,
            half_body_image_url: Some("https://images.example/half-body.webp".into()),
            business_card_style_id: None,
            avatar_frame_id: None,
            unlocked_profile_image_ids: Vec::new(),
            unlocked_face_item_ids: Vec::new(),
            unlocked_voice_ids: Vec::new(),
        });
        assert!(
            live.observe(&envelope(4, EventSensitivity::PublicGameplay, public_self,))
                .unwrap()
        );

        let packages = live
            .packages("live-session", "steam-24252055", "sha256:pack", 10)
            .unwrap();
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert_eq!(package.request.payload.body["display_name"], "MarieRose");
        assert_eq!(package.request.payload.body["master_score"], 1_234_567);
        assert_eq!(
            package.request.payload.body["appearance"]["profile_image_url"],
            "https://images.example/profile.webp"
        );
        assert_eq!(
            package.request.payload.body["appearance"]["unlocked_profile_image_ids"][1],
            45
        );
        assert_eq!(
            package.request.payload.body["modules"]["inventory"][0]["config_id"],
            314
        );
        assert_eq!(package.source.observation_count, 2);
        assert_eq!(package.source.last_event_sequence, 4);
    }
}
