import {
  EVENT_VIEWER_TOPICS,
  type EventViewerTopic,
  type LiveEventBatch,
  type LiveEventDetail,
  type LiveEventLine,
} from "./event-viewer";
import {
  createCustomTriggerDraft,
  readStoredDrafts,
  storeCustomTriggerDraft,
  type CustomTriggerDraft,
} from "./custom-trigger-draft";
import type { MountedSurface } from "../shell/types";

const MAXIMUM_VISIBLE_EVENTS = 1_000;
const MAXIMUM_PINNED_EVENTS = 16;

export interface EventInspectorDependencies {
  subscribe(
    onBatch: (batch: LiveEventBatch) => void,
    onError: (error: unknown) => void,
  ): () => void;
  detail(sessionId: string, event: LiveEventLine): Promise<LiveEventDetail>;
}

export function mountCustomTriggerEventInspector(
  container: HTMLElement,
  dependencies: EventInspectorDependencies,
): MountedSurface {
  let alive = true;
  let frozen = false;
  let activeSessionId: string | null = null;
  let selected: LiveEventLine | null = null;
  let selectedDetail: LiveEventDetail | null = null;
  let selectedDetailError: string | null = null;
  let selectedDetailRequest = 0;
  let selectedFieldPaths = new Set<string>();
  let currentDraft: CustomTriggerDraft | null = null;
  let visibleEvents: LiveEventLine[] = [];
  let pinnedEvents: LiveEventLine[] = [];
  let viewerDropped = 0;

  const root = element("div", "plugin-surface event-inspector-surface");
  const intro = element("section", "content-card event-inspector-intro");
  const introCopy = element("div");
  introCopy.append(
    text("span", "LIVE EVENT DISCOVERY", "eyebrow"),
    text("h2", "Event Inspector"),
    text(
      "p",
      "Watch bounded, privacy-reviewed canonical events and decoded protocol messages, isolate what an in-game action produced, and preserve exact IDs for trigger creation.",
      "card-copy",
    ),
  );
  const connection = text("span", "CONNECTING", "event-inspector-state");
  connection.dataset.state = "waiting";
  intro.append(introCopy, connection);

  const telemetry = element("section", "content-card event-inspector-telemetry");
  const sessionMetric = metric("Session", "Waiting for capture");
  const retainedMetric = metric("Host ring", "0 rows");
  const memoryMetric = metric("Memory", "0 B");
  const gapMetric = metric("Viewer gaps", "0");
  telemetry.append(
    sessionMetric.root,
    retainedMetric.root,
    memoryMetric.root,
    gapMetric.root,
  );

  const controls = element("section", "content-card event-inspector-controls");
  const filterGrid = element("div", "event-inspector-filter-grid");
  const source = selectField("Source", "All sources", [
    ["Canonical events", "canonical"],
    ["Protocol messages", "protocol"],
  ]);
  const topic = selectField(
    "Topic",
    "All topics",
    EVENT_VIEWER_TOPICS.map((value) => [formatIdentifier(value), value]),
  );
  const kind = inputField("Event kind", "damage, status, scene…");
  const search = inputField("IDs or values", "source, target, ability, status…", "search");
  filterGrid.append(source.label, topic.label, kind.label, search.label);
  const controlActions = element("div", "runtime-card-actions event-inspector-actions");
  const pause = button("Freeze log", "secondary-button");
  const clear = button("Clear visible events", "quiet-button");
  const status = text(
    "span",
    "Waiting for the game parser to publish decoded events.",
    "runtime-action-message",
  );
  controlActions.append(pause, clear, status);
  controls.append(filterGrid, controlActions);

  const workspace = element("section", "event-inspector-workspace");
  const streamCard = element("section", "content-card event-inspector-stream-card");
  const streamHeader = element("header", "card-heading");
  const streamHeading = element("div");
  streamHeading.append(
    text("span", "ACKNOWLEDGED REVIEW STREAM", "eyebrow"),
    text("h2", "Live stream"),
  );
  const visibleCount = text("span", "0 visible", "event-inspector-count");
  streamHeader.append(streamHeading, visibleCount);
  const stream = element("div", "event-inspector-stream");
  stream.setAttribute("role", "log");
  stream.setAttribute("aria-label", "Live decoded events");
  stream.setAttribute("aria-live", "off");
  streamCard.append(streamHeader, stream);

  const detailCard = element("aside", "content-card event-inspector-detail");
  const detailHeader = element("header", "card-heading");
  const detailHeading = element("div");
  detailHeading.append(
    text("span", "SELECTED EVENT", "eyebrow"),
    text("h2", "Nothing selected"),
  );
  detailHeader.append(detailHeading);
  const detailBody = element("div", "event-inspector-detail-body");
  const detailEmpty = text(
    "p",
    "Select a live row to inspect its exact sequence, timing, reviewed IDs, and source protobuf when available.",
    "runtime-empty-result",
  );
  detailBody.append(detailEmpty);
  const detailActions = element("div", "runtime-card-actions event-inspector-detail-actions");
  const pin = button("Pin event", "secondary-button");
  const createRule = button("Create trigger from event", "primary-button");
  pin.disabled = true;
  createRule.disabled = true;
  const detailMessage = text(
    "span",
    "Choose an event, then select the exact canonical fields the trigger should match.",
    "runtime-action-message",
  );
  detailActions.append(pin, createRule, detailMessage);
  detailCard.append(detailHeader, detailBody, detailActions);
  workspace.append(streamCard, detailCard);

  const draftCard = element("section", "content-card event-inspector-draft");
  const draftHeader = element("header", "card-heading");
  const draftHeading = element("div");
  draftHeading.append(
    text("span", "DISABLED UNTIL REVIEWED", "eyebrow"),
    text("h2", "Trigger draft"),
  );
  const storedDraftCount = text(
    "span",
    `${readStoredDrafts(window.localStorage).length} saved locally`,
    "event-inspector-count",
  );
  draftHeader.append(draftHeading, storedDraftCount);
  const draftBody = element("div", "event-inspector-draft-body");
  draftBody.append(
    text(
      "p",
      "Select trigger-safe fields from an event to create a readable, disabled When draft. No actions execute from the Inspector.",
      "runtime-empty-result",
    ),
  );
  draftCard.append(draftHeader, draftBody);

  const pinsCard = element("section", "content-card event-inspector-pins");
  const pinsHeader = element("header", "card-heading");
  const pinsHeading = element("div");
  pinsHeading.append(
    text("span", "BOUNDED WORKING SET", "eyebrow"),
    text("h2", "Pinned events"),
  );
  const clearPins = button("Clear pins", "quiet-button");
  clearPins.disabled = true;
  pinsHeader.append(pinsHeading, clearPins);
  const pins = element("div", "event-inspector-pin-list");
  pins.append(
    text(
      "p",
      `Pin up to ${MAXIMUM_PINNED_EVENTS} events for later field comparison.`,
      "runtime-empty-result",
    ),
  );
  pinsCard.append(pinsHeader, pins);

  const boundary = element("section", "event-inspector-boundary");
  boundary.append(
    text("strong", "Local and bounded"),
    text(
      "span",
      "This view receives compact canonical summaries and only allowlisted decoded gameplay messages. It cannot read login, account-secret, private-chat, opaque, prohibited, or raw encrypted payload data, and it never uploads inspection data.",
    ),
  );

  root.append(intro, telemetry, controls, workspace, draftCard, pinsCard, boundary);
  container.replaceChildren(root);

  const matches = (event: LiveEventLine): boolean => {
    if (source.select.value !== "" && event.sourceKind !== source.select.value) return false;
    if (topic.select.value !== "" && event.topic !== topic.select.value) return false;
    const requestedKind = kind.input.value.trim().toLowerCase();
    if (requestedKind !== "" && !event.kind.toLowerCase().includes(requestedKind)) return false;
    const requestedText = search.input.value.trim().toLowerCase();
    if (requestedText === "") return true;
    return (
      event.rawIds.toLowerCase().includes(requestedText) ||
      event.kind.toLowerCase().includes(requestedText) ||
      event.topic.toLowerCase().includes(requestedText) ||
      String(event.sequence).includes(requestedText)
    );
  };

  const selectEvent = async (event: LiveEventLine) => {
    selected = event;
    selectedDetail = null;
    selectedDetailError = null;
    selectedFieldPaths = new Set();
    const request = ++selectedDetailRequest;
    renderSelection();
    renderStream();
    if (activeSessionId === null) {
      selectedDetailError = "The capture session ended before this event could be inspected.";
      renderSelection();
      return;
    }
    try {
      const detail = await dependencies.detail(activeSessionId, event);
      if (!alive || request !== selectedDetailRequest) return;
      selectedDetail = detail;
      selectedFieldPaths = defaultTriggerFieldPaths(detail);
      renderSelection();
    } catch (error) {
      if (!alive || request !== selectedDetailRequest) return;
      selectedDetailError = errorMessage(error);
      renderSelection();
    }
  };

  const renderStream = () => {
    const matching = visibleEvents.filter(matches);
    visibleCount.textContent = `${matching.length.toLocaleString()} visible`;
    const fragment = document.createDocumentFragment();
    if (viewerDropped > 0) {
      const reasons = [];
      if (viewerDropped > 0) reasons.push(`${viewerDropped.toLocaleString()} left a bounded viewer buffer`);
      fragment.append(
        text(
          "div",
          `[viewer notice] ${reasons.join("; ")}. Capture and the authoritative canonical writer continued.`,
          "event-live-line event-live-gap",
        ),
      );
    }
    for (const event of matching) {
      const row = button("", "event-inspector-row");
      row.dataset.selected = String(selected?.revision === event.revision);
      row.dataset.source = event.sourceKind;
      row.setAttribute(
        "aria-label",
        `${formatIdentifier(event.sourceKind)} row ${event.sequence}, ${formatIdentifier(event.kind)}, ${event.rawIds}`,
      );
      row.append(
        text(
          "span",
          `${event.sourceKind === "protocol" ? "P" : "E"}#${event.sequence}`,
          "event-live-sequence",
        ),
        text("span", formatObservedMicros(event.observedMicros), "event-live-time"),
        text(
          "span",
          `${formatIdentifier(event.topic)} / ${formatIdentifier(event.kind)}`,
          "event-live-kind",
        ),
        text("code", event.rawIds, "event-live-ids"),
      );
      row.addEventListener("click", () => {
        void selectEvent(event);
      });
      fragment.append(row);
    }
    if (matching.length === 0) {
      fragment.append(
        text(
          "p",
          visibleEvents.length === 0
            ? "No live events have arrived yet."
            : "No retained events match these filters.",
          "runtime-empty-result",
        ),
      );
    }
    stream.replaceChildren(fragment);
    if (!frozen) stream.scrollTop = stream.scrollHeight;
  };

  const renderSelection = () => {
    if (selected === null) {
      detailMessage.classList.remove("error");
      detailHeading.querySelector("h2")!.textContent = "Nothing selected";
      detailBody.replaceChildren(detailEmpty);
      pin.disabled = true;
      createRule.disabled = true;
      detailMessage.textContent =
        "Choose an event, then select the exact canonical fields the trigger should match.";
      return;
    }
    detailHeading.querySelector("h2")!.textContent =
      `#${selected.sequence} · ${formatIdentifier(selected.kind)}`;
    const rows: HTMLElement[] = [
      detailRow("Revision", selected.revision.toLocaleString()),
      detailRow("Source", formatIdentifier(selected.sourceKind)),
      detailRow("Observed", formatObservedMicros(selected.observedMicros)),
      detailRow("Topic", formatIdentifier(selected.topic)),
      detailRow("Event", formatIdentifier(selected.kind)),
      detailRow("Raw canonical IDs", selected.rawIds, true),
    ];
    if (selectedDetailError !== null) {
      rows.push(text("p", selectedDetailError, "runtime-action-message error"));
      detailMessage.textContent = "Choose another retained event or wait for a new event.";
      createRule.disabled = true;
    } else if (selectedDetail === null) {
      rows.push(text("p", "Loading selected canonical fields…", "runtime-empty-result"));
      detailMessage.textContent = "Loading bounded selected-event detail.";
      createRule.disabled = true;
    } else {
      detailMessage.classList.remove("error");
      const fieldTree = element("div", "event-inspector-field-tree");
      const fieldHeading = element("header", "event-inspector-field-heading");
      fieldHeading.append(
        text("strong", selectedDetail.sourceKind === "protocol" ? "Reviewed route fields" : "Canonical fields"),
        text(
          "span",
          selectedDetail.sourceKind === "protocol" ? "Evidence only" : "Select fields to match",
          "event-inspector-count",
        ),
      );
      fieldTree.append(fieldHeading);
      for (const field of selectedDetail.fields) {
        const fieldRow = element("label", "event-inspector-field-row");
        fieldRow.dataset.triggerSafe = String(field.usableInTrigger);
        if (field.usableInTrigger) {
          const checkbox = element("input");
          checkbox.type = "checkbox";
          checkbox.checked = selectedFieldPaths.has(field.path);
          checkbox.setAttribute("aria-label", `Use ${field.label} in trigger`);
          checkbox.addEventListener("change", () => {
            if (checkbox.checked) selectedFieldPaths.add(field.path);
            else selectedFieldPaths.delete(field.path);
            renderSelection();
          });
          fieldRow.append(checkbox);
        } else {
          fieldRow.append(text("span", "•", "event-inspector-field-bullet"));
        }
        const fieldCopy = element("span", "event-inspector-field-copy");
        fieldCopy.append(
          text("strong", field.label),
          text("code", field.path),
        );
        fieldRow.append(
          fieldCopy,
          text("code", field.value, "event-inspector-field-value"),
          text("span", field.valueType, "event-inspector-field-type"),
        );
        fieldTree.append(fieldRow);
      }
      rows.push(fieldTree);
      const protocol = selectedDetail.protocol;
      const protocolTree = element("div", "event-inspector-protocol-tree");
      const protocolHeading = element("header", "event-inspector-field-heading");
      protocolHeading.append(
        text("strong", "Decoded protobuf"),
        text("span", "Local review only", "event-inspector-count"),
      );
      protocolTree.append(protocolHeading);
      if (selectedDetail.protocolCaptureSequence === null) {
        protocolTree.append(
          text(
            "p",
            "This canonical event is derived or manual and has no single wire message.",
            "runtime-empty-result",
          ),
        );
      } else if (protocol === null) {
        protocolTree.append(
          text(
            "p",
            "The allowlisted source message is no longer in the separate bounded protocol buffer, or its route was not eligible for inspection.",
            "runtime-empty-result",
          ),
        );
      } else {
        const routeName = `${protocol.serviceName}.${protocol.methodName}`;
        const protocolMeta = element("div", "event-inspector-protocol-meta");
        protocolMeta.append(
          detailRow("Route", routeName, true),
          detailRow("Message", protocol.messageName ?? "Schema name unavailable", true),
          detailRow("Direction", formatIdentifier(protocol.direction)),
          detailRow("Fragment", formatIdentifier(protocol.fragment)),
          detailRow("Decode", formatIdentifier(protocol.decodeStatus)),
          detailRow("Application payload", formatBytes(protocol.applicationBytes)),
        );
        protocolTree.append(protocolMeta);
        if (!protocol.payloadRetained) {
          protocolTree.append(
            text(
              "p",
              `Payload not retained: ${formatIdentifier(protocol.omissionReason ?? "bounded policy")}.`,
              "runtime-action-message",
            ),
          );
        } else {
          const wireFields = element("div", "event-inspector-protocol-fields");
          for (const field of protocol.fields) {
            const fieldRow = element("div", "event-inspector-protocol-field");
            fieldRow.append(
              text("code", field.path),
              text("span", formatIdentifier(field.wireType), "event-inspector-field-type"),
              text("code", field.value, "event-inspector-field-value"),
            );
            wireFields.append(fieldRow);
          }
          if (protocol.fields.length === 0) {
            wireFields.append(
              text("p", "The retained payload has no displayable fields.", "runtime-empty-result"),
            );
          }
          protocolTree.append(wireFields);
          if (protocol.parseError !== null) {
            protocolTree.append(
              text(
                "p",
                `Bounded wire reader stopped: ${protocol.parseError}.`,
                "runtime-action-message",
              ),
            );
          }
        }
      }
      rows.push(protocolTree);
      createRule.disabled = selectedDetail.sourceKind === "protocol" || selectedFieldPaths.size === 0;
      detailMessage.textContent = selectedDetail.sourceKind === "protocol"
        ? "Protocol fields remain local evidence until a reviewed canonical selector is promoted."
        : selectedFieldPaths.size === 0
          ? "Select at least one trigger-safe field."
          : `${selectedFieldPaths.size.toLocaleString()} field${selectedFieldPaths.size === 1 ? "" : "s"} will be copied into a disabled draft.`;
    }
    detailBody.replaceChildren(...rows);
    pin.disabled = pinnedEvents.some((event) => event.revision === selected!.revision);
    pin.textContent = pin.disabled ? "Pinned" : "Pin event";
  };

  const renderDraft = () => {
    storedDraftCount.textContent =
      `${readStoredDrafts(window.localStorage).length.toLocaleString()} saved locally`;
    if (currentDraft === null) {
      draftBody.replaceChildren(
        text(
          "p",
          "Select trigger-safe fields from an event to create a readable, disabled When draft. No actions execute from the Inspector.",
          "runtime-empty-result",
        ),
      );
      return;
    }
    const extraCriteria = currentDraft.when.criteria
      .filter((criterion) => criterion.path !== "event.kind")
      .map((criterion) => `${criterion.label} equals ${criterion.value}`);
    const sentence = `When ${formatIdentifier(currentDraft.when.eventKind)}${extraCriteria.length > 0 ? ` and ${extraCriteria.join(" and ")}` : ""}`;
    const criteria = element("ul", "event-inspector-draft-criteria");
    for (const criterion of currentDraft.when.criteria) {
      criteria.append(
        text(
          "li",
          `${criterion.label} (${criterion.path}) equals ${criterion.value}`,
        ),
      );
    }
    draftBody.replaceChildren(
      text("strong", currentDraft.name),
      text("p", sentence, "card-copy"),
      criteria,
      text(
        "p",
        "Saved locally and disabled. It has no actions and cannot execute until it is reviewed and completed in Rules.",
        "runtime-action-message",
      ),
    );
  };

  const renderPins = () => {
    clearPins.disabled = pinnedEvents.length === 0;
    if (pinnedEvents.length === 0) {
      pins.replaceChildren(
        text(
          "p",
          `Pin up to ${MAXIMUM_PINNED_EVENTS} events for later field comparison.`,
          "runtime-empty-result",
        ),
      );
      return;
    }
    pins.replaceChildren(
      ...pinnedEvents.map((event) => {
        const item = button(
          `#${event.sequence} · ${formatIdentifier(event.kind)} · ${event.rawIds}`,
          "event-inspector-pin",
        );
        item.addEventListener("click", () => {
          void selectEvent(event);
        });
        return item;
      }),
    );
  };

  let unsubscribe: (() => void) | null = null;
  let stopFollowing = () => {};
  let startFollowing = () => {};

  pause.addEventListener("click", () => {
    frozen = !frozen;
    pause.textContent = frozen ? "Resume live" : "Freeze log";
    if (frozen) {
      stopFollowing();
      connection.textContent = "FROZEN";
      connection.dataset.state = "frozen";
      status.textContent =
        "Frozen for review. Inspector polling and retention are stopped; canonical capture continues.";
      renderStream();
      return;
    }
    connection.textContent = "CONNECTING";
    connection.dataset.state = "waiting";
    status.textContent = "Reconnecting at the bounded live tail…";
    startFollowing();
  });
  clear.addEventListener("click", () => {
    visibleEvents = [];
    selected = null;
    selectedDetail = null;
    selectedDetailError = null;
    selectedFieldPaths = new Set();
    selectedDetailRequest += 1;
    viewerDropped = 0;
    renderSelection();
    renderStream();
  });
  pin.addEventListener("click", () => {
    if (selected === null || pinnedEvents.some((event) => event.revision === selected!.revision)) return;
    if (pinnedEvents.length >= MAXIMUM_PINNED_EVENTS) pinnedEvents.shift();
    pinnedEvents.push(selected);
    renderSelection();
    renderPins();
  });
  createRule.addEventListener("click", () => {
    if (selectedDetail === null) return;
    try {
      currentDraft = createCustomTriggerDraft(selectedDetail, selectedFieldPaths);
      storeCustomTriggerDraft(window.localStorage, currentDraft);
      detailMessage.classList.remove("error");
      detailMessage.textContent = "Disabled trigger draft saved locally.";
      renderDraft();
    } catch (error) {
      detailMessage.textContent = errorMessage(error);
      detailMessage.classList.add("error");
    }
  });
  clearPins.addEventListener("click", () => {
    pinnedEvents = [];
    renderSelection();
    renderPins();
  });
  for (const input of [source.select, topic.select, kind.input, search.input]) {
    input.addEventListener("input", renderStream);
  }

  startFollowing = () => {
    if (!alive || frozen || unsubscribe !== null) return;
    unsubscribe = dependencies.subscribe(
      (batch) => {
      if (!alive || frozen) return;
      if (activeSessionId !== null && batch.sessionId !== activeSessionId) {
        selected = null;
        selectedDetail = null;
        selectedDetailError = null;
        selectedFieldPaths = new Set();
        selectedDetailRequest += 1;
        pinnedEvents = [];
        renderSelection();
        renderPins();
      }
      activeSessionId = batch.sessionId;
      connection.textContent = batch.sessionId === null ? "WAITING FOR GAME" : "LIVE";
      connection.dataset.state = batch.sessionId === null ? "waiting" : "live";
      sessionMetric.value.textContent = batch.sessionId ?? "Waiting for capture";
      retainedMetric.value.textContent = batch.capacityEvents > 0
        ? `${batch.retainedEvents.toLocaleString()} / ${batch.capacityEvents.toLocaleString()} rows`
        : `${batch.retainedEvents.toLocaleString()} rows`;
      memoryMetric.value.textContent = batch.capacityBytes > 0
        ? `${formatBytes(batch.retainedBytes)} / ${formatBytes(batch.capacityBytes)}`
        : formatBytes(batch.retainedBytes);
      viewerDropped += batch.droppedBefore;
      gapMetric.value.textContent = viewerDropped.toLocaleString();
      visibleEvents.push(...batch.events);
      if (visibleEvents.length > MAXIMUM_VISIBLE_EVENTS) {
        viewerDropped += visibleEvents.length - MAXIMUM_VISIBLE_EVENTS;
        visibleEvents = visibleEvents.slice(-MAXIMUM_VISIBLE_EVENTS);
        gapMetric.value.textContent = viewerDropped.toLocaleString();
      }
      status.classList.remove("error");
      status.textContent = batch.sessionId === null
        ? "Connected. Waiting for the game parser to begin a live session."
        : `${batch.sessionId} · following reviewed canonical events and protocol messages`;
      renderStream();
      },
      (error) => {
        if (!alive || frozen) return;
        connection.textContent = "RECONNECTING";
        connection.dataset.state = "error";
        status.textContent = errorMessage(error);
        status.classList.add("error");
      },
    );
  };
  stopFollowing = () => {
    unsubscribe?.();
    unsubscribe = null;
  };

  renderStream();
  renderDraft();
  startFollowing();
  return {
    dispose() {
      alive = false;
      stopFollowing();
      root.remove();
    },
  };
}

function metric(label: string, initial: string) {
  const root = element("article");
  const value = text("strong", initial);
  root.append(text("span", label), value);
  return { root, value };
}

function detailRow(label: string, value: string, code = false): HTMLDivElement {
  const row = element("div");
  row.append(text("span", label), text(code ? "code" : "strong", value));
  return row;
}

function selectField(
  labelText: string,
  placeholder: string,
  options: readonly (readonly [string, string])[],
) {
  const label = element("label", "runtime-field");
  const select = element("select");
  select.append(new Option(placeholder, ""));
  for (const [display, value] of options) select.append(new Option(display, value));
  label.append(text("span", labelText), select);
  return { label, select };
}

function inputField(labelText: string, placeholder: string, type = "text") {
  const label = element("label", "runtime-field");
  const input = element("input");
  input.type = type;
  input.placeholder = placeholder;
  label.append(text("span", labelText), input);
  return { label, input };
}

function button(value: string, className: string): HTMLButtonElement {
  const node = element("button", className);
  node.type = "button";
  node.textContent = value;
  return node;
}

function formatObservedMicros(value: number): string {
  const totalMillis = Math.floor(value / 1_000);
  const millis = totalMillis % 1_000;
  const totalSeconds = Math.floor(totalMillis / 1_000);
  const seconds = totalSeconds % 60;
  const minutes = Math.floor(totalSeconds / 60);
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

function formatIdentifier(value: EventViewerTopic | string): string {
  return value
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function defaultTriggerFieldPaths(detail: LiveEventDetail): Set<string> {
  const preferred = new Set([
    "event.kind",
    "event.ability_id",
    "event.status_id",
    "event.monster_id",
    "event.scene_id",
    "event.dungeon_id",
  ]);
  return new Set(
    detail.fields
      .filter((field) => field.usableInTrigger && preferred.has(field.path))
      .map((field) => field.path),
  );
}

function formatBytes(value: number): string {
  if (value < 1_024) return `${value} B`;
  if (value < 1_048_576) return `${(value / 1_024).toFixed(1)} KiB`;
  return `${(value / 1_048_576).toFixed(1)} MiB`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== undefined) node.className = className;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  value: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = value;
  return node;
}
