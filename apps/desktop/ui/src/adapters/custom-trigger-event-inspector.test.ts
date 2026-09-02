import { describe, expect, it } from "vitest";

import { compareEventInspectorDetails } from "./custom-trigger-event-inspector";
import type { LiveEventDetail } from "./event-viewer";

function detail(
  amount: string,
  extraFields: LiveEventDetail["fields"] = [],
  protocolValue = "7",
): LiveEventDetail {
  return {
    schemaVersion: 1,
    sessionId: "session-1",
    revision: 1,
    sequence: 1,
    timelineSequence: 1,
    observedMicros: 1_000,
    sourceKind: "canonical",
    gameTimeMillis: 10,
    topic: "combat",
    kind: "damage",
    fields: [
      {
        path: "event.ability_id",
        label: "Ability ID",
        value: "2203521",
        valueType: "u32",
        usableInTrigger: true,
      },
      {
        path: "event.amount",
        label: "Amount",
        value: amount,
        valueType: "i64",
        usableInTrigger: false,
      },
      ...extraFields,
    ],
    protocolCaptureSequence: 4,
    protocol: {
      schemaVersion: 1,
      captureSequence: 4,
      observedMicros: 1_000,
      connectionId: 1,
      streamId: 2,
      direction: "ServerToClient",
      fragment: "Notify",
      compression: "NotCompressed",
      serviceId: 3,
      methodId: 4,
      stubId: 5,
      callId: null,
      serviceName: "WorldNtf",
      methodName: "SyncNearDelta",
      messageName: "SyncNearDeltaNtf",
      domain: "combat",
      decodeStatus: "decoded",
      applicationBytes: 8,
      payloadRetained: true,
      omissionReason: null,
      fields: [{
        path: "field.1[0]",
        fieldNumber: 1,
        wireType: "varint",
        value: protocolValue,
      }],
      truncated: false,
      parseError: null,
    },
  };
}

describe("Event Inspector pinned-event comparison", () => {
  it("diffs canonical and decoded protobuf fields without rounding values", () => {
    const before = detail("9223372036854775807");
    const after = detail(
      "9223372036854775806",
      [{
        path: "event.status_id",
        label: "Status ID",
        value: "9007199254740993",
        valueType: "u64",
        usableInTrigger: true,
      }],
      "8",
    );

    const differences = compareEventInspectorDetails(before, after);

    expect(differences).toEqual(expect.arrayContaining([
      expect.objectContaining({
        source: "canonical",
        path: "event.amount",
        before: "9223372036854775807",
        after: "9223372036854775806",
        changed: true,
      }),
      expect.objectContaining({
        source: "canonical",
        path: "event.status_id",
        before: null,
        after: "9007199254740993",
        changed: true,
      }),
      expect.objectContaining({
        source: "protocol",
        path: "field.1[0]",
        before: "7",
        after: "8",
        changed: true,
      }),
      expect.objectContaining({
        source: "canonical",
        path: "event.ability_id",
        changed: false,
      }),
    ]));
    expect(differences.slice(0, 3).every((row) => row.changed)).toBe(true);
  });
});
