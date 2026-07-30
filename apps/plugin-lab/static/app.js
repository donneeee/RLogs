const view = {
  state: null,
  optimizerCatalog: null,
  filter: "all",
  query: "",
  showPaths: false,
};

const $ = (selector) => document.querySelector(selector);
const make = (tag, className, text) => {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
};

async function scan() {
  const refresh = $("#refresh");
  refresh.classList.add("busy");
  refresh.disabled = true;
  $("#error").hidden = true;
  try {
    const [stateResponse, catalogResponse] = await Promise.all([
      fetch("/api/state", { cache: "no-store" }),
      fetch("/api/module-optimizer/catalog", { cache: "no-store" }),
    ]);
    if (!stateResponse.ok) throw new Error(`Scan failed with HTTP ${stateResponse.status}`);
    if (!catalogResponse.ok) throw new Error(`Optimizer catalog failed with HTTP ${catalogResponse.status}`);
    view.state = await stateResponse.json();
    view.optimizerCatalog = await catalogResponse.json();
    render();
    $("#loading").hidden = true;
    $("#content").hidden = false;
    $("#last-scan").textContent = `Scanned ${new Date().toLocaleTimeString()}`;
  } catch (error) {
    $("#loading").hidden = true;
    $("#error").textContent = error.message;
    $("#error").hidden = false;
  } finally {
    refresh.classList.remove("busy");
    refresh.disabled = false;
  }
}

function render() {
  renderMetrics();
  renderContract();
  renderLocations();
  renderFilters();
  renderPlugins();
  renderOptimizerCatalog();
  renderFixtures();
  renderResources();
  renderPipeline();
  renderIssues();
}

function renderOptimizerCatalog() {
  const catalog = view.optimizerCatalog;
  $("#optimizer-revision").textContent =
    `${catalog.catalog_revision} / ${catalog.attributes.length} attributes / client ${catalog.client_builds.join(", ")}`;
  const previous = new Map(
    [...document.querySelectorAll(".optimizer-attribute-row")].map((row) => [
      Number(row.dataset.attributeId),
      {
        mode: row.querySelector("select").value,
        minimum: row.querySelector("input").value,
      },
    ]),
  );
  $("#optimizer-attributes").replaceChildren(...catalog.attributes.map((attribute) => {
    const row = make("div", "optimizer-attribute-row");
    row.dataset.attributeId = attribute.id;
    const identity = make("div", "optimizer-attribute-name");
    identity.append(
      make("strong", "", attribute.name),
      make("small", "", `${attribute.id} - ${attribute.thresholds.join("/")}`),
    );
    const mode = make("select");
    [
      ["normal", "Normal"],
      ["target", "Priority"],
      ["exclude", "Ignore"],
    ].forEach(([value, label]) => {
      const option = make("option", "", label);
      option.value = value;
      mode.append(option);
    });
    const minimum = make("input");
    minimum.type = "number";
    minimum.min = "0";
    minimum.placeholder = "0";
    minimum.setAttribute("aria-label", `Minimum ${attribute.name}`);
    const saved = previous.get(attribute.id);
    if (saved) {
      mode.value = saved.mode;
      minimum.value = saved.minimum;
    }
    row.append(identity, mode, minimum);
    return row;
  }));
  updateOptimizerExactSearchAvailability();
}

function renderFixtures() {
  const select = $("#replay-fixture");
  const previous = select.value;
  select.replaceChildren(...view.state.fixtures.map((fixture) => {
    const option = make("option", "", `${fixture.display_name} (${formatBytes(fixture.bytes)})`);
    option.value = fixture.file_name;
    return option;
  }));
  if (view.state.fixtures.some((fixture) => fixture.file_name === previous)) {
    select.value = previous;
  }
  const available = view.state.fixtures.length > 0;
  $("#run-replay").disabled = !available;
  $("#replay-status").textContent = available
    ? "Ready. The selected file will be streamed through the real replay runtime."
    : "No sealed .rlog fixtures were found.";
}

function renderMetrics() {
  const values = [
    [view.state.summary.plugin_count, "Visible modules"],
    [view.state.summary.game_count, "Game integrations"],
    [view.state.summary.installed_count, "Installed add-ons"],
    [view.state.summary.resource_count, "Resource exports"],
    [view.state.summary.issue_count, "Diagnostics"],
  ];
  $("#metrics").replaceChildren(...values.map(([value, label]) => {
    const card = make("article", "metric");
    card.append(make("strong", "", value), make("span", "", label));
    return card;
  }));
}

function renderContract() {
  const values = [
    [`v${view.state.core.version}`, "rLogs core"],
    [`v${view.state.core.plugin_api}`, "plug-in API"],
    [`v${view.state.core.game_plugin_api}`, "game API"],
  ];
  $("#core-contract").replaceChildren(...values.map(([value, label]) => {
    const cell = make("div");
    cell.append(make("strong", "", value), make("span", "", label));
    return cell;
  }));
}

function renderLocations() {
  const labels = {
    install_root: "Install root",
    installed_plugins: "Drop plug-ins here",
    plugin_assets: "Plug-in assets",
    shared_assets: "Shared assets",
  };
  $("#locations").replaceChildren(...Object.entries(labels).map(([key, label]) => {
    const row = make("div", "location");
    const path = make("code", view.showPaths ? "" : "concealed");
    path.textContent = view.showPaths ? view.state.locations[key] : folderHint(view.state.locations[key]);
    path.title = view.state.locations[key];
    row.append(make("span", "", label), path);
    return row;
  }));
  $("#toggle-paths").textContent = view.showPaths ? "Hide paths" : "Show paths";
}

function folderHint(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return `…/${parts.slice(-3).join("/")}`;
}

function renderFilters() {
  const sources = ["all", ...new Set(view.state.plugins.map((plugin) => plugin.source))];
  $("#filters").replaceChildren(...sources.map((source) => {
    const count = source === "all"
      ? view.state.plugins.length
      : view.state.plugins.filter((plugin) => plugin.source === source).length;
    const button = make("button", `filter${view.filter === source ? " active" : ""}`, `${source} · ${count}`);
    button.type = "button";
    button.addEventListener("click", () => {
      view.filter = source;
      renderFilters();
      renderPlugins();
    });
    return button;
  }));
}

function renderPlugins() {
  const query = view.query.toLowerCase();
  const plugins = view.state.plugins.filter((plugin) => {
    if (view.filter !== "all" && plugin.source !== view.filter) return false;
    return [
      plugin.name,
      plugin.id,
      plugin.runtime,
      ...plugin.capabilities,
      ...plugin.subscriptions,
    ].join(" ").toLowerCase().includes(query);
  });
  $("#plugin-grid").replaceChildren(...plugins.map(pluginCard));
  $("#plugin-empty").hidden = plugins.length !== 0;
}

function pluginCard(plugin) {
  const card = make("article", "plugin-card");
  const top = make("div", "plugin-card-top");
  const title = make("div", "plugin-title");
  title.append(make("h3", "", plugin.name), make("p", "", plugin.id));
  top.append(
    make("div", "plugin-icon", plugin.source.slice(0, 2)),
    title,
    make("span", "plugin-version", `v${plugin.version}`),
  );
  const meta = make("div", "plugin-meta");
  [["exports", plugin.export_count], ["imports", plugin.import_count], ["hooks", plugin.hook_count]].forEach(([label, value]) => {
    const cell = make("div");
    cell.append(make("strong", "", value), make("span", "", label));
    meta.append(cell);
  });
  const tags = make("div", "tags");
  const visibleTags = [plugin.runtime, ...plugin.capabilities].slice(0, 5);
  tags.append(...visibleTags.map((tag) => make("span", "tag", tag.replaceAll("_", " "))));
  if (plugin.capabilities.length + 1 > visibleTags.length) {
    tags.append(make("span", "tag", `+${plugin.capabilities.length + 1 - visibleTags.length}`));
  }
  const footer = make("div", "plugin-footer");
  footer.append(
    make("span", "", plugin.folder_name),
    make("span", `pill ${plugin.compatible ? "good" : "warn"}`, plugin.compatible ? "compatible" : "update needed"),
  );
  card.title = `Package: ${plugin.package_path}\nAssets: ${plugin.asset_namespace}\nShared: ${plugin.shared_asset_namespace}`;
  card.append(top, meta, tags, footer);
  return card;
}

function renderResources() {
  const rows = view.state.resources.map((resource) => {
    const row = make("tr");
    const name = make("td", "resource-name");
    name.append(make("strong", "", resource.name), make("small", "", resource.kind.replaceAll("-", " ")));
    const state = make("td", `state${resource.exists ? "" : " missing"}`, resource.exists ? "ready" : "missing");
    row.title = resource.path;
    row.append(
      name,
      make("td", "", resource.owner_plugin_id),
      make("td", "", resource.storage.replaceAll("_", " ")),
      make("td", "", resource.schema),
      state,
    );
    return row;
  });
  $("#resource-rows").replaceChildren(...rows);
}

function renderPipeline() {
  $("#pipeline-list").replaceChildren(...view.state.hook_stages.map((stage) => {
    const card = make("article", "pipeline");
    card.append(make("h3", "", stage.stage.replaceAll("_", " ")));
    const flow = make("div", "pipeline-flow");
    const before = stage.before_core.length ? stage.before_core.join("\n") : "no before hooks";
    const after = stage.after_core.length ? stage.after_core.join("\n") : "no after hooks";
    flow.append(
      make("div", "pipeline-step", before),
      make("span", "pipeline-arrow", "›"),
      make("div", "pipeline-step core", "rLogs core"),
      make("span", "pipeline-arrow", "›"),
      make("div", "pipeline-step", after),
    );
    card.append(flow);
    return card;
  }));

  const order = $("#load-order");
  order.replaceChildren();
  if (!view.state.load_order.length) {
    order.append(make("span", "", "No executable dependency chain"));
    return;
  }
  view.state.load_order.forEach((plugin, index) => {
    if (index) order.append(make("i", "", "→"));
    order.append(make("span", "", plugin));
  });
}

function renderIssues() {
  const badge = $("#issue-badge");
  badge.textContent = view.state.issues.length ? `${view.state.issues.length} found` : "clean";
  badge.className = `pill ${view.state.issues.length ? "warn" : "good"}`;
  if (!view.state.issues.length) {
    const clear = make("div", "issue-clear");
    clear.append(make("span", "", "●"), make("span", "", "All discovered manifests, resource paths, imports, and operation plans passed."));
    $("#issue-list").replaceChildren(clear);
    return;
  }
  $("#issue-list").replaceChildren(...view.state.issues.map((issue) => {
    const row = make("div", "issue");
    row.append(make("strong", "", issue.scope), make("span", "", issue.detail));
    return row;
  }));
}

async function runReplay() {
  const button = $("#run-replay");
  const fixture = $("#replay-fixture").value;
  if (!fixture) return;
  button.disabled = true;
  button.textContent = "Running...";
  $("#replay-status").textContent = "Verifying seal and executing subscribed canonical events...";
  $("#replay-result").hidden = true;
  try {
    const response = await fetch("/api/replay", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fixture }),
    });
    const body = await response.json();
    if (!response.ok) throw new Error(body.error || `Replay failed with HTTP ${response.status}`);
    renderReplayResult(body);
    $("#replay-status").textContent = `Completed ${body.fixture}`;
  } catch (error) {
    $("#replay-status").textContent = error.message;
    $("#replay-result").hidden = true;
  } finally {
    button.disabled = view.state.fixtures.length === 0;
    button.textContent = "Run fixture";
  }
}

function renderReplayResult(result) {
  const snapshotOutput = result.report.outputs.find((output) =>
    output.type === "snapshot" && output.schema_id === "app.rlogs.combat-meter.snapshot"
  );
  if (!snapshotOutput) throw new Error("Combat timeline did not publish its snapshot");
  const snapshot = snapshotOutput.payload;
  const metrics = [
    [result.report.rlog.event_count, "rlog events"],
    [result.report.metrics.events_delivered, "delivered"],
    [formatDuration(snapshot.active_combat_micros), "active combat"],
    [formatNumber(snapshot.data_gap_count), "data gaps"],
    [`${formatNumber(result.report.metrics.plugin_elapsed_micros)} us`, "plug-in CPU"],
  ];
  $("#replay-metrics").replaceChildren(...metrics.map(([value, label]) => {
    const cell = make("div");
    cell.append(make("strong", "", value), make("span", "", label));
    return cell;
  }));

  const actors = [...snapshot.actors].sort((left, right) =>
    right.damage_during_combat - left.damage_during_combat || left.actor_id - right.actor_id
  );
  $("#replay-actors").replaceChildren(...actors.map((actor) => {
    const row = make("tr");
    const identity = make("td", "resource-name");
    identity.append(
      make("strong", "", actor.display_name || `Actor ${actor.actor_id}`),
      make("small", "", `${actor.actor_kind || "unknown"} / actor ${actor.actor_id}`),
    );
    row.append(
      identity,
      make("td", "", formatNumber(actor.reported_damage)),
      make("td", "", formatNumber(actor.dps, 1)),
      make("td", "", formatNumber(actor.effective_healing)),
      make("td", "", formatNumber(actor.deaths)),
      make("td", "", formatNumber(actor.path_distance, 2)),
    );
    return row;
  }));
  const digest = result.report.rlog.content_sha256;
  $("#replay-footnote").textContent =
    `${snapshot.deployment_id} / ${snapshot.region_id} / ${snapshot.client_build} - ` +
    `${snapshot.combat_window_count} combat window(s) - seal ${digest.slice(0, 22)}...`;
  $("#replay-result").hidden = false;
}

function loadOptimizerDemo() {
  const attributes = [
    [1110, 1111], [1110, 2104], [1111, 1409], [1112, 1410],
    [1113, 2105], [1114, 2404], [1205, 2204], [1206, 2205],
    [1307, 2304], [1308, 2405], [1407, 2406], [1408, 1110],
  ];
  const modules = attributes.map((parts, index) => ({
    instance_id: String(9007199254740993n + BigInt(index) * 2n),
    config_id: 5500101 + (index % 4),
    quality: 5,
    parts: parts.map((partId, partIndex) => ({
      part_id: partId,
      initial_link_points: 3 + ((index + partIndex * 3) % 8),
    })),
  }));
  $("#optimizer-inventory").value = JSON.stringify({
    inventory: modules,
    equipped_slots: Object.fromEntries(
      modules.slice(0, 4).map((module, index) => [index + 1, module.instance_id]),
    ),
  }, null, 2);
  const strength = document.querySelector(
    '.optimizer-attribute-row[data-attribute-id="1110"] select',
  );
  if (strength) strength.value = "target";
  $("#optimizer-require-target").checked = false;
  updateOptimizerExactSearchAvailability();
  $("#optimizer-status").textContent =
    "Safe 12-module demo loaded. No character data is included.";
}

function optimizerInputFromJson(value) {
  if (Array.isArray(value)) return { modules: value, currentInstanceIds: [] };
  const moduleState = Array.isArray(value?.inventory)
    ? value
    : Array.isArray(value?.modules?.inventory)
      ? value.modules
      : Array.isArray(value?.body?.modules?.inventory)
        ? value.body.modules
        : null;
  if (moduleState) {
    const currentInstanceIds = Object.entries(moduleState.equipped_slots || {})
      .sort(([left], [right]) => Number(left) - Number(right))
      .map(([, instanceId]) => instanceId);
    return { modules: moduleState.inventory, currentInstanceIds };
  }
  if (Array.isArray(value?.modules)) {
    return { modules: value.modules, currentInstanceIds: [] };
  }
  throw new Error(
    "JSON must be an inventory array, a modules object, or a profile with modules.inventory",
  );
}

function optimizerCombinationCount(itemCount, selectionSize) {
  if (
    !Number.isSafeInteger(itemCount) ||
    !Number.isSafeInteger(selectionSize) ||
    itemCount < 0 ||
    selectionSize < 0 ||
    selectionSize > itemCount
  ) return 0n;
  const smallerSide = Math.min(selectionSize, itemCount - selectionSize);
  let result = 1n;
  for (let index = 1; index <= smallerSide; index += 1) {
    result =
      (result * BigInt(itemCount - smallerSide + index)) / BigInt(index);
  }
  return result;
}

function optimizerExactEstimate(modules) {
  const minimumTotalRaw = $("#optimizer-min-total").value;
  const minimumTotal =
    minimumTotalRaw === "" ? null : Number(minimumTotalRaw);
  const priorityAttributes = new Set(
    [...document.querySelectorAll(".optimizer-attribute-row")]
      .filter((row) => row.querySelector("select").value === "target")
      .map((row) => Number(row.dataset.attributeId)),
  );
  const requirePriority =
    $("#optimizer-require-target").checked && priorityAttributes.size > 0;
  const candidateCount = modules.filter((module) => {
    if (!Array.isArray(module.parts) || module.parts.length < 2) return false;
    const total = module.parts.reduce(
      (sum, part) => sum + Number(part.initial_link_points || 0),
      0,
    );
    if (minimumTotal != null && total < minimumTotal) return false;
    return (
      !requirePriority ||
      module.parts.some((part) => priorityAttributes.has(Number(part.part_id)))
    );
  }).length;
  return {
    candidateCount,
    combinations: optimizerCombinationCount(
      candidateCount,
      Number($("#optimizer-combination-size").value),
    ),
  };
}

function updateOptimizerExactSearchAvailability() {
  let modules = [];
  try {
    const text = $("#optimizer-inventory").value.trim();
    if (text) modules = optimizerInputFromJson(JSON.parse(text)).modules;
  } catch {
    // The main Optimize action reports malformed JSON; feasibility can wait.
  }
  const estimate = optimizerExactEstimate(modules);
  const tooLarge = estimate.combinations > 500000n;
  const searchMode = $("#optimizer-search-mode");
  const exactOption = searchMode.querySelector('option[value="exact"]');
  exactOption.disabled = tooLarge;
  exactOption.textContent = tooLarge
    ? "Exact verification (too many sets)"
    : "Exact verification";
  if (tooLarge && searchMode.value === "exact") searchMode.value = "auto";
  $("#optimizer-search-help").textContent = modules.length === 0
    ? "Exact verification is available for small inventories."
    : tooLarge
      ? `Exact disabled: ${formatNumber(estimate.candidateCount)} eligible modules can produce ` +
        `up to ${estimate.combinations.toLocaleString()} sets. Auto uses bounded search.`
      : `Exact available for ${estimate.combinations.toLocaleString()} possible sets.`;
}

function optimizerComputeBudget() {
  const cores =
    Number.isFinite(navigator.hardwareConcurrency) &&
    navigator.hardwareConcurrency > 0
      ? navigator.hardwareConcurrency
      : 4;
  const memoryGb =
    Number.isFinite(navigator.deviceMemory) && navigator.deviceMemory > 0
      ? navigator.deviceMemory
      : undefined;
  const mobile =
    navigator.userAgentData?.mobile ??
    /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent);
  if (cores <= 2 || (memoryGb !== undefined && memoryGb <= 2)) {
    return { beamWidth: 128, label: "constrained" };
  }
  if (cores <= 4 || (memoryGb !== undefined && memoryGb <= 4)) {
    return { beamWidth: 256, label: mobile ? "mobile" : "constrained" };
  }
  if (mobile) return { beamWidth: 512, label: "mobile" };
  if (cores >= 12 && memoryGb !== undefined && memoryGb >= 16) {
    return { beamWidth: 2048, label: "workstation" };
  }
  if (cores >= 8 && (memoryGb === undefined || memoryGb >= 8)) {
    return { beamWidth: 1024, label: "thorough" };
  }
  return { beamWidth: 512, label: "balanced" };
}

async function runModuleOptimizer() {
  const button = $("#run-optimizer");
  button.disabled = true;
  button.textContent = "Optimizing...";
  $("#optimizer-result").hidden = true;
  try {
    const parsed = JSON.parse($("#optimizer-inventory").value);
    const { modules, currentInstanceIds } = optimizerInputFromJson(parsed);
    const targetAttributes = [];
    const excludeAttributes = [];
    const minimums = {};
    document.querySelectorAll(".optimizer-attribute-row").forEach((row) => {
      const attributeId = Number(row.dataset.attributeId);
      const mode = row.querySelector("select").value;
      const minimum = Number(row.querySelector("input").value || 0);
      if (mode === "target") targetAttributes.push(attributeId);
      if (mode === "exclude") excludeAttributes.push(attributeId);
      if (minimum > 0) minimums[attributeId] = minimum;
    });
    const minimumTotalRaw = $("#optimizer-min-total").value;
    const exactEstimate = optimizerExactEstimate(modules);
    const fellBackFromExact =
      $("#optimizer-search-mode").value === "exact" &&
      exactEstimate.combinations > 500000n;
    if (fellBackFromExact) {
      $("#optimizer-search-mode").value = "auto";
      updateOptimizerExactSearchAvailability();
    }
    const computeBudget = optimizerComputeBudget();
    const payload = {
      modules,
      current_instance_ids: currentInstanceIds,
      target_attributes: targetAttributes,
      exclude_attributes: excludeAttributes,
      min_attr_requirements: minimums,
      combination_size: Number($("#optimizer-combination-size").value),
      max_solutions: Number($("#optimizer-result-count").value),
      search_mode: $("#optimizer-search-mode").value,
      beam_width: computeBudget.beamWidth,
      minimum_module_total: minimumTotalRaw === "" ? null : Number(minimumTotalRaw),
      require_target_match: $("#optimizer-require-target").checked,
    };
    $("#optimizer-status").textContent =
      fellBackFromExact
        ? `Exact search would require ${exactEstimate.combinations.toLocaleString()} sets; ` +
          `using ${computeBudget.label} bounded search automatically.`
        : `Searching ${formatNumber(modules.length)} modules with ${view.optimizerCatalog.catalog_revision} ` +
          `and the ${computeBudget.label} device budget (${formatNumber(computeBudget.beamWidth)} beam states)...`;
    const response = await fetch("/api/module-optimizer/optimize", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    const body = await response.json();
    if (!response.ok) {
      throw new Error(body.error || `Optimizer failed with HTTP ${response.status}`);
    }
    renderOptimizerResult(body);
    $("#optimizer-status").textContent =
      `Found ${body.solutions.length} result(s) using ${body.search.used_mode} search.`;
  } catch (error) {
    $("#optimizer-status").textContent = error.message;
  } finally {
    button.disabled = false;
    button.textContent = "Optimize modules";
  }
}

function renderOptimizerResult(result) {
  const current = result.current_setup;
  const top = result.solutions[0];
  const currentIsComparable =
    current?.instance_ids.length === result.search.combination_size;
  const metrics = [
    [current ? formatNumber(current.score) : "-", "current actual"],
    [top ? formatNumber(top.score) : "-", "top recommendation"],
    [
      currentIsComparable && current && top
        ? `${top.score - current.score >= 0 ? "+" : ""}${formatNumber(top.score - current.score)}`
        : "-",
      "actual change",
    ],
    [top ? formatNumber(top.ranking_score) : "-", "preference score"],
    [formatNumber(result.search.candidate_module_count), "candidates"],
  ];
  $("#optimizer-metrics").replaceChildren(...metrics.map(([value, label]) => {
    const cell = make("div");
    cell.append(make("strong", "", value), make("span", "", label));
    return cell;
  }));
  const solutionRow = (solution, label, isCurrent = false) => {
    const row = make("tr");
    if (isCurrent) row.classList.add("optimizer-current-row");
    const ids = make("td", "optimizer-module-ids");
    solution.modules.forEach((module) => {
      const line = make("span");
      line.append(
        make("strong", "", module.instance_id),
        make(
          "small",
          "",
          `config ${module.config_id}${module.quality == null ? "" : ` / Q${module.quality}`}`,
        ),
      );
      ids.append(line);
    });
    const attributes = solution.breakdown.attributes
      .filter((attribute) => attribute.total > 0)
      .map((attribute) => {
        const catalog = view.optimizerCatalog.attributes.find(
          (entry) => entry.id === attribute.attribute_id,
        );
        const suffix = attribute.multiplier === 2
          ? " (priority)"
          : attribute.multiplier === 0
            ? " (ignored for ranking)"
            : "";
        return `${catalog?.name || attribute.attribute_id}: ${attribute.total}${suffix}`;
      })
      .join(" - ");
    row.append(
      make("td", isCurrent ? "optimizer-current-label" : "", label),
      make("td", "optimizer-score", formatNumber(solution.score)),
      make("td", "optimizer-ranking-score", formatNumber(solution.ranking_score)),
      ids,
      make("td", "optimizer-attribute-summary", attributes),
    );
    return row;
  };
  const signature = (solution) => [...solution.instance_ids].sort().join("\0");
  const currentSignature = current ? signature(current) : null;
  const recommendations = result.solutions.filter(
    (solution) => signature(solution) !== currentSignature,
  );
  const rows = recommendations.map((solution, index) =>
    solutionRow(solution, `#${index + 1}`),
  );
  if (current) rows.unshift(solutionRow(current, "Current", true));
  $("#optimizer-result-rows").replaceChildren(...rows);
  $("#optimizer-footnote").textContent =
    `Actual power is unweighted. Preference score only orders recommendations. ` +
    `${formatNumber(result.search.total_combinations)} possible sets; ` +
    `${formatNumber(result.search.evaluated_states)} states evaluated with ` +
    `${result.search.exact ? "exact" : "bounded"} search. ${result.scoring_revision}.`;
  $("#optimizer-result").hidden = false;
}

function formatDuration(micros) {
  return `${(micros / 1_000_000).toFixed(2)}s`;
}

function formatNumber(value, maximumFractionDigits = 0) {
  return Number(value).toLocaleString(undefined, { maximumFractionDigits });
}

function formatBytes(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}

$("#refresh").addEventListener("click", scan);
$("#optimizer-demo").addEventListener("click", loadOptimizerDemo);
$("#run-optimizer").addEventListener("click", runModuleOptimizer);
$("#optimizer-combination-size").addEventListener(
  "change",
  updateOptimizerExactSearchAvailability,
);
$("#optimizer-min-total").addEventListener(
  "input",
  updateOptimizerExactSearchAvailability,
);
$("#optimizer-require-target").addEventListener(
  "change",
  updateOptimizerExactSearchAvailability,
);
$("#optimizer-attributes").addEventListener(
  "change",
  updateOptimizerExactSearchAvailability,
);
$("#optimizer-inventory").addEventListener(
  "input",
  updateOptimizerExactSearchAvailability,
);
$("#run-replay").addEventListener("click", runReplay);
$("#toggle-paths").addEventListener("click", () => {
  view.showPaths = !view.showPaths;
  renderLocations();
});
$("#plugin-search").addEventListener("input", (event) => {
  view.query = event.target.value;
  renderPlugins();
});
document.querySelectorAll(".nav-link").forEach((link) => {
  link.addEventListener("click", () => {
    document.querySelectorAll(".nav-link").forEach((item) => item.classList.remove("active"));
    link.classList.add("active");
  });
});

scan();
