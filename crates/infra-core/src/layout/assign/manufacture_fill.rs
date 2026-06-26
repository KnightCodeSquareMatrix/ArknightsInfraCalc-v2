use std::collections::HashSet;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
use crate::layout::context::LayoutContext;
use crate::manufacture::input::ManuSearchRecipeMode;
use crate::pool::{
    expand_manufacture_candidate_pool, filter_general_manufacture_search_pool,
    filter_manufacture_pool, filter_standalone_exact, ManuPool,
};
use crate::search::{search_manufacture_triples, ManuSearchHit, ManuSearchOptions};
use crate::skill_table::SkillTable;
use crate::types::RecipeKind;

use super::commit::{
    commit_anchor_room, commit_manu_room, manu_hit_names, names_disjoint_except,
    pick_disjoint_from_report,
};
use super::AssignBaseOptions;

const GONGSUN_GOLD_MANU_ANCHORS: [&str; 2] = ["清流", "温蒂"];
const GONGSUN_GOLD_MANU_THIRD_CHOICES: [&str; 2] = ["森蚺", "冬时"];
pub(super) const QINGLIU_RENEWABLE_ENERGY_BUFF: &str = "manu_prod_spd&trade[000]";
pub(super) const WENDY_BIONIC_SEADRAGON_BUFF: &str = "manu_prod_spd&power[020]";

fn manu_recipe_fill_priority(recipe: RecipeKind) -> u8 {
    match recipe {
        RecipeKind::Gold => 0,
        RecipeKind::BattleRecord => 1,
        RecipeKind::Originium => 2,
        RecipeKind::All => 3,
    }
}

fn try_commit_fixed_manu_team(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    team: &[&str],
    pool: &ManuPool,
    used: &mut HashSet<String>,
    anchors: &[String],
) -> Result<bool> {
    if !anchors.iter().all(|a| team.contains(&a.as_str())) {
        return Ok(false);
    }
    let mut used_wo = used.clone();
    for a in anchors {
        used_wo.remove(a.as_str());
    }
    let names: Vec<String> = team.iter().map(|s| s.to_string()).collect();
    if !names.iter().all(|n| pool.entry(n).is_some()) {
        return Ok(false);
    }
    if !names_disjoint_except(&names, &used_wo) {
        return Ok(false);
    }
    commit_anchor_room(
        assignment,
        room_id,
        &names,
        |name| {
            pool.entry(name)
                .map(|e| AssignedOperator::from_progress(name, e.progress))
                .unwrap_or_else(|| AssignedOperator::new(name, 0))
        },
        used,
        anchors,
        "manufacture fixed team",
    )?;
    Ok(true)
}

pub(super) fn try_assign_gongsun_gold_manu_team(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    pool: &ManuPool,
    used: &mut HashSet<String>,
) -> Result<()> {
    if !gongsun_gold_manu_anchors_ready(pool) {
        return Ok(());
    }

    // 优先：已有自动化组落位（清流+温蒂）的金房间，补齐第三人森蚺
    if let Some(room) = blueprint.rooms.iter().find(|r| {
        r.kind == FacilityKind::Factory
            && matches!(
                r.product.as_ref(),
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold
                })
            )
    }) {
        let existing = assignment.operators_in(&room.id);
        let has_qingliu = existing.iter().any(|o| o.name == "清流");
        let has_wendy = existing.iter().any(|o| o.name == "温蒂");
        if has_qingliu && has_wendy && existing.len() < 3 {
            for candidate in GONGSUN_GOLD_MANU_THIRD_CHOICES {
                let Some(entry) = pool.entry(candidate) else {
                    continue;
                };
                if used.contains(candidate) {
                    continue;
                }
                let mut ops: Vec<AssignedOperator> = existing.to_vec();
                ops.push(AssignedOperator::from_progress(candidate, entry.progress));
                assignment.set_room(room.id.clone(), ops);
                used.insert(candidate.to_string());
                return Ok(());
            }
        }
    }

    // 退而求其次：找空金房间全部落位（旧逻辑）
    let Some(room) = blueprint.rooms.iter().find(|r| {
        r.kind == FacilityKind::Factory
            && matches!(
                r.product.as_ref(),
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold
                })
            )
            && assignment.operators_in(&r.id).is_empty()
    }) else {
        return Ok(());
    };
    for candidate in GONGSUN_GOLD_MANU_THIRD_CHOICES {
        let team = [
            GONGSUN_GOLD_MANU_ANCHORS[0],
            GONGSUN_GOLD_MANU_ANCHORS[1],
            candidate,
        ];
        if try_commit_fixed_manu_team(assignment, &room.id, &team, pool, used, &[])? {
            break;
        }
    }
    Ok(())
}

pub(super) fn gongsun_gold_manu_anchors_ready(pool: &ManuPool) -> bool {
    pool.entry("清流").is_some_and(|entry| {
        entry
            .buff_ids
            .iter()
            .any(|id| id == QINGLIU_RENEWABLE_ENERGY_BUFF)
    }) && pool.entry("温蒂").is_some_and(|entry| {
        entry
            .buff_ids
            .iter()
            .any(|id| id == WENDY_BIONIC_SEADRAGON_BUFF)
    })
}

pub(super) fn assign_manufacture_lines(
    blueprint: &BaseBlueprint,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    try_assign_gongsun_gold_manu_team(blueprint, assignment, pool, used)?;

    let mut rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::Factory)
        .collect();
    rooms.sort_by_key(|r| match r.product.as_ref() {
        Some(RoomProduct::Factory { recipe }) => manu_recipe_fill_priority(*recipe),
        _ => 99,
    });

    let room_count = rooms
        .iter()
        .filter(|room| assignment.operators_in(&room.id).is_empty())
        .count();
    let candidate_pool = manufacture_candidate_pool_for_demand(pool, used, room_count);

    for room in rooms {
        if assignment.operators_in(&room.id).len() >= 3 {
            continue;
        }
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => continue,
        };
        let opts = manu_options(layout, options, recipe);
        let existing = assignment.operators_in(&room.id);
        let hit = if existing.is_empty() {
            pick_manu_hit(&candidate_pool, table, opts.clone(), used, options.top_k)
                .or_else(|_| pick_manu_hit(pool, table, opts, used, options.top_k))
                .or_else(|_| pick_capacity_manu_hit(pool, table, layout, options, recipe, used))
        } else {
            let anchor = existing[0].name.clone();
            pick_manu_hit_with_anchor(pool, table, opts, used, options.top_k, &anchor)
        }
        .map_err(|e| Error::msg(format!("manufacture {}: {e}", room.id.0)))?;
        commit_manu_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

pub(super) fn manufacture_candidate_pool_for_demand(
    pool: &ManuPool,
    used: &HashSet<String>,
    room_count: usize,
) -> ManuPool {
    let full_sub = filter_general_manufacture_search_pool(&filter_manufacture_pool(pool, used));
    let Some(primary_sub) = filter_standalone_exact(&full_sub, FacilityKind::Factory) else {
        return full_sub;
    };
    let required = room_count.saturating_mul(3);
    if required == 0 || primary_sub.entries.len() >= required {
        return primary_sub;
    }
    let expanded = expand_manufacture_candidate_pool(&primary_sub, &full_sub);
    if expanded.entries.len() >= required {
        expanded
    } else {
        full_sub
    }
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
) -> Result<ManuSearchHit> {
    let mut used_for_filter = used.clone();
    used_for_filter.remove(anchor);
    let sub = filter_manufacture_pool(pool, &used_for_filter);
    let mut search_opts = search_opts;
    search_opts.must_include_name = Some(anchor.to_string());
    search_manu_hit_in_pool(&sub, table, search_opts, &used_for_filter, top_k, "anchor pool")
}

pub(super) fn pick_capacity_manu_hit(
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    recipe: RecipeKind,
    used: &HashSet<String>,
) -> Result<ManuSearchHit> {
    let entries: Vec<_> = pool
        .entries
        .iter()
        .filter(|entry| !used.contains(&entry.name))
        .take(3)
        .cloned()
        .collect();
    if entries.len() < 3 {
        return Err(Error::msg(format!(
            "manufacture capacity fallback has {} ready operators (need 3)",
            entries.len()
        )));
    }
    let operators = entries
        .iter()
        .map(|entry| entry.to_manu_operator())
        .collect();
    let input = crate::manufacture::ManuRoomInput {
        level: 3,
        operators,
        active_recipe: recipe,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
    };
    let result = crate::manufacture::solve_manufacture(&input, table)?;
    let names = entries.iter().map(|entry| entry.name.clone()).collect();
    let mut per_station = crate::manufacture::ManuProdBreakdown::default();
    let mut storage = crate::manufacture::ManuStorageBreakdown::default();
    let recipe_label = match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.prod_total;
            storage.gold = result.storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.prod_total;
            storage.battle_record = result.storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = result.prod_total;
            storage.originium = result.storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };

    Ok(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        composite_score: result.prod_total,
        per_station,
        storage,
        breakdown: crate::search::ManuScoreBreakdown {
            prod_base: result.prod_base,
            prod_skill: result.prod_skill,
            prod_global: result.prod_total - result.prod_base - result.prod_skill,
            prod_total: result.prod_total,
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
    if pool.entries.len() < 3 {
        return Err(Error::msg(format!(
            "{label} has {} ready operators (need 3)",
            pool.entries.len()
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;
    let report = search_manufacture_triples(pool, table, &opts)?;
    if manu_hit_names(&report.best).is_empty()
        && report.top.iter().all(|hit| manu_hit_names(hit).is_empty())
    {
        return Err(Error::msg(format!(
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
) -> ManuSearchOptions {
    ManuSearchOptions {
        top_k: options.top_k,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
        recipe_mode: ManuSearchRecipeMode::Single(recipe),
        ..ManuSearchOptions::default()
    }
}
