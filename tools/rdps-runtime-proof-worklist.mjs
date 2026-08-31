#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const build = options.build || discoverLatestBuild();
const inventoryRoot = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  `steam-${build}`,
);
const manifestPath = resolvePath(
  options.manifest || path.join(inventoryRoot, "rdps-matching-build-validation-manifest.v1.json"),
);
const manifest = readJson(manifestPath, "matching-build validation manifest");
const reportSchema = Number(options.reportSchema || manifest.validation_report_schema);
if (!Number.isSafeInteger(reportSchema) || reportSchema <= 0) {
  throw new Error(
    "validation report schema is missing; regenerate the manifest or pass --reportSchema",
  );
}
const cumulativePath = resolvePath(
  options.cumulative
    || path.join(
      repoRoot,
      "runtime-data/research/rdps/live-validation",
      `steam-${build}.v${reportSchema}.cumulative.validation.json`,
    ),
);
const outputPath = resolvePath(
  options.output || path.join(inventoryRoot, "rdps-runtime-proof-worklist.v1.json"),
);

if (String(manifest.game_build) !== build) {
  throw new Error(`manifest build ${manifest.game_build} does not match requested build ${build}`);
}
const cumulative = existsSync(cumulativePath)
  ? readJson(cumulativePath, "cumulative validation report")
  : null;
if (cumulative && String(cumulative.manifest_game_build) !== build) {
  throw new Error(
    `cumulative validation build ${cumulative.manifest_game_build} does not match requested build ${build}`,
  );
}
if (cumulative?.report && Number(cumulative.report.schema_version) !== reportSchema) {
  throw new Error(
    `cumulative validation report schema ${cumulative.report.schema_version} does not match requested schema ${reportSchema}`,
  );
}
if (cumulative?.report) validateReportContract(cumulative.report, manifest, "cumulative");
const cumulativeSessionIds = new Set(cumulative?.session_ids || []);
const pendingSessionReports = discoverPendingSessionReports(
  path.dirname(cumulativePath),
  build,
  cumulativeSessionIds,
);
const evidenceReports = [
  ...(cumulative?.report ? [{ session_id: null, kind: "cumulative", path: cumulativePath, report: cumulative.report }] : []),
  ...pendingSessionReports,
];

const observedById = mergeObservedObligations(evidenceReports.map((entry) => entry.report));
const obligations = manifest.obligations.map((obligation) => {
  const observed = observedById.get(obligation.obligation_id);
  const observedEventKinds = observed?.observed_event_kinds || [];
  const missingEventKinds = obligation.required_event_kinds.filter(
    (eventKind) => !observedEventKinds.includes(eventKind),
  );
  const coverageState = observedEventKinds.length === 0
    ? "no-candidate-evidence"
    : missingEventKinds.length === 0
      ? "candidate-event-coverage-complete"
      : "partial-candidate-event-coverage";
  return {
    obligation_id: obligation.obligation_id,
    domain: obligation.domain,
    subject_kind: obligation.subject_kind,
    subject_id: obligation.subject_id,
    subject_name: obligation.subject_name,
    priority: priorityFor(obligation.domain),
    proof_state: obligation.proof_state,
    candidate_event_coverage: coverageState,
    proof_promoted: false,
    required_event_kinds: obligation.required_event_kinds,
    observed_event_kinds: observedEventKinds,
    missing_event_kinds: missingEventKinds,
    requirements: obligation.requirements,
    selectors: obligation.selectors,
    evidence: obligation.evidence,
  };
});

const cohortMap = new Map();
for (const obligation of obligations) {
  const selectorKey = stableJson(obligation.selectors);
  const key = selectorKey;
  let cohort = cohortMap.get(key);
  if (!cohort) {
    cohort = {
      cohort_id: `selector-${createHash("sha256").update(key).digest("hex").slice(0, 16)}`,
      priority: obligation.priority,
      selectors: obligation.selectors,
      required_event_kinds: new Set(),
      obligation_ids: [],
      domains: new Set(),
      subject_names: [],
      states: [],
    };
    cohortMap.set(key, cohort);
  }
  cohort.priority = Math.min(cohort.priority, obligation.priority);
  cohort.obligation_ids.push(obligation.obligation_id);
  cohort.domains.add(obligation.domain);
  cohort.subject_names.push(obligation.subject_name);
  cohort.states.push(obligation.candidate_event_coverage);
  for (const eventKind of obligation.required_event_kinds) {
    cohort.required_event_kinds.add(eventKind);
  }
}

const cohorts = [...cohortMap.values()].map((cohort) => ({
  cohort_id: cohort.cohort_id,
  priority: cohort.priority,
  candidate_event_coverage: cohortState(cohort.states),
  obligation_count: cohort.obligation_ids.length,
  obligation_ids: cohort.obligation_ids.sort(),
  domains: [...cohort.domains].sort(),
  subject_names: [...new Set(cohort.subject_names)].sort(),
  required_event_kinds: [...cohort.required_event_kinds].sort(),
  selectors: cohort.selectors,
})).sort((left, right) =>
  left.priority - right.priority
    || coverageRank(left.candidate_event_coverage) - coverageRank(right.candidate_event_coverage)
    || right.obligation_count - left.obligation_count
    || left.cohort_id.localeCompare(right.cohort_id)
);

const output = {
  schema_version: 1,
  generated_by: "tools/rdps-runtime-proof-worklist.mjs",
  game: manifest.game,
  game_build: build,
  policy: {
    exact_matching_build_evidence_required: true,
    candidate_event_coverage_is_not_formula_proof: true,
    proof_promotion_is_never_automatic: true,
    unresolved_evidence_hidden: false,
    guessed_relationships_allowed: false,
    immutable_session_reports_are_source_of_truth: true,
    validation_report_schema: reportSchema,
  },
  inputs: {
    manifest: artifactReference(manifestPath),
    cumulative_validation: existsSync(cumulativePath)
      ? artifactReference(cumulativePath)
      : null,
    pending_session_validation: pendingSessionReports.map((entry) => ({
      session_id: entry.session_id,
      kind: entry.kind,
      artifact: artifactReference(entry.path),
    })),
  },
  summary: {
    total_obligations: obligations.length,
    proof_promotions: evidenceReports.reduce(
      (total, entry) => total + Number(entry.report?.summary?.proof_promotions || 0),
      0,
    ),
    candidate_event_coverage: countBy(obligations, (row) => row.candidate_event_coverage),
    by_domain: domainSummary(obligations),
    selector_cohorts: cohorts.length,
    multi_obligation_selector_cohorts: cohorts.filter((cohort) => cohort.obligation_count > 1).length,
    highest_obligations_in_one_cohort: Math.max(...cohorts.map((cohort) => cohort.obligation_count)),
    captured_sessions: cumulativeSessionIds.size + pendingSessionReports.length,
    pending_session_reports: pendingSessionReports.length,
  },
  priority_order: [
    { priority: 1, domains: ["packet-output-route", "target-mitigation"] },
    { priority: 2, domains: ["offensive-runtime-gate"] },
    { priority: 3, domains: ["mastery-property"] },
    { priority: 4, domains: ["psychoscope-factor"] },
  ],
  cohorts,
  obligations,
};

writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`);
console.log(JSON.stringify({ output: artifactReference(outputPath), ...output.summary }, null, 2));

function discoverLatestBuild() {
  const root = path.join(
    repoRoot,
    "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  );
  const builds = readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^steam-\d+$/.test(entry.name))
    .map((entry) => entry.name.slice("steam-".length))
    .filter((candidate) => existsSync(path.join(
      root,
      `steam-${candidate}`,
      "rdps-matching-build-validation-manifest.v1.json",
    )))
    .sort((left, right) => Number(right) - Number(left));
  if (builds.length === 0) throw new Error("no generated rDPS validation manifest was found");
  return builds[0];
}

function discoverPendingSessionReports(directory, gameBuild, completedSessionIds) {
  if (!existsSync(directory)) return [];
  const finalSuffix = `-steam-${gameBuild}.v${reportSchema}.validation.json`;
  const checkpointSuffix = `-steam-${gameBuild}.v${reportSchema}.checkpoint.validation.json`;
  const bySession = new Map();
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const candidate = entry.name.endsWith(checkpointSuffix)
      ? { sessionId: entry.name.slice(0, -checkpointSuffix.length), kind: "checkpoint" }
      : entry.name.endsWith(finalSuffix)
        ? { sessionId: entry.name.slice(0, -finalSuffix.length), kind: "final" }
        : null;
    if (!candidate?.sessionId || completedSessionIds.has(candidate.sessionId)) continue;
    const existing = bySession.get(candidate.sessionId);
    if (existing && !(candidate.kind === "final" && existing.kind === "checkpoint")) continue;
    const reportPath = path.join(directory, entry.name);
    const report = readJson(reportPath, `${candidate.kind} validation report`);
    if (String(report.manifest_game_build) !== gameBuild) {
      throw new Error(
        `${candidate.kind} validation build ${report.manifest_game_build} does not match requested build ${gameBuild}`,
      );
    }
    if (Number(report.schema_version) !== reportSchema) {
      throw new Error(
        `${candidate.kind} validation report schema ${report.schema_version} does not match requested schema ${reportSchema}`,
      );
    }
    validateReportContract(report, manifest, `${candidate.kind} ${candidate.sessionId}`);
    bySession.set(candidate.sessionId, {
      session_id: candidate.sessionId,
      kind: candidate.kind,
      path: reportPath,
      report,
    });
  }
  return [...bySession.values()].sort((left, right) => left.session_id.localeCompare(right.session_id));
}

function mergeObservedObligations(reports) {
  const merged = new Map();
  for (const report of reports) {
    for (const obligation of report?.obligations || []) {
      const existing = merged.get(obligation.obligation_id);
      if (!existing) {
        merged.set(obligation.obligation_id, obligation);
        continue;
      }
      const coverageState = coverageRank(obligation.coverage_state)
        > coverageRank(existing.coverage_state)
        ? obligation.coverage_state
        : existing.coverage_state;
      const observedEventKinds = [...new Set([
        ...(existing.observed_event_kinds || []),
        ...(obligation.observed_event_kinds || []),
      ])].sort();
      merged.set(obligation.obligation_id, {
        ...existing,
        coverage_state: coverageState,
        observed_event_kinds: observedEventKinds,
      });
    }
  }
  return merged;
}

function validateReportContract(report, manifest, label) {
  const manifestById = new Map(
    (manifest.obligations || []).map((obligation) => [obligation.obligation_id, obligation]),
  );
  if ((report.obligations || []).length !== manifestById.size) {
    throw new Error(
      `${label} validation obligation count ${(report.obligations || []).length} does not match manifest ${manifestById.size}`,
    );
  }
  for (const observed of report.obligations || []) {
    const expected = manifestById.get(observed.obligation_id);
    if (!expected) {
      throw new Error(`${label} validation contains unknown obligation ${observed.obligation_id}`);
    }
    let observedSelectors;
    try {
      observedSelectors = JSON.parse(observed.selector_contract);
    } catch {
      throw new Error(`${label} validation obligation ${observed.obligation_id} has an invalid selector contract`);
    }
    if (stableJson(observedSelectors) !== stableJson(expected.selectors || {})) {
      throw new Error(
        `${label} validation obligation ${observed.obligation_id} selector contract differs from the manifest`,
      );
    }
  }
}

function priorityFor(domain) {
  if (domain === "packet-output-route" || domain === "target-mitigation") return 1;
  if (domain === "offensive-runtime-gate") return 2;
  if (domain === "mastery-property") return 3;
  if (domain === "psychoscope-factor") return 4;
  throw new Error(`unknown runtime proof domain ${domain}`);
}

function cohortState(states) {
  if (states.every((state) => state === "candidate-event-coverage-complete")) {
    return "candidate-event-coverage-complete";
  }
  if (states.some((state) => state !== "no-candidate-evidence")) {
    return "partial-candidate-event-coverage";
  }
  return "no-candidate-evidence";
}

function coverageRank(state) {
  return state === "no-candidate-evidence" ? 0
    : state === "partial-candidate-event-coverage" ? 1
      : 2;
}

function domainSummary(rows) {
  const output = {};
  for (const row of rows) {
    output[row.domain] ||= {
      total: 0,
      no_candidate_evidence: 0,
      partial_candidate_event_coverage: 0,
      candidate_event_coverage_complete: 0,
      proof_promotions: 0,
    };
    const summary = output[row.domain];
    summary.total += 1;
    summary[row.candidate_event_coverage.replaceAll("-", "_")] += 1;
  }
  return output;
}

function countBy(rows, selector) {
  const output = {};
  for (const row of rows) {
    const key = selector(row);
    output[key] = (output[key] || 0) + 1;
  }
  return output;
}

function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const key = args[index];
    if (!key.startsWith("--")) throw new Error(`unexpected argument ${key}`);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${key}`);
    output[key.slice(2)] = value;
    index += 1;
  }
  return output;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`could not read ${label} ${filePath}: ${error.message}`);
  }
}

function artifactReference(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
