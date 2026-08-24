//! 班次绑定：指定干员组在 αβγ 轮换中 **同上同下**（上 N 休 M）。
//!
//! 绑定来源：统一 `AssignmentPlan.shift_binds`（由声明式规则编译器产出）。
//! 消费方：`schedule_team_rotation`（非编排层、非 global effect）。

use std::collections::HashSet;

use crate::layout::{AssignmentPlan, BaseAssignment, RoomId};

use super::team_rotation::{FacilityHalf, TeamLabel, TeamRotationReport};

/// 一组须同队、同休的轮换干员（运行期，来自统一 plan）。
#[derive(Debug, Clone)]
pub struct RuntimeShiftBind {
    pub operators: Vec<String>,
    /// αβγ 周期内上岗班次数（当前固定 12h+6h+6h → 2）。
    pub on_shifts: u8,
    pub off_shifts: u8,
}

/// 从统一 plan 提取运行期绑定组。plan 的 shift_binds 仅在体系激活时产出，已隐含 ownership gate。
pub fn shift_binds_from_plan(plan: &AssignmentPlan) -> Vec<RuntimeShiftBind> {
    let mut raw: Vec<_> = plan
        .resolved_producer_dependencies
        .iter()
        .filter(|dependency| {
            dependency.relation
                == crate::response_dependency::ScheduleDependencyRelation::ExactPresence
        })
        .filter_map(|dependency| {
            let mut operators = dependency.producers.clone();
            operators.extend(dependency.consumers.iter().cloned());
            operators.sort();
            operators.dedup();
            (operators.len() > 1).then_some(RuntimeShiftBind {
                operators,
                on_shifts: dependency.on_shifts,
                off_shifts: dependency.off_shifts,
            })
        })
        .collect();
    for bind in &plan.shift_binds {
        let mut operators = bind.operators.clone();
        operators.sort();
        operators.dedup();
        if operators.len() < 2 {
            continue;
        }
        raw.push(RuntimeShiftBind {
            operators,
            on_shifts: bind.on_shifts,
            off_shifts: bind.off_shifts,
        });
    }

    // Exact-presence dependencies and explicit system binds may overlap. They describe
    // one same-team closure, not competing teams, so merge connected operator sets.
    let mut normalized: Vec<RuntimeShiftBind> = Vec::new();
    for bind in raw {
        let mut merged = bind;
        let mut index = 0;
        while index < normalized.len() {
            if normalized[index]
                .operators
                .iter()
                .any(|operator| merged.operators.contains(operator))
            {
                let existing = normalized.remove(index);
                merged.operators.extend(existing.operators);
                merged.operators.sort();
                merged.operators.dedup();
                continue;
            }
            index += 1;
        }
        normalized.push(merged);
    }
    normalized
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
    binds: &[RuntimeShiftBind],
    h1: &mut FacilityHalf,
    h2: &mut FacilityHalf,
) {
    for bind in binds {
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
        let Some(anchor) = rooms
            .iter()
            .find(|room| half_contains_room(h1, room) || half_contains_room(h2, room))
        else {
            continue;
        };
        let anchor_in_h1 = half_contains_room(h1, anchor);
        for wanderer in &rooms {
            if wanderer == anchor
                || (!half_contains_room(h1, wanderer) && !half_contains_room(h2, wanderer))
            {
                continue;
            }
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

    if source.trade.iter().any(|r| r == wanderer) && !target.trade.is_empty() {
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
    if source.manu.iter().any(|r| r == wanderer) && !target.manu.is_empty() {
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
    if source.power.iter().any(|r| r == wanderer) && !target.power.is_empty() {
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
    binds: &[RuntimeShiftBind],
    peak: &BaseAssignment,
) -> Result<(), String> {
    for bind in binds {
        let label = bind.operators.join("+");
        let present: Vec<&str> = bind
            .operators
            .iter()
            .map(String::as_str)
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
                    label,
                    shift.index + 1,
                    present
                ));
            }
        }
        let team = team_of_operator(report, present[0])
            .ok_or_else(|| format!("{}: 未找到 {} 所属队", label, present[0]))?;
        if bind.off_shifts > 0 {
            let rest = resting_shift_index(team);
            for name in &present {
                if operator_in_shift(report, rest, name) {
                    return Err(format!(
                        "{}: {} 应在休息班 shift{} 缺席",
                        label,
                        name,
                        rest + 1
                    ));
                }
            }
        }
        let active = report
            .shifts
            .iter()
            .filter(|s| {
                present
                    .iter()
                    .all(|n| operator_in_shift(report, s.index, n))
            })
            .count();
        if bind.on_shifts > 0 && active != bind.on_shifts as usize {
            return Err(format!(
                "{}: 期望上岗 {} 班，实际 {}",
                label, bind.on_shifts, active
            ));
        }
    }
    Ok(())
}

pub fn bound_operator_names(binds: &[RuntimeShiftBind], peak: &BaseAssignment) -> HashSet<String> {
    let mut names = HashSet::new();
    for bind in binds {
        let all_in_peak = bind
            .operators
            .iter()
            .all(|n| room_for_operator(peak, n).is_some());
        if all_in_peak {
            for n in &bind.operators {
                names.insert(n.clone());
            }
        }
    }
    names
}
