//! Shared-state construction for the trade domain. Other facilities stay fixed;
//! existing room members are Plan/seed requirements, rather than greedy winners.

use std::collections::HashSet;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::global_resource::TradeTaggedCountScope;
use crate::layout::{
    AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, RoomId, SystemConstraint,
};
use crate::pool::{filter_trade_pool, TradePool};
use crate::search::trade_allocation::{
    search_trade_allocation, TradeAllocationRequest, TradeAllocationRoom, TradeAllocationState,
};
use crate::types::{Action, AtomScope, Condition, Selector};
use crate::{OperatorInstances, SkillTable};

use super::commit::commit_trade_room;
use super::trade_fill::{trade_order_from_room, trade_room_options};
use super::AssignBaseOptions;

#[allow(clippy::too_many_arguments)]
pub(super) fn assign_trade_joint(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    options: &AssignBaseOptions,
    durin_plan: Option<u8>,
    plan: Option<&crate::layout::orchestrate::AssignmentPlan>,
    room_ids: Option<&[RoomId]>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<bool> {
    let mut stage = crate::profile::start_solver_stage("shift_pipeline", "trade_joint_fill");
    let mut rooms = blueprint.rooms_of(FacilityKind::TradePost);
    rooms.retain(|room| room_ids.is_none_or(|ids| ids.contains(&room.id)));
    rooms.sort_by_key(|room| {
        (
            trade_order_from_room(room).is_ok_and(|order| !order.is_gold()),
            room.id.0.clone(),
        )
    });
    let room_members: Vec<_> = rooms
        .iter()
        .map(|room| assignment.operators_in(&room.id).to_vec())
        .collect();
    let required_names: HashSet<_> = room_members
        .iter()
        .flatten()
        .map(|op| op.name.clone())
        .collect();
    let mut fixed = assignment.clone();
    for room in &rooms {
        fixed.remove_room(&room.id.0);
    }
    let mut unavailable = used.clone();
    unavailable.retain(|name| !required_names.contains(name));
    // Blackkey's employment is decided by the upstream cross-facility Plan.
    if !required_names.contains("黑键") {
        unavailable.insert("黑键".into());
    }
    let pool = filter_trade_pool(pool, &unavailable);
    let layout = crate::layout::resolve_base(
        blueprint,
        &fixed,
        Some(instances),
        Some(table),
        options.mood,
        durin_plan,
    )?
    .layout;
    if layout.global_inject.trade_tagged().iter().any(|rule| {
        matches!(
            rule.count_scope,
            TradeTaggedCountScope::QualifiedTradeRooms { .. }
        )
    }) {
        // Explicitly deferred domain (凛御银灰), retaining its current production path.
        stage.finish();
        return Ok(false);
    }
    let index_of = |name: &str| pool.entries.iter().position(|entry| entry.name == name);
    let required_by_room: Vec<Vec<usize>> = room_members
        .iter()
        .map(|members| {
            members
                .iter()
                .map(|member| {
                    index_of(&member.name).ok_or_else(|| {
                        Error::no_candidate(format!(
                            "required trade operator {} missing from admitted pool",
                            member.name
                        ))
                    })
                })
                .collect()
        })
        .collect::<Result<_>>()?;
    let required: Vec<_> = required_by_room.iter().flatten().copied().collect();
    let named_targets: HashSet<_> = pool
        .entries
        .iter()
        .flat_map(|entry| entry.compiled_atoms.iter())
        .filter_map(|compiled| match &compiled.atom.condition {
            Some(Condition::OperatorInBase { name } | Condition::OperatorInTrade { name }) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    let mut counted_tags: HashSet<_> = pool
        .entries
        .iter()
        .flat_map(|entry| entry.compiled_atoms.iter())
        .filter_map(|compiled| match &compiled.atom.selector {
            Some(Selector::TaggedCountInBase { tag } | Selector::TaggedCountInTradeSum { tag }) => {
                Some(tag.clone())
            }
            _ => None,
        })
        .collect();
    counted_tags.extend(
        layout
            .global_inject
            .trade_tagged()
            .iter()
            .filter(|rule| matches!(rule.count_scope, TradeTaggedCountScope::AllTradeRooms))
            .map(|rule| rule.target_tag.clone()),
    );
    let sources: Vec<_> = pool
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (!required.contains(&index)
                && (named_targets.contains(&entry.name)
                    || (layout.durin_in_base < 4
                        && entry.tags.iter().any(|tag| tag == crate::layout::TAG_DURIN))
                    || entry.tags.iter().any(|tag| counted_tags.contains(tag))
                    || crate::trade::gold_flow::virtual_line_yield(
                        &entry.buff_ids,
                        blueprint.gold_manu_line_count(),
                        4,
                    ) > 0
                    || entry.compiled_atoms.iter().any(|compiled| {
                        compiled.atom.scope == AtomScope::Global
                            && matches!(compiled.atom.action, Action::StateProduce { .. })
                    })))
            .then_some(index)
        })
        .collect();
    let varying_global = sources.iter().any(|&index| {
        pool.entries[index].compiled_atoms.iter().any(|compiled| {
            compiled.atom.scope == AtomScope::Global
                && matches!(compiled.atom.action, Action::StateProduce { .. })
        })
    });
    let varying_injection = layout.global_inject.trade_tagged().iter().any(|rule| {
        matches!(rule.count_scope, TradeTaggedCountScope::AllTradeRooms)
            && sources
                .iter()
                .any(|&index| pool.entries[index].tags.contains(&rule.target_tag))
    });
    let varying = pool.entries.iter().enumerate().map(|(index, entry)| {
        sources.contains(&index) || varying_injection
            || crate::trade::gold_flow::reads_virtual_lines(&entry.buff_ids)
            || entry.compiled_atoms.iter().any(|compiled| {
                matches!(&compiled.atom.condition, Some(Condition::OperatorInBase { name } | Condition::OperatorInTrade { name }) if sources.iter().any(|&index| pool.entries[index].name == *name))
                    || matches!(compiled.atom.selector, Some(Selector::TaggedCountInBase { .. } | Selector::TaggedCountInTradeSum { .. }))
                    || (varying_global && matches!(compiled.atom.action, Action::StateConsume { .. } | Action::StateConsumeToEff { .. } | Action::AddLimitFromState { .. }))
            })
    }).collect();
    let mut selections = Vec::new();
    source_subsets(
        &sources,
        0,
        rooms
            .iter()
            .map(|room| room.operator_capacity())
            .sum::<usize>()
            .saturating_sub(required.len()),
        &mut Vec::new(),
        &mut selections,
    );
    let mut states = Vec::new();
    for selected in selections {
        let mut context = fixed.clone();
        for (room, members) in rooms.iter().zip(&room_members) {
            context.set_room(room.id.clone(), members.clone());
        }
        let mut remaining = selected.iter();
        for room in &rooms {
            let mut members = context.operators_in(&room.id).to_vec();
            while members.len() < room.operator_capacity() {
                let Some(&index) = remaining.next() else {
                    break;
                };
                let entry = &pool.entries[index];
                members.push(AssignedOperator::from_progress(&entry.name, entry.progress));
            }
            context.set_room(room.id.clone(), members);
        }
        context.sort_trade_gold_flow_operators(blueprint);
        let context_layout = crate::layout::resolve_base(
            blueprint,
            &context,
            Some(instances),
            Some(table),
            options.mood,
            durin_plan,
        )?
        .layout;
        let mut allowed = vec![true; pool.entries.len()];
        for &index in &sources {
            allowed[index] = selected.contains(&index);
        }
        let mut state_required = required.clone();
        state_required.extend(selected);
        states.push(TradeAllocationState {
            layout: Arc::new(context_layout),
            allowed,
            required: state_required,
        });
    }
    let allocation_rooms = rooms
        .iter()
        .enumerate()
        .map(|(room_index, room)| {
            Ok(TradeAllocationRoom {
                options: trade_room_options(
                    &layout,
                    blueprint.gold_manu_line_count(),
                    options,
                    trade_order_from_room(room)?,
                    room.level,
                ),
                order: trade_order_from_room(room)?,
                allowed: (0..pool.entries.len())
                    .map(|index| {
                        !required.contains(&index) || required_by_room[room_index].contains(&index)
                    })
                    .collect(),
                required: required_by_room[room_index].clone(),
            })
        })
        .collect::<Result<_>>()?;
    let mut forbidden_pairs: Vec<_> = plan
        .map_or(&[][..], |plan| plan.constraints.as_slice())
        .iter()
        .filter_map(|constraint| match constraint {
            SystemConstraint::ForbidSameRoom { a, b }
            | SystemConstraint::ForbidSameStation { a, b } => Some((index_of(a)?, index_of(b)?)),
            _ => None,
        })
        .collect();
    forbidden_pairs.extend(
        crate::trade::legality::GLOBAL_FORBIDDEN_TRADE_PAIRS
            .iter()
            .filter_map(|&(a, b)| Some((index_of(a)?, index_of(b)?))),
    );
    let request = TradeAllocationRequest {
        rooms: allocation_rooms,
        states,
        varying,
        forbidden_pairs,
    };
    let winner = search_trade_allocation(&pool, table, &request).map_err(|error| {
        if let (Error::NoCandidate(message), Some(plan)) = (&error, plan) {
            if let Some(anchor) = plan.anchors.iter().find(|anchor| {
                anchor.optional_trade_role.is_some() && required_names.contains(&anchor.operator)
            }) {
                return Error::OptionalTradeRoleInfeasible {
                    system_id: anchor.system_id.clone(),
                    role_id: anchor.optional_trade_role.clone().unwrap_or_default(),
                    operator: anchor.operator.clone(),
                    message: message.clone(),
                };
            }
            if plan.anchors.iter().any(|anchor| {
                anchor.system_id == "pozyomka_durin" && required_names.contains(&anchor.operator)
            }) {
                return Error::GoldFlowCoreInfeasible {
                    message: message.clone(),
                };
            }
        }
        error
    })?;
    for (room_index, (room, members)) in rooms.iter().zip(&winner.rooms).enumerate() {
        if members.is_empty() {
            continue;
        }
        let mut room_options = request.rooms[room_index].options.clone();
        room_options.layout = Arc::clone(&request.states[winner.state].layout);
        let (hit, _) = room_options
            .solve_trade_combo(
                &pool,
                table,
                request.rooms[room_index].order,
                members,
                None,
                None,
            )
            .ok_or_else(|| Error::msg("selected trade allocation could not be materialized"))?;
        commit_trade_room(assignment, &room.id, &hit, &pool, used)?;
    }
    stage.finish();
    Ok(true)
}

fn source_subsets(
    sources: &[usize],
    index: usize,
    remaining: usize,
    selected: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if index == sources.len() {
        out.push(selected.clone());
        return;
    }
    source_subsets(sources, index + 1, remaining, selected, out);
    if remaining > 0 {
        selected.push(sources[index]);
        source_subsets(sources, index + 1, remaining - 1, selected, out);
        selected.pop();
    }
}
