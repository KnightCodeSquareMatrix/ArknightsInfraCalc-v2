use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment, RoomEfficiencySnapshot};
use crate::layout::blueprint::RoomId;
use crate::pool::{ManuPool, TradePool};
use crate::search::{ManuSearchHit, TradeSearchHit};

pub(super) fn names_disjoint_except(names: &[String], used_wo: &HashSet<String>) -> bool {
    names.iter().all(|n| !used_wo.contains(n))
}

pub(super) fn commit_anchor_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    operator_of: impl Fn(&str) -> AssignedOperator,
    used: &mut HashSet<String>,
    anchors: &[String],
    facility: &str,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !anchors.contains(name) && !used.insert(name.clone()) {
                return Err(Error::msg(format!("{facility} duplicate {name}")));
            }
            Ok(operator_of(name))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), ops);
    Ok(())
}

pub(super) fn commit_trade_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    hit: &TradeSearchHit,
    pool: &TradePool,
    used: &mut HashSet<String>,
) -> Result<()> {
    commit_operators_to_room(
        assignment,
        room_id,
        trade_hit_names(hit),
        |name| {
            pool.entry(name)
                .map(|e| AssignedOperator::from_progress(name, e.progress))
                .unwrap_or_else(|| AssignedOperator::new(name, 0))
        },
        used,
        "trade",
        Some(trade_efficiency_snapshot(hit)),
    )
}

pub(super) fn commit_manu_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    hit: &ManuSearchHit,
    pool: &ManuPool,
    used: &mut HashSet<String>,
) -> Result<()> {
    commit_operators_to_room(
        assignment,
        room_id,
        manu_hit_names(hit),
        |name| {
            pool.entry(name)
                .map(|e| AssignedOperator::from_progress(name, e.progress))
                .unwrap_or_else(|| AssignedOperator::new(name, 0))
        },
        used,
        "manufacture",
        Some(manu_efficiency_snapshot(hit)),
    )
}

pub(super) fn names_disjoint(names: &[String], used: &HashSet<String>) -> bool {
    names.iter().all(|n| !used.contains(n))
}

pub(super) fn first_nonempty_names<'a>(
    a: &'a [String],
    b: &'a [String],
    c: &'a [String],
) -> &'a [String] {
    if !a.is_empty() {
        a
    } else if !b.is_empty() {
        b
    } else {
        c
    }
}

pub(super) fn trade_hit_names(hit: &TradeSearchHit) -> &[String] {
    first_nonempty_names(&hit.names, &hit.gold_names, &hit.originium_names)
}

pub(super) fn manu_hit_names(hit: &ManuSearchHit) -> &[String] {
    first_nonempty_names(&hit.names, &hit.gold_names, &hit.battle_record_names)
}

pub(super) fn trade_efficiency_snapshot(hit: &TradeSearchHit) -> RoomEfficiencySnapshot {
    RoomEfficiencySnapshot {
        trade_score: hit.breakdown.effective_eff_multiplier,
        trade_pct: hit.trade_pct,
        trade_skill_pct: hit.breakdown.order_eff_skill,
        trade_gold_pct: hit.gold_pct,
        ..RoomEfficiencySnapshot::default()
    }
}

pub(super) fn manu_efficiency_snapshot(hit: &ManuSearchHit) -> RoomEfficiencySnapshot {
    RoomEfficiencySnapshot {
        manu_prod_total: hit.breakdown.prod_total,
        manu_prod_skill: hit.breakdown.prod_skill,
        manu_storage_limit: hit.breakdown.storage_limit,
        ..RoomEfficiencySnapshot::default()
    }
}

pub(super) fn power_efficiency_snapshot(
    hit: &crate::search::PowerSearchHit,
) -> RoomEfficiencySnapshot {
    RoomEfficiencySnapshot {
        power_charge_speed_pct: hit.charge_speed_pct,
        ..RoomEfficiencySnapshot::default()
    }
}

pub(super) fn pick_first_disjoint<T: Clone>(
    hits: impl IntoIterator<Item = T>,
    names_of: &impl Fn(&T) -> &[String],
    used: &HashSet<String>,
) -> Option<T> {
    hits.into_iter().find(|h| names_disjoint(names_of(h), used))
}

pub(super) fn pick_disjoint_from_report<T: Clone>(
    best: T,
    top: Vec<T>,
    names_of: impl Fn(&T) -> &[String],
    used: &HashSet<String>,
    err: &str,
) -> Result<T> {
    pick_first_disjoint(
        top.into_iter().chain(std::iter::once(best)),
        &names_of,
        used,
    )
    .ok_or_else(|| Error::msg(err))
}

fn commit_operators_to_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    operator_of: impl Fn(&str) -> AssignedOperator,
    used: &mut HashSet<String>,
    facility: &str,
    efficiency: Option<RoomEfficiencySnapshot>,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !used.insert(name.clone()) {
                return Err(Error::msg(format!("{facility} duplicate {name}")));
            }
            Ok(operator_of(name))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room_with_efficiency(room_id.clone(), ops, efficiency);
    Ok(())
}
