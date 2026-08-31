#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const DIRECT_STATE_KEY = "CurSeasonId";
const ROUTE_SYMBOLS = ["CurSeasonId", "RefreshSeasonData", "SyncSeason"];
const REQUIRED_LIFECYCLE_FILES = new Set([
  "game.lua",
  "ui/model/data_base.lua",
  "ui/model/data_manager.lua",
  "ui/model/season_data.lua",
  "ui/service/service_mgr.lua",
  "ui/view_model/login_vm.lua",
  "utility/connect_manager.lua",
]);
const EXPECTED_DIRECT_WRITERS = new Map([
  ["ui/model/season_data.lua", ["0"]],
  ["ui/view_model/season_vm.lua", ["seasonId"]],
]);

const [command = "help", ...rest] = process.argv.slice(2);
try {
  if (command === "build") build(parseArgs(rest));
  else if (command === "verify") verify(path.resolve(required(parseArgs(rest), "input")));
  else if (command === "self-test") selfTest();
  else usage(command === "help" ? 0 : 1);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

function build(options) {
  const buildId = String(required(options, "build"));
  const luaRoot = path.resolve(required(options, "lua-root"));
  const decompiler = path.resolve(required(options, "decompiler"));
  const output = path.resolve(required(options, "output"));
  requireDirectory(luaRoot, "exact-build Lua root");
  requireFile(decompiler, "Lua decompiler");

  const luaFiles = enumerateLuaFiles(luaRoot);
  if (luaFiles.length === 0) throw new Error("Exact-build Lua root contains no .lua bytecode");
  const manifest = [];
  const candidates = [];
  let totalBytes = 0;
  for (const file of luaFiles) {
    const bytes = readFileSync(file.absolute);
    const descriptor = {
      relative_path: file.relative,
      bytes: bytes.length,
      sha256: sha256(bytes),
    };
    manifest.push(descriptor);
    totalBytes += bytes.length;
    const routeSymbols = ROUTE_SYMBOLS.filter((symbol) =>
      bytes.includes(Buffer.from(symbol, "ascii"))
    );
    const requiredLifecycleFile = REQUIRED_LIFECYCLE_FILES.has(file.relative);
    const containsDataManagerClearConstants =
      bytes.includes(Buffer.from("DataMgr", "ascii")) &&
      bytes.includes(Buffer.from("Clear", "ascii"));
    if (routeSymbols.length > 0 || requiredLifecycleFile || containsDataManagerClearConstants) {
      candidates.push({
        ...descriptor,
        selection_reasons: {
          route_symbols: routeSymbols,
          required_lifecycle_file: requiredLifecycleFile,
          contains_data_manager_and_clear_constants: containsDataManagerClearConstants,
        },
      });
    }
  }

  const candidatePaths = new Set(candidates.map((candidate) => candidate.relative_path));
  const missingLifecycleFiles = [...REQUIRED_LIFECYCLE_FILES]
    .filter((relativePath) => !candidatePaths.has(relativePath));
  if (missingLifecycleFiles.length > 0) {
    throw new Error(`Missing required lifecycle files: ${missingLifecycleFiles.join(", ")}`);
  }

  const decompiledSources = new Map();
  for (const candidate of candidates) {
    const absolute = path.join(luaRoot, ...candidate.relative_path.split("/"));
    decompiledSources.set(candidate.relative_path, decompile(decompiler, absolute));
  }
  const analysis = analyzeDecompiledSources(decompiledSources);
  requireExpectedSurface(analysis);

  const proof = {
    schema_version: SCHEMA_VERSION,
    tool: "tools/bpsr-season-state-mutation-proof.mjs",
    game_build: buildId,
    policy: {
      exact_input_build_authoritative: true,
      exact_numeric_world_route_authoritative: true,
      localized_names_are_not_runtime_keys: true,
      complete_direct_literal_lua_writer_scan_required: true,
      dynamically_constructed_field_writers_proven_absent: false,
      native_or_external_state_writers_proven_absent: false,
      monitor_chain_logout_or_clear_absence_proven: false,
      normal_reconnect_static_control_flow_proven: true,
      explicit_logout_reset_static_control_flow_proven: true,
      static_reconnect_lifecycle_never_grants_event_time_logout_exclusion: true,
      promoted_protocol_event_coverage_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      unresolved_evidence_is_preserved: true,
    },
    inputs: {
      lua_root: "<exact-build-full-extract>/Luac/lua",
      decompiler: fileDescriptor(decompiler),
      lua_manifest_root_sha256: manifestRootHash(manifest),
      lua_files: manifest,
    },
    exact_route: {
      direction: "server_to_client",
      fragment: "notify",
      service_id: 1664308034,
      method_id: 27,
      method_name_evidence: "SyncSeason",
      wire_value_field: "vSeason",
      generated_dispatch_file: "zservice/world_ntf_gen.lua",
      implementation_file: "zservice/world_ntf_impl.lua",
      state_mutator_file: "ui/view_model/season_vm.lua",
      state_model_file: "ui/model/season_data.lua",
      mutation_chain: [
        "WorldNtf method 27 decodes vSeason",
        "world_ntf_impl.SyncSeason calls seasonVm.RefreshSeasonData(vSeason)",
        "SeasonVM.RefreshSeasonData writes season_data.CurSeasonId = seasonId",
      ],
      logout_or_clear_reset: "SeasonData.Clear writes CurSeasonId = 0",
    },
    direct_literal_writer_surface: analysis,
    lifecycle_surface: analysis.lifecycle_surface,
    summary: {
      lua_files_scanned: manifest.length,
      lua_bytes_scanned: totalBytes,
      route_symbol_candidate_files: candidates.length,
      candidate_files_decompiled: decompiledSources.size,
      required_lifecycle_files_decompiled: REQUIRED_LIFECYCLE_FILES.size,
      direct_data_manager_clear_callsites:
        analysis.lifecycle_surface.direct_data_manager_clear_callsites.length,
      decompile_failures: 0,
      direct_literal_state_writer_files: analysis.direct_literal_writers.length,
      direct_literal_state_writer_assignments:
        analysis.direct_literal_writers.reduce((sum, item) => sum + item.assignments.length, 0),
      exact_server_route_to_positive_season_writer_proven: true,
      exact_clear_to_zero_writer_proven: true,
      direct_literal_lua_writer_surface_complete: true,
      normal_reconnect_preserves_season_state_by_static_control_flow: true,
      explicit_logout_resets_season_state_by_static_control_flow: true,
      intervening_monitor_chain_explicit_logout_proven_absent: false,
      event_time_season_authority_complete: false,
    },
    blockers: [
      "dynamic-or-aliased-season-state-mutation-surface-not-proven-absent",
      "intervening-monitor-chain-explicit-logout-or-reinitialization-not-proven-absent",
      "matching-build-protocol-pack-and-protocol-event-coverage-not-promoted",
    ],
  };
  proof.content_sha256 = contentHash(proof);
  writeFileSync(output, `${JSON.stringify(proof, null, 2)}\n`, "utf8");
  verify(output);
  console.log(
    `Season-state mutation proof built for ${buildId}: ${manifest.length} Lua files, ` +
    `${analysis.direct_literal_writers.length} direct writer files, authority remains closed.`,
  );
}

function analyzeDecompiledSources(sources) {
  const directLiteralWriters = [];
  const refreshCallsites = [];
  const syncSeasonCallsites = [];
  const directDataManagerClearCallsites = [];
  const dataManagerReconnectCallsites = [];
  const assignmentPattern = /\b(?:[A-Za-z_]\w*\.)?CurSeasonId\s*=\s*([^\r\n]+)/g;
  for (const [relativePath, source] of [...sources.entries()].sort(([left], [right]) =>
    left.localeCompare(right))) {
    const lines = source.split(/\r?\n/);
    const assignments = [];
    for (let index = 0; index < lines.length; index += 1) {
      assignmentPattern.lastIndex = 0;
      let match;
      while ((match = assignmentPattern.exec(lines[index])) !== null) {
        assignments.push({
          line: index + 1,
          expression: match[1].trim(),
          source_line: lines[index].trim(),
        });
      }
      if (/\bRefreshSeasonData\s*\(/.test(lines[index])) {
        refreshCallsites.push({ relative_path: relativePath, line: index + 1, source_line: lines[index].trim() });
      }
      if (/\bSyncSeason\b/.test(lines[index])) {
        syncSeasonCallsites.push({ relative_path: relativePath, line: index + 1, source_line: lines[index].trim() });
      }
      if (/\b(?:Z\.)?DataMgr[.:]Clear\s*\(/.test(lines[index])) {
        directDataManagerClearCallsites.push({
          relative_path: relativePath,
          line: index + 1,
          source_line: lines[index].trim(),
        });
      }
      if (/\b(?:Z\.)?DataMgr[.:]OnReconnect\s*\(/.test(lines[index])) {
        dataManagerReconnectCallsites.push({
          relative_path: relativePath,
          line: index + 1,
          source_line: lines[index].trim(),
        });
      }
    }
    if (assignments.length > 0) directLiteralWriters.push({ relative_path: relativePath, assignments });
  }
  const lifecycleSurface = analyzeLifecycleSurface(
    sources,
    directDataManagerClearCallsites,
    dataManagerReconnectCallsites,
  );
  return {
    state_key: DIRECT_STATE_KEY,
    scan_scope: "all exact-build Lua bytecode files containing CurSeasonId, RefreshSeasonData, or SyncSeason string constants",
    direct_literal_writers: directLiteralWriters,
    refresh_season_data_callsites: refreshCallsites,
    sync_season_symbol_lines: syncSeasonCallsites,
    lifecycle_surface: lifecycleSurface,
    dynamic_or_aliased_writes_excluded: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function requireExpectedSurface(analysis, requireLifecycle = true) {
  const actual = new Map(analysis.direct_literal_writers.map((writer) => [
    writer.relative_path,
    writer.assignments.map((assignment) => assignment.expression),
  ]));
  if (actual.size !== EXPECTED_DIRECT_WRITERS.size) {
    throw new Error(`Unexpected direct CurSeasonId writer-file count ${actual.size}`);
  }
  for (const [relativePath, expressions] of EXPECTED_DIRECT_WRITERS) {
    if (stableStringify(actual.get(relativePath)) !== stableStringify(expressions)) {
      throw new Error(`Unexpected direct CurSeasonId writer surface in ${relativePath}`);
    }
  }
  const generated = analysis.sync_season_symbol_lines.some((entry) =>
    entry.relative_path === "zservice/world_ntf_gen.lua" && entry.source_line.includes("impl:SyncSeason"));
  const implementation = analysis.refresh_season_data_callsites.some((entry) =>
    entry.relative_path === "zservice/world_ntf_impl.lua" && entry.source_line.includes("RefreshSeasonData(vSeason)"));
  if (!generated || !implementation) throw new Error("Exact method-27 season mutation chain is incomplete");
  if (requireLifecycle) requireExpectedLifecycleSurface(analysis.lifecycle_surface);
}

function analyzeLifecycleSurface(sources, directDataManagerClearCallsites, dataManagerReconnectCallsites) {
  const required = (relativePath) => {
    const source = sources.get(relativePath);
    if (source === undefined) throw new Error(`Missing decompiled lifecycle source ${relativePath}`);
    return source;
  };
  const seasonData = required("ui/model/season_data.lua");
  const dataManager = required("ui/model/data_manager.lua");
  const dataBase = required("ui/model/data_base.lua");
  const loginVm = required("ui/view_model/login_vm.lua");
  const game = required("game.lua");
  const connectManager = required("utility/connect_manager.lua");
  const serviceManager = required("ui/service/service_mgr.lua");

  const seasonInit = functionRegion(seasonData, /^function SeasonData:Init\(\)/, /^function /);
  const seasonClear = functionRegion(seasonData, /^function SeasonData:Clear\(\)/, /^function /);
  const dataManagerGet = functionRegion(dataManager, /^local getData = function\(name\)/, /^local \w+ = function/);
  const dataManagerReconnect = functionRegion(dataManager, /^local onReconnect = function\(\)/, /^local \w+ = function/);
  const dataManagerClear = functionRegion(dataManager, /^local clear = function\(\)/, /^local \w+ = function/);
  const dataBaseReconnect = functionRegion(dataBase, /^function DataBase:OnReconnect\(\)/, /^function /);
  const loginReconnect = functionRegion(loginVm, /^\s*AsyncReconnect = function\(self\)/, /^\s{17}\w+ = function/);
  const loginLogout = functionRegion(loginVm, /^\s*Logout = function\(self, clearSDK\)/, /^\s{17}\w+ = function/);
  const gameReconnect = functionRegion(game, /^\s*OnReconnect = function\(isSelectedChar\)/, /^\s{14}\w+ = function/);
  const connectReconnect = functionRegion(connectManager, /^function ConnectManager:asyncReconnect\(channelType\)/, /^function /);
  const serviceReconnect = functionRegion(serviceManager, /^function ServiceMgr.OnReconnect\(\)/, /^function /);

  return {
    required_lifecycle_files: [...REQUIRED_LIFECYCLE_FILES].sort(),
    direct_data_manager_clear_callsites: directDataManagerClearCallsites,
    data_manager_on_reconnect_callsites: dataManagerReconnectCallsites,
    assertions: {
      season_init_calls_clear: /self:Clear\s*\(\)/.test(seasonInit),
      season_clear_sets_cur_season_id_zero: /self\.CurSeasonId\s*=\s*0\b/.test(seasonClear),
      data_manager_initializes_new_cached_models: /cached_data\[name\]:Init\s*\(\)/.test(dataManagerGet),
      data_manager_clear_invokes_every_cached_model_clear:
        /for key, value in pairs\(cached_data\)/.test(dataManagerClear) &&
        /value:Clear\s*\(\)/.test(dataManagerClear),
      data_manager_reconnect_invokes_every_cached_model_on_reconnect:
        /for key, value in pairs\(cached_data\)/.test(dataManagerReconnect) &&
        /value:OnReconnect\s*\(\)/.test(dataManagerReconnect),
      season_data_overrides_on_reconnect: /^function SeasonData:OnReconnect\(/m.test(seasonData),
      data_base_on_reconnect_is_noop: isEmptyFunctionBody(dataBaseReconnect),
      successful_login_reconnect_calls_game_on_reconnect:
        /Z\.Game\.OnReconnect\s*\(isSelectedChar\)/.test(loginReconnect),
      login_reconnect_calls_data_manager_clear: /Z\.DataMgr[.:]Clear\s*\(/.test(loginReconnect),
      explicit_login_logout_calls_data_manager_clear: /Z\.DataMgr[.:]Clear\s*\(/.test(loginLogout),
      game_reconnect_calls_data_manager_on_reconnect:
        /Z\.DataMgr[.:]OnReconnect\s*\(\)/.test(gameReconnect),
      game_reconnect_calls_data_manager_clear: /Z\.DataMgr[.:]Clear\s*\(/.test(gameReconnect),
      connect_manager_retry_calls_login_async_reconnect:
        /vm:AsyncReconnect\s*\(\)/.test(connectReconnect),
      service_reconnect_calls_data_manager_clear: /Z\.DataMgr[.:]Clear\s*\(/.test(serviceReconnect),
      normal_reconnect_preserves_season_state_by_static_control_flow: false,
      explicit_logout_resets_season_state_by_static_control_flow: false,
      intervening_monitor_chain_explicit_logout_proven_absent: false,
    },
    dynamic_or_aliased_clear_callsites_proven_absent: false,
    event_time_authority: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function requireExpectedLifecycleSurface(surface) {
  const assertions = surface?.assertions;
  if (!assertions ||
    surface.direct_data_manager_clear_callsites?.length !== 1 ||
    surface.direct_data_manager_clear_callsites[0]?.relative_path !== "ui/view_model/login_vm.lua" ||
    surface.data_manager_on_reconnect_callsites?.length !== 1 ||
    surface.data_manager_on_reconnect_callsites[0]?.relative_path !== "game.lua" ||
    assertions.season_init_calls_clear !== true ||
    assertions.season_clear_sets_cur_season_id_zero !== true ||
    assertions.data_manager_initializes_new_cached_models !== true ||
    assertions.data_manager_clear_invokes_every_cached_model_clear !== true ||
    assertions.data_manager_reconnect_invokes_every_cached_model_on_reconnect !== true ||
    assertions.season_data_overrides_on_reconnect !== false ||
    assertions.data_base_on_reconnect_is_noop !== true ||
    assertions.successful_login_reconnect_calls_game_on_reconnect !== true ||
    assertions.login_reconnect_calls_data_manager_clear !== false ||
    assertions.explicit_login_logout_calls_data_manager_clear !== true ||
    assertions.game_reconnect_calls_data_manager_on_reconnect !== true ||
    assertions.game_reconnect_calls_data_manager_clear !== false ||
    assertions.connect_manager_retry_calls_login_async_reconnect !== true ||
    assertions.service_reconnect_calls_data_manager_clear !== false) {
    throw new Error("Exact-build season lifecycle control flow is incomplete or changed");
  }
  assertions.normal_reconnect_preserves_season_state_by_static_control_flow = true;
  assertions.explicit_logout_resets_season_state_by_static_control_flow = true;
  if (assertions.intervening_monitor_chain_explicit_logout_proven_absent !== false ||
    surface.dynamic_or_aliased_clear_callsites_proven_absent !== false ||
    surface.event_time_authority !== false || surface.formula_authority !== false ||
    surface.runtime_authority !== false || surface.ui_display_authority !== false ||
    surface.provider_rdps_credit_allowed !== false) {
    throw new Error("Season lifecycle proof incorrectly grants authority");
  }
}

function functionRegion(source, headerPattern, nextHeaderPattern) {
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex((line) => headerPattern.test(line));
  if (start < 0) throw new Error(`Missing lifecycle function matching ${headerPattern}`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (nextHeaderPattern.test(lines[index])) {
      end = index;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

function isEmptyFunctionBody(region) {
  const body = region.split(/\r?\n/).slice(1)
    .map((line) => line.trim())
    .filter((line) => line !== "" && line !== "end");
  return body.length === 0;
}

function verify(input) {
  requireFile(input, "season-state mutation proof");
  const proof = JSON.parse(readFileSync(input, "utf8"));
  const proofSchema = Number(proof.schema_version);
  if (![1, SCHEMA_VERSION].includes(proofSchema) ||
    proof.tool !== "tools/bpsr-season-state-mutation-proof.mjs" ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.ui_display_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    proof.policy?.complete_direct_literal_lua_writer_scan_required !== true ||
    proof.policy?.dynamically_constructed_field_writers_proven_absent !== false ||
    proof.summary?.direct_literal_lua_writer_surface_complete !== true ||
    proof.summary?.event_time_season_authority_complete !== false ||
    proof.exact_route?.service_id !== 1664308034 || proof.exact_route?.method_id !== 27 ||
    proof.content_sha256 !== contentHash(proof)) {
    throw new Error("Season-state mutation proof is invalid or unsafe");
  }
  if (proofSchema >= 2 &&
    (proof.policy?.normal_reconnect_static_control_flow_proven !== true ||
      proof.policy?.explicit_logout_reset_static_control_flow_proven !== true ||
      proof.policy?.static_reconnect_lifecycle_never_grants_event_time_logout_exclusion !== true ||
      proof.summary?.normal_reconnect_preserves_season_state_by_static_control_flow !== true ||
      proof.summary?.explicit_logout_resets_season_state_by_static_control_flow !== true ||
      proof.summary?.intervening_monitor_chain_explicit_logout_proven_absent !== false)) {
    throw new Error("Season-state lifecycle proof is invalid or unsafe");
  }
  requireExpectedSurface(proof.direct_literal_writer_surface, proofSchema >= 2);
  if (proofSchema >= 2 &&
    stableStringify(proof.lifecycle_surface) !==
      stableStringify(proof.direct_literal_writer_surface.lifecycle_surface)) {
      throw new Error("Season lifecycle surface receipt is inconsistent");
  }
  console.log(
    `Season-state mutation proof verified for build ${proof.game_build}: ` +
    `${proof.summary.lua_files_scanned} Lua files, event-time authority remains closed.`,
  );
}

function selfTest() {
  const sources = new Map([
    ["game.lua", "              OnReconnect = function(isSelectedChar)\n  Z.ServiceMgr.OnReconnect()\n  Z.DataMgr.OnReconnect()\n              OpenLoading = function()"],
    ["ui/model/data_base.lua", "function DataBase:OnReconnect()\nend\nfunction DataBase:Clear()\nend"],
    ["ui/model/data_manager.lua", "local getData = function(name)\n  cached_data[name]:Init()\nlocal onReconnect = function()\n  for key, value in pairs(cached_data) do\n    value:OnReconnect()\n  end\nlocal clear = function()\n  for key, value in pairs(cached_data) do\n    value:Clear()\n  end\nlocal unInit = function()"],
    ["ui/model/season_data.lua", "function SeasonData:Init()\n  self:Clear()\nend\nfunction SeasonData:Clear()\n  self.CurSeasonId = 0\nend\nfunction SeasonData:UnInit()"],
    ["ui/service/service_mgr.lua", "function ServiceMgr.OnReconnect()\n  value:OnReconnect()\nend\nfunction ServiceMgr.OnEnterStage()"],
    ["ui/view_model/login_vm.lua", "                 AsyncReconnect = function(self)\n  Z.Game.OnReconnect(isSelectedChar)\n                 Logout = function(self, clearSDK)\n  Z.DataMgr.Clear()\n                 KickOffByClient = function(self)"],
    ["utility/connect_manager.lua", "function ConnectManager:asyncReconnect(channelType)\n  if vm:AsyncReconnect() then\n  end\nfunction ConnectManager:onDisconnect(channelType)"],
    ["ui/view_model/season_vm.lua", "local SeasonVM = {RefreshSeasonData = function(seasonId)\n  seasonData.CurSeasonId = seasonId\nend}"],
    ["zservice/world_ntf_impl.lua", "seasonVm.RefreshSeasonData(vSeason)"],
    ["zservice/world_ntf_gen.lua", "impl:SyncSeason(call, pbMsg.vSeason)"],
  ]);
  const analysis = analyzeDecompiledSources(sources);
  requireExpectedSurface(analysis);
  if (analysis.direct_literal_writers.length !== 2) throw new Error("Self-test writer count failed");
  console.log("bpsr-season-state-mutation-proof self-test passed");
}

function enumerateLuaFiles(root) {
  const output = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(absolute);
      else if (entry.isFile() && entry.name.toLowerCase().endsWith(".lua")) {
        output.push({ absolute, relative: normalize(path.relative(root, absolute)) });
      }
    }
  };
  visit(root);
  return output.sort((left, right) => left.relative.localeCompare(right.relative));
}

function decompile(decompiler, input) {
  const result = spawnSync(decompiler, ["--dec", input], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`Unable to decompile ${input}: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout;
}

function manifestRootHash(manifest) {
  const hash = createHash("sha256");
  for (const entry of manifest) {
    hash.update(entry.relative_path, "utf8");
    hash.update("\0");
    hash.update(String(entry.bytes), "utf8");
    hash.update("\0");
    hash.update(entry.sha256, "utf8");
    hash.update("\n");
  }
  return hash.digest("hex");
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return sha256(Buffer.from(stableStringify(copy), "utf8"));
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return { path: normalize(file), bytes: bytes.length, sha256: sha256(bytes) };
}

function sha256(value) { return createHash("sha256").update(value).digest("hex"); }
function normalize(value) { return String(value).replaceAll("\\", "/"); }
function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}
function requireDirectory(directory, label) {
  if (!existsSync(directory) || !statSync(directory).isDirectory()) {
    throw new Error(`Missing ${label}: ${directory}`);
  }
}
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`Invalid argument ${key ?? ""}`);
    output[key.slice(2)] = value;
  }
  return output;
}
function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}
function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-season-state-mutation-proof.mjs build --build <id> --lua-root <Luac/lua> --decompiler <cLuaDecompiler.exe> --output <json>\n  node tools/bpsr-season-state-mutation-proof.mjs verify --input <json>\n  node tools/bpsr-season-state-mutation-proof.mjs self-test");
  process.exit(exitCode);
}
