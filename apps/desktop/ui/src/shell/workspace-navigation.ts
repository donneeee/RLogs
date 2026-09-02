export const WORKSPACE_NAVIGATION_EVENT = "rlogs:navigate-workspace";

export interface WorkspaceNavigationRequest {
  workspaceId: string;
  entrypoint: string;
}

export function requestWorkspaceNavigation(
  request: WorkspaceNavigationRequest,
): void {
  window.dispatchEvent(
    new CustomEvent<WorkspaceNavigationRequest>(WORKSPACE_NAVIGATION_EVENT, {
      detail: request,
    }),
  );
}

export function readWorkspaceNavigationRequest(
  event: Event,
): WorkspaceNavigationRequest | null {
  const detail = (event as CustomEvent<unknown>).detail;
  if (
    typeof detail !== "object" ||
    detail === null ||
    !("workspaceId" in detail) ||
    !("entrypoint" in detail) ||
    typeof detail.workspaceId !== "string" ||
    typeof detail.entrypoint !== "string" ||
    detail.workspaceId.length === 0 ||
    detail.entrypoint.length === 0
  ) {
    return null;
  }
  return {
    workspaceId: detail.workspaceId,
    entrypoint: detail.entrypoint,
  };
}
