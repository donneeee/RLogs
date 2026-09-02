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
  statusInstance: string | null;
  statusOriginType: string | null;
  statusOriginConfig: string | null;
  statusState: string | null;
  statusStacks: string | null;
  statusDurationMillis: string | null;
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

export interface LiveEventLine {
  revision: number;
  sequence: number;
  observedMicros: number;
  sourceKind: "canonical" | "protocol";
  topic: EventViewerTopic;
  kind: string;
  rawIds: string;
}

export interface LiveEventBatch {
  schemaVersion: 1 | 2 | 3;
  sessionId: string | null;
  revision: number;
  droppedBefore: number;
  hasMore: boolean;
  retainedEvents: number;
  retainedBytes: number;
  capacityEvents: number;
  capacityBytes: number;
  events: readonly LiveEventLine[];
}

export interface LiveEventDetailField {
  path: string;
  label: string;
  value: string;
  valueType: string;
  usableInTrigger: boolean;
}

export interface LiveProtocolField {
  path: string;
  fieldNumber: number;
  wireType: "varint" | "fixed64" | "length_delimited" | "fixed32";
  value: string;
}

export interface LiveProtocolDetail {
  schemaVersion: 1;
  captureSequence: number;
  observedMicros: number;
  connectionId: number;
  streamId: number;
  direction: string;
  fragment: string;
  compression: string;
  serviceId: number;
  methodId: number;
  stubId: number;
  callId: number | null;
  serviceName: string;
  methodName: string;
  messageName: string | null;
  domain: string;
  decodeStatus: string;
  applicationBytes: number;
  payloadRetained: boolean;
  omissionReason: string | null;
  fields: readonly LiveProtocolField[];
  truncated: boolean;
  parseError: string | null;
}

export interface LiveEventDetail {
  schemaVersion: 1;
  sessionId: string;
  revision: number;
  sequence: number;
  timelineSequence: number | null;
  observedMicros: number;
  sourceKind: "canonical" | "protocol";
  gameTimeMillis: number | null;
  topic: EventViewerTopic;
  kind: string;
  fields: readonly LiveEventDetailField[];
  protocolCaptureSequence: number | null;
  protocol: LiveProtocolDetail | null;
}

/**
 * A new or resumed Inspector deliberately joins at the bounded live tail.
 * Events older than that tail were never part of this viewer session, so they
 * must not be reported as viewer gaps. Later batches keep their authoritative
 * dropped-before count unchanged.
 */
export function acknowledgeInitialLiveTail(batch: LiveEventBatch): LiveEventBatch {
  return batch.droppedBefore === 0 ? batch : { ...batch, droppedBefore: 0 };
}

export function parseLiveEventBatch(value: unknown): LiveEventBatch {
  if (
    !isRecord(value) ||
    (value.schemaVersion !== 1 && value.schemaVersion !== 2 && value.schemaVersion !== 3) ||
    !isOptionalString(value.sessionId) ||
    !isSafeCounter(value.revision) ||
    !isSafeCounter(value.droppedBefore) ||
    typeof value.hasMore !== "boolean" ||
    !Array.isArray(value.events) ||
    !value.events.every(isLiveEvent)
  ) {
    throw new Error("The native host returned an invalid live Event Viewer batch.");
  }
  if (
    value.schemaVersion >= 2 &&
    (!isSafeCounter(value.retainedEvents) ||
      !isSafeCounter(value.retainedBytes) ||
      !isSafeCounter(value.capacityEvents) ||
      !isSafeCounter(value.capacityBytes) ||
      value.retainedEvents > value.capacityEvents ||
      value.retainedBytes > value.capacityBytes)
  ) {
    throw new Error("The native host returned invalid Event Inspector memory bounds.");
  }
  const bounded = value.schemaVersion >= 2;
  return {
    ...(value as unknown as Omit<LiveEventBatch, "retainedEvents" | "retainedBytes" | "capacityEvents" | "capacityBytes" | "events">),
    retainedEvents: bounded ? (value.retainedEvents as number) : 0,
    retainedBytes: bounded ? (value.retainedBytes as number) : 0,
    capacityEvents: bounded ? (value.capacityEvents as number) : 0,
    capacityBytes: bounded ? (value.capacityBytes as number) : 0,
    events: value.events.map((event) => ({
      ...event,
      sourceKind: event.sourceKind ?? "canonical",
    })),
  };
}

export function parseLiveEventDetail(value: unknown): LiveEventDetail {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    typeof value.sessionId !== "string" ||
    !isSafeCounter(value.revision) ||
    !isSafeCounter(value.sequence) ||
    (value.timelineSequence !== null && !isSafeCounter(value.timelineSequence)) ||
    !isSafeCounter(value.observedMicros) ||
    !isLiveSourceKind(value.sourceKind) ||
    (value.gameTimeMillis !== null && !Number.isSafeInteger(value.gameTimeMillis)) ||
    !isTopic(value.topic) ||
    typeof value.kind !== "string" ||
    !Array.isArray(value.fields) ||
    !value.fields.every(isLiveEventDetailField) ||
    (value.protocolCaptureSequence !== null &&
      !isSafeCounter(value.protocolCaptureSequence)) ||
    (value.protocol !== null && !isLiveProtocolDetail(value.protocol))
  ) {
    throw new Error("The native host returned invalid selected-event detail.");
  }
  return value as unknown as LiveEventDetail;
}

function isLiveProtocolDetail(value: unknown): value is LiveProtocolDetail {
  return (
    isRecord(value) &&
    value.schemaVersion === 1 &&
    isSafeCounter(value.captureSequence) &&
    isSafeCounter(value.observedMicros) &&
    isSafeCounter(value.connectionId) &&
    isSafeCounter(value.streamId) &&
    typeof value.direction === "string" &&
    typeof value.fragment === "string" &&
    typeof value.compression === "string" &&
    isSafeCounter(value.serviceId) &&
    isSafeCounter(value.methodId) &&
    isSafeCounter(value.stubId) &&
    (value.callId === null || isSafeCounter(value.callId)) &&
    typeof value.serviceName === "string" &&
    typeof value.methodName === "string" &&
    isOptionalString(value.messageName) &&
    typeof value.domain === "string" &&
    typeof value.decodeStatus === "string" &&
    isSafeCounter(value.applicationBytes) &&
    typeof value.payloadRetained === "boolean" &&
    isOptionalString(value.omissionReason) &&
    Array.isArray(value.fields) &&
    value.fields.every(isLiveProtocolField) &&
    typeof value.truncated === "boolean" &&
    isOptionalString(value.parseError)
  );
}

function isLiveProtocolField(value: unknown): value is LiveProtocolField {
  return (
    isRecord(value) &&
    typeof value.path === "string" &&
    isSafeCounter(value.fieldNumber) &&
    ["varint", "fixed64", "length_delimited", "fixed32"].includes(
      String(value.wireType),
    ) &&
    typeof value.value === "string"
  );
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

function isLiveEvent(value: unknown): value is LiveEventLine {
  return (
    isRecord(value) &&
    isSafeCounter(value.revision) &&
    isSafeCounter(value.sequence) &&
    isSafeCounter(value.observedMicros) &&
    (value.sourceKind === undefined || isLiveSourceKind(value.sourceKind)) &&
    isTopic(value.topic) &&
    typeof value.kind === "string" &&
    typeof value.rawIds === "string"
  );
}

function isLiveSourceKind(value: unknown): value is LiveEventLine["sourceKind"] {
  return value === "canonical" || value === "protocol";
}

function isLiveEventDetailField(value: unknown): value is LiveEventDetailField {
  return (
    isRecord(value) &&
    typeof value.path === "string" &&
    value.path.length > 0 &&
    typeof value.label === "string" &&
    typeof value.value === "string" &&
    typeof value.valueType === "string" &&
    typeof value.usableInTrigger === "boolean"
  );
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
    isOptionalString(value.statusInstance) &&
    isOptionalString(value.statusOriginType) &&
    isOptionalString(value.statusOriginConfig) &&
    isOptionalString(value.statusState) &&
    isOptionalString(value.statusStacks) &&
    isOptionalString(value.statusDurationMillis) &&
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
