//! 将 `AssignmentPlan` 落位为 `BaseAssignment`（不含 trade/manu search 贪心）。

use std::collections::HashSet;

use crate::error::Result;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, RoomProduct};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::apply_registry_system_claim;
use crate::operbox::OperBox;
use crate::skill_table::SkillTable;

use super::plan::AssignmentPlan;

/// `execute_plan` 输出：编制结果 + 已占用干员集合。
#[derive(Debug, Clone)]
pub struct ExecuteResult {
    pub assignment: BaseAssignment,
    pub used: HashSet<String>,
}

/// 把计划中的 registry 体系认领写入编制。
pub fn execute_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    table: &SkillTable,
    plan: &AssignmentPlan,
    seed: &BaseAssignment,
) -> Result<ExecuteResult> {
    let _ = table;
    let mut assignment = seed.clone();
    let mut used = assignment.operator_names();

    if plan.mode == AssignShiftMode::Peak {
        for anchor in &plan.anchors {
            if used.contains(&anchor.operator) {
                assignment.construction_warnings.push(format!(
                    "planned operator {} for {} was occupied before execute",
                    anchor.operator, anchor.system_id
                ));
                continue;
            }
            let room_id = anchor.room_id.as_ref().ok_or_else(|| {
                crate::error::Error::msg(format!(
                    "unresolved room for planned operator {} in {}",
                    anchor.operator, anchor.system_id
                ))
            })?;
            let room = blueprint.room(room_id).ok_or_else(|| {
                crate::error::Error::msg(format!("planned room {} no longer exists", room_id.0))
            })?;
            let product_matches = anchor.recipe.is_none_or(|required| {
                matches!(room.product, Some(RoomProduct::Factory { recipe }) if recipe == required)
            }) && anchor.trade_order.is_none_or(|required| {
                matches!(room.product, Some(RoomProduct::Trade { order }) if order == required)
            });
            if room.kind != anchor.facility
                || !product_matches
                || assignment.operators_in(room_id).len() >= room.operator_capacity()
            {
                assignment.construction_warnings.push(format!(
                    "planned placement {} -> {} is no longer feasible",
                    anchor.operator, room_id.0
                ));
                continue;
            }
            let progress = operbox.progress_of(&anchor.operator).ok_or_else(|| {
                crate::error::Error::msg(format!(
                    "planned operator {} disappeared from operbox",
                    anchor.operator
                ))
            })?;
            let mut operators = assignment.operators_in(room_id).to_vec();
            operators.push(
                crate::layout::AssignedOperator::from_progress(&anchor.operator, progress)
                    .with_work_mood(anchor.work_mood),
            );
            assignment.set_room(room_id.clone(), operators);
            used.insert(anchor.operator.clone());
        }
        for claim in &plan.registry_claims {
            let mut trial = assignment.clone();
            let mut trial_used = used.clone();
            match apply_registry_system_claim(blueprint, claim, &mut trial, &mut trial_used) {
                Ok(()) => {
                    assignment = trial;
                    used = trial_used;
                }
                Err(crate::error::Error::ScheduleQuality(message)) => {
                    assignment.construction_warnings.push(message)
                }
                Err(error) => return Err(error),
            }
        }
    }
    used.extend(plan.excluded_operators.iter().cloned());
    used.extend(
        plan.rotation_reserves
            .iter()
            .flat_map(|reserve| reserve.operators.iter().cloned()),
    );

    Ok(ExecuteResult { assignment, used })
}
