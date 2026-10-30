//! 自动建议的工作时长约束。只读取本房、实际中枢和固定主力预算，不解释协同或资源来源。
use std::sync::Arc;

use crate::instances::OperatorInstances;
use crate::types::RecipeKind;

use super::{AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext, RoomId};

pub(crate) type BudgetMember = (String, u8, Vec<String>, Vec<String>);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct RoomWorkState {
    room: RoomId,
    kind: FacilityKind,
    recipe: Option<RecipeKind>,
    hours: f64,
    members: Vec<BudgetMember>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct WorkState {
    rooms: Vec<RoomWorkState>,
    control: Vec<(String, u8)>,
    outside_abyssal: usize,
}

/// resolve 每次只准备中枢和必要人数；固定主力的房预算以 Arc 共享。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct WorkBudgetContext {
    control: Vec<(String, u8)>,
    outside_abyssal: usize,
    fixed_main: Option<Arc<WorkState>>,
}

fn room_members(
    assignment: &BaseAssignment,
    room: &RoomId,
    kind: FacilityKind,
    instances: &OperatorInstances,
) -> Vec<BudgetMember> {
    assignment
        .operators_in(room)
        .iter()
        .map(|op| {
            let buffs = if kind == FacilityKind::ControlCenter {
                instances.resolve_control_buff_ids(&op.name, op.tier())
            } else {
                Vec::new()
            };
            (
                op.name.clone(),
                op.elite,
                buffs,
                instances.tags_for(&op.name, op.tier()),
            )
        })
        .collect()
}

fn abyssal_count(operators: &[AssignedOperator], instances: &OperatorInstances) -> usize {
    operators
        .iter()
        .filter(|op| {
            instances
                .tags_for(&op.name, op.tier())
                .iter()
                .any(|tag| tag == "cc.g.abyssal")
        })
        .count()
}

fn outside_abyssal(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> usize {
    blueprint
        .rooms
        .iter()
        .filter(|room| {
            !matches!(
                room.kind,
                FacilityKind::Dormitory | FacilityKind::ControlCenter
            )
        })
        .map(|room| abyssal_count(assignment.operators_in(&room.id), instances))
        .sum()
}

pub(crate) fn context(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> WorkBudgetContext {
    WorkBudgetContext {
        control: assignment
            .control_operators()
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect(),
        outside_abyssal: outside_abyssal(blueprint, assignment, instances),
        fixed_main: assignment.auto_main_work_budget.clone(),
    }
}

pub(crate) fn work_state(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> WorkState {
    WorkState {
        rooms: blueprint
            .rooms
            .iter()
            .filter(|room| super::ordinary_workplace(room.kind))
            .map(|room| RoomWorkState {
                room: room.id.clone(),
                kind: room.kind,
                recipe: match room.product {
                    Some(super::RoomProduct::Factory { recipe }) => Some(recipe),
                    _ => None,
                },
                hours: assignment.work_hours.unwrap_or(f64::NAN),
                members: room_members(assignment, &room.id, room.kind, instances),
            })
            .collect(),
        control: assignment
            .control_operators()
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect(),
        outside_abyssal: outside_abyssal(blueprint, assignment, instances),
    }
}

fn fits(
    kind: FacilityKind,
    recipe: Option<RecipeKind>,
    members: &[BudgetMember],
    hours: f64,
    control: &[(String, u8)],
    outside: usize,
) -> bool {
    hours.is_finite()
        && hours > 0.0
        && crate::mood::room_workable_hours_with_control(control, outside, kind, recipe, members)
            .is_ok_and(|workable| hours <= workable + 1e-9)
}

fn state_fits_control(state: &WorkState, control: &[(String, u8)]) -> bool {
    state.rooms.iter().all(|room| {
        fits(
            room.kind,
            room.recipe,
            &room.members,
            room.hours,
            control,
            state.outside_abyssal,
        )
    })
}

pub(crate) fn candidate_fits(
    layout: &LayoutContext,
    kind: FacilityKind,
    recipe: Option<RecipeKind>,
    members: &[BudgetMember],
) -> bool {
    if !layout.auto_rotation {
        return true;
    }
    let (Some(hours), Some(budget)) = (layout.work_hours, layout.auto_work_budget.as_ref()) else {
        return false;
    };
    if !fits(
        kind,
        recipe,
        members,
        hours,
        &budget.control,
        budget.outside_abyssal,
    ) {
        return false;
    }
    let Some(main) = &budget.fixed_main else {
        return true;
    };
    if kind == FacilityKind::ControlCenter {
        let control: Vec<_> = members.iter().map(|op| (op.0.clone(), op.1)).collect();
        state_fits_control(main, &control)
    } else {
        fits(
            kind,
            recipe,
            members,
            hours,
            &main.control,
            budget.outside_abyssal,
        )
    }
}

pub(crate) fn room_work_duration_fits(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    room_id: &RoomId,
    instances: &OperatorInstances,
) -> bool {
    if !assignment.auto_rotation {
        return true;
    }
    let Some(room) = blueprint.room(room_id) else {
        return false;
    };
    let layout = LayoutContext {
        auto_rotation: true,
        work_hours: assignment.work_hours,
        auto_work_budget: Some(Arc::new(context(blueprint, assignment, instances))),
        ..Default::default()
    };
    let recipe = match room.product {
        Some(super::RoomProduct::Factory { recipe }) => Some(recipe),
        _ => None,
    };
    candidate_fits(
        &layout,
        room.kind,
        recipe,
        &room_members(assignment, room_id, room.kind, instances),
    )
}

/// 备中枢选定（包括缺员/空中枢）后复核冻结主力，不依赖候选命中。
pub(crate) fn fixed_main_fits(assignment: &BaseAssignment) -> bool {
    assignment
        .auto_main_work_budget
        .as_ref()
        .is_none_or(|main| {
            let control: Vec<_> = assignment
                .control_operators()
                .iter()
                .map(|op| (op.name.clone(), op.elite))
                .collect();
            state_fits_control(main, &control)
        })
}

/// 每房两名单 × 两中枢。当前唯一额外心情维度（外站深海人数）按各房可选最大值取最坏输入。
pub(crate) fn assignments_fit(
    blueprint: &BaseBlueprint,
    main: &BaseAssignment,
    backup: &BaseAssignment,
    instances: &OperatorInstances,
) -> bool {
    let outside = blueprint
        .rooms
        .iter()
        .filter(|room| {
            !matches!(
                room.kind,
                FacilityKind::Dormitory | FacilityKind::ControlCenter
            )
        })
        .map(|room| {
            abyssal_count(main.operators_in(&room.id), instances)
                .max(abyssal_count(backup.operators_in(&room.id), instances))
        })
        .sum();
    let states = [
        work_state(blueprint, main, instances),
        work_state(blueprint, backup, instances),
    ];
    states.iter().all(|state| {
        state.rooms.iter().all(|room| {
            states.iter().all(|control| {
                fits(
                    room.kind,
                    room.recipe,
                    &room.members,
                    room.hours,
                    &control.control,
                    outside,
                )
            })
        })
    })
}
