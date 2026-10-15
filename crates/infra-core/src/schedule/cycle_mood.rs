use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{BaseBlueprint, FacilityKind};
use crate::mood::{default_mood_model, shift_eta_with_instances};

use super::TeamRotationReport;

#[derive(Clone)]
struct WorkCost {
    room_id: String,
    cost: f64,
}

pub(crate) fn is_budget_rejection(error: &Error) -> bool {
    match error {
        Error::Msg(message) | Error::PartialRotation { message, .. } => {
            message.starts_with("rotation_cycle_mood failed:")
        }
        _ => false,
    }
}

pub(crate) fn validate_cycle_mood(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
    initial_mood: f64,
) -> Result<()> {
    let model = default_mood_model()?;
    let count = report.shifts.len();
    if count == 0 || !initial_mood.is_finite() || initial_mood < model.rest_threshold {
        return Err(Error::msg("rotation_cycle_mood invalid initial state"));
    }
    let mut work: BTreeMap<String, Vec<Option<WorkCost>>> = BTreeMap::new();
    for (index, shift) in report.shifts.iter().enumerate() {
        if !shift.duration_hours.is_finite() || shift.duration_hours <= 0.0 {
            return Err(Error::msg(format!(
                "rotation_cycle_mood invalid duration in shift {}",
                index + 1
            )));
        }
        if let Some(assistant) = &shift.assignment.training_assist {
            return Err(Error::msg(format!(
                "rotation_cycle_mood_unsupported_activity: shift {} training_assist {} has no training activity budget",
                index + 1, assistant.name
            )));
        }
        let eta = shift_eta_with_instances(&model, blueprint, &shift.assignment, instances);
        for room in shift.assignment.iter_rooms() {
            let spec = blueprint.room(&room.room_id).ok_or_else(|| {
                Error::msg(format!(
                    "rotation_cycle_mood unknown room {} in shift {}",
                    room.room_id.0,
                    index + 1
                ))
            })?;
            if spec.kind == FacilityKind::Dormitory {
                continue;
            }
            for operator in &room.operators {
                // Workshop and training-room occupancy has no hourly cost, but is not a
                // rest boundary. The assignment carries no training task, so a training
                // room seat is a pure support placement without a training activity.
                let drain = if matches!(
                    spec.kind,
                    FacilityKind::Workshop | FacilityKind::TrainingRoom
                ) {
                    0.0
                } else {
                    eta.per_op
                        .iter()
                        .find(|entry| {
                            entry.name == operator.name && entry.room_id == room.room_id.0
                        })
                        .ok_or_else(|| {
                            Error::msg(format!(
                                "rotation_cycle_mood missing drain for {} in shift {} room {}",
                                operator.name,
                                index + 1,
                                room.room_id.0
                            ))
                        })?
                        .drain_per_hour
                };
                let cost = drain * shift.duration_hours;
                if !drain.is_finite() || drain < 0.0 || !cost.is_finite() {
                    return Err(Error::msg(format!(
                        "rotation_cycle_mood invalid drain for {} in shift {}",
                        operator.name,
                        index + 1
                    )));
                }
                let entries = work
                    .entry(operator.name.clone())
                    .or_insert_with(|| vec![None; count]);
                if entries[index].is_some() {
                    return Err(Error::msg(format!(
                        "rotation_cycle_mood duplicate operator {} in shift {}",
                        operator.name,
                        index + 1
                    )));
                }
                entries[index] = Some(WorkCost {
                    room_id: room.room_id.0.clone(),
                    cost,
                });
            }
        }
    }

    let capacity = model.workable_mood();
    for (name, entries) in work {
        let restores = |index: usize| {
            entries[index].is_none()
                || report.shifts[index]
                    .fiammetta
                    .as_ref()
                    .is_some_and(|action| action.target == name)
        };
        let cycle_cost: f64 = entries.iter().flatten().map(|entry| entry.cost).sum();
        if !(0..count).any(restores) && cycle_cost > 1e-9 {
            return Err(Error::msg(format!(
                "rotation_cycle_mood failed: {name} has no rest or recovery boundary in {}; cycle_cost={cycle_cost:.2}; positions={}",
                report.profile.cli_name(),
                positions(&entries, 0, count)
            )));
        }

        // The first execution uses the caller's mood; cycle boundaries never reset it.
        let mut available = initial_mood.min(model.mood_cap) - model.rest_threshold;
        for (index, entry) in entries.iter().enumerate() {
            if restores(index) {
                available = capacity;
            }
            if let Some(entry) = entry {
                if entry.cost > available + 1e-9 {
                    return Err(Error::msg(format!(
                        "rotation_cycle_mood failed: {name} in initial shift {} room {}; cost={:.2} available={available:.2}",
                        index + 1, entry.room_id, entry.cost
                    )));
                }
                available = (available - entry.cost).max(0.0);
            }
        }

        // Every rotation of the array covers a possible tail-to-head work span.
        for start in 0..count {
            let mut consumed = 0.0;
            let mut span_start = start;
            for offset in 0..count {
                let index = (start + offset) % count;
                if restores(index) {
                    consumed = 0.0;
                    span_start = index;
                }
                if let Some(entry) = &entries[index] {
                    consumed += entry.cost;
                    if consumed > capacity + 1e-9 {
                        return Err(Error::msg(format!(
                            "rotation_cycle_mood failed: {name} in {}; continuous_cost={consumed:.2} available={capacity:.2}; positions={}",
                            report.profile.cli_name(),
                            positions(&entries, span_start, (index + count - span_start) % count + 1)
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn positions(entries: &[Option<WorkCost>], start: usize, length: usize) -> String {
    (0..length)
        .filter_map(|offset| {
            let index = (start + offset) % entries.len();
            entries[index]
                .as_ref()
                .map(|entry| format!("S{}:{}({:.2})", index + 1, entry.room_id, entry.cost))
        })
        .collect::<Vec<_>>()
        .join(",")
}
