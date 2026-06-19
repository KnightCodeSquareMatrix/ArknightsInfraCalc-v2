use std::sync::Arc;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{assign_base_greedy, resolve_base, AssignBaseOptions, BaseBlueprint};
use crate::manufacture::ManuSearchRecipeMode;
use crate::operbox::OperBox;
use crate::pool::{build_manufacture_pool, build_trade_pool};
use crate::schedule::schedule_team_rotation;
use crate::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchReport, TradeSearchOptions,
    ManuSearchOptions, TradeSearchReport,
};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};

use super::eval::{
    manu_report_from_assignment, reference_shift_assignment, trade_report_from_assignment,
};

#[derive(Debug, Clone)]
pub struct LayoutProbe {
    pub owned: usize,
    pub tier_up_owned: usize,
    pub trade_pool_ready: usize,
    pub manu_pool_ready: usize,
    pub trade_report: TradeSearchReport,
    pub manu_report: ManuSearchReport,
    pub rotation: crate::schedule::TeamRotationReport,
}

/// 用户 αβγ 排班 + 从排班编制提取分域指标（current 侧，无全池 search）。
pub fn run_user_rotation_probe(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
) -> Result<LayoutProbe> {
    let rotation = schedule_team_rotation(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let assignment = reference_shift_assignment(&rotation);
    let trade_report =
        trade_report_from_assignment(blueprint, assignment, instances, table, durin_plan)?;
    let manu_report =
        manu_report_from_assignment(blueprint, assignment, instances, table, durin_plan)?;

    let tier_up_owned = operbox
        .owned
        .values()
        .filter(|p| PromotionTier::is_tier_up(**p))
        .count();
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let manu_pool = build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;

    Ok(LayoutProbe {
        owned: operbox.owned_count(),
        tier_up_owned,
        trade_pool_ready: trade_pool.stats(3).ready,
        manu_pool_ready: manu_pool.stats(3).ready,
        trade_report,
        manu_report,
        rotation,
    })
}

/// 全池 search + 用户排班（旧路径，仅 benchmark / 调试）。
pub fn run_layout_probe(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
) -> Result<LayoutProbe> {
    let assignment = assign_base_greedy(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;
    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let resolved = resolve_base(
        blueprint,
        &assignment,
        Some(instances),
        Some(table),
        24.0,
        Some(durin_plan),
    )?;
    let layout = Arc::new(resolved.layout_snapshot());

    let trade_scenario = blueprint.trade_station_scenario();
    let trade_order_mode = if trade_scenario.total_stations() == 0 {
        TradeSearchOrderMode::Single(TradeOrderKind::Gold)
    } else {
        TradeSearchOrderMode::Stations(trade_scenario)
    };

    let trade_roster = operbox.trade_roster(instances);
    let trade_pool = build_trade_pool(&trade_roster, instances, table)?;
    let trade_report = search_trade_triples(
        &trade_pool,
        table,
        &TradeSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            gold_production_lines: blueprint.gold_manu_line_count(),
            order_mode: trade_order_mode,
            ..TradeSearchOptions::default()
        },
    )?;

    let manu_roster = operbox.manufacture_roster(instances);
    let manu_pool = build_manufacture_pool(&manu_roster, instances, table)?;
    let manu_report = search_manufacture_triples(
        &manu_pool,
        table,
        &ManuSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            recipe_mode: ManuSearchRecipeMode::Lines(blueprint.manu_line_scenario()),
            ..ManuSearchOptions::default()
        },
    )?;

    let rotation = schedule_team_rotation(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;

    let tier_up_owned = operbox
        .owned
        .values()
        .filter(|p| PromotionTier::is_tier_up(**p))
        .count();

    Ok(LayoutProbe {
        owned: operbox.owned_count(),
        tier_up_owned,
        trade_pool_ready: trade_pool.stats(3).ready,
        manu_pool_ready: manu_pool.stats(3).ready,
        trade_report,
        manu_report,
        rotation,
    })
}
