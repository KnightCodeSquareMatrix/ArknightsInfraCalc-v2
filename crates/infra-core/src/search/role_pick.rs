//! Meta 贸易站落位：读 `trade_segments.json` 的 `roles[].pick_steps` 驱动 fallback 链。

use std::collections::HashSet;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::layout::LayoutContext;
use crate::pool::{filter_trade_pool, TradePool};
use crate::skill_table::SkillTable;
use crate::trade::segment::{segment_producer_active, trade_segment_cache};

use super::trade::{
    hit_blackkey_closure_shortcut, hit_closure_shortcut, hit_docus_solo_shortcut,
    hit_witch_shortcut, search_trade_triples, search_trade_triples_filtered, SearchTripleFilter,
    TradeSearchHit, TradeSearchOptions,
};

const KARLAN_TRADE_NAMES: &[&str] = &["银灰", "崖心", "讯使", "角峰"];

pub fn hit_docus_syracusa_shortcut(hit: &TradeSearchHit) -> bool {
    hit.rule_id.as_deref() == Some("gsl_docus_syracusa")
}

pub fn hit_shortcut_id(hit: &TradeSearchHit, shortcut_id: &str) -> bool {
    hit.rule_id.as_deref() == Some(shortcut_id)
}

/// Meta 站落位：按 `roles[].pick_steps` 顺序尝试。
///
/// `trade_segments.json` 的 role 只表达一个核心策略，例如“必须包含但书/可露/巫恋，
/// 优先命中某个 L3 shortcut”。普通散件兜底由调用方显式处理，避免 role 自己退化成
/// 无核心干员的 plain 站。
pub fn pick_trade_role_hit(
    role_id: &str,
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    layout: &LayoutContext,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    pick_trade_role_hit_requiring(role_id, pool, table, search_opts, layout, used, top_k, &[])
}

/// 在 role 自身核心约束之外，再要求包含一组已解析成员；用于轮换 reserve
/// 在不同目标房复用同一个实际 cohort。
#[allow(clippy::too_many_arguments)]
pub fn pick_trade_role_hit_requiring(
    role_id: &str,
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    layout: &LayoutContext,
    used: &HashSet<String>,
    top_k: usize,
    additional_required: &[String],
) -> Result<TradeSearchHit> {
    let sub = filter_trade_pool(pool, used);
    let operator_capacity = search_opts.operator_capacity.clamp(1, 3);
    if sub.entries.len() < operator_capacity {
        return Err(crate::error::Error::msg(format!(
            "trade pool has {} ready operators (need {operator_capacity})",
            sub.entries.len(),
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;

    let cache = trade_segment_cache()
        .ok_or_else(|| crate::error::Error::msg("trade_segments.json not loaded"))?;
    let role = cache
        .role(role_id)
        .ok_or_else(|| crate::error::Error::msg(format!("unknown trade role {role_id}")))?;

    for step in &role.pick_steps {
        match step.kind.as_str() {
            "segment" => {
                let Some(seg_id) = step.segment_id.as_deref() else {
                    continue;
                };
                let Some(seg) = cache.segment(seg_id) else {
                    continue;
                };
                if step.only_if_producer
                    && !segment_producer_active(&seg.producer, &layout.global_inject)
                {
                    continue;
                }
                let Some(hit_filter) = shortcut_hit_filter(&seg.shortcut_id) else {
                    continue;
                };
                if let Ok(hit) = pick_with_step_filter(
                    &sub,
                    table,
                    &opts,
                    Some(hit_filter),
                    step_must_include_names(step, additional_required),
                    used,
                ) {
                    return Ok(hit);
                }
            }
            "shortcut" => {
                let Some(sid) = step.shortcut_id.as_deref() else {
                    continue;
                };
                let Some(hit_filter) = shortcut_hit_filter(sid) else {
                    continue;
                };
                if let Ok(hit) = pick_with_step_filter(
                    &sub,
                    table,
                    &opts,
                    Some(hit_filter),
                    step_must_include_names(step, additional_required),
                    used,
                ) {
                    return Ok(hit);
                }
            }
            "filtered" => {
                let Some(filter_id) = step.hit_filter.as_deref() else {
                    continue;
                };
                let Some(hit_filter) = named_hit_filter(filter_id) else {
                    continue;
                };
                if let Ok(hit) = pick_with_step_filter(
                    &sub,
                    table,
                    &opts,
                    Some(hit_filter),
                    step_must_include_names(step, additional_required),
                    used,
                ) {
                    return Ok(hit);
                }
            }
            "unfiltered" => {
                if let Ok(hit) = pick_with_step_filter(
                    &sub,
                    table,
                    &opts,
                    None,
                    step_must_include_names(step, additional_required),
                    used,
                ) {
                    return Ok(hit);
                }
            }
            _ => {}
        }
    }

    Err(crate::error::Error::msg(format!(
        "trade role {role_id}: no pick step succeeded"
    )))
}

/// 精二但书核心站：只约束必须包含但书，队友按最终效率统一排序。
///
/// `gsl_docus_*` shortcut 只参与组合结算，不参与候选优先级。
pub fn pick_docus_trade_hit(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    layout: &LayoutContext,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    if pool.entry("但书").is_none_or(|entry| entry.elite < 2) {
        return Err(crate::error::Error::msg("docus role requires elite 2 但书"));
    }
    pick_trade_role_hit("docus", pool, table, search_opts, layout, used, top_k)
}

fn pick_with_step_filter(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: &TradeSearchOptions,
    hit_filter: Option<fn(&TradeSearchHit) -> bool>,
    must_include_names: Vec<String>,
    used: &HashSet<String>,
) -> Result<TradeSearchHit> {
    if hit_filter.is_none() && must_include_names.is_empty() {
        let report = search_trade_triples(pool, table, search_opts)?;
        return pick_disjoint_trade_hit(report.best, report.top, used);
    }
    let report = search_trade_triples_filtered(
        pool,
        table,
        search_opts,
        SearchTripleFilter {
            must_include_names,
            hit_filter,
            ..SearchTripleFilter::default()
        },
    )?;
    pick_disjoint_trade_hit(report.best, report.top, used)
}

fn step_must_include_names(
    step: &crate::trade::segment::RolePickStep,
    additional_required: &[String],
) -> Vec<String> {
    let mut names = additional_required.to_vec();
    if let Some(name) = step.must_include_name.as_ref() {
        names.push(name.clone());
    }
    for name in &step.must_include_names {
        if !names.iter().any(|n| n == name) {
            names.push(name.clone());
        }
    }
    names
}

fn shortcut_hit_filter(shortcut_id: &str) -> Option<fn(&TradeSearchHit) -> bool> {
    match shortcut_id {
        "gsl_docus_solo" => Some(hit_docus_solo_shortcut),
        "gsl_docus_syracusa" => Some(hit_docus_syracusa_shortcut),
        id if id.starts_with("gsl_closure") => Some(hit_closure_shortcut),
        id if id.starts_with("gsl_witch") => Some(hit_witch_shortcut),
        "gsl_penguin_texlap_e0" => Some(|hit| hit_shortcut_id(hit, "gsl_penguin_texlap_e0")),
        "gsl_penguin_texangel_e2" => Some(|hit| hit_shortcut_id(hit, "gsl_penguin_texangel_e2")),
        "gsl_penguin_exusiai_lemuen" => {
            Some(|hit| hit_shortcut_id(hit, "gsl_penguin_exusiai_lemuen"))
        }
        "gsl_blackkey_closure" => Some(|hit| hit_shortcut_id(hit, "gsl_blackkey_closure")),
        _ => None,
    }
}

fn named_hit_filter(filter_id: &str) -> Option<fn(&TradeSearchHit) -> bool> {
    match filter_id {
        "docus_solo" => Some(hit_docus_solo_shortcut),
        "docus_syracusa" => Some(hit_docus_syracusa_shortcut),
        "closure" => Some(hit_closure_shortcut),
        "blackkey_closure" => Some(hit_blackkey_closure_shortcut),
        "witch" => Some(hit_witch_with_tailor),
        "witch_fallback" => Some(hit_witch_shortcut),
        "karlan" => Some(hit_karlan_jie),
        _ => None,
    }
}

fn hit_witch_with_tailor(hit: &TradeSearchHit) -> bool {
    matches!(
        hit.rule_id.as_deref(),
        Some("gsl_witch_long_beta" | "gsl_witch_long_alpha")
    )
}

fn hit_karlan_jie(hit: &TradeSearchHit) -> bool {
    hit.names.iter().any(|n| n == "孑")
        && hit
            .names
            .iter()
            .any(|n| KARLAN_TRADE_NAMES.contains(&n.as_str()))
}

fn pick_disjoint_trade_hit(
    best: TradeSearchHit,
    top: Vec<TradeSearchHit>,
    used: &HashSet<String>,
) -> Result<TradeSearchHit> {
    top.into_iter()
        .chain(std::iter::once(best))
        .filter(|hit| trade_hit_names(hit).iter().all(|n| !used.contains(n)))
        .max_by_key(|hit| role_pick_sort_key(hit))
        .ok_or_else(|| crate::error::Error::msg("no disjoint trade triple"))
}

fn role_pick_sort_key(hit: &TradeSearchHit) -> Efficiency {
    hit.final_efficiency
}

fn trade_hit_names(hit: &TradeSearchHit) -> &[String] {
    if !hit.names.is_empty() {
        &hit.names
    } else if !hit.gold_names.is_empty() {
        &hit.gold_names
    } else {
        &hit.originium_names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::LayoutContext;
    use crate::pool::build_trade_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};
    use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    fn fixtures(names: &[(&str, u8)]) -> (TradePool, SkillTable, LayoutContext) {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            names
                .iter()
                .map(|(name, elite)| ((*name).to_string(), *elite))
                .collect::<HashMap<_, _>>(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        (pool, table, LayoutContext::search_baseline())
    }

    fn gold_opts(layout: &LayoutContext) -> TradeSearchOptions {
        TradeSearchOptions {
            top_k: 20,
            layout: Arc::new(layout.clone()),
            order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
            ..TradeSearchOptions::default()
        }
    }

    #[test]
    fn docus_role_requires_docus_and_never_degrades_to_plain() {
        let (pool, table, layout) =
            fixtures(&[("可露希尔", 2), ("古米", 2), ("夜刀", 2), ("斑点", 1)]);
        let err = pick_trade_role_hit(
            "docus",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("docus"),
            "docus role should fail without docus instead of returning plain: {err}"
        );
    }

    #[test]
    fn closure_role_keeps_closure_without_blackkey() {
        let (pool, table, layout) =
            fixtures(&[("可露希尔", 2), ("古米", 2), ("夜刀", 2), ("斑点", 1)]);
        let hit = pick_trade_role_hit(
            "closure",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();
        assert!(hit.names.iter().any(|n| n == "可露希尔"), "{hit:?}");
        assert!(
            hit.rule_id
                .as_deref()
                .is_none_or(|id| id.starts_with("gsl_closure")),
            "{hit:?}"
        );
    }

    #[test]
    fn closure_role_uses_actual_lv1_capacity() {
        let (pool, table, layout) = fixtures(&[("可露希尔", 2)]);
        let mut options = gold_opts(&layout);
        options.operator_capacity = 1;
        options.trade_level = 1;

        let hit = pick_trade_role_hit(
            "closure",
            &pool,
            &table,
            options,
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();

        assert_eq!(trade_hit_names(&hit), ["可露希尔"]);
    }

    #[test]
    fn witch_role_requires_and_uses_tailor_teammate() {
        let (pool, table, layout) =
            fixtures(&[("巫恋", 2), ("龙舌兰", 2), ("柏喙", 2), ("古米", 2)]);
        let hit = pick_trade_role_hit(
            "witch",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();
        assert!(hit.names.iter().any(|n| n == "巫恋"), "{hit:?}");
        assert!(hit.names.iter().any(|n| n == "龙舌兰"), "{hit:?}");
        assert!(hit.names.iter().any(|n| n == "柏喙"), "{hit:?}");
        assert_eq!(hit.rule_id.as_deref(), Some("gsl_witch_long_beta"));
    }

    #[test]
    fn witch_role_rejects_non_tailor_third_operator() {
        let (pool, table, layout) =
            fixtures(&[("巫恋", 2), ("龙舌兰", 2), ("古米", 2), ("夜刀", 2)]);
        let err = pick_trade_role_hit(
            "witch",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap_err();

        assert!(err.to_string().contains("witch"), "{err}");
    }

    #[test]
    fn witch_role_requires_tequila_while_fallback_keeps_witch() {
        let (pool, table, layout) = fixtures(&[("巫恋", 2), ("柏喙", 2), ("古米", 2), ("夜刀", 2)]);

        let err = pick_trade_role_hit(
            "witch",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("witch"),
            "dragon-witch role should fail without Tequila: {err}"
        );

        let hit = pick_trade_role_hit(
            "witch_fallback",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();
        assert!(hit.names.iter().any(|n| n == "巫恋"), "{hit:?}");
        assert_eq!(hit.rule_id.as_deref(), Some("gsl_witch_beta_blank"));
    }

    #[test]
    fn karlan_role_requires_market_jie_and_karlan_peer() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("孑", 2), ("银灰", 2), ("琳琅诗怀雅", 2), ("崖心", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect::<HashMap<_, _>>(),
        );
        let mut pool = build_trade_pool(&roster, &instances, &table).unwrap();
        crate::pool::add_jie_market_to_trade_pool(&mut pool, &instances, &table);
        let mut layout = LayoutContext::search_baseline();
        layout.global_inject.record_karlan_precision(-15.0, 6);

        let hit = pick_trade_role_hit(
            "karlan",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();

        assert!(hit.names.iter().any(|n| n == "孑"), "{hit:?}");
        assert!(hit.names.iter().any(|n| n == "银灰"), "{hit:?}");
        assert_eq!(hit.rule_id, None);
        assert!((hit.final_efficiency.as_f64() * 100.0) >= 120.0, "{hit:?}");
    }

    #[test]
    fn penguin_role_can_pick_texas_lappland_bond() {
        let (pool, table, layout) =
            fixtures(&[("德克萨斯", 2), ("拉普兰德", 2), ("空", 2), ("古米", 2)]);
        let hit = pick_trade_role_hit(
            "penguin",
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();

        assert!(hit.names.iter().any(|n| n == "德克萨斯"), "{hit:?}");
        assert!(hit.names.iter().any(|n| n == "拉普兰德"), "{hit:?}");
        assert_eq!(hit.rule_id.as_deref(), Some("gsl_penguin_texlap_e0"));
    }

    #[test]
    fn docus_role_naturally_selects_syracusa_when_final_efficiency_wins() {
        let (pool, table, mut layout) = fixtures(&[
            ("但书", 2),
            ("伺夜", 2),
            ("贝洛内", 2),
            ("德克萨斯", 2),
            ("拉普兰德", 2),
            ("空弦", 2),
            ("石英", 2),
        ]);
        layout
            .global_inject
            .record_active_source_buff("control_tra_limit&spd2[000]");
        let hit = pick_docus_trade_hit(
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap();

        for name in ["但书", "伺夜", "贝洛内"] {
            assert!(hit.names.iter().any(|n| n == name), "{hit:?}");
        }
        assert_eq!(hit.rule_id.as_deref(), Some("gsl_docus_syracusa"));
        assert_eq!(
            hit.breakdown.as_ref().unwrap().unit_output_multiplier,
            Efficiency::from_decimal(1.550)
        );
        assert!(
            hit.final_efficiency > hit.breakdown.as_ref().unwrap().paper_efficiency,
            "{hit:?}"
        );
    }

    #[test]
    fn docus_role_requires_elite_two_docus() {
        let (pool, table, layout) = fixtures(&[("但书", 1), ("空弦", 2), ("石英", 2)]);
        let err = pick_docus_trade_hit(
            &pool,
            &table,
            gold_opts(&layout),
            &layout,
            &HashSet::new(),
            20,
        )
        .unwrap_err();

        assert!(err.to_string().contains("elite 2 但书"), "{err}");
    }

    #[test]
    fn role_pick_uses_final_efficiency_instead_of_raw_unit_output() {
        let hit = |score: f64, unit_trade_per_day: f64| TradeSearchHit {
            names: vec![format!("score-{score}")],
            gold_names: vec![],
            originium_names: vec![],
            final_efficiency: Efficiency::from_decimal(score),
            mechanic_equivalent_efficiency: Efficiency::ZERO,
            rule_id: None,
            unit_trade_per_day,
            unit_gold_per_day: 0.0,
            unit_originium_per_day: 0.0,
            breakdown: Some(Default::default()),
        };
        let high_unit_low_final = hit(1.8, 20_000.0);
        let low_unit_high_final = hit(2.1, 12_000.0);
        assert!(
            role_pick_sort_key(&low_unit_high_final) > role_pick_sort_key(&high_unit_low_final)
        );
    }
}
