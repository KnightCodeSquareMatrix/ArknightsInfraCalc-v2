use crate::control::{
    apply_control_to_layout, apply_control_to_layout_with_trace, ControlOperator,
    ControlSettlementTrace,
};
use std::sync::Arc;
use std::time::Instant;

use crate::error::Result;
use crate::global_resource::GlobalResourceKey;
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId, RoomProduct};
use crate::layout::context::{LayoutContext, DEFAULT_DORM_OCCUPANT_COUNT};
use crate::layout::workforce::WorkforceIndex;
use crate::manufacture::input::ManuOperator;
use crate::pool::compile_operator_atoms;
use crate::power::{apply_power_to_layout, PowerOperator};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOperator, TradeOrderKind};
use crate::types::{AtomScope, CompiledAtom, RecipeKind, Selector};

#[derive(Debug, Clone)]
pub struct ResolvedTradeRoom {
    pub id: RoomId,
    pub level: u8,
    pub order: TradeOrderKind,
    pub operators: Vec<TradeOperator>,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedManuRoom {
    pub id: RoomId,
    pub level: u8,
    pub recipe: RecipeKind,
    pub operators: Vec<ManuOperator>,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedPowerRoom {
    pub id: RoomId,
    pub operator: PowerOperator,
    pub layout: LayoutContext,
}

#[derive(Debug, Clone)]
pub struct ResolvedSupportRoom {
    pub id: RoomId,
    pub result: Option<crate::support_facility::SupportRoomResult>,
    pub autofill: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedBase {
    pub blueprint: BaseBlueprint,
    pub assignment: BaseAssignment,
    pub layout: LayoutContext,
    pub trade_rooms: Vec<ResolvedTradeRoom>,
    pub manu_rooms: Vec<ResolvedManuRoom>,
    pub power_rooms: Vec<ResolvedPowerRoom>,
    pub office_rooms: Vec<ResolvedSupportRoom>,
    pub meeting_rooms: Vec<ResolvedSupportRoom>,
}

struct ResolvedBaseCore {
    workforce: WorkforceIndex,
    layout: LayoutContext,
    trace: ResolveTrace,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ResolveTrace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlSettlementTrace>,
}

impl ResolvedBase {
    pub fn gold_manu_line_count(&self) -> u32 {
        self.manu_rooms
            .iter()
            .filter(|r| r.recipe == RecipeKind::Gold)
            .count() as u32
    }

    pub fn layout_snapshot(&self) -> LayoutContext {
        crate::profile::record_resolve_construction(0, 1, 0, 0, 0);
        self.layout.clone()
    }
}

pub fn resolve_base(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    durin_dorm_planning: Option<u8>,
) -> Result<ResolvedBase> {
    let started = Instant::now();
    let _resolve_perf = crate::profile::begin_resolve_perf();
    let result = resolve_base_inner(
        blueprint,
        assignment,
        instances,
        table,
        mood,
        durin_dorm_planning,
        false,
    )
    .map(|resolved| resolved.0);
    crate::profile::record_resolve(started.elapsed());
    result
}

pub fn resolve_base_with_trace(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    durin_dorm_planning: Option<u8>,
) -> Result<(ResolvedBase, ResolveTrace)> {
    let started = Instant::now();
    let _resolve_perf = crate::profile::begin_resolve_perf();
    let result = resolve_base_inner(
        blueprint,
        assignment,
        instances,
        table,
        mood,
        durin_dorm_planning,
        true,
    );
    crate::profile::record_resolve(started.elapsed());
    result
}

/// 蓝图的静态场景输入；实际成员统计在后续结算时填入。
fn layout_from_blueprint(blueprint: &BaseBlueprint) -> LayoutContext {
    LayoutContext {
        meeting_max_level: blueprint.meeting_max_level(),
        training_room_level: blueprint.training_room_max_level(),
        dorm_level_sum: blueprint.dorm_level_sum(),
        facility_level_sum_excl_meeting: blueprint.facility_level_sum_excl_meeting(),
        manu_recipe_kinds: blueprint.manu_recipe_kinds(),
        elite_facility_count: blueprint.scenario.elite_facility_count.unwrap_or(0),
        sui_facility_count: blueprint.scenario.sui_facility_count.unwrap_or(0),
        dorm_occupant_count: effective_dorm_occupants(blueprint, &BaseAssignment::default()),
        trade_station_count: blueprint.count_facility(FacilityKind::TradePost),
        power_station_count: blueprint.count_facility(FacilityKind::PowerPlant),
        manufacture_station_count: blueprint.count_facility(FacilityKind::Factory),
        office_room_count: blueprint.count_facility(FacilityKind::Office),
        meeting_room_count: blueprint.count_facility(FacilityKind::MeetingRoom),
        global: blueprint.initial_global_pool(),
        drone_cap: blueprint.drone_cap,
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_base_core(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    _durin_dorm_planning: Option<u8>,
    trace_enabled: bool,
    stop_before_control: bool,
) -> Result<ResolvedBaseCore> {
    blueprint.validate()?;
    let mut trace = ResolveTrace::default();

    let phase_started = Instant::now();
    let workforce = WorkforceIndex::build(blueprint, assignment, instances);
    crate::profile::record_resolve_phase(
        crate::profile::ResolvePhase::Workforce,
        phase_started.elapsed(),
    );

    let phase_started = Instant::now();
    let mut layout = layout_from_blueprint(blueprint);
    layout.auto_rotation = assignment.auto_rotation;
    layout.work_hours = assignment.work_hours;
    layout.elite_facility_count =
        effective_elite_facility_count(blueprint, assignment, &workforce, instances);
    workforce.apply_to_layout(&mut layout);
    if let Some(instances) = instances {
        workforce.apply_cross_facility_stats(&mut layout, blueprint, Some(instances));
    }
    layout.gold_manu_line_count = blueprint.gold_manu_line_count();
    crate::profile::record_resolve_phase(
        crate::profile::ResolvePhase::Layout,
        phase_started.elapsed(),
    );

    let mut active_global_buffs = std::collections::HashSet::new();
    if let (Some(instances), Some(table)) = (instances, table) {
        let mechanics_started = Instant::now();
        let power_ops: Vec<(RoomId, PowerOperator)> = workforce
            .power_stations
            .iter()
            .map(|entry| {
                Ok((
                    entry.room_id.clone(),
                    to_power_operator(Some(instances), &entry.operator)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        apply_power_to_layout(
            &mut layout,
            &power_ops,
            table,
            mood,
            assignment.work_hours.unwrap_or(24.0),
            &workforce,
            instances,
        )?;
        crate::profile::record_resolve_phase(
            crate::profile::ResolvePhase::Mechanics,
            mechanics_started.elapsed(),
        );

        // 琴柳的中枢条件读取办公室当前联络速度。办公室的实际技能值必须在
        // 中枢结算前可见，避免把后续办公室成员错误地当成不存在。
        let support_started = Instant::now();
        layout.office_hire_spd_pct =
            support_office_hire_speed(blueprint, assignment, &layout, mood)?;
        crate::profile::record_resolve_phase(
            crate::profile::ResolvePhase::Support,
            support_started.elapsed(),
        );
        let mechanics_started = Instant::now();
        if stop_before_control {
            return Ok(ResolvedBaseCore {
                workforce,
                layout,
                trace,
            });
        }
        let control_ops: Vec<ControlOperator> = assignment
            .control_operators()
            .iter()
            .map(|op| to_control_operator(instances, op))
            .collect::<Result<Vec<_>>>()?;
        if !control_ops.is_empty() {
            if trace_enabled {
                let (_, control_trace) =
                    apply_control_to_layout_with_trace(&mut layout, &control_ops, table, mood);
                trace.control = Some(control_trace);
            } else {
                apply_control_to_layout(&mut layout, &control_ops, table, mood);
            }
            layout.control_buffs = control_ops
                .iter()
                .flat_map(|op| {
                    op.buff_ids
                        .iter()
                        .map(move |buff_id| (op.name.clone(), buff_id.clone()))
                })
                .collect();
        }

        crate::office::apply_office_to_layout(
            &mut layout,
            blueprint,
            assignment,
            instances,
            table,
            mood,
        );
        crate::profile::record_resolve_phase(
            crate::profile::ResolvePhase::Mechanics,
            mechanics_started.elapsed(),
        );
    }

    // 跨设施编排层 — 执行 scope=Global 的 atom（在 control/power/office 之后）
    if let (Some(instances), Some(table)) = (instances, table) {
        let phase_started = Instant::now();
        let global_atoms = crate::cross_facility::collect_global_atoms(
            blueprint, assignment, instances, table, &layout,
        );
        if !global_atoms.is_empty() {
            active_global_buffs.extend(
                global_atoms
                    .iter()
                    .map(|entry| (entry.owner_name.clone(), entry.buff_id.clone())),
            );
            let snapshot = crate::cross_facility::orchestrate_global_atoms(
                &global_atoms,
                &layout,
                layout.global.clone(),
            );
            layout.global = snapshot.global;
        }
        crate::profile::record_resolve_phase(
            crate::profile::ResolvePhase::CrossFacility,
            phase_started.elapsed(),
        );
    }

    layout.global.run_conversions(&active_global_buffs);

    if let Some(instances) = instances {
        for room in blueprint.rooms_of(FacilityKind::TradePost) {
            for operator in assignment.operators_in(&room.id) {
                let buffs = instances.resolve_trade_buff_ids(&operator.name, operator.tier());
                let amount = crate::trade::gold_flow::virtual_line_yield(
                    &buffs,
                    layout.gold_manu_line_count,
                    layout.durin_virtual_lines(),
                );
                if amount > 0 {
                    layout.trade_gold_line_sources.push((
                        operator.name.clone(),
                        amount,
                        buffs.iter().any(|id| id == "trade_ord_line_durin[010]"),
                    ));
                    layout
                        .global
                        .add(GlobalResourceKey::VirtualGoldLines, amount as f64);
                }
            }
        }
    }

    Ok(ResolvedBaseCore {
        workforce,
        layout,
        trace,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_base_inner(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    durin_dorm_planning: Option<u8>,
    trace_enabled: bool,
) -> Result<(ResolvedBase, ResolveTrace)> {
    let core = resolve_base_core(
        blueprint,
        assignment,
        instances,
        table,
        mood,
        durin_dorm_planning,
        trace_enabled,
        false,
    )?;
    let ResolvedBaseCore {
        workforce,
        layout,
        trace,
    } = core;
    let blueprint = blueprint.clone();
    let assignment = assignment.clone();
    let support_started = Instant::now();
    let (office_rooms, meeting_rooms) =
        build_support_rooms(&blueprint, &assignment, &layout, mood)?;
    crate::profile::record_resolve_phase(
        crate::profile::ResolvePhase::Support,
        support_started.elapsed(),
    );

    let materialize_started = Instant::now();
    let trade_rooms = build_trade_rooms(&blueprint, &assignment, instances, table, &layout);
    let manu_rooms = build_manu_rooms(&blueprint, &assignment, instances, table, &layout);
    let power_rooms = build_power_rooms(&workforce, instances, &layout);
    crate::profile::record_resolve_phase(
        crate::profile::ResolvePhase::RoomMaterialize,
        materialize_started.elapsed(),
    );
    crate::profile::record_resolve_construction(0, 0, 1, 1, 0);

    Ok((
        ResolvedBase {
            blueprint,
            assignment,
            layout,
            trade_rooms,
            manu_rooms,
            power_rooms,
            office_rooms,
            meeting_rooms,
        },
        trace,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_layout_snapshot(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    mood: f64,
    durin_dorm_planning: Option<u8>,
) -> Result<LayoutContext> {
    let started = Instant::now();
    let _resolve_perf = crate::profile::begin_resolve_perf();
    let result = resolve_base_core(
        blueprint,
        assignment,
        instances,
        table,
        mood,
        durin_dorm_planning,
        false,
        false,
    )
    .map(|core| core.layout);
    crate::profile::record_resolve(started.elapsed());
    result
}

/// The exact snapshot consumed by control settlement. A complete resolve is
/// not interchangeable with this phase: post-control office projections may
/// overwrite fields that the control conditions read.
pub(crate) fn resolve_control_input_layout(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
    durin_dorm_planning: Option<u8>,
) -> Result<LayoutContext> {
    resolve_base_core(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        mood,
        durin_dorm_planning,
        false,
        true,
    )
    .map(|core| core.layout)
}

fn support_office_hire_speed(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    layout: &LayoutContext,
    mood: f64,
) -> Result<f64> {
    use crate::support_facility::{SupportFacility, SupportRegistry, SupportRoomInput};

    let registry = SupportRegistry::load_default_shared()?;
    let extra_recruit_slots = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let mut best: f64 = 0.0;
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
    {
        let operators = support_operators(assignment.operators_in(&room.id), mood);
        if operators.is_empty() {
            continue;
        }
        let result = crate::office::evaluate_office(
            &SupportRoomInput {
                facility: SupportFacility::Office,
                operators,
                capacity: room.operator_capacity(),
                extra_recruit_slots,
                elapsed_hours: layout.work_hours.unwrap_or(24.0),
                external_speed_bonus_pct: 0.0,
                layout: {
                    crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
                    layout.clone()
                },
            },
            &registry,
        )?;
        best = best.max(result.skill_speed_bonus_pct);
    }
    Ok(best)
}

fn build_support_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    layout: &LayoutContext,
    mood: f64,
) -> Result<(Vec<ResolvedSupportRoom>, Vec<ResolvedSupportRoom>)> {
    use crate::support_facility::{SupportFacility, SupportRegistry, SupportRoomInput};

    let registry = SupportRegistry::load_default_shared()?;
    let extra_recruit_slots = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let mut office_rooms = Vec::new();
    let mut meeting_inject = layout.control_meeting_speed_pct;
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
    {
        let operators = support_operators(assignment.operators_in(&room.id), mood);
        if operators.is_empty() {
            office_rooms.push(ResolvedSupportRoom {
                id: room.id.clone(),
                result: None,
                autofill: true,
            });
            continue;
        }
        let result = crate::office::evaluate_office(
            &SupportRoomInput {
                facility: SupportFacility::Office,
                operators,
                capacity: room.operator_capacity(),
                extra_recruit_slots,
                elapsed_hours: layout.work_hours.unwrap_or(24.0),
                external_speed_bonus_pct: 0.0,
                layout: {
                    crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
                    layout.clone()
                },
            },
            &registry,
        )?;
        meeting_inject = meeting_inject.max(result.meeting_speed_inject_pct);
        office_rooms.push(ResolvedSupportRoom {
            id: room.id.clone(),
            result: Some(result),
            autofill: false,
        });
    }

    let mut meeting_rooms = Vec::new();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::MeetingRoom)
    {
        let operators = support_operators(assignment.operators_in(&room.id), mood);
        if operators.is_empty() {
            meeting_rooms.push(ResolvedSupportRoom {
                id: room.id.clone(),
                result: None,
                autofill: true,
            });
            continue;
        }
        let result = crate::meeting::evaluate_meeting(
            &SupportRoomInput {
                facility: SupportFacility::Meeting,
                operators,
                capacity: room.operator_capacity(),
                extra_recruit_slots,
                elapsed_hours: layout.work_hours.unwrap_or(24.0),
                external_speed_bonus_pct: meeting_inject,
                layout: {
                    crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
                    layout.clone()
                },
            },
            &registry,
        )?;
        meeting_rooms.push(ResolvedSupportRoom {
            id: room.id.clone(),
            result: Some(result),
            autofill: false,
        });
    }
    Ok((office_rooms, meeting_rooms))
}

fn support_operators(
    operators: &[AssignedOperator],
    mood: f64,
) -> Vec<crate::support_facility::SupportOperator> {
    operators
        .iter()
        .map(|operator| crate::support_facility::SupportOperator {
            name: operator.name.clone(),
            elite: operator.elite,
            rarity: operator.rarity,
            level: operator.level,
            mood: f64::from(operator.work_mood.unwrap_or(mood.max(0.0) as u8)),
        })
        .collect()
}

pub fn resolve_search_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_243_use_this()?;
    let assignment = BaseAssignment::default();
    Ok(resolve_base(&blueprint, &assignment, None, None, 24.0, None)?.layout)
}

/// 怪猎联动搜索/评估基准：243c 物理布局 + 中枢怪猎双人 → 木天蓼 12。
pub fn snhunt_default_assignment() -> BaseAssignment {
    snhunt_control_assignment(0)
}

/// `elite`：0 = 精0 木天蓼链；2 = 精2 追加全贸易 +7% / 全制造 +2%（需双人同中枢）。
pub fn snhunt_control_assignment(elite: u8) -> BaseAssignment {
    let mut assignment = BaseAssignment::default();
    assignment.set_room(
        "control",
        vec![
            AssignedOperator::new("火龙S黑角", elite),
            AssignedOperator::new("麒麟R夜刀", elite),
        ],
    );
    assignment
}

pub fn resolve_snhunt_elite2_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_snhunt()?;
    let assignment = snhunt_control_assignment(2);
    let instances = OperatorInstances::load(&crate::instances::default_instances_path()?)?;
    let table = SkillTable::load(&crate::skill_table::default_skill_table_path()?)?;
    Ok(resolve_base(
        &blueprint,
        &assignment,
        Some(&instances),
        Some(&table),
        24.0,
        None,
    )?
    .layout)
}

pub fn resolve_snhunt_baseline_layout() -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_snhunt()?;
    let assignment = snhunt_default_assignment();
    let instances = OperatorInstances::load(&crate::instances::default_instances_path()?)?;
    let table = SkillTable::load(&crate::skill_table::default_skill_table_path()?)?;
    Ok(resolve_base(
        &blueprint,
        &assignment,
        Some(&instances),
        Some(&table),
        24.0,
        None,
    )?
    .layout)
}

pub fn resolve_automation_group_1_layout(
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<LayoutContext> {
    let blueprint = BaseBlueprint::template_252_auto1()?;
    let mut assignment = BaseAssignment::default();
    assignment.set_power_operator("power_1", AssignedOperator::new("Lancet-2", 0));
    assignment.set_room("control", vec![AssignedOperator::new("森蚺", 2)]);
    Ok(resolve_base(
        &blueprint,
        &assignment,
        Some(instances),
        Some(table),
        24.0,
        None,
    )?
    .layout)
}

fn effective_elite_facility_count(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    workforce: &WorkforceIndex,
    instances: Option<&OperatorInstances>,
) -> u8 {
    if assignment.has_room_staffing() {
        return workforce.elite_facility_count(blueprint, instances);
    }
    blueprint.scenario.elite_facility_count.unwrap_or(0)
}

fn effective_dorm_occupants(blueprint: &BaseBlueprint, _assignment: &BaseAssignment) -> u8 {
    // 宿舍 producer 上岗 ≠ 全基建休息人数；黑键/迷迭香/乌有链按蓝图规划人数（默认 20）。
    blueprint
        .scenario
        .dorm_occupant_count
        .unwrap_or(DEFAULT_DORM_OCCUPANT_COUNT)
}

/// 通用扣回：对本房干员中 scope=Global 的 StateProduce atom 做扣回。
/// 编排层已统一执行这些 atom 并将结果写入 `layout.global`。
/// Per-room 求解时相同 atom 会再次执行，故须扣回编排层已计数的分量。
fn deduct_room_global_atoms<'a, I>(
    room_layout: &mut LayoutContext,
    compiled_atoms_iter: I,
    layout: &LayoutContext,
) where
    I: IntoIterator<Item = &'a Arc<[CompiledAtom]>>,
{
    for atoms in compiled_atoms_iter {
        for ca in atoms.iter() {
            if ca.atom.scope != AtomScope::Global {
                continue;
            }
            if let crate::types::Action::StateProduce { key, .. } = &ca.atom.action {
                let contribution = scope_global_contribution(&ca.atom, layout);
                if let Some(gk) = crate::global_resource::GlobalResourceKey::parse(key.as_str()) {
                    room_layout.global.add(gk, -contribution);
                }
            }
        }
    }
}

/// 计算 scope=Global 的 StateProduce atom 在跨设施编排层的贡献值。
/// cross_facility/interpreter.rs 的 resolve_selector_value 使用相同的 Selector 求值逻辑。
fn scope_global_contribution(atom: &crate::types::EffectAtom, layout: &LayoutContext) -> f64 {
    let Some(sel) = &atom.selector else {
        return 1.0
            * match &atom.action {
                crate::types::Action::StateProduce { amount, .. } => *amount,
                _ => 0.0,
            };
    };
    let scale = match sel {
        Selector::DormOccupantCount => f64::from(layout.dorm_occupant_count),
        Selector::FacilityLevel => 0.0,
        Selector::FacilityLevelMinusOne => 0.0,
        _ => 0.0,
    };
    match &atom.action {
        crate::types::Action::StateProduce { amount, .. } => scale * amount,
        _ => 0.0,
    }
}

/// 贸易房：扣回 scope=Global atom（编排层已产出）。
fn room_layout_for_trade(layout: &LayoutContext, operators: &[TradeOperator]) -> LayoutContext {
    crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
    let mut room_layout = layout.clone();
    deduct_room_global_atoms(
        &mut room_layout,
        operators.iter().map(|op| &op.compiled_atoms),
        layout,
    );
    room_layout
}

/// 制造房：扣回 scope=Global atom（编排层已产出）。
fn room_layout_for_manu(
    layout: &LayoutContext,
    operators: &[crate::manufacture::ManuOperator],
    table: Option<&SkillTable>,
) -> LayoutContext {
    crate::profile::record_resolve_construction(0, 1, 0, 0, 1);
    let mut room_layout = layout.clone();
    if let Some(table) = table {
        for op in operators {
            let atoms = compile_operator_atoms(&op.buff_ids, table);
            deduct_room_global_atoms(&mut room_layout, std::iter::once(&atoms), layout);
        }
    }
    room_layout
}

fn build_trade_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    layout: &LayoutContext,
) -> Vec<ResolvedTradeRoom> {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::TradePost)
        .filter_map(|room| {
            let RoomProduct::Trade { order } = room.product.as_ref()? else {
                return None;
            };
            let operators: Vec<TradeOperator> = assignment
                .operators_in(&room.id)
                .iter()
                .filter_map(|op| to_trade_operator(instances, table, layout, op).ok())
                .collect();
            Some(ResolvedTradeRoom {
                id: room.id.clone(),
                level: room.level,
                order: *order,
                layout: room_layout_for_trade(layout, &operators),
                operators,
            })
        })
        .collect()
}

fn build_manu_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    layout: &LayoutContext,
) -> Vec<ResolvedManuRoom> {
    blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::Factory)
        .filter_map(|room| {
            let RoomProduct::Factory { recipe } = room.product.as_ref()? else {
                return None;
            };
            let operators: Vec<ManuOperator> = assignment
                .operators_in(&room.id)
                .iter()
                .filter_map(|op| to_manu_operator(instances, table, op).ok())
                .collect();
            Some(ResolvedManuRoom {
                id: room.id.clone(),
                level: room.level,
                recipe: *recipe,
                layout: room_layout_for_manu(layout, &operators, table),
                operators,
            })
        })
        .collect()
}

fn build_power_rooms(
    workforce: &WorkforceIndex,
    instances: Option<&OperatorInstances>,
    layout: &LayoutContext,
) -> Vec<ResolvedPowerRoom> {
    workforce
        .power_stations
        .iter()
        .filter_map(|entry| {
            let op = to_power_operator(instances, &entry.operator).ok()?;
            let room_layout = workforce.layout_for_power_room(
                layout,
                &entry.room_id,
                &entry.operator.name,
                instances,
            );
            Some(ResolvedPowerRoom {
                id: entry.room_id.clone(),
                operator: op,
                layout: room_layout,
            })
        })
        .collect()
}

fn to_trade_operator(
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    _layout: &LayoutContext,
    op: &AssignedOperator,
) -> Result<TradeOperator> {
    let tier = op.tier();
    let buff_ids = resolve_buff_ids(instances, op, "trade")?;
    let compiled_atoms = table
        .map(|t| compile_operator_atoms(&buff_ids, t))
        .unwrap_or_else(|| Arc::from([]));
    Ok(TradeOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags: instance_tags(instances, &op.name, tier),
        compiled_atoms,
    })
}

fn to_manu_operator(
    instances: Option<&OperatorInstances>,
    table: Option<&SkillTable>,
    op: &AssignedOperator,
) -> Result<ManuOperator> {
    let tier = op.tier();
    let buff_ids = resolve_buff_ids(instances, op, "manufacture")?;
    let tags = instance_tags(instances, &op.name, tier);
    Ok(ManuOperator {
        compiled_atoms: table.map(|table| table.compile_manufacture_atoms(&buff_ids)),
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags,
    })
}

fn to_power_operator(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
) -> Result<PowerOperator> {
    let buff_ids = resolve_buff_ids(instances, op, "power")?;
    let mut power = PowerOperator::new(op.name.clone(), op.elite, buff_ids);
    power.work_mood = op.work_mood.map(f64::from);
    Ok(power)
}

fn to_control_operator(
    instances: &OperatorInstances,
    op: &AssignedOperator,
) -> Result<ControlOperator> {
    let tier = op.tier();
    let buff_ids = instances.resolve_control_buff_ids(&op.name, tier);
    let tags = instances.tags_for(&op.name, tier);
    Ok(ControlOperator {
        name: op.name.clone(),
        elite: op.elite,
        buff_ids,
        tags,
    })
}

fn instance_tags(
    instances: Option<&OperatorInstances>,
    name: &str,
    tier: PromotionTier,
) -> Vec<String> {
    instances
        .map(|inst| inst.tags_for(name, tier))
        .unwrap_or_default()
}

fn resolve_buff_ids(
    instances: Option<&OperatorInstances>,
    op: &AssignedOperator,
    facility: &str,
) -> Result<Vec<String>> {
    let Some(instances) = instances else {
        return Ok(Vec::new());
    };
    let tier = op.tier();
    let ids = match facility {
        "trade" => instances.resolve_trade_buff_ids(&op.name, tier),
        "manufacture" => instances.resolve_manufacture_buff_ids(&op.name, tier),
        "power" => instances.resolve_power_buff_ids(&op.name, tier),
        _ => Vec::new(),
    };
    Ok(ids)
}
