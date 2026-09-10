use std::sync::OnceLock;

use serde::Deserialize;

use crate::scene_localization::bundled_localization_supports_identity;

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
pub(crate) fn localized_monster_name(
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

/// Resolves a packet-derived monster label only for the exact deployment and
/// client build that supplied the bundled MonsterTable localization joins.
/// Unknown builds retain their numeric monster identity without borrowing a
/// possibly stale display name.
pub fn localized_monster_name_for_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    monster_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    if !bundled_localization_supports_identity(deployment_id, client_build, protocol_pack_digest)? {
        return Ok(None);
    }
    localized_monster_name(monster_id, locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae";

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
    fn build_scoped_monster_localization_fails_closed() {
        assert_eq!(
            localized_monster_name_for_identity("global", "24687926", DIGEST, 33_701, "en-US")
                .unwrap(),
            Some("Tina - Void Reverie")
        );
        assert_eq!(
            localized_monster_name_for_identity("global", "24687927", DIGEST, 33_701, "en-US")
                .unwrap(),
            None
        );
        assert_eq!(
            localized_monster_name_for_identity("cn", "24687926", DIGEST, 33_701, "en-US").unwrap(),
            None
        );
        assert_eq!(
            localized_monster_name_for_identity(
                "global",
                "24687926",
                "sha256:wrong-pack",
                33_701,
                "en-US",
            )
            .unwrap(),
            None
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

    #[test]
    fn current_build_observed_monster_coverage_gate_reports_every_gap() {
        use std::collections::BTreeSet;

        use sha2::{Digest, Sha256};

        const BUILD: &str = "24687926";
        const OBSERVED: &str =
            include_str!("../game-data/catalog/combat-actions/observed-technical.v1.json");
        const LOCALES: &[&str] = &[
            "de-DE", "en-US", "es-ES", "fr-FR", "id-ID", "ja-JP", "ko-KR", "pt-BR", "th-TH",
            "zh-CN", "zh-TW",
        ];

        let gate: serde_json::Value = serde_json::from_str(include_str!(
            "../game-data/catalog/coverage/observed-monsters.v1.json"
        ))
        .unwrap();
        assert_eq!(gate["schema_version"], 1);
        assert_eq!(gate["deployment_id"], "global");
        assert_eq!(gate["channel"], "steam");
        assert_eq!(gate["game_build"], BUILD);
        assert_eq!(
            gate["scope"],
            "packet-derived-static-monster-ids-in-saved-history-observed-actions"
        );
        assert_eq!(gate["policy"]["exact_build_required"], true);
        assert_eq!(gate["policy"]["all_shipped_locales_required"], true);
        assert_eq!(gate["policy"]["raw_id_fallback_is_preserved"], true);
        assert_eq!(gate["policy"]["invented_labels_are_forbidden"], true);
        assert_eq!(gate["policy"]["unreported_coverage_drift_fails_ci"], true);
        assert_eq!(
            gate["source"]["path"],
            "../combat-actions/observed-technical.v1.json"
        );
        assert_eq!(
            gate["source"]["sha256"].as_str(),
            Some(
                format!(
                    "{:x}",
                    Sha256::digest(OBSERVED.replace("\r\n", "\n").as_bytes())
                )
                .as_str()
            ),
            "observed action inventory changed; review every packet-derived monster ID"
        );

        let observed: serde_json::Value = serde_json::from_str(OBSERVED).unwrap();
        assert_eq!(observed["game_build"], BUILD);
        let mut reference_count = 0_u64;
        let mut observed_ids = BTreeSet::new();
        for action in observed["actions"].as_array().unwrap() {
            for monster_id in action["observed_monster_ids"].as_array().unwrap() {
                let monster_id = monster_id.as_i64().unwrap();
                assert!(monster_id > 0, "observed invalid monster ID {monster_id}");
                reference_count += 1;
                observed_ids.insert(monster_id);
            }
        }

        let expected_uncovered = gate["summary"]["uncovered_monster_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_i64().unwrap())
            .collect::<BTreeSet<_>>();
        let mut actual_uncovered = BTreeSet::new();
        let mut malformed = Vec::new();
        for monster_id in &observed_ids {
            let mut names = Vec::new();
            for locale in LOCALES {
                match localized_monster_name(*monster_id, locale) {
                    Ok(Some(name)) if !name.trim().is_empty() && !name.contains('\u{fffd}') => {
                        if *locale == "en-US"
                            && (name.contains("Unresolved")
                                || name.chars().any(|character| {
                                    ('\u{3400}'..='\u{9fff}').contains(&character)
                                }))
                        {
                            malformed.push(format!(
                                "{monster_id}: en-US is not a user-facing English identity: {name}"
                            ));
                        }
                        names.push(Some(name));
                    }
                    Ok(Some(_)) => {
                        malformed.push(format!("{monster_id}: corrupt {locale} name"));
                        names.push(None);
                    }
                    Ok(None) => names.push(None),
                    Err(error) => {
                        malformed.push(format!("{monster_id}: {locale} lookup error: {error}"));
                        names.push(None);
                    }
                }
            }
            let localized_locale_count = names.iter().filter(|name| name.is_some()).count();
            match localized_locale_count {
                0 => {
                    actual_uncovered.insert(*monster_id);
                }
                count if count == LOCALES.len() => {}
                count => malformed.push(format!(
                    "{monster_id}: localized in only {count}/{} shipped locales",
                    LOCALES.len()
                )),
            }
        }

        assert!(
            malformed.is_empty(),
            "observed monster presentation errors:\n{}",
            malformed.join("\n")
        );
        assert_eq!(
            actual_uncovered, expected_uncovered,
            "observed monster coverage changed; resolve or explicitly review every gap"
        );
        assert_eq!(
            gate["source"]["monster_id_reference_count"].as_u64(),
            Some(reference_count)
        );
        assert_eq!(
            gate["summary"]["observed_monster_count"].as_u64(),
            Some(observed_ids.len() as u64)
        );
        assert_eq!(
            gate["summary"]["localized_monster_count"].as_u64(),
            Some((observed_ids.len() - actual_uncovered.len()) as u64)
        );
    }
}
