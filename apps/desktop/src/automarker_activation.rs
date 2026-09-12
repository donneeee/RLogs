//! Pure automarker preset activation orchestration.
//!
//! This module has no transport or native-action implementation. It only
//! issues abstract dispatch intents and advances after authoritative inbound
//! marker observations prove the requested slot and coordinates appeared.

use crate::automarker_presets::{AutomarkerPoint, AutomarkerSceneContext};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationContext {
    pub session_id: String,
    pub scene: AutomarkerSceneContext,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivationPolicy {
    pub coordinate_tolerance: f32,
    pub timeout_micros: u64,
    pub max_retries_per_marker: u8,
}

impl Default for ActivationPolicy {
    fn default() -> Self {
        Self {
            coordinate_tolerance: 0.25,
            timeout_micros: 3_000_000,
            max_retries_per_marker: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchIntent {
    pub dispatch_id: u64,
    pub marker_number: u8,
    pub slot_id: i32,
    pub skill_id: i32,
    pub target: AutomarkerPoint,
    pub attempt: u8,
    pub requested_micros: u64,
    pub context: ActivationContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchResultReceipt {
    pub dispatch_id: u64,
    pub marker_number: u8,
    pub accepted: bool,
    pub dispatched_micros: u64,
    pub context: ActivationContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InboundMarkerObservation {
    pub marker_number: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub observed_micros: u64,
    pub context: ActivationContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationStatus {
    Ready,
    DispatchInFlight,
    AwaitingInbound,
    Complete,
    Cancelled,
    Failed(&'static str),
}

#[derive(Debug, Clone)]
pub struct AutomarkerActivation {
    context: ActivationContext,
    plan: Vec<AutomarkerPoint>,
    policy: ActivationPolicy,
    index: usize,
    retries: u8,
    status: ActivationStatus,
    deadline_micros: Option<u64>,
    dispatched_micros: Option<u64>,
    requested_micros: Option<u64>,
    next_dispatch_id: u64,
    in_flight_dispatch_id: Option<u64>,
}

impl AutomarkerActivation {
    pub fn start(
        mut plan: Vec<AutomarkerPoint>,
        context: ActivationContext,
        policy: ActivationPolicy,
    ) -> Result<Self, &'static str> {
        if context.session_id.trim().is_empty()
            || context.scene.client_build.trim().is_empty()
            || context.scene.activity_family_id.trim().is_empty()
            || !policy.coordinate_tolerance.is_finite()
            || policy.coordinate_tolerance < 0.0
            || policy.timeout_micros == 0
        {
            return Err("invalid_activation_context_or_policy");
        }
        let mut numbers = BTreeSet::new();
        for point in &plan {
            if !(1..=6).contains(&point.marker_number)
                || !numbers.insert(point.marker_number)
                || ![point.x, point.y, point.z]
                    .iter()
                    .all(|value| value.is_finite())
            {
                return Err("invalid_marker_plan");
            }
        }
        if plan.is_empty() {
            return Err("empty_marker_plan");
        }
        plan.sort_by_key(|point| point.marker_number);
        Ok(Self {
            context,
            plan,
            policy,
            index: 0,
            retries: 0,
            status: ActivationStatus::Ready,
            deadline_micros: None,
            dispatched_micros: None,
            requested_micros: None,
            next_dispatch_id: 1,
            in_flight_dispatch_id: None,
        })
    }

    pub fn status(&self) -> &ActivationStatus {
        &self.status
    }

    pub fn request_dispatch(
        &mut self,
        context: &ActivationContext,
        now_micros: u64,
    ) -> Result<DispatchIntent, &'static str> {
        self.require_context(context)?;
        if self.status != ActivationStatus::Ready {
            return Err("dispatch_not_ready");
        }
        let target = self.plan[self.index].clone();
        let dispatch_id = self.next_dispatch_id;
        self.next_dispatch_id = self.next_dispatch_id.saturating_add(1);
        self.status = ActivationStatus::DispatchInFlight;
        self.deadline_micros = Some(now_micros.saturating_add(self.policy.timeout_micros));
        self.requested_micros = Some(now_micros);
        self.in_flight_dispatch_id = Some(dispatch_id);
        Ok(DispatchIntent {
            dispatch_id,
            marker_number: target.marker_number,
            slot_id: 200 + i32::from(target.marker_number),
            skill_id: 1100 + i32::from(target.marker_number),
            target,
            attempt: self.retries.saturating_add(1),
            requested_micros: now_micros,
            context: self.context.clone(),
        })
    }

    pub fn observe_dispatch_result(
        &mut self,
        receipt: DispatchResultReceipt,
    ) -> Result<(), &'static str> {
        self.require_context(&receipt.context)?;
        if self.status != ActivationStatus::DispatchInFlight {
            return Err("unexpected_dispatch_receipt");
        }
        if Some(receipt.dispatch_id) != self.in_flight_dispatch_id
            || receipt.marker_number != self.plan[self.index].marker_number
            || self
                .requested_micros
                .is_none_or(|requested| receipt.dispatched_micros < requested)
        {
            return self.fail("dispatch_marker_mismatch");
        }
        if !receipt.accepted {
            return self.retry_or_fail("dispatch_rejected");
        }
        self.dispatched_micros = Some(receipt.dispatched_micros);
        self.deadline_micros = Some(
            receipt
                .dispatched_micros
                .saturating_add(self.policy.timeout_micros),
        );
        self.status = ActivationStatus::AwaitingInbound;
        Ok(())
    }

    /// Consumes only positive inbound placement observations. Passive-end
    /// events are intentionally outside this API, so stale ends cannot advance
    /// or cancel a newer replacement attempt.
    pub fn observe_inbound(
        &mut self,
        observation: InboundMarkerObservation,
    ) -> Result<bool, &'static str> {
        self.require_context(&observation.context)?;
        if self.status != ActivationStatus::AwaitingInbound {
            return Ok(false);
        }
        let expected = &self.plan[self.index];
        let Some(dispatched_micros) = self.dispatched_micros else {
            return self.fail("missing_dispatch_clock");
        };
        if observation.marker_number != expected.marker_number
            || observation.observed_micros <= dispatched_micros
            || !coordinates_match(expected, &observation, self.policy.coordinate_tolerance)
        {
            return Ok(false);
        }
        self.index += 1;
        self.retries = 0;
        self.deadline_micros = None;
        self.dispatched_micros = None;
        self.requested_micros = None;
        self.in_flight_dispatch_id = None;
        self.status = if self.index == self.plan.len() {
            ActivationStatus::Complete
        } else {
            ActivationStatus::Ready
        };
        Ok(true)
    }

    pub fn tick(
        &mut self,
        context: &ActivationContext,
        now_micros: u64,
    ) -> Result<(), &'static str> {
        self.require_context(context)?;
        if matches!(
            self.status,
            ActivationStatus::DispatchInFlight | ActivationStatus::AwaitingInbound
        ) && self
            .deadline_micros
            .is_some_and(|deadline| now_micros >= deadline)
        {
            self.retry_or_fail("activation_timeout")?;
        }
        Ok(())
    }

    pub fn cancel(&mut self) {
        if !matches!(
            self.status,
            ActivationStatus::Complete | ActivationStatus::Failed(_)
        ) {
            self.status = ActivationStatus::Cancelled;
            self.deadline_micros = None;
            self.dispatched_micros = None;
            self.requested_micros = None;
            self.in_flight_dispatch_id = None;
        }
    }

    fn require_context(&mut self, context: &ActivationContext) -> Result<(), &'static str> {
        if context != &self.context {
            return self.fail("activation_context_changed");
        }
        Ok(())
    }

    fn retry_or_fail(&mut self, terminal_reason: &'static str) -> Result<(), &'static str> {
        self.deadline_micros = None;
        self.dispatched_micros = None;
        self.requested_micros = None;
        self.in_flight_dispatch_id = None;
        if self.retries < self.policy.max_retries_per_marker {
            self.retries += 1;
            self.status = ActivationStatus::Ready;
            Ok(())
        } else {
            self.fail(terminal_reason)
        }
    }

    fn fail<T>(&mut self, reason: &'static str) -> Result<T, &'static str> {
        self.status = ActivationStatus::Failed(reason);
        self.deadline_micros = None;
        self.dispatched_micros = None;
        self.requested_micros = None;
        self.in_flight_dispatch_id = None;
        Err(reason)
    }
}

fn coordinates_match(
    expected: &AutomarkerPoint,
    observed: &InboundMarkerObservation,
    tolerance: f32,
) -> bool {
    [observed.x, observed.y, observed.z]
        .iter()
        .all(|value| value.is_finite())
        && ((expected.x - observed.x).powi(2)
            + (expected.y - observed.y).powi(2)
            + (expected.z - observed.z).powi(2))
        .sqrt()
            <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ActivationContext {
        ActivationContext {
            session_id: "capture-session".into(),
            scene: AutomarkerSceneContext {
                client_build: "25247556".into(),
                scene_id: 1633,
                map_id: 1633,
                activity_family_id: "dungeon.1633".into(),
                scene_name: Some("Tina M20".into()),
            },
        }
    }

    fn point(marker_number: u8, x: f32) -> AutomarkerPoint {
        AutomarkerPoint {
            marker_number,
            x,
            y: 110.0,
            z: 5.0,
        }
    }

    fn accepted(intent: &DispatchIntent, dispatched_micros: u64) -> DispatchResultReceipt {
        DispatchResultReceipt {
            dispatch_id: intent.dispatch_id,
            marker_number: intent.marker_number,
            accepted: true,
            dispatched_micros,
            context: intent.context.clone(),
        }
    }

    fn observed(intent: &DispatchIntent, observed_micros: u64) -> InboundMarkerObservation {
        InboundMarkerObservation {
            marker_number: intent.marker_number,
            x: intent.target.x,
            y: intent.target.y,
            z: intent.target.z,
            observed_micros,
            context: intent.context.clone(),
        }
    }

    #[test]
    fn advances_in_slot_order_only_after_post_dispatch_coordinate_ack() {
        let ctx = context();
        let mut run = AutomarkerActivation::start(
            vec![point(2, 20.0), point(1, 10.0)],
            ctx.clone(),
            ActivationPolicy::default(),
        )
        .unwrap();
        let first = run.request_dispatch(&ctx, 100).unwrap();
        assert_eq!(
            (first.marker_number, first.slot_id, first.skill_id),
            (1, 201, 1101)
        );
        run.observe_dispatch_result(accepted(&first, 110)).unwrap();
        assert!(!run.observe_inbound(observed(&first, 110)).unwrap());
        let mut wrong = observed(&first, 120);
        wrong.x += 1.0;
        assert!(!run.observe_inbound(wrong).unwrap());
        assert!(run.observe_inbound(observed(&first, 121)).unwrap());
        let second = run.request_dispatch(&ctx, 130).unwrap();
        assert_eq!(second.marker_number, 2);
        run.observe_dispatch_result(accepted(&second, 140)).unwrap();
        assert!(run.observe_inbound(observed(&second, 141)).unwrap());
        assert_eq!(run.status(), &ActivationStatus::Complete);
    }

    #[test]
    fn one_in_flight_and_stale_or_unrelated_observations_do_not_advance() {
        let ctx = context();
        let mut run = AutomarkerActivation::start(
            vec![point(1, 1.0)],
            ctx.clone(),
            ActivationPolicy::default(),
        )
        .unwrap();
        let intent = run.request_dispatch(&ctx, 10).unwrap();
        assert_eq!(run.request_dispatch(&ctx, 11), Err("dispatch_not_ready"));
        assert!(!run.observe_inbound(observed(&intent, 12)).unwrap());
        run.observe_dispatch_result(accepted(&intent, 20)).unwrap();
        let mut wrong_slot = observed(&intent, 21);
        wrong_slot.marker_number = 2;
        assert!(!run.observe_inbound(wrong_slot).unwrap());
        assert_eq!(run.status(), &ActivationStatus::AwaitingInbound);
    }

    #[test]
    fn context_change_fails_closed_across_every_continuity_field() {
        let mutations: &[fn(&mut ActivationContext)] = &[
            |c| c.session_id = "other".into(),
            |c| c.scene.client_build = "other".into(),
            |c| c.scene.scene_id += 1,
            |c| c.scene.map_id += 1,
            |c| c.scene.activity_family_id = "other".into(),
        ];
        for mutate in mutations {
            let ctx = context();
            let mut run = AutomarkerActivation::start(
                vec![point(1, 1.0)],
                ctx.clone(),
                ActivationPolicy::default(),
            )
            .unwrap();
            let mut changed = ctx.clone();
            mutate(&mut changed);
            assert_eq!(
                run.request_dispatch(&changed, 1),
                Err("activation_context_changed")
            );
            assert_eq!(
                run.status(),
                &ActivationStatus::Failed("activation_context_changed")
            );
        }
    }

    #[test]
    fn stale_or_pre_request_dispatch_receipt_fails_closed() {
        for mutation in ["dispatch_id", "clock"] {
            let ctx = context();
            let mut run = AutomarkerActivation::start(
                vec![point(1, 1.0)],
                ctx.clone(),
                ActivationPolicy::default(),
            )
            .unwrap();
            let intent = run.request_dispatch(&ctx, 100).unwrap();
            let mut receipt = accepted(&intent, 101);
            if mutation == "dispatch_id" {
                receipt.dispatch_id += 1;
            } else {
                receipt.dispatched_micros = 99;
            }
            assert_eq!(
                run.observe_dispatch_result(receipt),
                Err("dispatch_marker_mismatch")
            );
            assert_eq!(
                run.status(),
                &ActivationStatus::Failed("dispatch_marker_mismatch")
            );
        }
    }

    #[test]
    fn timeout_retries_deterministically_then_fails() {
        let ctx = context();
        let policy = ActivationPolicy {
            timeout_micros: 10,
            max_retries_per_marker: 1,
            ..ActivationPolicy::default()
        };
        let mut run =
            AutomarkerActivation::start(vec![point(1, 1.0)], ctx.clone(), policy).unwrap();
        let first = run.request_dispatch(&ctx, 100).unwrap();
        run.observe_dispatch_result(accepted(&first, 101)).unwrap();
        run.tick(&ctx, 111).unwrap();
        let retry = run.request_dispatch(&ctx, 112).unwrap();
        assert_eq!(retry.attempt, 2);
        assert_eq!(run.tick(&ctx, 122), Err("activation_timeout"));
        assert_eq!(
            run.status(),
            &ActivationStatus::Failed("activation_timeout")
        );
    }

    #[test]
    fn cancel_and_invalid_plans_never_issue_work() {
        let ctx = context();
        let mut run = AutomarkerActivation::start(
            vec![point(1, 1.0)],
            ctx.clone(),
            ActivationPolicy::default(),
        )
        .unwrap();
        run.cancel();
        assert_eq!(run.request_dispatch(&ctx, 1), Err("dispatch_not_ready"));
        assert!(
            AutomarkerActivation::start(
                vec![point(1, 1.0), point(1, 2.0)],
                ctx.clone(),
                ActivationPolicy::default()
            )
            .is_err()
        );
        assert!(AutomarkerActivation::start(vec![], ctx, ActivationPolicy::default()).is_err());
    }

    #[test]
    fn tina_m20_saved_six_slot_plan_completes_deterministically() {
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/automarker/tina-m20-six-marker-points.v1.json"
        ))
        .unwrap();
        let plan = receipt["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| AutomarkerPoint {
                marker_number: row["markerNumber"].as_u64().unwrap() as u8,
                x: row["x"].as_f64().unwrap() as f32,
                y: row["y"].as_f64().unwrap() as f32,
                z: row["z"].as_f64().unwrap() as f32,
            })
            .rev()
            .collect();
        let ctx = context();
        let mut run =
            AutomarkerActivation::start(plan, ctx.clone(), ActivationPolicy::default()).unwrap();
        for marker_number in 1..=6 {
            let base = u64::from(marker_number) * 100;
            let intent = run.request_dispatch(&ctx, base).unwrap();
            assert_eq!(intent.marker_number, marker_number);
            run.observe_dispatch_result(accepted(&intent, base + 1))
                .unwrap();
            assert!(run.observe_inbound(observed(&intent, base + 2)).unwrap());
        }
        assert_eq!(run.status(), &ActivationStatus::Complete);
    }
}
