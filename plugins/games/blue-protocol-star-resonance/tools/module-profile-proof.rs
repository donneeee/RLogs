use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{BufRead, BufReader, BufWriter},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, EventEnvelope};
use rlogs_game_bpsr::{ActiveModuleEffectSnapshot, CharacterProfilePatch, ModuleEffectCatalog};
use serde::Serialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("module profile proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let catalog = ModuleEffectCatalog::load_from_path(&arguments.catalog)?;
    let reader = BufReader::new(File::open(&arguments.events)?);
    let mut snapshots = Vec::new();
    let mut lines_read = 0_u64;
    let mut profile_events = 0_u64;
    let mut module_profiles = 0_u64;

    for line in reader.lines() {
        lines_read = lines_read.saturating_add(1);
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: EventEnvelope = serde_json::from_str(&line)?;
        let CanonicalEvent::CharacterProfileObserved { profile } = &envelope.event else {
            continue;
        };
        profile_events = profile_events.saturating_add(1);
        let patch = CharacterProfilePatch::from_game_event(profile)?;
        if arguments
            .character
            .as_deref()
            .is_some_and(|expected| patch.character.character_id != expected)
        {
            continue;
        }
        let Some(modules) = patch.modules.as_ref() else {
            continue;
        };
        module_profiles = module_profiles.saturating_add(1);
        let resolved = catalog.resolve(modules);
        if arguments
            .effect
            .is_some_and(|effect_id| resolved.effect(effect_id).is_none())
        {
            continue;
        }
        snapshots.push(ModuleProfileProofSnapshot {
            session_id: envelope.session_id,
            sequence: envelope.sequence,
            observed_micros: envelope.time.observed_micros,
            character_id: patch.character.character_id,
            display_name: patch.display_name,
            equipped_slots: modules.equipped_slots.clone(),
            module_inventory_count: modules.inventory.len(),
            resolved,
        });
    }

    let bundle = ModuleProfileProofBundle {
        schema_version: 1,
        generated_by: "rlogs-bpsr-module-profile-proof",
        policy: ProofPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            profile_scope: "only immutable packet-captured profile snapshots carrying an authoritative module payload",
            link_point_scope: "initial_link_points only; upgrade records are not added because current-build snapshots already carry the upgraded value",
            unresolved_evidence_is_hidden: false,
        },
        input_events: arguments.events,
        catalog_root: arguments.catalog,
        catalog_revision: catalog.catalog_revision,
        character_filter: arguments.character,
        effect_filter: arguments.effect,
        lines_read,
        profile_events,
        module_profiles,
        snapshots,
    };
    let writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(writer, &bundle)?;
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    events: PathBuf,
    catalog: PathBuf,
    output: PathBuf,
    character: Option<String>,
    effect: Option<i32>,
}

#[derive(Debug, Serialize)]
struct ModuleProfileProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    input_events: PathBuf,
    catalog_root: PathBuf,
    catalog_revision: String,
    character_filter: Option<String>,
    effect_filter: Option<i32>,
    lines_read: u64,
    profile_events: u64,
    module_profiles: u64,
    snapshots: Vec<ModuleProfileProofSnapshot>,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    runtime_use: &'static str,
    profile_scope: &'static str,
    link_point_scope: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct ModuleProfileProofSnapshot {
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    character_id: String,
    display_name: Option<String>,
    equipped_slots: std::collections::BTreeMap<i32, String>,
    module_inventory_count: usize,
    resolved: ActiveModuleEffectSnapshot,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::env::args_os().skip(1).collect::<Vec<_>>();
    let events = required_path(&mut values, "--events")?;
    let catalog = required_path(&mut values, "--catalog")?;
    let output = required_path(&mut values, "--output")?;
    let character = optional_value(&mut values, "--character")
        .map(|value| value.to_string_lossy().into_owned());
    let effect = optional_value(&mut values, "--effect")
        .map(|value| parse_i32(value, "--effect"))
        .transpose()?;
    if !values.is_empty() {
        return Err(format!("unexpected arguments: {values:?}"));
    }
    Ok(Arguments {
        events,
        catalog,
        output,
        character,
        effect,
    })
}

fn required_path(values: &mut Vec<OsString>, flag: &str) -> Result<PathBuf, String> {
    optional_value(values, flag)
        .map(PathBuf::from)
        .ok_or_else(|| usage(flag))
}

fn optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let index = values.iter().position(|value| value == OsStr::new(flag))?;
    values.remove(index);
    (index < values.len()).then(|| values.remove(index))
}

fn parse_i32(value: OsString, flag: &str) -> Result<i32, String> {
    value
        .to_string_lossy()
        .parse::<i32>()
        .map_err(|_| format!("{flag} must be an integer"))
}

fn usage(missing: &str) -> String {
    format!(
        "missing {missing}; usage: rlogs-bpsr-module-profile-proof --events <canonical.jsonl> --catalog <game-data/catalog> --output <proof.json> [--character <public-character-id>] [--effect <module-effect-id>]"
    )
}
