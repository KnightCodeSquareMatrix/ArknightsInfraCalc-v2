//! 控制中枢：全局资源 producer + 贸易/制造全局 % 注入。

mod input;
mod interpreter;

pub use crate::global_resource::{
    GlobalInjectManifest, INJECT_FAMILY_MANU_GLOBAL_ALL, INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
pub use input::{ControlOperator, ControlRoomInput};
pub use interpreter::{
    solve_control, solve_control_with_trace, ControlCenterResult, ControlConditionStatus,
    ControlSettlementTrace, ControlStateValue, ControlTraceEvent, ControlTraceOutcome,
};

use crate::layout::LayoutContext;
use crate::skill_table::SkillTable;

/// 在中枢编制上求值，将资源池与全局注入写回 `layout`。
pub fn apply_control_to_layout(
    layout: &mut LayoutContext,
    operators: &[ControlOperator],
    table: &SkillTable,
    mood: f64,
) -> ControlCenterResult {
    let input = ControlRoomInput {
        operators: operators.to_vec(),
        mood,
        layout: layout.clone(),
    };
    let result = solve_control(&input, table);
    layout.global = result.global.clone();
    layout.global_inject = result.inject.clone();
    result
}

pub fn apply_control_to_layout_with_trace(
    layout: &mut LayoutContext,
    operators: &[ControlOperator],
    table: &SkillTable,
    mood: f64,
) -> (ControlCenterResult, ControlSettlementTrace) {
    let input = ControlRoomInput {
        operators: operators.to_vec(),
        mood,
        layout: layout.clone(),
    };
    let (result, trace) = solve_control_with_trace(&input, table);
    layout.global = result.global.clone();
    layout.global_inject = result.inject.clone();
    (result, trace)
}
