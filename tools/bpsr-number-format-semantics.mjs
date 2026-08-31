#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    bytecode: path.resolve(required(parsed, "bytecode")),
    decompiledSource: path.resolve(required(parsed, "decompiled-source")),
    logicalPath: parsed["logical-path"] ?? "Luac/lua/utility/number_tools.lua",
    decompiler: parsed.decompiler ?? "unluac-rs",
    decompilerVersion: parsed["decompiler-version"] ?? "unrecorded",
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  requireFile(context.bytecode, "number_tools bytecode");
  requireFile(context.decompiledSource, "decompiled number_tools source");
  const source = readFileSync(context.decompiledSource, "utf8");
  const functions = ["MarkAndPercentFormat", "UnMarkAndPercentFormat"]
    .map((name) => extractPercentFunction(source, name));
  const divisors = [...new Set(functions.map((entry) => entry.raw_to_display_percent_divisor))];
  const exactCommonDivisor = divisors.length === 1 ? divisors[0] : null;
  const normalizationProven = exactCommonDivisor === 100
    && functions.every((entry) => entry.returns_percent_localization && entry.removes_trailing_zeros);
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-number-format-semantics.mjs",
    game_build: context.build,
    policy: {
      exact_current_build_client_code_required: true,
      display_normalization_does_not_prove_runtime_application_order: true,
      no_guessing: true,
    },
    source: {
      logical_path: context.logicalPath.replaceAll("\\", "/"),
      bytecode_bytes: readFileSync(context.bytecode).length,
      bytecode_sha256: sha256(readFileSync(context.bytecode)),
      decompiled_source_bytes: readFileSync(context.decompiledSource).length,
      decompiled_source_sha256: sha256(readFileSync(context.decompiledSource)),
      decompiler: context.decompiler,
      decompiler_version: context.decompilerVersion,
      lua_dialect: detectDialect(source),
    },
    percent_format_functions: functions,
    normalization: {
      proof_state: normalizationProven ? "current-build-exact-client-code" : "unresolved",
      semantics_proven: normalizationProven,
      raw_to_display_percent_divisor: exactCommonDivisor,
      raw_to_fractional_ratio_divisor: normalizationProven ? exactCommonDivisor * 100 : null,
      examples: normalizationProven ? [
        { raw: 520, display_percent: 5.2, fractional_ratio: 0.052 },
        { raw: 340, display_percent: 3.4, fractional_ratio: 0.034 },
      ] : [],
      proof_scope: "Decision.markpercent and Decision.unmarkpercent description rendering",
      runtime_formula_order_proven: false,
    },
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`);
  verify(context.output);
  console.log(`Number-format semantics built for ${context.build}: raw / ${exactCommonDivisor ?? "unresolved"} display-percent normalization.`);
}

function extractPercentFunction(source, name) {
  const marker = `function ret.${name}`;
  const start = source.indexOf(marker);
  if (start < 0) throw new Error(`Missing ${name} in decompiled source`);
  const nextFunction = source.indexOf("\nfunction ", start + marker.length);
  const block = source.slice(start, nextFunction < 0 ? source.length : nextFunction).trim();
  const divisorMatch = block.match(/\blocal\s+v\s*=\s*value\s*\/\s*(\d+(?:\.\d+)?)\b/);
  if (!divisorMatch) throw new Error(`${name} does not expose an exact value divisor`);
  const divisor = Number(divisorMatch[1]);
  if (!Number.isFinite(divisor) || divisor <= 0) throw new Error(`${name} has an invalid divisor`);
  return {
    name,
    raw_to_display_percent_divisor: divisor,
    removes_trailing_zeros: /ret\.removeTrailingZeros\(v\)/.test(block),
    returns_percent_localization: /Lang\("(?:PositivePercent|Percent)"/.test(block),
    exact_decompiled_function: block,
  };
}

function verify(input) {
  const report = readJson(input, "number-format semantics");
  if (report.schema_version !== 1) throw new Error("Number-format semantics schema_version must be 1");
  if (!/^\d+$/.test(String(report.game_build ?? ""))) throw new Error("Number-format semantics lacks an exact game build");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Number-format semantics content hash mismatch");
  if (report.policy?.no_guessing !== true) throw new Error("Number-format semantics must preserve no-guessing policy");
  if (report.normalization?.semantics_proven !== true) throw new Error("Percentage normalization is not proven");
  if (report.normalization?.proof_state !== "current-build-exact-client-code") throw new Error("Percentage normalization lacks exact client-code proof");
  if (report.normalization?.raw_to_display_percent_divisor !== 100) throw new Error("Unexpected display-percent divisor");
  if (report.normalization?.raw_to_fractional_ratio_divisor !== 10000) throw new Error("Unexpected fractional-ratio divisor");
  if (report.normalization?.runtime_formula_order_proven !== false) throw new Error("Display proof must not claim runtime formula order");
  const functions = report.percent_format_functions ?? [];
  for (const name of ["MarkAndPercentFormat", "UnMarkAndPercentFormat"]) {
    const entry = functions.find((candidate) => candidate.name === name);
    if (!entry || entry.raw_to_display_percent_divisor !== 100 || !entry.returns_percent_localization || !entry.removes_trailing_zeros) {
      throw new Error(`${name} lacks complete exact normalization evidence`);
    }
  }
  console.log(`Number-format semantics verified for build ${report.game_build}: raw 520 renders as 5.2%.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-number-format-test-"));
  try {
    const bytecode = path.join(root, "number_tools.lua");
    const decompiled = path.join(root, "number_tools.decompiled.lua");
    const output = path.join(root, "proof.json");
    writeFileSync(bytecode, Buffer.from([0x1b, 0x4c, 0x75, 0x61]));
    writeFileSync(decompiled, `-- dialect: lua5.3
function ret.MarkAndPercentFormat(value, notApplySymbol)
    local v = value / 100
    local str = ret.removeTrailingZeros(v)
    return Lang("Percent", { val = str })
end
function ret.UnMarkAndPercentFormat(value)
    local v = value / 100
    local str = ret.removeTrailingZeros(v)
    return Lang("Percent", { val = str })
end
`);
    build({ build: "1", bytecode, decompiledSource: decompiled, logicalPath: "fixture/number_tools.lua", decompiler: "fixture", decompilerVersion: "1", output });
    verify(output);
    console.log("Number-format semantics self-test passed.");
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function detectDialect(source) {
  return source.match(/^--\s*dialect:\s*(\S+)/m)?.[1] ?? null;
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return sha256(JSON.stringify(copy));
}

function sha256(value) { return createHash("sha256").update(value).digest("hex"); }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-number-format-semantics.mjs build --build <id> --bytecode <lua-bytecode> --decompiled-source <lua> --output <json> [--logical-path <path>] [--decompiler <name>] [--decompiler-version <version>]
  node tools/bpsr-number-format-semantics.mjs verify --input <json>
  node tools/bpsr-number-format-semantics.mjs self-test`);
  process.exit(exitCode);
}
