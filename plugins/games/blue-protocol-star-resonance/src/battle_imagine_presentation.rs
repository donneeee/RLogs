use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleImaginePresentation {
    pub skill_id: i64,
    pub item_id: i64,
    pub item_tier: u32,
    pub maximum_tier: u32,
    pub icon: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleImaginePresentationCatalog {
    schema_version: u16,
    imagines: Vec<BattleImaginePresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleImagineLocalizationCatalog {
    schema_version: u16,
    locale: String,
    imagines: Vec<(i64, String)>,
}

static PRESENTATION: OnceLock<Result<BattleImaginePresentationCatalog, String>> = OnceLock::new();

fn presentation_catalog() -> Result<&'static BattleImaginePresentationCatalog, String> {
    PRESENTATION
        .get_or_init(|| {
            let catalog: BattleImaginePresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/battle-imagine-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR Battle Imagine presentation is invalid: {error}")
            })?;
            if catalog.schema_version != 1
                || catalog.imagines.is_empty()
                || catalog.imagines.len() > 512
                || catalog
                    .imagines
                    .windows(2)
                    .any(|pair| pair[0].skill_id >= pair[1].skill_id)
                || catalog.imagines.iter().any(|imagine| {
                    imagine.skill_id <= 0
                        || imagine.item_id <= 0
                        || imagine.item_tier == 0
                        || imagine.maximum_tier == 0
                        || imagine.icon.trim().is_empty()
                })
            {
                return Err(
                    "bundled BPSR Battle Imagine presentation has an unsupported shape".into(),
                );
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

struct BundledLocale {
    locale: &'static str,
    json: &'static str,
    catalog: OnceLock<Result<BattleImagineLocalizationCatalog, String>>,
}

impl BundledLocale {
    const fn new(locale: &'static str, json: &'static str) -> Self {
        Self {
            locale,
            json,
            catalog: OnceLock::new(),
        }
    }

    fn catalog(&'static self) -> Result<&'static BattleImagineLocalizationCatalog, String> {
        self.catalog
            .get_or_init(|| {
                let catalog: BattleImagineLocalizationCatalog = serde_json::from_str(self.json)
                    .map_err(|error| {
                        format!(
                            "bundled BPSR {} Battle Imagine localization is invalid: {error}",
                            self.locale
                        )
                    })?;
                if catalog.schema_version != 1
                    || catalog.locale != self.locale
                    || catalog.imagines.is_empty()
                    || catalog.imagines.len() > 512
                    || catalog
                        .imagines
                        .windows(2)
                        .any(|pair| pair[0].0 >= pair[1].0)
                    || catalog
                        .imagines
                        .iter()
                        .any(|(id, name)| *id <= 0 || name.trim().is_empty())
                {
                    return Err(format!(
                        "bundled BPSR {} Battle Imagine localization has an unsupported shape",
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
                "/battle-imagine-names.v1.json"
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

pub fn battle_imagine_presentation(
    skill_id: i64,
) -> Result<Option<&'static BattleImaginePresentation>, String> {
    let catalog = presentation_catalog()?;
    Ok(catalog
        .imagines
        .binary_search_by_key(&skill_id, |imagine| imagine.skill_id)
        .ok()
        .map(|index| &catalog.imagines[index]))
}

pub fn localized_battle_imagine_name(
    item_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let catalog = bundled_locale(locale).catalog()?;
    Ok(catalog
        .imagines
        .binary_search_by_key(&item_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.imagines[index].1.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_capture_primary_imagines() {
        let rorola = battle_imagine_presentation(3_948).unwrap().unwrap();
        assert_eq!(rorola.item_id, 3_000_101);
        assert_eq!(rorola.maximum_tier, 5);
        assert_eq!(
            localized_battle_imagine_name(rorola.item_id, "en-US").unwrap(),
            Some("Battle Imagine - Rorola")
        );

        let igoreus = battle_imagine_presentation(3_969).unwrap().unwrap();
        assert_eq!(igoreus.item_id, 3_000_121);
        assert_eq!(
            localized_battle_imagine_name(igoreus.item_id, "ja-JP")
                .unwrap()
                .is_some(),
            true
        );
    }

    #[test]
    fn every_locale_has_the_same_reviewed_imagine_identity_set() {
        let expected = EN_US.catalog().unwrap();
        let expected_ids = expected
            .imagines
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for source in [
            &DE_DE, &ES_ES, &FR_FR, &ID_ID, &JA_JP, &KO_KR, &PT_BR, &TH_TH, &ZH_CN, &ZH_TW,
        ] {
            let actual = source.catalog().unwrap();
            assert_eq!(
                actual
                    .imagines
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                expected_ids,
                "{} Battle Imagine identity set differs from en-US",
                source.locale
            );
        }
    }
}
