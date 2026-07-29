const view = {
  state: null,
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
    const response = await fetch("/api/state", { cache: "no-store" });
    if (!response.ok) throw new Error(`Scan failed with HTTP ${response.status}`);
    view.state = await response.json();
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
  renderResources();
  renderPipeline();
  renderIssues();
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

$("#refresh").addEventListener("click", scan);
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
