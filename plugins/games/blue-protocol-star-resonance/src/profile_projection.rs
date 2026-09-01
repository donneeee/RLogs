use std::io::BufRead;

use rlogs_events::{CanonicalEvent, EventEnvelope, EventSensitivity};
use rlogs_log_format::{RlogError, RlogLimits, RlogReader};
use rlogs_profiles::{LocalProfilePackage, ProfilePackageError, ProfilePackageSource};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    BpsrWebsiteProfileError, CharacterProfilePatch, CharacterProgression, ProfileEventError,
    SeasonProfile, website_profile_request,
};

pub const MAXIMUM_LOCAL_PROFILE_CHARACTERS: usize = 8;
const MAXIMUM_PENDING_PUBLIC_PROFILE_CHARACTERS: usize = 64;

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
        replace_if_some(&mut self.collection_summary, newer.collection_summary);
        replace_if_some(&mut self.activity_progress, newer.activity_progress);
        replace_if_some(&mut self.season_medals, newer.season_medals);
        replace_if_some(&mut self.season_cultivation, newer.season_cultivation);
        replace_if_some(&mut self.reputations, newer.reputations);
        replace_if_some(
            &mut self.current_profession_project_id,
            newer.current_profession_project_id,
        );
        replace_if_some(&mut self.social_display, newer.social_display);
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
        replace_if_some(&mut self.social_display, newer.social_display);
        Ok(())
    }
}

fn replace_if_some<T>(target: &mut Option<T>, newer: Option<T>) {
    if newer.is_some() {
        *target = newer;
    }
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
