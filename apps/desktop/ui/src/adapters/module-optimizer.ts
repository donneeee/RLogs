export interface ModulePart {
  part_id: number;
  initial_link_points: number | null;
}

export interface ModuleCandidate {
  instance_id: string;
  config_id: number;
  quality: number | null;
  parts: readonly ModulePart[];
}

export interface OptimizerAttribute {
  id: number;
  name: string;
  official_name: string | null;
  icon: string | null;
  thresholds: readonly number[];
  fight_values: readonly number[];
}

export interface OptimizerCatalog {
  game_id: string;
  catalog_revision: string;
  scoring_revision: string;
  client_builds: readonly string[];
  attributes: readonly OptimizerAttribute[];
  combination_sizes: readonly number[];
  default_max_solutions: number;
}

export interface LocalModuleCharacter {
  package_id: string;
  character_id: string;
  display_name: string | null;
  deployment: string;
  region: string;
  source_client_build: string;
  observed_unix_millis: number;
  modules: readonly ModuleCandidate[];
  current_instance_ids: readonly string[];
  module_snapshot_available: boolean;
  module_snapshot_detail: string;
}

export interface LocalModuleInventory {
  schema_version: 1;
  characters: readonly LocalModuleCharacter[];
  issues: readonly string[];
}

export interface OptimizeRequest {
  modules: readonly ModuleCandidate[];
  current_instance_ids: readonly string[];
  target_attributes: readonly number[];
  exclude_attributes: readonly number[];
  min_attr_requirements: Readonly<Record<string, number>>;
  combination_size: number;
  max_solutions: number;
  search_mode: "auto" | "exact" | "beam";
  use_gpu: boolean;
  exact_combination_limit: number;
  beam_width: number;
  minimum_parts: number;
  minimum_module_total: number | null;
  require_target_match: boolean;
}

export interface AttributeScore {
  attribute_id: number;
  total: number;
  reached_threshold: number | null;
  base_power: number;
  multiplier: number;
  applied_power: number;
}

export interface ModuleSolution {
  instance_ids: readonly string[];
  modules: readonly ModuleCandidate[];
  score: number;
  ranking_score: number;
  breakdown: {
    threshold_power: number;
    ranking_threshold_power: number;
    total_link_points: number;
    total_link_power: number;
    attributes: readonly AttributeScore[];
  };
}

export interface OptimizeResponse {
  scoring_revision: string;
  catalog_revision: string;
  current_setup: ModuleSolution | null;
  solutions: readonly ModuleSolution[];
  search: {
    requested_mode: "auto" | "exact" | "beam";
    used_mode: "auto" | "exact" | "beam";
    exact: boolean;
    input_module_count: number;
    candidate_module_count: number;
    excluded_module_count: number;
    total_combinations: number;
    evaluated_states: number;
    combination_size: number;
    beam_width: number | null;
    backend: "cpu" | "open_cl" | "cpu_open_cl_hybrid";
    accelerator_name: string | null;
    accelerator_fallback: string | null;
  };
}

export interface GpuSupport {
  available: boolean;
  backend: "cpu" | "open_cl" | "cpu_open_cl_hybrid";
  device_name: string | null;
  vendor: string | null;
  detail: string;
}

interface ModulePresentation {
  name: string;
  icon: string;
  quality: number;
}

const MODULES: Readonly<Record<string, ModulePresentation>> = {
  "5500101": module("Basic Attack Module", "item_mod_device_attack2.png", 2),
  "5500102": module("Advanced Attack Module", "item_mod_device_attack3.png", 3),
  "5500103": module("Excellent Attack Module", "item_mod_device_attack4.png", 4),
  "5500104": module("Excellent Attack Module - Premium", "item_icons_mod_device_attack5.png", 4),
  "5500201": module("Basic Support Module", "item_mod_device_2.png", 2),
  "5500202": module("Advanced Support Module", "item_mod_device_3.png", 3),
  "5500203": module("Excellent Support Module", "item_mod_device_4.png", 4),
  "5500204": module("Excellent Support Module - Premium", "item_icons_mod_device_5.png", 4),
  "5500301": module("Basic Guard Module", "item_mod_device_protect2.png", 2),
  "5500302": module("Advanced Guard Module", "item_mod_device_protect3.png", 3),
  "5500303": module("Excellent Guard Module", "item_mod_device_protect4.png", 4),
  "5500304": module("Excellent Guard Module - Premium", "item_icons_device_protect5.png", 4),
};

const QUALITY_NAMES: Readonly<Record<string, string>> = {
  "1": "Common",
  "2": "Uncommon",
  "3": "Rare",
  "4": "Epic",
  "5": "Legendary",
};

function module(name: string, icon: string, quality: number): ModulePresentation {
  return {
    name,
    icon: `/game-assets/blue-protocol-star-resonance/shared/icons/profile/modules/${icon}`,
    quality,
  };
}

export function modulePresentation(value: ModuleCandidate): ModulePresentation {
  return (
    MODULES[String(value.config_id)] ?? {
      name: `Module ${value.config_id.toLocaleString()}`,
      icon: "/game-assets/blue-protocol-star-resonance/shared/icons/modules/types/4-universal.png",
      quality: value.quality ?? 0,
    }
  );
}

export function moduleQuality(value: ModuleCandidate): string {
  const presentation = modulePresentation(value);
  const quality = value.quality ?? presentation.quality;
  return QUALITY_NAMES[String(quality)] ?? (quality > 0 ? `Quality ${quality}` : "Unrated");
}

export function optimizerAssetUrl(icon: string | null): string | null {
  if (icon === null || icon.trim() === "") return null;
  return `/game-assets/blue-protocol-star-resonance/shared/${icon.replace(/^\/+/, "")}`;
}

export function parseOptimizerCatalog(value: unknown): OptimizerCatalog {
  if (
    !isRecord(value) ||
    typeof value.game_id !== "string" ||
    typeof value.catalog_revision !== "string" ||
    typeof value.scoring_revision !== "string" ||
    !isStringArray(value.client_builds) ||
    !Array.isArray(value.attributes) ||
    !value.attributes.every(isAttribute) ||
    !isPositiveIntegerArray(value.combination_sizes) ||
    !positiveInteger(value.default_max_solutions)
  ) {
    throw new Error("The local host returned an invalid module optimizer catalog.");
  }
  return value as unknown as OptimizerCatalog;
}

export function parseLocalModuleInventory(value: unknown): LocalModuleInventory {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !Array.isArray(value.characters) ||
    !value.characters.every(isCharacter) ||
    !isStringArray(value.issues)
  ) {
    throw new Error("The local host returned an invalid module inventory.");
  }
  return value as unknown as LocalModuleInventory;
}

export function parseGpuSupport(value: unknown): GpuSupport {
  if (
    !isRecord(value) ||
    typeof value.available !== "boolean" ||
    !isSearchBackend(value.backend) ||
    !nullableString(value.device_name) ||
    !nullableString(value.vendor) ||
    typeof value.detail !== "string"
  ) {
    throw new Error("The local host returned invalid GPU support details.");
  }
  return value as unknown as GpuSupport;
}

export function parseOptimizeResponse(value: unknown): OptimizeResponse {
  if (
    !isRecord(value) ||
    typeof value.scoring_revision !== "string" ||
    typeof value.catalog_revision !== "string" ||
    !(value.current_setup === null || isSolution(value.current_setup)) ||
    !Array.isArray(value.solutions) ||
    !value.solutions.every(isSolution) ||
    !isRecord(value.search) ||
    !isSearchMode(value.search.requested_mode) ||
    !isSearchMode(value.search.used_mode) ||
    !isSearchBackend(value.search.backend) ||
    typeof value.search.exact !== "boolean" ||
    !nonnegativeInteger(value.search.input_module_count) ||
    !nonnegativeInteger(value.search.candidate_module_count) ||
    !nonnegativeInteger(value.search.excluded_module_count) ||
    !nonnegativeInteger(value.search.evaluated_states) ||
    !nonnegativeInteger(value.search.total_combinations) ||
    !positiveInteger(value.search.combination_size) ||
    !(value.search.beam_width === null || positiveInteger(value.search.beam_width)) ||
    !nullableString(value.search.accelerator_name) ||
    !nullableString(value.search.accelerator_fallback)
  ) {
    throw new Error("The local host returned an invalid module optimization result.");
  }
  return value as unknown as OptimizeResponse;
}

function isCharacter(value: unknown): boolean {
  return (
    isRecord(value) &&
    isSha256(value.package_id) &&
    typeof value.character_id === "string" &&
    nullableString(value.display_name) &&
    typeof value.deployment === "string" &&
    typeof value.region === "string" &&
    typeof value.source_client_build === "string" &&
    positiveInteger(value.observed_unix_millis) &&
    Array.isArray(value.modules) &&
    value.modules.every(isModuleCandidate) &&
    isStringArray(value.current_instance_ids) &&
    typeof value.module_snapshot_available === "boolean" &&
    typeof value.module_snapshot_detail === "string"
  );
}

function isModuleCandidate(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.instance_id === "string" &&
    value.instance_id.length > 0 &&
    Number.isSafeInteger(value.config_id) &&
    (value.quality === null || Number.isSafeInteger(value.quality)) &&
    Array.isArray(value.parts) &&
    value.parts.every(
      (part) =>
        isRecord(part) &&
        Number.isSafeInteger(part.part_id) &&
        (part.initial_link_points === null ||
          Number.isSafeInteger(part.initial_link_points)),
    )
  );
}

function isAttribute(value: unknown): boolean {
  return (
    isRecord(value) &&
    Number.isSafeInteger(value.id) &&
    typeof value.name === "string" &&
    nullableString(value.official_name) &&
    nullableString(value.icon) &&
    isIntegerArray(value.thresholds) &&
    isIntegerArray(value.fight_values) &&
    value.thresholds.length === value.fight_values.length
  );
}

function isSolution(value: unknown): boolean {
  return (
    isRecord(value) &&
    isStringArray(value.instance_ids) &&
    Array.isArray(value.modules) &&
    value.modules.every(isModuleCandidate) &&
    Number.isSafeInteger(value.score) &&
    Number.isSafeInteger(value.ranking_score) &&
    isRecord(value.breakdown) &&
    Number.isSafeInteger(value.breakdown.threshold_power) &&
    Number.isSafeInteger(value.breakdown.ranking_threshold_power) &&
    Number.isSafeInteger(value.breakdown.total_link_points) &&
    Number.isSafeInteger(value.breakdown.total_link_power) &&
    Array.isArray(value.breakdown.attributes)
  );
}

function isSearchMode(value: unknown): boolean {
  return value === "auto" || value === "exact" || value === "beam";
}

function isSearchBackend(value: unknown): boolean {
  return value === "cpu" || value === "open_cl" || value === "cpu_open_cl_hybrid";
}

function isIntegerArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every(Number.isSafeInteger);
}

function isPositiveIntegerArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every(positiveInteger);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

function nullableString(value: unknown): boolean {
  return value === null || typeof value === "string";
}

function positiveInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) > 0;
}

function nonnegativeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isSha256(value: unknown): boolean {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
