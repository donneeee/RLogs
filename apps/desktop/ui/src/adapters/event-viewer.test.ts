import { describe, expect, it } from "vitest";

import {
  parseEventViewerPage,
  parseLiveEventBatch,
  parseLiveEventDetail,
} from "./event-viewer";

function examplePage(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    queryId: "events-1",
    sessionId: "fixture",
    artifactDigest: "sha256:fixture",
    header: {
      schema_version: 1,
      event_schema_version: 1,
      session_id: "fixture",
      producer: "test",
      region: {
        identity: {
          deployment_id: "global",
          region_id: "north-america",
          realm_id: null,
          world_id: "asteria",
        },
        client_build: "build",
        protocol_pack_digest: "sha256:pack",
      },
    },
    filter: { topic: "combat", kind: "damage", search: null },
    pageIndex: 1,
    scannedThisPage: 1,
    scannedTotal: 1,
    matchedTotal: 1,
    integrityVerified: true,
    complete: true,
    events: [
      {
        sequence: 1,
        timelineSequence: 1,
        observedMicros: 42,
        gameTimeMillis: null,
        topic: "combat",
        kind: "damage",
        summary:
          "entity:9223372036854775807 [actor:1] -> entity:-9223372036854775808 [actor:2]",
        amount: "9223372036854775807",
        identifiers: {
          actor: null,
          source: {
            actorId: "1",
            entityUuid: "9223372036854775807",
          },
          directSource: null,
          target: {
            actorId: "2",
            entityUuid: "-9223372036854775808",
          },
          ability: "9007199254740993",
          status: null,
          statusInstance: null,
          statusOriginType: null,
          statusOriginConfig: null,
          statusState: null,
          statusStacks: null,
          statusDurationMillis: null,
          monster: null,
          scene: null,
          map: null,
          dungeon: null,
          characterId: null,
        },
        canonicalJson:
          '{"sequence":1,"event":{"type":"timeline","data":{"entity_uuid":9223372036854775807}}}',
      },
    ],
  };
}

describe("Event Viewer page contract", () => {
  it("preserves 64-bit canonical values as strings", () => {
    const page = parseEventViewerPage(examplePage());

    expect(page.events[0]?.identifiers.source?.entityUuid).toBe(
      "9223372036854775807",
    );
    expect(page.events[0]?.identifiers.ability).toBe("9007199254740993");
    expect(page.events[0]?.canonicalJson).toContain("9223372036854775807");
  });

  it("rejects numeric identifiers that JavaScript could round", () => {
    const page = examplePage();
    const events = page.events as Array<Record<string, unknown>>;
    const identifiers = events[0]?.identifiers as Record<string, unknown>;
    identifiers.ability = 9_007_199_254_740_992;

    expect(() => parseEventViewerPage(page)).toThrow(
      "invalid Event Viewer page",
    );
  });

  it("accepts compact ID-first live lines without canonical JSON", () => {
    const event = (examplePage().events as Array<Record<string, unknown>>)[0]!;
    const batch = parseLiveEventBatch({
      schemaVersion: 1,
      sessionId: "live",
      revision: 12,
      droppedBefore: 0,
      hasMore: false,
      events: [
        {
          revision: 12,
          sequence: event.sequence,
          observedMicros: event.observedMicros,
          topic: event.topic,
          kind: event.kind,
          rawIds: event.summary,
        },
      ],
    });

    expect(batch.events[0]?.rawIds).toContain("9223372036854775807");
    expect("canonicalJson" in batch.events[0]!).toBe(false);
    expect(batch.capacityBytes).toBe(0);
  });

  it("accepts and validates bounded Event Inspector telemetry", () => {
    const batch = parseLiveEventBatch({
      schemaVersion: 2,
      sessionId: "live",
      revision: 12,
      droppedBefore: 3,
      hasMore: false,
      retainedEvents: 120,
      retainedBytes: 65_536,
      capacityEvents: 8_192,
      capacityBytes: 4_194_304,
      events: [],
    });

    expect(batch.retainedEvents).toBe(120);
    expect(batch.capacityBytes).toBe(4_194_304);
  });

  it("rejects Event Inspector telemetry outside its declared bounds", () => {
    expect(() =>
      parseLiveEventBatch({
        schemaVersion: 2,
        sessionId: "live",
        revision: 12,
        droppedBefore: 0,
        hasMore: false,
        retainedEvents: 9,
        retainedBytes: 33,
        capacityEvents: 8,
        capacityBytes: 32,
        events: [],
      }),
    ).toThrow("invalid Event Inspector memory bounds");
  });

  it("accepts bounded selected-event canonical fields", () => {
    const detail = parseLiveEventDetail({
      schemaVersion: 1,
      sessionId: "capture-1",
      revision: 14,
      sequence: 9,
      timelineSequence: 7,
      observedMicros: 12_345,
      sourceKind: "canonical",
      gameTimeMillis: null,
      topic: "combat",
      kind: "damage",
      fields: [{
        path: "event.ability_id",
        label: "Ability ID",
        value: "2203521",
        valueType: "u32",
        usableInTrigger: true,
      }],
      protocolCaptureSequence: 33,
      protocol: null,
    });

    expect(detail.fields[0]?.path).toBe("event.ability_id");
    expect(detail.fields[0]?.usableInTrigger).toBe(true);
    expect(detail.protocolCaptureSequence).toBe(33);
  });

  it("accepts privacy-reviewed local protobuf detail", () => {
    const detail = parseLiveEventDetail({
      schemaVersion: 1,
      sessionId: "capture-1",
      revision: 14,
      sequence: 9,
      timelineSequence: 7,
      observedMicros: 12_345,
      sourceKind: "canonical",
      gameTimeMillis: null,
      topic: "combat",
      kind: "damage",
      fields: [],
      protocolCaptureSequence: 33,
      protocol: {
        schemaVersion: 1,
        captureSequence: 33,
        observedMicros: 12_345,
        connectionId: 2,
        streamId: 4,
        direction: "ServerToClient",
        fragment: "Notify",
        compression: "NotCompressed",
        serviceId: 100,
        methodId: 200,
        stubId: 3,
        callId: null,
        serviceName: "WorldNtf",
        methodName: "SyncNearDelta",
        messageName: "SyncNearDeltaNtf",
        domain: "combat",
        decodeStatus: "decoded",
        applicationBytes: 3,
        payloadRetained: true,
        omissionReason: null,
        fields: [{
          path: "field.1[0]",
          fieldNumber: 1,
          wireType: "varint",
          value: "150 (0x96)",
        }],
        truncated: false,
        parseError: null,
      },
    });

    expect(detail.protocol?.serviceName).toBe("WorldNtf");
    expect(detail.protocol?.fields[0]?.value).toBe("150 (0x96)");
  });

  it("accepts protocol-only rows in feed schema 3", () => {
    const batch = parseLiveEventBatch({
      schemaVersion: 3,
      sessionId: "live",
      revision: 20,
      droppedBefore: 0,
      hasMore: false,
      retainedEvents: 1,
      retainedBytes: 256,
      capacityEvents: 8_192,
      capacityBytes: 4_194_304,
      events: [{
        revision: 20,
        sequence: 33,
        observedMicros: 55_000,
        sourceKind: "protocol",
        topic: "world",
        kind: "protocol_message",
        rawIds: "WorldNtf.SyncNearDelta · service:100 · method:200",
      }],
    });

    expect(batch.events[0]?.sourceKind).toBe("protocol");
    expect(batch.events[0]?.kind).toBe("protocol_message");
  });
});
