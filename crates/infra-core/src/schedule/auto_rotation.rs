//! 每站主力/替补排班。选人、互斥和普通完成复用公共排班 owner。
use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    AssignBaseOptions, AssignShiftResult, AssignedOperator, AssignmentPlan, BaseAssignment,
    BaseBlueprint, FacilityKind, RoomId, RoomProduct,
};
use crate::operbox::OperBox;
use crate::skill_table::SkillTable;

const FALLBACK_WARNING: &str =
    "可用干员太少，部分设施无法使用有技能的干员，已使用 Fallback 随机补位。";

#[derive(Debug, Clone, Serialize)]
pub struct AutoRoomRotation {
    pub room_id: RoomId,
    pub facility: FacilityKind,
    pub main: AutoRoomState,
    pub backup: AutoRoomState,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoRoomState {
    pub operators: Vec<AssignedOperator>,
    pub efficiency: Option<crate::layout::RoomEfficiencySnapshot>,
    pub power_charge_efficiency: Option<crate::Efficiency>,
    pub support_speed_bonus_pct: Option<f64>,
    pub recycle_efficiency: Option<crate::recycle::RecycleEfficiency>,
    pub control_effects: Option<AutoControlEffects>,
    pub evaluation_hours: f64,
    pub first_red_face: Option<String>,
    pub missing_seats: usize,
    pub complete: bool,
}

/// 中枢实际结算的静态加成；沿用效果账本，不在排班层重算机制。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoControlEffects {
    pub trade_bonus_pct: f64,
    pub manufacture_gold_bonus_pct: f64,
    pub manufacture_battle_record_bonus_pct: f64,
    pub manufacture_originium_bonus_pct: f64,
    pub office_speed_bonus_pct: f64,
    pub meeting_speed_bonus_pct: f64,
}

impl AutoControlEffects {
    fn from_layout(layout: &crate::layout::LayoutContext) -> Self {
        use crate::types::RecipeKind;
        Self {
            trade_bonus_pct: layout.global_inject.trade_eff_pct(),
            manufacture_gold_bonus_pct: layout.global_inject.manu_eff_for(RecipeKind::Gold),
            manufacture_battle_record_bonus_pct: layout
                .global_inject
                .manu_eff_for(RecipeKind::BattleRecord),
            manufacture_originium_bonus_pct: layout
                .global_inject
                .manu_eff_for(RecipeKind::Originium),
            office_speed_bonus_pct: layout.control_office_hire_spd_pct,
            meeting_speed_bonus_pct: layout.control_meeting_speed_pct,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoRotationReport {
    pub mode: &'static str,
    pub timing: super::AutoRotationTiming,
    pub complete: bool,
    pub rooms: Vec<AutoRoomRotation>,
    pub constant_dorm_support: Vec<(RoomId, Vec<AssignedOperator>)>,
    pub main_assignment: BaseAssignment,
    pub backup_assignment: BaseAssignment,
    pub assignment_plans: Vec<AssignmentPlan>,
    pub fiammetta: AutoFiammettaConfig,
    pub warnings: Vec<String>,
}

/// Confirmed continuous targets and a repeated Fiammetta service cycle.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoFiammettaConfig {
    pub enabled: bool,
    pub available: bool,
    pub priority: Vec<String>,
    pub targets: Vec<AutoFiammettaTarget>,
    pub dorm_room_id: Option<RoomId>,
    pub service_period_hours: f64,
    pub recovery_per_hour: f64,
    pub exchanges: Vec<super::fiammetta_events::FiammettaExchange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoFiammettaTarget {
    pub target: String,
    pub room_id: RoomId,
    pub state: &'static str,
}

fn configure_auto_fiammetta(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    enabled: bool,
    targets: &[(String, RoomId)],
    dorm_room_id: Option<RoomId>,
    period: f64,
    assignments: [&BaseAssignment; 2],
    instances: &OperatorInstances,
) -> Result<AutoFiammettaConfig> {
    let priority = super::fiammetta_return_priority(blueprint);
    let available = operbox.owns("菲亚梅塔");
    let model = crate::mood::default_mood_model()?;
    if let Some(dorm_id) = &dorm_room_id {
        let dorm = blueprint
            .room(dorm_id)
            .ok_or_else(|| Error::FiammettaCandidateUnavailable("Fiammetta dorm absent".into()))?;
        if dorm.kind != FacilityKind::Dormitory
            || assignments.iter().any(|assignment| {
                assignment
                    .room_of_operator(super::fiammetta_events::FIAMMETTA_NAME)
                    .map(|room| &room.room_id)
                    != Some(dorm_id)
                    || assignment.operators_in(dorm_id).len() >= dorm.operator_capacity()
            })
        {
            return Err(Error::FiammettaCandidateUnavailable(
                "Fiammetta dorm has no exchange seat".into(),
            ));
        }
    }
    let drains = continuous_target_drains(blueprint, instances, assignments, targets)?;
    let exchanges = super::fiammetta_events::continuous_exchanges(model, &drains, period)?;
    Ok(AutoFiammettaConfig {
        enabled,
        available,
        priority: priority.into_iter().map(str::to_string).collect(),
        targets: targets
            .iter()
            .map(|(target, room_id)| AutoFiammettaTarget {
                target: target.clone(),
                room_id: room_id.clone(),
                state: "continuous",
            })
            .collect(),
        dorm_room_id,
        service_period_hours: period,
        recovery_per_hour: super::fiammetta_events::recovery_rate(model),
        exchanges,
    })
}

/// Each room can switch independently. Use the worst actual local/control drain
/// for each supported target, without imposing new admission on other workers.
fn continuous_target_drains(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    assignments: [&BaseAssignment; 2],
    targets: &[(String, RoomId)],
) -> Result<Vec<(String, f64)>> {
    let model = crate::mood::default_mood_model()?;
    let mut drains = vec![0.0_f64; targets.len()];
    for assignment in assignments {
        for control in assignments {
            let mut trial = assignment.clone();
            for room in blueprint
                .rooms
                .iter()
                .filter(|room| room.kind == FacilityKind::ControlCenter)
            {
                trial.set_room(room.id.clone(), control.operators_in(&room.id).to_vec());
            }
            let eta = crate::mood::shift_eta_with_instances(model, blueprint, &trial, instances);
            for (index, (target, room)) in targets.iter().enumerate() {
                let entry = eta
                    .per_op
                    .iter()
                    .find(|entry| entry.name == *target && entry.room_id == room.0)
                    .ok_or_else(|| {
                        Error::FiammettaCandidateUnavailable(
                            "continuous target missing from its production room".into(),
                        )
                    })?;
                drains[index] = drains[index].max(entry.drain_per_hour.max(0.0));
            }
        }
    }
    Ok(targets
        .iter()
        .zip(drains)
        .map(|((target, _), drain)| (target.clone(), drain))
        .collect())
}

pub(crate) fn select_control(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    seed: &mut BaseAssignment,
) -> Result<()> {
    if !blueprint
        .rooms
        .iter()
        .any(|room| room.kind == FacilityKind::ControlCenter)
    {
        return Ok(());
    }
    let control_operbox = crate::pool::auto_rotation_control_roster(operbox, instances, table)?;
    let mut pool =
        crate::pool::build_control_pool_with_fillers(&control_operbox, instances, table)?;
    crate::pool::admit_control_roster(
        &mut pool,
        options.control_work_hours,
        options.mood,
        &seed
            .control_operators()
            .into_iter()
            .map(|op| op.name)
            .collect(),
    )?;
    let layout = crate::layout::resolve_base(
        blueprint,
        seed,
        Some(instances),
        Some(table),
        options.mood,
        None,
    )?
    .layout;
    let mut used = seed.operator_names();
    crate::layout::assign_control_with_saileach(
        seed,
        &pool,
        table,
        &layout,
        options,
        &[],
        &Default::default(),
        None,
        false,
        None,
        &mut used,
    )
}

fn fixed_main_backup_error(stage: &str, reason: impl std::fmt::Display) -> Error {
    Error::msg(format!(
        "自动轮换：当前固定主力下未找到兼容替补方案（{stage}）；第一次主力保持不变：{reason}"
    ))
}

pub fn schedule_auto_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    timing: super::AutoRotationTiming,
) -> Result<AutoRotationReport> {
    timing.validate()?;
    let fiammetta_enabled = options.fiammetta_enable;
    let source_operbox = operbox;
    let mut options = options.clone();
    options.auto_rotation = true;
    // 静态配置单独消费请求开关；公共选人仍不生成定时动作。
    options.fiammetta_enable = false;
    options.control_work_hours = Some(timing.shift_hours[0]);
    options.registry_sustain_hours = None;
    options.skip_standalone_control = true;
    options.shift_hours = timing.shift_hours[0];
    options.mood = crate::mood::default_mood_model()?.mood_cap;
    let source = operbox.excluding(&["菲亚梅塔".to_string()].into_iter().collect());
    let operbox = source.with_auto_roster_eta(blueprint, instances, timing.shift_hours[0])?;
    let mut seed = BaseAssignment::default();
    seed.auto_rotation = true;
    seed.work_hours = Some(options.shift_hours);
    select_control(blueprint, &operbox, instances, table, &options, &mut seed)?;
    let main = super::team_rotation::solve_owned_peak_seed(
        blueprint,
        &operbox,
        instances,
        table,
        &options,
        timing.shift_hours[0],
        &seed,
    )?;
    let mut main = AssignShiftResult {
        assignment: main.assignment,
        plan: main.plan,
    };
    crate::layout::complete_standalone_assignment(
        blueprint, &operbox, instances, table, &options, &mut main,
    )?;
    // 第一次完整主力的工作名单在此冻结；菲亚仅允许已确认目标同房续班。
    let mut backup_options = options.clone();
    backup_options.shift_hours = timing.shift_hours[1];
    backup_options.control_work_hours = Some(timing.shift_hours[1]);
    let backup_operbox =
        source.with_auto_roster_eta(blueprint, instances, timing.shift_hours[1])?;
    let period = timing.shift_hours[0].min(timing.shift_hours[1]);
    let solve_backup =
        |main: &AssignShiftResult, targets: &[(String, RoomId)]| -> Result<AssignShiftResult> {
            let backup = super::team_rotation::solve_policy_backup_with_continuous_targets(
                blueprint,
                &backup_operbox,
                instances,
                table,
                &backup_options,
                &main.assignment,
                timing.shift_hours[1],
                targets,
            )?;
            let mut excluded = main.assignment.work_position_operator_names(blueprint);
            for (target, _) in targets {
                excluded.remove(target);
            }
            let available = backup_operbox.excluding(&excluded);
            let mut backup = AssignShiftResult {
                assignment: backup.assignment,
                plan: backup.plan,
            };
            crate::layout::complete_standalone_assignment(
                blueprint,
                &available,
                instances,
                table,
                &backup_options,
                &mut backup,
            )?;
            Ok(backup)
        };
    let mut selected_targets = Vec::new();
    let mut selected_backup = None;
    let mut selected_dorm = None;
    if fiammetta_enabled && source_operbox.owns(super::fiammetta_events::FIAMMETTA_NAME) {
        // Reserve Fiammetta plus one temporary target seat without evicting supports.
        if let Some(dorm) = blueprint
            .rooms
            .iter()
            .filter(|room| {
                room.kind == FacilityKind::Dormitory
                    && room.operator_capacity() >= main.assignment.operators_in(&room.id).len() + 2
            })
            .min_by_key(|room| main.assignment.operators_in(&room.id).len())
        {
            let mut candidate_main = main.clone();
            let mut dorm_members = candidate_main.assignment.operators_in(&dorm.id).to_vec();
            dorm_members.push(AssignedOperator::from_progress(
                super::fiammetta_events::FIAMMETTA_NAME,
                source_operbox
                    .progress_of(super::fiammetta_events::FIAMMETTA_NAME)
                    .unwrap(),
            ));
            candidate_main
                .assignment
                .set_room(dorm.id.clone(), dorm_members);
            for target in super::fiammetta_return_priority(blueprint) {
                let Some(room) = super::team_rotation::fiammetta_assignment_target_room(
                    blueprint,
                    &main.assignment,
                    target,
                ) else {
                    continue;
                };
                if !backup_operbox.admits_facility(target, blueprint.room(&room).unwrap().kind) {
                    continue;
                }
                let mut targets = selected_targets.clone();
                targets.push((target.to_string(), room));
                // Reject unsupported proposals before rebuilding the backup roster.
                match configure_auto_fiammetta(
                    blueprint,
                    source_operbox,
                    true,
                    &targets,
                    Some(dorm.id.clone()),
                    period,
                    [&candidate_main.assignment, &candidate_main.assignment],
                    instances,
                ) {
                    Ok(_) => {}
                    Err(Error::FiammettaCandidateUnavailable(_)) => continue,
                    Err(error) => return Err(error),
                }
                let mut backup = match solve_backup(&candidate_main, &targets) {
                    Ok(backup) => backup,
                    Err(
                        Error::NoCandidate(_)
                        | Error::FiammettaCandidateUnavailable(_)
                        | Error::OptionalTradeRoleInfeasible { .. }
                        | Error::GoldFlowCoreInfeasible { .. }
                        | Error::ManufactureAnchoredCompletionInfeasible { .. },
                    ) => continue,
                    Err(error) => return Err(error),
                };
                match configure_auto_fiammetta(
                    blueprint,
                    source_operbox,
                    true,
                    &targets,
                    Some(dorm.id.clone()),
                    period,
                    [&candidate_main.assignment, &backup.assignment],
                    instances,
                ) {
                    Ok(_) => {}
                    Err(Error::FiammettaCandidateUnavailable(_)) => continue,
                    Err(error) => return Err(error),
                }
                // Both Plan states declare the same supported production targets.
                backup
                    .plan
                    .active_dependencies
                    .push(crate::layout::ActiveDependency {
                        consumers: targets.iter().map(|(target, _)| target.clone()).collect(),
                        required: vec![super::fiammetta_events::FIAMMETTA_NAME.to_string()],
                    });
                selected_targets = targets;
                selected_backup = Some(backup);
                selected_dorm = Some(dorm.id.clone());
            }
            if selected_backup.is_some() {
                main = candidate_main;
                main.plan
                    .active_dependencies
                    .push(crate::layout::ActiveDependency {
                        consumers: selected_targets
                            .iter()
                            .map(|(target, _)| target.clone())
                            .collect(),
                        required: vec![super::fiammetta_events::FIAMMETTA_NAME.to_string()],
                    });
            }
        }
    }
    let mut backup = match selected_backup {
        Some(backup) => backup,
        None => solve_backup(&main, &[])
            .map_err(|error| fixed_main_backup_error("替补选人与补位", error))?,
    };
    // 替补阶段新增的显式挂件成为两态常驻；归属继承固定主力的工作房。
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Dormitory)
    {
        main.assignment.set_room(
            room.id.clone(),
            backup.assignment.operators_in(&room.id).to_vec(),
        );
    }
    crate::layout::assign::refresh_standalone_efficiency(
        blueprint,
        &operbox,
        instances,
        table,
        &options,
        &mut main.assignment,
    )?;
    main.plan.resolve_actual_producer_dependencies(
        blueprint,
        &main.assignment,
        instances,
        table,
    )?;
    backup.plan.resolve_actual_producer_dependencies(
        blueprint,
        &backup.assignment,
        instances,
        table,
    )?;
    validate_explicit_supports(
        blueprint,
        [&main.assignment, &backup.assignment],
        [&main.plan, &backup.plan],
    )?;
    let model = crate::mood::default_mood_model()?;
    let main_eta =
        crate::mood::shift_eta_with_instances(model, blueprint, &main.assignment, instances);
    let backup_eta =
        crate::mood::shift_eta_with_instances(model, blueprint, &backup.assignment, instances);
    let main_resolved = crate::layout::resolve_base(
        blueprint,
        &main.assignment,
        Some(instances),
        Some(table),
        options.mood,
        None,
    )?;
    let backup_resolved = crate::layout::resolve_base(
        blueprint,
        &backup.assignment,
        Some(instances),
        Some(table),
        options.mood,
        None,
    )?;
    let mut warnings = Vec::new();
    let rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|room| crate::layout::ordinary_workplace(room.kind))
        .map(|room| {
            let state = |assignment: &BaseAssignment,
                         eta: &crate::mood::ShiftEta,
                         resolved: &crate::layout::ResolvedBase|
             -> Result<AutoRoomState> {
                let members = assignment.operators_in(&room.id);
                let bottleneck = eta
                    .per_op
                    .iter()
                    .filter(|entry| entry.room_id == room.id.0)
                    .filter(|entry| {
                        !selected_targets
                            .iter()
                            .any(|(target, _)| *target == entry.name)
                    })
                    .filter_map(|entry| {
                        entry
                            .workable_hours
                            .map(|hours| (entry.name.clone(), hours))
                    })
                    .min_by(|left, right| left.1.total_cmp(&right.1));
                Ok(AutoRoomState {
                    operators: members.to_vec(),
                    efficiency: assignment.efficiency_in(&room.id).cloned(),
                    power_charge_efficiency: (room.kind == FacilityKind::PowerPlant).then(|| {
                        assignment
                            .efficiency_in(&room.id)
                            .map_or(crate::Efficiency::ZERO, |snapshot| {
                                snapshot.power_final_efficiency
                            })
                    }),
                    support_speed_bonus_pct: resolved
                        .office_rooms
                        .iter()
                        .chain(&resolved.meeting_rooms)
                        .find(|support| support.id == room.id)
                        .and_then(|support| support.result.as_ref())
                        .map(|result| result.total_speed_bonus_pct),
                    recycle_efficiency: (room.kind == FacilityKind::Recycle)
                        .then(|| {
                            crate::recycle::evaluate_recycle(room.level, members, instances, table)
                        })
                        .transpose()?,
                    control_effects: (room.kind == FacilityKind::ControlCenter)
                        .then(|| AutoControlEffects::from_layout(&resolved.layout)),
                    evaluation_hours: assignment.work_hours.expect("auto request work duration"),
                    first_red_face: bottleneck.map(|(name, _)| name),
                    missing_seats: room.operator_capacity().saturating_sub(members.len()),
                    complete: members.len() == room.operator_capacity(),
                })
            };
            Ok(AutoRoomRotation {
                room_id: room.id.clone(),
                facility: room.kind,
                main: state(&main.assignment, &main_eta, &main_resolved)?,
                backup: state(&backup.assignment, &backup_eta, &backup_resolved)?,
            })
        })
        .collect::<Result<_>>()?;
    for room in &rooms {
        for (label, state) in [("主力", &room.main), ("替补", &room.backup)] {
            if !state.complete {
                warnings.push(format!(
                    "{} {}缺{}人，排班不完整",
                    room.room_id.0, label, state.missing_seats
                ));
            }
        }
    }
    if !main.assignment.fallback_operators.is_empty()
        || !backup.assignment.fallback_operators.is_empty()
    {
        warnings.push(FALLBACK_WARNING.to_string());
    }
    let constant_dorm_support = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Dormitory)
        .filter_map(|room| {
            let members = main.assignment.operators_in(&room.id);
            (!members.is_empty()).then(|| (room.id.clone(), members.to_vec()))
        })
        .collect();
    let mut report = AutoRotationReport {
        mode: "auto_rotation",
        timing,
        complete: rooms
            .iter()
            .all(|room| room.main.complete && room.backup.complete),
        rooms,
        constant_dorm_support,
        fiammetta: configure_auto_fiammetta(
            blueprint,
            source_operbox,
            fiammetta_enabled,
            &selected_targets,
            selected_dorm,
            period,
            [&main.assignment, &backup.assignment],
            instances,
        )?,
        main_assignment: main.assignment,
        backup_assignment: backup.assignment,
        assignment_plans: vec![main.plan, backup.plan],
        warnings,
    };
    report
        .warnings
        .extend(auto_work_duration_issues(blueprint, instances, &report)?);
    Ok(report)
}

pub fn assert_auto_rotation_invariants(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &AutoRotationReport,
    fiammetta_enabled: bool,
) -> Result<()> {
    if report.mode != "auto_rotation" || report.assignment_plans.len() != 2 {
        return Err(Error::msg("assert-invariants auto_report_shape failed"));
    }
    crate::layout::assert_auto_rotation_plan_policy(&report.assignment_plans)?;
    report.timing.validate()?;
    let targets = report
        .fiammetta
        .targets
        .iter()
        .map(|target| (target.target.clone(), target.room_id.clone()))
        .collect::<Vec<_>>();
    let priority = super::fiammetta_return_priority(blueprint);
    let mut last_priority = None;
    for target in &report.fiammetta.targets {
        let position = priority
            .iter()
            .position(|name| *name == target.target)
            .ok_or_else(|| Error::msg("assert-invariants auto_fiammetta_priority failed"))?;
        if last_priority.is_some_and(|last| position <= last)
            || target.state != "continuous"
            || [&report.main_assignment, &report.backup_assignment]
                .iter()
                .any(|assignment| {
                    super::team_rotation::fiammetta_assignment_target_room(
                        blueprint,
                        assignment,
                        &target.target,
                    )
                    .as_ref()
                        != Some(&target.room_id)
                })
            || report.assignment_plans.iter().any(|plan| {
                !plan.active_dependencies.iter().any(|dependency| {
                    dependency.consumers.contains(&target.target)
                        && dependency
                            .required
                            .iter()
                            .any(|name| name == super::fiammetta_events::FIAMMETTA_NAME)
                })
            })
            || !report.assignment_plans[1].anchors.iter().any(|anchor| {
                anchor.system_id == "fiammetta_continuous"
                    && anchor.operator == target.target
                    && anchor.room_id.as_ref() == Some(&target.room_id)
            })
        {
            return Err(Error::msg(
                "assert-invariants auto_fiammetta_continuous_placement failed",
            ));
        }
        last_priority = Some(position);
    }
    if !(fiammetta_enabled && operbox.owns(super::fiammetta_events::FIAMMETTA_NAME))
        && !targets.is_empty()
    {
        return Err(Error::msg(
            "assert-invariants auto_fiammetta_disabled failed",
        ));
    }
    if targets.is_empty() != report.fiammetta.dorm_room_id.is_none() {
        return Err(Error::msg("assert-invariants auto_fiammetta_dorm failed"));
    }
    for assignment in [&report.main_assignment, &report.backup_assignment] {
        let room = assignment.room_of_operator(super::fiammetta_events::FIAMMETTA_NAME);
        if let Some(dorm_id) = &report.fiammetta.dorm_room_id {
            let dorm = blueprint
                .room(dorm_id)
                .ok_or_else(|| Error::msg("assert-invariants auto_fiammetta_dorm failed"))?;
            if dorm.kind != FacilityKind::Dormitory
                || room.map(|room| &room.room_id) != Some(dorm_id)
                || assignment.operators_in(dorm_id).len() >= dorm.operator_capacity()
            {
                return Err(Error::msg("assert-invariants auto_fiammetta_dorm failed"));
            }
        } else if room.is_some() {
            return Err(Error::msg("assert-invariants auto_fiammetta_dorm failed"));
        }
    }
    let model = crate::mood::default_mood_model()?;
    let period = report.timing.shift_hours[0].min(report.timing.shift_hours[1]);
    if report.fiammetta.enabled != fiammetta_enabled
        || report.fiammetta.available != operbox.owns(super::fiammetta_events::FIAMMETTA_NAME)
        || report.fiammetta.priority
            != priority
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        || report.fiammetta.service_period_hours != period
        || report.fiammetta.recovery_per_hour != super::fiammetta_events::recovery_rate(model)
    {
        return Err(Error::msg(
            "assert-invariants auto_fiammetta_exchange_budget failed",
        ));
    }
    let drains = continuous_target_drains(
        blueprint,
        instances,
        [&report.main_assignment, &report.backup_assignment],
        &targets,
    )?;
    super::fiammetta_events::validate_continuous_exchanges(
        model,
        &drains,
        period,
        &report.fiammetta.exchanges,
    )?;
    validate_explicit_supports(
        blueprint,
        [&report.main_assignment, &report.backup_assignment],
        [&report.assignment_plans[0], &report.assignment_plans[1]],
    )?;
    let expected_rooms = blueprint
        .rooms
        .iter()
        .filter(|room| crate::layout::ordinary_workplace(room.kind))
        .map(|room| &room.id.0)
        .collect::<std::collections::BTreeSet<_>>();
    let actual_rooms = report
        .rooms
        .iter()
        .map(|room| &room.room_id.0)
        .collect::<std::collections::BTreeSet<_>>();
    if expected_rooms != actual_rooms || actual_rooms.len() != report.rooms.len() {
        return Err(Error::msg("assert-invariants auto_room_coverage failed"));
    }
    let expected_dorm = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Dormitory)
        .filter_map(|room| {
            (!report.main_assignment.operators_in(&room.id).is_empty()).then(|| {
                (
                    room.id.clone(),
                    report.main_assignment.operators_in(&room.id).to_vec(),
                )
            })
        })
        .collect::<Vec<_>>();
    if report.constant_dorm_support != expected_dorm
        || blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::Dormitory)
            .any(|room| {
                report.main_assignment.operators_in(&room.id)
                    != report.backup_assignment.operators_in(&room.id)
            })
    {
        return Err(Error::msg("assert-invariants auto_constant_support failed"));
    }
    let work = report
        .main_assignment
        .work_position_operator_names(blueprint);
    let backup_work = report
        .backup_assignment
        .work_position_operator_names(blueprint);
    let actual_shared = work
        .intersection(&backup_work)
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let allowed_shared = targets
        .iter()
        .map(|(target, _)| target.clone())
        .collect::<std::collections::HashSet<_>>();
    if actual_shared != allowed_shared {
        return Err(Error::msg("assert-invariants auto_work_disjoint failed"));
    }
    for (index, assignment) in [&report.main_assignment, &report.backup_assignment]
        .into_iter()
        .enumerate()
    {
        let admitted =
            operbox.with_auto_roster_eta(blueprint, instances, report.timing.shift_hours[index])?;
        let mut fallback_seen = std::collections::HashSet::new();
        for name in &assignment.fallback_operators {
            if !fallback_seen.insert(name)
                || !blueprint.rooms.iter().any(|room| {
                    crate::layout::ordinary_workplace(room.kind)
                        && assignment
                            .operators_in(&room.id)
                            .iter()
                            .any(|op| &op.name == name)
                })
            {
                return Err(Error::msg(
                    "assert-invariants auto_fallback_identity failed",
                ));
            }
        }
        for room in &blueprint.rooms {
            if assignment.operators_in(&room.id).iter().any(|operator| {
                !assignment.fallback_operators.contains(&operator.name)
                    && !admitted.admits_facility(&operator.name, room.kind)
            }) {
                return Err(Error::msg(format!(
                    "assert-invariants auto_initial_eta_admission failed: {}",
                    room.id.0
                )));
            }
        }
        if !assignment.auto_rotation
            || assignment.work_hours != Some(report.timing.shift_hours[index])
        {
            return Err(Error::msg(
                "assert-invariants auto_work_duration_input failed",
            ));
        }
        super::team_rotation::assert_assignment_identity(
            blueprint,
            operbox,
            assignment,
            &report.assignment_plans[index],
        )?;
        super::shift_bind::validate_assignment_dependency_presence(
            blueprint,
            assignment,
            &report.assignment_plans[index],
        )
        .map_err(Error::msg)?;
        for anchor in &report.assignment_plans[index].anchors {
            if anchor.room_id.as_ref().is_none_or(|room| {
                !assignment
                    .operators_in(room)
                    .iter()
                    .any(|operator| operator.name == anchor.operator)
            }) {
                return Err(Error::msg("assert-invariants auto_plan_placement failed"));
            }
        }
        let model = crate::mood::default_mood_model()?;
        let eta = crate::mood::shift_eta_with_instances(model, blueprint, assignment, instances);
        let options = AssignBaseOptions {
            auto_rotation: true,
            mood: model.mood_cap,
            shift_hours: report.timing.shift_hours[index],
            ..Default::default()
        };
        let mut recalculated = assignment.clone();
        crate::layout::assign::refresh_standalone_efficiency(
            blueprint,
            operbox,
            instances,
            table,
            &options,
            &mut recalculated,
        )?;
        let resolved = crate::layout::resolve_base(
            blueprint,
            assignment,
            Some(instances),
            Some(table),
            model.mood_cap,
            None,
        )?;
        for room_rotation in &report.rooms {
            let state = if index == 0 {
                &room_rotation.main
            } else {
                &room_rotation.backup
            };
            let room = blueprint
                .room(&room_rotation.room_id)
                .ok_or_else(|| Error::msg("assert-invariants auto_room_owner failed"))?;
            if assignment.operators_in(&room.id) != state.operators
                || state.missing_seats
                    != room
                        .operator_capacity()
                        .saturating_sub(state.operators.len())
                || state.complete != (state.missing_seats == 0)
            {
                return Err(Error::msg("assert-invariants auto_shortfall failed"));
            }
            if room.kind != room_rotation.facility || !crate::layout::ordinary_workplace(room.kind)
            {
                return Err(Error::msg("assert-invariants auto_room_owner failed"));
            }
            if state.efficiency.as_ref() != recalculated.efficiency_in(&room.id) {
                return Err(Error::msg(
                    "assert-invariants auto_efficiency_snapshot failed",
                ));
            }
            let power = (room.kind == FacilityKind::PowerPlant).then(|| {
                recalculated
                    .efficiency_in(&room.id)
                    .map_or(crate::Efficiency::ZERO, |snapshot| {
                        snapshot.power_final_efficiency
                    })
            });
            let support = resolved
                .office_rooms
                .iter()
                .chain(&resolved.meeting_rooms)
                .find(|support| support.id == room.id)
                .and_then(|support| support.result.as_ref())
                .map(|result| result.total_speed_bonus_pct);
            let control = (room.kind == FacilityKind::ControlCenter)
                .then(|| AutoControlEffects::from_layout(&resolved.layout));
            if state.power_charge_efficiency != power
                || state.support_speed_bonus_pct != support
                || state.control_effects != control
                || state.recycle_efficiency
                    != (room.kind == FacilityKind::Recycle)
                        .then(|| {
                            crate::recycle::evaluate_recycle(
                                room.level,
                                &state.operators,
                                instances,
                                table,
                            )
                        })
                        .transpose()?
            {
                return Err(Error::msg("assert-invariants auto_aux_efficiency failed"));
            }
            let first = eta
                .per_op
                .iter()
                .filter(|entry| entry.room_id == room.id.0)
                .filter(|entry| !allowed_shared.contains(&entry.name))
                .filter_map(|entry| entry.workable_hours.map(|hours| (&entry.name, hours)))
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(name, _)| name.clone());
            if state.first_red_face != first {
                return Err(Error::msg("assert-invariants auto_first_red_face failed"));
            }
            let hours = report.timing.shift_hours[index];
            if !state.evaluation_hours.is_finite() || (hours - state.evaluation_hours).abs() > 1e-8
            {
                return Err(Error::msg("assert-invariants auto_room_window failed"));
            }
            if let Some(RoomProduct::Factory { .. }) = &room.product {
                let operators = state
                    .operators
                    .iter()
                    .map(|op| {
                        crate::manufacture::ManuOperator::new(
                            &op.name,
                            op.elite,
                            instances.resolve_manufacture_buff_ids(&op.name, op.tier()),
                        )
                    })
                    .collect::<Vec<_>>();
                if !state
                    .operators
                    .iter()
                    .any(|operator| assignment.fallback_operators.contains(&operator.name))
                    && !crate::search::manufacture_peers_compatible(&operators, table)
                {
                    return Err(Error::msg(
                        "assert-invariants auto_manufacture_peer_group failed",
                    ));
                }
            }
        }
    }
    let mut expected_warnings = report
        .rooms
        .iter()
        .flat_map(|room| {
            [("主力", &room.main), ("替补", &room.backup)]
                .into_iter()
                .filter(|(_, state)| !state.complete)
                .map(move |(label, state)| {
                    format!(
                        "{} {}缺{}人，排班不完整",
                        room.room_id.0, label, state.missing_seats
                    )
                })
        })
        .collect::<Vec<_>>();
    if !report.main_assignment.fallback_operators.is_empty()
        || !report.backup_assignment.fallback_operators.is_empty()
    {
        expected_warnings.push(FALLBACK_WARNING.to_string());
    }
    expected_warnings.extend(auto_work_duration_issues(blueprint, instances, report)?);
    if report.warnings != expected_warnings {
        return Err(Error::msg(
            "assert-invariants auto_quality_annotations failed",
        ));
    }
    if report.complete
        != report
            .rooms
            .iter()
            .all(|room| room.main.complete && room.backup.complete)
    {
        return Err(Error::msg("assert-invariants auto_complete failed"));
    }
    Ok(())
}

/// 显式质量检查以实际两态名单重建完整 ETA，不参与选人。
fn auto_work_duration_issues(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    report: &AutoRotationReport,
) -> Result<Vec<String>> {
    let model = crate::mood::default_mood_model()?;
    let mut issues = Vec::new();
    let assignments = [&report.main_assignment, &report.backup_assignment];
    // 当前唯一外站心情项是深海人数加耗：逐房取人数最多的真实名单，构造可共存混搭。
    let mut worst_outside = BaseAssignment::default();
    for room in &blueprint.rooms {
        if matches!(
            room.kind,
            FacilityKind::ControlCenter | FacilityKind::Dormitory
        ) {
            continue;
        }
        let members = assignments
            .iter()
            .map(|assignment| assignment.operators_in(&room.id))
            .max_by_key(|members| {
                members
                    .iter()
                    .filter(|op| {
                        instances
                            .tags_for(&op.name, op.tier())
                            .iter()
                            .any(|tag| tag == "cc.g.abyssal")
                    })
                    .count()
            })
            .unwrap();
        worst_outside.set_room(room.id.clone(), members.to_vec());
    }
    for (index, assignment) in assignments.into_iter().enumerate() {
        let hours = report.timing.shift_hours[index];
        // 非中枢成员只依赖本房与当时中枢；检查本态名单在两个实际中枢下的 ETA。
        for control in assignments {
            let mut trial = assignment.clone();
            for room in blueprint
                .rooms
                .iter()
                .filter(|room| room.kind == FacilityKind::ControlCenter)
            {
                trial.set_room(room.id.clone(), control.operators_in(&room.id).to_vec());
            }
            let eta = crate::mood::shift_eta_with_instances(model, blueprint, &trial, instances);
            for member in &eta.per_op {
                if report
                    .fiammetta
                    .targets
                    .iter()
                    .any(|target| target.target == member.name)
                {
                    continue;
                }
                if blueprint
                    .room(&RoomId::from(member.room_id.as_str()))
                    .is_some_and(|room| room.kind == FacilityKind::ControlCenter)
                {
                    continue;
                }
                if !crate::mood::fits_work_budget(
                    model,
                    model.mood_cap,
                    member.drain_per_hour,
                    hours,
                ) {
                    issues.push(format!(
                        "自动轮换心情不足：{} {} 需工作 {hours} 小时，预计可工作 {:?} 小时",
                        member.room_id, member.name, member.workable_hours,
                    ));
                }
            }
        }
        let mut trial = worst_outside.clone();
        for room in blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::ControlCenter)
        {
            trial.set_room(room.id.clone(), assignment.operators_in(&room.id).to_vec());
        }
        let eta = crate::mood::shift_eta_with_instances(model, blueprint, &trial, instances);
        for member in &eta.per_op {
            if blueprint
                .room(&RoomId::from(member.room_id.as_str()))
                .is_some_and(|room| room.kind == FacilityKind::ControlCenter)
                && !crate::mood::fits_work_budget(
                    model,
                    model.mood_cap,
                    member.drain_per_hour,
                    hours,
                )
            {
                issues.push(format!(
                    "自动轮换混搭中枢心情不足：{} 需工作 {hours} 小时，预计可工作 {:?} 小时",
                    member.name, member.workable_hours,
                ));
            }
        }
    }
    issues.sort();
    issues.dedup();
    Ok(issues)
}

/// 独立从两个最终 Plan 和实际岗位重建显式挂件归属。
fn validate_explicit_supports(
    blueprint: &BaseBlueprint,
    assignments: [&BaseAssignment; 2],
    plans: [&AssignmentPlan; 2],
) -> Result<()> {
    let mut claims = std::collections::BTreeMap::new();
    for (assignment, plan) in assignments.into_iter().zip(plans) {
        claims = crate::layout::claim_support_rooms(
            assignment,
            &plan.anchors,
            &plan.active_dependencies,
            &claims,
        )
        .ok_or_else(|| Error::msg("assert-invariants auto_explicit_support_ownership failed"))?;
        for support in claims.keys() {
            if !blueprint
                .rooms
                .iter()
                .filter(|room| room.kind == FacilityKind::Dormitory)
                .any(|room| {
                    assignment
                        .operators_in(&room.id)
                        .iter()
                        .any(|op| op.name == *support)
                })
            {
                return Err(Error::msg("assert-invariants auto_constant_support failed"));
            }
        }
    }
    Ok(())
}
