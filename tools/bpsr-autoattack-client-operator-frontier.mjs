import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-autoattack-client-operator-frontier.mjs";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function buildReport(options) {
  const gameBuild = required(options, "build");
  const identity = source(required(options, "identity"));
  const callsites = source(required(options, "callsite-audit"));
  const lua = source(required(options, "lua-audit"));
  const operator = source(required(options, "operator-frontier"));
  const clientJournalCensus = options["client-journal-census"]
    ? source(options["client-journal-census"])
    : null;
  const nativeDispatchOptions = [
    "getter-dispatch-shape",
    "getter-pointer-inventory",
    "getter-pointer-rip-audit",
  ];
  const suppliedNativeDispatchOptions = nativeDispatchOptions.filter((key) => options[key]);
  if (suppliedNativeDispatchOptions.length !== 0 &&
      suppliedNativeDispatchOptions.length !== nativeDispatchOptions.length) {
    fail(`native dispatch evidence requires all of: ${nativeDispatchOptions.join(", ")}`);
  }
  const getterDispatchShape = options["getter-dispatch-shape"]
    ? source(options["getter-dispatch-shape"])
    : null;
  const getterPointerInventory = options["getter-pointer-inventory"]
    ? source(options["getter-pointer-inventory"])
    : null;
  const getterPointerRipAudit = options["getter-pointer-rip-audit"]
    ? source(options["getter-pointer-rip-audit"])
    : null;
  const getterPointerTableContext = options["getter-pointer-table-context"]
    ? source(options["getter-pointer-table-context"])
    : null;
  if (getterPointerTableContext && !getterDispatchShape) {
    fail("getter pointer table context requires the complete native dispatch evidence group");
  }

  assert.equal(identity.value.game_build, gameBuild);
  assert.equal(callsites.value.schema_version, 3);
  assert.equal(callsites.value.game_build, gameBuild);
  assert.equal(callsites.value.binary?.byte_length, identity.value.game_assembly?.byte_length);
  assert.equal(callsites.value.binary?.sha256, identity.value.game_assembly?.sha256);
  assert.equal(callsites.value.summary?.selected_exact_target_rvas, 3);
  assert.equal(callsites.value.summary?.unique_target_rvas, 3);
  assert.equal(callsites.value.summary?.direct_callsites, 0);
  assert.equal(callsites.value.summary?.named_caller_callsites, 0);
  assert.equal(callsites.value.policy?.direct_call_match_is_exact, true);
  assert.equal(callsites.value.policy?.indirect_calls_are_not_claimed_absent, true);
  const targetNames = callsites.value.targets.flatMap((target) => target.names ?? []);
  assert.equal(targetNames.some((name) => name.includes("get_DamageScript")), true);
  assert.equal(targetNames.some((name) => name.includes("get_PVEDamageRadio")), true);
  assert.equal(targetNames.some((name) => name.includes("get_PVEFixedParameter")), true);

  assert.equal(lua.value.schema_version, 1);
  assert.equal(lua.value.generated_by, "tools/lua53-constant-audit.py");
  assert.equal(lua.value.summary?.files_scanned, 4_821);
  assert.equal(lua.value.summary?.parse_failures, 0);
  const requestedStrings = new Set(lua.value.targets?.strings ?? []);
  for (const target of [
    "pvedamageradio",
    "pvefixedparameter",
    "damagescript",
    "damageattrtable",
    "autoattack",
  ]) assert.equal(requestedStrings.has(target), true, `Lua audit omitted ${target}`);
  const matchedFiles = lua.value.files.map((entry) => entry.file.replaceAll("\\", "/"));
  assert.equal(matchedFiles.length, 2);
  assert.equal(matchedFiles.some((file) => file.endsWith("/lua/table/gen/DamageAttrTableMgr.lua")), true);
  assert.equal(matchedFiles.some((file) => file.endsWith("/lua/ui/view_model/skill_vm.lua")), true);
  const matchedStrings = new Set(lua.value.files.flatMap((entry) =>
    entry.matches.flatMap((match) => match.string_hits ?? [])));
  assert.deepEqual([...matchedStrings].sort(), ["damageattrtable", "pvedamageradio"]);

  assert.equal(operator.value.game_build, gameBuild);
  assert.equal(operator.value.identity?.ability_id, 2_900_840);
  assert.equal(operator.value.identity?.damage_script, "AutoAttack");
  assert.equal(operator.value.conclusion?.exact_autoattack_row_coefficient_plus_fixed_relation_proven,
    true);
  assert.equal(operator.value.conclusion?.exact_autoattack_stat_lane_proven, true);
  assert.equal(operator.value.conclusion?.exact_autoattack_operator_proven, false);
  assert.equal(operator.value.conclusion?.exact_integer_rounding_proven, false);

  if (clientJournalCensus) {
    assert.equal(clientJournalCensus.value.schema_version, 1);
    assert.equal(
      clientJournalCensus.value.generated_by,
      "tools/bpsr-autoattack-client-journal-census.mjs",
    );
    assert.equal(clientJournalCensus.value.game_build, gameBuild);
    assert.equal(clientJournalCensus.value.identity?.ability_id, 2_900_840);
    assert.equal(clientJournalCensus.value.summary?.parse_errors, 0);
    assert.ok(clientJournalCensus.value.summary?.journal_files > 0);
    assert.ok(clientJournalCensus.value.summary?.client_packet_records > 0);
    assert.equal(clientJournalCensus.value.summary?.client_payloads_with_ability_varint, 0);
    assert.equal(clientJournalCensus.value.summary?.ability_varint_occurrences, 0);
    assert.equal(
      clientJournalCensus.value.conclusion?.all_retained_client_packet_records_scanned,
      true,
    );
    assert.equal(
      clientJournalCensus.value.conclusion?.retained_raw_journal_frontier_exhausted,
      true,
    );
    assert.equal(clientJournalCensus.value.conclusion?.provider_rdps_credit_allowed, false);
  }

  if (getterDispatchShape) {
    assert.equal(
      getterDispatchShape.value.generated_by,
      "tools/il2cpp-getter-dispatch-shape-audit.py",
    );
    assert.equal(getterDispatchShape.value.game_build, gameBuild);
    assert.equal(getterDispatchShape.value.binary?.bytes, identity.value.game_assembly?.byte_length);
    assert.equal(getterDispatchShape.value.binary?.sha256, identity.value.game_assembly?.sha256);
    assert.equal(getterDispatchShape.value.getters?.length, 3);
    const columnOffsets = Object.fromEntries(getterDispatchShape.value.getters.map((getter) => [
      getter.label,
      getter.edx_immediates,
    ]));
    assert.deepEqual(columnOffsets.DamageScript, [24]);
    assert.deepEqual(columnOffsets.PVEDamageRadio, [28]);
    assert.deepEqual(columnOffsets.PVEFixedParameter, [32]);
    assert.equal(
      getterDispatchShape.value.policy?.immediate_edx_value_is_object_field_offset,
      false,
    );
    assert.equal(getterDispatchShape.value.policy?.getter_dispatch_is_formula_consumer, false);

    assert.equal(
      getterPointerInventory.value.generated_by,
      "tools/il2cpp-pointer-slot-inventory.py",
    );
    assert.equal(getterPointerInventory.value.game_build, gameBuild);
    assert.equal(getterPointerInventory.value.binary?.bytes, identity.value.game_assembly?.byte_length);
    assert.equal(getterPointerInventory.value.binary?.sha256, identity.value.game_assembly?.sha256);
    assert.equal(getterPointerInventory.value.summary?.targets, 3);
    assert.equal(
      getterPointerInventory.value.summary?.preferred_image_absolute_pointer_slots,
      3,
    );
    assert.equal(
      getterPointerInventory.value.summary?.targets_with_preferred_image_absolute_pointer_slots,
      3,
    );

    assert.equal(
      getterPointerRipAudit.value.generated_by,
      "tools/il2cpp-rip-relative-reference-audit.py",
    );
    assert.equal(getterPointerRipAudit.value.game_build, gameBuild);
    assert.equal(getterPointerRipAudit.value.binary?.bytes, identity.value.game_assembly?.byte_length);
    assert.equal(getterPointerRipAudit.value.binary?.sha256, identity.value.game_assembly?.sha256);
    assert.equal(getterPointerRipAudit.value.summary?.target_rvas, 3);
    assert.equal(getterPointerRipAudit.value.summary?.exact_rip_relative_references, 0);
    assert.equal(getterPointerRipAudit.value.summary?.target_rvas_with_references, 0);
    assert.equal(
      getterPointerRipAudit.value.policy?.absence_of_direct_rip_reference_proves_no_semantic_access,
      false,
    );

    if (getterPointerTableContext) {
      assert.equal(
        getterPointerTableContext.value.generated_by,
        "tools/il2cpp-method-pointer-table-context-audit.py",
      );
      assert.equal(getterPointerTableContext.value.game_build, gameBuild);
      assert.equal(
        getterPointerTableContext.value.binary?.bytes,
        identity.value.game_assembly?.byte_length,
      );
      assert.equal(
        getterPointerTableContext.value.binary?.sha256,
        identity.value.game_assembly?.sha256,
      );
      assert.equal(getterPointerTableContext.value.targets?.length, 3);
      assert.equal(getterPointerTableContext.value.sequence?.entries, 23);
      assert.equal(getterPointerTableContext.value.sequence?.matches?.length, 1);
      assert.equal(
        getterPointerTableContext.value.conclusion
          ?.selected_pointers_are_generic_method_registration_sequence,
        true,
      );
      assert.equal(
        getterPointerTableContext.value.conclusion
          ?.selected_pointer_sequence_is_combat_consumer_proof,
        false,
      );
    }
  }

  const report = {
    schema_version: getterPointerTableContext ? 4 : getterDispatchShape ? 3 : clientJournalCensus ? 2 : 1,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    identity: {
      ability_id: 2_900_840,
      damage_script: "AutoAttack",
      game_assembly_bytes: identity.value.game_assembly.byte_length,
      game_assembly_sha256: identity.value.game_assembly.sha256,
      metadata_bytes: identity.value.metadata.byte_length,
      metadata_sha256: identity.value.metadata.sha256,
    },
    policy: {
      exact_build_binary_and_metadata_are_authoritative: true,
      localized_formula_text_is_operator_authority: false,
      presentation_code_is_server_formula_authority: false,
      absence_of_direct_calls_proves_absence_of_indirect_calls: false,
      unresolved_native_consumers_are_preserved: true,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    sources: {
      current_build_identity: receipt(identity),
      exact_native_direct_callsite_audit: receipt(callsites),
      complete_lua_constant_audit: receipt(lua),
      one_skill_operator_frontier: receipt(operator),
      ...(clientJournalCensus
        ? { complete_retained_client_journal_census: receipt(clientJournalCensus) }
        : {}),
      ...(getterDispatchShape
        ? {
            exact_getter_dispatch_shape: receipt(getterDispatchShape),
            exact_getter_pointer_inventory: receipt(getterPointerInventory),
            exact_getter_pointer_rip_audit: receipt(getterPointerRipAudit),
            ...(getterPointerTableContext
              ? { exact_getter_pointer_table_context: receipt(getterPointerTableContext) }
              : {}),
          }
        : {}),
    },
    native_search: {
      method_index_entries: callsites.value.method_index.method_entries,
      getter_targets: targetNames,
      exact_direct_callsites: 0,
      indirect_calls_exhaustively_excluded: false,
      ...(getterDispatchShape ? {
        getter_table_column_offsets: Object.fromEntries(
          getterDispatchShape.value.getters.map((getter) => [
            getter.label,
            getter.edx_immediates[0],
          ]),
        ),
        getter_pointer_slots: getterPointerInventory.value.summary
          .preferred_image_absolute_pointer_slots,
        executable_instructions_decoded_for_pointer_slot_search:
          getterPointerRipAudit.value.summary.decoded_instruction_rows,
        exact_rip_relative_getter_pointer_slot_references:
          getterPointerRipAudit.value.summary.exact_rip_relative_references,
        pointer_slot_audit_peak_working_set_bytes:
          getterPointerRipAudit.value.resource_bounds.measured_process_peak_working_set_bytes,
        runtime_indexed_metadata_dispatch_exhaustively_excluded: false,
        ...(getterPointerTableContext ? {
          pointer_table_declaration_sequence_entries:
            getterPointerTableContext.value.sequence.entries,
          pointer_table_exact_sequence_matches:
            getterPointerTableContext.value.sequence.matches.length,
          generic_method_registration_sequence_proven: true,
          combat_specific_indexed_dispatch_proven: false,
        } : {}),
        result: getterPointerTableContext
          ? "the getter pointers are part of one exact generic dump-declaration registration sequence, not a combat-specific consumer; no direct calls or direct RIP-relative pointer-slot consumers exist and no combat-specific indexed dispatcher is proven"
          : "the getters are registered in a native method-pointer table but have no direct calls or direct RIP-relative pointer-slot consumers; runtime-indexed metadata dispatch remains unresolved",
      } : {
        result: "no direct native consumer of the exact coefficient or fixed-parameter getters was found",
      }),
    },
    lua_search: {
      files_scanned: lua.value.summary.files_scanned,
      parse_failures: lua.value.summary.parse_failures,
      files_with_matches: lua.value.summary.files_with_matches,
      functions_with_matches: lua.value.summary.functions_with_matches,
      matched_files: matchedFiles,
      matched_strings: [...matchedStrings].sort(),
      formula_operator_strings_without_matches: [
        "PVEFixedParameter",
        "DamageScript",
        "AutoAttack",
      ],
      result: "the only coefficient consumer is the previously reviewed skill-description presentation path; no Lua server-equivalent damage operator was found",
    },
    ...(clientJournalCensus ? {
      retained_client_journal_search: {
        journal_files: clientJournalCensus.value.summary.journal_files,
        nonempty_exact_build_journals:
          clientJournalCensus.value.summary.nonempty_exact_build_journals,
        total_bytes: clientJournalCensus.value.summary.total_bytes,
        total_lines: clientJournalCensus.value.summary.total_lines,
        client_packet_records: clientJournalCensus.value.summary.client_packet_records,
        client_application_bytes: clientJournalCensus.value.summary.client_application_bytes,
        client_payloads_with_ability_varint:
          clientJournalCensus.value.summary.client_payloads_with_ability_varint,
        ability_varint_occurrences:
          clientJournalCensus.value.summary.ability_varint_occurrences,
        peak_working_set_bytes: clientJournalCensus.value.summary.peak_working_set_bytes,
        result:
          "every retained exact-build client packet was scanned; no client application payload contains the numeric ability-2900840 varint",
      },
    } : {}),
    conclusion: {
      exact_static_coefficient_fixed_relation_proven: true,
      authoritative_client_damage_operator_found: false,
      authoritative_server_damage_operator_found: false,
      exact_integer_rounding_proven: false,
      further_broad_lua_formula_text_mining_warranted: false,
      ...(clientJournalCensus
        ? { retained_client_journal_frontier_exhausted: true }
        : {}),
      ...(getterDispatchShape ? {
        exact_getter_dispatch_bodies_audited: true,
        exact_getter_pointer_slots_found: true,
        exact_rip_relative_getter_pointer_slot_search_exhausted: true,
        runtime_indexed_metadata_dispatch_exhausted: false,
        ...(getterPointerTableContext ? {
          generic_method_registration_sequence_proven: true,
          combat_specific_indexed_dispatch_proven: false,
        } : {}),
      } : {}),
      smallest_next_proof: getterPointerTableContext
        ? "Acquire one controlled local ability-2900840 capture retaining the exact event-time Physical Attack transition and matching SyncDamageInfo response, or obtain authoritative server damage-operator code. The native getter pointers are generic IL2CPP registration entries rather than a combat-specific dispatch seed; direct calls, direct RIP-relative slot consumers, Lua operator constants, and every retained exact-build client packet have been exhausted. Remote-player cast packets are not required."
        : getterDispatchShape
        ? "Resolve one concrete runtime-indexed metadata dispatch seed for the DamageAttr getter pointer table and instruction-audit its caller, or acquire one controlled local ability-2900840 capture retaining the exact event-time Physical Attack transition and matching SyncDamageInfo response. Direct calls, direct RIP-relative slot consumers, Lua operator constants, and every retained exact-build client packet have been exhausted; remote-player cast packets are not required."
        : clientJournalCensus
        ? "Acquire one controlled local ability-2900840 capture retaining its client request, exact event-time Physical Attack transition, and matching SyncDamageInfo response, or identify and instruction-audit a new concrete indirect native consumer seed. The complete retained raw-journal frontier contains no client payload with ability 2900840; remote-player cast packets are not required."
        : "Capture a controlled same-hit ability-2900840 repeat across one exact current-Physical-Attack transition with identical downstream target and hit state, or identify and instruction-audit a new concrete indirect native consumer seed. Remote-player cast packets are not required.",
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
  return { ...report, content_sha256: contentSha256(report) };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  const report = buildReport(options);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  const report = JSON.parse(readFileSync(input, "utf8"));
  const rebuilt = buildReport({
    build: report.game_build,
    identity: report.sources.current_build_identity.path,
    "callsite-audit": report.sources.exact_native_direct_callsite_audit.path,
    "lua-audit": report.sources.complete_lua_constant_audit.path,
    "operator-frontier": report.sources.one_skill_operator_frontier.path,
    ...(report.sources.complete_retained_client_journal_census
      ? { "client-journal-census": report.sources.complete_retained_client_journal_census.path }
      : {}),
    ...(report.sources.exact_getter_dispatch_shape
      ? {
          "getter-dispatch-shape": report.sources.exact_getter_dispatch_shape.path,
          "getter-pointer-inventory": report.sources.exact_getter_pointer_inventory.path,
          "getter-pointer-rip-audit": report.sources.exact_getter_pointer_rip_audit.path,
          ...(report.sources.exact_getter_pointer_table_context
            ? {
                "getter-pointer-table-context":
                  report.sources.exact_getter_pointer_table_context.path,
              }
            : {}),
        }
      : {}),
  });
  assert.deepEqual(report, rebuilt);
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-autoattack-client-operator-frontier.mjs generate --build <id> --identity <json> --callsite-audit <json> --lua-audit <json> --operator-frontier <json> [--client-journal-census <json>] [--getter-dispatch-shape <json> --getter-pointer-inventory <json> --getter-pointer-rip-audit <json> [--getter-pointer-table-context <json>]] --output <json>\n  node tools/bpsr-autoattack-client-operator-frontier.mjs verify --input <json>");
  process.exit(1);
}
