import { describe, expect, it } from "vitest";

import { engineStateFromRuntime } from "./engine-state";

describe("shell engine state", () => {
  it("distinguishes an active capture from post-processing", () => {
    const monitoring = engineStateFromRuntime({
      phase: "processing",
      detail: "Monitoring exact game-owned flows.",
      live_capture_can_stop: true,
    });
    expect(monitoring.phase).toBe("capturing");
    expect(monitoring.label).toBe("Monitoring");
    expect(monitoring.detail).toBe(
      "Watching the game. Waiting for a dungeon.",
    );
    expect(monitoring.technicalDetail).toBe(
      "Monitoring exact game-owned flows.",
    );
    expect(
      engineStateFromRuntime({
        phase: "processing",
        detail: "Decoding and sealing.",
        live_capture_can_stop: false,
      }).phase,
    ).toBe("processing");
  });

  it("uses a direct recording message while a dungeon is being saved", () => {
    const recording = engineStateFromRuntime({
      phase: "processing",
      detail: "Technical packet and decoder counters.",
      live_capture_can_stop: true,
      saving_run: true,
    });
    expect(recording.label).toBe("Monitoring");
    expect(recording.detail).toBe("Recording this dungeon.");
    expect(recording.technicalDetail).toBe(
      "Technical packet and decoder counters.",
    );
  });

  it("surfaces completed and failed sessions", () => {
    expect(
      engineStateFromRuntime({
        phase: "complete",
        detail: "Log ready.",
        live_capture_can_stop: false,
      }).label,
    ).toBe("Log ready");
    expect(
      engineStateFromRuntime({
        phase: "failed",
        detail: "Decoder failed.",
        live_capture_can_stop: false,
      }).label,
    ).toBe("Something went wrong");
  });
});
