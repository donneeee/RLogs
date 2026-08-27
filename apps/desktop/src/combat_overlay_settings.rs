use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 128 * 1024;
const MAX_LAYERS: usize = 24;
const MAX_HEADER_FIELDS: usize = 32;
const MAX_SUMMARY_FIELDS: usize = 8;
const MIN_CANVAS_HEIGHT: u16 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayBackgroundMode {
    Transparent,
    Solid,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMetric {
    Dps,
    Edps,
    Bdps,
    Rdps,
    Hps,
    Tps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayBarColorMode {
    Random,
    Class,
    Specialization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayNumberFormat {
    Compact,
    Detailed,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayNumberFormats {
    pub player_metrics: OverlayNumberFormat,
    pub percentages: OverlayNumberFormat,
    pub summary_totals: OverlayNumberFormat,
    pub boss_health: OverlayNumberFormat,
    pub boss_metrics: OverlayNumberFormat,
    pub skill_values: OverlayNumberFormat,
    pub counts: OverlayNumberFormat,
}

impl Default for OverlayNumberFormats {
    fn default() -> Self {
        Self {
            player_metrics: OverlayNumberFormat::Detailed,
            percentages: OverlayNumberFormat::Compact,
            summary_totals: OverlayNumberFormat::Detailed,
            boss_health: OverlayNumberFormat::Detailed,
            boss_metrics: OverlayNumberFormat::Detailed,
            skill_values: OverlayNumberFormat::Detailed,
            counts: OverlayNumberFormat::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayHeaderField {
    Rank,
    ClassSpec,
    Name,
    Weapon,
    MainImagines,
    Damage,
    EffectiveDamage,
    HpDamage,
    ShieldDamage,
    Dps,
    Edps,
    Bdps,
    Rdps,
    Hps,
    Tps,
    Healing,
    EffectiveHealing,
    Overheal,
    Shielding,
    DamageTaken,
    Hits,
    CriticalRate,
    Casts,
    Deaths,
    Revives,
    RdpsDamage,
    ContributionGiven,
    ContributionReceived,
    /// Legacy generic metric column. The UI migrates this to the view's
    /// explicit metric column when settings are loaded.
    Value,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlaySummaryField {
    EncounterTime,
    RunTime,
    GameTime,
    TrueTime,
    Scene,
    TeamDps,
    TeamDamage,
    BossHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayButtonAction {
    CycleMetric,
    CycleTimer,
    CycleSegment,
    ResetEncounter,
    ToggleVisibility,
    OpenHistory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayButton {
    pub id: String,
    pub label: String,
    pub action: OverlayButtonAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayLayer {
    pub id: String,
    pub title: String,
    pub metric: OverlayMetric,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub header_fields: Vec<OverlayHeaderField>,
    #[serde(default = "default_header_widths")]
    pub header_widths: BTreeMap<OverlayHeaderField, u16>,
    #[serde(default)]
    pub hidden_header_labels: Vec<OverlayHeaderField>,
    #[serde(default = "default_summary_fields")]
    pub summary_fields: Vec<OverlaySummaryField>,
    /// User-owned summary row placement. Missing entries use the UI's safe
    /// semantic defaults so layouts saved before row editing remain valid.
    #[serde(default)]
    pub summary_field_rows: BTreeMap<OverlaySummaryField, u8>,
    #[serde(default)]
    pub hidden_summary_labels: Vec<OverlaySummaryField>,
    #[serde(default = "default_show_boss_dps")]
    pub show_boss_dps: bool,
    pub buttons: Vec<OverlayButton>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatOverlaySettings {
    pub schema_version: u16,
    pub canvas_width: u16,
    pub canvas_height: u16,
    pub opacity_percent: u8,
    #[serde(default = "default_bar_opacity_percent")]
    pub bar_opacity_percent: u8,
    #[serde(default = "default_summary_opacity_percent")]
    pub summary_opacity_percent: u8,
    #[serde(default = "default_bar_color_mode")]
    pub bar_color_mode: OverlayBarColorMode,
    #[serde(default)]
    pub bar_color_overrides: BTreeMap<String, String>,
    #[serde(default = "default_number_format")]
    pub number_format: OverlayNumberFormat,
    #[serde(default)]
    pub number_formats: OverlayNumberFormats,
    pub background_mode: OverlayBackgroundMode,
    pub background_color: String,
    pub background_opacity_percent: u8,
    pub custom_background_revision: Option<u64>,
    /// Whether the native Combat Overlay should be armed when rLogs starts.
    /// Auto-hide may still keep the window hidden until combat begins.
    #[serde(default)]
    pub live_overlay_enabled: bool,
    pub always_on_top: bool,
    pub click_through: bool,
    /// Presentation policy owned by the Combat Overlay plug-in. The Combat
    /// Meter supplies reducer state; the overlay alone decides visibility.
    #[serde(default)]
    pub auto_hide_outside_combat: bool,
    #[serde(default = "default_auto_hide_delay_seconds")]
    pub auto_hide_delay_seconds: u16,
    #[serde(default = "default_dynamic_height")]
    pub dynamic_height: bool,
    /// Whether the native overlay exposes resize handles. Resizing the live
    /// window changes presentation scale; canvas dimensions remain owned by
    /// the Overlay designer.
    #[serde(default = "default_allow_live_resize")]
    pub allow_live_resize: bool,
    #[serde(default = "default_max_visible_players")]
    pub max_visible_players: u8,
    #[serde(default = "default_scale_percent")]
    pub scale_percent: u16,
    pub layers: Vec<OverlayLayer>,
}

const fn default_dynamic_height() -> bool {
    true
}

const fn default_allow_live_resize() -> bool {
    true
}

const fn default_show_boss_dps() -> bool {
    true
}

const fn default_max_visible_players() -> u8 {
    20
}

const fn default_scale_percent() -> u16 {
    100
}

const fn default_auto_hide_delay_seconds() -> u16 {
    5
}

const fn default_bar_opacity_percent() -> u8 {
    25
}

const fn default_summary_opacity_percent() -> u8 {
    85
}

const fn default_bar_color_mode() -> OverlayBarColorMode {
    OverlayBarColorMode::Random
}

const fn default_number_format() -> OverlayNumberFormat {
    OverlayNumberFormat::Detailed
}

fn default_header_widths() -> BTreeMap<OverlayHeaderField, u16> {
    [
        (OverlayHeaderField::Rank, 30),
        (OverlayHeaderField::ClassSpec, 32),
        (OverlayHeaderField::Name, 190),
        (OverlayHeaderField::Weapon, 32),
        (OverlayHeaderField::MainImagines, 54),
        (OverlayHeaderField::Damage, 102),
        (OverlayHeaderField::EffectiveDamage, 112),
        (OverlayHeaderField::HpDamage, 102),
        (OverlayHeaderField::ShieldDamage, 102),
        (OverlayHeaderField::Dps, 90),
        (OverlayHeaderField::Edps, 90),
        (OverlayHeaderField::Bdps, 90),
        (OverlayHeaderField::Rdps, 90),
        (OverlayHeaderField::Hps, 90),
        (OverlayHeaderField::Tps, 90),
        (OverlayHeaderField::Healing, 102),
        (OverlayHeaderField::EffectiveHealing, 112),
        (OverlayHeaderField::Overheal, 92),
        (OverlayHeaderField::Shielding, 92),
        (OverlayHeaderField::DamageTaken, 108),
        (OverlayHeaderField::Hits, 62),
        (OverlayHeaderField::CriticalRate, 62),
        (OverlayHeaderField::Casts, 62),
        (OverlayHeaderField::Deaths, 62),
        (OverlayHeaderField::Revives, 62),
        (OverlayHeaderField::RdpsDamage, 108),
        (OverlayHeaderField::ContributionGiven, 112),
        (OverlayHeaderField::ContributionReceived, 112),
        (OverlayHeaderField::Value, 90),
        (OverlayHeaderField::Percent, 48),
    ]
    .into_iter()
    .collect()
}

fn default_summary_fields() -> Vec<OverlaySummaryField> {
    vec![
        OverlaySummaryField::EncounterTime,
        OverlaySummaryField::Scene,
        OverlaySummaryField::TeamDps,
        OverlaySummaryField::TeamDamage,
        OverlaySummaryField::BossHealth,
    ]
}

fn default_summary_field_rows() -> BTreeMap<OverlaySummaryField, u8> {
    [
        (OverlaySummaryField::EncounterTime, 0),
        (OverlaySummaryField::Scene, 0),
        (OverlaySummaryField::TeamDps, 1),
        (OverlaySummaryField::TeamDamage, 1),
        (OverlaySummaryField::BossHealth, 2),
    ]
    .into_iter()
    .collect()
}

impl Default for CombatOverlaySettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            canvas_width: 720,
            canvas_height: 520,
            opacity_percent: 92,
            bar_opacity_percent: default_bar_opacity_percent(),
            summary_opacity_percent: default_summary_opacity_percent(),
            bar_color_mode: default_bar_color_mode(),
            bar_color_overrides: BTreeMap::new(),
            number_format: default_number_format(),
            number_formats: OverlayNumberFormats::default(),
            background_mode: OverlayBackgroundMode::Solid,
            background_color: "#0b1522".into(),
            background_opacity_percent: 92,
            custom_background_revision: None,
            live_overlay_enabled: false,
            always_on_top: true,
            click_through: false,
            auto_hide_outside_combat: false,
            auto_hide_delay_seconds: default_auto_hide_delay_seconds(),
            dynamic_height: default_dynamic_height(),
            allow_live_resize: default_allow_live_resize(),
            max_visible_players: default_max_visible_players(),
            scale_percent: default_scale_percent(),
            layers: vec![OverlayLayer {
                id: "party-meter".into(),
                title: "Party damage".into(),
                metric: OverlayMetric::Dps,
                x: 18,
                y: 18,
                width: 680,
                header_fields: vec![
                    OverlayHeaderField::Rank,
                    OverlayHeaderField::ClassSpec,
                    OverlayHeaderField::Name,
                    OverlayHeaderField::Weapon,
                    OverlayHeaderField::MainImagines,
                    OverlayHeaderField::Dps,
                    OverlayHeaderField::Percent,
                ],
                header_widths: default_header_widths(),
                hidden_header_labels: Vec::new(),
                summary_fields: default_summary_fields(),
                summary_field_rows: default_summary_field_rows(),
                hidden_summary_labels: Vec::new(),
                show_boss_dps: default_show_boss_dps(),
                buttons: vec![
                    OverlayButton {
                        id: "segment".into(),
                        label: "Entire run".into(),
                        action: OverlayButtonAction::CycleSegment,
                    },
                    OverlayButton {
                        id: "timer".into(),
                        label: "Encounter".into(),
                        action: OverlayButtonAction::CycleTimer,
                    },
                    OverlayButton {
                        id: "metric".into(),
                        label: "DPS".into(),
                        action: OverlayButtonAction::CycleMetric,
                    },
                    OverlayButton {
                        id: "visibility".into(),
                        label: "Hide".into(),
                        action: OverlayButtonAction::ToggleVisibility,
                    },
                ],
            }],
        }
    }
}

#[derive(Debug)]
pub struct CombatOverlaySettingsStore {
    path: PathBuf,
    settings: CombatOverlaySettings,
}

impl CombatOverlaySettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> CombatOverlaySettings {
        self.settings.clone()
    }

    pub fn update(
        &mut self,
        settings: CombatOverlaySettings,
    ) -> Result<CombatOverlaySettings, String> {
        validate(&settings)?;
        write(&self.path, &settings)?;
        self.settings = settings;
        Ok(self.snapshot())
    }
}

fn load(path: &Path) -> Result<CombatOverlaySettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CombatOverlaySettings::default());
        }
        Err(error) => {
            return Err(format!(
                "could not inspect Combat Overlay settings: {error}"
            ));
        }
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Combat Overlay settings exceed the 128 KiB safety limit".into());
    }
    let mut settings: CombatOverlaySettings = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("could not read Combat Overlay settings: {error}"))?,
    )
    .map_err(|error| format!("Combat Overlay settings are invalid: {error}"))?;
    ensure_live_switch_controls(&mut settings);
    validate(&settings)?;
    Ok(settings)
}

fn ensure_live_switch_controls(settings: &mut CombatOverlaySettings) {
    for layer in &mut settings.layers {
        if layer.buttons.len() < 8
            && !layer
                .buttons
                .iter()
                .any(|button| button.action == OverlayButtonAction::CycleSegment)
        {
            layer.buttons.insert(
                0,
                OverlayButton {
                    id: unique_button_id(&layer.buttons, "segment"),
                    label: "Entire run".into(),
                    action: OverlayButtonAction::CycleSegment,
                },
            );
        }
        if layer.buttons.len() < 8
            && !layer
                .buttons
                .iter()
                .any(|button| button.action == OverlayButtonAction::CycleTimer)
        {
            let index = usize::from(!layer.buttons.is_empty());
            layer.buttons.insert(
                index,
                OverlayButton {
                    id: unique_button_id(&layer.buttons, "timer"),
                    label: "Encounter".into(),
                    action: OverlayButtonAction::CycleTimer,
                },
            );
        }
    }
}

fn unique_button_id(buttons: &[OverlayButton], base: &str) -> String {
    if buttons.iter().all(|button| button.id != base) {
        return base.into();
    }
    (2_u8..=8)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| buttons.iter().all(|button| button.id != *candidate))
        .unwrap_or_else(|| format!("{base}-control"))
}

fn validate(settings: &CombatOverlaySettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported Combat Overlay settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    if !(320..=2560).contains(&settings.canvas_width)
        || !(MIN_CANVAS_HEIGHT..=1440).contains(&settings.canvas_height)
    {
        return Err("Combat Overlay canvas dimensions are outside the supported range".into());
    }
    if !(20..=100).contains(&settings.opacity_percent) {
        return Err("Combat Overlay opacity must be between 20 and 100 percent".into());
    }
    if settings.bar_opacity_percent > 100 {
        return Err("Combat Overlay colored-bar opacity must be between 0 and 100 percent".into());
    }
    if settings.summary_opacity_percent > 100 {
        return Err("Combat Overlay summary opacity must be between 0 and 100 percent".into());
    }
    if settings.bar_color_overrides.len() > 64
        || settings
            .bar_color_overrides
            .iter()
            .any(|(key, color)| !is_bar_color_identity(key) || !is_hex_color(color))
    {
        return Err("Combat Overlay bar-color overrides are invalid".into());
    }
    if settings.background_opacity_percent > 100 {
        return Err("Combat Overlay background opacity must be between 0 and 100 percent".into());
    }
    if !(1..=20).contains(&settings.max_visible_players) {
        return Err("Combat Overlay visible-player limit must be between 1 and 20".into());
    }
    if settings.auto_hide_delay_seconds > 300 {
        return Err("Combat Overlay auto-hide delay must be between 0 and 300 seconds".into());
    }
    if !(50..=200).contains(&settings.scale_percent) {
        return Err("Combat Overlay scale must be between 50 and 200 percent".into());
    }
    if !is_hex_color(&settings.background_color) {
        return Err("Combat Overlay background color must use #RRGGBB format".into());
    }
    if settings.background_mode == OverlayBackgroundMode::Custom
        && settings.custom_background_revision.is_none()
    {
        return Err("Combat Overlay custom background has not been uploaded".into());
    }
    if settings.layers.is_empty() {
        return Err("Combat Overlay requires at least one header view".into());
    }
    if settings.layers.len() > MAX_LAYERS {
        return Err(format!(
            "Combat Overlay supports at most {MAX_LAYERS} header views"
        ));
    }
    let mut layer_ids = std::collections::BTreeSet::new();
    for layer in &settings.layers {
        validate_identifier("header view", &layer.id)?;
        if !layer_ids.insert(&layer.id) {
            return Err(format!(
                "duplicate Combat Overlay header-view id {:?}",
                layer.id
            ));
        }
        if layer.title.trim().is_empty() || layer.title.len() > 80 {
            return Err(format!(
                "Combat Overlay header view {:?} has an invalid title",
                layer.id
            ));
        }
        if !(240..=1200).contains(&layer.width) {
            return Err(format!(
                "Combat Overlay header view {:?} has an invalid width",
                layer.id
            ));
        }
        if layer.header_fields.is_empty() || layer.header_fields.len() > MAX_HEADER_FIELDS {
            return Err(format!(
                "Combat Overlay header view {:?} has invalid headers",
                layer.id
            ));
        }
        let unique_headers = layer
            .header_fields
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique_headers.len() != layer.header_fields.len() {
            return Err(format!(
                "Combat Overlay header view {:?} repeats a header",
                layer.id
            ));
        }
        if layer.header_widths.len() > MAX_HEADER_FIELDS
            || layer.header_widths.values().any(|width| *width > 480)
        {
            return Err(format!(
                "Combat Overlay header view {:?} has invalid header widths",
                layer.id
            ));
        }
        let unique_hidden_labels = layer
            .hidden_header_labels
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique_hidden_labels.len() != layer.hidden_header_labels.len()
            || layer
                .hidden_header_labels
                .iter()
                .any(|field| !unique_headers.contains(field))
        {
            return Err(format!(
                "Combat Overlay header view {:?} has invalid hidden header labels",
                layer.id
            ));
        }
        if layer.summary_fields.len() > MAX_SUMMARY_FIELDS {
            return Err(format!(
                "Combat Overlay header view {:?} has too many summary items",
                layer.id
            ));
        }
        let unique_summary_fields = layer
            .summary_fields
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique_summary_fields.len() != layer.summary_fields.len() {
            return Err(format!(
                "Combat Overlay header view {:?} repeats a summary item",
                layer.id
            ));
        }
        if layer.summary_field_rows.len() > MAX_SUMMARY_FIELDS
            || layer
                .summary_field_rows
                .iter()
                .any(|(field, row)| !unique_summary_fields.contains(field) || *row >= 8)
        {
            return Err(format!(
                "Combat Overlay header view {:?} has invalid summary row placement",
                layer.id
            ));
        }
        let unique_hidden_summary_labels = layer
            .hidden_summary_labels
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique_hidden_summary_labels.len() != layer.hidden_summary_labels.len()
            || layer
                .hidden_summary_labels
                .iter()
                .any(|field| !unique_summary_fields.contains(field))
        {
            return Err(format!(
                "Combat Overlay header view {:?} has invalid hidden summary labels",
                layer.id
            ));
        }
        if layer.buttons.len() > 8 {
            return Err(format!(
                "Combat Overlay header view {:?} has too many buttons",
                layer.id
            ));
        }
        let mut button_ids = std::collections::BTreeSet::new();
        for button in &layer.buttons {
            validate_identifier("button", &button.id)?;
            if !button_ids.insert(&button.id) {
                return Err(format!(
                    "Combat Overlay header view {:?} repeats a button id",
                    layer.id
                ));
            }
            if button.label.trim().is_empty() || button.label.len() > 24 {
                return Err(format!(
                    "Combat Overlay button {:?} has an invalid label",
                    button.id
                ));
            }
        }
    }
    Ok(())
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_bar_color_identity(value: &str) -> bool {
    ["class:", "specialization:"].into_iter().any(|prefix| {
        value
            .strip_prefix(prefix)
            .and_then(|id| id.parse::<i32>().ok())
            .is_some_and(|id| id > 0)
    })
}

fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("Combat Overlay {label} id {value:?} is invalid"));
    }
    Ok(())
}

fn write(path: &Path, settings: &CombatOverlaySettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Combat Overlay settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Combat Overlay settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode Combat Overlay settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write Combat Overlay settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_is_valid_and_editable() {
        let mut settings = CombatOverlaySettings::default();
        validate(&settings).unwrap();
        assert_eq!(settings.layers.len(), 1);
        assert_eq!(settings.layers[0].header_fields.len(), 7);
        assert!(settings.layers[0].hidden_header_labels.is_empty());
        assert!(settings.layers[0].hidden_summary_labels.is_empty());
        assert!(settings.layers[0].show_boss_dps);
        assert_eq!(
            settings.layers[0]
                .summary_field_rows
                .get(&OverlaySummaryField::TeamDamage),
            Some(&1)
        );
        assert_eq!(settings.layers[0].buttons.len(), 4);
        assert!(settings.dynamic_height);
        assert_eq!(settings.max_visible_players, 20);
        assert_eq!(settings.scale_percent, 100);
        assert!(!settings.live_overlay_enabled);
        assert!(!settings.auto_hide_outside_combat);
        assert_eq!(settings.auto_hide_delay_seconds, 5);
        assert_eq!(settings.bar_opacity_percent, 25);
        assert_eq!(settings.bar_color_mode, OverlayBarColorMode::Random);
        assert!(settings.bar_color_overrides.is_empty());
        assert_eq!(settings.number_format, OverlayNumberFormat::Detailed);
        assert_eq!(settings.number_formats, OverlayNumberFormats::default());
        assert_eq!(
            settings.layers[0]
                .header_widths
                .get(&OverlayHeaderField::Name),
            Some(&190)
        );
        settings.layers[0]
            .header_widths
            .insert(OverlayHeaderField::Name, 0);
        validate(&settings).expect("a column may be collapsed without a minimum width");
    }

    #[test]
    fn duplicate_layer_ids_are_rejected() {
        let mut settings = CombatOverlaySettings::default();
        settings.layers.push(settings.layers[0].clone());
        assert!(validate(&settings).is_err());
    }

    #[test]
    fn compact_overlay_height_is_supported() {
        let mut settings = CombatOverlaySettings {
            canvas_height: MIN_CANVAS_HEIGHT,
            ..CombatOverlaySettings::default()
        };
        validate(&settings).expect("the overlay may be collapsed to the native window minimum");

        settings.canvas_height = MIN_CANVAS_HEIGHT - 1;
        assert!(validate(&settings).is_err());
    }

    #[test]
    fn expanded_live_columns_round_trip_through_settings() {
        let mut settings = CombatOverlaySettings::default();
        settings.layers[0].header_fields = vec![
            OverlayHeaderField::Name,
            OverlayHeaderField::Damage,
            OverlayHeaderField::EffectiveHealing,
            OverlayHeaderField::DamageTaken,
            OverlayHeaderField::CriticalRate,
            OverlayHeaderField::RdpsDamage,
            OverlayHeaderField::ContributionGiven,
            OverlayHeaderField::ContributionReceived,
        ];
        validate(&settings).unwrap();

        let encoded = serde_json::to_string(&settings).unwrap();
        assert!(encoded.contains("effective_healing"));
        assert!(encoded.contains("contribution_received"));
        let decoded: CombatOverlaySettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn class_and_specialization_bar_colors_are_validated() {
        let mut settings = CombatOverlaySettings {
            bar_color_mode: OverlayBarColorMode::Specialization,
            ..CombatOverlaySettings::default()
        };
        settings
            .bar_color_overrides
            .insert("class:11".into(), "#d95b68".into());
        settings
            .bar_color_overrides
            .insert("specialization:117".into(), "#f0a83b".into());
        validate(&settings).unwrap();

        settings
            .bar_color_overrides
            .insert("name:MarieRose".into(), "#ffffff".into());
        assert!(validate(&settings).is_err());
    }

    #[test]
    fn at_least_one_header_view_is_required() {
        let mut settings = CombatOverlaySettings::default();
        settings.layers.clear();
        assert!(
            validate(&settings)
                .unwrap_err()
                .contains("at least one header view")
        );
    }

    #[test]
    fn older_layouts_receive_safe_sizing_defaults() {
        let mut value = serde_json::to_value(CombatOverlaySettings::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("scalePercent");
        object.remove("liveOverlayEnabled");
        object.remove("autoHideOutsideCombat");
        object.remove("autoHideDelaySeconds");
        object.remove("barOpacityPercent");
        object.remove("barColorMode");
        object.remove("barColorOverrides");
        object.remove("numberFormat");
        object.remove("numberFormats");
        object["layers"].as_array_mut().unwrap()[0]
            .as_object_mut()
            .unwrap()
            .remove("headerWidths");
        object["layers"].as_array_mut().unwrap()[0]
            .as_object_mut()
            .unwrap()
            .remove("summaryFieldRows");
        object["layers"].as_array_mut().unwrap()[0]
            .as_object_mut()
            .unwrap()
            .remove("showBossDps");

        let settings: CombatOverlaySettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.scale_percent, 100);
        assert!(!settings.live_overlay_enabled);
        assert!(!settings.auto_hide_outside_combat);
        assert_eq!(settings.auto_hide_delay_seconds, 5);
        assert_eq!(settings.bar_opacity_percent, 25);
        assert_eq!(settings.bar_color_mode, OverlayBarColorMode::Random);
        assert!(settings.bar_color_overrides.is_empty());
        assert_eq!(settings.number_format, OverlayNumberFormat::Detailed);
        assert_eq!(settings.number_formats, OverlayNumberFormats::default());
        assert_eq!(settings.layers[0].header_widths, default_header_widths());
        assert!(settings.layers[0].summary_field_rows.is_empty());
        assert!(settings.layers[0].show_boss_dps);
        validate(&settings).unwrap();
    }

    #[test]
    fn custom_background_requires_an_uploaded_revision() {
        let mut settings = CombatOverlaySettings {
            background_mode: OverlayBackgroundMode::Custom,
            ..CombatOverlaySettings::default()
        };
        assert!(validate(&settings).is_err());

        settings.custom_background_revision = Some(1);
        validate(&settings).unwrap();
    }
}
