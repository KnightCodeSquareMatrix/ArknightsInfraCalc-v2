use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment, RoomEfficiencySnapshot};
use crate::layout::blueprint::RoomId;
use crate::pool::{ManuPool, TradePool};
use crate::search::{ManuSearchHit, TradeSearchHit};

pub(super) fn commit_trade_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    hit: &TradeSearchHit,
    pool: &TradePool,
    used: &mut HashSet<String>,
) -> Result<()> {
    let anchors: HashSet<String> = assignment
        .operators_in(room_id)
        .iter()
        .map(|op| op.name.clone())
        .collect();
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
        &anchors,
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
    let anchors: HashSet<String> = assignment
        .operators_in(room_id)
        .iter()
        .map(|op| op.name.clone())
        .collect();
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
        &anchors,
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
    let breakdown = hit
        .breakdown
        .as_ref()
        .expect("room assignment requires a single-station trade hit");
    RoomEfficiencySnapshot {
        trade_paper_efficiency: breakdown.paper_efficiency,
        trade_occupancy_efficiency: breakdown.occupancy_efficiency,
        trade_control_efficiency: breakdown.control_efficiency,
        trade_unit_output_multiplier: breakdown.unit_output_multiplier,
        trade_final_efficiency: breakdown.final_efficiency,
        trade_equivalent_skill_efficiency: breakdown.equivalent_skill_efficiency,
        trade_rule_id: breakdown.rule_id.clone(),
        trade_skill_efficiency: breakdown.skill_efficiency,
        trade_mechanic_equivalent_efficiency: breakdown.mechanic_equivalent_efficiency,
        ..RoomEfficiencySnapshot::default()
    }
}

pub(super) fn manu_efficiency_snapshot(hit: &ManuSearchHit) -> RoomEfficiencySnapshot {
    RoomEfficiencySnapshot {
        manufacture_final_efficiency: hit.breakdown.final_efficiency,
        manufacture_occupancy_efficiency: hit.breakdown.occupancy_efficiency,
        manufacture_global_efficiency: hit.breakdown.global_efficiency,
        manufacture_skill_efficiency: hit.breakdown.skill_efficiency,
        manufacture_storage_limit: hit.breakdown.storage_limit,
        ..RoomEfficiencySnapshot::default()
    }
}

pub(super) fn power_efficiency_snapshot(
    hit: &crate::search::PowerSearchHit,
) -> RoomEfficiencySnapshot {
    RoomEfficiencySnapshot {
        power_final_efficiency: hit.final_efficiency,
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
    anchors: &HashSet<String>,
    facility: &str,
    efficiency: Option<RoomEfficiencySnapshot>,
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
    assignment.set_room_with_efficiency(room_id.clone(), ops, efficiency);
    Ok(())
}
