#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fmt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    CaptureError, CaptureSource, CaptureSourceKind, CaptureSourceMetadata, CapturedFrame,
    OfflineCapture,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpcapLiveConfig {
    pub dumpcap_path: PathBuf,
    pub interface: String,
    pub duration_seconds: u32,
}

impl DumpcapLiveConfig {
    pub fn new(
        dumpcap_path: impl Into<PathBuf>,
        interface: impl Into<String>,
        duration_seconds: u32,
    ) -> Result<Self, CaptureError> {
        let config = Self {
            dumpcap_path: dumpcap_path.into(),
            interface: interface.into(),
            duration_seconds,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), CaptureError> {
        if self.interface.trim().is_empty() {
            return Err(adapter_error("capture interface must not be empty"));
        }
        if self.duration_seconds > 3_600 {
            return Err(adapter_error(
                "capture duration must be 0 (continuous) or at most 3600 seconds",
            ));
        }
        if !Path::new(&self.dumpcap_path).is_file() {
            return Err(adapter_error("dumpcap executable was not found"));
        }
        Ok(())
    }
}

/// Live dumpcap ingress.
///
/// Dumpcap writes pcapng to a child-process pipe. This adapter never gives it a
/// filesystem output path, so unfiltered interface traffic cannot be persisted
/// by dumpcap. `OwnedProcessCapture` must wrap this source before any consumer
/// writes frames or sends them into the protocol pipeline.
pub(crate) struct DumpcapLiveCapture {
    control: Arc<DumpcapLiveControl>,
    source: OfflineCapture,
    metadata: CaptureSourceMetadata,
    finished: bool,
}

struct DumpcapLiveControl {
    child: Mutex<Child>,
    stop_requested: AtomicBool,
}

impl fmt::Debug for DumpcapLiveControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DumpcapLiveControl")
            .field(
                "stop_requested",
                &self.stop_requested.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

/// Thread-safe cooperative stop token for one live capture.
///
/// Stopping terminates only the private dumpcap ingress. The capture consumer
/// still receives EOF and can flush process-owned frames, connection evidence,
/// and log files before reporting completion.
#[derive(Debug, Clone)]
pub struct LiveCaptureStopHandle {
    control: Arc<DumpcapLiveControl>,
}

impl LiveCaptureStopHandle {
    pub fn request_stop(&self) -> Result<(), CaptureError> {
        self.control.stop_requested.store(true, Ordering::Release);
        let mut child = self
            .control
            .child
            .lock()
            .map_err(|_| adapter_error("dumpcap process lock was poisoned"))?;
        if child
            .try_wait()
            .map_err(|error| adapter_error(format!("could not inspect dumpcap: {error}")))?
            .is_none()
        {
            child
                .kill()
                .map_err(|error| adapter_error(format!("could not stop dumpcap: {error}")))?;
        }
        Ok(())
    }
}

impl fmt::Debug for DumpcapLiveCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let child_id = self.control.child.lock().ok().map(|child| child.id());
        formatter
            .debug_struct("DumpcapLiveCapture")
            .field("child_id", &child_id)
            .field("metadata", &self.metadata)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl DumpcapLiveCapture {
    pub(crate) fn spawn(config: DumpcapLiveConfig) -> Result<Self, CaptureError> {
        config.validate()?;
        let mut command = Command::new(&config.dumpcap_path);
        command
            .args([
                "-q",
                "--update-interval",
                "10",
                "-i",
                config.interface.as_str(),
                "-p",
                "-s",
                "0",
                "-f",
                "tcp",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // dumpcap's status text belongs to the host's diagnostics, not a
            // user-facing console window. Exit status is still validated.
            .stderr(Stdio::null());
        if config.duration_seconds > 0 {
            command.args([
                "-a",
                format!("duration:{}", config.duration_seconds).as_str(),
            ]);
        }
        command.args(["-w", "-"]);
        // `dumpcap.exe` is a console-subsystem binary. A GUI parent still gets
        // a visible child console unless Windows is explicitly told not to
        // create one; stream redirection alone is insufficient.
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|error| adapter_error(format!("could not start dumpcap: {error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| adapter_error("dumpcap stdout pipe was unavailable"))?;
        let source = match OfflineCapture::from_reader(
            "dumpcap-live-ingress",
            "Process-filtered dumpcap ingress",
            stdout,
        ) {
            Ok(source) => source,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        Ok(Self {
            control: Arc::new(DumpcapLiveControl {
                child: Mutex::new(child),
                stop_requested: AtomicBool::new(false),
            }),
            source,
            metadata: CaptureSourceMetadata {
                source_id: "dumpcap-process-owned".into(),
                display_name: "Process-owned live capture".into(),
                kind: CaptureSourceKind::Live,
                link_types: Vec::new(),
                file_format: None,
            },
            finished: false,
        })
    }

    pub(crate) fn stop_handle(&self) -> LiveCaptureStopHandle {
        LiveCaptureStopHandle {
            control: Arc::clone(&self.control),
        }
    }

    fn finish_child(&mut self) -> Result<(), CaptureError> {
        if self.finished {
            return Ok(());
        }
        let status = self
            .control
            .child
            .lock()
            .map_err(|_| adapter_error("dumpcap process lock was poisoned"))?
            .wait()
            .map_err(|error| adapter_error(format!("could not wait for dumpcap: {error}")))?;
        self.finished = true;
        if !status.success() && !self.control.stop_requested.load(Ordering::Acquire) {
            return Err(adapter_error(format!(
                "dumpcap exited unsuccessfully ({status})"
            )));
        }
        Ok(())
    }
}

impl CaptureSource for DumpcapLiveCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        &self.metadata
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        let frame = self.source.next_frame()?;
        if let Some(frame) = &frame {
            if !self.metadata.link_types.contains(&frame.link_type) {
                self.metadata.link_types.push(frame.link_type);
            }
            self.metadata.file_format = self.source.metadata().file_format;
        } else {
            self.finish_child()?;
        }
        Ok(frame)
    }
}

impl Drop for DumpcapLiveCapture {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(mut child) = self.control.child.lock() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.finished = true;
    }
}

fn adapter_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "dumpcap".into(),
        message: message.into(),
    }
}
