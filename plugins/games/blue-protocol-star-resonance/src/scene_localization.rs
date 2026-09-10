use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalizationRuntimeIdentity {
    schema_version: u16,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
}

static LOCALIZATION_RUNTIME_IDENTITY: OnceLock<Result<LocalizationRuntimeIdentity, String>> =
    OnceLock::new();

fn localization_runtime_identity() -> Result<&'static LocalizationRuntimeIdentity, String> {
    LOCALIZATION_RUNTIME_IDENTITY
        .get_or_init(|| {
            let identity: LocalizationRuntimeIdentity = serde_json::from_str(include_str!(
                "../game-data/runtime/localization-runtime.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR localization identity is invalid: {error}"))?;
            if identity.schema_version != 1
                || identity.deployment_id.trim().is_empty()
                || identity.client_build.trim().is_empty()
                || !identity.protocol_pack_digest.starts_with("sha256:")
                || identity.protocol_pack_digest.len() != 71
                || !identity
                    .protocol_pack_digest
                    .strip_prefix("sha256:")
                    .is_some_and(|digest| digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            {
                return Err("bundled BPSR localization identity has an unsupported shape".into());
            }
            Ok(identity)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn bundled_localization_supports_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
) -> Result<bool, String> {
    let identity = localization_runtime_identity()?;
    Ok(deployment_id == identity.deployment_id
        && client_build == identity.client_build
        && protocol_pack_digest == identity.protocol_pack_digest)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePresentation {
    pub scene_id: i64,
    pub scene_type: i32,
    pub scene_subtype: i32,
    pub parent_scene_id: i64,
    pub scene_resource_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenePresentationCatalog {
    schema_version: u16,
    scenes: Vec<ScenePresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneLocalizationCatalog {
    schema_version: u16,
    locale: String,
    scenes: Vec<(i64, String)>,
}

static PRESENTATION: OnceLock<Result<ScenePresentationCatalog, String>> = OnceLock::new();

fn presentation_catalog() -> Result<&'static ScenePresentationCatalog, String> {
    PRESENTATION
        .get_or_init(|| {
            let catalog: ScenePresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/scene-presentation.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR scene presentation is invalid: {error}"))?;
            if catalog.schema_version != 1
                || catalog.scenes.is_empty()
                || catalog.scenes.len() > 10_000
                || catalog
                    .scenes
                    .windows(2)
                    .any(|pair| pair[0].scene_id >= pair[1].scene_id)
                || catalog.scenes.iter().any(|scene| {
                    scene.scene_id <= 0
                        || scene.scene_type < 0
                        || scene.scene_subtype < 0
                        || scene.parent_scene_id < 0
                        || scene.scene_resource_id <= 0
                })
            {
                return Err("bundled BPSR scene presentation has an unsupported shape".into());
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

struct BundledLocale {
    locale: &'static str,
    json: &'static str,
    catalog: OnceLock<Result<SceneLocalizationCatalog, String>>,
}

impl BundledLocale {
    const fn new(locale: &'static str, json: &'static str) -> Self {
        Self {
            locale,
            json,
            catalog: OnceLock::new(),
        }
    }

    fn catalog(&'static self) -> Result<&'static SceneLocalizationCatalog, String> {
        self.catalog
            .get_or_init(|| {
                let catalog: SceneLocalizationCatalog =
                    serde_json::from_str(self.json).map_err(|error| {
                        format!(
                            "bundled BPSR {} scene localization is invalid: {error}",
                            self.locale
                        )
                    })?;
                if catalog.schema_version != 1
                    || catalog.locale != self.locale
                    || catalog.scenes.is_empty()
                    || catalog.scenes.len() > 10_000
                    || catalog.scenes.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                    || catalog
                        .scenes
                        .iter()
                        .any(|(id, name)| *id <= 0 || name.trim().is_empty())
                {
                    return Err(format!(
                        "bundled BPSR {} scene localization has an unsupported shape",
                        self.locale
                    ));
                }
                Ok(catalog)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

macro_rules! bundled_locale {
    ($static_name:ident, $locale:literal) => {
        static $static_name: BundledLocale = BundledLocale::new(
            $locale,
            include_str!(concat!(
                "../game-data/runtime/localization/",
                $locale,
                "/scene-names.v1.json"
            )),
        );
    };
}

bundled_locale!(DE_DE, "de-DE");
bundled_locale!(EN_US, "en-US");
bundled_locale!(ES_ES, "es-ES");
bundled_locale!(FR_FR, "fr-FR");
bundled_locale!(ID_ID, "id-ID");
bundled_locale!(JA_JP, "ja-JP");
bundled_locale!(KO_KR, "ko-KR");
bundled_locale!(PT_BR, "pt-BR");
bundled_locale!(TH_TH, "th-TH");
bundled_locale!(ZH_CN, "zh-CN");
bundled_locale!(ZH_TW, "zh-TW");

fn bundled_locale(locale: &str) -> &'static BundledLocale {
    match locale {
        "de-DE" => &DE_DE,
        "en-US" => &EN_US,
        "es-ES" => &ES_ES,
        "fr-FR" => &FR_FR,
        "id-ID" => &ID_ID,
        "ja-JP" => &JA_JP,
        "ko-KR" => &KO_KR,
        "pt-BR" => &PT_BR,
        "th-TH" => &TH_TH,
        "zh-CN" => &ZH_CN,
        "zh-TW" => &ZH_TW,
        _ => &EN_US,
    }
}

/// Resolves current-build scene identity without loading localization data.
pub fn scene_presentation(scene_id: i64) -> Result<Option<&'static ScenePresentation>, String> {
    let catalog = presentation_catalog()?;
    Ok(catalog
        .scenes
        .binary_search_by_key(&scene_id, |scene| scene.scene_id)
        .ok()
        .map(|index| &catalog.scenes[index]))
}

/// Resolves a packet-derived scene ID through one independently lazy-loaded
/// official locale bundle. Scene 1 intentionally has no current game label.
pub fn localized_scene_name(scene_id: i64, locale: &str) -> Result<Option<&'static str>, String> {
    let catalog = bundled_locale(locale).catalog()?;
    Ok(catalog
        .scenes
        .binary_search_by_key(&scene_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.scenes[index].1.as_str()))
}

/// Resolves a scene label only when the captured artifact exactly matches the
/// build that supplied the bundled locale tables.
///
/// Presentation is deliberately fail-closed here: numeric scene identity
/// remains available for every build, but a label from a different deployment
/// or client build must never be presented as if it were observed there.
pub fn localized_scene_name_for_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    scene_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    if !bundled_localization_supports_identity(deployment_id, client_build, protocol_pack_digest)? {
        return Ok(None);
    }
    localized_scene_name(scene_id, locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED_LOCALES: &[&str] = &[
        "de-DE", "en-US", "es-ES", "fr-FR", "id-ID", "ja-JP", "ko-KR", "pt-BR", "th-TH", "zh-CN",
        "zh-TW",
    ];

    #[test]
    fn resolves_verified_guild_hunt_scene() {
        let normal = scene_presentation(12_022).unwrap().unwrap();
        assert_eq!(normal.scene_type, 2);
        assert_eq!(normal.scene_subtype, 9);
        assert_eq!(normal.parent_scene_id, 0);
        assert_eq!(normal.scene_resource_id, 18);
        assert_eq!(
            localized_scene_name(12_022, "en-US").unwrap(),
            Some("Guild Hunt - Normal")
        );

        let scene = scene_presentation(12_023).unwrap().unwrap();
        assert_eq!(scene.scene_type, 2);
        assert_eq!(scene.scene_subtype, 9);
        assert_eq!(scene.parent_scene_id, 0);
        assert_eq!(scene.scene_resource_id, 18);
        assert_eq!(
            localized_scene_name(12_023, "en-US").unwrap(),
            Some("Guild Hunt - Hard")
        );
    }

    #[test]
    fn resolves_current_master_sea_ringed_reef_scene() {
        let scene = scene_presentation(6565).unwrap().unwrap();
        assert_eq!(scene.scene_type, 2);
        assert_eq!(scene.scene_subtype, 5);
        assert_eq!(scene.scene_resource_id, 6561);
        assert_eq!(
            localized_scene_name(6565, "en-US").unwrap(),
            Some("Chaotic - Sea-Ringed Reef")
        );
    }

    #[test]
    fn resolves_current_master_mech_facility_scene() {
        let scene = scene_presentation(6525).unwrap().unwrap();
        assert_eq!(scene.scene_type, 2);
        assert_eq!(scene.scene_subtype, 5);
        assert_eq!(scene.scene_resource_id, 6521);
        assert_eq!(
            localized_scene_name(6525, "en-US").unwrap(),
            Some("Chaotic - Mech Facility")
        );
    }

    #[test]
    fn preserves_unnamed_and_unknown_scene_identity() {
        assert!(scene_presentation(1).unwrap().is_some());
        assert_eq!(localized_scene_name(1, "en-US").unwrap(), None);
        assert!(scene_presentation(20_043).unwrap().is_none());
        assert_eq!(localized_scene_name(20_043, "en-US").unwrap(), None);
    }

    #[test]
    fn unsupported_locale_uses_english_without_loading_every_language() {
        assert_eq!(
            localized_scene_name(12_023, "unsupported").unwrap(),
            Some("Guild Hunt - Hard")
        );
    }

    #[test]
    fn exact_build_lookup_fails_closed_across_builds_and_deployments() {
        assert_eq!(
            localized_scene_name_for_identity(
                "global",
                "24687926",
                "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                12_023,
                "en-US",
            )
            .unwrap(),
            Some("Guild Hunt - Hard")
        );
        assert_eq!(
            localized_scene_name_for_identity(
                "global",
                "24687927",
                "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                12_023,
                "en-US",
            )
            .unwrap(),
            None
        );
        assert_eq!(
            localized_scene_name_for_identity(
                "cn",
                "24687926",
                "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                12_023,
                "en-US",
            )
            .unwrap(),
            None
        );
        assert_eq!(
            localized_scene_name_for_identity(
                "global",
                "24687926",
                "sha256:wrong-pack",
                12_023,
                "en-US",
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn every_locale_has_the_same_reviewed_scene_identity_set() {
        let expected = EN_US.catalog().unwrap();
        let expected_ids = expected
            .scenes
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for source in [
            &DE_DE, &ES_ES, &FR_FR, &ID_ID, &JA_JP, &KO_KR, &PT_BR, &TH_TH, &ZH_CN, &ZH_TW,
        ] {
            let actual = source.catalog().unwrap();
            assert_eq!(
                actual.scenes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                expected_ids,
                "{} scene identity set differs from en-US",
                source.locale
            );
        }
    }

    #[test]
    fn current_build_reviewed_map_scene_coverage_gate_reports_every_gap() {
        use std::collections::BTreeSet;

        use sha2::{Digest, Sha256};

        const BUILD: &str = "24687926";
        const REVIEWED_MAPS: &str = include_str!(
            "../../../../apps/desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json"
        );

        let gate: serde_json::Value = serde_json::from_str(include_str!(
            "../game-data/catalog/coverage/reviewed-map-scenes.v1.json"
        ))
        .unwrap();
        assert_eq!(gate["schema_version"], 1);
        assert_eq!(gate["deployment_id"], "global");
        assert_eq!(gate["channel"], "steam");
        assert_eq!(gate["game_build"], BUILD);
        assert_eq!(
            gate["scope"],
            "scene-ids-with-reviewed-current-build-map-assets"
        );
        assert_eq!(gate["policy"]["exact_build_required"], true);
        assert_eq!(gate["policy"]["all_shipped_locales_required"], true);
        assert_eq!(gate["policy"]["raw_id_fallback_is_preserved"], true);
        assert_eq!(gate["policy"]["invented_labels_are_forbidden"], true);
        assert_eq!(gate["policy"]["unreported_coverage_drift_fails_ci"], true);
        assert_eq!(
            gate["source"]["path"],
            "../../../../../../apps/desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json"
        );
        assert_eq!(
            gate["source"]["sha256"].as_str(),
            Some(
                format!(
                    "{:x}",
                    Sha256::digest(REVIEWED_MAPS.replace("\r\n", "\n").as_bytes())
                )
                .as_str()
            ),
            "reviewed map inventory changed; review every map-backed scene ID"
        );

        let reviewed_maps: serde_json::Value = serde_json::from_str(REVIEWED_MAPS).unwrap();
        assert_eq!(reviewed_maps["schema_version"], 1);
        let maps = reviewed_maps["builds"][BUILD].as_array().unwrap();
        let mut reference_count = 0_u64;
        let mut reviewed_scene_ids = BTreeSet::new();
        for map in maps {
            for scene_id in map["scene_ids"].as_array().unwrap() {
                let scene_id = scene_id.as_i64().unwrap();
                assert!(scene_id > 0, "reviewed map has invalid scene ID {scene_id}");
                reference_count += 1;
                assert!(
                    reviewed_scene_ids.insert(scene_id),
                    "scene {scene_id} is assigned to more than one reviewed map asset"
                );
            }
        }

        let expected_uncovered = gate["summary"]["uncovered_scene_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_i64().unwrap())
            .collect::<BTreeSet<_>>();
        let mut actual_uncovered = BTreeSet::new();
        let mut malformed = Vec::new();
        for scene_id in &reviewed_scene_ids {
            let mut localized_locale_count = 0;
            for locale in SHIPPED_LOCALES {
                match localized_scene_name_for_identity(
                    "global",
                    BUILD,
                    localization_runtime_identity()
                        .unwrap()
                        .protocol_pack_digest
                        .as_str(),
                    *scene_id,
                    locale,
                ) {
                    Ok(Some(name)) if !name.trim().is_empty() && !name.contains('\u{fffd}') => {
                        if *locale == "en-US"
                            && (name.contains("Unresolved")
                                || name.chars().any(|character| {
                                    ('\u{3400}'..='\u{9fff}').contains(&character)
                                }))
                        {
                            malformed.push(format!(
                                "{scene_id}: en-US is not a user-facing English identity: {name}"
                            ));
                        }
                        localized_locale_count += 1;
                    }
                    Ok(Some(_)) => malformed.push(format!("{scene_id}: corrupt {locale} name")),
                    Ok(None) => {}
                    Err(error) => {
                        malformed.push(format!("{scene_id}: {locale} lookup error: {error}"));
                    }
                }
            }
            match localized_locale_count {
                0 => {
                    actual_uncovered.insert(*scene_id);
                }
                count if count == SHIPPED_LOCALES.len() => {}
                count => malformed.push(format!(
                    "{scene_id}: localized in only {count}/{} shipped locales",
                    SHIPPED_LOCALES.len()
                )),
            }
        }

        assert!(
            malformed.is_empty(),
            "reviewed map scene presentation errors:\n{}",
            malformed.join("\n")
        );
        assert_eq!(
            actual_uncovered, expected_uncovered,
            "reviewed map scene coverage changed; resolve or explicitly review every gap"
        );
        assert_eq!(
            gate["source"]["map_asset_count"].as_u64(),
            Some(maps.len() as u64)
        );
        assert_eq!(
            gate["source"]["scene_id_reference_count"].as_u64(),
            Some(reference_count)
        );
        assert_eq!(
            gate["summary"]["reviewed_map_scene_count"].as_u64(),
            Some(reviewed_scene_ids.len() as u64)
        );
        assert_eq!(
            gate["summary"]["localized_scene_count"].as_u64(),
            Some((reviewed_scene_ids.len() - actual_uncovered.len()) as u64)
        );
    }
}
