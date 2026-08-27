use std::sync::OnceLock;

use serde::Deserialize;

const MAXIMUM_COMBAT_ACTIONS: usize = 50_000;
const MAXIMUM_STATUS_EFFECTS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombatActionPresentation {
    pub ability_id: i64,
    pub kind: String,
    pub resolution: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub recount_group_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusEffectPresentation {
    pub effect_id: i64,
    pub kind: String,
    pub resolution: String,
    pub level: u32,
    pub technical_name: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CombatActionPresentationCatalog {
    schema_version: u16,
    actions: Vec<CombatActionPresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusEffectPresentationCatalog {
    schema_version: u16,
    effects: Vec<StatusEffectPresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CombatActionLocalizationCatalog {
    schema_version: u16,
    locale: String,
    actions: Vec<(i64, String)>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusEffectLocalizationCatalog {
    schema_version: u16,
    locale: String,
    effects: Vec<(i64, String)>,
}

static ACTION_PRESENTATION: OnceLock<Result<CombatActionPresentationCatalog, String>> =
    OnceLock::new();
static REVIEWED_ACTION_PRESENTATION: OnceLock<Result<CombatActionPresentationCatalog, String>> =
    OnceLock::new();
static EFFECT_PRESENTATION: OnceLock<Result<StatusEffectPresentationCatalog, String>> =
    OnceLock::new();

fn action_presentation_catalog() -> Result<&'static CombatActionPresentationCatalog, String> {
    ACTION_PRESENTATION
        .get_or_init(|| {
            let catalog: CombatActionPresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/combat-action-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR combat action presentation is invalid: {error}")
            })?;
            validate_action_presentation(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn reviewed_action_presentation_catalog() -> Result<&'static CombatActionPresentationCatalog, String>
{
    REVIEWED_ACTION_PRESENTATION
        .get_or_init(|| {
            let catalog: CombatActionPresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/reviewed-combat-action-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR reviewed combat action presentation is invalid: {error}")
            })?;
            validate_action_presentation(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn effect_presentation_catalog() -> Result<&'static StatusEffectPresentationCatalog, String> {
    EFFECT_PRESENTATION
        .get_or_init(|| {
            let catalog: StatusEffectPresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/status-effect-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR status effect presentation is invalid: {error}")
            })?;
            validate_effect_presentation(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn validate_action_presentation(catalog: &CombatActionPresentationCatalog) -> Result<(), String> {
    if catalog.schema_version != 1
        || catalog.actions.is_empty()
        || catalog.actions.len() > MAXIMUM_COMBAT_ACTIONS
        || catalog
            .actions
            .windows(2)
            .any(|pair| pair[0].ability_id >= pair[1].ability_id)
        || catalog.actions.iter().any(|action| {
            action.ability_id <= 0
                || action.kind.trim().is_empty()
                || !matches!(action.resolution.as_str(), "localized" | "unresolved")
                || action.recount_group_id.is_some_and(|id| id <= 0)
                || action.icon.as_deref().is_some_and(str::is_empty)
        })
    {
        return Err("bundled BPSR combat action presentation has an unsupported shape".into());
    }
    Ok(())
}

fn validate_effect_presentation(catalog: &StatusEffectPresentationCatalog) -> Result<(), String> {
    if catalog.schema_version != 1
        || catalog.effects.is_empty()
        || catalog.effects.len() > MAXIMUM_STATUS_EFFECTS
        || catalog
            .effects
            .windows(2)
            .any(|pair| pair[0].effect_id >= pair[1].effect_id)
        || catalog.effects.iter().any(|effect| {
            effect.effect_id <= 0
                || effect.kind.trim().is_empty()
                || !matches!(effect.resolution.as_str(), "localized" | "design-only")
                || (effect.resolution == "design-only"
                    && effect
                        .technical_name
                        .as_deref()
                        .is_none_or(|name| name.trim().is_empty() || name.contains('\u{fffd}')))
                || effect.icon.as_deref().is_some_and(str::is_empty)
        })
    {
        return Err("bundled BPSR status effect presentation has an unsupported shape".into());
    }
    Ok(())
}

struct BundledLocale {
    locale: &'static str,
    action_json: &'static str,
    reviewed_action_json: &'static str,
    effect_json: &'static str,
    actions: OnceLock<Result<CombatActionLocalizationCatalog, String>>,
    reviewed_actions: OnceLock<Result<CombatActionLocalizationCatalog, String>>,
    effects: OnceLock<Result<StatusEffectLocalizationCatalog, String>>,
}

impl BundledLocale {
    const fn new(
        locale: &'static str,
        action_json: &'static str,
        reviewed_action_json: &'static str,
        effect_json: &'static str,
    ) -> Self {
        Self {
            locale,
            action_json,
            reviewed_action_json,
            effect_json,
            actions: OnceLock::new(),
            reviewed_actions: OnceLock::new(),
            effects: OnceLock::new(),
        }
    }

    fn reviewed_actions(&'static self) -> Result<&'static CombatActionLocalizationCatalog, String> {
        self.reviewed_actions
            .get_or_init(|| {
                let catalog: CombatActionLocalizationCatalog = serde_json::from_str(
                    self.reviewed_action_json,
                )
                .map_err(|error| {
                    format!(
                        "bundled BPSR {} reviewed combat action localization is invalid: {error}",
                        self.locale
                    )
                })?;
                validate_localization(
                    self.locale,
                    catalog.schema_version,
                    &catalog.locale,
                    &catalog.actions,
                    MAXIMUM_COMBAT_ACTIONS,
                    "reviewed combat action",
                )?;
                Ok(catalog)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn actions(&'static self) -> Result<&'static CombatActionLocalizationCatalog, String> {
        self.actions
            .get_or_init(|| {
                let catalog: CombatActionLocalizationCatalog =
                    serde_json::from_str(self.action_json).map_err(|error| {
                        format!(
                            "bundled BPSR {} combat action localization is invalid: {error}",
                            self.locale
                        )
                    })?;
                validate_localization(
                    self.locale,
                    catalog.schema_version,
                    &catalog.locale,
                    &catalog.actions,
                    MAXIMUM_COMBAT_ACTIONS,
                    "combat action",
                )?;
                Ok(catalog)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn effects(&'static self) -> Result<&'static StatusEffectLocalizationCatalog, String> {
        self.effects
            .get_or_init(|| {
                let catalog: StatusEffectLocalizationCatalog =
                    serde_json::from_str(self.effect_json).map_err(|error| {
                        format!(
                            "bundled BPSR {} status effect localization is invalid: {error}",
                            self.locale
                        )
                    })?;
                validate_localization(
                    self.locale,
                    catalog.schema_version,
                    &catalog.locale,
                    &catalog.effects,
                    MAXIMUM_STATUS_EFFECTS,
                    "status effect",
                )?;
                Ok(catalog)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

fn validate_localization(
    expected_locale: &str,
    schema_version: u16,
    actual_locale: &str,
    rows: &[(i64, String)],
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    if schema_version != 1
        || actual_locale != expected_locale
        || rows.is_empty()
        || rows.len() > maximum
        || rows.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        || rows
            .iter()
            .any(|(id, name)| *id <= 0 || name.trim().is_empty())
    {
        return Err(format!(
            "bundled BPSR {expected_locale} {label} localization has an unsupported shape"
        ));
    }
    Ok(())
}

macro_rules! bundled_locale {
    ($static_name:ident, $locale:literal) => {
        static $static_name: BundledLocale = BundledLocale::new(
            $locale,
            include_str!(concat!(
                "../game-data/runtime/localization/",
                $locale,
                "/combat-action-names.v1.json"
            )),
            include_str!(concat!(
                "../game-data/runtime/localization/",
                $locale,
                "/reviewed-combat-action-names.v1.json"
            )),
            include_str!(concat!(
                "../game-data/runtime/localization/",
                $locale,
                "/status-effect-names.v1.json"
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

fn localized_action_from_catalog(
    catalog: &'static CombatActionLocalizationCatalog,
    ability_id: i64,
) -> Option<&'static str> {
    catalog
        .actions
        .binary_search_by_key(&ability_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.actions[index].1.as_str())
}

fn direct_action_has_user_facing_english(ability_id: i64) -> Result<bool, String> {
    let Some(name) = localized_action_from_catalog(EN_US.actions()?, ability_id) else {
        return Ok(false);
    };
    Ok(!name
        .chars()
        .any(|character| ('\u{3400}'..='\u{9fff}').contains(&character)))
}

pub fn combat_action_presentation(
    ability_id: i64,
) -> Result<Option<&'static CombatActionPresentation>, String> {
    let reviewed = reviewed_action_presentation_catalog()?;
    if let Ok(index) = reviewed
        .actions
        .binary_search_by_key(&ability_id, |action| action.ability_id)
    {
        return Ok(Some(&reviewed.actions[index]));
    }
    let catalog = action_presentation_catalog()?;
    Ok(catalog
        .actions
        .binary_search_by_key(&ability_id, |action| action.ability_id)
        .ok()
        .map(|index| &catalog.actions[index]))
}

pub fn status_effect_presentation(
    effect_id: i64,
) -> Result<Option<&'static StatusEffectPresentation>, String> {
    let catalog = effect_presentation_catalog()?;
    Ok(catalog
        .effects
        .binary_search_by_key(&effect_id, |effect| effect.effect_id)
        .ok()
        .map(|index| &catalog.effects[index]))
}

pub fn localized_combat_action_name(
    ability_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let locale = bundled_locale(locale);
    let presentation = combat_action_presentation(ability_id)?;
    let catalog = locale.actions()?;
    let direct_name = localized_action_from_catalog(catalog, ability_id);

    // A Recount relation adds a parent; it must not rename or hide the raw
    // child action. Prefer the action's direct SkillTable localization and use
    // the reviewed label only when the child has no direct localized identity.
    if presentation.is_some_and(|action| action.recount_group_id.is_some())
        && direct_name.is_some()
        && direct_action_has_user_facing_english(ability_id)?
    {
        return Ok(direct_name);
    }

    let reviewed = locale.reviewed_actions()?;
    if let Ok(index) = reviewed
        .actions
        .binary_search_by_key(&ability_id, |(id, _)| *id)
    {
        return Ok(Some(reviewed.actions[index].1.as_str()));
    }
    Ok(direct_name)
}

pub fn localized_recount_group_name(
    ability_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let Some(presentation) = combat_action_presentation(ability_id)? else {
        return Ok(None);
    };
    if presentation.recount_group_id.is_none() {
        return Ok(None);
    }

    let reviewed = bundled_locale(locale).reviewed_actions()?;
    Ok(reviewed
        .actions
        .binary_search_by_key(&ability_id, |(id, _)| *id)
        .ok()
        .map(|index| reviewed.actions[index].1.as_str()))
}

pub fn localized_status_effect_name(
    effect_id: i64,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let catalog = bundled_locale(locale).effects()?;
    Ok(catalog
        .effects
        .binary_search_by_key(&effect_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.effects[index].1.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_capture_skill_and_effect_source_actions() {
        let powerdraw = combat_action_presentation(2_233).unwrap().unwrap();
        assert_eq!(powerdraw.kind, "base-skill");
        assert_eq!(
            localized_combat_action_name(2_233, "en-US").unwrap(),
            Some("Powerdraw")
        );
        assert!(
            powerdraw
                .icon
                .as_deref()
                .is_some_and(|path| path.ends_with(".png"))
        );

        let steel_beak = combat_action_presentation(2_203_521).unwrap().unwrap();
        assert_eq!(
            localized_combat_action_name(2_203_521, "en-US").unwrap(),
            Some("Steel Beak")
        );
        assert_eq!(steel_beak.resolution, "localized");
    }

    #[test]
    fn reviewed_observed_action_relations_override_unresolved_base_rows() {
        let recovery = combat_action_presentation(21_406).unwrap().unwrap();
        assert_eq!(recovery.resolution, "localized");
        assert_eq!(
            localized_combat_action_name(21_406, "en-US").unwrap(),
            Some("Grove Wish")
        );
        assert_eq!(
            localized_combat_action_name(2_203_311, "en-US").unwrap(),
            Some("Explosive Arrow")
        );
        assert_eq!(
            combat_action_presentation(2_203_311)
                .unwrap()
                .unwrap()
                .recount_group_id,
            Some(106)
        );
        assert!(
            localized_combat_action_name(3_059_080, "ja-JP")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn current_build_recount_relations_separate_identically_named_child_actions() {
        let cases = [
            (220_101, 75, "Bullseye", "Bullseye"),
            (220_103, 75, "Bullseye", "Bullseye"),
            (220_106, 77, "Bullseye", "Double Arrow"),
            (220_108, 106, "Bullseye", "Explosive Arrow"),
            (220_110, 87, "Bullseye", "Blast Shot"),
            (220_111, 78, "Bullseye", "Quadraflare"),
            (220_113, 96, "Bullseye", "Phantom Falcon"),
            (2_203_521, 101, "Steel Beak", "Implosion"),
        ];

        for (ability_id, recount_group_id, expected_child_name, expected_parent_name) in cases {
            let presentation = combat_action_presentation(ability_id)
                .unwrap()
                .unwrap_or_else(|| panic!("current-build action {ability_id} is absent"));
            assert_eq!(presentation.recount_group_id, Some(recount_group_id));
            assert_eq!(
                localized_combat_action_name(ability_id, "en-US").unwrap(),
                Some(expected_child_name)
            );
            assert_eq!(
                localized_recount_group_name(ability_id, "en-US").unwrap(),
                Some(expected_parent_name)
            );
        }
    }

    #[test]
    fn every_generated_current_build_recount_action_keeps_a_parent_relation() {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../game-data/catalog/combat-actions/current-build-recount.v1.json"
        ))
        .unwrap();
        let actions = source["actions"].as_array().unwrap();
        assert!(actions.len() >= 400);

        for action in actions {
            let ability_id = action["ability_id"].as_i64().unwrap();
            let presentation = combat_action_presentation(ability_id)
                .unwrap()
                .unwrap_or_else(|| panic!("generated Recount child {ability_id} is absent"));
            assert!(
                presentation.recount_group_id.is_some(),
                "generated Recount child {ability_id} lost its parent relation"
            );
        }
    }

    #[test]
    fn every_observed_player_action_is_resolved_for_the_current_build() {
        const OBSERVED_PLAYER_ACTIONS: &[i64] = &[
            1_502, 1_503, 1_551, 1_902, 1_903, 1_904, 2_289, 2_302, 2_303, 2_304, 2_330, 2_402,
            2_403, 2_404, 3_524, 3_525, 100_730, 220_203, 220_301, 230_401, 230_501, 391_007,
            391_008, 1_011_011, 1_121_508, 1_700_820, 1_700_825, 1_700_826, 1_700_827, 2_002_853,
            2_203_291, 2_203_531, 3_003_213, 3_054_440, 3_059_210, 10_040_102,
        ];
        const LOCALES: &[&str] = &[
            "de-DE", "en-US", "es-ES", "fr-FR", "id-ID", "ja-JP", "ko-KR", "pt-BR", "th-TH",
            "zh-CN", "zh-TW",
        ];

        for ability_id in OBSERVED_PLAYER_ACTIONS {
            let presentation = combat_action_presentation(*ability_id)
                .unwrap()
                .unwrap_or_else(|| panic!("observed player action {ability_id} is absent"));
            assert_ne!(
                presentation.resolution, "unresolved",
                "observed player action {ability_id} remains unresolved"
            );
            for locale in LOCALES {
                let name = localized_combat_action_name(*ability_id, locale)
                    .unwrap()
                    .unwrap_or_else(|| {
                        panic!("observed player action {ability_id} has no {locale} name")
                    });
                assert!(!name.trim().is_empty());
                assert!(
                    !name.contains('\u{fffd}'),
                    "observed player action {ability_id} has corrupt {locale} text"
                );
            }
            let english = localized_combat_action_name(*ability_id, "en-US")
                .unwrap()
                .unwrap();
            assert!(
                !english.contains("Unresolved")
                    && !english
                        .chars()
                        .any(|character| ('\u{3400}'..='\u{9fff}').contains(&character)),
                "observed player action {ability_id} has a non-English en-US label: {english}"
            );
        }
    }

    #[test]
    fn every_generated_observed_action_is_resolved_without_hiding_rows() {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../game-data/catalog/combat-actions/observed-technical.v1.json"
        ))
        .unwrap();
        let actions = source["actions"].as_array().unwrap();
        assert!(!actions.is_empty());
        for action in actions {
            let ability_id = action["ability_id"].as_i64().unwrap();
            let presentation = combat_action_presentation(ability_id)
                .unwrap()
                .unwrap_or_else(|| panic!("generated observed action {ability_id} is absent"));
            assert_ne!(presentation.resolution, "unresolved");
            for locale in [
                "de-DE", "en-US", "es-ES", "fr-FR", "id-ID", "ja-JP", "ko-KR", "pt-BR", "th-TH",
                "zh-CN", "zh-TW",
            ] {
                let name = localized_combat_action_name(ability_id, locale)
                    .unwrap()
                    .unwrap_or_else(|| {
                        panic!("generated observed action {ability_id} has no {locale} name")
                    });
                assert!(!name.trim().is_empty());
                assert!(!name.contains('\u{fffd}'));
            }
            let english = localized_combat_action_name(ability_id, "en-US")
                .unwrap()
                .unwrap();
            assert!(!english.contains("Unresolved"));
            assert!(
                !english
                    .chars()
                    .any(|character| ('\u{3400}'..='\u{9fff}').contains(&character)),
                "generated observed action {ability_id} has a non-English label: {english}"
            );
        }
    }

    #[test]
    fn keeps_design_only_effects_exactly_identified_without_claiming_localization() {
        let presentation = status_effect_presentation(2_203_291).unwrap().unwrap();
        assert_eq!(presentation.resolution, "design-only");
        assert_eq!(
            presentation.technical_name.as_deref(),
            Some("苍穹之落-子BUFF")
        );
        assert_eq!(
            localized_status_effect_name(2_203_291, "en-US").unwrap(),
            None
        );
    }

    #[test]
    fn includes_unnamed_current_build_buff_table_rows() {
        for (effect_id, level) in [(682_501, 1), (682_503, 3), (682_505, 5)] {
            let presentation = status_effect_presentation(effect_id).unwrap().unwrap();
            assert_eq!(presentation.resolution, "design-only");
            assert_eq!(presentation.level, level);
            let expected = format!("BuffTable {effect_id} (Level {level})");
            assert_eq!(
                presentation.technical_name.as_deref(),
                Some(expected.as_str())
            );
        }
    }

    #[test]
    fn every_current_build_buff_table_row_has_a_display_identity() {
        let catalog = effect_presentation_catalog().unwrap();
        assert!(!catalog.effects.is_empty());
        for effect in &catalog.effects {
            let name = localized_status_effect_name(effect.effect_id, "en-US")
                .unwrap()
                .or(effect.technical_name.as_deref())
                .unwrap_or_else(|| {
                    panic!(
                        "current-build BuffTable effect {} has no display identity",
                        effect.effect_id
                    )
                });
            assert!(!name.trim().is_empty());
            assert!(
                !name.contains('\u{fffd}'),
                "current-build BuffTable effect {} has corrupt display text",
                effect.effect_id
            );
        }
    }

    #[test]
    fn selected_locale_catalogs_are_independent_and_fall_back_during_generation() {
        assert_eq!(
            localized_combat_action_name(2_233, "ja-JP").unwrap(),
            Some("チャージショット")
        );
        assert!(
            localized_status_effect_name(21_412, "fr-FR")
                .unwrap()
                .is_some()
        );
    }
}
