use std::collections::{BTreeMap, BTreeSet};

use crate::types::{
    AttributeCatalogEntry, AttributeScore, ModuleCandidate, OptimizerError, ScoreBreakdown,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringRules {
    pub(crate) catalog_revision: String,
    pub(crate) scoring_revision: String,
    pub(crate) attributes: BTreeMap<i32, Vec<(i32, i32)>>,
    pub(crate) link_power: Vec<i32>,
}

impl ScoringRules {
    pub fn catalog_revision(&self) -> &str {
        &self.catalog_revision
    }

    pub fn scoring_revision(&self) -> &str {
        &self.scoring_revision
    }

    pub fn attribute_ids(&self) -> impl Iterator<Item = i32> + '_ {
        self.attributes.keys().copied()
    }

    pub fn contains_attribute(&self, attribute_id: i32) -> bool {
        self.attributes.contains_key(&attribute_id)
    }

    pub(crate) fn attribute_power(&self, attribute_id: i32, value: i32) -> (Option<i32>, i32) {
        let Some(levels) = self.attributes.get(&attribute_id) else {
            return (None, 0);
        };
        levels
            .iter()
            .filter(|(threshold, _)| value >= *threshold)
            .next_back()
            .map_or((None, 0), |(threshold, power)| (Some(*threshold), *power))
    }

    pub(crate) fn total_link_power(&self, value: i32) -> i32 {
        let index = usize::try_from(value.max(0))
            .unwrap_or(usize::MAX)
            .min(self.link_power.len().saturating_sub(1));
        self.link_power[index]
    }

    pub(crate) fn score_dense(
        &self,
        values: &[i32],
        total_link_points: i32,
        target_attributes: &BTreeSet<i32>,
        exclude_attributes: &BTreeSet<i32>,
    ) -> i32 {
        let threshold_power = self
            .attributes
            .keys()
            .zip(values)
            .map(|(attribute_id, value)| {
                let (_, power) = self.attribute_power(*attribute_id, *value);
                power * attribute_multiplier(*attribute_id, target_attributes, exclude_attributes)
            })
            .sum::<i32>();
        threshold_power + self.total_link_power(total_link_points)
    }

    pub(crate) fn score_breakdown(
        &self,
        modules: &[ModuleCandidate],
        target_attributes: &BTreeSet<i32>,
        exclude_attributes: &BTreeSet<i32>,
    ) -> ScoreBreakdown {
        let mut totals = BTreeMap::<i32, i32>::new();
        let mut total_link_points = 0;
        for module in modules {
            for part in &module.parts {
                let value = part.initial_link_points.unwrap_or_default();
                *totals.entry(part.part_id).or_default() += value;
                total_link_points += value;
            }
        }
        let attributes = totals
            .into_iter()
            .map(|(attribute_id, total)| {
                let (reached_threshold, base_power) = self.attribute_power(attribute_id, total);
                let multiplier =
                    attribute_multiplier(attribute_id, target_attributes, exclude_attributes);
                AttributeScore {
                    attribute_id,
                    total,
                    reached_threshold,
                    base_power,
                    multiplier,
                    applied_power: base_power * multiplier,
                }
            })
            .collect::<Vec<_>>();
        let threshold_power = attributes.iter().map(|entry| entry.applied_power).sum();
        ScoreBreakdown {
            threshold_power,
            total_link_points,
            total_link_power: self.total_link_power(total_link_points),
            attributes,
        }
    }

    pub(crate) fn from_catalog_entries(
        catalog_revision: String,
        attributes: &[AttributeCatalogEntry],
        link_power: Vec<i32>,
    ) -> Result<Self, OptimizerError> {
        if attributes.is_empty() {
            return Err(OptimizerError::InvalidCatalog(
                "no module effects were available".into(),
            ));
        }
        if link_power.is_empty() {
            return Err(OptimizerError::InvalidCatalog(
                "no total-link fight values were available".into(),
            ));
        }
        let mut rules = BTreeMap::new();
        for attribute in attributes {
            if attribute.thresholds.len() != attribute.fight_values.len()
                || attribute.thresholds.is_empty()
            {
                return Err(OptimizerError::InvalidCatalog(format!(
                    "module effect {} has mismatched thresholds and fight values",
                    attribute.id
                )));
            }
            let levels = attribute
                .thresholds
                .iter()
                .copied()
                .zip(attribute.fight_values.iter().copied())
                .collect::<Vec<_>>();
            if levels.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
                return Err(OptimizerError::InvalidCatalog(format!(
                    "module effect {} thresholds are not strictly increasing",
                    attribute.id
                )));
            }
            rules.insert(attribute.id, levels);
        }
        Ok(Self {
            scoring_revision: format!("resonance-logs-cn-0.2.0-compatible+{catalog_revision}"),
            catalog_revision,
            attributes: rules,
            link_power,
        })
    }

    #[cfg(test)]
    pub(crate) fn cn_0_2_0_fixture() -> Self {
        const BASIC: [i32; 6] = [7, 14, 29, 44, 167, 254];
        const SPECIAL: [i32; 6] = [14, 29, 59, 89, 298, 448];
        const THRESHOLDS: [i32; 6] = [1, 4, 8, 12, 16, 20];
        const ATTRIBUTE_IDS: [i32; 21] = [
            1110, 1111, 1112, 1113, 1114, 1205, 1206, 1307, 1308, 1407, 1408, 1409, 1410, 2104,
            2105, 2204, 2205, 2304, 2404, 2405, 2406,
        ];
        const SPECIAL_IDS: [i32; 8] = [2104, 2105, 2204, 2205, 2304, 2404, 2405, 2406];
        const TOTAL: [i32; 121] = [
            0, 5, 11, 17, 23, 29, 34, 40, 46, 52, 58, 64, 69, 75, 81, 87, 93, 99, 104, 110, 116,
            122, 128, 133, 139, 145, 151, 157, 163, 168, 174, 180, 186, 192, 198, 203, 209, 215,
            221, 227, 233, 238, 244, 250, 256, 262, 267, 273, 279, 285, 291, 297, 302, 308, 314,
            320, 326, 332, 337, 343, 349, 355, 361, 366, 372, 378, 384, 390, 396, 401, 407, 413,
            419, 425, 431, 436, 442, 448, 454, 460, 466, 471, 477, 483, 489, 495, 500, 506, 512,
            518, 524, 530, 535, 541, 547, 553, 559, 565, 570, 576, 582, 588, 594, 599, 605, 611,
            617, 623, 629, 634, 640, 646, 652, 658, 664, 669, 675, 681, 687, 693, 699,
        ];
        let attributes = ATTRIBUTE_IDS
            .into_iter()
            .map(|attribute_id| {
                let power = if SPECIAL_IDS.contains(&attribute_id) {
                    SPECIAL
                } else {
                    BASIC
                };
                (
                    attribute_id,
                    THRESHOLDS.into_iter().zip(power).collect::<Vec<_>>(),
                )
            })
            .collect();
        Self {
            catalog_revision: "cn-0.2.0-test-fixture".into(),
            scoring_revision: "resonance-logs-cn-0.2.0-compatible+test".into(),
            attributes,
            link_power: TOTAL.to_vec(),
        }
    }
}

pub(crate) fn attribute_multiplier(
    attribute_id: i32,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> i32 {
    if target_attributes.contains(&attribute_id) {
        2
    } else if exclude_attributes.contains(&attribute_id) {
        0
    } else {
        1
    }
}
