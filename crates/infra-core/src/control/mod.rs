//! 控制中枢：全局资源 producer + 贸易/制造全局 % 注入。

mod input;
mod interpreter;

pub use crate::global_resource::{
    GlobalInjectManifest, INJECT_FAMILY_MANU_GLOBAL_ALL, INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
pub use input::{ControlOperator, ControlRoomInput};
pub use interpreter::{solve_control, ControlCenterResult};

use crate::skill_table::SkillTable;
use crate::layout::LayoutContext;

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
    let mut result = solve_control(&input, table);
    if control_has_haru_e2(operators) {
        result.inject.record_haru_e2_in_control();
    }
    layout.global = result.global.clone();
    layout.global_inject = result.inject.clone();
    result
}

/// 中枢八幡海铃 E2「家族认可」——叙拉古但书链段 producer 条件。
pub fn control_has_haru_e2(operators: &[ControlOperator]) -> bool {
    const HARU: &str = "八幡海铃";
    const FAMILY_RECOGNITION: &str = "control_tra_limit&spd2[000]";
    operators.iter().any(|o| {
        o.name == HARU
            && o.elite >= 2
            && o.buff_ids.iter().any(|b| b == FAMILY_RECOGNITION)
    })
}
