#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const GENERATED_BY = "tools/bpsr-harmony-grace-capture-refresh.mjs";
const SCHEMA_VERSION = 1;
const EFFECT_ID = 3_003_052;
const ABILITY_ID = 2_352;
const MAX_SEQUENCE = "18446744073709551615";
const DEFAULT_MEMORY_LIMIT_MIB = 512;
const MAX_MEMORY_LIMIT_MIB = 36 * 1024;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "run") run(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function run(values) {
  const build = required(values, "build");
  assert.match(build, /^\d+$/);
  const rlog = requireFile(required(values, "rlog"));
  const protocolStatus = requireFile(required(values, "protocol-status"));
  const segmentReport = requireFile(required(values, "segment-report"));
  const binDir = path.resolve(required(values, "bin-dir"));
  const outputDir = path.resolve(required(values, "output-dir"));
  const memoryLimitMib = integerOption(
    values.get("memory-limit-mib") ?? String(DEFAULT_MEMORY_LIMIT_MIB),
    "memory-limit-mib",
    1,
    MAX_MEMORY_LIMIT_MIB,
  );
  const resume = booleanOption(values.get("resume") ?? "false", "resume");
  const executables = {
    replayAudit: requireFile(path.join(binDir, executable("rlogs-bpsr-rdps-replay-audit"))),
    eventSlice: requireFile(path.join(binDir, executable("rlogs-bpsr-event-slice"))),
    cohort: requireFile(path.join(binDir, executable("rlogs-bpsr-state-scaling-damage-proof"))),
    counterfactual: requireFile(path.join(
      binDir,
      executable("rlogs-bpsr-status-effect-counterfactual-proof"),
    )),
  };
  const tools = {
    trace: requireFile(path.resolve("tools/bpsr-harmony-grace-single-effect-trace.mjs")),
    boundary: requireFile(path.resolve("tools/bpsr-harmony-grace-capture-boundary.mjs")),
    transition: requireFile(path.resolve("tools/bpsr-harmony-grace-lifecycle-transition-proof.mjs")),
    acquisition: requireFile(path.resolve("tools/bpsr-harmony-grace-final-integer-acquisition.mjs")),
  };

  if (fs.existsSync(outputDir) && fs.readdirSync(outputDir).length > 0 && !resume) {
    throw new Error(`refusing nonempty output directory: ${outputDir}`);
  }
  fs.mkdirSync(outputDir, { recursive: true });
  const outputs = outputPaths(outputDir);
  if (fs.existsSync(outputs.manifest)) {
    throw new Error(`refresh is already complete; verify ${outputs.manifest}`);
  }
  if (!resume) {
    for (const output of Object.values(outputs)) refuseExisting(output);
  }
  const commands = [];
  const rssSamples = [];
  const execute = (label, program, args) => {
    const outputFlag = args.includes("--output")
      ? "--output"
      : args.includes("--formula-cohort-output")
        ? "--formula-cohort-output"
        : null;
    const expectedOutput = outputFlag == null ? null : args[args.indexOf(outputFlag) + 1];
    if (resume && expectedOutput && fs.existsSync(expectedOutput)) {
      commands.push({
        label,
        program,
        args,
        skipped_existing_output: true,
        existing_output: fileReceipt(expectedOutput),
      });
      rssSamples.push({ stage: `${label}-resumed`, rss_bytes: process.memoryUsage().rss });
      return;
    }
    const startedAt = new Date().toISOString();
    const started = process.hrtime.bigint();
    const result = spawnSync(program, args, {
      cwd: process.cwd(),
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    const elapsedMillis = Number((process.hrtime.bigint() - started) / 1_000_000n);
    const row = {
      label,
      program,
      args,
      started_at: startedAt,
      elapsed_millis: elapsedMillis,
      exit_code: result.status,
      stdout_tail: tail(result.stdout, 4_000),
      stderr_tail: tail(result.stderr, 4_000),
    };
    commands.push(row);
    rssSamples.push({ stage: label, rss_bytes: process.memoryUsage().rss });
    if (result.error) throw result.error;
    if (result.status !== 0) {
      throw new Error(
        `${label} failed with exit ${result.status}\n${row.stderr_tail}\n${row.stdout_tail}`,
      );
    }
  };

  execute("replay-audit", executables.replayAudit, [
    "--rlog", rlog,
    "--audit-enable-harmony-grace-candidate",
    "--output", outputs.audit,
  ]);
  execute("effect-lifecycle-slice", executables.eventSlice, [
    "--rlog", rlog,
    "--output", outputs.lifecycleEvents,
    "--first", "0",
    "--last", MAX_SEQUENCE,
    "--effect-id", String(EFFECT_ID),
  ]);
  execute("single-effect-trace", process.execPath, [
    tools.trace,
    "generate",
    "--audit", outputs.audit,
    "--lifecycle-events", outputs.lifecycleEvents,
    "--output", outputs.trace,
  ]);
  execute("capture-boundary", process.execPath, [
    tools.boundary,
    "generate",
    "--build", build,
    "--trace", outputs.trace,
    "--audit", outputs.audit,
    "--protocol-status", protocolStatus,
    "--segment-report", segmentReport,
    "--output", outputs.boundary,
  ]);
  execute("ability-cohort", executables.cohort, [
    "--rlog", rlog,
    "--ability", String(ABILITY_ID),
    "--proof-only",
    "--formula-cohort-output", outputs.cohort,
  ]);
  execute("bounded-counterfactual", executables.counterfactual, [
    "--cohort", outputs.cohort,
    "--output", outputs.counterfactual,
    "--effect", String(EFFECT_ID),
    "--memory-limit-mib", String(memoryLimitMib),
    "--source-transition-attribute", "11030",
    "--source-transition-attribute", "11031",
    "--source-transition-attribute", "11034",
    "--cross-entity-formula-state-diagnostic", "true",
  ]);

  const trace = readJson(outputs.trace);
  const providers = unique(trace.traces.map((row) => String(row.provider_actor_id)));
  const recipients = unique(trace.traces.map((row) => String(row.recipient_actor_id)));
  if (providers.length !== 1 || recipients.length !== 1) {
    throw new Error(
      `capture has ${providers.length} providers and ${recipients.length} recipients; ` +
      "one acquisition refresh must isolate one exact provider-recipient pair",
    );
  }
  const provider = providers[0];
  const recipient = recipients[0];
  execute("provider-event-slice", executables.eventSlice, [
    "--rlog", rlog,
    "--output", outputs.providerEvents,
    "--first", "0",
    "--last", MAX_SEQUENCE,
    "--actor-id", provider,
  ]);
  execute("recipient-event-slice", executables.eventSlice, [
    "--rlog", rlog,
    "--output", outputs.recipientEvents,
    "--first", "0",
    "--last", MAX_SEQUENCE,
    "--actor-id", recipient,
  ]);
  execute("lifecycle-transition-proof", process.execPath, [
    tools.transition,
    "build",
    "--build", build,
    "--provider", provider,
    "--recipient", recipient,
    "--events", outputs.providerEvents,
    "--events", outputs.recipientEvents,
    "--audit", outputs.audit,
    "--trace", outputs.trace,
    "--output", outputs.transitionProof,
  ]);
  execute("final-integer-acquisition", process.execPath, [
    tools.acquisition,
    "generate",
    "--boundary", outputs.boundary,
    "--trace", outputs.trace,
    "--cohort", outputs.cohort,
    "--transition-proof", outputs.transitionProof,
    "--counterfactual", outputs.counterfactual,
    "--output", outputs.acquisition,
  ]);

  const acquisition = readJson(outputs.acquisition);
  const counterfactual = readJson(outputs.counterfactual);
  assert.equal(counterfactual.processing.measured_peak_within_configured_limit, true);
  assert.equal(acquisition.resource_bounds.sampled_rss_within_configured_ceiling, true);
  const manifest = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    effect_id: EFFECT_ID,
    ability_id: ABILITY_ID,
    inputs: {
      sealed_rlog: fileReceipt(rlog),
      protocol_status: fileReceipt(protocolStatus),
      run_segmentation: fileReceipt(segmentReport),
    },
    executables: Object.fromEntries(
      Object.entries(executables).map(([key, value]) => [key, fileReceipt(value)]),
    ),
    tools: Object.fromEntries(
      Object.entries(tools).map(([key, value]) => [key, fileReceipt(value)]),
    ),
    outputs: Object.fromEntries(
      Object.entries(outputs)
        .filter(([key]) => key !== "manifest")
        .map(([key, value]) => [key, fileReceipt(value)]),
    ),
    commands,
    identity: {
      provider_actor_id: provider,
      recipient_actor_id: recipient,
      session_id: trace.session_id,
      protocol_pack_digest: trace.protocol_pack_digest,
    },
    resource_bounds: {
      counterfactual_memory_limit_mib: memoryLimitMib,
      counterfactual_measured_peak_working_set_bytes:
        counterfactual.processing.measured_peak_working_set_bytes,
      acquisition_maximum_sampled_rss_bytes:
        acquisition.resource_bounds.maximum_sampled_rss_bytes,
      configured_ram_ceiling_bytes: MAX_MEMORY_LIMIT_MIB * 1024 ** 2,
      orchestrator_rss_samples: rssSamples,
      all_reported_bounds_within_configured_limits: true,
    },
    result: {
      exact_aba_groups: acquisition.current_exact_aba_search.qualifying_groups,
      selected_final_server_integer_boundary:
        acquisition.current_exact_aba_search.selected_final_server_integer_boundary,
      exact_final_server_integer_counterfactual_proven:
        acquisition.current_exact_aba_search.exact_final_server_integer_counterfactual_proven,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    policy: {
      immutable_outputs: true,
      partial_refresh_resume_enabled: true,
      exact_numeric_ids_build_and_protocol_pack_are_authoritative: true,
      unknown_and_unresolved_events_preserved: true,
      remote_player_cast_packets_required: false,
      ordinary_damage_mutation_allowed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
  };
  manifest.content_sha256 = contentHash(manifest);
  fs.writeFileSync(outputs.manifest, `${JSON.stringify(manifest, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify({
    manifest: outputs.manifest,
    commands: commands.length,
    provider,
    recipient,
    exact_aba_groups: manifest.result.exact_aba_groups,
    selected_boundary: manifest.result.selected_final_server_integer_boundary,
    exact_final_integer_proven:
      manifest.result.exact_final_server_integer_counterfactual_proven,
    counterfactual_peak_bytes:
      manifest.resource_bounds.counterfactual_measured_peak_working_set_bytes,
    acquisition_sampled_rss_bytes:
      manifest.resource_bounds.acquisition_maximum_sampled_rss_bytes,
    runtime_promotion_allowed: false,
  }, null, 2)}\n`);
}

function verify(values) {
  const manifestPath = path.resolve(required(values, "input"));
  const manifest = readJson(manifestPath);
  assert.equal(manifest.schema_version, SCHEMA_VERSION);
  assert.equal(manifest.generated_by, GENERATED_BY);
  assert.equal(Number(manifest.effect_id), EFFECT_ID);
  assert.equal(Number(manifest.ability_id), ABILITY_ID);
  assert.equal(manifest.policy.immutable_outputs, true);
  assert.equal(manifest.policy.remote_player_cast_packets_required, false);
  assert.equal(manifest.result.provider_rdps_credit_allowed, false);
  assert.equal(manifest.result.runtime_promotion_allowed, false);
  assert.equal(manifest.result.ui_display_allowed, false);
  for (const group of [manifest.inputs, manifest.executables, manifest.tools, manifest.outputs]) {
    for (const receipt of Object.values(group)) verifyReceipt(receipt);
  }
  assert.equal(manifest.content_sha256, contentHash(manifest));

  const runVerifier = (toolReceipt, inputReceipt) => {
    const result = spawnSync(process.execPath, [toolReceipt.path, "verify", "--input", inputReceipt.path], {
      cwd: process.cwd(),
      encoding: "utf8",
      windowsHide: true,
    });
    if (result.status !== 0) {
      throw new Error(`verification failed for ${inputReceipt.path}\n${result.stderr}`);
    }
  };
  runVerifier(manifest.tools.trace, manifest.outputs.trace);
  runVerifier(manifest.tools.boundary, manifest.outputs.boundary);
  runVerifier(manifest.tools.transition, manifest.outputs.transitionProof);
  runVerifier(manifest.tools.acquisition, manifest.outputs.acquisition);
  const counterfactual = readJson(manifest.outputs.counterfactual.path);
  assert.equal(counterfactual.processing.measured_peak_within_configured_limit, true);
  const acquisition = readJson(manifest.outputs.acquisition.path);
  assert.equal(acquisition.resource_bounds.sampled_rss_within_configured_ceiling, true);
  assert.equal(
    manifest.result.exact_final_server_integer_counterfactual_proven,
    acquisition.current_exact_aba_search.exact_final_server_integer_counterfactual_proven,
  );
  process.stdout.write(`${JSON.stringify({
    manifest: manifestPath,
    outputs_verified: Object.keys(manifest.outputs).length,
    exact_final_integer_proven:
      manifest.result.exact_final_server_integer_counterfactual_proven,
    runtime_promotion_allowed: manifest.result.runtime_promotion_allowed,
  }, null, 2)}\n`);
}

function outputPaths(outputDir) {
  return {
    audit: path.join(outputDir, "01-harmony-replay-audit.json"),
    lifecycleEvents: path.join(outputDir, "02-effect-3003052-lifecycle-events.jsonl"),
    trace: path.join(outputDir, "03-harmony-single-effect-trace.json"),
    boundary: path.join(outputDir, "04-harmony-capture-boundary.json"),
    cohort: path.join(outputDir, "05-ability-2352-formula-cohort.json"),
    counterfactual: path.join(outputDir, "06-harmony-counterfactual.json"),
    providerEvents: path.join(outputDir, "07-provider-events.jsonl"),
    recipientEvents: path.join(outputDir, "08-recipient-events.jsonl"),
    transitionProof: path.join(outputDir, "09-harmony-transition-proof.json"),
    acquisition: path.join(outputDir, "10-harmony-final-integer-acquisition.json"),
    manifest: path.join(outputDir, "capture-refresh-manifest.schema1.json"),
  };
}

function executable(name) {
  return process.platform === "win32" ? `${name}.exe` : name;
}

function fileReceipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: path.resolve(filePath),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function verifyReceipt(receipt) {
  assert.deepEqual(fileReceipt(receipt.path), receipt);
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      (key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function requireFile(filePath) {
  const resolved = path.resolve(filePath);
  if (!fs.statSync(resolved).isFile()) throw new Error(`required file is not a file: ${resolved}`);
  return resolved;
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath)) throw new Error(`refusing to overwrite ${filePath}`);
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function integerOption(value, label, minimum, maximum) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < minimum || parsed > maximum) {
    throw new Error(`--${label} must be an integer from ${minimum} through ${maximum}`);
  }
  return parsed;
}

function booleanOption(value, label) {
  if (value === "true") return true;
  if (value === "false") return false;
  throw new Error(`--${label} must be true or false`);
}

function tail(value, limit) {
  const text = String(value ?? "");
  return text.length <= limit ? text : text.slice(text.length - limit);
}

function unique(values) {
  return [...new Set(values)].sort();
}

function parseArgs(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value === undefined) usage(1);
    parsed.set(flag.slice(2), value);
  }
  return parsed;
}

function selfTest() {
  assert.equal(integerOption("512", "memory-limit-mib", 1, MAX_MEMORY_LIMIT_MIB), 512);
  assert.equal(booleanOption("true", "resume"), true);
  assert.deepEqual(unique(["2", "1", "2"]), ["1", "2"]);
  assert.equal(outputPaths("x").manifest, path.join("x", "capture-refresh-manifest.schema1.json"));
  process.stdout.write("self-test passed\n");
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
      "  node tools/bpsr-harmony-grace-capture-refresh.mjs run --build ID --rlog FILE --protocol-status FILE --segment-report FILE --bin-dir DIR --output-dir DIR [--memory-limit-mib N] [--resume true|false]\n" +
      "  node tools/bpsr-harmony-grace-capture-refresh.mjs verify --input MANIFEST\n" +
      "  node tools/bpsr-harmony-grace-capture-refresh.mjs self-test\n",
  );
  process.exit(exitCode);
}
