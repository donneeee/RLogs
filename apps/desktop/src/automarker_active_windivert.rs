//! Production WinDivert transport for the active Automarker worker.
//!
//! This adapter is intentionally not wired into the desktop lifecycle yet.
//! Construction requires an already-proven connection epoch and opens only
//! the reviewed exact-tuple bidirectional filter. Every receive is overlapped
//! and bounded; an ordinary timeout/control wake never shuts down the handle.

#![allow(dead_code)]

use std::{path::Path, sync::Arc, time::Duration};

use rlogs_game_bpsr::{
    AUTOMARKER_WINDIVERT_ACTIVE_NETWORK_POLICY, AutomarkerActiveFilterPlan,
    AutomarkerActivePacketRole, AutomarkerPacketSendPreparation, AutomarkerWinDivertAddress,
    OfflineAutomarkerConnectionEpochBinding, PinnedWinDivertChecksumHelper,
    prepare_automarker_ipv4_tcp_packet, reviewed_automarker_active_filter_plan,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{CreateEventW, ResetEvent, SetEvent},
};

use crate::{
    automarker_active_worker::{
        ActiveAutomarkerBackend, ActiveAutomarkerPacket, ActiveAutomarkerWake,
    },
    automarker_windivert_backend::{
        ArbitratedNetworkOpen, FLAG_NO_INSTALL, OverlappedReceive, OverlappedReceiveWait,
        PARAM_VERSION_MAJOR, PARAM_VERSION_MINOR, PinnedWinDivertBackend, ReceiveControlEvent,
        WinDivertAddress, WinDivertHandle,
    },
};

const MAXIMUM_PACKET_BYTES: usize = 65_535;
const RECEIVE_WAIT: Duration = Duration::from_millis(75);

/// Cloneable wake signal for a future lifecycle owner. Signalling it only
/// wakes the outstanding receive wait; it does not cancel I/O or close the
/// WinDivert handle.
#[derive(Clone)]
pub(crate) struct ActiveAutomarkerReceiveControl(Arc<OwnedEvent>);

struct OwnedEvent(HANDLE);

unsafe impl Send for OwnedEvent {}
unsafe impl Sync for OwnedEvent {}

impl Drop for OwnedEvent {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

impl ActiveAutomarkerReceiveControl {
    fn create() -> Result<Self, String> {
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err("failed to create Automarker receive control event".into());
        }
        Ok(Self(Arc::new(OwnedEvent(event))))
    }

    pub(crate) fn wake(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.0.0) } == 0 {
            return Err("failed to signal Automarker receive control event".into());
        }
        Ok(())
    }

    fn borrowed_event(&self) -> Result<ReceiveControlEvent, String> {
        unsafe { ReceiveControlEvent::from_raw(self.0.0) }.map_err(str::to_owned)
    }

    fn reset(&self) -> Result<(), String> {
        if unsafe { ResetEvent(self.0.0) } == 0 {
            return Err("failed to reset Automarker receive control event".into());
        }
        Ok(())
    }
}

/// Owns the only active NETWORK handle, exact filter plan, and pinned checksum
/// helper used by one active Automarker worker.
pub(crate) struct WindowsActiveAutomarkerBackend {
    handle: Option<Arc<WinDivertHandle>>,
    pending_receive: Option<OverlappedReceive<'static>>,
    checksum: PinnedWinDivertChecksumHelper,
    filter_plan: AutomarkerActiveFilterPlan,
    receive_control: ActiveAutomarkerReceiveControl,
}

// The backend is constructed before the worker spawn and then exclusively
// owned and used by that one worker thread. Its DLL function table and native
// handle have process lifetime validity, and no field is concurrently used
// except the independently synchronized control event.
unsafe impl Send for WindowsActiveAutomarkerBackend {}

impl WindowsActiveAutomarkerBackend {
    /// Open a production transport for exactly one already-proven connection
    /// epoch. Driver installation and all activation/readiness policy remain
    /// outside this adapter.
    pub(crate) fn open(
        dependency_directory: &Path,
        binding: OfflineAutomarkerConnectionEpochBinding,
    ) -> Result<(Self, ActiveAutomarkerReceiveControl), String> {
        let policy = AUTOMARKER_WINDIVERT_ACTIVE_NETWORK_POLICY;
        if policy.priority != 0 || policy.sniff || policy.recv_only || !policy.no_install {
            return Err("reviewed active WinDivert policy changed unexpectedly".into());
        }

        let filter_plan = reviewed_automarker_active_filter_plan(binding);
        let checksum = PinnedWinDivertChecksumHelper::load(dependency_directory)?;
        let receive_control = ActiveAutomarkerReceiveControl::create()?;
        let dll = dependency_directory.join("WinDivert.dll");
        let driver = unsafe { PinnedWinDivertBackend::load(&dll) }
            .map_err(|error| format!("failed to load pinned WinDivert backend: {error}"))?;
        driver
            .compile_network_filter(filter_plan.expression())
            .map_err(|error| {
                format!("reviewed active WinDivert filter did not compile: {error}")
            })?;
        let handle = match driver
            .open_arbitrated_network(filter_plan.expression(), policy.priority, FLAG_NO_INSTALL)
            .map_err(|error| format!("failed to open active WinDivert handle: {error}"))?
        {
            ArbitratedNetworkOpen::Open(handle) => handle,
            ArbitratedNetworkOpen::Conflict => {
                return Err("a same-priority WinDivert NETWORK handle is already open".into());
            }
        };
        let version = handle.get_param(PARAM_VERSION_MAJOR).and_then(|major| {
            handle
                .get_param(PARAM_VERSION_MINOR)
                .map(|minor| (major, minor))
        });
        let version_error = match version {
            Ok((2, 2)) => None,
            Ok(_) => Some("active Automarker requires WinDivert driver 2.2".to_owned()),
            Err(error) => Some(error),
        };
        if let Some(error) = version_error {
            return match handle.try_close() {
                Ok(()) => Err(error),
                Err((_handle, close_error)) => Err(format!("{error}; additionally, {close_error}")),
            };
        }
        Ok((
            Self {
                handle: Some(Arc::new(handle)),
                pending_receive: None,
                checksum,
                filter_plan,
                receive_control: receive_control.clone(),
            },
            receive_control,
        ))
    }

    fn handle(&self) -> Result<&Arc<WinDivertHandle>, String> {
        self.handle
            .as_ref()
            .ok_or_else(|| "active WinDivert interception is already closed".into())
    }

    fn receive_checked(&mut self) -> Result<ActiveAutomarkerWake, String> {
        let control = self.receive_control.borrowed_event()?;
        let mut receive = match self.pending_receive.take() {
            Some(receive) => receive,
            None => self
                .handle()?
                .receive_overlapped_owned(MAXIMUM_PACKET_BYTES)?,
        };
        let wait = match receive.wait(RECEIVE_WAIT, Some(control)) {
            Ok(wait) => wait,
            Err(error) => {
                // Keep the I/O storage and handle reference alive. A
                // caller receiving this error may retain the backend and
                // retry rather than losing indeterminate receive state.
                self.pending_receive = Some(receive);
                return Err(error);
            }
        };
        match wait {
            OverlappedReceiveWait::TimedOut => {
                self.pending_receive = Some(receive);
                return Ok(ActiveAutomarkerWake::Timeout);
            }
            OverlappedReceiveWait::ControlWoken => {
                self.receive_control.reset()?;
                self.pending_receive = Some(receive);
                return Ok(ActiveAutomarkerWake::Timeout);
            }
            OverlappedReceiveWait::Completed => {}
        }
        let (bytes, address) = receive
            .take_packet()?
            .ok_or_else(|| "completed WinDivert receive had no packet".to_owned())?;
        let address = to_public_address(address);
        if packet_matches_reviewed_filter(&self.filter_plan, &bytes, address) {
            return Ok(ActiveAutomarkerWake::Packet(ActiveAutomarkerPacket {
                bytes,
                address,
            }));
        }

        // A kernel-filter escape is not eligible for coordinator
        // classification or mutation. Transfer its exact owned bytes to
        // the worker's explicit pass-through branch.
        Ok(ActiveAutomarkerWake::PassThroughOnly(
            ActiveAutomarkerPacket { bytes, address },
        ))
    }
}

impl ActiveAutomarkerBackend for WindowsActiveAutomarkerBackend {
    fn receive(&mut self) -> Result<ActiveAutomarkerWake, String> {
        self.receive_checked()
    }

    fn prepare_modified_send(
        &mut self,
        original: &ActiveAutomarkerPacket,
        approved_changed_bytes: &[u8],
    ) -> Result<AutomarkerPacketSendPreparation, String> {
        let inspection = self.filter_plan.inspect(&original.bytes).map_err(|error| {
            format!("modified carrier failed exact filter inspection: {error:?}")
        })?;
        if inspection.role != AutomarkerActivePacketRole::OutboundPayload
            || !address_matches_role(original.address, inspection.role)
        {
            return Err("modified carrier was not exact outbound active traffic".into());
        }
        prepare_automarker_ipv4_tcp_packet(
            &original.bytes,
            &original.address,
            Some(approved_changed_bytes),
            &self.checksum,
        )
        .map_err(|error| format!("modified carrier checksum preparation failed: {error:?}"))
    }

    fn send(&mut self, packet: &ActiveAutomarkerPacket) -> Result<usize, String> {
        self.handle()?
            .send_unchanged(&packet.bytes, &to_backend_address(packet.address))
    }

    fn close_interception(&mut self) -> Result<(), String> {
        // Cancels only this outstanding OVERLAPPED receive and synchronously
        // drains its completion before attempting handle closure.
        drop(self.pending_receive.take());
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        let handle = match Arc::try_unwrap(handle) {
            Ok(handle) => handle,
            Err(handle) => {
                self.handle = Some(handle);
                return Err("active WinDivert handle still has an outstanding owner".into());
            }
        };
        match handle.try_close() {
            Ok(()) => Ok(()),
            Err((handle, error)) => {
                // Retain the still-live handle so fatal ownership can retry.
                self.handle = Some(Arc::new(handle));
                Err(error)
            }
        }
    }
}

fn packet_matches_reviewed_filter(
    plan: &AutomarkerActiveFilterPlan,
    bytes: &[u8],
    address: AutomarkerWinDivertAddress,
) -> bool {
    if address.layer() != 0 || address.event() != 0 || address.is_ipv6() {
        return false;
    }
    plan.inspect(bytes)
        .is_ok_and(|inspection| address_matches_role(address, inspection.role))
}

fn address_matches_role(
    address: AutomarkerWinDivertAddress,
    role: AutomarkerActivePacketRole,
) -> bool {
    address.is_outbound() == (role == AutomarkerActivePacketRole::OutboundPayload)
}

fn to_public_address(address: WinDivertAddress) -> AutomarkerWinDivertAddress {
    // Both types assert the exact 80-byte WinDivert 2.x x64 ABI and consist
    // solely of integer/byte storage, so every bit pattern is valid.
    AutomarkerWinDivertAddress::from_opaque_bytes(unsafe {
        std::mem::transmute::<[u64; 10], [u8; 80]>(address.0)
    })
}

fn to_backend_address(address: AutomarkerWinDivertAddress) -> WinDivertAddress {
    // See `to_public_address`; this preserves all opaque metadata exactly.
    WinDivertAddress(unsafe {
        std::mem::transmute::<[u8; 80], [u64; 10]>(address.into_opaque_bytes())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    use rlogs_game_bpsr::{
        AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection,
        bind_offline_automarker_connection_epoch,
    };

    fn connection() -> AutomarkerOwnedTcpConnection {
        AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(203, 0, 113, 7),
                port: 44_321,
            },
        }
    }

    fn plan() -> AutomarkerActiveFilterPlan {
        let connection = connection();
        reviewed_automarker_active_filter_plan(
            bind_offline_automarker_connection_epoch(42, connection, 9, true, &[connection])
                .unwrap(),
        )
    }

    fn packet(outbound: bool) -> Vec<u8> {
        let connection = connection();
        let (source, destination) = if outbound {
            (connection.local, connection.remote)
        } else {
            (connection.remote, connection.local)
        };
        let mut packet = vec![0_u8; if outbound { 41 } else { 40 }];
        packet[0] = 0x45;
        let packet_len = packet.len() as u16;
        packet[2..4].copy_from_slice(&packet_len.to_be_bytes());
        packet[6..8].copy_from_slice(&0x4000_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 6;
        packet[12..16].copy_from_slice(&source.address.octets());
        packet[16..20].copy_from_slice(&destination.address.octets());
        packet[20..22].copy_from_slice(&source.port.to_be_bytes());
        packet[22..24].copy_from_slice(&destination.port.to_be_bytes());
        packet[32] = 0x50;
        packet[33] = 0x10;
        packet
    }

    fn address(outbound: bool) -> AutomarkerWinDivertAddress {
        let mut bytes = [0_u8; 80];
        let flags = if outbound { 1_u32 << 17 } else { 0 };
        bytes[8..12].copy_from_slice(&flags.to_ne_bytes());
        AutomarkerWinDivertAddress::from_opaque_bytes(bytes)
    }

    #[test]
    fn exact_roles_require_matching_windivert_direction() {
        assert!(packet_matches_reviewed_filter(
            &plan(),
            &packet(true),
            address(true)
        ));
        assert!(packet_matches_reviewed_filter(
            &plan(),
            &packet(false),
            address(false)
        ));
        assert!(!packet_matches_reviewed_filter(
            &plan(),
            &packet(true),
            address(false)
        ));
        assert!(!packet_matches_reviewed_filter(
            &plan(),
            &packet(false),
            address(true)
        ));
    }

    #[test]
    fn address_conversion_preserves_all_opaque_bytes() {
        let address = AutomarkerWinDivertAddress::from_opaque_bytes([0xa5; 80]);
        assert_eq!(to_public_address(to_backend_address(address)), address);
    }
}
