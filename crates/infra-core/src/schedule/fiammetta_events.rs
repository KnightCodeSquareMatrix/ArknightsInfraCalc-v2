use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{BaseBlueprint, RoomId};
use crate::mood::{shift_eta_with_instances, MoodModel, ShiftEta};

use super::rotation_types::{FiammettaShiftAction, TeamShiftResult};

pub(super) const FIAMMETTA_NAME: &str = "菲亚梅塔";

const READY_EPSILON: f64 = 1e-9;

pub(super) fn recovery_rate(model: &MoodModel) -> f64 {
    model
        .dorm_skills(FIAMMETTA_NAME, 2)
        .into_iter()
        .find(|skill| skill.no_external)
        .map(|skill| skill.value)
        .unwrap_or_else(|| model.dorm_base_recovery(5))
}

struct Timeline {
    cap: f64,
    recovery_rate: f64,
    fiammetta_mood: f64,
    target_moods: HashMap<String, f64>,
}

impl Timeline {
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
        if self.fiammetta_mood + READY_EPSILON < self.cap {
            return Err(Error::msg(format!(
                "fiammetta_ready failed in shift {}: Fiammetta mood {:.3} is below cap {:.3}",
                shift_index + 1,
                self.fiammetta_mood,
                self.cap
            )));
        }
        let target_mood = self.target_moods.get(target).copied().unwrap_or(self.cap);
        self.fiammetta_mood = target_mood;
        self.target_moods.insert(target.to_string(), self.cap);
        Ok(())
    }

    fn advance(&mut self, shift: &TeamShiftResult, eta: &ShiftEta) {
        if let Some(operator) = eta
            .per_op
            .iter()
            .find(|operator| operator.name == FIAMMETTA_NAME)
        {
            self.fiammetta_mood = (self.fiammetta_mood
                - operator.drain_per_hour.max(0.0) * shift.duration_hours)
                .clamp(0.0, self.cap);
        } else {
            self.fiammetta_mood =
                (self.fiammetta_mood + self.recovery_rate * shift.duration_hours).min(self.cap);
        }
        for (target, mood) in &mut self.target_moods {
            let Some(operator) = eta.per_op.iter().find(|operator| operator.name == *target) else {
                continue;
            };
            *mood = (*mood - operator.drain_per_hour.max(0.0) * shift.duration_hours)
                .clamp(0.0, self.cap);
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

pub(super) fn accept_proposals(
    shifts: &mut [TeamShiftResult],
    proposals: &[Option<FiammettaShiftAction>],
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) {
    for (index, proposal) in proposals.iter().enumerate() {
        let Some(action) = proposal else {
            continue;
        };
        let _ = try_accept(shifts, index, action.clone(), model, blueprint, instances);
    }
}
