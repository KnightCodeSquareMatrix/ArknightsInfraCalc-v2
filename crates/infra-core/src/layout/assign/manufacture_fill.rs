use std::collections::HashSet;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{
    station_operator_capacity, BaseBlueprint, FacilityKind, RoomId, RoomProduct,
};
use crate::layout::context::LayoutContext;
use crate::manufacture::input::ManuRoomInput;
use crate::manufacture::input::ManuSearchRecipeMode;
use crate::operbox::OperBox;
use crate::pool::{
    filter_general_manufacture_search_pool, filter_manufacture_pool, ManuPool, ManuPoolEntry,
};
use crate::search::{
    manufacture_combo_allowed_for_shift, search_manufacture_triples, ManuSearchHit,
    ManuSearchOptions,
};
use crate::skill_table::SkillTable;
use crate::types::RecipeKind;

use super::commit::{commit_manu_room, manu_hit_names, pick_disjoint_from_report};
use super::AssignBaseOptions;

fn manu_recipe_fill_priority(recipe: RecipeKind) -> u8 {
    match recipe {
        RecipeKind::Gold => 0,
        RecipeKind::BattleRecord => 1,
        RecipeKind::Originium => 2,
        RecipeKind::All => 3,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assign_manufacture_lines(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &crate::instances::OperatorInstances,
    pool: &ManuPool,
    table: &SkillTable,
    _layout: &LayoutContext,
    options: &AssignBaseOptions,
    forbid_same_room: &[(String, String)],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let mut rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::Factory)
        .collect();
    rooms.sort_by_key(|r| match r.product.as_ref() {
        Some(RoomProduct::Factory { recipe }) => manu_recipe_fill_priority(*recipe),
        _ => 99,
    });

    let candidate_pool = manufacture_candidate_pool_for_demand(pool, used);

    for room in rooms {
        if assignment.operators_in(&room.id).len() >= room.operator_capacity() {
            continue;
        }
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => continue,
        };
        let current_layout = crate::layout::resolve_base(
            blueprint,
            assignment,
            Some(instances),
            Some(table),
            options.mood,
            Some(operbox.durin_dorm_planning_count(instances)),
        )?
        .layout;
        let opts = manu_options(&current_layout, options, recipe, room.level);
        let existing = assignment.operators_in(&room.id);
        if existing.len() > 1 {
            let anchors: Vec<_> = existing
                .iter()
                .filter_map(|operator| pool.entry(&operator.name).cloned())
                .collect();
            if anchors.len() != existing.len() {
                assignment.construction_warnings.push(format!(
                    "manufacture {}: committed anchor missing from candidate pool",
                    room.id.0
                ));
                continue;
            }
            for anchor in &anchors {
                used.remove(&anchor.name);
            }
            assign_manu_room_with_anchors(
                assignment,
                &room.id,
                anchors,
                pool,
                table,
                &current_layout,
                options,
                recipe,
                room.level,
                used,
                &[],
            )?;
            continue;
        }
        let hit = if existing.is_empty() {
            pick_manu_hit(&candidate_pool, table, opts.clone(), used, options.top_k)
                .or_else(|_| pick_manu_hit(pool, table, opts, used, options.top_k))
                .or_else(|_| {
                    pick_capacity_manu_hit(
                        pool,
                        table,
                        &current_layout,
                        options,
                        recipe,
                        used,
                        room.level,
                    )
                })
        } else {
            let anchor = existing[0].name.clone();
            // forbid-same-room：anchor 房禁止纳入指定干员（迷迭香 ≠ 清流/温蒂同房）。
            let forbidden = forbidden_teammates(&anchor, forbid_same_room);
            pick_manu_hit_with_anchor(pool, table, opts, used, options.top_k, &anchor, &forbidden)
        };
        let hit = match hit {
            Ok(hit) => hit,
            Err(Error::NoCandidate(message)) => {
                assignment
                    .construction_warnings
                    .push(format!("manufacture {}: {message}", room.id.0));
                continue;
            }
            Err(error) => return Err(error),
        };
        commit_manu_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

pub(super) fn refresh_manufacture_efficiency_snapshots(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    instances: &crate::instances::OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
) -> Result<()> {
    let final_resolved = crate::layout::resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_dorm_planning,
    )?;
    for room in final_resolved.manu_rooms {
        if assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let input = ManuRoomInput {
            level: room.level,
            operators: room.operators,
            active_recipe: room.recipe,
            mood: options.mood,
            layout: Arc::new(room.layout),
        };
        if !crate::search::manufacture_peers_compatible(&input.operators, table) {
            return Err(Error::msg(format!(
                "manufacture {} has incompatible peer-efficiency operators",
                room.id.0
            )));
        }
        let solved =
            crate::manufacture::solve_manufacture_with_shift(&input, table, options.shift_hours)?;
        let operators = assignment.operators_in(&room.id).to_vec();
        assignment.set_room_with_efficiency(
            room.id,
            operators,
            Some(crate::layout::RoomEfficiencySnapshot {
                manufacture_final_efficiency: solved.final_efficiency,
                manufacture_occupancy_efficiency: solved.occupancy_efficiency,
                manufacture_global_efficiency: solved.global_efficiency,
                manufacture_skill_efficiency: solved.skill_efficiency,
                manufacture_storage_limit: solved.storage_limit,
                ..Default::default()
            }),
        );
    }
    Ok(())
}

/// anchor 房按 forbid-same-room 约束收集禁止同房的队友名（双向匹配）。
pub(super) fn forbidden_teammates(
    anchor: &str,
    forbid_same_room: &[(String, String)],
) -> HashSet<String> {
    let mut out = HashSet::new();
    for (a, b) in forbid_same_room {
        if a == anchor {
            out.insert(b.clone());
        } else if b == anchor {
            out.insert(a.clone());
        }
    }
    out
}

pub(super) fn manufacture_candidate_pool_for_demand(
    pool: &ManuPool,
    used: &HashSet<String>,
) -> ManuPool {
    filter_general_manufacture_search_pool(&filter_manufacture_pool(pool, used))
}

pub(super) fn pick_manu_hit(
    pool: &ManuPool,
    table: &SkillTable,
    search_opts: ManuSearchOptions,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<ManuSearchHit> {
    let sub = filter_manufacture_pool(pool, used);
    search_manu_hit_in_pool(&sub, table, search_opts, used, top_k, "manufacture pool")
}

pub(super) fn pick_manu_hit_with_anchor(
    pool: &ManuPool,
    table: &SkillTable,
    search_opts: ManuSearchOptions,
    used: &HashSet<String>,
    top_k: usize,
    anchor: &str,
    forbidden: &HashSet<String>,
) -> Result<ManuSearchHit> {
    let mut used_for_filter = used.clone();
    used_for_filter.remove(anchor);
    // forbid-same-room：把禁止同房的干员当作「已占用」滤出候选池，搜索自然不会选中。
    for name in forbidden {
        used_for_filter.insert(name.clone());
    }
    let sub = filter_manufacture_pool(pool, &used_for_filter);
    let mut search_opts = search_opts;
    search_opts.must_include_name = Some(anchor.to_string());
    search_manu_hit_in_pool(
        &sub,
        table,
        search_opts,
        &used_for_filter,
        top_k,
        "anchor pool",
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn refill_manu_room_with_core(
    blueprint: &crate::layout::BaseBlueprint,
    room_id: &RoomId,
    core: &str,
    pool: &ManuPool,
    table: &SkillTable,
    instances: &crate::instances::OperatorInstances,
    options: &AssignBaseOptions,
    durin_dorm_planning: Option<u8>,
    forbidden_names: &[String],
    assignment: &mut BaseAssignment,
) -> Result<()> {
    let room = blueprint
        .room(room_id)
        .ok_or_else(|| Error::msg(format!("Fiammetta factory {} disappeared", room_id.0)))?;
    let crate::layout::RoomProduct::Factory { recipe } = room
        .product
        .as_ref()
        .ok_or_else(|| Error::msg("Fiammetta manufacture target lacks a recipe"))?
    else {
        return Err(Error::msg("Fiammetta manufacture target is not a factory"));
    };
    assignment.remove_operators_where(|_, operator| operator.name == core);
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
    let hit = pick_manu_hit_with_anchor(
        pool,
        table,
        ManuSearchOptions {
            level: room.level,
            operator_capacity: room.operator_capacity(),
            top_k: options.top_k,
            mood: options.mood,
            shift_hours: Some(options.shift_hours),
            layout: std::sync::Arc::new(layout),
            recipe_mode: crate::manufacture::ManuSearchRecipeMode::Single(*recipe),
            must_include_name: Some(core.to_string()),
            full_pool: true,
            bake_mode: crate::bake::BakeMode::Disabled,
        },
        &used,
        options.top_k,
        core,
        &forbidden_names.iter().cloned().collect(),
    )?;
    let operators = hit
        .names
        .iter()
        .map(|name| {
            pool.entry(name)
                .map(|entry| AssignedOperator::from_progress(name, entry.progress))
                .ok_or_else(|| Error::msg(format!("Fiammetta factory candidate {name} missing")))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), operators);
    Ok(())
}

pub(crate) fn assign_manu_room_with_anchors(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    anchors: Vec<ManuPoolEntry>,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    recipe: RecipeKind,
    room_level: u8,
    used: &mut HashSet<String>,
    forbidden_filler_buff_ids: &[&str],
) -> Result<()> {
    let capacity = station_operator_capacity(room_level);
    if anchors.is_empty() || anchors.len() > capacity {
        assignment.construction_warnings.push(format!(
            "manufacture {} invalid anchor count {}",
            room_id.0,
            anchors.len()
        ));
        return Ok(());
    }
    let mut seen = HashSet::new();
    for anchor in &anchors {
        if !seen.insert(anchor.name.clone()) {
            assignment.construction_warnings.push(format!(
                "manufacture {} duplicate anchor {}",
                room_id.0, anchor.name
            ));
            return Ok(());
        }
        if used.contains(&anchor.name) {
            assignment.construction_warnings.push(format!(
                "manufacture {} anchor {} already used",
                room_id.0, anchor.name
            ));
            return Ok(());
        }
    }

    let requested_filler_count = capacity - anchors.len();
    let general_pool = filter_general_manufacture_search_pool(pool);
    let fillers: Vec<ManuPoolEntry> = general_pool
        .entries
        .iter()
        .filter(|entry| !seen.contains(&entry.name) && !used.contains(&entry.name))
        .filter(|entry| {
            !entry
                .buff_ids
                .iter()
                .any(|buff_id| forbidden_filler_buff_ids.contains(&buff_id.as_str()))
        })
        .cloned()
        .collect();
    let max_filler_count = requested_filler_count.min(fillers.len());
    let allow_preclaim_shortfall = options.preclaimed_factory_room.as_ref() == Some(room_id);
    let mut best_hit = None;
    let filler_counts: Vec<usize> = if allow_preclaim_shortfall {
        (0..=max_filler_count).rev().collect()
    } else {
        vec![max_filler_count]
    };
    for filler_count in filler_counts {
        let mut combos = Vec::new();
        collect_manu_filler_combos(&fillers, filler_count, 0, &mut Vec::new(), &mut combos);
        best_hit = combos
            .into_iter()
            .filter_map(|mut combo| {
                let mut entries = anchors.clone();
                entries.append(&mut combo);
                score_manu_entries(&entries, table, layout, options, recipe, room_level)
            })
            .max_by(|a, b| {
                a.final_efficiency
                    .cmp(&b.final_efficiency)
                    .then_with(|| {
                        a.storage
                            .gold
                            .max(a.storage.battle_record)
                            .cmp(&b.storage.gold.max(b.storage.battle_record))
                    })
                    .then_with(|| b.names.cmp(&a.names))
            });
        if best_hit.is_some() {
            break;
        }
    }
    let Some(hit) = best_hit else {
        return Err(if allow_preclaim_shortfall {
            Error::ManufactureAnchoredCompletionInfeasible {
                room_id: room_id.0.clone(),
                anchor_count: anchors.len(),
            }
        } else {
            assignment.construction_warnings.push(format!(
                "manufacture {} produced no anchored hit",
                room_id.0
            ));
            used.extend(
                assignment
                    .operators_in(room_id)
                    .iter()
                    .map(|op| op.name.clone()),
            );
            return Ok(());
        });
    };
    let mut commit_pool = pool.clone();
    for anchor in anchors {
        if commit_pool.entry(&anchor.name).is_none() {
            commit_pool.entries.push(anchor);
        }
    }
    commit_manu_room(assignment, room_id, &hit, &commit_pool, used)
}

fn collect_manu_filler_combos(
    entries: &[ManuPoolEntry],
    need: usize,
    start: usize,
    current: &mut Vec<ManuPoolEntry>,
    out: &mut Vec<Vec<ManuPoolEntry>>,
) {
    if current.len() == need {
        out.push(current.clone());
        return;
    }
    if need == 0 {
        out.push(Vec::new());
        return;
    }
    for idx in start..entries.len() {
        if current.len() + entries.len().saturating_sub(idx) < need {
            break;
        }
        current.push(entries[idx].clone());
        collect_manu_filler_combos(entries, need, idx + 1, current, out);
        current.pop();
    }
}

fn score_manu_entries(
    entries: &[ManuPoolEntry],
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    recipe: RecipeKind,
    room_level: u8,
) -> Option<ManuSearchHit> {
    let operators = entries
        .iter()
        .map(ManuPoolEntry::to_manu_operator)
        .collect::<Vec<_>>();
    if !manufacture_combo_allowed_for_shift(&operators, table, Some(options.shift_hours)) {
        return None;
    }
    let input = ManuRoomInput {
        level: room_level,
        operators,
        active_recipe: recipe,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
    };
    let result =
        crate::manufacture::solve_manufacture_with_shift(&input, table, options.shift_hours)
            .ok()?;
    let names = entries.iter().map(|entry| entry.name.clone()).collect();
    let mut per_station = crate::manufacture::ManuProdBreakdown::default();
    let mut storage = crate::manufacture::ManuStorageBreakdown::default();
    let recipe_label = match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.final_efficiency;
            storage.gold = result.storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.final_efficiency;
            storage.battle_record = result.storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = result.final_efficiency;
            storage.originium = result.storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };
    Some(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency: result.final_efficiency,
        per_station,
        storage,
        breakdown: crate::search::ManuEfficiencyBreakdown {
            base_efficiency: result.base_efficiency,
            occupancy_efficiency: result.occupancy_efficiency,
            skill_efficiency: result.skill_efficiency,
            global_efficiency: result.global_efficiency,
            final_efficiency: result.final_efficiency,
            unit_output_per_day: result.unit_output_per_day,
            output_per_day: result.output_per_day,
            storage_limit: result.storage_limit,
            recipe: recipe_label.to_string(),
        },
    })
}

pub(super) fn pick_capacity_manu_hit(
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    recipe: RecipeKind,
    used: &HashSet<String>,
    room_level: u8,
) -> Result<ManuSearchHit> {
    let capacity = station_operator_capacity(room_level);
    let general_pool = filter_general_manufacture_search_pool(pool);
    let entries: Vec<_> = general_pool
        .entries
        .iter()
        .filter(|entry| !used.contains(&entry.name))
        .filter(|entry| {
            manufacture_combo_allowed_for_shift(
                &[entry.to_manu_operator()],
                table,
                Some(options.shift_hours),
            )
        })
        .take(capacity)
        .cloned()
        .collect();
    let operators = entries
        .iter()
        .map(|entry| entry.to_manu_operator())
        .collect();
    let input = crate::manufacture::ManuRoomInput {
        level: room_level,
        operators,
        active_recipe: recipe,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
    };
    let result =
        crate::manufacture::solve_manufacture_with_shift(&input, table, options.shift_hours)?;
    let names = entries.iter().map(|entry| entry.name.clone()).collect();
    let mut per_station = crate::manufacture::ManuProdBreakdown::default();
    let mut storage = crate::manufacture::ManuStorageBreakdown::default();
    let recipe_label = match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.final_efficiency;
            storage.gold = result.storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.final_efficiency;
            storage.battle_record = result.storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = result.final_efficiency;
            storage.originium = result.storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };

    Ok(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency: result.final_efficiency,
        per_station,
        storage,
        breakdown: crate::search::ManuEfficiencyBreakdown {
            base_efficiency: result.base_efficiency,
            occupancy_efficiency: result.occupancy_efficiency,
            skill_efficiency: result.skill_efficiency,
            global_efficiency: result.global_efficiency,
            final_efficiency: result.final_efficiency,
            unit_output_per_day: result.unit_output_per_day,
            output_per_day: result.output_per_day,
            storage_limit: result.storage_limit,
            recipe: recipe_label.to_string(),
        },
    })
}

fn search_manu_hit_in_pool(
    pool: &ManuPool,
    table: &SkillTable,
    search_opts: ManuSearchOptions,
    used: &HashSet<String>,
    top_k: usize,
    label: &str,
) -> Result<ManuSearchHit> {
    let capacity = search_opts.operator_capacity.clamp(1, 3);
    if pool.entries.len() < capacity {
        return Err(Error::no_candidate(format!(
            "{label} has {} ready operators (need {capacity})",
            pool.entries.len(),
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;
    let report = search_manufacture_triples(pool, table, &opts)?;
    if manu_hit_names(&report.best).is_empty()
        && report.top.iter().all(|hit| manu_hit_names(hit).is_empty())
    {
        return Err(Error::no_candidate(format!(
            "{label} produced no manufacture triple"
        )));
    }
    pick_disjoint_from_report(
        report.best,
        report.top,
        manu_hit_names,
        used,
        "no disjoint manufacture triple",
    )
}

pub(super) fn manu_options(
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    recipe: RecipeKind,
    room_level: u8,
) -> ManuSearchOptions {
    ManuSearchOptions {
        level: room_level,
        operator_capacity: station_operator_capacity(room_level),
        top_k: options.top_k,
        mood: options.mood,
        shift_hours: Some(options.shift_hours),
        layout: Arc::new(layout.clone()),
        recipe_mode: ManuSearchRecipeMode::Single(recipe),
        full_pool: true,
        ..ManuSearchOptions::default()
    }
}
