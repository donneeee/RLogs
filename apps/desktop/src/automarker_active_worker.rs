//! Owned, injectable lifecycle for a future active Automarker packet worker.
//!
//! This module is deliberately not connected to WinDivert or the live bridge.
//! It makes the resource and failure ordering testable without granting the
//! desktop host a production packet-mutation path. The reviewed coordinator
//! can be adapted to [`ActiveAutomarkerCoordinator`] only in the later slice
//! that deliberately wires and enables active placement.

#![allow(dead_code)]

use std::{
    mem::ManuallyDrop,
    ops::Deref,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use rlogs_game_bpsr::{AutomarkerPacketSendPreparation, AutomarkerWinDivertAddress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveAutomarkerPacket {
    pub bytes: Vec<u8>,
    pub address: AutomarkerWinDivertAddress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerWake {
    Packet(ActiveAutomarkerPacket),
    /// The backend observed bytes outside the reviewed packet role despite
    /// the exact kernel filter. They remain diverted and must bypass all
    /// mutation/classification logic for byte-exact reinjection.
    PassThroughOnly(ActiveAutomarkerPacket),
    Timeout,
    EndOfStream,
}

pub(crate) trait ActiveAutomarkerBackend: Send + 'static {
    /// Receive one intercepted packet or return `Timeout` within a bounded
    /// polling interval. This must not close or shut down interception; stop
    /// requests still need the same handle to observe ACK/FIN retirement.
    fn receive(&mut self) -> Result<ActiveAutomarkerWake, String>;

    /// This boundary must return only a checksum-repaired packet whose bytes
    /// correspond to the coordinator-approved change.
    fn prepare_modified_send(
        &mut self,
        original: &ActiveAutomarkerPacket,
        approved_changed_bytes: &[u8],
    ) -> Result<AutomarkerPacketSendPreparation, String>;

    fn send(&mut self, packet: &ActiveAutomarkerPacket) -> Result<usize, String>;

    /// Explicit close exists so the worker can prove that interception closes
    /// before the coordinator's retransmission ledger is discarded.
    fn close_interception(&mut self) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerDisposition {
    /// The packet was unrelated. It must be reinjected byte-for-byte.
    PassThrough,
    /// The exact full World.UseSlot carrier is held while checksum repair and
    /// the one authorized modified send are completed.
    HoldExactCarrier {
        preparation_id: u64,
        approved_changed_bytes: Vec<u8>,
    },
    /// The coordinator cannot prove a safe reinjection decision.
    AbortWithoutReinject,
    /// A FIN was observed and must be passed through. A single FIN only proves
    /// one TCP direction is half-closed and cannot retire rewrite ownership.
    FinObservedPassThrough,
    /// The coordinator has independently proven that the connection is fully
    /// terminated (for example, an exact matching RST or completed close).
    ConnectionTerminatedAfterPassThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerClassificationMode {
    /// Existing rewrites and reverse retirement traffic are serviced and one
    /// new exact carrier may be authorized.
    AuthorizeNewCarrier,
    /// Only already-existing rewrite/ACK/close state may be serviced. A new
    /// carrier must be passed through unchanged or fail closed.
    DrainExistingOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerSendOutcome {
    Complete,
    FailedOrIndeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerCancelDisposition {
    /// Authorize reinjection of the exact original packet held by the worker.
    /// The coordinator cannot substitute packet bytes or WinDivert metadata.
    ReinjectHeldOriginal,
    AbortWithoutReinject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerCommitDisposition {
    Committed,
    CommittedButConfirmationAborted,
    AbortWithoutReinject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerTimeoutDisposition {
    Continue,
    Terminate,
}

pub(crate) trait ActiveAutomarkerCoordinator: Send + 'static {
    fn classify(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        mode: ActiveAutomarkerClassificationMode,
    ) -> ActiveAutomarkerDisposition;

    fn cancel_after_checksum_failure(
        &mut self,
        preparation_id: u64,
    ) -> ActiveAutomarkerCancelDisposition;

    fn commit_send(
        &mut self,
        preparation_id: u64,
        outcome: ActiveAutomarkerSendOutcome,
    ) -> ActiveAutomarkerCommitDisposition;

    /// Persist an indeterminate-emission obligation before the backend is
    /// permitted to begin a modified send. This makes both send and commit
    /// panics fail closed without losing retransmission ownership.
    fn record_modified_send_may_begin(
        &mut self,
        preparation_id: u64,
    ) -> ActiveAutomarkerCommitDisposition;

    fn observe_timeout(&mut self) -> ActiveAutomarkerTimeoutDisposition;

    /// True while any TCP byte range already emitted with changed bytes can
    /// still be retransmitted by the game. Interception and the ledger must
    /// both remain alive until this becomes false.
    fn rewrite_obligation_active(&self) -> bool;

    /// Return only originals that the coordinator proves have never had an
    /// attempted modified emission. Indeterminate sends are never recoverable.
    fn drain_unsent_originals(&mut self) -> Vec<ActiveAutomarkerPacket>;

    fn discard_retransmission_ledger(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerWorkerExit {
    Stopped,
    EndOfStream,
    Timeout,
    ConnectionTerminated,
    BackendFailure,
    CoordinatorAbort,
    /// An ordinary exit was requested after a modified emission. The worker
    /// stayed alive until an exact ACK or connection-termination observation
    /// retired the rewrite obligation.
    RequiresConnectionTermination,
    /// The worker reached a bounded fatal condition while transport ownership
    /// remained unresolved. Its backend/coordinator are returned to the caller.
    FatalOwnershipRetained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerStopDisposition {
    StopRequested,
    BlockedByRewriteObligation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveAutomarkerWorkerReport {
    pub exit: ActiveAutomarkerWorkerExit,
    pub packets_received: u64,
    pub unchanged_sent: u64,
    pub modified_sent: u64,
    pub send_failures: u64,
}

pub(crate) struct ActiveAutomarkerFatalOwnership {
    backend: ManuallyDrop<Box<dyn ActiveAutomarkerBackend>>,
    coordinator: ManuallyDrop<Box<dyn ActiveAutomarkerCoordinator>>,
    indeterminate_packet: Option<ActiveAutomarkerPacket>,
    released: bool,
}

impl ActiveAutomarkerFatalOwnership {
    /// Retry the previously unconfirmed close. Only a confirmed close permits
    /// the ledger to be discarded and both owners to be released.
    pub(crate) fn retry_close_and_discard(&mut self) -> Result<(), String> {
        if self.released {
            return Err("Automarker fatal ownership was already released".to_owned());
        }
        if self.coordinator.rewrite_obligation_active() {
            return Err(
                "cannot discard Automarker ownership while retransmission obligation remains"
                    .to_owned(),
            );
        }
        if self.indeterminate_packet.is_some() {
            return Err(
                "cannot close Automarker interception after an indeterminate packet send"
                    .to_owned(),
            );
        }
        self.backend.close_interception()?;
        self.coordinator.discard_retransmission_ledger();
        // SAFETY: `released` makes these one-shot and Drop deliberately does
        // not touch unresolved owners.
        unsafe {
            ManuallyDrop::drop(&mut self.backend);
            ManuallyDrop::drop(&mut self.coordinator);
        }
        self.released = true;
        Ok(())
    }
}

impl Drop for ActiveAutomarkerFatalOwnership {
    fn drop(&mut self) {
        // Unreleased ownership is intentionally retained. This is an explicit
        // fatal result, not a successful teardown; callers may retry closure.
    }
}

pub(crate) struct ActiveAutomarkerWorkerCompletion {
    pub report: ActiveAutomarkerWorkerReport,
    pub fatal_ownership: Option<ActiveAutomarkerFatalOwnership>,
}

impl Deref for ActiveAutomarkerWorkerCompletion {
    type Target = ActiveAutomarkerWorkerReport;

    fn deref(&self) -> &Self::Target {
        &self.report
    }
}

pub(crate) struct ActiveAutomarkerWorkerBundle {
    stop_requested: Arc<AtomicBool>,
    rewrite_obligation_active: Arc<AtomicBool>,
    termination_required: Arc<AtomicBool>,
    stop_acknowledged: Arc<AtomicBool>,
    worker: Option<JoinHandle<ActiveAutomarkerWorkerCompletion>>,
}

impl ActiveAutomarkerWorkerBundle {
    pub(crate) fn spawn<B, C>(backend: B, coordinator: C) -> Result<Self, String>
    where
        B: ActiveAutomarkerBackend,
        C: ActiveAutomarkerCoordinator,
    {
        let stop_requested = Arc::new(AtomicBool::new(false));
        let rewrite_obligation_active = Arc::new(AtomicBool::new(false));
        let termination_required = Arc::new(AtomicBool::new(false));
        let stop_acknowledged = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop_requested);
        let worker_obligation = Arc::clone(&rewrite_obligation_active);
        let worker_termination_required = Arc::clone(&termination_required);
        let worker_stop_acknowledged = Arc::clone(&stop_acknowledged);
        let owned = ActiveWorkerOwnership::new(Box::new(backend), Box::new(coordinator));
        let worker = thread::Builder::new()
            .name("rlogs-automarker-active".into())
            .spawn(move || {
                run_worker(
                    owned,
                    worker_stop,
                    worker_obligation,
                    worker_termination_required,
                    worker_stop_acknowledged,
                )
            })
            .map_err(|error| format!("failed to spawn Automarker active worker: {error}"))?;
        Ok(Self {
            stop_requested,
            rewrite_obligation_active,
            termination_required,
            stop_acknowledged,
            worker: Some(worker),
        })
    }

    pub(crate) fn request_stop(&self) -> Result<ActiveAutomarkerStopDisposition, String> {
        self.stop_requested.store(true, Ordering::Release);
        let started = Instant::now();
        while !self.stop_acknowledged.load(Ordering::Acquire)
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            if started.elapsed() >= Duration::from_secs(1) {
                return Err("Automarker active worker did not acknowledge stop".to_owned());
            }
            thread::yield_now();
        }
        Ok(
            if self.rewrite_obligation_active.load(Ordering::Acquire)
                || self.termination_required.load(Ordering::Acquire)
            {
                ActiveAutomarkerStopDisposition::BlockedByRewriteObligation
            } else {
                ActiveAutomarkerStopDisposition::StopRequested
            },
        )
    }

    pub(crate) fn stop_drain_join(&mut self) -> Result<ActiveAutomarkerWorkerCompletion, String> {
        if self.request_stop()? == ActiveAutomarkerStopDisposition::BlockedByRewriteObligation {
            return Err(
                "Automarker active worker is retaining interception until the exact rewrite obligation is retired"
                    .to_owned(),
            );
        }
        self.worker
            .take()
            .ok_or_else(|| "Automarker active worker was already joined".to_owned())?
            .join()
            .map_err(|_| "Automarker active worker panicked".to_owned())
    }

    pub(crate) fn join(mut self) -> Result<ActiveAutomarkerWorkerCompletion, String> {
        self.worker
            .take()
            .ok_or_else(|| "Automarker active worker was already joined".to_owned())?
            .join()
            .map_err(|_| "Automarker active worker panicked".to_owned())
    }
}

impl Drop for ActiveAutomarkerWorkerBundle {
    fn drop(&mut self) {
        self.stop_requested.store(true, Ordering::Release);
        // Never make implicit destruction a second teardown path. Detaching
        // leaves the worker as the sole owner of interception and its ledger;
        // it will close them in reviewed order only after every rewrite
        // obligation is retired.
        self.worker.take();
    }
}

/// Owns interception and coordinator state in an explicit destruction order.
/// On unwind, interception is closed and the backend is dropped before the
/// coordinator (and therefore its retransmission ledger) can be destroyed.
struct ActiveWorkerOwnership {
    backend: Option<Box<dyn ActiveAutomarkerBackend>>,
    coordinator: Option<Box<dyn ActiveAutomarkerCoordinator>>,
    interception_closed: bool,
    indeterminate_packet: Option<ActiveAutomarkerPacket>,
}

impl ActiveWorkerOwnership {
    fn new(
        backend: Box<dyn ActiveAutomarkerBackend>,
        coordinator: Box<dyn ActiveAutomarkerCoordinator>,
    ) -> Self {
        Self {
            backend: Some(backend),
            coordinator: Some(coordinator),
            interception_closed: false,
            indeterminate_packet: None,
        }
    }

    fn backend(&mut self) -> &mut dyn ActiveAutomarkerBackend {
        self.backend.as_deref_mut().expect("backend remains owned")
    }

    fn coordinator(&mut self) -> &mut dyn ActiveAutomarkerCoordinator {
        self.coordinator
            .as_deref_mut()
            .expect("coordinator remains owned")
    }

    fn close_confirmed(&mut self) -> Result<(), String> {
        if self.indeterminate_packet.is_some() {
            return Err("an Automarker packet send remains indeterminate".into());
        }
        self.backend().close_interception()?;
        self.interception_closed = true;
        Ok(())
    }

    fn take_fatal_ownership(&mut self) -> ActiveAutomarkerFatalOwnership {
        self.interception_closed = true;
        ActiveAutomarkerFatalOwnership {
            backend: ManuallyDrop::new(self.backend.take().expect("backend remains owned")),
            coordinator: ManuallyDrop::new(
                self.coordinator.take().expect("coordinator remains owned"),
            ),
            indeterminate_packet: self.indeterminate_packet.take(),
            released: false,
        }
    }

    fn send_exact(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        modified: bool,
        report: &mut ActiveAutomarkerWorkerReport,
    ) -> bool {
        if self.indeterminate_packet.is_some() {
            report.send_failures = report.send_failures.saturating_add(1);
            return false;
        }
        // Record ownership before entering a call whose outcome can fail,
        // short-write, or unwind. Clear it only after exact completion.
        self.indeterminate_packet = Some(packet.clone());
        let complete = send_exact(self.backend(), packet, modified, report);
        if complete {
            self.indeterminate_packet = None;
        }
        complete
    }
}

impl Drop for ActiveWorkerOwnership {
    fn drop(&mut self) {
        if !self.interception_closed {
            // Best effort during unwind. Crucially, the backend is still
            // dropped before the coordinator even if close itself fails.
            let close_confirmed = self
                .backend
                .as_mut()
                .is_some_and(|backend| backend.close_interception().is_ok());
            if !close_confirmed {
                // A panic escaping this worker is a process-fatal ownership
                // violation. Normal worker execution catches panics and
                // returns explicit fatal ownership instead.
                std::process::abort();
            }
        }
        drop(self.backend.take());
        drop(self.coordinator.take());
    }
}

fn run_worker(
    mut owned: ActiveWorkerOwnership,
    stop_requested: Arc<AtomicBool>,
    shared_rewrite_obligation: Arc<AtomicBool>,
    shared_termination_required: Arc<AtomicBool>,
    stop_acknowledged: Arc<AtomicBool>,
) -> ActiveAutomarkerWorkerCompletion {
    struct TerminalAcknowledgement(Arc<AtomicBool>);
    impl Drop for TerminalAcknowledgement {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let _terminal_acknowledgement = TerminalAcknowledgement(Arc::clone(&stop_acknowledged));

    let run_result = catch_unwind(AssertUnwindSafe(|| {
        let mut report = ActiveAutomarkerWorkerReport {
            exit: ActiveAutomarkerWorkerExit::EndOfStream,
            packets_received: 0,
            unchanged_sent: 0,
            modified_sent: 0,
            send_failures: 0,
        };

        let mut pending_exit = None;
        loop {
            let rewrite_obligation = owned.coordinator().rewrite_obligation_active();
            shared_rewrite_obligation.store(rewrite_obligation, Ordering::Release);
            if pending_exit.is_some() && rewrite_obligation {
                shared_termination_required.store(true, Ordering::Release);
            } else if let Some(exit) = pending_exit {
                report.exit = if shared_termination_required.load(Ordering::Acquire)
                    && exit != ActiveAutomarkerWorkerExit::CoordinatorAbort
                {
                    ActiveAutomarkerWorkerExit::RequiresConnectionTermination
                } else {
                    exit
                };
                break (report, false);
            }

            let received = owned.backend().receive();
            let wake = match received {
                Ok(value) => value,
                Err(_) => {
                    pending_exit.get_or_insert(ActiveAutomarkerWorkerExit::BackendFailure);
                    if stop_requested.load(Ordering::Acquire) {
                        stop_acknowledged.store(true, Ordering::Release);
                    }
                    if owned.coordinator().rewrite_obligation_active() {
                        shared_termination_required.store(true, Ordering::Release);
                        report.exit = ActiveAutomarkerWorkerExit::FatalOwnershipRetained;
                        break (report, true);
                    }
                    continue;
                }
            };
            match wake {
                ActiveAutomarkerWake::EndOfStream => {
                    pending_exit.get_or_insert(if stop_requested.load(Ordering::Acquire) {
                        ActiveAutomarkerWorkerExit::Stopped
                    } else {
                        ActiveAutomarkerWorkerExit::EndOfStream
                    });
                    if owned.coordinator().rewrite_obligation_active() {
                        shared_termination_required.store(true, Ordering::Release);
                        report.exit = ActiveAutomarkerWorkerExit::FatalOwnershipRetained;
                        break (report, true);
                    }
                }
                ActiveAutomarkerWake::Timeout => {
                    if owned.coordinator().observe_timeout()
                        == ActiveAutomarkerTimeoutDisposition::Terminate
                    {
                        pending_exit.get_or_insert(ActiveAutomarkerWorkerExit::Timeout);
                    }
                }
                ActiveAutomarkerWake::PassThroughOnly(packet) => {
                    report.packets_received = report.packets_received.saturating_add(1);
                    if !owned.send_exact(&packet, false, &mut report) {
                        pending_exit.get_or_insert(ActiveAutomarkerWorkerExit::BackendFailure);
                    }
                }
                ActiveAutomarkerWake::Packet(packet) => {
                    report.packets_received = report.packets_received.saturating_add(1);
                    if stop_requested.load(Ordering::Acquire) {
                        pending_exit.get_or_insert(ActiveAutomarkerWorkerExit::Stopped);
                        stop_acknowledged.store(true, Ordering::Release);
                    }
                    let mode = if pending_exit.is_some() {
                        ActiveAutomarkerClassificationMode::DrainExistingOnly
                    } else {
                        ActiveAutomarkerClassificationMode::AuthorizeNewCarrier
                    };
                    match owned.coordinator().classify(&packet, mode) {
                        ActiveAutomarkerDisposition::PassThrough => {
                            if !owned.send_exact(&packet, false, &mut report) {
                                pending_exit
                                    .get_or_insert(ActiveAutomarkerWorkerExit::BackendFailure);
                            }
                        }
                        ActiveAutomarkerDisposition::FinObservedPassThrough => {
                            if !owned.send_exact(&packet, false, &mut report) {
                                pending_exit
                                    .get_or_insert(ActiveAutomarkerWorkerExit::BackendFailure);
                            }
                        }
                        ActiveAutomarkerDisposition::ConnectionTerminatedAfterPassThrough => {
                            pending_exit.get_or_insert(
                                if owned.send_exact(&packet, false, &mut report) {
                                    ActiveAutomarkerWorkerExit::ConnectionTerminated
                                } else {
                                    ActiveAutomarkerWorkerExit::BackendFailure
                                },
                            );
                        }
                        ActiveAutomarkerDisposition::AbortWithoutReinject => {
                            pending_exit
                                .get_or_insert(ActiveAutomarkerWorkerExit::CoordinatorAbort);
                        }
                        ActiveAutomarkerDisposition::HoldExactCarrier {
                            preparation_id,
                            approved_changed_bytes,
                        } => match owned
                            .backend()
                            .prepare_modified_send(&packet, &approved_changed_bytes)
                        {
                            Ok(AutomarkerPacketSendPreparation::ModifiedAuthorized {
                                packet: repaired,
                                address,
                            }) if modified_preparation_matches(
                                &packet,
                                &approved_changed_bytes,
                                &repaired,
                                address,
                            ) =>
                            {
                                let repaired = ActiveAutomarkerPacket {
                                    bytes: repaired,
                                    address,
                                };
                                let begin = owned
                                    .coordinator()
                                    .record_modified_send_may_begin(preparation_id);
                                if begin != ActiveAutomarkerCommitDisposition::Committed {
                                    pending_exit.get_or_insert(
                                        ActiveAutomarkerWorkerExit::CoordinatorAbort,
                                    );
                                    continue;
                                }
                                let complete = owned.send_exact(&repaired, true, &mut report);
                                let commit = owned.coordinator().commit_send(
                                    preparation_id,
                                    if complete {
                                        ActiveAutomarkerSendOutcome::Complete
                                    } else {
                                        ActiveAutomarkerSendOutcome::FailedOrIndeterminate
                                    },
                                );
                                if commit != ActiveAutomarkerCommitDisposition::Committed {
                                    pending_exit.get_or_insert(
                                        ActiveAutomarkerWorkerExit::CoordinatorAbort,
                                    );
                                }
                                if !complete {
                                    pending_exit
                                        .get_or_insert(ActiveAutomarkerWorkerExit::BackendFailure);
                                }
                            }
                            Ok(AutomarkerPacketSendPreparation::Original { .. })
                            | Ok(_)
                            | Err(_) => {
                                match owned
                                    .coordinator()
                                    .cancel_after_checksum_failure(preparation_id)
                                {
                                    ActiveAutomarkerCancelDisposition::ReinjectHeldOriginal => {
                                        if !owned.send_exact(&packet, false, &mut report) {
                                            pending_exit.get_or_insert(
                                                ActiveAutomarkerWorkerExit::BackendFailure,
                                            );
                                        }
                                    }
                                    ActiveAutomarkerCancelDisposition::AbortWithoutReinject => {
                                        pending_exit.get_or_insert(
                                            ActiveAutomarkerWorkerExit::CoordinatorAbort,
                                        );
                                    }
                                }
                            }
                        },
                    }
                }
            }
            let current_obligation = owned.coordinator().rewrite_obligation_active();
            shared_rewrite_obligation.store(current_obligation, Ordering::Release);
            if pending_exit.is_some() && current_obligation {
                shared_termination_required.store(true, Ordering::Release);
            }
            if stop_requested.load(Ordering::Acquire) {
                pending_exit.get_or_insert(ActiveAutomarkerWorkerExit::Stopped);
                if current_obligation {
                    shared_termination_required.store(true, Ordering::Release);
                }
                stop_acknowledged.store(true, Ordering::Release);
            }
        }
    }));

    let (mut report, retain_fatal) = match run_result {
        Ok(result) => result,
        Err(_) => (
            ActiveAutomarkerWorkerReport {
                exit: ActiveAutomarkerWorkerExit::FatalOwnershipRetained,
                packets_received: 0,
                unchanged_sent: 0,
                modified_sent: 0,
                send_failures: 0,
            },
            true,
        ),
    };
    if retain_fatal {
        return ActiveAutomarkerWorkerCompletion {
            report,
            fatal_ownership: Some(owned.take_fatal_ownership()),
        };
    }

    // Recover only coordinator-proven never-emitted originals. Then close the
    // intercepting handle before destroying retransmission state.
    for original in owned.coordinator().drain_unsent_originals() {
        if !owned.send_exact(&original, false, &mut report) {
            report.exit = ActiveAutomarkerWorkerExit::BackendFailure;
        }
    }
    if owned.close_confirmed().is_err() {
        report.exit = ActiveAutomarkerWorkerExit::FatalOwnershipRetained;
        return ActiveAutomarkerWorkerCompletion {
            report,
            fatal_ownership: Some(owned.take_fatal_ownership()),
        };
    }
    owned.coordinator().discard_retransmission_ledger();
    shared_rewrite_obligation.store(false, Ordering::Release);
    shared_termination_required.store(false, Ordering::Release);
    ActiveAutomarkerWorkerCompletion {
        report,
        fatal_ownership: None,
    }
}

fn modified_preparation_matches(
    original: &ActiveAutomarkerPacket,
    approved: &[u8],
    repaired: &[u8],
    repaired_address: AutomarkerWinDivertAddress,
) -> bool {
    if approved.len() != original.bytes.len()
        || repaired.len() != original.bytes.len()
        || approved == original.bytes
        || repaired.len() < 40
        || repaired[0] >> 4 != 4
        || repaired[9] != 6
    {
        return false;
    }
    let ip_header_len = usize::from(repaired[0] & 0x0f) * 4;
    let total_len = usize::from(u16::from_be_bytes([repaired[2], repaired[3]]));
    if ip_header_len < 20 || total_len != repaired.len() || ip_header_len + 20 > total_len {
        return false;
    }
    let tcp_checksum = ip_header_len + 16;
    for (index, (&expected, &actual)) in approved.iter().zip(repaired).enumerate() {
        let checksum_byte = (10..12).contains(&index)
            || (tcp_checksum..tcp_checksum.saturating_add(2)).contains(&index);
        if !checksum_byte && expected != actual {
            return false;
        }
    }

    let original_address = original.address.into_opaque_bytes();
    let candidate_address = repaired_address.into_opaque_bytes();
    if original_address[..8] != candidate_address[..8]
        || original_address[12..] != candidate_address[12..]
    {
        return false;
    }
    let original_flags = u32::from_ne_bytes(original_address[8..12].try_into().unwrap());
    let candidate_flags = u32::from_ne_bytes(candidate_address[8..12].try_into().unwrap());
    const CHECKSUM_FLAGS_ALLOWED_TO_CHANGE: u32 = (1 << 21) | (1 << 22);
    (original_flags ^ candidate_flags) & !CHECKSUM_FLAGS_ALLOWED_TO_CHANGE == 0
        && candidate_flags & CHECKSUM_FLAGS_ALLOWED_TO_CHANGE == CHECKSUM_FLAGS_ALLOWED_TO_CHANGE
}

fn send_exact<B: ActiveAutomarkerBackend + ?Sized>(
    backend: &mut B,
    packet: &ActiveAutomarkerPacket,
    modified: bool,
    report: &mut ActiveAutomarkerWorkerReport,
) -> bool {
    match backend.send(packet) {
        Ok(sent) if sent == packet.bytes.len() => {
            if modified {
                report.modified_sent = report.modified_sent.saturating_add(1);
            } else {
                report.unchanged_sent = report.unchanged_sent.saturating_add(1);
            }
            true
        }
        Ok(_) | Err(_) => {
            report.send_failures = report.send_failures.saturating_add(1);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Condvar, Mutex},
        time::Duration,
    };

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Sent(Vec<u8>, [u8; 80]),
        Commit(u64, ActiveAutomarkerSendOutcome),
        AckRetired,
        FinObserved,
        BackendClosed,
        BackendDropped,
        LedgerDiscarded,
        CoordinatorDropped,
    }

    #[derive(Default)]
    struct Shared {
        ingress: VecDeque<Result<ActiveAutomarkerWake, String>>,
        events: Vec<Event>,
        short_send_for: Option<Vec<u8>>,
        failed_send_for: Option<Vec<u8>>,
        checksum_fails: bool,
        checksum_returns_original: bool,
        close_fails: bool,
        panic_on_classify: bool,
        panic_on_send: bool,
        panic_on_commit: bool,
        commit_disposition: Option<ActiveAutomarkerCommitDisposition>,
        block_send: bool,
        send_entered: bool,
        release_send: bool,
        block_carrier_commit: bool,
        carrier_commit_entered: bool,
        release_carrier_commit: bool,
    }

    #[derive(Clone, Default)]
    struct Harness(Arc<(Mutex<Shared>, Condvar)>);

    impl Harness {
        fn push(&self, wake: ActiveAutomarkerWake) {
            let (lock, ready) = &*self.0;
            let mut state = lock.lock().unwrap();
            state.ingress.push_back(Ok(wake));
            ready.notify_all();
        }

        fn push_error(&self, error: &str) {
            let (lock, ready) = &*self.0;
            lock.lock()
                .unwrap()
                .ingress
                .push_back(Err(error.to_owned()));
            ready.notify_all();
        }

        fn events(&self) -> Vec<Event> {
            self.0.0.lock().unwrap().events.clone()
        }

        fn wait_for_event(&self, expected: &Event) {
            let started = Instant::now();
            loop {
                if self.events().contains(expected) {
                    return;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(1),
                    "timed out waiting for {expected:?}; observed {:?}",
                    self.events()
                );
                thread::yield_now();
            }
        }

        fn wait_for_carrier_commit(&self) {
            let (lock, ready) = &*self.0;
            let mut state = lock.lock().unwrap();
            while !state.carrier_commit_entered {
                state = ready.wait(state).unwrap();
            }
        }

        fn release_carrier_commit(&self) {
            let (lock, ready) = &*self.0;
            lock.lock().unwrap().release_carrier_commit = true;
            ready.notify_all();
        }

        fn wait_for_send(&self) {
            let (lock, ready) = &*self.0;
            let mut state = lock.lock().unwrap();
            while !state.send_entered {
                state = ready.wait(state).unwrap();
            }
        }

        fn release_send(&self) {
            let (lock, ready) = &*self.0;
            lock.lock().unwrap().release_send = true;
            ready.notify_all();
        }
    }

    struct FakeBackend(Harness);

    impl ActiveAutomarkerBackend for FakeBackend {
        fn receive(&mut self) -> Result<ActiveAutomarkerWake, String> {
            let (lock, ready) = &*self.0.0;
            let mut state = lock.lock().unwrap();
            if state.ingress.is_empty() {
                let (next, _) = ready
                    .wait_timeout(state, Duration::from_millis(10))
                    .unwrap();
                state = next;
            }
            if state.ingress.is_empty() {
                return Ok(ActiveAutomarkerWake::Timeout);
            }
            state.ingress.pop_front().unwrap()
        }

        fn prepare_modified_send(
            &mut self,
            original: &ActiveAutomarkerPacket,
            approved_changed_bytes: &[u8],
        ) -> Result<AutomarkerPacketSendPreparation, String> {
            let state = self.0.0.0.lock().unwrap();
            if state.checksum_fails {
                return Err("injected checksum failure".into());
            }
            if state.checksum_returns_original {
                return Ok(AutomarkerPacketSendPreparation::Original {
                    packet: original.bytes.clone(),
                    address: original.address,
                });
            }
            Ok(AutomarkerPacketSendPreparation::ModifiedAuthorized {
                packet: approved_changed_bytes.to_vec(),
                address: original.address,
            })
        }

        fn send(&mut self, packet: &ActiveAutomarkerPacket) -> Result<usize, String> {
            let (lock, ready) = &*self.0.0;
            let mut state = lock.lock().unwrap();
            if state.block_send {
                state.send_entered = true;
                ready.notify_all();
                while !state.release_send {
                    state = ready.wait(state).unwrap();
                }
            }
            let panic_on_send = state.panic_on_send;
            drop(state);
            assert!(!panic_on_send, "injected send panic");
            let mut state = lock.lock().unwrap();
            state.events.push(Event::Sent(
                packet.bytes.clone(),
                packet.address.into_opaque_bytes(),
            ));
            if state.failed_send_for.as_ref() == Some(&packet.bytes) {
                Err("injected send failure".into())
            } else if state.short_send_for.as_ref() == Some(&packet.bytes) {
                Ok(packet.bytes.len().saturating_sub(1))
            } else {
                Ok(packet.bytes.len())
            }
        }

        fn close_interception(&mut self) -> Result<(), String> {
            let mut state = self.0.0.0.lock().unwrap();
            state.events.push(Event::BackendClosed);
            if state.close_fails {
                Err("injected close failure".into())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for FakeBackend {
        fn drop(&mut self) {
            self.0
                .0
                .0
                .lock()
                .unwrap()
                .events
                .push(Event::BackendDropped);
        }
    }

    struct FakeCoordinator {
        shared: Harness,
        pending: Option<ActiveAutomarkerPacket>,
        rewritten_sequence_active: bool,
        terminate_timeout: bool,
    }

    impl FakeCoordinator {
        fn new(shared: Harness) -> Self {
            Self {
                shared,
                pending: None,
                rewritten_sequence_active: false,
                terminate_timeout: false,
            }
        }
    }

    impl ActiveAutomarkerCoordinator for FakeCoordinator {
        fn classify(
            &mut self,
            packet: &ActiveAutomarkerPacket,
            mode: ActiveAutomarkerClassificationMode,
        ) -> ActiveAutomarkerDisposition {
            assert!(
                !self.shared.0.0.lock().unwrap().panic_on_classify,
                "injected panic"
            );
            match packet.bytes.get(40..).unwrap_or_default() {
                b"carrier" if mode == ActiveAutomarkerClassificationMode::AuthorizeNewCarrier => {
                    self.pending = Some(packet.clone());
                    ActiveAutomarkerDisposition::HoldExactCarrier {
                        preparation_id: 7,
                        approved_changed_bytes: super::tests::packet(b"rewrite").bytes,
                    }
                }
                b"retrans" if self.rewritten_sequence_active => {
                    self.pending = Some(packet.clone());
                    ActiveAutomarkerDisposition::HoldExactCarrier {
                        preparation_id: 8,
                        approved_changed_bytes: super::tests::packet(b"rewrite").bytes,
                    }
                }
                b"ack" => {
                    self.rewritten_sequence_active = false;
                    self.shared
                        .0
                        .0
                        .lock()
                        .unwrap()
                        .events
                        .push(Event::AckRetired);
                    ActiveAutomarkerDisposition::PassThrough
                }
                b"fin" => {
                    self.shared
                        .0
                        .0
                        .lock()
                        .unwrap()
                        .events
                        .push(Event::FinObserved);
                    ActiveAutomarkerDisposition::FinObservedPassThrough
                }
                b"rst" => {
                    self.rewritten_sequence_active = false;
                    ActiveAutomarkerDisposition::ConnectionTerminatedAfterPassThrough
                }
                b"abort" => ActiveAutomarkerDisposition::AbortWithoutReinject,
                _ => ActiveAutomarkerDisposition::PassThrough,
            }
        }

        fn cancel_after_checksum_failure(
            &mut self,
            _preparation_id: u64,
        ) -> ActiveAutomarkerCancelDisposition {
            self.pending
                .take()
                .map(|_| ActiveAutomarkerCancelDisposition::ReinjectHeldOriginal)
                .unwrap_or(ActiveAutomarkerCancelDisposition::AbortWithoutReinject)
        }

        fn commit_send(
            &mut self,
            preparation_id: u64,
            outcome: ActiveAutomarkerSendOutcome,
        ) -> ActiveAutomarkerCommitDisposition {
            self.pending = None;
            {
                let (lock, ready) = &*self.shared.0;
                let mut state = lock.lock().unwrap();
                if state.block_carrier_commit && preparation_id == 7 {
                    state.carrier_commit_entered = true;
                    ready.notify_all();
                    while !state.release_carrier_commit {
                        state = ready.wait(state).unwrap();
                    }
                }
            }
            // A short or failed modified send is indeterminate, not proof that
            // no changed byte reached the stack.
            self.rewritten_sequence_active = true;
            self.shared
                .0
                .0
                .lock()
                .unwrap()
                .events
                .push(Event::Commit(preparation_id, outcome));
            assert!(
                !self.shared.0.0.lock().unwrap().panic_on_commit,
                "injected commit panic"
            );
            self.shared
                .0
                .0
                .lock()
                .unwrap()
                .commit_disposition
                .unwrap_or(ActiveAutomarkerCommitDisposition::Committed)
        }

        fn record_modified_send_may_begin(
            &mut self,
            preparation_id: u64,
        ) -> ActiveAutomarkerCommitDisposition {
            self.rewritten_sequence_active = true;
            self.shared.0.0.lock().unwrap().events.push(Event::Commit(
                preparation_id,
                ActiveAutomarkerSendOutcome::FailedOrIndeterminate,
            ));
            ActiveAutomarkerCommitDisposition::Committed
        }

        fn observe_timeout(&mut self) -> ActiveAutomarkerTimeoutDisposition {
            if self.terminate_timeout {
                ActiveAutomarkerTimeoutDisposition::Terminate
            } else {
                ActiveAutomarkerTimeoutDisposition::Continue
            }
        }

        fn rewrite_obligation_active(&self) -> bool {
            self.rewritten_sequence_active
        }

        fn drain_unsent_originals(&mut self) -> Vec<ActiveAutomarkerPacket> {
            self.pending.take().into_iter().collect()
        }

        fn discard_retransmission_ledger(&mut self) {
            self.shared
                .0
                .0
                .lock()
                .unwrap()
                .events
                .push(Event::LedgerDiscarded);
        }
    }

    impl Drop for FakeCoordinator {
        fn drop(&mut self) {
            self.shared
                .0
                .0
                .lock()
                .unwrap()
                .events
                .push(Event::CoordinatorDropped);
        }
    }

    fn packet(bytes: &[u8]) -> ActiveAutomarkerPacket {
        let mut packet = vec![0_u8; 40 + bytes.len()];
        let packet_len = packet.len() as u16;
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&packet_len.to_be_bytes());
        packet[9] = 6;
        packet[20 + 12] = 0x50;
        packet[40..].copy_from_slice(bytes);
        let mut address = [0_u8; 80];
        address[8..12].copy_from_slice(&((1_u32 << 17) | (1 << 21) | (1 << 22)).to_ne_bytes());
        ActiveAutomarkerPacket {
            bytes: packet,
            address: AutomarkerWinDivertAddress::from_opaque_bytes(address),
        }
    }

    fn sent(bytes: &[u8]) -> Event {
        let packet = packet(bytes);
        Event::Sent(packet.bytes, packet.address.into_opaque_bytes())
    }

    fn finish(harness: &Harness, coordinator: FakeCoordinator) -> ActiveAutomarkerWorkerReport {
        let bundle =
            ActiveAutomarkerWorkerBundle::spawn(FakeBackend(harness.clone()), coordinator).unwrap();
        harness.push(ActiveAutomarkerWake::EndOfStream);
        bundle.join().unwrap().report
    }

    #[test]
    fn unrelated_packets_are_reinjected_byte_for_byte_and_close_precedes_ledger_drop() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"unrelated")));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.unchanged_sent, 1);
        assert_eq!(
            &harness.events()[..3],
            &[
                sent(b"unrelated"),
                Event::BackendClosed,
                Event::LedgerDiscarded,
            ]
        );
    }

    #[test]
    fn exact_carrier_is_held_repaired_sent_and_committed() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"ack")));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.modified_sent, 1);
        assert_eq!(
            &harness.events()[..3],
            &[
                Event::Commit(7, ActiveAutomarkerSendOutcome::FailedOrIndeterminate),
                sent(b"rewrite"),
                Event::Commit(7, ActiveAutomarkerSendOutcome::Complete),
            ]
        );
    }

    #[test]
    fn checksum_failure_cancels_and_recovers_only_the_original() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().checksum_fails = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.modified_sent, 0);
        assert_eq!(report.unchanged_sent, 1);
        assert!(harness.events().contains(&sent(b"carrier")));
    }

    #[test]
    fn non_authorizing_checksum_result_reinjects_exact_held_original() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().checksum_returns_original = true;
        let mut held = packet(b"carrier");
        held.address = AutomarkerWinDivertAddress::from_opaque_bytes([0x5a; 80]);
        harness.push(ActiveAutomarkerWake::Packet(held.clone()));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.modified_sent, 0);
        assert_eq!(report.unchanged_sent, 1);
        assert!(
            harness
                .events()
                .contains(&Event::Sent(held.bytes, [0x5a; 80]))
        );
    }

    #[test]
    fn indeterminate_modified_send_is_committed_failed_without_original_reinjection() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().short_send_for = Some(packet(b"rewrite").bytes);
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_event(&Event::Commit(
            7,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate,
        ));
        assert!(harness.events().contains(&Event::Commit(
            7,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate
        )));
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"ack")));
        let completion = bundle.join().unwrap();
        assert_eq!(completion.send_failures, 2);
        assert_eq!(
            completion.exit,
            ActiveAutomarkerWorkerExit::FatalOwnershipRetained
        );
        assert!(completion.fatal_ownership.is_some());
        assert!(!harness.events().contains(&sent(b"carrier")));
    }

    #[test]
    fn filter_escape_short_or_failed_send_retains_exact_fatal_ownership() {
        for short in [true, false] {
            let harness = Harness::default();
            let escaped = packet(if short {
                b"escape-short"
            } else {
                b"escape-error"
            });
            {
                let mut state = harness.0.0.lock().unwrap();
                state.panic_on_classify = true;
                if short {
                    state.short_send_for = Some(escaped.bytes.clone());
                } else {
                    state.failed_send_for = Some(escaped.bytes.clone());
                }
            }
            harness.push(ActiveAutomarkerWake::PassThroughOnly(escaped.clone()));
            let bundle = ActiveAutomarkerWorkerBundle::spawn(
                FakeBackend(harness.clone()),
                FakeCoordinator::new(harness.clone()),
            )
            .unwrap();
            let mut completion = bundle.join().unwrap();
            assert_eq!(
                completion.exit,
                ActiveAutomarkerWorkerExit::FatalOwnershipRetained
            );
            assert_eq!(completion.send_failures, 1);
            assert!(harness.events().contains(&Event::Sent(
                escaped.bytes.clone(),
                escaped.address.into_opaque_bytes()
            )));
            assert_eq!(
                completion
                    .fatal_ownership
                    .as_ref()
                    .unwrap()
                    .indeterminate_packet
                    .as_ref(),
                Some(&escaped)
            );
            assert!(!harness.events().contains(&Event::BackendClosed));
            assert!(!harness.events().contains(&Event::LedgerDiscarded));
            assert!(
                completion
                    .fatal_ownership
                    .as_mut()
                    .unwrap()
                    .retry_close_and_discard()
                    .is_err()
            );
        }
    }

    #[test]
    fn retransmission_is_rewritten_until_reverse_ack_retires_the_ledger() {
        let harness = Harness::default();
        for bytes in [b"carrier".as_slice(), b"retrans", b"ack", b"retrans"] {
            harness.push(ActiveAutomarkerWake::Packet(packet(bytes)));
        }
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.modified_sent, 2);
        assert_eq!(report.unchanged_sent, 2);
        assert!(harness.events().contains(&Event::AckRetired));
    }

    #[test]
    fn timeout_and_proven_reset_terminate_and_stop_always_joins() {
        let timeout_harness = Harness::default();
        timeout_harness.push(ActiveAutomarkerWake::Timeout);
        let mut coordinator = FakeCoordinator::new(timeout_harness.clone());
        coordinator.terminate_timeout = true;
        let timeout = finish(&timeout_harness, coordinator);
        assert_eq!(timeout.exit, ActiveAutomarkerWorkerExit::Timeout);

        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"rst")));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(
            report.exit,
            ActiveAutomarkerWorkerExit::ConnectionTerminated
        );
        assert_eq!(report.unchanged_sent, 1);

        let harness = Harness::default();
        let mut bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        let report = bundle.stop_drain_join().unwrap();
        assert_eq!(report.exit, ActiveAutomarkerWorkerExit::Stopped);
        assert_eq!(
            &harness.events()[harness.events().len() - 4..harness.events().len() - 2],
            &[Event::BackendClosed, Event::LedgerDiscarded]
        );
    }

    #[test]
    fn stop_is_observed_by_the_bounded_receive_poll() {
        let harness = Harness::default();
        let mut bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let report = bundle.stop_drain_join().unwrap();
        assert_eq!(report.exit, ActiveAutomarkerWorkerExit::Stopped);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn stop_racing_during_carrier_commit_stays_intercepting_until_reverse_ack() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().block_carrier_commit = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_carrier_commit();
        bundle.stop_requested.store(true, Ordering::Release);
        harness.release_carrier_commit();

        assert_eq!(
            bundle.request_stop().unwrap(),
            ActiveAutomarkerStopDisposition::BlockedByRewriteObligation
        );
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));

        harness.push(ActiveAutomarkerWake::Packet(packet(b"ack")));
        let report = bundle.join().unwrap();
        assert_eq!(
            report.exit,
            ActiveAutomarkerWorkerExit::RequiresConnectionTermination
        );
        let events = harness.events();
        let ack = events
            .iter()
            .position(|event| event == &Event::AckRetired)
            .unwrap();
        let close = events
            .iter()
            .position(|event| event == &Event::BackendClosed)
            .unwrap();
        let discard = events
            .iter()
            .position(|event| event == &Event::LedgerDiscarded)
            .unwrap();
        assert!(ack < close && close < discard);
    }

    #[test]
    fn coordinator_abort_with_obligation_does_not_close_until_connection_terminates() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"abort")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_event(&Event::Commit(7, ActiveAutomarkerSendOutcome::Complete));
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));

        harness.push(ActiveAutomarkerWake::Packet(packet(b"rst")));
        let report = bundle.join().unwrap();
        assert_eq!(report.exit, ActiveAutomarkerWorkerExit::CoordinatorAbort);
        assert_eq!(
            &harness.events()[harness.events().len() - 4..harness.events().len() - 2],
            &[Event::BackendClosed, Event::LedgerDiscarded]
        );
    }

    #[test]
    fn stop_does_not_drop_a_packet_already_removed_from_the_network() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"already-held")));
        let mut bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        let report = bundle.stop_drain_join().unwrap();
        assert_eq!(report.exit, ActiveAutomarkerWorkerExit::Stopped);
        assert_eq!(report.unchanged_sent, 1);
        assert!(harness.events().contains(&sent(b"already-held")));
    }

    #[test]
    fn stop_drains_a_coordinator_proven_unsent_original_before_close_and_discard() {
        let harness = Harness::default();
        let mut coordinator = FakeCoordinator::new(harness.clone());
        coordinator.pending = Some(packet(b"held-before-worker-stop"));
        let mut bundle =
            ActiveAutomarkerWorkerBundle::spawn(FakeBackend(harness.clone()), coordinator).unwrap();
        let report = bundle.stop_drain_join().unwrap();
        assert_eq!(report.unchanged_sent, 1);
        assert_eq!(
            &harness.events()[..3],
            &[
                sent(b"held-before-worker-stop"),
                Event::BackendClosed,
                Event::LedgerDiscarded,
            ]
        );
    }

    #[test]
    fn confirmation_abort_is_preserved_as_worker_exit_after_obligation_retires() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().commit_disposition =
            Some(ActiveAutomarkerCommitDisposition::CommittedButConfirmationAborted);
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"ack")));
        let report = finish(&harness, FakeCoordinator::new(harness.clone()));
        assert_eq!(report.exit, ActiveAutomarkerWorkerExit::CoordinatorAbort);
    }

    #[test]
    fn one_fin_does_not_retire_a_rewrite_obligation() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"fin")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_event(&Event::FinObserved);
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"rst")));
        let report = bundle.join().unwrap();
        assert_eq!(
            report.exit,
            ActiveAutomarkerWorkerExit::ConnectionTerminated
        );
    }

    #[test]
    fn failed_close_never_discards_the_retransmission_ledger() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().close_fails = true;
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.push(ActiveAutomarkerWake::EndOfStream);
        let mut completion = bundle.join().unwrap();
        assert_eq!(
            completion.exit,
            ActiveAutomarkerWorkerExit::FatalOwnershipRetained
        );
        assert!(completion.fatal_ownership.is_some());
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        assert!(!harness.events().contains(&Event::BackendDropped));
        assert!(!harness.events().contains(&Event::CoordinatorDropped));
        harness.0.0.lock().unwrap().close_fails = false;
        completion
            .fatal_ownership
            .as_mut()
            .unwrap()
            .retry_close_and_discard()
            .unwrap();
        assert!(harness.events().contains(&Event::LedgerDiscarded));
        assert_eq!(
            completion
                .fatal_ownership
                .as_mut()
                .unwrap()
                .retry_close_and_discard()
                .unwrap_err(),
            "Automarker fatal ownership was already released"
        );
    }

    #[test]
    fn panic_closes_and_drops_backend_before_coordinator() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().panic_on_classify = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"panic")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        let mut completion = bundle.join().unwrap();
        assert_eq!(
            completion.exit,
            ActiveAutomarkerWorkerExit::FatalOwnershipRetained
        );
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        completion
            .fatal_ownership
            .as_mut()
            .unwrap()
            .retry_close_and_discard()
            .unwrap();
        let events = harness.events();
        let close = events
            .iter()
            .position(|event| event == &Event::BackendClosed)
            .unwrap();
        let backend_drop = events
            .iter()
            .position(|event| event == &Event::BackendDropped)
            .unwrap();
        let coordinator_drop = events
            .iter()
            .position(|event| event == &Event::CoordinatorDropped)
            .unwrap();
        let discard = events
            .iter()
            .position(|event| event == &Event::LedgerDiscarded)
            .unwrap();
        assert!(close < discard && discard < backend_drop && backend_drop < coordinator_drop);
    }

    #[test]
    fn stop_during_backend_send_cannot_close_or_discard_held_ownership() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().block_send = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_send();
        bundle.stop_requested.store(true, Ordering::Release);
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        harness.release_send();
        harness.wait_for_event(&Event::Commit(7, ActiveAutomarkerSendOutcome::Complete));
        harness.push(ActiveAutomarkerWake::Packet(packet(b"ack")));
        let report = bundle.join().unwrap();
        assert_eq!(
            report.exit,
            ActiveAutomarkerWorkerExit::RequiresConnectionTermination
        );
    }

    #[test]
    fn stop_arriving_during_receive_quiesces_before_classifying_new_carrier() {
        let harness = Harness::default();
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        bundle.stop_requested.store(true, Ordering::Release);
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let completion = bundle.join().unwrap();
        assert_eq!(completion.exit, ActiveAutomarkerWorkerExit::Stopped);
        assert_eq!(completion.modified_sent, 0);
        assert_eq!(completion.unchanged_sent, 1);
        assert!(!harness.events().contains(&Event::Commit(
            7,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate
        )));
    }

    #[test]
    fn send_panic_retains_pre_recorded_indeterminate_ownership() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().panic_on_send = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        let mut completion = bundle.join().unwrap();
        assert_eq!(
            completion.exit,
            ActiveAutomarkerWorkerExit::FatalOwnershipRetained
        );
        assert!(completion.fatal_ownership.is_some());
        assert!(harness.events().contains(&Event::Commit(
            7,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate
        )));
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
        assert!(
            completion
                .fatal_ownership
                .as_mut()
                .unwrap()
                .retry_close_and_discard()
                .is_err()
        );
    }

    #[test]
    fn commit_panic_retains_pre_recorded_indeterminate_ownership() {
        let harness = Harness::default();
        harness.0.0.lock().unwrap().panic_on_commit = true;
        harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
        let bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        let completion = bundle.join().unwrap();
        assert_eq!(
            completion.exit,
            ActiveAutomarkerWorkerExit::FatalOwnershipRetained
        );
        assert!(completion.fatal_ownership.is_some());
        assert!(harness.events().contains(&Event::Commit(
            7,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate
        )));
        assert!(!harness.events().contains(&Event::BackendClosed));
        assert!(!harness.events().contains(&Event::LedgerDiscarded));
    }

    #[test]
    fn eos_or_receive_failure_with_obligation_returns_fatal_ownership() {
        for eos in [true, false] {
            let harness = Harness::default();
            harness.push(ActiveAutomarkerWake::Packet(packet(b"carrier")));
            if eos {
                harness.push(ActiveAutomarkerWake::EndOfStream);
            } else {
                harness.push_error("injected persistent receive failure");
            }
            let bundle = ActiveAutomarkerWorkerBundle::spawn(
                FakeBackend(harness.clone()),
                FakeCoordinator::new(harness.clone()),
            )
            .unwrap();
            let completion = bundle.join().unwrap();
            assert_eq!(
                completion.exit,
                ActiveAutomarkerWorkerExit::FatalOwnershipRetained
            );
            assert!(completion.fatal_ownership.is_some());
            assert!(!harness.events().contains(&Event::LedgerDiscarded));
        }
    }

    #[test]
    fn stop_request_joins_an_already_finished_worker_without_false_timeout() {
        let harness = Harness::default();
        harness.push(ActiveAutomarkerWake::EndOfStream);
        let mut bundle = ActiveAutomarkerWorkerBundle::spawn(
            FakeBackend(harness.clone()),
            FakeCoordinator::new(harness.clone()),
        )
        .unwrap();
        harness.wait_for_event(&Event::BackendClosed);
        let completion = bundle.stop_drain_join().unwrap();
        assert_eq!(completion.exit, ActiveAutomarkerWorkerExit::EndOfStream);
    }

    #[test]
    fn report_counters_saturate() {
        let harness = Harness::default();
        let mut backend = FakeBackend(harness);
        let mut report = ActiveAutomarkerWorkerReport {
            exit: ActiveAutomarkerWorkerExit::Stopped,
            packets_received: u64::MAX,
            unchanged_sent: u64::MAX,
            modified_sent: u64::MAX,
            send_failures: u64::MAX,
        };
        assert!(send_exact(
            &mut backend,
            &packet(b"ordinary"),
            false,
            &mut report
        ));
        assert_eq!(report.unchanged_sent, u64::MAX);
        assert_eq!(report.send_failures, u64::MAX);
    }

    #[test]
    fn modified_authorization_rejects_forged_bytes_and_address_identity() {
        let original = packet(b"carrier");
        let approved = packet(b"rewrite").bytes;
        assert!(modified_preparation_matches(
            &original,
            &approved,
            &approved,
            original.address
        ));

        let mut forged_packet = approved.clone();
        forged_packet[40] ^= 1;
        assert!(!modified_preparation_matches(
            &original,
            &approved,
            &forged_packet,
            original.address
        ));

        let mut forged_address = original.address.into_opaque_bytes();
        forged_address[16] ^= 1;
        assert!(!modified_preparation_matches(
            &original,
            &approved,
            &approved,
            AutomarkerWinDivertAddress::from_opaque_bytes(forged_address)
        ));
    }
}
