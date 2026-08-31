#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const sourceFile = resolvePath(required(options, "source"));
  const destinationFile = resolvePath(required(options, "destination"));
  const outputFile = resolvePath(required(options, "output"));
  const report = buildProof(build, sourceFile, destinationFile);
  mkdirSync(path.dirname(outputFile), { recursive: true });
  writeFileSync(outputFile, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyProof(report, sourceFile, destinationFile);
  console.log(
    `Protocol transition proof for ${build}: ${report.summary.route_count} exact routes, `
      + `${report.summary.safe_allowed_to_opaque_demotions} safe decoder demotions, `
      + "fresh destination replay still required.",
  );
}

function buildProof(build, sourceFile, destinationFile) {
  requireFile(sourceFile, "source protocol candidate");
  requireFile(destinationFile, "destination protocol candidate");
  const source = readJson(sourceFile, "source protocol candidate");
  const destination = readJson(destinationFile, "destination protocol candidate");
  if (String(source.target?.build_id ?? "") !== build ||
      String(destination.target?.build_id ?? "") !== build) {
    throw new Error("Protocol transition candidates do not match the requested build");
  }
  if (Number(source.schema_version) !== Number(destination.schema_version) ||
      stableStringify(source.target) !== stableStringify(destination.target) ||
      stableStringify(normalizeProvenance(source.provenance)) !==
        stableStringify(normalizeProvenance(destination.provenance))) {
    throw new Error("Protocol transition changed schema, target, or provenance");
  }

  const sourceRoutes = indexRoutes(source.routes, "source");
  const destinationRoutes = indexRoutes(destination.routes, "destination");
  if (sourceRoutes.size !== destinationRoutes.size) {
    throw new Error("Protocol transition changed the exact route count");
  }

  const safeDemotions = [];
  const unchanged = [];
  const unsafe = [];
  for (const [key, sourceRoute] of sourceRoutes) {
    const destinationRoute = destinationRoutes.get(key);
    if (!destinationRoute) {
      unsafe.push({ route: routeIdentity(sourceRoute), reason: "exact-route-removed" });
      continue;
    }
    if (stableStringify(routeEvidence(sourceRoute)) !== stableStringify(routeEvidence(destinationRoute))) {
      unsafe.push({ route: routeIdentity(sourceRoute), reason: "route-evidence-changed" });
      continue;
    }
    const from = disposition(sourceRoute);
    const to = disposition(destinationRoute);
    if (stableStringify(from) === stableStringify(to)) {
      unchanged.push({ route: routeIdentity(sourceRoute), disposition: to });
    } else if (from.kind === "allowed" && to.kind === "opaque") {
      safeDemotions.push({ route: routeIdentity(sourceRoute), from, to });
    } else {
      unsafe.push({ route: routeIdentity(sourceRoute), reason: "non-monotonic-disposition-change", from, to });
    }
  }
  for (const [key, route] of destinationRoutes) {
    if (!sourceRoutes.has(key)) unsafe.push({ route: routeIdentity(route), reason: "exact-route-added" });
  }
  if (unsafe.length > 0) {
    throw new Error(`Protocol transition contains ${unsafe.length} unsafe route changes`);
  }

  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-protocol-pack-transition-proof.mjs",
    game_build: build,
    generated_at: new Date().toISOString(),
    policy: {
      exact_numeric_route_set_must_be_unchanged: true,
      route_names_and_localization_are_not_runtime_identity: true,
      allowed_decoder_additions_forbidden: true,
      allowed_decoder_changes_forbidden: true,
      allowed_to_opaque_demotion_is_monotonic_safe: true,
      packet_absence_is_not_zero: true,
      capture_gaps_are_preserved: true,
      prior_report_identity_rebinding_allowed: false,
      destination_matching_build_replay_required: true,
      runtime_authority: false,
    },
    inputs: {
      source: descriptor(sourceFile, source),
      destination: descriptor(destinationFile, destination),
    },
    summary: {
      route_count: sourceRoutes.size,
      source_allowed_decoders: countDisposition(source.routes, "allowed"),
      destination_allowed_decoders: countDisposition(destination.routes, "allowed"),
      unchanged_routes: unchanged.length,
      safe_allowed_to_opaque_demotions: safeDemotions.length,
      unsafe_changes: unsafe.length,
      exact_route_set_unchanged: sourceRoutes.size === destinationRoutes.size,
      runtime_decoder_subset_proven: true,
      prior_report_identity_rebinding_allowed: false,
      destination_matching_build_replay_required: true,
      runtime_authority: false,
    },
    safe_demotions: safeDemotions,
    unsafe_changes: unsafe,
    conclusion: "safe-monotonic-decoder-demotion-fresh-destination-replay-required",
  };
  report.content_sha256 = contentHash(report);
  return report;
}

function verifyCommand(options) {
  const inputFile = resolvePath(required(options, "input"));
  const sourceFile = resolvePath(required(options, "source"));
  const destinationFile = resolvePath(required(options, "destination"));
  const report = readJson(inputFile, "protocol transition proof");
  verifyProof(report, sourceFile, destinationFile);
  console.log(`Protocol transition proof verified for build ${report.game_build}.`);
}

function verifyProof(report, sourceFile, destinationFile) {
  const original = readJsonFromObject(report);
  if (original.content_sha256 !== contentHash(original)) {
    throw new Error("Protocol transition proof content hash is invalid");
  }
  if (report.schema_version !== 1 ||
      report.generated_by !== "tools/bpsr-protocol-pack-transition-proof.mjs") {
    throw new Error("Unsupported protocol transition proof schema or generator");
  }
  const policy = report.policy ?? {};
  if (policy.exact_numeric_route_set_must_be_unchanged !== true ||
      policy.allowed_decoder_additions_forbidden !== true ||
      policy.allowed_decoder_changes_forbidden !== true ||
      policy.allowed_to_opaque_demotion_is_monotonic_safe !== true ||
      policy.packet_absence_is_not_zero !== true ||
      policy.capture_gaps_are_preserved !== true ||
      policy.prior_report_identity_rebinding_allowed !== false ||
      policy.destination_matching_build_replay_required !== true ||
      policy.runtime_authority !== false) {
    throw new Error("Protocol transition proof has an unsafe policy");
  }
  if (report.summary?.unsafe_changes !== 0 ||
      report.summary?.exact_route_set_unchanged !== true ||
      report.summary?.runtime_decoder_subset_proven !== true ||
      report.summary?.prior_report_identity_rebinding_allowed !== false ||
      report.summary?.destination_matching_build_replay_required !== true ||
      report.summary?.runtime_authority !== false ||
      !Array.isArray(report.safe_demotions) ||
      !Array.isArray(report.unsafe_changes) || report.unsafe_changes.length !== 0) {
    throw new Error("Protocol transition proof summary is unsafe");
  }
  if (report.inputs?.source?.sha256 !== sha256(sourceFile) ||
      report.inputs?.source?.bytes !== statSync(sourceFile).size ||
      report.inputs?.destination?.sha256 !== sha256(destinationFile) ||
      report.inputs?.destination?.bytes !== statSync(destinationFile).size) {
    throw new Error("Protocol transition proof input identity does not match disk");
  }
  const rebuilt = buildProof(String(report.game_build), sourceFile, destinationFile);
  const comparable = readJsonFromObject(original);
  for (const volatile of [comparable, rebuilt]) delete volatile.generated_at;
  delete comparable.content_sha256;
  delete rebuilt.content_sha256;
  if (stableStringify(comparable) !== stableStringify(rebuilt)) {
    throw new Error("Protocol transition proof does not match the current inputs");
  }
}

function readJsonFromObject(value) {
  return JSON.parse(JSON.stringify(value));
}

function indexRoutes(routes, label) {
  if (!Array.isArray(routes)) throw new Error(`${label} candidate routes are missing`);
  const index = new Map();
  for (const route of routes) {
    const key = routeKey(route);
    if (index.has(key)) throw new Error(`${label} candidate has duplicate route ${key}`);
    index.set(key, route);
  }
  return index;
}

function routeKey(route) {
  const value = route?.route ?? {};
  const direction = String(value.direction ?? "");
  const fragment = String(value.fragment?.kind ?? "");
  const service = Number(value.service_id);
  const method = Number(value.method_id);
  if (!direction || !fragment || !Number.isSafeInteger(service) || service <= 0 ||
      !Number.isSafeInteger(method) || method <= 0) {
    throw new Error("Protocol candidate contains an invalid exact route");
  }
  return `${direction}|${fragment}|${service}|${method}`;
}

function routeIdentity(route) {
  return {
    direction: route.route.direction,
    fragment: route.route.fragment.kind,
    service_id: Number(route.route.service_id),
    method_id: Number(route.route.method_id),
  };
}

function routeEvidence(route) {
  return {
    route: route.route,
    service_name: route.service_name ?? null,
    method_name: route.method_name ?? null,
    message_name: route.message_name ?? null,
    confidence: route.confidence ?? null,
    provenance: normalizeProvenance(route.provenance),
    features: route.features ?? [],
  };
}

function disposition(route) {
  const kind = String(route.disposition ?? "");
  if (kind === "allowed") return { kind, domain: route.domain ?? null, decoder: route.decoder ?? null };
  if (kind === "prohibited") return { kind, class: route.class ?? null };
  if (kind === "opaque") return { kind };
  throw new Error(`Unknown protocol route disposition ${kind || "<missing>"}`);
}

function countDisposition(routes, kind) {
  return routes.filter((route) => route.disposition === kind).length;
}

function normalizeProvenance(provenance) {
  return (provenance ?? []).map((entry) => ({
    ...entry,
    reference: typeof entry.reference === "string"
      ? entry.reference.replaceAll("\\", "/")
      : entry.reference,
  }));
}

function descriptor(file, value) {
  return {
    path: path.relative(repoRoot, file).replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: sha256(file),
    pack_id: value.pack_id,
    build_id: String(value.target?.build_id ?? ""),
  };
}

function contentHash(value) {
  const copy = readJsonFromObject(value);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(stableStringify(copy)).digest("hex")}`;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function readJson(file, label) {
  requireFile(file, label);
  return JSON.parse(readFileSync(file, "utf8"));
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}

function parseArgs(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined) throw new Error(`Invalid argument near ${flag ?? "<end>"}`);
    values[flag.slice(2)] = value;
  }
  return values;
}

function required(values, key) {
  if (!values[key]) throw new Error(`Missing --${key}`);
  return values[key];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-protocol-transition-"));
  try {
    const sourceFile = path.join(root, "source.json");
    const destinationFile = path.join(root, "destination.json");
    const route = {
      route: { direction: "client_to_server", fragment: { kind: "call" }, service_id: 7, method_id: 9 },
      service_name: "World",
      method_name: "UseSlot",
      message_name: null,
      confidence: "candidate",
      provenance: [],
      features: ["skill"],
    };
    const base = { schema_version: 1, target: { build_id: "123" }, provenance: [], routes: [] };
    writeFileSync(sourceFile, JSON.stringify({ ...base, pack_id: "source", routes: [{ ...route, disposition: "allowed", domain: "combat", decoder: "world_use_slot_v1" }] }));
    writeFileSync(destinationFile, JSON.stringify({ ...base, pack_id: "destination", routes: [{ ...route, disposition: "opaque" }] }));
    const proof = buildProof("123", sourceFile, destinationFile);
    verifyProof(proof, sourceFile, destinationFile);
    if (proof.summary.safe_allowed_to_opaque_demotions !== 1) throw new Error("Safe demotion self-test failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-protocol-pack-transition-proof self-test passed");
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-protocol-pack-transition-proof.mjs generate --build <id> --source <candidate.json> --destination <candidate.json> --output <proof.json>
  node tools/bpsr-protocol-pack-transition-proof.mjs verify --input <proof.json> --source <candidate.json> --destination <candidate.json>
  node tools/bpsr-protocol-pack-transition-proof.mjs self-test`);
  process.exit(exitCode);
}
