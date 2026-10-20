use std::collections::HashSet;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{
    station_operator_capacity, BaseBlueprint, FacilityKind, RoomBlueprint, RoomId, RoomProduct,
};
use crate::layout::context::LayoutContext;
use crate::pool::{filter_trade_pool, karlan_precision_active, try_filter_standalone, TradePool};
use crate::search::{
    pick_trade_role_hit_requiring, search_trade_triples_filtered, SearchTripleFilter,
    TradeSearchHit, TradeSearchOptions,
};
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::types::Action;

use super::commit::{commit_trade_room, pick_disjoint_from_report, trade_hit_names};
use super::{AssignBaseOptions, TradeRolePreference};

use crate::trade::legality::{
    names_have_global_forbidden_pair, witch_core_station_ok, PLAIN_FORBIDDEN_TRADE_PAIRS,
    TEQUILA_NAME as TEQUILA_TRADE_NAME, WITCH_NAME as WITCH_TRADE_NAME,
};

const DOCUS_TRADE_NAME: &str = "但书";
const CLOSURE_TRADE_NAME: &str = "可露希尔";
const GOLD_ORDER_CORE_NAMES: [&str; 4] = [
    DOCUS_TRADE_NAME,
    CLOSURE_TRADE_NAME,
    WITCH_TRADE_NAME,
    TEQUILA_TRADE_NAME,
];
const KARLAN_JIE_TRADE_NAME: &str = "孑";
/// 这些 `base_systems` 条目是 L3/兼容锚点或贸易 role 目录，不再由 registry fixed 早占岗位。
/// 主路径的可露希尔/但书 required anchor 来自 `orchestration_rules.json`；
/// 这里只跳过仍由贸易 role 搜索消费的旧 registry 条目，不声明跨体系总优先级。
const TRADE_ROLE_MANAGED_REGISTRY_SYSTEMS: [&str; 5] = [
    "blackkey_closure",
    "witch_long_beta",
    "penguin_exusiai_lemuen",
    "penguin_texangel_e2",
    "penguin_texlap_e0",
];

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
            names_have_global_forbidden_pair(
                assignment
                    .operators_in(&r.id)
                    .iter()
                    .map(|o| o.name.as_str()),
            )
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

pub(super) fn assign_trade_remainder(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    gold_lines: u32,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_trade_remainder_with_plan_anchors(
        blueprint,
        pool,
        table,
        instances,
        gold_lines,
        options,
        durin_dorm_planning,
        &[],
        assignment,
        used,
    )
}

pub(super) fn assign_trade_remainder_with_plan_anchors(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    gold_lines: u32,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
    plan_anchors: &[crate::layout::orchestrate::SystemAnchor],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let mut rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|room| {
            room.kind == FacilityKind::TradePost
                && assignment.operators_in(&room.id).len() < room.operator_capacity()
        })
        .collect();

    while let Some(room) = take_next_trade_room(&mut rooms, pool, assignment, used) {
        let current_layout = crate::layout::resolve_base(
            blueprint,
            assignment,
            Some(instances),
            Some(table),
            options.mood,
            durin_dorm_planning,
        )?
        .layout;
        let order = trade_order_from_room(room)?;
        let existing = assignment.operators_in(&room.id);
        let anchors: Vec<_> = existing.iter().map(|op| op.name.clone()).collect();
        if anchors.is_empty() && pool.entries.iter().all(|entry| used.contains(&entry.name)) {
            continue;
        }
        let hit = pick_trade_meta_then_plain(
            pool,
            table,
            &current_layout,
            gold_lines,
            options,
            order,
            room.level,
            used,
            &anchors,
        );
        let hit = match hit {
            Ok(hit) => hit,
            Err(error) => {
                if let Some(anchor) = plan_anchors.iter().find(|anchor| {
                    anchor.optional_trade_role.is_some()
                        && anchor.room_id.as_ref() == Some(&room.id)
                        && anchors.iter().any(|name| name == &anchor.operator)
                }) {
                    return Err(Error::OptionalTradeRoleInfeasible {
                        system_id: anchor.system_id.clone(),
                        role_id: anchor.optional_trade_role.clone().unwrap_or_default(),
                        operator: anchor.operator.clone(),
                        message: format!("trade {}: {error}", room.id.0),
                    });
                }
                if plan_anchors.iter().any(|anchor| {
                    anchor.system_id == "pozyomka_durin"
                        && anchor.facility == FacilityKind::TradePost
                        && anchor.trade_order == Some(TradeOrderKind::Gold)
                        && anchor.optional_trade_role.is_none()
                        && anchor.room_id.as_ref() == Some(&room.id)
                        && anchors.iter().any(|name| name == &anchor.operator)
                }) {
                    return Err(Error::GoldFlowCoreInfeasible {
                        message: format!(
                            "four-gold core cannot complete required trade room {}: {error}",
                            room.id.0
                        ),
                    });
                }
                return Err(Error::msg(format!("trade {}: {error}", room.id.0)));
            }
        };
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn refill_trade_room_with_core(
    blueprint: &BaseBlueprint,
    room_id: &RoomId,
    core: &str,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
    forbidden_names: &[String],
    assignment: &mut BaseAssignment,
) -> Result<()> {
    let room = blueprint
        .room(room_id)
        .ok_or_else(|| Error::msg(format!("Fiammetta trade room {} disappeared", room_id.0)))?;
    if room.kind != FacilityKind::TradePost || trade_order_from_room(room)? != TradeOrderKind::Gold
    {
        return Err(Error::msg(format!(
            "Fiammetta core {core} requires a gold trade room"
        )));
    }

    assignment.remove_operators_where(|_, operator| operator.name == core);
    assignment.set_room(room_id.clone(), Vec::new());
    let mut used = assignment.operator_names();
    used.extend(forbidden_names.iter().cloned());
    let layout = crate::layout::resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_dorm_planning,
    )?
    .layout;
    let role_id = match core {
        DOCUS_TRADE_NAME => "docus",
        CLOSURE_TRADE_NAME => "closure",
        other => {
            return Err(Error::msg(format!(
                "unsupported single-core Fiammetta trade target {other}"
            )))
        }
    };
    let hit = pick_trade_role_hit_requiring(
        role_id,
        pool,
        table,
        trade_room_options(
            &layout,
            blueprint.gold_manu_line_count(),
            options,
            TradeOrderKind::Gold,
            room.level,
        ),
        &layout,
        &used,
        options.top_k,
        &[core.to_string()],
    )?;
    let operators = hit
        .names
        .iter()
        .map(|name| {
            pool.entry(name)
                .map(|entry| crate::layout::AssignedOperator::from_progress(name, entry.progress))
                .ok_or_else(|| Error::msg(format!("Fiammetta trade candidate {name} missing")))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), operators);
    Ok(())
}

pub(crate) fn beta_tailor_names(pool: &TradePool, table: &SkillTable) -> Vec<String> {
    let mut names: Vec<_> = pool
        .entries
        .iter()
        .filter(|entry| {
            entry.buff_ids.iter().any(|buff_id| {
                matches!(
                    buff_id.as_str(),
                    "trade_ord_wt&cost[010]" | "trade_ord_wt&cost[011]" | "trade_ord_wt&cost[012]"
                ) || table.get(buff_id).is_some_and(|skill| {
                    skill.atoms.iter().any(|atom| {
                        matches!(&atom.action, Action::TagOrder { tag } if tag == "tailor_beta")
                    })
                })
            })
        })
        .map(|entry| entry.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn refill_witch_room_with_tailor(
    blueprint: &BaseBlueprint,
    room_id: &RoomId,
    tailor: &str,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
    assignment: &mut BaseAssignment,
) -> Result<crate::Efficiency> {
    let room = blueprint
        .room(room_id)
        .ok_or_else(|| Error::msg(format!("Witch trade room {} disappeared", room_id.0)))?;
    if room.kind != FacilityKind::TradePost || !witch_core_station_ok(room) {
        return Err(Error::msg(
            "Fiammetta Witch group requires a level-3 gold trade room",
        ));
    }
    assignment.remove_operators_where(|_, operator| {
        matches!(
            operator.name.as_str(),
            WITCH_TRADE_NAME | TEQUILA_TRADE_NAME
        ) || operator.name == tailor
    });
    assignment.set_room(room_id.clone(), Vec::new());
    let used = assignment.operator_names();
    let layout = crate::layout::resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_dorm_planning,
    )?
    .layout;
    let required = vec![
        WITCH_TRADE_NAME.to_string(),
        TEQUILA_TRADE_NAME.to_string(),
        tailor.to_string(),
    ];
    let hit = pick_trade_role_hit_requiring(
        "witch",
        pool,
        table,
        trade_room_options(
            &layout,
            blueprint.gold_manu_line_count(),
            options,
            TradeOrderKind::Gold,
            room.level,
        ),
        &layout,
        &used,
        options.top_k,
        &required,
    )?;
    let operators = hit
        .names
        .iter()
        .map(|name| {
            pool.entry(name)
                .map(|entry| crate::layout::AssignedOperator::from_progress(name, entry.progress))
                .ok_or_else(|| Error::msg(format!("Witch candidate {name} missing")))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), operators);
    Ok(hit.final_efficiency)
}

pub(super) fn take_next_trade_room<'a>(
    rooms: &mut Vec<&'a RoomBlueprint>,
    pool: &TradePool,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
) -> Option<&'a RoomBlueprint> {
    let docus_pending = !used.contains(DOCUS_TRADE_NAME) && pool.entry(DOCUS_TRADE_NAME).is_some();
    rooms.sort_by(|left, right| {
        let left_gold = trade_order_from_room(left).ok() == Some(TradeOrderKind::Gold);
        let right_gold = trade_order_from_room(right).ok() == Some(TradeOrderKind::Gold);
        right_gold
            .cmp(&left_gold)
            .then_with(|| {
                trade_room_core_rank(assignment, left).cmp(&trade_room_core_rank(assignment, right))
            })
            .then_with(|| {
                if left_gold && right_gold {
                    if docus_pending {
                        left.level.cmp(&right.level)
                    } else {
                        right.level.cmp(&left.level)
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    (!rooms.is_empty()).then(|| rooms.remove(0))
}

fn trade_room_core_rank(assignment: &BaseAssignment, room: &RoomBlueprint) -> u8 {
    if trade_room_has_operator(assignment, &room.id, DOCUS_TRADE_NAME) {
        0
    } else if trade_room_has_operator(assignment, &room.id, CLOSURE_TRADE_NAME) {
        1
    } else if trade_room_has_operator(assignment, &room.id, WITCH_TRADE_NAME)
        || trade_room_has_operator(assignment, &room.id, TEQUILA_TRADE_NAME)
    {
        2
    } else {
        3
    }
}

pub(super) fn refresh_trade_efficiency_snapshots(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
) -> Result<()> {
    let resolved = crate::layout::resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_dorm_planning,
    )?;
    let gold_lines = resolved.gold_manu_line_count();
    for room in resolved.trade_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let input = crate::trade::TradeRoomInput {
            level: room.level,
            operators: room.operators,
            order_count: None,
            mood: options.mood,
            gold_production_lines: Some(gold_lines),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(room.layout),
            active_order_kind: room.order,
        };
        let solved = crate::trade::solve_trade_with_shift(&input, table, options.shift_hours)?;
        let operators = assignment.operators_in(&room.id).to_vec();
        assignment.set_room_with_efficiency(
            room.id,
            operators,
            Some(crate::layout::RoomEfficiencySnapshot {
                trade_base_efficiency: solved.efficiency.paper.base_efficiency,
                trade_paper_efficiency: solved.efficiency.paper.paper_efficiency,
                trade_occupancy_efficiency: solved.efficiency.paper.occupancy_efficiency,
                trade_control_efficiency: solved.efficiency.paper.control_efficiency,
                trade_unit_output_multiplier: solved
                    .efficiency
                    .production_basis
                    .unit_output_multiplier,
                trade_final_efficiency: solved.efficiency.final_efficiency,
                trade_total_efficiency: solved.efficiency.total_efficiency,
                trade_equivalent_skill_efficiency: solved.efficiency.equivalent_skill_efficiency,
                trade_equivalent_trade_efficiency: solved.efficiency.trade_equivalent_efficiency,
                trade_equivalent_gold_efficiency: solved.efficiency.gold_equivalent_efficiency,
                trade_combined_output_value: solved.efficiency.combined_output_value,
                trade_output_per_day: solved.production.unit.unit_trade_per_day
                    * solved.efficiency.total_efficiency.as_f64(),
                gold_consumption_per_day: solved.production.unit.gold_consumption_per_day
                    * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE
                    * solved.efficiency.total_efficiency.as_f64(),
                trade_originium_shards_per_day: solved.production.unit.unit_originium_per_day
                    * solved.efficiency.total_efficiency.as_f64(),
                trade_rule_id: solved.rule_id,
                trade_skill_efficiency: solved.efficiency.paper.skill_efficiency,
                trade_mechanic_equivalent_efficiency: solved
                    .order_mechanic
                    .mechanic_equivalent_efficiency,
                ..Default::default()
            }),
        );
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

/// 未被 plan 认领的金单贸易站：但书 -> 可露希尔 -> 完整龙巫 -> 巫恋兜底 -> 喀兰 -> 企鹅 -> 普通散件。
///
/// Peak 的首个但书 / 一贸可露希尔通常已由 Plan 落位；同一入口也供 gamma fill 使用。
pub(super) fn pick_trade_meta_then_plain(
    pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
    room_level: u8,
    used: &mut HashSet<String>,
    required_names: &[String],
) -> Result<TradeSearchHit> {
    if order == TradeOrderKind::Originium
        && required_names
            .iter()
            .any(|name| GOLD_ORDER_CORE_NAMES.contains(&name.as_str()))
    {
        return Err(Error::msg(
            "originium trade room cannot contain a gold-order core operator",
        ));
    }

    let mut role_used = used.clone();
    for name in required_names {
        role_used.remove(name);
    }
    let witch_preferred = order == TradeOrderKind::Gold
        && options.trade_role_preference == Some(TradeRolePreference::Witch);
    if witch_preferred && !role_used.contains(WITCH_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "witch",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            if hit.names.iter().any(|name| name == WITCH_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold
        && !role_used.contains(DOCUS_TRADE_NAME)
        && pool.entry(DOCUS_TRADE_NAME).is_some()
    {
        match pick_trade_role_hit_requiring(
            "docus",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            Ok(hit) => return Ok(hit),
            Err(error) if required_names.iter().any(|name| name == DOCUS_TRADE_NAME) => {
                return Err(Error::msg(format!(
                    "required docus core has no legal teammate combination: {error}"
                )));
            }
            Err(_) => {}
        }
    }
    if order == TradeOrderKind::Gold
        && !role_used.contains(CLOSURE_TRADE_NAME)
        && pool
            .entry(CLOSURE_TRADE_NAME)
            .is_some_and(|entry| entry.elite >= 2)
    {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "closure",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            return Ok(hit);
        }
    }
    if order == TradeOrderKind::Gold && !role_used.contains(WITCH_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "witch",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            if hit.names.iter().any(|n| n == WITCH_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold
        && !role_used.contains(WITCH_TRADE_NAME)
        && pool
            .entry(TEQUILA_TRADE_NAME)
            .is_none_or(|_| role_used.contains(TEQUILA_TRADE_NAME))
    {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "witch_fallback",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            if hit.names.iter().any(|n| n == WITCH_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold
        && karlan_precision_active(&layout.global_inject)
        && !role_used.contains(KARLAN_JIE_TRADE_NAME)
    {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "karlan",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            if hit.names.iter().any(|n| n == KARLAN_JIE_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold {
        if let Ok(hit) = pick_trade_role_hit_requiring(
            "penguin",
            pool,
            table,
            trade_room_options(
                layout,
                gold_lines,
                options,
                TradeOrderKind::Gold,
                room_level,
            ),
            layout,
            &role_used,
            options.top_k,
            required_names,
        ) {
            return Ok(hit);
        }
    }
    let plain_forbidden_pairs: Vec<(String, String)> = PLAIN_FORBIDDEN_TRADE_PAIRS
        .iter()
        .map(|&(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let mut plain_used = used.clone();
    if order == TradeOrderKind::Originium {
        plain_used.extend(GOLD_ORDER_CORE_NAMES.map(str::to_string));
    }
    pick_trade_hit(
        pool,
        table,
        trade_room_options(layout, gold_lines, options, order, room_level),
        SearchTripleFilter {
            must_include_names: required_names.to_vec(),
            forbidden_pairs: plain_forbidden_pairs,
            ..SearchTripleFilter::default()
        },
        &plain_used,
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
    for anchor in filter.must_include_names() {
        used_for_filter.remove(&anchor);
    }
    let base_sub = filter_trade_pool(pool, &used_for_filter);
    // Search the reduced standalone pool first; expand only when it cannot
    // provide a complete legal room.
    let reduced_sub = if filter.has_must_include() {
        base_sub.clone()
    } else if karlan_precision_active(&search_opts.layout.global_inject) {
        base_sub.clone()
    } else {
        try_filter_standalone(
            &base_sub,
            FacilityKind::TradePost,
            search_opts.operator_capacity,
        )
    };
    let full_pool = base_sub;
    let sub = if reduced_sub.entries.len() < search_opts.operator_capacity {
        full_pool.clone()
    } else {
        reduced_sub
    };
    if sub.entries.is_empty() {
        if !filter.has_must_include() {
            return Ok(TradeSearchHit {
                names: Vec::new(),
                gold_names: Vec::new(),
                originium_names: Vec::new(),
                final_efficiency: crate::Efficiency::ZERO,
                mechanic_equivalent_efficiency: crate::Efficiency::ZERO,
                rule_id: None,
                unit_trade_per_day: 0.0,
                unit_gold_per_day: 0.0,
                unit_originium_per_day: 0.0,
                breakdown: None,
            });
        }
        return Err(Error::msg(format!(
            "trade pool has {} ready operators (need {})",
            sub.entries.len(),
            search_opts.operator_capacity
        )));
    }
    let mut opts = search_opts;
    opts.operator_capacity = opts.operator_capacity.min(sub.entries.len());
    opts.top_k = top_k;
    let ordinary = match search_trade_triples_filtered(&sub, table, &opts, filter.clone()) {
        Ok(report) => Ok(report),
        Err(_) if !filter.has_must_include() => {
            let mut fallback_filter = filter.clone();
            fallback_filter.hit_filter = None;
            search_trade_triples_filtered(&full_pool, table, &opts, fallback_filter)
        }
        Err(error) => Err(error),
    }
    .and_then(|report| {
        pick_disjoint_from_report(
            report.best,
            report.top,
            trade_hit_names,
            &used_for_filter,
            "no disjoint trade triple",
        )
    });

    // The standalone pool omits this two-line gold-flow pair. Compare its best
    // legal third teammate against the ordinary winner without claiming the core Plan.
    if opts.order_mode == TradeSearchOrderMode::Single(TradeOrderKind::Gold)
        && opts.gold_production_lines == 2
        && !filter.has_must_include()
        && filter.hit_filter.is_none()
        && full_pool.entry("鸿雪").is_some()
        && full_pool.entry("图耶").is_some()
        && opts.operator_capacity >= 2
    {
        let mut pair_filter = filter;
        pair_filter.must_include_names = vec!["鸿雪".to_string(), "图耶".to_string()];
        let mut pair_opts = opts;
        pair_opts.full_pool = true;
        if let Ok(report) =
            search_trade_triples_filtered(&full_pool, table, &pair_opts, pair_filter)
        {
            if let Ok(pair) = pick_disjoint_from_report(
                report.best,
                report.top,
                trade_hit_names,
                &used_for_filter,
                "no disjoint trade triple",
            ) {
                let score = |hit: &TradeSearchHit| {
                    hit.breakdown
                        .as_ref()
                        .map_or(hit.final_efficiency, |value| value.final_efficiency)
                };
                if ordinary
                    .as_ref()
                    .map_or(true, |winner| score(&pair) > score(winner))
                {
                    return Ok(pair);
                }
            }
        }
    }
    ordinary
}

pub(super) fn trade_room_options(
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
    room_level: u8,
) -> TradeSearchOptions {
    TradeSearchOptions {
        trade_level: room_level,
        operator_capacity: station_operator_capacity(room_level),
        top_k: options.top_k,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: Arc::new(layout.clone()),
        gold_production_lines: gold_lines,
        order_mode: TradeSearchOrderMode::Single(order),
        ..TradeSearchOptions::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::blueprint::{BlueprintScenario, RoomBlueprint};
    use crate::pool::build_trade_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn gold_room(id: &str, level: u8) -> RoomBlueprint {
        RoomBlueprint {
            id: RoomId::from(id),
            kind: FacilityKind::TradePost,
            level,
            product: Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold,
            }),
            dorm_beds: None,
            dorm_ambience_level: None,
        }
    }

    #[test]
    fn trade_room_order_uses_low_for_docus_then_high_for_closure_before_originium() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("但书", 2), ("可露希尔", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let source = RoomBlueprint {
            product: Some(RoomProduct::Trade {
                order: TradeOrderKind::Originium,
            }),
            ..gold_room("source-first", 3)
        };
        let high = gold_room("gold-high", 3);
        let low = gold_room("gold-low", 2);
        let mut rooms = vec![&source, &high, &low];
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        let first = take_next_trade_room(&mut rooms, &pool, &assignment, &used).unwrap();
        assert_eq!(first.id.0, "gold-low");
        assignment.set_room(
            first.id.clone(),
            vec![crate::AssignedOperator::new("但书", 2)],
        );
        used.insert("但书".to_string());

        let second = take_next_trade_room(&mut rooms, &pool, &assignment, &used).unwrap();
        assert_eq!(second.id.0, "gold-high");
        assignment.set_room(
            second.id.clone(),
            vec![crate::AssignedOperator::new("可露希尔", 2)],
        );
        used.insert("可露希尔".to_string());

        assert_eq!(
            take_next_trade_room(&mut rooms, &pool, &assignment, &used)
                .unwrap()
                .id
                .0,
            "source-first"
        );
    }

    #[test]
    fn trade_role_order_prefers_closure_and_rejects_incomplete_lv2_witch_tequila() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [
                ("可露希尔", 2),
                ("巫恋", 2),
                ("龙舌兰", 2),
                ("柏喙", 2),
                ("古米", 2),
                ("夜刀", 2),
                ("石英", 2),
                ("月见夜", 2),
            ]
            .into_iter()
            .map(|(name, elite)| (name.to_string(), elite))
            .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let layout = LayoutContext::search_baseline();
        let options = AssignBaseOptions::default();
        let mut used = HashSet::new();

        let closure = pick_trade_meta_then_plain(
            &pool,
            &table,
            &layout,
            2,
            &options,
            TradeOrderKind::Gold,
            3,
            &mut used,
            &[],
        )
        .unwrap();
        assert!(closure.names.iter().any(|name| name == CLOSURE_TRADE_NAME));
        used.extend(closure.names);

        let low_room = pick_trade_meta_then_plain(
            &pool,
            &table,
            &layout,
            2,
            &options,
            TradeOrderKind::Gold,
            2,
            &mut used,
            &[],
        )
        .unwrap();
        assert!(
            !(low_room.names.iter().any(|name| name == WITCH_TRADE_NAME)
                && low_room.names.iter().any(|name| name == TEQUILA_TRADE_NAME)),
            "二级站不得选择不完整巫恋+龙舌兰核心: {low_room:?}"
        );
    }

    #[test]
    fn sparse_originium_fill_excludes_gold_order_cores() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [
                ("但书", 2),
                ("可露希尔", 2),
                ("巫恋", 2),
                ("龙舌兰", 2),
                ("空爆", 2),
                ("深巡", 2),
                ("玫兰莎", 2),
            ]
            .into_iter()
            .map(|(name, elite)| (name.to_string(), elite))
            .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let hit = pick_trade_meta_then_plain(
            &pool,
            &table,
            &LayoutContext::search_baseline(),
            0,
            &AssignBaseOptions::default(),
            TradeOrderKind::Originium,
            3,
            &mut HashSet::new(),
            &[],
        )
        .unwrap();

        for core in [
            DOCUS_TRADE_NAME,
            CLOSURE_TRADE_NAME,
            WITCH_TRADE_NAME,
            TEQUILA_TRADE_NAME,
        ] {
            assert!(
                !hit.names.iter().any(|name| name == core),
                "源石站不得通过 plain 回退选择金单核心 {core}: {hit:?}"
            );
        }
    }

    #[test]
    fn seeded_lv2_witch_room_cannot_complete_with_tequila() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [
                ("巫恋", 2),
                ("龙舌兰", 2),
                ("古米", 2),
                ("夜刀", 2),
                ("石英", 2),
            ]
            .into_iter()
            .map(|(name, elite)| (name.to_string(), elite))
            .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let seeded = vec![WITCH_TRADE_NAME.to_string()];
        let hit = pick_trade_meta_then_plain(
            &pool,
            &table,
            &LayoutContext::search_baseline(),
            2,
            &AssignBaseOptions::default(),
            TradeOrderKind::Gold,
            2,
            &mut HashSet::from([WITCH_TRADE_NAME.to_string()]),
            &seeded,
        )
        .unwrap();

        assert!(hit.names.iter().any(|name| name == WITCH_TRADE_NAME));
        assert!(
            !hit.names.iter().any(|name| name == TEQUILA_TRADE_NAME),
            "半满二级站不得用龙舌兰补成不完整龙巫: {hit:?}"
        );
    }

    #[test]
    fn unclaimed_partial_high_gold_room_waits_for_pending_docus_low_room() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("但书", 2), ("可露希尔", 2), ("古米", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let high = gold_room("gold-high", 3);
        let low = gold_room("gold-low", 2);
        let mut rooms = vec![&high, &low];
        let mut assignment = BaseAssignment::default();
        assignment.set_room("gold-high", vec![crate::AssignedOperator::new("古米", 2)]);

        assert_eq!(
            take_next_trade_room(
                &mut rooms,
                &pool,
                &assignment,
                &HashSet::from(["古米".to_string()]),
            )
            .unwrap()
            .id
            .0,
            "gold-low",
            "普通半满高等级站不得抢在待落位的但书低等级站之前"
        );
    }

    #[test]
    fn sparse_lv1_multi_gold_keeps_docus_then_closure() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [(DOCUS_TRADE_NAME, 2), (CLOSURE_TRADE_NAME, 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let blueprint = BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms: vec![gold_room("gold-a", 1), gold_room("gold-b", 1)],
        };
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_trade_remainder(
            &blueprint,
            &pool,
            &table,
            &instances,
            0,
            &AssignBaseOptions::default(),
            None,
            &mut assignment,
            &mut used,
        )
        .unwrap();

        assert_eq!(
            assignment.operators_in(&RoomId::from("gold-a"))[0].name,
            DOCUS_TRADE_NAME
        );
        assert_eq!(
            assignment.operators_in(&RoomId::from("gold-b"))[0].name,
            CLOSURE_TRADE_NAME
        );
    }

    #[test]
    fn later_trade_room_sees_vigil_committed_in_earlier_room() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("伺夜", 2), ("贝洛内", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let blueprint = BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms: vec![
                RoomBlueprint {
                    id: RoomId::from("trade_a"),
                    kind: FacilityKind::TradePost,
                    level: 1,
                    product: Some(RoomProduct::Trade {
                        order: TradeOrderKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("trade_b"),
                    kind: FacilityKind::TradePost,
                    level: 1,
                    product: Some(RoomProduct::Trade {
                        order: TradeOrderKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("meeting"),
                    kind: FacilityKind::MeetingRoom,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
            ],
        };
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        assign_trade_remainder(
            &blueprint,
            &pool,
            &table,
            &instances,
            0,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
            None,
            &mut assignment,
            &mut used,
        )
        .unwrap();

        assert_eq!(
            assignment.operators_in(&RoomId::from("trade_a"))[0].name,
            "伺夜"
        );
        assert_eq!(
            assignment.operators_in(&RoomId::from("trade_b"))[0].name,
            "贝洛内"
        );
        assert_eq!(
            assignment
                .efficiency_in(&RoomId::from("trade_b"))
                .unwrap()
                .trade_skill_efficiency,
            crate::Efficiency::from_decimal(0.400),
            "后站贝洛内必须看见前站已提交的伺夜并获得跨站 +10%"
        );
    }

    #[test]
    fn final_refresh_keeps_earlier_trade_snapshot_current_room_scoped() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let blueprint = BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms: vec![
                RoomBlueprint {
                    id: RoomId::from("control"),
                    kind: FacilityKind::ControlCenter,
                    level: 5,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("trade_a"),
                    kind: FacilityKind::TradePost,
                    level: 1,
                    product: Some(RoomProduct::Trade {
                        order: TradeOrderKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("trade_b"),
                    kind: FacilityKind::TradePost,
                    level: 1,
                    product: Some(RoomProduct::Trade {
                        order: TradeOrderKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("meeting"),
                    kind: FacilityKind::MeetingRoom,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
            ],
        };
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            vec![crate::layout::AssignedOperator::new("八幡海铃", 2)],
        );
        assignment.set_room(
            "trade_a",
            vec![crate::layout::AssignedOperator::new("伺夜", 2)],
        );
        let options = AssignBaseOptions::default();

        refresh_trade_efficiency_snapshots(
            &blueprint,
            &mut assignment,
            &instances,
            &table,
            &options,
            None,
        )
        .unwrap();
        let one_consumer = assignment
            .efficiency_in(&RoomId::from("trade_a"))
            .unwrap()
            .trade_paper_efficiency;
        let one_control = assignment
            .efficiency_in(&RoomId::from("trade_a"))
            .unwrap()
            .trade_control_efficiency;

        assignment.set_room(
            "trade_b",
            vec![crate::layout::AssignedOperator::new("贝洛内", 2)],
        );
        refresh_trade_efficiency_snapshots(
            &blueprint,
            &mut assignment,
            &instances,
            &table,
            &options,
            None,
        )
        .unwrap();
        let two_consumers = assignment
            .efficiency_in(&RoomId::from("trade_a"))
            .unwrap()
            .trade_paper_efficiency;
        assert_eq!(
            one_control,
            crate::Efficiency::from_decimal(0.050),
            "trade_a 站内 1 名叙拉古（伺夜）→ +5% 中枢注入"
        );
        assert_eq!(
            two_consumers - one_consumer,
            crate::Efficiency::ZERO,
            "八幡海铃按站内 scope 结算，trade_b 加入叙拉古不得改变 trade_a 注入"
        );
    }
}
