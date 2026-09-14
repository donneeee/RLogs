//! Pure deny-by-default policy for a possible private native Automarkers adapter.
//!
//! This module performs no I/O, process access, memory access, scheduling,
//! native invocation, or packet transmission. It deliberately stays unwired:
//! satisfying this policy would only allow a future adapter to begin its own
//! fail-closed preflight; it is not placement or send authorization.

#![allow(dead_code)]

pub(crate) const REVIEWED_DEPLOYMENT: &str = "global";
pub(crate) const REVIEWED_CHANNEL: &str = "steam";
pub(crate) const REVIEWED_BUILD: &str = "25247556";
pub(crate) const REVIEWED_PROCESS_NAME: &str = "BPSR_STEAM.exe";
pub(crate) const REVIEWED_EXECUTABLE_BYTES: u64 = 808_496;
pub(crate) const REVIEWED_EXECUTABLE_SHA256: &str =
    "90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588";
pub(crate) const REVIEWED_GAME_ASSEMBLY_BYTES: u64 = 218_074_672;
pub(crate) const REVIEWED_GAME_ASSEMBLY_SHA256: &str =
    "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3";
pub(crate) const RISK_NOTICE_REVISION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeAdapterDistribution {
    PrivateUnpublished,
    PublicOrReleased,
}

/// A capability created only after a current-launch, explicit user action.
/// It intentionally cannot be represented by a boolean that persists across
/// game launches or silently carries forward to another build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentLaunchAuthorization {
    launch_id: [u8; 16],
    risk_notice_revision: u32,
    acknowledged_in_process_execution: bool,
    acknowledged_crash_and_account_risk: bool,
}

impl CurrentLaunchAuthorization {
    pub(crate) fn new(
        launch_id: [u8; 16],
        risk_notice_revision: u32,
        acknowledged_in_process_execution: bool,
        acknowledged_crash_and_account_risk: bool,
    ) -> Option<Self> {
        (launch_id.iter().any(|byte| *byte != 0)
            && risk_notice_revision == RISK_NOTICE_REVISION
            && acknowledged_in_process_execution
            && acknowledged_crash_and_account_risk)
            .then_some(Self {
                launch_id,
                risk_notice_revision,
                acknowledged_in_process_execution,
                acknowledged_crash_and_account_risk,
            })
    }

    pub(crate) fn applies_to(self, launch_id: [u8; 16]) -> bool {
        self.launch_id == launch_id
            && self.risk_notice_revision == RISK_NOTICE_REVISION
            && self.acknowledged_in_process_execution
            && self.acknowledged_crash_and_account_risk
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactBuildIdentity<'a> {
    pub deployment: &'a str,
    pub channel: &'a str,
    pub build: &'a str,
    pub process_name: &'a str,
    pub executable_bytes: u64,
    pub executable_sha256: &'a str,
    pub game_assembly_bytes: u64,
    pub game_assembly_sha256: &'a str,
}

impl ExactBuildIdentity<'_> {
    pub(crate) fn is_reviewed(self) -> bool {
        self.deployment == REVIEWED_DEPLOYMENT
            && self.channel == REVIEWED_CHANNEL
            && self.build == REVIEWED_BUILD
            && self
                .process_name
                .eq_ignore_ascii_case(REVIEWED_PROCESS_NAME)
            && self.executable_bytes == REVIEWED_EXECUTABLE_BYTES
            && self
                .executable_sha256
                .eq_ignore_ascii_case(REVIEWED_EXECUTABLE_SHA256)
            && self.game_assembly_bytes == REVIEWED_GAME_ASSEMBLY_BYTES
            && self
                .game_assembly_sha256
                .eq_ignore_ascii_case(REVIEWED_GAME_ASSEMBLY_SHA256)
    }
}

/// Facts that must be freshly established for one exact placement attempt.
/// No field may be inferred from a prior attempt, saved preset, or stale scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeAdapterGateSnapshot<'a> {
    pub distribution: NativeAdapterDistribution,
    pub launch_id: [u8; 16],
    pub authorization: Option<CurrentLaunchAuthorization>,
    pub build: ExactBuildIdentity<'a>,
    pub anti_cheat_risk_acknowledged: bool,
    pub authenticated_private_adapter_peer: bool,
    pub peer_owned_by_current_game_process: bool,
    pub supported_in_process_entry_proven: bool,
    pub il2cpp_thread_attached: bool,
    pub exactly_once_main_thread_scheduler_proven: bool,
    pub current_dungeon_scene_proven: bool,
    pub preset_family_matches_scene: bool,
    pub local_party_leader_proven: bool,
    pub live_object_chain_stable: bool,
    pub input_and_skill_eligibility_proven: bool,
    pub indicator_operation_atomicity_proven: bool,
    pub one_marker_in_flight: bool,
    pub source_unbound_interceptor_prearmed: bool,
    pub fresh_confirmation_observers_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeAdapterDenial {
    PublicDistributionForbidden,
    MissingCurrentLaunchAuthorization,
    UnsupportedBuildIdentity,
    MissingAntiCheatRiskAcknowledgement,
    UnauthenticatedAdapterPeer,
    WrongAdapterOwner,
    UnsupportedInProcessEntry,
    Il2CppThreadNotAttached,
    MainThreadSchedulerUnproven,
    DungeonSceneUnproven,
    PresetFamilyMismatch,
    PartyLeadershipUnproven,
    StaleNativeObjects,
    SkillEligibilityUnproven,
    IndicatorAtomicityUnproven,
    ConcurrentMarkerAttempt,
    InterceptorNotPrearmed,
    ConfirmationObserversNotReady,
}

/// Returns every failed gate so diagnostics cannot accidentally treat one
/// repaired condition as overall authorization. An empty result authorizes
/// only entry into a future adapter preflight.
pub(crate) fn adapter_preflight_denials(
    snapshot: NativeAdapterGateSnapshot<'_>,
) -> Vec<NativeAdapterDenial> {
    let mut denied = Vec::new();
    if snapshot.distribution != NativeAdapterDistribution::PrivateUnpublished {
        denied.push(NativeAdapterDenial::PublicDistributionForbidden);
    }
    if !snapshot
        .authorization
        .is_some_and(|authorization| authorization.applies_to(snapshot.launch_id))
    {
        denied.push(NativeAdapterDenial::MissingCurrentLaunchAuthorization);
    }
    if !snapshot.build.is_reviewed() {
        denied.push(NativeAdapterDenial::UnsupportedBuildIdentity);
    }
    // Acknowledgement is not proof that an anti-cheat-sensitive operation is
    // safe or permitted; it only prevents silent activation of the experiment.
    if !snapshot.anti_cheat_risk_acknowledged {
        denied.push(NativeAdapterDenial::MissingAntiCheatRiskAcknowledgement);
    }
    if !snapshot.authenticated_private_adapter_peer {
        denied.push(NativeAdapterDenial::UnauthenticatedAdapterPeer);
    }
    if !snapshot.peer_owned_by_current_game_process {
        denied.push(NativeAdapterDenial::WrongAdapterOwner);
    }
    if !snapshot.supported_in_process_entry_proven {
        denied.push(NativeAdapterDenial::UnsupportedInProcessEntry);
    }
    if !snapshot.il2cpp_thread_attached {
        denied.push(NativeAdapterDenial::Il2CppThreadNotAttached);
    }
    if !snapshot.exactly_once_main_thread_scheduler_proven {
        denied.push(NativeAdapterDenial::MainThreadSchedulerUnproven);
    }
    if !snapshot.current_dungeon_scene_proven {
        denied.push(NativeAdapterDenial::DungeonSceneUnproven);
    }
    if !snapshot.preset_family_matches_scene {
        denied.push(NativeAdapterDenial::PresetFamilyMismatch);
    }
    if !snapshot.local_party_leader_proven {
        denied.push(NativeAdapterDenial::PartyLeadershipUnproven);
    }
    if !snapshot.live_object_chain_stable {
        denied.push(NativeAdapterDenial::StaleNativeObjects);
    }
    if !snapshot.input_and_skill_eligibility_proven {
        denied.push(NativeAdapterDenial::SkillEligibilityUnproven);
    }
    if !snapshot.indicator_operation_atomicity_proven {
        denied.push(NativeAdapterDenial::IndicatorAtomicityUnproven);
    }
    if snapshot.one_marker_in_flight {
        denied.push(NativeAdapterDenial::ConcurrentMarkerAttempt);
    }
    if !snapshot.source_unbound_interceptor_prearmed {
        denied.push(NativeAdapterDenial::InterceptorNotPrearmed);
    }
    if !snapshot.fresh_confirmation_observers_ready {
        denied.push(NativeAdapterDenial::ConfirmationObserversNotReady);
    }
    denied
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH: [u8; 16] = [7; 16];

    fn reviewed_build() -> ExactBuildIdentity<'static> {
        ExactBuildIdentity {
            deployment: REVIEWED_DEPLOYMENT,
            channel: REVIEWED_CHANNEL,
            build: REVIEWED_BUILD,
            process_name: REVIEWED_PROCESS_NAME,
            executable_bytes: REVIEWED_EXECUTABLE_BYTES,
            executable_sha256: REVIEWED_EXECUTABLE_SHA256,
            game_assembly_bytes: REVIEWED_GAME_ASSEMBLY_BYTES,
            game_assembly_sha256: REVIEWED_GAME_ASSEMBLY_SHA256,
        }
    }

    fn complete_snapshot() -> NativeAdapterGateSnapshot<'static> {
        NativeAdapterGateSnapshot {
            distribution: NativeAdapterDistribution::PrivateUnpublished,
            launch_id: LAUNCH,
            authorization: CurrentLaunchAuthorization::new(
                LAUNCH,
                RISK_NOTICE_REVISION,
                true,
                true,
            ),
            build: reviewed_build(),
            anti_cheat_risk_acknowledged: true,
            authenticated_private_adapter_peer: true,
            peer_owned_by_current_game_process: true,
            supported_in_process_entry_proven: true,
            il2cpp_thread_attached: true,
            exactly_once_main_thread_scheduler_proven: true,
            current_dungeon_scene_proven: true,
            preset_family_matches_scene: true,
            local_party_leader_proven: true,
            live_object_chain_stable: true,
            input_and_skill_eligibility_proven: true,
            indicator_operation_atomicity_proven: true,
            one_marker_in_flight: false,
            source_unbound_interceptor_prearmed: true,
            fresh_confirmation_observers_ready: true,
        }
    }

    #[test]
    fn exact_reviewed_identity_is_closed_to_other_builds_channels_and_hashes() {
        assert!(reviewed_build().is_reviewed());
        let mut identity = reviewed_build();
        identity.build = "25247557";
        assert!(!identity.is_reviewed());
        identity = reviewed_build();
        identity.channel = "epic";
        assert!(!identity.is_reviewed());
        identity = reviewed_build();
        identity.game_assembly_sha256 =
            "5a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3";
        assert!(!identity.is_reviewed());
    }

    #[test]
    fn authorization_is_current_launch_risk_revision_and_acknowledgement_bound() {
        assert!(CurrentLaunchAuthorization::new([0; 16], 1, true, true).is_none());
        assert!(CurrentLaunchAuthorization::new(LAUNCH, 2, true, true).is_none());
        assert!(CurrentLaunchAuthorization::new(LAUNCH, 1, false, true).is_none());
        let authorization = CurrentLaunchAuthorization::new(LAUNCH, 1, true, true).unwrap();
        assert!(authorization.applies_to(LAUNCH));
        assert!(!authorization.applies_to([8; 16]));
    }

    #[test]
    fn private_preflight_requires_every_fresh_gate() {
        assert!(adapter_preflight_denials(complete_snapshot()).is_empty());
        let mut snapshot = complete_snapshot();
        snapshot.supported_in_process_entry_proven = false;
        snapshot.local_party_leader_proven = false;
        snapshot.source_unbound_interceptor_prearmed = false;
        assert_eq!(
            adapter_preflight_denials(snapshot),
            vec![
                NativeAdapterDenial::UnsupportedInProcessEntry,
                NativeAdapterDenial::PartyLeadershipUnproven,
                NativeAdapterDenial::InterceptorNotPrearmed,
            ]
        );
    }

    #[test]
    fn public_distribution_and_stale_authorization_fail_closed() {
        let mut snapshot = complete_snapshot();
        snapshot.distribution = NativeAdapterDistribution::PublicOrReleased;
        snapshot.launch_id = [8; 16];
        assert_eq!(
            adapter_preflight_denials(snapshot),
            vec![
                NativeAdapterDenial::PublicDistributionForbidden,
                NativeAdapterDenial::MissingCurrentLaunchAuthorization,
            ]
        );
    }

    #[test]
    fn unresolved_real_world_gates_keep_the_adapter_disabled() {
        let mut snapshot = complete_snapshot();
        snapshot.supported_in_process_entry_proven = false;
        snapshot.il2cpp_thread_attached = false;
        snapshot.exactly_once_main_thread_scheduler_proven = false;
        snapshot.input_and_skill_eligibility_proven = false;
        snapshot.indicator_operation_atomicity_proven = false;
        assert!(!adapter_preflight_denials(snapshot).is_empty());
    }
}
