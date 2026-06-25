use std::collections::HashSet;
use std::time::Instant;

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
use crate::layout::orchestrate::{build_plan, execute_plan, AssignmentPlan};
use crate::layout::resolve::resolve_base;
use crate::layout::shift::AssignShiftMode;
use crate::layout::LayoutContext;
use crate::manufacture::input::ManuSearchRecipeMode;
use crate::operbox::OperBox;
use crate::pool::{
    add_jie_market_to_trade_pool, build_control_pool, build_manufacture_pool, build_power_pool,
    build_trade_pool, filter_manufacture_pool, filter_trade_pool, jie_e0_trade_operator,
    karlan_precision_active, try_filter_standalone, ControlPool, ManuPool, PowerPool, TradePool,
    JIE_TRADE_NAME,
};
use crate::search::{
    control_entry_plugin_fill, hit_witch_shortcut, pick_docus_trade_hit, pick_trade_role_hit,
    search_control_combos, search_manufacture_triples, search_power_assignment,
    search_trade_triples, search_trade_triples_filtered, ControlFillPolicy, ControlSearchOptions,
    ManuSearchHit, ManuSearchOptions, PowerSearchOptions, SearchTripleFilter, TradeSearchHit,
    TradeSearchOptions, MATATABI_CONSUMER_NAME,
};
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::types::RecipeKind;

const SENXI_DORM_CUISINE_BUFF: &str = "dorm_rec_bd_dungeon[000]";

fn ms(a: Instant, b: Instant) -> f64 {
    a.duration_since(b).as_secs_f64() * 1000.0
}

#[derive(Debug, Clone)]
pub struct AssignBaseOptions {
    pub top_k: usize,
    pub mood: f64,
    pub shift_hours: f64,
    /// 中枢分配时跳过 standalone_roster 白名单过滤（轮换编排中体系绑定干员可能不在白名单内）。
    pub skip_standalone_control: bool,
}

impl Default for AssignBaseOptions {
    fn default() -> Self {
        Self {
            top_k: 20,
            mood: 24.0,
            shift_hours: 24.0,
            skip_standalone_control: false,
        }
    }
}

/// 全基建单班进驻编制：producer 落位 → resolve → consumer 搜 + `used` 顺序认领。
pub fn assign_base_greedy(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<BaseAssignment> {
    assign_shift(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
    )
}

/// `assign_shift` 完整输出：编制 + 编排计划（轮换层只读 plan，不重跑 `build_plan`）。
#[derive(Debug, Clone)]
pub struct AssignShiftResult {
    pub assignment: BaseAssignment,
    pub plan: AssignmentPlan,
}

/// 单班进驻；`seed` 非空时保留已钉死房间（中枢/宿舍），仅补贸易/制造/发电。
pub fn assign_shift(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<BaseAssignment> {
    Ok(
        assign_shift_with_plan(blueprint, operbox, instances, table, options, mode, seed)?
            .assignment,
    )
}

/// 从编排计划提取 claimed 干员名并按 tier 标注池条目。
fn tag_pool_from_plan<T: crate::pool::HasName + crate::pool::TierTagged>(
    plan: &AssignmentPlan,
    pool: &mut crate::pool::PoolCore<T>,
) {
    for claim in &plan.registry_claims {
        let mut names = HashSet::new();
        for slot in &claim.slots {
            for op in &slot.operators {
                names.insert(op.name.clone());
            }
        }
        pool.tag_tier(&names, claim.tier);
    }
}

/// 同 [`assign_shift`]，额外返回编排 `AssignmentPlan`（peak 班供 αβγ 轮换只读）。
pub fn assign_shift_with_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
) -> Result<AssignShiftResult> {
    assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        options,
        mode,
        seed,
        &HashSet::new(),
    )
}

/// 同 [`assign_shift_with_plan`]，额外允许跳过指定体系。
pub fn assign_shift_with_plan_skip(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &HashSet<String>,
) -> Result<AssignShiftResult> {
    let t0 = Instant::now();
    blueprint.validate()?;

    let mut skip_system_ids = skip_system_ids.clone();
    skip_trade_core_registry_systems(&mut skip_system_ids);
    let plan = build_plan(blueprint, operbox, mode, seed, &skip_system_ids)?;
    let t1 = Instant::now();

    let executed = execute_plan(blueprint, operbox, table, &plan, seed)?;
    let mut assignment = executed.assignment;
    let mut used = executed.used;
    let t2 = Instant::now();

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let producer_layout = resolve_base(
        blueprint,
        &assignment,
        Some(instances),
        None,
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();
    let t3 = Instant::now();

    if mode == AssignShiftMode::Peak && assignment.control_operators().len() < 5 {
        let mut control_pool =
            build_control_pool(&operbox.control_roster(instances), instances, table)?;
        tag_pool_from_plan(&plan, &mut control_pool);
        assign_control(
            &mut assignment,
            &control_pool,
            table,
            &producer_layout,
            options,
            &mut used,
        )?;
    }
    let t4 = Instant::now();

    if mode == AssignShiftMode::Peak {
        assign_perception_producers(blueprint, operbox, &mut assignment, &mut used)?;
        assign_dorm_producers(blueprint, operbox, instances, &mut assignment, &mut used)?;
    }
    let t5 = Instant::now();

    let layout = resolve_base(
        blueprint,
        &assignment,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();
    let t6 = Instant::now();

    let mut trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    if karlan_precision_active(&layout.global_inject) {
        add_jie_market_to_trade_pool(&mut trade_pool, instances, table);
    }
    let mut manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let mut power_pool = build_power_pool(&operbox.power_roster(instances), instances, table)?;
    tag_pool_from_plan(&plan, &mut trade_pool);
    tag_pool_from_plan(&plan, &mut manu_pool);
    tag_pool_from_plan(&plan, &mut power_pool);
    let gold_lines = blueprint.gold_manu_line_count();
    let t7 = Instant::now();

    match mode {
        AssignShiftMode::Peak => {
            assign_power_stations(
                blueprint,
                &power_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t8 = Instant::now();
            let trade_layout = resolve_base(
                blueprint,
                &assignment,
                Some(instances),
                Some(table),
                options.mood,
                Some(durin_plan),
            )?
            .layout_snapshot();
            let t9 = Instant::now();
            assign_trade_remainder(
                blueprint,
                &trade_pool,
                table,
                &trade_layout,
                gold_lines,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t10 = Instant::now();
            let manu_layout = resolve_base(
                blueprint,
                &assignment,
                Some(instances),
                Some(table),
                options.mood,
                Some(durin_plan),
            )?
            .layout_snapshot();
            let t11 = Instant::now();
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &manu_layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t12 = Instant::now();

            eprintln!(
                "[计时] 编排选型={:.2}ms  编排落位={:.2}ms  resolve(1st)={:.2}ms  中枢={:.2}ms  perception+dorm={:.2}ms  resolve(2nd)={:.2}ms  建池={:.2}ms  \
                 发电={:.2}ms  resolve(3rd)={:.2}ms  贸易余站={:.2}ms  resolve(4th)={:.2}ms  制造={:.2}ms  单班总计={:.2}ms",
                ms(t1, t0), ms(t2, t1), ms(t3, t2), ms(t4, t3), ms(t5, t4),
                ms(t6, t5), ms(t7, t6), ms(t8, t7), ms(t9, t8), ms(t10, t9),
                ms(t11, t10), ms(t12, t11), ms(t12, t0),
            );
        }
        AssignShiftMode::Recovery => {
            assign_trade_jie_remainder(
                blueprint,
                &trade_pool,
                table,
                instances,
                &layout,
                gold_lines,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t8 = Instant::now();
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t9 = Instant::now();
            assign_power_stations(
                blueprint,
                &power_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            let t10 = Instant::now();

            eprintln!(
                "[计时] 编排选型={:.2}ms  编排落位={:.2}ms  resolve(1st)={:.2}ms  中枢={:.2}ms  perception+dorm={:.2}ms  resolve(2nd)={:.2}ms  建池={:.2}ms  \
                  trade孑余站={:.2}ms  制造={:.2}ms  发电={:.2}ms  单班总计={:.2}ms",
                ms(t1, t0), ms(t2, t1), ms(t3, t2), ms(t4, t3), ms(t5, t4),
                ms(t6, t5), ms(t7, t6), ms(t8, t7), ms(t9, t8), ms(t10, t9),
                ms(t10, t0),
            );
        }
    }

    Ok(AssignShiftResult { assignment, plan })
}

/// 编制内所有上岗干员。
pub fn assignment_operator_names(assignment: &BaseAssignment) -> HashSet<String> {
    assignment.operator_names()
}

/// 贸易 / 制造 / 发电岗位干员（跨班互斥池）。
pub fn rotating_workers(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> HashSet<String> {
    let rotating_kinds = [
        FacilityKind::TradePost,
        FacilityKind::Factory,
        FacilityKind::PowerPlant,
    ];
    let mut names = HashSet::new();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !rotating_kinds.contains(&bp.kind) {
            continue;
        }
        for op in &room.operators {
            names.insert(op.name.clone());
        }
    }
    names
}

/// 宿舍 + 办公室感知 producer（三班钉死，从高峰班拷贝）。
///
/// 中枢在 `schedule_team_rotation` 中按 αβγ 队伍轮休重分配，不在这里钉死。
pub fn pinned_assignment(assignment: &BaseAssignment, blueprint: &BaseBlueprint) -> BaseAssignment {
    let mut pinned = BaseAssignment::default();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office) {
            continue;
        }
        if room.operators.is_empty() {
            continue;
        }
        pinned.set_room(room.room_id.clone(), room.operators.clone());
    }
    pinned
}

fn assignment_has_matatabi_consumer(assignment: &BaseAssignment) -> bool {
    assignment.rooms.iter().any(|room| {
        room.operators
            .iter()
            .any(|op| op.name == MATATABI_CONSUMER_NAME)
    })
}

pub(crate) fn assign_control(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    used: &mut HashSet<String>,
) -> Result<()> {
    const MAX_CONTROL: usize = 5;
    if pool.entries.is_empty() {
        return Ok(());
    }
    let pinned: HashSet<String> = assignment
        .control_operators()
        .into_iter()
        .map(|o| o.name)
        .collect();
    if pinned.len() >= MAX_CONTROL {
        return Ok(());
    }

    let control_opts = ControlSearchOptions {
        max_operators: 5,
        top_k: options.top_k,
        mood: options.mood,
        layout: layout.clone(),
        matatabi_consumer_active: assignment_has_matatabi_consumer(assignment),
        must_include: pinned.clone(),
        fill_policy: ControlFillPolicy::HrAndMood,
    };

    let base_pool = if options.skip_standalone_control || !pinned.is_empty() {
        pool.clone()
    } else {
        try_filter_standalone(pool, FacilityKind::ControlCenter, 1)
    };
    let filtered_pool =
        filter_control_pool_for_fill(&base_pool, used, &pinned, control_opts.fill_policy);

    let hit = if pinned.is_empty() {
        let combos = search_control_combos(&filtered_pool, table, &control_opts)?;
        pick_cached_or_rescan_control(
            &combos,
            &pinned,
            used,
            || search_control_combos(&filtered_pool, table, &control_opts),
            |h| &h.names,
            "control: no disjoint combo after pool filter",
        )?
    } else {
        let combos = search_control_combos(&filtered_pool, table, &control_opts)?;
        pick_control_extending_pins(combos.iter().cloned(), &pinned, used, &|h| &h.names)
            .ok_or_else(|| Error::msg("control: no combo extending pinned after pool filter"))?
    };
    let control_id = RoomId::from("control");
    commit_control_combo(
        assignment,
        &control_id,
        &hit.names,
        |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
        used,
        &pinned,
    )
}

fn filter_control_pool_for_fill(
    pool: &ControlPool,
    used: &HashSet<String>,
    pinned: &HashSet<String>,
    fill_policy: ControlFillPolicy,
) -> ControlPool {
    ControlPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| {
                (!used.contains(&e.name) || pinned.contains(&e.name))
                    && (fill_policy != ControlFillPolicy::HrAndMood
                        || pinned.contains(&e.name)
                        || control_entry_plugin_fill(e))
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn pick_cached_or_rescan_control<T, F>(
    cached: &[T],
    pinned: &HashSet<String>,
    used: &HashSet<String>,
    rescan: F,
    names_of: impl Fn(&T) -> &[String],
    err: &str,
) -> Result<T>
where
    T: Clone,
    F: FnOnce() -> Result<Vec<T>>,
{
    if let Some(hit) = pick_control_extending_pins(cached.iter().cloned(), pinned, used, &names_of)
    {
        return Ok(hit);
    }
    let fresh = rescan()?;
    pick_control_extending_pins(fresh, pinned, used, &names_of).ok_or_else(|| Error::msg(err))
}

fn pick_control_extending_pins<T: Clone>(
    hits: impl IntoIterator<Item = T>,
    pinned: &HashSet<String>,
    used: &HashSet<String>,
    names_of: &impl Fn(&T) -> &[String],
) -> Option<T> {
    hits.into_iter().find(|h| {
        let names = names_of(h);
        pinned.iter().all(|p| names.contains(p))
            && names
                .iter()
                .all(|n| pinned.contains(n) || !used.contains(n))
    })
}

fn commit_control_combo(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    elite_of: impl Fn(&str) -> u8,
    used: &mut HashSet<String>,
    pinned: &HashSet<String>,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !pinned.contains(name) && !used.insert(name.clone()) {
                return Err(Error::msg(format!("control duplicate {name}")));
            }
            Ok(AssignedOperator::new(name, elite_of(name)))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), ops);
    Ok(())
}

/// 感知链 producer 落位（非编排 System）：黑键/迷迭香在盒时堆感知源，供 resolve + 贪心消费。
fn assign_perception_producers(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    if !operbox.owns("黑键") || !operbox.owns("迷迭香") {
        return Ok(());
    }
    if operbox.owns("夕") && !used.contains("夕") {
        let control = assignment.control_operators();
        if control.len() < 5 {
            let elite = operbox.elite_of("夕").unwrap_or(0);
            let mut ops = control;
            ops.push(AssignedOperator::new("夕", elite));
            used.insert("夕".into());
            assignment.set_room(RoomId::from("control"), ops);
        }
    }
    if operbox.elite_of("絮雨").unwrap_or(0) >= 2 && !used.contains("絮雨") {
        for room in blueprint.rooms_of(FacilityKind::Office) {
            if !assignment.operators_in(&room.id).is_empty() {
                continue;
            }
            used.insert("絮雨".into());
            assignment.set_room(room.id.clone(), vec![AssignedOperator::new("絮雨", 2)]);
            break;
        }
    }
    for name in ["爱丽丝", "车尔尼"] {
        if operbox.elite_of(name).unwrap_or(0) < 2 || used.contains(name) {
            continue;
        }
        let Some(room) = blueprint
            .rooms_of(FacilityKind::Dormitory)
            .into_iter()
            .find(|r| assignment.operators_in(&r.id).is_empty())
        else {
            continue;
        };
        used.insert(name.into());
        assignment.set_room(room.id.clone(), vec![AssignedOperator::new(name, 2)]);
    }
    Ok(())
}

fn assign_dorm_producers(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for room in blueprint.rooms_of(FacilityKind::Dormitory) {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let Some((name, elite)) = best_dorm_producer(operbox, instances, used) else {
            continue;
        };
        used.insert(name.clone());
        assignment.set_room(room.id.clone(), vec![AssignedOperator::new(name, elite)]);
    }
    Ok(())
}

fn best_dorm_producer(
    operbox: &OperBox,
    instances: &OperatorInstances,
    used: &HashSet<String>,
) -> Option<(String, u8)> {
    let mut best: Option<(String, u8, u8)> = None;
    for (name, progress) in &operbox.owned {
        if used.contains(name) || progress.elite < 2 {
            continue;
        }
        let tier = crate::tier::PromotionTier::from_progress(*progress);
        let buffs = instances.resolve_dorm_buff_ids(name, tier);
        if !buffs.iter().any(|b| b == SENXI_DORM_CUISINE_BUFF) {
            continue;
        }
        let replace = best
            .as_ref()
            .is_none_or(|(_, _, level)| progress.elite > *level);
        if replace {
            best = Some((name.clone(), progress.elite, progress.elite));
        }
    }
    best.map(|(name, elite, _)| (name, elite))
}

fn trade_hit_excludes_blackkey_witch_collide(hit: &TradeSearchHit) -> bool {
    !hit.names.iter().any(|n| n == WITCH_TRADE_NAME) && !hit_witch_shortcut(hit)
}

fn trade_hit_ok_for_greedy(hit: &TradeSearchHit) -> bool {
    let has_blackkey = hit.names.iter().any(|n| n == BLACKKEY_NAME);
    if !has_blackkey {
        return true;
    }
    trade_hit_excludes_blackkey_witch_collide(hit)
}

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
            trade_room_has_operator(assignment, &r.id, BLACKKEY_NAME)
                && trade_room_has_operator(assignment, &r.id, WITCH_TRADE_NAME)
        })
}

const BLACKKEY_NAME: &str = "黑键";
const WITCH_TRADE_NAME: &str = "巫恋";
const DOCUS_TRADE_NAME: &str = "但书";
const CLOSURE_TRADE_NAME: &str = "可露希尔";
/// 这些旧 `base_systems` 条目是 L3/兼容锚点，不再由 registry fixed 早占贸易站。
/// 主路径改由 `trade_segments.roles` 的核心优先策略落位：但书 -> 可露希尔 -> 巫恋。
const TRADE_ROLE_MANAGED_REGISTRY_SYSTEMS: [&str; 6] = [
    "blackkey_closure",
    "witch_long_beta",
    "penguin_exusiai_lemuen",
    "penguin_texangel_e2",
    "penguin_texlap_e0",
    "vina_lungmen",
];
/// 公孙 243 金线固定 trio（`ideal_e2_saria_qingliu_weedy_gold_140`）。
const GONGSUN_GOLD_MANU_TEAM: [&str; 3] = ["清流", "温蒂", "森蚺"];

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
        |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
        used,
        anchors,
        "manufacture fixed team",
    )?;
    Ok(true)
}

fn try_assign_gongsun_gold_manu_team(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    pool: &ManuPool,
    used: &mut HashSet<String>,
) -> Result<()> {
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
            let senxi_entry = pool.entry("森蚺");
            if senxi_entry.is_some() && !used.contains("森蚺") {
                let mut ops: Vec<AssignedOperator> = existing.to_vec();
                ops.push(AssignedOperator::new("森蚺", senxi_entry.unwrap().elite));
                assignment.set_room(room.id.clone(), ops);
                used.insert("森蚺".to_string());
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
    let _ = try_commit_fixed_manu_team(
        assignment,
        &room.id,
        &GONGSUN_GOLD_MANU_TEAM,
        pool,
        used,
        &[],
    )?;
    Ok(())
}

fn trade_room_has_operator(assignment: &BaseAssignment, room_id: &RoomId, name: &str) -> bool {
    assignment
        .operators_in(room_id)
        .iter()
        .any(|o| o.name == name)
}

/// `names` 中非锚点成员均不在 `used_wo`（锚点已从 `used_wo` 剔除，天然通过）。
fn names_disjoint_except(names: &[String], used_wo: &HashSet<String>) -> bool {
    names.iter().all(|n| !used_wo.contains(n))
}

/// 提交补满后的锚点房：锚点已在 `used`（跳过插入），其余队友计入 `used`。
fn commit_anchor_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    elite_of: impl Fn(&str) -> u8,
    used: &mut HashSet<String>,
    anchors: &[String],
    facility: &str,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !anchors.contains(name) && !used.insert(name.clone()) {
                return Err(Error::msg(format!("{facility} duplicate {name}")));
            }
            Ok(AssignedOperator::new(name, elite_of(name)))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), ops);
    Ok(())
}

/// 恢复班贸易：精0 孑一站（若有），其余站贪心；按蓝图贸易站数填满。
fn assign_trade_jie_remainder(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let trade_rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .collect();
    if trade_rooms.is_empty() {
        return Ok(());
    }

    let jie_lead = !karlan_precision_active(&layout.global_inject)
        && jie_e0_trade_operator(instances, table).is_some();

    if jie_lead {
        if let Some(room) = trade_rooms
            .iter()
            .find(|r| assignment.operators_in(&r.id).is_empty())
        {
            let sub = filter_trade_pool(pool, used);
            if sub.entries.len() >= 3 {
                if let Some(jie_op) = jie_e0_trade_operator(instances, table) {
                    let search_opts =
                        trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold);
                    if let Ok(report) = search_trade_triples_filtered(
                        &sub,
                        table,
                        &search_opts,
                        SearchTripleFilter {
                            must_include_name: Some(JIE_TRADE_NAME.to_string()),
                            must_operator_override: Some(jie_op),
                            ..SearchTripleFilter::default()
                        },
                    ) {
                        commit_trade_room(assignment, &room.id, &report.best, pool, used)?;
                    }
                }
            }
        }
    }

    for room in &trade_rooms {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let order = trade_order_from_room(room)?;
        let hit = pick_trade_hit(
            pool,
            table,
            trade_room_options(layout, gold_lines, options, order),
            SearchTripleFilter::default(),
            used,
            options.top_k,
        )
        .map_err(|e| Error::msg(format!("trade recovery {}: {e}", room.id.0)))?;
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

fn assign_trade_remainder(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for room in blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
    {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let order = trade_order_from_room(room)?;
        let hit = pick_trade_meta_then_plain(pool, table, layout, gold_lines, options, order, used)
            .map_err(|e| Error::msg(format!("trade {}: {e}", room.id.0)))?;
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

fn assign_manufacture_lines(
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

    for room in rooms {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => continue,
        };
        let hit = pick_manu_hit(
            pool,
            table,
            manu_options(layout, options, recipe),
            used,
            options.top_k,
        )
        .map_err(|e| Error::msg(format!("manufacture {}: {e}", room.id.0)))?;
        commit_manu_room(assignment, &room.id, &hit, pool, used)?;
    }
    Ok(())
}

/// 为一支队伍填满指定的贸易/制造房间（站绑定），共享 `used` 实现跨队互斥。
/// 贸易站取当前可用最优三人组（shortcut 自然高分），制造站同理；发电/中枢/宿舍不在此处理。
#[allow(clippy::too_many_arguments)]
pub fn assign_team_producer_rooms(
    blueprint: &BaseBlueprint,
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
        trade_pool,
        table,
        layout,
        options,
        trade_rooms,
        assignment,
        used,
    )?;
    assign_team_manu_rooms(
        blueprint, manu_pool, table, layout, options, manu_rooms, assignment, used,
    )
}

/// γ 替补半区：贸易沿用 meta 核心优先级，制造/发电仍站绑定搜索。
#[allow(clippy::too_many_arguments)]
pub fn assign_team_gamma_half(
    blueprint: &BaseBlueprint,
    trade_pool: &TradePool,
    manu_pool: &ManuPool,
    power_pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    trade_rooms: &[RoomId],
    manu_rooms: &[RoomId],
    power_rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_team_trade_meta_rooms(
        blueprint,
        trade_pool,
        table,
        layout,
        options,
        trade_rooms,
        assignment,
        used,
    )?;
    assign_team_manu_rooms(
        blueprint, manu_pool, table, layout, options, manu_rooms, assignment, used,
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
    )
}

#[allow(clippy::too_many_arguments)]
fn assign_team_trade_meta_rooms(
    blueprint: &BaseBlueprint,
    trade_pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    trade_rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let gold_lines = blueprint.gold_manu_line_count();
    for room_id in trade_rooms {
        if !assignment.operators_in(room_id).is_empty() {
            continue;
        }
        let room = blueprint
            .room(room_id)
            .ok_or_else(|| Error::msg(format!("team trade room {} not in blueprint", room_id.0)))?;
        let order = trade_order_from_room(room)?;
        let hit =
            pick_trade_meta_then_plain(trade_pool, table, layout, gold_lines, options, order, used)
                .map_err(|e| Error::msg(format!("team trade {}: {e}", room_id.0)))?;
        commit_trade_room(assignment, room_id, &hit, trade_pool, used)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn assign_team_manu_rooms(
    blueprint: &BaseBlueprint,
    manu_pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    manu_rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
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
        let hit = pick_manu_hit(
            manu_pool,
            table,
            manu_options(layout, options, recipe),
            used,
            options.top_k,
        )
        .map_err(|e| Error::msg(format!("team manu {}: {e}", room_id.0)))?;
        commit_manu_room(assignment, room_id, &hit, manu_pool, used)?;
    }
    Ok(())
}

/// 填满蓝图全部空发电站（每站 1 人、贪心取可用最优）；跨班复用，受 `used` 约束。
pub fn assign_power_stations(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let room_ids: Vec<RoomId> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::PowerPlant)
        .map(|r| r.id.clone())
        .collect();
    assign_power_rooms(
        blueprint, pool, table, layout, options, &room_ids, assignment, used,
    )
}

/// 填满指定发电站（每站 1 人、贪心取可用最优）；供三队轮换按半区分配。
#[allow(clippy::too_many_arguments)]
pub fn assign_power_rooms(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let total_stations = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::PowerPlant)
        .count();
    if total_stations == 0 || rooms.is_empty() {
        return Ok(());
    }

    let power_opts = PowerSearchOptions {
        station_count: total_stations.min(255) as u8,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: layout.clone(),
    };

    let empty_rooms: Vec<RoomId> = rooms
        .iter()
        .filter(|room_id| {
            blueprint
                .room(room_id)
                .is_some_and(|r| r.kind == FacilityKind::PowerPlant)
                && assignment.operators_in(room_id).is_empty()
        })
        .cloned()
        .collect();
    if empty_rooms.is_empty() {
        return Ok(());
    }

    let sub = filter_power_pool(pool, used);
    let sub = try_filter_standalone(&sub, FacilityKind::PowerPlant, 1);
    if sub.entries.is_empty() {
        return Err(Error::msg("power: no available operators"));
    }

    let mut opts = power_opts;
    opts.station_count = empty_rooms.len().min(255) as u8;
    let report = search_power_assignment(&sub, table, &opts)?;
    if report.assignments.len() != empty_rooms.len() {
        return Err(Error::msg(format!(
            "power: expected {} assignments, got {}",
            empty_rooms.len(),
            report.assignments.len()
        )));
    }

    for (room_id, station) in empty_rooms.iter().zip(report.assignments.iter()) {
        let elite = pool.entry(&station.hit.name).map(|e| e.elite).unwrap_or(0);
        if !used.insert(station.hit.name.clone()) {
            return Err(Error::msg(format!(
                "power {}: duplicate {}",
                room_id.0, station.hit.name
            )));
        }
        assignment.set_power_operator(
            room_id.clone(),
            AssignedOperator::new(&station.hit.name, elite),
        );
    }
    Ok(())
}

fn trade_order_from_room(room: &crate::layout::blueprint::RoomBlueprint) -> Result<TradeOrderKind> {
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

fn skip_trade_core_registry_systems(skip: &mut HashSet<String>) {
    for id in TRADE_ROLE_MANAGED_REGISTRY_SYSTEMS {
        skip.insert(id.to_string());
    }
}

/// 团队贸易站取人：但书 → 可露希尔 → 龙巫 → 普通散件。
///
/// 这些 meta 是核心优先级，不是固定三人组；每个核心站内部仍由贸易搜索选择最优队友。
fn pick_trade_meta_then_plain(
    pool: &TradePool,
    table: &SkillTable,
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
    used: &mut HashSet<String>,
) -> Result<TradeSearchHit> {
    if order == TradeOrderKind::Gold && !used.contains(DOCUS_TRADE_NAME) {
        if let Ok(hit) = pick_docus_trade_hit(
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == DOCUS_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold && !used.contains(CLOSURE_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit(
            "closure",
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == CLOSURE_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    if order == TradeOrderKind::Gold && !used.contains(WITCH_TRADE_NAME) {
        if let Ok(hit) = pick_trade_role_hit(
            "witch",
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            layout,
            used,
            options.top_k,
        ) {
            if hit.names.iter().any(|n| n == WITCH_TRADE_NAME) {
                return Ok(hit);
            }
        }
    }
    pick_trade_hit(
        pool,
        table,
        trade_room_options(layout, gold_lines, options, order),
        SearchTripleFilter {
            hit_filter: Some(trade_hit_ok_for_greedy),
            ..SearchTripleFilter::default()
        },
        used,
        options.top_k,
    )
}

fn pick_trade_hit(
    pool: &TradePool,
    table: &SkillTable,
    search_opts: TradeSearchOptions,
    filter: SearchTripleFilter,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<TradeSearchHit> {
    let sub = filter_trade_pool(pool, used);
    let sub = if karlan_precision_active(&search_opts.layout.global_inject) {
        sub
    } else {
        try_filter_standalone(&sub, FacilityKind::TradePost, 3)
    };
    if sub.entries.len() < 3 {
        return Err(Error::msg(format!(
            "trade pool has {} ready operators (need 3)",
            sub.entries.len()
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;
    let report = match search_trade_triples_filtered(&sub, table, &opts, filter.clone()) {
        Ok(r) => r,
        Err(_) if filter.hit_filter.is_some() || filter.must_include_name.is_some() => {
            search_trade_triples(&sub, table, &opts)?
        }
        Err(e) => return Err(e),
    };
    pick_disjoint_from_report(
        report.best,
        report.top,
        trade_hit_names,
        used,
        "no disjoint trade triple",
    )
}

fn pick_manu_hit(
    pool: &ManuPool,
    table: &SkillTable,
    search_opts: ManuSearchOptions,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<ManuSearchHit> {
    let sub = filter_manufacture_pool(pool, used);
    let sub = try_filter_standalone(&sub, FacilityKind::Factory, 3);
    if sub.entries.len() < 3 {
        return Err(Error::msg(format!(
            "manufacture pool has {} ready operators (need 3)",
            sub.entries.len()
        )));
    }
    let mut opts = search_opts;
    opts.top_k = top_k;
    let report = search_manufacture_triples(&sub, table, &opts)?;
    pick_disjoint_from_report(
        report.best,
        report.top,
        manu_hit_names,
        used,
        "no disjoint manufacture triple",
    )
}

fn commit_trade_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    hit: &TradeSearchHit,
    pool: &TradePool,
    used: &mut HashSet<String>,
) -> Result<()> {
    commit_operators_to_room(
        assignment,
        room_id,
        trade_hit_names(hit),
        |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
        used,
        "trade",
    )
}

fn commit_manu_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    hit: &ManuSearchHit,
    pool: &ManuPool,
    used: &mut HashSet<String>,
) -> Result<()> {
    commit_operators_to_room(
        assignment,
        room_id,
        manu_hit_names(hit),
        |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
        used,
        "manufacture",
    )
}

fn trade_room_options(
    layout: &LayoutContext,
    gold_lines: u32,
    options: &AssignBaseOptions,
    order: TradeOrderKind,
) -> TradeSearchOptions {
    TradeSearchOptions {
        top_k: options.top_k,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: Arc::new(layout.clone()),
        gold_production_lines: gold_lines,
        order_mode: TradeSearchOrderMode::Single(order),
        ..TradeSearchOptions::default()
    }
}

fn manu_options(
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

fn filter_power_pool(pool: &PowerPool, exclude: &HashSet<String>) -> PowerPool {
    PowerPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| !exclude.contains(&e.name))
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn names_disjoint(names: &[String], used: &HashSet<String>) -> bool {
    names.iter().all(|n| !used.contains(n))
}

fn first_nonempty_names<'a>(a: &'a [String], b: &'a [String], c: &'a [String]) -> &'a [String] {
    if !a.is_empty() {
        a
    } else if !b.is_empty() {
        b
    } else {
        c
    }
}

fn trade_hit_names(hit: &TradeSearchHit) -> &[String] {
    first_nonempty_names(&hit.names, &hit.gold_names, &hit.originium_names)
}

fn manu_hit_names(hit: &ManuSearchHit) -> &[String] {
    first_nonempty_names(&hit.names, &hit.gold_names, &hit.battle_record_names)
}

fn pick_first_disjoint<T: Clone>(
    hits: impl IntoIterator<Item = T>,
    names_of: &impl Fn(&T) -> &[String],
    used: &HashSet<String>,
) -> Option<T> {
    hits.into_iter().find(|h| names_disjoint(names_of(h), used))
}

fn pick_disjoint_from_report<T: Clone>(
    best: T,
    top: Vec<T>,
    names_of: impl Fn(&T) -> &[String],
    used: &HashSet<String>,
    err: &str,
) -> Result<T> {
    pick_first_disjoint(
        top.into_iter().chain(std::iter::once(best)),
        &names_of,
        used,
    )
    .ok_or_else(|| Error::msg(err))
}

fn commit_operators_to_room(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    elite_of: impl Fn(&str) -> u8,
    used: &mut HashSet<String>,
    facility: &str,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !used.insert(name.clone()) {
                return Err(Error::msg(format!("{facility} duplicate {name}")));
            }
            Ok(AssignedOperator::new(name, elite_of(name)))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), ops);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::instances::default_instances_path;
    use crate::layout::shift::AssignShiftMode;
    use crate::layout::BaseBlueprint;
    use crate::operbox::{default_operbox_gongsun_path, OperBox, OperBoxEntry};
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    fn operbox_from_names(entries: &[(&str, u8, u8)]) -> OperBox {
        OperBox::from_entries(
            entries
                .iter()
                .enumerate()
                .map(|(i, (name, elite, rarity))| OperBoxEntry {
                    id: format!("test_{i:03}"),
                    name: (*name).to_string(),
                    elite: *elite,
                    level: 1,
                    own: true,
                    potential: 1,
                    rarity: *rarity,
                })
                .collect(),
        )
    }

    #[test]
    fn assign_ideal_e2_peak_claims_docus_syracusa_system() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(
            &crate::skill_table::data_path("schedule_243/operbox_ideal_e2.json").unwrap(),
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("八幡海铃")
            || !operbox.owns("但书")
            || !operbox.owns("伺夜")
            || !operbox.owns("贝洛内")
        {
            return;
        }
        let assignment = assign_base_greedy(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
        )
        .unwrap();
        // 但书链 meta（registry）；迷迭香/黑键感知链不在编排层进编（Phase 4 global effect）。
        let docus_room = assignment.rooms.iter().find(|r| {
            r.operators.iter().any(|o| o.name == "但书")
                && r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        assert!(docus_room.is_some(), "但书三人组应独占一站");

        let control_ops = assignment.control_operators();
        let control: HashSet<_> = control_ops.iter().map(|o| o.name.as_str()).collect();
        assert!(control.contains("八幡海铃"), "control: {:?}", control);
        assert!(
            control.contains("斩业星熊") && control.contains("诗怀雅"),
            "control: {:?}",
            control
        );
        assert!(
            !control.contains("三角初华") && !control.contains("若叶睦"),
            "钉死后补位应为公招/心情而非 MyGO 热情链: {:?}",
            control
        );
        assert!(
            control.contains("薇薇安娜") || control.contains("焰尾"),
            "应有中枢心情回复补位: {:?}",
            control
        );
        assert!(
            !control.contains("火龙S黑角") && !control.contains("麒麟R夜刀"),
            "高峰无调查团时不应因木天蓼选怪猎中枢: {:?}",
            control
        );
    }

    #[test]
    fn assign_243_use_this_has_no_duplicate_operators() {
        let (blueprint, operbox, instances, table) = fixtures();
        let assignment = assign_shift_with_plan_skip(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &HashSet::new(),
        )
        .unwrap()
        .assignment;
        let mut seen = HashSet::new();
        for room in &assignment.rooms {
            for op in &room.operators {
                assert!(seen.insert(op.name.clone()), "duplicate {}", op.name);
            }
        }
    }

    #[test]
    fn assign_full_e2_blackkey_never_colocated_with_witch() {
        use crate::operbox::default_operbox_full_e2_path;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("黑键") || !operbox.owns("巫恋") {
            return;
        }
        let assignment = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 10,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        assert!(
            !blackkey_witch_same_trade_room(&assignment, &blueprint),
            "243 双贸：黑键与巫恋不得同房"
        );
        let report = crate::schedule::schedule_team_rotation(
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
        for shift in &report.shifts {
            assert!(
                !blackkey_witch_same_trade_room(&shift.assignment, &blueprint),
                "team-rotation shift {} 黑键与巫恋同房",
                shift.index + 1
            );
        }
    }

    #[test]
    fn team_rotation_partial_trade_meta_keeps_docus_closure_and_witch() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = operbox_from_names(&[
            ("但书", 2, 5),
            ("可露希尔", 2, 6),
            ("巫恋", 2, 5),
            ("龙舌兰", 2, 5),
            ("卡夫卡", 1, 5),
            ("德克萨斯", 2, 5),
            ("拉普兰德", 2, 5),
            ("古米", 2, 4),
            ("夜刀", 0, 2),
            ("石英", 2, 4),
            ("慕斯", 2, 4),
            ("空弦", 2, 6),
            ("能天使", 2, 6),
            ("空", 2, 5),
            ("八幡海铃", 2, 5),
            ("灵知", 2, 6),
            ("斩业星熊", 2, 6),
            ("诗怀雅", 2, 5),
            ("Mon3tr", 2, 6),
            ("凯尔希", 2, 6),
            ("明椒", 0, 5),
            ("望", 2, 5),
            ("薇薇安娜", 2, 6),
            ("阿米娅", 2, 5),
            ("格雷伊", 2, 4),
            ("烛煌", 2, 6),
            ("澄闪", 2, 6),
            ("雷蛇", 2, 5),
            ("海霓", 2, 5),
            ("清流", 2, 4),
            ("砾", 2, 4),
            ("苍苔", 2, 5),
            ("白雪", 2, 4),
            ("红豆", 2, 4),
            ("酒神", 2, 6),
            ("褐果", 2, 4),
            ("卡达", 2, 4),
            ("槐琥", 2, 5),
            ("铅踝", 2, 5),
            ("雪猎", 2, 5),
            ("斑点", 2, 3),
            ("乌尔比安", 2, 6),
            ("斯卡蒂", 1, 6),
            ("冬时", 2, 5),
            ("幽灵鲨", 2, 5),
            ("安哲拉", 0, 5),
            ("水月", 2, 6),
            ("炎熔", 2, 3),
            ("艾雅法拉", 2, 6),
            ("阿罗玛", 0, 5),
        ]);
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();

        let report = crate::schedule::schedule_team_rotation(
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
            report.shifts.iter().any(|shift| trade_room_contains(
                &shift.assignment,
                &blueprint,
                &["但书"]
            )),
            "partial meta account should keep docus core in rotation"
        );
        assert!(
            report.shifts.iter().any(|shift| {
                trade_room_contains(&shift.assignment, &blueprint, &["可露希尔"])
            }),
            "partial meta account should keep closure core in rotation"
        );
        assert!(
            report.shifts.iter().any(|shift| {
                trade_room_contains(&shift.assignment, &blueprint, &["巫恋", "龙舌兰"])
            }),
            "partial meta account should keep witch + tequila fallback in rotation"
        );
    }

    fn trade_room_contains(
        assignment: &BaseAssignment,
        blueprint: &BaseBlueprint,
        names: &[&str],
    ) -> bool {
        assignment.rooms.iter().any(|room| {
            blueprint
                .rooms
                .iter()
                .any(|bp| bp.id == room.room_id && bp.kind == FacilityKind::TradePost)
                && names
                    .iter()
                    .all(|name| room.operators.iter().any(|op| op.name == *name))
        })
    }

    #[test]
    fn assign_full_e2_peak_manu_teams_match_gongsun() {
        use crate::manufacture::{solve_manufacture, ManuRoomInput};
        use crate::operbox::default_operbox_full_e2_path;
        use crate::pool::build_manufacture_pool;
        use std::sync::Arc;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("清流") || !operbox.owns("迷迭香") {
            return;
        }
        let peak = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 30,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        let durin = operbox.durin_dorm_planning_count(&instances);
        let resolved = resolve_base(
            &blueprint,
            &peak,
            Some(&instances),
            Some(&table),
            24.0,
            Some(durin),
        )
        .unwrap();

        let room_ops = |room_id: &str| -> Vec<String> {
            peak.operators_in(&RoomId::from(room_id))
                .iter()
                .map(|o| o.name.clone())
                .collect()
        };

        let gold_trio: HashSet<_> = GONGSUN_GOLD_MANU_TEAM.iter().map(|s| *s).collect();
        let gold_room = peak.rooms.iter().find(|r| {
            blueprint.rooms.iter().any(|b| {
                b.id == r.room_id
                    && b.kind == FacilityKind::Factory
                    && matches!(
                        b.product.as_ref(),
                        Some(RoomProduct::Factory {
                            recipe: RecipeKind::Gold
                        })
                    )
            }) && gold_trio
                .iter()
                .all(|n| r.operators.iter().any(|o| o.name == *n))
        });
        assert!(
            gold_room.is_some(),
            "金线应有清流+温蒂+森蚺，实际制造编制: {:?}",
            peak.rooms
                .iter()
                .filter(|r| {
                    blueprint
                        .rooms
                        .iter()
                        .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
                })
                .collect::<Vec<_>>()
        );

        let br_winter = room_ops("manu_2");
        assert!(
            !br_winter.contains(&"清流".to_string()),
            "经验线 manu_2 不应占清流 trio: {br_winter:?}"
        );

        let pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        let mk = |names: &[&str]| -> Vec<_> {
            names
                .iter()
                .map(|n| pool.entry(n).unwrap().to_manu_operator())
                .collect()
        };
        let gold_room_resolved = resolved
            .manu_rooms
            .iter()
            .find(|r| {
                gold_trio
                    .iter()
                    .all(|n| r.operators.iter().any(|o| o.name == *n))
            })
            .expect("resolved gold trio");
        let gold_skill = solve_manufacture(
            &ManuRoomInput {
                level: gold_room_resolved.level,
                operators: mk(&GONGSUN_GOLD_MANU_TEAM),
                active_recipe: RecipeKind::Gold,
                mood: 24.0,
                layout: Arc::new(gold_room_resolved.layout.clone()),
            },
            &table,
        )
        .unwrap()
        .prod_skill;
        assert!(
            (gold_skill - 140.0).abs() <= 1.0,
            "清流金线纸面约 140，got {gold_skill:.1}"
        );
    }

    #[test]
    fn assign_snhunt_control_gets_monhun_ops_when_owned() {
        let blueprint =
            BaseBlueprint::load(&crate::skill_table::data_path("layout/snhunt.json").unwrap())
                .unwrap();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("火龙S黑角") || !operbox.owns("麒麟R夜刀") {
            return;
        }
        // 怪猎评估布局：须本班有调查团 consumer，木天蓼才计入中枢正分。
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "trade_1",
            vec![AssignedOperator::new(MATATABI_CONSUMER_NAME, 2)],
        );
        let assignment = assign_shift(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &seed,
        )
        .unwrap();
        let control = assignment.control_operators();
        let names: HashSet<_> = control.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains("火龙S黑角"));
        assert!(names.contains("麒麟R夜刀"));
    }
}
