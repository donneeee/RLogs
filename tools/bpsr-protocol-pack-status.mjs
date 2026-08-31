#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  constants,
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const STATUS_SCHEMA_VERSION = 4;

if (command === "generate") generate(options);
else if (command === "promote") promote(options);
else if (command === "verify") verifyFile(resolvePath(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const candidateFile = resolvePath(required(options, "candidate"));
  const auditFile = resolvePath(required(options, "audit"));
  const reportsRoot = resolvePath(required(options, "reports-root"));
  const promotedFile = resolvePath(required(options, "promoted-pack"));
  const outputFile = resolvePath(required(options, "output"));
  const status = buildStatus({ build, candidateFile, auditFile, reportsRoot, promotedFile });
  mkdirSync(path.dirname(outputFile), { recursive: true });
  writeFileSync(outputFile, `${JSON.stringify(status, null, 2)}\n`, "utf8");
  verifyStatus(status);
  console.log(
    `Protocol status for build ${build}: ${status.status}; `
      + `${status.evidence.matching_build_report_count} reports, `
      + `${status.audit.validated_observable_migrated_decoder_route_count}/${status.audit.observable_migrated_decoder_route_count} observable migrated routes validated, `
      + `${status.audit.structural_non_obligation_route_count} structural non-obligations, `
      + `${status.blockers.length} blockers.`,
  );
}

function promote(options) {
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const candidateFile = resolvePath(required(options, "candidate"));
  const auditFile = resolvePath(required(options, "audit"));
  const reportsRoot = resolvePath(required(options, "reports-root"));
  const promotedFile = resolvePath(required(options, "promoted-pack"));
  const outputFile = resolvePath(required(options, "output"));
  const before = buildStatus({ build, candidateFile, auditFile, reportsRoot, promotedFile });
  if (before.status !== "promotion_ready_not_installed") {
    throw new Error(`Refusing protocol-pack install from status ${before.status}`);
  }
  mkdirSync(path.dirname(promotedFile), { recursive: true });
  copyFileSync(candidateFile, promotedFile, constants.COPYFILE_EXCL);
  const after = buildStatus({ build, candidateFile, auditFile, reportsRoot, promotedFile });
  if (after.status !== "promoted") {
    throw new Error(`Installed protocol pack did not verify as promoted: ${after.status}`);
  }
  mkdirSync(path.dirname(outputFile), { recursive: true });
  writeFileSync(outputFile, `${JSON.stringify(after, null, 2)}\n`, "utf8");
  verifyStatus(after);
  console.log(
    `Protocol pack installed for build ${build}: exact audited candidate bytes only; `
      + "canonical replay conservation and rDPS runtime promotion remain separate gates.",
  );
}

function buildStatus({ build, candidateFile, auditFile, reportsRoot, promotedFile }) {
  requireFile(candidateFile, "protocol-pack candidate");
  requireFile(auditFile, "protocol-pack promotion audit");
  if (!existsSync(reportsRoot) || !statSync(reportsRoot).isDirectory()) {
    throw new Error(`Missing protocol recording reports directory: ${reportsRoot}`);
  }
  const candidate = readJson(candidateFile, "protocol-pack candidate");
  const audit = readJson(auditFile, "protocol-pack promotion audit");
  const candidateBuild = String(candidate.target?.build_id ?? "");
  if (candidateBuild !== String(build)) {
    throw new Error(`Protocol candidate is build ${candidateBuild || "<missing>"}, expected ${build}`);
  }
  if (String(audit.build_id ?? "") !== String(build)) {
    throw new Error(`Protocol promotion audit is build ${audit.build_id ?? "<missing>"}, expected ${build}`);
  }
  if (audit.protocol_pack_id !== candidate.pack_id) {
    throw new Error(`Promotion audit pack ${audit.protocol_pack_id} does not match candidate ${candidate.pack_id}`);
  }
  if (![2, 3, 4, 5, 6].includes(Number(audit.schema_version))) {
    throw new Error(`Unsupported protocol promotion audit schema ${audit.schema_version}`);
  }

  const reportFiles = readdirSync(reportsRoot)
    .filter((name) => name.endsWith(".offline-recording-report.json"))
    .map((name) => path.join(reportsRoot, name))
    .sort();
  if (reportFiles.length === 0) throw new Error(`No offline recording reports found in ${reportsRoot}`);
  const reports = reportFiles.map((file) => ({ file, value: readJson(file, "offline recording report") }));
  const mismatchedReports = reports.filter(({ value }) =>
    value.protocol_pack_id !== candidate.pack_id
      || value.protocol_pack_digest !== audit.protocol_pack_digest,
  );
  if (mismatchedReports.length > 0) {
    throw new Error(`${mismatchedReports.length} recording reports do not match the audited candidate identity`);
  }
  const auditReportNames = new Set((audit.report_paths ?? []).map((file) => path.basename(file)));
  const reportNames = new Set(reportFiles.map((file) => path.basename(file)));
  const absentFromAudit = [...reportNames].filter((name) => !auditReportNames.has(name)).sort();
  const absentFromDisk = [...auditReportNames].filter((name) => !reportNames.has(name)).sort();
  if (absentFromAudit.length > 0 || absentFromDisk.length > 0) {
    throw new Error(
      `Promotion audit/report inventory mismatch: ${absentFromAudit.length} not audited, ${absentFromDisk.length} absent from disk`,
    );
  }

  const reportTotals = reports.reduce((totals, { value }) => {
    totals.packet_count += Number(value.capture?.packet_count ?? 0);
    totals.gap_count += Number(value.capture?.gap_count ?? 0);
    totals.known_packet_count += Number(value.capture?.known_packet_count ?? 0);
    totals.unknown_packet_count += Number(value.capture?.unknown_packet_count ?? 0);
    totals.decoded_records += (value.routes ?? []).reduce(
      (sum, route) => sum + Number(route.decode?.decoded_records ?? 0), 0,
    );
    return totals;
  }, { packet_count: 0, gap_count: 0, known_packet_count: 0, unknown_packet_count: 0, decoded_records: 0 });
  if (reportTotals.gap_count !== Number(audit.capture_gap_count ?? 0)) {
    throw new Error(`Capture-gap conservation failed: reports ${reportTotals.gap_count}, audit ${audit.capture_gap_count}`);
  }

  const promotedPresent = existsSync(promotedFile);
  const promoted = promotedPresent ? readJson(promotedFile, "promoted protocol pack") : null;
  const promotedBuild = promoted ? String(promoted.target?.build_id ?? "") : null;
  const promotedMatchesBuild = promoted ? promotedBuild === String(build) : false;
  const promotedMatchesCandidate = promoted
    ? promoted.pack_id === candidate.pack_id && sha256(promotedFile) === sha256(candidateFile)
    : false;
  const routeAudits = audit.route_audits ?? [];
  const unvalidatedRoutes = routeAudits
    .filter((route) => !route.validated &&
      (route.coverage_requirement ?? "matching_build_packet_evidence") ===
        "matching_build_packet_evidence")
    .map((route) => ({
      direction: route.route?.direction ?? null,
      fragment: route.route?.fragment?.kind ?? null,
      service_id: route.route?.service_id ?? null,
      method_id: route.route?.method_id ?? null,
      method_name: route.method_name ?? null,
      decoder: route.decoder ?? null,
      packet_count: Number(route.packet_count ?? 0),
      decoded_records: Number(route.decoded_records ?? 0),
      missing_application_payload_records: Number(route.missing_application_payload_records ?? 0),
      decode_failed_records: Number(route.decode_failed_records ?? 0),
    }));
  const structuralNonObligationRoutes = routeAudits
    .filter((route) => route.coverage_requirement === "structural_non_obligation")
    .map((route) => ({
      direction: route.route?.direction ?? null,
      fragment: route.route?.fragment?.kind ?? null,
      service_id: route.route?.service_id ?? null,
      method_id: route.route?.method_id ?? null,
      method_name_evidence: route.method_name ?? null,
      decoder: route.decoder ?? null,
      packet_count: Number(route.packet_count ?? 0),
      decoded_records: Number(route.decoded_records ?? 0),
      promotion_requirement_satisfied: route.promotion_requirement_satisfied === true,
      reason: route.structural_non_obligation_reason ?? null,
    }));
  if (Number(audit.schema_version) >= 3) {
    if (!String(audit.observability_contract_path ?? "") ||
      Number(audit.structural_non_obligation_route_count) !== structuralNonObligationRoutes.length ||
      Number(audit.observable_migrated_decoder_route_count) +
        structuralNonObligationRoutes.length !== Number(audit.migrated_decoder_route_count) ||
      Number(audit.validated_observable_migrated_decoder_route_count) !==
        routeAudits.filter((route) =>
          route.coverage_requirement === "matching_build_packet_evidence" &&
          route.validated === true
        ).length ||
      structuralNonObligationRoutes.some((route) =>
        route.packet_count !== 0 || route.decoded_records !== 0 ||
        route.promotion_requirement_satisfied !== true || !String(route.reason ?? "")
      )) {
      throw new Error("Protocol promotion audit has unsafe structural non-obligation accounting");
    }
  }
  const blockers = [...(audit.blockers ?? [])];
  if (promotedPresent && !promotedMatchesBuild) {
    blockers.push(`promoted protocol pack targets build ${promotedBuild || "<missing>"}, expected ${build}`);
  }
  if (promotedPresent && !audit.promotion_ready) {
    blockers.push("a promoted pack is present although matching-build promotion proof is incomplete");
  }
  if (promotedPresent && audit.promotion_ready && !promotedMatchesCandidate) {
    blockers.push("promoted protocol pack is not byte-identical to the audited candidate");
  }
  const status = audit.promotion_ready && blockers.length === 0 && promotedMatchesBuild && promotedMatchesCandidate
    ? "promoted"
    : audit.promotion_ready && blockers.length === 0
      ? "promotion_ready_not_installed"
      : "blocked";

  const output = {
    schema_version: STATUS_SCHEMA_VERSION,
    generated_by: "tools/bpsr-protocol-pack-status.mjs",
    game_build: String(build),
    generated_at: new Date().toISOString(),
    policy: {
      native_dispatch_order_is_wire_identity: false,
      matching_build_packet_evidence_required_for_observable_routes: true,
      capture_gaps_hidden: false,
      unobserved_migrated_routes_promoted: false,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      structural_non_obligations_are_not_packet_absence_as_zero: true,
      structural_non_obligations_never_synthesize_canonical_events: true,
      unknown_and_unresolved_canonical_events_are_preserved: true,
      statically_exact_unreplayed_routes_remain_opaque: true,
      use_slot_namespace_guessed: false,
      absent_promoted_pack_synthesized: false,
      segmented_route_evidence_does_not_prove_canonical_replay_conservation: true,
      protocol_pack_promotion_does_not_enable_rdps_runtime_by_itself: true,
    },
    status,
    candidate: {
      path: relativePath(candidateFile),
      bytes: statSync(candidateFile).size,
      sha256: sha256(candidateFile),
      pack_id: candidate.pack_id,
      audited_digest: audit.protocol_pack_digest,
      route_count: Array.isArray(candidate.routes) ? candidate.routes.length : 0,
    },
    evidence: {
      reports_root: relativePath(reportsRoot),
      matching_build_report_count: reportFiles.length,
      gap_free_segment_receipt_count: Number(audit.gap_free_segment_receipt_count ?? 0),
      report_inventory_sha256: hashJson(reportFiles.map((file) => ({
        path: path.basename(file), bytes: statSync(file).size, sha256: sha256(file),
      }))),
      ...reportTotals,
    },
    audit: {
      schema_version: Number(audit.schema_version),
      path: relativePath(auditFile),
      bytes: statSync(auditFile).size,
      sha256: sha256(auditFile),
      promotion_ready: Boolean(audit.promotion_ready),
      exact_world_service_id: audit.exact_world_service_id ?? null,
      exact_world_call_service_id: audit.exact_world_call_service_id ?? null,
      observed_exact_world_route_count: Number(audit.observed_exact_world_route_count ?? 0),
      migrated_decoder_route_count: Number(audit.migrated_decoder_route_count ?? 0),
      validated_migrated_decoder_route_count: Number(audit.validated_migrated_decoder_route_count ?? 0),
      observable_migrated_decoder_route_count:
        Number(audit.observable_migrated_decoder_route_count ?? audit.migrated_decoder_route_count ?? 0),
      validated_observable_migrated_decoder_route_count:
        Number(audit.validated_observable_migrated_decoder_route_count ??
          audit.validated_migrated_decoder_route_count ?? 0),
      structural_non_obligation_route_count: structuralNonObligationRoutes.length,
      observability_contract_path: audit.observability_contract_path ?? null,
      structural_non_obligation_routes: structuralNonObligationRoutes,
      unvalidated_routes: unvalidatedRoutes,
      use_slot_method_id: audit.use_slot_method_id ?? null,
      use_slot_candidate_disposition: audit.use_slot_candidate_disposition ?? null,
      use_slot_runtime_decoder_required: audit.use_slot_runtime_decoder_required ?? null,
      use_slot_promotion_requirement_satisfied:
        audit.use_slot_promotion_requirement_satisfied ?? null,
      use_slot_service_ids: audit.use_slot_service_ids ?? [],
      use_slot_routes: audit.use_slot_routes ?? [],
      capture_gap_count: Number(audit.capture_gap_count ?? 0),
      report_receipt_paths: audit.report_receipt_paths ?? [],
      gap_free_segment_receipt_count: Number(audit.gap_free_segment_receipt_count ?? 0),
      segmented_report_evidence_does_not_prove_canonical_replay_conservation:
        audit.segmented_report_evidence_does_not_prove_canonical_replay_conservation ?? false,
      canonical_replay_conservation_proven_by_this_audit:
        audit.canonical_replay_conservation_proven_by_this_audit ?? false,
      runtime_rdps_promotion_allowed_by_this_audit:
        audit.runtime_rdps_promotion_allowed_by_this_audit ?? false,
    },
    promoted_pack: {
      expected_path: relativePath(promotedFile),
      present: promotedPresent,
      build_matches: promotedMatchesBuild,
      byte_identical_to_candidate: promotedMatchesCandidate,
      pack_id: promoted?.pack_id ?? null,
      sha256: promotedPresent ? sha256(promotedFile) : null,
    },
    blockers,
  };
  verifyStatus(output);
  return output;
}

function verifyFile(file) {
  const value = readJson(file, "protocol-pack status");
  verifyStatus(value);
  console.log(`Protocol-pack status verified for build ${value.game_build}: ${value.status}.`);
}

function verifyStatus(value) {
  if (![1, 2, 3, 4].includes(Number(value.schema_version)) ||
    value.generated_by !== "tools/bpsr-protocol-pack-status.mjs") {
    throw new Error("Unsupported protocol-pack status schema or generator");
  }
  if (!/^\d+$/.test(String(value.game_build ?? ""))) throw new Error("Protocol status lacks a valid build");
  if (!Array.isArray(value.blockers)) throw new Error("Protocol status blockers must be an array");
  if (value.evidence.gap_count !== value.audit.capture_gap_count) {
    throw new Error("Protocol status capture gaps do not conserve");
  }
  if (value.audit.validated_migrated_decoder_route_count > value.audit.migrated_decoder_route_count) {
    throw new Error("Validated migrated route count exceeds total routes");
  }
  if (Number(value.schema_version) >= 2) {
    const structuralRoutes = value.audit?.structural_non_obligation_routes;
    if (value.policy?.matching_build_packet_evidence_required_for_observable_routes !== true ||
      value.policy
        ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
      value.policy?.structural_non_obligations_are_not_packet_absence_as_zero !== true ||
      value.policy?.structural_non_obligations_never_synthesize_canonical_events !== true ||
      value.policy?.unknown_and_unresolved_canonical_events_are_preserved !== true ||
      value.policy?.unobserved_migrated_routes_promoted !== false ||
      !Array.isArray(structuralRoutes) ||
      structuralRoutes.length !== Number(value.audit.structural_non_obligation_route_count) ||
      Number(value.audit.observable_migrated_decoder_route_count) + structuralRoutes.length !==
        Number(value.audit.migrated_decoder_route_count) ||
      Number(value.audit.validated_observable_migrated_decoder_route_count) >
        Number(value.audit.observable_migrated_decoder_route_count) ||
      structuralRoutes.some((route) =>
        !Number.isSafeInteger(Number(route.service_id)) || Number(route.service_id) <= 0 ||
        !Number.isSafeInteger(Number(route.method_id)) || Number(route.method_id) <= 0 ||
        route.packet_count !== 0 || route.decoded_records !== 0 ||
        route.promotion_requirement_satisfied !== true || !String(route.reason ?? "")
      )) {
      throw new Error("Protocol status has unsafe structural non-obligation accounting");
    }
  }
  if (Number(value.schema_version) >= 3) {
    if (value.policy?.statically_exact_unreplayed_routes_remain_opaque !== true) {
      throw new Error("Protocol status lost the fail-closed static-route policy");
    }
    if (Number(value.audit?.schema_version) >= 4) {
      const disposition = String(value.audit?.use_slot_candidate_disposition ?? "");
      const required = value.audit?.use_slot_runtime_decoder_required;
      const satisfied = value.audit?.use_slot_promotion_requirement_satisfied;
      if (Number(value.audit?.exact_world_call_service_id) !== 103198054 ||
        !["opaque", "allowed:world_use_slot_v1"].includes(disposition) ||
        typeof required !== "boolean" || typeof satisfied !== "boolean" ||
        (required && disposition !== "allowed:world_use_slot_v1") ||
        (!required && (disposition !== "opaque" || satisfied !== true))) {
        throw new Error("Protocol status has unsafe World.UseSlot activation accounting");
      }
    }
  }
  if (Number(value.schema_version) >= 4 && Number(value.audit?.schema_version) >= 6) {
    const receiptPaths = value.audit?.report_receipt_paths;
    if (value.policy?.segmented_route_evidence_does_not_prove_canonical_replay_conservation !== true ||
      value.policy?.protocol_pack_promotion_does_not_enable_rdps_runtime_by_itself !== true ||
      !Array.isArray(receiptPaths) || receiptPaths.length === 0 ||
      receiptPaths.length !== Number(value.audit?.gap_free_segment_receipt_count) ||
      Number(value.evidence?.gap_free_segment_receipt_count) !== receiptPaths.length ||
      value.audit?.segmented_report_evidence_does_not_prove_canonical_replay_conservation !== true ||
      value.audit?.canonical_replay_conservation_proven_by_this_audit !== false ||
      value.audit?.runtime_rdps_promotion_allowed_by_this_audit !== false) {
      throw new Error("Protocol status lost the receipted segment/conservation boundary");
    }
  }
  if (value.status === "promoted" && (!value.promoted_pack.present || !value.promoted_pack.byte_identical_to_candidate)) {
    throw new Error("Promoted status requires the audited candidate to be installed byte-for-byte");
  }
  if (value.status !== "promoted" && value.audit.promotion_ready && value.blockers.length === 0
      && value.status !== "promotion_ready_not_installed") {
    throw new Error("Promotion-ready evidence has an inconsistent status");
  }
  if (value.status === "blocked" && value.blockers.length === 0) {
    throw new Error("Blocked protocol status must retain at least one blocker");
  }
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-protocol-status-"));
  try {
    const reportsRoot = path.join(root, "reports");
    mkdirSync(reportsRoot);
    const candidateFile = path.join(root, "candidate.json");
    const auditFile = path.join(root, "audit.json");
    const promotedFile = path.join(root, "pack.json");
    const candidate = { schema_version: 1, pack_id: "candidate", target: { build_id: "123" }, routes: [] };
    writeJson(candidateFile, candidate);
    const report = {
      schema_version: 3,
      protocol_pack_id: "candidate",
      protocol_pack_digest: "sha256:test",
      capture: { packet_count: 2, gap_count: 1, known_packet_count: 2, unknown_packet_count: 0 },
      routes: [{ decode: { decoded_records: 2 } }],
    };
    const reportFile = path.join(reportsRoot, "one.offline-recording-report.json");
    writeJson(reportFile, report);
    writeJson(auditFile, {
      schema_version: 2,
      build_id: "123",
      protocol_pack_id: "candidate",
      protocol_pack_digest: "sha256:test",
      report_paths: [reportFile],
      observed_exact_world_route_count: 1,
      migrated_decoder_route_count: 1,
      validated_migrated_decoder_route_count: 0,
      use_slot_method_id: 249858,
      use_slot_service_ids: [],
      use_slot_routes: [],
      capture_gap_count: 1,
      route_audits: [{ route: { direction: "server_to_client", fragment: { kind: "notify" }, service_id: 1, method_id: 2 }, method_name: "X", decoder: "X1", packet_count: 0, decoded_records: 0, validated: false }],
      promotion_ready: false,
      blockers: ["retained proof gap"],
    });
    const blocked = buildStatus({ build: "123", candidateFile, auditFile, reportsRoot, promotedFile });
    if (blocked.status !== "blocked" || blocked.evidence.decoded_records !== 2 || blocked.blockers.length !== 1) {
      throw new Error("Blocked protocol status self-test failed");
    }
    writeJson(reportFile, { ...report, capture: { ...report.capture, gap_count: 0 } });
    writeJson(auditFile, {
      ...readJson(auditFile, "self-test audit"),
      validated_migrated_decoder_route_count: 1,
      capture_gap_count: 0,
      route_audits: [{ route: { direction: "server_to_client", fragment: { kind: "notify" }, service_id: 1, method_id: 2 }, method_name: "X", decoder: "X1", packet_count: 2, decoded_records: 2, validated: true }],
      promotion_ready: true,
      blockers: [],
    });
    const ready = buildStatus({ build: "123", candidateFile, auditFile, reportsRoot, promotedFile });
    if (ready.status !== "promotion_ready_not_installed") {
      throw new Error("Promotion-ready protocol status self-test failed");
    }
    mkdirSync(path.dirname(promotedFile), { recursive: true });
    copyFileSync(candidateFile, promotedFile, constants.COPYFILE_EXCL);
    const installed = buildStatus({ build: "123", candidateFile, auditFile, reportsRoot, promotedFile });
    if (installed.status !== "promoted") throw new Error("Installed candidate self-test failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-protocol-pack-status self-test passed");
}

function readJson(file, label) {
  requireFile(file, label);
  return JSON.parse(readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
}
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return String(value[key]); }
function resolvePath(value) { return path.isAbsolute(value) ? path.normalize(value) : path.resolve(repoRoot, value); }
function relativePath(value) { return path.relative(repoRoot, value).replaceAll("\\", "/"); }
function sha256(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashJson(value) { return createHash("sha256").update(JSON.stringify(value)).digest("hex"); }
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 2) {
    const token = args[index];
    if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[token.slice(2)] = next;
  }
  return output;
}
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-protocol-pack-status.mjs generate --build <id> --candidate <json> --audit <json> --reports-root <dir> --promoted-pack <json> --output <json>
  node tools/bpsr-protocol-pack-status.mjs promote --build <id> --candidate <json> --audit <json> --reports-root <dir> --promoted-pack <json> --output <json>
  node tools/bpsr-protocol-pack-status.mjs verify --input <json>
  node tools/bpsr-protocol-pack-status.mjs self-test

Compiles matching-build protocol candidate, recording, promotion-audit, and installed-pack
state into one conservative status artifact. The promote command uses an exclusive create and
installs only byte-identical audited candidate bytes. Missing proof is retained as a blocker.`);
  process.exit(exitCode);
}
