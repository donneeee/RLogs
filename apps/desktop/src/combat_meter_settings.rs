use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerDetailPresentation {
    InAppLayer,
    Popover,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryPartyColorMode {
    #[default]
    PartyOrder,
    Randomized,
    Specialization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryPartyViewSettings {
    pub id: String,
    pub label: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub widths: BTreeMap<String, u16>,
    pub sort_key: String,
    pub sort_direction: String,
    #[serde(default = "default_history_detail_mode")]
    pub detail_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatMeterSettings {
    pub schema_version: u16,
    pub player_detail_presentation: PlayerDetailPresentation,
    #[serde(default = "default_true")]
    pub show_class: bool,
    #[serde(default = "default_true")]
    pub show_specialization: bool,
    #[serde(default = "default_true")]
    pub show_level: bool,
    #[serde(default = "default_true")]
    pub show_ability_score: bool,
    #[serde(default = "default_true")]
    pub show_seasonal_score: bool,
    #[serde(default = "default_true")]
    pub show_character_uid: bool,
    #[serde(default = "default_true")]
    pub show_party_icons: bool,
    #[serde(default = "default_true")]
    pub show_weapon: bool,
    #[serde(default = "default_true")]
    pub show_primary_imagines: bool,
    #[serde(default = "default_true")]
    pub show_role_loadout: bool,
    #[serde(default = "default_true")]
    pub show_history_player_column: bool,
    #[serde(default = "default_true")]
    pub show_history_damage_column: bool,
    #[serde(default = "default_true")]
    pub show_history_dps_column: bool,
    #[serde(default = "default_true")]
    pub show_history_encounter_dps_column: bool,
    #[serde(default = "default_true")]
    pub show_history_hps_column: bool,
    #[serde(default = "default_true")]
    pub show_history_tps_column: bool,
    #[serde(default = "default_true")]
    pub show_history_rdps_column: bool,
    #[serde(default = "default_true")]
    pub show_history_apm_column: bool,
    #[serde(default = "default_true")]
    pub show_history_deaths_column: bool,
    #[serde(default = "default_history_party_views")]
    pub history_party_views: Vec<HistoryPartyViewSettings>,
    #[serde(default)]
    pub history_party_color_mode: HistoryPartyColorMode,
    #[serde(default)]
    pub history_specialization_colors: BTreeMap<String, String>,
    #[serde(default = "default_history_body_font_size_px")]
    pub history_body_font_size_px: u16,
    #[serde(default = "default_history_heading_font_size_px")]
    pub history_heading_font_size_px: u16,
    #[serde(default = "default_history_table_font_size_px")]
    pub history_table_font_size_px: u16,
    #[serde(default = "default_history_metadata_font_size_px")]
    pub history_metadata_font_size_px: u16,
    #[serde(default = "default_history_metric_font_size_px")]
    pub history_metric_font_size_px: u16,
    #[serde(default = "default_history_icon_size_px")]
    pub history_icon_size_px: u16,
}

const fn default_true() -> bool {
    true
}

const fn default_history_body_font_size_px() -> u16 {
    15
}

const fn default_history_heading_font_size_px() -> u16 {
    24
}

const fn default_history_table_font_size_px() -> u16 {
    13
}

const fn default_history_metadata_font_size_px() -> u16 {
    11
}

const fn default_history_metric_font_size_px() -> u16 {
    18
}

const fn default_history_icon_size_px() -> u16 {
    48
}

impl Default for CombatMeterSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            player_detail_presentation: PlayerDetailPresentation::InAppLayer,
            show_class: true,
            show_specialization: true,
            show_level: true,
            show_ability_score: true,
            show_seasonal_score: true,
            show_character_uid: true,
            show_party_icons: true,
            show_weapon: true,
            show_primary_imagines: true,
            show_role_loadout: true,
            show_history_player_column: true,
            show_history_damage_column: true,
            show_history_dps_column: true,
            show_history_encounter_dps_column: true,
            show_history_hps_column: true,
            show_history_tps_column: true,
            show_history_rdps_column: true,
            show_history_apm_column: true,
            show_history_deaths_column: true,
            history_party_views: default_history_party_views(),
            history_party_color_mode: HistoryPartyColorMode::PartyOrder,
            history_specialization_colors: BTreeMap::new(),
            history_body_font_size_px: default_history_body_font_size_px(),
            history_heading_font_size_px: default_history_heading_font_size_px(),
            history_table_font_size_px: default_history_table_font_size_px(),
            history_metadata_font_size_px: default_history_metadata_font_size_px(),
            history_metric_font_size_px: default_history_metric_font_size_px(),
            history_icon_size_px: default_history_icon_size_px(),
        }
    }
}

#[derive(Debug)]
pub struct CombatMeterSettingsStore {
    path: PathBuf,
    settings: CombatMeterSettings,
}

impl CombatMeterSettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> CombatMeterSettings {
        self.settings.clone()
    }

    pub fn update(&mut self, settings: CombatMeterSettings) -> Result<CombatMeterSettings, String> {
        validate(&settings)?;
        write(&self.path, &settings)?;
        self.settings = settings;
        Ok(self.snapshot())
    }
}

fn load(path: &Path) -> Result<CombatMeterSettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CombatMeterSettings::default());
        }
        Err(error) => return Err(format!("could not inspect Combat Meter settings: {error}")),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Combat Meter settings exceed the 64 KiB safety limit".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read Combat Meter settings: {error}"))?;
    let settings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Combat Meter settings are invalid: {error}"))?;
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &CombatMeterSettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported Combat Meter settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    if settings.history_specialization_colors.len() > 256 {
        return Err("History specialization colors exceed the 256-entry safety limit".into());
    }
    validate_history_party_views(&settings.history_party_views)?;
    for (key, color) in &settings.history_specialization_colors {
        if key.is_empty()
            || key.len() > 64
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        {
            return Err(format!(
                "History specialization color key {key:?} is invalid"
            ));
        }
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!(
                "History specialization color for {key:?} is invalid"
            ));
        }
    }
    for (label, value, minimum, maximum) in [
        (
            "History body font size",
            settings.history_body_font_size_px,
            11,
            24,
        ),
        (
            "History heading font size",
            settings.history_heading_font_size_px,
            16,
            40,
        ),
        (
            "History table font size",
            settings.history_table_font_size_px,
            10,
            24,
        ),
        (
            "History metadata font size",
            settings.history_metadata_font_size_px,
            9,
            20,
        ),
        (
            "History metric font size",
            settings.history_metric_font_size_px,
            13,
            36,
        ),
        ("History icon size", settings.history_icon_size_px, 20, 64),
    ] {
        if !(minimum..=maximum).contains(&value) {
            return Err(format!(
                "{label} must be between {minimum} and {maximum} px"
            ));
        }
    }
    Ok(())
}

fn default_history_party_views() -> Vec<HistoryPartyViewSettings> {
    [
        (
            "damage",
            "Damage",
            &["player", "damage", "dps", "encounterDps", "rdps", "deaths"][..],
            "encounterDps",
        ),
        (
            "healing",
            "Healing",
            &[
                "player",
                "effectiveHealing",
                "healing",
                "shielding",
                "hps",
                "deaths",
            ][..],
            "hps",
        ),
        (
            "defense",
            "Defense",
            &["player", "damageTaken", "tps", "deaths"][..],
            "tps",
        ),
    ]
    .into_iter()
    .map(|(id, label, columns, sort_key)| HistoryPartyViewSettings {
        id: id.into(),
        label: label.into(),
        columns: columns.iter().map(|column| (*column).into()).collect(),
        widths: BTreeMap::new(),
        sort_key: sort_key.into(),
        sort_direction: "descending".into(),
        detail_mode: match id {
            "healing" => "healing",
            "defense" => "defense",
            _ => "damage",
        }
        .into(),
    })
    .collect()
}

fn validate_history_party_views(views: &[HistoryPartyViewSettings]) -> Result<(), String> {
    const COLUMNS: &[&str] = &[
        "player",
        "damage",
        "effectiveDamage",
        "damageTaken",
        "healing",
        "effectiveHealing",
        "shielding",
        "hits",
        "criticalRate",
        "dps",
        "encounterDps",
        "hps",
        "tps",
        "rdps",
        "rdpsGiven",
        "rdpsReceived",
        "apm",
        "deaths",
    ];
    if views.is_empty() || views.len() > 12 {
        return Err("History must contain between 1 and 12 party views".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for view in views {
        if view.id.is_empty()
            || view.id.len() > 40
            || !view
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || !ids.insert(view.id.as_str())
        {
            return Err(format!(
                "History party view {:?} has an invalid or duplicate ID",
                view.id
            ));
        }
        let label = view.label.trim();
        if label.is_empty() || label.len() > 32 {
            return Err(format!(
                "History party view {} has an invalid label",
                view.id
            ));
        }
        if view.columns.is_empty() || view.columns.len() > COLUMNS.len() {
            return Err(format!(
                "History party view {} has invalid columns",
                view.id
            ));
        }
        let mut columns = std::collections::BTreeSet::new();
        for column in &view.columns {
            if !COLUMNS.contains(&column.as_str()) || !columns.insert(column.as_str()) {
                return Err(format!(
                    "History party view {} contains an invalid or duplicate column",
                    view.id
                ));
            }
        }
        if !columns.contains(view.sort_key.as_str()) {
            return Err(format!(
                "History party view {} sort column must be visible",
                view.id
            ));
        }
        if view.sort_direction != "ascending" && view.sort_direction != "descending" {
            return Err(format!(
                "History party view {} sort direction is invalid",
                view.id
            ));
        }
        if !["damage", "healing", "defense"].contains(&view.detail_mode.as_str()) {
            return Err(format!(
                "History party view {} detail mode is invalid",
                view.id
            ));
        }
        for (column, width) in &view.widths {
            if !COLUMNS.contains(&column.as_str()) || !(24..=800).contains(width) {
                return Err(format!(
                    "History party view {} width for {} is invalid",
                    view.id, column
                ));
            }
        }
    }
    Ok(())
}

fn default_history_detail_mode() -> String {
    "damage".into()
}

fn write(path: &Path, settings: &CombatMeterSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Combat Meter settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Combat Meter settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode Combat Meter settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write Combat Meter settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_the_in_app_navigation_layer() {
        let settings = CombatMeterSettings::default();
        assert_eq!(
            settings.player_detail_presentation,
            PlayerDetailPresentation::InAppLayer
        );
        assert!(settings.show_class);
        assert!(settings.show_specialization);
        assert!(settings.show_level);
        assert!(settings.show_ability_score);
        assert!(settings.show_seasonal_score);
        assert!(settings.show_character_uid);
        assert!(settings.show_party_icons);
        assert!(settings.show_weapon);
        assert!(settings.show_primary_imagines);
        assert!(settings.show_role_loadout);
        assert!(settings.show_history_player_column);
        assert!(settings.show_history_damage_column);
        assert!(settings.show_history_dps_column);
        assert!(settings.show_history_encounter_dps_column);
        assert!(settings.show_history_hps_column);
        assert!(settings.show_history_tps_column);
        assert!(settings.show_history_rdps_column);
        assert!(settings.show_history_apm_column);
        assert!(settings.show_history_deaths_column);
        assert_eq!(settings.history_party_views.len(), 3);
        assert_eq!(settings.history_party_views[0].label, "Damage");
        assert_eq!(
            settings.history_party_color_mode,
            HistoryPartyColorMode::PartyOrder
        );
        assert!(settings.history_specialization_colors.is_empty());
        assert_eq!(settings.history_body_font_size_px, 15);
        assert_eq!(settings.history_heading_font_size_px, 24);
        assert_eq!(settings.history_table_font_size_px, 13);
        assert_eq!(settings.history_metadata_font_size_px, 11);
        assert_eq!(settings.history_metric_font_size_px, 18);
        assert_eq!(settings.history_icon_size_px, 48);
    }

    #[test]
    fn old_saved_settings_receive_history_sizing_defaults() {
        let settings: CombatMeterSettings = serde_json::from_str(
            r#"{
                "schemaVersion": 1,
                "playerDetailPresentation": "in_app_layer",
                "showClass": true,
                "showSpecialization": true,
                "showLevel": true,
                "showSeasonalScore": true,
                "showPartyIcons": true
            }"#,
        )
        .unwrap();
        assert_eq!(settings.history_body_font_size_px, 15);
        assert_eq!(settings.history_icon_size_px, 48);
        assert!(settings.show_ability_score);
        assert!(settings.show_character_uid);
        assert!(settings.show_weapon);
        assert!(settings.show_primary_imagines);
        assert!(settings.show_role_loadout);
        assert!(settings.show_history_player_column);
        assert!(settings.show_history_damage_column);
        assert!(settings.show_history_dps_column);
        assert!(settings.show_history_encounter_dps_column);
        assert!(settings.show_history_hps_column);
        assert!(settings.show_history_tps_column);
        assert!(settings.show_history_rdps_column);
        assert!(settings.show_history_apm_column);
        assert!(settings.show_history_deaths_column);
        assert_eq!(settings.history_party_views.len(), 3);
        assert_eq!(
            settings.history_party_color_mode,
            HistoryPartyColorMode::PartyOrder
        );
        assert!(settings.history_specialization_colors.is_empty());
        validate(&settings).unwrap();
    }

    #[test]
    fn validates_custom_specialization_colors() {
        let mut settings = CombatMeterSettings {
            history_party_color_mode: HistoryPartyColorMode::Specialization,
            ..CombatMeterSettings::default()
        };
        settings
            .history_specialization_colors
            .insert("117".into(), "#f97316".into());
        validate(&settings).unwrap();

        settings
            .history_specialization_colors
            .insert("unsafe key".into(), "orange".into());
        assert!(validate(&settings).is_err());
    }
}
