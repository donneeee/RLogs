use std::{collections::BTreeMap, sync::OnceLock};

use serde::Deserialize;

const MAXIMUM_COMBAT_ACTIONS: usize = 50_000;
const MAXIMUM_CAST_RECOUNT_RELATIONS: usize = 10_000;
const MAXIMUM_STATUS_EFFECTS: usize = 20_000;
const MAXIMUM_RDPS_ATTRIBUTION_EFFECTS: usize = 256;

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

/// Presentation-only identity for a production rDPS attribution endpoint.
///
/// Numeric effect and exact-build identity remain the runtime authority. These
/// reviewed names are deliberately kept outside every formula digest and must
/// never be used to select or enable an attribution rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RdpsAttributionEffectPresentation {
    pub effect_id: i64,
    pub name: String,
    pub resolution: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CombatActionPresentationCatalog {
    schema_version: u16,
    actions: Vec<CombatActionPresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CombatCastRecountRelation {
    ability_id: i64,
    recount_group_id: i64,
    evidence_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CombatCastRecountRelationCatalog {
    schema_version: u16,
    game_build: String,
    generation_scope: String,
    source_sha256: BTreeMap<String, String>,
    relations: Vec<CombatCastRecountRelation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusEffectPresentationCatalog {
    schema_version: u16,
    effects: Vec<StatusEffectPresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsAttributionEffectPresentationCatalog {
    schema_version: u16,
    deployment_id: String,
    game_build: String,
    locale: String,
    effects: Vec<RdpsAttributionEffectPresentation>,
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
static SUPPORT_GENERATED_ACTION_PRESENTATION: OnceLock<
    Result<CombatActionPresentationCatalog, String>,
> = OnceLock::new();
static REVIEWED_ACTION_PRESENTATION: OnceLock<Result<CombatActionPresentationCatalog, String>> =
    OnceLock::new();
static CAST_RECOUNT_RELATIONS: OnceLock<Result<CombatCastRecountRelationCatalog, String>> =
    OnceLock::new();
static EFFECT_PRESENTATION: OnceLock<Result<StatusEffectPresentationCatalog, String>> =
    OnceLock::new();
static RDPS_ATTRIBUTION_EFFECT_PRESENTATION: OnceLock<
    Result<RdpsAttributionEffectPresentationCatalog, String>,
> = OnceLock::new();

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

fn support_generated_action_presentation_catalog()
-> Result<&'static CombatActionPresentationCatalog, String> {
    SUPPORT_GENERATED_ACTION_PRESENTATION
        .get_or_init(|| {
            let catalog: CombatActionPresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/support-generated-combat-action-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR support-generated action presentation is invalid: {error}")
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

fn cast_recount_relation_catalog() -> Result<&'static CombatCastRecountRelationCatalog, String> {
    CAST_RECOUNT_RELATIONS
        .get_or_init(|| {
            let catalog: CombatCastRecountRelationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/combat-cast-recount-relations.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR cast/Recount relation catalog is invalid: {error}")
            })?;
            if catalog.schema_version != 1
                || catalog.game_build != "24687926"
                || catalog.generation_scope
                    != "all-exact-current-build-recount-damage-and-unambiguous-cast-relations"
                || catalog.source_sha256.len() != 4
                || [
                    "SkillTable",
                    "SkillEffectTable",
                    "SkillFightLevelTable",
                    "RecountTable",
                ]
                .into_iter()
                .any(|source| {
                    catalog.source_sha256.get(source).is_none_or(|digest| {
                        digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                })
                || catalog.relations.is_empty()
                || catalog.relations.len() > MAXIMUM_CAST_RECOUNT_RELATIONS
                || catalog
                    .relations
                    .windows(2)
                    .any(|pair| pair[0].ability_id >= pair[1].ability_id)
                || catalog.relations.iter().any(|relation| {
                    relation.ability_id <= 0
                        || relation.recount_group_id <= 0
                        || !matches!(
                            relation.evidence_kind.as_str(),
                            "recount-damage-id"
                                | "skill-effect-single-group"
                                | "skill-effect-name-match"
                                | "skill-next-chain"
                        )
                })
            {
                return Err(
                    "bundled BPSR cast/Recount relation catalog has an unsupported shape".into(),
                );
            }
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

fn rdps_attribution_effect_presentation_catalog()
-> Result<&'static RdpsAttributionEffectPresentationCatalog, String> {
    RDPS_ATTRIBUTION_EFFECT_PRESENTATION
        .get_or_init(|| {
            let catalog: RdpsAttributionEffectPresentationCatalog = serde_json::from_str(
                include_str!("../game-data/runtime/rdps-attribution-effect-presentation.v1.json"),
            )
            .map_err(|error| {
                format!("bundled BPSR rDPS attribution presentation is invalid: {error}")
            })?;
            validate_rdps_attribution_effect_presentation(&catalog)?;
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

fn validate_rdps_attribution_effect_presentation(
    catalog: &RdpsAttributionEffectPresentationCatalog,
) -> Result<(), String> {
    if catalog.schema_version != 1
        || catalog.deployment_id != "global"
        || catalog.game_build != "24687926"
        || catalog.locale != "en-US"
        || catalog.effects.is_empty()
        || catalog.effects.len() > MAXIMUM_RDPS_ATTRIBUTION_EFFECTS
        || catalog
            .effects
            .windows(2)
            .any(|pair| pair[0].effect_id >= pair[1].effect_id)
        || catalog.effects.iter().any(|effect| {
            effect.effect_id <= 0
                || effect.name.trim().is_empty()
                || effect.name.contains('\u{fffd}')
                || !matches!(
                    effect.resolution.as_str(),
                    "localized-status-effect" | "reviewed-source-name"
                )
        })
    {
        return Err("bundled BPSR rDPS attribution presentation has an unsupported shape".into());
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
    let support_generated = support_generated_action_presentation_catalog()?;
    if let Ok(index) = support_generated
        .actions
        .binary_search_by_key(&ability_id, |action| action.ability_id)
    {
        return Ok(Some(&support_generated.actions[index]));
    }
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

/// Returns the exact current-build Recount parent for a cast request or damage
/// action. Reviewed presentation relations take precedence; the generated
/// fallback is limited to direct RecountTable DamageIds and SkillEffectTable
/// routes that resolve unambiguously. It never estimates casts from hits.
pub fn combat_recount_group_id(ability_id: i64) -> Result<Option<i64>, String> {
    if let Some(recount_group_id) = combat_action_presentation(ability_id)?
        .and_then(|presentation| presentation.recount_group_id)
    {
        return Ok(Some(recount_group_id));
    }
    let catalog = cast_recount_relation_catalog()?;
    Ok(catalog
        .relations
        .binary_search_by_key(&ability_id, |relation| relation.ability_id)
        .ok()
        .map(|index| catalog.relations[index].recount_group_id))
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

pub fn rdps_attribution_effect_presentation(
    effect_id: i64,
    locale: &str,
) -> Result<Option<&'static RdpsAttributionEffectPresentation>, String> {
    let catalog = rdps_attribution_effect_presentation_catalog()?;
    if locale != catalog.locale {
        return Ok(None);
    }
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
    fn all_promoted_rdps_effects_have_exact_id_english_presentation() {
        let cases = [
            (31_602, "Inspire"),
            (55_228, "Luminary Bolt Vulnerability"),
            (55_333, "Encore"),
            (997_511, "Coordinated Strike"),
            (997_513, "Element Sharing"),
            (997_515, "Attribute Transfer"),
            (997_518, "Enhanced Synergy"),
            (997_534, "Synergy Luck Field"),
            (997_538, "Synergy Crit Field"),
            (997_570, "Tactical Blessing"),
            (998_542, "All-Class Aura"),
            (2_100_154, "Blessing"),
            (2_110_034, "Arcane! Time Decree — Lower CD"),
            (2_110_065, "Fiery Battle Will"),
            (
                2_110_096,
                "Arcane! Thunder Roar — Electro Shield (Thunderstrike)",
            ),
            (2_110_099, "Arcane! Poison Explosion — Vulnerability"),
            (2_110_125, "Highland Blood"),
            (2_110_140, "Mechanical Power"),
            (2_110_143, "Functional Amp"),
            (2_110_167, "Morale Reduction — Vulnerability"),
            (2_202_041, "Inspiration"),
            (2_204_471, "Critical Cold"),
            (2_207_252, "Stat Resonance"),
            (2_302_121, "Team Luck & Crit"),
            (2_302_421, "Life Wave"),
            (2_404_261, "Spring Breeze — Season 2 healer 2-piece"),
            (2_404_271, "Full Bloom"),
            (3_003_052, "Harmony Grace"),
            (3_003_411, "Endless Mind"),
        ];
        assert_eq!(cases.len(), 29);

        for (effect_id, expected_name) in cases {
            let presentation = rdps_attribution_effect_presentation(effect_id, "en-US")
                .unwrap()
                .unwrap_or_else(|| panic!("promoted rDPS effect {effect_id} is absent"));
            assert_eq!(presentation.name, expected_name);
        }
        assert!(
            rdps_attribution_effect_presentation(9_999_999, "en-US")
                .unwrap()
                .is_none()
        );
        assert!(
            rdps_attribution_effect_presentation(2_204_471, "ja-JP")
                .unwrap()
                .is_none()
        );
    }

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
    fn support_generated_actions_are_not_presented_as_native_kit_skills() {
        for ability_id in [230_401, 230_501] {
            let presentation = combat_action_presentation(ability_id).unwrap().unwrap();
            assert_eq!(presentation.kind, "support-generated-damage");
            assert_eq!(presentation.recount_group_id, Some(215));
            assert_eq!(
                localized_combat_action_name(ability_id, "en-US").unwrap(),
                Some("Encore")
            );
        }

        for ability_id in [2_207_141, 2_207_411] {
            let presentation = combat_action_presentation(ability_id).unwrap().unwrap();
            assert_eq!(presentation.kind, "support-generated-healing");
            assert_eq!(presentation.recount_group_id, Some(222));
            assert_eq!(
                localized_combat_action_name(ability_id, "en-US").unwrap(),
                Some("Note")
            );
        }
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
    fn falcon_damage_attr_rows_have_distinct_localized_breakdown_names() {
        for (damage_attr_id, expected_name, expected_group) in [
            (2_220_329_107, "Falcon Strike", 94),
            (2_220_329_109, "Falcon Lightning Strike", 95),
        ] {
            let presentation = combat_action_presentation(damage_attr_id)
                .unwrap()
                .unwrap_or_else(|| panic!("DamageAttr row {damage_attr_id} is absent"));
            assert_eq!(presentation.resolution, "localized");
            assert_eq!(
                crate::psychoscope_recount_parent_for_damage_id(damage_attr_id)
                    .unwrap()
                    .map(|parent| parent.recount_group_id),
                Some(expected_group)
            );
            assert_eq!(
                localized_combat_action_name(damage_attr_id, "en-US").unwrap(),
                Some(expected_name)
            );
        }
    }

    #[test]
    fn cast_requests_and_damage_rows_share_exact_current_build_recount_groups() {
        let cases = [
            (2_201, 322_010_100, 75),
            (2_202, 322_010_100, 75),
            (2_203, 322_010_100, 75),
            (2_204, 322_010_100, 75),
            (2_222, 322_010_600, 77),
            (2_233, 122_330_103, 84),
            (2_234, 25_524_003, 85),
            (2_238, 322_011_000, 87),
        ];

        for (cast_ability_id, damage_ability_id, expected_recount_group_id) in cases {
            assert_eq!(
                combat_recount_group_id(cast_ability_id).unwrap(),
                Some(expected_recount_group_id),
                "cast request {cast_ability_id} lost its exact Recount parent"
            );
            assert_eq!(
                combat_recount_group_id(damage_ability_id).unwrap(),
                Some(expected_recount_group_id),
                "damage action {damage_ability_id} lost its exact Recount parent"
            );
        }
    }

    #[test]
    fn reviewed_recount_relations_override_generated_fallbacks() {
        assert_eq!(combat_recount_group_id(1_735).unwrap(), Some(63));
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
