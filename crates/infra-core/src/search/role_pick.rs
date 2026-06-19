//! Meta 贸易站落位：读 `trade_segments.json` 的 `roles[].pick_steps` 驱动 fallback 链。

use std::collections::HashSet;

use crate::error::Result;
use crate::layout::LayoutContext;
use crate::pool::{filter_trade_pool, TradePool};
use crate::skill_table::SkillTable;
use crate::trade::segment::{segment_producer_active, trade_segment_cache};

use super::trade::{
    hit_docus_solo_shortcut, search_trade_triples, search_trade_triples_filtered,
    SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
};

pub fn hit_docus_syracusa_shortcut(hit: &TradeSearchHit) -> bool {
    hit.shortcut.as_deref() == Some("gsl_docus_syracusa")
}

pub fn hit_shortcut_id(hit: &TradeSearchHit, shortcut_id: &str) -> bool {
    hit.shortcut.as_deref() == Some(shortcut_id)
}

/// Meta 站落位：按 `roles[].pick_steps` 顺序尝试，末步 `unfiltered` 与旧 Plain fallback 一致。
pub fn pick_trade_role_hit(
    role_id: &str,
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    layout: &LayoutContext,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    let sub = filter_trade_pool(pool, used);
    if sub.entries.len() < 3 {
        return Err(crate::error::Error::msg(format!(
            "trade pool has {} ready operators (need 3)",
            sub.entries.len()
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;

    let cache = trade_segment_cache().ok_or_else(|| {
        crate::error::Error::msg("trade_segments.json not loaded")
    })?;
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
                if let Ok(hit) = pick_with_shortcut_filter(
                    &sub,
                    table,
                    &opts,
                    &seg.shortcut_id,
                    used,
                ) {
                    return Ok(hit);
                }
            }
            "shortcut" => {
                let Some(sid) = step.shortcut_id.as_deref() else {
                    continue;
                };
                if let Ok(hit) = pick_with_shortcut_filter(&sub, table, &opts, sid, used) {
                    return Ok(hit);
                }
            }
            "unfiltered" => {
                let report = search_trade_triples(&sub, table, &opts)?;
                if let Ok(hit) = pick_disjoint_trade_hit(report.best, report.top, used) {
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

/// 但书 meta 站（`roles.docus`）。
pub fn pick_docus_trade_hit(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    layout: &LayoutContext,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    pick_trade_role_hit("docus", pool, table, search_opts, layout, used, top_k)
}

fn pick_with_shortcut_filter(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: &TradeSearchOptions,
    shortcut_id: &str,
    used: &HashSet<String>,
) -> Result<TradeSearchHit> {
    let Some(hit_filter) = shortcut_hit_filter(shortcut_id) else {
        return Err(crate::error::Error::msg(format!(
            "no search hit_filter for shortcut {shortcut_id}"
        )));
    };
    let report = search_trade_triples_filtered(
        pool,
        table,
        search_opts,
        SearchTripleFilter {
            hit_filter: Some(hit_filter),
            ..SearchTripleFilter::default()
        },
    )?;
    pick_disjoint_trade_hit(report.best, report.top, used)
}

fn shortcut_hit_filter(shortcut_id: &str) -> Option<fn(&TradeSearchHit) -> bool> {
    match shortcut_id {
        "gsl_docus_solo" => Some(hit_docus_solo_shortcut),
        "gsl_docus_syracusa" => Some(hit_docus_syracusa_shortcut),
        "gsl_vina_lungmen" => Some(|hit| hit_shortcut_id(hit, "gsl_vina_lungmen")),
        "gsl_penguin_texlap_e0" => Some(|hit| hit_shortcut_id(hit, "gsl_penguin_texlap_e0")),
        "gsl_penguin_texangel_e2" => Some(|hit| hit_shortcut_id(hit, "gsl_penguin_texangel_e2")),
        "gsl_penguin_exusiai_lemuen" => Some(|hit| hit_shortcut_id(hit, "gsl_penguin_exusiai_lemuen")),
        "gsl_blackkey_closure" => Some(|hit| hit_shortcut_id(hit, "gsl_blackkey_closure")),
        _ => None,
    }
}

fn pick_disjoint_trade_hit(
    best: TradeSearchHit,
    top: Vec<TradeSearchHit>,
    used: &HashSet<String>,
) -> Result<TradeSearchHit> {
    for hit in top.into_iter().chain(std::iter::once(best)) {
        let names = trade_hit_names(&hit);
        if names.iter().all(|n| !used.contains(n)) {
            return Ok(hit);
        }
    }
    Err(crate::error::Error::msg("no disjoint trade triple"))
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
