import type { WorkspaceTabDescriptor } from "./types";

export interface OrderedTabSection {
  id: string;
  tabs: WorkspaceTabDescriptor[];
}

export function orderTabSections(
  tabs: readonly WorkspaceTabDescriptor[],
  savedTabOrder: readonly string[],
  savedSectionOrder: readonly string[],
): OrderedTabSection[] {
  const defaultTabs = [...tabs].sort(
    (left, right) =>
      left.defaultOrder - right.defaultOrder ||
      left.contributorPluginId.localeCompare(right.contributorPluginId) ||
      left.id.localeCompare(right.id),
  );
  const tabsById = new Map(defaultTabs.map((tab) => [tab.id, tab]));
  const orderedTabIds = mergeKnownOrder(
    defaultTabs.map((tab) => tab.id),
    savedTabOrder,
  );
  const sections = new Map<string, OrderedTabSection>();

  for (const tabId of orderedTabIds) {
    const tab = tabsById.get(tabId);
    if (tab === undefined) {
      continue;
    }
    let section = sections.get(tab.sectionId);
    if (section === undefined) {
      section = { id: tab.sectionId, tabs: [] };
      sections.set(tab.sectionId, section);
    }
    section.tabs.push(tab);
  }

  return mergeKnownOrder([...sections.keys()], savedSectionOrder)
    .map((sectionId) => sections.get(sectionId))
    .filter((section): section is OrderedTabSection => section !== undefined);
}

export function moveTabInsideSection(
  sections: readonly OrderedTabSection[],
  sourceTabId: string,
  targetTabId: string,
): string[] | null {
  const sourceSection = sections.find((section) =>
    section.tabs.some((tab) => tab.id === sourceTabId),
  );
  const targetSection = sections.find((section) =>
    section.tabs.some((tab) => tab.id === targetTabId),
  );
  if (
    sourceSection === undefined ||
    targetSection === undefined ||
    sourceSection.id !== targetSection.id
  ) {
    return null;
  }

  return sections.flatMap((section) =>
    section.id === sourceSection.id
      ? moveToTargetIndex(
          section.tabs.map((tab) => tab.id),
          sourceTabId,
          targetTabId,
        )
      : section.tabs.map((tab) => tab.id),
  );
}

export function moveSection(
  sections: readonly OrderedTabSection[],
  sourceSectionId: string,
  targetSectionId: string,
): string[] {
  return moveToTargetIndex(
    sections.map((section) => section.id),
    sourceSectionId,
    targetSectionId,
  );
}

function mergeKnownOrder(
  defaults: readonly string[],
  saved: readonly string[],
): string[] {
  const known = new Set(defaults);
  const seen = new Set<string>();
  const result: string[] = [];
  for (const id of saved) {
    if (known.has(id) && !seen.has(id)) {
      seen.add(id);
      result.push(id);
    }
  }
  for (const id of defaults) {
    if (!seen.has(id)) {
      seen.add(id);
      result.push(id);
    }
  }
  return result;
}

function moveToTargetIndex(
  order: readonly string[],
  sourceId: string,
  targetId: string,
): string[] {
  const sourceIndex = order.indexOf(sourceId);
  const targetIndex = order.indexOf(targetId);
  if (sourceId === targetId || sourceIndex < 0 || targetIndex < 0) {
    return [...order];
  }
  const result = [...order];
  result.splice(sourceIndex, 1);
  result.splice(targetIndex, 0, sourceId);
  return result;
}
