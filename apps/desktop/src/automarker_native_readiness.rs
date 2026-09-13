//! Private, read-only game-PC readiness discovery for the Automarker bridge.
//!
//! This boundary may inspect local process/socket state and briefly open only
//! SNIFF|RECV_ONLY false-filter handles. It never diverts a game packet and it
//! exposes no HTTP or plug-in API.

use std::{
    ffi::OsString,
    mem::size_of,
    net::{IpAddr, Ipv4Addr},
    os::windows::ffi::OsStringExt,
    path::Path,
    process::Command,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use rlogs_capture::{TcpConnection, WindowsProcessSocketOwner};
use rlogs_game_bpsr::{
    AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection, OfflineAutomarkerConnectionEpochBinding,
    PinnedWinDivertChecksumHelper, bind_offline_automarker_connection_epoch,
    bind_offline_automarker_established_connection_epoch,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

use crate::{
    automarker_bridge_evidence::AutomarkerBridgeCaptureTcpConnection,
    automarker_windivert_backend::{
        ArbitratedNetworkOpen, FLAG_NO_INSTALL, FLAG_RECV_ONLY, FLAG_SNIFF, PARAM_VERSION_MAJOR,
        PARAM_VERSION_MINOR, PinnedWinDivertBackend,
    },
};

const PASSIVE_READINESS_FLAGS: u64 = FLAG_SNIFF | FLAG_RECV_ONLY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomarkerNativeFailureCategory {
    NotElevated,
    DependencyHashMismatch,
    DriverSignatureInvalid,
    DriverOpenFailed,
    HandleConflict,
    ProcessOrSocket,
    Internal,
}

impl AutomarkerNativeFailureCategory {
    const fn code(self) -> u8 {
        match self {
            Self::NotElevated => 1,
            Self::DependencyHashMismatch => 2,
            Self::DriverSignatureInvalid => 3,
            Self::DriverOpenFailed => 4,
            Self::HandleConflict => 5,
            Self::ProcessOrSocket => 6,
            Self::Internal => 7,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::NotElevated,
            2 => Self::DependencyHashMismatch,
            3 => Self::DriverSignatureInvalid,
            4 => Self::DriverOpenFailed,
            5 => Self::HandleConflict,
            6 => Self::ProcessOrSocket,
            7 => Self::Internal,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotElevated => "not_elevated",
            Self::DependencyHashMismatch => "dependency_hash_mismatch",
            Self::DriverSignatureInvalid => "driver_signature_invalid",
            Self::DriverOpenFailed => "driver_open_failed",
            Self::HandleConflict => "handle_conflict",
            Self::ProcessOrSocket => "process_or_socket",
            Self::Internal => "internal",
        }
    }
}

pub(crate) fn sanitized_failure_category(error: &str) -> AutomarkerNativeFailureCategory {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("elevated") {
        AutomarkerNativeFailureCategory::NotElevated
    } else if normalized.contains("hash") || normalized.contains("release identity") {
        AutomarkerNativeFailureCategory::DependencyHashMismatch
    } else if normalized.contains("signature") || normalized.contains("authenticode") {
        AutomarkerNativeFailureCategory::DriverSignatureInvalid
    } else if normalized.contains("same-priority") || normalized.contains("conflict") {
        AutomarkerNativeFailureCategory::HandleConflict
    } else if normalized.contains("process")
        || normalized.contains("pid")
        || normalized.contains("socket")
        || normalized.contains("owned tuple")
        || normalized.contains("syn")
    {
        AutomarkerNativeFailureCategory::ProcessOrSocket
    } else if normalized.contains("windivert")
        || normalized.contains("driver")
        || normalized.contains("filtering engine")
        || normalized.contains("open")
        || normalized.contains("dll")
    {
        AutomarkerNativeFailureCategory::DriverOpenFailed
    } else {
        AutomarkerNativeFailureCategory::Internal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomarkerPassiveReadinessStatus {
    WaitingForNewSyn,
    NativeReadinessProven,
    Failed(AutomarkerNativeFailureCategory),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AutomarkerNativeReadinessEvidence {
    pub binding: OfflineAutomarkerConnectionEpochBinding,
    /// A false-filter REFLECT inventory was clear during this read-only probe.
    /// Active open must arbitrate again; this is not send authorization.
    pub reflect_preflight_clear: bool,
    pub pinned_backend_ready: bool,
    pub checksum_helper_ready: bool,
}

pub(crate) struct AutomarkerNativeReadinessRequest<'a> {
    pub process: AutomarkerReviewedGameProcessIdentity<'a>,
    pub dependency_directory: &'a Path,
    pub capture: AutomarkerBridgeCaptureTcpConnection,
    pub connection_epoch: u64,
    pub observed_syn_packet: &'a [u8],
    pub syn_capture_sequence: u64,
    pub syn_observed_micros: u64,
}

/// A process identity already selected by the host's manifest-backed game
/// discovery. The readiness boundary independently corroborates both fields
/// against the current process snapshot before trusting its socket table.
#[allow(dead_code)] // Integration is deliberately deferred to the native bridge.
pub(crate) struct AutomarkerReviewedGameProcessIdentity<'a> {
    pub process_id: u32,
    pub executable_name: &'a str,
}

/// Inputs for proving readiness from an already-active parser connection.
/// `connection_epoch` is a fresh host-owned epoch supplied for this proof; no
/// packet timestamp or SYN observation is accepted by this path.
#[allow(dead_code)] // Integration is deliberately deferred to the native bridge.
pub(crate) struct AutomarkerEstablishedConnectionReadinessRequest<'a> {
    pub process: AutomarkerReviewedGameProcessIdentity<'a>,
    pub dependency_directory: &'a Path,
    pub capture: AutomarkerBridgeCaptureTcpConnection,
    pub connection_epoch: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AutomarkerPassiveReadinessObservation {
    pub readiness: AutomarkerNativeReadinessEvidence,
    pub syn_ordinal: u64,
    pub syn_observed_micros: u64,
}

pub(crate) struct AutomarkerPassiveReadinessWorker {
    handle: Option<Arc<crate::automarker_windivert_backend::WinDivertHandle>>,
    cancel: Option<mpsc::Sender<()>>,
    result: mpsc::Receiver<Result<AutomarkerPassiveReadinessObservation, String>>,
    join: Option<JoinHandle<()>>,
    milestone: Arc<AtomicU8>,
    failure_category: Arc<AtomicU8>,
}

impl AutomarkerPassiveReadinessWorker {
    /// Start a packet-free readiness worker for one exact parser-observed,
    /// already-established connection.
    pub(crate) fn spawn_established(
        process_id: u32,
        executable_name: &str,
        dependency_directory: &Path,
        capture: AutomarkerBridgeCaptureTcpConnection,
        connection_epoch: u64,
    ) -> Result<Self, String> {
        if process_id == 0
            || executable_name.trim().is_empty()
            || capture.capture_connection_id == 0
            || connection_epoch == 0
        {
            return Err("established readiness requires an exact process and parser tuple".into());
        }
        let (sender, result) = mpsc::sync_channel(1);
        let milestone = Arc::new(AtomicU8::new(0));
        let failure_category = Arc::new(AtomicU8::new(0));
        let worker_milestone = Arc::clone(&milestone);
        let worker_failure_category = Arc::clone(&failure_category);
        let executable_name = executable_name.to_owned();
        let dependency_directory = dependency_directory.to_path_buf();
        let join = thread::Builder::new()
            .name("rlogs-automarker-established-readiness".into())
            .spawn(move || {
                let request = AutomarkerEstablishedConnectionReadinessRequest {
                    process: AutomarkerReviewedGameProcessIdentity {
                        process_id,
                        executable_name: &executable_name,
                    },
                    dependency_directory: &dependency_directory,
                    capture,
                    connection_epoch,
                };
                let outcome =
                    discover_established_connection_readiness(&request).map(|readiness| {
                        AutomarkerPassiveReadinessObservation {
                            readiness,
                            // Explicit EstablishedSnapshot provenance means these
                            // are deliberately not fabricated SYN observations.
                            syn_ordinal: 0,
                            syn_observed_micros: 0,
                        }
                    });
                match &outcome {
                    Ok(_) => worker_milestone.store(1, Ordering::Release),
                    Err(error) => {
                        worker_failure_category
                            .store(sanitized_failure_category(error).code(), Ordering::Release);
                        worker_milestone.store(2, Ordering::Release);
                    }
                }
                let _ = sender.send(outcome);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            handle: None,
            cancel: None,
            result,
            join: Some(join),
            milestone,
            failure_category,
        })
    }

    /// Start a SNIFF|RECV_ONLY worker. Received packets are inspected only
    /// long enough to identify an exact process-owned outbound SYN and are
    /// never retained in the worker result.
    pub(crate) fn spawn(
        process_id: u32,
        executable_name: &str,
        dependency_directory: &Path,
        connection_epoch: u64,
    ) -> Result<Self, String> {
        if process_id == 0 || executable_name.trim().is_empty() || connection_epoch == 0 {
            return Err("passive readiness requires a process and connection epoch".into());
        }
        let (sender, result) = mpsc::sync_channel(1);
        let (cancel, cancelled) = mpsc::channel();
        let milestone = Arc::new(AtomicU8::new(0));
        let failure_category = Arc::new(AtomicU8::new(0));
        let worker_milestone = Arc::clone(&milestone);
        let worker_failure_category = Arc::clone(&failure_category);
        let executable_name = executable_name.to_owned();
        let dependency_directory = dependency_directory.to_path_buf();
        let join = thread::Builder::new()
            .name("rlogs-automarker-readiness-setup".into())
            .spawn(move || {
                let publish = |outcome: Result<AutomarkerPassiveReadinessObservation, String>| {
                    match &outcome {
                        Ok(_) => worker_milestone.store(1, Ordering::Release),
                        Err(error) => worker_failure_category
                            .store(sanitized_failure_category(error).code(), Ordering::Release),
                    }
                    if worker_milestone.load(Ordering::Acquire) == 0 {
                        worker_milestone.store(2, Ordering::Release);
                    }
                    let _ = sender.send(outcome);
                };
                let inner = match Self::spawn_blocking(
                    process_id,
                    &executable_name,
                    &dependency_directory,
                    connection_epoch,
                ) {
                    Ok(inner) => inner,
                    Err(error) => {
                        publish(Err(error));
                        return;
                    }
                };
                loop {
                    if cancelled.try_recv().is_ok() {
                        inner.stop_drain_join();
                        return;
                    }
                    if let Some(outcome) = inner.try_take_result() {
                        inner.stop_drain_join();
                        publish(outcome);
                        return;
                    }
                    thread::sleep(std::time::Duration::from_millis(5));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            handle: None,
            cancel: Some(cancel),
            result,
            join: Some(join),
            milestone,
            failure_category,
        })
    }

    fn spawn_blocking(
        process_id: u32,
        executable_name: &str,
        dependency_directory: &Path,
        connection_epoch: u64,
    ) -> Result<Self, String> {
        if process_id == 0 || executable_name.trim().is_empty() || connection_epoch == 0 {
            return Err("passive readiness requires a process and connection epoch".into());
        }
        if !is_elevated()? {
            return Err("Automarker native readiness requires an elevated rLogs process".into());
        }
        // Verify pinned bytes/signature before loading the backend used by the
        // passive handle.
        drop(PinnedWinDivertChecksumHelper::load(dependency_directory)?);
        let dll = dependency_directory.join("WinDivert.dll");
        let backend =
            unsafe { PinnedWinDivertBackend::load(&dll) }.map_err(|error| error.to_string())?;
        const FILTER: &str = "outbound and ip and tcp and tcp.Syn and !tcp.Ack";
        backend
            .compile_network_filter(FILTER)
            .map_err(|error| error.to_string())?;
        // WinDivert's REFLECT arbitration opens with NO_INSTALL, so establish
        // the reviewed driver first through a false-filter, read-only handle.
        // The temporary handle is dropped before arbitration and cannot
        // receive, divert, mutate, or send a game packet.
        let driver_start = backend
            .open_network("false", 0, PASSIVE_READINESS_FLAGS)
            .map_err(|error| error.to_string())?;
        if driver_start.get_param(PARAM_VERSION_MAJOR)? != 2
            || driver_start.get_param(PARAM_VERSION_MINOR)? != 2
        {
            return Err("loaded WinDivert driver is not version 2.2".into());
        }
        let observer = backend
            .open_arbitrated_network(FILTER, 1, PASSIVE_READINESS_FLAGS)
            .map_err(|error| error.to_string());
        // Keep the provisioning handle alive until REFLECT arbitration and
        // the retained observer open have both finished, then close it on
        // every outcome before handling the result.
        drop(driver_start);
        let handle = match observer? {
            ArbitratedNetworkOpen::Open(handle) => Arc::new(handle),
            ArbitratedNetworkOpen::Conflict => {
                return Err("a same-priority passive WinDivert handle is already open".into());
            }
            ArbitratedNetworkOpen::RejectedAfterOpen(handle, reason) => {
                if let Err(failure) = handle.drain_reinject_and_close(65_535) {
                    let detail = failure.message().to_owned();
                    failure.retain_forever();
                    return Err(format!("{reason}; drain ownership retained: {detail}"));
                }
                return Err(reason);
            }
        };
        if handle.get_param(PARAM_VERSION_MAJOR)? != 2
            || handle.get_param(PARAM_VERSION_MINOR)? != 2
        {
            return Err("loaded WinDivert driver is not version 2.2".into());
        }

        let (sender, result) = mpsc::sync_channel(1);
        let milestone = Arc::new(AtomicU8::new(0));
        let failure_category = Arc::new(AtomicU8::new(0));
        let worker_milestone = Arc::clone(&milestone);
        let worker_failure_category = Arc::clone(&failure_category);
        let worker_handle = Arc::clone(&handle);
        let dependency_directory = dependency_directory.to_path_buf();
        let executable_name = executable_name.to_owned();
        let join = thread::Builder::new()
            .name("rlogs-automarker-passive-readiness".into())
            .spawn(move || {
                let publish = |outcome: Result<AutomarkerPassiveReadinessObservation, String>| {
                    match &outcome {
                        Ok(_) => worker_milestone.store(1, Ordering::Release),
                        Err(error) => {
                            worker_failure_category
                                .store(sanitized_failure_category(error).code(), Ordering::Release);
                            worker_milestone.store(2, Ordering::Release);
                        }
                    }
                    let _ = sender.send(outcome);
                };
                let owner = match WindowsProcessSocketOwner::new(process_id) {
                    Ok(owner) => owner,
                    Err(error) => {
                        publish(Err(error.to_string()));
                        return;
                    }
                };
                let started = Instant::now();
                let mut ordinal = 0_u64;
                loop {
                    let received = match worker_handle.receive(65_535) {
                        Ok(Some(received)) => received,
                        Ok(None) => {
                            publish(Err("passive readiness worker stopped before SYN".into()));
                            return;
                        }
                        Err(error) => {
                            publish(Err(error));
                            return;
                        }
                    };
                    let Some(next_ordinal) = ordinal.checked_add(1) else {
                        publish(Err("passive SYN ordinal exhausted".into()));
                        return;
                    };
                    ordinal = next_ordinal;
                    let Some(capture) = capture_from_outbound_syn(&received.0) else {
                        continue;
                    };
                    let owned = match process_owns_capture(&owner, capture) {
                        Ok(owned) => owned,
                        Err(error) => {
                            publish(Err(error));
                            return;
                        }
                    };
                    if !owned {
                        continue;
                    }
                    let micros = started
                        .elapsed()
                        .as_micros()
                        .max(1)
                        .min(u128::from(u64::MAX)) as u64;
                    let request = AutomarkerNativeReadinessRequest {
                        process: AutomarkerReviewedGameProcessIdentity {
                            process_id,
                            executable_name: &executable_name,
                        },
                        dependency_directory: &dependency_directory,
                        capture,
                        connection_epoch,
                        observed_syn_packet: &received.0,
                        syn_capture_sequence: ordinal,
                        syn_observed_micros: micros,
                    };
                    let outcome = discover_native_readiness(&request).map(|readiness| {
                        AutomarkerPassiveReadinessObservation {
                            readiness,
                            syn_ordinal: ordinal,
                            syn_observed_micros: micros,
                        }
                    });
                    publish(outcome);
                    return;
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            handle: Some(handle),
            cancel: None,
            result,
            join: Some(join),
            milestone,
            failure_category,
        })
    }

    pub(crate) fn sanitized_status(&self) -> AutomarkerPassiveReadinessStatus {
        match self.milestone.load(Ordering::Acquire) {
            1 => AutomarkerPassiveReadinessStatus::NativeReadinessProven,
            2 => AutomarkerPassiveReadinessStatus::Failed(
                AutomarkerNativeFailureCategory::from_code(
                    self.failure_category.load(Ordering::Acquire),
                )
                .unwrap_or(AutomarkerNativeFailureCategory::Internal),
            ),
            _ => AutomarkerPassiveReadinessStatus::WaitingForNewSyn,
        }
    }

    pub(crate) fn try_take_result(
        &self,
    ) -> Option<Result<AutomarkerPassiveReadinessObservation, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("passive readiness worker ended without evidence".into()))
            }
        }
    }

    pub(crate) fn stop_drain_join(mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.shutdown_receive();
        }
        while self.result.try_recv().is_ok() {}
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    pub(crate) fn stop_drain_join_async(self) {
        let _ = thread::Builder::new()
            .name("rlogs-automarker-readiness-reaper".into())
            .spawn(move || self.stop_drain_join());
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        result: Result<AutomarkerPassiveReadinessObservation, String>,
        joined: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let milestone = Arc::new(AtomicU8::new(if result.is_ok() { 1 } else { 2 }));
        let failure_category = Arc::new(AtomicU8::new(
            result
                .as_ref()
                .err()
                .map(|error| sanitized_failure_category(error).code())
                .unwrap_or(0),
        ));
        sender.send(result).expect("test result receiver exists");
        drop(sender);
        let join = thread::spawn(move || {
            joined.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        Self {
            handle: None,
            cancel: None,
            result: receiver,
            join: Some(join),
            milestone,
            failure_category,
        }
    }
}

impl Drop for AutomarkerPassiveReadinessWorker {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.shutdown_receive();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Validate one already-observed SYN candidate against local process ownership
/// and every static native dependency. No active NETWORK handle is retained.
pub(crate) fn discover_native_readiness(
    request: &AutomarkerNativeReadinessRequest<'_>,
) -> Result<AutomarkerNativeReadinessEvidence, String> {
    if request.connection_epoch == 0
        || request.syn_capture_sequence == 0
        || !is_exact_outbound_syn(request.observed_syn_packet, request.capture)
    {
        return Err("a nonzero SYN-observed connection epoch is required".into());
    }
    if !is_unique_supported_process(&request.process)? {
        return Err("the selected PID is not the unique manifest-supported game process".into());
    }
    if !is_elevated()? {
        return Err("Automarker native readiness requires an elevated rLogs process".into());
    }
    if !base_filtering_engine_running()? {
        return Err("Windows Base Filtering Engine is not reported RUNNING".into());
    }

    let connection = AutomarkerOwnedTcpConnection {
        process_id: request.process.process_id,
        local: AutomarkerIpv4Endpoint {
            address: request.capture.client_address,
            port: request.capture.client_port,
        },
        remote: AutomarkerIpv4Endpoint {
            address: request.capture.server_address,
            port: request.capture.server_port,
        },
    };
    let owner = WindowsProcessSocketOwner::new(request.process.process_id)
        .map_err(|error| error.to_string())?;
    let expected = TcpConnection::new(
        rlogs_capture::TcpEndpoint::new(
            IpAddr::V4(request.capture.client_address),
            request.capture.client_port,
        ),
        rlogs_capture::TcpEndpoint::new(
            IpAddr::V4(request.capture.server_address),
            request.capture.server_port,
        ),
    );
    let matches = owner
        .snapshot_all_connections()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|candidate| *candidate == expected)
        .count();
    let owned = vec![connection; matches];
    let binding = bind_offline_automarker_connection_epoch(
        request.process.process_id,
        connection,
        request.connection_epoch,
        true,
        &owned,
    )
    .map_err(|error| format!("exact SYN-owned tuple binding failed: {error:?}"))?;

    finish_native_readiness(binding, request.dependency_directory)
}

/// Prove native readiness from one parser-provided connection that is already
/// established. This performs only process/socket snapshots and static native
/// dependency probes. It never opens a traffic-matching NETWORK handle and it
/// never receives, diverts, mutates, or sends a game packet.
#[allow(dead_code)] // Integration is deliberately deferred to the native bridge.
pub(crate) fn discover_established_connection_readiness(
    request: &AutomarkerEstablishedConnectionReadinessRequest<'_>,
) -> Result<AutomarkerNativeReadinessEvidence, String> {
    if request.connection_epoch == 0 || request.capture.capture_connection_id == 0 {
        return Err("a parser connection and fresh nonzero connection epoch are required".into());
    }
    if !is_unique_supported_process(&request.process)? {
        return Err("the selected PID is not the unique manifest-supported game process".into());
    }
    if !is_elevated()? {
        return Err("Automarker native readiness requires an elevated rLogs process".into());
    }
    if !base_filtering_engine_running()? {
        return Err("Windows Base Filtering Engine is not reported RUNNING".into());
    }

    let connection = owned_connection(request.process.process_id, request.capture);
    let owner = WindowsProcessSocketOwner::new(request.process.process_id)
        .map_err(|error| error.to_string())?;
    let snapshot = owner
        .snapshot_established_connections()
        .map_err(|error| error.to_string())?;
    let binding = bind_exact_established_capture(
        request.process.process_id,
        request.capture,
        request.connection_epoch,
        &snapshot,
    )?;
    debug_assert_eq!(binding.connection(), connection);

    finish_native_readiness(binding, request.dependency_directory)
}

#[allow(dead_code)]
fn owned_connection(
    process_id: u32,
    capture: AutomarkerBridgeCaptureTcpConnection,
) -> AutomarkerOwnedTcpConnection {
    AutomarkerOwnedTcpConnection {
        process_id,
        local: AutomarkerIpv4Endpoint {
            address: capture.client_address,
            port: capture.client_port,
        },
        remote: AutomarkerIpv4Endpoint {
            address: capture.server_address,
            port: capture.server_port,
        },
    }
}

#[allow(dead_code)]
fn bind_exact_established_capture(
    process_id: u32,
    capture: AutomarkerBridgeCaptureTcpConnection,
    connection_epoch: u64,
    established_connections: &[TcpConnection],
) -> Result<OfflineAutomarkerConnectionEpochBinding, String> {
    if process_id == 0 || connection_epoch == 0 || capture.capture_connection_id == 0 {
        return Err("an exact parser tuple and nonzero process/epoch are required".into());
    }
    let connection = owned_connection(process_id, capture);
    let expected = TcpConnection::new(
        rlogs_capture::TcpEndpoint::new(IpAddr::V4(capture.client_address), capture.client_port),
        rlogs_capture::TcpEndpoint::new(IpAddr::V4(capture.server_address), capture.server_port),
    );
    let matches = established_connections
        .iter()
        .filter(|candidate| **candidate == expected)
        .count();
    let owned = vec![connection; matches];
    bind_offline_automarker_established_connection_epoch(
        process_id,
        connection,
        connection_epoch,
        &owned,
    )
    .map_err(|error| format!("exact established owned tuple binding failed: {error:?}"))
}

fn finish_native_readiness(
    binding: OfflineAutomarkerConnectionEpochBinding,
    dependency_directory: &Path,
) -> Result<AutomarkerNativeReadinessEvidence, String> {
    // This loader verifies the pinned DLL/driver hashes and driver signature,
    // then resolves only the checksum helper. It opens no driver handle.
    let checksum = PinnedWinDivertChecksumHelper::load(dependency_directory)?;
    let dll = dependency_directory.join("WinDivert.dll");
    let backend =
        unsafe { PinnedWinDivertBackend::load(&dll) }.map_err(|error| error.to_string())?;
    backend
        .compile_network_filter("false")
        .map_err(|error| error.to_string())?;
    let probe = match backend
        .open_arbitrated_network("false", 0, FLAG_SNIFF | FLAG_RECV_ONLY | FLAG_NO_INSTALL)
        .map_err(|error| error.to_string())?
    {
        ArbitratedNetworkOpen::Open(handle) => handle,
        ArbitratedNetworkOpen::Conflict => {
            return Err("a same-priority WinDivert NETWORK handle is already open".into());
        }
        ArbitratedNetworkOpen::RejectedAfterOpen(handle, reason) => {
            if let Err(failure) = handle.drain_reinject_and_close(65_535) {
                let detail = failure.message().to_owned();
                failure.retain_forever();
                return Err(format!("{reason}; drain ownership retained: {detail}"));
            }
            return Err(reason);
        }
    };
    if probe.get_param(PARAM_VERSION_MAJOR)? != 2 || probe.get_param(PARAM_VERSION_MINOR)? != 2 {
        return Err("loaded WinDivert driver is not version 2.2".into());
    }
    drop(probe);
    drop(checksum);

    Ok(AutomarkerNativeReadinessEvidence {
        binding,
        reflect_preflight_clear: true,
        pinned_backend_ready: true,
        checksum_helper_ready: true,
    })
}

fn is_exact_outbound_syn(packet: &[u8], capture: AutomarkerBridgeCaptureTcpConnection) -> bool {
    capture_from_outbound_syn(packet).is_some_and(|observed| {
        observed.client_address == capture.client_address
            && observed.client_port == capture.client_port
            && observed.server_address == capture.server_address
            && observed.server_port == capture.server_port
    })
}

fn capture_from_outbound_syn(packet: &[u8]) -> Option<AutomarkerBridgeCaptureTcpConnection> {
    if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
        return None;
    }
    let ip_header = usize::from(packet[0] & 0x0f).saturating_mul(4);
    let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if ip_header < 20
        || total != packet.len()
        || total < ip_header.saturating_add(20)
        || u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0
    {
        return None;
    }
    let tcp_header = usize::from(packet[ip_header + 12] >> 4).saturating_mul(4);
    if tcp_header < 20 || ip_header.saturating_add(tcp_header) > total {
        return None;
    }
    let flags = packet[ip_header + 13];
    let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let destination = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
    let source_port = u16::from_be_bytes([packet[ip_header], packet[ip_header + 1]]);
    let destination_port = u16::from_be_bytes([packet[ip_header + 2], packet[ip_header + 3]]);
    if flags & 0x02 == 0 || flags & 0x10 != 0 || source_port == 0 || destination_port == 0 {
        return None;
    }
    Some(AutomarkerBridgeCaptureTcpConnection {
        capture_connection_id: 0,
        client_address: source,
        client_port: source_port,
        server_address: destination,
        server_port: destination_port,
    })
}

fn process_owns_capture(
    owner: &WindowsProcessSocketOwner,
    capture: AutomarkerBridgeCaptureTcpConnection,
) -> Result<bool, String> {
    let expected = TcpConnection::new(
        rlogs_capture::TcpEndpoint::new(IpAddr::V4(capture.client_address), capture.client_port),
        rlogs_capture::TcpEndpoint::new(IpAddr::V4(capture.server_address), capture.server_port),
    );
    Ok(owner
        .snapshot_all_connections()
        .map_err(|error| error.to_string())?
        .iter()
        .filter(|candidate| **candidate == expected)
        .count()
        == 1)
}

struct OwnedKernelHandle(HANDLE);

impl Drop for OwnedKernelHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn is_elevated() -> Result<bool, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err("OpenProcessToken failed".into());
    }
    let token = OwnedKernelHandle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    Ok(ok != 0
        && returned as usize == size_of::<TOKEN_ELEVATION>()
        && elevation.TokenIsElevated != 0)
}

fn base_filtering_engine_running() -> Result<bool, String> {
    let output = Command::new("sc.exe")
        .args(["query", "BFE"])
        .output()
        .map_err(|error| error.to_string())?;
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).contains("RUNNING"))
}

#[allow(dead_code)]
fn is_unique_supported_process(
    selected: &AutomarkerReviewedGameProcessIdentity<'_>,
) -> Result<bool, String> {
    if selected.process_id == 0 || selected.executable_name.trim().is_empty() {
        return Ok(false);
    }
    let manifest = rlogs_game_bpsr::bundled_manifest()
        .map_err(|error| format!("BPSR manifest is invalid: {error}"))?;
    let supported = manifest
        .process_selector
        .ok_or("BPSR manifest has no process selector")?
        .windows_executable_names;
    let running = snapshot_supported_processes(&supported)?;
    Ok(selected_process_matches_snapshot(
        selected, &supported, &running,
    ))
}

#[allow(dead_code)]
fn selected_process_matches_snapshot(
    selected: &AutomarkerReviewedGameProcessIdentity<'_>,
    supported: &[String],
    running: &[(u32, String)],
) -> bool {
    supported
        .iter()
        .any(|name| name.eq_ignore_ascii_case(selected.executable_name))
        && running.len() == 1
        && running[0].0 == selected.process_id
        && running[0].1.eq_ignore_ascii_case(selected.executable_name)
}

#[allow(dead_code)]
fn snapshot_supported_processes(supported: &[String]) -> Result<Vec<(u32, String)>, String> {
    // SAFETY: the snapshot handle is checked, iterated with a correctly sized
    // PROCESSENTRY32W, and closed exactly once before return.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let snapshot = OwnedKernelHandle(snapshot);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    let mut matches = Vec::new();
    while present {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = OsString::from_wide(&entry.szExeFile[..end])
            .to_string_lossy()
            .into_owned();
        if supported
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(&name))
        {
            matches.push((entry.th32ProcessID, name));
        }
        present = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn established_capture() -> AutomarkerBridgeCaptureTcpConnection {
        AutomarkerBridgeCaptureTcpConnection {
            capture_connection_id: 7,
            client_address: Ipv4Addr::new(10, 0, 0, 2),
            client_port: 40_000,
            server_address: Ipv4Addr::new(10, 0, 0, 3),
            server_port: 443,
        }
    }

    fn capture_tcp_connection(capture: AutomarkerBridgeCaptureTcpConnection) -> TcpConnection {
        TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(capture.client_address),
                capture.client_port,
            ),
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(capture.server_address),
                capture.server_port,
            ),
        )
    }

    #[test]
    fn passive_readiness_flags_are_read_only_and_allow_reviewed_driver_start() {
        assert_ne!(PASSIVE_READINESS_FLAGS & FLAG_SNIFF, 0);
        assert_ne!(PASSIVE_READINESS_FLAGS & FLAG_RECV_ONLY, 0);
        assert_eq!(PASSIVE_READINESS_FLAGS & FLAG_NO_INSTALL, 0);
    }

    #[test]
    fn passive_setup_and_shutdown_are_deferred_from_the_caller() {
        let started = Instant::now();
        let worker = AutomarkerPassiveReadinessWorker::spawn(
            42,
            "BPSR_STEAM.exe",
            Path::new("definitely-missing-readiness-dependencies"),
            1,
        )
        .expect("argument validation and thread creation are the only synchronous work");
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
        let stopping = Instant::now();
        worker.stop_drain_join_async();
        assert!(stopping.elapsed() < std::time::Duration::from_millis(250));
    }

    #[test]
    fn private_errors_collapse_to_bounded_operator_categories() {
        assert_eq!(
            sanitized_failure_category("the desktop host is not elevated"),
            AutomarkerNativeFailureCategory::NotElevated
        );
        assert_eq!(
            sanitized_failure_category("dependency hash mismatch at a private path"),
            AutomarkerNativeFailureCategory::DependencyHashMismatch
        );
        assert_eq!(
            sanitized_failure_category("Authenticode signature invalid"),
            AutomarkerNativeFailureCategory::DriverSignatureInvalid
        );
        assert_eq!(
            sanitized_failure_category("WinDivert driver open failed"),
            AutomarkerNativeFailureCategory::DriverOpenFailed
        );
        assert_eq!(
            sanitized_failure_category("same-priority handle conflict"),
            AutomarkerNativeFailureCategory::HandleConflict
        );
        assert_eq!(
            sanitized_failure_category("owned socket tuple unavailable"),
            AutomarkerNativeFailureCategory::ProcessOrSocket
        );
        assert_eq!(
            sanitized_failure_category("private implementation detail"),
            AutomarkerNativeFailureCategory::Internal
        );
    }

    #[test]
    fn completed_worker_exposes_only_a_non_consuming_sanitized_milestone() {
        let joined = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = AutomarkerPassiveReadinessWorker::completed_for_test(
            Err("dependency hash mismatch: C:\\private\\driver.dll".into()),
            joined,
        );
        assert_eq!(
            worker.sanitized_status(),
            AutomarkerPassiveReadinessStatus::Failed(
                AutomarkerNativeFailureCategory::DependencyHashMismatch
            )
        );
        assert!(worker.try_take_result().is_some());
        worker.stop_drain_join();
    }

    #[test]
    fn capture_tuple_maps_without_persisting_any_packet_bytes() {
        let capture = AutomarkerBridgeCaptureTcpConnection {
            capture_connection_id: 7,
            client_address: Ipv4Addr::new(127, 0, 0, 1),
            client_port: 40_000,
            server_address: Ipv4Addr::new(127, 0, 0, 1),
            server_port: 40_001,
        };
        let connection = AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: capture.client_address,
                port: capture.client_port,
            },
            remote: AutomarkerIpv4Endpoint {
                address: capture.server_address,
                port: capture.server_port,
            },
        };
        let binding =
            bind_offline_automarker_connection_epoch(42, connection, 9, true, &[connection])
                .expect("exact tuple");
        assert_eq!(binding.connection(), connection);
        assert_eq!(binding.connection_epoch(), 9);
    }

    #[test]
    fn established_fallback_binds_one_exact_parser_tuple_to_caller_epoch() {
        let capture = established_capture();
        let exact = capture_tcp_connection(capture);
        let unrelated = TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1),
            rlogs_capture::TcpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2),
        );
        let binding = bind_exact_established_capture(42, capture, 99, &[unrelated, exact])
            .expect("one exact established tuple");
        assert_eq!(binding.connection(), owned_connection(42, capture));
        assert_eq!(binding.connection_epoch(), 99);
        assert_eq!(
            binding.ownership_proof(),
            rlogs_game_bpsr::OfflineAutomarkerConnectionOwnershipProof::EstablishedSnapshot
        );
    }

    #[test]
    fn established_fallback_fails_closed_on_zero_or_multiple_tuple_matches() {
        let capture = established_capture();
        let exact = capture_tcp_connection(capture);
        assert!(bind_exact_established_capture(42, capture, 99, &[]).is_err());
        assert!(bind_exact_established_capture(42, capture, 99, &[exact, exact]).is_err());
    }

    #[test]
    fn established_fallback_rejects_invalid_pid_or_epoch() {
        let capture = established_capture();
        let exact = capture_tcp_connection(capture);
        assert!(bind_exact_established_capture(0, capture, 99, &[exact]).is_err());
        assert!(bind_exact_established_capture(42, capture, 0, &[exact]).is_err());
    }

    #[test]
    fn selected_process_must_be_the_only_manifest_supported_client() {
        let supported = vec!["BPSR_STEAM.exe".into(), "BPSR_EPIC.exe".into()];
        let selected = AutomarkerReviewedGameProcessIdentity {
            process_id: 42,
            executable_name: "bpsr_epic.exe",
        };
        assert!(selected_process_matches_snapshot(
            &selected,
            &supported,
            &[(42, "BPSR_EPIC.exe".into())]
        ));
        assert!(!selected_process_matches_snapshot(
            &selected,
            &supported,
            &[(7, "BPSR_EPIC.exe".into())]
        ));
        assert!(!selected_process_matches_snapshot(
            &selected,
            &supported,
            &[(42, "BPSR_STEAM.exe".into())]
        ));
        assert!(!selected_process_matches_snapshot(
            &selected,
            &supported,
            &[(42, "BPSR_EPIC.exe".into()), (7, "BPSR_STEAM.exe".into())]
        ));
    }

    #[test]
    fn selected_process_name_cannot_escape_manifest_authority() {
        let supported = vec!["BPSR_STEAM.exe".into(), "BPSR_EPIC.exe".into()];
        let selected = AutomarkerReviewedGameProcessIdentity {
            process_id: 42,
            executable_name: "invented-client.exe",
        };
        assert!(!selected_process_matches_snapshot(
            &selected,
            &supported,
            &[(42, "invented-client.exe".into())]
        ));
    }

    #[test]
    fn syn_candidate_requires_exact_outbound_tuple_and_flags() {
        let capture = AutomarkerBridgeCaptureTcpConnection {
            capture_connection_id: 7,
            client_address: Ipv4Addr::new(10, 0, 0, 2),
            client_port: 40_000,
            server_address: Ipv4Addr::new(10, 0, 0, 3),
            server_port: 443,
        };
        let mut packet = vec![0u8; 40];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&40_u16.to_be_bytes());
        packet[9] = 6;
        packet[12..16].copy_from_slice(&capture.client_address.octets());
        packet[16..20].copy_from_slice(&capture.server_address.octets());
        packet[20..22].copy_from_slice(&capture.client_port.to_be_bytes());
        packet[22..24].copy_from_slice(&capture.server_port.to_be_bytes());
        packet[32] = 0x50;
        packet[33] = 0x02;
        assert!(is_exact_outbound_syn(&packet, capture));
        packet[33] |= 0x10;
        assert!(!is_exact_outbound_syn(&packet, capture));
    }
}
