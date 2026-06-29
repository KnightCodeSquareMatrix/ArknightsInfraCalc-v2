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
use crate::pool::{
    expand_manufacture_candidate_pool, filter_general_manufacture_search_pool,
    filter_manufacture_pool, filter_standalone_exact, ManuPool, ManuPoolEntry,
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
        if has_qingliu && has_wendy && existing.len() < room.operator_capacity() {
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
    forbid_same_room: &[(String, String)],
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

    let required_slots = rooms
        .iter()
        .filter(|room| assignment.operators_in(&room.id).is_empty())
        .map(|room| room.operator_capacity())
        .sum();
    let candidate_pool = manufacture_candidate_pool_for_demand(pool, used, required_slots);

    for room in rooms {
        if assignment.operators_in(&room.id).len() >= room.operator_capacity() {
            continue;
        }
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => continue,
        };
        let opts = manu_options(layout, options, recipe, room.level);
        let existing = assignment.operators_in(&room.id);
        let hit = if existing.is_empty() {
            pick_manu_hit(&candidate_pool, table, opts.clone(), used, options.top_k)
                .or_else(|_| pick_manu_hit(pool, table, opts, used, options.top_k))
                .or_else(|_| {
                    pick_capacity_manu_hit(pool, table, layout, options, recipe, used, room.level)
                })
        } else {
            let anchor = existing[0].name.clone();
            // forbid-same-room：anchor 房禁止纳入指定干员（迷迭香 ≠ 清流/温蒂同房）。
            let forbidden = forbidden_teammates(&anchor, forbid_same_room);
            pick_manu_hit_with_anchor(pool, table, opts, used, options.top_k, &anchor, &forbidden)
        }
        .map_err(|e| Error::msg(format!("manufacture {}: {e}", room.id.0)))?;
        commit_manu_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

/// anchor 房按 forbid-same-room 约束收集禁止同房的队友名（双向匹配）。
fn forbidden_teammates(anchor: &str, forbid_same_room: &[(String, String)]) -> HashSet<String> {
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
    required_slots: usize,
) -> ManuPool {
    let full_sub = filter_general_manufacture_search_pool(&filter_manufacture_pool(pool, used));
    let Some(primary_sub) = filter_standalone_exact(&full_sub, FacilityKind::Factory) else {
        return full_sub;
    };
    if required_slots == 0 || primary_sub.entries.len() >= required_slots {
        return primary_sub;
    }
    let expanded = expand_manufacture_candidate_pool(&primary_sub, &full_sub);
    if expanded.entries.len() >= required_slots {
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
        return Err(Error::msg(format!(
            "manufacture {} invalid anchor count {}",
            room_id.0,
            anchors.len()
        )));
    }
    let mut seen = HashSet::new();
    for anchor in &anchors {
        if !seen.insert(anchor.name.clone()) {
            return Err(Error::msg(format!(
                "manufacture {} duplicate anchor {}",
                room_id.0, anchor.name
            )));
        }
        if used.contains(&anchor.name) {
            return Err(Error::msg(format!(
                "manufacture {} anchor {} already used",
                room_id.0, anchor.name
            )));
        }
    }

    let filler_need = capacity - anchors.len();
    let fillers: Vec<ManuPoolEntry> = pool
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
    if fillers.len() < filler_need {
        return Err(Error::msg(format!(
            "manufacture {} has {} ready fillers (need {filler_need})",
            room_id.0,
            fillers.len()
        )));
    }

    let mut combos = Vec::new();
    collect_manu_filler_combos(&fillers, filler_need, 0, &mut Vec::new(), &mut combos);
    let Some(hit) = combos
        .into_iter()
        .filter_map(|mut combo| {
            let mut entries = anchors.clone();
            entries.append(&mut combo);
            score_manu_entries(&entries, table, layout, options, recipe, room_level)
        })
        .max_by(|a, b| {
            a.composite_score
                .partial_cmp(&b.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.storage
                        .gold
                        .max(a.storage.battle_record)
                        .cmp(&b.storage.gold.max(b.storage.battle_record))
                })
                .then_with(|| b.names.cmp(&a.names))
        })
    else {
        return Err(Error::msg(format!(
            "manufacture {} produced no anchored hit",
            room_id.0
        )));
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
    let input = ManuRoomInput {
        level: room_level,
        operators: entries
            .iter()
            .map(ManuPoolEntry::to_manu_operator)
            .collect(),
        active_recipe: recipe,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
    };
    let result = crate::manufacture::solve_manufacture(&input, table).ok()?;
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
    Some(ManuSearchHit {
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
    let entries: Vec<_> = pool
        .entries
        .iter()
        .filter(|entry| !used.contains(&entry.name))
        .take(capacity)
        .cloned()
        .collect();
    if entries.len() < capacity {
        return Err(Error::msg(format!(
            "manufacture capacity fallback has {} ready operators (need {capacity})",
            entries.len(),
        )));
    }
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
    let capacity = search_opts.operator_capacity.clamp(1, 3);
    if pool.entries.len() < capacity {
        return Err(Error::msg(format!(
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
    room_level: u8,
) -> ManuSearchOptions {
    ManuSearchOptions {
        level: room_level,
        operator_capacity: station_operator_capacity(room_level),
        top_k: options.top_k,
        mood: options.mood,
        layout: Arc::new(layout.clone()),
        recipe_mode: ManuSearchRecipeMode::Single(recipe),
        ..ManuSearchOptions::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_teammates_matches_both_directions() {
        // forbid-same-room 名对双向匹配：迷迭香 anchor 房应排除清流/温蒂，反之亦然。
        let pairs = vec![
            ("迷迭香".to_string(), "清流".to_string()),
            ("迷迭香".to_string(), "温蒂".to_string()),
        ];
        let f = forbidden_teammates("迷迭香", &pairs);
        assert!(f.contains("清流") && f.contains("温蒂"));
        // 反向：以清流为 anchor 时应排除迷迭香。
        let f2 = forbidden_teammates("清流", &pairs);
        assert!(f2.contains("迷迭香"));
        // 无关 anchor 不受影响。
        assert!(forbidden_teammates("砾", &pairs).is_empty());
    }
}
