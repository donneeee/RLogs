import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const sourceUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/tools/automarker-windivert-passthrough.rs",
  import.meta.url,
);
const launcherUrl = new URL(
  "./windows/run-bpsr-automarker-windivert-passthrough.ps1",
  import.meta.url,
);
const setupUrl = new URL(
  "./windows/setup-bpsr-automarker-windivert-driver.ps1",
  import.meta.url,
);
const packageUrl = new URL(
  "./windows/package-bpsr-automarker-windivert-passthrough.ps1",
  import.meta.url,
);
const source = await readFile(fileURLToPath(sourceUrl), "utf8");
const launcher = await readFile(fileURLToPath(launcherUrl), "utf8");
const setup = await readFile(fileURLToPath(setupUrl), "utf8");
const packaging = await readFile(fileURLToPath(packageUrl), "utf8");

const token = "RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1";
assert.ok(source.includes(`const ARM_TOKEN: &str = "${token}"`));
assert.ok(launcher.includes(token));
assert.ok(source.includes('const BOOTSTRAP_TOKEN: &str = "RLOGS_WINDIVERT_DRIVER_BOOTSTRAP_V1"'));
assert.ok(setup.includes("[string]$Mode = 'DRY_RUN'"));
assert.ok(launcher.includes("'RLOGS_WINDIVERT_BOOTSTRAP_AND_BYTE_IDENTICAL_PASSTHROUGH_V1'"));
assert.ok(launcher.includes("'RLOGS_WINDIVERT_DRIVER_BOOTSTRAP_V1'"));
assert.ok(setup.includes("'RLOGS_WINDIVERT_DRIVER_REMOVE_V1'"));
assert.ok(packaging.includes("setup-bpsr-automarker-windivert-driver.ps1"));
assert.ok(source.indexOf("if !args.armed") < source.indexOf("load_pinned_api(&dll)"));
assert.match(source, /WINDIVERT_SHUTDOWN_RECV: i32 = 0x1/);
assert.match(source, /WINDIVERT_FLAG_NO_INSTALL: u64 = 16/);
assert.match(
  source,
  /open_handle\(&loaded\.api, &filter, 0, WINDIVERT_FLAG_NO_INSTALL\)/,
);
assert.match(
  source,
  /open_handle\(\s*&loaded\.api,\s*"false",\s*0,\s*WINDIVERT_FLAG_SNIFF \| WINDIVERT_FLAG_RECV_ONLY,\s*\)/s,
);
assert.equal(
  (source.match(/WINDIVERT_FLAG_SNIFF \| WINDIVERT_FLAG_RECV_ONLY,\s*\)/g) ?? []).length,
  1,
  "exactly one explicitly armed false-filter bootstrap may omit NO_INSTALL",
);
assert.ok(source.indexOf("let bootstrap_handle = if args.bootstrap") < source.indexOf("drain_byte_identically(&mut backend)"));
assert.ok(source.indexOf("drop(bootstrap_handle)") > source.indexOf("drain_byte_identically(&mut backend)"));
assert.match(source, /WINDIVERT_SHUTDOWN_RECV/);
assert.match(source, /drain_byte_identically\(&mut backend\)/);
assert.match(source, /send_unchanged\(received\.bytes\.as_slice\(\), &received\.address\)/);
assert.match(source, /if sent != received\.bytes\.len\(\)/);

for (const forbidden of [
  "copy_from_slice(",
  "WinDivertHelperCalcChecksums",
  "rewrite_copied_outbound_packet",
  "verify_offline_automarker_substitution",
  "OfflineAutomarkerTcpRewriteLedger",
]) {
  assert.equal(source.includes(forbidden), false, `live canary contains mutation path: ${forbidden}`);
}

for (const privacyInvariant of [
  "endpoints_persisted: false",
  "tcp_sequence_or_ack_persisted: false",
  "payloads_persisted: false",
  "protocol_or_account_ids_persisted: false",
  "substitution_enabled: false",
  "checksum_rewrite_enabled: false",
]) {
  assert.ok(source.includes(privacyInvariant), `missing ${privacyInvariant}`);
}

assert.match(launcher, /Mode = 'DRY_RUN'/);
assert.match(launcher, /if \(\$Mode -eq 'DRY_RUN'\)/);
assert.match(launcher, /-Verb RunAs/);
assert.match(setup, /Mode = 'DRY_RUN'/);
assert.match(setup, /Get-Service -Name 'WinDivert'/);
const dryRunBranch = setup.slice(setup.indexOf("if ($Mode -eq 'DRY_RUN')"), setup.indexOf("if (-not (Test-Administrator))"));
for (const forbidden of ["& $ExecutablePath", "Start-Process", "New-Item", "sc.exe stop", "sc.exe delete"]) {
  assert.equal(dryRunBranch.includes(forbidden), false, `setup dry-run mutates state through ${forbidden}`);
}
console.log("validated opt-in byte-identical Automarker WinDivert pass-through boundary");
