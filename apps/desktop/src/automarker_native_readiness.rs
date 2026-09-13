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
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Instant,
};

use rlogs_capture::{TcpConnection, WindowsProcessSocketOwner};
use rlogs_game_bpsr::{
    AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection, OfflineAutomarkerConnectionEpochBinding,
    PinnedWinDivertChecksumHelper, bind_offline_automarker_connection_epoch,
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

const GAME_PROCESS_NAME: &str = "BPSR_STEAM.exe";

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
    pub process_id: u32,
    pub dependency_directory: &'a Path,
    pub capture: AutomarkerBridgeCaptureTcpConnection,
    pub connection_epoch: u64,
    pub observed_syn_packet: &'a [u8],
    pub syn_capture_sequence: u64,
    pub syn_observed_micros: u64,
}

#[derive(Debug)]
pub(crate) struct AutomarkerPassiveReadinessObservation {
    pub readiness: AutomarkerNativeReadinessEvidence,
    pub syn_ordinal: u64,
    pub syn_observed_micros: u64,
}

pub(crate) struct AutomarkerPassiveReadinessWorker {
    handle: Option<Arc<crate::automarker_windivert_backend::WinDivertHandle>>,
    result: mpsc::Receiver<Result<AutomarkerPassiveReadinessObservation, String>>,
    join: Option<JoinHandle<()>>,
}

impl AutomarkerPassiveReadinessWorker {
    /// Start a SNIFF|RECV_ONLY worker. Received packets are inspected only
    /// long enough to identify an exact process-owned outbound SYN and are
    /// never retained in the worker result.
    pub(crate) fn spawn(
        process_id: u32,
        dependency_directory: &Path,
        connection_epoch: u64,
    ) -> Result<Self, String> {
        if process_id == 0 || connection_epoch == 0 {
            return Err("passive readiness requires a process and connection epoch".into());
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
        let handle = match backend
            .open_arbitrated_network(FILTER, 1, FLAG_SNIFF | FLAG_RECV_ONLY | FLAG_NO_INSTALL)
            .map_err(|error| error.to_string())?
        {
            ArbitratedNetworkOpen::Open(handle) => Arc::new(handle),
            ArbitratedNetworkOpen::Conflict => {
                return Err("a same-priority passive WinDivert handle is already open".into());
            }
        };
        if handle.get_param(PARAM_VERSION_MAJOR)? != 2
            || handle.get_param(PARAM_VERSION_MINOR)? != 2
        {
            return Err("loaded WinDivert driver is not version 2.2".into());
        }

        let (sender, result) = mpsc::sync_channel(1);
        let worker_handle = Arc::clone(&handle);
        let dependency_directory = dependency_directory.to_path_buf();
        let join = thread::Builder::new()
            .name("rlogs-automarker-passive-readiness".into())
            .spawn(move || {
                let owner = match WindowsProcessSocketOwner::new(process_id) {
                    Ok(owner) => owner,
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        return;
                    }
                };
                let started = Instant::now();
                let mut ordinal = 0_u64;
                loop {
                    let received = match worker_handle.receive(65_535) {
                        Ok(Some(received)) => received,
                        Ok(None) => return,
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            return;
                        }
                    };
                    let Some(next_ordinal) = ordinal.checked_add(1) else {
                        let _ = sender.send(Err("passive SYN ordinal exhausted".into()));
                        return;
                    };
                    ordinal = next_ordinal;
                    let Some(capture) = capture_from_outbound_syn(&received.0) else {
                        continue;
                    };
                    let owned = match process_owns_capture(&owner, capture) {
                        Ok(owned) => owned,
                        Err(error) => {
                            let _ = sender.send(Err(error));
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
                        process_id,
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
                    let _ = sender.send(outcome);
                    return;
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            handle: Some(handle),
            result,
            join: Some(join),
        })
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
        if let Some(handle) = self.handle.take() {
            let _ = handle.shutdown_receive();
        }
        while self.result.try_recv().is_ok() {}
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn completed_for_test(
        result: Result<AutomarkerPassiveReadinessObservation, String>,
        joined: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(result).expect("test result receiver exists");
        drop(sender);
        let join = thread::spawn(move || {
            joined.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        Self {
            handle: None,
            result: receiver,
            join: Some(join),
        }
    }
}

impl Drop for AutomarkerPassiveReadinessWorker {
    fn drop(&mut self) {
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
    if !is_exact_process(request.process_id, GAME_PROCESS_NAME)? {
        return Err("the selected PID is not the unique local BPSR_STEAM.exe process".into());
    }
    if !is_elevated()? {
        return Err("Automarker native readiness requires an elevated rLogs process".into());
    }
    if !base_filtering_engine_running()? {
        return Err("Windows Base Filtering Engine is not reported RUNNING".into());
    }

    let connection = AutomarkerOwnedTcpConnection {
        process_id: request.process_id,
        local: AutomarkerIpv4Endpoint {
            address: request.capture.client_address,
            port: request.capture.client_port,
        },
        remote: AutomarkerIpv4Endpoint {
            address: request.capture.server_address,
            port: request.capture.server_port,
        },
    };
    let owner =
        WindowsProcessSocketOwner::new(request.process_id).map_err(|error| error.to_string())?;
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
        request.process_id,
        connection,
        request.connection_epoch,
        true,
        &owned,
    )
    .map_err(|error| format!("exact SYN-owned tuple binding failed: {error:?}"))?;

    // This loader verifies the pinned DLL/driver hashes and driver signature,
    // then resolves only the checksum helper. It opens no driver handle.
    let checksum = PinnedWinDivertChecksumHelper::load(request.dependency_directory)?;
    let dll = request.dependency_directory.join("WinDivert.dll");
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

fn is_exact_process(process_id: u32, expected: &str) -> Result<bool, String> {
    if process_id == 0 {
        return Ok(false);
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("could not enumerate local processes".into());
    }
    let snapshot = OwnedKernelHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut matches = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while present {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = OsString::from_wide(&entry.szExeFile[..end]);
        if name.to_string_lossy().eq_ignore_ascii_case(expected) {
            matches.push(entry.th32ProcessID);
        }
        present = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(matches.as_slice() == [process_id])
}

#[cfg(test)]
mod tests {
    use super::*;

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
