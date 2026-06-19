//! System 选型：产出 `AssignmentPlan`（不调 solve）。

use crate::error::Result;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::select_registry_systems;
use crate::operbox::OperBox;

use super::plan::{registry_as_activated, AssignmentPlan};

/// 根据 operbox / 蓝图 / 班次模式 / 种子编制构建编排计划。
pub fn build_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<AssignmentPlan> {
    if mode != AssignShiftMode::Peak {
        return Ok(AssignmentPlan::recovery(mode));
    }

    let scratch = seed.clone();
    let used = scratch.operator_names();
    let registry_claims = select_registry_systems(
        blueprint,
        operbox,
        mode,
        &scratch,
        &used,
        &Default::default(),
    );
    let activated = registry_claims.iter().map(registry_as_activated).collect();

    Ok(AssignmentPlan {
        mode,
        activated,
        registry_claims,
    })
}
