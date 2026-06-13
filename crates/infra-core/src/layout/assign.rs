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
    search_manufacture_triples, search_power_top, search_trade_triples,
    search_trade_triples_filtered, control_entry_hr_mood_fill, ControlFillPolicy,
    ControlSearchOptions, ManuSearchHit, MATATABI_CONSUMER_NAME,
    ManuSearchOptions, PowerSearchOptions, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
};
use crate::skill_table::SkillTable;
use crate::layout::LayoutContext;
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
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

/// 中枢 + 宿舍（三班钉死，从高峰班拷贝）。
pub fn pinned_assignment(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
) -> BaseAssignment {
    let mut pinned = BaseAssignment::default();
    for room in &assignment.rooms {
        let Some(bp) = blueprint.rooms.iter().find(|r| r.id == room.room_id) else {
            continue;
        };
        if !matches!(bp.kind, FacilityKind::ControlCenter | FacilityKind::Dormitory) {
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
    }
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
    for room in blueprint.rooms.iter().filter(|r| r.kind == FacilityKind::Factory) {
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

fn assign_power_stations(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let power_rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::PowerPlant)
        .collect();
    if power_rooms.is_empty() {
        return Ok(());
    }

    let power_opts = PowerSearchOptions {
        station_count: power_rooms.len().min(255) as u8,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: layout.clone(),
    };

    for room in power_rooms {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let sub = filter_power_pool(pool, used);
        let hits = search_power_top(&sub, table, &power_opts, options.top_k)?;
        let hit = hits
            .into_iter()
            .find(|h| !used.contains(&h.name))
            .ok_or_else(|| Error::msg(format!("power {}: no available operator", room.id.0)))?;
        let elite = pool.entry(&hit.name).map(|e| e.elite).unwrap_or(0);
        used.insert(hit.name.clone());
        assignment.set_power_operator(room.id.clone(), AssignedOperator::new(hit.name, elite));
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
        let trade_1: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_1"))
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert!(trade_1.contains("黑键"), "trade_1: {:?}", trade_1);
        assert!(trade_1.contains("吉星"), "trade_1: {:?}", trade_1);
        assert!(trade_1.contains("可露希尔"), "trade_1: {:?}", trade_1);
        let trade_2: HashSet<_> = assignment
            .operators_in(&RoomId::from("trade_2"))
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert!(trade_2.contains("但书"), "trade_2: {:?}", trade_2);
        assert!(trade_2.contains("伺夜"));
        assert!(trade_2.contains("贝洛内"));
        let manu_4: HashSet<_> = assignment
            .operators_in(&RoomId::from("manu_4"))
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert!(manu_4.contains("迷迭香"), "manu_4: {:?}", manu_4);
        assert!(manu_4.contains("阿罗玛"), "manu_4: {:?}", manu_4);
        assert!(manu_4.contains("砾"), "manu_4: {:?}", manu_4);
        let dorm_1: HashSet<_> = assignment
            .operators_in(&RoomId::from("dorm_1"))
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert!(dorm_1.contains("车尔尼"), "dorm_1: {:?}", dorm_1);
        assert!(dorm_1.contains("爱丽丝"), "dorm_1: {:?}", dorm_1);
        assert!(dorm_1.contains("塑心"), "dorm_1: {:?}", dorm_1);
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
