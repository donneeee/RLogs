use prost::Message;
use serde::Serialize;

use crate::game_schema_v1;

/// Exact packet snapshot of one BPSR shield instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ShieldInstanceSnapshot {
    pub uuid: Option<i64>,
    pub shield_type: Option<i32>,
    pub current_value: Option<i64>,
    pub initial_value: Option<i64>,
    pub max_value: Option<i64>,
}

/// Exact decoded value of current-build entity attribute 60050.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShieldListSnapshot {
    pub shields: Vec<ShieldInstanceSnapshot>,
}

impl ShieldListSnapshot {
    /// Sums every packet-provided current shield value.
    ///
    /// An explicitly empty list is a proven zero total. A non-empty list in
    /// which every current value is absent remains unresolved instead of being
    /// coerced to zero.
    pub fn current_value_total(&self) -> Option<i64> {
        if self.shields.is_empty() {
            return Some(0);
        }
        let mut observed = false;
        let total = self.shields.iter().fold(0_i64, |total, shield| {
            let Some(current_value) = shield.current_value else {
                return total;
            };
            observed = true;
            total.saturating_add(current_value)
        });
        observed.then_some(total)
    }
}

/// Decodes entity attribute 60050 without interpreting or dropping any shield
/// type. Formula-specific consumers may subsequently select a subset only when
/// packet evidence proves that selection.
pub fn decode_shield_list(raw: &[u8]) -> Result<ShieldListSnapshot, prost::DecodeError> {
    let decoded = game_schema_v1::AttrShieldList::decode(raw)?;
    Ok(ShieldListSnapshot {
        shields: decoded
            .shields
            .into_iter()
            .map(|shield| ShieldInstanceSnapshot {
                uuid: shield.uuid,
                shield_type: shield.shield_type,
                current_value: shield.current_value,
                initial_value: shield.initial_value,
                max_value: shield.max_value,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_current_build_single_shield_packet_value() {
        let decoded = decode_shield_list(&[
            10, 17, 8, 240, 1, 16, 12, 24, 144, 147, 2, 32, 190, 201, 1, 40, 208, 141, 19,
        ])
        .expect("current build shield payload should decode");

        assert_eq!(
            decoded.shields,
            vec![ShieldInstanceSnapshot {
                uuid: Some(240),
                shield_type: Some(12),
                current_value: Some(35_216),
                initial_value: Some(25_790),
                max_value: Some(313_040),
            }]
        );
        assert_eq!(decoded.current_value_total(), Some(35_216));
    }

    #[test]
    fn decodes_and_sums_current_build_multi_shield_packet_value() {
        let decoded = decode_shield_list(&[
            10, 17, 8, 190, 3, 16, 102, 24, 225, 152, 36, 32, 136, 247, 25, 40, 173, 235, 54, 10,
            17, 8, 209, 3, 16, 12, 24, 136, 147, 2, 32, 224, 193, 1, 40, 151, 192, 18,
        ])
        .expect("current build repeated shield payload should decode");

        assert_eq!(decoded.shields.len(), 2);
        assert_eq!(decoded.shields[0].uuid, Some(446));
        assert_eq!(decoded.shields[0].shield_type, Some(102));
        assert_eq!(decoded.shields[0].current_value, Some(592_993));
        assert_eq!(decoded.shields[0].initial_value, Some(424_840));
        assert_eq!(decoded.shields[0].max_value, Some(898_477));
        assert_eq!(decoded.shields[1].uuid, Some(465));
        assert_eq!(decoded.shields[1].shield_type, Some(12));
        assert_eq!(decoded.shields[1].current_value, Some(35_208));
        assert_eq!(decoded.shields[1].initial_value, Some(24_800));
        assert_eq!(decoded.shields[1].max_value, Some(303_127));
        assert_eq!(decoded.current_value_total(), Some(628_201));
    }

    #[test]
    fn explicit_empty_shield_list_is_zero() {
        let decoded = decode_shield_list(&[]).expect("empty protobuf is an empty shield list");
        assert!(decoded.shields.is_empty());
        assert_eq!(decoded.current_value_total(), Some(0));
    }
}
