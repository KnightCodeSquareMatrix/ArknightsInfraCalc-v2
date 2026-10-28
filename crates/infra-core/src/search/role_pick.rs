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

/// 但书核心站：精0/精二均可作为核心，队友按最终效率统一排序。
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
    if pool.entry("但书").is_none() {
        return Err(crate::error::Error::msg("docus role requires owned 但书"));
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
        "witch_beta" => Some(hit_witch_beta),
        "witch_alpha" => Some(hit_witch_alpha),
        "witch_blank" => Some(hit_witch_blank),
        "witch_fallback" => Some(hit_witch_shortcut),
        "karlan" => Some(hit_karlan_jie),
        _ => None,
    }
}

fn hit_witch_beta(hit: &TradeSearchHit) -> bool {
    matches!(
        hit.rule_id.as_deref(),
        Some("gsl_witch_long_beta" | "gsl_witch_long0_beta")
    )
}

fn hit_witch_alpha(hit: &TradeSearchHit) -> bool {
    matches!(
        hit.rule_id.as_deref(),
        Some("gsl_witch_long_alpha" | "gsl_witch_long0_alpha")
    )
}

fn hit_witch_blank(hit: &TradeSearchHit) -> bool {
    matches!(
        hit.rule_id.as_deref(),
        Some("gsl_witch_long_blank" | "gsl_witch_long0_blank")
    )
}

/// 喀兰核心：银灰和孑必须同站。孑加崖心、讯使、角峰这类低档协同不算核心。
fn hit_karlan_jie(hit: &TradeSearchHit) -> bool {
    hit.names.iter().any(|n| n == "孑") && hit.names.iter().any(|n| n == "银灰")
}

fn pick_disjoint_trade_hit(
    best: TradeSearchHit,
    top: Vec<TradeSearchHit>,
    used: &HashSet<String>,
) -> Result<TradeSearchHit> {
    top.into_iter()
        .chain(std::iter::once(best))
        .filter(|hit| trade_hit_names(hit).iter().all(|n| !used.contains(n)))
        .max_by_key(role_pick_sort_key)
        .ok_or_else(|| crate::error::Error::msg("no disjoint trade triple"))
}

fn role_pick_sort_key(hit: &TradeSearchHit) -> Efficiency {
    hit.breakdown
        .as_ref()
        .map_or(hit.final_efficiency, |breakdown| breakdown.final_efficiency)
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
