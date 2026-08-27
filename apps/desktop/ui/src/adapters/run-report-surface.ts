import type { MountedSurface } from "../shell/types";
import {
  type RunAnalysis,
  type RunEncounter,
  type RunReport,
  type RunSegment,
  dispositionCopy,
  findingCopy,
  formatDurationMicros,
  formatRunDifficulty,
} from "./run-report";

export function mountRunReportSurface(
  container: HTMLElement,
  loadReport: () => Promise<RunReport>,
): MountedSurface {
  let alive = true;
  let busy = false;
  const root = document.createElement("div");
  root.className = "plugin-surface run-report-surface";
  const heading = actionCard(
    "Segmented Run Report",
    "A read-only capture-time projection retained with the sealed canonical log. Times, attempts, evidence findings, and eligibility are reducer output; full submission validation remains a separate background task.",
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const refresh = button("Refresh history", "primary-button");
  const message = text(
    "span",
    "Waiting for a completed canonical log.",
    "runtime-action-message",
  );
  actions.append(refresh, message);
  heading.append(actions);
  const content = document.createElement("div");
  content.className = "run-report-content";
  content.append(
    text(
      "p",
      "Run the safe reference replay or complete a capture to create a local report.",
      "runtime-empty-result",
    ),
  );
  root.append(heading, content);
  container.append(root);

  const load = async () => {
    if (busy) return;
    busy = true;
    refresh.disabled = true;
    message.classList.remove("error");
    message.textContent = "Loading the stored capture-time projection...";
    try {
      const report = await loadReport();
      if (!alive) return;
      renderReport(content, report);
      message.textContent =
        `${report.projection.runs.length.toLocaleString()} projected run(s) · ` +
        (report.integrityVerified
          ? "submission artifact validated"
          : "full submission validation deferred");
    } catch (error) {
      if (!alive) return;
      message.textContent = errorMessage(error);
      message.classList.add("error");
      content.replaceChildren(
        text(
          "p",
          "No report is available yet. Complete a capture or run the safe reference replay first.",
          "runtime-empty-result",
        ),
      );
    } finally {
      busy = false;
      refresh.disabled = false;
    }
  };
  refresh.addEventListener("click", () => void load());
  void load();

  return {
    dispose() {
      alive = false;
    },
  };
}

function renderReport(container: HTMLElement, report: RunReport): void {
  const metadata = document.createElement("section");
  metadata.className = "content-card run-report-metadata";
  metadata.append(
    fileRow("Session", report.projection.session_id),
    fileRow(
      "Deployment / region / world",
      [
        report.projection.deployment_id,
        report.projection.region_id,
        report.projection.world_id,
      ]
        .filter((value): value is string => value !== null && value !== "")
        .join(" / "),
    ),
    fileRow("Client build", report.projection.client_build),
    fileRow("Protocol pack", report.projection.protocol_pack_digest),
    fileRow("Canonical seal", report.artifactDigest),
    fileRow("Local source", report.sourceRlog),
  );

  const summary = metricGrid([
    [report.projection.runs.length.toLocaleString(), "Projected runs"],
    [report.replayMetrics.events_seen.toLocaleString(), "Canonical events"],
    [
      report.replayMetrics.events_delivered.toLocaleString(),
      "Run evidence events",
    ],
    [
      formatDurationMicros(report.replayMetrics.wall_elapsed_micros),
      report.integrityVerified ? "Validation time" : "Projection time",
    ],
  ]);

  if (report.projection.runs.length === 0) {
    container.replaceChildren(
      metadata,
      summary,
      text(
        "p",
        "The sealed log is valid, but it contains no bounded dungeon or raid run. This is expected for the safe combat fixture and world-load-only captures.",
        "runtime-empty-result",
      ),
      exactJson(report),
    );
    return;
  }

  const runs = document.createElement("div");
  runs.className = "run-report-list";
  report.projection.runs.forEach((run, index) => {
    runs.append(renderRun(run, index));
  });
  container.replaceChildren(metadata, summary, runs, exactJson(report));
}

function renderRun(run: RunAnalysis, runIndex: number): HTMLElement {
  const card = document.createElement("article");
  card.className = "content-card run-report-card";
  card.dataset.disposition = run.submission_disposition;
  const header = document.createElement("header");
  const copy = document.createElement("div");
  const rawActivityId =
    run.identity.activity_id ??
    run.identity.observed_dungeon_id ??
    "unresolved-activity";
  copy.append(
    text(
      "span",
      `${formatIdentifier(run.identity.activity_kind)} · Run ${runIndex + 1}`,
      "run-report-kicker",
    ),
    text("h2", rawActivityId),
    text(
      "p",
      [
        `terminal ${formatIdentifier(run.terminal_state)}`,
        `instance ${run.identity.instance_id ?? "unresolved"}`,
        `difficulty ${formatRunDifficulty(run.identity)}`,
      ].join(" · "),
    ),
  );
  const disposition = dispositionCopy(run.submission_disposition);
  const state = text("span", disposition.label, "state-pill run-disposition");
  state.dataset.state = run.submission_disposition;
  state.title = disposition.detail;
  header.append(copy, state);

  const totalAttempts = run.segments.reduce(
    (total, segment) => total + segment.attempt_count,
    0,
  );
  const totalRetries = run.segments.reduce(
    (total, segment) => total + segment.retry_count,
    0,
  );
  const metrics = metricGrid([
    [formatDurationMicros(run.timing.wall_time_micros), "Run wall time"],
    [formatDurationMicros(run.timing.active_combat_micros), "Active combat"],
    [formatDurationMicros(run.timing.noncombat_micros), "Outside combat"],
    [totalAttempts.toLocaleString(), "Bounded pulls"],
    [totalRetries.toLocaleString(), "Retries / repulls"],
    [formatDurationMicros(run.timing.manual_pause_micros), "Recorder pauses"],
  ]);
  metrics.classList.add("run-report-run-metrics");

  const identity = document.createElement("section");
  identity.className = "run-report-identity";
  identity.append(
    fileRow("Activity kind", formatIdentifier(run.identity.activity_kind)),
    fileRow("Activity ID", run.identity.activity_id ?? "Unresolved"),
    fileRow("Activity family", run.identity.activity_family_id ?? "Unresolved"),
    fileRow(
      "Scene ID",
      run.identity.scene_id === null ? "Unresolved" : String(run.identity.scene_id),
    ),
    fileRow(
      "Observed dungeon ID",
      run.identity.observed_dungeon_id ?? "Unresolved",
    ),
    fileRow("Instance UUID", run.identity.instance_id ?? "Unresolved"),
    fileRow("Difficulty", formatRunDifficulty(run.identity)),
    fileRow("Wire difficulty ID", run.identity.difficulty_id ?? "Unresolved"),
    fileRow(
      "Encounter ruleset",
      run.encounter_ruleset_id === null
        ? "Unresolved"
        : `${run.encounter_ruleset_id} v${run.encounter_ruleset_version ?? "?"}`,
    ),
    fileRow("Route ID", run.identity.route_id ?? "Not applicable / unresolved"),
    fileRow(
      "Authoritative start",
      run.authoritative_start ? "Yes" : "No",
    ),
    fileRow(
      "Authoritative completion",
      run.authoritative_completion ? "Yes" : "No",
    ),
  );

  const evidence = document.createElement("section");
  evidence.className = "run-report-evidence";
  const evidenceTitle = text("h3", "Leaderboard evidence");
  const evidenceDetail = text("p", disposition.detail);
  const findings = document.createElement("ul");
  if (run.findings.length === 0) {
    findings.append(text("li", "No review findings."));
  } else {
    for (const finding of run.findings) {
      findings.append(text("li", findingCopy(finding)));
    }
  }
  evidence.append(evidenceTitle, evidenceDetail, findings);

  const segments = document.createElement("div");
  segments.className = "run-segment-list";
  if (run.segments.length === 0) {
    segments.append(
      text(
        "p",
        "No explicit segment boundaries were projected.",
        "runtime-empty-result",
      ),
    );
  } else {
    for (const segment of run.segments) {
      segments.append(renderSegment(segment, run.encounters));
    }
  }
  card.append(header, metrics, identity, evidence, segments);
  return card;
}

function renderSegment(
  segment: RunSegment,
  encounters: readonly RunEncounter[],
): HTMLElement {
  const card = document.createElement("section");
  card.className = "run-segment-card";
  card.dataset.kind = segment.kind;
  const header = document.createElement("header");
  const copy = document.createElement("div");
  copy.append(
    text("span", `Segment ${segment.index + 1}`, "run-report-kicker"),
    text("h3", formatIdentifier(segment.kind)),
  );
  header.append(
    copy,
    text(
      "span",
      `${formatDurationMicros(segment.wall_time_micros)} wall time`,
      "run-segment-time",
    ),
  );

  const metrics = metricGrid([
    [formatDurationMicros(segment.active_combat_micros), "Active combat"],
    [
      formatDurationMicros(segment.total_attempt_wall_time_micros),
      "Time in pulls",
    ],
    [formatDurationMicros(segment.elapsed_trying_micros), "Elapsed trying"],
    [
      formatDurationMicros(segment.between_attempts_micros),
      "Between attempts",
    ],
    [
      formatDurationMicros(segment.winning_attempt_wall_time_micros),
      segment.kind === "boss" ? "Winning boss pull" : "Final cleared pull",
    ],
    [segment.retry_count.toLocaleString(), "Retries / repulls"],
  ]);
  metrics.classList.add("run-segment-metrics");

  const segmentEncounters = segment.encounter_indices
    .map((index) => encounters.find((encounter) => encounter.index === index))
    .filter((encounter): encounter is RunEncounter => encounter !== undefined);
  card.append(header, metrics, renderAttemptTable(segment, segmentEncounters));
  return card;
}

function renderAttemptTable(
  segment: RunSegment,
  encounters: readonly RunEncounter[],
): HTMLElement {
  if (encounters.length === 0) {
    return text(
      "p",
      "No bounded pulls were recorded in this segment.",
      "run-attempt-empty",
    );
  }
  const wrap = document.createElement("div");
  wrap.className = "run-attempt-table-wrap";
  const table = document.createElement("table");
  table.className = "run-attempt-table";
  const head = document.createElement("thead");
  const row = document.createElement("tr");
  for (const label of [
    "Pull",
    "Encounter ID",
    "Result",
    "Wall time",
    "Combat",
    "Evidence",
  ]) {
    row.append(text("th", label));
  }
  head.append(row);
  const body = document.createElement("tbody");
  for (const encounter of encounters) {
    const attempt = document.createElement("tr");
    const winning = segment.winning_attempt_index === encounter.index;
    if (winning) attempt.dataset.winning = "true";
    const evidence = [
      encounter.is_retry ? "retry" : "first pull",
      encounter.closed_at_run_end ? "closed at run end" : null,
      winning ? "winning pull" : null,
    ]
      .filter((value): value is string => value !== null)
      .join(" · ");
    attempt.append(
      text("td", `#${encounter.attempt_number}`),
      text("td", encounter.encounter_id ?? "Unresolved"),
      text("td", formatIdentifier(encounter.terminal_state)),
      text("td", formatDurationMicros(encounter.wall_time_micros)),
      text("td", formatDurationMicros(encounter.active_combat_micros)),
      text("td", evidence),
    );
    body.append(attempt);
  }
  table.append(head, body);
  wrap.append(table);
  return wrap;
}

function exactJson(report: RunReport): HTMLElement {
  const details = document.createElement("details");
  details.className = "content-card run-report-json";
  const summary = document.createElement("summary");
  summary.textContent = "Inspect exact reducer output";
  const copy = button("Copy exact JSON", "quiet-button");
  const message = text(
    "span",
    "This is the local server-replay preview; nothing is transmitted.",
    "runtime-action-message",
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const json = `${JSON.stringify(report.projection, null, 2)}\n`;
  const pre = document.createElement("pre");
  pre.textContent = json;
  copy.addEventListener("click", async () => {
    copy.disabled = true;
    try {
      await navigator.clipboard.writeText(json);
      message.textContent = "Copied exact reducer output.";
    } catch (error) {
      message.textContent = errorMessage(error);
      message.classList.add("error");
    } finally {
      copy.disabled = false;
    }
  });
  actions.append(copy, message);
  details.append(summary, actions, pre);
  return details;
}

function metricGrid(
  values: readonly (readonly [string, string])[],
): HTMLDivElement {
  const metrics = document.createElement("div");
  metrics.className = "runtime-result-grid";
  for (const [value, label] of values) {
    const metric = document.createElement("article");
    metric.append(text("strong", value), text("span", label));
    metrics.append(metric);
  }
  return metrics;
}

function actionCard(title: string, detail: string): HTMLElement {
  const card = document.createElement("section");
  card.className = "content-card runtime-action-card";
  card.append(text("h2", title), text("p", detail));
  return card;
}

function fileRow(label: string, value: string): HTMLElement {
  const row = document.createElement("div");
  row.append(text("span", label), text("code", value));
  return row;
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  value: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tagName);
  node.textContent = value;
  if (className !== undefined) node.className = className;
  return node;
}

function formatIdentifier(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
