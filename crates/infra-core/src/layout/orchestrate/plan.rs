//! 编排计划：声明式描述「启用哪些体系、各 slot 如何填」，不含 trade/manu solve 评分。

use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::RegistrySystemClaim;
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
    pub slots: Vec<SlotFill>,
}

/// 单班进驻前的编排计划；`execute_plan` 将其落成 `BaseAssignment`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AssignmentPlan {
    pub mode: AssignShiftMode,
    /// 计划阶段已确定的体系（`base_systems` 选型结果）。
    pub activated: Vec<ActivatedSystem>,
    /// `base_systems.json` 认领明细（`execute_plan` 落位用）。
    pub registry_claims: Vec<RegistrySystemClaim>,
}

impl AssignmentPlan {
    pub fn recovery(mode: AssignShiftMode) -> Self {
        Self {
            mode,
            activated: Vec::new(),
            registry_claims: Vec::new(),
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
        slots,
    }
}
