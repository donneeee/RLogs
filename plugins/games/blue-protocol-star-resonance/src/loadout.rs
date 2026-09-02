use rlogs_events::ActorLoadoutSlot;

use crate::{
    BattleImagineSkill, CharacterProfilePatch, auxiliary_action_presentation,
    battle_imagine_presentation,
};

const PRIMARY_IMAGINE_SLOTS: [i32; 2] = [7, 8];
const AUXILIARY_ACTION_SLOTS: [i32; 4] = [21, 22, 23, 24];

/// Projects the current BPSR action-bar snapshot into the game-neutral actor
/// loadout contract. Packet and opcode interpretation stops at this boundary;
/// history and other consumers only receive these normalized slots.
pub fn project_actor_loadouts(
    profile: &CharacterProfilePatch,
) -> (Vec<ActorLoadoutSlot>, Vec<ActorLoadoutSlot>) {
    let battle_skills = profile.battle_imagine_skills.as_deref().unwrap_or_default();
    // The action-slot map is the packet's exact placement authority. The
    // BattleImagineSkill entry supplies tier and presentation metadata, but
    // its embedded equipped_slot can lag behind an incremental slot swap.
    // Only fall back to the embedded slot when no exact slot map was observed.
    let mut primary = if let Some(action_slots) = profile.equipped_action_slots.as_deref() {
        action_slots
            .iter()
            .filter(|slot| PRIMARY_IMAGINE_SLOTS.contains(&slot.slot_id))
            .map(|slot| primary_loadout_slot(slot.slot_id, slot.skill_id, battle_skills))
            .collect::<Vec<_>>()
    } else {
        battle_skills
            .iter()
            .filter_map(|skill| {
                let slot_id = skill.equipped_slot?;
                PRIMARY_IMAGINE_SLOTS
                    .contains(&slot_id)
                    .then(|| loadout_slot(slot_id, skill))
            })
            .collect::<Vec<_>>()
    };
    primary.sort_unstable_by_key(|slot| slot.slot_id);

    let mut auxiliary = profile
        .equipped_action_slots
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|slot| AUXILIARY_ACTION_SLOTS.contains(&slot.slot_id))
        .map(|slot| auxiliary_loadout_slot(profile, slot.slot_id, slot.skill_id, battle_skills))
        .collect::<Vec<_>>();
    auxiliary.sort_unstable_by_key(|slot| slot.slot_id);
    (primary, auxiliary)
}

fn primary_loadout_slot(
    slot_id: i32,
    action_skill_id: i64,
    battle_skills: &[BattleImagineSkill],
) -> ActorLoadoutSlot {
    let imagine = battle_skills
        .iter()
        .find(|skill| battle_skill_matches(skill, action_skill_id));
    ActorLoadoutSlot {
        slot_id,
        ability_id: Some(action_skill_id),
        item_id: imagine.and_then(battle_imagine_item_id).or_else(|| {
            battle_imagine_presentation(action_skill_id)
                .ok()
                .flatten()
                .map(|presentation| presentation.item_id)
        }),
        tier: imagine.and_then(|skill| skill.remodel_level),
    }
}

fn auxiliary_loadout_slot(
    profile: &CharacterProfilePatch,
    slot_id: i32,
    action_skill_id: i64,
    battle_skills: &[BattleImagineSkill],
) -> ActorLoadoutSlot {
    let direct = battle_skills
        .iter()
        .find(|skill| battle_skill_matches(skill, action_skill_id));
    let replacement = auxiliary_action_presentation(action_skill_id)
        .ok()
        .flatten()
        .and_then(|presentation| presentation.replacement_imagine_skill_id)
        .and_then(|replacement_skill_id| {
            battle_skills
                .iter()
                .find(|skill| battle_skill_matches(skill, replacement_skill_id))
        });
    let imagine = direct.or(replacement);
    ActorLoadoutSlot {
        slot_id,
        ability_id: Some(action_skill_id),
        item_id: imagine.and_then(battle_imagine_item_id),
        // The Battle Imagine remodel level describes its primary-slot tier and
        // may legitimately be T5. Auxiliary replacements have their own exact
        // action-skill remodel level and only support T1 through T4.
        tier: imagine.and_then(|_| auxiliary_action_tier(profile, action_skill_id)),
    }
}

fn auxiliary_action_tier(profile: &CharacterProfilePatch, action_skill_id: i64) -> Option<u32> {
    let active_tier = profile
        .active_skills
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|skill| skill.skill_id == action_skill_id)
        .and_then(|skill| skill.remodel_level);
    let profession_tier = profile
        .combat_professions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|profession| {
            profile
                .class_id
                .is_none_or(|class_id| profession.profession_id == class_id)
        })
        .flat_map(|profession| profession.skills.iter())
        .find(|skill| skill.skill_id == action_skill_id)
        .and_then(|skill| skill.remodel_level);

    normalize_auxiliary_imagine_tier(active_tier)
        .or_else(|| normalize_auxiliary_imagine_tier(profession_tier))
}

/// Accepts only the tier domain used when a Battle Imagine replaces an
/// auxiliary action. Primary Battle Imagines use a different T0-T5 domain.
pub fn normalize_auxiliary_imagine_tier(tier: Option<u32>) -> Option<u32> {
    tier.filter(|tier| (1..=4).contains(tier))
}

fn battle_skill_matches(skill: &BattleImagineSkill, equipped_skill_id: i64) -> bool {
    skill.skill_id == equipped_skill_id
        || skill.base_skill_id == Some(equipped_skill_id)
        || skill.replacement_skill_ids.contains(&equipped_skill_id)
}

fn loadout_slot(slot_id: i32, skill: &BattleImagineSkill) -> ActorLoadoutSlot {
    let item_id = battle_imagine_item_id(skill);
    ActorLoadoutSlot {
        slot_id,
        ability_id: Some(skill.skill_id),
        item_id,
        tier: skill.remodel_level,
    }
}

fn battle_imagine_item_id(skill: &BattleImagineSkill) -> Option<i64> {
    battle_imagine_presentation(skill.skill_id)
        .ok()
        .flatten()
        .map(|presentation| presentation.item_id)
}

#[cfg(test)]
mod tests {
    use rlogs_events::{CharacterIdentity, RegionIdentity};

    use super::*;
    use crate::{EquippedActionSlot, SkillLevel};

    fn profile() -> CharacterProfilePatch {
        CharacterProfilePatch {
            character: CharacterIdentity {
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                character_id: "3296036".into(),
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
            combat_stats: None,
            season_strength: None,
            master_score: None,
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: Some(vec![
                BattleImagineSkill {
                    skill_id: 3_948,
                    base_skill_id: Some(3_948),
                    level: Some(1),
                    remodel_level: Some(5),
                    skin_id: None,
                    replacement_skill_ids: Vec::new(),
                    unlocked_skin_ids: Vec::new(),
                    equipped_slot: Some(7),
                },
                BattleImagineSkill {
                    skill_id: 3_969,
                    base_skill_id: Some(3_969),
                    level: Some(1),
                    remodel_level: Some(4),
                    skin_id: None,
                    replacement_skill_ids: vec![20_039_690],
                    unlocked_skin_ids: Vec::new(),
                    equipped_slot: Some(21),
                },
                BattleImagineSkill {
                    skill_id: 3_902,
                    base_skill_id: Some(3_902),
                    level: Some(1),
                    remodel_level: Some(5),
                    skin_id: None,
                    replacement_skill_ids: vec![3_902],
                    unlocked_skin_ids: Vec::new(),
                    equipped_slot: None,
                },
            ]),
            equipped_action_slots: Some(vec![
                EquippedActionSlot {
                    slot_id: 7,
                    skill_id: 3_948,
                    auto_battle_disabled: None,
                },
                EquippedActionSlot {
                    slot_id: 21,
                    skill_id: 3_021,
                    auto_battle_disabled: None,
                },
                EquippedActionSlot {
                    slot_id: 22,
                    skill_id: 3_612,
                    auto_battle_disabled: None,
                },
                EquippedActionSlot {
                    slot_id: 9,
                    skill_id: 2_231,
                    auto_battle_disabled: None,
                },
            ]),
            active_skills: Some(vec![SkillLevel {
                skill_id: 3_021,
                base_skill_id: Some(3_021),
                level: Some(1),
                remodel_level: Some(4),
                skin_id: None,
                replacement_skill_ids: Vec::new(),
                unlocked_skin_ids: Vec::new(),
            }]),
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

    #[test]
    fn separates_primary_imagines_from_four_auxiliary_slots() {
        let (primary, auxiliary) = project_actor_loadouts(&profile());
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].slot_id, 7);
        assert_eq!(primary[0].tier, Some(5));
        assert_eq!(auxiliary.len(), 2);
        assert_eq!(auxiliary[0].slot_id, 21);
        assert_eq!(auxiliary[0].ability_id, Some(3_021));
        assert_eq!(auxiliary[0].item_id, Some(3_000_009));
        assert_eq!(auxiliary[0].tier, Some(4));
        assert_eq!(auxiliary[1].slot_id, 22);
        assert_eq!(auxiliary[1].ability_id, Some(3_612));
        assert_eq!(auxiliary[1].tier, None);
    }

    #[test]
    fn exact_action_slot_overrides_stale_embedded_primary_placement() {
        let mut profile = profile();
        profile
            .battle_imagine_skills
            .as_mut()
            .unwrap()
            .push(BattleImagineSkill {
                skill_id: 3_982,
                base_skill_id: Some(3_982),
                level: Some(1),
                remodel_level: Some(5),
                skin_id: None,
                replacement_skill_ids: Vec::new(),
                unlocked_skin_ids: Vec::new(),
                equipped_slot: None,
            });
        profile
            .equipped_action_slots
            .as_mut()
            .unwrap()
            .push(EquippedActionSlot {
                slot_id: 8,
                skill_id: 3_982,
                auto_battle_disabled: None,
            });

        let (primary, _) = project_actor_loadouts(&profile);
        assert_eq!(primary.len(), 2);
        assert_eq!(primary[1].slot_id, 8);
        assert_eq!(primary[1].ability_id, Some(3_982));
        assert_eq!(primary[1].item_id, Some(3_001_001));
        assert_eq!(primary[1].tier, Some(5));
        assert!(!primary.iter().any(|slot| slot.ability_id == Some(3_969)));
    }

    #[test]
    fn auxiliary_imagine_tiers_reject_primary_only_and_missing_values() {
        assert_eq!(normalize_auxiliary_imagine_tier(None), None);
        assert_eq!(normalize_auxiliary_imagine_tier(Some(0)), None);
        assert_eq!(normalize_auxiliary_imagine_tier(Some(1)), Some(1));
        assert_eq!(normalize_auxiliary_imagine_tier(Some(4)), Some(4));
        assert_eq!(normalize_auxiliary_imagine_tier(Some(5)), None);
    }
}
