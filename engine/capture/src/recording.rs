use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::{
    CaptureError, CaptureSource, OwnedProcessCaptureMetrics, PcapWriteError, PcapWriter,
    TcpConnection, WindowsOwnedDumpcapCapture,
};

#[derive(Debug, Clone, Serialize)]
pub struct OwnedCaptureRecordingResult {
    pub capture_path: PathBuf,
    pub connections_path: PathBuf,
    pub metrics: OwnedProcessCaptureMetrics,
    pub connections: Vec<TcpConnection>,
}

/// Persists only frames that have already crossed the process-ownership
/// boundary, then atomically publishes the PCAP and exact connection evidence.
pub fn record_owned_capture_to_files(
    mut capture: WindowsOwnedDumpcapCapture,
    output_directory: &Path,
    capture_id: &str,
) -> Result<OwnedCaptureRecordingResult, OwnedCaptureRecordingError> {
    validate_capture_id(capture_id)?;
    std::fs::create_dir_all(output_directory)?;
    let directory = std::fs::canonicalize(output_directory)?;
    let capture_path = directory.join(format!("{capture_id}.pcap"));
    let capture_partial = directory.join(format!("{capture_id}.partial.pcap"));
    let connections_path = directory.join(format!("{capture_id}.connections.json"));
    let connections_partial = directory.join(format!("{capture_id}.connections.partial.json"));
    for path in [
        &capture_path,
        &capture_partial,
        &connections_path,
        &connections_partial,
    ] {
        if path.exists() {
            return Err(OwnedCaptureRecordingError::OutputExists(path.clone()));
        }
    }

    let result = (|| {
        let mut writer: Option<PcapWriter<BufWriter<File>>> = None;
        while let Some(frame) = capture.next_frame()? {
            let writer = match &mut writer {
                Some(writer) => writer,
                None => writer.insert(PcapWriter::new(
                    BufWriter::new(
                        OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&capture_partial)?,
                    ),
                    frame.link_type,
                )?),
            };
            writer.write_frame(&frame)?;
        }

        let metrics = capture.metrics().clone();
        let connections = capture.confirmed_connections();
        let Some(mut writer) = writer else {
            return Err(OwnedCaptureRecordingError::NoOwnedFrames);
        };
        writer.flush()?;
        let mut capture_buffer = writer.into_inner();
        capture_buffer.flush()?;
        capture_buffer.get_ref().sync_all()?;
        drop(capture_buffer);

        if connections.is_empty() {
            return Err(OwnedCaptureRecordingError::NoConnectionEvidence);
        }
        let accounted_frames = metrics
            .emitted_frames
            .saturating_add(metrics.non_tcp_frames_discarded)
            .saturating_add(metrics.unattributed_frames_discarded);
        if accounted_frames != metrics.ingress_frames {
            return Err(OwnedCaptureRecordingError::AccountingMismatch {
                ingress: metrics.ingress_frames,
                classified: accounted_frames,
            });
        }
        write_connection_evidence(&connections_partial, &connections)?;
        std::fs::rename(&capture_partial, &capture_path)?;
        std::fs::rename(&connections_partial, &connections_path)?;
        Ok(OwnedCaptureRecordingResult {
            capture_path: capture_path.clone(),
            connections_path: connections_path.clone(),
            metrics,
            connections,
        })
    })();

    if result.is_err() {
        remove_owned_partial(&capture_partial);
        remove_owned_partial(&connections_partial);
    }
    result
}

fn validate_capture_id(value: &str) -> Result<(), OwnedCaptureRecordingError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(OwnedCaptureRecordingError::InvalidCaptureId);
    }
    Ok(())
}

fn write_connection_evidence(
    path: &Path,
    connections: &[TcpConnection],
) -> Result<(), OwnedCaptureRecordingError> {
    #[derive(Serialize)]
    struct ConnectionEvidence<'a> {
        schema_version: u16,
        connections: &'a [TcpConnection],
    }

    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(
        &mut writer,
        &ConnectionEvidence {
            schema_version: 1,
            connections,
        },
    )
    .map_err(|error| OwnedCaptureRecordingError::ConnectionEvidence(error.to_string()))?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn remove_owned_partial(path: &Path) {
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, Error)]
pub enum OwnedCaptureRecordingError {
    #[error("capture ID must use 1-128 ASCII letters, digits, '.', '_', or '-'")]
    InvalidCaptureId,

    #[error("refusing to overwrite existing private capture output {0}")]
    OutputExists(PathBuf),

    #[error("no frames owned by the target process were captured")]
    NoOwnedFrames,

    #[error("owned frames were emitted without exact connection evidence")]
    NoConnectionEvidence,

    #[error("capture accounting mismatch: ingested {ingress} frames but classified {classified}")]
    AccountingMismatch { ingress: u64, classified: u64 },

    #[error("could not serialize exact connection evidence: {0}")]
    ConnectionEvidence(String),

    #[error(transparent)]
    Capture(#[from] CaptureError),

    #[error(transparent)]
    Pcap(#[from] PcapWriteError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
