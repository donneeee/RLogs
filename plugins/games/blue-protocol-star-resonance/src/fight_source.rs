//! BPSR protocol identities for the configured source that created a fight effect.
//!
//! The numeric discriminants are part of the game protocol. Canonical rLogs events retain the
//! raw integer so future client values remain lossless; this enum only labels values whose schema
//! identity is known.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum BpsrFightSourceKind {
    Skill = 0,
    Buff = 1,
    Bullet = 2,
    Task = 4,
    Talent = 6,
    SeasonMedal = 7,
    UnionEffect = 8,
    Mod = 9,
    Equip = 10,
    EquipSlotRefine = 11,
    Vehicle = 12,
    SeasonTalent = 13,
    SceneBegin = 1000,
    Scene = 1001,
    Affix = 1002,
    Other = 10000,
}

impl BpsrFightSourceKind {
    pub const fn from_protocol_id(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Skill),
            1 => Some(Self::Buff),
            2 => Some(Self::Bullet),
            4 => Some(Self::Task),
            6 => Some(Self::Talent),
            7 => Some(Self::SeasonMedal),
            8 => Some(Self::UnionEffect),
            9 => Some(Self::Mod),
            10 => Some(Self::Equip),
            11 => Some(Self::EquipSlotRefine),
            12 => Some(Self::Vehicle),
            13 => Some(Self::SeasonTalent),
            1000 => Some(Self::SceneBegin),
            1001 => Some(Self::Scene),
            1002 => Some(Self::Affix),
            10000 => Some(Self::Other),
            _ => None,
        }
    }

    pub const fn protocol_id(self) -> i32 {
        self as i32
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Buff => "buff",
            Self::Bullet => "bullet",
            Self::Task => "task",
            Self::Talent => "talent",
            Self::SeasonMedal => "season_medal",
            Self::UnionEffect => "union_effect",
            Self::Mod => "mod",
            Self::Equip => "equip",
            Self::EquipSlotRefine => "equip_slot_refine",
            Self::Vehicle => "vehicle",
            Self::SeasonTalent => "season_talent",
            Self::SceneBegin => "scene_begin",
            Self::Scene => "scene",
            Self::Affix => "affix",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BpsrFightSourceKind;

    #[test]
    fn protocol_discriminants_are_exact_and_unknown_values_remain_unknown() {
        let expected = [
            (0, BpsrFightSourceKind::Skill, "skill"),
            (1, BpsrFightSourceKind::Buff, "buff"),
            (2, BpsrFightSourceKind::Bullet, "bullet"),
            (4, BpsrFightSourceKind::Task, "task"),
            (6, BpsrFightSourceKind::Talent, "talent"),
            (7, BpsrFightSourceKind::SeasonMedal, "season_medal"),
            (8, BpsrFightSourceKind::UnionEffect, "union_effect"),
            (9, BpsrFightSourceKind::Mod, "mod"),
            (10, BpsrFightSourceKind::Equip, "equip"),
            (
                11,
                BpsrFightSourceKind::EquipSlotRefine,
                "equip_slot_refine",
            ),
            (12, BpsrFightSourceKind::Vehicle, "vehicle"),
            (13, BpsrFightSourceKind::SeasonTalent, "season_talent"),
            (1000, BpsrFightSourceKind::SceneBegin, "scene_begin"),
            (1001, BpsrFightSourceKind::Scene, "scene"),
            (1002, BpsrFightSourceKind::Affix, "affix"),
            (10000, BpsrFightSourceKind::Other, "other"),
        ];

        for (id, kind, label) in expected {
            assert_eq!(BpsrFightSourceKind::from_protocol_id(id), Some(kind));
            assert_eq!(kind.protocol_id(), id);
            assert_eq!(kind.as_str(), label);
        }

        assert_eq!(BpsrFightSourceKind::from_protocol_id(3), None);
        assert_eq!(BpsrFightSourceKind::from_protocol_id(1003), None);
    }
}
