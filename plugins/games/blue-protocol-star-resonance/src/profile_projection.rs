use std::io::BufRead;

use rlogs_events::{CanonicalEvent, EventEnvelope, EventSensitivity};
use rlogs_log_format::{RlogError, RlogLimits, RlogReader};
use rlogs_profiles::{LocalProfilePackage, ProfilePackageError, ProfilePackageSource};
use thiserror::Error;

use crate::{
    BpsrWebsiteProfileError, CharacterProfilePatch, CharacterProgression, ProfileEventError,
    SeasonProfile, website_profile_request,
};

pub const MAXIMUM_LOCAL_PROFILE_CHARACTERS: usize = 8;

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
        merge_season(&mut self.season, newer.season);
        replace_if_some(&mut self.appearance, newer.appearance);
        replace_if_some(&mut self.equipment, newer.equipment);
        replace_if_some(&mut self.modules, newer.modules);
        replace_if_some(&mut self.owned_imagines, newer.owned_imagines);
        replace_if_some(&mut self.battle_imagine_skills, newer.battle_imagine_skills);
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

    #[error("one profile accumulator received two character identities")]
    CharacterMismatch,

    #[error("profile observation count overflowed")]
    ObservationOverflow,

    #[error("sealed log contains more than {maximum} local characters")]
    TooManyLocalCharacters { maximum: usize },
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

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
            season: None,
            appearance: None,
            equipment: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
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
}
