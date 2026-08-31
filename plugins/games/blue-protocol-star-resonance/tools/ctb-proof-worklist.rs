use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    diff: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BuildDiff {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    baseline_build_id: String,
    build_id: String,
    changes: Vec<TableChange>,
}

#[derive(Debug, Deserialize)]
struct TableChange {
    table_key: u32,
    stable_key: String,
    names: Vec<String>,
    domain: String,
    change: String,
    current: Option<TableVersion>,
    shape_changes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TableVersion {
    relative_path: String,
    offset: u64,
    bytes: u64,
    sha256: String,
    shape: Shape,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Shape {
    rows: u32,
    row_size: u32,
    row_data_bytes: u32,
    pool_lengths: Vec<PoolLength>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PoolLength {
    r#type: u32,
    bytes: u32,
}

#[derive(Debug, Serialize)]
struct ProofWorklist {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    baseline_build_id: String,
    build_id: String,
    policy: Policy,
    summary: Summary,
    entries: Vec<WorkEntry>,
}

#[derive(Debug, Serialize)]
struct Policy {
    all_changed_tables_retained: bool,
    unresolved_tables_hidden: bool,
    changed_rules_auto_promoted: bool,
    exact_build_packet_replay_required: bool,
    exact_conservation_required: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    changed_or_added_tables: usize,
    by_route: BTreeMap<String, usize>,
    direct_r_dps_tables: usize,
    immediate_proof_or_identity_tables: usize,
    identity_resolution_tables: usize,
}

#[derive(Debug, Serialize)]
struct WorkEntry {
    order: usize,
    priority: u8,
    route: &'static str,
    table_key: u32,
    table_key_hex: String,
    stable_key: String,
    names: Vec<String>,
    domain: String,
    change: String,
    shape_changes: Vec<String>,
    reason: &'static str,
    proof_suites: Vec<&'static str>,
    current: Option<TableVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    priority: u8,
    name: &'static str,
    reason: &'static str,
    proof_suites: Vec<&'static str>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("CTB proof worklist failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!(
            "refusing to overwrite existing output {}",
            arguments.output.display()
        )
        .into());
    }

    let diff: BuildDiff = serde_json::from_slice(&fs::read(&arguments.diff)?)?;
    if diff.baseline_build_id == diff.build_id {
        return Err("baseline and current build IDs are identical".into());
    }

    let mut routed = diff
        .changes
        .into_iter()
        .map(|change| {
            let route = route(&change.stable_key, &change.domain);
            (route, change)
        })
        .collect::<Vec<_>>();
    routed.sort_by(|(left_route, left), (right_route, right)| {
        left_route
            .priority
            .cmp(&right_route.priority)
            .then_with(|| left_route.name.cmp(right_route.name))
            .then_with(|| left.stable_key.cmp(&right.stable_key))
            .then_with(|| left.table_key.cmp(&right.table_key))
    });

    let mut by_route = BTreeMap::new();
    let mut direct_r_dps_tables = 0_usize;
    let mut immediate_proof_or_identity_tables = 0_usize;
    let mut identity_resolution_tables = 0_usize;
    let entries = routed
        .into_iter()
        .enumerate()
        .map(|(index, (route, change))| {
            *by_route.entry(route.name.to_owned()).or_insert(0) += 1;
            if route.priority <= 2 {
                immediate_proof_or_identity_tables += 1;
            }
            if matches!(
                route.name,
                "formula-inputs" | "ability-effect-origin" | "equipment-state"
            ) {
                direct_r_dps_tables += 1;
            }
            if route.name == "identity-resolution" {
                identity_resolution_tables += 1;
            }
            WorkEntry {
                order: index + 1,
                priority: route.priority,
                route: route.name,
                table_key: change.table_key,
                table_key_hex: format!("0x{:08x}", change.table_key),
                stable_key: change.stable_key,
                names: change.names,
                domain: change.domain,
                change: change.change,
                shape_changes: change.shape_changes,
                reason: route.reason,
                proof_suites: route.proof_suites,
                current: change.current,
            }
        })
        .collect::<Vec<_>>();

    let worklist = ProofWorklist {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-ctb-proof-worklist",
        game: "blue-protocol-star-resonance",
        deployment_id: diff.deployment_id,
        channel: diff.channel,
        distribution_app_id: diff.distribution_app_id,
        baseline_build_id: diff.baseline_build_id,
        build_id: diff.build_id,
        policy: Policy {
            all_changed_tables_retained: true,
            unresolved_tables_hidden: false,
            changed_rules_auto_promoted: false,
            exact_build_packet_replay_required: true,
            exact_conservation_required: true,
        },
        summary: Summary {
            changed_or_added_tables: entries.len(),
            by_route,
            direct_r_dps_tables,
            immediate_proof_or_identity_tables,
            identity_resolution_tables,
        },
        entries,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &worklist)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn route(stable_key: &str, domain: &str) -> Route {
    if stable_key.starts_with("ctb.unknown-") {
        return Route {
            priority: 1,
            name: "identity-resolution",
            reason: "A changed or added table has no proven identity; it must remain visible and be named before promotion.",
            proof_suites: vec!["table-identity", "schema-diff", "packet-correlation"],
        };
    }

    let formula_inputs = [
        "ctb.DamageAttrTable",
        "ctb.EntityAttributeTable",
        "ctb.EntityDynAttrCoeTable",
        "ctb.FightAttrTable",
        "ctb.FightAttrTranTable",
    ];
    if formula_inputs.contains(&stable_key) {
        return Route {
            priority: 1,
            name: "formula-inputs",
            reason: "This table participates directly in damage, attribute, probability, or fixed-point transformation stages.",
            proof_suites: vec![
                "row-schema-diff",
                "formula-stage-replay",
                "provider-recipient-replay",
                "runtime-conservation",
            ],
        };
    }

    let ability_origins = [
        "ctb.BuffTable",
        "ctb.SkillEffectTable",
        "ctb.SkillFightLevelTable",
        "ctb.SkillTable",
        "ctb.ModelSpecialTable",
    ];
    if ability_origins.contains(&stable_key) {
        return Route {
            priority: 1,
            name: "ability-effect-origin",
            reason: "This table maps packet-observed skills, buffs, effects, levels, or proc origins used by recount and rDPS attribution.",
            proof_suites: vec![
                "row-schema-diff",
                "origin-graph-diff",
                "status-lifecycle-replay",
                "runtime-conservation",
            ],
        };
    }

    let equipment_state = ["ctb.EquipTable", "ctb.ItemTable", "ctb.ItemTempTable"];
    if equipment_state.contains(&stable_key) {
        return Route {
            priority: 2,
            name: "equipment-state",
            reason: "Equipment and set-linked state can provide combat effects and must be correlated to packet-observed status and damage events.",
            proof_suites: vec![
                "row-schema-diff",
                "equipment-effect-correlation",
                "status-lifecycle-replay",
                "runtime-conservation",
            ],
        };
    }

    if domain == "entities" || matches!(stable_key, "ctb.ModelTable" | "ctb.ModelNpcTable") {
        return Route {
            priority: 2,
            name: "entity-target-identity",
            reason: "Entity, target, projectile, and model identity changes affect ownership, recipient scope, and target filtering.",
            proof_suites: vec!["row-schema-diff", "entity-identity", "packet-correlation"],
        };
    }

    if domain == "localization" {
        return Route {
            priority: 3,
            name: "localization-evidence",
            reason: "Localized text is presentation evidence only; UID identity and packet behavior remain authoritative.",
            proof_suites: vec!["localization-diff", "uid-fallback-validation"],
        };
    }

    if domain == "world-and-instances" {
        return Route {
            priority: 3,
            name: "encounter-context",
            reason: "Scene and dungeon changes affect segmentation, encounter identity, and replay windows used by rDPS reports.",
            proof_suites: vec![
                "row-schema-diff",
                "segmentation-replay",
                "packet-correlation",
            ],
        };
    }

    Route {
        priority: 4,
        name: "retained-secondary-review",
        reason: "The table changed in this build but has no proven direct rDPS dependency; retain it for review without promoting runtime rules.",
        proof_suites: vec!["row-schema-diff", "dependency-review"],
    }
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut diff = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--diff" => diff = Some(PathBuf::from(next_value(&mut args, "--diff")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        diff: diff.ok_or("missing --diff")?,
        output: output.ok_or("missing --output")?,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_tables_are_first_priority() {
        let route = route("ctb.DamageAttrTable", "combat");
        assert_eq!(route.priority, 1);
        assert_eq!(route.name, "formula-inputs");
        assert!(route.proof_suites.contains(&"formula-stage-replay"));
    }

    #[test]
    fn unknown_tables_are_never_hidden_or_deferred() {
        let route = route("ctb.unknown-12345678", "unknown");
        assert_eq!(route.priority, 1);
        assert_eq!(route.name, "identity-resolution");
    }

    #[test]
    fn equipment_changes_route_through_effect_correlation() {
        let route = route("ctb.EquipTable", "items-and-equipment");
        assert_eq!(route.priority, 2);
        assert!(route.proof_suites.contains(&"equipment-effect-correlation"));
    }

    #[test]
    fn temporary_mode_items_route_through_effect_correlation() {
        let route = route("ctb.ItemTempTable", "items-and-equipment");
        assert_eq!(route.priority, 2);
        assert_eq!(route.name, "equipment-state");
        assert!(route.proof_suites.contains(&"status-lifecycle-replay"));
    }

    #[test]
    fn localization_never_becomes_identity_authority() {
        let route = route("ctb.MessageTable", "localization");
        assert_eq!(route.name, "localization-evidence");
        assert!(route.reason.contains("UID identity"));
    }
}
