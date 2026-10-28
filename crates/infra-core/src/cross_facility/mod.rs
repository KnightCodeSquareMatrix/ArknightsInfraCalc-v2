//! 跨设施编排层：统一执行 `scope=global` 的 EffectAtom。
//!
//! # 职责
//!
//! 1. 收集全基建所有设施的 `scope=Global` atom
//! 2. 按 Phase 顺序执行，写入 `GlobalResourcePool`
//! 3. 产出编排后的 `GlobalResourceSnapshot`，供 per-room 求解使用
//!
//! # 设计原则
//!
//! - 不改变现有 per-room 求解逻辑（trade/manufacture/power interpreter 不变）
//! - 只处理 `scope=Global` 的 atom，per-room 求解时跳过这些 atom
//! - 与 `GlobalInjectManifest` 协同：中枢 `GlobalInject` 阶段已有自己的机制，仍由 `control/interpreter.rs` 处理

mod collector;
mod interpreter;

pub use collector::collect_global_atoms;
pub use interpreter::orchestrate_global_atoms;

use crate::global_resource::GlobalInjectManifest;
use crate::global_resource::GlobalResourcePool;
use crate::layout::LayoutContext;

/// 跨设施编排输出：全基建快照，供 per-room 求解使用。
#[derive(Debug, Clone)]
pub struct GlobalResourceSnapshot {
    /// 全局资源池（所有 scope=Global 的 StateWrite 已执行完毕）。
    pub global: GlobalResourcePool,
    /// 中枢注入清单（仍由 `control/interpreter.rs` 产生，本层透传）。
    pub inject: GlobalInjectManifest,
    /// 全基建布局统计（不含 global/inject，仅 layout 字段）。
    pub layout: LayoutContext,
}
