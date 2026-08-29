import { describe, expect, it } from "vitest";
import { describeRdpsStatus } from "./rdps-status";

describe("describeRdpsStatus", () => {
  it.each([
    "formula_pack_blocked: formula=global/24687926; current-build proof gates incomplete",
    "formula_runtime_blocked: exact-build promotion proof gates are incomplete",
  ])("fails closed for a blocked exact-build formula status: %s", (status) => {
    const presentation = describeRdpsStatus(status);

    expect(presentation.state).toBe("blocked");
    expect(presentation.providerCreditEnabled).toBe(false);
    expect(presentation.compactLabel).toContain("Exact-build proof pending");
    expect(presentation.historyMessage).toContain("provider credit is disabled");
    expect(presentation.historyMessage).toContain(
      "Ordinary damage and all other combat metrics remain active.",
    );
  });

  it("does not reinterpret a permanently unobservable effect as a proven rule", () => {
    const presentation = describeRdpsStatus("partial_packet_proven_rules");

    expect(presentation.state).toBe("active");
    expect(presentation.compactLabel).toBe(
      "Packet-proven proportional rDPS active",
    );
    expect(presentation.historyMessage).toContain(
      "redistributes exact observed damage using lossless rational shares",
    );
    expect(presentation.historyMessage).toContain(
      "ordinary damage is unchanged",
    );
    expect(presentation.historyMessage).toContain(
      "Unproven, overlapping, or unobservable effects remain unresolved",
    );
  });

  it("keeps the last validated provider credit visible during its saved history refresh", () => {
    const presentation = describeRdpsStatus(
      "formula_refresh_queued: recalculating archived rDPS in the background",
    );

    expect(presentation.state).toBe("refreshing");
    expect(presentation.providerCreditEnabled).toBe(true);
    expect(presentation.compactLabel).toContain("Showing saved rDPS");
    expect(presentation.historyMessage).toContain("last validated, packet-proven rDPS");
    expect(presentation.historyMessage).toContain("Later opens will use that saved result immediately");
    expect(presentation.historyMessage).toContain("does not scan or replay the rest of history at startup");
    expect(presentation.historyMessage).toContain("opened immediately");
    expect(presentation.historyMessage).toContain("Live capture always has priority");
  });

  it("reports a fully ready formula status as active", () => {
    expect(describeRdpsStatus("ready")).toEqual({
      state: "active",
      providerCreditEnabled: true,
      blockerCodes: [],
      compactLabel: "Ready",
      historyMessage: null,
    });
  });

  it("fails closed for an unrecognized status", () => {
    const presentation = describeRdpsStatus("future_status_without_a_contract");

    expect(presentation.state).toBe("pending");
    expect(presentation.providerCreditEnabled).toBe(false);
    expect(presentation.compactLabel).toBe(
      "Unavailable — unrecognized proof state",
    );
  });

  it("requires exact game-build identity while waiting", () => {
    const presentation = describeRdpsStatus("waiting_for_client_build");

    expect(presentation.providerCreditEnabled).toBe(false);
    expect(presentation.compactLabel).toBe("Waiting for exact game build");
  });

  it("presents exact machine-readable blockers without requiring absent remote casts", () => {
    const presentation = describeRdpsStatus(
      "formula_pack_blocked: formula=global/24687926; blockers=protocol-pack-identity,canonical-replay-conservation,protocol-event-coverage,critical-damage-factor-interpretation-authority,party-support-formula-frontier",
    );

    expect(presentation.blockerCodes).toEqual([
      "protocol-pack-identity",
      "canonical-replay-conservation",
      "protocol-event-coverage",
      "critical-damage-factor-interpretation-authority",
      "party-support-formula-frontier",
    ]);
    expect(presentation.compactLabel).toBe("Exact-build proof pending — 5 gates");
    expect(presentation.historyMessage).toContain(
      "critical-damage factor interpretation, operation order, and integer rounding authority",
    );
    expect(presentation.historyMessage).toContain(
      "Remote-player cast packets that are structurally absent are not required or inferred.",
    );
    expect(presentation.historyMessage).toContain(
      "party-skill and team-entry provider, recipient-scope, formula, stacking, rounding, and conservation proof",
    );
    expect(presentation.providerCreditEnabled).toBe(false);
  });
});
