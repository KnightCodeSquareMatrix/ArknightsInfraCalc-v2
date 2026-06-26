use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_control, assign_shift_with_plan_skip, assign_team_gamma_half, pinned_assignment,
    resolve_base, ActivatedSystem, AssignBaseOptions, AssignShiftMode, AssignedOperator,
    AssignmentPlan, BaseAssignment, BaseBlueprint, FacilityKind, RoomId, RoomProduct, SlotFill,
};
use crate::manufacture::{solve_manufacture, ManuOperator, ManuRoomInput};
use crate::operbox::OperBox;
use crate::pool::{
    build_control_pool, build_manufacture_pool, build_power_pool, build_trade_pool,
    filter_general_manufacture_search_pool, ManuPool,
};
use crate::search::control_entry_plugin_fill;
use crate::skill_table::SkillTable;

use super::base_rotation::{score_base_assignment, ShiftScores};
use super::shift_bind::align_shift_binds_in_halves;

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
        target.set_room_assignment(room.clone());
    }
}

// ── 深海链 S2 短班入口 ──

const ABYSSAL_GLADIIA: &str = "歌蕾蒂娅";
const ABYSSAL_HUNTERS: [&str; 4] = ["乌尔比安", "斯卡蒂", "幽灵鲨", "安哲拉"];

struct AbyssalCandidate {
    assignment: BaseAssignment,
    gamma_ops: Vec<String>,
}

/// 构造 S2 深海短班候选：2+2 深海猎手、歌蕾蒂娅进中枢、每房按求解器补最佳第三人。
/// 歌蕾蒂娅只能承受约 7h 短班；深海链不进入普通 base_systems registry。
struct AbyssalBuildCtx<'a> {
    operbox: &'a OperBox,
    blueprint: &'a BaseBlueprint,
    instances: &'a OperatorInstances,
    table: &'a SkillTable,
    pools: &'a ProductionPools,
    used_ab: &'a HashSet<String>,
    shared: &'a BaseAssignment,
    beta: &'a BaseAssignment,
    gamma_h1: &'a BaseAssignment,
    mood: f64,
    durin_plan: Option<u8>,
}

fn build_abyssal_s2_candidate(ctx: &AbyssalBuildCtx<'_>) -> Option<AbyssalCandidate> {
    let gladiia_elite = ctx.operbox.elite_of(ABYSSAL_GLADIIA)?;
    if gladiia_elite < 2 || ctx.used_ab.contains(ABYSSAL_GLADIIA) {
        return None;
    }

    let mut hunters = Vec::new();
    for name in ABYSSAL_HUNTERS {
        if ctx.operbox.elite_of(name).is_some() && !ctx.used_ab.contains(name) {
            hunters.push(name.to_string());
        }
    }
    if hunters.len() < 4 {
        return None;
    }

    let manu_rooms: Vec<RoomId> = ctx
        .blueprint
        .rooms_of(FacilityKind::Factory)
        .iter()
        .filter(|r| ctx.gamma_h1.operators_in(&r.id).len() == 3)
        .take(2)
        .map(|r| r.id.clone())
        .collect();
    if manu_rooms.len() < 2 {
        return None;
    }

    let mut candidate = ctx.shared.clone();
    candidate.set_room(
        RoomId::from("control"),
        abyssal_control_room(ctx.shared, gladiia_elite)?,
    );
    merge_rooms(&mut candidate, ctx.beta);
    let mut gamma_ops = Vec::new();

    for i in 0..2 {
        let first = ctx.operbox.progress_of(&hunters[i * 2])?;
        let second = ctx.operbox.progress_of(&hunters[i * 2 + 1])?;
        let mut ops = vec![
            AssignedOperator::from_progress(&hunters[i * 2], first),
            AssignedOperator::from_progress(&hunters[i * 2 + 1], second),
        ];
        candidate.set_room(manu_rooms[i].clone(), ops.clone());

        let layout = resolve_base(
            ctx.blueprint,
            &candidate,
            Some(ctx.instances),
            Some(ctx.table),
            ctx.mood,
            ctx.durin_plan,
        )
        .ok()?
        .layout_snapshot();
        let used = candidate.operator_names();
        let filler = best_abyssal_room_filler(
            ctx.blueprint,
            &manu_rooms[i],
            &ops,
            ctx.instances,
            &ctx.pools.manu,
            ctx.table,
            &layout,
            ctx.mood,
            &used,
            ctx.used_ab,
        )?;
        ops.push(filler);
        gamma_ops.extend(ops.iter().map(|op| op.name.clone()));
        candidate.set_room(manu_rooms[i].clone(), ops);
    }

    for room in &ctx.gamma_h1.rooms {
        if !manu_rooms.contains(&room.room_id) {
            candidate.set_room_assignment(room.clone());
            gamma_ops.extend(room.operators.iter().map(|op| op.name.clone()));
        }
    }
    gamma_ops.sort();
    gamma_ops.dedup();

    Some(AbyssalCandidate {
        assignment: candidate,
        gamma_ops,
    })
}

fn abyssal_control_room(
    shared: &BaseAssignment,
    gladiia_elite: u8,
) -> Option<Vec<AssignedOperator>> {
    let mut control = shared.control_operators();
    if control.iter().any(|o| o.name == ABYSSAL_GLADIIA) {
        return Some(control);
    }
    if control.len() >= 5 {
        let drop = control.iter().position(|o| o.name == "焰尾")?;
        control[drop] = AssignedOperator::new(ABYSSAL_GLADIIA, gladiia_elite);
    } else {
        control.push(AssignedOperator::new(ABYSSAL_GLADIIA, gladiia_elite));
    }
    Some(control)
}

fn best_abyssal_room_filler(
    blueprint: &BaseBlueprint,
    room_id: &RoomId,
    pinned: &[AssignedOperator],
    instances: &OperatorInstances,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &crate::layout::LayoutContext,
    mood: f64,
    used: &HashSet<String>,
    used_ab: &HashSet<String>,
) -> Option<AssignedOperator> {
    let room = blueprint.room(room_id)?;
    let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
        return None;
    };
    let pinned_names: HashSet<&str> = pinned.iter().map(|op| op.name.as_str()).collect();

    let pool = filter_general_manufacture_search_pool(pool);
    pool.entries
        .iter()
        .filter(|entry| {
            !used.contains(&entry.name)
                && !used_ab.contains(&entry.name)
                && !pinned_names.contains(entry.name.as_str())
        })
        .filter_map(|entry| {
            let mut operators = pinned
                .iter()
                .filter_map(|op| assigned_to_manu_operator(op, instances))
                .collect::<Vec<_>>();
            if operators.len() != pinned.len() {
                return None;
            }
            operators.push(entry.to_manu_operator());
            let input = ManuRoomInput {
                level: room.level,
                operators,
                active_recipe: *recipe,
                mood,
                layout: std::sync::Arc::new(layout.clone()),
            };
            let score = solve_manufacture(&input, table).ok()?.prod_total;
            Some((score, entry.name.clone(), entry.progress))
        })
        .max_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.1.cmp(&a.1))
        })
        .map(|(_, name, progress)| AssignedOperator::from_progress(name, progress))
}

fn assigned_to_manu_operator(
    op: &AssignedOperator,
    instances: &OperatorInstances,
) -> Option<ManuOperator> {
    let tier = op.tier();
    let tags = instances.tags_for(&op.name, tier);
    Some(ManuOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids: instances.resolve_manufacture_buff_ids(&op.name, tier),
        tags,
    })
}

// ── 深海链短班入口结束 ──

// ── 中枢队伍归属 ──

/// 从编排计划中提取体系绑定的中枢干员名（含 PickOne 候选人）。
fn system_control_operators(plan: &AssignmentPlan) -> HashSet<String> {
    let mut names = HashSet::new();
    for sys in &plan.activated {
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

    // 2) 以脚手架解算 layout（中枢 buff 生效），供生产搜索。
    let layout = resolve_base(
        blueprint,
        &shared,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    let pools = ProductionPools {
        trade: build_trade_pool(&operbox.trade_roster(instances), instances, table)?,
        manu: build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?,
        power: build_power_pool(&operbox.power_roster(instances), instances, table)?,
    };
    let control_pool = build_control_pool(&operbox.control_roster(instances), instances, table)?;

    let [mut h1, mut h2] = split_production_facilities(blueprint);
    align_shift_binds_in_halves(&peak, operbox, &mut h1, &mut h2);

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
        &layout,
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
        &layout,
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
    for entry in &control_pool.entries {
        if team_ctrl.values().any(|names| names.contains(&entry.name)) {
            continue;
        }
        if !control_rotation_candidate(entry, table, &layout, options.mood) {
            continue;
        }
        let team = production_team_by_name
            .get(&entry.name)
            .copied()
            .unwrap_or_else(|| {
                TeamLabel::ALL
                    .iter()
                    .min_by_key(|t| team_ctrl.get(t).map(|v| v.len()).unwrap_or(0))
                    .copied()
                    .unwrap()
            });
        team_ctrl.entry(team).or_default().push(entry.name.clone());
    }
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
        let pin_active_system_control = |a: &mut BaseAssignment| {
            let mut ops = a.control_operators();
            let mut room_names: HashSet<String> = ops.iter().map(|o| o.name.clone()).collect();
            let assigned_names = a.operator_names();
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
        let assign_ctrl = |a: &mut BaseAssignment, used: &mut HashSet<String>| {
            pin_active_system_control(a);
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

            let mut ops = a.control_operators();
            if ops.len() < 5 {
                let mut names: HashSet<String> = ops.iter().map(|o| o.name.clone()).collect();
                let assigned = a.operator_names();
                for entry in &final_pool.entries {
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
            let mut base_used = base.operator_names();
            assign_ctrl(&mut base, &mut base_used)?;
            let score_base =
                score_base_assignment(blueprint, &base, instances, table, hours, Some(durin_plan))?;

            // 路径 B: 有深海（S2 短班候选）
            let abyssal = build_abyssal_s2_candidate(&AbyssalBuildCtx {
                operbox,
                blueprint,
                instances,
                table,
                pools: &pools,
                used_ab: &used_ab,
                shared: &shared,
                beta: &beta,
                gamma_h1: &gamma_h1,
                mood: options.mood,
                durin_plan: Some(durin_plan),
            });
            let (best_assignment, best_scores) = if let Some(mut candidate) = abyssal {
                let mut aby_used = candidate.assignment.operator_names();
                assign_ctrl(&mut candidate.assignment, &mut aby_used)?;
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
                if score_aby.manu_prod_sum > score_base.manu_prod_sum {
                    if let Some(team) = teams.iter_mut().find(|team| team.label == TeamLabel::Gamma)
                    {
                        let mut ops = candidate.gamma_ops.clone();
                        ops.push(ABYSSAL_GLADIIA.to_string());
                        ops.extend(
                            team_ctrl
                                .get(&TeamLabel::Gamma)
                                .cloned()
                                .unwrap_or_default(),
                        );
                        ops.sort();
                        ops.dedup();
                        team.operators = ops;
                    }
                    (candidate.assignment, score_aby)
                } else {
                    (base, score_base)
                }
            } else {
                (base, score_base)
            };
            assignment = best_assignment;
            let scores = best_scores;
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
            let mut s13_used = assignment.operator_names();
            assign_ctrl(&mut assignment, &mut s13_used)?;
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
    use crate::operbox::{default_operbox_full_e2_path, default_operbox_gongsun_path};
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
        if operbox.owns("但书") {
            assert!(
                report
                    .peak_plan
                    .registry_system_ids()
                    .contains(&"docus_syracusa"),
                "peak_plan 应含但书链: {:?}",
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
        use crate::schedule::shift_bind::{team_of_operator, verify_shift_binds};

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

        verify_shift_binds(&report, &operbox, &peak).expect("迷迭香+黑键 应同上同下、上2休1");
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
