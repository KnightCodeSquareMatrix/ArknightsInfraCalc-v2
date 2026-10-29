//! 每站主力/替补建议。选人、互斥和普通完成复用公共排班 owner。
use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    AssignBaseOptions, AssignShiftResult, AssignedOperator, AssignmentPlan, BaseAssignment,
    BaseBlueprint, FacilityKind, RoomId, RoomProduct,
};
use crate::operbox::OperBox;
use crate::skill_table::SkillTable;

#[derive(Debug, Clone, Serialize)]
pub struct AutoRoomRecommendation {
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
    pub control_effects: Option<AutoControlEffects>,
    pub evaluation_hours: f64,
    pub first_red_face: Option<String>,
    pub missing_seats: usize,
    pub complete: bool,
}

/// 中枢实际结算的静态加成；沿用效果账本，不在建议层重算机制。
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
    pub rooms: Vec<AutoRoomRecommendation>,
    pub constant_dorm_support: Vec<(RoomId, Vec<AssignedOperator>)>,
    pub main_assignment: BaseAssignment,
    pub backup_assignment: BaseAssignment,
    pub assignment_plans: Vec<AssignmentPlan>,
    pub warnings: Vec<String>,
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
    let pool = crate::pool::build_control_pool_with_fillers(operbox, instances, table)?;
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

pub fn schedule_auto_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    timing: super::AutoRotationTiming,
) -> Result<AutoRotationReport> {
    timing.validate()?;
    let mut options = options.clone();
    options.auto_rotation = true;
    options.fiammetta_enable = false;
    options.control_work_hours = None;
    options.registry_sustain_hours = None;
    options.skip_standalone_control = true;
    options.shift_hours = timing.shift_hours[0];
    options.mood = crate::mood::default_mood_model()?.mood_cap;
    let operbox = operbox.excluding(&["菲亚梅塔".to_string()].into_iter().collect());
    let mut seed = BaseAssignment::default();
    seed.auto_rotation = true;
    seed.work_hours = Some(options.shift_hours);
    select_control(blueprint, &operbox, instances, table, &options, &mut seed)?;
    let mut backup_options = options.clone();
    backup_options.shift_hours = timing.shift_hours[1];
    let mut backup_seed = BaseAssignment::default();
    backup_seed.auto_rotation = true;
    backup_seed.work_hours = Some(backup_options.shift_hours);
    backup_seed.auto_other_presence =
        crate::layout::admission::presence(blueprint, &seed, instances);
    select_control(
        blueprint,
        &operbox.excluding(&seed.work_position_operator_names(blueprint)),
        instances,
        table,
        &backup_options,
        &mut backup_seed,
    )?;
    seed.auto_other_presence =
        crate::layout::admission::presence(blueprint, &backup_seed, instances);
    let main_available = operbox.excluding(&backup_seed.work_position_operator_names(blueprint));
    let main = super::team_rotation::solve_owned_peak_seed(
        blueprint,
        &main_available,
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
        blueprint,
        &main_available,
        instances,
        table,
        &options,
        &mut main,
    )?;
    let backup = super::team_rotation::solve_policy_disjoint_backup(
        blueprint,
        &operbox,
        instances,
        table,
        &backup_options,
        &main.assignment,
        timing.shift_hours[1],
    )?;
    let available = operbox.excluding(&main.assignment.work_position_operator_names(blueprint));
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
    // 替补阶段新增的挂件成为两态常驻；其准入已同时检查固定主力。
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
    main.assignment.auto_other_presence =
        crate::layout::admission::presence(blueprint, &backup.assignment, instances);
    backup.assignment.auto_other_presence =
        crate::layout::admission::presence(blueprint, &main.assignment, instances);
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
    for assignment in [&main.assignment, &backup.assignment] {
        if !crate::layout::admission::assignment_admitted(blueprint, assignment, instances, table) {
            return Err(Error::msg(
                "auto_cooperation: final assignment cooperation rejected",
            ));
        }
    }
    if !crate::layout::admission::all_station_states_admitted(
        &crate::layout::admission::presence(blueprint, &main.assignment, instances),
        &crate::layout::admission::presence(blueprint, &backup.assignment, instances),
        table,
        &crate::layout::layout_from_blueprint(blueprint),
    ) {
        return Err(Error::msg(
            "自动轮换各房主备混合配置不能撑满输入时长或不满足协同准入",
        ));
    }
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
                         resolved: &crate::layout::ResolvedBase| {
                let members = assignment.operators_in(&room.id);
                let bottleneck = eta
                    .per_op
                    .iter()
                    .filter(|entry| entry.room_id == room.id.0)
                    .filter_map(|entry| {
                        entry
                            .workable_hours
                            .map(|hours| (entry.name.clone(), hours))
                    })
                    .min_by(|left, right| left.1.total_cmp(&right.1));
                AutoRoomState {
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
                    control_effects: (room.kind == FacilityKind::ControlCenter)
                        .then(|| AutoControlEffects::from_layout(&resolved.layout)),
                    evaluation_hours: assignment.work_hours.expect("auto request work duration"),
                    first_red_face: bottleneck.map(|(name, _)| name),
                    missing_seats: room.operator_capacity().saturating_sub(members.len()),
                    complete: members.len() == room.operator_capacity(),
                }
            };
            AutoRoomRecommendation {
                room_id: room.id.clone(),
                facility: room.kind,
                main: state(&main.assignment, &main_eta, &main_resolved),
                backup: state(&backup.assignment, &backup_eta, &backup_resolved),
            }
        })
        .collect();
    for room in &rooms {
        for (label, state) in [("主力", &room.main), ("替补", &room.backup)] {
            if !state.complete {
                warnings.push(format!(
                    "{} {}缺{}人，建议不完整",
                    room.room_id.0, label, state.missing_seats
                ));
            }
        }
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
    Ok(AutoRotationReport {
        mode: "auto_rotation",
        timing,
        complete: rooms
            .iter()
            .all(|room| room.main.complete && room.backup.complete),
        rooms,
        constant_dorm_support,
        main_assignment: main.assignment,
        backup_assignment: backup.assignment,
        assignment_plans: vec![main.plan, backup.plan],
        warnings,
    })
}

pub fn assert_auto_rotation_invariants(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &AutoRotationReport,
) -> Result<()> {
    if report.mode != "auto_rotation" || report.assignment_plans.len() != 2 {
        return Err(Error::msg("assert-invariants auto_report_shape failed"));
    }
    report.timing.validate()?;
    if !crate::layout::admission::all_station_states_admitted(
        &crate::layout::admission::presence(blueprint, &report.main_assignment, instances),
        &crate::layout::admission::presence(blueprint, &report.backup_assignment, instances),
        table,
        &crate::layout::layout_from_blueprint(blueprint),
    ) {
        return Err(Error::msg(
            "assert-invariants auto_independent_station_states failed",
        ));
    }
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
    if !work.is_disjoint(
        &report
            .backup_assignment
            .work_position_operator_names(blueprint),
    ) {
        return Err(Error::msg("assert-invariants auto_work_disjoint failed"));
    }
    for (index, assignment) in [&report.main_assignment, &report.backup_assignment]
        .into_iter()
        .enumerate()
    {
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
        if !crate::layout::admission::assignment_admitted(blueprint, assignment, instances, table) {
            return Err(Error::msg(format!(
                "assert-invariants auto_cooperation_admission failed in state {}",
                index + 1
            )));
        }
        let other = if index == 0 {
            &report.backup_assignment
        } else {
            &report.main_assignment
        };
        if !crate::layout::admission::stations_fit_work_duration(
            &crate::layout::admission::presence(blueprint, assignment, instances),
            &crate::layout::admission::presence(blueprint, other, instances),
            &crate::layout::layout_from_blueprint(blueprint),
        ) {
            return Err(Error::msg("assert-invariants auto_work_duration failed"));
        }
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
        // gate 从实际 assignment 独立重建 ETA，不以候选准入谓词代替预算证明。
        for alternate_control in [false, true] {
            let mut trial = assignment.clone();
            if alternate_control {
                for room in blueprint
                    .rooms
                    .iter()
                    .filter(|room| room.kind == FacilityKind::ControlCenter)
                {
                    trial.set_room(room.id.clone(), other.operators_in(&room.id).to_vec());
                }
            }
            let trial_eta =
                crate::mood::shift_eta_with_instances(model, blueprint, &trial, instances);
            for member in &trial_eta.per_op {
                let room = blueprint
                    .room(&RoomId::from(member.room_id.as_str()))
                    .ok_or_else(|| Error::msg("assert-invariants auto_eta_room_owner failed"))?;
                if alternate_control && room.kind == FacilityKind::ControlCenter {
                    continue;
                }
                if !crate::mood::fits_work_budget(
                    model,
                    model.mood_cap,
                    member.drain_per_hour,
                    report.timing.shift_hours[index],
                ) {
                    return Err(Error::msg(format!(
                        "assert-invariants auto_eta_work_duration failed: {} {} needs {}h, workable {:?}h",
                        member.room_id,
                        member.name,
                        report.timing.shift_hours[index],
                        member.workable_hours,
                    )));
                }
            }
        }
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
        for recommendation in &report.rooms {
            let state = if index == 0 {
                &recommendation.main
            } else {
                &recommendation.backup
            };
            let room = blueprint
                .room(&recommendation.room_id)
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
            if room.kind != recommendation.facility || !crate::layout::ordinary_workplace(room.kind)
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
            {
                return Err(Error::msg("assert-invariants auto_aux_efficiency failed"));
            }
            let first = eta
                .per_op
                .iter()
                .filter(|entry| entry.room_id == room.id.0)
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
                if !crate::search::manufacture_combo_allowed_for_shift(&operators, table, None) {
                    return Err(Error::msg(
                        "assert-invariants auto_manufacture_window_policy failed",
                    ));
                }
            }
        }
    }
    let expected_warnings = report
        .rooms
        .iter()
        .flat_map(|room| {
            [("主力", &room.main), ("替补", &room.backup)]
                .into_iter()
                .filter(|(_, state)| !state.complete)
                .map(move |(label, state)| {
                    format!(
                        "{} {}缺{}人，建议不完整",
                        room.room_id.0, label, state.missing_seats
                    )
                })
        })
        .collect::<Vec<_>>();
    if report.warnings != expected_warnings {
        return Err(Error::msg(
            "assert-invariants auto_shortfall_annotation failed",
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
