//! Typed BPSR identities carried by `Zproto.SyncDamageInfo`.
//!
//! Canonical rLogs events intentionally retain the raw protocol integers so a newer game client
//! cannot lose an unknown value. These enums only label discriminants proven by the exact-build
//! IL2CPP schema and must not be used to coerce unknown values into a known category.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum BpsrDamageSourceKind {
    Skill = 0,
    Bullet = 1,
    Buff = 2,
    Fall = 3,
    FakeBullet = 4,
    Other = 100,
}

impl BpsrDamageSourceKind {
    pub const fn from_protocol_id(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Skill),
            1 => Some(Self::Bullet),
            2 => Some(Self::Buff),
            3 => Some(Self::Fall),
            4 => Some(Self::FakeBullet),
            100 => Some(Self::Other),
            _ => None,
        }
    }

    pub const fn protocol_id(self) -> i32 {
        self as i32
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Bullet => "bullet",
            Self::Buff => "buff",
            Self::Fall => "fall",
            Self::FakeBullet => "fake_bullet",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum BpsrDamageType {
    Normal = 0,
    Miss = 1,
    Heal = 2,
    Immune = 3,
    Fall = 4,
    Absorbed = 5,
}

/// Exact elemental property carried by `Zproto.DamageInfo.property`.
///
/// Unknown values remain raw on the canonical event and are never coerced into
/// this enum. The discriminants come from the exact-build `EDamageProperty`
/// enum, not from localized skill descriptions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum BpsrDamageProperty {
    General = 0,
    Fire = 1,
    Water = 2,
    Electricity = 3,
    Wood = 4,
    Wind = 5,
    Rock = 6,
    Light = 7,
    Dark = 8,
}

impl BpsrDamageProperty {
    pub const fn from_protocol_id(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::General),
            1 => Some(Self::Fire),
            2 => Some(Self::Water),
            3 => Some(Self::Electricity),
            4 => Some(Self::Wood),
            5 => Some(Self::Wind),
            6 => Some(Self::Rock),
            7 => Some(Self::Light),
            8 => Some(Self::Dark),
            _ => None,
        }
    }

    pub const fn protocol_id(self) -> i32 {
        self as i32
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Fire => "fire",
            Self::Water => "water",
            Self::Electricity => "electricity",
            Self::Wood => "wood",
            Self::Wind => "wind",
            Self::Rock => "rock",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl BpsrDamageType {
    pub const fn from_protocol_id(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Miss),
            2 => Some(Self::Heal),
            3 => Some(Self::Immune),
            4 => Some(Self::Fall),
            5 => Some(Self::Absorbed),
            _ => None,
        }
    }

    pub const fn protocol_id(self) -> i32 {
        self as i32
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Miss => "miss",
            Self::Heal => "heal",
            Self::Immune => "immune",
            Self::Fall => "fall",
            Self::Absorbed => "absorbed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BpsrDamageProperty, BpsrDamageSourceKind, BpsrDamageType};

    #[test]
    fn damage_source_discriminants_are_exact_and_unknown_values_remain_unknown() {
        let expected = [
            (0, BpsrDamageSourceKind::Skill, "skill"),
            (1, BpsrDamageSourceKind::Bullet, "bullet"),
            (2, BpsrDamageSourceKind::Buff, "buff"),
            (3, BpsrDamageSourceKind::Fall, "fall"),
            (4, BpsrDamageSourceKind::FakeBullet, "fake_bullet"),
            (100, BpsrDamageSourceKind::Other, "other"),
        ];

        for (id, kind, label) in expected {
            assert_eq!(BpsrDamageSourceKind::from_protocol_id(id), Some(kind));
            assert_eq!(kind.protocol_id(), id);
            assert_eq!(kind.as_str(), label);
        }
        assert_eq!(BpsrDamageSourceKind::from_protocol_id(5), None);
    }

    #[test]
    fn damage_type_discriminants_are_exact_and_unknown_values_remain_unknown() {
        let expected = [
            (0, BpsrDamageType::Normal, "normal"),
            (1, BpsrDamageType::Miss, "miss"),
            (2, BpsrDamageType::Heal, "heal"),
            (3, BpsrDamageType::Immune, "immune"),
            (4, BpsrDamageType::Fall, "fall"),
            (5, BpsrDamageType::Absorbed, "absorbed"),
        ];

        for (id, kind, label) in expected {
            assert_eq!(BpsrDamageType::from_protocol_id(id), Some(kind));
            assert_eq!(kind.protocol_id(), id);
            assert_eq!(kind.as_str(), label);
        }
        assert_eq!(BpsrDamageType::from_protocol_id(6), None);
    }

    #[test]
    fn damage_property_discriminants_are_exact_and_unknown_values_remain_unknown() {
        let expected = [
            (0, BpsrDamageProperty::General, "general"),
            (1, BpsrDamageProperty::Fire, "fire"),
            (2, BpsrDamageProperty::Water, "water"),
            (3, BpsrDamageProperty::Electricity, "electricity"),
            (4, BpsrDamageProperty::Wood, "wood"),
            (5, BpsrDamageProperty::Wind, "wind"),
            (6, BpsrDamageProperty::Rock, "rock"),
            (7, BpsrDamageProperty::Light, "light"),
            (8, BpsrDamageProperty::Dark, "dark"),
        ];

        for (id, property, label) in expected {
            assert_eq!(BpsrDamageProperty::from_protocol_id(id), Some(property));
            assert_eq!(property.protocol_id(), id);
            assert_eq!(property.as_str(), label);
        }
        assert_eq!(BpsrDamageProperty::from_protocol_id(9), None);
    }
}
