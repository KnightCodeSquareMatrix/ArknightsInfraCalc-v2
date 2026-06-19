//! 将 `AssignmentPlan` 落位为 `BaseAssignment`（不含 trade/manu search 贪心）。

use std::collections::HashSet;

use crate::error::Result;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
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
    let _ = (blueprint, operbox, table);
    let mut assignment = seed.clone();
    let mut used = assignment.operator_names();

    if plan.mode == AssignShiftMode::Peak {
        for claim in &plan.registry_claims {
            apply_registry_system_claim(claim, &mut assignment, &mut used)?;
        }
    }

    Ok(ExecuteResult { assignment, used })
}
