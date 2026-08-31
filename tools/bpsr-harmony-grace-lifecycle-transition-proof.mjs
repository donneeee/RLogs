#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import path from "node:path";

const FAMILY = Object.freeze({
  final: 11030,
  total: 11031,
  base_add: 11032,
  extra_add: 11033,
  raw_percent: 11034,
  extra_percent: 11035,
});
const FAMILY_IDS = new Set(Object.values(FAMILY));
const EFFECT_ID = 3_003_052;
const SOURCE_TYPE_ID = 1;
const SOURCE_CONFIG_ID = 3_003_053;
const PROVIDER_DELTA = 200;
const SCHEMA_VERSION = 2;
const MINIMUM_REPLAY_SCHEMA_VERSION = 23;
const TRACE_SCHEMA_VERSION = 6;

const [command = "help", ...argv] = process.argv.slice(2);
const options = parseArgs(argv);

if (command === "build") await build(options);
else if (command === "verify") verify(path.resolve(requiredOne(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(parsed) {
  const buildId = requiredOne(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  const eventPaths = requiredMany(parsed, "events").map((value) => path.resolve(value));
  const auditPaths = requiredMany(parsed, "audit").map((value) => path.resolve(value));
  const tracePaths = requiredMany(parsed, "trace").map((value) => path.resolve(value));
  const output = path.resolve(requiredOne(parsed, "output"));
  const providerActorId = parsePositiveInteger(requiredOne(parsed, "provider"), "provider");
  const recipientActorId = parsePositiveInteger(requiredOne(parsed, "recipient"), "recipient");

  for (const file of [...eventPaths, ...auditPaths, ...tracePaths]) requireFile(file);
  const eventReceipts = [];
  for (const file of eventPaths) {
    eventReceipts.push(await scanEventSlice(file, buildId, providerActorId, recipientActorId));
  }
  const auditReceipts = auditPaths.map((file) => scanAudit(file, buildId, providerActorId, recipientActorId));
  const traceReceipts = tracePaths.map((file) => scanTrace(file, buildId, providerActorId, recipientActorId));
  const witnesses = eventReceipts.flatMap((entry) => entry.transition_witnesses);
  const acceptedStates = mergeAcceptedStates(auditReceipts.flatMap((entry) => entry.accepted_states));
  attachWitnessSupport(acceptedStates, witnesses);
  const damageRows = traceReceipts.flatMap((entry) => entry.damage_rows);
  crossCheckAuditAndTraceRows(auditReceipts, traceReceipts);
  attachDamageRowWitnessSupport(damageRows, witnesses);
  const damageGroups = mergeDamageRowCoverage(damageRows);

  const exactWitnesses = witnesses.filter((entry) => entry.classification === "exact-provider-percent-transition");
  const supportedRows = acceptedStates
    .filter((entry) => entry.packet_witness_ids.length > 0)
    .reduce((sum, entry) => sum + entry.damage_rows, 0);
  const acceptedRows = acceptedStates.reduce((sum, entry) => sum + entry.damage_rows, 0);
  const exactRawBackgrounds = [...new Set(exactWitnesses.flatMap((entry) => [entry.before.raw_percent, entry.after.raw_percent]))]
    .filter((value) => Number.isInteger(value))
    .sort((left, right) => left - right);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-harmony-grace-lifecycle-transition-proof.mjs",
    game: "Blue Protocol: Star Resonance",
    game_build: buildId,
    proof_state: supportedRows === acceptedRows
      ? "single-effect-packet-transition-witnesses-cover-accepted-states-runtime-disabled"
      : "single-effect-packet-transition-witnesses-partial-runtime-disabled",
    policy: {
      exact_numeric_ids_authoritative: true,
      localized_names_are_not_runtime_keys: true,
      event_time_packet_state_only: true,
      missing_fields_are_not_zero_filled: true,
      unresolved_transitions_retained: true,
      receipt_does_not_enable_runtime_or_ui_attribution: true,
      runtime_transfer_enabled: false,
    },
    identity: {
      effect_id: EFFECT_ID,
      source_type_id: SOURCE_TYPE_ID,
      source_config_id: SOURCE_CONFIG_ID,
      provider_actor_id: providerActorId,
      recipient_actor_id: recipientActorId,
      expected_primary_raw_percent_delta: PROVIDER_DELTA,
    },
    primary_family: {
      ...FAMILY,
      integer_encoding: "protobuf-int32-varint",
      fixed_point_denominator: 10_000,
    },
    event_inputs: eventReceipts.map(({ transition_witnesses: ignored, ...entry }) => entry),
    audit_inputs: auditReceipts.map(({ accepted_states: ignored, ...entry }) => entry),
    trace_inputs: traceReceipts.map(({ damage_rows: ignored, ...entry }) => entry),
    transition_witnesses: witnesses,
    accepted_damage_states: acceptedStates,
    provider_lifecycle_damage_witness_groups: damageGroups,
    open_obligations: [
      "Retain every damage row rejected for a missing same-lifecycle transition witness with zero provider credit.",
      "Prove same-effect multi-provider stacking and removal arbitration.",
      "Retain the unresolved base-6841 primary-family rows with zero provider credit.",
      "Close exact promoted protocol-pack identity, canonical replay conservation, and protocol event coverage before runtime or UI promotion.",
    ],
    summary: {
      lifecycle_rows: eventReceipts.reduce((sum, entry) => sum + entry.lifecycle_rows, 0),
      exact_transition_witnesses: exactWitnesses.length,
      unresolved_transition_rows: witnesses.length - exactWitnesses.length,
      exact_raw_percent_backgrounds: exactRawBackgrounds,
      distinct_accepted_damage_states: acceptedStates.length,
      accepted_damage_rows: acceptedRows,
      packet_witness_supported_damage_rows: supportedRows,
      unsupported_damage_rows: acceptedRows - supportedRows,
      provider_lifecycle_damage_chain_rows: damageRows.length,
      same_instance_transition_supported_damage_rows: damageRows.filter((entry) => entry.same_instance_witness_ids.length > 0).length,
      same_session_transition_supported_damage_rows: damageRows.filter((entry) => entry.same_session_witness_ids.length > 0).length,
      exact_build_transition_supported_damage_rows: damageRows.filter((entry) => entry.exact_build_witness_ids.length > 0).length,
      runtime_gates_closed: 0,
      rdps_obligations_promoted: 0,
    },
  };

  validateReport(report);
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(
    `Harmony Grace transition proof built: ${exactWitnesses.length} exact witnesses; ` +
    `${supportedRows}/${acceptedRows} accepted damage rows have an exact-background packet witness.`,
  );
}

async function scanEventSlice(file, buildId, providerActorId, recipientActorId) {
  const hash = createHash("sha256");
  const state = emptyFamily();
  const activeInstances = new Set();
  const witnessedInstances = new Set();
  let currentCapture = null;
  let pendingAttributes = [];
  let bytes = 0;
  let lines = 0;
  let sessionId = null;
  let protocolPackDigest = null;
  let lifecycleRows = 0;
  const transitionWitnesses = [];
  const input = createReadStream(file);
  input.on("data", (chunk) => {
    hash.update(chunk);
    bytes += chunk.length;
  });
  const reader = createInterface({ input, crlfDelay: Infinity });

  for await (const line of reader) {
    if (!line.trim()) continue;
    lines += 1;
    const row = JSON.parse(line);
    if (String(row.region?.client_build) !== buildId) throw new Error(`${file}: build mismatch`);
    sessionId ??= row.session_id;
    protocolPackDigest ??= row.region?.protocol_pack_digest ?? null;
    if (row.session_id !== sessionId) throw new Error(`${file}: mixed session IDs`);
    if ((row.region?.protocol_pack_digest ?? null) !== protocolPackDigest) {
      throw new Error(`${file}: mixed protocol-pack digests`);
    }
    const timeline = row.event?.type === "timeline" ? row.event.data : null;
    const event = timeline?.kind;
    const capture = timeline?.provenance?.source?.capture_sequence;
    if (!Number.isSafeInteger(capture)) continue;
    if (capture !== currentCapture) {
      currentCapture = capture;
      pendingAttributes = [];
    }

    if (event?.event === "entity_attributes" && event.data?.actor?.actor_id === recipientActorId) {
      const update = event.data;
      const relevant = (update.attributes ?? []).filter((attribute) => FAMILY_IDS.has(attribute.attribute_id));
      if (relevant.length === 0) continue;
      const before = cloneFamily(state);
      if (update.update_kind === "snapshot") clearFamily(state);
      for (const attribute of relevant) {
        setFamilyValue(state, attribute.attribute_id, decodeInt32Varint(attribute.raw_value));
      }
      pendingAttributes.push({
        sequence: row.sequence,
        event_sequence: timeline.sequence,
        capture_sequence: capture,
        update_kind: update.update_kind,
        changed_attribute_ids: relevant.map((attribute) => attribute.attribute_id),
        before,
        after: cloneFamily(state),
      });
      continue;
    }

    if (event?.event !== "status") continue;
    const status = event.data;
    if (status?.effect !== EFFECT_ID || status.source?.actor_id !== providerActorId ||
        status.target?.actor_id !== recipientActorId) continue;
    if (status.origin?.source_type_id !== SOURCE_TYPE_ID || status.origin?.source_config_id !== SOURCE_CONFIG_ID) {
      throw new Error(`${file}: Harmony Grace lifecycle origin changed at sequence ${row.sequence}`);
    }
    lifecycleRows += 1;
    const instanceId = status.instance_id;
    const wasActive = activeInstances.has(instanceId);
    const isActive = ["applied", "refreshed", "stacked"].includes(status.state);
    let direction = 0;
    if (isActive && !wasActive) direction = 1;
    else if (!isActive && wasActive) direction = -1;
    if (isActive) activeInstances.add(instanceId);
    else activeInstances.delete(instanceId);
    const attribute = pendingAttributes.at(-1) ?? null;
    if (direction === 0 && isActive && !witnessedInstances.has(instanceId) && attribute &&
        Number.isSafeInteger(attribute.before.raw_percent) && Number.isSafeInteger(attribute.after.raw_percent) &&
        attribute.after.raw_percent - attribute.before.raw_percent === PROVIDER_DELTA) {
      // Some activations publish the lifecycle row first and the correlated
      // family transition on a later Refreshed row. Count that first exact
      // +200 transition once for the instance; repeated refresh echoes remain
      // lifecycle evidence but are not duplicate magnitude witnesses.
      direction = 1;
    }
    const classified = classifyTransition({
      sessionId,
      statusSequence: row.sequence,
      statusEventSequence: timeline.sequence,
      captureSequence: capture,
      instanceId,
      lifecycleState: status.state,
      direction,
      attribute,
    });
    transitionWitnesses.push(classified);
    if (classified.classification === "exact-provider-percent-transition") witnessedInstances.add(instanceId);
    if (!isActive) witnessedInstances.delete(instanceId);
  }

  return {
    path: path.relative(process.cwd(), file),
    bytes,
    sha256: hash.digest("hex"),
    jsonl_rows: lines,
    session_id: sessionId,
    protocol_pack_digest: protocolPackDigest,
    lifecycle_rows: lifecycleRows,
    transition_witnesses: transitionWitnesses,
  };
}

function classifyTransition(context) {
  const id = `${context.sessionId}:${context.captureSequence}:${context.statusSequence}`;
  if (context.direction === 0) {
    const deltas = context.attribute && completeFamilyPair(context.attribute.before, context.attribute.after)
      ? familyDelta(context.attribute.before, context.attribute.after)
      : null;
    return witness(context, id, "lifecycle-echo-no-provider-transition", context.attribute, deltas);
  }
  if (!context.attribute) {
    return witness(context, id, "no-same-capture-primary-family-update", null);
  }
  const before = context.attribute.before;
  const after = context.attribute.after;
  if (![before.base_add, before.extra_add, before.raw_percent, before.total, before.final,
        after.base_add, after.extra_add, after.raw_percent, after.total, after.final].every(Number.isSafeInteger)) {
    return witness(context, id, "incomplete-prior-or-current-family", context.attribute);
  }
  const deltas = familyDelta(before, after);
  if (deltas.raw_percent !== context.direction * PROVIDER_DELTA) {
    return witness(context, id, "provider-percent-magnitude-mismatch", context.attribute, deltas);
  }
  if (deltas.base_add !== 0 || deltas.extra_add !== 0) {
    return witness(context, id, "simultaneous-family-input-change", context.attribute, deltas);
  }
  if (deltas.final !== deltas.total) {
    return witness(context, id, "final-total-marginal-mismatch", context.attribute, deltas);
  }
  if (deltas.total === 0 || Math.sign(deltas.total) !== context.direction) {
    return witness(context, id, "nonpositive-provider-primary-marginal", context.attribute, deltas);
  }
  return witness(context, id, "exact-provider-percent-transition", context.attribute, deltas);
}

function completeFamilyPair(before, after) {
  return [...Object.keys(FAMILY).filter((key) => key !== "extra_percent")]
    .every((key) => Number.isSafeInteger(before[key]) && Number.isSafeInteger(after[key]));
}

function witness(context, id, classification, attribute, deltas = null) {
  return {
    witness_id: id,
    session_id: context.sessionId,
    capture_sequence: context.captureSequence,
    status_sequence: context.statusSequence,
    status_event_sequence: context.statusEventSequence,
    attribute_sequence: attribute?.sequence ?? null,
    attribute_event_sequence: attribute?.event_sequence ?? null,
    instance_id: context.instanceId,
    lifecycle_state: context.lifecycleState,
    provider_direction: context.direction,
    classification,
    changed_attribute_ids: attribute?.changed_attribute_ids ?? [],
    before: attribute?.before ?? null,
    after: attribute?.after ?? null,
    observed_deltas: deltas,
  };
}

function scanAudit(file, buildId, providerActorId, recipientActorId) {
  const bytes = readFileSync(file);
  const audit = JSON.parse(bytes.toString("utf8"));
  if (Number(audit.schema_version) < MINIMUM_REPLAY_SCHEMA_VERSION ||
      String(audit.runtime_rule_build) !== buildId) {
    throw new Error(`${file}: audit schema or runtime build mismatch`);
  }
  const reports = Array.isArray(audit.reports) ? audit.reports : [audit.reports];
  const accepted = [];
  let sessionId = null;
  let protocolPackDigest = null;
  let acceptedRows = 0;
  for (const report of reports) {
    sessionId ??= report.session_id;
    protocolPackDigest ??= report.protocol_pack_digest;
    for (const row of report.emitted_contribution_ledger ?? []) {
      const trace = row.formula_trace;
      if (!trace || Number(trace.effect_id) !== EFFECT_ID || Number(trace.provider_actor_id) !== providerActorId ||
          Number(trace.recipient_actor_id) !== recipientActorId) continue;
      if (trace.primary_provider_marginal_basis !== "same_lifecycle_packet_transition" ||
          !Number.isSafeInteger(Number(trace.primary_transition_capture_sequence)) ||
          !Number.isSafeInteger(Number(trace.primary_transition_instance_id))) {
        throw new Error(`${file}: accepted row is missing an exact lifecycle transition identity`);
      }
      acceptedRows += 1;
      accepted.push({
        primary_base_add: exactInteger(trace.primary_base_add, "primary_base_add"),
        primary_raw_percent: exactInteger(trace.primary_raw_percent, "primary_raw_percent"),
        primary_intermediate: exactInteger(trace.primary_intermediate, "primary_intermediate"),
        primary_extra_add: exactInteger(trace.primary_extra_add, "primary_extra_add"),
        primary_final: exactInteger(trace.primary_final, "primary_final"),
        provider_raw_percent: exactInteger(trace.provider_primary_raw_percent, "provider_primary_raw_percent"),
        provider_primary_marginal: exactInteger(trace.primary_provider_marginal, "provider_primary_marginal"),
        damage_rows: 1,
        packet_witness_ids: [],
      });
    }
  }
  return {
    path: path.relative(process.cwd(), file),
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    schema_version: audit.schema_version,
    session_id: sessionId,
    protocol_pack_digest: protocolPackDigest,
    accepted_damage_rows: acceptedRows,
    accepted_states: accepted,
  };
}

function scanTrace(file, buildId, providerActorId, recipientActorId) {
  const bytes = readFileSync(file);
  const report = JSON.parse(bytes.toString("utf8"));
  if (report.schema_version !== TRACE_SCHEMA_VERSION || report.effect_id !== EFFECT_ID || String(report.game_build) !== buildId ||
      report.policy?.provider_rdps_credit_allowed !== false || report.policy?.runtime_promotion_allowed !== false ||
      report.proof?.exact_provider_recipient_lifecycle !== true || report.proof?.replay_conserved !== true ||
      report.proof?.exact_same_lifecycle_primary_transition_marginal !== true) {
    throw new Error(`${file}: unsafe or incompatible single-effect trace`);
  }
  const damageRows = [];
  for (const row of report.traces ?? []) {
    const arithmetic = row.arithmetic;
    if (Number(row.provider_actor_id) !== providerActorId || Number(row.recipient_actor_id) !== recipientActorId ||
        Number(arithmetic?.effect_id) !== EFFECT_ID || row.lifecycle?.instance_id == null) {
      throw new Error(`${file}: trace row identity mismatch at damage sequence ${row.damage_sequence}`);
    }
    damageRows.push({
      session_id: report.session_id,
      lifecycle_instance_id: String(row.lifecycle.instance_id),
      lifecycle_sequence: Number(row.lifecycle.sequence),
      lifecycle_terminal_sequence: Number(row.lifecycle.terminal?.sequence),
      damage_sequence: Number(row.damage_sequence),
      primary_base_add: exactInteger(arithmetic.primary_base_add, "primary_base_add"),
      primary_raw_percent: exactInteger(arithmetic.primary_raw_percent, "primary_raw_percent"),
      provider_raw_percent: exactInteger(arithmetic.provider_primary_raw_percent, "provider_primary_raw_percent"),
      provider_primary_marginal: exactInteger(arithmetic.primary_provider_marginal, "primary_provider_marginal"),
      primary_transition_capture_sequence: exactInteger(
        arithmetic.primary_transition_capture_sequence,
        "primary_transition_capture_sequence",
      ),
      primary_transition_instance_id: String(exactInteger(
        arithmetic.primary_transition_instance_id,
        "primary_transition_instance_id",
      )),
      same_instance_witness_ids: [],
      same_session_witness_ids: [],
      exact_build_witness_ids: [],
    });
  }
  return {
    path: path.relative(process.cwd(), file),
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    schema_version: report.schema_version,
    session_id: report.session_id,
    protocol_pack_digest: report.protocol_pack_digest,
    lifecycle_instances: report.summary?.lifecycle_instances ?? [],
    damage_rows: damageRows,
  };
}

function crossCheckAuditAndTraceRows(audits, traces) {
  const auditRows = new Map(audits.map((entry) => [entry.session_id, entry.accepted_damage_rows]));
  const traceRows = new Map(traces.map((entry) => [entry.session_id, entry.damage_rows.length]));
  if (auditRows.size !== traceRows.size) throw new Error("Audit and trace session counts differ");
  for (const [sessionId, count] of auditRows) {
    if (traceRows.get(sessionId) !== count) throw new Error(`${sessionId}: audit and trace damage row counts differ`);
  }
}

function attachDamageRowWitnessSupport(rows, witnesses) {
  const exact = witnesses.filter((entry) => entry.classification === "exact-provider-percent-transition");
  for (const row of rows) {
    const matching = exact.filter((entry) => {
      const active = entry.provider_direction > 0 ? entry.after : entry.before;
      return active.base_add === row.primary_base_add && active.raw_percent === row.primary_raw_percent &&
        Math.abs(entry.observed_deltas.total) === row.provider_primary_marginal &&
        Math.abs(entry.observed_deltas.raw_percent) === row.provider_raw_percent;
    });
    row.exact_build_witness_ids = matching.map((entry) => entry.witness_id);
    row.same_session_witness_ids = matching
      .filter((entry) => entry.session_id === row.session_id)
      .map((entry) => entry.witness_id);
    row.same_instance_witness_ids = matching
      .filter((entry) => entry.session_id === row.session_id &&
        String(entry.instance_id) === row.lifecycle_instance_id &&
        String(entry.instance_id) === row.primary_transition_instance_id &&
        entry.capture_sequence === row.primary_transition_capture_sequence)
      .map((entry) => entry.witness_id);
  }
}

function mergeDamageRowCoverage(rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = [row.session_id, row.lifecycle_instance_id, row.primary_base_add, row.primary_raw_percent,
      row.provider_raw_percent, row.provider_primary_marginal,
      row.same_instance_witness_ids.join(","), row.same_session_witness_ids.join(","),
      row.exact_build_witness_ids.join(",")].join(":");
    const existing = groups.get(key);
    if (existing) {
      existing.damage_rows += 1;
      existing.first_damage_sequence = Math.min(existing.first_damage_sequence, row.damage_sequence);
      existing.last_damage_sequence = Math.max(existing.last_damage_sequence, row.damage_sequence);
    } else {
      groups.set(key, {
        session_id: row.session_id,
        lifecycle_instance_id: row.lifecycle_instance_id,
        lifecycle_sequence: row.lifecycle_sequence,
        lifecycle_terminal_sequence: row.lifecycle_terminal_sequence,
        primary_base_add: row.primary_base_add,
        primary_raw_percent: row.primary_raw_percent,
        provider_raw_percent: row.provider_raw_percent,
        provider_primary_marginal: row.provider_primary_marginal,
        damage_rows: 1,
        first_damage_sequence: row.damage_sequence,
        last_damage_sequence: row.damage_sequence,
        same_instance_witness_ids: row.same_instance_witness_ids,
        same_session_witness_ids: row.same_session_witness_ids,
        exact_build_witness_ids: row.exact_build_witness_ids,
      });
    }
  }
  return [...groups.values()].sort((left, right) =>
    left.session_id.localeCompare(right.session_id) ||
    Number(left.lifecycle_instance_id) - Number(right.lifecycle_instance_id) ||
    left.primary_raw_percent - right.primary_raw_percent);
}

function mergeAcceptedStates(states) {
  const merged = new Map();
  for (const state of states) {
    const key = [state.primary_base_add, state.primary_raw_percent, state.primary_intermediate,
      state.primary_extra_add, state.primary_final, state.provider_raw_percent,
      state.provider_primary_marginal].join(":");
    const existing = merged.get(key);
    if (existing) existing.damage_rows += state.damage_rows;
    else merged.set(key, { ...state });
  }
  return [...merged.values()].sort((left, right) =>
    left.primary_base_add - right.primary_base_add ||
    left.primary_raw_percent - right.primary_raw_percent ||
    left.primary_extra_add - right.primary_extra_add);
}

function attachWitnessSupport(states, witnesses) {
  const exact = witnesses.filter((entry) => entry.classification === "exact-provider-percent-transition");
  for (const state of states) {
    state.packet_witness_ids = exact.filter((entry) => {
      const active = entry.provider_direction > 0 ? entry.after : entry.before;
      return active.base_add === state.primary_base_add &&
        active.raw_percent === state.primary_raw_percent &&
        Math.abs(entry.observed_deltas.total) === state.provider_primary_marginal &&
        Math.abs(entry.observed_deltas.raw_percent) === state.provider_raw_percent;
    }).map((entry) => entry.witness_id);
  }
}

function validateReport(report) {
  if (report.schema_version !== SCHEMA_VERSION || !report.proof_state?.startsWith("single-effect-packet-transition-witnesses-")) {
    throw new Error("Unexpected Harmony Grace transition proof schema or state");
  }
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("Invalid report build");
  if (report.identity?.effect_id !== EFFECT_ID || report.identity?.source_type_id !== SOURCE_TYPE_ID ||
      report.identity?.source_config_id !== SOURCE_CONFIG_ID ||
      report.identity?.expected_primary_raw_percent_delta !== PROVIDER_DELTA) {
    throw new Error("Harmony Grace identity changed");
  }
  const policy = report.policy ?? {};
  if (!policy.exact_numeric_ids_authoritative || !policy.event_time_packet_state_only ||
      !policy.missing_fields_are_not_zero_filled || !policy.unresolved_transitions_retained ||
      !policy.receipt_does_not_enable_runtime_or_ui_attribution || policy.runtime_transfer_enabled !== false) {
    throw new Error("Unsafe transition proof policy");
  }
  const exact = (report.transition_witnesses ?? []).filter((entry) => entry.classification === "exact-provider-percent-transition");
  for (const entry of exact) {
    if (Math.abs(entry.observed_deltas?.raw_percent) !== PROVIDER_DELTA ||
        entry.observed_deltas?.base_add !== 0 || entry.observed_deltas?.extra_add !== 0 ||
        entry.observed_deltas?.final !== entry.observed_deltas?.total ||
        Math.sign(entry.observed_deltas?.total) !== entry.provider_direction) {
      throw new Error(`Invalid exact transition witness ${entry.witness_id}`);
    }
  }
  const supported = (report.accepted_damage_states ?? [])
    .filter((entry) => entry.packet_witness_ids?.length > 0)
    .reduce((sum, entry) => sum + entry.damage_rows, 0);
  if (supported !== report.summary?.packet_witness_supported_damage_rows ||
      report.summary?.provider_lifecycle_damage_chain_rows !== report.summary?.accepted_damage_rows ||
      report.summary?.same_instance_transition_supported_damage_rows !== report.summary?.accepted_damage_rows ||
      report.summary?.exact_build_transition_supported_damage_rows !== report.summary?.accepted_damage_rows ||
      report.summary?.runtime_gates_closed !== 0 || report.summary?.rdps_obligations_promoted !== 0) {
    throw new Error("Transition proof summary or fail-closed policy changed");
  }
}

function verify(file) {
  requireFile(file);
  const report = JSON.parse(readFileSync(file, "utf8"));
  validateReport(report);
  console.log(
    `Harmony Grace transition proof verified: ${report.summary.exact_transition_witnesses} exact witnesses; ` +
    `${report.summary.packet_witness_supported_damage_rows}/${report.summary.accepted_damage_rows} accepted rows supported.`,
  );
}

function emptyFamily() {
  return { final: null, total: null, base_add: null, extra_add: null, raw_percent: null, extra_percent: null };
}

function clearFamily(state) {
  for (const key of Object.keys(FAMILY)) state[key] = null;
}

function cloneFamily(state) {
  return { ...state };
}

function setFamilyValue(state, attributeId, value) {
  const key = Object.entries(FAMILY).find(([, id]) => id === attributeId)?.[0];
  if (!key) throw new Error(`Unknown primary-family attribute ${attributeId}`);
  state[key] = value;
}

function familyDelta(before, after) {
  return Object.fromEntries(Object.keys(FAMILY).map((key) => [key, after[key] - before[key]]));
}

function decodeInt32Varint(bytes) {
  if (!Array.isArray(bytes) || bytes.length > 10) throw new Error("Invalid int32 varint bytes");
  // The scalar is nested inside a protobuf bytes field. Prost omits an int32
  // scalar's zero byte, so a present attribute row with [] is exact zero.
  if (bytes.length === 0) return 0;
  let value = 0n;
  let shift = 0n;
  let terminated = false;
  for (const byte of bytes) {
    if (!Number.isInteger(byte) || byte < 0 || byte > 255) throw new Error("Invalid varint byte");
    value |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) {
      terminated = true;
      break;
    }
    shift += 7n;
  }
  if (!terminated) throw new Error("Unterminated int32 varint");
  const low32 = value & 0xffff_ffffn;
  return Number(low32 >= 0x8000_0000n ? low32 - 0x1_0000_0000n : low32);
}

function selfTest() {
  if (decodeInt32Varint([180, 16]) !== 2100) throw new Error("positive int32 fixture failed");
  if (decodeInt32Varint([184, 229, 255, 255, 255, 255, 255, 255, 255, 1]) !== -3400) {
    throw new Error("negative int32 fixture failed");
  }
  const exact = classifyTransition({
    sessionId: "fixture",
    statusSequence: 11,
    statusEventSequence: 10,
    captureSequence: 7,
    instanceId: 3,
    lifecycleState: "applied",
    direction: 1,
    attribute: {
      sequence: 10,
      event_sequence: 9,
      changed_attribute_ids: [11030, 11031, 11034],
      before: { final: 8301, total: 8301, base_add: 6976, extra_add: 0, raw_percent: 1900, extra_percent: null },
      after: { final: 8441, total: 8441, base_add: 6976, extra_add: 0, raw_percent: 2100, extra_percent: null },
    },
  });
  if (exact.classification !== "exact-provider-percent-transition" || exact.observed_deltas.total !== 140) {
    throw new Error("exact transition fixture failed");
  }
  console.log("Harmony Grace lifecycle transition proof self-test passed.");
}

function exactInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) throw new Error(`Invalid ${label}: ${value}`);
  return parsed;
}

function parsePositiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive safe integer`);
  return parsed;
}

function parseArgs(args) {
  const parsed = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    const key = flag.slice(2);
    const values = parsed.get(key) ?? [];
    values.push(value);
    parsed.set(key, values);
  }
  return parsed;
}

function requiredOne(parsed, key) {
  const values = parsed.get(key) ?? [];
  if (values.length !== 1) throw new Error(`Expected exactly one --${key}`);
  return values[0];
}

function requiredMany(parsed, key) {
  const values = parsed.get(key) ?? [];
  if (values.length === 0) throw new Error(`Missing --${key}`);
  return values;
}

function required(parsed, key) {
  return requiredOne(parsed, key);
}

function requireFile(file) {
  if (!existsSync(file)) throw new Error(`Required file does not exist: ${file}`);
}

function usage(exitCode) {
  console.error(
    "usage: bpsr-harmony-grace-lifecycle-transition-proof.mjs build --build <id> " +
    "--provider <actor> --recipient <actor> --events <actor-events.jsonl> [--events <...>] " +
    "--audit <rdps-audit.json> [--audit <...>] --trace <single-effect-trace.json> [--trace <...>] " +
    "--output <proof.json>\n" +
    "       bpsr-harmony-grace-lifecycle-transition-proof.mjs verify --input <proof.json>\n" +
    "       bpsr-harmony-grace-lifecycle-transition-proof.mjs self-test",
  );
  process.exit(exitCode);
}
