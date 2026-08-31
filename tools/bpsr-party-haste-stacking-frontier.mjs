#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const DEFAULT_EFFECT_ID = 31602;
const WINDOWS_MARKER = '"windows": [';

const [command = "help", ...rest] = process.argv.slice(2);
try {
  if (command === "build") await build(parseArgs(rest));
  else if (command === "verify") verify(path.resolve(required(parseArgs(rest), "input")));
  else if (command === "self-test") selfTest();
  else usage(command === "help" ? 0 : 1);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

async function build(options) {
  const buildId = String(required(options, "build"));
  const effectId = Number(options.effect ?? DEFAULT_EFFECT_ID);
  const auditPath = path.resolve(required(options, "party-effect-window-audit"));
  const buffTablePath = path.resolve(required(options, "buff-table"));
  const output = path.resolve(required(options, "output"));
  requireFile(auditPath, "party-effect window audit");
  requireFile(buffTablePath, "exact-build BuffTable");
  if (!Number.isSafeInteger(effectId) || effectId <= 0) throw new Error("Effect must be a positive integer");

  const buffBytes = readFileSync(buffTablePath);
  const buffTable = JSON.parse(buffBytes.toString("utf8"));
  const buffRow = buffTable[String(effectId)] ?? Object.values(buffTable)
    .find((row) => Number(row?.Id) === effectId);
  if (!buffRow || Number(buffRow.Id) !== effectId) {
    throw new Error(`BuffTable has no exact numeric row ${effectId}`);
  }

  const streamed = await streamAuditWindows(auditPath, effectId);
  const audit = streamed.header;
  if (Number(audit.schema_version) !== 8 ||
    audit.generated_by !== "rlogs-bpsr-party-effect-window-audit" ||
    String(audit.game_build) !== buildId ||
    audit.policy?.exact_numeric_effect_ids_and_build_are_authoritative !== true ||
    audit.policy?.remote_player_cast_packets_required !== false ||
    audit.policy?.remote_player_cast_packets_synthesized !== false) {
    throw new Error("Party-effect window audit identity or policy is unsafe");
  }
  const effect = (audit.effects ?? []).find((entry) => Number(entry?.effect_id) === effectId);
  if (!effect || Number(effect.status_events) <= 0) {
    throw new Error(`Party-effect audit has no observed effect ${effectId}`);
  }
  const analysis = analyzeWindows(streamed.windows);
  if (analysis.windows !== streamed.windows.length ||
    analysis.lifecycle_event_count !== Number(effect.status_events)) {
    throw new Error("Selected window lifecycle counts do not match the effect aggregate");
  }

  const proof = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-party-haste-stacking-frontier.mjs",
    game_build: buildId,
    effect_id: effectId,
    policy: {
      exact_numeric_effect_id_and_build_authoritative: true,
      localized_names_are_runtime_keys: false,
      static_integer_rule_values_are_not_semantics_without_exact_interpretation: true,
      only_observed_lifecycle_overlap_is_reported: true,
      absent_remote_cast_packets_are_not_required_synthesized_or_zero_filled: true,
      unknown_stacking_arbitration_is_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      party_effect_window_audit: {
        path: normalize(auditPath),
        bytes: streamed.bytes,
        sha256: streamed.sha256,
      },
      buff_table: {
        path: normalize(buffTablePath),
        bytes: buffBytes.length,
        sha256: sha256(buffBytes),
      },
    },
    exact_static_row: {
      id: Number(buffRow.Id),
      level: Number(buffRow.Level),
      buff_type: Number(buffRow.BuffType),
      buff_priority: Number(buffRow.BuffPriority),
      repeat_add_rule: structuredClone(buffRow.RepeatAddRule ?? []),
      time_refresh_type: Number(buffRow.TimeRefreshType),
      destroy_param: structuredClone(buffRow.DestroyParam ?? []),
      delete_dead: buffRow.DeleteDead === true,
      delete_offline: buffRow.DeleteOffline === true,
      delete_change_scene: buffRow.DeleteChangeScene === true,
      numeric_repeat_add_rule_semantics_proven: false,
      numeric_time_refresh_type_semantics_proven: false,
      stacking_arbitration_authority: false,
    },
    observed_lifecycle_surface: analysis,
    summary: {
      status_events: Number(effect.status_events),
      windows: analysis.windows,
      windows_with_terminal_event: analysis.windows_with_terminal_event,
      orphan_lifecycle_windows: analysis.orphan_lifecycle_windows,
      reported_stack_values: analysis.reported_stack_values,
      overlapping_window_pairs: analysis.overlapping_window_pairs,
      overlapping_window_pairs_with_distinct_provider_sets:
        analysis.overlapping_window_pairs_with_distinct_provider_sets,
      max_concurrent_windows_for_same_session_and_target:
        analysis.max_concurrent_windows_for_same_session_and_target,
      observed_same_effect_multi_provider_arbitration_cases:
        analysis.overlapping_window_pairs_with_distinct_provider_sets,
      exact_static_integer_rule_values_preserved: true,
      exact_static_integer_rule_semantics_proven: false,
      server_stacking_arbitration_proven: false,
      downstream_operation_order_and_rounding_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "repeat-add-rule-and-time-refresh-type-exact-integer-semantics-not-proven",
      ...(analysis.overlapping_window_pairs_with_distinct_provider_sets === 0
        ? ["same-effect-distinct-provider-overlap-not-observed-in-reviewed-cohort"]
        : []),
      "server-stacking-arbitration-not-proven",
      "downstream-action-opportunity-operation-order-and-rounding-not-proven",
      "counterfactual-damage-projection-and-conservation-not-proven",
    ],
  };
  proof.content_sha256 = contentHash(proof);
  writeFileSync(output, `${JSON.stringify(proof, null, 2)}\n`, "utf8");
  verify(output);
  console.log(
    `Party-Haste stacking frontier built for ${buildId}: ${analysis.windows} windows, ` +
    `${analysis.overlapping_window_pairs} overlap pairs; authority remains closed.`,
  );
}

async function streamAuditWindows(input, effectId) {
  const hash = createHash("sha256");
  let bytes = 0;
  let prefix = "";
  let header = null;
  let markerFound = false;
  let arrayEnded = false;
  let depth = 0;
  let inString = false;
  let escaped = false;
  let objectText = "";
  const windows = [];

  const consume = (text) => {
    for (let index = 0; index < text.length; index += 1) {
      const character = text[index];
      if (depth === 0) {
        if (character === "{") {
          depth = 1;
          objectText = "{";
          inString = false;
          escaped = false;
        } else if (character === "]") {
          arrayEnded = true;
          return;
        }
        continue;
      }
      objectText += character;
      if (inString) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === '"') inString = false;
        continue;
      }
      if (character === '"') inString = true;
      else if (character === "{") depth += 1;
      else if (character === "}") {
        depth -= 1;
        if (depth === 0) {
          const window = JSON.parse(objectText);
          if (Number(window.effect_id) === effectId) windows.push(selectWindow(window));
          objectText = "";
        }
      }
    }
  };

  for await (const chunk of createReadStream(input)) {
    const bytesChunk = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    hash.update(bytesChunk);
    bytes += bytesChunk.length;
    const text = bytesChunk.toString("utf8");
    if (!markerFound) {
      prefix += text;
      const markerIndex = prefix.indexOf(WINDOWS_MARKER);
      if (markerIndex < 0) {
        if (prefix.length > 64 * 1024 * 1024) {
          throw new Error("Party-effect audit windows marker was not found within 64 MiB");
        }
        continue;
      }
      markerFound = true;
      const headerText = `${prefix.slice(0, markerIndex + WINDOWS_MARKER.length)}]}`;
      header = JSON.parse(headerText);
      consume(prefix.slice(markerIndex + WINDOWS_MARKER.length));
      prefix = "";
    } else if (!arrayEnded) consume(text);
  }
  if (!markerFound || !arrayEnded || depth !== 0 || objectText !== "" || !header) {
    throw new Error("Party-effect audit windows array is truncated or malformed");
  }
  return { header, windows, bytes, sha256: hash.digest("hex") };
}

function selectWindow(window) {
  const selected = {
    session_id: String(window.session_id),
    affected_entity_actor_id: String(window.affected_entity_actor_id),
    affected_entity_uuid: String(window.affected_entity_uuid),
    instance_id: window.instance_id === null ? null : String(window.instance_id),
    source_entity_uuids: [...(window.source_entity_uuids ?? [])].map(String).sort(),
    missing_source_observed: window.missing_source_observed === true,
    provider_conflict_observed: window.provider_conflict_observed === true,
    reported_stacks: [...(window.reported_stacks ?? [])].map(Number).sort((a, b) => a - b),
    start_sequence: Number(window.start_sequence),
    end_sequence: window.end_sequence === null ? null : Number(window.end_sequence),
    start_observed_micros: Number(window.start_observed_micros),
    end_observed_micros:
      window.end_observed_micros === null ? null : Number(window.end_observed_micros),
    close_reason: window.close_reason === null ? null : String(window.close_reason),
    lifecycle_counts: structuredClone(window.lifecycle_counts ?? {}),
    orphan_lifecycle_start: window.orphan_lifecycle_start === true,
  };
  if (!Number.isSafeInteger(selected.start_sequence) ||
    !Number.isSafeInteger(selected.start_observed_micros) ||
    (selected.end_sequence !== null && !Number.isSafeInteger(selected.end_sequence)) ||
    (selected.end_observed_micros !== null && !Number.isSafeInteger(selected.end_observed_micros)) ||
    selected.reported_stacks.some((value) => !Number.isSafeInteger(value) || value < 0)) {
    throw new Error("Party-effect window contains an invalid selected lifecycle field");
  }
  return selected;
}

function analyzeWindows(windows) {
  const groups = new Map();
  const reportedStacks = new Set();
  const closeReasons = new Map();
  const lifecycleCounts = new Map();
  let windowsWithTerminalEvent = 0;
  let orphanLifecycleWindows = 0;
  let providerConflictWindows = 0;
  for (const window of windows) {
    const key = `${window.session_id}\0${window.affected_entity_actor_id}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(window);
    for (const value of window.reported_stacks) reportedStacks.add(value);
    if (window.end_observed_micros !== null) windowsWithTerminalEvent += 1;
    if (window.orphan_lifecycle_start) orphanLifecycleWindows += 1;
    if (window.provider_conflict_observed) providerConflictWindows += 1;
    increment(closeReasons, window.close_reason ?? "open_at_log_end", 1);
    for (const [state, count] of Object.entries(window.lifecycle_counts)) {
      increment(lifecycleCounts, state, Number(count));
    }
  }

  const overlapExamples = [];
  let overlappingWindowPairs = 0;
  let distinctProviderOverlapPairs = 0;
  let maxConcurrent = 0;
  for (const entries of groups.values()) {
    entries.sort((left, right) => left.start_observed_micros - right.start_observed_micros ||
      left.start_sequence - right.start_sequence);
    const active = [];
    for (const current of entries) {
      for (let index = active.length - 1; index >= 0; index -= 1) {
        const end = active[index].end_observed_micros;
        if (end !== null && end <= current.start_observed_micros) active.splice(index, 1);
      }
      for (const prior of active) {
        overlappingWindowPairs += 1;
        const distinctProviders = stableStringify(prior.source_entity_uuids) !==
          stableStringify(current.source_entity_uuids);
        if (distinctProviders) distinctProviderOverlapPairs += 1;
        if (overlapExamples.length < 32) {
          overlapExamples.push({
            session_id: current.session_id,
            affected_entity_actor_id: current.affected_entity_actor_id,
            prior_instance_id: prior.instance_id,
            current_instance_id: current.instance_id,
            prior_source_entity_uuids: prior.source_entity_uuids,
            current_source_entity_uuids: current.source_entity_uuids,
            distinct_provider_sets: distinctProviders,
            overlap_start_micros: current.start_observed_micros,
            overlap_end_micros: prior.end_observed_micros,
          });
        }
      }
      active.push(current);
      maxConcurrent = Math.max(maxConcurrent, active.length);
    }
  }
  return {
    windows: windows.length,
    lifecycle_event_count: [...lifecycleCounts.values()].reduce((sum, count) => sum + count, 0),
    lifecycle_counts: Object.fromEntries([...lifecycleCounts].sort()),
    windows_with_terminal_event: windowsWithTerminalEvent,
    windows_open_at_log_end: windows.length - windowsWithTerminalEvent,
    orphan_lifecycle_windows: orphanLifecycleWindows,
    provider_conflict_windows: providerConflictWindows,
    reported_stack_values: [...reportedStacks].sort((a, b) => a - b),
    close_reason_counts: Object.fromEntries([...closeReasons].sort()),
    session_target_groups: groups.size,
    overlapping_window_pairs: overlappingWindowPairs,
    overlapping_window_pairs_with_distinct_provider_sets: distinctProviderOverlapPairs,
    max_concurrent_windows_for_same_session_and_target: maxConcurrent,
    overlap_examples: overlapExamples,
    observed_absence_of_overlap_is_not_server_stacking_semantics: true,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function verify(input) {
  requireFile(input, "party-Haste stacking frontier");
  const proof = JSON.parse(readFileSync(input, "utf8"));
  const summary = proof.summary;
  const surface = proof.observed_lifecycle_surface;
  if (Number(proof.schema_version) !== SCHEMA_VERSION ||
    proof.generated_by !== "tools/bpsr-party-haste-stacking-frontier.mjs" ||
    Number(proof.effect_id) !== DEFAULT_EFFECT_ID ||
    proof.policy?.static_integer_rule_values_are_not_semantics_without_exact_interpretation !== true ||
    proof.policy?.only_observed_lifecycle_overlap_is_reported !== true ||
    proof.policy?.absent_remote_cast_packets_are_not_required_synthesized_or_zero_filled !== true ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    proof.exact_static_row?.id !== DEFAULT_EFFECT_ID ||
    proof.exact_static_row?.numeric_repeat_add_rule_semantics_proven !== false ||
    proof.exact_static_row?.numeric_time_refresh_type_semantics_proven !== false ||
    proof.exact_static_row?.stacking_arbitration_authority !== false ||
    !Number.isSafeInteger(Number(surface?.windows)) || Number(surface.windows) <= 0 ||
    Number(surface.lifecycle_event_count) !== Number(summary?.status_events) ||
    Number(surface.windows) !== Number(summary?.windows) ||
    Number(surface.overlapping_window_pairs) !== Number(summary?.overlapping_window_pairs) ||
    Number(surface.overlapping_window_pairs_with_distinct_provider_sets) !==
      Number(summary?.overlapping_window_pairs_with_distinct_provider_sets) ||
    summary?.exact_static_integer_rule_semantics_proven !== false ||
    summary?.server_stacking_arbitration_proven !== false ||
    summary?.downstream_operation_order_and_rounding_proven !== false ||
    summary?.formula_authority !== false || summary?.runtime_authority !== false ||
    summary?.ui_display_authority !== false || summary?.provider_rdps_credit_allowed !== false ||
    proof.content_sha256 !== contentHash(proof)) {
    throw new Error("Party-Haste stacking frontier is invalid or unsafe");
  }
  console.log(
    `Party-Haste stacking frontier verified for ${proof.game_build}: ${summary.windows} windows, ` +
    `server arbitration remains open.`,
  );
  return proof;
}

function selfTest() {
  const base = {
    session_id: "s",
    affected_entity_actor_id: "1",
    affected_entity_uuid: "11",
    source_entity_uuids: ["21"],
    missing_source_observed: false,
    provider_conflict_observed: false,
    reported_stacks: [1],
    end_sequence: 3,
    close_reason: "removed",
    lifecycle_counts: { applied: 1, removed: 1 },
    orphan_lifecycle_start: false,
  };
  const analysis = analyzeWindows([
    { ...base, instance_id: "a", start_sequence: 1, start_observed_micros: 10, end_observed_micros: 30 },
    { ...base, instance_id: "b", source_entity_uuids: ["22"], start_sequence: 2,
      start_observed_micros: 20, end_observed_micros: 40 },
    { ...base, instance_id: "c", start_sequence: 4, start_observed_micros: 40,
      end_sequence: null, end_observed_micros: null, close_reason: null,
      lifecycle_counts: { applied: 1 } },
  ]);
  if (analysis.windows !== 3 || analysis.overlapping_window_pairs !== 1 ||
    analysis.overlapping_window_pairs_with_distinct_provider_sets !== 1 ||
    analysis.max_concurrent_windows_for_same_session_and_target !== 2 ||
    analysis.lifecycle_event_count !== 5) {
    throw new Error("Party-Haste stacking frontier self-test failed");
  }
  console.log("bpsr-party-haste-stacking-frontier self-test passed");
}

function increment(map, key, amount) { map.set(key, (map.get(key) ?? 0) + amount); }
function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return sha256(Buffer.from(stableStringify(copy), "utf8"));
}
function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
function sha256(value) { return createHash("sha256").update(value).digest("hex"); }
function normalize(value) { return String(value).replaceAll("\\", "/"); }
function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`Invalid argument ${key ?? ""}`);
    output[key.slice(2)] = value;
  }
  return output;
}
function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}
function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-party-haste-stacking-frontier.mjs build --build <id> [--effect 31602] --party-effect-window-audit <json> --buff-table <json> --output <json>\n  node tools/bpsr-party-haste-stacking-frontier.mjs verify --input <json>\n  node tools/bpsr-party-haste-stacking-frontier.mjs self-test");
  process.exit(exitCode);
}
