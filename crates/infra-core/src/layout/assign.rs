use std::collections::HashSet;
use std::time::Instant;

mod commit;
mod control_fill;
mod manufacture_fill;
mod pipeline;
mod power_fill;
mod producer_fill;
mod run;
mod team_fill;
pub(crate) mod trade_fill;

pub(crate) use control_fill::assign_control_with_saileach;
pub(crate) use manufacture_fill::assign_manu_room_with_anchors;
pub(crate) use manufacture_fill::refill_manu_room_with_core;
pub(crate) use power_fill::filter_power_pool;
pub use power_fill::{assign_power_rooms, assign_power_stations};
pub use team_fill::{assign_team_gamma_half, assign_team_producer_rooms};
pub use trade_fill::blackkey_witch_same_trade_room;
use trade_fill::skip_trade_core_registry_systems;
pub(crate) use trade_fill::{
    beta_tailor_names, refill_trade_room_with_core, refill_witch_room_with_tailor,
};

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, FacilityKind};
use crate::layout::orchestrate::AssignmentPlan;
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::{explain_registry_systems, SlotFillMode, SystemExplainReport};
use crate::notes::annotate_assignment;
use crate::operbox::OperBox;
use crate::pool::{compile_operator_atoms, ManuPoolEntry, TradePoolEntry};
use crate::scoring::{ScoringPolicyId, TradeManufactureEfficiencyWeights};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

/// 未被 Plan 认领的金单贸易站的 role 首选顺序偏好。
/// 只改变 `pick_trade_meta_then_plain` 的候选尝试顺序（顺序驱动、不看效率），
/// 首选 role 不可行时回退默认顺序，不构成 required admission。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeRolePreference {
    Witch,
}

#[derive(Debug, Clone)]
pub struct AssignBaseOptions {
    /// 建议模式能力边界，沿用同一 Plan / 搜索 / 完成步骤。
    pub auto_rotation: bool,
    pub top_k: usize,
    pub mood: f64,
    pub shift_hours: f64,
    /// Initial single-operator control roster ETA window for an explicit profile.
    /// No later search, fill or final-assignment mood admission is performed.
    pub control_work_hours: Option<f64>,
    /// Variant C: peak-step registry selection rejects a system whose own control pins
    /// cannot sustain this many continuous hours. None disables the check.
    pub registry_sustain_hours: Option<f64>,
    /// 中枢分配时跳过 standalone_roster 白名单过滤（轮换编排中体系绑定干员可能不在白名单内）。
    pub skip_standalone_control: bool,
    /// `system_id=alternative_id` 解析后的通用体系路径偏好。
    pub system_preferences: std::collections::HashMap<String, String>,
    /// 金单贸易站自然搜索的 role 首选顺序偏好（B 分支：巫恋组优先）。
    pub trade_role_preference: Option<TradeRolePreference>,
    /// 启用菲亚候选分支时，把 N/A/B 三份最终排班报告写入该目录（调试/对比用）。
    pub fiammetta_candidates_out: Option<std::path::PathBuf>,
    /// 菲亚梅塔主力回岗覆盖开关；`false` 时 ABC 排班不生成回岗动作。
    pub fiammetta_enable: bool,
    pub support_combinations_enable: bool,
    /// Dynamic producer prefix comparator. The public baseline remains V0;
    /// shadow runs opt into V1 explicitly.
    pub dynamic_producer_policy: ScoringPolicyId,
    pub trade_manufacture_weights: TradeManufactureEfficiencyWeights,
    /// Frozen timed-rotation preclaim room.  A shadow production rebuild must
    /// leave this room and its anchored members untouched.
    pub preclaimed_factory_room: Option<crate::layout::RoomId>,
}

impl Default for AssignBaseOptions {
    fn default() -> Self {
        Self {
            auto_rotation: false,
            top_k: 20,
            mood: 24.0,
            shift_hours: 24.0,
            control_work_hours: None,
            registry_sustain_hours: None,
            skip_standalone_control: false,
            system_preferences: std::collections::HashMap::new(),
            trade_role_preference: None,
            fiammetta_candidates_out: None,
            fiammetta_enable: true,
            support_combinations_enable: false,
            dynamic_producer_policy: ScoringPolicyId::ControlInjectRawSumV0,
            trade_manufacture_weights: TradeManufactureEfficiencyWeights::default(),
            preclaimed_factory_room: None,
        }
    }
}

/// 全基建单班进驻编制：producer 落位 → resolve → consumer 搜 + `used` 顺序认领。
pub fn assign_base_greedy(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<BaseAssignment> {
    assign_shift(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
    )
}

/// `assign_shift` 完整输出：编制 + 编排计划（轮换层只读 plan，不重跑 `build_plan`）。
#[derive(Debug, Clone)]
pub struct AssignShiftResult {
    pub assignment: BaseAssignment,
    pub plan: AssignmentPlan,
}

/// 单班进驻；`seed` 非空时保留已钉死房间（中枢/宿舍），仅补贸易/制造/发电。
pub fn assign_shift(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<BaseAssignment> {
    Ok(
        assign_shift_with_plan(blueprint, operbox, instances, table, options, mode, seed)?
            .assignment,
    )
}

/// 解释主路径 `assign_shift` 会如何认领 registry systems。
///
/// 这会套用与实际排班一致的 trade role registry skip 规则，但不执行设施搜索或效率结算。
pub fn explain_assignment_systems(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> SystemExplainReport {
    let mut skip_system_ids = HashSet::new();
    skip_trade_core_registry_systems(&mut skip_system_ids);
    let used = seed.operator_names();
    explain_registry_systems(blueprint, operbox, mode, seed, &used, &skip_system_ids)
}

/// 从编排计划提取 claimed 干员名并按 tier 标注池条目。
pub(super) fn tag_pool_from_plan<T: crate::pool::HasName + crate::pool::TierTagged>(
    plan: &AssignmentPlan,
    pool: &mut crate::pool::PoolCore<T>,
) {
    for claim in &plan.registry_claims {
        let mut names = HashSet::new();
        for slot in &claim.slots {
            for op in &slot.operators {
                names.insert(op.name.clone());
            }
        }
        pool.tag_tier(&names, claim.tier);
    }
}

pub(super) fn inject_search_anchor_pool_entries(
    plan: &AssignmentPlan,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    trade_pool: &mut crate::pool::TradePool,
    manu_pool: &mut crate::pool::ManuPool,
) {
    for claim in &plan.registry_claims {
        for slot in &claim.slots {
            if slot.fill != SlotFillMode::Search {
                continue;
            }
            for op in &slot.operators {
                match slot.facility {
                    FacilityKind::TradePost => {
                        inject_trade_anchor(op.name.as_str(), operbox, instances, table, trade_pool)
                    }
                    FacilityKind::Factory => {
                        inject_manu_anchor(op.name.as_str(), operbox, instances, table, manu_pool)
                    }
                    _ => {}
                }
            }
        }
    }
}

fn inject_trade_anchor(
    name: &str,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    pool: &mut crate::pool::TradePool,
) {
    if pool.entry(name).is_some() {
        return;
    }
    let Some(progress) = operbox.progress_of(name) else {
        return;
    };
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_trade_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return;
    }
    pool.entries.push(TradePoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids: buff_ids.clone(),
        tags: instances.tags_for(name, tier),
        compiled_atoms: compile_operator_atoms(&buff_ids, table),
        flat_eff_hint: 0.0,
        is_mechanic: true,
        tier: crate::layout::tier::OperatorTier::CrossStation,
    });
}

fn inject_manu_anchor(
    name: &str,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    pool: &mut crate::pool::ManuPool,
) {
    if pool.entry(name).is_some() {
        return;
    }
    let Some(progress) = operbox.progress_of(name) else {
        return;
    };
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_manufacture_buff_ids(name, tier);
    if buff_ids.is_empty() {
        return;
    }
    pool.entries.push(ManuPoolEntry {
        compiled_atoms: Some(table.compile_manufacture_atoms(&buff_ids)),
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags: instances.tags_for(name, tier),
        flat_eff_hint: 0.0,
        has_l2_delegate: false,
        tier: crate::layout::tier::OperatorTier::CrossStation,
    });
}

/// 同 [`assign_shift`]，额外返回编排 `AssignmentPlan`（peak 班供 αβγ 轮换只读）。
pub fn assign_shift_with_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<AssignShiftResult> {
    let _memo_scope = crate::search::ensure_search_memo_scope();
    let enabled = assign_shift_with_preclaimed_plan(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        seed,
        seed,
        &HashSet::new(),
        |_| Ok(()),
    )?;
    if !four_gold_selected(&enabled.plan) {
        let mut enabled = enabled;
        complete_standalone_assignment(
            blueprint,
            operbox,
            instances,
            table,
            options,
            &mut enabled,
        )?;
        return Ok(enabled);
    }

    let mut disabled_skip = HashSet::new();
    disabled_skip.insert("pozyomka_durin".to_string());
    let disabled = assign_shift_with_preclaimed_plan(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        seed,
        seed,
        &disabled_skip,
        |_| Ok(()),
    )?;
    let mut enabled = enabled;
    let mut disabled = disabled;
    complete_standalone_assignment(blueprint, operbox, instances, table, options, &mut enabled)?;
    complete_standalone_assignment(blueprint, operbox, instances, table, options, &mut disabled)?;
    Ok(select_four_gold_trade_winner(enabled, disabled))
}

pub(crate) fn complete_standalone_assignment(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    result: &mut AssignShiftResult,
) -> Result<()> {
    let control_operbox;
    let forbidden_control_names = if options.auto_rotation {
        control_operbox = crate::pool::auto_rotation_control_roster(operbox, instances, table)?;
        operbox
            .owned
            .keys()
            .filter(|name| !control_operbox.owns(name))
            .cloned()
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let mut excluded: HashSet<_> = result.plan.excluded_operators.iter().cloned().collect();
    excluded.extend(
        result
            .plan
            .anchors
            .iter()
            .map(|anchor| anchor.operator.clone()),
    );
    excluded.extend(
        result
            .plan
            .rotation_reserves
            .iter()
            .flat_map(|reserve| reserve.operators.iter().cloned()),
    );
    let layout = crate::layout::resolve_base(
        blueprint,
        &result.assignment,
        Some(instances),
        Some(table),
        options.mood,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?
    .layout;
    excluded.extend(result.assignment.occupied_operator_names());
    let office_candidates = crate::office::rank_candidates(
        operbox,
        &layout,
        options.mood,
        options.shift_hours,
        &excluded,
    );
    crate::office::fill_vacancies(
        &mut result.assignment,
        blueprint,
        &office_candidates,
        |_, trial, room, candidate| {
            crate::layout::plan_admits_workplace_candidate(&result.plan, trial, room, candidate)
        },
    );
    let roster = operbox.roster_excluding(&excluded);
    let mut candidates: Vec<_> = roster
        .names()
        .filter_map(|name| {
            roster
                .progress(name)
                .map(|progress| crate::layout::AssignedOperator::from_progress(name, progress))
        })
        .collect();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    crate::layout::fill_workplace_vacancies(
        blueprint,
        &mut result.assignment,
        &candidates,
        instances,
        table,
        |trial, room, candidate| {
            if room.kind == FacilityKind::ControlCenter
                && forbidden_control_names.contains(&candidate.name)
            {
                return false;
            }
            crate::layout::plan_admits_workplace_candidate(&result.plan, trial, room, candidate)
        },
    )?;
    refresh_standalone_efficiency(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &mut result.assignment,
    )?;
    annotate_assignment(blueprint, &mut result.assignment);
    Ok(())
}

pub(crate) fn refresh_standalone_efficiency(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
) -> Result<()> {
    manufacture_fill::refresh_manufacture_efficiency_snapshots(
        blueprint,
        assignment,
        instances,
        table,
        options,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?;
    trade_fill::refresh_trade_efficiency_snapshots(
        blueprint,
        assignment,
        instances,
        table,
        options,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?;
    if options.auto_rotation {
        let resolved = crate::layout::resolve_base(
            blueprint,
            assignment,
            Some(instances),
            Some(table),
            options.mood,
            None,
        )?;
        for room in resolved.power_rooms {
            let hours = room.layout.evaluation_hours(options.shift_hours)?;
            let solved = crate::power::solve_power(
                &crate::power::PowerRoomInput {
                    operator: room.operator,
                    mood: options.mood,
                    shift_hours: hours,
                    layout: room.layout,
                },
                table,
            )?;
            let operators = assignment.operators_in(&room.id).to_vec();
            assignment.set_room_with_efficiency(
                room.id,
                operators,
                Some(crate::layout::RoomEfficiencySnapshot {
                    power_final_efficiency: solved.final_efficiency,
                    ..Default::default()
                }),
            );
        }
    }
    Ok(())
}

fn four_gold_selected(plan: &AssignmentPlan) -> bool {
    plan.selected_rules
        .iter()
        .any(|rule| rule.rule_id == "pozyomka_durin")
}

fn trade_assignment_score(assignment: &BaseAssignment) -> crate::Efficiency {
    assignment
        .iter_rooms()
        .filter_map(|room| room.efficiency.as_ref())
        .map(|snapshot| {
            snapshot.trade_total_efficiency
                * if snapshot.trade_unit_output_multiplier.is_zero() {
                    crate::Efficiency::ONE
                } else {
                    snapshot.trade_unit_output_multiplier
                }
        })
        .sum()
}

fn select_four_gold_trade_winner(
    enabled: AssignShiftResult,
    disabled: AssignShiftResult,
) -> AssignShiftResult {
    let enabled_score = trade_assignment_score(&enabled.assignment);
    let disabled_score = trade_assignment_score(&disabled.assignment);
    if enabled_score > disabled_score {
        eprintln!(
            "[four-gold-candidate] winner=enabled enabled_trade={} disabled_trade={}",
            enabled_score, disabled_score
        );
        enabled
    } else {
        eprintln!(
            "[four-gold-candidate] winner=disabled enabled_trade={} disabled_trade={}",
            enabled_score, disabled_score
        );
        disabled
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_shift_with_preclaimed_four_gold_comparison<F>(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    planning_seed: &BaseAssignment,
    execution_seed: &BaseAssignment,
    skip_system_ids: &HashSet<String>,
    augment_plan: F,
) -> Result<AssignShiftResult>
where
    F: Fn(&mut AssignmentPlan) -> Result<()>,
{
    let enabled = assign_shift_with_preclaimed_plan(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        planning_seed,
        execution_seed,
        skip_system_ids,
        &augment_plan,
    )?;
    if skip_system_ids.contains("pozyomka_durin") || !four_gold_selected(&enabled.plan) {
        return Ok(enabled);
    }

    let mut ordinary_skip = skip_system_ids.clone();
    ordinary_skip.insert("pozyomka_durin".to_string());
    let disabled = assign_shift_with_preclaimed_plan(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        planning_seed,
        execution_seed,
        &ordinary_skip,
        &augment_plan,
    )?;
    Ok(select_four_gold_trade_winner(enabled, disabled))
}

/// 同 [`assign_shift_with_plan`]，额外允许跳过指定体系。
pub fn assign_shift_with_plan_skip(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &HashSet<String>,
) -> Result<AssignShiftResult> {
    assign_shift_with_preclaimed_four_gold_comparison(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        seed,
        seed,
        skip_system_ids,
        |_| Ok(()),
    )
}

/// Build a plan around `planning_seed`, then execute the augmented plan from
/// `execution_seed`. Timed profiles use this to make hard preclaims visible to
/// ordinary system selection without double-placing their final anchors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_shift_with_preclaimed_plan<F>(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    planning_seed: &BaseAssignment,
    execution_seed: &BaseAssignment,
    skip_system_ids: &HashSet<String>,
    mut augment_plan: F,
) -> Result<AssignShiftResult>
where
    F: FnMut(&mut AssignmentPlan) -> Result<()>,
{
    let _memo_scope = crate::search::ensure_search_memo_scope();
    blueprint.validate()?;

    let mut planning_seed = planning_seed.clone();
    let mut execution_seed = execution_seed.clone();
    planning_seed.work_hours = Some(options.shift_hours);
    execution_seed.work_hours = Some(options.shift_hours);
    if options.auto_rotation {
        planning_seed.auto_rotation = true;
        execution_seed.auto_rotation = true;
    }

    let mut skip_system_ids = skip_system_ids.clone();
    skip_trade_core_registry_systems(&mut skip_system_ids);
    let mut disabled_conditional_packs = HashSet::new();
    let mut skipped_optional_trade_roles = HashSet::new();
    loop {
        let stage_started = Instant::now();
        let stage_before = crate::profile::solver_perf_snapshot();
        let plan_result = crate::layout::orchestrate::build_plan_with_runtime_and_disabled_packs(
            blueprint,
            operbox,
            instances,
            table,
            options.mood,
            &options.system_preferences,
            &skipped_optional_trade_roles,
            options.support_combinations_enable,
            options.registry_sustain_hours,
            options.control_work_hours,
            mode,
            &planning_seed,
            &skip_system_ids,
            &mut disabled_conditional_packs,
        );
        let stage_after = crate::profile::solver_perf_snapshot();
        crate::profile::emit_solver_stage(
            "shift",
            "plan_build",
            if plan_result.is_ok() { "ok" } else { "error" },
            stage_started.elapsed(),
            stage_before,
            stage_after,
        );
        let mut plan = plan_result?;
        augment_plan(&mut plan)?;

        let stage_started = Instant::now();
        let stage_before = crate::profile::solver_perf_snapshot();
        let assignment_result = pipeline::run_shift_pipeline(
            blueprint,
            operbox,
            instances,
            table,
            options,
            mode,
            &execution_seed,
            &plan,
        );
        let stage_after = crate::profile::solver_perf_snapshot();
        crate::profile::emit_solver_stage(
            "shift",
            "pipeline",
            if assignment_result.is_ok() {
                "ok"
            } else {
                "error"
            },
            stage_started.elapsed(),
            stage_before,
            stage_after,
        );
        let assignment = match assignment_result {
            Ok(assignment) => assignment,
            Err(Error::OptionalTradeRoleInfeasible {
                system_id,
                role_id,
                operator,
                message,
            }) => {
                let retry = (system_id, role_id, operator);
                if skipped_optional_trade_roles.len() >= 4
                    || !skipped_optional_trade_roles.insert(retry.clone())
                {
                    return Err(Error::msg(message));
                }
                eprintln!(
                    "[plan-retry] omit optional trade recommendation {}/{}/{} after infeasible required fill: {}",
                    retry.0, retry.1, retry.2, message
                );
                continue;
            }
            Err(Error::GoldFlowCoreInfeasible { message }) => {
                if !skip_system_ids.insert("pozyomka_durin".to_string()) {
                    return Err(Error::msg(message));
                }
                eprintln!(
                    "[plan-fallback] return the three gold-flow operators to ordinary trade search after the complete core could not fill: {message}"
                );
                continue;
            }
            Err(error) => return Err(error),
        };

        let requires_final_pack_check = plan.rotation_reserves.iter().any(|reserve| {
            reserve.require_pre_split_halves
                && reserve.reuse_policy
                    == crate::layout::orchestrate::ReserveReusePolicy::EveryEligibleHalf
        });
        if requires_final_pack_check {
            let mut validation_plan = plan.clone();
            validation_plan.resolve_actual_producer_dependencies(
                blueprint,
                &assignment,
                instances,
                table,
            )?;
            if let Some(pack_key) = crate::layout::orchestrate::guarded_pack_violation(
                blueprint,
                &assignment,
                &validation_plan,
            )? {
                if !disabled_conditional_packs.insert(pack_key.clone()) {
                    return Err(Error::msg(format!(
                        "conditional pack {pack_key} remained rotation-infeasible after final assignment downgrade"
                    )));
                }
                continue;
            }
        }

        let mut assignment = assignment;
        annotate_assignment(blueprint, &mut assignment);
        return Ok(AssignShiftResult { assignment, plan });
    }
}

/// 编制内所有上岗干员。
pub fn assignment_operator_names(assignment: &BaseAssignment) -> HashSet<String> {
    assignment.operator_names()
}

/// 贸易 / 制造 / 发电岗位干员（跨班互斥池）。
pub fn rotating_workers(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> HashSet<String> {
    let rotating_kinds = [
        FacilityKind::TradePost,
        FacilityKind::Factory,
        FacilityKind::PowerPlant,
    ];
    let mut names = HashSet::new();
    for room in assignment.iter_rooms() {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !rotating_kinds.contains(&bp.kind) {
            continue;
        }
        for op in &room.operators {
            names.insert(op.name.clone());
        }
    }
    names
}

/// 宿舍 + 办公室感知 producer（三班钉死，从高峰班拷贝）。
///
/// 中枢在 `schedule_team_rotation` 中按 αβγ 队伍轮休重分配，不在这里钉死。
pub fn pinned_assignment(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> BaseAssignment {
    pinned_assignment_excluding(assignment, blueprint, &HashSet::new())
}

/// 生成宿舍共享脚手架，并允许调用方从中排除 peak 办公室成员。
///
/// 定时轮换的普通办公室不在 shared 中钉死，而是在每个班次的当前剩余池中重新选择；
/// 真实绑定成员仍由 schedule 根据实际 `shift_bind` 处理。
pub fn pinned_assignment_excluding(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    excluded: &HashSet<String>,
) -> BaseAssignment {
    let mut pinned = BaseAssignment::default();
    for room in assignment.iter_rooms() {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office) {
            continue;
        }
        let mut room = room.clone();
        room.operators.retain(|op| !excluded.contains(&op.name));
        if room.operators.is_empty() {
            continue;
        }
        pinned.set_room_assignment(room);
    }
    pinned
}
