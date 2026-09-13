import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const sourceUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/tools/automarker-windivert-passthrough.rs",
  import.meta.url,
);
const backendUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/src/automarker_windivert_backend.rs",
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
const backend = await readFile(fileURLToPath(backendUrl), "utf8");
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
assert.ok(source.includes('#[path = "../src/automarker_windivert_backend.rs"]'));
assert.ok(source.indexOf("if !args.armed") < source.indexOf("PinnedWinDivertBackend::load(&dll)"));
assert.ok(source.includes("same_priority_reflect_arbitration: false"));
assert.ok(source.includes('"blocked_same_priority_network_handle"'));
assert.equal(
  (source.match(/\.open_arbitrated_network\(/g) ?? []).length,
  2,
  "both guarded opens must use the shared backend",
);
assert.equal(
  (source.match(/ArbitratedNetworkOpen::Open\(handle\)/g) ?? []).length,
  2,
  "both discovery and active NETWORK opens require a fresh REFLECT proof",
);
const firstReflectProof = source.indexOf("let discovery_handle = match loaded.open_arbitrated_network(");
const discoveryOpen = source.indexOf("let connection = discover_exact_bpsr_syn_epoch(");
const secondReflectProof = source.indexOf(
  "let active = match loaded.open_arbitrated_network(",
  firstReflectProof + 1,
);
const activeOpen = source.indexOf("gates.exact_filter_opened = true");
assert.ok(firstReflectProof > source.indexOf("PinnedWinDivertBackend::load(&dll)"));
assert.ok(firstReflectProof < discoveryOpen);
assert.ok(secondReflectProof > discoveryOpen);
assert.ok(secondReflectProof < activeOpen);
assert.ok(backend.includes("static ARBITRATION_LOCK: Mutex<()>"));
const arbitrationLock = backend.indexOf(".lock()", backend.indexOf("open_arbitrated_network"));
const reflectOpen = backend.indexOf(
  'self.open_at_layer("true", LAYER_REFLECT_ABI, 0, flags_reflect)',
  arbitrationLock,
);
const preOpenBarrier = backend.indexOf("self.reflect_inventory_through_barrier(", reflectOpen);
const networkOpen = backend.indexOf("self.open_network(filter, priority, flags)?", preOpenBarrier);
const postOpenBarrier = backend.indexOf("self.reflect_post_open_barrier(", networkOpen);
const retainedMonitor = backend.indexOf(
  "active.lifetime_monitor = Some(ReflectLifetimeMonitor::spawn(reflect, lifetime)?)",
  postOpenBarrier,
);
assert.ok(arbitrationLock > 0);
assert.ok(arbitrationLock < reflectOpen);
assert.ok(reflectOpen < preOpenBarrier);
assert.ok(preOpenBarrier < networkOpen);
assert.ok(networkOpen < postOpenBarrier);
assert.ok(postOpenBarrier < retainedMonitor);
assert.ok(backend.includes("struct ReflectLifetimeMonitor"));
assert.ok(backend.includes('name("rlogs-automarker-reflect".into())'));
assert.ok(backend.includes("ActiveLifetimeArbitrationStatus::PeerConflict"));
assert.ok(backend.includes("ActiveLifetimeArbitrationStatus::MonitorFailure"));
assert.ok(backend.includes("drop(self.lifetime_monitor.take())"));
assert.ok(backend.includes("LAYER_REFLECT_ABI"));
assert.ok(backend.includes("SENTINEL_PRIORITY"));
assert.ok(backend.includes("decode_reflect_event(&address)"));
assert.ok(backend.includes("identity.priority == self.target_priority"));
assert.match(backend, /SHUTDOWN_RECV: i32 = 1/);
assert.match(backend, /FLAG_NO_INSTALL: u64 = 16/);
assert.match(
  source,
  /loaded\.open_arbitrated_network\(\s*&filter,\s*0,\s*WINDIVERT_FLAG_NO_INSTALL,?\s*\)/s,
);
assert.match(
  source,
  /loaded\.open_network\(\s*"false",\s*BOOTSTRAP_PRIORITY,\s*WINDIVERT_FLAG_SNIFF \| WINDIVERT_FLAG_RECV_ONLY,\s*\)/s,
);
assert.equal(
  (source.match(/WINDIVERT_FLAG_SNIFF \| WINDIVERT_FLAG_RECV_ONLY,\s*\)/g) ?? []).length,
  1,
  "exactly one explicitly armed false-filter bootstrap may omit NO_INSTALL",
);
assert.ok(source.indexOf("let bootstrap_handle = if args.bootstrap") < source.indexOf("drain_byte_identically(&mut backend)"));
assert.ok(source.indexOf("drop(bootstrap_handle)") > source.indexOf("drain_byte_identically(&mut backend)"));
assert.match(source, /shutdown_receive\(\)/);
assert.match(source, /drain_byte_identically\(&mut backend\)/);
assert.match(source, /send_unchanged\(received\.bytes\.as_slice\(\), &received\.address\)/);
assert.match(source, /if sent != received\.bytes\.len\(\)/);
assert.ok(backend.includes('close: export!("WinDivertClose", CloseFn)'));
assert.ok(backend.includes('compile_filter: export!("WinDivertHelperCompileFilter", CompileFilterFn)'));
assert.ok(source.indexOf("loaded.compile_network_filter(&filter)?") < activeOpen);
assert.ok(backend.includes("loaded: Arc<LoadedApi>"));
assert.ok(source.indexOf("let relay_result = drain_byte_identically(&mut backend)") < source.indexOf("let _ = stop_send.send(())"));
assert.ok(source.indexOf("let _ = stop_send.send(())") < source.indexOf("let relay = relay_result?"));
assert.match(source, /total != packet\.len\(\)/);
assert.ok(source.includes("let start_sequence = view.sequence.wrapping_add(1)"));
assert.ok(source.includes("DiscoveryPrefix::new(start_sequence)"));
assert.ok(source.includes("owned_connection_count(owner, connection)? == 1"));

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
