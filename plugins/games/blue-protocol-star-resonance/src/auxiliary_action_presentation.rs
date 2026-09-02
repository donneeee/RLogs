use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuxiliaryActionPresentation {
    pub skill_id: i64,
    pub icon: String,
    pub action_kind: String,
    pub maximum_tier: Option<u32>,
    pub replacement_imagine_skill_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuxiliaryActionPresentationCatalog {
    schema_version: u16,
    skills: Vec<AuxiliaryActionPresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuxiliaryActionLocalizationCatalog {
    schema_version: u16,
    locale: String,
    skills: Vec<(i64, String)>,
}

static PRESENTATION: OnceLock<Result<AuxiliaryActionPresentationCatalog, String>> = OnceLock::new();

fn presentation_catalog() -> Result<&'static AuxiliaryActionPresentationCatalog, String> {
    PRESENTATION
        .get_or_init(|| {
            let catalog: AuxiliaryActionPresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/auxiliary-action-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR auxiliary action presentation is invalid: {error}")
            })?;
            if catalog.schema_version != 1
                || catalog.skills.is_empty()
                || catalog.skills.len() > 128
                || catalog
                    .skills
                    .windows(2)
                    .any(|pair| pair[0].skill_id >= pair[1].skill_id)
                || catalog.skills.iter().any(|skill| {
                    skill.skill_id <= 0
                        || skill.icon.trim().is_empty()
                        || skill.replacement_imagine_skill_id.is_some_and(|id| id <= 0)
                        || match skill.action_kind.as_str() {
                            "role_skill" => {
                                skill.replacement_imagine_skill_id.is_some()
                                    || skill.maximum_tier.is_some()
                            }
                            "role_imagine" => {
                                skill.replacement_imagine_skill_id.is_none()
                                    || skill.maximum_tier != Some(4)
                            }
                            _ => true,
                        }
                })
            {
                return Err(
                    "bundled BPSR auxiliary action presentation has an unsupported shape".into(),
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
    catalog: OnceLock<Result<AuxiliaryActionLocalizationCatalog, String>>,
}

impl BundledLocale {
    const fn new(locale: &'static str, json: &'static str) -> Self {
        Self {
            locale,
            json,
            catalog: OnceLock::new(),
        }
    }

    fn catalog(&'static self) -> Result<&'static AuxiliaryActionLocalizationCatalog, String> {
        self.catalog
            .get_or_init(|| {
                let catalog: AuxiliaryActionLocalizationCatalog = serde_json::from_str(self.json)
                    .map_err(|error| {
                    format!(
                        "bundled BPSR {} auxiliary action localization is invalid: {error}",
                        self.locale
                    )
                })?;
                if catalog.schema_version != 1
                    || catalog.locale != self.locale
                    || catalog.skills.is_empty()
                    || catalog.skills.len() > 128
                    || catalog.skills.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                    || catalog
                        .skills
                        .iter()
                        .any(|(id, name)| *id <= 0 || name.trim().is_empty())
                {
                    return Err(format!(
                        "bundled BPSR {} auxiliary action localization has an unsupported shape",
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
                "/auxiliary-action-names.v1.json"
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

pub fn auxiliary_action_presentation(
    skill_id: i64,
) -> Result<Option<&'static AuxiliaryActionPresentation>, String> {
    let catalog = presentation_catalog()?;
    Ok(catalog
        .skills
        .binary_search_by_key(&skill_id, |skill| skill.skill_id)
        .ok()
        .map(|index| &catalog.skills[index]))
}

pub fn localized_auxiliary_action_name(
    skill_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let catalog = bundled_locale(locale).catalog()?;
    Ok(catalog
        .skills
        .binary_search_by_key(&skill_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.skills[index].1.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_capture_auxiliary_actions() {
        let thunderfall = auxiliary_action_presentation(3_021).unwrap().unwrap();
        assert_eq!(thunderfall.replacement_imagine_skill_id, Some(3_902));
        assert_eq!(thunderfall.action_kind, "role_imagine");
        assert_eq!(thunderfall.maximum_tier, Some(4));
        assert_eq!(
            localized_auxiliary_action_name(3_021, "en-US").unwrap(),
            Some("Thunderfall Grasp")
        );

        let unyielding = auxiliary_action_presentation(3_612).unwrap().unwrap();
        assert_eq!(unyielding.replacement_imagine_skill_id, None);
        assert_eq!(unyielding.action_kind, "role_skill");
        assert_eq!(unyielding.maximum_tier, None);
        assert_eq!(
            localized_auxiliary_action_name(3_612, "en-US").unwrap(),
            Some("Unyielding Spirit")
        );
    }

    #[test]
    fn every_locale_has_the_same_reviewed_action_identity_set() {
        let expected = EN_US.catalog().unwrap();
        let expected_ids = expected
            .skills
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for source in [
            &DE_DE, &ES_ES, &FR_FR, &ID_ID, &JA_JP, &KO_KR, &PT_BR, &TH_TH, &ZH_CN, &ZH_TW,
        ] {
            let actual = source.catalog().unwrap();
            assert_eq!(
                actual.skills.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                expected_ids,
                "{} auxiliary action identity set differs from en-US",
                source.locale
            );
        }
    }
}
