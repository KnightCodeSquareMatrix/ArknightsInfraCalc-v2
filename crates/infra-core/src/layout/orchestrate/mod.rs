//! 全基建进驻编排：`Rule → resolved Plan → Execute`。
//!
//! `select` 汇合高优先声明式规则、兼容 registry 与 late 挂件规则；
//! `execute` 只消费已解析 placement，不按体系 id 重判。

pub(crate) mod auto_support;
mod execute;
mod plan;
mod rules;
mod select;
pub(crate) use rules::{assert_auto_rotation_plan_policy, assert_support_combination_policy};

pub use execute::{execute_plan, ExecuteResult};
pub(crate) use plan::pack_production_components;
pub use plan::{
    ActivatedSystem, ActiveDependency, AnchorFillPolicy, AssignmentPlan, ContinuousRole,
    ControlCandidateRequirement, ProducerSlot, ReserveReusePolicy, ResolvedRoleReserve,
    SelectedRuleAlternative, ShiftBind, SlotFill, SystemAnchor, SystemConstraint,
};
pub use select::{build_plan, build_plan_with_runtime};
pub(crate) use select::{build_plan_with_runtime_and_disabled_packs, guarded_pack_violation};
