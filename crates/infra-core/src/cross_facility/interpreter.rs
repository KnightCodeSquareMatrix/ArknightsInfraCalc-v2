//! 跨设施编排执行器：执行 `scope=Global` 的 EffectAtom，产出全局资源池。

use crate::global_resource::GlobalResourcePool;
use crate::layout::LayoutContext;
use crate::types::{Action, Condition, Phase, Selector, StateKey};

use super::collector::GlobalAtomEntry;
use super::GlobalResourceSnapshot;

/// 编排执行所有 `scope=Global` 的 atom，产出全局资源快照。
///
/// 执行范围：
/// - `StateWrite`：写入 `global` 资源池
/// - 其他 Phase 暂不处理（`GlobalInject` 仍由 `control/interpreter.rs` 管理）
pub fn orchestrate_global_atoms(
    atoms: &[GlobalAtomEntry],
    layout: &LayoutContext,
    existing_global: GlobalResourcePool,
) -> GlobalResourceSnapshot {
    orchestrate_global_atoms_inner(atoms, layout, existing_global, false).0
}

/// 由同一执行器记录实际非零写入，供准入读取来源；不另算资源公式。
pub(crate) fn orchestrate_global_atoms_with_writes(
    atoms: &[GlobalAtomEntry],
    layout: &LayoutContext,
    existing_global: GlobalResourcePool,
) -> (
    GlobalResourceSnapshot,
    Vec<(usize, StateKey, f64, Option<StateKey>)>,
) {
    orchestrate_global_atoms_inner(atoms, layout, existing_global, true)
}

fn orchestrate_global_atoms_inner(
    atoms: &[GlobalAtomEntry],
    layout: &LayoutContext,
    existing_global: GlobalResourcePool,
    record_writes: bool,
) -> (
    GlobalResourceSnapshot,
    Vec<(usize, StateKey, f64, Option<StateKey>)>,
) {
    let mut global = existing_global;

    let mut writes = Vec::new();
    for (index, entry) in atoms.iter().enumerate() {
        if !condition_met(&entry.atom.condition, entry, layout) {
            continue;
        }
        match entry.atom.phase {
            Phase::StateWrite => {
                if let Some((key, delta, from)) =
                    apply_state_write(&mut global, &entry.atom, entry, layout)
                {
                    if record_writes {
                        writes.push((index, key, delta, from));
                    }
                }
            }
            _ => {
                // 其他 Phase（GlobalInject, Constant 等）暂不由本层处理
                // GlobalInject 仍由 control/interpreter.rs 管理
                // Constant/EffVar 等对跨设施场景无意义
            }
        }
    }

    (
        GlobalResourceSnapshot {
            global,
            inject: layout.global_inject.clone(),
            layout: layout.clone(),
        },
        writes,
    )
}

/// 求值 Condition（仅处理在跨设施上下文中可用的条件）。
fn condition_met(
    cond: &Option<crate::types::Condition>,
    _entry: &GlobalAtomEntry,
    _layout: &LayoutContext,
) -> bool {
    let Some(cond) = cond else {
        return true;
    };
    match cond {
        Condition::MoodAbove { n: _ } => true, // 跨设施编排暂不关心心情，视为满足
        Condition::MoodAboveOrEq { n: _ } => true,
        Condition::MoodBelow { n: _ } => false,
        Condition::MoodBelowOrEq { n: _ } => false,
        Condition::PartnerInRoom { name: _ } => true, // scope=Global 的跨设施 atom 不依赖同房条件
        _ => true,                                    // 其他条件视为满足（跨设施场景简化处理）
    }
}

/// 执行 `StateWrite` action。
fn apply_state_write(
    global: &mut GlobalResourcePool,
    atom: &crate::types::EffectAtom,
    entry: &GlobalAtomEntry,
    layout: &LayoutContext,
) -> Option<(StateKey, f64, Option<StateKey>)> {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            let Some(sk) = StateKey::parse(key) else {
                return None;
            };
            let scale = resolve_selector_value(&atom.selector, entry, layout);
            let add = if atom.selector.is_some() {
                scale * amount
            } else {
                *amount
            };
            if add != 0.0 {
                global.add(sk, add);
                return Some((sk, add, None));
            }
        }
        Action::StateConvert { from, to, ratio } => {
            let Some(from_k) = StateKey::parse(from) else {
                return None;
            };
            let Some(to_k) = StateKey::parse(to) else {
                return None;
            };
            let src = global.get(from_k);
            if src > 0.0 {
                let converted = src * ratio;
                global.add(to_k, converted);
                if converted != 0.0 {
                    return Some((to_k, converted, Some(from_k)));
                }
            }
        }
        Action::AddFlatEff { value: _, .. } => {
            // scope=Global 的 AddFlatEff 仅在跨设施上下文中用于外部记账
            // 目前无实际用例，忽略
        }
        _ => {}
    }
    None
}

/// 求值 Selector（跨设施上下文：取 layout 级统计数据或 operator 属性）。
fn resolve_selector_value(
    selector: &Option<crate::types::Selector>,
    entry: &GlobalAtomEntry,
    layout: &LayoutContext,
) -> f64 {
    let Some(sel) = selector else {
        return 1.0;
    };
    match sel {
        Selector::DormOccupantCount => f64::from(layout.dorm_occupant_count),
        Selector::FacilityLevel => f64::from(entry.room_level),
        Selector::FacilityLevelMinusOne => f64::from(entry.room_level.saturating_sub(1)),
        Selector::RoomOperatorCount => 1.0, // scope=Global 时取 1（自身分子已包含在 amount）
        Selector::TradeStationCount => f64::from(layout.trade_station_count),
        Selector::PowerStationCount => f64::from(layout.power_station_count),
        Selector::DormLevelSum => f64::from(layout.dorm_level_sum),
        Selector::MeetingMaxLevel => f64::from(layout.meeting_max_level),
        Selector::SuiFacilityCount => f64::from(layout.sui_facility_count),
        Selector::EliteFacilityCount => f64::from(layout.elite_facility_count),
        Selector::TaggedCountInRoom { tag }
            // scope=Global 时，取该干员自身的 tag 匹配
            if entry.owner_tags.iter().any(|t| t == tag) => {
                1.0
            }
        // 以下 Selector 在跨设施上下文中不可用或恒为 0
        _ => 0.0,
    }
}
