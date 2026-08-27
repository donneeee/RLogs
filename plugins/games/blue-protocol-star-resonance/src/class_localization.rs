use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

const BUNDLED_CLASS_LOCALIZATION: &str =
    include_str!("../game-data/runtime/class-localization.v1.json");
const BUNDLED_SPECIALIZATION_LOCALIZATION: &str =
    include_str!("../game-data/runtime/specialization-localization.v1.json");
const BUNDLED_SPECIALIZATION_PRESENTATION: &str =
    include_str!("../game-data/runtime/specialization-presentation.v1.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassLocalizationCatalog {
    schema_version: u16,
    default_locale: String,
    classes: Vec<ClassLocalization>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassLocalization {
    class_id: i32,
    localization_key: String,
    icon: String,
    weapon_icon: Option<String>,
    names: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializationLocalizationCatalog {
    schema_version: u16,
    default_locale: String,
    locales: BTreeMap<String, BTreeMap<i32, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializationPresentationCatalog {
    schema_version: u16,
    specializations: Vec<SpecializationPresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializationPresentation {
    specialization_id: i32,
    class_id: i32,
    role: String,
    accent: String,
    icon: Option<String>,
    mapping_state: String,
}

fn catalog() -> Result<&'static ClassLocalizationCatalog, String> {
    static CATALOG: OnceLock<Result<ClassLocalizationCatalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog: ClassLocalizationCatalog =
                serde_json::from_str(BUNDLED_CLASS_LOCALIZATION).map_err(|error| {
                    format!("bundled BPSR class localization is invalid: {error}")
                })?;
            if catalog.schema_version != 1 || catalog.classes.len() > 64 {
                return Err("bundled BPSR class localization has an unsupported shape".into());
            }
            for class in &catalog.classes {
                let valid_weapon_icon = class.weapon_icon.as_deref().is_none_or(|icon| {
                    icon.starts_with("icons/weapons/classes/")
                        && icon.ends_with(".png")
                        && !icon.contains("..")
                });
                if class.localization_key != format!("class.{}.name", class.class_id)
                    || !class.names.contains_key(&catalog.default_locale)
                    || !class.icon.starts_with("icons/classes/")
                    || !class.icon.ends_with("/horizontal.png")
                    || class.icon.contains("..")
                    || !valid_weapon_icon
                {
                    return Err(format!(
                        "bundled BPSR class {} is missing its canonical name",
                        class.class_id
                    ));
                }
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn specialization_catalog() -> Result<&'static SpecializationLocalizationCatalog, String> {
    static CATALOG: OnceLock<Result<SpecializationLocalizationCatalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog: SpecializationLocalizationCatalog =
                serde_json::from_str(BUNDLED_SPECIALIZATION_LOCALIZATION).map_err(|error| {
                    format!("bundled BPSR specialization localization is invalid: {error}")
                })?;
            if catalog.schema_version != 1
                || catalog.locales.len() > 32
                || !catalog.locales.contains_key(&catalog.default_locale)
                || catalog.locales.values().any(|names| names.len() > 64)
            {
                return Err(
                    "bundled BPSR specialization localization has an unsupported shape".into(),
                );
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn specialization_presentation_catalog()
-> Result<&'static SpecializationPresentationCatalog, String> {
    static CATALOG: OnceLock<Result<SpecializationPresentationCatalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog: SpecializationPresentationCatalog =
                serde_json::from_str(BUNDLED_SPECIALIZATION_PRESENTATION).map_err(|error| {
                    format!("bundled BPSR specialization presentation is invalid: {error}")
                })?;
            if catalog.schema_version != 1 || catalog.specializations.len() > 64 {
                return Err(
                    "bundled BPSR specialization presentation has an unsupported shape".into(),
                );
            }
            let mut ids = std::collections::BTreeSet::new();
            for specialization in &catalog.specializations {
                let valid_icon = specialization.icon.as_deref().is_none_or(|icon| {
                    icon.starts_with("icons/talents/shared/")
                        && icon.ends_with(".png")
                        && !icon.contains("..")
                });
                if !ids.insert(specialization.specialization_id)
                    || specialization.class_id <= 0
                    || !matches!(specialization.role.as_str(), "damage" | "healer" | "tank")
                    || !matches!(specialization.accent.as_str(), "none" | "damage_glow")
                    || !matches!(
                        specialization.mapping_state.as_str(),
                        "current_build_talent_tree" | "unresolved_current_build_talent_tree"
                    )
                    || !valid_icon
                {
                    return Err(format!(
                        "bundled BPSR specialization {} has invalid presentation metadata",
                        specialization.specialization_id
                    ));
                }
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Returns the plug-in-owned relative class icon path. The desktop host may
/// expose this under its read-only game-asset route without hardcoding BPSR
/// class folders into the game-neutral UI.
pub fn class_icon_path(class_id: i32) -> Result<Option<&'static str>, String> {
    Ok(catalog()?
        .classes
        .iter()
        .find(|class| class.class_id == class_id)
        .map(|class| class.icon.as_str()))
}

/// Returns the exact class-weapon icon referenced by the current-build
/// ProfessionSystemTable. This is a weapon-category glyph, not guessed item
/// artwork; classes without a proven game icon deliberately return `None`.
pub fn class_weapon_icon_path(class_id: i32) -> Result<Option<&'static str>, String> {
    Ok(catalog()?
        .classes
        .iter()
        .find(|class| class.class_id == class_id)
        .and_then(|class| class.weapon_icon.as_deref()))
}

/// Returns the reviewed talent-tree specialization icon. Missing mappings are
/// deliberately `None` so a caller can use the class icon without guessing.
pub fn specialization_icon_path(specialization_id: i32) -> Result<Option<&'static str>, String> {
    Ok(specialization_presentation_catalog()?
        .specializations
        .iter()
        .find(|specialization| specialization.specialization_id == specialization_id)
        .and_then(|specialization| specialization.icon.as_deref()))
}

/// Returns the class that owns a reviewed specialization. Consumers must use
/// this relationship before combining class and specialization evidence; both
/// values can arrive through independently timed packet/profile updates.
pub fn specialization_class_id(specialization_id: i32) -> Result<Option<i32>, String> {
    Ok(specialization_presentation_catalog()?
        .specializations
        .iter()
        .find(|specialization| specialization.specialization_id == specialization_id)
        .map(|specialization| specialization.class_id))
}

/// Returns the class role used only for BPSR presentation coloring.
pub fn specialization_role(specialization_id: i32) -> Result<Option<&'static str>, String> {
    Ok(specialization_presentation_catalog()?
        .specializations
        .iter()
        .find(|specialization| specialization.specialization_id == specialization_id)
        .map(|specialization| specialization.role.as_str()))
}

/// Returns the common role of the class's reviewed specializations. This is a
/// fallback for class-named party companions that do not expose a spec ID.
pub fn class_role(class_id: i32) -> Result<Option<&'static str>, String> {
    Ok(specialization_presentation_catalog()?
        .specializations
        .iter()
        .find(|specialization| specialization.class_id == class_id)
        .map(|specialization| specialization.role.as_str()))
}

/// Returns an optional visual accent. Smite and Dissonance remain healer-green
/// while receiving the requested damage-red glow.
pub fn specialization_accent(specialization_id: i32) -> Result<Option<&'static str>, String> {
    Ok(specialization_presentation_catalog()?
        .specializations
        .iter()
        .find(|specialization| specialization.specialization_id == specialization_id)
        .and_then(|specialization| {
            (specialization.accent != "none").then_some(specialization.accent.as_str())
        }))
}

/// Returns the current-build class name without loading the full localization
/// corpus. The compact bundle is generated from the human-readable class and
/// per-language localization catalogs.
pub fn localized_class_name(class_id: i32, locale: &str) -> Result<Option<&'static str>, String> {
    let catalog = catalog()?;
    let Some(class) = catalog
        .classes
        .iter()
        .find(|class| class.class_id == class_id)
    else {
        return Ok(None);
    };
    Ok(class
        .names
        .get(locale)
        .or_else(|| class.names.get(&catalog.default_locale))
        .map(String::as_str))
}

/// Returns a compact localized specialization label without loading the full
/// game localization corpus. Unknown locales fall back to the official
/// English game label until their compact language bundle is installed.
pub fn localized_specialization_name(
    specialization_id: i32,
    locale: &str,
) -> Result<Option<&'static str>, String> {
    let catalog = specialization_catalog()?;
    Ok(catalog
        .locales
        .get(locale)
        .or_else(|| catalog.locales.get(&catalog.default_locale))
        .and_then(|names| names.get(&specialization_id))
        .map(String::as_str))
}

/// Returns the compact class identities available to game-neutral consumers
/// such as the overlay color editor. The consumer receives numeric keys and
/// localized labels without loading or duplicating the BPSR catalog.
pub fn localized_class_identities(locale: &str) -> Result<Vec<(i32, String)>, String> {
    let catalog = catalog()?;
    Ok(catalog
        .classes
        .iter()
        .filter_map(|class| {
            class
                .names
                .get(locale)
                .or_else(|| class.names.get(&catalog.default_locale))
                .map(|name| (class.class_id, name.clone()))
        })
        .collect())
}

/// Returns every reviewed specialization identity and its compact localized
/// label. This remains plug-in-owned so the overlay never hardcodes a game's
/// class tree.
pub fn localized_specialization_identities(locale: &str) -> Result<Vec<(i32, String)>, String> {
    let localization = specialization_catalog()?;
    let names = localization
        .locales
        .get(locale)
        .or_else(|| localization.locales.get(&localization.default_locale))
        .ok_or_else(|| {
            "bundled BPSR specialization localization has no fallback locale".to_owned()
        })?;
    let presentations = specialization_presentation_catalog()?;
    Ok(presentations
        .specializations
        .iter()
        .filter_map(|specialization| {
            names
                .get(&specialization.specialization_id)
                .map(|name| (specialization.specialization_id, name.clone()))
        })
        .collect())
}

/// Identifies server-provided class labels in any supported game language.
/// This is used to distinguish class-named companion actors from characters
/// whose actual profile name has been observed.
pub fn is_localized_class_name(class_id: i32, value: &str) -> Result<bool, String> {
    let value = value.trim();
    Ok(catalog()?
        .classes
        .iter()
        .find(|class| class.class_id == class_id)
        .is_some_and(|class| class.names.values().any(|name| name == value)))
}

/// BPSR player-like entity UUIDs carry the stable character UID above the
/// low 16 runtime-instance bits. This helper stays in the game integration so
/// the game-neutral Combat Meter never adopts that wire-specific rule.
pub fn character_id_from_entity_uuid(entity_uuid: i64) -> Option<String> {
    let value = u64::try_from(entity_uuid).ok()?;
    let character_id = value >> 16;
    (character_id > 0).then(|| character_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_build_class_names_and_cross_locale_aliases() {
        assert_eq!(
            localized_class_name(4, "en-US").unwrap(),
            Some("Wind Knight")
        );
        assert!(is_localized_class_name(4, "青岚骑士").unwrap());
        assert_eq!(localized_class_name(11, "en-US").unwrap(), Some("Marksman"));
        assert_eq!(
            localized_specialization_name(117, "en-US").unwrap(),
            Some("Falconry Spec")
        );
        assert_eq!(
            localized_specialization_name(117, "es-ES").unwrap(),
            Some("Especialización de Cetrería")
        );
        assert_eq!(
            class_icon_path(11).unwrap(),
            Some("icons/classes/marksman/horizontal.png")
        );
        assert_eq!(
            class_weapon_icon_path(11).unwrap(),
            Some("icons/weapons/classes/marksman.png")
        );
        assert_eq!(class_weapon_icon_path(8).unwrap(), None);
        assert_eq!(
            specialization_icon_path(117).unwrap(),
            Some("icons/talents/shared/marksman-falconry-spec-1129-falconry-spec.png")
        );
        assert_eq!(specialization_role(117).unwrap(), Some("damage"));
        assert_eq!(specialization_class_id(117).unwrap(), Some(11));
        assert_eq!(specialization_class_id(119).unwrap(), Some(13));
        assert_eq!(class_role(11).unwrap(), Some("damage"));
        assert_eq!(class_role(9).unwrap(), Some("tank"));
        assert_eq!(specialization_role(110).unwrap(), Some("healer"));
        assert_eq!(specialization_accent(110).unwrap(), Some("damage_glow"));
        assert_eq!(specialization_accent(111).unwrap(), None);
        assert_eq!(specialization_icon_path(125).unwrap(), None);
    }

    #[test]
    fn exposes_complete_localized_identities_for_game_neutral_editors() {
        let classes = localized_class_identities("en-US").unwrap();
        let specializations = localized_specialization_identities("en-US").unwrap();

        assert!(classes.contains(&(11, "Marksman".to_owned())));
        assert!(specializations.contains(&(117, "Falconry Spec".to_owned())));
        assert_eq!(classes.len(), catalog().unwrap().classes.len());
        assert_eq!(
            specializations.len(),
            specialization_presentation_catalog()
                .unwrap()
                .specializations
                .len()
        );
    }

    #[test]
    fn extracts_confirmed_character_uid_from_player_entity_uuid() {
        assert_eq!(
            character_id_from_entity_uuid(216_009_015_936).as_deref(),
            Some("3296036")
        );
    }
}
