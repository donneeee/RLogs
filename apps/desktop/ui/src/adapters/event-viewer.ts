export const EVENT_VIEWER_TOPICS = [
  "combat",
  "encounter",
  "actor",
  "character_profile",
  "party",
  "world",
  "map",
  "dungeon",
  "chat",
  "data_quality",
] as const;

export type EventViewerTopic = (typeof EVENT_VIEWER_TOPICS)[number];

export interface EventViewerFilter {
  topic: EventViewerTopic | null;
  kind: string | null;
  search: string | null;
}

export interface EventViewerEntity {
  actorId: string;
  entityUuid: string;
}

export interface EventViewerIdentifiers {
  actor: EventViewerEntity | null;
  source: EventViewerEntity | null;
  directSource: EventViewerEntity | null;
  target: EventViewerEntity | null;
  ability: string | null;
  status: string | null;
  monster: string | null;
  scene: string | null;
  map: string | null;
  dungeon: string | null;
  characterId: string | null;
}

export interface EventViewerEvent {
  sequence: number;
  timelineSequence: number | null;
  observedMicros: number;
  gameTimeMillis: number | null;
  topic: EventViewerTopic;
  kind: string;
  summary: string;
  amount: string | null;
  identifiers: EventViewerIdentifiers;
  canonicalJson: string;
}

export interface EventViewerHeader {
  schema_version: number;
  event_schema_version: number;
  session_id: string;
  producer: string;
  region: {
    identity: {
      deployment_id: string;
      region_id: string;
      realm_id: string | null;
      world_id: string | null;
    };
    client_build: string;
    protocol_pack_digest: string;
  };
}

export interface EventViewerPage {
  schemaVersion: number;
  queryId: string;
  sessionId: string;
  artifactDigest: string;
  header: EventViewerHeader;
  filter: EventViewerFilter;
  pageIndex: number;
  scannedThisPage: number;
  scannedTotal: number;
  matchedTotal: number;
  integrityVerified: boolean;
  complete: boolean;
  events: readonly EventViewerEvent[];
}

export function parseEventViewerPage(value: unknown): EventViewerPage {
  if (!isRecord(value) || value.schemaVersion !== 1) {
    throw new Error("The local host returned an unsupported Event Viewer page.");
  }
  if (
    typeof value.queryId !== "string" ||
    typeof value.sessionId !== "string" ||
    typeof value.artifactDigest !== "string" ||
    !isHeader(value.header) ||
    !isFilter(value.filter) ||
    !isSafeCounter(value.pageIndex) ||
    !isSafeCounter(value.scannedThisPage) ||
    !isSafeCounter(value.scannedTotal) ||
    !isSafeCounter(value.matchedTotal) ||
    typeof value.integrityVerified !== "boolean" ||
    typeof value.complete !== "boolean" ||
    !Array.isArray(value.events) ||
    !value.events.every(isEvent)
  ) {
    throw new Error("The local host returned an invalid Event Viewer page.");
  }
  return value as unknown as EventViewerPage;
}

function isEvent(value: unknown): value is EventViewerEvent {
  return (
    isRecord(value) &&
    isSafeCounter(value.sequence) &&
    (value.timelineSequence === null ||
      isSafeCounter(value.timelineSequence)) &&
    isSafeCounter(value.observedMicros) &&
    (value.gameTimeMillis === null ||
      (typeof value.gameTimeMillis === "number" &&
        Number.isSafeInteger(value.gameTimeMillis))) &&
    isTopic(value.topic) &&
    typeof value.kind === "string" &&
    typeof value.summary === "string" &&
    (value.amount === null || typeof value.amount === "string") &&
    isIdentifiers(value.identifiers) &&
    typeof value.canonicalJson === "string"
  );
}

function isIdentifiers(value: unknown): value is EventViewerIdentifiers {
  return (
    isRecord(value) &&
    isOptionalEntity(value.actor) &&
    isOptionalEntity(value.source) &&
    isOptionalEntity(value.directSource) &&
    isOptionalEntity(value.target) &&
    isOptionalString(value.ability) &&
    isOptionalString(value.status) &&
    isOptionalString(value.monster) &&
    isOptionalString(value.scene) &&
    isOptionalString(value.map) &&
    isOptionalString(value.dungeon) &&
    isOptionalString(value.characterId)
  );
}

function isOptionalEntity(value: unknown): boolean {
  return (
    value === null ||
    (isRecord(value) &&
      typeof value.actorId === "string" &&
      typeof value.entityUuid === "string")
  );
}

function isFilter(value: unknown): value is EventViewerFilter {
  return (
    isRecord(value) &&
    (value.topic === null || isTopic(value.topic)) &&
    isOptionalString(value.kind) &&
    isOptionalString(value.search)
  );
}

function isHeader(value: unknown): value is EventViewerHeader {
  if (
    !isRecord(value) ||
    !isSafeCounter(value.schema_version) ||
    !isSafeCounter(value.event_schema_version) ||
    typeof value.session_id !== "string" ||
    typeof value.producer !== "string" ||
    !isRecord(value.region) ||
    !isRecord(value.region.identity)
  ) {
    return false;
  }
  const identity = value.region.identity;
  return (
    typeof identity.deployment_id === "string" &&
    typeof identity.region_id === "string" &&
    isOptionalString(identity.realm_id) &&
    isOptionalString(identity.world_id) &&
    typeof value.region.client_build === "string" &&
    typeof value.region.protocol_pack_digest === "string"
  );
}

function isTopic(value: unknown): value is EventViewerTopic {
  return (
    typeof value === "string" &&
    (EVENT_VIEWER_TOPICS as readonly string[]).includes(value)
  );
}

function isOptionalString(value: unknown): boolean {
  return value === null || typeof value === "string";
}

function isSafeCounter(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
