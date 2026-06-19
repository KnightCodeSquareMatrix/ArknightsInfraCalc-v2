use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::pool::{
    build_trade_combo_operators, combinations_triples, combinations_triples_with_anchor, TradePool,
};
use crate::skill_table::SkillTable;
use crate::layout::{LayoutContext, SharedLayout};
use crate::trade::input::{
    TradeOperator, TradeOrderKind, TradeRoomInput,
    TradeSearchOrderMode, TradeStationScenario,
};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::solve_trade_with_shift_prevalidated;

#[derive(Debug, Clone, Serialize)]
pub struct TradeSearchHit {
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gold_names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub originium_names: Vec<String>,
    pub score: f64,
    pub trade_pct: f64,
    pub gold_pct: f64,
    pub shortcut: Option<String>,
    pub unit_trade_per_day: f64,
    pub unit_gold_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unit_originium_per_day: f64,
    pub output_multiplier: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeSearchReport {
    pub order_mode: TradeSearchOrderMode,
    pub best: TradeSearchHit,
    pub top: Vec<TradeSearchHit>,
    pub combinations: u64,
    pub evaluated: u64,
    pub elapsed: Duration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gold_order_line: Option<TradeSearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub originium_order_line: Option<TradeSearchHit>,
}

#[derive(Debug, Clone)]
pub struct TradeSearchOptions {
    pub trade_level: u8,
    pub mood: f64,
    pub top_k: usize,
    /// 制造站赤金真实生产线数（公孙长乐基准常用 4）。
    pub gold_production_lines: u32,
    /// 全基建布局上下文（进驻编制派生的 OperatorInBase、空弦/石英/风絮/状态链等）.
    pub layout: SharedLayout,
    /// 上班时长（小时）；产量公式用 `eff × (shift/24) × 单位产出`。
    pub shift_hours: f64,
    pub order_mode: TradeSearchOrderMode,
}

impl Default for TradeSearchOptions {
    fn default() -> Self {
        Self {
            trade_level: 3,
            mood: 24.0,
            top_k: 5,
            gold_production_lines: 4,
            layout: Arc::new(LayoutContext::search_baseline()),
            shift_hours: 24.0,
            order_mode: TradeSearchOrderMode::default(),
        }
    }
}

impl TradeSearchOptions {
    pub fn gold_order_only() -> Self {
        Self {
            order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
            ..Self::default()
        }
    }
}

pub fn search_trade_triples(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
) -> Result<TradeSearchReport> {
    search_trade_triples_filtered(pool, table, options, SearchTripleFilter::default())
}

/// Optional constraints for triple search (meta 站 / 孑带队等).
#[derive(Debug, Clone, Default)]
pub struct SearchTripleFilter {
    /// Combo must include this operator name (e.g. `"孑"`).
    pub must_include_name: Option<String>,
    /// When set, that named slot uses this operator instead of pool entry (精0 孑摊贩).
    pub must_operator_override: Option<TradeOperator>,
    /// Keep only hits passing this predicate (e.g. witch / closure shortcut).
    pub hit_filter: Option<fn(&TradeSearchHit) -> bool>,
}

pub fn search_trade_triples_filtered(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    filter: SearchTripleFilter,
) -> Result<TradeSearchReport> {
    match options.order_mode {
        TradeSearchOrderMode::Stations(scenario) => {
            search_trade_split_stations(pool, table, options, scenario, filter)
        }
        TradeSearchOrderMode::Single(kind) => {
            search_trade_single_order(pool, table, options, kind, filter)
        }
    }
}

fn search_trade_split_stations(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    scenario: TradeStationScenario,
    filter: SearchTripleFilter,
) -> Result<TradeSearchReport> {
    let start = Instant::now();
    let mut gold_opts = options.clone();
    gold_opts.order_mode = TradeSearchOrderMode::Single(TradeOrderKind::Gold);
    let gold_report = search_trade_single_order(
        pool,
        table,
        &gold_opts,
        TradeOrderKind::Gold,
        filter.clone(),
    )?;

    let mut ori_opts = options.clone();
    ori_opts.order_mode = TradeSearchOrderMode::Single(TradeOrderKind::Originium);
    let ori_report = search_trade_single_order(
        pool,
        table,
        &ori_opts,
        TradeOrderKind::Originium,
        filter,
    )?;

    let composite_score = f64::from(scenario.gold_order_stations) * gold_report.best.score
        + f64::from(scenario.originium_order_stations) * ori_report.best.score;

    let best = TradeSearchHit {
        names: vec![],
        gold_names: gold_report.best.names.clone(),
        originium_names: ori_report.best.names.clone(),
        score: composite_score,
        trade_pct: gold_report.best.trade_pct,
        gold_pct: gold_report.best.gold_pct,
        shortcut: None,
        unit_trade_per_day: gold_report.best.unit_trade_per_day,
        unit_gold_per_day: gold_report.best.unit_gold_per_day,
        unit_originium_per_day: ori_report.best.unit_originium_per_day,
        output_multiplier: gold_report.best.output_multiplier,
    };

    Ok(TradeSearchReport {
        order_mode: TradeSearchOrderMode::Stations(scenario),
        best: best.clone(),
        top: vec![best],
        combinations: gold_report
            .combinations
            .saturating_add(ori_report.combinations),
        evaluated: gold_report.evaluated.saturating_add(ori_report.evaluated),
        elapsed: start.elapsed(),
        gold_order_line: Some(gold_report.best),
        originium_order_line: Some(ori_report.best),
    })
}

fn search_trade_single_order(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    filter: SearchTripleFilter,
) -> Result<TradeSearchReport> {
    let n = pool.entries.len();
    if n < 3 {
        return Err(crate::error::Error::msg(
            "trade pool has fewer than 3 ready operators",
        ));
    }

    let must_idx = filter.must_include_name.as_ref().and_then(|name| {
        pool.entries
            .iter()
            .position(|e| e.name == *name)
            .or_else(|| {
                if filter.must_operator_override.as_ref().is_some_and(|o| o.name == *name) {
                    Some(0)
                } else {
                    None
                }
            })
    });

    if filter.must_include_name.is_some() && must_idx.is_none() {
        return Err(crate::error::Error::msg(format!(
            "trade pool missing must-include operator {:?}",
            filter.must_include_name
        )));
    }

    let combo_count = if let Some(_) = must_idx {
        crate::pool::n_choose_k_u64(n.saturating_sub(1), 2)
    } else {
        crate::pool::n_choose_k_u64(n, 3)
    };

    let start = Instant::now();
    let override_op = filter.must_operator_override.clone();
    let must_name = filter.must_include_name.clone();
    let hit_filter = filter.hit_filter;

    let mut hits: Vec<TradeSearchHit> = if let Some(anchor) = must_idx {
        combinations_triples_with_anchor(n, anchor)
            .collect::<Vec<_>>()
            .par_iter()
            .filter_map(|combo| {
                eval_combo_hit(
                    pool,
                    table,
                    options,
                    order_kind,
                    *combo,
                    must_name.as_deref(),
                    override_op.as_ref(),
                )
            })
            .filter(|hit| hit_filter.is_none_or(|f| f(hit)))
            .collect()
    } else {
        combinations_triples(n)
            .collect::<Vec<_>>()
            .par_iter()
            .filter_map(|combo| {
                eval_combo_hit(
                    pool,
                    table,
                    options,
                    order_kind,
                    *combo,
                    None,
                    None,
                )
            })
            .filter(|hit| hit_filter.is_none_or(|f| f(hit)))
            .collect()
    };

    let evaluated = hits.len() as u64;
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = hits.first().cloned().ok_or_else(|| {
        crate::error::Error::msg(if filter.hit_filter.is_some() || filter.must_include_name.is_some()
        {
            "no trade triple matched search filter"
        } else {
            "trade pool has fewer than 3 ready operators"
        })
    })?;
    let top = hits.into_iter().take(options.top_k).collect();

    Ok(TradeSearchReport {
        order_mode: TradeSearchOrderMode::Single(order_kind),
        best,
        top,
        combinations: combo_count,
        evaluated,
        elapsed: start.elapsed(),
        gold_order_line: None,
        originium_order_line: None,
    })
}

fn eval_combo_hit(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    combo: [usize; 3],
    must_name: Option<&str>,
    override_op: Option<&TradeOperator>,
) -> Option<TradeSearchHit> {
    let ops = build_trade_combo_operators(pool, combo, must_name, override_op);
    if trade_station_exclusive_violation(&ops, table) {
        return None;
    }
    let gold_lines = if options.layout.gold_manu_line_count > 0 {
        options.layout.gold_manu_line_count
    } else {
        options.gold_production_lines
    };
    let input = TradeRoomInput {
        level: options.trade_level,
        operators: ops.to_vec(),
        order_count: None,
        mood: options.mood,
        gold_production_lines: Some(gold_lines),
        durin_virtual_lines: None,
        human_fireworks: None,
        layout: Arc::clone(&options.layout),
        active_order_kind: order_kind,
    };
    let result = solve_trade_with_shift_prevalidated(&input, table, options.shift_hours).ok()?;
    let names: Vec<String> = input.operators.iter().map(|o| o.name.clone()).collect();
    Some(TradeSearchHit {
        names,
        gold_names: vec![],
        originium_names: vec![],
        score: result.effective_eff_multiplier,
        trade_pct: result.order_eff_total,
        gold_pct: result.order_mechanic.mechanic_equiv_eff_pct,
        shortcut: result.trade_shortcut,
        unit_trade_per_day: result.production.unit.unit_trade_per_day,
        unit_gold_per_day: result.production.unit.unit_gold_per_day,
        unit_originium_per_day: result.production.unit.unit_originium_per_day,
        output_multiplier: result.production.unit.multiplier_vs_lv3_regular,
    })
}

pub fn hit_witch_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut
        .as_deref()
        .is_some_and(|id| id.starts_with("gsl_witch"))
}

pub fn hit_closure_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut.as_deref().is_some_and(|id| {
        id.starts_with("gsl_closure") || id == "gsl_blackkey_closure"
    })
}

pub fn hit_blackkey_closure_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut.as_deref() == Some("gsl_blackkey_closure")
}

pub fn hit_docus_solo_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut.as_deref() == Some("gsl_docus_solo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::instances::OperatorInstances;
    use crate::pool::build_trade_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    #[test]
    fn default_search_options_includes_layout_baseline() {
        let opts = TradeSearchOptions::default();
        assert_eq!(opts.layout.meeting_max_level, 3);
        assert_eq!(opts.layout.dorm_occupant_count, 20);
        // MonsterCuisine 基线已移除；无 assignment 时编排层不运行，所以初始为 0
        assert_eq!(
            opts.layout.global.get(crate::global_resource::GlobalResourceKey::MonsterCuisine),
            0.0
        );
        assert!(opts.layout.base_workforce.is_empty());
    }

    #[test]
    fn roster_search_finds_docus_station() {
        let roster = Roster::load_csv_for_facility(
            &crate::roster::default_roster_path().unwrap(),
            "trade",
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        if pool.entries.len() < 3 {
            return;
        }
        let report = search_trade_triples(
            &pool,
            &table,
            &TradeSearchOptions {
                top_k: 20,
                order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
                ..TradeSearchOptions::default()
            },
        )
        .unwrap();
        assert!(report.evaluated > 0);
        let docus_solo = report.top.iter().find(|h| h.names.contains(&"但书".to_string()));
        assert!(docus_solo.is_some(), "但书应出现在 top 结果中: {:?}", report.top);
        let hit = docus_solo.unwrap();
        assert!(
            hit.shortcut.as_deref() == Some("gsl_docus_solo"),
            "但书应命中 gsl_docus_solo: {:?}",
            hit
        );
        assert!(
            !hit.names.contains(&"巫恋".to_string()),
            "但书单走站不应与巫恋同房: {:?}",
            hit
        );
    }

    #[test]
    fn split_station_search_reports_gold_and_originium_lines() {
        let roster = Roster::load_csv_for_facility(
            &crate::roster::default_roster_path().unwrap(),
            "trade",
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        if pool.entries.len() < 3 {
            return;
        }
        let report = search_trade_triples(&pool, &table, &TradeSearchOptions::default()).unwrap();
        assert!(report.gold_order_line.is_some());
        assert!(report.originium_order_line.is_some());
        let gold = report.gold_order_line.as_ref().unwrap();
        let ori = report.originium_order_line.as_ref().unwrap();
        assert_eq!(gold.unit_gold_per_day > 0.0, true);
        assert_eq!(ori.unit_originium_per_day > 0.0, true);
        let scenario = TradeStationScenario::standard_three_stations();
        let expected = f64::from(scenario.gold_order_stations) * gold.score
            + f64::from(scenario.originium_order_stations) * ori.score;
        assert!((report.best.score - expected).abs() < 0.001);
    }
}
