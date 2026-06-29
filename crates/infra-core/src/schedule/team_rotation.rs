use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_control, assign_manu_room_with_anchors, assign_shift_with_plan_skip,
    assign_team_gamma_half, pinned_assignment, resolve_base, ActivatedSystem, AssignBaseOptions,
    AssignShiftMode, AssignedOperator, AssignmentPlan, BaseAssignment, BaseBlueprint, FacilityKind,
    LayoutContext, RoomAssignment, RoomId, RoomProduct, SlotFill,
};
use crate::operbox::OperBox;
use crate::pool::{
    add_jie_market_to_trade_pool, build_control_pool, build_manufacture_pool, build_power_pool,
    build_trade_pool, karlan_precision_active, ManuPool, ManuPoolEntry,
};
use crate::search::{control_efficiency_fill_sort_weight, control_entry_plugin_fill};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

use super::base_rotation::{score_base_assignment, ShiftScores};
use super::shift_bind::{align_shift_binds_in_halves, shift_binds_from_plan};

/// αβγ 三队标签。每班两队上岗、一队休息；设施每班全部满编（不空转）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLabel {
    Alpha,
    Beta,
    Gamma,
}

impl TeamLabel {
    pub const ALL: [TeamLabel; 3] = [TeamLabel::Alpha, TeamLabel::Beta, TeamLabel::Gamma];
}

/// 一支队伍（轮休 cohort）：休息一个班次的一批干员。
#[derive(Debug, Clone, Serialize)]
pub struct TeamAssignment {
    pub label: TeamLabel,
    pub operators: Vec<String>,
}

/// 单个班次结果：当班两队合起来铺满全部设施。
#[derive(Debug, Clone, Serialize)]
pub struct TeamShiftResult {
    pub index: usize,
    pub duration_hours: f64,
    pub active_teams: Vec<TeamLabel>,
    pub resting_team: TeamLabel,
    pub assignment: BaseAssignment,
    pub scores: ShiftScores,
    /// 贸易分按时长折算（三类各自独立，不混合量纲）。
    pub weighted_trade: f64,
    /// 制造产量按时长折算。
    pub weighted_manu: f64,
    /// 发电充能% 按时长折算。
    pub weighted_power: f64,
}

/// 三类各自的每日加权产出（贸易/制造/发电分开，不相加）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct DailyTotals {
    pub trade: f64,
    pub manu: f64,
    pub power: f64,
}

/// αβγ 三队轮换报告。
#[derive(Debug, Clone, Serialize)]
pub struct TeamRotationReport {
    /// peak 班编排计划（只读；α/β 切半与 γ 贸易 role 填充均据此对齐）。
    pub peak_plan: AssignmentPlan,
    pub teams: Vec<TeamAssignment>,
    pub shifts: Vec<TeamShiftResult>,
    /// 三类各自的每日加权产出（12h×αβ + 6h×βγ + 6h×γα，分别汇总）。
    pub daily: DailyTotals,
    pub elapsed: Duration,
}

/// 生产设施一个半区（trade/manu/power 各一组完整房间）。
#[derive(Debug, Clone, Default)]
pub struct FacilityHalf {
    pub trade: Vec<RoomId>,
    pub manu: Vec<RoomId>,
    pub power: Vec<RoomId>,
}

/// 把全部生产设施（贸易/制造/发电）按同类房间交替切成两半，尽量均衡负载。
fn split_production_facilities(blueprint: &BaseBlueprint) -> [FacilityHalf; 2] {
    let mut halves: [FacilityHalf; 2] = Default::default();
    for (i, room) in blueprint
        .rooms_of(FacilityKind::TradePost)
        .iter()
        .enumerate()
    {
        halves[i % 2].trade.push(room.id.clone());
    }
    for (i, room) in blueprint.rooms_of(FacilityKind::Factory).iter().enumerate() {
        halves[i % 2].manu.push(room.id.clone());
    }
    for (i, room) in blueprint
        .rooms_of(FacilityKind::PowerPlant)
        .iter()
        .enumerate()
    {
        halves[i % 2].power.push(room.id.clone());
    }
    halves
}

/// γ 替补半区：贸易沿用 core role 顺序，制造/发电站绑定搜索。
#[allow(clippy::too_many_arguments)]
fn assign_gamma_half(
    blueprint: &BaseBlueprint,
    pools: &ProductionPools,
    table: &SkillTable,
    layout: &crate::layout::LayoutContext,
    options: &AssignBaseOptions,
    half: &FacilityHalf,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_team_gamma_half(
        blueprint,
        &pools.trade,
        &pools.manu,
        &pools.power,
        table,
        layout,
        options,
        &half.trade,
        &half.manu,
        &half.power,
        assignment,
        used,
    )
}

fn production_half_from_peak(peak: &BaseAssignment, half: &FacilityHalf) -> BaseAssignment {
    let mut half_assignment = BaseAssignment::default();
    for room_id in half
        .trade
        .iter()
        .chain(half.manu.iter())
        .chain(half.power.iter())
    {
        if let Some(room) = peak.room_assignment(room_id) {
            if !room.operators.is_empty() {
                half_assignment.set_room_assignment(room.clone());
            }
        }
    }
    half_assignment
}

struct ProductionPools {
    trade: crate::pool::TradePool,
    manu: crate::pool::ManuPool,
    power: crate::pool::PowerPool,
}

fn operators_of(assignment: &BaseAssignment) -> Vec<String> {
    let mut names: Vec<String> = assignment
        .rooms
        .iter()
        .flat_map(|r| r.operators.iter().map(|o| o.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn merge_rooms(target: &mut BaseAssignment, source: &BaseAssignment) {
    for room in &source.rooms {
        if room.operators.is_empty() {
            continue;
        }
        target.set_room_assignment(room.clone());
    }
}

fn clear_room(assignment: &mut BaseAssignment, room_id: &str) {
    assignment.rooms.retain(|room| room.room_id.0 != room_id);
}

// ── 深海链 S2 短班入口 ──

const ABYSSAL_GLADIIA: &str = "歌蕾蒂娅";
const ABYSSAL_HUNTERS: [&str; 4] = ["乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"];
const ABYSSAL_FORBID_SAME_ROOM_MANU_BUFFS: [&str; 3] = [
    "manu_prod_spd&power[000]",
    "manu_prod_spd&power[010]",
    "manu_prod_spd&power[020]",
];
const TAG_ABYSSAL: &str = "cc.g.abyssal";
const DAIFEEN: &str = "戴菲恩";
const VINA_TRADE_GROUP: [&str; 3] = ["推进之王", "摩根", "维娜·维多利亚"];
const WARMUP_STICKY_TRADE_OPERATORS: [&str; 2] = ["巫恋", "龙舌兰"];

fn room_has_all(room: &RoomAssignment, names: &[&str]) -> bool {
    names
        .iter()
        .all(|name| room.operators.iter().any(|op| op.name == *name))
}

fn trade_rooms_compatible_for_swap(blueprint: &BaseBlueprint, a: &RoomId, b: &RoomId) -> bool {
    if a == b {
        return true;
    }
    let Some(room_a) = blueprint.room(a) else {
        return false;
    };
    let Some(room_b) = blueprint.room(b) else {
        return false;
    };
    room_a.kind == FacilityKind::TradePost
        && room_b.kind == FacilityKind::TradePost
        && room_a.level == room_b.level
        && room_a.product == room_b.product
        && matches!(room_a.product, Some(RoomProduct::Trade { .. }))
}

fn trade_room_containing_group(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    names: &[&str],
) -> Option<RoomId> {
    assignment.rooms.iter().find_map(|room| {
        let bp_room = blueprint.room(&room.room_id)?;
        (bp_room.kind == FacilityKind::TradePost && room_has_all(room, names))
            .then(|| room.room_id.clone())
    })
}

fn swap_trade_room_assignments(assignment: &mut BaseAssignment, a: &RoomId, b: &RoomId) -> bool {
    let Some(ai) = assignment.rooms.iter().position(|room| &room.room_id == a) else {
        return false;
    };
    let Some(bi) = assignment.rooms.iter().position(|room| &room.room_id == b) else {
        return false;
    };
    if ai == bi {
        return true;
    }
    let ops_a = assignment.rooms[ai].operators.clone();
    let eff_a = assignment.rooms[ai].efficiency.clone();
    assignment.rooms[ai].operators = assignment.rooms[bi].operators.clone();
    assignment.rooms[ai].efficiency = assignment.rooms[bi].efficiency.clone();
    assignment.rooms[bi].operators = ops_a;
    assignment.rooms[bi].efficiency = eff_a;
    true
}

/// 巫恋/龙舌兰订单体系有暖机成本；跨短班连续上岗时保持同一贸易站。
fn align_trade_warmup_room(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    sticky_room: &mut Option<RoomId>,
) {
    let Some(current_room) =
        trade_room_containing_group(assignment, blueprint, &WARMUP_STICKY_TRADE_OPERATORS)
    else {
        return;
    };
    let Some(anchor_room) = sticky_room.clone() else {
        *sticky_room = Some(current_room);
        return;
    };
    if current_room == anchor_room {
        return;
    }
    if trade_rooms_compatible_for_swap(blueprint, &current_room, &anchor_room) {
        swap_trade_room_assignments(assignment, &current_room, &anchor_room);
    }
}

struct AbyssalCandidate {
    assignment: BaseAssignment,
    gamma_ops: Vec<String>,
}

/// 构造 S2 深海短班候选：四名深海猎手视作等价生产锚，按房间人数计数枚举。
/// 歌蕾蒂娅只能承受约 7h 短班；深海链不进入普通 base_systems registry。
struct AbyssalBuildCtx<'a> {
    operbox: &'a OperBox,
    instances: &'a OperatorInstances,
    table: &'a SkillTable,
    blueprint: &'a BaseBlueprint,
    layout: &'a LayoutContext,
    options: &'a AssignBaseOptions,
    manu_pool: &'a ManuPool,
    used_ab: &'a HashSet<String>,
    blocked_ops: &'a HashSet<String>,
    shared: &'a BaseAssignment,
    beta: &'a BaseAssignment,
    gamma_h1: &'a BaseAssignment,
    mutable_manu_rooms: &'a [RoomId],
}

fn owned_abyssal_hunters(operbox: &OperBox, used_ab: &HashSet<String>) -> Vec<String> {
    ABYSSAL_HUNTERS
        .iter()
        .filter(|name| operbox.owns(name) && !used_ab.contains(**name))
        .map(|name| (*name).to_string())
        .collect()
}

fn abyssal_manu_entry(
    name: &str,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Option<ManuPoolEntry> {
    let progress = operbox.progress_of(name)?;
    let tier = PromotionTier::from_progress(progress);
    let buff_ids = instances.resolve_manufacture_buff_ids(name, tier);
    for buff_id in &buff_ids {
        let skill = table.get(buff_id)?;
        if skill.facility != "manufacture" {
            return None;
        }
    }
    let mut tags = instances.tags_for(name, tier);
    if !tags.iter().any(|tag| tag == TAG_ABYSSAL) {
        tags.push(TAG_ABYSSAL.to_string());
    }
    Some(ManuPoolEntry {
        name: name.to_string(),
        elite: progress.elite,
        progress,
        buff_ids,
        tags,
        flat_eff_hint: 0.0,
        has_l2_delegate: false,
        tier: crate::layout::OperatorTier::CrossStation,
    })
}

fn build_abyssal_s2_candidates(ctx: &AbyssalBuildCtx<'_>) -> Vec<AbyssalCandidate> {
    let Some(gladiia_elite) = ctx.operbox.elite_of(ABYSSAL_GLADIIA) else {
        return Vec::new();
    };
    if gladiia_elite < 2 || ctx.used_ab.contains(ABYSSAL_GLADIIA) {
        return Vec::new();
    }

    let hunters = owned_abyssal_hunters(ctx.operbox, ctx.used_ab);
    if hunters.len() < 3 {
        return Vec::new();
    }

    if ctx.mutable_manu_rooms.is_empty() {
        return Vec::new();
    }

    let mut base = ctx.shared.clone();
    merge_rooms(&mut base, ctx.beta);
    merge_rooms(&mut base, ctx.gamma_h1);

    let gamma_original: HashSet<String> = operators_of(ctx.gamma_h1).into_iter().collect();
    let hunter_names: HashSet<String> = hunters.iter().cloned().collect();
    let hunter_entries: Vec<ManuPoolEntry> = hunters
        .iter()
        .filter_map(|name| abyssal_manu_entry(name, ctx.operbox, ctx.instances, ctx.table))
        .collect();
    if hunter_entries.len() != hunters.len() {
        return Vec::new();
    }

    let mut count_vectors = Vec::new();
    enumerate_abyssal_counts(
        hunters.len(),
        ctx.mutable_manu_rooms.len(),
        &mut Vec::new(),
        &mut count_vectors,
    );

    let mut out = Vec::new();
    for counts in count_vectors {
        let mut candidate = base.clone();
        for room_id in ctx.mutable_manu_rooms {
            clear_room(&mut candidate, room_id.0.as_str());
        }
        let mut used = candidate.operator_names();
        used.extend(ctx.blocked_ops.iter().cloned());
        let mut next_hunter = 0;
        let mut ok = true;
        for (room_idx, count) in counts.iter().copied().enumerate() {
            if count == 0 {
                continue;
            }
            let room_id = &ctx.mutable_manu_rooms[room_idx];
            let Some(room) = ctx.blueprint.room(room_id) else {
                ok = false;
                break;
            };
            let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
                ok = false;
                break;
            };
            let anchors = hunter_entries[next_hunter..next_hunter + count].to_vec();
            next_hunter += count;
            if assign_manu_room_with_anchors(
                &mut candidate,
                room_id,
                anchors,
                ctx.manu_pool,
                ctx.table,
                ctx.layout,
                ctx.options,
                *recipe,
                room.level,
                &mut used,
                &ABYSSAL_FORBID_SAME_ROOM_MANU_BUFFS,
            )
            .is_err()
            {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }

        let mut gamma_ops: Vec<String> = candidate
            .rooms
            .iter()
            .filter(|room| ctx.mutable_manu_rooms.contains(&room.room_id))
            .flat_map(|room| room.operators.iter())
            .filter(|op| gamma_original.contains(&op.name) || hunter_names.contains(&op.name))
            .map(|op| op.name.clone())
            .collect();
        gamma_ops.sort();
        gamma_ops.dedup();
        out.push(AbyssalCandidate {
            assignment: candidate,
            gamma_ops,
        });
    }

    out
}

fn enumerate_abyssal_counts(
    remaining: usize,
    room_count: usize,
    current: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if current.len() == room_count {
        if remaining == 0 {
            out.push(current.clone());
        }
        return;
    }
    for count in 0..=remaining.min(3) {
        current.push(count);
        enumerate_abyssal_counts(remaining - count, room_count, current, out);
        current.pop();
    }
}

// ── 深海链短班入口结束 ──

// ── 中枢队伍归属 ──

/// 从编排计划中提取体系绑定的中枢干员名（含 PickOne 候选人）。
fn system_control_operators(plan: &AssignmentPlan) -> HashSet<String> {
    let mut names = HashSet::new();
    for sys in &plan.activated {
        let has_production_slot = sys.slots.iter().any(|slot| {
            let facility = match slot {
                SlotFill::Fixed { facility, .. }
                | SlotFill::PickOne { facility, .. }
                | SlotFill::OptionalFixed { facility, .. } => facility,
            };
            matches!(facility, FacilityKind::TradePost | FacilityKind::Factory)
        });
        if !has_production_slot {
            continue;
        }
        for slot in &sys.slots {
            let facility = match slot {
                SlotFill::Fixed { facility, .. }
                | SlotFill::PickOne { facility, .. }
                | SlotFill::OptionalFixed { facility, .. } => facility,
            };
            if *facility != FacilityKind::ControlCenter {
                continue;
            }
            match slot {
                SlotFill::Fixed { operator, .. } | SlotFill::OptionalFixed { operator, .. } => {
                    names.insert(operator.clone());
                }
                SlotFill::PickOne { candidates, .. } => {
                    for c in candidates {
                        names.insert(c.clone());
                    }
                }
            }
        }
    }
    names
}

/// 判断体系的控制中枢干员应归属哪个队伍（基于生产 slot 所在半区）。
/// H1 → Alpha, H2 → Beta；纯中枢体系（无生产 slot）返回 None。
fn system_control_team(sys: &ActivatedSystem, h1: &FacilityHalf) -> Option<TeamLabel> {
    for slot in &sys.slots {
        let (facility, room_id) = match slot {
            SlotFill::Fixed {
                facility, room_id, ..
            }
            | SlotFill::PickOne {
                facility, room_id, ..
            } => (facility, room_id.as_ref()),
            SlotFill::OptionalFixed { .. } => continue,
        };
        let Some(rid) = room_id else {
            continue;
        };
        if !matches!(facility, FacilityKind::TradePost | FacilityKind::Factory) {
            continue;
        }
        if h1.trade.contains(rid) || h1.manu.contains(rid) || h1.power.contains(rid) {
            return Some(TeamLabel::Alpha);
        }
        return Some(TeamLabel::Beta);
    }
    None
}

/// 按"体系归属 + 散件均分"规则将 peak 中枢干员分入 α/β/γ 三队。
///
/// - 体系绑定干员：跟随其生产体系所在半区（H1→α, H2→β；纯中枢体系默认归 α）
/// - 补位散件：按人数最少优先轮询分入三队
fn build_team_control_map(
    peak_ctrl: &[String],
    plan: &AssignmentPlan,
    h1: &FacilityHalf,
) -> HashMap<TeamLabel, Vec<String>> {
    let system_names = system_control_operators(plan);
    let peak_set: HashSet<String> = peak_ctrl.iter().cloned().collect();
    let mut team_map: HashMap<TeamLabel, Vec<String>> = HashMap::new();

    // Step 1: 体系干员归属到对应队伍
    for sys in &plan.activated {
        let team = system_control_team(sys, h1).unwrap_or(TeamLabel::Alpha);
        for slot in &sys.slots {
            let facility = match slot {
                SlotFill::Fixed { facility, .. }
                | SlotFill::PickOne { facility, .. }
                | SlotFill::OptionalFixed { facility, .. } => facility,
            };
            if *facility != FacilityKind::ControlCenter {
                continue;
            }
            let op_name: Option<String> = match slot {
                SlotFill::Fixed { operator, .. } | SlotFill::OptionalFixed { operator, .. } => {
                    if peak_set.contains(operator) {
                        Some(operator.clone())
                    } else {
                        None
                    }
                }
                SlotFill::PickOne { candidates, .. } => {
                    candidates.iter().find(|c| peak_set.contains(*c)).cloned()
                }
            };
            if let Some(name) = op_name {
                team_map.entry(team).or_default().push(name);
            }
        }
    }

    // Step 2: 散件（非体系绑定）按人数最少优先轮询分入三队
    let filler: Vec<String> = peak_ctrl
        .iter()
        .filter(|n| !system_names.contains(*n))
        .cloned()
        .collect();
    let teams = [TeamLabel::Alpha, TeamLabel::Beta, TeamLabel::Gamma];
    for name in filler {
        let t = teams
            .iter()
            .min_by_key(|t| team_map.get(t).map(|v| v.len()).unwrap_or(0))
            .copied()
            .unwrap();
        team_map.entry(t).or_default().push(name);
    }

    team_map
}

fn control_rotation_candidate(
    entry: &crate::pool::ControlPoolEntry,
    _table: &SkillTable,
    _layout: &crate::layout::LayoutContext,
    _mood: f64,
) -> bool {
    control_entry_plugin_fill(entry)
}

fn control_entry_trade_inject(entry: &crate::pool::ControlPoolEntry) -> bool {
    entry
        .buff_ids
        .iter()
        .any(|bid| bid.starts_with("control_tra_spd") || bid == "control_token_tra_spd[000]")
}

fn control_entry_manu_inject(entry: &crate::pool::ControlPoolEntry) -> bool {
    entry.buff_ids.iter().any(|bid| {
        bid.starts_with("control_prod_spd")
            || bid.starts_with("control_token_prod_spd")
            || bid == "control_bd_spd[000]"
            || bid == "control_prod_tra_spd[000]"
    })
}

fn team_control_class_count(
    team_ctrl: &HashMap<TeamLabel, Vec<String>>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
    team: TeamLabel,
    class: fn(&crate::pool::ControlPoolEntry) -> bool,
) -> usize {
    team_ctrl
        .get(&team)
        .into_iter()
        .flatten()
        .filter(|name| entry_by_name.get(*name).is_some_and(class))
        .count()
}

fn pick_control_plugin_team(
    entry: &crate::pool::ControlPoolEntry,
    team_ctrl: &HashMap<TeamLabel, Vec<String>>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
) -> TeamLabel {
    let class = if control_entry_manu_inject(entry) {
        Some(control_entry_manu_inject as fn(&crate::pool::ControlPoolEntry) -> bool)
    } else if control_entry_trade_inject(entry) {
        Some(control_entry_trade_inject as fn(&crate::pool::ControlPoolEntry) -> bool)
    } else {
        None
    };
    TeamLabel::ALL
        .iter()
        .min_by_key(|team| {
            let class_count = class
                .map(|c| team_control_class_count(team_ctrl, entry_by_name, **team, c))
                .unwrap_or(0);
            let total_count = team_ctrl.get(team).map(|v| v.len()).unwrap_or(0);
            (class_count, total_count)
        })
        .copied()
        .unwrap()
}

fn balance_control_plugin_class(
    team_ctrl: &mut HashMap<TeamLabel, Vec<String>>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
    class: fn(&crate::pool::ControlPoolEntry) -> bool,
) {
    loop {
        let uncovered: Vec<_> = TeamLabel::ALL
            .into_iter()
            .filter(|team| team_control_class_count(team_ctrl, entry_by_name, *team, class) == 0)
            .collect();
        if uncovered.is_empty() {
            break;
        }
        let Some(from_team) = TeamLabel::ALL
            .into_iter()
            .filter(|team| team_control_class_count(team_ctrl, entry_by_name, *team, class) > 1)
            .max_by_key(|team| {
                (
                    team_control_class_count(team_ctrl, entry_by_name, *team, class),
                    team_ctrl.get(team).map(|names| names.len()).unwrap_or(0),
                )
            })
        else {
            break;
        };
        let to_team = *uncovered
            .iter()
            .min_by_key(|team| team_ctrl.get(team).map(|names| names.len()).unwrap_or(0))
            .unwrap();
        let Some(move_idx) = team_ctrl.get(&from_team).and_then(|names| {
            names
                .iter()
                .enumerate()
                .filter(|(_, name)| entry_by_name.get(*name).is_some_and(class))
                .max_by(|(_, a), (_, b)| {
                    let aw = entry_by_name
                        .get(*a)
                        .map(control_efficiency_fill_sort_weight)
                        .unwrap_or(0.0);
                    let bw = entry_by_name
                        .get(*b)
                        .map(control_efficiency_fill_sort_weight)
                        .unwrap_or(0.0);
                    aw.partial_cmp(&bw)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.cmp(a))
                })
                .map(|(idx, _)| idx)
        }) else {
            break;
        };
        let moved = team_ctrl.get_mut(&from_team).unwrap().remove(move_idx);
        team_ctrl.entry(to_team).or_default().push(moved);
    }
}

fn normalize_control_team_membership(
    team_ctrl: &mut HashMap<TeamLabel, Vec<String>>,
    production_team_by_name: &HashMap<String, TeamLabel>,
) {
    let mut chosen: HashMap<String, TeamLabel> = HashMap::new();
    for team in TeamLabel::ALL {
        for name in team_ctrl.get(&team).into_iter().flatten() {
            let target = production_team_by_name.get(name).copied().unwrap_or(team);
            chosen.entry(name.clone()).or_insert(target);
        }
    }
    for names in team_ctrl.values_mut() {
        names.clear();
    }
    for (name, team) in chosen {
        team_ctrl.entry(team).or_default().push(name);
    }
}

fn control_room_has_class(
    ops: &[AssignedOperator],
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
    class: fn(&crate::pool::ControlPoolEntry) -> bool,
) -> bool {
    ops.iter()
        .any(|op| entry_by_name.get(&op.name).is_some_and(class))
}

fn control_replace_rank(
    op: &AssignedOperator,
    system_ctrl_names: &HashSet<String>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
) -> i32 {
    if op.name == ABYSSAL_GLADIIA || op.name == DAIFEEN {
        return -1;
    }
    if op.name == "八幡海铃" {
        return 3;
    }
    if system_ctrl_names.contains(&op.name) {
        return -1;
    }
    let Some(entry) = entry_by_name.get(&op.name) else {
        return 0;
    };
    if control_entry_trade_inject(entry) || control_entry_manu_inject(entry) {
        1
    } else {
        3
    }
}

fn ensure_control_inject_coverage(
    assignment: &mut BaseAssignment,
    final_pool: &crate::pool::ControlPool,
    system_ctrl_names: &HashSet<String>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
) {
    let mut ops = assignment.control_operators();
    let assigned = assignment.operator_names();
    for class in [
        control_entry_trade_inject as fn(&crate::pool::ControlPoolEntry) -> bool,
        control_entry_manu_inject as fn(&crate::pool::ControlPoolEntry) -> bool,
    ] {
        if control_room_has_class(&ops, entry_by_name, class) {
            continue;
        }
        let Some(candidate) = final_pool
            .entries
            .iter()
            .filter(|entry| class(entry))
            .filter(|entry| {
                !assigned.contains(&entry.name) || ops.iter().any(|op| op.name == entry.name)
            })
            .max_by(|a, b| {
                control_efficiency_fill_sort_weight(a)
                    .partial_cmp(&control_efficiency_fill_sort_weight(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.name.cmp(&a.name))
            })
        else {
            continue;
        };
        if ops.iter().any(|op| op.name == candidate.name) {
            continue;
        }
        let Some(drop_idx) = ops
            .iter()
            .enumerate()
            .filter_map(|(idx, op)| {
                let rank = control_replace_rank(op, system_ctrl_names, entry_by_name);
                (rank >= 0).then_some((idx, rank))
            })
            .max_by_key(|(_, rank)| *rank)
            .map(|(idx, _)| idx)
        else {
            continue;
        };
        ops[drop_idx] = AssignedOperator::from_progress(&candidate.name, candidate.progress);
    }
    assignment.set_room(RoomId::from("control"), ops);
}

fn move_control_operator_to_team(
    team_ctrl: &mut HashMap<TeamLabel, Vec<String>>,
    operator: &str,
    team: TeamLabel,
) {
    for names in team_ctrl.values_mut() {
        names.retain(|name| name != operator);
    }
    team_ctrl
        .entry(team)
        .or_default()
        .push(operator.to_string());
}

// ── 中枢队伍归属结束 ──

/// 全基建 αβγ 三队均衡轮休排班（公孙长乐替补池模型）。
///
/// - **设施每班全部满编，绝不空转**：每班由当班两队合力铺满所有贸易/制造/发电站。
/// - 生产设施切成两半 H1/H2：α 跑 H1、β 跑 H2；γ 作为轮换替补，第 2 班接 H1、第 3 班接 H2。
/// - 班次结构 12h + 6h + 6h；每队休息一个班次（α 休 S2、β 休 S3、γ 休 S1）。
/// - 宿舍 / 办公室为共享脚手架，三班钉死。
/// - 中枢按 αβγ 队伍轮休重分配：每班只用活跃两队候选，体系中枢先 pin，再补满 5 人。
pub fn schedule_team_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let t0 = Instant::now();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);

    // 1) 参考高峰班 + 编排计划 → 取宿舍/办公室作为三班共享脚手架。
    // 中枢后续按 αβγ 队伍轮休重分配，不能从 peak 直接钉死。
    // 深海链因歌蕾蒂娅心情消耗 ~3.2/h（最长约 7h），不可进入 12h 主班；
    // 唯一入口是后续 S2 6h 短班的有/无深海双路径评分对比。
    let peak_result = assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
        &HashSet::new(),
    )?;
    let peak = peak_result.assignment;
    let peak_plan = peak_result.plan;
    let shared = pinned_assignment(&peak, blueprint);
    let scaffold_used: HashSet<String> = operators_of(&shared).into_iter().collect();
    let t1 = Instant::now();

    // 2) 以脚手架解算共享 layout；生产搜索另带 peak 中枢注入，供喀兰等贸易 role 使用。
    let layout = resolve_base(
        blueprint,
        &shared,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    let mut production_seed = shared.clone();
    let mut production_control = peak.control_operators();
    if operbox.owns("灵知") && !production_control.iter().any(|op| op.name == "灵知") {
        if let Some(progress) = operbox.progress_of("灵知") {
            production_control.push(AssignedOperator::from_progress("灵知", progress));
        }
    }
    if !production_control.is_empty() {
        production_seed.set_room("control", production_control);
    }
    let production_layout = resolve_base(
        blueprint,
        &production_seed,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    let mut trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    if karlan_precision_active(&production_layout.global_inject) {
        add_jie_market_to_trade_pool(&mut trade_pool, instances, table);
    }
    let pools = ProductionPools {
        trade: trade_pool,
        manu: build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?,
        power: build_power_pool(&operbox.power_roster(instances), instances, table)?,
    };
    let control_pool = build_control_pool(&operbox.control_roster(instances), instances, table)?;

    let [mut h1, mut h2] = split_production_facilities(blueprint);
    // 班次绑定（上2休1）来自统一 plan，不再硬编码 ROSEMARY_BLACKKEY_BIND。
    let shift_binds = shift_binds_from_plan(&peak_plan);
    align_shift_binds_in_halves(&peak, &shift_binds, &mut h1, &mut h2);

    // 3) α/β 从 peak 编制按 H1/H2 切半（保留编排已认领的 meta 锚点）；γ 走贸易 role 替补。
    let alpha = production_half_from_peak(&peak, &h1);
    let beta = production_half_from_peak(&peak, &h2);
    peak_plan
        .verify_registry_trade_in_alpha_beta(&alpha, &beta)
        .map_err(Error::msg)?;
    let mut used = scaffold_used.clone();
    for name in operators_of(&alpha).into_iter().chain(operators_of(&beta)) {
        used.insert(name);
    }
    let t2 = Instant::now();

    // 4) γ 为替补：S2 接 H1、S3 接 H2，干员与 α/β 互斥（两次各自从剩余池取，可复用同人）。
    let used_ab = used.clone();

    let mut gamma_h1 = BaseAssignment::default();
    let mut used_g1 = used_ab.clone();
    assign_gamma_half(
        blueprint,
        &pools,
        table,
        &production_layout,
        options,
        &h1,
        &mut gamma_h1,
        &mut used_g1,
    )?;

    let mut gamma_h2 = BaseAssignment::default();
    let mut used_g2 = used_ab.clone();
    assign_gamma_half(
        blueprint,
        &pools,
        table,
        &production_layout,
        options,
        &h2,
        &mut gamma_h2,
        &mut used_g2,
    )?;
    let t3 = Instant::now();

    // 中枢每班独立分配：体系绑定干员跟随生产队同上同下，散件均分三队，
    // 每班仅允许活跃两队的中枢干员入池（保证每人休息一班）。
    let peak_ctrl: Vec<String> = peak
        .control_operators()
        .iter()
        .map(|o| o.name.clone())
        .collect();
    let system_ctrl_names = system_control_operators(&peak_plan);
    let mut team_ctrl = build_team_control_map(&peak_ctrl, &peak_plan, &h1);
    for sys in &peak_plan.activated {
        let team = system_control_team(sys, &h1).unwrap_or(TeamLabel::Alpha);
        for slot in &sys.slots {
            let facility = match slot {
                SlotFill::Fixed { facility, .. }
                | SlotFill::PickOne { facility, .. }
                | SlotFill::OptionalFixed { facility, .. } => facility,
            };
            if *facility != FacilityKind::ControlCenter {
                continue;
            }
            match slot {
                SlotFill::Fixed { operator, .. } | SlotFill::OptionalFixed { operator, .. } => {
                    if operbox.owns(operator) {
                        team_ctrl.entry(team).or_default().push(operator.clone());
                    }
                }
                SlotFill::PickOne { candidates, .. } => {
                    for candidate in candidates.iter().filter(|name| operbox.owns(name)) {
                        team_ctrl.entry(team).or_default().push(candidate.clone());
                    }
                }
            }
        }
    }
    let mut production_team_by_name: HashMap<String, TeamLabel> = HashMap::new();
    for name in operators_of(&alpha) {
        production_team_by_name
            .entry(name)
            .or_insert(TeamLabel::Alpha);
    }
    for name in operators_of(&beta) {
        production_team_by_name
            .entry(name)
            .or_insert(TeamLabel::Beta);
    }
    for name in operators_of(&gamma_h1)
        .into_iter()
        .chain(operators_of(&gamma_h2))
    {
        production_team_by_name
            .entry(name)
            .or_insert(TeamLabel::Gamma);
    }
    if let Some(jie_team) = production_team_by_name.get("孑").copied() {
        if operbox.owns("灵知") {
            move_control_operator_to_team(&mut team_ctrl, "灵知", jie_team);
        }
    }
    let vina_team = VINA_TRADE_GROUP
        .iter()
        .filter_map(|name| production_team_by_name.get(*name).copied())
        .next();
    if let Some(team) = vina_team {
        if operbox.owns(DAIFEEN) {
            move_control_operator_to_team(&mut team_ctrl, DAIFEEN, team);
        }
    }
    let entry_by_name: HashMap<String, crate::pool::ControlPoolEntry> = control_pool
        .entries
        .iter()
        .map(|entry| (entry.name.clone(), entry.clone()))
        .collect();
    let mut plugin_entries: Vec<_> = control_pool
        .entries
        .iter()
        .filter(|entry| !team_ctrl.values().any(|names| names.contains(&entry.name)))
        .filter(|entry| control_rotation_candidate(entry, table, &layout, options.mood))
        .collect();
    plugin_entries.sort_by(|a, b| {
        control_efficiency_fill_sort_weight(b)
            .partial_cmp(&control_efficiency_fill_sort_weight(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    for entry in plugin_entries {
        if team_ctrl.values().any(|names| names.contains(&entry.name)) {
            continue;
        }
        let team = production_team_by_name
            .get(&entry.name)
            .copied()
            .unwrap_or_else(|| pick_control_plugin_team(entry, &team_ctrl, &entry_by_name));
        team_ctrl.entry(team).or_default().push(entry.name.clone());
    }
    balance_control_plugin_class(&mut team_ctrl, &entry_by_name, control_entry_trade_inject);
    balance_control_plugin_class(&mut team_ctrl, &entry_by_name, control_entry_manu_inject);
    normalize_control_team_membership(&mut team_ctrl, &production_team_by_name);
    for names in team_ctrl.values_mut() {
        names.sort();
        names.dedup();
    }

    // 队伍花名册（cohort）。生产干员 + 中枢归队干员共同构成轮休队伍。
    let mut alpha_ops = operators_of(&alpha);
    alpha_ops.extend(
        team_ctrl
            .get(&TeamLabel::Alpha)
            .cloned()
            .unwrap_or_default(),
    );
    alpha_ops.sort();
    alpha_ops.dedup();

    let mut beta_ops = operators_of(&beta);
    beta_ops.extend(team_ctrl.get(&TeamLabel::Beta).cloned().unwrap_or_default());
    beta_ops.sort();
    beta_ops.dedup();

    let mut gamma_ops: Vec<String> = operators_of(&gamma_h1);
    gamma_ops.extend(operators_of(&gamma_h2));
    gamma_ops.extend(
        team_ctrl
            .get(&TeamLabel::Gamma)
            .cloned()
            .unwrap_or_default(),
    );
    gamma_ops.sort();
    gamma_ops.dedup();

    let mut teams = vec![
        TeamAssignment {
            label: TeamLabel::Alpha,
            operators: alpha_ops,
        },
        TeamAssignment {
            label: TeamLabel::Beta,
            operators: beta_ops,
        },
        TeamAssignment {
            label: TeamLabel::Gamma,
            operators: gamma_ops,
        },
    ];

    // 5) 组装三班（每班满编）并评分：
    //    S1(12h)=脚手架+α(H1)+β(H2)；S2(6h)=脚手架+β(H2)+γ(H1)；S3(6h)=脚手架+α(H1)+γ(H2)。
    //
    //    S2 深海链双路径对比：预占猎手 → γ fill 绕开 → 补第3人 → 评分，与无深海基线对比，选优胜。
    let shift_specs: [(f64, [TeamLabel; 2], TeamLabel, [&BaseAssignment; 2]); 3] = [
        (
            12.0,
            [TeamLabel::Alpha, TeamLabel::Beta],
            TeamLabel::Gamma,
            [&alpha, &beta],
        ),
        (
            6.0,
            [TeamLabel::Beta, TeamLabel::Gamma],
            TeamLabel::Alpha,
            [&beta, &gamma_h1],
        ),
        (
            6.0,
            [TeamLabel::Gamma, TeamLabel::Alpha],
            TeamLabel::Beta,
            [&gamma_h2, &alpha],
        ),
    ];

    let mut shifts = Vec::with_capacity(3);
    let mut daily = DailyTotals::default();
    let mut control_options = options.clone();
    control_options.skip_standalone_control = true;
    let mut warmup_sticky_trade_room: Option<RoomId> = None;
    for (index, (hours, active, resting, parts)) in shift_specs.into_iter().enumerate() {
        // 活跃两队的中枢干员名册
        let active_names: HashSet<String> = active
            .iter()
            .flat_map(|t| team_ctrl.get(t).cloned().unwrap_or_default())
            .collect();
        // 从全池按名提取活跃队中枢干员；体系中枢干员可能不在 standalone 中枢白名单，
        // 但已经由 base_systems/peak_plan 验证可用，因此要按名补回。
        // 所有中枢候选已在进入三班前归入 α/β/γ；这里不再用全局池补位，
        // 避免未归队散件绕过轮休、三班连续上岗。
        let mut team_entries: Vec<crate::pool::ControlPoolEntry> = control_pool
            .entries
            .iter()
            .filter(|e| active_names.contains(&e.name))
            .cloned()
            .collect();
        let present_entries: HashSet<String> =
            team_entries.iter().map(|e| e.name.clone()).collect();
        for name in active_names
            .iter()
            .filter(|n| system_ctrl_names.contains(*n) && !present_entries.contains(*n))
        {
            let progress = operbox.progress_of(name).unwrap_or_default();
            team_entries.push(crate::pool::ControlPoolEntry {
                name: name.clone(),
                elite: progress.elite,
                progress,
                buff_ids: instances.resolve_control_buff_ids(
                    name,
                    crate::tier::PromotionTier::from_progress(progress),
                ),
                tags: vec![],
                tier: crate::layout::tier::OperatorTier::CrossStation,
            });
        }
        let base_control_pool = crate::pool::ControlPool {
            entries: team_entries,
            skipped: vec![],
        };

        // 体系中枢干员是当班硬锚点：先 pin 到 control，再由 assign_control 补满 5 人。
        // 这样薇薇安娜/夕等不在 standalone 中枢白名单内的体系位不会被搜索阶段丢掉。
        let pin_active_system_control = |a: &mut BaseAssignment, extra_control_pins: &[&str]| {
            let mut ops = a.control_operators();
            let mut room_names: HashSet<String> = ops.iter().map(|o| o.name.clone()).collect();
            let assigned_names = a.operator_names();
            for name in extra_control_pins {
                if ops.len() >= 5 {
                    break;
                }
                if room_names.contains(*name) || assigned_names.contains(*name) {
                    continue;
                }
                let Some(progress) = operbox.progress_of(name) else {
                    continue;
                };
                ops.push(AssignedOperator::from_progress(*name, progress));
                room_names.insert((*name).to_string());
            }
            let requires_karlan_control = assigned_names.contains("孑") && operbox.owns("灵知");
            if requires_karlan_control && !room_names.contains("灵知") && ops.len() < 5 {
                let op = operbox
                    .progress_of("灵知")
                    .map(|progress| AssignedOperator::from_progress("灵知", progress))
                    .unwrap_or_else(|| AssignedOperator::new("灵知", 2));
                ops.push(op);
                room_names.insert("灵知".to_string());
            }
            let requires_vina_control = VINA_TRADE_GROUP
                .iter()
                .all(|name| assigned_names.contains(*name))
                && operbox.owns(DAIFEEN);
            if requires_vina_control && !room_names.contains(DAIFEEN) && ops.len() < 5 {
                let op = operbox
                    .progress_of(DAIFEEN)
                    .map(|progress| AssignedOperator::from_progress(DAIFEEN, progress))
                    .unwrap_or_else(|| AssignedOperator::new(DAIFEEN, 2));
                ops.push(op);
                room_names.insert(DAIFEEN.to_string());
            }
            for name in active_names
                .iter()
                .filter(|n| system_ctrl_names.contains(*n))
            {
                if ops.len() >= 5 {
                    break;
                }
                if room_names.contains(name) || assigned_names.contains(name) {
                    continue;
                }
                let op = operbox
                    .progress_of(name)
                    .map(|progress| AssignedOperator::from_progress(name, progress))
                    .unwrap_or_else(|| AssignedOperator::new(name, 0));
                ops.push(op);
                room_names.insert(name.clone());
            }
            if !ops.is_empty() {
                a.set_room(RoomId::from("control"), ops);
            }
        };

        // 从队池分配中枢（池小不报错，有多少填多少）
        let assign_ctrl = |a: &mut BaseAssignment,
                           used: &mut HashSet<String>,
                           extra_control_pins: &[&str]| {
            pin_active_system_control(a, extra_control_pins);
            *used = a.operator_names();
            let mut final_pool = base_control_pool.clone();
            let present: HashSet<String> =
                final_pool.entries.iter().map(|e| e.name.clone()).collect();
            for op in a.control_operators() {
                if present.contains(&op.name) {
                    continue;
                }
                final_pool.entries.push(crate::pool::ControlPoolEntry {
                    name: op.name.clone(),
                    elite: op.elite,
                    progress: crate::roster::OperatorProgress::new(op.elite, op.level, op.rarity),
                    buff_ids: instances.resolve_control_buff_ids(&op.name, op.tier()),
                    tags: vec![],
                    tier: crate::layout::tier::OperatorTier::CrossStation,
                });
            }
            assign_control(a, &final_pool, table, &layout, &control_options, used)?;
            ensure_control_inject_coverage(a, &final_pool, &system_ctrl_names, &entry_by_name);

            let mut ops = a.control_operators();
            if ops.len() < 5 {
                let mut names: HashSet<String> = ops.iter().map(|o| o.name.clone()).collect();
                let assigned = a.operator_names();
                let mut entries = final_pool.entries.iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| {
                    control_efficiency_fill_sort_weight(b)
                        .partial_cmp(&control_efficiency_fill_sort_weight(a))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.name.cmp(&b.name))
                });
                for entry in entries {
                    if ops.len() >= 5 {
                        break;
                    }
                    if names.contains(&entry.name) || assigned.contains(&entry.name) {
                        continue;
                    }
                    ops.push(AssignedOperator::from_progress(&entry.name, entry.progress));
                    names.insert(entry.name.clone());
                }
                a.set_room(RoomId::from("control"), ops);
                *used = a.operator_names();
            }
            Ok::<(), Error>(())
        };

        let mut assignment;
        if index == 1 {
            // ── S2 双路径对比 ──
            // 路径 A: 无深海（基线）
            let mut base = shared.clone();
            for part in parts {
                merge_rooms(&mut base, part);
            }
            clear_room(&mut base, "control");
            let mut base_used = base.operator_names();
            assign_ctrl(&mut base, &mut base_used, &[])?;
            let mut base_warmup_room = warmup_sticky_trade_room.clone();
            align_trade_warmup_room(blueprint, &mut base, &mut base_warmup_room);
            let score_base =
                score_base_assignment(blueprint, &base, instances, table, hours, Some(durin_plan))?;

            // 路径 B: 有深海（S2 短班候选）
            let alpha_blocked_ops: HashSet<String> = operators_of(&alpha).into_iter().collect();
            let abyssal_candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
                operbox,
                instances,
                table,
                blueprint,
                layout: &production_layout,
                options,
                manu_pool: &pools.manu,
                used_ab: &used_ab,
                blocked_ops: &alpha_blocked_ops,
                shared: &shared,
                beta: &beta,
                gamma_h1: &gamma_h1,
                mutable_manu_rooms: &h1.manu,
            });
            let mut best_abyssal: Option<(AbyssalCandidate, ShiftScores, Option<RoomId>)> = None;
            for mut candidate in abyssal_candidates {
                clear_room(&mut candidate.assignment, "control");
                let mut aby_used = candidate.assignment.operator_names();
                assign_ctrl(&mut candidate.assignment, &mut aby_used, &[ABYSSAL_GLADIIA])?;
                let mut candidate_warmup_room = warmup_sticky_trade_room.clone();
                align_trade_warmup_room(
                    blueprint,
                    &mut candidate.assignment,
                    &mut candidate_warmup_room,
                );
                if !candidate
                    .assignment
                    .control_operators()
                    .iter()
                    .any(|o| o.name == ABYSSAL_GLADIIA)
                {
                    return Err(Error::msg("abyssal S2 candidate lost Gladiia control pin"));
                }
                let score_aby = score_base_assignment(
                    blueprint,
                    &candidate.assignment,
                    instances,
                    table,
                    hours,
                    Some(durin_plan),
                )?;
                let replace = best_abyssal
                    .as_ref()
                    .is_none_or(|(_, best, _)| score_aby.manu_prod_sum > best.manu_prod_sum);
                if replace {
                    best_abyssal = Some((candidate, score_aby, candidate_warmup_room));
                }
            }
            let (best_assignment, best_scores, best_warmup_room) =
                if let Some((candidate, score_aby, candidate_warmup_room)) = best_abyssal {
                    if score_aby.manu_prod_sum > score_base.manu_prod_sum {
                        let alpha_beta: HashSet<String> = teams
                            .iter()
                            .filter(|team| matches!(team.label, TeamLabel::Alpha | TeamLabel::Beta))
                            .flat_map(|team| team.operators.iter().cloned())
                            .collect();
                        if let Some(team) =
                            teams.iter_mut().find(|team| team.label == TeamLabel::Gamma)
                        {
                            let mut ops = team.operators.clone();
                            ops.extend(candidate.gamma_ops.clone());
                            ops.push(ABYSSAL_GLADIIA.to_string());
                            ops.sort();
                            ops.dedup();
                            ops.retain(|name| !alpha_beta.contains(name));
                            team.operators = ops;
                        }
                        (candidate.assignment, score_aby, candidate_warmup_room)
                    } else {
                        (base, score_base, base_warmup_room)
                    }
                } else {
                    (base, score_base, base_warmup_room)
                };
            assignment = best_assignment;
            let scores = best_scores;
            warmup_sticky_trade_room = best_warmup_room;
            let weighted_trade = scores.weighted_trade(hours);
            let weighted_manu = scores.weighted_manu(hours);
            let weighted_power = scores.weighted_power(hours);
            daily.trade += weighted_trade;
            daily.manu += weighted_manu;
            daily.power += weighted_power;
            shifts.push(TeamShiftResult {
                index,
                duration_hours: hours,
                active_teams: active.to_vec(),
                resting_team: resting,
                assignment,
                scores,
                weighted_trade,
                weighted_manu,
                weighted_power,
            });
        } else {
            // ── S1 / S3 正常组装 ──
            assignment = shared.clone();
            for part in parts {
                merge_rooms(&mut assignment, part);
            }
            clear_room(&mut assignment, "control");
            let mut s13_used = assignment.operator_names();
            assign_ctrl(&mut assignment, &mut s13_used, &[])?;
            align_trade_warmup_room(blueprint, &mut assignment, &mut warmup_sticky_trade_room);
            let scores = score_base_assignment(
                blueprint,
                &assignment,
                instances,
                table,
                hours,
                Some(durin_plan),
            )?;
            let weighted_trade = scores.weighted_trade(hours);
            let weighted_manu = scores.weighted_manu(hours);
            let weighted_power = scores.weighted_power(hours);
            daily.trade += weighted_trade;
            daily.manu += weighted_manu;
            daily.power += weighted_power;
            shifts.push(TeamShiftResult {
                index,
                duration_hours: hours,
                active_teams: active.to_vec(),
                resting_team: resting,
                assignment,
                scores,
                weighted_trade,
                weighted_manu,
                weighted_power,
            });
        }
    }
    let t4 = Instant::now();

    fn ms(a: Instant, b: Instant) -> f64 {
        a.duration_since(b).as_secs_f64() * 1000.0
    }

    eprintln!(
        "[计时] 轮换·peak班={:.2}ms  resolve+建池+切半={:.2}ms  γ替补={:.2}ms  三班评分={:.2}ms  轮换总计={:.2}ms",
        ms(t1, t0), ms(t2, t1), ms(t3, t2), ms(t4, t3), ms(t4, t0),
    );

    Ok(TeamRotationReport {
        peak_plan,
        teams,
        shifts,
        daily,
        elapsed: t4.duration_since(t0),
    })
}

/// 干员 → 所属队伍 的查表（输出层给每个设施打队伍标签用）。
pub fn operator_team_map(report: &TeamRotationReport) -> HashMap<String, TeamLabel> {
    let mut map = HashMap::new();
    for team in &report.teams {
        for op in &team.operators {
            map.entry(op.clone()).or_insert(team.label);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::layout::{assign_shift, blackkey_witch_same_trade_room};
    use crate::operbox::{
        default_operbox_full_e2_path, default_operbox_gongsun_path, OperBoxEntry,
    };
    use crate::skill_table::default_skill_table_path;

    fn fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap())
            .or_else(|_| OperBox::load(&default_operbox_gongsun_path().unwrap()))
            .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    fn build_test_manu_ctx(
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        instances: &OperatorInstances,
        table: &SkillTable,
    ) -> (LayoutContext, ManuPool, AssignBaseOptions) {
        let layout = resolve_base(
            blueprint,
            &BaseAssignment::default(),
            Some(instances),
            Some(table),
            24.0,
            None,
        )
        .unwrap()
        .layout_snapshot();
        let manu_pool =
            build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)
                .unwrap();
        (layout, manu_pool, AssignBaseOptions::default())
    }

    fn trade_room_contains(
        assignment: &BaseAssignment,
        blueprint: &BaseBlueprint,
        names: &[&str],
    ) -> bool {
        assignment.rooms.iter().any(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|bp| bp.kind == FacilityKind::TradePost)
                && names
                    .iter()
                    .all(|name| room.operators.iter().any(|op| op.name == *name))
        })
    }

    #[test]
    fn abyssal_candidate_does_not_materialize_empty_factory_rooms() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !["歌蕾蒂娅", "乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"]
            .iter()
            .all(|name| operbox.owns(name))
        {
            return;
        }
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);

        let shared = BaseAssignment::default();
        let beta = BaseAssignment::default();
        let mut gamma_h1 = BaseAssignment::default();
        gamma_h1.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("芬", 0),
                AssignedOperator::new("克洛丝", 0),
                AssignedOperator::new("泡普卡", 0),
            ],
        );

        let blocked_ops = HashSet::new();
        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1"), RoomId::from("manu_3")],
        });

        assert!(!candidates.is_empty());
        for candidate in candidates {
            for room in &candidate.assignment.rooms {
                let is_empty_factory = blueprint
                    .room(&room.room_id)
                    .is_some_and(|bp| bp.kind == FacilityKind::Factory)
                    && room.operators.is_empty();
                assert!(
                    !is_empty_factory,
                    "深海候选不应把未改动制造站写成显式空房: {:?}",
                    room
                );
            }
        }
    }

    #[test]
    fn abyssal_candidate_only_replaces_mutable_gamma_rooms() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !["歌蕾蒂娅", "乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"]
            .iter()
            .all(|name| operbox.owns(name))
        {
            return;
        }
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);

        let shared = BaseAssignment::default();
        let mut beta = BaseAssignment::default();
        beta.set_room(
            "manu_2",
            vec![
                AssignedOperator::new("清流", 2),
                AssignedOperator::new("温蒂", 2),
                AssignedOperator::new("森蚺", 2),
            ],
        );
        let mut gamma_h1 = BaseAssignment::default();
        gamma_h1.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("芬", 0),
                AssignedOperator::new("克洛丝", 0),
                AssignedOperator::new("泡普卡", 0),
            ],
        );
        gamma_h1.set_room(
            "manu_3",
            vec![
                AssignedOperator::new("斑点", 0),
                AssignedOperator::new("米格鲁", 0),
                AssignedOperator::new("玫兰莎", 0),
            ],
        );

        let blocked_ops = HashSet::new();
        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1"), RoomId::from("manu_3")],
        });

        assert!(!candidates.is_empty());
        for candidate in candidates {
            let beta_names: Vec<_> = candidate
                .assignment
                .operators_in(&RoomId::from("manu_2"))
                .iter()
                .map(|op| op.name.as_str())
                .collect();
            assert_eq!(
                beta_names,
                vec!["清流", "温蒂", "森蚺"],
                "深海候选不应替换 β/活跃体系制造房"
            );
        }
    }

    #[test]
    fn abyssal_candidate_does_not_fill_with_blocked_resting_ops() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !["歌蕾蒂娅", "乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"]
            .iter()
            .all(|name| operbox.owns(name))
        {
            return;
        }
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);

        let shared = BaseAssignment::default();
        let beta = BaseAssignment::default();
        let gamma_h1 = BaseAssignment::default();
        let blocked_ops: HashSet<String> = ["芬"].into_iter().map(str::to_string).collect();

        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1"), RoomId::from("manu_3")],
        });

        assert!(!candidates.is_empty());
        for candidate in candidates {
            assert!(
                !operators_of(&candidate.assignment).contains(&"芬".to_string()),
                "深海候选补位不应使用 S2 休息队员"
            );
        }
    }

    #[test]
    fn abyssal_candidate_accepts_original_hunters_at_any_tier_but_not_alternates() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(vec![
            OperBoxEntry {
                id: "gladiia".into(),
                name: "歌蕾蒂娅".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ulpian".into(),
                name: "乌尔比安".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "skadi".into(),
                name: "斯卡蒂".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ghost".into(),
                name: "幽灵鲨".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "angel".into(),
                name: "安哲拉".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ghost2".into(),
                name: "归溟幽灵鲨".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "fen".into(),
                name: "芬".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 3,
            },
            OperBoxEntry {
                id: "kroos".into(),
                name: "克洛丝".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 3,
            },
        ]);
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);
        let shared = BaseAssignment::default();
        let beta = BaseAssignment::default();
        let gamma_h1 = BaseAssignment::default();
        let blocked_ops = HashSet::new();

        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1"), RoomId::from("manu_3")],
        });

        assert!(
            !candidates.is_empty(),
            "原阵营四名深海猎人齐备时应进入 S2 深海候选"
        );
    }

    #[test]
    fn abyssal_candidate_runs_with_three_original_hunters() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(vec![
            OperBoxEntry {
                id: "gladiia".into(),
                name: "歌蕾蒂娅".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ulpian".into(),
                name: "乌尔比安".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "skadi".into(),
                name: "斯卡蒂".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "angel".into(),
                name: "安哲拉".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ghost2".into(),
                name: "归溟幽灵鲨".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
        ]);
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);
        let shared = BaseAssignment::default();
        let beta = BaseAssignment::default();
        let gamma_h1 = BaseAssignment::default();
        let blocked_ops = HashSet::new();

        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1")],
        });

        assert!(
            !candidates.is_empty(),
            "缺一名原阵营深海猎人时应进入降级 S2 深海候选"
        );
        for candidate in candidates {
            assert!(
                !operators_of(&candidate.assignment).contains(&"归溟幽灵鲨".to_string()),
                "归溟幽灵鲨不能代替本体幽灵鲨进入深海候选"
            );
        }
    }

    #[test]
    fn abyssal_candidate_requires_at_least_three_original_hunters() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let operbox = OperBox::from_entries(vec![
            OperBoxEntry {
                id: "gladiia".into(),
                name: "歌蕾蒂娅".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "skadi".into(),
                name: "斯卡蒂".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "angel".into(),
                name: "安哲拉".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "ghost2".into(),
                name: "归溟幽灵鲨".into(),
                elite: 2,
                level: 60,
                own: true,
                potential: 1,
                rarity: 6,
            },
        ]);
        let (layout, manu_pool, options) =
            build_test_manu_ctx(&blueprint, &operbox, &instances, &table);
        let shared = BaseAssignment::default();
        let beta = BaseAssignment::default();
        let gamma_h1 = BaseAssignment::default();
        let blocked_ops = HashSet::new();

        let candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
            operbox: &operbox,
            instances: &instances,
            table: &table,
            blueprint: &blueprint,
            layout: &layout,
            options: &options,
            manu_pool: &manu_pool,
            used_ab: &HashSet::new(),
            blocked_ops: &blocked_ops,
            shared: &shared,
            beta: &beta,
            gamma_h1: &gamma_h1,
            mutable_manu_rooms: &[RoomId::from("manu_1"), RoomId::from("manu_3")],
        });

        assert!(
            candidates.is_empty(),
            "两名原阵营深海猎人收益不足，不应进入 S2 深海候选"
        );
    }

    #[test]
    fn team_rotation_fills_every_facility_each_shift() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(report.shifts.len(), 3);
        assert_eq!(report.teams.len(), 3);

        let production_rooms: Vec<&RoomId> = blueprint
            .rooms
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                )
            })
            .map(|r| &r.id)
            .collect();

        // 关键：每班每个生产设施都满编，绝不空转。
        for shift in &report.shifts {
            for room_id in &production_rooms {
                let ops = shift.assignment.operators_in(room_id);
                assert!(
                    !ops.is_empty(),
                    "shift {} 设施 {} 空转",
                    shift.index + 1,
                    room_id.0
                );
            }
            // 每班内部无重复干员。
            let mut seen = HashSet::new();
            for room in &shift.assignment.rooms {
                for op in &room.operators {
                    assert!(
                        seen.insert(op.name.clone()),
                        "shift {} dup {}",
                        shift.index,
                        op.name
                    );
                }
            }
        }

        // 三队两两互斥。
        for i in 0..report.teams.len() {
            for j in (i + 1)..report.teams.len() {
                let a: HashSet<_> = report.teams[i].operators.iter().collect();
                let b: HashSet<_> = report.teams[j].operators.iter().collect();
                assert!(a.is_disjoint(&b), "teams {i} & {j} overlap");
            }
        }

        assert!((report.shifts[0].duration_hours - 12.0).abs() < f64::EPSILON);
        assert!(report.daily.trade > 0.0);
        assert!(report.daily.manu > 0.0);
    }

    #[test]
    fn team_rotation_control_center_operator_does_not_work_all_three_shifts() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let mut shifts_by_name: HashMap<String, HashSet<usize>> = HashMap::new();
        for shift in &report.shifts {
            for op in shift.assignment.control_operators() {
                shifts_by_name
                    .entry(op.name)
                    .or_default()
                    .insert(shift.index);
            }
        }

        for (name, shifts) in shifts_by_name {
            assert!(
                shifts.len() < 3,
                "中枢干员 {name} 不应三班连续上岗: shifts={shifts:?}"
            );
        }
    }

    #[test]
    fn team_rotation_control_center_respects_resting_team() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let team_by_name = operator_team_map(&report);

        for shift in &report.shifts {
            for op in shift.assignment.control_operators() {
                let team = team_by_name
                    .get(&op.name)
                    .copied()
                    .unwrap_or_else(|| panic!("中枢干员 {} 缺少 α/β/γ 归属", op.name));
                assert_ne!(
                    team,
                    shift.resting_team,
                    "shift {} 中枢干员 {} 属于休息队 {:?}",
                    shift.index + 1,
                    op.name,
                    shift.resting_team
                );
            }
        }
    }

    #[test]
    fn team_rotation_control_prefers_trade_manu_inject_over_resource_only_fillers() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let mut control_by_shift: Vec<Vec<String>> = Vec::new();
        for shift in &report.shifts {
            control_by_shift.push(
                shift
                    .assignment
                    .control_operators()
                    .into_iter()
                    .map(|op| op.name)
                    .collect(),
            );
        }

        let trade_injectors = ["阿米娅", "诗怀雅", "明椒", "阿斯卡纶", "望", "火龙S黑角"];
        let manu_injectors = ["斩业星熊", "Mon3tr", "凯尔希", "布丁", "麒麟R夜刀"];
        for names in &control_by_shift {
            assert_eq!(
                names.len(),
                5,
                "每班中枢仍应满编 5 人: {:?}",
                control_by_shift
            );
            assert!(
                names
                    .iter()
                    .any(|name| trade_injectors.contains(&name.as_str())),
                "每班中枢应优先包含全贸易订单效率注入: {:?}",
                control_by_shift
            );
            assert!(
                names
                    .iter()
                    .any(|name| manu_injectors.contains(&name.as_str())),
                "每班中枢应优先包含全制造生产力注入: {:?}",
                control_by_shift
            );
        }
        for resource_only in ["三角初华", "战车", "闪击"] {
            assert!(
                control_by_shift
                    .iter()
                    .all(|names| names.iter().all(|name| name != resource_only)),
                "{resource_only} 不应作为无当前消费方的中枢补位进入班次: {:?}",
                control_by_shift
            );
        }
    }

    #[test]
    fn team_rotation_abyssal_only_runs_in_s2_short_shift() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !operbox.owns("歌蕾蒂娅") || !operbox.owns("乌尔比安") {
            return;
        }
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let abyssal_names: HashSet<&str> = ["乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"]
            .into_iter()
            .collect();

        let mut shifts_with_abyssal_manu = Vec::new();
        for shift in &report.shifts {
            let has_abyssal_manu = shift.assignment.rooms.iter().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|bp| bp.kind == FacilityKind::Factory)
                    && room
                        .operators
                        .iter()
                        .any(|op| abyssal_names.contains(op.name.as_str()))
            });
            if has_abyssal_manu {
                shifts_with_abyssal_manu.push(shift.index);
                assert_eq!(shift.index, 1, "深海链只允许 S2 6h 短班");
                assert!(
                    shift
                        .assignment
                        .control_operators()
                        .iter()
                        .any(|op| op.name == "歌蕾蒂娅"),
                    "S2 深海候选必须保留歌蕾蒂娅中枢"
                );
                let resting = report
                    .teams
                    .iter()
                    .find(|team| team.label == shift.resting_team)
                    .unwrap();
                for room in &shift.assignment.rooms {
                    let is_factory = blueprint
                        .room(&room.room_id)
                        .is_some_and(|bp| bp.kind == FacilityKind::Factory);
                    if is_factory
                        && room
                            .operators
                            .iter()
                            .any(|op| abyssal_names.contains(op.name.as_str()))
                    {
                        assert!(
                            room.operators
                                .iter()
                                .all(|op| op.name != "冬时" && op.name != "温蒂"),
                            "自动化组体系专用干员不应作为深海制造散件: {:?}",
                            room.operators
                        );
                    }
                    for op in &room.operators {
                        assert!(
                            !resting.operators.contains(&op.name),
                            "S2 上岗干员 {} 不应属于休息队 {:?}",
                            op.name,
                            shift.resting_team
                        );
                    }
                }
            }
        }

        assert!(
            shifts_with_abyssal_manu.is_empty() || shifts_with_abyssal_manu == vec![1],
            "深海制造只应出现在 S2: {shifts_with_abyssal_manu:?}"
        );
    }

    #[test]
    fn team_rotation_assignments_do_not_use_resting_team_members() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let team_by_name = operator_team_map(&report);

        for shift in &report.shifts {
            for room in &shift.assignment.rooms {
                if blueprint.room(&room.room_id).is_some_and(|bp| {
                    matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office)
                }) {
                    continue;
                }
                for op in &room.operators {
                    let team = team_by_name
                        .get(&op.name)
                        .copied()
                        .unwrap_or_else(|| panic!("上岗干员 {} 缺少 α/β/γ 归属", op.name));
                    assert_ne!(
                        team,
                        shift.resting_team,
                        "shift {} 房间 {} 使用了休息队 {:?} 干员 {}",
                        shift.index + 1,
                        room.room_id.0,
                        shift.resting_team,
                        op.name
                    );
                }
            }
        }
    }

    #[test]
    fn team_rotation_carries_peak_plan() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.peak_plan.mode, AssignShiftMode::Peak);
        if operbox.owns("伺夜") && operbox.owns("贝洛内") {
            assert!(
                report
                    .peak_plan
                    .registry_system_ids()
                    .contains(&"syracusa_pair"),
                "peak_plan 应含叙拉古同站 meta: {:?}",
                report.peak_plan.registry_system_ids()
            );
        }
    }

    #[test]
    fn team_rotation_carries_peak_blackkey_trade_station() {
        let (blueprint, operbox, instances, table) = fixtures();
        if !operbox.owns("黑键") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        let peak_has_blackkey = peak.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "黑键")
        });
        if !peak_has_blackkey {
            return;
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let blackkey_in_rotation = report.shifts.iter().any(|shift| {
            shift.assignment.rooms.iter().any(|room| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == room.room_id && b.kind == FacilityKind::TradePost)
                    && room.operators.iter().any(|o| o.name == "黑键")
            })
        });
        assert!(
            blackkey_in_rotation,
            "peak 已认领黑键贸站时 team-rotation 应保留"
        );
        assert!(
            report.teams[0]
                .operators
                .iter()
                .chain(report.teams[1].operators.iter())
                .any(|n| n == "黑键"),
            "黑键应在 α 或 β 队: alpha={:?} beta={:?}",
            report.teams[0].operators,
            report.teams[1].operators
        );
        for shift in &report.shifts {
            assert!(
                !blackkey_witch_same_trade_room(&shift.assignment, &blueprint),
                "shift {} 黑键与巫恋不得同房",
                shift.index + 1
            );
        }
    }

    #[test]
    fn team_rotation_rosemary_blackkey_shift_bind() {
        use crate::layout::build_plan;
        use crate::schedule::shift_bind::{
            shift_binds_from_plan, team_of_operator, verify_shift_binds,
        };

        let (blueprint, operbox, instances, table) = fixtures();
        if !operbox.owns("迷迭香") || !operbox.owns("黑键") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        assert!(
            peak.rooms
                .iter()
                .any(|r| r.operators.iter().any(|o| o.name == "迷迭香")),
            "迷迭香链激活时 peak 应含迷迭香"
        );
        assert!(
            peak.rooms
                .iter()
                .any(|r| r.operators.iter().any(|o| o.name == "黑键")),
            "迷迭香链激活时 peak 应含黑键"
        );

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let binds = shift_binds_from_plan(
            &build_plan(
                &blueprint,
                &operbox,
                AssignShiftMode::Peak,
                &BaseAssignment::default(),
                &std::collections::HashSet::new(),
            )
            .unwrap(),
        );
        verify_shift_binds(&report, &binds, &peak).expect("迷迭香+黑键 应同上同下、上2休1");
        let team = team_of_operator(&report, "迷迭香").unwrap();
        assert_eq!(
            team_of_operator(&report, "黑键"),
            Some(team),
            "迷迭香与黑键应同队"
        );
    }

    #[test]
    fn team_rotation_docus_and_blackkey_closure_share_12h_shift() {
        use crate::operbox::default_operbox_full_e2_path;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["但书", "伺夜", "贝洛内", "可露希尔", "黑键", "吉星"] {
            if !operbox.owns(name) {
                return;
            }
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let shift1 = &report.shifts[0].assignment;
        let trade_rooms: Vec<_> = shift1
            .rooms
            .iter()
            .filter(|r| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
            })
            .collect();
        assert!(
            trade_rooms.iter().any(|r| {
                ["但书", "伺夜", "贝洛内"]
                    .iter()
                    .all(|name| r.operators.iter().any(|o| o.name == *name))
            }),
            "12h 班应包含但书站: {:?}",
            trade_rooms
        );
        assert!(
            trade_rooms.iter().any(|r| {
                ["可露希尔", "黑键", "吉星"]
                    .iter()
                    .all(|name| r.operators.iter().any(|o| o.name == *name))
            }),
            "12h 班应包含可露希尔黑键吉星站: {:?}",
            trade_rooms
        );
    }

    #[test]
    fn team_rotation_vina_trade_shift_pins_daifeen_control() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let base = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = base.excluding(&HashSet::from([
            "龙舌兰".to_string(),
            "可露希尔".to_string(),
            "但书".to_string(),
        ]));
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in [DAIFEEN, "推进之王", "摩根", "维娜·维多利亚"] {
            if !operbox.owns(name) {
                return;
            }
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
        )
        .unwrap();

        let mut checked = 0;
        for shift in &report.shifts {
            let has_vina_trade = trade_room_contains(
                &shift.assignment,
                &blueprint,
                &["推进之王", "摩根", "维娜·维多利亚"],
            );
            if has_vina_trade {
                checked += 1;
                assert!(
                    shift
                        .assignment
                        .control_operators()
                        .iter()
                        .any(|op| op.name == DAIFEEN),
                    "shift {} 推王组上站时中枢必须同步戴菲恩: {:?}",
                    shift.index + 1,
                    shift.assignment.control_operators()
                );
            }
        }
        assert!(checked > 0, "ban 三核心后轮换应至少出现一班推王组");
    }

    #[test]
    fn team_rotation_feedback_000142_docus_lv2_prefers_higher_tool_over_vina() {
        use crate::operbox::default_operbox_full_e2_path;

        let blueprint =
            BaseBlueprint::load(&crate::skill_table::data_path("layout/252.json").unwrap())
                .unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
        )
        .unwrap();

        for shift in &report.shifts {
            for room in &shift.assignment.rooms {
                let Some(bp_room) = blueprint.room(&room.room_id) else {
                    continue;
                };
                if bp_room.kind != FacilityKind::TradePost {
                    continue;
                }
                let names: Vec<_> = room.operators.iter().map(|op| op.name.as_str()).collect();
                assert!(
                    !(names.contains(&"但书") && names.contains(&"维娜·维多利亚")),
                    "shift {} 但书二级站不应选择退化态维娜: {:?}",
                    shift.index + 1,
                    names
                );
            }
        }
    }

    #[test]
    fn team_rotation_three_trade_keeps_karlan_as_fourth_trade_meta() {
        let mut blueprint = BaseBlueprint::template_snhunt().unwrap();
        blueprint.scenario.initial_global.clear();
        let base = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = base.excluding(&HashSet::from(["八幡海铃".to_string(), "黑键".to_string()]));
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(
            report.shifts.iter().any(|shift| {
                shift
                    .assignment
                    .control_operators()
                    .iter()
                    .any(|op| op.name == "灵知")
                    && trade_room_contains(&shift.assignment, &blueprint, &["孑", "银灰"])
            }),
            "3 贸易轮换应在前三 meta 之外保留喀兰市井孑站: {:?}",
            report.shifts
        );
        assert_eq!(
            operator_team_map(&report).get("灵知"),
            operator_team_map(&report).get("孑"),
            "灵知中枢应跟随喀兰贸易站同队"
        );
    }

    #[test]
    fn team_rotation_feedback_010457_witch_group_keeps_trade_room() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["巫恋", "龙舌兰"] {
            if !operbox.owns(name) {
                return;
            }
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
        )
        .unwrap();

        let mut witch_rooms = Vec::new();
        for shift in &report.shifts {
            if let Some(room_id) = trade_room_containing_group(
                &shift.assignment,
                &blueprint,
                &WARMUP_STICKY_TRADE_OPERATORS,
            ) {
                witch_rooms.push((shift.index, room_id));
            }
        }
        assert!(
            witch_rooms.len() >= 2,
            "feedback operbox should schedule witch group in short shifts: {witch_rooms:?}"
        );
        let first_room = witch_rooms[0].1.clone();
        assert!(
            witch_rooms.iter().all(|(_, room)| *room == first_room),
            "巫恋/龙舌兰组跨班不应变更贸易站: {witch_rooms:?}"
        );
    }

    /// γ 队贸易站不得抢占 peak α/β 已认领的 meta 干员（如但书、巫恋）。
    #[test]
    fn team_rotation_gamma_trade_disjoint_from_peak_meta() {
        const META_TRADE_OPS: &[&str] = &["但书", "巫恋", "龙舌兰", "可露希尔"];

        let (blueprint, operbox, instances, table) = fixtures();
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();

        let peak_meta: HashSet<String> = peak
            .rooms
            .iter()
            .filter(|r| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
            })
            .flat_map(|r| r.operators.iter().map(|o| o.name.clone()))
            .filter(|n| META_TRADE_OPS.contains(&n.as_str()))
            .collect();
        if peak_meta.is_empty() {
            return;
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let gamma_ops: HashSet<_> = report.teams[2].operators.iter().cloned().collect();
        for name in peak_meta {
            assert!(
                !gamma_ops.contains(&name),
                "γ 队不应含 peak meta 干员 {name}"
            );
        }
    }
}
