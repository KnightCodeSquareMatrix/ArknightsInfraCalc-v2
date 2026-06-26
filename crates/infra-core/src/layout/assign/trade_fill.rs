use std::collections::HashSet;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomBlueprint, RoomId, RoomProduct};
use crate::layout::context::LayoutContext;
use crate::pool::{
    filter_trade_pool, jie_e0_trade_operator, karlan_precision_active, try_filter_standalone,
    TradePool, JIE_TRADE_NAME,
};
use crate::search::{
    hit_witch_shortcut, pick_docus_trade_hit, pick_trade_role_hit, search_trade_triples,
    search_trade_triples_filtered, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
};
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};

use super::commit::{commit_trade_room, pick_disjoint_from_report, trade_hit_names};
use super::AssignBaseOptions;

const BLACKKEY_NAME: &str = "黑键";
const WITCH_TRADE_NAME: &str = "巫恋";
const DOCUS_TRADE_NAME: &str = "但书";
const CLOSURE_TRADE_NAME: &str = "可露希尔";
/// 这些旧 `base_systems` 条目是 L3/兼容锚点，不再由 registry fixed 早占贸易站。
/// 主路径改由 `trade_segments.roles` 的核心优先策略落位：但书 -> 可露希尔 -> 巫恋。
const TRADE_ROLE_MANAGED_REGISTRY_SYSTEMS: [&str; 6] = [
    "blackkey_closure",
    "witch_long_beta",
    "penguin_exusiai_lemuen",
    "penguin_texangel_e2",
    "penguin_texlap_e0",
    "vina_lungmen",
];

fn trade_hit_excludes_blackkey_witch_collide(hit: &TradeSearchHit) -> bool {
    !hit.names.iter().any(|n| n == WITCH_TRADE_NAME) && !hit_witch_shortcut(hit)
}

fn trade_hit_ok_for_greedy(hit: &TradeSearchHit) -> bool {
    let has_blackkey = hit.names.iter().any(|n| n == BLACKKEY_NAME);
    if !has_blackkey {
        return true;
    }
    trade_hit_excludes_blackkey_witch_collide(hit)
}

/// 黑键贸站不得与巫恋同房（含巫恋 shortcut 三人组）。
pub fn blackkey_witch_same_trade_room(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
) -> bool {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .any(|r| {
            trade_room_has_operator(assignment, &r.id, BLACKKEY_NAME)
                && trade_room_has_operator(assignment, &r.id, WITCH_TRADE_NAME)
        })
}

pub(super) fn trade_room_has_operator(
    assignment: &BaseAssignment,
    room_id: &RoomId,
    name: &str,
) -> bool {
    assignment
        .operators_in(room_id)
        .iter()
        .any(|o| o.name == name)
}

/// 恢复班贸易：精0 孑一站（若有），其余站贪心；按蓝图贸易站数填满。
#[allow(clippy::too_many_arguments)]
pub(super) fn assign_trade_jie_remainder(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let trade_rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .collect();
    if trade_rooms.is_empty() {
        return Ok(());
    }

    let jie_lead = !karlan_precision_active(&layout.global_inject)
        && jie_e0_trade_operator(instances, table).is_some();

    if jie_lead {
        if let Some(room) = trade_rooms
            .iter()
            .find(|r| assignment.operators_in(&r.id).is_empty())
        {
            let sub = filter_trade_pool(pool, used);
            if sub.entries.len() >= 3 {
                if let Some(jie_op) = jie_e0_trade_operator(instances, table) {
                    let search_opts =
                        trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold);
                    if let Ok(report) = search_trade_triples_filtered(
                        &sub,
                        table,
                        &search_opts,
                        SearchTripleFilter {
                            must_include_name: Some(JIE_TRADE_NAME.to_string()),
                            must_operator_override: Some(jie_op),
                            ..SearchTripleFilter::default()
                        },
                    ) {
                        commit_trade_room(assignment, &room.id, &report.best, pool, used)?;
                    }
                }
            }
        }
    }

    for room in &trade_rooms {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let order = trade_order_from_room(room)?;
        let hit = pick_trade_hit(
            pool,
            table,
            trade_room_options(layout, gold_lines, options, order),
            SearchTripleFilter::default(),
            used,
            options.top_k,
        )
        .map_err(|e| Error::msg(format!("trade recovery {}: {e}", room.id.0)))?;
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

pub(super) fn assign_trade_remainder(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for room in blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
    {
        if assignment.operators_in(&room.id).len() >= 3 {
            continue;
        }
        let order = trade_order_from_room(room)?;
        let existing = assignment.operators_in(&room.id);
        let hit = if existing.is_empty() {
            pick_trade_meta_then_plain(pool, table, layout, gold_lines, options, order, used)
        } else {
            let anchor = existing[0].name.clone();
            pick_trade_hit(
                pool,
                table,
                trade_room_options(layout, gold_lines, options, order),
                SearchTripleFilter {
                    must_include_name: Some(anchor),
                    hit_filter: Some(trade_hit_ok_for_greedy),
                    ..SearchTripleFilter::default()
                },
                used,
                options.top_k,
            )
        }
        .map_err(|e| Error::msg(format!("trade {}: {e}", room.id.0)))?;
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

pub(super) fn trade_order_from_room(room: &RoomBlueprint) -> Result<TradeOrderKind> {
    match room.product.as_ref() {
        Some(RoomProduct::Trade { order }) => Ok(*order),
        Some(RoomProduct::Factory { .. }) => Err(Error::msg(format!(
            "trade room {} has factory product",
            room.id.0
        ))),
        None => Err(Error::msg(format!(
            "trade room {} missing product",
            room.id.0
        ))),
    }
}

pub(super) fn skip_trade_core_registry_systems(skip: &mut HashSet<String>) {
    for id in TRADE_ROLE_MANAGED_REGISTRY_SYSTEMS {
        skip.insert(id.to_string());
    }
}

/// 团队贸易站取人：但书 -> 可露希尔 -> 龙巫 -> 普通散件。
///
/// 这些 meta 是核心优先级，不是固定三人组；每个核心站内部仍由贸易搜索选择最优队友。
pub(super) fn pick_trade_meta_then_plain(
    pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
    used: &mut HashSet<String>,
) -> Result<TradeSearchHit> {
    if order == TradeOrderKind::Gold && !used.contains(DOCUS_TRADE_NAME) {
        if let Ok(hit) = pick_docus_trade_hit(
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == DOCUS_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold && !used.contains(CLOSURE_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit(
            "closure",
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == CLOSURE_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold && !used.contains(WITCH_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit(
            "witch",
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == WITCH_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    pick_trade_hit(
        pool,
        table,
        trade_room_options(layout, gold_lines, options, order),
        SearchTripleFilter {
            hit_filter: Some(trade_hit_ok_for_greedy),
            ..SearchTripleFilter::default()
        },
        used,
        options.top_k,
    )
}

pub(super) fn pick_trade_hit(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    filter: SearchTripleFilter,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    let mut used_for_filter = used.clone();
    if let Some(anchor) = filter.must_include_name.as_deref() {
        used_for_filter.remove(anchor);
    }
    let sub = filter_trade_pool(pool, &used_for_filter);
    // anchor 搜索（must_include）下不做 standalone 收窄：anchor 干员（如黑键这类
    // 机械/订单速度 buffer）通常不是 standalone，收窄会把 anchor 本身滤掉，
    // 触发 "missing must-include"。standalone 收窄仅用于无 anchor 的常规余站搜索。
    let sub = if filter.must_include_name.is_some() {
        sub
    } else if karlan_precision_active(&search_opts.layout.global_inject) {
        sub
    } else {
        try_filter_standalone(&sub, FacilityKind::TradePost, 3)
    };
    if sub.entries.len() < 3 {
        return Err(Error::msg(format!(
            "trade pool has {} ready operators (need 3)",
            sub.entries.len()
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;
    let report = match search_trade_triples_filtered(&sub, table, &opts, filter.clone()) {
        Ok(r) => r,
        Err(_) if filter.hit_filter.is_some() && filter.must_include_name.is_none() => {
            search_trade_triples(&sub, table, &opts)?
        }
        Err(e) => return Err(e),
    };
    pick_disjoint_from_report(
        report.best,
        report.top,
        trade_hit_names,
        &used_for_filter,
        "no disjoint trade triple",
    )
}

pub(super) fn trade_room_options(
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
) -> TradeSearchOptions {
    TradeSearchOptions {
        top_k: options.top_k,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: Arc::new(layout.clone()),
        gold_production_lines: gold_lines,
        order_mode: TradeSearchOrderMode::Single(order),
        ..TradeSearchOptions::default()
    }
}
