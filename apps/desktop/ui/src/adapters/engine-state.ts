import type { EngineState } from "../shell/types";

export interface RuntimeEngineSnapshot {
  phase: "idle" | "processing" | "complete" | "failed";
  detail: string;
  live_capture_can_stop: boolean;
  saving_run?: boolean;
}

export function engineStateFromRuntime(
  snapshot: RuntimeEngineSnapshot,
): EngineState {
  if (snapshot.phase === "processing" && snapshot.live_capture_can_stop) {
    return {
      phase: "capturing",
      label: "Monitoring",
      detail: snapshot.saving_run
        ? "Recording this dungeon."
        : "Watching the game. Waiting for a dungeon.",
      technicalDetail: snapshot.detail,
    };
  }
  if (snapshot.phase === "processing") {
    return {
      phase: "processing",
      label: "Saving run",
      detail: "Saving your latest run.",
      technicalDetail: snapshot.detail,
    };
  }
  if (snapshot.phase === "complete") {
    return {
      phase: "complete",
      label: "Log ready",
      detail: "Your latest run is ready.",
      technicalDetail: snapshot.detail,
    };
  }
  if (snapshot.phase === "failed") {
    return {
      phase: "failed",
      label: "Something went wrong",
      detail: "Open Settings to review the app and network status.",
      technicalDetail: snapshot.detail,
    };
  }
  return {
    phase: "idle",
    label: "Core ready",
    detail: "Ready. Open the game to begin.",
    technicalDetail: snapshot.detail,
  };
}
