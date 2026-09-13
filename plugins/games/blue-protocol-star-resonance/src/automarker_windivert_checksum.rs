//! WinDivert 2.2.2 checksum boundary for the research-only automarker canary.
//!
//! This module cannot open a WinDivert handle, capture, or send. It only turns
//! an already-approved, changed full IPv4/TCP packet into a send-authorized
//! owned copy after the pinned official checksum helper succeeds and all ABI,
//! address, and checksum postconditions have been verified.

use std::mem::{align_of, offset_of, size_of};

const LAYER_MASK: u32 = 0x0000_00ff;
const EVENT_MASK: u32 = 0x0000_ff00;
const OUTBOUND_FLAG: u32 = 1 << 17;
const IPV6_FLAG: u32 = 1 << 20;
const IP_CHECKSUM_FLAG: u32 = 1 << 21;
const TCP_CHECKSUM_FLAG: u32 = 1 << 22;
const UDP_CHECKSUM_FLAG: u32 = 1 << 23;
const CHECKSUM_FLAGS: u32 = IP_CHECKSUM_FLAG | TCP_CHECKSUM_FLAG | UDP_CHECKSUM_FLAG;
const WINDIVERT_LAYER_NETWORK: u8 = 0;

/// Exact WinDivert 2.x `WINDIVERT_ADDRESS` x64 layout.
///
/// The final 64 bytes are the documented layer-specific union. Keeping them
/// opaque prevents this boundary from accidentally changing interface or
/// endpoint metadata while still giving the checksum helper the official ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerWinDivertAddress {
    pub timestamp: i64,
    flags: u32,
    reserved2: u32,
    layer_data: [u8; 64],
}

const _: () = {
    assert!(size_of::<AutomarkerWinDivertAddress>() == 80);
    assert!(align_of::<AutomarkerWinDivertAddress>() == 8);
    assert!(offset_of!(AutomarkerWinDivertAddress, timestamp) == 0);
    assert!(offset_of!(AutomarkerWinDivertAddress, flags) == 8);
    assert!(offset_of!(AutomarkerWinDivertAddress, reserved2) == 12);
    assert!(offset_of!(AutomarkerWinDivertAddress, layer_data) == 16);
};

impl AutomarkerWinDivertAddress {
    pub fn from_opaque_bytes(bytes: [u8; 80]) -> Self {
        // All bit patterns are valid for the integer/byte-only C layout.
        unsafe { std::mem::transmute(bytes) }
    }

    pub fn into_opaque_bytes(self) -> [u8; 80] {
        unsafe { std::mem::transmute(self) }
    }

    pub fn layer(self) -> u8 {
        (self.flags & LAYER_MASK) as u8
    }

    pub fn is_outbound(self) -> bool {
        self.flags & OUTBOUND_FLAG != 0
    }

    pub fn event(self) -> u8 {
        ((self.flags & EVENT_MASK) >> 8) as u8
    }

    pub fn is_ipv6(self) -> bool {
        self.flags & IPV6_FLAG != 0
    }

    pub fn has_ip_checksum(self) -> bool {
        self.flags & IP_CHECKSUM_FLAG != 0
    }

    pub fn has_tcp_checksum(self) -> bool {
        self.flags & TCP_CHECKSUM_FLAG != 0
    }
}

pub trait AutomarkerWinDivertChecksumHelper {
    /// Must have the exact ABI and behavior of WinDivert 2.2.2
    /// `WinDivertHelperCalcChecksums(packet, len, address, 0)`.
    fn calculate_checksums(
        &self,
        packet: &mut [u8],
        address: &mut AutomarkerWinDivertAddress,
    ) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomarkerPacketSendPreparation {
    /// Byte-for-byte and address-for-address pass-through. The checksum helper
    /// is never invoked on this branch.
    Original {
        packet: Vec<u8>,
        address: AutomarkerWinDivertAddress,
    },
    /// An owned copy authorized for one send by the calling research bridge.
    ModifiedAuthorized {
        packet: Vec<u8>,
        address: AutomarkerWinDivertAddress,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerWinDivertChecksumError {
    AbiMismatch,
    NotNetworkLayer,
    NotNetworkPacketEvent,
    NotOutbound,
    NotIpv4,
    PacketUnchanged,
    PacketLengthChanged,
    InvalidIpv4TcpPacket,
    FragmentedIpv4Packet,
    HelperRejected,
    HelperChangedUnexpectedPacketBytes,
    HelperChangedUnexpectedAddressBytes,
    MissingIpChecksumFlag,
    MissingTcpChecksumFlag,
    InvalidIpv4Checksum,
    InvalidTcpChecksum,
}

/// Prepare either an exact original packet or an explicitly approved changed
/// packet. Passing `None` is the non-candidate path and is deliberately unable
/// to invoke `helper`.
pub fn prepare_automarker_ipv4_tcp_packet<H: AutomarkerWinDivertChecksumHelper>(
    original_packet: &[u8],
    original_address: &AutomarkerWinDivertAddress,
    approved_changed_packet: Option<&[u8]>,
    helper: &H,
) -> Result<AutomarkerPacketSendPreparation, AutomarkerWinDivertChecksumError> {
    if !abi_is_exact() {
        return Err(AutomarkerWinDivertChecksumError::AbiMismatch);
    }
    let Some(changed_packet) = approved_changed_packet else {
        return Ok(AutomarkerPacketSendPreparation::Original {
            packet: original_packet.to_vec(),
            address: *original_address,
        });
    };
    validate_network_outbound_ipv4(*original_address)?;
    if changed_packet.len() != original_packet.len() {
        return Err(AutomarkerWinDivertChecksumError::PacketLengthChanged);
    }
    if changed_packet == original_packet {
        return Err(AutomarkerWinDivertChecksumError::PacketUnchanged);
    }
    let (ip_checksum_offset, tcp_checksum_offset) = ipv4_tcp_checksum_offsets(changed_packet)?;

    let mut packet = changed_packet.to_vec();
    let before_helper = packet.clone();
    let mut address = *original_address;
    if !helper.calculate_checksums(&mut packet, &mut address) {
        return Err(AutomarkerWinDivertChecksumError::HelperRejected);
    }
    validate_network_outbound_ipv4(address)?;
    if address.flags & !CHECKSUM_FLAGS != original_address.flags & !CHECKSUM_FLAGS
        || address.layer_data != original_address.layer_data
        || address.timestamp != original_address.timestamp
        || address.reserved2 != original_address.reserved2
        || address.flags & UDP_CHECKSUM_FLAG != original_address.flags & UDP_CHECKSUM_FLAG
    {
        return Err(AutomarkerWinDivertChecksumError::HelperChangedUnexpectedAddressBytes);
    }
    if !address.has_ip_checksum() {
        return Err(AutomarkerWinDivertChecksumError::MissingIpChecksumFlag);
    }
    if !address.has_tcp_checksum() {
        return Err(AutomarkerWinDivertChecksumError::MissingTcpChecksumFlag);
    }
    for (index, (before, after)) in before_helper.iter().zip(packet.iter()).enumerate() {
        let checksum_byte = (ip_checksum_offset..ip_checksum_offset + 2).contains(&index)
            || (tcp_checksum_offset..tcp_checksum_offset + 2).contains(&index);
        if !checksum_byte && before != after {
            return Err(AutomarkerWinDivertChecksumError::HelperChangedUnexpectedPacketBytes);
        }
    }
    let ipv4_header_len = usize::from(packet[0] & 0x0f) * 4;
    if checksum16(&packet[..ipv4_header_len]) != 0 {
        return Err(AutomarkerWinDivertChecksumError::InvalidIpv4Checksum);
    }
    if !tcp_checksum_valid(&packet)? {
        return Err(AutomarkerWinDivertChecksumError::InvalidTcpChecksum);
    }
    Ok(AutomarkerPacketSendPreparation::ModifiedAuthorized { packet, address })
}

fn abi_is_exact() -> bool {
    size_of::<AutomarkerWinDivertAddress>() == 80
        && align_of::<AutomarkerWinDivertAddress>() == 8
        && offset_of!(AutomarkerWinDivertAddress, flags) == 8
        && offset_of!(AutomarkerWinDivertAddress, layer_data) == 16
}

fn validate_network_outbound_ipv4(
    address: AutomarkerWinDivertAddress,
) -> Result<(), AutomarkerWinDivertChecksumError> {
    if address.layer() != WINDIVERT_LAYER_NETWORK {
        return Err(AutomarkerWinDivertChecksumError::NotNetworkLayer);
    }
    if address.event() != 0 {
        return Err(AutomarkerWinDivertChecksumError::NotNetworkPacketEvent);
    }
    if !address.is_outbound() {
        return Err(AutomarkerWinDivertChecksumError::NotOutbound);
    }
    if address.is_ipv6() {
        return Err(AutomarkerWinDivertChecksumError::NotIpv4);
    }
    Ok(())
}

fn ipv4_tcp_checksum_offsets(
    packet: &[u8],
) -> Result<(usize, usize), AutomarkerWinDivertChecksumError> {
    if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
        return Err(AutomarkerWinDivertChecksumError::InvalidIpv4TcpPacket);
    }
    let header_len = usize::from(packet[0] & 0x0f) * 4;
    let total_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    let fragment = u16::from_be_bytes([packet[6], packet[7]]);
    if fragment & 0x3fff != 0 {
        return Err(AutomarkerWinDivertChecksumError::FragmentedIpv4Packet);
    }
    if header_len < 20
        || total_len != packet.len()
        || header_len + 20 > total_len
        || usize::from(packet[header_len + 12] >> 4) * 4 < 20
        || header_len + usize::from(packet[header_len + 12] >> 4) * 4 > total_len
    {
        return Err(AutomarkerWinDivertChecksumError::InvalidIpv4TcpPacket);
    }
    Ok((10, header_len + 16))
}

fn checksum16(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(2) {
        let word = u16::from_be_bytes([chunk[0], chunk.get(1).copied().unwrap_or(0)]);
        sum = sum.wrapping_add(u32::from(word));
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn tcp_checksum_valid(packet: &[u8]) -> Result<bool, AutomarkerWinDivertChecksumError> {
    let header_len = usize::from(packet[0] & 0x0f) * 4;
    let tcp = &packet[header_len..];
    let mut pseudo = Vec::with_capacity(12 + tcp.len());
    pseudo.extend_from_slice(&packet[12..20]);
    pseudo.extend_from_slice(&[0, 6]);
    pseudo.extend_from_slice(&(tcp.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(tcp);
    Ok(checksum16(&pseudo) == 0)
}

#[cfg(windows)]
mod windows_helper {
    use super::*;
    use crate::{
        AUTOMARKER_WINDIVERT_VERSION, AUTOMARKER_WINDIVERT_X64_DLL_SHA256,
        AUTOMARKER_WINDIVERT_X64_DRIVER_SHA256,
    };
    use sha2::{Digest, Sha256};
    use std::{ffi::c_void, fs, os::windows::ffi::OsStrExt, path::Path, process::Command};
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW},
    };

    const DLL_NAME: &str = "WinDivert.dll";
    const OFFICIAL_VERSION: &str = "2.2.2";
    const DRIVER_SIGNER_THUMBPRINT: &str = "043589F75FCE2795E7F2CC3E526D46784D5DDAB3";
    type CalcChecksumsFn =
        unsafe extern "system" fn(*mut c_void, u32, *mut AutomarkerWinDivertAddress, u64) -> u32;

    pub struct PinnedWinDivertChecksumHelper {
        module: HMODULE,
        calculate: CalcChecksumsFn,
    }

    impl PinnedWinDivertChecksumHelper {
        /// Loads only the helper DLL. This performs no handle open, capture,
        /// driver install, or network operation.
        pub fn load(official_directory: &Path) -> Result<Self, String> {
            let dll = official_directory.join(DLL_NAME);
            let driver = official_directory.join("WinDivert64.sys");
            if sha256(&dll)? != AUTOMARKER_WINDIVERT_X64_DLL_SHA256 {
                return Err("WinDivert.dll does not match the pinned official 2.2.2 hash".into());
            }
            if sha256(&driver)? != AUTOMARKER_WINDIVERT_X64_DRIVER_SHA256 {
                return Err("WinDivert64.sys does not match the pinned official 2.2.2 hash".into());
            }
            // The official DLL carries no Windows ProductVersion resource.
            // Its exact hash plus the reviewed release identity is the patch
            // version gate; a future live bridge must retain the existing
            // handle major/minor gate before capture is enabled.
            if AUTOMARKER_WINDIVERT_VERSION != OFFICIAL_VERSION {
                return Err("reviewed WinDivert release identity is not 2.2.2".into());
            }
            let signer = powershell_scalar(
                "$s=Get-AuthenticodeSignature -LiteralPath $env:RLOGS_WINDIVERT_VERIFY_DRIVER; if($s.Status -eq 'Valid' -and $s.SignerCertificate){[Console]::Write($s.SignerCertificate.Thumbprint)}",
                "RLOGS_WINDIVERT_VERIFY_DRIVER",
                &driver,
            )?;
            if !signer.trim().eq_ignore_ascii_case(DRIVER_SIGNER_THUMBPRINT) {
                return Err(
                    "WinDivert64.sys does not have the pinned valid official signature".into(),
                );
            }
            let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
            let module = unsafe {
                LoadLibraryExW(
                    wide.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
                )
            };
            if module.is_null() {
                return Err("failed to load the pinned WinDivert.dll".into());
            }
            let Some(symbol) = (unsafe {
                GetProcAddress(module, c"WinDivertHelperCalcChecksums".as_ptr().cast())
            }) else {
                unsafe { FreeLibrary(module) };
                return Err("pinned DLL lacks WinDivertHelperCalcChecksums".into());
            };
            let calculate = unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, CalcChecksumsFn>(symbol)
            };
            Ok(Self { module, calculate })
        }
    }

    impl AutomarkerWinDivertChecksumHelper for PinnedWinDivertChecksumHelper {
        fn calculate_checksums(
            &self,
            packet: &mut [u8],
            address: &mut AutomarkerWinDivertAddress,
        ) -> bool {
            unsafe {
                (self.calculate)(packet.as_mut_ptr().cast(), packet.len() as u32, address, 0) != 0
            }
        }
    }

    impl Drop for PinnedWinDivertChecksumHelper {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module) };
        }
    }

    fn sha256(path: &Path) -> Result<String, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(format!("{:X}", Sha256::digest(bytes)))
    }

    fn powershell_scalar(script: &str, variable: &str, path: &Path) -> Result<String, String> {
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env(variable, path)
            .output()
            .map_err(|error| format!("PowerShell verification failed: {error}"))?;
        if !output.status.success() {
            return Err("PowerShell verification returned failure".into());
        }
        String::from_utf8(output.stdout).map_err(|_| "PowerShell verification was not UTF-8".into())
    }
}

#[cfg(windows)]
pub use windows_helper::PinnedWinDivertChecksumHelper;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct MockHelper {
        calls: Cell<u32>,
        mode: MockMode,
    }

    enum MockMode {
        Valid,
        Reject,
        CorruptPayload,
        CorruptAddress,
        MissingFlags,
    }

    impl AutomarkerWinDivertChecksumHelper for MockHelper {
        fn calculate_checksums(
            &self,
            packet: &mut [u8],
            address: &mut AutomarkerWinDivertAddress,
        ) -> bool {
            self.calls.set(self.calls.get() + 1);
            if matches!(self.mode, MockMode::Reject) {
                return false;
            }
            let header_len = usize::from(packet[0] & 0x0f) * 4;
            packet[10..12].fill(0);
            let ip = checksum16(&packet[..header_len]).to_be_bytes();
            packet[10..12].copy_from_slice(&ip);
            packet[header_len + 16..header_len + 18].fill(0);
            let mut pseudo = Vec::new();
            pseudo.extend_from_slice(&packet[12..20]);
            pseudo.extend_from_slice(&[0, 6]);
            pseudo.extend_from_slice(&((packet.len() - header_len) as u16).to_be_bytes());
            pseudo.extend_from_slice(&packet[header_len..]);
            let tcp = checksum16(&pseudo).to_be_bytes();
            packet[header_len + 16..header_len + 18].copy_from_slice(&tcp);
            if !matches!(self.mode, MockMode::MissingFlags) {
                address.flags |= IP_CHECKSUM_FLAG | TCP_CHECKSUM_FLAG;
            }
            if matches!(self.mode, MockMode::CorruptPayload) {
                packet[packet.len() - 1] ^= 1;
            }
            if matches!(self.mode, MockMode::CorruptAddress) {
                address.layer_data[0] ^= 1;
            }
            true
        }
    }

    fn address() -> AutomarkerWinDivertAddress {
        AutomarkerWinDivertAddress {
            timestamp: 123,
            flags: OUTBOUND_FLAG,
            reserved2: 0,
            layer_data: [7; 64],
        }
    }

    fn packet() -> Vec<u8> {
        let mut packet = vec![0u8; 44];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(44u16).to_be_bytes());
        packet[8] = 64;
        packet[9] = 6;
        packet[12..16].copy_from_slice(&[10, 0, 0, 1]);
        packet[16..20].copy_from_slice(&[10, 0, 0, 2]);
        packet[20..22].copy_from_slice(&1234u16.to_be_bytes());
        packet[22..24].copy_from_slice(&4321u16.to_be_bytes());
        packet[32] = 0x50;
        packet[33] = 0x18;
        packet[34..36].copy_from_slice(&4096u16.to_be_bytes());
        packet[40..44].copy_from_slice(b"test");
        packet
    }

    fn helper(mode: MockMode) -> MockHelper {
        MockHelper {
            calls: Cell::new(0),
            mode,
        }
    }

    #[test]
    fn abi_and_flag_positions_are_exact() {
        assert!(abi_is_exact());
        let address = address();
        assert_eq!(address.into_opaque_bytes()[10] & 0x02, 0x02);
        assert_eq!(
            AutomarkerWinDivertAddress::from_opaque_bytes(address.into_opaque_bytes()),
            address
        );
    }

    #[test]
    fn non_candidate_is_preserved_without_calling_helper() {
        let packet = packet();
        let address = address();
        let helper = helper(MockMode::Reject);
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&packet, &address, None, &helper),
            Ok(AutomarkerPacketSendPreparation::Original { packet, address })
        );
        assert_eq!(helper.calls.get(), 0);
    }

    #[test]
    fn changed_packet_requires_helper_and_valid_postconditions() {
        let original = packet();
        let mut changed = original.clone();
        changed[43] = b'!';
        let address = address();
        let helper = helper(MockMode::Valid);
        let result =
            prepare_automarker_ipv4_tcp_packet(&original, &address, Some(&changed), &helper)
                .expect("valid helper authorizes send");
        let AutomarkerPacketSendPreparation::ModifiedAuthorized { packet, address } = result else {
            panic!("changed packet was not authorized");
        };
        assert_eq!(packet[43], b'!');
        assert!(address.has_ip_checksum());
        assert!(address.has_tcp_checksum());
        assert_eq!(helper.calls.get(), 1);
    }

    #[test]
    fn helper_failure_and_faults_fail_closed() {
        let original = packet();
        let mut changed = original.clone();
        changed[43] = b'!';
        let address = address();
        for (mode, expected) in [
            (
                MockMode::Reject,
                AutomarkerWinDivertChecksumError::HelperRejected,
            ),
            (
                MockMode::CorruptPayload,
                AutomarkerWinDivertChecksumError::HelperChangedUnexpectedPacketBytes,
            ),
            (
                MockMode::CorruptAddress,
                AutomarkerWinDivertChecksumError::HelperChangedUnexpectedAddressBytes,
            ),
            (
                MockMode::MissingFlags,
                AutomarkerWinDivertChecksumError::MissingIpChecksumFlag,
            ),
        ] {
            assert_eq!(
                prepare_automarker_ipv4_tcp_packet(
                    &original,
                    &address,
                    Some(&changed),
                    &helper(mode)
                ),
                Err(expected)
            );
        }
    }

    #[test]
    fn wrong_direction_layer_family_and_length_are_rejected_before_helper() {
        let original = packet();
        let mut changed = original.clone();
        changed[43] = b'!';
        let helper = helper(MockMode::Valid);
        let mut inbound = address();
        inbound.flags &= !OUTBOUND_FLAG;
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &inbound, Some(&changed), &helper),
            Err(AutomarkerWinDivertChecksumError::NotOutbound)
        );
        let mut ipv6 = address();
        ipv6.flags |= IPV6_FLAG;
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &ipv6, Some(&changed), &helper),
            Err(AutomarkerWinDivertChecksumError::NotIpv4)
        );
        let mut wrong_layer = address();
        wrong_layer.flags = (wrong_layer.flags & !LAYER_MASK) | 1;
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &wrong_layer, Some(&changed), &helper),
            Err(AutomarkerWinDivertChecksumError::NotNetworkLayer)
        );
        let mut wrong_event = address();
        wrong_event.flags |= 1 << 8;
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &wrong_event, Some(&changed), &helper),
            Err(AutomarkerWinDivertChecksumError::NotNetworkPacketEvent)
        );
        let mut fragmented = changed.clone();
        fragmented[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &address(), Some(&fragmented), &helper),
            Err(AutomarkerWinDivertChecksumError::FragmentedIpv4Packet)
        );
        changed.push(0);
        assert_eq!(
            prepare_automarker_ipv4_tcp_packet(&original, &address(), Some(&changed), &helper),
            Err(AutomarkerWinDivertChecksumError::PacketLengthChanged)
        );
        assert_eq!(helper.calls.get(), 0);
    }
}
