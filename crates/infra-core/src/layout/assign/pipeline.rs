//! 单班填房流水线主体（ADR 0001 决策 A）。
//!
//! 阶段顺序与 resolve 时机的唯一事实源。facade（`assign.rs`）只负责构建
//! `skip_system_ids` 与 `build_plan`，随后委托给本模块。
//! Peak 会从同一 execute seed 比较普通中枢与可选动态贸易 producer 的完整生产前缀；
//! 每个贸易站在前站提交后重新 resolve，制造只消费胜出的前缀。

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::orchestrate::{execute_plan, AssignmentPlan};
use crate::layout::shift::AssignShiftMode;
use crate::operbox::OperBox;
use crate::pool::{
    add_jie_market_to_trade_pool, build_control_pool_with_fillers, build_manufacture_pool,
    build_power_pool, build_trade_pool, karlan_precision_active, ControlPool, PowerPool, TradePool,
};
use crate::scoring::{evaluate_trade_manufacture_weighted_efficiency, ScoringPolicyId};
use crate::search::{
    control_entry_deferred_trade_tags, control_inject_policy_sort_key_for_assignment,
    control_inject_policy_sort_key_upper_bound,
};
use crate::skill_table::SkillTable;

use super::control_fill::{assign_control, assign_control_matching_dynamic_set};
use super::manufacture_fill::{assign_manufacture_lines, refresh_manufacture_efficiency_snapshots};
use super::power_fill::assign_power_stations;
use super::producer_fill::{
    assign_dorm_producers, assign_sphinx_urrbian_dorm_anchor,
    cleanup_unused_sphinx_urrbian_dorm_anchor, place_system_producers,
};
use super::run::{AssignmentRun, StageTimer};
use super::trade_fill::{
    assign_trade_jie_remainder, assign_trade_remainder, refresh_trade_efficiency_snapshots,
};
use super::{inject_search_anchor_pool_entries, tag_pool_from_plan, AssignBaseOptions};

/// 执行单班填房流水线，返回最终编制。
///
/// `plan` 已由调用方通过 `build_plan` 构造（含 trade core registry skip 规则）。
#[allow(clippy::too_many_arguments)]
pub(super) fn run_shift_pipeline(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    plan: &AssignmentPlan,
) -> Result<BaseAssignment> {
    run_shift_pipeline_with_producer_pruning(
        blueprint, operbox, instances, table, options, mode, seed, plan, true,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_shift_pipeline_with_producer_pruning(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    plan: &AssignmentPlan,
    enable_producer_pruning: bool,
) -> Result<BaseAssignment> {
    let enable_producer_pruning =
        producer_pruning_enabled(enable_producer_pruning, options.dynamic_producer_policy);
    let mut timer = StageTimer::new("单班");

    // 编排落位（execute_plan：fixed/anchor core → assignment + used）。
    let executed = execute_plan(blueprint, operbox, table, plan, seed)?;
    let mut run = AssignmentRun::new(
        blueprint,
        operbox,
        instances,
        table,
        options,
        executed.assignment,
        executed.used,
    );
    timer.mark("编排落位");

    // 建池（中枢 / 贸易 / 制造 / 发电）+ tier 标注 + anchor 注入。
    let mut control_pool = build_control_pool_with_fillers(operbox, instances, table)?;
    let mut trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let mut manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let mut power_pool = build_power_pool(&operbox.power_roster(instances), instances, table)?;
    let control_exclusions: HashSet<_> = plan.control_exclusions.iter().cloned().collect();
    tag_pool_from_plan(plan, &mut control_pool);
    tag_pool_from_plan(plan, &mut trade_pool);
    tag_pool_from_plan(plan, &mut manu_pool);
    tag_pool_from_plan(plan, &mut power_pool);
    inject_search_anchor_pool_entries(
        plan,
        operbox,
        instances,
        table,
        &mut trade_pool,
        &mut manu_pool,
    );
    let gold_lines = blueprint.gold_manu_line_count();
    // forbid-same-room 约束（迷迭香 ≠ 清流/温蒂同制造站）从统一 plan 提取，供 anchor 房搜索排除。
    let forbid_same_room = forbid_same_room_pairs(plan);
    timer.mark("建池");

    match mode {
        AssignShiftMode::Peak => {
            let dynamic_producers =
                dynamic_producer_candidates(&control_pool, &trade_pool, &manu_pool, table)?;
            if dynamic_producers.len() >= usize::BITS as usize {
                return Err(Error::msg(
                    "too many deferred producer presence combinations",
                ));
            }
            let all_dynamic: HashSet<_> = dynamic_producers.iter().cloned().collect();
            let producer_layout = run.resolve_snapshot(false)?;
            let mut prepared = Vec::new();
            for mask in 0..(1usize << dynamic_producers.len()) {
                let required: HashSet<_> = dynamic_producers
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1usize << index) != 0)
                    .map(|(_, producer)| producer.clone())
                    .collect();
                let excluded: HashSet<_> = all_dynamic.difference(&required).cloned().collect();
                if let Some(candidate) = prepare_peak_control_candidate(
                    run.clone(),
                    plan,
                    &control_pool,
                    &producer_layout,
                    &control_exclusions,
                    Some((&required, &excluded)),
                    mask,
                )? {
                    prepared.push(candidate);
                }
            }
            prepared.sort_by(|left, right| {
                right
                    .policy_upper_bound
                    .partial_cmp(&left.policy_upper_bound)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.stable_order.cmp(&right.stable_order))
            });
            timer.mark("中枢候选上界");
            let mut best = None;
            for prepared in prepared {
                if should_prune_peak_candidate(
                    enable_producer_pruning,
                    best.as_ref()
                        .map(|best: &PeakPrefixCandidate<'_>| best.policy_sort_key),
                    prepared.policy_upper_bound,
                ) {
                    continue;
                }
                let stable_order = prepared.stable_order;
                let Some(candidate) = finish_peak_prefix_candidate(
                    prepared.run,
                    plan,
                    &trade_pool,
                    &manu_pool,
                    &power_pool,
                    gold_lines,
                    &forbid_same_room,
                    stable_order,
                )?
                else {
                    continue;
                };
                if best.as_ref().is_none_or(|best: &PeakPrefixCandidate<'_>| {
                    candidate.policy_sort_key > best.policy_sort_key
                        || (candidate.policy_sort_key == best.policy_sort_key
                            && candidate.stable_order < best.stable_order)
                }) {
                    best = Some(candidate);
                }
            }
            run = best
                .ok_or_else(|| Error::msg("all peak prefix candidates are infeasible"))?
                .run;
            timer.mark("中枢-生产前缀候选");

            timer.mark("制造");
        }
        AssignShiftMode::Recovery => {
            let layout = run.resolve_snapshot(true)?;
            if karlan_precision_active(&layout.global_inject) {
                add_jie_market_to_trade_pool(&mut trade_pool, instances, table);
            }
            assign_trade_jie_remainder(
                blueprint,
                &trade_pool,
                table,
                instances,
                gold_lines,
                options,
                Some(run.durin_plan),
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("trade孑余站");
            assign_manufacture_lines(
                blueprint,
                operbox,
                instances,
                &manu_pool,
                table,
                &layout,
                options,
                &forbid_same_room,
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("制造");
            assign_power_stations(
                blueprint,
                &power_pool,
                table,
                &layout,
                options,
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("发电");
        }
    }

    refresh_manufacture_efficiency_snapshots(
        blueprint,
        &mut run.assignment,
        instances,
        table,
        options,
        Some(run.durin_plan),
    )?;
    refresh_trade_efficiency_snapshots(
        blueprint,
        &mut run.assignment,
        instances,
        table,
        options,
        Some(run.durin_plan),
    )?;

    assign_meeting_rooms(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &mut run.assignment,
        &mut run.used,
    )?;
    timer.mark("会客室");

    timer.report();
    Ok(run.assignment)
}

fn producer_pruning_enabled(requested: bool, policy: ScoringPolicyId) -> bool {
    requested && policy == ScoringPolicyId::ControlInjectRawSumV0
}

/// 用未占用干员填满所有会客室（每间两人）。
///
/// 选人顺序：
/// 1. 优先选择会客室线索搜集速度加成最高、且尚未被本班占用的干员（按
///    `evaluate_meeting` 的 `skill_speed_bonus_pct` 排序，搭档条件在两人一起
///    求值时自然生效）；
/// 2. 没有可用的会客室技能干员时，兜底选取任意空闲干员；
/// 3. 空余位置不足时保持空位（导出层按 `autofill` 提示）。
///
/// 本阶段只消费生产域分配后剩余的空闲干员，不进入任何生产域评分。
fn assign_meeting_rooms(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let registry = crate::support_facility::SupportRegistry::load_default()?;
    let extra_recruit_slots = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let elapsed_hours = options.shift_hours.max(1.0);
    let layout = crate::layout::resolve::resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?
    .layout_snapshot();

    let roster = operbox.roster();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::MeetingRoom)
    {
        let mut operators = assignment.operators_in(&room.id).to_vec();
        while operators.len() < room.operator_capacity() {
            let mut best: Option<(f64, String, crate::roster::OperatorProgress)> = None;
            for name in roster.names() {
                if used.contains(name.as_str()) {
                    continue;
                }
                if operators.iter().any(|op| op.name == *name) {
                    continue;
                }
                let Some(progress) = roster.progress(name) else {
                    continue;
                };
                // 会客室总是两人同房：候选评分用哨兵补足第二席，使
                // `SpeedIfSolo` 失效（solo 加成在双人房不生效），
                // `SpeedIfPartner` 搭档加成按哨兵不匹配记 0。
                let mut support_ops: Vec<_> = operators
                    .iter()
                    .map(|op| crate::support_facility::SupportOperator {
                        name: op.name.clone(),
                        elite: op.elite,
                        level: op.level,
                    })
                    .chain(std::iter::once(crate::support_facility::SupportOperator {
                        name: name.clone(),
                        elite: progress.elite,
                        level: progress.level,
                    }))
                    .collect();
                if support_ops.len() < room.operator_capacity() {
                    support_ops.push(crate::support_facility::SupportOperator {
                        name: "\0meeting-partner-sentinel".to_string(),
                        elite: 0,
                        level: 1,
                    });
                }
                let score = crate::meeting::evaluate_meeting(
                    &crate::support_facility::SupportRoomInput {
                        facility: crate::support_facility::SupportFacility::Meeting,
                        operators: support_ops,
                        capacity: room.operator_capacity(),
                        extra_recruit_slots,
                        elapsed_hours,
                        external_speed_bonus_pct: 0.0,
                        layout: layout.clone(),
                    },
                    &registry,
                )?
                .skill_speed_bonus_pct;
                if best.as_ref().is_none_or(|(best_score, best_name, _)| {
                    score > *best_score || (score == *best_score && *name < *best_name)
                }) {
                    best = Some((score, name.clone(), progress));
                }
            }
            let Some((_, name, progress)) = best else {
                break;
            };
            used.insert(name.clone());
            operators.push(crate::layout::AssignedOperator::from_progress(
                &name, progress,
            ));
        }
        if !operators.is_empty() {
            assignment.set_room(room.id.clone(), operators);
        }
    }
    Ok(())
}

struct PeakPrefixCandidate<'a> {
    run: AssignmentRun<'a>,
    policy_sort_key: f64,
    stable_order: usize,
}

struct PreparedPeakControlCandidate<'a> {
    run: AssignmentRun<'a>,
    policy_upper_bound: f64,
    stable_order: usize,
}

fn should_prune_peak_candidate(
    enabled: bool,
    best_policy_sort_key: Option<f64>,
    candidate_upper_bound: f64,
) -> bool {
    enabled
        && best_policy_sort_key
            .is_some_and(|best_policy_sort_key| best_policy_sort_key > candidate_upper_bound)
}

/// 从同一个 execute/producer seed 运行完整生产候选前缀。
/// control 的选择会改变 `used`、producer/dorm、power、贸易与制造上下文，因此这些阶段
/// 必须作为一个候选整体推进；轮换与导出只消费最终胜者。
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn run_peak_prefix_candidate<'a>(
    run: AssignmentRun<'a>,
    plan: &AssignmentPlan,
    control_pool: &ControlPool,
    base_trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    power_pool: &PowerPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
    dynamic_set: Option<(&HashSet<String>, &HashSet<String>)>,
) -> Result<Option<PeakPrefixCandidate<'a>>> {
    let producer_layout = run.resolve_snapshot(false)?;
    let control_exclusions: HashSet<_> = plan.control_exclusions.iter().cloned().collect();
    let Some(prepared) = prepare_peak_control_candidate(
        run,
        plan,
        control_pool,
        &producer_layout,
        &control_exclusions,
        dynamic_set,
        0,
    )?
    else {
        return Ok(None);
    };
    let stable_order = prepared.stable_order;
    finish_peak_prefix_candidate(
        prepared.run,
        plan,
        base_trade_pool,
        manu_pool,
        power_pool,
        gold_lines,
        forbid_same_room,
        stable_order,
    )
}

fn control_layout_for_upper_bound(
    run: &AssignmentRun<'_>,
    producer_layout: &crate::layout::LayoutContext,
) -> crate::layout::LayoutContext {
    let operators = run
        .assignment
        .control_operators()
        .into_iter()
        .map(|operator| crate::control::ControlOperator {
            name: operator.name.clone(),
            elite: operator.elite,
            buff_ids: run
                .instances
                .resolve_control_buff_ids(&operator.name, operator.tier()),
            tags: run.instances.tags_for(&operator.name, operator.tier()),
        })
        .collect();
    let result = crate::control::solve_control(
        &crate::control::ControlRoomInput {
            operators,
            mood: run.options.mood,
            layout: producer_layout.clone(),
        },
        run.table,
    );
    let mut layout = producer_layout.clone();
    layout.global = result.global;
    layout.global_inject = result.inject;
    layout
}

fn prepare_peak_control_candidate<'a>(
    mut run: AssignmentRun<'a>,
    plan: &AssignmentPlan,
    control_pool: &ControlPool,
    producer_layout: &crate::layout::LayoutContext,
    control_exclusions: &HashSet<String>,
    dynamic_set: Option<(&HashSet<String>, &HashSet<String>)>,
    stable_order: usize,
) -> Result<Option<PreparedPeakControlCandidate<'a>>> {
    if run.assignment.control_operators().len() < 5 {
        if let Some((required, excluded)) = dynamic_set {
            if !assign_control_matching_dynamic_set(
                &mut run.assignment,
                control_pool,
                run.table,
                producer_layout,
                run.options,
                &plan.control_candidate_requirements,
                required,
                excluded,
                control_exclusions,
                &mut run.used,
            )? {
                return Ok(None);
            }
        } else {
            assign_control(
                &mut run.assignment,
                control_pool,
                run.table,
                producer_layout,
                run.options,
                &plan.control_candidate_requirements,
                control_exclusions,
                &mut run.used,
            )?;
        }
    }
    if let Some((required, excluded)) = dynamic_set {
        let control_names: HashSet<String> = run
            .assignment
            .control_operators()
            .into_iter()
            .map(|op| op.name)
            .collect();
        if !required.is_subset(&control_names) || !control_names.is_disjoint(excluded) {
            return Ok(None);
        }
    }

    let control_layout = control_layout_for_upper_bound(&run, producer_layout);
    Ok(Some(PreparedPeakControlCandidate {
        policy_upper_bound: control_inject_policy_sort_key_upper_bound(
            &control_layout,
            run.blueprint,
            run.table,
        ),
        stable_order,
        run,
    }))
}

#[allow(clippy::too_many_arguments)]
fn finish_peak_prefix_candidate<'a>(
    mut run: AssignmentRun<'a>,
    plan: &AssignmentPlan,
    base_trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    power_pool: &PowerPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
    stable_order: usize,
) -> Result<Option<PeakPrefixCandidate<'a>>> {
    place_system_producers(
        run.blueprint,
        run.operbox,
        &plan.producers,
        &mut run.assignment,
        &mut run.used,
    );
    assign_sphinx_urrbian_dorm_anchor(
        run.blueprint,
        run.operbox,
        &mut run.assignment,
        &mut run.used,
    );
    assign_dorm_producers(
        run.blueprint,
        run.operbox,
        run.instances,
        &mut run.assignment,
        &mut run.used,
    )?;
    let layout = run.resolve_snapshot(true)?;
    let mut trade_pool = base_trade_pool.clone();
    if karlan_precision_active(&layout.global_inject) {
        add_jie_market_to_trade_pool(&mut trade_pool, run.instances, run.table);
    }
    assign_power_stations(
        run.blueprint,
        power_pool,
        run.table,
        &layout,
        run.options,
        &mut run.assignment,
        &mut run.used,
    )?;
    let remaining_trade_slots: usize = run
        .blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::TradePost)
        .map(|room| {
            room.operator_capacity()
                .saturating_sub(run.assignment.operators_in(&room.id).len())
        })
        .sum();
    let available_trade_operators = trade_pool
        .entries
        .iter()
        .filter(|entry| !run.used.contains(&entry.name))
        .count();
    if available_trade_operators < remaining_trade_slots {
        return Ok(None);
    }
    assign_trade_remainder(
        run.blueprint,
        &trade_pool,
        run.table,
        run.instances,
        gold_lines,
        run.options,
        Some(run.durin_plan),
        &forbid_trade_station_pairs(plan),
        &mut run.assignment,
        &mut run.used,
    )?;
    cleanup_unused_sphinx_urrbian_dorm_anchor(run.blueprint, &mut run.assignment, &mut run.used);

    let manu_layout = run.resolve_snapshot(true)?;
    assign_manufacture_lines(
        run.blueprint,
        run.operbox,
        run.instances,
        manu_pool,
        run.table,
        &manu_layout,
        run.options,
        forbid_same_room,
        &mut run.assignment,
        &mut run.used,
    )?;

    let policy_sort_key = match run.options.dynamic_producer_policy {
        ScoringPolicyId::ControlInjectRawSumV0 => {
            let final_layout = run.resolve_snapshot(true)?;
            control_inject_policy_sort_key_for_assignment(
                &final_layout,
                run.blueprint,
                &run.assignment,
                run.instances,
            )
        }
        ScoringPolicyId::TradeManufactureWeightedEfficiencyV1 => {
            refresh_manufacture_efficiency_snapshots(
                run.blueprint,
                &mut run.assignment,
                run.instances,
                run.table,
                run.options,
                Some(run.durin_plan),
            )?;
            refresh_trade_efficiency_snapshots(
                run.blueprint,
                &mut run.assignment,
                run.instances,
                run.table,
                run.options,
                Some(run.durin_plan),
            )?;
            let (trade_total, manufacture_total) =
                trade_manufacture_efficiency_totals(&run.assignment);
            evaluate_trade_manufacture_weighted_efficiency(
                trade_total,
                manufacture_total,
                run.options.trade_manufacture_weights,
            )
            .sort_key_millis as f64
        }
    };
    Ok(Some(PeakPrefixCandidate {
        policy_sort_key,
        stable_order,
        run,
    }))
}

fn trade_manufacture_efficiency_totals(
    assignment: &BaseAssignment,
) -> (crate::Efficiency, crate::Efficiency) {
    assignment.rooms.iter().fold(
        (crate::Efficiency::ZERO, crate::Efficiency::ZERO),
        |(trade_total, manufacture_total), room| {
            let Some(snapshot) = &room.efficiency else {
                return (trade_total, manufacture_total);
            };
            (
                trade_total + snapshot.trade_final_efficiency,
                manufacture_total + snapshot.manufacture_final_efficiency,
            )
        },
    )
}

/// 只为拥有实际贸易标签消费者的动态中枢 producer 建候选；按 atom 结构识别，
/// 不枚举任何消费方人员套餐。
fn dynamic_producer_candidates(
    control_pool: &ControlPool,
    trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    table: &SkillTable,
) -> Result<Vec<String>> {
    let trade_tags: HashSet<&str> = trade_pool
        .entries
        .iter()
        .flat_map(|entry| entry.tags.iter().map(String::as_str))
        .collect();
    let manu_tags: HashSet<&str> = manu_pool
        .entries
        .iter()
        .flat_map(|entry| entry.tags.iter().map(String::as_str))
        .collect();
    let mut producers = Vec::new();
    for entry in &control_pool.entries {
        let trade_match = control_entry_deferred_trade_tags(entry, table)?
            .iter()
            .any(|tag| trade_tags.contains(tag.as_str()));
        let manu_rules = crate::response_dependency::deferred_producer_rules_for_buffs(
            table,
            entry.buff_ids.iter().map(String::as_str),
            "manufacture",
        )?;
        let manu_match = manu_rules.iter().any(|rule| {
            table.get(&rule.source_buff_id).is_some_and(|skill| {
                skill.atoms.iter().any(|atom| match atom.selector.as_ref() {
                    Some(crate::types::Selector::TaggedCountInManuSum { tag }) => {
                        manu_tags.contains(tag.as_str())
                    }
                    _ => false,
                })
            })
        });
        if trade_match || manu_match {
            producers.push(entry.name.clone());
        }
    }
    producers.sort();
    producers.dedup();
    Ok(producers)
}

/// 从统一 plan 的 constraints 提取 forbid-same-room 名对，供制造 anchor 房搜索排除。
fn forbid_same_room_pairs(plan: &AssignmentPlan) -> Vec<(String, String)> {
    plan.constraints
        .iter()
        .filter_map(|c| match c {
            crate::layout::orchestrate::SystemConstraint::ForbidSameRoom { a, b } => {
                Some((a.clone(), b.clone()))
            }
            _ => None,
        })
        .collect()
}

fn forbid_trade_station_pairs(plan: &AssignmentPlan) -> Vec<(String, String)> {
    plan.constraints
        .iter()
        .filter_map(|constraint| match constraint {
            crate::layout::orchestrate::SystemConstraint::ForbidSameStation { a, b } => {
                Some((a.clone(), b.clone()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::blueprint::{
        BlueprintScenario, FacilityKind, RoomBlueprint, RoomId, RoomProduct,
    };
    use crate::operbox::OperBoxEntry;
    use crate::skill_table::{default_skill_table_path, SkillTable};
    use crate::trade::TradeOrderKind;

    fn e2(name: &str) -> OperBoxEntry {
        OperBoxEntry {
            id: name.to_string(),
            name: name.to_string(),
            elite: 2,
            level: 90,
            own: true,
            potential: 0,
            rarity: 6,
        }
    }

    #[test]
    fn meeting_rooms_are_filled_with_meeting_skilled_operators_first() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let blueprint = control_blueprint(true);
        let operbox = OperBox::from_entries(
            ["12F", "和弦", "复奏", "凛视", "麦哲伦"]
                .into_iter()
                .map(e2)
                .collect(),
        )
        .unwrap();
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        assign_meeting_rooms(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            &mut assignment,
            &mut used,
        )
        .unwrap();
        let names: Vec<_> = assignment
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert_eq!(names.len(), 2, "meeting room must be filled to capacity");
        assert!(
            !names.contains(&"麦哲伦"),
            "skillless operator must not displace meeting-skilled candidates: {names:?}"
        );
        assert!(
            names.contains(&"12F") || names.contains(&"复奏"),
            "fixed-speed meeting-skilled operator expected (solo-only 和弦 must not win in a two-person room): {names:?}"
        );
    }

    #[test]
    fn meeting_rooms_prefer_fixed_speed_over_solo_only_in_two_person_room() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let blueprint = control_blueprint(true);
        // 三名候选：和弦 solo 50%（双人房失效）、见行者固定 35%、12F 固定 20%。
        // 容量 2，若 solo 未失效，和弦(50) 必入选；哨兵修复后应选见行者+12F。
        let operbox =
            OperBox::from_entries(["和弦", "见行者", "12F"].into_iter().map(e2).collect()).unwrap();
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        assign_meeting_rooms(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            &mut assignment,
            &mut used,
        )
        .unwrap();
        let names: Vec<_> = assignment
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert_eq!(names.len(), 2, "meeting room must be filled to capacity");
        assert!(
            !names.contains(&"和弦"),
            "solo-only 和弦 (50%) must not win in a two-person room: {names:?}"
        );
        assert!(
            names.contains(&"见行者") && names.contains(&"12F"),
            "fixed-speed 见行者(35%) and 12F(20%) expected: {names:?}"
        );
    }

    #[test]
    fn meeting_rooms_fall_back_to_idle_operators_when_no_skill_candidates() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let blueprint = control_blueprint(true);
        let operbox = OperBox::from_entries(
            ["甲", "乙", "丙"]
                .into_iter()
                .map(|name| e2(name))
                .collect(),
        )
        .unwrap();
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        assign_meeting_rooms(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            &mut assignment,
            &mut used,
        )
        .unwrap();
        let names: Vec<_> = assignment
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert_eq!(names.len(), 2, "fallback must fill meeting room: {names:?}");
    }

    #[test]
    fn weighted_policy_never_uses_raw_inject_pruning() {
        assert_eq!(
            AssignBaseOptions::default().dynamic_producer_policy,
            ScoringPolicyId::ControlInjectRawSumV0
        );
        assert!(producer_pruning_enabled(
            true,
            ScoringPolicyId::ControlInjectRawSumV0
        ));
        assert!(!producer_pruning_enabled(
            true,
            ScoringPolicyId::TradeManufactureWeightedEfficiencyV1
        ));
        assert!(!producer_pruning_enabled(
            false,
            ScoringPolicyId::ControlInjectRawSumV0
        ));
    }

    #[test]
    fn weighted_policy_totals_only_consume_trade_and_manufacture_snapshots() {
        let mut assignment = BaseAssignment::default();
        assignment.set_room_with_efficiency(
            "trade_one",
            vec![],
            Some(crate::layout::RoomEfficiencySnapshot {
                trade_final_efficiency: crate::Efficiency::from_decimal(2.125),
                ..Default::default()
            }),
        );
        assignment.set_room_with_efficiency(
            "manu_one",
            vec![],
            Some(crate::layout::RoomEfficiencySnapshot {
                manufacture_final_efficiency: crate::Efficiency::from_decimal(3.250),
                ..Default::default()
            }),
        );
        assignment.set_room_with_efficiency(
            "power_one",
            vec![],
            Some(crate::layout::RoomEfficiencySnapshot {
                power_final_efficiency: crate::Efficiency::from_decimal(1.500),
                ..Default::default()
            }),
        );

        assert_eq!(
            trade_manufacture_efficiency_totals(&assignment),
            (
                crate::Efficiency::from_decimal(2.125),
                crate::Efficiency::from_decimal(3.250)
            )
        );
    }

    fn control_blueprint(with_trade: bool) -> BaseBlueprint {
        let mut rooms = vec![RoomBlueprint {
            id: RoomId::from("control"),
            kind: FacilityKind::ControlCenter,
            level: 5,
            product: None,
            dorm_beds: None,
            dorm_ambience_level: None,
        }];
        if with_trade {
            rooms.push(RoomBlueprint {
                id: RoomId::from("trade_one"),
                kind: FacilityKind::TradePost,
                level: 1,
                product: Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
                dorm_beds: None,
                dorm_ambience_level: None,
            });
            rooms.push(RoomBlueprint {
                id: RoomId::from("meeting"),
                kind: FacilityKind::MeetingRoom,
                level: 3,
                product: None,
                dorm_beds: None,
                dorm_ambience_level: None,
            });
        }
        BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms,
        }
    }

    fn control_candidates(include_vigil: bool) -> OperBox {
        let mut entries: Vec<_> = ["八幡海铃", "阿米娅", "Mon3tr", "焰尾", "薇薇安娜", "玛恩纳"]
            .into_iter()
            .map(e2)
            .collect();
        if include_vigil {
            entries.push(e2("伺夜"));
        }
        OperBox::from_entries(entries).unwrap()
    }

    fn plan_requiring_control_fillers(names: &[&str]) -> AssignmentPlan {
        let mut plan = AssignmentPlan::recovery(AssignShiftMode::Peak);
        plan.control_candidate_requirements
            .push(crate::layout::ControlCandidateRequirement {
                candidates: names.iter().map(|name| (*name).to_string()).collect(),
                min_count: names.len() as u8,
            });
        plan
    }

    fn assert_producer_pruning_matches_unpruned(
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        instances: &OperatorInstances,
        table: &SkillTable,
        options: &AssignBaseOptions,
        seed: &BaseAssignment,
        plan: &AssignmentPlan,
    ) -> (
        BaseAssignment,
        f64,
        Vec<crate::response_dependency::ResolvedProducerDependency>,
    ) {
        let pruned = run_shift_pipeline_with_producer_pruning(
            blueprint,
            operbox,
            instances,
            table,
            options,
            AssignShiftMode::Peak,
            seed,
            plan,
            true,
        )
        .unwrap();
        let unpruned = run_shift_pipeline_with_producer_pruning(
            blueprint,
            operbox,
            instances,
            table,
            options,
            AssignShiftMode::Peak,
            seed,
            plan,
            false,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&pruned).unwrap(),
            serde_json::to_value(&unpruned).unwrap(),
            "safe reduction changed the winner assignment"
        );

        let durin_plan = operbox.durin_dorm_planning_count(instances);
        let outcome = |assignment: &BaseAssignment| {
            let layout = crate::layout::resolve_base(
                blueprint,
                assignment,
                Some(instances),
                Some(table),
                options.mood,
                Some(durin_plan),
            )
            .unwrap()
            .layout_snapshot();
            let policy_sort_key = control_inject_policy_sort_key_for_assignment(
                &layout, blueprint, assignment, instances,
            );
            let dependencies =
                crate::response_dependency::resolve_assignment_producer_dependencies(
                    blueprint, assignment, instances, table,
                )
                .unwrap();
            (policy_sort_key, dependencies)
        };
        let (pruned_policy, pruned_dependencies) = outcome(&pruned);
        let (unpruned_policy, unpruned_dependencies) = outcome(&unpruned);
        assert_eq!(
            pruned_policy.to_bits(),
            unpruned_policy.to_bits(),
            "safe reduction changed the named policy result"
        );
        assert_eq!(
            pruned_dependencies, unpruned_dependencies,
            "safe reduction changed resolved producer dependencies"
        );
        (pruned, pruned_policy, pruned_dependencies)
    }

    #[test]
    fn single_syracusa_trade_consumer_can_make_dynamic_control_prefix_win() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let assignment = run_shift_pipeline(
            &control_blueprint(true),
            &control_candidates(true),
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        )
        .unwrap();

        assert!(assignment
            .control_operators()
            .iter()
            .any(|operator| operator.name == "八幡海铃"));
        assert_eq!(
            assignment.operators_in(&RoomId::from("trade_one"))[0].name,
            "伺夜"
        );
    }

    #[test]
    fn no_syracusa_trade_consumer_does_not_force_dynamic_control_producer() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let assignment = run_shift_pipeline(
            &control_blueprint(false),
            &control_candidates(false),
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        )
        .unwrap();

        assert!(assignment
            .control_operators()
            .iter()
            .all(|operator| operator.name != "八幡海铃"));
    }

    #[test]
    fn single_consumer_does_not_force_haru_when_ordinary_control_is_better() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "玫兰莎",
                "阿米娅",
                "八幡海铃",
                "伺夜",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点", "玫兰莎"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );
        let assignment = run_shift_pipeline(
            &control_blueprint(true),
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &seed,
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        )
        .unwrap();
        let control: HashSet<_> = assignment
            .control_operators()
            .into_iter()
            .map(|operator| operator.name)
            .collect();
        assert!(control.contains("阿米娅"), "control={control:?}");
        assert!(!control.contains("八幡海铃"), "control={control:?}");
        assert_eq!(
            assignment.operators_in(&RoomId::from("trade_one"))[0].name,
            "伺夜"
        );
    }

    #[test]
    fn coexisting_dynamic_producers_are_compared_as_one_presence_set() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "八幡海铃",
                "戴菲恩",
                "玛恩纳",
                "焰尾",
                "伺夜",
                "摩根",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let mut blueprint = control_blueprint(true);
        blueprint
            .rooms
            .iter_mut()
            .find(|room| room.kind == FacilityKind::TradePost)
            .unwrap()
            .level = 2;
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );

        let (assignment, _, dependencies) = assert_producer_pruning_matches_unpruned(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
            &seed,
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        );

        let control: HashSet<_> = assignment
            .control_operators()
            .into_iter()
            .map(|operator| operator.name)
            .collect();
        assert!(control.contains("八幡海铃"), "control={control:?}");
        assert!(control.contains("戴菲恩"), "control={control:?}");
        let trade: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_one"))
            .iter()
            .map(|operator| operator.name.as_str())
            .collect();
        assert_eq!(trade, HashSet::from(["伺夜", "摩根"]));
        assert_eq!(dependencies.len(), 2);
        assert!(dependencies
            .iter()
            .any(|dependency| dependency.rule_id == "trade_siracusa_scaling"));
        assert!(dependencies
            .iter()
            .any(|dependency| dependency.rule_id == "trade_glasgow_scaling"));
    }

    #[test]
    fn threshold_trade_producer_competes_through_the_same_rule_path() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "玫兰莎",
                "玛恩纳",
                "凛御银灰",
                "银灰",
                "崖心",
                "讯使",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let mut blueprint = control_blueprint(true);
        blueprint
            .rooms
            .iter_mut()
            .find(|room| room.kind == FacilityKind::TradePost)
            .unwrap()
            .level = 3;
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点", "玫兰莎"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );
        let (assignment, _, dependencies) = assert_producer_pruning_matches_unpruned(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            &seed,
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        );
        assert!(assignment
            .control_operators()
            .iter()
            .any(|operator| operator.name == "凛御银灰"));
        let trade: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_one"))
            .iter()
            .map(|operator| operator.name.as_str())
            .collect();
        assert_eq!(trade, HashSet::from(["银灰", "崖心", "讯使"]));
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].rule_id, "trade_karlan_station");
    }

    #[test]
    fn manufacture_target_producer_uses_the_same_complete_prefix_comparison() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "玫兰莎",
                "玛恩纳",
                "涤火杰西卡",
                "杰西卡",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let blueprint = BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms: vec![
                RoomBlueprint {
                    id: RoomId::from("control"),
                    kind: FacilityKind::ControlCenter,
                    level: 5,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("manu_one"),
                    kind: FacilityKind::Factory,
                    level: 1,
                    product: Some(RoomProduct::Factory {
                        recipe: crate::types::RecipeKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("meeting"),
                    kind: FacilityKind::MeetingRoom,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
            ],
        };
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点", "玫兰莎"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );
        let (assignment, _, dependencies) = assert_producer_pruning_matches_unpruned(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            &seed,
            &AssignmentPlan::recovery(AssignShiftMode::Peak),
        );
        assert!(assignment
            .control_operators()
            .iter()
            .any(|operator| operator.name == "涤火杰西卡"));
        assert_eq!(
            assignment.operators_in(&RoomId::from("manu_one"))[0].name,
            "杰西卡"
        );
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].rule_id, "manu_blacksteel_scaling");
    }

    #[test]
    fn producer_safe_reduction_preserves_equal_policy_tie() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "玫兰莎",
                "八幡海铃",
                "涤火杰西卡",
                "伺夜",
                "贝洛内",
                "杰西卡",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let mut blueprint = control_blueprint(true);
        blueprint
            .rooms
            .iter_mut()
            .find(|room| room.kind == FacilityKind::TradePost)
            .unwrap()
            .level = 2;
        blueprint.rooms.push(RoomBlueprint {
            id: RoomId::from("manu_one"),
            kind: FacilityKind::Factory,
            level: 1,
            product: Some(RoomProduct::Factory {
                recipe: crate::types::RecipeKind::Gold,
            }),
            dorm_beds: None,
            dorm_ambience_level: None,
        });
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点", "玫兰莎"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );
        let options = AssignBaseOptions {
            top_k: 20,
            ..Default::default()
        };
        let plan = plan_requiring_control_fillers(&["古米", "夜刀", "斑点", "玫兰莎"]);
        let control_pool = build_control_pool_with_fillers(&operbox, &instances, &table).unwrap();
        let trade_pool =
            build_trade_pool(&operbox.trade_roster(&instances), &instances, &table).unwrap();
        let manu_pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        assert_eq!(
            dynamic_producer_candidates(&control_pool, &trade_pool, &manu_pool, &table)
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>(),
            HashSet::from(["八幡海铃".to_string(), "涤火杰西卡".to_string()])
        );

        let (_, policy_sort_key, dependencies) = assert_producer_pruning_matches_unpruned(
            &blueprint, &operbox, &instances, &table, &options, &seed, &plan,
        );
        // Haru contributes 5 * 2 Siracusa to trade; Jessica contributes
        // 5 * 1 Blacksteel to both manufacture recipe components in this policy.
        assert_eq!(policy_sort_key, 10.0);
        assert!(!should_prune_peak_candidate(true, Some(10.0), 10.0));
        assert_eq!(dependencies.len(), 1);
        match dependencies[0].rule_id.as_str() {
            "trade_siracusa_scaling" => {
                assert_eq!(dependencies[0].effective_contribution, Some(10.0));
            }
            "manu_blacksteel_scaling" => {
                assert_eq!(dependencies[0].effective_contribution, Some(5.0));
            }
            rule_id => panic!("unexpected tie winner {rule_id}"),
        }
    }

    #[test]
    fn producer_safe_reduction_matches_unpruned_after_infinite_upper_bound() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [
                "古米",
                "夜刀",
                "斑点",
                "玫兰莎",
                "八幡海铃",
                "若叶睦",
                "伺夜",
            ]
            .into_iter()
            .map(e2)
            .collect(),
        )
        .unwrap();
        let blueprint = control_blueprint(true);
        let options = AssignBaseOptions {
            top_k: 20,
            ..Default::default()
        };
        let plan = plan_requiring_control_fillers(&["古米", "夜刀", "斑点", "玫兰莎"]);
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "control",
            ["古米", "夜刀", "斑点", "玫兰莎"]
                .into_iter()
                .map(|name| crate::layout::AssignedOperator::new(name, 2))
                .collect(),
        );

        let mut unresolved_support = seed.clone();
        let mut control = unresolved_support.control_operators();
        control.push(crate::layout::AssignedOperator::new("若叶睦", 2));
        unresolved_support.set_room("control", control);
        let used = unresolved_support
            .control_operators()
            .into_iter()
            .map(|operator| operator.name)
            .collect();
        let run = AssignmentRun::new(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &options,
            unresolved_support,
            used,
        );
        let producer_layout = run.resolve_snapshot(false).unwrap();
        let upper_layout = control_layout_for_upper_bound(&run, &producer_layout);
        let unresolved_upper_bound =
            control_inject_policy_sort_key_upper_bound(&upper_layout, &blueprint, &table);
        assert!(unresolved_upper_bound.is_infinite());
        assert!(!should_prune_peak_candidate(
            true,
            Some(f64::MAX),
            unresolved_upper_bound
        ));

        let (assignment, _, dependencies) = assert_producer_pruning_matches_unpruned(
            &blueprint, &operbox, &instances, &table, &options, &seed, &plan,
        );
        assert!(assignment
            .control_operators()
            .iter()
            .any(|operator| operator.name == "八幡海铃"));
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].rule_id, "trade_siracusa_scaling");
    }

    #[test]
    fn dynamic_prefix_propagates_real_resolve_errors_instead_of_falling_back() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = control_candidates(true);
        let mut blueprint = control_blueprint(true);
        blueprint
            .rooms
            .iter_mut()
            .find(|room| room.kind == FacilityKind::TradePost)
            .unwrap()
            .product = None;
        let options = AssignBaseOptions::default();
        let plan = AssignmentPlan::recovery(AssignShiftMode::Peak);
        let control_pool = build_control_pool_with_fillers(&operbox, &instances, &table).unwrap();
        let trade_pool =
            build_trade_pool(&operbox.trade_roster(&instances), &instances, &table).unwrap();
        let manu_pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        let power_pool =
            build_power_pool(&operbox.power_roster(&instances), &instances, &table).unwrap();
        let run = AssignmentRun::new(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &options,
            BaseAssignment::default(),
            HashSet::new(),
        );

        let result = run_peak_prefix_candidate(
            run,
            &plan,
            &control_pool,
            &trade_pool,
            &manu_pool,
            &power_pool,
            0,
            &[],
            Some((&HashSet::from(["八幡海铃".to_string()]), &HashSet::new())),
        );
        assert!(result.is_err(), "真实 resolve 错误不得降级成不可行候选");
    }
}
