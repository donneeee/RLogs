export type FightAttributeComponent =
  | "final"
  | "total"
  | "add"
  | "extra_add"
  | "percent"
  | "extra_percent";

export interface FightAttributePresentation {
  attribute_id: number;
  family_id: number;
  component: FightAttributeComponent;
  name: string;
  description: string | null;
  number_type: number;
  format_type: number;
  icon: string | null;
  displayable: boolean;
}

export interface FightAttributePresentationCatalog {
  schema_version: 1;
  game_build: string;
  locale: string;
  source: string;
  source_sha256: string;
  attributes: readonly FightAttributePresentation[];
}

export interface LiveCharacterStatsSnapshot {
  schema_version: 1;
  revision: number;
  character: {
    character_id: string;
    region: {
      deployment_id: string;
      region_id: string;
      realm_id: string | null;
      world_id: string | null;
    };
  } | null;
  snapshot_values: Readonly<Record<string, number>>;
  current_values: Readonly<Record<string, number>>;
  last_event_sequence: number | null;
  last_game_time_millis: number | null;
}

export interface LiveCharacterStatComponentView {
  presentation: FightAttributePresentation;
  snapshotValue: number | null;
  currentValue: number;
}

export interface LiveCharacterStatFamilyView {
  familyId: number;
  name: string;
  description: string | null;
  changed: boolean;
  components: readonly LiveCharacterStatComponentView[];
}

const COMPONENTS: readonly FightAttributeComponent[] = [
  "final",
  "total",
  "add",
  "extra_add",
  "percent",
  "extra_percent",
];

export function parseFightAttributePresentationCatalog(
  value: unknown,
): FightAttributePresentationCatalog {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.game_build !== "string" ||
    typeof value.locale !== "string" ||
    typeof value.source !== "string" ||
    typeof value.source_sha256 !== "string" ||
    !/^[0-9a-f]{64}$/u.test(value.source_sha256) ||
    !Array.isArray(value.attributes) ||
    !value.attributes.every(isFightAttribute)
  ) {
    throw new Error("The local host returned an invalid Fight Attribute catalog.");
  }
  return value as unknown as FightAttributePresentationCatalog;
}

export function parseLiveCharacterStatsSnapshot(
  value: unknown,
): LiveCharacterStatsSnapshot {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    !nonnegativeInteger(value.revision) ||
    !(value.character === null || isCharacter(value.character)) ||
    !isIntegerRecord(value.snapshot_values) ||
    !isIntegerRecord(value.current_values) ||
    !nullableNonnegativeInteger(value.last_event_sequence) ||
    !nullableInteger(value.last_game_time_millis)
  ) {
    throw new Error("The local host returned an invalid live character-stat snapshot.");
  }
  return value as unknown as LiveCharacterStatsSnapshot;
}

export function resolveLiveCharacterStatFamilies(
  snapshot: LiveCharacterStatsSnapshot,
  catalog: FightAttributePresentationCatalog,
): LiveCharacterStatFamilyView[] {
  const byId = new Map(catalog.attributes.map((attribute) => [attribute.attribute_id, attribute]));
  const families = new Map<number, LiveCharacterStatComponentView[]>();
  for (const [rawAttributeId, currentValue] of Object.entries(snapshot.current_values)) {
    const attributeId = Number(rawAttributeId);
    const presentation = byId.get(attributeId);
    if (presentation === undefined || !presentation.displayable) continue;
    const snapshotValue = snapshot.snapshot_values[rawAttributeId] ?? null;
    const components = families.get(presentation.family_id) ?? [];
    components.push({ presentation, snapshotValue, currentValue });
    families.set(presentation.family_id, components);
  }
  return [...families.entries()]
    .map(([familyId, components]) => {
      components.sort(
        (left, right) =>
          COMPONENTS.indexOf(left.presentation.component) -
            COMPONENTS.indexOf(right.presentation.component) ||
          left.presentation.attribute_id - right.presentation.attribute_id,
      );
      const primary = components.find((component) => component.presentation.component === "final") ??
        components[0]!;
      return {
        familyId,
        name: primary.presentation.name,
        description: primary.presentation.description,
        changed: components.some(
          (component) =>
            component.snapshotValue !== null &&
            component.snapshotValue !== component.currentValue,
        ),
        components,
      };
    })
    .sort(
      (left, right) =>
        Number(right.changed) - Number(left.changed) ||
        left.name.localeCompare(right.name) ||
        left.familyId - right.familyId,
    );
}

export function formatFightAttributeValue(
  value: number,
  numberType: number,
  formatType: number,
): string {
  if (numberType === 1 || (numberType === 0 && formatType === 4)) {
    return `${formatReadableNumber(value / 100)}%`;
  }
  if (numberType === 2) return `${formatReadableNumber(value / 1_000)}s`;
  return formatReadableNumber(value);
}

export function fightAttributeComponentLabel(component: FightAttributeComponent): string {
  return ({
    final: "Final",
    total: "Total",
    add: "Additive",
    extra_add: "Extra additive",
    percent: "Percent",
    extra_percent: "Extra percent",
  } satisfies Record<FightAttributeComponent, string>)[component];
}

function formatReadableNumber(value: number): string {
  return value.toLocaleString("en-US", { maximumFractionDigits: 6 });
}

function isFightAttribute(value: unknown): boolean {
  return (
    isRecord(value) &&
    positiveInteger(value.attribute_id) &&
    positiveInteger(value.family_id) &&
    COMPONENTS.includes(value.component as FightAttributeComponent) &&
    typeof value.name === "string" &&
    value.name.trim().length > 0 &&
    nullableString(value.description) &&
    Number.isSafeInteger(value.number_type) &&
    Number.isSafeInteger(value.format_type) &&
    nullableString(value.icon) &&
    typeof value.displayable === "boolean"
  );
}

function isCharacter(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.character_id === "string" &&
    isRecord(value.region) &&
    typeof value.region.deployment_id === "string" &&
    typeof value.region.region_id === "string" &&
    nullableString(value.region.realm_id) &&
    nullableString(value.region.world_id)
  );
}

function isIntegerRecord(value: unknown): boolean {
  return (
    isRecord(value) &&
    Object.entries(value).every(
      ([key, entry]) => /^-?[0-9]+$/u.test(key) && Number.isSafeInteger(entry),
    )
  );
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

function nullableNonnegativeInteger(value: unknown): boolean {
  return value === null || nonnegativeInteger(value);
}

function nullableInteger(value: unknown): boolean {
  return value === null || Number.isSafeInteger(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
