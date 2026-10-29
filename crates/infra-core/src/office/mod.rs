//! 人力办公室：读取全局资源池的 consumer（不影响贸易/制造算分）。

mod input;
mod interpreter;

pub use input::{OfficeOperator, OfficeRoomInput};
pub use interpreter::{solve_office, OfficeResult};

use crate::support_facility::{
    evaluate_support_room, SupportFacility, SupportRegistry, SupportRoomInput, SupportRoomResult,
};

pub fn evaluate_office(
    input: &SupportRoomInput,
    registry: &SupportRegistry,
) -> Result<SupportRoomResult> {
    if input.facility != SupportFacility::Office {
        return Err(crate::error::Error::msg(
            "office evaluator requires office input",
        ));
    }
    evaluate_support_room(input, registry)
}

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

pub(crate) fn rank_candidates(
    operbox: &crate::operbox::OperBox,
    layout: &LayoutContext,
    mood: f64,
    shift_hours: f64,
    excluded: &std::collections::HashSet<String>,
) -> Vec<AssignedOperator> {
    let roster = operbox.roster();
    let mut registry = None;
    let winter_available = roster.names().any(|name| {
        !excluded.contains(name.as_str())
            && name == "忍冬"
            && roster
                .progress(name)
                .is_some_and(|progress| progress.elite >= 2)
    });
    let mut candidates: Vec<(f64, AssignedOperator)> = roster
        .names()
        .filter(|name| !excluded.contains(name.as_str()))
        .filter(|name| name.as_str() != "铃兰" || winter_available)
        .filter_map(|name| {
            let progress = roster.progress(name)?;
            let registry = registry.get_or_insert_with(|| {
                SupportRegistry::load_default_shared()
                    .expect("support registry loads in office candidate build")
            });
            let hours = layout.evaluation_hours(shift_hours).ok()?;
            let score = evaluate_office(
                &SupportRoomInput {
                    facility: SupportFacility::Office,
                    operators: vec![crate::support_facility::SupportOperator {
                        name: name.clone(),
                        elite: progress.elite,
                        rarity: progress.rarity,
                        level: progress.level,
                        mood: mood.max(0.0),
                    }],
                    capacity: 1,
                    extra_recruit_slots: 0,
                    elapsed_hours: hours,
                    external_speed_bonus_pct: 0.0,
                    layout: layout.clone(),
                },
                registry,
            )
            .map(|result| result.skill_speed_bonus_pct)
            .unwrap_or(0.0);
            Some((score, AssignedOperator::from_progress(name, progress)))
        })
        .collect();
    candidates.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    candidates
        .into_iter()
        .map(|(_, operator)| operator)
        .collect()
}

pub fn apply_office_to_layout(
    layout: &mut LayoutContext,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
) {
    let mut best = layout.control_office_hire_spd_pct;
    for room in blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::Office)
    {
        let operators: Vec<OfficeOperator> = assignment
            .operators_in(&room.id)
            .iter()
            .filter_map(|op| to_office_operator(instances, op).ok())
            .collect();
        if operators.is_empty() {
            continue;
        }
        let result = solve_office(
            &OfficeRoomInput {
                operators,
                mood,
                layout: layout.clone(),
            },
            table,
        );
        best = best.max(layout.control_office_hire_spd_pct + result.hire_spd_pct);
    }
    layout.office_hire_spd_pct = best;
}

fn to_office_operator(
    instances: &OperatorInstances,
    op: &AssignedOperator,
) -> Result<OfficeOperator> {
    let tier = PromotionTier::from_elite(op.elite);
    let buff_ids = instances.resolve_office_buff_ids(&op.name, tier);
    if buff_ids.is_empty() {
        return Err(crate::error::Error::msg(format!(
            "no office buff for {}@e{}",
            op.name, op.elite
        )));
    }
    Ok(OfficeOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
    })
}

/// 排序由调用方提供；试放始终包含已选成员，模式只负责其准入条件。
pub(crate) fn fill_vacancies(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    candidates: &[AssignedOperator],
    mut admits: impl FnMut(
        &BaseAssignment,
        &BaseAssignment,
        &crate::layout::RoomBlueprint,
        &AssignedOperator,
    ) -> bool,
) {
    let mut used = assignment.occupied_operator_names();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
    {
        for candidate in candidates {
            if assignment.operators_in(&room.id).len() >= room.operator_capacity() {
                break;
            }
            if used.contains(&candidate.name) {
                continue;
            }
            let mut trial = assignment.clone();
            let mut operators = trial.operators_in(&room.id).to_vec();
            operators.push(candidate.clone());
            trial.set_room(room.id.clone(), operators);
            if admits(assignment, &trial, room, candidate) {
                *assignment = trial;
                used.insert(candidate.name.clone());
            }
        }
    }
}
