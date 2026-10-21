use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{assign_base_greedy, resolve_base, AssignBaseOptions, BaseBlueprint};
use crate::manufacture::ManuSearchRecipeMode;
use crate::operbox::OperBox;
use crate::pool::{build_manufacture_pool, build_trade_pool};
use crate::schedule::{
    schedule_team_rotation, schedule_timed_rotation_request, TimedRotationProfile,
    TimedRotationRequest,
};
use crate::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, ManuSearchReport,
    TradeSearchOptions, TradeSearchReport,
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
    pub manufacture_pool_ready: usize,
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
    run_user_rotation_probe_with_profile(
        blueprint,
        operbox,
        instances,
        table,
        top_k,
        TimedRotationProfile::default(),
    )
}

pub fn run_user_rotation_probe_with_profile(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
    profile: TimedRotationProfile,
) -> Result<LayoutProbe> {
    run_user_rotation_probe_with_profile_and_preferences(
        blueprint,
        operbox,
        instances,
        table,
        top_k,
        profile,
        &HashMap::new(),
        true,
        false,
        None,
    )
}

pub fn run_user_rotation_probe_with_profile_and_preferences(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
    profile: TimedRotationProfile,
    system_preferences: &HashMap<String, String>,
    fiammetta_enable: bool,
    support_combinations_enable: bool,
    fiammetta_candidates_out: Option<std::path::PathBuf>,
) -> Result<LayoutProbe> {
    run_user_rotation_probe_with_request_and_preferences(
        blueprint,
        operbox,
        instances,
        table,
        top_k,
        TimedRotationRequest::Named(profile),
        system_preferences,
        fiammetta_enable,
        support_combinations_enable,
        fiammetta_candidates_out,
    )
}

pub fn run_user_rotation_probe_with_request_and_preferences(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
    rotation_request: TimedRotationRequest,
    system_preferences: &HashMap<String, String>,
    fiammetta_enable: bool,
    support_combinations_enable: bool,
    fiammetta_candidates_out: Option<std::path::PathBuf>,
) -> Result<LayoutProbe> {
    let rotation = schedule_timed_rotation_request(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            system_preferences: system_preferences.clone(),
            fiammetta_enable,
            support_combinations_enable,
            fiammetta_candidates_out,
            ..AssignBaseOptions::default()
        },
        rotation_request,
    )?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let assignment = reference_shift_assignment(&rotation);
    let trade_report =
        match trade_report_from_assignment(blueprint, assignment, instances, table, durin_plan) {
            Ok(report) => report,
            Err(error) => return Err(crate::error::Error::with_rotation(&rotation, error)),
        };
    let manu_report =
        match manu_report_from_assignment(blueprint, assignment, instances, table, durin_plan) {
            Ok(report) => report,
            Err(error) => return Err(crate::error::Error::with_rotation(&rotation, error)),
        };

    let tier_up_owned = operbox
        .owned
        .values()
        .filter(|p| PromotionTier::is_tier_up(**p))
        .count();
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;

    Ok(LayoutProbe {
        owned: operbox.owned_count(),
        tier_up_owned,
        trade_pool_ready: trade_pool.stats(3).ready,
        manufacture_pool_ready: manu_pool.stats(3).ready,
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
        manufacture_pool_ready: manu_pool.stats(3).ready,
        trade_report,
        manu_report,
        rotation,
    })
}
