import type { LiveEventDetail } from "./event-viewer";

export const CUSTOM_TRIGGER_DRAFT_STORAGE_KEY = "rlogs.custom-trigger-drafts.v1";
const MAXIMUM_STORED_DRAFTS = 32;
const MAXIMUM_STORED_DRAFT_BYTES = 64 * 1024;

export interface CustomTriggerDraftCriterion {
  path: string;
  label: string;
  operator: "equals";
  value: string;
  valueType: string;
}

export interface CustomTriggerDraft {
  schemaVersion: 1;
  id: string;
  name: string;
  enabled: false;
  createdUnixMillis: number;
  source: {
    sessionId: string;
    sequence: number;
    revision: number;
  };
  when: {
    eventKind: string;
    match: "all";
    criteria: readonly CustomTriggerDraftCriterion[];
  };
  actions: readonly [];
}

export function createCustomTriggerDraft(
  detail: LiveEventDetail,
  selectedPaths: ReadonlySet<string>,
  createdUnixMillis = Date.now(),
): CustomTriggerDraft {
  const criteria = detail.fields
    .filter((field) => field.usableInTrigger && selectedPaths.has(field.path))
    .map((field) => ({
      path: field.path,
      label: field.label,
      operator: "equals" as const,
      value: field.value,
      valueType: field.valueType,
    }));
  if (criteria.length === 0) {
    throw new Error("Select at least one trigger-safe field.");
  }
  return {
    schemaVersion: 1,
    id: `draft-${createdUnixMillis}-${detail.sequence}-${detail.revision}`,
    name: `When ${formatIdentifier(detail.kind)}`,
    enabled: false,
    createdUnixMillis,
    source: {
      sessionId: detail.sessionId,
      sequence: detail.sequence,
      revision: detail.revision,
    },
    when: {
      eventKind: detail.kind,
      match: "all",
      criteria,
    },
    actions: [],
  };
}

export function storeCustomTriggerDraft(
  storage: Pick<Storage, "getItem" | "setItem">,
  draft: CustomTriggerDraft,
): void {
  const retained = readStoredDrafts(storage)
    .filter((candidate) => candidate.id !== draft.id)
    .slice(-(MAXIMUM_STORED_DRAFTS - 1));
  retained.push(draft);
  let encoded = JSON.stringify(retained);
  while (new TextEncoder().encode(encoded).length > MAXIMUM_STORED_DRAFT_BYTES) {
    if (retained.length === 1) {
      throw new Error("The trigger draft exceeds the local draft-storage limit.");
    }
    retained.shift();
    encoded = JSON.stringify(retained);
  }
  storage.setItem(CUSTOM_TRIGGER_DRAFT_STORAGE_KEY, encoded);
}

export function readStoredDrafts(
  storage: Pick<Storage, "getItem">,
): CustomTriggerDraft[] {
  const encoded = storage.getItem(CUSTOM_TRIGGER_DRAFT_STORAGE_KEY);
  if (encoded === null) return [];
  try {
    const value: unknown = JSON.parse(encoded);
    if (!Array.isArray(value)) return [];
    return value.filter(isCustomTriggerDraft).slice(-MAXIMUM_STORED_DRAFTS);
  } catch {
    return [];
  }
}

function isCustomTriggerDraft(value: unknown): value is CustomTriggerDraft {
  if (!isRecord(value) || value.schemaVersion !== 1 || typeof value.id !== "string") return false;
  if (
    typeof value.name !== "string" ||
    value.enabled !== false ||
    !Number.isSafeInteger(value.createdUnixMillis) ||
    !isRecord(value.source) ||
    typeof value.source.sessionId !== "string" ||
    !isSafeCounter(value.source.sequence) ||
    !isSafeCounter(value.source.revision) ||
    !isRecord(value.when) ||
    typeof value.when.eventKind !== "string" ||
    value.when.match !== "all" ||
    !Array.isArray(value.when.criteria) ||
    !value.when.criteria.every(isCriterion) ||
    !Array.isArray(value.actions) ||
    value.actions.length !== 0
  ) {
    return false;
  }
  return true;
}

function isCriterion(value: unknown): value is CustomTriggerDraftCriterion {
  return (
    isRecord(value) &&
    typeof value.path === "string" &&
    typeof value.label === "string" &&
    value.operator === "equals" &&
    typeof value.value === "string" &&
    typeof value.valueType === "string"
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSafeCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function formatIdentifier(value: string): string {
  return value
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}
