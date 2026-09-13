import fs from "node:fs";

const sourcePath =
  "plugins/games/blue-protocol-star-resonance/src/automarker_windivert_checksum.rs";
const source = fs.readFileSync(sourcePath, "utf8");
const required = [
  "#[repr(C)]",
  "size_of::<AutomarkerWinDivertAddress>() == 80",
  "offset_of!(AutomarkerWinDivertAddress, layer_data) == 16",
  "WinDivertHelperCalcChecksums",
  "AUTOMARKER_WINDIVERT_X64_DLL_SHA256",
  "AUTOMARKER_WINDIVERT_X64_DRIVER_SHA256",
  'const OFFICIAL_VERSION: &str = "2.2.2"',
  "AUTOMARKER_WINDIVERT_VERSION != OFFICIAL_VERSION",
  "AUTOMARKER_WINDIVERT_RELEASE_TAG_COMMIT != OFFICIAL_RELEASE_TAG_COMMIT",
  "DRIVER_SIGNER_THUMBPRINT",
  "approved_changed_packet: Option<&[u8]>",
  "HelperChangedUnexpectedPacketBytes",
  "HelperChangedUnexpectedAddressBytes",
  "MissingIpChecksumFlag",
  "MissingTcpChecksumFlag",
  "InvalidIpv4Checksum",
  "InvalidTcpChecksum",
  "FragmentedIpv4Packet",
  "non_candidate_is_preserved_without_calling_helper",
  "helper_failure_and_faults_fail_closed",
];

for (const needle of required) {
  if (!source.includes(needle)) {
    throw new Error(`missing checksum-boundary invariant: ${needle}`);
  }
}

const forbidden = [
  "WinDivertOpen",
  "WinDivertRecv",
  "WinDivertSend",
  "WriteProcessMemory",
];
for (const needle of forbidden) {
  if (source.includes(needle)) {
    throw new Error(`checksum boundary unexpectedly owns live capability: ${needle}`);
  }
}

console.log("BPSR automarker WinDivert checksum boundary invariants verified.");
