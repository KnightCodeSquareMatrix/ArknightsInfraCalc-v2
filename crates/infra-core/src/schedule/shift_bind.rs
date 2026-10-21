//! 班次绑定：指定干员组在 αβγ 轮换中 **同上同下**（上 N 休 M）。
//!
//! 绑定来源：统一 `AssignmentPlan.shift_binds`（由声明式规则编译器产出）。
//! 消费方：`schedule_team_rotation`（非编排层、非 global effect）。

use std::collections::HashSet;

use crate::layout::{AssignmentPlan, BaseAssignment, BaseBlueprint, FacilityKind, RoomId};

use super::rotation_types::{FacilityHalf, RuntimeShiftBind, TeamLabel, TeamRotationReport};

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
            .filter_map(|name| peak.room_of_operator(name).map(|r| r.room_id.clone()))
            .collect();
        if rooms.len() < bind.operators.len() {
            continue;
        }
        let Some(anchor) = rooms
            .iter()
            .find(|room| h1.contains(room) || h2.contains(room))
        else {
            continue;
        };
        let anchor_in_h1 = h1.contains(anchor);
        for wanderer in &rooms {
            if wanderer == anchor || (!h1.contains(wanderer) && !h2.contains(wanderer)) {
                continue;
            }
            let wanderer_in_h1 = h1.contains(wanderer);
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
    report.shifts[shift_idx].assignment.has_operator(name)
}

fn bind_required_support<'a>(
    plan: &'a AssignmentPlan,
    bind: &'a RuntimeShiftBind,
) -> HashSet<&'a str> {
    plan.active_dependencies
        .iter()
        .filter(|dependency| {
            dependency
                .consumers
                .iter()
                .all(|consumer| bind.operators.contains(consumer))
        })
        .flat_map(|dependency| dependency.required.iter().map(String::as_str))
        .collect()
}

fn plan_dorm_support(plan: &AssignmentPlan) -> HashSet<&str> {
    plan.anchors
        .iter()
        .filter(|anchor| anchor.facility == FacilityKind::Dormitory)
        .map(|anchor| anchor.operator.as_str())
        .chain(
            plan.registry_claims
                .iter()
                .flat_map(|claim| &claim.slots)
                .filter(|slot| slot.facility == FacilityKind::Dormitory)
                .flat_map(|slot| slot.operators.iter().map(|operator| operator.name.as_str())),
        )
        .collect()
}

fn assignment_dorm_presence<'a>(
    blueprint: &'a BaseBlueprint,
    assignment: &'a BaseAssignment,
) -> HashSet<&'a str> {
    assignment
        .iter_rooms()
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|room| room.kind == FacilityKind::Dormitory)
        })
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.as_str()))
        .collect()
}

fn bind_work_operators<'a>(plan: &'a AssignmentPlan, bind: &'a RuntimeShiftBind) -> Vec<&'a str> {
    let required = bind_required_support(plan, bind);
    bind.operators
        .iter()
        .map(String::as_str)
        .filter(|name| !required.contains(name))
        .collect()
}

fn operator_is_bind_work(
    blueprint: &BaseBlueprint,
    plan: &AssignmentPlan,
    assignment: &BaseAssignment,
    work_names: &HashSet<String>,
    name: &str,
) -> bool {
    if !work_names.contains(name) {
        return false;
    }
    let Some(room) = assignment.room_of_operator(name) else {
        return true;
    };
    let Some(room_kind) = blueprint
        .room(&room.room_id)
        .map(|blueprint_room| blueprint_room.kind)
    else {
        return true;
    };
    !plan.anchors.iter().any(|anchor| {
        anchor.operator == name
            && anchor
                .rest_facility
                .is_some_and(|rest_facility| rest_facility == room_kind)
    })
}

fn shift_plan<'a>(
    report: &'a TeamRotationReport,
    shift_index: usize,
) -> Result<&'a AssignmentPlan, String> {
    let shift = report
        .shifts
        .get(shift_index)
        .ok_or_else(|| "rotation_bind: missing shift".to_string())?;
    report
        .assignment_plans
        .get(shift.plan_index)
        .ok_or_else(|| {
            format!(
                "rotation_bind: shift {} lost its owning Plan",
                shift.index + 1
            )
        })
}

fn operator_bind_work_states(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
    source_plan: &AssignmentPlan,
    required: &HashSet<&str>,
    name: &str,
) -> Result<Vec<bool>, String> {
    report
        .shifts
        .iter()
        .map(|shift| {
            if required.contains(name) {
                return Ok(false);
            }
            let work_names = shift.assignment.work_position_operator_names(blueprint);
            Ok(operator_is_bind_work(
                blueprint,
                source_plan,
                &shift.assignment,
                &work_names,
                name,
            ))
        })
        .collect()
}

fn bind_core_operators(plan: &AssignmentPlan, bind: &RuntimeShiftBind) -> Vec<String> {
    let dorm_support = plan_dorm_support(plan);
    let mut core: Vec<_> = bind_work_operators(plan, bind)
        .into_iter()
        .filter(|name| !dorm_support.contains(name))
        .map(str::to_string)
        .collect();
    core.sort();
    core
}

fn bind_dorm_support<'a>(
    plan: &'a AssignmentPlan,
    bind: &'a RuntimeShiftBind,
    core: &[String],
    required: &HashSet<&'a str>,
) -> Vec<&'a str> {
    let dorm_support = plan_dorm_support(plan);
    let mut support: Vec<_> = bind
        .operators
        .iter()
        .map(String::as_str)
        .filter(|name| dorm_support.contains(name))
        .filter(|name| !required.contains(name))
        .collect();
    support.sort();
    support.dedup();
    if core.is_empty() {
        Vec::new()
    } else {
        support
    }
}

fn validate_bind_dependency_presence(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
) -> Result<(), String> {
    for (index, shift) in report.shifts.iter().enumerate() {
        let plan = shift_plan(report, index)?;
        let work_names = shift.assignment.work_position_operator_names(blueprint);
        let present = shift.assignment.operator_names();
        for dependency in &plan.active_dependencies {
            let consumer_working = dependency.consumers.iter().any(|consumer| {
                operator_is_bind_work(blueprint, plan, &shift.assignment, &work_names, consumer)
            });
            if !consumer_working {
                continue;
            }
            let missing: Vec<_> = dependency
                .required
                .iter()
                .filter(|required| !present.contains(*required))
                .collect();
            if !missing.is_empty() {
                return Err(format!(
                    "rotation_bind: shift {} active dependency {:?} missing required {:?}",
                    shift.index + 1,
                    dependency.consumers,
                    missing
                ));
            }
        }
    }
    Ok(())
}

/// Final-state bind evidence shared by normal finalization and `--assert-invariants`.
/// Required supports from the same Plan are checked for presence while their consumer
/// works; they stay outside the work-member topology so dorm/support duty is not split.
pub(crate) fn validate_final_bind_cycle(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
) -> Result<(), String> {
    let abc = report.profile.is_ordinary_abc();
    let mut source_binds = Vec::new();
    for (plan_index, plan) in report.assignment_plans.iter().enumerate() {
        for bind in shift_binds_from_plan(plan) {
            source_binds.push((plan_index, plan, bind));
        }
    }
    for bind in report.runtime_shift_binds.iter() {
        if !source_binds
            .iter()
            .any(|(_, _, existing)| existing.operators == bind.operators)
        {
            let peak_index = report
                .assignment_plans
                .iter()
                .position(|plan| std::ptr::eq(plan, &report.peak_plan))
                .unwrap_or(0);
            source_binds.push((peak_index, &report.peak_plan, bind.clone()));
        }
    }
    source_binds.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.2.operators.cmp(&right.2.operators))
    });
    for (_, source_plan, bind) in source_binds {
        let core = bind_core_operators(source_plan, &bind);
        let required = bind_required_support(source_plan, &bind);
        let dorm_support = bind_dorm_support(source_plan, &bind, &core, &required);
        let states: Vec<Vec<bool>> = core
            .iter()
            .map(|name| operator_bind_work_states(blueprint, report, source_plan, &required, name))
            .collect::<Result<_, _>>()?;
        if core.len() >= 2 {
            for (index, shift) in report.shifts.iter().enumerate() {
                let flags: Vec<_> = states.iter().map(|state| state[index]).collect();
                if flags
                    .first()
                    .is_some_and(|first| flags.iter().any(|flag| flag != first))
                {
                    return Err(format!(
                        "rotation_bind: shift {} bind {:?} does not move together",
                        shift.index + 1,
                        core
                    ));
                }
            }
        }
        if abc && core.len() >= 2 {
            let active = report
                .shifts
                .iter()
                .enumerate()
                .filter(|(index, _)| states.iter().all(|state| state[*index]))
                .count();
            let inactive = report.shifts.len().saturating_sub(active);
            if bind.on_shifts > 0 && active != usize::from(bind.on_shifts) {
                return Err(format!(
                    "rotation_bind: bind {:?} expected {} active shifts, got {active}",
                    core, bind.on_shifts
                ));
            }
            if bind.off_shifts > 0 && inactive != usize::from(bind.off_shifts) {
                return Err(format!(
                    "rotation_bind: bind {:?} expected {} resting shifts, got {inactive}",
                    core, bind.off_shifts
                ));
            }
        }
        if !abc && core.len() >= 2 {
            let teams: Vec<_> = report
                .teams
                .iter()
                .filter(|team| {
                    core.iter()
                        .all(|operator| team.operators.contains(operator))
                })
                .collect();
            let team = match teams.as_slice() {
                [single] => *single,
                [] => {
                    return Err(format!(
                        "rotation_bind: bind {:?} belongs to no complete team",
                        core
                    ));
                }
                _ => {
                    return Err(format!(
                        "rotation_bind: bind {:?} belongs to multiple complete teams",
                        core
                    ));
                }
            };
            for (index, shift) in report.shifts.iter().enumerate() {
                let expected = shift.active_teams.contains(&team.label);
                for (operator, state) in core.iter().zip(&states) {
                    let actual = state[index];
                    if actual != expected {
                        return Err(format!(
                            "rotation_bind: profile bind {} work {:?} does not match team {:?} topology",
                            operator, actual, team.label
                        ));
                    }
                }
            }
        }
        if !dorm_support.is_empty() {
            for shift in &report.shifts {
                let core_working = core
                    .iter()
                    .any(|name| shift.assignment.is_work_position_operator(blueprint, name));
                if !core_working {
                    continue;
                }
                let dorm_presence = assignment_dorm_presence(blueprint, &shift.assignment);
                let missing: Vec<_> = dorm_support
                    .iter()
                    .filter(|name| !dorm_presence.contains(**name))
                    .collect();
                if !missing.is_empty() {
                    return Err(format!(
                        "rotation_bind: shift {} dormitory support {:?} missing while bound core is working",
                        shift.index + 1,
                        missing
                    ));
                }
            }
        }
    }
    validate_bind_dependency_presence(blueprint, report)?;
    Ok(())
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
            .filter(|name| peak.has_operator(name))
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
        // 宿舍常驻干员（如塑心）不属于任何轮换队，取第一个有队伍归属的在岗者代表绑定组。
        let team = present
            .iter()
            .find_map(|name| team_of_operator(report, name))
            .ok_or_else(|| format!("{}: 绑定组内无任何干员属于轮换队", label))?;
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
        let all_in_peak = bind.operators.iter().all(|n| peak.has_operator(n));
        if all_in_peak {
            for n in &bind.operators {
                names.insert(n.clone());
            }
        }
    }
    names
}
