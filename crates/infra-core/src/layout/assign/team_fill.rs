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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assignment::AssignedOperator;
    use crate::layout::blueprint::{BlueprintScenario, FacilityKind, RoomBlueprint};
    use crate::operbox::{OperBox, OperBoxEntry};
    use crate::roster::Roster;
    use crate::trade::TradeOrderKind;

    fn two_level_one_trade_blueprint() -> BaseBlueprint {
        BaseBlueprint {
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
        }
    }

    #[test]
    fn gamma_trade_search_sees_coexist_and_earlier_partial_trade_workforce() {
        let blueprint = two_level_one_trade_blueprint();
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();

        let bellone_pool = crate::pool::build_trade_pool(
            &Roster::from_elite_map([("贝洛内".to_string(), 2)].into_iter().collect()),
            &instances,
            &table,
        )
        .unwrap();
        let mut coexist = BaseAssignment::default();
        coexist.set_room("trade_a", vec![AssignedOperator::new("伺夜", 2)]);
        let mut from_coexist = BaseAssignment::default();
        assign_team_trade_meta_rooms(
            &blueprint,
            &instances,
            &coexist,
            None,
            &bellone_pool,
            &table,
            &AssignBaseOptions::default(),
            &[RoomId::from("trade_b")],
            &mut from_coexist,
            &mut HashSet::from(["伺夜".to_string()]),
        )
        .unwrap();
        assert_eq!(
            from_coexist
                .efficiency_in(&RoomId::from("trade_b"))
                .unwrap()
                .trade_skill_efficiency,
            crate::Efficiency::from_decimal(0.400)
        );

        let both_pool = crate::pool::build_trade_pool(
            &Roster::from_elite_map(
                [("伺夜", 2), ("贝洛内", 2)]
                    .into_iter()
                    .map(|(name, elite)| (name.to_string(), elite))
                    .collect(),
            ),
            &instances,
            &table,
        )
        .unwrap();
        let mut partial = BaseAssignment::default();
        assign_team_trade_meta_rooms(
            &blueprint,
            &instances,
            &BaseAssignment::default(),
            None,
            &both_pool,
            &table,
            &AssignBaseOptions::default(),
            &[RoomId::from("trade_a"), RoomId::from("trade_b")],
            &mut partial,
            &mut HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            partial.operators_in(&RoomId::from("trade_a"))[0].name,
            "伺夜"
        );
        assert_eq!(
            partial.operators_in(&RoomId::from("trade_b"))[0].name,
            "贝洛内"
        );
        assert_eq!(
            partial
                .efficiency_in(&RoomId::from("trade_b"))
                .unwrap()
                .trade_skill_efficiency,
            crate::Efficiency::from_decimal(0.400)
        );
    }

    #[test]
    fn gamma_manufacture_search_consumes_coexisting_rhine_workforce() {
        let mut blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let manu_id = blueprint
            .rooms
            .iter_mut()
            .find(|room| room.kind == crate::FacilityKind::Factory)
            .map(|room| {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold,
                });
                room.id.clone()
            })
            .unwrap();
        let dorm_id = blueprint
            .rooms
            .iter()
            .find(|room| room.kind == crate::FacilityKind::Dormitory)
            .unwrap()
            .id
            .clone();
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(
            [("娜斯提", 6), ("史都华德", 3), ("杰西卡", 4)]
                .into_iter()
                .enumerate()
                .map(|(index, (name, rarity))| OperBoxEntry {
                    id: format!("gamma_{index}"),
                    name: name.into(),
                    elite: if rarity == 3 { 1 } else { 2 },
                    level: 1,
                    own: true,
                    potential: 1,
                    rarity,
                })
                .collect(),
        )
        .unwrap();
        let pool = crate::pool::build_manufacture_pool(
            &operbox.manufacture_roster(&instances),
            &instances,
            &table,
        )
        .unwrap();
        let run = |coexist: &BaseAssignment, used: HashSet<String>| {
            let mut assignment = BaseAssignment::default();
            let mut used = used;
            assign_team_manu_rooms(
                &blueprint,
                &instances,
                coexist,
                Some(2),
                &pool,
                &table,
                &LayoutContext::search_baseline(),
                &AssignBaseOptions::default(),
                std::slice::from_ref(&manu_id),
                &HashMap::new(),
                &[],
                &mut assignment,
                &mut used,
            )
            .unwrap();
            assignment
        };
        let baseline = run(&BaseAssignment::default(), HashSet::new());
        let mut coexist = BaseAssignment::default();
        coexist.set_room(
            dorm_id,
            ["多萝西", "星源"]
                .into_iter()
                .map(|name| AssignedOperator::new(name, 2))
                .collect(),
        );
        let enhanced = run(
            &coexist,
            HashSet::from(["多萝西".to_string(), "星源".to_string()]),
        );
        assert_eq!(
            baseline.operators_in(&manu_id),
            enhanced.operators_in(&manu_id)
        );
        let base_eff = baseline
            .efficiency_in(&manu_id)
            .unwrap()
            .manufacture_final_efficiency;
        let enhanced_eff = enhanced
            .efficiency_in(&manu_id)
            .unwrap()
            .manufacture_final_efficiency;
        assert_eq!(
            enhanced_eff - base_eff,
            crate::efficiency::Efficiency::from_percent_points(6.0)
        );

        let mut merged = coexist.clone();
        merged.set_room_assignment(enhanced.room_assignment(&manu_id).unwrap().clone());
        let resolved = crate::layout::resolve_base(
            &blueprint,
            &merged,
            Some(&instances),
            Some(&table),
            24.0,
            Some(2),
        )
        .unwrap();
        let room = resolved
            .manu_rooms
            .iter()
            .find(|room| room.id == manu_id)
            .unwrap();
        let solved = crate::manufacture::solve_manufacture(
            &crate::manufacture::ManuRoomInput {
                level: room.level,
                operators: room.operators.clone(),
                active_recipe: room.recipe,
                mood: 24.0,
                layout: std::sync::Arc::new(room.layout.clone()),
            },
            &table,
        )
        .unwrap();
        assert_eq!(enhanced_eff, solved.final_efficiency);
    }

    #[test]
    fn gamma_manufacture_search_consumes_earlier_room_in_current_partial_assignment() {
        let mut blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let factory_ids: Vec<_> = blueprint
            .rooms
            .iter_mut()
            .filter(|room| room.kind == crate::FacilityKind::Factory)
            .take(2)
            .map(|room| {
                room.product = Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold,
                });
                room.id.clone()
            })
            .collect();
        let first = &factory_ids[0];
        let second = &factory_ids[1];
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();

        for name in ["多萝西", "星源"] {
            assert!(instances
                .tags_for(name, crate::tier::PromotionTier::TierUp)
                .iter()
                .any(|tag| tag == crate::layout::TAG_RHINE));
        }
        for name in ["泡普卡", "卡达", "香草"] {
            assert!(!instances
                .tags_for(name, crate::tier::PromotionTier::TierUp)
                .iter()
                .any(|tag| tag == crate::layout::TAG_RHINE));
        }

        let operbox = OperBox::from_entries(
            [("娜斯提", 6), ("史都华德", 3), ("杰西卡", 4)]
                .into_iter()
                .enumerate()
                .map(|(index, (name, rarity))| OperBoxEntry {
                    id: format!("gamma_partial_{index}"),
                    name: name.into(),
                    elite: if rarity == 3 { 1 } else { 2 },
                    level: 1,
                    own: true,
                    potential: 1,
                    rarity,
                })
                .collect(),
        )
        .unwrap();
        let pool = crate::pool::build_manufacture_pool(
            &operbox.manufacture_roster(&instances),
            &instances,
            &table,
        )
        .unwrap();
        let pool_names: HashSet<_> = pool
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(pool_names, HashSet::from(["娜斯提", "史都华德", "杰西卡"]));

        let coexist = BaseAssignment::default();
        let run = |first_room_names: [&str; 3]| {
            let mut assignment = BaseAssignment::default();
            assignment.set_room(
                first.clone(),
                first_room_names
                    .into_iter()
                    .map(|name| AssignedOperator::new(name, 2))
                    .collect(),
            );
            let mut used: HashSet<String> =
                first_room_names.into_iter().map(str::to_string).collect();
            assert_eq!(used, assignment.operator_names());
            assign_team_manu_rooms(
                &blueprint,
                &instances,
                &coexist,
                Some(2),
                &pool,
                &table,
                &LayoutContext::search_baseline(),
                &AssignBaseOptions::default(),
                &[first.clone(), second.clone()],
                &HashMap::new(),
                &[],
                &mut assignment,
                &mut used,
            )
            .unwrap();
            assignment
        };

        let baseline = run(["泡普卡", "卡达", "香草"]);
        let enhanced = run(["多萝西", "星源", "香草"]);
        assert_eq!(
            baseline
                .operators_in(first)
                .iter()
                .map(|operator| operator.name.as_str())
                .collect::<Vec<_>>(),
            vec!["泡普卡", "卡达", "香草"]
        );
        assert_eq!(
            enhanced
                .operators_in(first)
                .iter()
                .map(|operator| operator.name.as_str())
                .collect::<Vec<_>>(),
            vec!["多萝西", "星源", "香草"]
        );
        assert_eq!(baseline.operators_in(second), enhanced.operators_in(second));

        let baseline_eff = baseline
            .efficiency_in(second)
            .unwrap()
            .manufacture_final_efficiency;
        let enhanced_eff = enhanced
            .efficiency_in(second)
            .unwrap()
            .manufacture_final_efficiency;
        assert_eq!(
            enhanced_eff - baseline_eff,
            crate::efficiency::Efficiency::from_percent_points(6.0)
        );

        for (assignment, searched_efficiency) in
            [(&baseline, baseline_eff), (&enhanced, enhanced_eff)]
        {
            let mut merged = coexist.clone();
            for room in assignment.iter_rooms() {
                merged.set_room_assignment(room.clone());
            }
            let resolved = crate::layout::resolve_base(
                &blueprint,
                &merged,
                Some(&instances),
                Some(&table),
                24.0,
                Some(2),
            )
            .unwrap();
            let room = resolved
                .manu_rooms
                .iter()
                .find(|room| room.id == *second)
                .unwrap();
            let solved = crate::manufacture::solve_manufacture(
                &crate::manufacture::ManuRoomInput {
                    level: room.level,
                    operators: room.operators.clone(),
                    active_recipe: room.recipe,
                    mood: 24.0,
                    layout: std::sync::Arc::new(room.layout.clone()),
                },
                &table,
            )
            .unwrap();
            assert_eq!(searched_efficiency, solved.final_efficiency);
        }
    }
}
