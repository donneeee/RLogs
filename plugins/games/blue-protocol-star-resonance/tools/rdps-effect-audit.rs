use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_game_bpsr::{RDPS_AUDIT_SCHEMA_VERSION, RdpsAuditReport, RdpsEffectAuditAnalyzer};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    client_build: String,
    deployment_id: String,
    sources: Vec<AuditSource>,
    reports: Vec<RdpsAuditReport>,
}

#[derive(Debug, Serialize)]
struct AuditSource {
    file_name: String,
    session_id: String,
    client_build: String,
    deployment_id: String,
    producer: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS effect audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    let mut sources = Vec::with_capacity(arguments.rlogs.len());
    let mut expected_build = None::<String>;
    let mut expected_deployment = None::<String>;
    for path in arguments.rlogs {
        let file = File::open(&path)?;
        let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
        let header = reader.header();
        let client_build = header.region.client_build.clone();
        let deployment_id = header.region.identity.deployment_id.clone();
        if client_build.trim().is_empty() {
            return Err(format!("{} has an empty client build", path.display()).into());
        }
        if deployment_id.trim().is_empty() {
            return Err(format!("{} has an empty deployment id", path.display()).into());
        }
        if expected_build
            .as_ref()
            .is_some_and(|expected| expected != &client_build)
        {
            return Err(format!(
                "{} belongs to client build {client_build}, expected {}",
                path.display(),
                expected_build.as_deref().unwrap_or_default()
            )
            .into());
        }
        if expected_deployment
            .as_ref()
            .is_some_and(|expected| expected != &deployment_id)
        {
            return Err(format!(
                "{} belongs to deployment {deployment_id}, expected {}",
                path.display(),
                expected_deployment.as_deref().unwrap_or_default()
            )
            .into());
        }
        expected_build.get_or_insert_with(|| client_build.clone());
        expected_deployment.get_or_insert_with(|| deployment_id.clone());
        sources.push(AuditSource {
            file_name: path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| format!("{} has a non-UTF-8 file name", path.display()))?
                .to_owned(),
            session_id: header.session_id.clone(),
            client_build,
            deployment_id,
            producer: header.producer.clone(),
        });
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        while let Some(envelope) = reader.next_event()? {
            analyzer.observe(&envelope)?;
        }
        if reader.summary().is_none() {
            return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
        }
        reports.push(analyzer.finish()?);
    }
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(
        &mut writer,
        &AuditBundle {
            schema_version: RDPS_AUDIT_SCHEMA_VERSION,
            client_build: expected_build.ok_or("audit has no client build")?,
            deployment_id: expected_deployment.ok_or("audit has no deployment id")?,
            sources,
            reports,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = take_value(&mut values, "--output")?;
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".into());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs,
        output: output.into(),
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Err(format!("missing {flag}\n{}", usage()));
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    values.remove(position);
    Ok(values.remove(position))
}

fn usage() -> String {
    "usage: rlogs-bpsr-rdps-effect-audit --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <report.json>".into()
}
