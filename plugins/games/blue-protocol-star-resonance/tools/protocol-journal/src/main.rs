use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rlogs_capture::{CaptureSource, OfflineCapture, ValidatedCapture};
use rlogs_core::ResearchConnectionFile;
use rlogs_game_bpsr::{
    BpsrFrameUpLayout, BpsrFramerSetConfig, BpsrFramingConfig, CaptureAdapter, CaptureSession,
    GameBuild, JsonlJournalWriter, ProtocolPack, ProtocolPackJournalAuthority, ResearchPipeline,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol journal failed: {error}");
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

    let pack = ProtocolPack::from_json(&std::fs::read(&arguments.pack)?)?;
    let connections: ResearchConnectionFile =
        serde_json::from_slice(&std::fs::read(&arguments.connections)?)?;
    let connections = connections.validate()?;
    let target = &pack.definition().target;
    let pack_build = GameBuild {
        deployment_id: target.deployment_id.clone(),
        region_id: target.region_id.clone(),
        channel: target.channel.clone(),
        build_id: target.build_id.clone(),
        executable_version: target.executable_version.clone(),
    };
    if !pack.matches(&pack_build) {
        return Err("protocol pack does not match its own exact build target".into());
    }
    let (game_build, protocol_pack_authority) = journal_identity(
        &pack_build,
        arguments.captured_build.as_deref(),
        arguments
            .unverified_carry_forward_pack_source_build
            .as_deref(),
    )?;

    let offline = OfflineCapture::open(&arguments.input)?;
    let adapter_name = offline.metadata().source_id.clone();
    let mut capture = ValidatedCapture::new(offline);
    let first = capture
        .next_frame()?
        .ok_or("capture contains no packet frames")?;
    let session = CaptureSession {
        format_version: 1,
        capture_id: arguments.capture_id,
        started_unix_micros: first
            .source_timestamp_nanos
            .and_then(|value| value.checked_div(1_000)),
        game_build,
        adapter: CaptureAdapter {
            name: adapter_name,
            version: Some(env!("CARGO_PKG_VERSION").into()),
        },
        protocol_pack_digest: Some(pack.digest().to_owned()),
        protocol_pack_authority,
    };

    let partial = partial_path(&arguments.output)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let mut writer = JsonlJournalWriter::new(BufWriter::new(file), session)?;
    let mut pipeline = if arguments.nested_frame_up {
        ResearchPipeline::try_with_framing_config(
            connections,
            BpsrFramerSetConfig {
                stream: BpsrFramingConfig {
                    frame_up_layout: BpsrFrameUpLayout::NestedAfterFourBytes,
                    ..BpsrFramingConfig::default()
                },
                ..BpsrFramerSetConfig::default()
            },
        )?
    } else {
        ResearchPipeline::new(connections)
    };
    let mut frames = 0_u64;
    let mut records = 0_u64;

    process_frame(&mut pipeline, &mut writer, first, &mut records)?;
    frames += 1;
    while let Some(frame) = capture.next_frame()? {
        process_frame(&mut pipeline, &mut writer, frame, &mut records)?;
        frames = frames.saturating_add(1);
    }
    append_pipeline_records(&mut writer, |emit| pipeline.finish(emit), &mut records)?;
    writer.flush()?;
    writer.inner_ref().get_ref().sync_all()?;
    drop(writer);
    std::fs::rename(&partial, &arguments.output)?;

    println!(
        "wrote {records} private research records from {frames} frames to {}",
        arguments.output.display()
    );
    println!(
        "protocol pack: {} ({})",
        pack.definition().pack_id,
        pack.digest()
    );
    println!("raw capture and JSONL remain local-only; neither is a website submission file");
    Ok(())
}

fn process_frame(
    pipeline: &mut ResearchPipeline,
    writer: &mut JsonlJournalWriter<BufWriter<File>>,
    frame: rlogs_capture::CapturedFrame,
    records: &mut u64,
) -> Result<(), Box<dyn std::error::Error>> {
    append_pipeline_records(writer, |emit| pipeline.process_frame(&frame, emit), records)
}

fn append_pipeline_records(
    writer: &mut JsonlJournalWriter<BufWriter<File>>,
    process: impl FnOnce(&mut dyn FnMut(rlogs_game_bpsr::CaptureRecordDraft)),
    records: &mut u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut error = None;
    let mut emit = |record| {
        if error.is_some() {
            return;
        }
        match writer.append(record) {
            Ok(_) => *records = records.saturating_add(1),
            Err(source) => error = Some(source),
        }
    };
    process(&mut emit);
    if let Some(error) = error {
        return Err(error.into());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    pack: PathBuf,
    connections: PathBuf,
    capture_id: String,
    input: PathBuf,
    output: PathBuf,
    nested_frame_up: bool,
    captured_build: Option<String>,
    unverified_carry_forward_pack_source_build: Option<String>,
}

fn arguments() -> Result<Arguments, String> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut pack = None;
    let mut connections = None;
    let mut capture_id = None;
    let mut private_research = false;
    let mut nested_frame_up = false;
    let mut captured_build = None;
    let mut unverified_carry_forward_pack_source_build = None;
    let mut positional = Vec::new();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--private-research") {
            private_research = true;
        } else if argument == OsStr::new("--nested-frame-up") {
            nested_frame_up = true;
        } else if argument == OsStr::new("--pack") {
            pack = unique_value(pack, arguments.next(), "--pack")?;
        } else if argument == OsStr::new("--connections") {
            connections = unique_value(connections, arguments.next(), "--connections")?;
        } else if argument == OsStr::new("--capture-id") {
            capture_id = unique_value(capture_id, arguments.next(), "--capture-id")?;
        } else if argument == OsStr::new("--captured-build") {
            captured_build = unique_value(captured_build, arguments.next(), "--captured-build")?;
        } else if argument == OsStr::new("--unverified-carry-forward-pack-source-build") {
            unverified_carry_forward_pack_source_build = unique_value(
                unverified_carry_forward_pack_source_build,
                arguments.next(),
                "--unverified-carry-forward-pack-source-build",
            )?;
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(usage());
        } else {
            positional.push(PathBuf::from(argument));
        }
    }

    if !private_research || positional.len() != 2 {
        return Err(usage());
    }
    let capture_id = capture_id
        .and_then(|value: OsString| value.into_string().ok())
        .ok_or_else(usage)?;
    if !valid_capture_id(&capture_id) {
        return Err("capture ID must use 1-128 ASCII letters, digits, '.', '_', or '-'".into());
    }
    let captured_build = optional_build(captured_build, "captured build")?;
    let unverified_carry_forward_pack_source_build = optional_build(
        unverified_carry_forward_pack_source_build,
        "carry-forward pack source build",
    )?;
    if captured_build.is_some() != unverified_carry_forward_pack_source_build.is_some() {
        return Err("--captured-build and --unverified-carry-forward-pack-source-build must be supplied together".into());
    }

    Ok(Arguments {
        pack: pack.map(PathBuf::from).ok_or_else(usage)?,
        connections: connections.map(PathBuf::from).ok_or_else(usage)?,
        capture_id,
        input: positional.remove(0),
        output: positional.remove(0),
        nested_frame_up,
        captured_build,
        unverified_carry_forward_pack_source_build,
    })
}

fn unique_value(
    current: Option<OsString>,
    next: Option<OsString>,
    _flag: &str,
) -> Result<Option<OsString>, String> {
    if current.is_some() {
        return Err(usage());
    }
    Ok(Some(next.ok_or_else(usage)?))
}

fn valid_capture_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn journal_identity(
    pack_build: &GameBuild,
    captured_build: Option<&str>,
    carry_forward_source_build: Option<&str>,
) -> Result<(GameBuild, Option<ProtocolPackJournalAuthority>), String> {
    match (captured_build, carry_forward_source_build) {
        (None, None) => Ok((pack_build.clone(), None)),
        (Some(captured_build), Some(source_build)) => {
            if source_build != pack_build.build_id {
                return Err(format!(
                    "declared carry-forward pack source build {source_build} does not match pack target {}",
                    pack_build.build_id
                ));
            }
            if captured_build == source_build {
                return Err("unverified carry-forward mode requires distinct captured and pack-source builds".into());
            }
            let mut game_build = pack_build.clone();
            game_build.build_id = captured_build.to_owned();
            Ok((
                game_build,
                Some(ProtocolPackJournalAuthority {
                    kind: "unverified-carry-forward-decoder-hypothesis".into(),
                    source_build: source_build.to_owned(),
                    captured_build: captured_build.to_owned(),
                    exact_for_captured_build: false,
                    runtime_authority: false,
                }),
            ))
        }
        _ => Err("--captured-build and --unverified-carry-forward-pack-source-build must be supplied together".into()),
    }
}

fn optional_build(value: Option<OsString>, description: &str) -> Result<Option<String>, String> {
    let Some(value) = value else { return Ok(None) };
    let value = value
        .into_string()
        .map_err(|_| format!("{description} must be valid UTF-8"))?;
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "{description} must use 1-128 ASCII letters, digits, '.', '_', or '-'"
        ));
    }
    Ok(Some(value))
}

fn partial_path(output: &Path) -> Result<PathBuf, String> {
    let name = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("output must have a valid UTF-8 filename")?;
    Ok(output.with_file_name(format!(".{name}.partial")))
}

fn usage() -> String {
    "usage: rlogs-protocol-journal --private-research [--nested-frame-up] [--captured-build <actual-build> --unverified-carry-forward-pack-source-build <pack-build>] --pack <pack.json> --connections <connections.json> --capture-id <id> <capture.pcapng> <output.jsonl>".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn private_acknowledgement_and_exact_inputs_are_required() {
        let parsed = parse_arguments(os(&[
            "--private-research",
            "--pack",
            "pack.json",
            "--connections",
            "connections.json",
            "--capture-id",
            "controlled-001",
            "capture.pcapng",
            "capture.jsonl",
        ]))
        .unwrap();

        assert_eq!(parsed.capture_id, "controlled-001");
        assert!(!parsed.nested_frame_up);
        assert_eq!(parsed.captured_build, None);
        let nested = parse_arguments(os(&[
            "--private-research",
            "--nested-frame-up",
            "--pack",
            "pack.json",
            "--connections",
            "connections.json",
            "--capture-id",
            "controlled-002",
            "capture.pcapng",
            "capture.jsonl",
        ]))
        .unwrap();
        assert!(nested.nested_frame_up);
        let carry_forward = parse_arguments(os(&[
            "--private-research",
            "--pack",
            "pack.json",
            "--connections",
            "connections.json",
            "--capture-id",
            "controlled-003",
            "--captured-build",
            "25247556",
            "--unverified-carry-forward-pack-source-build",
            "24687926",
            "capture.pcapng",
            "capture.jsonl",
        ]))
        .unwrap();
        assert_eq!(carry_forward.captured_build.as_deref(), Some("25247556"));
        assert_eq!(
            carry_forward
                .unverified_carry_forward_pack_source_build
                .as_deref(),
            Some("24687926")
        );
        assert!(
            parse_arguments(os(&[
                "--private-research",
                "--pack",
                "pack.json",
                "--connections",
                "connections.json",
                "--capture-id",
                "controlled-004",
                "--captured-build",
                "25247556",
                "capture.pcapng",
                "capture.jsonl",
            ]))
            .is_err()
        );
        assert!(
            parse_arguments(os(&[
                "--pack",
                "pack.json",
                "--connections",
                "connections.json",
                "--capture-id",
                "controlled-001",
                "capture.pcapng",
                "capture.jsonl",
            ]))
            .is_err()
        );
    }

    #[test]
    fn output_partial_file_is_visible_and_adjacent() {
        assert_eq!(
            partial_path(Path::new("private/capture.jsonl")).unwrap(),
            PathBuf::from("private/.capture.jsonl.partial")
        );
    }

    #[test]
    fn carry_forward_identity_keeps_captured_and_pack_builds_distinct() {
        let pack_build = GameBuild {
            deployment_id: "global".into(),
            region_id: None,
            channel: "steam".into(),
            build_id: "24687926".into(),
            executable_version: None,
        };
        let (captured, authority) =
            journal_identity(&pack_build, Some("25247556"), Some("24687926")).unwrap();
        let authority = authority.unwrap();
        assert_eq!(captured.build_id, "25247556");
        assert_eq!(authority.source_build, "24687926");
        assert_eq!(authority.captured_build, "25247556");
        assert!(!authority.exact_for_captured_build);
        assert!(!authority.runtime_authority);
        assert!(journal_identity(&pack_build, Some("25247556"), Some("wrong")).is_err());
        assert!(journal_identity(&pack_build, Some("24687926"), Some("24687926")).is_err());
    }
}
