import type { MechanicsMapSignal, MechanicsMapSnapshot } from "./mechanics-map";

export const AUTOMARKER_PLAN_SCHEMA_VERSION = 1 as const;

export interface AutomarkerRule {
  reviewState: "reviewed_current_build";
  clientBuild: string;
  sceneIds: readonly number[];
  mechanicKind: string;
  labels: readonly string[];
  repeatLabel?: string;
  maxTargets: number;
}

export interface AutomarkerPreviewAssignment {
  actorId: number;
  label: string;
  mechanicKind: string;
  effectId: number;
  appliedAtMicros: number;
  confidence: "packet_target_exact_rule_reviewed";
  provenance: "reviewed_canonical_mechanic_signal";
}

export interface AutomarkerPlacementPlan {
  schemaVersion: typeof AUTOMARKER_PLAN_SCHEMA_VERSION;
  mode: "preview_only";
  assignmentOrder: "actor_id_not_party_order";
  nativePlacement: {
    supported: false;
    reason: "native_party_marker_protocol_unverified";
  };
  assignments: readonly AutomarkerPreviewAssignment[];
}

/**
 * The one reviewed player-target mechanic currently useful as an automarker
 * preview. Native placement remains disabled until a current-build capture
 * proves the game's leader-authorized request, acknowledgement, broadcast,
 * update and clear protocol.
 */
export const REVIEWED_AUTOMARKER_RULES: readonly AutomarkerRule[] = [{
  reviewState: "reviewed_current_build",
  clientBuild: "24687926",
  sceneIds: [1150, 1151, 1152],
  mechanicKind: "sticky_bomb",
  labels: ["BOMB"],
  repeatLabel: "BOMB",
  maxTargets: 40,
}];

export function planAutomarkerPreview(
  snapshot: MechanicsMapSnapshot,
  rules: readonly AutomarkerRule[] = REVIEWED_AUTOMARKER_RULES,
): AutomarkerPlacementPlan {
  const plan: AutomarkerPlacementPlan = {
    schemaVersion: AUTOMARKER_PLAN_SCHEMA_VERSION,
    mode: "preview_only",
    assignmentOrder: "actor_id_not_party_order",
    nativePlacement: {
      supported: false,
      reason: "native_party_marker_protocol_unverified",
    },
    assignments: [],
  };
  if (!snapshot.encounter_pack_reviewed || snapshot.data_gap !== null ||
      snapshot.client_build === null || snapshot.scene_id === null) return plan;

  const roster = new Set<number>();
  if (snapshot.player !== null) roster.add(snapshot.player.actor_id);
  for (const member of snapshot.party) roster.add(member.actor_id);

  const assignments: AutomarkerPreviewAssignment[] = [];
  for (const rule of rules) {
    if (rule.clientBuild !== snapshot.client_build || !rule.sceneIds.includes(snapshot.scene_id) ||
        rule.maxTargets < 1 || (rule.labels.length === 0 && !rule.repeatLabel)) continue;
    const signals = newestSignalsByTarget(snapshot.mechanics, rule.mechanicKind)
      .filter((signal) => roster.has(signal.target_actor_id))
      .sort((left, right) => left.target_actor_id - right.target_actor_id)
      .slice(0, rule.maxTargets);
    for (let index = 0; index < signals.length; index += 1) {
      const signal = signals[index]!;
      assignments.push({
        actorId: signal.target_actor_id,
        label: rule.labels[index] ?? rule.repeatLabel!,
        mechanicKind: rule.mechanicKind,
        effectId: signal.effect_id,
        appliedAtMicros: signal.applied_at_micros,
        confidence: "packet_target_exact_rule_reviewed",
        provenance: "reviewed_canonical_mechanic_signal",
      });
    }
  }
  assignments.sort((left, right) => left.actorId - right.actorId ||
    left.mechanicKind.localeCompare(right.mechanicKind));
  return { ...plan, assignments };
}

function newestSignalsByTarget(
  signals: readonly MechanicsMapSignal[],
  mechanicKind: string,
): MechanicsMapSignal[] {
  const newest = new Map<number, MechanicsMapSignal>();
  for (const signal of signals) {
    if (signal.mechanic_kind !== mechanicKind) continue;
    const current = newest.get(signal.target_actor_id);
    if (current === undefined || current.applied_at_micros < signal.applied_at_micros) {
      newest.set(signal.target_actor_id, signal);
    }
  }
  return [...newest.values()];
}
