use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::run_rules::bundled_run_reducer_config;
use crate::run_rules::bundled_run_rules_support_identity;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BossMonsterCatalog {
    schema_version: u16,
    monster_type: i32,
    source_build: String,
    monster_ids: Vec<i64>,
}

static BOSS_MONSTER_CATALOG: OnceLock<Result<BossMonsterCatalog, String>> = OnceLock::new();
static SCENE_BOSS_MONSTER_IDS: OnceLock<Result<BTreeMap<i32, BTreeSet<i64>>, String>> =
    OnceLock::new();

fn boss_monster_catalog() -> Result<&'static BossMonsterCatalog, String> {
    BOSS_MONSTER_CATALOG
        .get_or_init(|| {
            let catalog: BossMonsterCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/boss-monster-ids.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR boss monster catalog is invalid: {error}"))?;
            if catalog.schema_version != 1
                || catalog.monster_type != 2
                || catalog.source_build.trim().is_empty()
                || catalog.monster_ids.is_empty()
                || catalog
                    .monster_ids
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || catalog.monster_ids.iter().any(|id| *id <= 0)
            {
                return Err("bundled BPSR boss monster catalog has an unsupported shape".into());
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn authoritative_boss_monster_ids() -> Result<&'static [i64], String> {
    Ok(boss_monster_catalog()?.monster_ids.as_slice())
}

fn scene_boss_monster_catalog() -> Result<&'static BTreeMap<i32, BTreeSet<i64>>, String> {
    SCENE_BOSS_MONSTER_IDS
        .get_or_init(|| {
            bundled_run_reducer_config()
                .map_err(|error| format!("could not load BPSR scene boss identities: {error}"))
                .map(|config| {
                    config
                        .scene_rules
                        .into_iter()
                        .filter_map(|(scene_id, rule)| {
                            (!rule.boss_monster_ids.is_empty())
                                .then_some((scene_id, rule.boss_monster_ids))
                        })
                        .collect()
                })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Returns true when the reviewed current-build MonsterTable classifies this
/// identity as a boss. This is the fallback for activities whose run rule has
/// not yet narrowed its exact boss identities.
pub fn is_boss_monster(monster_id: i64) -> Result<bool, String> {
    Ok(boss_monster_catalog()?
        .monster_ids
        .binary_search(&monster_id)
        .is_ok())
}

pub fn is_boss_monster_for_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    monster_id: i64,
) -> Result<bool, String> {
    if !bundled_run_rules_support_identity(deployment_id, client_build, protocol_pack_digest)
        .map_err(|error| error.to_string())?
    {
        return Ok(false);
    }
    is_boss_monster(monster_id)
}

/// Returns the exact reviewed boss identities for a scene when its run rule
/// provides them. An absent set means callers must use the boss-type catalog;
/// it never means that every monster with an HP bar is a boss.
pub fn scene_boss_monster_ids(scene_id: i32) -> Result<Option<&'static BTreeSet<i64>>, String> {
    Ok(scene_boss_monster_catalog()?.get(&scene_id))
}

pub fn scene_boss_monster_ids_for_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    scene_id: i32,
) -> Result<Option<&'static BTreeSet<i64>>, String> {
    if !bundled_run_rules_support_identity(deployment_id, client_build, protocol_pack_digest)
        .map_err(|error| error.to_string())?
    {
        return Ok(None);
    }
    scene_boss_monster_ids(scene_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorized_identity() -> (&'static str, &'static str, &'static str) {
        (
            crate::BUNDLED_RUN_RULE_DEPLOYMENT_ID,
            crate::BUNDLED_RUN_RULE_CLIENT_BUILD,
            crate::BUNDLED_RUN_RULE_PROTOCOL_PACK_DIGEST,
        )
    }

    #[test]
    fn current_build_boss_type_catalog_is_sorted_and_queryable() {
        assert!(is_boss_monster(33_500).unwrap());
        assert!(!is_boss_monster(9_999_999_999).unwrap());
    }

    #[test]
    fn exact_scene_rules_identify_bosses_without_encoding_a_count() {
        let mech_facility = scene_boss_monster_ids(6_525).unwrap().unwrap();
        assert_eq!(mech_facility, &BTreeSet::from([33_500]));
        let guild_hunt = scene_boss_monster_ids(12_023).unwrap().unwrap();
        assert_eq!(
            guild_hunt,
            &BTreeSet::from([80_015, 80_016, 80_017, 80_018, 80_019])
        );
        assert_eq!(scene_boss_monster_ids(12_022).unwrap().unwrap(), guild_hunt);
        assert!(scene_boss_monster_ids(12_021).unwrap().is_none());
        assert!(scene_boss_monster_ids(12_024).unwrap().is_none());
    }

    #[test]
    fn boss_presentation_requires_the_exact_authorized_runtime_identity() {
        let (deployment, build, digest) = authorized_identity();
        assert!(is_boss_monster_for_identity(deployment, build, digest, 33_500).unwrap());
        assert!(
            scene_boss_monster_ids_for_identity(deployment, build, digest, 6_525)
                .unwrap()
                .is_some()
        );
        for (candidate_deployment, candidate_build, candidate_digest) in [
            ("cn", build, digest),
            (deployment, "wrong-build", digest),
            (deployment, build, "sha256:wrong-pack"),
        ] {
            assert!(
                !is_boss_monster_for_identity(
                    candidate_deployment,
                    candidate_build,
                    candidate_digest,
                    33_500,
                )
                .unwrap()
            );
            assert!(
                scene_boss_monster_ids_for_identity(
                    candidate_deployment,
                    candidate_build,
                    candidate_digest,
                    6_525,
                )
                .unwrap()
                .is_none()
            );
        }
    }
}
