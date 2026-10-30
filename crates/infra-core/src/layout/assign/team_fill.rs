use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::{BaseBlueprint, RoomId, RoomProduct};
use crate::layout::context::LayoutContext;
use crate::layout::orchestrate::AssignmentPlan;
use crate::pool::{ManuPool, PowerPool, TradePool};
use crate::skill_table::SkillTable;

use super::commit::{commit_manu_room, commit_trade_room};
use super::manufacture_fill::{
    assign_manu_room_with_anchors, forbidden_teammates, manu_options,
    manufacture_candidate_pool_for_demand, pick_capacity_manu_hit, pick_manu_hit,
};
use super::pipeline::forbid_same_room_pairs;
use super::power_fill::assign_power_rooms_with_instances as assign_power_rooms;
use super::trade_fill::{pick_trade_meta_then_plain, trade_order_from_room};
use super::AssignBaseOptions;

/// 为一支队伍填满指定的贸易/制造房间（站绑定），共享 `used` 实现跨队互斥。
/// 贸易站取当前可用最优三人组（shortcut 自然高分），制造站同理；发电/中枢/宿舍不在此处理。
#[allow(clippy::too_many_arguments)]
pub fn assign_team_producer_rooms(
    blueprint: &BaseBlueprint,
    instances: &crate::instances::OperatorInstances,
    coexist_assignment: &BaseAssignment,
    durin_dorm_planning: Option<u8>,
    trade_pool: &TradePool,
    manu_pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    trade_rooms: &[RoomId],
    manu_rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_team_trade_meta_rooms(
        blueprint,
        instances,
        coexist_assignment,
        durin_dorm_planning,
        trade_pool,
        table,
        options,
        trade_rooms,
        assignment,
        used,
    )?;
    assign_team_manu_rooms(
        blueprint,
        instances,
        coexist_assignment,
        durin_dorm_planning,
        manu_pool,
        table,
        layout,
        options,
        manu_rooms,
        &HashMap::new(),
        &[],
        assignment,
        used,
    )?;
    clear_team_trade_snapshots(assignment, trade_rooms);
    Ok(())
}

/// γ 替补半区：贸易沿用 meta 核心优先级，制造/发电仍站绑定搜索。
///
/// `plan` 是当前排班尝试已编译的统一 Plan（peak 班同一份）。γ 不重新选型，
/// 也不重新执行 anchor；只消费其中的 `ForbidSameStation` / `ForbidSameRoom`
/// 约束，与主流水线共用同一提取函数。
#[allow(clippy::too_many_arguments)]
pub fn assign_team_gamma_half(
    blueprint: &BaseBlueprint,
    instances: &crate::instances::OperatorInstances,
    coexist_assignment: &BaseAssignment,
    durin_dorm_planning: Option<u8>,
    trade_pool: &TradePool,
    manu_pool: &ManuPool,
    power_pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    trade_rooms: &[RoomId],
    manu_rooms: &[RoomId],
    power_rooms: &[RoomId],
    required_manu_by_room: &HashMap<RoomId, Vec<String>>,
    plan: &AssignmentPlan,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let forbid_same_room = forbid_same_room_pairs(plan);
    assign_team_trade_meta_rooms(
        blueprint,
        instances,
        coexist_assignment,
        durin_dorm_planning,
        trade_pool,
        table,
        options,
        trade_rooms,
        assignment,
        used,
    )?;
    assign_team_manu_rooms(
        blueprint,
        instances,
        coexist_assignment,
        durin_dorm_planning,
        manu_pool,
        table,
        layout,
        options,
        manu_rooms,
        required_manu_by_room,
        &forbid_same_room,
        assignment,
        used,
    )?;
    assign_power_rooms(
        blueprint,
        power_pool,
        table,
        layout,
        options,
        power_rooms,
        assignment,
        used,
        Some(instances),
        Some(coexist_assignment),
    )?;
    clear_team_trade_snapshots(assignment, trade_rooms);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn assign_team_trade_meta_rooms(
    blueprint: &BaseBlueprint,
    instances: &crate::instances::OperatorInstances,
    coexist_assignment: &BaseAssignment,
    durin_dorm_planning: Option<u8>,
    trade_pool: &TradePool,
    table: &SkillTable,
    options: &AssignBaseOptions,
    trade_rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let gold_lines = blueprint.gold_manu_line_count();
    let mut rooms = trade_rooms
        .iter()
        .map(|room_id| {
            blueprint.room(room_id).ok_or_else(|| {
                Error::msg(format!("team trade room {} not in blueprint", room_id.0))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    eprintln!(
        "[rotation-debug] gamma trade input rooms={:?} used={:?} coexist={:?}",
        rooms
            .iter()
            .map(|room| room.id.0.as_str())
            .collect::<Vec<_>>(),
        sorted_names(used),
        sorted_names(&coexist_assignment.operator_names()),
    );
    while let Some(room) =
        super::trade_fill::take_next_trade_room(&mut rooms, trade_pool, assignment, used)
    {
        if !assignment.operators_in(&room.id).is_empty() {
            eprintln!(
                "[rotation-debug] gamma trade skip room={} already_assigned={:?}",
                room.id.0,
                assignment
                    .operators_in(&room.id)
                    .iter()
                    .map(|operator| operator.name.as_str())
                    .collect::<Vec<_>>(),
            );
            continue;
        }
        eprintln!(
            "[rotation-debug] gamma trade select room={} remaining_rooms={:?} used_before={:?} partial={:?}",
            room.id.0,
            rooms.iter().map(|candidate| candidate.id.0.as_str()).collect::<Vec<_>>(),
            sorted_names(used),
            sorted_names(&assignment.operator_names()),
        );
        let mut current_assignment = coexist_assignment.clone();
        for assigned_room in assignment.iter_rooms() {
            current_assignment.set_room_assignment(assigned_room.clone());
        }
        let current_layout = crate::layout::resolve_base(
            blueprint,
            &current_assignment,
            Some(instances),
            Some(table),
            options.mood,
            durin_dorm_planning,
        )?
        .layout;
        let order = trade_order_from_room(room)?;
        let hit = pick_trade_meta_then_plain(
            trade_pool,
            table,
            &current_layout,
            gold_lines,
            options,
            order,
            room.level,
            used,
            &[],
            false,
        )
        .map_err(|e| Error::msg(format!("team trade {}: {e}", room.id.0)))?;
        eprintln!(
            "[rotation-debug] gamma trade hit room={} names={:?} rule={:?} efficiency={} used_after_search={:?}",
            room.id.0,
            &hit.names,
            &hit.rule_id,
            hit.final_efficiency,
            sorted_names(used),
        );
        commit_trade_room(assignment, &room.id, &hit, trade_pool, used)?;
    }
    Ok(())
}

fn sorted_names(names: &HashSet<String>) -> Vec<&str> {
    let mut names: Vec<_> = names.iter().map(String::as_str).collect();
    names.sort_unstable();
    names
}

/// 队伍 partial assignment 的最终结算必须使用完整班次（coexist + partial）。
/// 清掉逐房搜索时的局部快照，让 schedule 最终合并后按完整上下文重算。
fn clear_team_trade_snapshots(assignment: &mut BaseAssignment, trade_rooms: &[RoomId]) {
    for room_id in trade_rooms {
        if assignment.operators_in(room_id).is_empty() {
            continue;
        }
        let operators = assignment.operators_in(room_id).to_vec();
        assignment.set_room(room_id.clone(), operators);
    }
}

#[allow(clippy::too_many_arguments)]
fn assign_team_manu_rooms(
    blueprint: &BaseBlueprint,
    instances: &crate::instances::OperatorInstances,
    coexist_assignment: &BaseAssignment,
    durin_dorm_planning: Option<u8>,
    manu_pool: &ManuPool,
    table: &SkillTable,
    _layout: &LayoutContext,
    options: &AssignBaseOptions,
    manu_rooms: &[RoomId],
    required_manu_by_room: &HashMap<RoomId, Vec<String>>,
    forbid_same_room: &[(String, String)],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let candidate_pool = manufacture_candidate_pool_for_demand(manu_pool, used);

    for room_id in manu_rooms {
        if !assignment.operators_in(room_id).is_empty() {
            continue;
        }
        let room = blueprint
            .room(room_id)
            .ok_or_else(|| Error::msg(format!("team manu room {} not in blueprint", room_id.0)))?;
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => {
                return Err(Error::msg(format!(
                    "team manu room {} missing factory product",
                    room_id.0
                )))
            }
        };
        let mut current_assignment = coexist_assignment.clone();
        for assigned_room in assignment.iter_rooms() {
            current_assignment.set_room_assignment(assigned_room.clone());
        }
        let current_layout = crate::layout::resolve_base(
            blueprint,
            &current_assignment,
            Some(instances),
            Some(table),
            options.mood,
            durin_dorm_planning,
        )?
        .layout;
        let opts = manu_options(&current_layout, options, recipe, room.level);
        if let Some(required) = required_manu_by_room.get(room_id) {
            let anchors = required
                .iter()
                .map(|name| {
                    manu_pool.entry(name).cloned().ok_or_else(|| {
                        Error::msg(format!(
                            "team manu {} required operator {} missing from pool",
                            room_id.0, name
                        ))
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut room_used = used.clone();
            for name in required {
                room_used.remove(name);
            }
            // Plan forbid-same-room：anchor 的禁同房队友当作已占用，filler 搜索不会选中。
            let mut injected_forbidden = Vec::new();
            for name in required {
                for teammate in forbidden_teammates(name, forbid_same_room) {
                    if !required.contains(&teammate) && room_used.insert(teammate.clone()) {
                        injected_forbidden.push(teammate);
                    }
                }
            }
            assign_manu_room_with_anchors(
                assignment,
                room_id,
                anchors,
                manu_pool,
                table,
                &current_layout,
                options,
                recipe,
                room.level,
                &mut room_used,
                &[],
            )?;
            for teammate in &injected_forbidden {
                room_used.remove(teammate);
            }
            *used = room_used;
        } else {
            let hit = pick_manu_hit(&candidate_pool, table, opts.clone(), used, options.top_k)
                .or_else(|_| pick_manu_hit(manu_pool, table, opts, used, options.top_k))
                .or_else(|_| {
                    pick_capacity_manu_hit(
                        manu_pool,
                        table,
                        &current_layout,
                        options,
                        recipe,
                        used,
                        room.level,
                    )
                })
                .map_err(|e| Error::msg(format!("team manu {}: {e}", room_id.0)))?;
            commit_manu_room(assignment, room_id, &hit, manu_pool, used)?;
        }
    }
    Ok(())
}
