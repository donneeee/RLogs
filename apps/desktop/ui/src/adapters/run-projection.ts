const RUN_PROJECTION_SCHEMA_ID = "app.rlogs.encounter-recorder.runs";

interface RunProjectionOutput {
  type: "snapshot";
  schema_id: typeof RUN_PROJECTION_SCHEMA_ID;
  payload: {
    runs: unknown[];
  };
}

export function projectedRunCount(outputs: readonly unknown[]): number {
  for (const output of outputs) {
    if (isRunProjectionOutput(output)) {
      return output.payload.runs.length;
    }
  }
  return 0;
}

function isRunProjectionOutput(value: unknown): value is RunProjectionOutput {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const output = value as Record<string, unknown>;
  if (
    output.type !== "snapshot" ||
    output.schema_id !== RUN_PROJECTION_SCHEMA_ID ||
    typeof output.payload !== "object" ||
    output.payload === null
  ) {
    return false;
  }
  const payload = output.payload as Record<string, unknown>;
  return Array.isArray(payload.runs);
}
