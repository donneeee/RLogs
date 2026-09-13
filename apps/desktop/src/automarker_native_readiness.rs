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
    if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
        return false;
    }
    let ip_header = usize::from(packet[0] & 0x0f).saturating_mul(4);
    let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if ip_header < 20
        || total != packet.len()
        || total < ip_header.saturating_add(20)
        || u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0
    {
        return false;
    }
    let tcp_header = usize::from(packet[ip_header + 12] >> 4).saturating_mul(4);
    if tcp_header < 20 || ip_header.saturating_add(tcp_header) > total {
        return false;
    }
    let flags = packet[ip_header + 13];
    let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let destination = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
    let source_port = u16::from_be_bytes([packet[ip_header], packet[ip_header + 1]]);
    let destination_port = u16::from_be_bytes([packet[ip_header + 2], packet[ip_header + 3]]);
    flags & 0x02 != 0
        && flags & 0x10 == 0
        && source == capture.client_address
        && source_port == capture.client_port
        && destination == capture.server_address
        && destination_port == capture.server_port
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
