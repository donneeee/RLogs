#[cfg(any(windows, test))]
use std::ffi::{OsStr, OsString};
#[cfg(any(windows, test))]
use std::path::PathBuf;
#[cfg(windows)]
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

#[cfg(windows)]
use rlogs_capture::{
    CaptureSource, DumpcapLiveConfig, OwnedProcessCaptureConfig, PcapWriter, TcpConnection,
    WindowsOwnedDumpcapCapture,
};
#[cfg(windows)]
use serde::Serialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("process-owned capture failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("process-owned live capture is currently implemented only for Windows".into())
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let paths = CapturePaths::new(&arguments.output_directory, &arguments.capture_id)?;
    paths.ensure_available()?;

    let mut capture = WindowsOwnedDumpcapCapture::spawn(
        arguments.process_id,
        DumpcapLiveConfig::new(
            &arguments.dumpcap,
            arguments.interface,
            arguments.duration_seconds,
        )?,
        OwnedProcessCaptureConfig::default(),
    )?;
    let mut writer: Option<PcapWriter<BufWriter<File>>> = None;

    while let Some(frame) = capture.next_frame()? {
        let writer = match &mut writer {
            Some(writer) => writer,
            None => writer.insert(PcapWriter::new(
                BufWriter::new(
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&paths.capture_partial)?,
                ),
                frame.link_type,
            )?),
        };
        writer.write_frame(&frame)?;
    }

    let metrics = capture.metrics().clone();
    let connections = capture.confirmed_connections();
    let Some(mut writer) = writer else {
        return Err(format!(
            "no frames owned by the target process were captured; empty partial file remains at {}",
            paths.capture_partial.display()
        )
        .into());
    };
    writer.flush()?;
    let mut capture_buffer = writer.into_inner();
    capture_buffer.flush()?;
    capture_buffer.get_ref().sync_all()?;
    drop(capture_buffer);

    if connections.is_empty() {
        return Err("owned frames were emitted without connection evidence".into());
    }
    let accounted_frames = metrics
        .emitted_frames
        .saturating_add(metrics.non_tcp_frames_discarded)
        .saturating_add(metrics.unattributed_frames_discarded);
    if accounted_frames != metrics.ingress_frames {
        return Err(format!(
            "capture accounting mismatch: ingested {} frames but classified {}",
            metrics.ingress_frames, accounted_frames
        )
        .into());
    }
    write_connection_evidence(&paths.connections_partial, connections)?;
    std::fs::rename(&paths.capture_partial, &paths.capture)?;
    std::fs::rename(&paths.connections_partial, &paths.connections)?;

    println!(
        "captured {} process-owned frames ({} bytes) across {} exact game connection(s)",
        metrics.emitted_frames,
        metrics.emitted_bytes,
        capture.confirmed_connections().len()
    );
    println!(
        "discarded {} unattributed interface frames before persistence or protocol decoding",
        metrics.unattributed_frames_discarded
    );
    println!(
        "discarded {} non-TCP or unparseable frames before persistence",
        metrics.non_tcp_frames_discarded
    );
    println!(
        "capture accounting: {} ingested = {} retained + {} discarded",
        metrics.ingress_frames,
        metrics.emitted_frames,
        metrics
            .non_tcp_frames_discarded
            .saturating_add(metrics.unattributed_frames_discarded)
    );
    println!("private capture: {}", paths.capture.display());
    println!(
        "private connection evidence: {}",
        paths.connections.display()
    );
    println!("these files remain local research data and are not website submissions");
    Ok(())
}

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    process_id: u32,
    interface: String,
    dumpcap: PathBuf,
    capture_id: String,
    duration_seconds: u32,
    output_directory: PathBuf,
}

#[cfg(windows)]
fn arguments() -> Result<Arguments, String> {
    parse_arguments(std::env::args_os().skip(1))
}

#[cfg(any(windows, test))]
fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut private_research = false;
    let mut process_id = None;
    let mut interface = None;
    let mut dumpcap = None;
    let mut capture_id = None;
    let mut duration_seconds = None;
    let mut output_directory = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--private-research") {
            private_research = true;
        } else if argument == OsStr::new("--process-id") {
            process_id = unique_value(process_id, arguments.next(), "--process-id")?;
        } else if argument == OsStr::new("--interface") {
            interface = unique_value(interface, arguments.next(), "--interface")?;
        } else if argument == OsStr::new("--dumpcap") {
            dumpcap = unique_value(dumpcap, arguments.next(), "--dumpcap")?;
        } else if argument == OsStr::new("--capture-id") {
            capture_id = unique_value(capture_id, arguments.next(), "--capture-id")?;
        } else if argument == OsStr::new("--duration-seconds") {
            duration_seconds =
                unique_value(duration_seconds, arguments.next(), "--duration-seconds")?;
        } else if argument == OsStr::new("--output-directory") {
            output_directory =
                unique_value(output_directory, arguments.next(), "--output-directory")?;
        } else {
            return Err(usage());
        }
    }

    if !private_research {
        return Err(usage());
    }
    let process_id = parse_u32(process_id, "--process-id", 1, u32::MAX)?;
    let duration_seconds = parse_u32(duration_seconds, "--duration-seconds", 1, 3_600)?;
    let interface = required_utf8(interface, "--interface")?;
    if interface.trim().is_empty() {
        return Err("--interface must not be empty".into());
    }
    let capture_id = required_utf8(capture_id, "--capture-id")?;
    if !valid_capture_id(&capture_id) {
        return Err("capture ID must use 1-128 ASCII letters, digits, '.', '_', or '-'".into());
    }

    Ok(Arguments {
        process_id,
        interface,
        dumpcap: PathBuf::from(dumpcap.ok_or_else(usage)?),
        capture_id,
        duration_seconds,
        output_directory: PathBuf::from(output_directory.ok_or_else(usage)?),
    })
}

#[cfg(any(windows, test))]
fn unique_value(
    current: Option<OsString>,
    next: Option<OsString>,
    flag: &str,
) -> Result<Option<OsString>, String> {
    if current.is_some() {
        return Err(format!("{flag} may be supplied only once"));
    }
    next.map(Some)
        .ok_or_else(|| format!("{flag} requires a value"))
}

#[cfg(any(windows, test))]
fn required_utf8(value: Option<OsString>, flag: &str) -> Result<String, String> {
    value
        .ok_or_else(usage)?
        .into_string()
        .map_err(|_| format!("{flag} must be valid UTF-8"))
}

#[cfg(any(windows, test))]
fn parse_u32(
    value: Option<OsString>,
    flag: &str,
    minimum: u32,
    maximum: u32,
) -> Result<u32, String> {
    let value = required_utf8(value, flag)?;
    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("{flag} must be an integer"))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(format!("{flag} must be between {minimum} and {maximum}"));
    }
    Ok(parsed)
}

#[cfg(any(windows, test))]
fn valid_capture_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(any(windows, test))]
fn usage() -> String {
    "usage: rlogs-process-capture --private-research --process-id <pid> --interface <npcap-interface> --dumpcap <dumpcap.exe> --capture-id <id> --duration-seconds <1-3600> --output-directory <directory>".into()
}

#[cfg(windows)]
#[derive(Debug)]
struct CapturePaths {
    capture: PathBuf,
    capture_partial: PathBuf,
    connections: PathBuf,
    connections_partial: PathBuf,
}

#[cfg(windows)]
impl CapturePaths {
    fn new(directory: &Path, capture_id: &str) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(directory)?;
        let directory = std::fs::canonicalize(directory)?;
        Ok(Self {
            capture: directory.join(format!("{capture_id}.pcap")),
            capture_partial: directory.join(format!("{capture_id}.partial.pcap")),
            connections: directory.join(format!("{capture_id}.connections.json")),
            connections_partial: directory.join(format!("{capture_id}.connections.partial.json")),
        })
    }

    fn ensure_available(&self) -> Result<(), Box<dyn std::error::Error>> {
        for path in [
            &self.capture,
            &self.capture_partial,
            &self.connections,
            &self.connections_partial,
        ] {
            if path.exists() {
                return Err(format!(
                    "refusing to overwrite existing private research file: {}",
                    path.display()
                )
                .into());
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
#[derive(Serialize)]
struct ConnectionEvidence {
    schema_version: u16,
    connections: Vec<TcpConnection>,
}

#[cfg(windows)]
fn write_connection_evidence(
    path: &Path,
    connections: Vec<TcpConnection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(
        &mut writer,
        &ConnectionEvidence {
            schema_version: 1,
            connections,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn private_acknowledgement_and_bounded_duration_are_required() {
        assert!(parse_arguments(args(&[])).is_err());
        assert!(
            parse_arguments(args(&[
                "--private-research",
                "--process-id",
                "10",
                "--interface",
                "npcap",
                "--dumpcap",
                "dumpcap.exe",
                "--capture-id",
                "test",
                "--duration-seconds",
                "0",
                "--output-directory",
                "private",
            ]))
            .is_err()
        );
    }

    #[test]
    fn exact_arguments_are_parsed_without_positionals() {
        let parsed = parse_arguments(args(&[
            "--private-research",
            "--process-id",
            "10",
            "--interface",
            r"\Device\NPF_test",
            "--dumpcap",
            r"C:\Wireshark\dumpcap.exe",
            "--capture-id",
            "world-load-process-001",
            "--duration-seconds",
            "180",
            "--output-directory",
            r"C:\private",
        ]))
        .unwrap();

        assert_eq!(parsed.process_id, 10);
        assert_eq!(parsed.duration_seconds, 180);
        assert_eq!(parsed.capture_id, "world-load-process-001");
    }
}
