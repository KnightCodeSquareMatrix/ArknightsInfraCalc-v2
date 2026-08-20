//! 收集全基建中所有 `scope=Global` 的 EffectAtom。

use crate::instances::{resolve_buff_ids, OperatorInstances};
use crate::layout::{BaseAssignment, BaseBlueprint, LayoutContext};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::types::{Action, AtomScope, EffectAtom};
use crate::{AssignedOperator, FacilityKind, RoomId};

/// 从全基建编制中收集所有 `scope=Global` 的 atom。
///
/// 返回按 `(phase.sort_key, phase_order)` 排序的 atom 列表，
/// 附带每个 atom 所属 operator 的信息（用于 Condition 求值）。
#[derive(Debug, Clone)]
pub struct GlobalAtomEntry {
    pub atom: EffectAtom,
    pub buff_id: String,
    pub owner_name: String,
    pub owner_elite: u8,
    pub owner_tags: Vec<String>,
    pub room_kind: FacilityKind,
    pub room_level: u8,
    pub room_id: RoomId,
}

pub fn collect_global_atoms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    _layout: &LayoutContext,
) -> Vec<GlobalAtomEntry> {
    let mut entries = Vec::new();

    for room in &blueprint.rooms {
        let ops = assignment.operators_in(&room.id);
        if ops.is_empty() {
            continue;
        }
        for op in ops {
            let tier = PromotionTier::from_elite(op.elite);
            let tags = instances
                .get(&op.name, tier)
                .map(|i| i.tags.clone())
                .unwrap_or_default();
            let buff_ids = resolve_facility_buff_ids(instances, op, &room.kind);
            for bid in &buff_ids {
                let Some(skill) = table.get(bid) else {
                    continue;
                };
                for atom in &skill.atoms {
                    if atom.scope != AtomScope::Global {
                        continue;
                    }
                    // 精0/精1 的宿舍与办公室干员默认跳过全局 atom，防止低练度
                    // 资源（梦境/小节/记忆碎片等）误入感知门槛链；塑心「无声共鸣」
                    // 按 PRTS 在精0 开放，其 SilentEcho 产出不受该保护影响。
                    if matches!(room.kind, FacilityKind::Dormitory | FacilityKind::Office)
                        && op.elite < 2
                        && !produces_silent_echo(atom)
                    {
                        continue;
                    }
                    entries.push(GlobalAtomEntry {
                        atom: atom.clone(),
                        buff_id: bid.clone(),
                        owner_name: op.name.clone(),
                        owner_elite: op.elite,
                        owner_tags: tags.clone(),
                        room_kind: room.kind,
                        room_level: room.level,
                        room_id: room.id.clone(),
                    });
                }
            }
        }
    }

    // 按 phase 排序
    entries.sort_by(|a, b| {
        let pa = a.atom.phase.sort_key();
        let pb = b.atom.phase.sort_key();
        pa.cmp(&pb)
            .then(a.atom.phase_order.cmp(&b.atom.phase_order))
    });

    entries
}

/// 是否为塑心「无声共鸣」式的 `SilentEcho` 产出 atom（精0 即生效）。
fn produces_silent_echo(atom: &EffectAtom) -> bool {
    matches!(atom.action, Action::StateProduce { ref key, .. } if key == "SilentEcho")
}

/// 根据设施类型和干员 elite 解析 buff_ids。
fn resolve_facility_buff_ids(
    instances: &OperatorInstances,
    op: &AssignedOperator,
    kind: &FacilityKind,
) -> Vec<String> {
    let tier = PromotionTier::from_elite(op.elite);
    let facility = match kind {
        FacilityKind::TradePost => "trade",
        FacilityKind::Factory => "manufacture",
        FacilityKind::PowerPlant => "power",
        FacilityKind::ControlCenter => "control",
        FacilityKind::Dormitory => "dorm",
        FacilityKind::Office => "office",
        _ => return Vec::new(),
    };
    let Some(inst) = instances.get(&op.name, tier) else {
        return Vec::new();
    };
    let Some(binding) = inst.facilities.get(facility) else {
        return Vec::new();
    };
    // 检查是否有 tier0 绑定用于 stepwise 合并
    let tier0_binding = if tier == PromotionTier::TierUp {
        instances
            .get(&op.name, PromotionTier::Tier0)
            .and_then(|inst| inst.facilities.get(facility))
    } else {
        None
    };
    resolve_buff_ids(tier, binding, tier0_binding)
}
