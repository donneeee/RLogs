#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const LEDGER_PATH = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24687926/rdps-exhaustive-party-route-ledger.v1.json",
);
const INVENTORY_PATH = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-promotion-inventory.v1.json",
);
const PRESENTATION_PATH = path.join(
  ROOT,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-attribution-effect-presentation.v1.json",
);
const DOC_PATH = path.join(ROOT, "docs/COMBAT_INFLUENCE_LEDGER.md");
const BEGIN = "<!-- BEGIN GENERATED RDPS PROMOTION FRONTIER -->";
const END = "<!-- END GENERATED RDPS PROMOTION FRONTIER -->";

const command = process.argv[2] ?? "verify";
if (!new Set(["verify", "render"]).has(command)) {
  throw new Error("usage: node tools/bpsr-promotion-frontier-audit.mjs [verify|render]");
}

const ledger = readJson(LEDGER_PATH);
const inventory = readJson(INVENTORY_PATH);
const presentation = readJson(PRESENTATION_PATH);
verifyFrontier(ledger, inventory, presentation);

const rendered = renderFrontier(ledger, inventory);
if (command === "render") {
  process.stdout.write(`${rendered}\n`);
  process.exit(0);
}

const docs = fs.readFileSync(DOC_PATH, "utf8").replaceAll("\r\n", "\n");
const beginIndex = docs.indexOf(BEGIN);
const endIndex = docs.indexOf(END);
assert(beginIndex >= 0 && endIndex > beginIndex, "generated rDPS frontier markers are missing or reversed");
const actual = docs.slice(beginIndex, endIndex + END.length);
assert(actual === rendered, "generated rDPS frontier documentation drifted; run the render command and update the marked block");

console.log(
  `verified build ${inventory.game_build}: ${inventory.production_effects.length} production effects, `
    + `${inventory.remaining_candidates.length} fail-closed candidates, `
    + `${ledger.reconciliation.exact_id_route_rows} exact routes`,
);

function verifyFrontier(ledgerValue, inventoryValue, presentationValue) {
  assert(ledgerValue.schema_version === 1, "unexpected exhaustive ledger schema");
  assert(inventoryValue.schema_version === 1, "unexpected promotion inventory schema");
  assert(presentationValue.schema_version === 1, "unexpected presentation schema");
  assert(ledgerValue.game_build === inventoryValue.game_build, "ledger/inventory build mismatch");
  assert(presentationValue.game_build === inventoryValue.game_build, "presentation/inventory build mismatch");
  assert(presentationValue.deployment_id === inventoryValue.deployment_id, "presentation/inventory deployment mismatch");
  assert(presentationValue.locale === "en-US", "promotion presentation must be en-US");

  const withoutDigest = { ...ledgerValue };
  delete withoutDigest.content_sha256;
  const computedDigest = crypto
    .createHash("sha256")
    .update(JSON.stringify(withoutDigest))
    .digest("hex");
  assert(ledgerValue.content_sha256 === computedDigest, "exhaustive ledger content digest is invalid");
  assert(
    inventoryValue.review_coverage.exhaustive_ledger_content_sha256 === computedDigest,
    "promotion inventory does not bind the checked-in exhaustive ledger",
  );

  const reconciliation = ledgerValue.reconciliation;
  const coverage = inventoryValue.review_coverage;
  assert(coverage.consolidated_unique_effect_ids === reconciliation.consolidated_unique_effect_ids,
    "consolidated effect count drifted");
  assert(coverage.exact_id_route_rows === reconciliation.exact_id_route_rows,
    "exact route count drifted");
  assert(coverage.exact_id_route_unique_ids === reconciliation.exact_id_route_unique_ids,
    "unique exact ID count drifted");
  assert(coverage.zero_effect_rows_without_disposition === reconciliation.zero_effect_rows_without_disposition,
    "effect disposition coverage drifted");
  assert(coverage.zero_exact_id_route_rows_without_disposition
    === reconciliation.zero_exact_id_route_rows_without_disposition,
  "exact-route disposition coverage drifted");
  assert(reconciliation.zero_production_effect_ids_missing_runtime_source_tests_or_config === true,
    "a production effect lacks runtime source, an exact-ID test, or a runtime binding");
  assert(Array.isArray(reconciliation.production_effect_ids_missing_runtime_source_tests_or_config)
    && reconciliation.production_effect_ids_missing_runtime_source_tests_or_config.length === 0,
  "production evidence-gap list is not empty");

  const productionIds = inventoryValue.production_effects.map((row) => row.effect_id);
  const ledgerProductionIds = ledgerValue.consolidated_effect_rows
    .filter((row) => row.production_enabled)
    .map((row) => row.effect_id)
    .sort((left, right) => left - right);
  assert(strictlyIncreasingPositive(productionIds), "production IDs are not sorted, unique, and positive");
  assert(equalJson(productionIds, ledgerProductionIds), "production IDs differ from the exhaustive ledger");
  assert(coverage.ledger_production_effect_ids === productionIds.length,
    "ledger production count differs from the inventory");
  assert(Array.isArray(coverage.post_ledger_production_effect_ids)
    && coverage.post_ledger_production_effect_ids.length === 0,
  "post-ledger production IDs are forbidden");

  const candidateIds = inventoryValue.remaining_candidates.map((row) => row.effect_id);
  const ledgerCandidateIds = ledgerValue.consolidated_effect_rows
    .filter((row) => row.reviewed_disposition === "runtime-candidate")
    .map((row) => row.effect_id)
    .sort((left, right) => left - right);
  assert(strictlyIncreasingPositive(candidateIds), "candidate IDs are not sorted, unique, and positive");
  assert(equalJson(candidateIds, ledgerCandidateIds), "candidate IDs differ from the exhaustive ledger");
  assert(inventoryValue.remaining_candidates.every((row) =>
    row.disposition === "candidate-fail-closed"
      && typeof row.full_name === "string" && row.full_name.trim().length > 0
      && typeof row.remaining_proof_obligation === "string"
      && row.remaining_proof_obligation.trim().length > 0),
  "candidate fail-closed metadata is incomplete");
  assert(candidateIds.every((id) => !productionIds.includes(id)),
    "an effect is both production and candidate");

  const presented = presentationValue.effects.map((row) => ({
    effect_id: row.effect_id,
    full_name: row.name,
  }));
  assert(equalJson(presented, inventoryValue.production_effects),
    "localized production presentation differs from the inventory");
  assert(inventoryValue.production_effects.every((row) => row.full_name.trim().length > 0),
    "a production effect has no localized name");

  const policy = inventoryValue.policy;
  assert(policy.ordinary_damage_and_dps_unchanged === true, "ordinary damage preservation is disabled");
  assert(policy.unknown_and_unresolved_events_retained === true, "unresolved event retention is disabled");
  assert(policy.candidate_effects_grant_provider_credit === false, "a candidate can grant provider credit");
  assert(policy.production_effect_ids_are_sorted_and_unique === true, "sorted/unique production policy is disabled");
  assert(policy.complete_localized_names_required === true, "localized-name policy is disabled");
}

function renderFrontier(ledgerValue, inventoryValue) {
  const reconciliation = ledgerValue.reconciliation;
  const production = inventoryValue.production_effects
    .map((row) => `- \`${row.effect_id}\` — ${row.full_name}`)
    .join("\n");
  const candidates = inventoryValue.remaining_candidates
    .map((row) => `- \`${row.effect_id}\` — ${row.full_name}: ${row.remaining_proof_obligation}`)
    .join("\n");
  return `${BEGIN}
The build \`${inventoryValue.game_build}\` party-route ledger is an exhaustive joined census, not a
shortlist of effects seen in one run. Its checked cardinalities are:

- ${reconciliation.aoyi_parent_skills} Aoyi parent skills and ${reconciliation.aoyi_reconciled_descendants_including_reviewed_component_routes} reconciled descendants;
- ${reconciliation.party_skill_candidates} party-skill rows and ${reconciliation.party_buff_candidates} party-buff rows;
- ${reconciliation.rogue_party_entry_candidates} rogue/team-entry rows;
- ${reconciliation.observed_external_effects} packet-observed external effects;
- ${reconciliation.consolidated_unique_effect_ids} consolidated effect identities; and
- ${reconciliation.exact_id_route_rows.toLocaleString("en-US")} exact ID/route rows covering ${reconciliation.exact_id_route_unique_ids} unique exact IDs.

Every route row carries origin, localization evidence, provider and recipient
scope, magnitude evidence, stacking, lifecycle, operation order, aliases,
runtime bindings, focused tests, reviewed disposition, and any remaining proof
obligation. The generator asserts those fields and cardinalities, including
zero missing runtime/config/test bindings for all ${inventoryValue.production_effects.length} production-enabled effect
IDs. This is the coverage gate used to answer whether every promotion candidate
was actually reviewed.

The production allowlist is:

${production}

Exactly ${inventoryValue.remaining_candidates.length} offensive candidates remain deliberately fail-closed:

${candidates}

Life Wave (\`2302421\`) is included in the production allowlist. For current-build
joint replay, rLogs requires the exact HP/max-HP trigger owner, a verified module
profile, the recipient's adjacent attribute transition that selects the affected
secondary-stat lane, a reviewed damage-action route, and packet-final
counterfactual conservation. Ambiguous ownership, overlap, missing cross-vantage
witnesses, and unsupported actions still grant zero provider credit while
ordinary damage remains unchanged.
${END}`;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
}

function strictlyIncreasingPositive(values) {
  return values.every((value, index) => Number.isSafeInteger(value)
    && value > 0 && (index === 0 || values[index - 1] < value));
}

function equalJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
