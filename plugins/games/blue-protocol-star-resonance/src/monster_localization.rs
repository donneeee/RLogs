use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MonsterLocalizationCatalog {
    schema_version: u16,
    locale: String,
    monsters: Vec<(i64, String)>,
}

struct BundledLocale {
    locale: &'static str,
    json: &'static str,
    catalog: OnceLock<Result<MonsterLocalizationCatalog, String>>,
}

impl BundledLocale {
    const fn new(locale: &'static str, json: &'static str) -> Self {
        Self {
            locale,
            json,
            catalog: OnceLock::new(),
        }
    }

    fn catalog(&'static self) -> Result<&'static MonsterLocalizationCatalog, String> {
        self.catalog
            .get_or_init(|| {
                let catalog: MonsterLocalizationCatalog =
                    serde_json::from_str(self.json).map_err(|error| {
                        format!(
                            "bundled BPSR {} monster localization is invalid: {error}",
                            self.locale
                        )
                    })?;
                if catalog.schema_version != 1
                    || catalog.locale != self.locale
                    || catalog.monsters.is_empty()
                    || catalog.monsters.len() > 100_000
                    || catalog
                        .monsters
                        .windows(2)
                        .any(|pair| pair[0].0 >= pair[1].0)
                    || catalog
                        .monsters
                        .iter()
                        .any(|(id, name)| *id <= 0 || name.trim().is_empty())
                {
                    return Err(format!(
                        "bundled BPSR {} monster localization has an unsupported shape",
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
                "/monster-names.v1.json"
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

/// Resolves a packet-derived static monster ID through the reviewed current-
/// build game catalog. Each locale is parsed independently on first use; the
/// full game localization corpus and unused languages never enter the heap.
pub fn localized_monster_name(
    monster_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let catalog = bundled_locale(locale).catalog()?;
    Ok(catalog
        .monsters
        .binary_search_by_key(&monster_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.monsters[index].1.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_build_monsters_without_cross_locale_loading() {
        assert_eq!(
            localized_monster_name(33_701, "en-US").unwrap(),
            Some("Tina - Void Reverie")
        );
        assert_eq!(
            localized_monster_name(33_701, "id-ID").unwrap(),
            Some("Tina - Void Mind")
        );
        assert_eq!(
            localized_monster_name(9_999_999_999, "en-US").unwrap(),
            None
        );
        assert_eq!(
            localized_monster_name(33_701, "unsupported").unwrap(),
            Some("Tina - Void Reverie")
        );
    }

    #[test]
    fn every_supported_locale_bundle_has_the_same_reviewed_identity_set() {
        let expected = EN_US.catalog().unwrap();
        let expected_ids = expected
            .monsters
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for source in [
            &DE_DE, &ES_ES, &FR_FR, &ID_ID, &JA_JP, &KO_KR, &PT_BR, &TH_TH, &ZH_CN, &ZH_TW,
        ] {
            let actual = source.catalog().unwrap();
            assert_eq!(
                actual
                    .monsters
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                expected_ids,
                "{} monster identity set differs from en-US",
                source.locale
            );
        }
    }
}
