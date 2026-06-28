use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::layout::{LayoutContext, SharedLayout};
use crate::pool::{
    build_trade_combo_operators_vec, combinations_indices, filter_standalone_exact_with,
    StandaloneFilter, TradePool,
};
use crate::skill_table::SkillTable;
use crate::trade::input::{
    TradeOperator, TradeOrderKind, TradeRoomInput, TradeSearchOrderMode, TradeStationScenario,
};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::solve_trade_with_shift_prevalidated;

/// 贸易站评分的完整分解。
///
/// 当前搜索排序主键是 [`TradeSearchHit::score`]，其口径为贸易效率
/// `order_eff_total_pct`。对 L3 shortcut 命中的复杂贸易组合，该值来自
/// `data/trade_shortcuts.json` 的公孙长乐等效贸易效率；赤金侧等效值单独放在
/// `mechanic_equiv_eff_pct` / [`TradeSearchHit::gold_pct`]。
///
/// `effective_eff_multiplier` 仅是内部调试用的乘积解释值，不作为用户侧结论；
/// CLI 应优先展示「贸易效率%」和「赤金效率%」，不要把乘积称为最终倍率。
/// PepeExclusive 订单走固定节奏，调试倍率 = 日产量 / 基准日产量。
#[derive(Debug, Clone, Serialize, Default)]
pub struct TradeScoreBreakdown {
    /// 基础效率 (每名干员 +100%)
    pub order_eff_base: f64,
    /// 技能效率总和
    pub order_eff_skill: f64,
    /// 全局注入 (如阿米娅中枢 +7%)
    pub order_eff_global: f64,
    /// 纸面贸易效率%
    pub order_eff_total_pct: f64,
    /// 订单机制等效效率%
    pub mechanic_equiv_eff_pct: f64,
    /// 效率因子 = 1 + order_eff_total/100
    pub eff_factor: f64,
    /// 机制因子 = 1 + mechanic_equiv_eff/100
    pub mech_factor: f64,
    /// 内部调试用乘积解释值；用户侧以 trade_pct / gold_pct 拆开阅读。
    pub effective_eff_multiplier: f64,
    /// 实际日产量（龙门币/天）
    pub unit_trade_per_day: f64,
    /// 实际日耗赤金
    pub unit_gold_per_day: f64,
    /// 命中的短路 ID
    pub shortcut_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeSearchHit {
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gold_names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub originium_names: Vec<String>,
    /// 搜索排序主键：贸易效率 `order_eff_total`（%）；shortcut 命中时为公孙等效贸易效率。
    pub score: f64,
    pub trade_pct: f64,
    pub gold_pct: f64,
    pub shortcut: Option<String>,
    pub unit_trade_per_day: f64,
    pub unit_gold_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unit_originium_per_day: f64,
    pub output_multiplier: f64,
    /// 评分明细分解
    pub breakdown: TradeScoreBreakdown,
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
    pub operator_capacity: usize,
    pub mood: f64,
    pub top_k: usize,
    /// 制造站赤金真实生产线数（公孙长乐基准常用 4）。
    pub gold_production_lines: u32,
    /// 全基建布局上下文（进驻编制派生的 OperatorInBase、空弦/石英/风絮/状态链等）.
    pub layout: SharedLayout,
    /// 上班时长（小时）；产量公式用 `eff × (shift/24) × 单位产出`。
    pub shift_hours: f64,
    pub order_mode: TradeSearchOrderMode,
    pub use_baked: bool,
    pub full_pool: bool,
}

impl Default for TradeSearchOptions {
    fn default() -> Self {
        Self {
            trade_level: 3,
            operator_capacity: 3,
            mood: 24.0,
            top_k: 5,
            gold_production_lines: 4,
            layout: Arc::new(LayoutContext::search_baseline()),
            shift_hours: 24.0,
            order_mode: TradeSearchOrderMode::default(),
            use_baked: true,
            full_pool: false,
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
    /// Combo must include all of these operator names (e.g. anchored same-room pair).
    pub must_include_names: Vec<String>,
    /// When set, that named slot uses this operator instead of pool entry (精0 孑摊贩).
    pub must_operator_override: Option<TradeOperator>,
    /// Keep only hits passing this predicate (e.g. witch / closure shortcut).
    pub hit_filter: Option<fn(&TradeSearchHit) -> bool>,
}

impl SearchTripleFilter {
    pub fn must_include_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Some(name) = self.must_include_name.as_ref() {
            names.push(name.clone());
        }
        for name in &self.must_include_names {
            if !names.iter().any(|n| n == name) {
                names.push(name.clone());
            }
        }
        names
    }

    pub fn has_must_include(&self) -> bool {
        self.must_include_name.is_some() || !self.must_include_names.is_empty()
    }
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
    let ori_report =
        search_trade_single_order(pool, table, &ori_opts, TradeOrderKind::Originium, filter)?;

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
        breakdown: TradeScoreBreakdown::default(),
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

fn trade_efficiency_sort_key(hit: &TradeSearchHit) -> f64 {
    hit.trade_pct
}

fn search_trade_single_order(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    filter: SearchTripleFilter,
) -> Result<TradeSearchReport> {
    let sub = trade_search_pool_for_order(pool, order_kind, options, &filter);
    let n = sub.entries.len();
    let operator_capacity = options.operator_capacity.clamp(1, 3);
    if n < operator_capacity {
        return Err(crate::error::Error::msg(format!(
            "trade pool has fewer than {operator_capacity} ready operators"
        )));
    }

    let must_names = filter.must_include_names();
    if must_names.len() > operator_capacity {
        return Err(crate::error::Error::msg(format!(
            "trade search requires {} operators but station capacity is {}",
            must_names.len(),
            operator_capacity
        )));
    }
    let mut must_indices = Vec::with_capacity(must_names.len());
    for name in &must_names {
        let Some(idx) = sub
            .entries
            .iter()
            .position(|e| e.name == *name)
            .or_else(|| {
                if filter
                    .must_operator_override
                    .as_ref()
                    .is_some_and(|o| o.name == *name)
                {
                    Some(0)
                } else {
                    None
                }
            })
        else {
            return Err(crate::error::Error::msg(format!(
                "trade pool missing must-include operator {name:?}"
            )));
        };
        if !must_indices.contains(&idx) {
            must_indices.push(idx);
        }
    }

    let combo_count = if must_indices.is_empty() {
        crate::pool::n_choose_k_u64(n, operator_capacity)
    } else {
        crate::pool::n_choose_k_u64(
            n.saturating_sub(must_indices.len()),
            operator_capacity.saturating_sub(must_indices.len()),
        )
    };

    let start = Instant::now();
    if options.use_baked {
        if let Some(report) = crate::bake::try_baked_trade_search(
            &sub,
            table,
            options,
            order_kind,
            &filter,
            combo_count,
            start,
        )? {
            return Ok(report);
        }
    }

    let override_op = filter.must_operator_override.clone();
    let must_name = filter.must_include_name.clone();
    let hit_filter = filter.hit_filter;

    let mut combos: Vec<_> = combinations_indices(n, operator_capacity).collect();
    if !must_indices.is_empty() {
        combos.retain(|combo| must_indices.iter().all(|idx| combo.contains(idx)));
    }
    let mut hits: Vec<TradeSearchHit> = combos
        .par_iter()
        .filter_map(|combo| {
            eval_combo_hit(
                &sub,
                table,
                options,
                order_kind,
                combo,
                must_name.as_deref(),
                override_op.as_ref(),
            )
        })
        .filter(|hit| hit_filter.is_none_or(|f| f(hit)))
        .collect();

    let evaluated = hits.len() as u64;
    hits.sort_by(|a, b| {
        trade_efficiency_sort_key(b)
            .partial_cmp(&trade_efficiency_sort_key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = hits.first().cloned().ok_or_else(|| {
        crate::error::Error::msg(
            if filter.hit_filter.is_some() || filter.has_must_include() {
                "no trade combo matched search filter"
            } else {
                "trade pool has fewer ready operators than station capacity"
            }
            .to_string(),
        )
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

fn trade_search_pool_for_order(
    pool: &TradePool,
    order_kind: TradeOrderKind,
    options: &TradeSearchOptions,
    filter: &SearchTripleFilter,
) -> TradePool {
    if options.full_pool {
        return pool.clone();
    }
    if filter.has_must_include() || filter.hit_filter.is_some() {
        return pool.clone();
    }

    let sub = filter_standalone_exact_with(
        pool,
        crate::FacilityKind::TradePost,
        StandaloneFilter::for_order(order_kind),
    )
    .unwrap_or_else(|| pool.clone());
    if sub.entries.len() >= 3 {
        sub
    } else {
        pool.clone()
    }
}

fn eval_combo_hit(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    combo: &[usize],
    must_name: Option<&str>,
    override_op: Option<&TradeOperator>,
) -> Option<TradeSearchHit> {
    let ops = build_trade_combo_operators_vec(pool, combo, must_name, override_op);
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
        operators: ops,
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

    let order_eff_global = result.order_eff_total - result.order_eff_base - result.order_eff_skill;
    let eff_factor = if result.order_mechanic.ignores_order_eff() {
        1.0
    } else {
        1.0 + result.order_eff_total / 100.0
    };
    let mech_factor = 1.0 + result.order_mechanic.mechanic_equiv_eff_pct / 100.0;
    let breakdown = TradeScoreBreakdown {
        order_eff_base: result.order_eff_base,
        order_eff_skill: result.order_eff_skill,
        order_eff_global,
        order_eff_total_pct: result.order_eff_total,
        mechanic_equiv_eff_pct: result.order_mechanic.mechanic_equiv_eff_pct,
        eff_factor,
        mech_factor,
        effective_eff_multiplier: result.effective_eff_multiplier,
        unit_trade_per_day: result.production.unit.unit_trade_per_day,
        unit_gold_per_day: result.production.unit.unit_gold_per_day,
        shortcut_id: result.trade_shortcut.clone(),
    };

    Some(TradeSearchHit {
        names,
        gold_names: vec![],
        originium_names: vec![],
        score: result.order_eff_total,
        trade_pct: result.order_eff_total,
        gold_pct: result.order_mechanic.mechanic_equiv_eff_pct,
        shortcut: result.trade_shortcut,
        unit_trade_per_day: result.production.unit.unit_trade_per_day,
        unit_gold_per_day: result.production.unit.unit_gold_per_day,
        unit_originium_per_day: result.production.unit.unit_originium_per_day,
        output_multiplier: result.production.unit.multiplier_vs_lv3_regular,
        breakdown,
    })
}

pub fn hit_witch_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut
        .as_deref()
        .is_some_and(|id| id.starts_with("gsl_witch"))
}

pub fn hit_closure_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut
        .as_deref()
        .is_some_and(|id| id.starts_with("gsl_closure") || id == "gsl_blackkey_closure")
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
    use crate::pool::{add_jie_market_to_trade_pool, build_trade_pool};
    use crate::roster::{OperatorProgress, Roster};
    use crate::skill_table::{default_skill_table_path, SkillTable};

    #[test]
    fn default_search_options_includes_layout_baseline() {
        let opts = TradeSearchOptions::default();
        assert_eq!(opts.layout.meeting_max_level, 3);
        assert_eq!(opts.layout.dorm_occupant_count, 20);
        // MonsterCuisine 基线已移除；无 assignment 时编排层不运行，所以初始为 0
        assert_eq!(
            opts.layout
                .global
                .get(crate::global_resource::GlobalResourceKey::MonsterCuisine),
            0.0
        );
        assert!(opts.layout.base_workforce.is_empty());
    }

    #[test]
    fn level_two_trade_search_uses_two_operator_capacity() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("古米", 2), ("夜刀", 2), ("斑点", 1)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let report = search_trade_triples(
            &pool,
            &table,
            &TradeSearchOptions {
                trade_level: 2,
                operator_capacity: 2,
                order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
                ..TradeSearchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.best.names.len(), 2);
        assert!(report.combinations > 0);
        assert_eq!(report.best.breakdown.order_eff_base, 2.0);
    }

    #[test]
    fn vina_beta_requires_peer_glasgow_for_extra_ten_pct() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("维娜·维多利亚", 2), ("但书", 2), ("空弦", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let report = search_trade_triples_filtered(
            &pool,
            &table,
            &TradeSearchOptions {
                trade_level: 2,
                operator_capacity: 2,
                order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
                use_baked: false,
                top_k: 10,
                ..TradeSearchOptions::default()
            },
            SearchTripleFilter {
                must_include_names: vec!["维娜·维多利亚".to_string(), "但书".to_string()],
                ..SearchTripleFilter::default()
            },
        )
        .unwrap();

        assert_eq!(report.best.names, vec!["维娜·维多利亚", "但书"]);
        assert!(
            (report.best.breakdown.order_eff_skill - 30.0).abs() < 0.01,
            "维娜未与其他格拉斯哥同房时应为 30% 退化态: {:?}",
            report.best
        );
    }

    #[test]
    fn roster_search_finds_docus_station() {
        let roster =
            Roster::load_csv_for_facility(&crate::roster::default_roster_path().unwrap(), "trade")
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
                top_k: 200,
                order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
                ..TradeSearchOptions::default()
            },
        )
        .unwrap();
        assert!(report.evaluated > 0);
        let docus_solo = report
            .top
            .iter()
            .find(|h| h.names.contains(&"但书".to_string()));
        assert!(
            docus_solo.is_some(),
            "但书应出现在搜索结果中（score={} 为贸易效率%，赤金效率另列）: top_k={}",
            docus_solo.map(|h| h.score).unwrap_or(0.0),
            report.top.len()
        );
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
    fn trade_search_score_is_paper_efficiency_sort_key() {
        let hit = TradeSearchHit {
            names: vec!["a".into()],
            gold_names: vec![],
            originium_names: vec![],
            score: 123.0,
            trade_pct: 123.0,
            gold_pct: 45.0,
            shortcut: None,
            unit_trade_per_day: 10_000.0,
            unit_gold_per_day: 1_000.0,
            unit_originium_per_day: 0.0,
            output_multiplier: 3.14,
            breakdown: TradeScoreBreakdown {
                order_eff_total_pct: 123.0,
                effective_eff_multiplier: 3.14,
                ..Default::default()
            },
        };
        assert_eq!(trade_efficiency_sort_key(&hit), hit.trade_pct);
        assert_eq!(hit.score, hit.breakdown.order_eff_total_pct);
    }

    #[test]
    fn karlan_precision_search_finds_ling_jie_by_l1() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut roster = Roster::default();
        for (name, elite) in [
            ("孑", 2),
            ("银灰", 2),
            ("琳琅诗怀雅", 2),
            ("崖心", 2),
            ("讯使", 0),
        ] {
            roster.insert(name, OperatorProgress::elite_only(elite));
        }
        let mut pool = build_trade_pool(&roster, &instances, &table).unwrap();
        assert!(pool.entry("孑").is_none(), "精1+孑默认不进通用池");
        add_jie_market_to_trade_pool(&mut pool, &instances, &table);
        assert!(pool.entry("孑").is_some(), "灵知线搜索应注入市井孑");

        let mut layout = LayoutContext::default();
        layout.global_inject.record_karlan_precision(-15.0, 6);
        let report = search_trade_triples(
            &pool,
            &table,
            &TradeSearchOptions {
                top_k: 5,
                layout: Arc::new(layout),
                order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
                ..TradeSearchOptions::default()
            },
        )
        .unwrap();

        let best = report.best;
        assert_eq!(best.shortcut, None);
        assert!(best.names.contains(&"孑".to_string()), "{:?}", best.names);
        assert!(best.names.contains(&"银灰".to_string()), "{:?}", best.names);
        assert!(
            best.names.contains(&"琳琅诗怀雅".to_string()),
            "{:?}",
            best.names
        );
        assert!((best.trade_pct - 129.0).abs() < 0.01, "best={best:?}");
    }

    #[test]
    fn split_station_search_reports_gold_and_originium_lines() {
        let roster =
            Roster::load_csv_for_facility(&crate::roster::default_roster_path().unwrap(), "trade")
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
