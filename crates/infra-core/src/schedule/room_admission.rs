//! Shared candidate admission helpers for ordinary scheduled rooms.
//!
//! Trial assignments and ETA/mood checks are shared with complete-assignment
//! ordinary vacancy filling. Profile owners supply the already-trimmed candidates.

use std::collections::HashMap;

use crate::instances::OperatorInstances;
use crate::layout::{AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, RoomId};
use crate::mood::{
    default_mood_model, fits_shift_budget, shift_eta_in_context, shift_eta_with_instances,
    MoodModel, OperatorEta,
};

/// Shift-local ETA context shared by ordinary room admission paths.
pub(crate) struct FacilityRotationEligibility<'a> {
    pub(crate) worked_hours: &'a HashMap<String, f64>,
    pub(crate) shift_hours: f64,
}

pub(crate) fn nonproduction_eta_failure(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    assignment: &BaseAssignment,
    mood: &HashMap<String, f64>,
    hours: f64,
    first_shift: Option<(&BaseAssignment, f64)>,
    facility: Option<FacilityKind>,
) -> Option<String> {
    let eta = shift_eta_with_instances(model, blueprint, assignment, instances);
    let first_eta = first_shift
        .map(|(assignment, _)| shift_eta_with_instances(model, blueprint, assignment, instances));
    eta.per_op.iter().find_map(|op| {
        let kind = blueprint.room(&RoomId::from(op.room_id.as_str()))?.kind;
        if !matches!(
            kind,
            FacilityKind::ControlCenter | FacilityKind::Office | FacilityKind::MeetingRoom
        ) || facility.is_some_and(|want| kind != want)
        {
            return None;
        }
        let next_hours = first_eta
            .as_ref()
            .and_then(|first| first.per_op.iter().find(|next| next.name == op.name))
            .map_or(0.0, |next| {
                next.drain_per_hour * first_shift.expect("first ETA has a shift").1
            });
        let available = mood.get(&op.name).copied().unwrap_or(model.mood_cap);
        (!fits_shift_budget(model, available, op.drain_per_hour, hours, next_hours))
            .then(|| op.name.clone())
    })
}

/// Build the trial state used by every ordinary-room candidate check.
pub(crate) fn assignment_with_candidate(
    assignment: &BaseAssignment,
    room_id: &RoomId,
    candidate: &AssignedOperator,
) -> BaseAssignment {
    let mut trial = assignment.clone();
    let mut operators = trial.operators_in(room_id).to_vec();
    operators.push(candidate.clone());
    trial.set_room(room_id.clone(), operators);
    trial
}

/// Read the candidate ETA from the trial assignment through the single ETA owner.
pub(crate) fn candidate_trial_eta(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: Option<&OperatorInstances>,
    assignment: &BaseAssignment,
    room_id: &RoomId,
    candidate: &AssignedOperator,
) -> Option<OperatorEta> {
    let trial = assignment_with_candidate(assignment, room_id, candidate);
    shift_eta_in_context(model, blueprint, &trial, instances)
        .operator_in_room(&candidate.name, &room_id.0)
        .cloned()
}

pub(crate) fn candidate_fits_facility_shift(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    room_id: &RoomId,
    candidate: &AssignedOperator,
    eligibility: Option<&FacilityRotationEligibility<'_>>,
) -> bool {
    let Some(eligibility) = eligibility else {
        return true;
    };
    let model = default_mood_model().expect("mood model loads for rotation eligibility");
    let Some(eta) = candidate_trial_eta(&model, blueprint, None, assignment, room_id, candidate)
    else {
        return false;
    };
    let max_hours = eta.workable_hours.unwrap_or(f64::INFINITY);
    let already_worked = eligibility
        .worked_hours
        .get(&candidate.name)
        .copied()
        .unwrap_or(0.0);
    eligibility.shift_hours <= max_hours + 1e-9
        && (eta.workable_hours.is_none()
            || already_worked + eligibility.shift_hours <= max_hours + 1e-9)
}

/// Check the current and next shift against the candidate's mood budget.
#[allow(clippy::too_many_arguments)]
pub(crate) fn candidate_fits_mood_budget(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    assignment: &BaseAssignment,
    room_id: &RoomId,
    candidate: &AssignedOperator,
    mood: &HashMap<String, f64>,
    next_consumption: &HashMap<String, f64>,
    hours: f64,
) -> bool {
    let trial = assignment_with_candidate(assignment, room_id, candidate);
    nonproduction_eta_failure(model, blueprint, instances, &trial, mood, hours, None, None)
        .is_none()
        && shift_eta_with_instances(model, blueprint, &trial, instances)
            .operator_in_room(&candidate.name, &room_id.0)
            .is_some_and(|op| {
                fits_shift_budget(
                    model,
                    mood.get(&candidate.name).copied().unwrap_or(model.mood_cap),
                    op.drain_per_hour,
                    hours,
                    next_consumption
                        .get(&candidate.name)
                        .copied()
                        .unwrap_or(0.0),
                )
            })
}

pub(crate) fn candidate_fits_rotation_shift(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    room_id: &RoomId,
    candidate: &AssignedOperator,
    eligibility: Option<&FacilityRotationEligibility<'_>>,
    mood_state: Option<(&HashMap<String, f64>, &HashMap<String, f64>)>,
) -> bool {
    let Some((mood, next_consumption)) = mood_state else {
        return candidate_fits_facility_shift(
            assignment,
            blueprint,
            room_id,
            candidate,
            eligibility,
        );
    };
    let Some(eligibility) = eligibility else {
        return true;
    };
    let model = default_mood_model().expect("mood model already loaded for rotation eligibility");
    candidate_fits_mood_budget(
        model,
        blueprint,
        instances,
        assignment,
        room_id,
        candidate,
        mood,
        next_consumption,
        eligibility.shift_hours,
    )
}

pub(crate) fn candidate_fits_current_shift(
    model: &MoodModel,
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    room_id: &RoomId,
    candidate: &AssignedOperator,
    shift_hours: f64,
    available_mood: f64,
    first_shift_drain: f64,
) -> bool {
    candidate_trial_eta(model, blueprint, None, assignment, room_id, candidate).is_some_and(|eta| {
        fits_shift_budget(
            model,
            available_mood,
            eta.drain_per_hour,
            shift_hours,
            first_shift_drain,
        )
    })
}
