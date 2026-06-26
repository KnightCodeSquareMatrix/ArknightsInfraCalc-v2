//! 编排计划：声明式描述「启用哪些体系、各 slot 如何填」，不含 trade/manu solve 评分。

use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::{RegistrySystemClaim, SlotFillMode};
use crate::layout::tier::OperatorTier;
use std::collections::HashSet;

/// 单个 slot 的落位方式（Phase 0 以 fixed / optional 为主；bond / core 在 Phase 2+ 扩展）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotFill {
    Fixed {
        operator: String,
        elite: u8,
        facility: FacilityKind,
        room_id: Option<RoomId>,
    },
    PickOne {
        candidates: Vec<String>,
        elite: u8,
        facility: FacilityKind,
        room_id: Option<RoomId>,
    },
    /// 可选 producer：缺房 / 缺人时 execute 静默跳过。
    OptionalFixed {
        operator: String,
        elite: u8,
        facility: FacilityKind,
    },
}

/// 已选中的成套体系（来自 `base_systems` 认领）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActivatedSystem {
    pub system_id: String,
    pub priority: i32,
    pub tier: OperatorTier,
    pub slots: Vec<SlotFill>,
}

// ── Phase 2 体系语义类型（ADR 0001 决策 C） ──────────────────────────────
// 当前仅定义类型并挂在 `AssignmentPlan` 上（默认空），不改变排班结果。
// 由后续 Phase（体系层接线 / execute 三态 / anchor fill）逐步产出与消费。
// 设计依据：docs/TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md §2。

/// anchor 填充策略：决定 fill 阶段如何补齐 anchor 房的队友。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnchorFillPolicy {
    /// 贸易 anchor：走 trade role / L3 shortcut 补齐。
    TradeRole { role_id: Option<String> },
    /// 制造 anchor：按 recipe 约束搜索补齐。
    ManufactureRecipe,
    /// 普通搜索补齐（must_include = 核心）。
    Plain,
}

/// 体系锚点：钉核心干员与设施，队友由 fill 阶段补齐（蓝本 `system_integrity/plan.rs:30`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemAnchor {
    pub system_id: String,
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    /// `None` = 该设施类型首个空房，不绑定具体 `room_id`。
    pub room_id: Option<RoomId>,
    pub fill_policy: AnchorFillPolicy,
}

/// 只提供全局 / 跨设施资源的 producer slot（蓝本 `OptionalProducer`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProducerSlot {
    pub system_id: String,
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    /// 缺人时裁剪，不拖死核心。
    pub optional: bool,
}

/// 不占房但影响补齐搜索的约束（补 pairwise；现仅有系统级 `exclusive_group`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemConstraint {
    /// 两名干员禁止同房（迷迭香 ↔ 清流+温蒂）。
    ForbidSameRoom { a: String, b: String },
    /// 两名干员禁止同贸易站（黑键 ↔ 巫恋）。
    ForbidSameStation { a: String, b: String },
    /// 体系要求蓝图存在某设施。
    RequireFacility { facility: FacilityKind },
}

/// 降级阶梯结果（蓝本 `RosemaryTier` + `producers_present/missing`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DegradationLadder {
    pub system_id: String,
    /// 满配 / 档1 / 档2 / 档3 / 替代感知源。
    pub tier_label: String,
    /// priority-by-tier：高档保持高优先，低档降至核心体系之下。
    pub priority: i32,
    pub producers_present: Vec<String>,
    pub producers_missing: Vec<String>,
}

/// 同班绑定（已存在于 `system_integrity/plan.rs:21`，迁入 plan 供轮换消费）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ShiftBind {
    pub operators: Vec<String>,
    pub on_shifts: u8,
    pub off_shifts: u8,
}

/// 单班进驻前的编排计划；`execute_plan` 将其落成 `BaseAssignment`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AssignmentPlan {
    pub mode: AssignShiftMode,
    /// 计划阶段已确定的体系（`base_systems` 选型结果）。
    pub activated: Vec<ActivatedSystem>,
    /// `base_systems.json` 认领明细（`execute_plan` 落位用）。
    pub registry_claims: Vec<RegistrySystemClaim>,
    /// Phase 2 体系语义：两路径（registry + 代码化体系层）汇合产出。
    /// 当前默认空，由后续 Phase 产出与消费，不影响现有排班。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<SystemAnchor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producers: Vec<ProducerSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<SystemConstraint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradations: Vec<DegradationLadder>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shift_binds: Vec<ShiftBind>,
}

impl AssignmentPlan {
    pub fn recovery(mode: AssignShiftMode) -> Self {
        Self {
            mode,
            activated: Vec::new(),
            registry_claims: Vec::new(),
            anchors: Vec::new(),
            producers: Vec::new(),
            constraints: Vec::new(),
            degradations: Vec::new(),
            shift_binds: Vec::new(),
        }
    }

    /// 已选 `base_systems` 体系 id（按 priority 降序）。
    pub fn registry_system_ids(&self) -> Vec<&str> {
        let mut claims: Vec<_> = self.registry_claims.iter().collect();
        claims.sort_by_key(|c| std::cmp::Reverse(c.priority));
        claims.into_iter().map(|c| c.system_id.as_str()).collect()
    }

    /// 编排认领的贸易站房间（meta 锚点；余站由 `assign_trade_remainder` 填）。
    pub fn registry_trade_room_ids(&self) -> HashSet<RoomId> {
        self.registry_claims
            .iter()
            .flat_map(|c| &c.slots)
            .filter(|s| s.facility == FacilityKind::TradePost)
            .map(|s| s.room_id.clone())
            .collect()
    }

    /// 编排认领的全部干员名（registry fixed/pick 落位结果）。
    pub fn registry_operator_names(&self) -> HashSet<String> {
        self.registry_claims
            .iter()
            .flat_map(|c| &c.slots)
            .flat_map(|s| s.operators.iter().map(|o| o.name.clone()))
            .collect()
    }

    /// peak 切半后，registry 贸易 meta 房间与干员应落在 α 或 β（不在 γ 替补池）。
    pub fn verify_registry_trade_in_alpha_beta(
        &self,
        alpha: &crate::layout::BaseAssignment,
        beta: &crate::layout::BaseAssignment,
    ) -> Result<(), String> {
        for claim in &self.registry_claims {
            for slot in &claim.slots {
                if slot.facility != FacilityKind::TradePost {
                    continue;
                }
                if slot.fill == SlotFillMode::Search {
                    for op in &slot.operators {
                        let in_ab = alpha.operator_names().contains(&op.name)
                            || beta.operator_names().contains(&op.name);
                        if !in_ab {
                            return Err(format!(
                                "registry anchor operator {} not in α/β slice",
                                op.name
                            ));
                        }
                    }
                    continue;
                }
                let room = &slot.room_id;
                let alpha_ops = alpha.operators_in(room);
                let beta_ops = beta.operators_in(room);
                if alpha_ops.is_empty() && beta_ops.is_empty() {
                    return Err(format!(
                        "registry trade {} @ {} missing from α/β",
                        claim.system_id, room.0
                    ));
                }
                for op in &slot.operators {
                    let in_ab = alpha_ops.iter().any(|a| a.name == op.name)
                        || beta_ops.iter().any(|a| a.name == op.name);
                    if !in_ab {
                        return Err(format!(
                            "registry operator {} @ {} not in α/β slice",
                            op.name, room.0
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn registry_as_activated(claim: &RegistrySystemClaim) -> ActivatedSystem {
    let slots = claim
        .slots
        .iter()
        .flat_map(|slot| {
            slot.operators.iter().map(|op| SlotFill::Fixed {
                operator: op.name.clone(),
                elite: op.elite,
                facility: slot.facility,
                room_id: Some(slot.room_id.clone()),
            })
        })
        .collect();
    ActivatedSystem {
        system_id: claim.system_id.clone(),
        priority: claim.priority,
        tier: claim.tier,
        slots,
    }
}
