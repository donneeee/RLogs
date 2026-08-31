#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "analyze") analyze(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const build = required(parsed, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const effect = Number(required(parsed, "effect"));
  if (!Number.isSafeInteger(effect) || effect <= 0) throw new Error("Effect must be an exact positive integer");
  return {
    build,
    effect,
    providerOwnershipProof: path.resolve(required(parsed, "provider-ownership-proof")),
    output: path.resolve(required(parsed, "output")),
  };
}

function analyze(context) {
  const proof = readJson(context.providerOwnershipProof, "provider ownership proof");
  validateProviderProof(proof, context.build, context.effect);
  const effect = proof.effects.find((entry) => Number(entry.effect_id) === context.effect);
  const selected = (proof.resolutions ?? []).filter((entry) => Number(entry.effect_id) === context.effect);
  const proven = selected.filter(isStablePlayerResolution);
  const unresolved = selected.filter((entry) => !isStablePlayerResolution(entry));
  const provenBySource = groupBy(proven, sourceLifecycleKey);
  const gapGroups = unresolved.map((entry) => {
    const sameSourceProven = provenBySource.get(sourceLifecycleKey(entry)) ?? [];
    return {
      rlog: String(entry.rlog),
      session_id: String(entry.session_id),
      run_ordinal: exactCount(entry.run_ordinal, "run ordinal"),
      resolution_class: String(entry.class),
      origin_source_type_id: nullableInteger(entry.origin_source_type_id),
      origin_source_config_id: nullableInteger(entry.origin_source_config_id),
      source_actor_id: nullableInteger(entry.source?.actor_id),
      source_entity_uuid: nullableInteger(entry.source?.entity_uuid),
      status_events: exactCount(entry.status_events, "gap status events"),
      status_state_counts: normalizedCounts(entry.status_state_counts ?? {}),
      first_sequence: exactCount(entry.first_sequence, "first sequence"),
      last_sequence: exactCount(entry.last_sequence, "last sequence"),
      first_observed_micros: exactCount(entry.first_observed_micros, "first observed micros"),
      last_observed_micros: exactCount(entry.last_observed_micros, "last observed micros"),
      same_source_has_separate_stable_player_resolution: sameSourceProven.length > 0,
      same_source_separate_stable_player_status_events: sum(
        sameSourceProven,
        (row) => exactCount(row.status_events, "same-source proven status events"),
      ),
      same_source_separate_stable_player_character_ids: uniqueSorted(
        sameSourceProven.map((row) => row.resolved_owner?.character_id ?? row.source?.character_id).filter(Boolean),
      ),
      later_or_separate_resolution_is_diagnostic_only: true,
      future_evidence_backfill_allowed: false,
      required_capture_evidence: captureRequirementsFor(entry.class),
      examples: (entry.examples ?? []).map((example) => ({
        sequence: exactCount(example.sequence, "example sequence"),
        observed_micros: exactCount(example.observed_micros, "example observed micros"),
        state: String(example.state),
        source_actor_id: nullableInteger(example.source_actor_id),
        source_entity_uuid: nullableInteger(example.source_entity_uuid),
        target_actor_id: exactCount(example.target_actor_id, "example target actor"),
        target_entity_uuid: nullableInteger(example.target_entity_uuid),
        instance_id: nullableInteger(example.instance_id),
        source_actor_snapshot_sequence: nullableInteger(example.source_actor_snapshot_sequence),
        owner_actor_snapshot_sequence: nullableInteger(example.owner_actor_snapshot_sequence),
      })),
    };
  }).sort(compareGapGroups);

  const unresolvedEvents = sum(gapGroups, (entry) => entry.status_events);
  const sameSourceDiagnosticEvents = sum(
    gapGroups.filter((entry) => entry.same_source_has_separate_stable_player_resolution),
    (entry) => entry.status_events,
  );
  const report = {
    schema_version: 3,
    generated_by: "tools/bpsr-provider-ownership-gap-worklist.mjs",
    game_build: context.build,
    effect_id: context.effect,
    policy: {
      exact_numeric_effect_id_and_build_are_authoritative: true,
      provider_ownership_proof_is_the_only_resolution_authority: true,
      localized_names_are_evidence_only: true,
      later_or_separate_same_source_rows_are_diagnostic_only: true,
      prior_exact_status_instance_player_ownership_may_flow_forward_only: true,
      exact_same_wire_packet_attributed_combat_relations_may_resolve_earlier_emitted_statuses: true,
      same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time: true,
      future_actor_or_ownership_evidence_may_backfill_prior_status_events: false,
      unresolved_events_are_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    input: {
      provider_ownership_proof: fileDescriptor(context.providerOwnershipProof),
    },
    summary: {
      selected_status_events: exactCount(effect.status_events, "selected status events"),
      stable_player_owned_status_events: exactCount(
        effect.status_events_with_stable_player_character_id,
        "stable player owned status events",
      ),
      prior_status_instance_player_owned_status_events: exactCount(
        proof.summary?.selected_events_with_prior_status_instance_player_owner ?? 0,
        "prior status instance player owned status events",
      ),
      same_wire_packet_player_owned_status_events: exactCount(
        proof.summary?.selected_events_with_same_wire_packet_player_owner ?? 0,
        "same wire packet player owned status events",
      ),
      unresolved_status_events: unresolvedEvents,
      gap_groups: gapGroups.length,
      gap_groups_with_same_source_separate_stable_player_resolution: gapGroups.filter(
        (entry) => entry.same_source_has_separate_stable_player_resolution,
      ).length,
      unresolved_events_with_same_source_separate_stable_player_resolution: sameSourceDiagnosticEvents,
      unresolved_events_without_same_source_stable_player_resolution:
        unresolvedEvents - sameSourceDiagnosticEvents,
      resolution_class_counts: countBy(gapGroups, (entry) => entry.resolution_class, (entry) => entry.status_events),
      status_state_counts: mergeCounts(gapGroups.map((entry) => entry.status_state_counts)),
      rlog_counts: countBy(gapGroups, (entry) => entry.rlog, (entry) => entry.status_events),
      exact_provider_ownership_proven: unresolvedEvents === 0,
      acquisition_required: unresolvedEvents > 0,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    acquisition_contract: {
      target: unresolvedEvents > 0
        ? "capture exact source identity and ownership evidence at or within the same exact wire packet as each unresolved status application; later lifecycle rows may inherit only from an earlier player-owned transition of the exact same status instance"
        : "satisfied: every selected status lifecycle event has exact packet-proven player ownership",
      required_event_routes: [
        "canonical Actor identity for the source actor and entity before the status event",
        "canonical EntityAttributes ownership or attributed combat source relation before the status event",
        "or a later-emitted attributed combat source relation in the exact same capture sequence, connection, stream, and observed time",
        "canonical Actor identity for the resolved owner before the status event",
        "exact same-run source actor, source entity, owner actor, and owner entity continuity",
      ],
      forbidden_shortcuts: [
        "later actor snapshots backfilled into earlier status events",
        "ownership relations from a later wire packet backfilled into earlier status events",
        "shared projectile config treated as provider identity",
        "localized source or skill name treated as provider identity",
        "current character snapshot substituted into an older run",
      ],
      success_condition:
        "all selected status instances establish a stable player provider no later than the end of the exact wire packet containing their first active transition; later lifecycle rows preserve that exact forward-only ownership",
    },
    gap_groups: gapGroups,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `Provider ownership gap worklist built for ${context.build} effect ${context.effect}: ` +
      `${unresolvedEvents} unresolved events across ${gapGroups.length} groups; ` +
      `${sameSourceDiagnosticEvents} have later or separate same-source evidence that remains diagnostic only.`,
  );
}

function validateProviderProof(proof, build, effectId) {
  if (![3, 4, 5].includes(Number(proof?.schema_version)) ||
    proof?.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof" ||
    String(proof.game_build) !== build) {
    throw new Error("Unsupported provider ownership proof schema, generator, or build");
  }
  if (proof.policy?.scope !== "provider_ownership_only" ||
    proof.policy?.exact_numeric_effect_ids_authoritative !== true ||
    proof.policy?.exact_input_build_authoritative !== true ||
    proof.policy?.actor_kind_or_packet_proven_ancestry_required_for_player_ownership !== true ||
    (Number(proof.schema_version) >= 4 &&
      (proof.policy?.prior_exact_status_instance_player_ownership_may_flow_forward !== true ||
        proof.policy
          ?.forward_status_instance_ownership_requires_exact_run_target_effect_instance_and_source !== true ||
        proof.policy?.conflicting_status_instance_owners_disable_inheritance !== true)) ||
    (Number(proof.schema_version) >= 5 &&
      (proof.policy?.later_attributed_combat_relation_in_same_exact_wire_packet_may_resolve_provider !== true ||
        proof.policy
          ?.same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time !== true)) ||
    proof.policy?.future_actor_snapshots_may_backfill_prior_status_events !== false ||
    proof.policy?.unknown_and_unresolved_events_preserved !== true ||
    proof.policy?.formula_authority !== false || proof.policy?.runtime_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false) {
    throw new Error("Provider ownership proof violates its fail-closed authority contract");
  }
  const effect = (proof.effects ?? []).find((entry) => Number(entry.effect_id) === effectId);
  if (!effect || !Number.isSafeInteger(Number(effect.status_events)) ||
    !Number.isSafeInteger(Number(effect.status_events_with_stable_player_character_id)) ||
    Number(effect.status_events_with_stable_player_character_id) > Number(effect.status_events)) {
    throw new Error(`Effect ${effectId} is absent or has invalid provider-ownership counts`);
  }
}

function verify(input) {
  const report = readJson(input, "provider ownership gap worklist");
  if (![1, 2, 3].includes(Number(report?.schema_version)) ||
    report?.generated_by !== "tools/bpsr-provider-ownership-gap-worklist.mjs" ||
    report?.content_sha256 !== contentHash(report) ||
    !/^\d+$/.test(String(report.game_build)) ||
    !Number.isSafeInteger(Number(report.effect_id)) || Number(report.effect_id) <= 0) {
    throw new Error("Invalid provider ownership gap worklist identity or content hash");
  }
  const policy = report.policy ?? {};
  if (policy.exact_numeric_effect_id_and_build_are_authoritative !== true ||
    policy.provider_ownership_proof_is_the_only_resolution_authority !== true ||
    policy.later_or_separate_same_source_rows_are_diagnostic_only !== true ||
    (Number(report.schema_version) >= 2 &&
      policy.prior_exact_status_instance_player_ownership_may_flow_forward_only !== true) ||
    (Number(report.schema_version) >= 3 &&
      (policy.exact_same_wire_packet_attributed_combat_relations_may_resolve_earlier_emitted_statuses !== true ||
        policy.same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time !== true)) ||
    policy.future_actor_or_ownership_evidence_may_backfill_prior_status_events !== false ||
    policy.unresolved_events_are_preserved !== true ||
    policy.formula_authority !== false || policy.runtime_authority !== false ||
    policy.provider_rdps_credit_allowed !== false) {
    throw new Error("Provider ownership gap worklist policy is unsafe");
  }
  const descriptor = report.input?.provider_ownership_proof;
  if (!descriptor || !existsSync(descriptor.path) ||
    statSync(descriptor.path).size !== Number(descriptor.bytes) ||
    sha256File(descriptor.path) !== descriptor.sha256) {
    throw new Error("Provider ownership gap worklist input provenance is missing or changed");
  }
  const proof = readJson(descriptor.path, "provider ownership proof");
  validateProviderProof(proof, String(report.game_build), Number(report.effect_id));
  const groups = report.gap_groups ?? [];
  const unresolved = sum(groups, (entry) => exactCount(entry.status_events, "gap status events"));
  const sameSource = sum(
    groups.filter((entry) => entry.same_source_has_separate_stable_player_resolution === true),
    (entry) => entry.status_events,
  );
  const exactOwnership = unresolved === 0;
  if ((Number(report.schema_version) < 3 && (groups.length === 0 || unresolved <= 0)) ||
    unresolved !== Number(report.summary?.unresolved_status_events) ||
    groups.length !== Number(report.summary?.gap_groups) ||
    sameSource !== Number(report.summary?.unresolved_events_with_same_source_separate_stable_player_resolution) ||
    unresolved - sameSource !== Number(report.summary?.unresolved_events_without_same_source_stable_player_resolution) ||
    report.summary?.exact_provider_ownership_proven !== exactOwnership ||
    report.summary?.acquisition_required !== !exactOwnership ||
    report.summary?.formula_authority !== false || report.summary?.runtime_authority !== false ||
    report.summary?.provider_rdps_credit_allowed !== false ||
    groups.some((entry) => entry.future_evidence_backfill_allowed !== false ||
      entry.later_or_separate_resolution_is_diagnostic_only !== true ||
      !Array.isArray(entry.required_capture_evidence) || entry.required_capture_evidence.length === 0)) {
    throw new Error("Provider ownership gap worklist counts or fail-closed gates are inconsistent");
  }
  console.log(
    `Provider ownership gap worklist verified for ${report.game_build} effect ${report.effect_id}: ` +
      `${unresolved} unresolved events, no backfill or rDPS authority.`,
  );
  return report;
}

function isStablePlayerResolution(entry) {
  if (!new Set(["direct_player", "owned_by_player", "same_wire_packet_owned_by_player", "prior_status_instance_player"])
    .has(String(entry.class))) return false;
  const characterId = entry.resolved_owner?.character_id ?? entry.source?.character_id;
  return typeof characterId === "string" && characterId.length > 0;
}

function sourceLifecycleKey(entry) {
  return [
    String(entry.rlog),
    String(entry.session_id),
    Number(entry.run_ordinal),
    entry.source?.actor_id ?? "missing",
    entry.source?.entity_uuid ?? "missing",
  ].join("|");
}

function captureRequirementsFor(resolutionClass) {
  if (resolutionClass === "source_identity_unobserved") {
    return [
      "observe the exact source Actor identity before the selected status event",
      "observe the exact source-to-owner relation before the selected status event",
      "observe the resolved owner Actor identity and stable player character ID before the selected status event",
    ];
  }
  if (resolutionClass === "non_player_unowned") {
    return [
      "observe confirmed EntityAttributes ownership or an attributed combat source relation before the selected status event",
      "observe the resolved owner Actor identity and stable player character ID before the selected status event",
    ];
  }
  if (resolutionClass === "owner_identity_unobserved") {
    return ["observe the resolved owner Actor identity and stable player character ID before the selected status event"];
  }
  if (resolutionClass === "owned_by_non_player") {
    return ["observe a complete packet-proven ancestry chain from the source to a stable player before the selected status event"];
  }
  if (resolutionClass === "missing_source") {
    return ["capture a non-null exact source actor and entity on the selected status event"];
  }
  return ["capture exact packet-proven player ownership at or before the selected status event"];
}

function compareGapGroups(left, right) {
  return String(left.rlog).localeCompare(String(right.rlog)) ||
    Number(left.run_ordinal) - Number(right.run_ordinal) ||
    Number(left.source_actor_id ?? -1) - Number(right.source_actor_id ?? -1) ||
    Number(left.source_entity_uuid ?? -1) - Number(right.source_entity_uuid ?? -1) ||
    String(left.resolution_class).localeCompare(String(right.resolution_class));
}

function groupBy(values, keyFn) {
  const grouped = new Map();
  for (const value of values) {
    const key = keyFn(value);
    if (!grouped.has(key)) grouped.set(key, []);
    grouped.get(key).push(value);
  }
  return grouped;
}

function countBy(values, keyFn, countFn) {
  const counts = {};
  for (const value of values) {
    const key = String(keyFn(value));
    counts[key] = (counts[key] ?? 0) + Number(countFn(value));
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)));
}

function normalizedCounts(value) {
  const result = {};
  for (const [key, count] of Object.entries(value).sort(([left], [right]) => left.localeCompare(right))) {
    result[key] = exactCount(count, `${key} count`);
  }
  return result;
}

function mergeCounts(values) {
  const result = {};
  for (const value of values) {
    for (const [key, count] of Object.entries(value)) result[key] = (result[key] ?? 0) + Number(count);
  }
  return Object.fromEntries(Object.entries(result).sort(([left], [right]) => left.localeCompare(right)));
}

function exactCount(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error(`${label} must be a non-negative safe integer`);
  return number;
}

function nullableInteger(value) {
  if (value === null || value === undefined) return null;
  const number = Number(value);
  if (!Number.isSafeInteger(number)) throw new Error("Expected a nullable safe integer");
  return number;
}

function sum(values, valueFn) {
  return values.reduce((total, value) => total + Number(valueFn(value)), 0);
}

function uniqueSorted(values) {
  return [...new Set(values.map(String))].sort((left, right) => left.localeCompare(right));
}

function fileDescriptor(input) {
  return {
    path: path.resolve(input).replaceAll("\\", "/"),
    bytes: statSync(input).size,
    sha256: sha256File(input),
  };
}

function sha256File(input) {
  return createHash("sha256").update(readFileSync(input)).digest("hex");
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return createHash("sha256").update(stableStringify(clone)).digest("hex");
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}

function readJson(input, label) {
  if (!existsSync(input)) throw new Error(`${label} is missing: ${input}`);
  return JSON.parse(readFileSync(input, "utf8"));
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${key ?? "end of command"}`);
    }
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  if (!value) throw new Error(`Missing --${key}`);
  return value;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-provider-ownership-gap-"));
  try {
    const proofPath = path.join(root, "provider.json");
    const outputPath = path.join(root, "worklist.json");
    writeFileSync(proofPath, `${JSON.stringify({
      schema_version: 3,
      tool: "rlogs-bpsr-status-effect-provider-ownership-proof",
      game_build: "1",
      policy: {
        scope: "provider_ownership_only",
        exact_numeric_effect_ids_authoritative: true,
        exact_input_build_authoritative: true,
        actor_kind_or_packet_proven_ancestry_required_for_player_ownership: true,
        future_actor_snapshots_may_backfill_prior_status_events: false,
        unknown_and_unresolved_events_preserved: true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      effects: [{
        effect_id: 9,
        status_events: 3,
        status_events_with_stable_player_character_id: 1,
        stable_player_character_id_proven_for_every_sourced_event: false,
      }],
      resolutions: [
        {
          rlog: "a.rlog", session_id: "a", run_ordinal: 1, effect_id: 9, class: "owned_by_player",
          source: { actor_id: 2, entity_uuid: 20 }, resolved_owner: { character_id: "7" }, status_events: 1,
        },
        {
          rlog: "a.rlog", session_id: "a", run_ordinal: 1, effect_id: 9, class: "non_player_unowned",
          source: { actor_id: 2, entity_uuid: 20 }, status_events: 2, status_state_counts: { applied: 2 },
          first_sequence: 10, last_sequence: 11, first_observed_micros: 100, last_observed_micros: 101,
          examples: [{ sequence: 10, observed_micros: 100, state: "applied", source_actor_id: 2,
            source_entity_uuid: 20, target_actor_id: 3, target_entity_uuid: 30 }],
        },
      ],
    }, null, 2)}\n`, "utf8");
    analyze({ build: "1", effect: 9, providerOwnershipProof: proofPath, output: outputPath });
    const report = verify(outputPath);
    if (report.summary.unresolved_status_events !== 2 ||
      report.summary.unresolved_events_with_same_source_separate_stable_player_resolution !== 2 ||
      report.gap_groups[0].future_evidence_backfill_allowed !== false) {
      throw new Error("Self-test did not preserve the unresolved same-source diagnostic hold");
    }
    console.log("bpsr-provider-ownership-gap-worklist self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
      "  node tools/bpsr-provider-ownership-gap-worklist.mjs analyze --build <id> --effect <id> --provider-ownership-proof <json> --output <json>\n" +
      "  node tools/bpsr-provider-ownership-gap-worklist.mjs verify --input <json>\n" +
      "  node tools/bpsr-provider-ownership-gap-worklist.mjs self-test",
  );
  process.exit(exitCode);
}
