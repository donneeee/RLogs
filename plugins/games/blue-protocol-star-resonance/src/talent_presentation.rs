use std::{collections::BTreeMap, sync::OnceLock};

use serde::Deserialize;

const TALENT_NODE_PRESENTATION_JSON: &str =
    include_str!("../game-data/runtime/talent-node-presentation.v1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct TalentNodePresentation {
    pub talent_id: i64,
    pub talent_level: Option<u32>,
    pub profession_id: Option<i32>,
    pub specialization_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct TalentNodeCatalog {
    schema_version: u16,
    nodes: BTreeMap<String, TalentNodePresentation>,
}

pub(crate) fn talent_node_presentation(node_id: i64) -> Option<TalentNodePresentation> {
    static CATALOG: OnceLock<TalentNodeCatalog> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        let catalog: TalentNodeCatalog = serde_json::from_str(TALENT_NODE_PRESENTATION_JSON)
            .expect("bundled talent-node presentation catalog must be valid");
        assert_eq!(catalog.schema_version, 1);
        catalog
    });
    catalog.nodes.get(&node_id.to_string()).copied()
}

#[cfg(test)]
mod tests {
    use super::talent_node_presentation;

    #[test]
    fn resolves_tree_node_to_actual_talent() {
        let endurance = talent_node_presentation(3_061).expect("Endurance node");
        assert_eq!(endurance.talent_id, 4);
        assert_eq!(endurance.profession_id, Some(11));

        let dexterity = talent_node_presentation(1_100_011).expect("Dexterity node");
        assert_eq!(dexterity.talent_id, 1_134);
    }
}
