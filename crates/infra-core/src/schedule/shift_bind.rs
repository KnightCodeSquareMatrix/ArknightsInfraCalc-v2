//! 班次绑定：指定干员组在 αβγ 轮换中 **同上同下**（上 N 休 M）。
//!
//! 消费方：`schedule_team_rotation`（非编排层、非 global effect）。

use std::collections::HashSet;

use crate::layout::{BaseAssignment, RoomId};
use crate::operbox::OperBox;

use super::team_rotation::{FacilityHalf, TeamLabel, TeamRotationReport};

/// 一组须同队、同休的轮换干员。
#[derive(Debug, Clone, Copy)]
pub struct ShiftBindDef {
    pub id: &'static str,
    pub operators: &'static [&'static str],
    /// αβγ 周期内上岗班次数（当前固定 12h+6h+6h → 2）。
    pub on_shifts: u8,
    pub off_shifts: u8,
}

pub const ROSEMARY_BLACKKEY_BIND: ShiftBindDef = ShiftBindDef {
    id: "rosemary_blackkey",
    operators: &["迷迭香", "黑键"],
    on_shifts: 2,
    off_shifts: 1,
};

const ALL_BINDS: &[ShiftBindDef] = &[ROSEMARY_BLACKKEY_BIND];

/// operbox 满足全部成员时激活的绑定。
pub fn active_shift_binds(operbox: &OperBox) -> Vec<&'static ShiftBindDef> {
    ALL_BINDS
        .iter()
        .filter(|b| b.operators.iter().all(|name| operbox.owns(name)))
        .collect()
}

fn room_for_operator(assignment: &BaseAssignment, name: &str) -> Option<RoomId> {
    assignment
        .rooms
        .iter()
        .find(|r| r.operators.iter().any(|o| o.name == name))
        .map(|r| r.room_id.clone())
}

fn half_contains_room(half: &FacilityHalf, room: &RoomId) -> bool {
    half.trade.iter().any(|r| r == room)
        || half.manu.iter().any(|r| r == room)
        || half.power.iter().any(|r| r == room)
}

/// 绑定组跨 H1/H2 时，将 wanderer 所在房间换到 anchor 所在半区。
pub fn align_shift_binds_in_halves(
    peak: &BaseAssignment,
    operbox: &OperBox,
    h1: &mut FacilityHalf,
    h2: &mut FacilityHalf,
) {
    for bind in active_shift_binds(operbox) {
        if bind.operators.len() < 2 {
            continue;
        }
        let rooms: Vec<RoomId> = bind
            .operators
            .iter()
            .filter_map(|name| room_for_operator(peak, name))
            .collect();
        if rooms.len() < bind.operators.len() {
            continue;
        }
        let anchor = &rooms[0];
        let anchor_in_h1 = half_contains_room(h1, anchor);
        for wanderer in &rooms[1..] {
            let wanderer_in_h1 = half_contains_room(h1, wanderer);
            if wanderer_in_h1 == anchor_in_h1 {
                continue;
            }
            let _ = swap_room_across_halves(h1, h2, anchor_in_h1, anchor, wanderer);
        }
    }
}

fn swap_room_across_halves(
    h1: &mut FacilityHalf,
    h2: &mut FacilityHalf,
    anchor_in_h1: bool,
    anchor: &RoomId,
    wanderer: &RoomId,
) -> bool {
    let (target, source) = if anchor_in_h1 { (h1, h2) } else { (h2, h1) };

    if source.trade.iter().any(|r| r == wanderer) && target.trade.iter().any(|r| r == anchor) {
        let Some(wi) = source.trade.iter().position(|r| r == wanderer) else {
            return false;
        };
        let ti = target
            .trade
            .iter()
            .position(|r| r != anchor)
            .unwrap_or(0)
            .min(target.trade.len().saturating_sub(1));
        let w = source.trade.remove(wi);
        let t = target.trade.remove(ti);
        target.trade.push(w);
        source.trade.push(t);
        return true;
    }
    if source.manu.iter().any(|r| r == wanderer) && target.manu.iter().any(|r| r == anchor) {
        let Some(wi) = source.manu.iter().position(|r| r == wanderer) else {
            return false;
        };
        let ti = target
            .manu
            .iter()
            .position(|r| r != anchor)
            .unwrap_or(0)
            .min(target.manu.len().saturating_sub(1));
        let w = source.manu.remove(wi);
        let t = target.manu.remove(ti);
        target.manu.push(w);
        source.manu.push(t);
        return true;
    }
    if source.power.iter().any(|r| r == wanderer) && target.power.iter().any(|r| r == anchor) {
        let Some(wi) = source.power.iter().position(|r| r == wanderer) else {
            return false;
        };
        let ti = target
            .power
            .iter()
            .position(|r| r != anchor)
            .unwrap_or(0)
            .min(target.power.len().saturating_sub(1));
        let w = source.power.remove(wi);
        let t = target.power.remove(ti);
        target.power.push(w);
        source.power.push(t);
        return true;
    }
    false
}

/// 某队在 αβγ 周期内休息的班次下标（0=12h，1/2=6h）。
pub fn resting_shift_index(team: TeamLabel) -> usize {
    match team {
        TeamLabel::Gamma => 0,
        TeamLabel::Alpha => 1,
        TeamLabel::Beta => 2,
    }
}

pub fn team_of_operator(report: &TeamRotationReport, name: &str) -> Option<TeamLabel> {
    report
        .teams
        .iter()
        .find(|t| t.operators.iter().any(|o| o == name))
        .map(|t| t.label)
}

pub fn operator_in_shift(report: &TeamRotationReport, shift_idx: usize, name: &str) -> bool {
    report.shifts[shift_idx]
        .assignment
        .rooms
        .iter()
        .flat_map(|r| r.operators.iter())
        .any(|o| o.name == name)
}

/// 验证绑定组：同上同下 + 上2休1。
pub fn verify_shift_binds(
    report: &TeamRotationReport,
    operbox: &OperBox,
    peak: &BaseAssignment,
) -> Result<(), String> {
    for bind in active_shift_binds(operbox) {
        let present: Vec<&str> = bind
            .operators
            .iter()
            .copied()
            .filter(|name| room_for_operator(peak, name).is_some())
            .collect();
        if present.len() < 2 {
            continue;
        }
        for shift in &report.shifts {
            let flags: Vec<bool> = present
                .iter()
                .map(|name| operator_in_shift(report, shift.index, name))
                .collect();
            let all_in = flags.iter().all(|&b| b);
            let all_out = flags.iter().all(|&b| !b);
            if !all_in && !all_out {
                return Err(format!(
                    "{}: shift{} 绑定组 {:?} 未同上同下",
                    bind.id,
                    shift.index + 1,
                    present
                ));
            }
        }
        let team = team_of_operator(report, present[0])
            .ok_or_else(|| format!("{}: 未找到 {} 所属队", bind.id, present[0]))?;
        let rest = resting_shift_index(team);
        for name in &present {
            if operator_in_shift(report, rest, name) {
                return Err(format!(
                    "{}: {} 应在休息班 shift{} 缺席",
                    bind.id,
                    name,
                    rest + 1
                ));
            }
        }
        let active = report
            .shifts
            .iter()
            .filter(|s| present.iter().all(|n| operator_in_shift(report, s.index, n)))
            .count();
        if active != bind.on_shifts as usize {
            return Err(format!(
                "{}: 期望上岗 {} 班，实际 {}",
                bind.id, bind.on_shifts, active
            ));
        }
    }
    Ok(())
}

pub fn bound_operator_names(operbox: &OperBox, peak: &BaseAssignment) -> HashSet<String> {
    let mut names = HashSet::new();
    for bind in active_shift_binds(operbox) {
        let all_in_peak = bind
            .operators
            .iter()
            .all(|n| room_for_operator(peak, n).is_some());
        if all_in_peak {
            for n in bind.operators {
                names.insert((*n).to_string());
            }
        }
    }
    names
}
