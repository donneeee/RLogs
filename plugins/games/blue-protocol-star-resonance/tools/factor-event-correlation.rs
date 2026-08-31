use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_game_bpsr::{
    FACTOR_CORRELATION_SCHEMA_VERSION, PsychoscopeFactorCorrelationAnalyzer,
    PsychoscopeFactorCorrelationReport,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct CorrelationBundle {
    schema_version: u16,
    policy: &'static str,
    reports: Vec<PsychoscopeFactorCorrelationReport>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("factor event correlation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    for path in arguments.rlogs {
        let file = File::open(&path)?;
        let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        while let Some(envelope) = reader.next_event()? {
            analyzer.observe(&envelope)?;
        }
        if reader.summary().is_none() {
            return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
        }
        reports.push(analyzer.finish()?);
    }
    let bundle = CorrelationBundle {
        schema_version: FACTOR_CORRELATION_SCHEMA_VERSION,
        policy: "evidence_only_no_rdps_credit",
        reports,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
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
    if values
        .iter()
        .any(|value| value == "--help" || value == "-h")
    {
        return Err(usage());
    }
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
    "usage: rlogs-bpsr-factor-event-correlation --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <report.json>".into()
}
