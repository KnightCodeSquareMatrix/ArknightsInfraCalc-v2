use std::collections::HashSet;

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
use crate::layout::resolve::resolve_base;
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::claim_base_systems;
use crate::manufacture::input::ManuSearchRecipeMode;
use crate::operbox::OperBox;
use crate::pool::{
    build_control_pool, build_manufacture_pool, build_power_pool, build_trade_pool,
    filter_manufacture_pool, filter_trade_pool, jie_e0_trade_operator,
    karlan_precision_active,
    ControlPool, ManuPool, PowerPool, TradePool, JIE_TRADE_NAME,
};
use crate::search::{
    hit_closure_shortcut, hit_witch_shortcut, pick_docus_trade_hit, search_control_combos,
    search_manufacture_triples, search_power_assignment, search_trade_triples,
    search_trade_triples_filtered, control_entry_hr_mood_fill, ControlFillPolicy,
    ControlSearchOptions, ManuSearchHit, MATATABI_CONSUMER_NAME,
    ManuSearchOptions, PowerSearchOptions, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
};
use crate::skill_table::SkillTable;
use crate::layout::LayoutContext;
use crate::trade::input::{TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use crate::trade::solve_trade_with_shift;
use crate::types::RecipeKind;

const SENXI_DORM_CUISINE_BUFF: &str = "dorm_rec_bd_dungeon[000]";

#[derive(Debug, Clone)]
pub struct AssignBaseOptions {
    pub top_k: usize,
    pub mood: f64,
    pub shift_hours: f64,
}

impl Default for AssignBaseOptions {
    fn default() -> Self {
        Self {
            top_k: 20,
            mood: 24.0,
            shift_hours: 24.0,
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
    blueprint.validate()?;

    let mut assignment = seed.clone();
    let mut used = assignment_operator_names(&assignment);

    if mode == AssignShiftMode::Peak {
        claim_base_systems(
            blueprint,
            operbox,
            table,
            mode,
            &mut assignment,
            &mut used,
        )?;
    }

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

    if mode == AssignShiftMode::Peak && assignment.control_operators().len() < 5 {
        let control_pool =
            build_control_pool(&operbox.control_roster(instances), instances, table)?;
        assign_control(
            &mut assignment,
            &control_pool,
            table,
            &producer_layout,
            options,
            &mut used,
        )?;
    }

    if mode == AssignShiftMode::Peak {
        assign_dorm_producers(blueprint, operbox, instances, &mut assignment, &mut used)?;
    }

    let layout = resolve_base(
        blueprint,
        &assignment,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let power_pool = build_power_pool(&operbox.power_roster(instances), instances, table)?;
    let gold_lines = blueprint.gold_manu_line_count();

    match mode {
        AssignShiftMode::Peak => {
            // 迷迭香等制造锚点（base_systems 只钉了单人）先补满队友——它因感知是全基建最高产
            // 制造位，应优先于普通制造贪心拿到最佳队友。
            complete_manu_anchor_rooms(
                blueprint,
                &manu_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            assign_trade_meta(
                blueprint,
                &trade_pool,
                table,
                &layout,
                gold_lines,
                options,
                &mut assignment,
                &mut used,
            )?;
            // 黑键贸易锚点在但书/巫恋/可露希尔认领贸位后补满——高效散件优先给但书，
            // 黑键拿剩余次优（文档 §5.1）。锚点已占非巫恋贸位，故天然不与巫恋同站。
            complete_trade_anchor_rooms(
                blueprint,
                &trade_pool,
                table,
                &layout,
                gold_lines,
                options,
                &mut assignment,
                &mut used,
            )?;
            // 发电先于制造搜索：虚拟发电站计入 layout，金线 automation trio 才到 140。
            assign_power_stations(
                blueprint,
                &power_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            let manu_layout = resolve_base(
                blueprint,
                &assignment,
                Some(instances),
                Some(table),
                options.mood,
                Some(durin_plan),
            )?
            .layout_snapshot();
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &manu_layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            assign_trade_remainder(
                blueprint,
                &trade_pool,
                table,
                &layout,
                gold_lines,
                options,
                &mut assignment,
                &mut used,
            )?;
            // 文档 §5.1：黑键可与但书/可露希尔同站（黑键感知效率是优质 docus 队友）。
            // 仅当同站后两房合计贸易分提升时才合并，否则保留已验证的 docus 工具组合。
            try_colocate_blackkey_with_meta(
                blueprint,
                instances,
                table,
                options,
                Some(durin_plan),
                &mut assignment,
            )?;
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
            assign_manufacture_lines(
                blueprint,
                &manu_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
            assign_power_stations(
                blueprint,
                &power_pool,
                table,
                &layout,
                options,
                &mut assignment,
                &mut used,
            )?;
        }
    }

    Ok(assignment)
}

/// 编制内所有上岗干员。
pub fn assignment_operator_names(assignment: &BaseAssignment) -> HashSet<String> {
    let mut names = HashSet::new();
    for room in &assignment.rooms {
        for op in &room.operators {
            names.insert(op.name.clone());
        }
    }
    names
}

/// 贸易 / 制造 / 发电岗位干员（跨班互斥池）。
pub fn rotating_workers(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
) -> HashSet<String> {
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

/// 中枢 + 宿舍 + 办公室感知 producer（三班钉死，从高峰班拷贝）。
pub fn pinned_assignment(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
) -> BaseAssignment {
    let mut pinned = BaseAssignment::default();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !matches!(
            bp.kind,
            FacilityKind::ControlCenter | FacilityKind::Dormitory | FacilityKind::Office
        ) {
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

fn assign_control(
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

    let mut control_opts = ControlSearchOptions {
        max_operators: 5,
        top_k: options.top_k,
        mood: options.mood,
        layout: layout.clone(),
        matatabi_consumer_active: assignment_has_matatabi_consumer(assignment),
        must_include: pinned.clone(),
        fill_policy: if pinned.is_empty() {
            ControlFillPolicy::Efficiency
        } else {
            ControlFillPolicy::HrAndMood
        },
    };

    let hit = if pinned.is_empty() {
        let combos = search_control_combos(pool, table, &control_opts)?;
        pick_cached_or_rescan_control(
            &combos,
            &pinned,
            used,
            || {
                let sub = filter_control_pool_for_fill(pool, used, &pinned, control_opts.fill_policy);
                search_control_combos(&sub, table, &control_opts)
            },
            |h| &h.names,
            "control: no disjoint combo after pool filter",
        )?
    } else {
        let sub = filter_control_pool_for_fill(pool, used, &pinned, control_opts.fill_policy);
        let combos = search_control_combos(&sub, table, &control_opts)?;
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
                        || control_entry_hr_mood_fill(e))
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
    if let Some(hit) = pick_control_extending_pins(cached.iter().cloned(), pinned, used, &names_of) {
        return Ok(hit);
    }
    let fresh = rescan()?;
    pick_control_extending_pins(fresh, pinned, used, &names_of)
        .ok_or_else(|| Error::msg(err))
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
        assignment.set_room(
            room.id.clone(),
            vec![AssignedOperator::new(name, elite)],
        );
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
        let replace = best.as_ref().is_none_or(|(_, _, level)| progress.elite > *level);
        if replace {
            best = Some((name.clone(), progress.elite, progress.elite));
        }
    }
    best.map(|(name, elite, _)| (name, elite))
}

fn assignment_has_operator(assignment: &BaseAssignment, name: &str) -> bool {
    assignment.rooms.iter().any(|room| {
        room.operators
            .iter()
            .any(|op| op.name == name)
    })
}

fn next_empty_trade_room<'a>(
    trade_rooms: &'a [&crate::layout::blueprint::RoomBlueprint],
    assignment: &BaseAssignment,
    from: usize,
) -> Option<(usize, &'a crate::layout::blueprint::RoomBlueprint)> {
    trade_rooms.iter().enumerate().skip(from).find_map(|(i, r)| {
        if assignment.operators_in(&r.id).is_empty() {
            Some((i, *r))
        } else {
            None
        }
    })
}

fn assign_trade_meta(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
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
    let mut cursor = 0;

    // 243 等双贸布局：黑键已占一站时，剩余仅一站给 meta（但书 > 巫恋 > 可露希尔），
    // 不可再尝试把巫恋/可露希尔各塞独立第三站——否则 complete_trade 会把巫恋补进黑键房。
    let compact_meta = assignment_has_blackkey_trade_anchor(assignment, blueprint);

    if !assignment_has_operator(assignment, "但书") {
        if let Some((next, room)) = next_empty_trade_room(&trade_rooms, assignment, cursor) {
            let hit = pick_docus_trade_hit(
                pool,
                table,
                trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
                layout,
                used,
                options.top_k,
            )
            .map_err(|e| Error::msg(format!("trade meta docus: {e}")))?;
            commit_trade_room(assignment, &room.id, &hit, pool, used)?;
            cursor = next + 1;
            if compact_meta {
                return Ok(());
            }
        }
    }

    for (label, hit_filter, anchor) in [
        ("witch", hit_witch_shortcut as fn(&TradeSearchHit) -> bool, "巫恋"),
        ("closure", hit_closure_shortcut, "可露希尔"),
    ] {
        if assignment_has_operator(assignment, anchor) {
            continue;
        }
        let Some((next, room)) = next_empty_trade_room(&trade_rooms, assignment, cursor) else {
            return Ok(());
        };
        let hit = pick_trade_hit(
            pool,
            table,
            trade_room_options(layout, gold_lines, options, TradeOrderKind::Gold),
            SearchTripleFilter {
                hit_filter: Some(hit_filter),
                ..SearchTripleFilter::default()
            },
            used,
            options.top_k,
        )
        .map_err(|e| Error::msg(format!("trade meta {label}: {e}")))?;
        commit_trade_room(assignment, &room.id, &hit, pool, used)?;
        cursor = next + 1;
        if compact_meta {
            return Ok(());
        }
    }
    Ok(())
}

/// 补满已被 base_systems 钉了单人锚点（如黑键）的贸易站：must_include 锚点搜三人组，
/// 队友取当前可用最优。锚点已在 `used`，仅新增队友计入 `used`。补不满则保留锚点单人。
fn complete_trade_anchor_rooms(
    blueprint: &BaseBlueprint,
    pool: &TradePool,
    table: &SkillTable,
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
    for room in trade_rooms {
        let anchors = partial_room_anchors(assignment, &room.id);
        let Some(anchors) = anchors else { continue };
        let order = trade_order_from_room(room)?;

        let mut used_wo = used.clone();
        for a in &anchors {
            used_wo.remove(a);
        }
        let sub = filter_trade_pool(pool, &used_wo);
        if sub.entries.len() < 3 {
            continue;
        }
        let mut opts = trade_room_options(layout, gold_lines, options, order);
        opts.top_k = options.top_k;
        let blackkey_anchor = anchors.iter().any(|a| a == BLACKKEY_NAME);
        let filter = SearchTripleFilter {
            must_include_name: Some(anchors[0].clone()),
            hit_filter: if blackkey_anchor {
                Some(trade_hit_excludes_blackkey_witch_collide)
            } else {
                None
            },
            ..SearchTripleFilter::default()
        };
        let Ok(report) = search_trade_triples_filtered(&sub, table, &opts, filter) else {
            continue;
        };
        let Ok(hit) = pick_disjoint_from_report(
            report.best,
            report.top,
            trade_hit_names,
            &used_wo,
            "no disjoint trade anchor triple",
        ) else {
            continue;
        };
        commit_anchor_room(
            assignment,
            &room.id,
            trade_hit_names(&hit),
            |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
            used,
            &anchors,
            "trade anchor",
        )?;
    }
    Ok(())
}

/// 补满已被 base_systems 钉了单人锚点（如迷迭香）的制造站：搜本配方最优三人组，
/// 取首个包含全部锚点且队友可用的命中。锚点已在 `used`，仅新增队友计入 `used`。
fn complete_manu_anchor_rooms(
    blueprint: &BaseBlueprint,
    pool: &ManuPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for room in blueprint.rooms.iter().filter(|r| r.kind == FacilityKind::Factory) {
        let Some(anchors) = partial_room_anchors(assignment, &room.id) else {
            continue;
        };
        let recipe = match room.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => *recipe,
            _ => continue,
        };

        let mut used_wo = used.clone();
        for a in &anchors {
            used_wo.remove(a);
        }
        let sub = filter_manufacture_pool(pool, &used_wo);
        if sub.entries.len() < 3 {
            continue;
        }
        let mut opts = manu_options(layout, options, recipe);
        opts.top_k = options.top_k.max(30);

        if anchors.iter().any(|a| a == ROSEMARY_NAME) {
            if try_commit_fixed_manu_team(
                assignment,
                &room.id,
                &ROSEMARY_MANU_TEAM,
                pool,
                used,
                &anchors,
            )? {
                continue;
            }
        }

        let report = search_manufacture_triples(&sub, table, &opts)?;
        let hit = std::iter::once(report.best.clone())
            .chain(report.top.into_iter())
            .find(|h| {
                let names = manu_hit_names(h);
                anchors.iter().all(|a| names.contains(a))
                    && names_disjoint_except(names, &used_wo)
                    && !manu_hit_forbidden_with_rosemary(names)
            });
        let Some(hit) = hit else { continue };
        commit_anchor_room(
            assignment,
            &room.id,
            manu_hit_names(&hit),
            |name| pool.entry(name).map(|e| e.elite).unwrap_or(0),
            used,
            &anchors,
            "manufacture anchor",
        )?;
    }
    Ok(())
}

const BLACKKEY_NAME: &str = "黑键";
const CLOSURE_NAME: &str = "可露希尔";
const WITCH_TRADE_NAME: &str = "巫恋";
const ROSEMARY_NAME: &str = "迷迭香";
/// 公孙 243 金线固定 trio（`ideal_e2_saria_qingliu_weedy_gold_140`）。
const GONGSUN_GOLD_MANU_TEAM: [&str; 3] = ["清流", "温蒂", "森蚺"];
/// 感知链制造锚点推荐队友（文档 §3.3；优于槐琥等 BR 纸面散件）。
const ROSEMARY_MANU_TEAM: [&str; 3] = ["阿罗玛", "食铁兽", ROSEMARY_NAME];

fn manu_recipe_fill_priority(recipe: RecipeKind) -> u8 {
    match recipe {
        RecipeKind::Gold => 0,
        RecipeKind::BattleRecord => 1,
        RecipeKind::Originium => 2,
        RecipeKind::All => 3,
    }
}

fn manu_hit_forbidden_with_rosemary(names: &[String]) -> bool {
    names.contains(&"清流".to_string()) && names.contains(&"温蒂".to_string())
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
    let Some(room) = blueprint.rooms.iter().find(|r| {
        r.kind == FacilityKind::Factory
            && matches!(r.product.as_ref(), Some(RoomProduct::Factory { recipe: RecipeKind::Gold }))
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

fn assignment_has_blackkey_trade_anchor(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
) -> bool {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .any(|r| trade_room_has_operator(assignment, &r.id, BLACKKEY_NAME))
}

fn trade_hit_excludes_blackkey_witch_collide(hit: &TradeSearchHit) -> bool {
    !hit.names.iter().any(|n| n == WITCH_TRADE_NAME) && !hit_witch_shortcut(hit)
}

/// 文档 §5.1 / §8.4：黑键贸站不得与巫恋同房（含巫恋 shortcut 三人组）。
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

/// 文档 §5.1：尝试把黑键并入但书（或可露希尔）所在贸易站，组成「meta + 黑键 + 高效散件」。
/// 仅当并站后这两间贸易房合计 `effective_eff_multiplier` 严格提升时才采用——否则保留
/// 已验证的 docus 工具组合（黑键留在自己的贸易站）。`used` 不变（同 6 人重排两房）。
fn try_colocate_blackkey_with_meta(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    durin_plan: Option<u8>,
    assignment: &mut BaseAssignment,
) -> Result<()> {
    let trade_ids: Vec<RoomId> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .map(|r| r.id.clone())
        .collect();

    let room_full_with = |name: &str| -> Option<RoomId> {
        trade_ids.iter().find_map(|id| {
            let ops = assignment.operators_in(id);
            (ops.len() == 3 && ops.iter().any(|o| o.name == name)).then(|| id.clone())
        })
    };

    let Some(bk_id) = room_full_with(BLACKKEY_NAME) else {
        return Ok(());
    };
    // meta 优先但书，其次可露希尔；不能是黑键自己那间，也不能是巫恋站。
    let meta = [DOCUS_TRADE_NAME, CLOSURE_NAME].into_iter().find_map(|lead| {
        room_full_with(lead).filter(|id| {
            *id != bk_id && !trade_room_has_operator(assignment, id, WITCH_TRADE_NAME)
        }).map(|id| (lead, id))
    });
    let Some((lead, meta_id)) = meta else {
        return Ok(());
    };

    let base = score_trade_rooms(
        blueprint, assignment, instances, table, options, durin_plan, &[&bk_id, &meta_id],
    )?;

    // 六人池：两房当前干员。候选 = {lead, 黑键, 第三人} 占 meta 房（机制位），其余三人占另一房。
    let mut six: Vec<AssignedOperator> = assignment.operators_in(&bk_id).to_vec();
    six.extend(assignment.operators_in(&meta_id).iter().cloned());
    let lead_op = six.iter().find(|o| o.name == lead).cloned();
    let bk_op = six.iter().find(|o| o.name == BLACKKEY_NAME).cloned();
    let (Some(lead_op), Some(bk_op)) = (lead_op, bk_op) else {
        return Ok(());
    };

    let mut best: Option<(f64, BaseAssignment)> = None;
    for third in &six {
        if third.name == lead || third.name == BLACKKEY_NAME {
            continue;
        }
        let meta_ops = vec![lead_op.clone(), bk_op.clone(), third.clone()];
        let other_ops: Vec<AssignedOperator> = six
            .iter()
            .filter(|o| o.name != lead && o.name != BLACKKEY_NAME && o.name != third.name)
            .cloned()
            .collect();
        if other_ops.len() != 2 && other_ops.len() != 3 {
            continue;
        }
        let mut cand = assignment.clone();
        cand.set_room(meta_id.clone(), meta_ops);
        cand.set_room(bk_id.clone(), other_ops);
        // 同站机制互斥（如 docus+可露希尔）会让 solve 报错 → 跳过该候选。
        let Ok(score) = score_trade_rooms(
            blueprint, &cand, instances, table, options, durin_plan, &[&bk_id, &meta_id],
        ) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, cand));
        }
    }

    if let Some((score, cand)) = best {
        if score > base + 1e-6 {
            *assignment = cand;
        }
    }
    Ok(())
}

/// 解析编制后，对指定贸易房求 `effective_eff_multiplier` 之和（满心情、按 shift_hours）。
/// 任一房 solve 失败（含同房机制互斥）→ 整体返回 Err，供候选筛除。
fn score_trade_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    durin_plan: Option<u8>,
    room_ids: &[&RoomId],
) -> Result<f64> {
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_plan,
    )?;
    let mut total = 0.0;
    for room in &resolved.trade_rooms {
        if room.operators.is_empty() || !room_ids.iter().any(|id| **id == room.id) {
            continue;
        }
        let input = TradeRoomInput {
            level: room.level,
            operators: room.operators.clone(),
            order_count: None,
            mood: options.mood,
            gold_production_lines: Some(resolved.gold_manu_line_count()),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(room.layout.clone()),
            active_order_kind: room.order,
        };
        total += solve_trade_with_shift(&input, table, options.shift_hours)?.effective_eff_multiplier;
    }
    Ok(total)
}

/// 房内已有 1-2 人（base_systems 单人锚点）时返回其名单；空房 / 已满（≥3）返回 None。
fn partial_room_anchors(assignment: &BaseAssignment, room_id: &RoomId) -> Option<Vec<String>> {
    let ops = assignment.operators_in(room_id);
    if ops.is_empty() || ops.len() >= 3 {
        return None;
    }
    Some(ops.iter().map(|o| o.name.clone()).collect())
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
                    let search_opts = trade_room_options(
                        layout,
                        gold_lines,
                        options,
                        TradeOrderKind::Gold,
                    );
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
    for room in blueprint.rooms.iter().filter(|r| r.kind == FacilityKind::TradePost) {
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
    rooms.sort_by_key(|r| {
        match r.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => manu_recipe_fill_priority(*recipe),
            _ => 99,
        }
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
    let gold_lines = blueprint.gold_manu_line_count();
    for room_id in trade_rooms {
        if !assignment.operators_in(room_id).is_empty() {
            continue;
        }
        let room = blueprint
            .room(room_id)
            .ok_or_else(|| Error::msg(format!("team trade room {} not in blueprint", room_id.0)))?;
        let order = trade_order_from_room(room)?;
        // 但书（docus）效率最高（≈纸面工具效率×1.55），必须最优先进站：
        // 搜索按 effective_eff_multiplier 排序时 docus 不一定自动浮顶，故显式置顶。
        // 因 αβγ 顺序填充且共享 `used`，但书会落到最先填充的峰值队（最长班 + 最佳队友）。
        let hit = pick_trade_meta_then_plain(
            trade_pool,
            table,
            layout,
            gold_lines,
            options,
            order,
            used,
        )
        .map_err(|e| Error::msg(format!("team trade {}: {e}", room_id.0)))?;
        commit_trade_room(assignment, room_id, &hit, trade_pool, used)?;
    }
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
    assign_power_rooms(blueprint, pool, table, layout, options, &room_ids, assignment, used)
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
        let elite = pool
            .entry(&station.hit.name)
            .map(|e| e.elite)
            .unwrap_or(0);
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
        Some(RoomProduct::Factory { .. }) => {
            Err(Error::msg(format!("trade room {} has factory product", room.id.0)))
        }
        None => Err(Error::msg(format!("trade room {} missing product", room.id.0))),
    }
}

/// 但书干员名（合同法/违约 docus 机制核心，效率 ≈ 纸面工具效率 × 1.55）。
const DOCUS_TRADE_NAME: &str = "但书";

/// 团队贸易站取人：但书（docus）最高优先 → 否则纸面贪心。
///
/// 但书房间效率 = (1 + 队友订单效率/100) × 1.55，是全基建最高产贸易位；纯按
/// `effective_eff_multiplier` 排序的搜索不一定把它顶到最前，故在此显式置顶，保证它落到
/// 最先填充的峰值队（最长班 + 最佳队友）。
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
    pick_trade_hit(
        pool,
        table,
        trade_room_options(layout, gold_lines, options, order),
        SearchTripleFilter::default(),
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
    pick_disjoint_from_report(report.best, report.top, trade_hit_names, used, "no disjoint trade triple")
}

fn pick_manu_hit(
    pool: &ManuPool,
    table: &SkillTable,
    search_opts: ManuSearchOptions,
    used: &HashSet<String>,
    top_k: usize,
) -> Result<ManuSearchHit> {
    let sub = filter_manufacture_pool(pool, used);
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
    hits.into_iter()
        .find(|h| names_disjoint(names_of(h), used))
}

fn pick_disjoint_from_report<T: Clone>(
    best: T,
    top: Vec<T>,
    names_of: impl Fn(&T) -> &[String],
    used: &HashSet<String>,
    err: &str,
) -> Result<T> {
    pick_first_disjoint(top.into_iter().chain(std::iter::once(best)), &names_of, used)
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
    use crate::operbox::{default_operbox_gongsun_path, OperBox};
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
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
        if !operbox.owns("八幡海铃") || !operbox.owns("但书") || !operbox.owns("伺夜") || !operbox.owns("贝洛内")
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
        // 迷迭香链「定位不定队友」：黑键定贸易站、迷迭香定制造站，各自补满三人。
        let blackkey_room = assignment.rooms.iter().find(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "黑键")
        });
        let blackkey_room = blackkey_room.expect("黑键应定位某贸易站");
        assert_eq!(blackkey_room.operators.len(), 3, "黑键站应补满三人: {:?}", blackkey_room.operators);

        let rosemary_room = assignment.rooms.iter().find(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
                && r.operators.iter().any(|o| o.name == "迷迭香")
        });
        let rosemary_room = rosemary_room.expect("迷迭香应定位某制造站");
        assert_eq!(rosemary_room.operators.len(), 3, "迷迭香站应补满三人: {:?}", rosemary_room.operators);

        // 但书必在岗；§5.1 评分门控可能把黑键并入但书站（替换工具人），故只校验
        // 「但书三人组完整」或「但书+黑键同站」二者其一，且黑键在某满员贸易站。
        let blackkey_in_full_trade = blackkey_room.operators.len() == 3;
        assert!(blackkey_in_full_trade, "黑键应在满员贸易站: {:?}", blackkey_room.operators);
        assert!(
            !blackkey_witch_same_trade_room(&assignment, &blueprint),
            "黑键不得与巫恋同站: {:?}",
            blackkey_room.operators
        );
        let docus_intact = assignment.rooms.iter().any(|r| {
            r.operators.iter().any(|o| o.name == "但书")
                && r.operators.iter().any(|o| o.name == "伺夜")
                && r.operators.iter().any(|o| o.name == "贝洛内")
        });
        let docus_blackkey_colocated = assignment.rooms.iter().any(|r| {
            r.operators.iter().any(|o| o.name == "但书")
                && r.operators.iter().any(|o| o.name == "黑键")
        });
        assert!(
            docus_intact || docus_blackkey_colocated,
            "但书应保持工具三人组，或按 §5.1 与黑键同站"
        );

        // 感知 producer（爱丽丝/车尔尼宿舍、絮雨办公室）作为可选 slot 进驻。
        let dorm_producers: HashSet<_> = blueprint
            .rooms
            .iter()
            .filter(|b| b.kind == FacilityKind::Dormitory)
            .flat_map(|b| assignment.operators_in(&b.id))
            .map(|o| o.name.clone())
            .collect();
        assert!(
            dorm_producers.contains("爱丽丝") && dorm_producers.contains("车尔尼"),
            "爱丽丝/车尔尼应作为感知 producer 进驻宿舍: {:?}",
            dorm_producers
        );
        if operbox.owns("絮雨") {
            let office: HashSet<_> = blueprint
                .rooms
                .iter()
                .filter(|b| b.kind == FacilityKind::Office)
                .flat_map(|b| assignment.operators_in(&b.id))
                .map(|o| o.name.clone())
                .collect();
            assert!(office.contains("絮雨"), "絮雨应作为办公室感知 producer 进驻: {:?}", office);
        }
        let control_ops = assignment.control_operators();
        let control: HashSet<_> = control_ops.iter().map(|o| o.name.as_str()).collect();
        assert!(control.contains("夕"), "control: {:?}", control);
        assert!(control.contains("八幡海铃"), "control: {:?}", control);
        assert!(control.contains("斩业星熊") && control.contains("诗怀雅"), "control: {:?}", control);
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
        let mut seen = HashSet::new();
        for room in &assignment.rooms {
            for op in &room.operators {
                assert!(
                    seen.insert(op.name.clone()),
                    "duplicate {}",
                    op.name
                );
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
    fn assign_full_e2_peak_manu_teams_match_gongsun() {
        use crate::manufacture::{ManuRoomInput, solve_manufacture};
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
            peak
                .operators_in(&RoomId::from(room_id))
                .iter()
                .map(|o| o.name.clone())
                .collect()
        };

        let gold_trio: HashSet<_> = GONGSUN_GOLD_MANU_TEAM.iter().map(|s| *s).collect();
        let gold_room = peak.rooms.iter().find(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| {
                    b.id == r.room_id
                        && b.kind == FacilityKind::Factory
                        && matches!(
                            b.product.as_ref(),
                            Some(RoomProduct::Factory {
                                recipe: RecipeKind::Gold
                            })
                        )
                })
                && gold_trio.iter().all(|n| r.operators.iter().any(|o| o.name == *n))
        });
        assert!(
            gold_room.is_some(),
            "金线应有清流+温蒂+森蚺，实际制造编制: {:?}",
            peak.rooms.iter().filter(|r| {
                blueprint
                    .rooms
                    .iter()
                    .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
            }).collect::<Vec<_>>()
        );

        let rosemary_ops: HashSet<_> = ROSEMARY_MANU_TEAM.iter().map(|s| *s).collect();
        let rosemary_room = peak.rooms.iter().find(|r| {
            r.operators.iter().any(|o| o.name == ROSEMARY_NAME)
                && rosemary_ops.iter().all(|n| r.operators.iter().any(|o| o.name == *n))
        });
        assert!(
            rosemary_room.is_some(),
            "迷迭香站应为阿罗玛+食铁兽+迷迭香，got {:?}",
            peak
                .rooms
                .iter()
                .find(|r| r.operators.iter().any(|o| o.name == ROSEMARY_NAME))
        );

        let br_winter = room_ops("manu_2");
        assert!(
            !br_winter.contains(&"清流".to_string()),
            "经验线 manu_2 不应占清流 trio: {br_winter:?}"
        );

        let pool = build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
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
            .find(|r| gold_trio.iter().all(|n| r.operators.iter().any(|o| o.name == *n)))
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
        let blueprint = BaseBlueprint::load(
            &crate::skill_table::data_path("layout/snhunt.json").unwrap(),
        )
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
