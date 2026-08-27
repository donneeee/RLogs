export type RdpsReadinessState =
  | "active"
  | "blocked"
  | "incompatible"
  | "provisional"
  | "replay_unavailable"
  | "waiting"
  | "pending";

export interface RdpsStatusPresentation {
  state: RdpsReadinessState;
  providerCreditEnabled: boolean;
  blockerCodes: readonly string[];
  compactLabel: string;
  historyMessage: string | null;
}

const ORDINARY_DAMAGE_REMAINS_ACTIVE =
  "Ordinary damage and all other combat metrics remain active.";

export function describeRdpsStatus(status: string): RdpsStatusPresentation {
  if (status === "ready") {
    return {
      state: "active",
      providerCreditEnabled: true,
      blockerCodes: [],
      compactLabel: "Ready",
      historyMessage: null,
    };
  }

  if (status === "partial_packet_proven_rules") {
    return {
      state: "active",
      providerCreditEnabled: true,
      blockerCodes: [],
      compactLabel: "Packet-proven proportional rDPS active",
      historyMessage:
        "Packet-proven proportional rDPS is active only for reviewed effect and recipient scopes. It redistributes exact observed damage using lossless rational shares, then rounds the summed display result once. Unproven, overlapping, or unobservable effects remain unresolved and receive zero provider credit; ordinary damage is unchanged.",
    };
  }

  const blockedDetail =
    statusDetail(status, "formula_pack_blocked:") ??
    statusDetail(status, "formula_runtime_blocked:");
  if (blockedDetail !== null) {
    const detail = blockedDetail || "exact-build promotion proof gates are incomplete";
    const blockerCodes = statusList(detail, "blockers=");
    const blockerLabels = blockerCodes.map(rdpsBlockerLabel);
    const blockerSummary = blockerLabels.length === 0
      ? detail
      : blockerLabels.join("; ");
    return {
      state: "blocked",
      providerCreditEnabled: false,
      blockerCodes,
      compactLabel: blockerCodes.length === 0
        ? `Exact-build proof pending — ${detail}`
        : `Exact-build proof pending — ${blockerCodes.length} gates`,
      historyMessage: `rDPS provider credit is disabled because these exact-build gates remain open: ${blockerSummary}. Remote-player cast packets that are structurally absent are not required or inferred. ${ORDINARY_DAMAGE_REMAINS_ACTIVE}`,
    };
  }

  const outdatedDetail = statusDetail(status, "formula_pack_outdated:");
  if (outdatedDetail !== null) {
    const detail = outdatedDetail || "the exact-build formula pack is unavailable";
    return {
      state: "provisional",
      providerCreditEnabled: false,
      blockerCodes: [],
      compactLabel: `Provisional — ${detail}`,
      historyMessage: `rDPS formulas are provisional and provider credit remains disabled — ${detail}. ${ORDINARY_DAMAGE_REMAINS_ACTIVE}`,
    };
  }

  const incompatibleDetail = statusDetail(status, "formula_pack_incompatible:");
  if (incompatibleDetail !== null) {
    const detail = incompatibleDetail || "the formula pack does not match this deployment";
    return {
      state: "incompatible",
      providerCreditEnabled: false,
      blockerCodes: [],
      compactLabel: `Unavailable — ${detail}`,
      historyMessage: `rDPS formulas are unavailable for this deployment — ${detail}. ${ORDINARY_DAMAGE_REMAINS_ACTIVE}`,
    };
  }

  if (status.startsWith("formula_replay_unavailable:")) {
    return {
      state: "replay_unavailable",
      providerCreditEnabled: false,
      blockerCodes: [],
      compactLabel: "Unavailable — exact sealed-log replay was not validated",
      historyMessage:
        "Archived rDPS is unavailable because the exact sealed-log replay could not be validated. Ordinary damage and all other saved combat metrics remain unchanged.",
    };
  }

  if (status === "waiting_for_client_build") {
    return {
      state: "waiting",
      providerCreditEnabled: false,
      blockerCodes: [],
      compactLabel: "Waiting for exact game build",
      historyMessage: `rDPS is waiting for an authoritative exact game-build identity. ${ORDINARY_DAMAGE_REMAINS_ACTIVE}`,
    };
  }

  if (status === "pending_reviewed_effect_rules") {
    return {
      state: "pending",
      providerCreditEnabled: false,
      blockerCodes: [],
      compactLabel: "No reviewed rules active",
      historyMessage:
        "rDPS waits for reviewed contribution rules with exact provider, recipient scope, magnitude, operation order, stacking, integer rounding, and build evidence.",
    };
  }

  return {
    state: "pending",
    providerCreditEnabled: false,
    blockerCodes: [],
    compactLabel: "Unavailable — unrecognized proof state",
    historyMessage: `rDPS formula readiness is unknown, so provider credit is disabled. ${ORDINARY_DAMAGE_REMAINS_ACTIVE}`,
  };
}

function statusDetail(status: string, prefix: string): string | null {
  return status.startsWith(prefix) ? status.slice(prefix.length).trim() : null;
}

function statusList(detail: string, prefix: string): string[] {
  const segment = detail
    .split(";")
    .map((value) => value.trim())
    .find((value) => value.startsWith(prefix));
  if (segment === undefined) return [];
  return segment
    .slice(prefix.length)
    .split(",")
    .map((value) => value.trim())
    .filter((value, index, values) => value.length > 0 && values.indexOf(value) === index);
}

function rdpsBlockerLabel(code: string): string {
  switch (code) {
    case "protocol-pack-identity":
      return "authoritative current-build protocol-pack identity";
    case "canonical-replay-conservation":
      return "canonical replay conservation";
    case "protocol-event-coverage":
      return "protocol event coverage";
    case "critical-damage-factor-interpretation-authority":
      return "critical-damage factor interpretation, operation order, and integer rounding authority";
    case "party-support-formula-frontier":
      return "party-skill and team-entry provider, recipient-scope, formula, stacking, rounding, and conservation proof";
    case "historical-build-runtime-promotion-not-reviewed":
      return "historical-build runtime promotion review";
    default:
      return `unrecognized proof gate ${code}`;
  }
}
