use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{RegionEvidence, RegionEvidenceKind, RegionIdentity};
use rlogs_game_bpsr::{
    JournalTailPolicy, JsonlJournalReader, OfflineRecordingConfig, OfflineRecordingLimits,
    ProtocolPack, ProtocolRuntimeConfig, record_offline_journal_transition_with_tail_policy,
    record_offline_journal_with_tail_policy,
};

#[derive(Debug)]
struct Arguments {
    journal: PathBuf,
    pack: PathBuf,
    source_pack: Option<PathBuf>,
    output_rlog: PathBuf,
    output_report: PathBuf,
    recover_truncated_tail: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol journal recorder failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    ensure_available(&arguments.output_rlog)?;
    ensure_available(&arguments.output_report)?;

    let pack = ProtocolPack::from_json(&fs::read(&arguments.pack)?)?;
    let source_pack = arguments
        .source_pack
        .as_ref()
        .map(fs::read)
        .transpose()?
        .map(|bytes| ProtocolPack::from_json(&bytes))
        .transpose()?;
    let session = JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?))
        .into_record_stream()?
        .session()
        .clone();
    if !pack.matches(&session.game_build) {
        return Err(format!(
            "protocol pack {} does not match journal build {}",
            pack.definition().pack_id,
            session.game_build.build_id
        )
        .into());
    }
    if let Some(source_pack) = &source_pack
        && !source_pack.matches(&session.game_build)
    {
        return Err(format!(
            "source protocol pack {} does not match journal build {}",
            source_pack.definition().pack_id,
            session.game_build.build_id
        )
        .into());
    }

    let region_id = session
        .game_build
        .region_id
        .clone()
        .unwrap_or_else(|| session.game_build.deployment_id.clone());
    let region = RegionIdentity {
        deployment_id: session.game_build.deployment_id.clone(),
        region_id: region_id.clone(),
        realm_id: None,
        world_id: None,
    };
    let output_partial = partial_path(&arguments.output_rlog)?;
    let report_partial = partial_path(&arguments.output_report)?;
    ensure_available(&output_partial)?;
    ensure_available(&report_partial)?;

    let output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_partial)?;
    let config = OfflineRecordingConfig {
        session_id: session.capture_id.clone(),
        producer: format!(
            "rlogs-bpsr-protocol-journal-recorder/{}",
            env!("CARGO_PKG_VERSION")
        ),
        build: session.game_build,
        region,
        region_evidence: vec![RegionEvidence {
            kind: RegionEvidenceKind::ReplayManifest,
            reference: format!("protocol-journal-region:{region_id}"),
        }],
        limits: OfflineRecordingLimits::default(),
        decoder: ProtocolRuntimeConfig::default(),
        objective_catalog: None,
    };
    let tail_policy = if arguments.recover_truncated_tail {
        JournalTailPolicy::RecoverTruncatedFinalLine
    } else {
        JournalTailPolicy::Strict
    };
    let journal = BufReader::new(File::open(&arguments.journal)?);
    let output = BufWriter::new(output_file);
    let result = if let Some(source_pack) = &source_pack {
        record_offline_journal_transition_with_tail_policy(
            journal,
            source_pack,
            &pack,
            config,
            output,
            tail_policy,
        )?
    } else {
        record_offline_journal_with_tail_policy(journal, &pack, config, output, tail_policy)?
    };
    let mut output = result.output;
    output.flush()?;
    output.get_ref().sync_all()?;
    drop(output);

    let report_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report_partial)?;
    let mut report = BufWriter::new(report_file);
    serde_json::to_writer_pretty(&mut report, &result.report)?;
    report.write_all(b"\n")?;
    report.flush()?;
    report.get_ref().sync_all()?;
    drop(report);

    fs::rename(&report_partial, &arguments.output_report)?;
    fs::rename(&output_partial, &arguments.output_rlog)?;
    println!(
        "decoded {} journal records into {} canonical events",
        result.report.record_count, result.report.rlog.event_count
    );
    println!("wrote {}", arguments.output_report.display());
    println!("wrote {}", arguments.output_rlog.display());
    Ok(())
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut journal = None;
    let mut pack = None;
    let mut source_pack = None;
    let mut output_rlog = None;
    let mut output_report = None;
    let mut recover_truncated_tail = false;
    let mut args = env::args_os().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--recover-truncated-tail" {
            recover_truncated_tail = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{} requires a value", flag.to_string_lossy()))?;
        match flag.to_string_lossy().as_ref() {
            "--journal" => journal = Some(PathBuf::from(value)),
            "--pack" => pack = Some(PathBuf::from(value)),
            "--source-pack" => source_pack = Some(PathBuf::from(value)),
            "--output-rlog" => output_rlog = Some(PathBuf::from(value)),
            "--output-report" => output_report = Some(PathBuf::from(value)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        journal: journal.ok_or("--journal is required")?,
        pack: pack.ok_or("--pack is required")?,
        source_pack,
        output_rlog: output_rlog.ok_or("--output-rlog is required")?,
        output_report: output_report.ok_or("--output-report is required")?,
        recover_truncated_tail,
    })
}

fn ensure_available(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Err(format!("refusing to overwrite {}", path.display()).into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn partial_path(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let mut partial_name = file_name.to_os_string();
    partial_name.push(".partial");
    Ok(path.with_file_name(partial_name))
}
