//! 体系完整性判定：模拟 / 排班前决定「是否启用某体系、钉哪些锚点」。
//!
//! 不依赖 trade/manu search 评分；散件填充仍由 `assign` + `search` 负责。
//! 设计依据：`docs/ROSEMARY_PERCEPTION_CHAIN.md`（公孙长乐）。

mod apply;
mod context;
mod plan;
mod rosemary;

pub use apply::apply_rosemary_plan;
pub use context::EvaluateContext;
pub use plan::{
    EvaluateResult, OptionalProducer, RosemaryPlan, RosemaryTier, RosemaryVerdict, ShiftBind,
    SkipReason, SystemAnchor,
};
pub use rosemary::evaluate_rosemary;

/// 当前仅实现迷迭香感知链；后续在此汇总自动化/红松林等体系。
pub fn evaluate_systems(ctx: &EvaluateContext<'_>) -> EvaluateResult {
    EvaluateResult {
        rosemary: evaluate_rosemary(ctx),
    }
}
