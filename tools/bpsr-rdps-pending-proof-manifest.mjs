import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

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

function loadReports(options, gameBuild, proofRoot) {
  const reportsDirectory = options["reports-dir"];
  if (!reportsDirectory) return new Map();
  const absoluteDirectory = path.resolve(reportsDirectory);
  const reports = new Map();
  for (const name of readdirSync(absoluteDirectory).filter((entry) => entry.endsWith(".json")).sort()) {
    const absolute = path.join(absoluteDirectory, name);
    const bytes = readFileSync(absolute);
    const report = JSON.parse(bytes.toString("utf8"));
    assert.equal(report.schema_version, 1, `${name} schema`);
    assert.equal(report.game_build, gameBuild, `${name} build`);
    assert.equal(report.conclusion?.suite_status, "passed", `${name} status`);
    assert.equal(report.conclusion?.exact_party_conservation, true, `${name} conservation`);
    assert.ok(report.conclusion?.observed_event_count > 0, `${name} observed events`);
    assert.equal(reports.has(report.suite_id), false, `duplicate suite ${report.suite_id}`);
    reports.set(report.suite_id, {
      report_path: path.relative(proofRoot, absolute).replaceAll("\\", "/"),
      report_sha256: sha256(bytes),
      observed_event_count: report.conclusion.observed_event_count,
    });
  }
  return reports;
}

function buildManifest(diff, reports) {
  assert.equal(diff.schema_version, 1);
  assert.equal(diff.requires_reproof, true);
  assert.equal(diff.runtime_promotion_allowed, false);
  assert.ok(Array.isArray(diff.required_proof_suites) && diff.required_proof_suites.length > 0);
  assert.deepEqual([...diff.required_proof_suites].sort(), diff.required_proof_suites);

  for (const id of reports.keys()) {
    assert.ok(diff.required_proof_suites.includes(id), `report for non-required suite ${id}`);
  }

  return {
    schema_version: 1,
    game_build: diff.candidate_build,
    review_state: "pending",
    canonical_events_retained: true,
    unresolved_events_hidden: false,
    suites: diff.required_proof_suites.map((id) => {
      const report = reports.get(id);
      return report ? {
        id,
        status: "passed",
        exact_party_conservation: true,
        observed_event_count: report.observed_event_count,
        report_path: report.report_path,
        report_sha256: report.report_sha256,
      } : {
        id,
        status: "pending",
        exact_party_conservation: false,
        observed_event_count: 0,
        report_path: "",
        report_sha256: "",
      };
    }),
  };
}

function loadDiff(options) {
  return JSON.parse(readFileSync(path.resolve(required(options, "diff")), "utf8"));
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  const diff = loadDiff(options);
  const reports = loadReports(options, diff.candidate_build, path.dirname(output));
  writeFileSync(output, `${JSON.stringify(buildManifest(diff, reports), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  const diff = loadDiff(options);
  const reports = loadReports(options, diff.candidate_build, path.dirname(input));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildManifest(diff, reports));
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-rdps-pending-proof-manifest.mjs generate --diff <json> --reports-dir <dir> --output <json>\n  node tools/bpsr-rdps-pending-proof-manifest.mjs verify --diff <json> --reports-dir <dir> --input <json>");
  process.exit(1);
}
