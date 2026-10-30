//! 单班填房流水线主体（ADR 0001 决策 A）。
//!
//! 阶段顺序与 resolve 时机的唯一事实源。facade（`assign.rs`）只负责构建
//! `skip_system_ids` 与 `build_plan`，随后委托给本模块。
//! Peak 普通模式会从同一 execute seed 比较普通中枢与可选动态 producer 的完整生产前缀；
//! 自动模式不搜索 producer 组合，只用一次公共生产填充。每个贸易站在前站提交后重新
//! resolve，制造只消费选中的前缀。

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomProduct};
use crate::layout::orchestrate::{execute_plan, AssignmentPlan};
use crate::layout::shift::AssignShiftMode;
use crate::operbox::OperBox;
use crate::pool::{
    add_owned_jie_market_to_trade_pool, build_control_pool_with_fillers, build_manufacture_pool,
    build_power_pool_with_fillers, build_trade_pool, karlan_precision_active, ControlPool,
    PowerPool, TradePool,
};
use crate::scoring::{evaluate_trade_manufacture_weighted_efficiency, ScoringPolicyId};
use crate::search::{
    control_entry_deferred_trade_tags, control_inject_policy_sort_key_for_assignment,
    control_inject_policy_sort_key_upper_bound, ManuSearchHit, TradeSearchHit,
};
use crate::skill_table::SkillTable;

use super::commit::{commit_manu_room, commit_trade_room, manu_hit_names, trade_hit_names};
use super::control_fill::{
    assign_control_matching_dynamic_set_with_saileach, assign_control_with_saileach,
};
use super::manufacture_fill::{
    assign_manufacture_lines, manu_options, manufacture_candidate_pool_for_demand,
    pick_capacity_manu_hit, pick_manu_hit, refresh_manufacture_efficiency_snapshots,
};
use super::power_fill::assign_power_stations_with_instances as assign_power_stations;
use super::producer_fill::{
    assign_dorm_producers, cleanup_unused_sphinx_urrbian_dorm_anchor, place_system_producers,
};
use super::run::{AssignmentRun, StageTimer};
use super::trade_fill::{
    assign_trade_remainder, assign_trade_remainder_with_plan_anchors, pick_trade_meta_then_plain,
    refresh_trade_efficiency_snapshots, take_next_trade_room, trade_order_from_room,
};
use super::{inject_search_anchor_pool_entries, tag_pool_from_plan, AssignBaseOptions};

const SPHINX_NAME: &str = "深巡";
const ULPIANUS_NAME: &str = "乌尔比安";
/// 唯一需要贸易/制造跨设施归属比较的干员（能力影子候选适用域，用户裁决 2026-09-01）。
const SHADOW_TRADE_MANUFACTURE_OPERATOR: &str = "夜烟";

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
    let observe_control =
        std::env::var_os("ARKNIGHTS_INFRA_OBSERVE_CONTROL").is_some_and(|value| value == "1");
    macro_rules! observe {
        ($($arg:tt)*) => {
            if observe_control {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "[INFRA-CONTROL-OBSERVE] {}",
                    format_args!($($arg)*)
                );
            }
        };
    }
    if observe_control {
        let mut roster: Vec<_> = operbox.roster().names().cloned().collect();
        roster.sort();
        let mut pins: Vec<_> = seed
            .control_operators()
            .into_iter()
            .map(|op| op.name)
            .collect();
        pins.sort();
        observe!("pipeline mode={mode:?} shift_hours={} control_work_hours={:?} control_room_capacity={:?} operbox_size={} operbox_identity={:?} seed_control_pins={pins:?}",
            options.shift_hours,
            options.control_work_hours,
            blueprint.rooms.iter().find(|room| room.kind == FacilityKind::ControlCenter).map(|room| room.operator_capacity()),
            roster.len(),
            roster);
        for claim in &plan.registry_claims {
            observe!(
                "plan_claim id={} bind_all={} on_shifts={} off_shifts={} control_allowed={:?} slots={:?}",
                claim.system_id,
                claim.bind_all,
                claim.on_shifts,
                claim.off_shifts,
                claim.control_allowed_operators,
                claim
                    .slots
                    .iter()
                    .filter(|slot| slot.facility == FacilityKind::ControlCenter)
                    .map(|slot| (
                        &slot.room_id,
                        slot.fill,
                        slot.required_count,
                        slot.operators
                            .iter()
                            .map(|op| op.name.as_str())
                            .collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        observe!(
            "plan_control_requirements={:?} control_exclusions={:?} control_pins={:?}",
            plan.control_candidate_requirements,
            plan.control_exclusions,
            seed.control_operators()
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>()
        );
    }
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
    timer.mark("plan_execute");

    // 建池（中枢 / 贸易 / 制造 / 发电）+ tier 标注 + anchor 注入。
    let mut control_pool = build_control_pool_with_fillers(operbox, instances, table)?;
    let mut trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let mut manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let mut power_pool = build_power_pool_with_fillers(operbox, instances, table)?;
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
    timer.mark("pool_build");

    match mode {
        AssignShiftMode::Peak => {
            if options.auto_rotation {
                // Plan controls are already in the assignment; only fill missing slots before
                // entering the single production fill shared with the ordinary prefix path.
                if run.assignment.control_operators().len() < 5 {
                    let producer_layout = run.resolve_snapshot(false)?;
                    let saileach_best_office_pct =
                        best_available_office_pct(&run, &producer_layout)?;
                    if !fill_peak_control(
                        &mut run,
                        plan,
                        &control_pool,
                        &producer_layout,
                        &control_exclusions,
                        None,
                        saileach_best_office_pct,
                        0,
                    )? {
                        return Err(Error::msg("auto peak control has no legal fill"));
                    }
                }
                if !crate::layout::work_budget::fixed_main_fits(&run.assignment) {
                    return Err(Error::msg(
                        "auto_fixed_main_work_duration: control fill cannot sustain frozen main",
                    ));
                }
                timer.mark("peak_prepare_control");
                run = finish_peak_prefix(
                    run,
                    plan,
                    &trade_pool,
                    &manu_pool,
                    &power_pool,
                    gold_lines,
                    &forbid_same_room,
                )?;
            } else {
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
                    if observe_control {
                        let mut required_log: Vec<_> = required.iter().cloned().collect();
                        required_log.sort();
                        let mut excluded_log: Vec<_> = excluded.iter().cloned().collect();
                        excluded_log.sort();
                        observe!("producer_mask mask={mask} required={required_log:?} excluded={excluded_log:?}");
                    }
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
                    } else {
                        observe!("prepare_return_none mask={mask}");
                    }
                }
                prepared.sort_by(|left, right| {
                    right
                        .policy_upper_bound
                        .partial_cmp(&left.policy_upper_bound)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| left.stable_order.cmp(&right.stable_order))
                });
                timer.mark("peak_prepare_control");
                let mut best = None;
                for prepared in prepared {
                    if should_prune_peak_candidate(
                        enable_producer_pruning,
                        best.as_ref()
                            .map(|best: &PeakPrefixCandidate<'_>| best.policy_sort_key),
                        prepared.policy_upper_bound,
                    ) {
                        observe!(
                            "peak_prefix mask={} result=pruned_by_policy_upper_bound upper_bound={:?}",
                            prepared.stable_order,
                            prepared.policy_upper_bound
                        );
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
                let best =
                    best.ok_or_else(|| Error::msg("all peak prefix candidates are infeasible"))?;
                observe!(
                    "peak_prefix_winner mask={} policy_sort_key={:?} control={:?}",
                    best.stable_order,
                    best.policy_sort_key,
                    best.run
                        .assignment
                        .control_operators()
                        .iter()
                        .map(|op| op.name.as_str())
                        .collect::<Vec<_>>()
                );
                run = best.run;
            }
            // 只对最终 winner 做一次夜烟 trade/manufacture 归属比较；候选循环内不再
            // 为每个中枢候选重算整套生产填充。候选循环使用含喀兰杰市场的 trade_pool，
            // 这里的比较必须使用同一池，否则重填结果会与 winner 原结果漂移。
            let mut winner_trade_pool = trade_pool.clone();
            let winner_layout = run.resolve_snapshot(true)?;
            if karlan_precision_active(&winner_layout.global_inject) {
                add_owned_jie_market_to_trade_pool(
                    &mut winner_trade_pool,
                    run.operbox,
                    run.instances,
                    run.table,
                );
            }
            if let Some((assignment, used)) = apply_shadow_trade_manufacture_pair(
                &run,
                &winner_trade_pool,
                &manu_pool,
                gold_lines,
                &forbid_same_room,
            )? {
                run.assignment = assignment;
                run.used = used;
            }
            timer.mark("peak_prefixes");
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
    run.assignment.sort_room_operators_stably();
    refresh_trade_efficiency_snapshots(
        blueprint,
        &mut run.assignment,
        instances,
        table,
        options,
        Some(run.durin_plan),
    )?;
    timer.mark("final_snapshot_refresh");

    assign_meeting_rooms(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &mut run.assignment,
        &mut run.used,
    )?;
    timer.mark("meeting_fill");

    run.assignment.sort_room_operators_stably();
    timer.mark("stable_sort");

    timer.report();
    observe!(
        "pipeline_result control={:?}",
        run.assignment
            .control_operators()
            .iter()
            .map(|op| op.name.as_str())
            .collect::<Vec<_>>()
    );
    Ok(run.assignment)
}

fn producer_pruning_enabled(requested: bool, policy: ScoringPolicyId) -> bool {
    requested && policy == ScoringPolicyId::ControlInjectRawSumV0
}

/// 夜烟 trade/manufacture 归属比较（能力影子候选适用域仅夜烟，用户裁决 2026-09-01）。
///
/// 只比较首个空贸易站与首个空制造站的实际 pick：仅当两侧都选中夜烟时，才构造
/// “夜烟进贸易”与“夜烟进制造”两条分支并结算完整分支；否则走普通贸易+制造填充。
/// Peak 的每个中枢候选在循环内已填满生产房，本函数从 clone 上清空生产房再探测，
/// 因此只对最终 winner 执行一次，候选循环内不产生重复重算。
#[allow(clippy::too_many_arguments)]
fn apply_shadow_trade_manufacture_pair(
    run: &AssignmentRun<'_>,
    trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
) -> Result<Option<(BaseAssignment, HashSet<String>)>> {
    let frozen_factory_room = run
        .options
        .preclaimed_factory_room
        .as_ref()
        .map(|room_id| {
            let room = run.blueprint.room(room_id).ok_or_else(|| {
                Error::msg(format!(
                    "preclaimed factory room {} disappeared before the shadow trade/manufacture comparison",
                    room_id.0
                ))
            })?;
            if room.kind != FacilityKind::Factory {
                return Err(Error::msg(format!(
                    "preclaimed room {} is not a factory",
                    room_id.0
                )));
            }
            Ok(room_id)
        })
        .transpose()?;
    let mut assignment = run.assignment.clone();
    let mut used = run.used.clone();
    clear_production_rooms(
        run.blueprint,
        &mut assignment,
        &mut used,
        frozen_factory_room,
    );

    let Some((trade_room, manu_room, trade_hit, manu_hit)) =
        probe_shadow_pair(run, trade_pool, manu_pool, gold_lines, &assignment, &used)?
    else {
        return Ok(None);
    };

    let trade_first = shadow_trade_first_branch(
        run,
        trade_pool,
        manu_pool,
        gold_lines,
        forbid_same_room,
        &assignment,
        &used,
        &trade_room,
        &trade_hit,
    );
    let manu_first = shadow_manufacture_first_branch(
        run,
        trade_pool,
        manu_pool,
        gold_lines,
        forbid_same_room,
        &assignment,
        &used,
        &manu_room,
        &manu_hit,
    );

    let winner = match (trade_first, manu_first) {
        (Ok(trade), Ok(manu)) => {
            if trade.score_millis >= manu.score_millis {
                trade
            } else {
                manu
            }
        }
        (Ok(trade), Err(_)) => trade,
        (Err(_), Ok(manu)) => manu,
        (Err(trade_error), Err(manu_error)) => {
            return Err(Error::msg(format!(
                "shadow pair has no feasible disjoint branch: trade-first={trade_error}; manufacture-first={manu_error}"
            )))
        }
    };
    Ok(Some((winner.assignment, winner.used)))
}

/// 首个空贸易站与首个空制造站的实际 pick 是否都选中夜烟（探测不提交占用）。
#[allow(clippy::too_many_arguments)]
fn probe_shadow_pair(
    run: &AssignmentRun<'_>,
    trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    gold_lines: u32,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
) -> Result<
    Option<(
        crate::layout::blueprint::RoomBlueprint,
        crate::layout::blueprint::RoomBlueprint,
        TradeSearchHit,
        ManuSearchHit,
    )>,
> {
    if used.contains(SHADOW_TRADE_MANUFACTURE_OPERATOR)
        || trade_pool
            .entry(SHADOW_TRADE_MANUFACTURE_OPERATOR)
            .is_none()
        || manu_pool.entry(SHADOW_TRADE_MANUFACTURE_OPERATOR).is_none()
    {
        return Ok(None);
    }
    let Some((trade_room, manu_room)) =
        first_shadow_rooms(run.blueprint, assignment, trade_pool, used)
    else {
        return Ok(None);
    };
    let layout = if run.options.auto_rotation {
        crate::layout::resolve::resolve_layout_snapshot(
            run.blueprint,
            assignment,
            Some(run.instances),
            Some(run.table),
            run.options.mood,
            Some(run.durin_plan),
        )?
    } else {
        run.resolve_snapshot(true)?
    };
    let mut probe_used = used.clone();
    let trade_hit = pick_trade_meta_then_plain(
        trade_pool,
        run.table,
        &layout,
        gold_lines,
        run.options,
        trade_order_from_room(trade_room)?,
        trade_room.level,
        &mut probe_used,
        &[],
        false,
    )?;
    let trade_takes_night_smoke = trade_hit_names(&trade_hit)
        .iter()
        .any(|name| name == SHADOW_TRADE_MANUFACTURE_OPERATOR);
    if !trade_takes_night_smoke {
        return Ok(None);
    }
    let manu_hit = pick_shadow_manufacture(
        run,
        manu_pool,
        &layout,
        room_recipe(manu_room)?,
        manu_room.level,
        used,
    )?;
    let manu_takes_night_smoke = manu_hit_names(&manu_hit)
        .iter()
        .any(|name| name == SHADOW_TRADE_MANUFACTURE_OPERATOR);
    if !manu_takes_night_smoke {
        return Ok(None);
    }
    Ok(Some((
        trade_room.clone(),
        manu_room.clone(),
        trade_hit,
        manu_hit,
    )))
}

/// 从工作副本上清空全部贸易/制造房占用，并释放对应 `used` 条目。
/// Peak winner 已由候选循环填满生产房，先清空才能回到“首个空房对”边界。
fn clear_production_rooms(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    frozen_factory_room: Option<&crate::layout::RoomId>,
) {
    let filled: HashSet<String> = assignment
        .iter_rooms()
        .filter(|room| {
            frozen_factory_room.is_none_or(|frozen| room.room_id != *frozen)
                && blueprint.room(&room.room_id).is_some_and(|spec| {
                    matches!(spec.kind, FacilityKind::TradePost | FacilityKind::Factory)
                })
        })
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
        .collect();
    if filled.is_empty() {
        return;
    }
    assignment.remove_operators_where(|room_id, _| {
        frozen_factory_room.is_none_or(|frozen| room_id != frozen)
            && blueprint.room(room_id).is_some_and(|spec| {
                matches!(spec.kind, FacilityKind::TradePost | FacilityKind::Factory)
            })
    });
    used.retain(|name| !filled.contains(name));
}

/// 分支 A：夜烟进贸易（commit 首个空贸易站 pick），制造侧排除夜烟后正常填充。
#[allow(clippy::too_many_arguments)]
fn shadow_trade_first_branch(
    run: &AssignmentRun<'_>,
    trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
    base_assignment: &BaseAssignment,
    base_used: &HashSet<String>,
    trade_room: &crate::layout::blueprint::RoomBlueprint,
    trade_hit: &TradeSearchHit,
) -> Result<ShadowBranch> {
    let mut assignment = base_assignment.clone();
    let mut used = base_used.clone();
    commit_trade_room(
        &mut assignment,
        &trade_room.id,
        trade_hit,
        trade_pool,
        &mut used,
    )?;
    fill_trade_remainder(run, trade_pool, gold_lines, &mut assignment, &mut used)?;
    cleanup_unused_sphinx_urrbian_dorm_anchor(run.blueprint, &mut assignment, &mut used);
    fill_manufacture_remainder(run, manu_pool, forbid_same_room, &mut assignment, &mut used)?;
    settle_shadow_branch(run, assignment, used)
}

/// 分支 B：夜烟进制造（commit 首个空制造站 pick），贸易侧排除夜烟后正常填充。
#[allow(clippy::too_many_arguments)]
fn shadow_manufacture_first_branch(
    run: &AssignmentRun<'_>,
    trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
    base_assignment: &BaseAssignment,
    base_used: &HashSet<String>,
    manu_room: &crate::layout::blueprint::RoomBlueprint,
    manu_hit: &ManuSearchHit,
) -> Result<ShadowBranch> {
    let mut assignment = base_assignment.clone();
    let mut used = base_used.clone();
    commit_manu_room(
        &mut assignment,
        &manu_room.id,
        manu_hit,
        manu_pool,
        &mut used,
    )?;
    fill_trade_remainder(run, trade_pool, gold_lines, &mut assignment, &mut used)?;
    cleanup_unused_sphinx_urrbian_dorm_anchor(run.blueprint, &mut assignment, &mut used);
    fill_manufacture_remainder(run, manu_pool, forbid_same_room, &mut assignment, &mut used)?;
    settle_shadow_branch(run, assignment, used)
}

fn fill_trade_remainder(
    run: &AssignmentRun<'_>,
    trade_pool: &TradePool,
    gold_lines: u32,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_trade_remainder(
        run.blueprint,
        trade_pool,
        run.table,
        run.instances,
        gold_lines,
        run.options,
        Some(run.durin_plan),
        assignment,
        used,
    )
}

fn fill_manufacture_remainder(
    run: &AssignmentRun<'_>,
    manu_pool: &crate::pool::ManuPool,
    forbid_same_room: &[(String, String)],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let layout = crate::layout::resolve_base(
        run.blueprint,
        assignment,
        Some(run.instances),
        Some(run.table),
        run.options.mood,
        Some(run.durin_plan),
    )?
    .layout;
    assign_manufacture_lines(
        run.blueprint,
        run.operbox,
        run.instances,
        manu_pool,
        run.table,
        &layout,
        run.options,
        forbid_same_room,
        assignment,
        used,
    )
}

struct ShadowBranch {
    assignment: BaseAssignment,
    used: HashSet<String>,
    score_millis: i64,
}

fn first_shadow_rooms<'a>(
    blueprint: &'a BaseBlueprint,
    assignment: &BaseAssignment,
    trade_pool: &TradePool,
    used: &HashSet<String>,
) -> Option<(
    &'a crate::layout::blueprint::RoomBlueprint,
    &'a crate::layout::blueprint::RoomBlueprint,
)> {
    let mut trade_rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|room| {
            room.kind == FacilityKind::TradePost
                && room.operator_capacity() > 0
                && assignment.operators_in(&room.id).is_empty()
        })
        .collect();
    let trade = take_next_trade_room(&mut trade_rooms, trade_pool, assignment, used)?;
    let manufacture = blueprint
        .rooms
        .iter()
        .filter(|room| {
            room.kind == FacilityKind::Factory
                && room.operator_capacity() > 0
                && assignment.operators_in(&room.id).is_empty()
                && matches!(
                    room.product,
                    Some(RoomProduct::Factory { recipe })
                        if recipe != crate::types::RecipeKind::All
                )
        })
        .min_by_key(|room| match room.product {
            Some(RoomProduct::Factory { recipe }) => match recipe {
                crate::types::RecipeKind::Gold => (0, room.id.0.as_str()),
                crate::types::RecipeKind::BattleRecord => (1, room.id.0.as_str()),
                crate::types::RecipeKind::Originium => (2, room.id.0.as_str()),
                crate::types::RecipeKind::All => (3, room.id.0.as_str()),
            },
            _ => (3, room.id.0.as_str()),
        })?;
    Some((trade, manufacture))
}

fn room_recipe(room: &crate::layout::blueprint::RoomBlueprint) -> Result<crate::types::RecipeKind> {
    match room.product.as_ref() {
        Some(RoomProduct::Factory { recipe }) if *recipe != crate::types::RecipeKind::All => {
            Ok(*recipe)
        }
        _ => Err(Error::msg(format!(
            "shadow target factory {} requires a concrete recipe",
            room.id.0
        ))),
    }
}

fn pick_shadow_manufacture(
    run: &AssignmentRun<'_>,
    pool: &crate::pool::ManuPool,
    layout: &crate::layout::LayoutContext,
    recipe: crate::types::RecipeKind,
    room_level: u8,
    used: &HashSet<String>,
) -> Result<ManuSearchHit> {
    let options = manu_options(layout, run.options, recipe, room_level);
    let top_k = options.top_k;
    let candidate_pool = manufacture_candidate_pool_for_demand(pool, used);
    pick_manu_hit(&candidate_pool, run.table, options.clone(), used, top_k)
        .or_else(|_| pick_manu_hit(pool, run.table, options, used, top_k))
        .or_else(|_| {
            pick_capacity_manu_hit(
                pool,
                run.table,
                layout,
                run.options,
                recipe,
                used,
                room_level,
            )
        })
}

fn settle_shadow_branch(
    run: &AssignmentRun<'_>,
    mut assignment: BaseAssignment,
    used: HashSet<String>,
) -> Result<ShadowBranch> {
    refresh_manufacture_efficiency_snapshots(
        run.blueprint,
        &mut assignment,
        run.instances,
        run.table,
        run.options,
        Some(run.durin_plan),
    )?;
    refresh_trade_efficiency_snapshots(
        run.blueprint,
        &mut assignment,
        run.instances,
        run.table,
        run.options,
        Some(run.durin_plan),
    )?;
    let (trade_total, manufacture_total) = trade_manufacture_efficiency_totals(&assignment);
    let score = evaluate_trade_manufacture_weighted_efficiency(
        trade_total,
        manufacture_total,
        run.options.trade_manufacture_weights,
    );
    Ok(ShadowBranch {
        assignment,
        used,
        score_millis: score.sort_key_millis,
    })
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
    let registry = crate::support_facility::SupportRegistry::load_default_shared()?;
    let extra_recruit_slots = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let elapsed_hours = options.shift_hours;
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
            let winter_available = operators
                .iter()
                .any(|operator| operator.name == "忍冬" && operator.elite >= 2)
                || roster.names().any(|name| {
                    !used.contains(name.as_str())
                        && name == "忍冬"
                        && roster
                            .progress(name)
                            .is_some_and(|progress| progress.elite >= 2)
                });
            let candidate_names = roster
                .names()
                .filter(|name| {
                    name.as_str() != "铃兰"
                        || operators
                            .iter()
                            .any(|op| op.name == "忍冬" && op.elite >= 2)
                })
                .filter(|name| !used.contains(name.as_str()))
                .filter_map(|name| roster.progress(name).map(|progress| (name, progress)))
                .collect::<Vec<_>>();

            // 空房先比较完整的双人组合，避免第一席用哨兵评分时丢掉
            // 忍冬精二与铃兰同室的 +30% 条件收益。
            if operators.is_empty() && room.operator_capacity() >= 2 {
                let mut pair_candidates = candidate_names
                    .iter()
                    .filter(|(name, progress)| {
                        registry.entries.iter().any(|entry| {
                            entry.facility == crate::support_facility::SupportFacility::Meeting
                                && entry.operator == **name
                                && (progress.elite > entry.min_elite
                                    || (progress.elite == entry.min_elite
                                        && (progress.level == 0
                                            || progress.level >= entry.min_level)))
                        })
                    })
                    .map(|(name, progress)| (name.to_string(), *progress))
                    .collect::<Vec<_>>();
                let mut fillers = candidate_names
                    .iter()
                    .filter(|(name, _)| {
                        !pair_candidates
                            .iter()
                            .any(|(skill_name, _)| skill_name == *name)
                    })
                    .collect::<Vec<_>>();
                fillers.sort_by(|(name_a, progress_a), (name_b, progress_b)| {
                    progress_b
                        .elite
                        .cmp(&progress_a.elite)
                        .then_with(|| progress_b.rarity.cmp(&progress_a.rarity))
                        .then_with(|| name_a.cmp(name_b))
                });
                pair_candidates.extend(
                    fillers
                        .into_iter()
                        .take(2)
                        .map(|(name, progress)| (name.to_string(), *progress)),
                );
                if winter_available && !used.contains("铃兰") {
                    if let Some(progress) = roster.progress("铃兰") {
                        pair_candidates.push(("铃兰".to_string(), progress));
                    }
                }
                let mut best_pair: Option<(
                    f64,
                    String,
                    String,
                    crate::roster::OperatorProgress,
                    crate::roster::OperatorProgress,
                )> = None;
                for (left_index, (left_name, left_progress)) in pair_candidates.iter().enumerate() {
                    for (right_name, right_progress) in pair_candidates.iter().skip(left_index + 1)
                    {
                        if (left_name.as_str() == "铃兰" && right_name.as_str() != "忍冬")
                            || (right_name.as_str() == "铃兰" && left_name.as_str() != "忍冬")
                        {
                            continue;
                        }
                        let support_ops = vec![
                            crate::support_facility::SupportOperator {
                                name: left_name.to_string(),
                                elite: left_progress.elite,
                                rarity: left_progress.rarity,
                                level: left_progress.level,
                                mood: options.mood.max(0.0),
                            },
                            crate::support_facility::SupportOperator {
                                name: right_name.to_string(),
                                elite: right_progress.elite,
                                rarity: right_progress.rarity,
                                level: right_progress.level,
                                mood: options.mood.max(0.0),
                            },
                        ];
                        let members = vec![
                            crate::layout::AssignedOperator::from_progress(
                                left_name,
                                *left_progress,
                            ),
                            crate::layout::AssignedOperator::from_progress(
                                right_name,
                                *right_progress,
                            ),
                        ];
                        let mut trial = assignment.clone();
                        trial.set_room(room.id.clone(), members.clone());
                        if options.auto_rotation
                            && !super::super::work_budget::room_work_duration_fits(
                                blueprint, &trial, &room.id, instances,
                            )
                        {
                            continue;
                        }
                        let candidate_hours = layout.evaluation_hours(elapsed_hours)?;
                        let score = crate::meeting::evaluate_meeting(
                            &crate::support_facility::SupportRoomInput {
                                facility: crate::support_facility::SupportFacility::Meeting,
                                operators: support_ops,
                                capacity: room.operator_capacity(),
                                extra_recruit_slots,
                                elapsed_hours: candidate_hours,
                                external_speed_bonus_pct: 0.0,
                                layout: layout.clone(),
                            },
                            &registry,
                        )?
                        .skill_speed_bonus_pct;
                        let ordered = if left_name <= right_name {
                            (left_name, right_name, left_progress, right_progress)
                        } else {
                            (right_name, left_name, right_progress, left_progress)
                        };
                        if best_pair.as_ref().is_none_or(|best| {
                            score > best.0
                                || (score == best.0
                                    && (ordered.0.as_str(), ordered.1.as_str())
                                        < (best.1.as_str(), best.2.as_str()))
                        }) {
                            best_pair = Some((
                                score,
                                ordered.0.to_string(),
                                ordered.1.to_string(),
                                *ordered.2,
                                *ordered.3,
                            ));
                        }
                    }
                }
                if let Some((_, left_name, right_name, left_progress, right_progress)) = best_pair {
                    used.insert(left_name.clone());
                    used.insert(right_name.clone());
                    operators.push(crate::layout::AssignedOperator::from_progress(
                        &left_name,
                        left_progress,
                    ));
                    operators.push(crate::layout::AssignedOperator::from_progress(
                        &right_name,
                        right_progress,
                    ));
                    continue;
                }
            }

            let mut best: Option<(f64, String, crate::roster::OperatorProgress)> = None;
            for (name, progress) in candidate_names {
                if used.contains(name.as_str()) {
                    continue;
                }
                if operators.iter().any(|op| op.name == *name) {
                    continue;
                }
                // 会客室总是两人同房：候选评分用哨兵补足第二席，使
                // `SpeedIfSolo` 失效（solo 加成在双人房不生效），
                // `SpeedIfPartner` 搭档加成按哨兵不匹配记 0。
                let mut support_ops: Vec<_> = operators
                    .iter()
                    .map(|op| crate::support_facility::SupportOperator {
                        name: op.name.clone(),
                        elite: op.elite,
                        rarity: op.rarity,
                        level: op.level,
                        mood: f64::from(op.work_mood.unwrap_or(options.mood.max(0.0) as u8)),
                    })
                    .chain(std::iter::once(crate::support_facility::SupportOperator {
                        name: name.clone(),
                        elite: progress.elite,
                        rarity: progress.rarity,
                        level: progress.level,
                        mood: options.mood.max(0.0),
                    }))
                    .collect();
                if support_ops.len() < room.operator_capacity() {
                    support_ops.push(crate::support_facility::SupportOperator {
                        name: "\0meeting-partner-sentinel".to_string(),
                        elite: 0,
                        rarity: 0,
                        level: 1,
                        mood: options.mood.max(0.0),
                    });
                }
                let mut members = operators.clone();
                members.push(crate::layout::AssignedOperator::from_progress(
                    name, progress,
                ));
                let mut trial = assignment.clone();
                trial.set_room(room.id.clone(), members.clone());
                if options.auto_rotation
                    && !super::super::work_budget::room_work_duration_fits(
                        blueprint, &trial, &room.id, instances,
                    )
                {
                    continue;
                }
                let candidate_hours = layout.evaluation_hours(elapsed_hours)?;
                let score = crate::meeting::evaluate_meeting(
                    &crate::support_facility::SupportRoomInput {
                        facility: crate::support_facility::SupportFacility::Meeting,
                        operators: support_ops,
                        capacity: room.operator_capacity(),
                        extra_recruit_slots,
                        elapsed_hours: candidate_hours,
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

fn best_available_office_pct(
    run: &AssignmentRun<'_>,
    producer_layout: &crate::layout::LayoutContext,
) -> Result<Option<f64>> {
    if producer_layout.office_room_count == 0 {
        return Ok(None);
    }

    let office_occupants: HashSet<_> = run
        .assignment
        .iter_rooms()
        .filter(|room| {
            run.blueprint
                .rooms
                .iter()
                .any(|bp| bp.id == room.room_id && bp.kind == FacilityKind::Office)
        })
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
        .collect();
    let registry = crate::support_facility::SupportRegistry::load_default_shared()?;
    let extra_recruit_slots = run
        .blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let mut best_office_pct: Option<f64> = None;
    for name in run.operbox.roster().names() {
        if run.used.contains(name) && !office_occupants.contains(name) {
            continue;
        }
        let Some(progress) = run.operbox.progress_of(name) else {
            continue;
        };
        let office_pct = crate::office::evaluate_office(
            &crate::support_facility::SupportRoomInput {
                facility: crate::support_facility::SupportFacility::Office,
                operators: vec![crate::support_facility::SupportOperator {
                    name: name.clone(),
                    elite: progress.elite,
                    rarity: progress.rarity,
                    level: progress.level,
                    mood: run.options.mood.max(0.0),
                }],
                capacity: 1,
                extra_recruit_slots,
                elapsed_hours: run.options.shift_hours,
                external_speed_bonus_pct: 0.0,
                layout: producer_layout.clone(),
            },
            &registry,
        )?
        .skill_speed_bonus_pct;
        best_office_pct = Some(best_office_pct.map_or(office_pct, |best| best.max(office_pct)));
    }
    Ok(best_office_pct)
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
    let saileach_best_office_pct = best_available_office_pct(&run, producer_layout)?;
    if !fill_peak_control(
        &mut run,
        plan,
        control_pool,
        producer_layout,
        control_exclusions,
        dynamic_set,
        saileach_best_office_pct,
        stable_order,
    )? {
        return Ok(None);
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
fn fill_peak_control(
    run: &mut AssignmentRun<'_>,
    plan: &AssignmentPlan,
    control_pool: &ControlPool,
    producer_layout: &crate::layout::LayoutContext,
    control_exclusions: &HashSet<String>,
    dynamic_set: Option<(&HashSet<String>, &HashSet<String>)>,
    saileach_best_office_pct: Option<f64>,
    stable_order: usize,
) -> Result<bool> {
    let observe_control =
        std::env::var_os("ARKNIGHTS_INFRA_OBSERVE_CONTROL").is_some_and(|value| value == "1");
    macro_rules! observe {
        ($($arg:tt)*) => {
            if observe_control {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "[INFRA-CONTROL-OBSERVE] {}",
                    format_args!($($arg)*)
                );
            }
        };
    }
    let control_names: HashSet<String> = run
        .assignment
        .control_operators()
        .into_iter()
        .map(|operator| operator.name)
        .collect();
    let saileach_required = control_names.contains("琴柳")
        || dynamic_set.is_some_and(|(required, _)| required.contains("琴柳"))
        || plan
            .control_candidate_requirements
            .iter()
            .any(|requirement| {
                requirement.min_count > 0
                    && requirement.candidates.len() == 1
                    && requirement.candidates.iter().any(|name| name == "琴柳")
            });
    if run.assignment.control_operators().len() < 5 {
        if let Some((required, excluded)) = dynamic_set {
            if !assign_control_matching_dynamic_set_with_saileach(
                &mut run.assignment,
                control_pool,
                run.table,
                producer_layout,
                run.options,
                &plan.control_candidate_requirements,
                required,
                excluded,
                control_exclusions,
                saileach_best_office_pct,
                saileach_required,
                None,
                &mut run.used,
            )? {
                observe!(
                    "prepare_return_none mask={stable_order} reason=dynamic_control_no_legal_fill"
                );
                return Ok(false);
            }
        } else {
            assign_control_with_saileach(
                &mut run.assignment,
                control_pool,
                run.table,
                producer_layout,
                run.options,
                &plan.control_candidate_requirements,
                control_exclusions,
                saileach_best_office_pct,
                saileach_required,
                None,
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
            let selected_names: Vec<_> = run
                .assignment
                .control_operators()
                .into_iter()
                .map(|operator| operator.name)
                .collect();
            observe!("prepare_return_none mask={stable_order} reason=dynamic_presence_mismatch selected={selected_names:?} missing_required={:?} present_excluded={:?}",
                required.difference(&control_names).collect::<std::collections::BTreeSet<_>>(),
                control_names.intersection(excluded).collect::<std::collections::BTreeSet<_>>());
            return Ok(false);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn finish_peak_prefix_candidate<'a>(
    run: AssignmentRun<'a>,
    plan: &AssignmentPlan,
    base_trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    power_pool: &PowerPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
    stable_order: usize,
) -> Result<Option<PeakPrefixCandidate<'a>>> {
    let mut run = finish_peak_prefix(
        run,
        plan,
        base_trade_pool,
        manu_pool,
        power_pool,
        gold_lines,
        forbid_same_room,
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

#[allow(clippy::too_many_arguments)]
fn finish_peak_prefix<'a>(
    mut run: AssignmentRun<'a>,
    plan: &AssignmentPlan,
    base_trade_pool: &TradePool,
    manu_pool: &crate::pool::ManuPool,
    power_pool: &PowerPool,
    gold_lines: u32,
    forbid_same_room: &[(String, String)],
) -> Result<AssignmentRun<'a>> {
    place_system_producers(
        run.blueprint,
        run.operbox,
        &plan.producers,
        &mut run.assignment,
        &mut run.used,
    )?;
    if !run.options.auto_rotation {
        assign_dorm_producers(
            run.blueprint,
            run.operbox,
            run.instances,
            &mut run.assignment,
            &mut run.used,
        )?;
    }
    if !operator_is_in_normal_base(run.blueprint, &run.assignment, ULPIANUS_NAME) {
        // Deepcolor's trade bonus requires Ulpianus to remain in the normal base.
        // This pipeline does not place Ulpianus beyond the producer anchor, so
        // exclude Deepcolor from every subsequent peak trade branch.
        run.used.insert(SPHINX_NAME.to_string());
    }
    let layout = run.resolve_snapshot(true)?;
    let mut trade_pool = base_trade_pool.clone();
    if karlan_precision_active(&layout.global_inject) {
        add_owned_jie_market_to_trade_pool(&mut trade_pool, run.operbox, run.instances, run.table);
    }
    assign_power_stations(
        run.blueprint,
        power_pool,
        run.table,
        &layout,
        run.options,
        &mut run.assignment,
        &mut run.used,
        run.instances,
    )?;
    assign_trade_remainder_with_plan_anchors(
        run.blueprint,
        &trade_pool,
        run.table,
        run.instances,
        gold_lines,
        run.options,
        Some(run.durin_plan),
        &plan.anchors,
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
    Ok(run)
}

fn operator_is_in_normal_base(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    name: &str,
) -> bool {
    assignment.iter_rooms().any(|room| {
        blueprint.room(&room.room_id).is_some_and(|spec| {
            spec.kind != FacilityKind::MeetingRoom
                && room.operators.iter().any(|operator| operator.name == name)
        })
    }) && assignment
        .training_assist
        .as_ref()
        .is_none_or(|assist| assist.name != name)
}

fn trade_manufacture_efficiency_totals(
    assignment: &BaseAssignment,
) -> (crate::Efficiency, crate::Efficiency) {
    assignment.iter_rooms().fold(
        (crate::Efficiency::ZERO, crate::Efficiency::ZERO),
        |(trade_total, manufacture_total), room| {
            let Some(snapshot) = &room.efficiency else {
                return (trade_total, manufacture_total);
            };
            (
                trade_total + snapshot.trade_total_efficiency,
                manufacture_total + snapshot.manufacture_total_efficiency,
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
        if crate::response_dependency::control_buff_is_automatically_excluded(
            table,
            entry.buff_ids.iter().map(String::as_str),
        )? {
            continue;
        }
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
pub(super) fn forbid_same_room_pairs(plan: &AssignmentPlan) -> Vec<(String, String)> {
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
