//! 单班填房流水线主体（ADR 0001 决策 A）。
//!
//! 阶段顺序与 resolve 时机的唯一事实源。facade（`assign.rs`）只负责构建
//! `skip_system_ids` 与 `build_plan`，随后委托给本模块。
//! 行为与拆分前的 `assign_shift_with_plan_skip` 等价：resolve 快照次数、
//! producer 落位顺序、贸易/制造/发电搜索顺序均未改变。

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::orchestrate::{execute_plan, AssignmentPlan};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system_integrity::{
    apply_rosemary_plan, evaluate_rosemary, EvaluateContext, RosemaryVerdict,
};
use crate::operbox::OperBox;
use crate::pool::{
    add_jie_market_to_trade_pool, build_control_pool, build_manufacture_pool, build_power_pool,
    build_trade_pool, karlan_precision_active,
};
use crate::skill_table::SkillTable;

use super::control_fill::assign_control;
use super::manufacture_fill::assign_manufacture_lines;
use super::power_fill::assign_power_stations;
use super::producer_fill::{
    assign_dorm_producers, assign_perception_producers, assign_sphinx_urrbian_dorm_anchor,
};
use super::run::{AssignmentRun, StageTimer};
use super::trade_fill::{assign_trade_jie_remainder, assign_trade_remainder};
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

    // resolve #1：仅 producer 视角快照（无 skill table）。
    let producer_layout = run.resolve_snapshot(false)?;
    timer.mark("resolve(1st)");

    // 中枢补位（仅 Peak，且未满 5 人）。
    if mode == AssignShiftMode::Peak && run.assignment.control_operators().len() < 5 {
        let mut control_pool =
            build_control_pool(&operbox.control_roster(instances), instances, table)?;
        tag_pool_from_plan(plan, &mut control_pool);
        assign_control(
            &mut run.assignment,
            &control_pool,
            table,
            &producer_layout,
            options,
            &mut run.used,
        )?;
    }
    timer.mark("中枢");

    // 感知 / 宿舍 producer（仅 Peak）。
    if mode == AssignShiftMode::Peak {
        assign_perception_producers(blueprint, operbox, &mut run.assignment, &mut run.used)?;
        // 迷迭香感知链：代码化体系层钉迷迭香制造 anchor（黑键不锚定，走贸易贪心 +
        // 上2休1 绑定）。producer（夕/絮雨/爱丽丝/车尔尼）已由上行落位，
        // apply_rosemary_plan 对已 used 的 producer 静默跳过，仅净新增迷迭香 anchor。
        let ctx = EvaluateContext::new(blueprint, operbox, mode);
        if let RosemaryVerdict::Activate(rplan) = evaluate_rosemary(&ctx) {
            apply_rosemary_plan(blueprint, &rplan, &mut run.assignment, &mut run.used)?;
        }
        assign_sphinx_urrbian_dorm_anchor(blueprint, operbox, &mut run.assignment, &mut run.used);
        assign_dorm_producers(
            blueprint,
            operbox,
            instances,
            &mut run.assignment,
            &mut run.used,
        )?;
    }
    timer.mark("perception+dorm");

    // resolve #2：producer 落位后含 skill table 的全局快照。
    let layout = run.resolve_snapshot(true)?;
    timer.mark("resolve(2nd)");

    // 建池（贸易 / 制造 / 发电）+ tier 标注 + anchor 注入。
    let mut trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    if karlan_precision_active(&layout.global_inject) {
        add_jie_market_to_trade_pool(&mut trade_pool, instances, table);
    }
    let mut manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let mut power_pool = build_power_pool(&operbox.power_roster(instances), instances, table)?;
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
    timer.mark("建池");

    match mode {
        AssignShiftMode::Peak => {
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

            // resolve #3：发电落位后贸易搜索前快照。
            let trade_layout = run.resolve_snapshot(true)?;
            timer.mark("resolve(3rd)");
            assign_trade_remainder(
                blueprint,
                &trade_pool,
                table,
                &trade_layout,
                gold_lines,
                options,
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("贸易余站");

            // resolve #4：贸易落位后制造搜索前快照。
            let manu_layout = run.resolve_snapshot(true)?;
            timer.mark("resolve(4th)");
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &manu_layout,
                options,
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("制造");
        }
        AssignShiftMode::Recovery => {
            assign_trade_jie_remainder(
                blueprint,
                &trade_pool,
                table,
                instances,
                &layout,
                gold_lines,
                options,
                &mut run.assignment,
                &mut run.used,
            )?;
            timer.mark("trade孑余站");
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &layout,
                options,
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

    timer.report();
    Ok(run.assignment)
}
