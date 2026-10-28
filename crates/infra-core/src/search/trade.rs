use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::layout::{LayoutContext, SharedLayout};
use crate::pool::{
    build_trade_combo_operators_vec, combinations_indices, filter_standalone_exact_with,
    trade_operators_require_candidate_projection, StandaloneFilter, TradePool,
};
use crate::skill_table::SkillTable;
use crate::trade::input::{
    TradeOperator, TradeOrderKind, TradeRoomInput, TradeSearchOrderMode, TradeStationScenario,
};
use crate::trade::shortcut::trade_station_exclusive_violation;
use crate::trade::solver::{solve_trade_with_shift_prevalidated, TradeResult};

/// 贸易站评分的完整分解。
///
/// 当前搜索排序主键是 [`TradeSearchHit::final_efficiency`]，其口径为可直接参与产出预估的
/// `final_efficiency`：三级贸易基准日产出 × final_efficiency × 工作时长占比。
#[derive(Debug, Clone, Serialize, Default)]
pub struct TradeEfficiencyBreakdown {
    pub base_efficiency: Efficiency,
    pub occupancy_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub control_efficiency: Efficiency,
    pub global_efficiency: Efficiency,
    pub paper_efficiency: Efficiency,
    pub total_efficiency: Efficiency,
    pub mechanic_equivalent_efficiency: Efficiency,
    pub unit_output_multiplier: Efficiency,
    #[serde(skip_serializing)]
    pub final_efficiency: Efficiency,
    pub equivalent_skill_efficiency: Efficiency,
    pub trade_equivalent_efficiency: Efficiency,
    pub gold_equivalent_efficiency: Efficiency,
    pub combined_output_value: Efficiency,
    /// 实际日产量（龙门币/天）
    pub unit_trade_per_day: f64,
    /// 实际日耗赤金
    pub unit_gold_per_day: f64,
    pub trade_output_per_day: f64,
    #[serde(skip_serializing)]
    pub gold_consumption_per_day: f64,
    pub originium_shards_per_day: f64,
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeSearchHit {
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gold_names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub originium_names: Vec<String>,
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub mechanic_equivalent_efficiency: Efficiency,
    pub rule_id: Option<String>,
    pub unit_trade_per_day: f64,
    pub unit_gold_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unit_originium_per_day: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<TradeEfficiencyBreakdown>,
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
    pub bake_mode: crate::bake::BakeMode,
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
            bake_mode: crate::bake::BakeMode::Auto,
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
    /// 同站禁配，包含 Plan 约束及贸易搜索的全布局产品政策。
    pub forbidden_pairs: Vec<(String, String)>,
}

pub(crate) use crate::trade::legality::is_global_trade_forbidden_pair;

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
    let _memo_scope = super::ensure_search_memo_scope();
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

    let composite_efficiency = trade_efficiency_sort_key(&gold_report.best)
        .scale_ratio(i64::from(scenario.gold_order_stations), 1)
        + trade_efficiency_sort_key(&ori_report.best)
            .scale_ratio(i64::from(scenario.originium_order_stations), 1);

    let best = TradeSearchHit {
        names: vec![],
        gold_names: gold_report.best.names.clone(),
        originium_names: ori_report.best.names.clone(),
        final_efficiency: composite_efficiency,
        mechanic_equivalent_efficiency: gold_report.best.mechanic_equivalent_efficiency,
        rule_id: None,
        unit_trade_per_day: gold_report.best.unit_trade_per_day,
        unit_gold_per_day: gold_report.best.unit_gold_per_day,
        unit_originium_per_day: ori_report.best.unit_originium_per_day,
        breakdown: None,
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

fn trade_efficiency_sort_key(hit: &TradeSearchHit) -> Efficiency {
    hit.breakdown
        .as_ref()
        .map_or(hit.final_efficiency, |breakdown| breakdown.final_efficiency)
}

fn search_trade_single_order(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    order_kind: TradeOrderKind,
    mut filter: SearchTripleFilter,
) -> Result<TradeSearchReport> {
    let leaf_started = Instant::now();
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Trade);
    let memo_key_started = Instant::now();
    let memo_key = super::memo::trade_key(pool, table, options, order_kind, &filter)
        .ok()
        .flatten();
    let memo_key_elapsed = memo_key_started.elapsed();
    let cached_report = memo_key.as_deref().and_then(super::memo::trade_get);
    perf.set_memo_lookup(
        memo_key_elapsed,
        memo_key.as_ref().map(|_| cached_report.is_some()),
    );
    if let Some(mut report) = cached_report {
        report.elapsed = leaf_started.elapsed();
        perf.finish(0, 0, 0, report.top.len() as u64, false, Default::default());
        return Ok(report);
    }
    let setup_started = Instant::now();
    if !filter
        .forbidden_pairs
        .iter()
        .any(|(a, b)| is_global_trade_forbidden_pair(a, b))
    {
        filter.forbidden_pairs.extend(
            crate::trade::legality::GLOBAL_FORBIDDEN_TRADE_PAIRS
                .iter()
                .map(|&(a, b)| (a.to_string(), b.to_string())),
        );
    }
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
    let setup_elapsed = setup_started.elapsed();
    perf.set_progress(
        combo_count,
        0,
        0,
        crate::profile::SearchPhaseDurations {
            prepare: setup_elapsed,
            ..Default::default()
        },
    );

    let start = Instant::now();
    let bake_started = Instant::now();
    let baked_report = if options.bake_mode != crate::bake::BakeMode::Disabled {
        crate::bake::try_baked_trade_search(
            &sub,
            table,
            options,
            order_kind,
            &filter,
            combo_count,
            start,
        )?
    } else {
        None
    };
    let bake_lookup_elapsed = bake_started.elapsed();
    if let Some(report) = baked_report {
        if let Some(key) = memo_key {
            super::memo::trade_insert(key, &report);
        }
        perf.finish(
            combo_count,
            0,
            report.evaluated,
            report.top.len() as u64,
            true,
            crate::profile::SearchPhaseDurations {
                prepare: setup_elapsed,
                bake_lookup: bake_lookup_elapsed,
                ..Default::default()
            },
        );
        return Ok(report);
    }

    let prepare_started = Instant::now();
    let override_op = filter.must_operator_override.clone();
    let must_name = filter.must_include_name.clone();
    let hit_filter = filter.hit_filter;
    let forbidden_pairs = &filter.forbidden_pairs;

    let mut combos: Vec<_> = combinations_indices(n, operator_capacity).collect();
    if !must_indices.is_empty() {
        combos.retain(|combo| must_indices.iter().all(|idx| combo.contains(idx)));
    }
    let prepare_elapsed = setup_elapsed + prepare_started.elapsed();
    perf.set_progress(
        combo_count,
        combos.len() as u64,
        0,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            ..Default::default()
        },
    );
    let evaluate_started = Instant::now();
    perf.mark_live_started();
    let batch_counters = perf.hot_path_batch_counters();
    let mut hits: Vec<TradeSearchHit> = combos
        .par_iter()
        .map_init(
            || crate::profile::start_hot_path_batch(std::sync::Arc::clone(&batch_counters)),
            |_, combo| {
                eval_combo_hit(
                    &sub,
                    table,
                    options,
                    order_kind,
                    combo,
                    must_name.as_deref(),
                    override_op.as_ref(),
                )
            },
        )
        .flatten()
        .filter(|hit| hit_filter.is_none_or(|f| f(hit)))
        .filter(|hit| {
            forbidden_pairs.iter().all(|(a, b)| {
                !(hit.names.iter().any(|name| name == a) && hit.names.iter().any(|name| name == b))
            })
        })
        .collect();
    let evaluate_elapsed = evaluate_started.elapsed();

    let candidate_attempts = combos.len() as u64;
    let evaluated = hits.len() as u64;
    perf.set_progress(
        combo_count,
        candidate_attempts,
        evaluated,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            evaluate: evaluate_elapsed,
            ..Default::default()
        },
    );
    let sort_started = Instant::now();
    hits.sort_by(|a, b| trade_efficiency_sort_key(b).cmp(&trade_efficiency_sort_key(a)));
    let best = hits.first().cloned().ok_or_else(|| {
        crate::error::Error::msg(
            if filter.hit_filter.is_some()
                || filter.has_must_include()
                || !filter.forbidden_pairs.is_empty()
            {
                "no trade combo matched search filter"
            } else {
                "trade pool has fewer ready operators than station capacity"
            }
            .to_string(),
        )
    })?;
    let top = hits.into_iter().take(options.top_k).collect();
    let sort_topk_elapsed = sort_started.elapsed();

    let report = TradeSearchReport {
        order_mode: TradeSearchOrderMode::Single(order_kind),
        best,
        top,
        combinations: combo_count,
        evaluated,
        elapsed: start.elapsed(),
        gold_order_line: None,
        originium_order_line: None,
    };
    if let Some(key) = memo_key {
        super::memo::trade_insert(key, &report);
    }
    perf.finish(
        combo_count,
        candidate_attempts,
        evaluated,
        report.top.len() as u64,
        false,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            evaluate: evaluate_elapsed,
            sort_topk: sort_topk_elapsed,
        },
    );
    Ok(report)
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
    if filter.has_must_include()
        || filter.hit_filter.is_some()
        || filter
            .forbidden_pairs
            .iter()
            .any(|(a, b)| !is_global_trade_forbidden_pair(a, b))
    {
        return pool.clone();
    }

    let sub = filter_standalone_exact_with(
        pool,
        crate::FacilityKind::TradePost,
        StandaloneFilter::for_order(order_kind),
    )
    .unwrap_or_else(|| pool.clone());
    if sub.entries.len() >= 3 {
        let gold_lines = if options.layout.gold_manu_line_count > 0 {
            options.layout.gold_manu_line_count
        } else {
            options.gold_production_lines
        };
        if order_kind == TradeOrderKind::Gold && gold_lines == 2 {
            let mut expanded = sub;
            if let (Some(pozyomka), Some(tuye)) = (pool.entry("鸿雪"), pool.entry("图耶")) {
                for entry in [pozyomka, tuye] {
                    if expanded.entry(&entry.name).is_none() {
                        expanded.entries.push(entry.clone());
                    }
                }
            }
            expanded
        } else {
            sub
        }
    } else {
        pool.clone()
    }
}

impl TradeSearchOptions {
    /// 单个组合的 live 命中构造（含候选投影与同房禁配预筛）。
    ///
    /// 实时搜索、Bake 行生成与 Bake 行级对账共用同一入口，保证三者对同一组合
    /// 产出的 `TradeSearchHit` 逐字段一致。
    pub(crate) fn solve_trade_combo(
        &self,
        pool: &TradePool,
        table: &SkillTable,
        order_kind: TradeOrderKind,
        combo: &[usize],
        must_name: Option<&str>,
        override_op: Option<&TradeOperator>,
    ) -> Option<(TradeSearchHit, TradeResult)> {
        let ops = build_trade_combo_operators_vec(pool, combo, must_name, override_op);
        if trade_station_exclusive_violation(&ops, table) {
            return None;
        }
        let gold_lines = if self.layout.gold_manu_line_count > 0 {
            self.layout.gold_manu_line_count
        } else {
            self.gold_production_lines
        };
        let projected_layout = trade_combo_layout(&self.layout, &ops);
        let input = TradeRoomInput {
            level: self.trade_level,
            operators: ops,
            order_count: None,
            mood: self.mood,
            gold_production_lines: Some(gold_lines),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: projected_layout,
            active_order_kind: order_kind,
        };
        let result = solve_trade_with_shift_prevalidated(&input, table, self.shift_hours).ok()?;
        let hit = trade_search_hit_from_result(&input.operators, &result);
        Some((hit, result))
    }
}

fn trade_search_hit_from_result(
    operators: &[TradeOperator],
    result: &TradeResult,
) -> TradeSearchHit {
    let names: Vec<String> = operators.iter().map(|o| o.name.clone()).collect();
    let efficiency = &result.efficiency;
    let breakdown = TradeEfficiencyBreakdown {
        base_efficiency: efficiency.paper.base_efficiency,
        occupancy_efficiency: efficiency.paper.occupancy_efficiency,
        skill_efficiency: efficiency.paper.skill_efficiency,
        control_efficiency: efficiency.paper.control_efficiency,
        global_efficiency: efficiency.paper.control_efficiency,
        paper_efficiency: efficiency.paper.paper_efficiency,
        mechanic_equivalent_efficiency: result.order_mechanic.mechanic_equivalent_efficiency,
        unit_output_multiplier: efficiency.production_basis.unit_output_multiplier,
        final_efficiency: efficiency.final_efficiency,
        total_efficiency: efficiency.total_efficiency,
        equivalent_skill_efficiency: efficiency.equivalent_skill_efficiency,
        trade_equivalent_efficiency: efficiency.trade_equivalent_efficiency,
        gold_equivalent_efficiency: efficiency.gold_equivalent_efficiency,
        combined_output_value: efficiency.combined_output_value,
        unit_trade_per_day: result.production.unit.unit_trade_per_day,
        unit_gold_per_day: result.production.unit.unit_gold_per_day,
        trade_output_per_day: result.production.unit.unit_trade_per_day
            * efficiency.total_efficiency.as_f64(),
        gold_consumption_per_day: result.production.unit.gold_consumption_per_day
            * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE
            * efficiency.total_efficiency.as_f64(),
        originium_shards_per_day: result.production.unit.unit_originium_per_day
            * efficiency.total_efficiency.as_f64(),
        rule_id: result.rule_id.clone(),
    };

    TradeSearchHit {
        names,
        gold_names: vec![],
        originium_names: vec![],
        final_efficiency: efficiency.final_efficiency,
        mechanic_equivalent_efficiency: result.order_mechanic.mechanic_equivalent_efficiency,
        rule_id: result.rule_id.clone(),
        unit_trade_per_day: result.production.unit.unit_trade_per_day,
        unit_gold_per_day: result.production.unit.unit_gold_per_day,
        unit_originium_per_day: result.production.unit.unit_originium_per_day,
        breakdown: Some(breakdown),
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
    options
        .solve_trade_combo(pool, table, order_kind, combo, must_name, override_op)
        .map(|(hit, _result)| hit)
}

fn trade_combo_layout(base: &SharedLayout, operators: &[TradeOperator]) -> SharedLayout {
    let changes_dynamic_tag_count = base.global_inject.trade_tagged().iter().any(|rule| {
        operators
            .iter()
            .any(|operator| operator.tags.contains(&rule.target_tag))
    });
    if changes_dynamic_tag_count || trade_operators_require_candidate_projection(operators) {
        project_trade_combo_layout(base, operators)
    } else {
        Arc::clone(base)
    }
}

/// 把尚未提交的当前贸易房候选投影进跨设施上下文。
///
/// 搜索候选在 `resolve_base` 的 assignment 中尚不存在；依赖“在基建内”、
/// 贸易 workforce 或 `TaggedCountInTradeSum` 的机制必须看到候选本身。
/// 已经存在于贸易 workforce 的 anchor 不重复计数；本 helper 不负责
/// `TradeStationsWithTaggedGte` 的逐房门槛投影。
fn project_trade_combo_layout(base: &LayoutContext, operators: &[TradeOperator]) -> SharedLayout {
    let mut layout = base.clone();
    let mut base_names: HashSet<String> = layout.base_workforce.iter().cloned().collect();
    let mut trade_names: HashSet<String> = layout.trade_workforce.iter().cloned().collect();

    for operator in operators {
        if base_names.insert(operator.name.clone()) {
            layout.base_workforce.push(operator.name.clone());
            if operator
                .tags
                .iter()
                .any(|tag| tag == crate::layout::TAG_DURIN)
            {
                layout.durin_in_base = layout.durin_in_base.saturating_add(1).min(4);
            }
        }
    }
    let durin_delta = layout
        .durin_virtual_lines()
        .saturating_sub(base.durin_virtual_lines());
    if durin_delta > 0 {
        for (_, amount, durin_scaled) in &mut layout.trade_gold_line_sources {
            if *durin_scaled {
                *amount += durin_delta;
                layout.global.add(
                    crate::global_resource::GlobalResourceKey::VirtualGoldLines,
                    durin_delta as f64,
                );
            }
        }
    }
    for operator in operators {
        if !trade_names.insert(operator.name.clone()) {
            continue;
        }
        let amount = crate::trade::gold_flow::virtual_line_yield(
            &operator.buff_ids,
            layout.gold_manu_line_count,
            layout.durin_virtual_lines(),
        );
        if amount > 0 {
            layout.trade_gold_line_sources.push((
                operator.name.clone(),
                amount,
                operator
                    .buff_ids
                    .iter()
                    .any(|id| id == "trade_ord_line_durin[010]"),
            ));
            layout.global.add(
                crate::global_resource::GlobalResourceKey::VirtualGoldLines,
                amount as f64,
            );
        }
        layout.trade_workforce.push(operator.name.clone());
        let mut seen_tags = HashSet::new();
        for tag in &operator.tags {
            if seen_tags.insert(tag.as_str()) {
                *layout
                    .trade_tagged_count_sum
                    .entry(tag.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    let threshold_rules: Vec<_> = layout
        .global_inject
        .trade_tagged()
        .iter()
        .filter_map(|rule| match rule.count_scope {
            crate::global_resource::TradeTaggedCountScope::QualifiedTradeRooms { min } => {
                Some((rule.target_tag.clone(), min))
            }
            _ => None,
        })
        .collect();
    for (tag, min) in threshold_rules {
        let count = operators
            .iter()
            .filter(|operator| operator.tags.iter().any(|candidate| candidate == &tag))
            .count();
        if count >= usize::from(min) {
            let key = crate::layout::trade_station_tagged_gte_key(&tag, min);
            *layout.trade_stations_tagged_gte.entry(key).or_insert(0) += 1;
        }
    }
    layout
        .global_inject
        .refresh_qualified_trade_counts(&layout.trade_stations_tagged_gte);

    Arc::new(layout)
}

pub fn hit_witch_shortcut(hit: &TradeSearchHit) -> bool {
    hit.rule_id
        .as_deref()
        .is_some_and(|id| id.starts_with("gsl_witch"))
}

pub fn hit_closure_shortcut(hit: &TradeSearchHit) -> bool {
    hit.rule_id
        .as_deref()
        .is_some_and(|id| id.starts_with("gsl_closure") || id == "gsl_blackkey_closure")
}

pub fn hit_blackkey_closure_shortcut(hit: &TradeSearchHit) -> bool {
    hit.rule_id.as_deref() == Some("gsl_blackkey_closure")
}

pub fn hit_docus_solo_shortcut(hit: &TradeSearchHit) -> bool {
    matches!(
        hit.rule_id.as_deref(),
        Some("gsl_docus_solo" | "gsl_docus_solo_e0")
    )
}
