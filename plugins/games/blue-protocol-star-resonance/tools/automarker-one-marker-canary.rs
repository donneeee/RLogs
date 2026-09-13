//! Fail-closed command boundary for the first one-marker active canary.
//!
//! The pure rewrite, checksum, TCP-ledger, and retrospective confirmation
//! components are complete. Live activation remains blocked until this binary
//! has a trusted authoritative inbound decoder/context source. In particular,
//! mirrored traffic from another computer is useful for passive research but
//! cannot prove a local process-owned tuple and cannot divert the packets the
//! server receives.

use std::{
    env, error::Error, ffi::OsString, fs::OpenOptions, io::Write, net::Ipv4Addr, path::PathBuf,
};

use rlogs_game_bpsr::{
    AUTOMARKER_REQUEST_BUILD, AUTOMARKER_REQUEST_PACK_DIGEST, AutomarkerBridgeCommitDisposition,
    AutomarkerBridgeCoordinator, AutomarkerBridgeCoordinatorError,
    AutomarkerBridgePrepareDisposition, AutomarkerConfirmationContext,
    AutomarkerPacketSendPreparation, AutomarkerRequestXyz, AutomarkerWinDivertAddress,
    AutomarkerWinDivertChecksumError, AutomarkerWinDivertChecksumHelper,
    SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN, SingleMarkerXyzCanaryContext,
    SingleMarkerXyzExternalSendOutcome, prepare_automarker_ipv4_tcp_packet,
};
use serde::Serialize;

const DRIVER_ARM_TOKEN: &str = "RLOGS_AUTOMARKER_ONE_MARKER_ACTIVE_CANARY_V1";
const BLOCKED_GATE: &str = "blocked_unresolved_authoritative_inbound_decoder_and_context_wiring";

#[derive(Debug, Clone, PartialEq)]
struct Arguments {
    arm_token: Option<String>,
    process_id: Option<u32>,
    dependency_directory: Option<PathBuf>,
    build: Option<String>,
    pack_digest: Option<String>,
    scene_family: Option<String>,
    marker_number: Option<u8>,
    target: Option<AutomarkerRequestXyz>,
    local_actor_id: Option<i64>,
    connection_epoch: Option<u64>,
    client_address: Option<Ipv4Addr>,
    client_port: Option<u16>,
    server_address: Option<Ipv4Addr>,
    server_port: Option<u16>,
    runtime_revision: Option<u64>,
    observed_micros: Option<u64>,
    observation_ordinal: Option<u64>,
    baseline_marker_instances: Vec<i64>,
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preflight {
    DryRun,
    BlockedMissing(&'static str),
    BlockedInvalid(&'static str),
    BlockedUnresolvedLiveGate,
}

/// Owned result of the pure coordinator/checksum handoff. A modified packet is
/// only prepared for one external send; its exact send result must be reported
/// through `commit_one_marker_send`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneMarkerPreparedSend {
    Original {
        packet: Vec<u8>,
        address: AutomarkerWinDivertAddress,
    },
    Modified {
        preparation_id: u64,
        packet: Vec<u8>,
        address: AutomarkerWinDivertAddress,
    },
    AbortWithoutReinject(OneMarkerDriverError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneMarkerDriverError {
    Coordinator(AutomarkerBridgeCoordinatorError),
    Checksum(AutomarkerWinDivertChecksumError),
    ChecksumCancellationDidNotReturnOriginal,
}

/// The only permitted coordinator-to-checksum handoff. No payload editing is
/// possible outside the coordinator-owned changed packet.
#[allow(clippy::too_many_arguments)]
pub fn prepare_one_marker_packet<H: AutomarkerWinDivertChecksumHelper>(
    coordinator: &mut AutomarkerBridgeCoordinator,
    helper: &H,
    connection_epoch: u64,
    tcp_sequence_start: u32,
    tcp_payload_offset: usize,
    original_packet: &[u8],
    address: AutomarkerWinDivertAddress,
    canary_context: SingleMarkerXyzCanaryContext<'_>,
    confirmation_context: &AutomarkerConfirmationContext,
    observation_ordinal: u64,
) -> OneMarkerPreparedSend {
    match coordinator.prepare(
        connection_epoch,
        tcp_sequence_start,
        tcp_payload_offset,
        original_packet,
        address,
        canary_context,
        confirmation_context,
        observation_ordinal,
    ) {
        AutomarkerBridgePrepareDisposition::SendOriginal(original) => {
            OneMarkerPreparedSend::Original {
                packet: original.packet,
                address: original.address,
            }
        }
        AutomarkerBridgePrepareDisposition::AbortWithoutReinject(reason) => {
            OneMarkerPreparedSend::AbortWithoutReinject(OneMarkerDriverError::Coordinator(reason))
        }
        AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(input) => {
            match prepare_automarker_ipv4_tcp_packet(
                &input.original_packet,
                &input.address,
                Some(input.changed_packet_for_checksum()),
                helper,
            ) {
                Ok(AutomarkerPacketSendPreparation::ModifiedAuthorized { packet, address }) => {
                    OneMarkerPreparedSend::Modified {
                        preparation_id: input.preparation_id,
                        packet,
                        address,
                    }
                }
                Ok(AutomarkerPacketSendPreparation::Original { .. }) => {
                    exact_original_after_cancel(
                        coordinator.cancel(input.preparation_id),
                        OneMarkerDriverError::ChecksumCancellationDidNotReturnOriginal,
                    )
                }
                Err(reason) => exact_original_after_cancel(
                    coordinator.cancel(input.preparation_id),
                    OneMarkerDriverError::Checksum(reason),
                ),
            }
        }
    }
}

fn exact_original_after_cancel(
    cancellation: AutomarkerBridgePrepareDisposition,
    failure: OneMarkerDriverError,
) -> OneMarkerPreparedSend {
    match cancellation {
        AutomarkerBridgePrepareDisposition::SendOriginal(original) => {
            OneMarkerPreparedSend::Original {
                packet: original.packet,
                address: original.address,
            }
        }
        AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(_)
        | AutomarkerBridgePrepareDisposition::AbortWithoutReinject(_) => {
            OneMarkerPreparedSend::AbortWithoutReinject(failure)
        }
    }
}

/// Commit only an exact full-length external send. Failed and short modified
/// sends are indeterminate and can never release original overlapping bytes.
pub fn commit_one_marker_send(
    coordinator: &mut AutomarkerBridgeCoordinator,
    preparation_id: u64,
    expected_packet_len: usize,
    send_result: Result<usize, ()>,
) -> AutomarkerBridgeCommitDisposition {
    let outcome = match send_result {
        Ok(bytes_sent) if bytes_sent == expected_packet_len => {
            SingleMarkerXyzExternalSendOutcome::Complete { bytes_sent }
        }
        Ok(bytes_sent) => SingleMarkerXyzExternalSendOutcome::Short { bytes_sent },
        Err(()) => SingleMarkerXyzExternalSendOutcome::Failed,
    };
    coordinator.commit(preparation_id, outcome)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    schema_version: u32,
    artifact_kind: &'static str,
    mode: &'static str,
    outcome: &'static str,
    exact_build: bool,
    exact_pack_digest: bool,
    exact_local_game_process_required: bool,
    exact_syn_owned_tuple_required: bool,
    reflect_arbitration_required: bool,
    complete_fresh_world_use_slot_249858_carrier_required: bool,
    checksum_repair_required: bool,
    deterministic_retransmission_mapping_required: bool,
    reverse_ack_rpc_authoritative_add_required: bool,
    player_distance_gate: bool,
    packet_or_memory_write_attempted: bool,
    unresolved_gate: Option<&'static str>,
}

impl Receipt {
    fn from_preflight(preflight: Preflight) -> Self {
        let (mode, outcome, unresolved_gate) = match preflight {
            Preflight::DryRun => ("dry-run", "dry_run_no_driver_load_no_handle_open", None),
            Preflight::BlockedMissing(reason) | Preflight::BlockedInvalid(reason) => {
                ("explicitly-armed", reason, None)
            }
            Preflight::BlockedUnresolvedLiveGate => {
                ("explicitly-armed", BLOCKED_GATE, Some(BLOCKED_GATE))
            }
        };
        Self {
            schema_version: 1,
            artifact_kind: "sanitized-automarker-one-marker-active-canary-preflight",
            mode,
            outcome,
            exact_build: false,
            exact_pack_digest: false,
            exact_local_game_process_required: true,
            exact_syn_owned_tuple_required: true,
            reflect_arbitration_required: true,
            complete_fresh_world_use_slot_249858_carrier_required: true,
            checksum_repair_required: true,
            deterministic_retransmission_mapping_required: true,
            reverse_ack_rpc_authoritative_add_required: true,
            player_distance_gate: false,
            packet_or_memory_write_attempted: false,
            unresolved_gate,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Automarker one-marker canary failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Arguments::parse(env::args_os().skip(1).collect())?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let preflight = evaluate_preflight(&args);
    let mut receipt = Receipt::from_preflight(preflight);
    receipt.exact_build = args.build.as_deref() == Some(AUTOMARKER_REQUEST_BUILD);
    receipt.exact_pack_digest = args.pack_digest.as_deref() == Some(AUTOMARKER_REQUEST_PACK_DIGEST);
    write_receipt(&args.output, &receipt)?;

    match preflight {
        Preflight::DryRun => Ok(()),
        Preflight::BlockedMissing(reason) | Preflight::BlockedInvalid(reason) => Err(reason.into()),
        Preflight::BlockedUnresolvedLiveGate => Err(
            "active interception is disabled: authoritative inbound RPC/marker decoding and fresh runtime context wiring are not yet trustworthy".into(),
        ),
    }
}

fn evaluate_preflight(args: &Arguments) -> Preflight {
    if args.arm_token.is_none() {
        return Preflight::DryRun;
    }
    if args.arm_token.as_deref() != Some(DRIVER_ARM_TOKEN) {
        return Preflight::BlockedInvalid("invalid_literal_arm_token");
    }
    if args.process_id.is_none() {
        return Preflight::BlockedMissing("missing_exact_local_process_id");
    }
    if args.dependency_directory.is_none() {
        return Preflight::BlockedMissing("missing_pinned_windivert_dependency_directory");
    }
    if args.build.as_deref() != Some(AUTOMARKER_REQUEST_BUILD) {
        return Preflight::BlockedInvalid("exact_build_mismatch");
    }
    if args.pack_digest.as_deref() != Some(AUTOMARKER_REQUEST_PACK_DIGEST) {
        return Preflight::BlockedInvalid("exact_pack_digest_mismatch");
    }
    if args
        .scene_family
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Preflight::BlockedMissing("missing_exact_scene_family");
    }
    if !matches!(args.marker_number, Some(1..=6)) {
        return Preflight::BlockedInvalid("invalid_marker_number");
    }
    if args.target.is_none_or(|target| {
        [target.x, target.y, target.z]
            .into_iter()
            .any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
    }) {
        return Preflight::BlockedInvalid("invalid_target_xyz");
    }
    if args.local_actor_id.is_none_or(|value| value == 0) {
        return Preflight::BlockedMissing("missing_local_actor_identity");
    }
    if args.connection_epoch.is_none_or(|value| value == 0)
        || args.client_address.is_none()
        || args.client_port.is_none_or(|value| value == 0)
        || args.server_address.is_none()
        || args.server_port.is_none_or(|value| value == 0)
    {
        return Preflight::BlockedMissing("missing_exact_syn_owned_tuple_epoch");
    }
    if args.runtime_revision.is_none_or(|value| value == 0)
        || args.observed_micros.is_none_or(|value| value == 0)
        || args.observation_ordinal.is_none_or(|value| value == 0)
    {
        return Preflight::BlockedMissing("missing_fresh_runtime_baseline");
    }
    if args.baseline_marker_instances.len() > 1 || args.baseline_marker_instances.contains(&0) {
        return Preflight::BlockedInvalid("ambiguous_same_number_marker_baseline");
    }

    // Deliberately before any DLL load, WinDivert handle, packet receive, or
    // send. The committed REFLECT arbitration and checksum boundary will be
    // called here only after authoritative inbound decoding and runtime
    // context provenance are wired into this executable.
    let _inner_coordinator_literal = SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN;
    Preflight::BlockedUnresolvedLiveGate
}

fn write_receipt(path: &PathBuf, receipt: &Receipt) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}

impl Arguments {
    fn parse(values: Vec<OsString>) -> Result<Self, Box<dyn Error>> {
        let mut args = Self {
            arm_token: None,
            process_id: None,
            dependency_directory: None,
            build: None,
            pack_digest: None,
            scene_family: None,
            marker_number: None,
            target: None,
            local_actor_id: None,
            connection_epoch: None,
            client_address: None,
            client_port: None,
            server_address: None,
            server_port: None,
            runtime_revision: None,
            observed_micros: None,
            observation_ordinal: None,
            baseline_marker_instances: Vec::new(),
            output: PathBuf::from("automarker-one-marker-canary.v1.json"),
        };
        let mut index = 0;
        let mut xyz = [None, None, None];
        while index < values.len() {
            let key = values[index].to_string_lossy();
            let value = |index: &mut usize| -> Result<String, Box<dyn Error>> {
                *index += 1;
                Ok(values
                    .get(*index)
                    .ok_or("missing option value")?
                    .to_string_lossy()
                    .into_owned())
            };
            match key.as_ref() {
                "--arm" => args.arm_token = Some(value(&mut index)?),
                "--process-id" => args.process_id = Some(value(&mut index)?.parse()?),
                "--dependency-directory" => {
                    args.dependency_directory = Some(value(&mut index)?.into())
                }
                "--build" => args.build = Some(value(&mut index)?),
                "--pack-digest" => args.pack_digest = Some(value(&mut index)?),
                "--scene-family" => args.scene_family = Some(value(&mut index)?),
                "--marker-number" => args.marker_number = Some(value(&mut index)?.parse()?),
                "--x" => xyz[0] = Some(value(&mut index)?.parse()?),
                "--y" => xyz[1] = Some(value(&mut index)?.parse()?),
                "--z" => xyz[2] = Some(value(&mut index)?.parse()?),
                "--local-actor-id" => args.local_actor_id = Some(value(&mut index)?.parse()?),
                "--connection-epoch" => args.connection_epoch = Some(value(&mut index)?.parse()?),
                "--client-address" => args.client_address = Some(value(&mut index)?.parse()?),
                "--client-port" => args.client_port = Some(value(&mut index)?.parse()?),
                "--server-address" => args.server_address = Some(value(&mut index)?.parse()?),
                "--server-port" => args.server_port = Some(value(&mut index)?.parse()?),
                "--runtime-revision" => args.runtime_revision = Some(value(&mut index)?.parse()?),
                "--observed-micros" => args.observed_micros = Some(value(&mut index)?.parse()?),
                "--observation-ordinal" => {
                    args.observation_ordinal = Some(value(&mut index)?.parse()?)
                }
                "--baseline-marker-instance" => args
                    .baseline_marker_instances
                    .push(value(&mut index)?.parse()?),
                "--output" => args.output = value(&mut index)?.into(),
                _ => return Err(format!("unknown option {key}").into()),
            }
            index += 1;
        }
        if let [Some(x), Some(y), Some(z)] = xyz {
            args.target = Some(AutomarkerRequestXyz { x, y, z });
        } else if xyz.iter().any(Option::is_some) {
            return Err("--x, --y, and --z must be supplied together".into());
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed() -> Arguments {
        Arguments {
            arm_token: Some(DRIVER_ARM_TOKEN.into()),
            process_id: Some(42),
            dependency_directory: Some("WinDivert-2.2.2-A".into()),
            build: Some(AUTOMARKER_REQUEST_BUILD.into()),
            pack_digest: Some(AUTOMARKER_REQUEST_PACK_DIGEST.into()),
            scene_family: Some("mech-facility".into()),
            marker_number: Some(1),
            target: Some(AutomarkerRequestXyz {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
            local_actor_id: Some(99),
            connection_epoch: Some(7),
            client_address: Some("10.0.0.2".parse().unwrap()),
            client_port: Some(50_000),
            server_address: Some("10.0.0.3".parse().unwrap()),
            server_port: Some(443),
            runtime_revision: Some(10),
            observed_micros: Some(20),
            observation_ordinal: Some(30),
            baseline_marker_instances: vec![123],
            output: "unused.json".into(),
        }
    }

    #[test]
    fn dry_run_is_incapable_of_live_activation() {
        let mut args = armed();
        args.arm_token = None;
        assert_eq!(evaluate_preflight(&args), Preflight::DryRun);
        let receipt = Receipt::from_preflight(Preflight::DryRun);
        assert!(!receipt.packet_or_memory_write_attempted);
        assert!(!receipt.player_distance_gate);
    }

    #[test]
    fn complete_inputs_still_fail_closed_at_unresolved_live_decoder() {
        assert_eq!(
            evaluate_preflight(&armed()),
            Preflight::BlockedUnresolvedLiveGate
        );
    }

    #[test]
    fn literal_consent_build_and_pack_are_mandatory() {
        let mut args = armed();
        args.arm_token = Some("wrong".into());
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedInvalid("invalid_literal_arm_token")
        );
        let mut args = armed();
        args.build = Some("other".into());
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedInvalid("exact_build_mismatch")
        );
        let mut args = armed();
        args.pack_digest = Some("other".into());
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedInvalid("exact_pack_digest_mismatch")
        );
    }

    #[test]
    fn tuple_baseline_and_xyz_are_strict_but_distance_is_not_a_gate() {
        let mut args = armed();
        args.client_port = Some(0);
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedMissing("missing_exact_syn_owned_tuple_epoch")
        );
        let mut args = armed();
        args.baseline_marker_instances.push(456);
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedInvalid("ambiguous_same_number_marker_baseline")
        );
        let mut args = armed();
        args.target = Some(AutomarkerRequestXyz {
            x: 999_999.0,
            y: -999_999.0,
            z: 0.0,
        });
        assert_eq!(
            evaluate_preflight(&args),
            Preflight::BlockedUnresolvedLiveGate
        );
    }

    #[test]
    fn parser_rejects_partial_xyz_and_unknown_options() {
        assert!(Arguments::parse(vec!["--x".into(), "1".into()]).is_err());
        assert!(Arguments::parse(vec!["--surprise".into()]).is_err());
    }

    #[test]
    fn pre_send_cancellation_releases_only_the_coordinator_stored_original() {
        let packet = vec![1, 2, 3, 4];
        let address = AutomarkerWinDivertAddress::from_opaque_bytes([7; 80]);
        let result = exact_original_after_cancel(
            AutomarkerBridgePrepareDisposition::SendOriginal(
                rlogs_game_bpsr::AutomarkerBridgeOriginalPacket {
                    packet: packet.clone(),
                    address,
                },
            ),
            OneMarkerDriverError::ChecksumCancellationDidNotReturnOriginal,
        );
        assert_eq!(result, OneMarkerPreparedSend::Original { packet, address });

        let failed = exact_original_after_cancel(
            AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::PreparationMismatch,
            ),
            OneMarkerDriverError::ChecksumCancellationDidNotReturnOriginal,
        );
        assert_eq!(
            failed,
            OneMarkerPreparedSend::AbortWithoutReinject(
                OneMarkerDriverError::ChecksumCancellationDidNotReturnOriginal
            )
        );
    }
}
