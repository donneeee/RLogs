#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generate(options);
else if (command === "verify") verify(readJson(resolvePath(required(options, "input"))));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(options) {
  const build = required(options, "build");
  const files = {
    protocolStatus: resolvePath(required(options, "protocol-status")),
    segmentReceipt: resolvePath(required(options, "segment-receipt")),
    recordingReport: resolvePath(required(options, "recording-report")),
    replayAudit: resolvePath(required(options, "replay-audit")),
    supportTimeline: resolvePath(required(options, "support-timeline")),
    priorPackFullReplayAudit: resolvePath(required(options, "prior-pack-full-replay-audit")),
  };
  const status = readJson(files.protocolStatus);
  const receipt = readJson(files.segmentReceipt);
  const recording = readJson(files.recordingReport);
  const replay = readJson(files.replayAudit);
  const prior = readJson(files.priorPackFullReplayAudit);
  const timeline = await readJsonlEdges(files.supportTimeline);
  const exactDigest = String(status.candidate?.audited_digest ?? "");
  const replayReport = replay.reports?.[0];
  const currentPriorReports = (prior.reports ?? []).filter((entry) => String(entry.client_build) === build);
  const actors = Object.values(replayReport?.summary?.actors ?? {});
  const rawDamage = actors.reduce((sum, actor) => sum + integer(actor.raw_damage, "actor raw damage"), 0);
  const rdpsDamage = actors.reduce((sum, actor) => sum + integer(actor.rdps_damage, "actor rDPS damage"), 0);
  const priorDigests = [...new Set(currentPriorReports.map((entry) => String(entry.protocol_pack_digest)))].sort();

  assert(status.schema_version === 4 && status.status === "promoted", "Protocol pack is not promoted");
  assert(status.promoted_pack?.present === true && status.promoted_pack?.byte_identical_to_candidate === true,
    "Installed protocol pack is not the exact audited candidate");
  assert(status.audit?.promotion_ready === true && status.audit?.capture_gap_count === 0,
    "Protocol event coverage audit is not gap-free and promotion-ready");
  assert(status.audit?.validated_observable_migrated_decoder_route_count === status.audit?.observable_migrated_decoder_route_count,
    "Observable protocol routes are not completely validated");
  assert(receipt.schema_version === 2 && String(receipt.game_build) === build,
    "Segment receipt build or schema mismatch");
  assert(receipt.authority?.gap_free_selected_segment_proven === true && receipt.selected_capture_gap_records === 0,
    "Segment receipt is not gap-free");
  assert(receipt.policy?.output_proves_encounter_or_lifecycle_conservation === false
    && receipt.authority?.canonical_replay_conservation_proven === false,
  "Segment receipt improperly grants lifecycle conservation authority");
  assert(recording.capture?.gap_count === 0 && recording.record_count === receipt.selected_record_count,
    "Recording report does not match the receipted gap-free segment");
  assert(recording.protocol_pack_digest === exactDigest && recording.rlog?.event_count === timeline.summary.canonical_events,
    "Recording report, installed pack, and support timeline do not share exact identity");
  assert(replay.schema_version === 15 && replay.reports?.length === 1 && replayReport?.conserved === true,
    "Exact-pack replay audit is absent or unconserved");
  assert(replayReport.client_build === build && replayReport.protocol_pack_digest === exactDigest,
    "Replay audit does not belong to the exact installed pack");
  assert(replayReport.event_count === timeline.summary.canonical_events
    && replayReport.summary?.damage_event_count === timeline.summary.event_counts?.damage,
  "Replay and timeline event totals disagree");
  assert(replay.rule_effect_ids?.length === 0 && replayReport.summary?.attributed_damage_event_count === 0
    && replayReport.summary?.attributed_bonus_damage === 0,
  "Fail-closed exact-pack replay unexpectedly emitted provider credit");
  assert(rawDamage === rdpsDamage && rawDamage > 0, "Ordinary damage is not exactly conserved");
  assert(timeline.manifest.policy?.remote_player_cast_packets_required === false
    && timeline.manifest.policy?.remote_player_cast_packets_synthesized === false
    && timeline.summary.remote_cast_rows_synthesized === 0,
  "Support timeline requires or synthesizes remote casts");
  assert(timeline.manifest.topology?.effect_edge === "provider -> effect/status lifecycle -> recipient or enemy target"
    && timeline.manifest.topology?.source_side_join === "effect endpoint equals damage actor"
    && timeline.manifest.topology?.target_side_join === "effect endpoint equals damage target",
  "Support timeline lost allegiance-neutral source-side or target-side relationships");
  assert(currentPriorReports.length > 0 && currentPriorReports.every((entry) => entry.conserved === true),
    "Prior-pack full-run baseline is absent or unconserved");
  assert(priorDigests.every((digest) => digest !== exactDigest),
    "Prior-pack baseline was expected to remain distinct from the installed exact pack");

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-current-pack-conservation-boundary.mjs",
    game_build: build,
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      packet_absence_treated_as_zero: false,
      unknown_and_unresolved_lifecycles_preserved: true,
      current_character_snapshots_substituted_into_historical_runs: false,
      segmented_evidence_grants_closed_lifecycle_authority: false,
      ordinary_damage_totals_may_change_during_attribution: false,
      provider_credit_authorized_by_this_report: false,
    },
    inputs: Object.fromEntries(Object.entries(files).map(([key, file]) => [key, descriptor(file)])),
    installed_protocol_pack: {
      pack_id: status.candidate.pack_id,
      protocol_pack_digest: exactDigest,
      byte_identical_to_audited_candidate: true,
      observable_routes_validated: status.audit.validated_observable_migrated_decoder_route_count,
      observable_routes_required: status.audit.observable_migrated_decoder_route_count,
      structural_non_obligations: status.audit.structural_non_obligation_route_count,
    },
    exact_pack_gap_free_segment: {
      source_sequence_start: receipt.source_sequence_start,
      source_sequence_end: receipt.source_sequence_end,
      packet_records: recording.record_count,
      capture_gaps: recording.capture.gap_count,
      canonical_events: timeline.summary.canonical_events,
      resolved_status_events: timeline.summary.event_counts.status,
      unresolved_status_lifecycle_events: timeline.summary.event_counts.unresolved_status,
      damage_events: timeline.summary.event_counts.damage,
      remote_cast_rows_synthesized: timeline.summary.remote_cast_rows_synthesized,
      ordinary_raw_damage: rawDamage,
      ordinary_rdps_damage: rdpsDamage,
      attributed_damage_events: replayReport.summary.attributed_damage_event_count,
      attributed_bonus_damage: replayReport.summary.attributed_bonus_damage,
      ordinary_damage_conserved: true,
      closed_encounter_or_lifecycle_scope_proven: false,
      formula_counterfactual_replayed: false,
    },
    prior_pack_full_run_baseline: {
      protocol_pack_digests: priorDigests,
      current_build_sealed_runs: currentPriorReports.length,
      canonical_events: currentPriorReports.reduce((sum, entry) => sum + integer(entry.event_count, "prior event count"), 0),
      all_runs_conserved: true,
      exact_installed_pack_identity: false,
    },
    conclusion: {
      protocol_pack_identity_installed: true,
      protocol_event_coverage_proven: true,
      exact_pack_gap_free_segment_ordinary_damage_conservation_proven: true,
      exact_pack_closed_lifecycle_canonical_replay_conservation_proven: false,
      formula_specific_counterfactual_conservation_proven: false,
      runtime_rdps_promotion_allowed: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "capture one gap-free closed encounter or run with the installed exact protocol pack and replay its complete lifecycle",
      "replay each candidate formula with exact provider ownership, endpoint scope, magnitude, operation order, stacking, integer rounding, and party conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  verify(report);
  const output = resolvePath(required(options, "output"));
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
  console.log(`Current-pack conservation boundary verified for build ${build}: ordinary segment damage conserved; closed-lifecycle and formula conservation remain blocked.`);
}

function verify(report) {
  assert(report.schema_version === 1 && report.generated_by === "tools/bpsr-current-pack-conservation-boundary.mjs",
    "Unsupported conservation-boundary schema or generator");
  assert(report.policy?.segmented_evidence_grants_closed_lifecycle_authority === false
    && report.policy?.provider_credit_authorized_by_this_report === false,
  "Conservation-boundary policy is unsafe");
  assert(report.exact_pack_gap_free_segment?.ordinary_damage_conserved === true
    && report.exact_pack_gap_free_segment?.ordinary_raw_damage === report.exact_pack_gap_free_segment?.ordinary_rdps_damage,
  "Ordinary damage conservation is inconsistent");
  assert(report.exact_pack_gap_free_segment?.closed_encounter_or_lifecycle_scope_proven === false
    && report.exact_pack_gap_free_segment?.formula_counterfactual_replayed === false,
  "Segment evidence improperly claims lifecycle or formula authority");
  assert(report.conclusion?.exact_pack_closed_lifecycle_canonical_replay_conservation_proven === false
    && report.conclusion?.formula_specific_counterfactual_conservation_proven === false
    && report.conclusion?.runtime_rdps_promotion_allowed === false
    && report.conclusion?.provider_rdps_credit_allowed === false,
  "Conservation boundary improperly promotes rDPS");
  assert(report.blockers?.length === 2, "Conservation boundary lost its proof obligations");
  assert(report.content_sha256 === contentHash(report), "Conservation-boundary content digest mismatch");
  return report;
}

async function readJsonlEdges(file) {
  const input = createInterface({ input: createReadStream(file, "utf8"), crlfDelay: Infinity });
  let first = null;
  let last = null;
  for await (const line of input) {
    if (!line.trim()) continue;
    const value = JSON.parse(line);
    if (first === null) first = value;
    last = value;
  }
  assert(first?.row_type === "manifest" && last?.row_type === "run_summary",
    "Support timeline is missing manifest or run summary");
  return { manifest: first, summary: last };
}

function selfTest() {
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-current-pack-conservation-boundary.mjs",
    policy: { segmented_evidence_grants_closed_lifecycle_authority: false, provider_credit_authorized_by_this_report: false },
    exact_pack_gap_free_segment: { ordinary_damage_conserved: true, ordinary_raw_damage: 5, ordinary_rdps_damage: 5, closed_encounter_or_lifecycle_scope_proven: false, formula_counterfactual_replayed: false },
    conclusion: { exact_pack_closed_lifecycle_canonical_replay_conservation_proven: false, formula_specific_counterfactual_conservation_proven: false, runtime_rdps_promotion_allowed: false, provider_rdps_credit_allowed: false },
    blockers: ["closed lifecycle", "formula replay"],
  };
  report.content_sha256 = contentHash(report);
  verify(report);
  report.conclusion.runtime_rdps_promotion_allowed = true;
  let rejected = false;
  try { verify(report); } catch { rejected = true; }
  assert(rejected, "Unsafe runtime promotion was not rejected");
  console.log("bpsr-current-pack-conservation-boundary self-test passed");
}

function descriptor(file) {
  assert(existsSync(file) && statSync(file).isFile(), `Missing input file ${file}`);
  const bytes = readFileSync(file);
  return { path: file.replaceAll("\\", "/"), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}
function readJson(file) { return JSON.parse(readFileSync(file, "utf8").replace(/^\uFEFF/, "")); }
function contentHash(report) { const copy = structuredClone(report); delete copy.content_sha256; return `sha256:${createHash("sha256").update(stableStringify(copy)).digest("hex")}`; }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function integer(value, label) { const number = Number(value); assert(Number.isSafeInteger(number), `${label} must be a safe integer`); return number; }
function assert(condition, message) { if (!condition) throw new Error(message); }
function required(values, key) { assert(values[key], `Missing --${key}`); return String(values[key]); }
function resolvePath(value) { return path.isAbsolute(value) ? path.normalize(value) : path.resolve(repoRoot, value); }
function parseArgs(values) { const result = {}; for (let index = 0; index < values.length; index += 2) { assert(values[index]?.startsWith("--") && values[index + 1], "Options require --name value pairs"); result[values[index].slice(2)] = values[index + 1]; } return result; }
function usage(code) { console.log("Usage:\n  node tools/bpsr-current-pack-conservation-boundary.mjs generate --build <id> --protocol-status <json> --segment-receipt <json> --recording-report <json> --replay-audit <json> --support-timeline <jsonl> --prior-pack-full-replay-audit <json> --output <json>\n  node tools/bpsr-current-pack-conservation-boundary.mjs verify --input <json>\n  node tools/bpsr-current-pack-conservation-boundary.mjs self-test"); process.exit(code); }
