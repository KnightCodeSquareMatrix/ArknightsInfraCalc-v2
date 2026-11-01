use serde::Serialize;
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{BaseBlueprint, RoomId};
use crate::mood::{shift_eta_with_instances, MoodModel, ShiftEta};

use super::rotation_types::{FiammettaShiftAction, TeamShiftResult};

pub(super) const FIAMMETTA_NAME: &str = "菲亚梅塔";

const READY_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiammettaExchange {
    pub target: String,
    /// Offset within the repeated service cycle, not a base-wide shift time.
    pub at_hours: f64,
    pub target_mood_before: f64,
    pub target_mood_after: f64,
    pub fiammetta_mood_before: f64,
    pub fiammetta_mood_after: f64,
    pub ready_after_hours: f64,
}

/// Conservative service cycle for independently switched rooms. The actual
/// exchange and recharge use the same Timeline as timed rotation.
pub(super) fn continuous_exchanges(
    model: &MoodModel,
    drains: &[(String, f64)],
    period_hours: f64,
) -> Result<Vec<FiammettaExchange>> {
    let rate = recovery_rate(model);
    if !period_hours.is_finite()
        || period_hours <= 0.0
        || rate <= 0.0
        || drains.iter().any(|(_, drain)| {
            !drain.is_finite()
                || *drain < 0.0
                || drain * period_hours > model.mood_cap + READY_EPSILON
        })
        || drains.iter().map(|(_, drain)| drain).sum::<f64>() > rate + READY_EPSILON
    {
        return Err(Error::FiammettaCandidateUnavailable(
            "continuous recovery budget exceeded".into(),
        ));
    }
    let targets = drains
        .iter()
        .map(|(target, _)| target.clone())
        .collect::<Vec<_>>();
    let mut timeline = Timeline::for_targets(model, &targets);
    let mut offsets = Vec::new();
    let mut offset = 0.0;
    for (target, drain) in drains {
        offsets.push(offset);
        timeline.target_moods.insert(
            target.clone(),
            model.mood_cap - drain * (period_hours - offset),
        );
        offset += drain * period_hours / rate;
    }
    let mut exchanges = Vec::new();
    let mut now = 0.0;
    // Replay two cycles: a one-off full Fiammetta cannot justify permanence.
    for cycle in 0..2 {
        for ((target, drain), offset) in drains.iter().zip(&offsets) {
            let at = cycle as f64 * period_hours + offset;
            timeline.advance_drains(
                at - now,
                None,
                drains
                    .iter()
                    .map(|(target, drain)| (target.as_str(), *drain)),
            );
            let (before, after) = timeline.exchange(target)?;
            if cycle == 0 {
                exchanges.push(FiammettaExchange {
                    target: target.clone(),
                    at_hours: *offset,
                    target_mood_before: before,
                    target_mood_after: after,
                    fiammetta_mood_before: after,
                    fiammetta_mood_after: before,
                    ready_after_hours: drain * period_hours / rate,
                });
            } else if (before
                - exchanges[exchanges
                    .iter()
                    .position(|event| event.target == *target)
                    .unwrap()]
                .target_mood_before)
                .abs()
                > 1e-8
            {
                return Err(Error::FiammettaCandidateUnavailable(
                    "continuous exchange cycle did not close".into(),
                ));
            }
            now = at;
        }
    }
    Ok(exchanges)
}

/// Verify the reported events by replaying their times, independently of the
/// service-cycle generator and its capacity/offset formula. Only the exchange
/// and recovery mechanism is shared with generation and timed rotation.
pub(super) fn validate_continuous_exchanges(
    model: &MoodModel,
    drains: &[(String, f64)],
    period_hours: f64,
    exchanges: &[FiammettaExchange],
) -> Result<()> {
    let invalid = || Error::msg("assert-invariants auto_fiammetta_exchange_budget failed");
    let cap = model.mood_cap;
    let rate = recovery_rate(model);
    if !period_hours.is_finite()
        || period_hours <= 0.0
        || !cap.is_finite()
        || cap <= 0.0
        || !rate.is_finite()
        || rate <= 0.0
        || drains.len() != exchanges.len()
    {
        return Err(invalid());
    }
    let targets = drains
        .iter()
        .map(|(target, _)| target.clone())
        .collect::<Vec<_>>();
    let mut timeline = Timeline::for_targets(model, &targets);
    let mut previous = 0.0;
    for ((target, drain), event) in drains.iter().zip(exchanges) {
        if !drain.is_finite()
            || *drain < 0.0
            || event.target != *target
            || !event.at_hours.is_finite()
            || event.at_hours < previous
            || event.at_hours >= period_hours
            || [
                event.target_mood_before,
                event.target_mood_after,
                event.fiammetta_mood_before,
                event.fiammetta_mood_after,
            ]
            .iter()
            .any(|mood| !mood.is_finite() || *mood < 0.0 || *mood > cap)
            || !event.ready_after_hours.is_finite()
            || event.ready_after_hours < 0.0
        {
            return Err(invalid());
        }
        // Reconstruct time zero from each declared first exchange. The next
        // cycle must reproduce that mood from the actual previous exchange.
        let initial = event.target_mood_before + drain * event.at_hours;
        if initial > cap + READY_EPSILON {
            return Err(invalid());
        }
        timeline.target_moods.insert(target.clone(), initial);
        previous = event.at_hours;
    }
    let mut now = 0.0;
    for cycle in 0..2 {
        for event in exchanges {
            let at = cycle as f64 * period_hours + event.at_hours;
            let elapsed = at - now;
            // Check before clamping: zero in Timeline must not hide an
            // exhausted target between two exchanges.
            if drains.iter().any(|(target, drain)| {
                timeline.target_moods[target] - drain * elapsed < -READY_EPSILON
            }) {
                return Err(invalid());
            }
            timeline.advance_drains(
                elapsed,
                None,
                drains
                    .iter()
                    .map(|(target, drain)| (target.as_str(), *drain)),
            );
            let (before, after) = timeline.exchange(&event.target).map_err(|_| invalid())?;
            if [
                (before, event.target_mood_before),
                (after, event.target_mood_after),
                (after, event.fiammetta_mood_before),
                (before, event.fiammetta_mood_after),
                ((cap - before) / rate, event.ready_after_hours),
            ]
            .iter()
            .any(|(actual, reported)| (actual - reported).abs() > 1e-8)
            {
                return Err(invalid());
            }
            now = at;
        }
    }
    Ok(())
}

pub(super) fn recovery_rate(model: &MoodModel) -> f64 {
    model
        .dorm_skills(FIAMMETTA_NAME, 2)
        .into_iter()
        .find(|skill| skill.no_external)
        .map(|skill| skill.value)
        .unwrap_or_else(|| model.dorm_base_recovery(5))
}

pub(super) struct Timeline {
    cap: f64,
    recovery_rate: f64,
    fiammetta_mood: f64,
    target_moods: HashMap<String, f64>,
}

impl Timeline {
    pub(super) fn for_targets(model: &MoodModel, targets: &[String]) -> Self {
        Self {
            cap: model.mood_cap,
            recovery_rate: recovery_rate(model).max(0.0),
            fiammetta_mood: model.mood_cap,
            target_moods: targets
                .iter()
                .map(|target| (target.clone(), model.mood_cap))
                .collect(),
        }
    }

    fn new(model: &MoodModel, shifts: &[TeamShiftResult], extra_target: Option<&str>) -> Self {
        let mut target_moods = HashMap::new();
        for shift in shifts {
            if let Some(action) = &shift.fiammetta {
                target_moods
                    .entry(action.target.clone())
                    .or_insert(model.mood_cap);
            }
        }
        if let Some(target) = extra_target {
            target_moods
                .entry(target.to_string())
                .or_insert(model.mood_cap);
        }
        Self {
            cap: model.mood_cap,
            recovery_rate: recovery_rate(model).max(0.0),
            fiammetta_mood: model.mood_cap,
            target_moods,
        }
    }

    fn swap(&mut self, target: &str, eta: &ShiftEta, shift_index: usize) -> Result<()> {
        let target_present = eta.per_op.iter().any(|operator| operator.name == target);
        if !target_present {
            return Err(Error::msg(format!(
                "fiammetta_ready failed in shift {}: target {} is not present in shift_eta",
                shift_index + 1,
                target
            )));
        }
        self.exchange(target).map(|_| ()).map_err(|error| {
            Error::msg(format!(
                "fiammetta_ready failed in shift {}: {error}",
                shift_index + 1
            ))
        })
    }

    /// Shared exchange mechanism; callers own presence and event scheduling.
    pub(super) fn exchange(&mut self, target: &str) -> Result<(f64, f64)> {
        if self.fiammetta_mood + READY_EPSILON < self.cap {
            return Err(Error::msg(format!(
                "Fiammetta mood {:.3} is below cap {:.3}",
                self.fiammetta_mood, self.cap
            )));
        }
        let target_mood = self.target_moods.get(target).copied().unwrap_or(self.cap);
        let fiammetta_mood = self.fiammetta_mood;
        self.fiammetta_mood = target_mood;
        self.target_moods.insert(target.to_string(), self.cap);
        Ok((target_mood, fiammetta_mood))
    }

    fn advance(&mut self, shift: &TeamShiftResult, eta: &ShiftEta) {
        let fiammetta_drain = eta
            .per_op
            .iter()
            .find(|operator| operator.name == FIAMMETTA_NAME)
            .map(|operator| operator.drain_per_hour);
        self.advance_drains(
            shift.duration_hours,
            fiammetta_drain,
            eta.per_op
                .iter()
                .map(|operator| (operator.name.as_str(), operator.drain_per_hour)),
        );
    }

    pub(super) fn advance_drains<'a>(
        &mut self,
        hours: f64,
        fiammetta_drain: Option<f64>,
        drains: impl IntoIterator<Item = (&'a str, f64)>,
    ) {
        if let Some(drain) = fiammetta_drain {
            self.fiammetta_mood =
                (self.fiammetta_mood - drain.max(0.0) * hours).clamp(0.0, self.cap);
        } else {
            self.fiammetta_mood = (self.fiammetta_mood + self.recovery_rate * hours).min(self.cap);
        }
        for (target, drain) in drains {
            if let Some(mood) = self.target_moods.get_mut(target) {
                *mood = (*mood - drain.max(0.0) * hours).clamp(0.0, self.cap);
            }
        }
    }
}

pub(super) fn validate_actions(
    shifts: &[TeamShiftResult],
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> Result<()> {
    if shifts.iter().all(|shift| shift.fiammetta.is_none()) {
        return Ok(());
    }
    let mut timeline = Timeline::new(model, shifts, None);
    for (index, shift) in shifts.iter().enumerate() {
        let eta = shift_eta_with_instances(model, blueprint, &shift.assignment, instances);
        if let Some(action) = &shift.fiammetta {
            timeline.swap(&action.target, &eta, index)?;
        }
        timeline.advance(shift, &eta);
    }
    Ok(())
}

pub(super) fn retain_ready_actions(
    shifts: &mut [TeamShiftResult],
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> Vec<String> {
    let mut timeline = Timeline::new(model, shifts, None);
    let mut warnings = Vec::new();
    for (index, shift) in shifts.iter_mut().enumerate() {
        let eta = shift_eta_with_instances(model, blueprint, &shift.assignment, instances);
        if let Some(action) = &shift.fiammetta {
            if let Err(error) = timeline.swap(&action.target, &eta, index) {
                warnings.push(format!("{error}; action omitted"));
                shift.fiammetta = None;
            }
        }
        timeline.advance(shift, &eta);
    }
    warnings
}

pub(super) fn ready_events(
    shifts: &[TeamShiftResult],
    target: &str,
    candidate_indices: &[usize],
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> Vec<usize> {
    let mut timeline = Timeline::new(model, shifts, Some(target));
    let mut events = Vec::new();
    for (index, shift) in shifts.iter().enumerate() {
        let eta = shift_eta_with_instances(model, blueprint, &shift.assignment, instances);
        if let Some(action) = &shift.fiammetta {
            if timeline.swap(&action.target, &eta, index).is_err() {
                return Vec::new();
            }
        } else if candidate_indices.contains(&index)
            && timeline.fiammetta_mood + READY_EPSILON >= timeline.cap
            && eta.per_op.iter().any(|operator| operator.name == target)
            && timeline.swap(target, &eta, index).is_ok()
        {
            events.push(index);
        }
        timeline.advance(shift, &eta);
    }
    events
}

fn try_accept(
    shifts: &mut [TeamShiftResult],
    index: usize,
    action: FiammettaShiftAction,
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> bool {
    if index >= shifts.len() || shifts[index].fiammetta.is_some() {
        return false;
    }
    let mut trial = shifts.to_vec();
    trial[index].fiammetta = Some(action.clone());
    if validate_actions(&trial, model, blueprint, instances).is_err() {
        return false;
    }
    shifts[index].fiammetta = Some(action);
    true
}

pub(super) fn try_accept_action(
    shifts: &mut [TeamShiftResult],
    index: usize,
    target: &str,
    room_id: &RoomId,
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> bool {
    try_accept(
        shifts,
        index,
        FiammettaShiftAction {
            target: target.to_string(),
            displaced: String::new(),
            room_id: room_id.clone(),
        },
        model,
        blueprint,
        instances,
    )
}
