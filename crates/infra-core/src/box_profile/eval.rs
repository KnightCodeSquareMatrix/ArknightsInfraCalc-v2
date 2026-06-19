//! 固定排班 eval + 从 `BaseAssignment` 提取分域指标（供用户排班 vs 公孙 baseline）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::export::maa::{assignment_from_maa_plan, load_maa_schedule, MaaPlanImport};
use crate::instances::OperatorInstances;
use crate::layout::{
    AssignmentPlan, AssignShiftMode, BaseAssignment, BaseBlueprint, ResolvedBase,
    ResolvedManuRoom, ResolvedTradeRoom, resolve_base,
};
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::solver::{solve_manufacture, ManuProdBreakdown, ManuStorageBreakdown};
use crate::operbox::{default_operbox_full_e2_path, OperBox};
use crate::pool::{build_manufacture_pool, build_trade_pool};
use crate::schedule::{score_base_assignment, DailyTotals, TeamRotationReport};
use crate::search::{ManuSearchHit, ManuSearchReport, TradeSearchHit, TradeSearchReport};
use crate::skill_table::{data_path, SkillTable};
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::trade::solver::solve_trade_with_shift;
use crate::types::RecipeKind;

use super::probe::LayoutProbe;

/// αβγ 日加权：与 `team_rotation` 一致 12h + 6h + 6h。
pub const SHIFT_HOURS: [f64; 3] = [12.0, 6.0, 6.0];

pub fn default_schedule_export_path() -> Result<PathBuf> {
    if let Ok(path) = data_path("fixtures/243/schedule_export.json") {
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(crate::skill_table::workspace_root()?
        .join("data/fixtures/243/schedule_export.json"))
}

/// 与 baseline 对齐：取第 2 班编制看巫恋线 / 制造分域（index 1，6h 班）。
pub fn reference_shift_assignment(report: &TeamRotationReport) -> &BaseAssignment {
    &report
        .shifts
        .get(1)
        .or_else(|| report.shifts.first())
        .expect("rotation report has shifts")
        .assignment
}

/// 公孙 `schedule_export` + 给定 operbox 练度 eval（baseline 侧，无 search）。
pub fn run_schedule_eval_probe(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    schedule_path: &Path,
) -> Result<LayoutProbe> {
    let schedule = load_maa_schedule(schedule_path)?;
    let durin_plan = operbox.durin_dorm_planning_count(instances);

    let mut daily = DailyTotals::default();
    for (plan, &hours) in schedule.plans.iter().zip(SHIFT_HOURS.iter()) {
        let assignment = assignment_from_maa_plan(plan, operbox);
        let scores =
            score_base_assignment(blueprint, &assignment, instances, table, hours, Some(durin_plan))?;
        daily.trade += scores.weighted_trade(hours);
        daily.manu += scores.weighted_manu(hours);
        daily.power += scores.weighted_power(hours);
    }

    let reference = schedule
        .plans
        .get(1)
        .or_else(|| schedule.plans.first())
        .ok_or_else(|| crate::error::Error::msg("schedule_export: no plans"))?;
    let assignment = assignment_from_maa_plan(reference, operbox);
    let trade_report =
        trade_report_from_assignment(blueprint, &assignment, instances, table, durin_plan)?;
    let manu_report =
        manu_report_from_assignment(blueprint, &assignment, instances, table, durin_plan)?;

    probe_shell(operbox, instances, table, trade_report, manu_report, daily)
}

pub fn trade_report_from_assignment(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    durin_plan: u8,
) -> Result<TradeSearchReport> {
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        24.0,
        Some(durin_plan),
    )?;

    let mut gold_line = None;
    let mut originium_line = None;
    let mut best_score = 0.0f64;
    let mut best_hit: Option<TradeSearchHit> = None;

    for room in &resolved.trade_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let hit = eval_trade_room(&resolved, room, table, 24.0)?;
        best_score = best_score.max(hit.score);
        if best_hit.as_ref().is_none_or(|b| hit.score > b.score) {
            best_hit = Some(hit.clone());
        }
        let names: Vec<_> = room.operators.iter().map(|o| o.name.clone()).collect();
        if names.iter().any(|n| n == "巫恋") {
            gold_line = Some(hit);
        } else if originium_line.is_none() {
            originium_line = Some(hit);
        }
    }

    let best = best_hit.unwrap_or(empty_trade_hit());

    Ok(TradeSearchReport {
        order_mode: TradeSearchOrderMode::default(),
        best: best.clone(),
        top: vec![best],
        combinations: 0,
        evaluated: 0,
        elapsed: Duration::ZERO,
        gold_order_line: gold_line,
        originium_order_line: originium_line,
    })
}

pub fn manu_report_from_assignment(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    durin_plan: u8,
) -> Result<ManuSearchReport> {
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        24.0,
        Some(durin_plan),
    )?;

    let scenario = blueprint.manu_line_scenario();
    let mut gold_hits = Vec::new();
    let mut br_hits = Vec::new();

    for room in &resolved.manu_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let hit = eval_manu_room(&resolved, room, table)?;
        match room.recipe {
            RecipeKind::Gold => gold_hits.push(hit),
            RecipeKind::BattleRecord => br_hits.push(hit),
            _ => {}
        }
    }

    let gold_line = average_manu_hit(&gold_hits);
    let br_line = average_manu_hit(&br_hits);
    let composite = f64::from(scenario.gold_lines) * gold_line.as_ref().map(|h| h.composite_score).unwrap_or(0.0)
        + f64::from(scenario.battle_record_lines)
            * br_line.as_ref().map(|h| h.composite_score).unwrap_or(0.0);

    let best = ManuSearchHit {
        names: gold_line
            .as_ref()
            .map(|h| h.names.clone())
            .unwrap_or_default(),
        gold_names: gold_line.as_ref().map(|h| h.names.clone()).unwrap_or_default(),
        battle_record_names: br_line.as_ref().map(|h| h.names.clone()).unwrap_or_default(),
        composite_score: composite,
        per_station: Default::default(),
        storage: Default::default(),
    };

    Ok(ManuSearchReport {
        recipe_mode: crate::manufacture::input::ManuSearchRecipeMode::Lines(scenario),
        best: best.clone(),
        top: vec![best],
        combinations: 0,
        evaluated: 0,
        elapsed: Duration::ZERO,
        gold_line,
        battle_record_line: br_line,
    })
}

fn probe_shell(
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    trade_report: TradeSearchReport,
    manu_report: ManuSearchReport,
    daily: DailyTotals,
) -> Result<LayoutProbe> {
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
        rotation: TeamRotationReport {
            peak_plan: AssignmentPlan::recovery(AssignShiftMode::Peak),
            teams: vec![],
            shifts: vec![],
            daily,
            elapsed: Duration::ZERO,
        },
    })
}

fn empty_trade_hit() -> TradeSearchHit {
    TradeSearchHit {
        names: vec![],
        gold_names: vec![],
        originium_names: vec![],
        score: 0.0,
        trade_pct: 0.0,
        gold_pct: 0.0,
        shortcut: None,
        unit_trade_per_day: 0.0,
        unit_gold_per_day: 0.0,
        unit_originium_per_day: 0.0,
        output_multiplier: 0.0,
    }
}

fn eval_trade_room(
    resolved: &ResolvedBase,
    room: &ResolvedTradeRoom,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<TradeSearchHit> {
    let input = crate::trade::input::TradeRoomInput {
        level: room.level,
        operators: room.operators.clone(),
        order_count: None,
        mood: 24.0,
        gold_production_lines: Some(resolved.gold_manu_line_count()),
        durin_virtual_lines: None,
        human_fireworks: None,
        layout: Arc::new(room.layout.clone()),
        active_order_kind: room.order,
    };
    let result = solve_trade_with_shift(&input, table, shift_hours)?;
    let names: Vec<_> = room.operators.iter().map(|o| o.name.clone()).collect();
    let (gold_names, originium_names) = if room.order == TradeOrderKind::Gold {
        (names.clone(), vec![])
    } else {
        (vec![], names.clone())
    };
    let unit = &result.production.unit;
    Ok(TradeSearchHit {
        names,
        gold_names,
        originium_names,
        score: result.effective_eff_multiplier,
        trade_pct: result.order_eff_total,
        gold_pct: result.order_mechanic.mechanic_equiv_eff_pct,
        shortcut: result.trade_shortcut.clone(),
        unit_trade_per_day: unit.unit_trade_per_day,
        unit_gold_per_day: unit.unit_gold_per_day,
        unit_originium_per_day: unit.unit_originium_per_day,
        output_multiplier: unit.multiplier_vs_lv3_regular,
    })
}

fn eval_manu_room(
    resolved: &ResolvedBase,
    room: &ResolvedManuRoom,
    table: &SkillTable,
) -> Result<ManuSearchHit> {
    let _ = resolved;
    let input = ManuRoomInput {
        level: room.level,
        operators: room.operators.clone(),
        active_recipe: room.recipe,
        mood: 24.0,
        layout: Arc::new(room.layout.clone()),
    };
    let result = solve_manufacture(&input, table)?;
    let names: Vec<_> = room.operators.iter().map(|o| o.name.clone()).collect();
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();
    match room.recipe {
        RecipeKind::Gold => {
            per_station.gold = result.prod_total;
            storage.gold = result.storage_limit;
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.prod_total;
            storage.battle_record = result.storage_limit;
        }
        RecipeKind::Originium => {
            per_station.originium = result.prod_total;
            storage.originium = result.storage_limit;
        }
        RecipeKind::All => {}
    }
    Ok(ManuSearchHit {
        names: names.clone(),
        gold_names: if room.recipe == RecipeKind::Gold {
            names.clone()
        } else {
            vec![]
        },
        battle_record_names: if room.recipe == RecipeKind::BattleRecord {
            names
        } else {
            vec![]
        },
        composite_score: result.prod_total,
        per_station,
        storage,
    })
}

fn average_manu_hit(hits: &[ManuSearchHit]) -> Option<ManuSearchHit> {
    if hits.is_empty() {
        return None;
    }
    let n = hits.len() as f64;
    let composite_score = hits.iter().map(|h| h.composite_score).sum::<f64>() / n;
    Some(ManuSearchHit {
        names: hits[0].names.clone(),
        gold_names: hits[0].gold_names.clone(),
        battle_record_names: hits[0].battle_record_names.clone(),
        composite_score,
        per_station: hits[0].per_station.clone(),
        storage: hits[0].storage.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::OperatorInstances;
    use crate::skill_table::SkillTable;
    use std::time::Instant;

    #[test]
    fn schedule_eval_faster_than_search_probe() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap())
            .unwrap();
        let schedule = default_schedule_export_path().unwrap();
        if !schedule.exists() {
            return;
        }

        let t0 = Instant::now();
        run_schedule_eval_probe(&blueprint, &operbox, &instances, &table, &schedule).unwrap();
        let eval_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t1 = Instant::now();
        super::super::probe::run_layout_probe(&blueprint, &operbox, &instances, &table, 10)
            .unwrap();
        let search_ms = t1.elapsed().as_secs_f64() * 1000.0;

        assert!(
            eval_ms < search_ms,
            "eval {eval_ms:.0}ms should beat search probe {search_ms:.0}ms"
        );
    }
}
