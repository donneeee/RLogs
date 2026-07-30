import type { WorkspaceDescriptor } from "./types";

export function mergeWorkspaceOrder(
  workspaces: readonly WorkspaceDescriptor[],
  savedOrder: readonly string[],
): string[] {
  const byId = new Map(workspaces.map((workspace) => [workspace.id, workspace]));
  const merged = savedOrder.filter((id, index) => {
    return byId.has(id) && savedOrder.indexOf(id) === index;
  });
  const alreadyPlaced = new Set(merged);
  const newWorkspaces = workspaces
    .filter((workspace) => !alreadyPlaced.has(workspace.id))
    .slice()
    .sort((left, right) => {
      return (
        left.defaultOrder - right.defaultOrder ||
        left.name.localeCompare(right.name) ||
        left.id.localeCompare(right.id)
      );
    });

  return [...merged, ...newWorkspaces.map((workspace) => workspace.id)];
}

export function moveWorkspace(
  order: readonly string[],
  sourceId: string,
  targetId: string,
): string[] {
  if (sourceId === targetId) {
    return [...order];
  }
  const sourceIndex = order.indexOf(sourceId);
  const targetIndex = order.indexOf(targetId);
  if (sourceIndex < 0 || targetIndex < 0) {
    return [...order];
  }

  const next = [...order];
  next.splice(sourceIndex, 1);
  next.splice(targetIndex, 0, sourceId);
  return next;
}

export function moveWorkspaceByOffset(
  order: readonly string[],
  workspaceId: string,
  offset: -1 | 1,
): string[] {
  const sourceIndex = order.indexOf(workspaceId);
  const targetIndex = sourceIndex + offset;
  if (sourceIndex < 0 || targetIndex < 0 || targetIndex >= order.length) {
    return [...order];
  }
  const targetId = order[targetIndex];
  return targetId === undefined
    ? [...order]
    : moveWorkspace(order, workspaceId, targetId);
}
