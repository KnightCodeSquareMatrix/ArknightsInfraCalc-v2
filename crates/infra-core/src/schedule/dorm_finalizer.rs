use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::{AssignedOperator, BaseBlueprint, FacilityKind, RoomId};
use crate::operbox::OperBox;

use super::rotation_types::{DormRestPlan, TeamShiftResult};

/// 普通轮换休息不足时保留给报告层的确定性信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DormFinalizerWarning {
    pub shift_index: usize,
    pub available_beds: usize,
    pub candidate_count: usize,
    pub omitted_names: Vec<String>,
}

pub(crate) fn render_warning(warning: DormFinalizerWarning) -> String {
    format!(
        "shift {} 宿舍普通休息候选 {} 人超过可用空床 {}，普通休息启用 MAA autofill: {}",
        warning.shift_index + 1,
        warning.candidate_count,
        warning.available_beds,
        warning.omitted_names.join(", ")
    )
}

/// 在所有班次的工作 assignment 完成后，统一写入宿舍成员。
///
/// 普通休息成员只来自相邻班次的实际非宿舍工作差集：
/// `previous_work - current_work`。已有宿舍成员和显式 `DormRestPlan`
/// 与普通休息向量合并后按姓名去重；本函数不重新选择菲亚梅塔或其他挂件。
pub(crate) fn finalize_dorms(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    shifts: &mut [TeamShiftResult],
) -> Result<Vec<DormFinalizerWarning>> {
    if shifts.is_empty() {
        return Ok(Vec::new());
    }

    let work_by_shift: Vec<Vec<AssignedOperator>> = shifts
        .iter()
        .map(|shift| working_operators(blueprint, &shift.assignment))
        .collect();
    let mut warnings = Vec::new();

    for index in 0..shifts.len() {
        let previous_index = if index == 0 {
            shifts.len() - 1
        } else {
            index - 1
        };
        let current_work_names: HashSet<String> = work_by_shift[index]
            .iter()
            .map(|operator| operator.name.clone())
            .collect();
        let ordinary_rest: Vec<AssignedOperator> = work_by_shift[previous_index]
            .iter()
            .filter(|operator| !current_work_names.contains(&operator.name))
            .cloned()
            .collect();

        if let Some(warning) =
            finalize_shift_dorm(blueprint, operbox, &ordinary_rest, &mut shifts[index])?
        {
            warnings.push(warning);
        }
    }

    Ok(warnings)
}

fn working_operators(
    blueprint: &BaseBlueprint,
    assignment: &crate::layout::BaseAssignment,
) -> Vec<AssignedOperator> {
    unique_operators(assignment.work_position_operators(blueprint).cloned())
}

fn finalize_shift_dorm(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    ordinary_rest: &[AssignedOperator],
    shift: &mut TeamShiftResult,
) -> Result<Option<DormFinalizerWarning>> {
    let dorms = blueprint.rooms_of(FacilityKind::Dormitory);
    if dorms.is_empty() {
        return if ordinary_rest.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DormFinalizerWarning {
                shift_index: shift.index,
                available_beds: 0,
                candidate_count: ordinary_rest.len(),
                omitted_names: ordinary_rest
                    .iter()
                    .map(|operator| operator.name.clone())
                    .collect(),
            }))
        };
    }

    let existing = dorm_occupants(blueprint, &shift.assignment);
    let existing_operators = existing
        .iter()
        .flat_map(|(_, operators)| operators.iter().cloned());
    let mut resting = unique_operators(existing_operators.chain(ordinary_rest.iter().cloned()));

    let special = shift
        .dorm_rest
        .as_ref()
        .map(|plan| special_rest_operators(plan, &resting, shift, operbox))
        .transpose()?;
    let special_names: HashSet<String> = special
        .as_ref()
        .map(|operators| {
            operators
                .iter()
                .map(|operator| operator.name.clone())
                .collect()
        })
        .unwrap_or_default();
    resting.retain(|operator| !special_names.contains(&operator.name));

    let capacities: Vec<usize> = dorms
        .iter()
        .map(|room| room.dorm_bed_count().max(1) as usize)
        .collect();
    let total_capacity: usize = capacities.iter().sum();
    let special_room_index = shift
        .dorm_rest
        .as_ref()
        .map(|plan| {
            dorms
                .iter()
                .position(|room| room.id == plan.room_id)
                .ok_or_else(|| {
                    Error::msg(format!(
                        "shift {} dorm recovery room {} is not a dormitory",
                        shift.index + 1,
                        plan.room_id.0
                    ))
                })
        })
        .transpose()?;
    if let (Some(index), Some(special)) = (special_room_index, special.as_ref()) {
        if special.len() > capacities[index] {
            return Err(Error::msg(format!(
                "shift {} dorm recovery group exceeds room {} capacity",
                shift.index + 1,
                dorms[index].id.0
            )));
        }
    }

    let preserved_names: HashSet<String> = existing
        .iter()
        .flat_map(|(_, operators)| operators)
        .map(|operator| operator.name.clone())
        .collect();
    let protected_count = preserved_names.len()
        + special
            .as_ref()
            .map(|operators| {
                operators
                    .iter()
                    .filter(|operator| !preserved_names.contains(&operator.name))
                    .count()
            })
            .unwrap_or(0);
    if protected_count > total_capacity {
        return Err(Error::msg(format!(
            "shift {} existing dorm occupants and recovery group exceed total dorm capacity",
            shift.index + 1
        )));
    }

    let ordinary_names: HashSet<String> = ordinary_rest
        .iter()
        .map(|operator| operator.name.clone())
        .collect();
    let ordinary_new_count = ordinary_names
        .iter()
        .filter(|name| !preserved_names.contains(*name) && !special_names.contains(*name))
        .count();
    let ordinary_overflow = ordinary_new_count > total_capacity.saturating_sub(protected_count);
    if ordinary_overflow {
        resting.retain(|operator| {
            !ordinary_names.contains(&operator.name) || preserved_names.contains(&operator.name)
        });
    }
    let mut remaining = resting.into_iter();
    for (index, room) in dorms.iter().enumerate() {
        let mut occupants = Vec::new();
        if Some(index) == special_room_index {
            for operator in special.as_ref().expect("special room has special group") {
                occupants.push(operator.clone());
            }
        }
        while occupants.len() < capacities[index] {
            let Some(operator) = remaining.next() else {
                break;
            };
            occupants.push(operator);
        }
        shift.assignment.set_room(room.id.clone(), occupants);
    }

    let omitted_names: Vec<String> = ordinary_rest
        .iter()
        .filter(|operator| {
            !dorms.iter().any(|room| {
                shift
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .any(|placed| placed.name == operator.name)
            })
        })
        .map(|operator| operator.name.clone())
        .collect();
    if omitted_names.is_empty() {
        return Ok(None);
    }

    let available_beds = total_capacity.saturating_sub(protected_count);
    Ok(Some(DormFinalizerWarning {
        shift_index: shift.index,
        available_beds,
        candidate_count: ordinary_names.len(),
        omitted_names,
    }))
}

fn dorm_occupants(
    blueprint: &BaseBlueprint,
    assignment: &crate::layout::BaseAssignment,
) -> Vec<(RoomId, Vec<AssignedOperator>)> {
    blueprint
        .rooms_of(FacilityKind::Dormitory)
        .into_iter()
        .map(|room| (room.id.clone(), assignment.operators_in(&room.id).to_vec()))
        .collect()
}

fn special_rest_operators(
    plan: &DormRestPlan,
    existing: &[AssignedOperator],
    shift: &TeamShiftResult,
    operbox: &OperBox,
) -> Result<Vec<AssignedOperator>> {
    let names = [&plan.single_manager, &plan.group_manager, &plan.target];
    let unique_names: HashSet<&str> = names.iter().map(|name| name.as_str()).collect();
    if unique_names.len() != names.len() {
        return Err(Error::msg(format!(
            "shift {} dorm recovery group contains duplicate members",
            shift.index + 1
        )));
    }

    names
        .into_iter()
        .map(|name| {
            existing
                .iter()
                .find(|operator| operator.name == *name)
                .cloned()
                .or_else(|| {
                    operbox
                        .progress_of(name)
                        .map(|progress| AssignedOperator::from_progress(name, progress))
                })
                .ok_or_else(|| {
                    Error::msg(format!(
                        "shift {} dorm recovery member {} is absent",
                        shift.index + 1,
                        name
                    ))
                })
        })
        .collect()
}

fn unique_operators(
    operators: impl IntoIterator<Item = AssignedOperator>,
) -> Vec<AssignedOperator> {
    let mut names = HashSet::new();
    operators
        .into_iter()
        .filter(|operator| names.insert(operator.name.clone()))
        .collect()
}
