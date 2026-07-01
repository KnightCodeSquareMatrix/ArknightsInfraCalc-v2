use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

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
const AUTOMATION_SYSTEM_ID: &str = "automation_group";
const SYSTEM_BAKED_SOURCE: &str = "system-baked";
pub(super) const QINGLIU_RENEWABLE_ENERGY_BUFF: &str = "manu_prod_spd&trade[000]";
pub(super) const WENDY_BIONIC_SEADRAGON_BUFF: &str = "manu_prod_spd&power[020]";
const DONGSHI_FLOW_OPTIMIZATION_BUFF: &str = "manu_prod_spd&manu[100]";

#[derive(Debug, Clone)]
struct ManufactureSystemCandidateRow {
    source_system: &'static str,
    source: &'static str,
    recipe: RecipeKind,
    operators: [&'static str; 3],
    linked_power_operator: &'static str,
    evidence: [&'static str; 2],
}

impl ManufactureSystemCandidateRow {
    fn automation_gold_windflit() -> Self {
        Self {
            source_system: AUTOMATION_SYSTEM_ID,
            source: SYSTEM_BAKED_SOURCE,
            recipe: RecipeKind::Gold,
            operators: ["清流", "温蒂", "冬时"],
            linked_power_operator: "承曦格雷伊",
            evidence: [
                "base_systems.json: automation_group gold factory anchors 清流 + 温蒂",
                "feedback seed expects 清流 + 温蒂 + 冬时 visibility",
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ManufactureLinkedProducer {
    pub station: String,
    pub operator: String,
    pub required_elite: Option<u8>,
    pub current_elite: Option<u8>,
    pub satisfied: bool,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ManufactureSystemCandidateTrace {
    pub room: String,
    pub recipe: String,
    pub operators: Vec<String>,
    pub source: String,
    pub selected: bool,
    pub rejected: bool,
    pub rejection_reason: Option<String>,
    pub raw_score: Option<f64>,
    pub evaluation_failed: Option<String>,
    pub linked_producers: Vec<ManufactureLinkedProducer>,
    pub source_system: String,
    pub evidence: Vec<String>,
}

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

pub(super) fn trace_gongsun_gold_windflit_candidate(
    room: &crate::layout::blueprint::RoomBlueprint,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    operbox: &OperBox,
    selected_hit: Option<&ManuSearchHit>,
) -> Option<ManufactureSystemCandidateTrace> {
    let row = ManufactureSystemCandidateRow::automation_gold_windflit();
    trace_system_manu_candidate(
        &row,
        room,
        assignment,
        used,
        pool,
        table,
        layout,
        options,
        operbox,
        selected_hit,
    )
}

#[allow(clippy::too_many_arguments)]
fn trace_system_manu_candidate(
    row: &ManufactureSystemCandidateRow,
    room: &crate::layout::blueprint::RoomBlueprint,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    operbox: &OperBox,
    selected_hit: Option<&ManuSearchHit>,
) -> Option<ManufactureSystemCandidateTrace> {
    if !room_matches_system_candidate(row, room) {
        return None;
    }
    let operators: Vec<String> = row
        .operators
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let linked = ManufactureLinkedProducer {
        station: "power".to_string(),
        operator: row.linked_power_operator.to_string(),
        required_elite: Some(2),
        current_elite: operbox.elite_of(row.linked_power_operator),
        satisfied: operbox
            .elite_of(row.linked_power_operator)
            .is_some_and(|elite| elite >= 2),
        role: "linked_virtual_power".to_string(),
    };

    let mut missing = Vec::new();
    let mut entries = Vec::new();
    for name in &operators {
        match pool.entry(name) {
            Some(entry) => entries.push(entry.clone()),
            None => missing.push(name.clone()),
        }
    }

    let (raw_score, evaluation_failed) = if missing.is_empty() {
        let opts = manu_options(layout, options, row.recipe, room.level);
        match score_manu_entries(&entries, table, layout, options, row.recipe, opts.level) {
            Some(hit) => (Some(hit.composite_score), None),
            None => (None, Some("evaluation_failed".to_string())),
        }
    } else {
        (
            None,
            Some(format!("missing_operator:{}", missing.join(","))),
        )
    };

    let wendy_ready = pool.entry("温蒂").is_some_and(|entry| {
        entry
            .buff_ids
            .iter()
            .any(|id| id == WENDY_BIONIC_SEADRAGON_BUFF)
    });
    let dongshi_ready = pool.entry("冬时").is_some_and(|entry| {
        entry
            .buff_ids
            .iter()
            .any(|id| id == DONGSHI_FLOW_OPTIMIZATION_BUFF)
    });
    let qingliu_ready = pool.entry("清流").is_some_and(|entry| {
        entry
            .buff_ids
            .iter()
            .any(|id| id == QINGLIU_RENEWABLE_ENERGY_BUFF)
    });
    let current_room_names: HashSet<String> = assignment
        .operators_in(&room.id)
        .iter()
        .map(|op| op.name.clone())
        .collect();
    let used_elsewhere = operators
        .iter()
        .any(|name| used.contains(name) && !current_room_names.contains(name));
    let selected_by_assignment = {
        let assigned_names: Vec<_> = assignment
            .operators_in(&room.id)
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assigned_names.len() == row.operators.len()
            && row
                .operators
                .iter()
                .all(|name| assigned_names.contains(name))
    };
    let selected_by_hit = selected_hit.is_some_and(|hit| manu_hit_matches_names(hit, &operators));
    let selected = selected_by_assignment || selected_by_hit;
    let selected_score = selected_hit.map(|hit| hit.composite_score);
    let rejection_reason = if selected {
        None
    } else if !missing.is_empty() {
        Some("missing_operator".to_string())
    } else if used_elsewhere {
        Some("operator_used".to_string())
    } else if !wendy_ready || !dongshi_ready {
        Some("tier_gate_not_met".to_string())
    } else if !qingliu_ready {
        Some("required_buff_missing".to_string())
    } else if !linked.satisfied {
        Some("linked_producer_not_satisfied".to_string())
    } else if let (Some(raw), Some(selected)) = (raw_score, selected_score) {
        if raw < selected {
            Some("raw_score_below_selected".to_string())
        } else {
            Some("trace_only_low_progress".to_string())
        }
    } else if evaluation_failed.is_some() {
        Some("evaluation_failed".to_string())
    } else {
        Some("trace_only_low_progress".to_string())
    };

    Some(ManufactureSystemCandidateTrace {
        room: room.id.0.clone(),
        recipe: recipe_label(row.recipe).to_string(),
        operators,
        source: row.source.to_string(),
        selected,
        rejected: !selected && rejection_reason.is_some(),
        rejection_reason,
        raw_score,
        evaluation_failed,
        linked_producers: vec![linked],
        source_system: row.source_system.to_string(),
        evidence: row.evidence.iter().map(|value| (*value).to_string()).collect(),
    })
}

fn room_matches_system_candidate(
    row: &ManufactureSystemCandidateRow,
    room: &crate::layout::blueprint::RoomBlueprint,
) -> bool {
    room.kind == FacilityKind::Factory
        && matches!(
            room.product.as_ref(),
            Some(RoomProduct::Factory { recipe }) if *recipe == row.recipe
        )
}

fn recipe_label(recipe: RecipeKind) -> &'static str {
    match recipe {
        RecipeKind::Gold => "gold",
        RecipeKind::BattleRecord => "battle_record",
        RecipeKind::Originium => "originium",
        RecipeKind::All => "all",
    }
}

fn manu_hit_matches_names(hit: &ManuSearchHit, expected: &[String]) -> bool {
    let names = manu_hit_names(hit);
    names.len() == expected.len() && expected.iter().all(|name| names.contains(name))
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
    operbox: &OperBox,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    forbid_same_room: &[(String, String)],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    mut trace_sink: Option<&mut Vec<ManufactureSystemCandidateTrace>>,
) -> Result<()> {
    try_assign_gongsun_gold_manu_team(blueprint, assignment, pool, used)?;
    if let Some(trace_sink) = trace_sink.as_deref_mut() {
        let selected_gold_room = blueprint.rooms.iter().find(|room| {
            room.kind == FacilityKind::Factory
                && matches!(
                    room.product.as_ref(),
                    Some(RoomProduct::Factory {
                        recipe: RecipeKind::Gold
                    })
                )
                && {
                    let assigned_names: Vec<_> = assignment
                        .operators_in(&room.id)
                        .iter()
                        .map(|op| op.name.as_str())
                        .collect();
                    assigned_names.len() == 3
                        && ["清流", "温蒂", "冬时"]
                            .iter()
                            .all(|name| assigned_names.contains(name))
                }
        });
        if let Some(room) = selected_gold_room {
            if let Some(trace) = trace_gongsun_gold_windflit_candidate(
                room, assignment, used, pool, table, layout, options, operbox, None,
            ) {
                if trace.selected {
                    trace_sink.push(trace);
                }
            }
        }
    }

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
        if matches!(recipe, RecipeKind::Gold) {
            if let Some(trace_sink) = trace_sink.as_deref_mut() {
                if let Some(trace) = trace_gongsun_gold_windflit_candidate(
                    room,
                    assignment,
                    used,
                    pool,
                    table,
                    layout,
                    options,
                    operbox,
                    Some(&hit),
                ) {
                    trace_sink.push(trace);
                }
            }
        }
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
    use crate::layout::assignment::AssignedOperator;

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

    fn trace_test_entry(name: &str, buff_ids: &[&str], elite: u8, rarity: u8) -> ManuPoolEntry {
        let progress = crate::roster::OperatorProgress::new(elite, 1, rarity);
        ManuPoolEntry {
            name: name.to_string(),
            elite,
            progress,
            buff_ids: buff_ids.iter().map(|id| (*id).to_string()).collect(),
            tags: vec![],
            flat_eff_hint: 0.0,
            has_l2_delegate: false,
            tier: crate::layout::tier::OperatorTier::Standalone,
        }
    }

    #[test]
    fn manufacture_trace_sink_reports_operator_used_for_later_gold_room() {
        let mut blueprint = BaseBlueprint::template_243_use_this().unwrap();
        blueprint
            .rooms
            .retain(|room| matches!(room.id.0.as_str(), "manu_1" | "manu_2"));
        let operbox = OperBox::from_entries(vec![
            crate::operbox::OperBoxEntry {
                id: "test_qingliu".to_string(),
                name: "清流".to_string(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
            crate::operbox::OperBoxEntry {
                id: "test_wendy".to_string(),
                name: "温蒂".to_string(),
                elite: 1,
                level: 80,
                own: true,
                potential: 1,
                rarity: 6,
            },
            crate::operbox::OperBoxEntry {
                id: "test_dongshi".to_string(),
                name: "冬时".to_string(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 5,
            },
            crate::operbox::OperBoxEntry {
                id: "test_greyy2".to_string(),
                name: "承曦格雷伊".to_string(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 5,
            },
        ]);
        let pool = ManuPool {
            entries: vec![
                trace_test_entry("清流", &[QINGLIU_RENEWABLE_ENERGY_BUFF], 2, 4),
                trace_test_entry("温蒂", &["manu_prod_spd&power[010]"], 1, 6),
                trace_test_entry("冬时", &["manu_prod_spd&manu[000]"], 0, 5),
                trace_test_entry("斑点", &["manu_prod_spd[020]"], 1, 3),
                trace_test_entry("至简", &["manu_prod_spd&fac_cnt[020]"], 2, 5),
                trace_test_entry("砾", &["manu_prod_spd&limit[010]"], 1, 4),
            ],
            skipped: vec![],
        };
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let layout = LayoutContext::search_baseline();
        let options = AssignBaseOptions::default();
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            RoomId::from("manu_1"),
            vec![AssignedOperator::new("清流", 2)],
        );
        let mut used = HashSet::from(["清流".to_string()]);
        let mut trace_sink = Vec::new();

        assign_manufacture_lines(
            &blueprint,
            &operbox,
            &pool,
            &table,
            &layout,
            &options,
            &[],
            &mut assignment,
            &mut used,
            Some(&mut trace_sink),
        )
        .unwrap();

        assert!(
            trace_sink.iter().any(|trace| {
                trace.room == "manu_2"
                    && trace.operators == vec!["清流", "温蒂", "冬时"]
                    && trace.rejection_reason.as_deref() == Some("operator_used")
            }),
            "later gold room should report operator_used trace, got {trace_sink:?}"
        );
    }
}
