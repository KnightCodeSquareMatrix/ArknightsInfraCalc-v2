//! 人力办公室：读取全局资源池的 consumer（不影响贸易/制造算分）。

mod input;
mod interpreter;

pub use input::{OfficeOperator, OfficeRoomInput};
pub use interpreter::{solve_office, OfficeResult};

use crate::layout::{
    AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext,
};
use crate::instances::OperatorInstances;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::error::Result;

pub fn apply_office_to_layout(
    layout: &mut LayoutContext,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
) {
    let mut best = layout.office_hire_spd_pct;
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
        best = best.max(result.hire_spd_pct);
    }
    layout.office_hire_spd_pct = best;
}

fn to_office_operator(instances: &OperatorInstances, op: &AssignedOperator) -> Result<OfficeOperator> {
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
