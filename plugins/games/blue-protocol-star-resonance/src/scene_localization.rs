use std::sync::OnceLock;

use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
