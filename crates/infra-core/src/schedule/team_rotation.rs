use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use crate::control::{solve_control, ControlRoomInput};
use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::global_resource::GlobalResourceKey;
use crate::instances::OperatorInstances;
#[cfg(test)]
use crate::layout::RoomAssignment;
use crate::layout::{
    assign_manu_room_with_anchors, assign_shift_with_plan_skip, assign_shift_with_preclaimed_plan,
    assign_team_gamma_half, beta_tailor_names, fill_workplace_vacancies, ordinary_workplace,
    pinned_assignment_excluding, refill_manu_room_with_core, refill_trade_room_with_core,
    refill_witch_room_with_tailor, resolve_base, ActivatedSystem, AnchorFillPolicy,
    AssignBaseOptions, AssignShiftMode, AssignedOperator, AssignmentPlan, BaseAssignment,
    BaseBlueprint, FacilityKind, LayoutContext, ReserveReusePolicy, ResolvedRoleReserve, RoomId,
    RoomProduct, SelectedRuleAlternative, ShiftBind, SlotFill, SystemAnchor, TradeRolePreference,
};
#[cfg(test)]
use crate::mood::shift_eta;
use crate::mood::{
    default_mood_model, dorm_recovery_rates, fits_shift_budget, shift_eta_with_instances,
    DormOccupant, MoodModel,
};
use crate::notes::annotate_assignment;
use crate::operbox::OperBox;
use crate::pool::{
    build_control_pool_with_fillers, build_manufacture_pool, build_power_pool_with_fillers,
    build_trade_pool, ManuPool, ManuPoolEntry,
};
#[cfg(test)]
use crate::search::control_inject_components_for_assignment;
use crate::search::{
    control_efficiency_fill_sort_weight, manufacture_operator_has_mood_gap_decay,
    MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS,
};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput};
use crate::trade::legality::witch_core_station_ok;
use crate::trade::shortcut::match_witch_group_shortcut;

use super::base_rotation::evaluate_base_assignment_efficiencies;
use super::control_rotation::{assert_final_control_rotation, ControlRotationClosure};
use super::cycle_mood::{is_budget_rejection, validate_cycle_mood, validate_operator_cycle_mood};
use super::dorm_finalizer;
use super::fiammetta_events;
use super::room_admission::{
    candidate_fits_current_shift, candidate_fits_facility_shift, candidate_fits_mood_budget,
    candidate_fits_rotation_shift, nonproduction_eta_failure, FacilityRotationEligibility,
};
use super::rotation_scoring::{clear_production_efficiencies, rescore_standard_shifts};
use super::rotation_types::{
    DailyProduction, DailyTotals, DormRestPlan, DroneAction, FacilityHalf, FiammettaShiftAction,
    RoomEfficiencyLine, RuntimeShiftBind, ShiftEfficiencies, ShiftTransition, TeamAssignment,
    TeamLabel, TeamRotationReport, TeamShiftResult, TimedRotationProfile,
};
use super::shift_bind::{shift_binds_from_plan, team_of_operator, validate_final_bind_cycle};

const FIAMMETTA_NAME: &str = "菲亚梅塔";
const SPHINX_NAME: &str = "深巡";
const ULPIANUS_NAME: &str = "乌尔比安";
// ABC admission uses this state only for ETA; it does not choose beds or change
// production assignments. A 12/12/12 off-shift is the approved full-rest approximation;
// any other off-shift recovers at the lowest dormitory base rate (no skill bonus, no
// bed assumption), capped at mood_cap.
fn advance_rotation_mood(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    assignment: &BaseAssignment,
    hours: f64,
    full_rest: bool,
    mood: &mut HashMap<String, f64>,
) {
    let eta = shift_eta_with_instances(model, blueprint, assignment, instances);
    let working: HashSet<_> = eta.per_op.iter().map(|op| op.name.as_str()).collect();
    for (name, value) in mood.iter_mut() {
        if !working.contains(name.as_str()) {
            *value = if full_rest {
                model.mood_cap
            } else {
                (*value + model.dorm_base_recovery(1) * hours).min(model.mood_cap)
            };
        }
    }
    for op in eta.per_op {
        let value = mood.entry(op.name).or_insert(model.mood_cap);
        *value = (*value - op.drain_per_hour * hours).max(0.0);
    }
}

/// 兼容入口：事件、物理菲亚预算与 ready 判定统一由 `fiammetta_events` 重放。
fn fiammetta_ready_events(
    shifts: &[TeamShiftResult],
    target: &str,
    candidate_indices: &[usize],
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
) -> Vec<usize> {
    fiammetta_events::ready_events(
        shifts,
        target,
        candidate_indices,
        model,
        blueprint,
        instances,
    )
}

/// 返回班次的无人机目标：首个实际产品为赤金的制造房；菲亚-only 班次或
/// 无赤金制造房时返回 `None`（export 必须输出 `enable: false`）。
pub fn drone_action(blueprint: &BaseBlueprint, transition: ShiftTransition) -> Option<DroneAction> {
    if transition == ShiftTransition::FiammettaOnly {
        return None;
    }
    blueprint
        .rooms
        .iter()
        .find(|room| {
            room.kind == FacilityKind::Factory
                && matches!(
                    room.product.as_ref(),
                    Some(RoomProduct::Factory {
                        recipe: crate::types::RecipeKind::Gold
                    })
                )
        })
        .map(|room| DroneAction {
            room_id: room.id.clone(),
            order: "pre",
        })
}

fn rotation_shortfall_warnings(
    blueprint: &BaseBlueprint,
    shifts: &[TeamShiftResult],
) -> Vec<String> {
    let mut warnings = Vec::new();
    for shift in shifts {
        for room in &blueprint.rooms {
            if matches!(
                room.kind,
                FacilityKind::Dormitory | FacilityKind::TrainingRoom | FacilityKind::Workshop
            ) {
                continue;
            }
            let actual = shift.assignment.operators_in(&room.id).len();
            let capacity = room.operator_capacity();
            if actual < capacity {
                let label = match room.kind {
                    FacilityKind::Office => "办公室",
                    FacilityKind::MeetingRoom => "会客室",
                    FacilityKind::Factory => "制造站",
                    FacilityKind::TradePost => "贸易站",
                    FacilityKind::PowerPlant => "发电站",
                    FacilityKind::ControlCenter => "控制中枢",
                    FacilityKind::Recycle => "回收站",
                    _ => continue,
                };
                warnings.push(format!(
                    "shift {} {label} {} 缺 {} 名",
                    shift.index + 1,
                    room.id.0,
                    capacity - actual
                ));
            }
        }
    }
    warnings
}

const CYCLE_REST_WARNING_PREFIX: &str = "rotation_cycle_rest:";
/// 最终排班校验违规：不拒绝输出，只作为诊断警告，便于人工修正与 Agent 定位。
const FINAL_INVARIANT_WARNING_PREFIX: &str = "rotation_final_invariant:";

fn automation_lancet_shift_proof(
    blueprint: &BaseBlueprint,
    shift: &TeamShiftResult,
    plan: &AssignmentPlan,
) -> bool {
    let selected = plan.selected_rules.iter().any(|rule| {
        rule.rule_id == "automation_group" && rule.operators.iter().any(|name| name == "Lancet-2")
    });
    if !selected {
        return false;
    }
    plan.anchors.iter().any(|anchor| {
        anchor.system_id == "automation_group"
            && anchor.operator == "Lancet-2"
            && anchor.facility == FacilityKind::PowerPlant
            && anchor.rest_facility == Some(FacilityKind::Workshop)
            && shift.assignment.iter_rooms().any(|room| {
                let Some(spec) = blueprint.room(&room.room_id) else {
                    return false;
                };
                let expected_room = match spec.kind {
                    FacilityKind::PowerPlant => {
                        anchor.room_id.as_ref().is_none_or(|id| *id == room.room_id)
                    }
                    FacilityKind::Workshop => true,
                    _ => false,
                };
                expected_room
                    && room
                        .operators
                        .iter()
                        .any(|operator| operator.name == "Lancet-2")
            })
    })
}

fn automation_lancet_cycle_exempt(blueprint: &BaseBlueprint, report: &TeamRotationReport) -> bool {
    let work_shifts: Vec<_> = report
        .shifts
        .iter()
        .filter(|shift| {
            shift
                .assignment
                .is_work_position_operator(blueprint, "Lancet-2")
        })
        .collect();
    !work_shifts.is_empty()
        && work_shifts.iter().all(|shift| {
            report
                .assignment_plans
                .get(shift.plan_index)
                .is_some_and(|plan| automation_lancet_shift_proof(blueprint, shift, plan))
        })
        && work_shifts.iter().any(|shift| {
            shift.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|spec| spec.kind == FacilityKind::PowerPlant)
                    && room
                        .operators
                        .iter()
                        .any(|operator| operator.name == "Lancet-2")
            })
        })
}

/// Attendance is independent of team labels, mechanism workforce and mood.
fn rotation_cycle_rest_warnings(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
) -> Vec<String> {
    let mut attendance: BTreeMap<&str, BTreeMap<usize, Vec<String>>> = BTreeMap::new();
    for (position, shift) in report.shifts.iter().enumerate() {
        for room in shift.assignment.iter_rooms() {
            let Some(spec) = blueprint.room(&room.room_id) else {
                continue;
            };
            let label = match spec.kind {
                FacilityKind::ControlCenter => "中枢",
                FacilityKind::TradePost => "贸易站",
                FacilityKind::Factory => "制造站",
                FacilityKind::PowerPlant => "发电站",
                FacilityKind::Office => "办公室",
                FacilityKind::MeetingRoom => "会客室",
                FacilityKind::TrainingRoom => "训练室",
                FacilityKind::Workshop => "加工站",
                FacilityKind::Recycle => "回收站",
                FacilityKind::Dormitory => continue,
            };
            for operator in &room.operators {
                attendance
                    .entry(&operator.name)
                    .or_default()
                    .entry(position)
                    .or_default()
                    .push(format!("{}({label})", room.room_id.0));
            }
        }
        if let Some(assistant) = &shift.assignment.training_assist {
            let room_id = blueprint
                .rooms
                .iter()
                .find(|room| room.kind == FacilityKind::TrainingRoom)
                .map_or("training_assist", |room| room.id.0.as_str());
            attendance
                .entry(&assistant.name)
                .or_default()
                .entry(position)
                .or_default()
                .push(format!("{room_id}(训练室副手)"));
        }
    }
    let automation_lancet = automation_lancet_cycle_exempt(blueprint, report);
    let fiammetta_targets: HashSet<&str> = report
        .shifts
        .iter()
        .filter_map(|shift| {
            shift
                .fiammetta
                .as_ref()
                .map(|action| action.target.as_str())
        })
        .collect();
    attendance
        .into_iter()
        .filter(|(name, shifts)| {
            shifts.len() == report.shifts.len()
                && !(*name == "Lancet-2" && automation_lancet)
                && !fiammetta_targets.contains(name)
        })
        .map(|(name, shifts)| {
            let positions = shifts
                .into_iter()
                .map(|(position, mut rooms)| {
                    rooms.sort();
                    rooms.dedup();
                    format!("shift {}: {}", report.shifts[position].index + 1, rooms.join(", "))
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "{CYCLE_REST_WARNING_PREFIX} {name} 整周期无休班；工作岗位=[{positions}]；豁免=无（未实际入选自动化 Lancet-2，且不是本周期菲亚动作目标）"
            )
        })
        .collect()
}

fn average_cycle_production(
    blueprint: &BaseBlueprint,
    shifts: &[TeamShiftResult],
) -> DailyProduction {
    let total_hours: f64 = shifts.iter().map(|shift| shift.duration_hours).sum();
    if total_hours <= 0.0 {
        return DailyProduction::default();
    }
    let mut output = DailyProduction::default();
    for shift in shifts {
        for line in &shift.efficiencies.room_lines {
            let Some(room) = blueprint.room(&RoomId::new(line.room_id.clone())) else {
                continue;
            };
            let weight = shift.duration_hours / total_hours;
            match room.product {
                Some(RoomProduct::Trade {
                    order: crate::trade::input::TradeOrderKind::Gold,
                }) => {
                    output.lmd += line.trade_output_per_day * weight;
                }
                Some(RoomProduct::Trade {
                    order: crate::trade::input::TradeOrderKind::Originium,
                }) => {
                    output.orundum += line.trade_output_per_day * weight;
                }
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Gold,
                }) => {
                    output.pure_gold += line.manufacture_output_per_day * weight;
                }
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::BattleRecord,
                }) => {
                    output.battle_records += line.manufacture_output_per_day * weight;
                }
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Originium,
                }) => {
                    output.originium_shards += line.manufacture_output_per_day * weight;
                }
                _ => {}
            }
        }
    }
    output
}

const SUI_MOOD_OPERATORS: [&str; 2] = ["令", "夕"];

fn control_mood_for_shift(
    mood_by_operator: &HashMap<String, f64>,
    active_names: &HashSet<String>,
    default_mood: f64,
) -> f64 {
    SUI_MOOD_OPERATORS
        .iter()
        .filter(|name| active_names.contains(**name))
        .filter_map(|name| mood_by_operator.get(*name).copied())
        .reduce(f64::min)
        .unwrap_or(default_mood)
}

#[cfg(test)]
fn advance_sui_mood(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    resting_names: &HashSet<String>,
    hours: f64,
    mood_by_operator: &mut HashMap<String, f64>,
) {
    let eta = shift_eta(model, blueprint, assignment);
    let drain_by_name: HashMap<&str, f64> = eta
        .per_op
        .iter()
        .map(|op| (op.name.as_str(), op.drain_per_hour))
        .collect();
    for name in SUI_MOOD_OPERATORS {
        let mood = mood_by_operator
            .entry(name.to_string())
            .or_insert(model.mood_cap);
        if let Some(drain) = drain_by_name.get(name) {
            *mood = (*mood - drain * hours).max(0.0);
        } else if resting_names.contains(name) {
            *mood = (*mood + model.dorm_base_recovery(5) * hours).min(model.mood_cap);
        }
    }
}

fn bound_office_operators(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
) -> HashSet<String> {
    binds
        .iter()
        .flat_map(|bind| &bind.operators)
        .filter(|name| {
            peak.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|bp| bp.kind == FacilityKind::Office)
                    && room.operators.iter().any(|op| op.name == **name)
            })
        })
        .cloned()
        .collect()
}

fn peak_office_operators(blueprint: &BaseBlueprint, peak: &BaseAssignment) -> HashSet<String> {
    peak.rooms_of_kind(blueprint, FacilityKind::Office)
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
        .collect()
}

/// exact bind 中已经实际落在 peak 中枢的成员属于其生产 cohort；γ 生产搜索必须
/// 预留这些名字，避免后续中枢重建再次放入同一干员。
fn bound_control_operators(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
) -> HashSet<String> {
    binds
        .iter()
        .flat_map(|bind| &bind.operators)
        .filter(|name| {
            peak.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|room| room.kind == FacilityKind::ControlCenter)
                    && room
                        .operators
                        .iter()
                        .any(|operator| operator.name == **name)
            })
        })
        .cloned()
        .collect()
}

fn bound_rotating_anchor_operators(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
) -> HashSet<String> {
    binds
        .iter()
        .flat_map(|bind| &bind.operators)
        .filter(|name| {
            peak.iter_rooms().any(|room| {
                blueprint.room(&room.room_id).is_some_and(|bp| {
                    !matches!(
                        bp.kind,
                        FacilityKind::TradePost
                            | FacilityKind::Factory
                            | FacilityKind::PowerPlant
                            | FacilityKind::ControlCenter
                            | FacilityKind::Office
                    )
                }) && room
                    .operators
                    .iter()
                    .any(|operator| operator.name == **name)
            })
        })
        .cloned()
        .collect()
}

fn rotating_team_for_names(
    binds: &[RuntimeShiftBind],
    names: &HashSet<String>,
    production_team_by_name: &HashMap<String, TeamLabel>,
) -> HashMap<String, TeamLabel> {
    let mut teams = HashMap::new();
    for bind in binds {
        let Some(team) = bind
            .operators
            .iter()
            .find_map(|name| production_team_by_name.get(name).copied())
        else {
            continue;
        };
        for name in bind.operators.iter().filter(|name| names.contains(*name)) {
            teams.insert(name.clone(), team);
        }
    }
    teams
}

fn assign_rotating_anchors(
    assignment: &mut BaseAssignment,
    peak: &BaseAssignment,
    active_teams: &[TeamLabel],
    rotating_team: &HashMap<String, TeamLabel>,
) -> Result<()> {
    let mut active_names: Vec<_> = rotating_team
        .iter()
        .filter(|(_, team)| active_teams.contains(team))
        .map(|(name, _)| name)
        .collect();
    active_names.sort();
    for name in active_names {
        if assignment.operator_names().contains(name) {
            continue;
        }
        let Some(peak_room) = peak.room_of_operator(name) else {
            return Err(Error::msg(format!(
                "rotating anchor {name} missing from peak"
            )));
        };
        let operator = peak_room
            .operators
            .iter()
            .find(|operator| operator.name == *name)
            .expect("room matched rotating anchor")
            .clone();
        let mut operators = assignment.operators_in(&peak_room.room_id).to_vec();
        operators.push(operator);
        assignment.set_room(peak_room.room_id.clone(), operators);
    }
    Ok(())
}

/// 当前 active dependency 的 required 端采用全周期共享策略。只从 peak 复制 plan
/// 实际声明的 required 成员，不把 Meeting/Workshop/Training 的其他旧成员扩大为 shared。
/// 已在班次绑定内的 required 成员（如开启后的莱茵组）改由绑定组同上同下，不在此钉住。
fn pin_active_dependency_required(
    shared: &mut BaseAssignment,
    peak: &BaseAssignment,
    dependencies: &[crate::layout::ActiveDependency],
    binds: &[RuntimeShiftBind],
) -> Result<()> {
    let mut required: Vec<_> = dependencies
        .iter()
        .flat_map(|dependency| &dependency.required)
        .collect();
    required.sort();
    required.dedup();
    for name in required {
        // 已被班次绑定覆盖的 required 成员跟随其绑定组同上同下，不再全周期共享；
        // 否则共享钉住会让绑定组内的其他成员下班后它仍留在岗位上。
        if binds
            .iter()
            .any(|bind| bind.operators.iter().any(|member| member == name))
        {
            continue;
        }
        let Some(peak_room) = peak.room_of_operator(name) else {
            return Err(Error::msg(format!(
                "active dependency required operator {name} missing from peak"
            )));
        };
        if shared.operator_names().contains(name) {
            continue;
        }
        let operator = peak_room
            .operators
            .iter()
            .find(|operator| operator.name == *name)
            .expect("room matched required operator")
            .clone();
        let mut operators = shared.operators_in(&peak_room.room_id).to_vec();
        operators.push(operator);
        shared.set_room(peak_room.room_id.clone(), operators);
    }
    Ok(())
}

/// 办公室候选：全 roster 按 `support_skill_registry` 办公室速度评分排序
/// （无技能干员 0 分，按名序兜底）。
///
/// 评分固定使用单人办公室语义（办公室一人房）与当前调用方提供的 layout；
/// `hire_spd_cost[210]` 等动态条件按注册表求值。该排序只负责当前班的候选优先级，
/// 当前班已占用成员由调用方在填充前裁剪，不保存跨班办公室状态。
fn office_candidates(
    operbox: &OperBox,
    _instances: &OperatorInstances,
    _table: &SkillTable,
    layout: &LayoutContext,
    _mood: f64,
    excluded: &HashSet<String>,
) -> Vec<AssignedOperator> {
    crate::office::rank_candidates(operbox, layout, _mood, excluded)
}

fn office_remainder_pool(
    ranked_pool: &[AssignedOperator],
    assignment: &BaseAssignment,
) -> Vec<AssignedOperator> {
    let assigned = assignment.occupied_operator_names();
    ranked_pool
        .iter()
        .filter(|operator| !assigned.contains(&operator.name))
        .cloned()
        .collect()
}

fn assign_office_for_shift_with_mood(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_bound_names: &HashSet<String>,
    bound_names: &HashSet<String>,
    candidates: &[AssignedOperator],
    shift_hours: f64,
    instances: Option<&OperatorInstances>,
    mood_state: Option<(&HashMap<String, f64>, &HashMap<String, f64>)>,
    mut eta_failure: Option<&mut Option<String>>,
) -> Result<()> {
    let model = default_mood_model()?;
    let fits_candidate =
        |assignment: &BaseAssignment, room_id: &RoomId, candidate: &AssignedOperator| {
            if let (Some(instances), Some((mood, first_consumption))) = (instances, mood_state) {
                candidate_fits_mood_budget(
                    &model,
                    blueprint,
                    instances,
                    assignment,
                    room_id,
                    candidate,
                    mood,
                    first_consumption,
                    shift_hours,
                )
            } else {
                candidate_fits_current_shift(
                    &model,
                    assignment,
                    blueprint,
                    room_id,
                    candidate,
                    shift_hours,
                    model.mood_cap,
                    0.0,
                )
            }
        };
    let available_candidates = office_remainder_pool(candidates, assignment);
    let mut used = assignment.occupied_operator_names();
    let mut active_bound: Vec<&String> = active_bound_names.iter().collect();
    active_bound.sort_by_key(|name| {
        available_candidates
            .iter()
            .position(|candidate| candidate.name == name.as_str())
            .unwrap_or(usize::MAX)
    });

    let office_rooms: Vec<(RoomId, usize)> = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| (room.id.clone(), room.operator_capacity()))
        .collect();

    // A bound office member cannot be replaced independently: place every active
    // member first, then use ordinary candidates for the remaining seats.
    for name in active_bound {
        if used.contains(name.as_str()) {
            return Err(Error::msg(format!(
                "active bound office operator {name} is already occupied in the current shift"
            )));
        }
        let candidate = available_candidates
            .iter()
            .find(|candidate| candidate.name == name.as_str())
            .ok_or_else(|| {
                Error::msg(format!(
                    "active bound office operator {name} is unavailable in the current shift"
                ))
            })?;
        let Some((room_id, _)) = office_rooms.iter().find(|(room_id, capacity)| {
            assignment.operators_in(room_id).len() < *capacity
                && fits_candidate(assignment, room_id, candidate)
        }) else {
            if mood_state.is_some()
                && office_rooms
                    .iter()
                    .any(|(room_id, capacity)| assignment.operators_in(room_id).len() < *capacity)
            {
                if let Some(failure) = eta_failure.as_deref_mut() {
                    *failure = Some(name.clone());
                }
            }
            return Err(Error::msg(format!(
                "active bound office operator {name} cannot cover the current shift ETA"
            )));
        };
        let mut operators = assignment.operators_in(room_id).to_vec();
        operators.push(candidate.clone());
        assignment.set_room(room_id.clone(), operators);
        used.insert(candidate.name.clone());
    }

    for (room_id, capacity) in office_rooms {
        let mut operators = assignment.operators_in(&room_id).to_vec();
        while operators.len() < capacity {
            let ordinary = available_candidates.iter().find(|candidate| {
                !used.contains(&candidate.name)
                    && !bound_names.contains(&candidate.name)
                    && fits_candidate(assignment, &room_id, candidate)
            });
            let Some(operator) = ordinary.cloned() else {
                break;
            };
            used.insert(operator.name.clone());
            operators.push(operator);
        }
        assignment.set_room(room_id, operators);
    }
    Ok(())
}

#[cfg(test)]
fn assign_office_for_shift(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_bound_names: &HashSet<String>,
    bound_names: &HashSet<String>,
    candidates: &[AssignedOperator],
    shift_hours: f64,
) -> Result<()> {
    assign_office_for_shift_with_mood(
        assignment,
        blueprint,
        active_bound_names,
        bound_names,
        candidates,
        shift_hours,
        None,
        None,
        None,
    )
}

fn active_bound_office_names(
    bound_office_team: &HashMap<String, TeamLabel>,
    active_teams: &[TeamLabel],
) -> HashSet<String> {
    bound_office_team
        .iter()
        .filter(|(_, team)| active_teams.contains(team))
        .map(|(name, _)| name.clone())
        .collect()
}

fn record_meeting_usage_hours(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    worked_hours: &mut HashMap<String, f64>,
    hours: f64,
) {
    for room in assignment.rooms_of_kind(blueprint, FacilityKind::MeetingRoom) {
        for operator in &room.operators {
            *worked_hours.entry(operator.name.clone()).or_default() += hours;
        }
    }
}

/// 会客室候选：全 roster 按线索搜集速度加成排序（无技能干员 0 分，按名序兜底）。
/// 会客室轮换候选：全 roster 按线索搜集速度静态求值排序（无技能干员 0 分，按名序兜底）。
///
/// 评分固定使用双人房间语义（哨兵搭档补足第二席，使 `SpeedIfSolo` 失效），
/// 因为会客室总是两人同房；`SpeedIfPartner` 搭档加成按哨兵不匹配记 0，
/// 只在最终同房两人中真实存在搭档时由班次求值生效。
/// 这是静态 heuristic：在班次组装前构建，固定使用 24 小时、0 额外招募位和 shared
/// 脚手架 layout，不反映当班办公室注入、全局资源（情报储备/魔物料理/人间烟火）或
/// 短班时长；本排序不保证最终组合收益。与 `office_candidates` 同口径。
fn meeting_candidates(
    operbox: &OperBox,
    layout: &LayoutContext,
    excluded: &HashSet<String>,
) -> Vec<AssignedOperator> {
    let registry = crate::support_facility::SupportRegistry::load_default_shared()
        .expect("support registry loads in meeting candidate build");
    let roster = operbox.roster();
    let winter_available = roster.names().any(|name| {
        !excluded.contains(name.as_str())
            && name == "忍冬"
            && roster
                .progress(name)
                .is_some_and(|progress| progress.elite >= 2)
    });
    let extra_recruit_slots = 0;
    let partner_sentinel = crate::support_facility::SupportOperator {
        name: "\0meeting-partner-sentinel".to_string(),
        elite: 0,
        rarity: 0,
        level: 1,
        mood: 24.0,
    };
    let mut candidates: Vec<(f64, AssignedOperator)> = roster
        .names()
        .filter(|name| !excluded.contains(name.as_str()))
        .filter(|name| name.as_str() != "铃兰" || winter_available)
        .filter_map(|name| {
            let progress = roster.progress(name)?;
            let score = crate::meeting::evaluate_meeting(
                &crate::support_facility::SupportRoomInput {
                    facility: crate::support_facility::SupportFacility::Meeting,
                    operators: vec![
                        crate::support_facility::SupportOperator {
                            name: name.clone(),
                            elite: progress.elite,
                            rarity: progress.rarity,
                            level: progress.level,
                            mood: 24.0,
                        },
                        partner_sentinel.clone(),
                    ],
                    capacity: 2,
                    extra_recruit_slots,
                    elapsed_hours: 24.0,
                    external_speed_bonus_pct: 0.0,
                    layout: layout.clone(),
                },
                &registry,
            )
            // 求值失败与"无会客室技能"都按 0 分兜底；单班流水线内的
            // `assign_meeting_rooms` 使用完整班次上下文重新求值并传播错误。
            .map(|result| result.skill_speed_bonus_pct)
            .unwrap_or(0.0);
            Some((score, AssignedOperator::from_progress(name, progress)))
        })
        .collect();
    candidates.sort_by(|(a_score, a), (b_score, b)| {
        b_score
            .partial_cmp(a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    candidates.into_iter().map(|(_, op)| op).collect()
}

fn meeting_pair_score(
    members: &[AssignedOperator],
    layout: &LayoutContext,
    hours: f64,
    mood_state: Option<(&HashMap<String, f64>, &HashMap<String, f64>)>,
    extra_recruit_slots: u8,
) -> Result<f64> {
    if members.is_empty() {
        return Ok(0.0);
    }
    let registry = crate::support_facility::SupportRegistry::load_default_shared()?;
    Ok(crate::meeting::evaluate_meeting(
        &crate::support_facility::SupportRoomInput {
            facility: crate::support_facility::SupportFacility::Meeting,
            operators: members
                .iter()
                .map(|member| crate::support_facility::SupportOperator {
                    name: member.name.clone(),
                    elite: member.elite,
                    rarity: member.rarity,
                    level: member.level,
                    mood: mood_state
                        .and_then(|(mood, _)| mood.get(&member.name).copied())
                        .unwrap_or(24.0),
                })
                .collect(),
            capacity: 2,
            extra_recruit_slots,
            elapsed_hours: hours,
            external_speed_bonus_pct: 0.0,
            layout: layout.clone(),
        },
        &registry,
    )?
    .skill_speed_bonus_pct)
}

/// 每班从活跃队中选会客室成员填满会客室（每间两人）。
///
/// 活跃队成员按会客室候选评分序选择；cohort 不足时兜底 replacement，但
/// replacement 必须排除休息队与所有已编队成员（`reserved`），避免休息队
/// 成员因 meeting 兜底而返岗破坏"每班两队上岗、一队休息"的不变量。
#[allow(clippy::too_many_arguments)]
fn assign_rotating_meetings_with_context(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_teams: &[TeamLabel],
    resting_team: TeamLabel,
    rotating_team: &HashMap<String, TeamLabel>,
    candidates: &[AssignedOperator],
    reserved: &HashSet<String>,
    eligibility: Option<&FacilityRotationEligibility<'_>>,
    instances: Option<&OperatorInstances>,
    mood_state: Option<(&HashMap<String, f64>, &HashMap<String, f64>)>,
    layout: &LayoutContext,
) -> Result<()> {
    let extra_recruit_slots = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| room.level.saturating_sub(1))
        .max()
        .unwrap_or(0);
    let mut used = assignment.operator_names();
    let mut active_rotating: Vec<&String> = rotating_team
        .iter()
        .filter(|(_, team)| active_teams.contains(team))
        .map(|(name, _)| name)
        .collect();
    // 活跃队成员按会客室候选评分序（candidates 已按线索速度分降序）排列，
    // 每班优先选择技能分更高的活跃成员。
    active_rotating.sort_by_key(|name| {
        candidates
            .iter()
            .position(|op| op.name == name.as_str())
            .unwrap_or(usize::MAX)
    });
    let resting_reserved: HashSet<String> = reserved
        .iter()
        .filter(|name| rotating_team.get(name.as_str()) == Some(&resting_team))
        .cloned()
        .collect();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::MeetingRoom)
    {
        let mut operators = assignment.operators_in(&room.id).to_vec();
        let pair_team = (rotating_team.get("忍冬"), rotating_team.get("铃兰"));
        let pair_available = match pair_team {
            (Some(left), Some(right)) => left == right && active_teams.contains(left),
            (Some(team), None) => active_teams.contains(team) && !reserved.contains("铃兰"),
            (None, Some(team)) => active_teams.contains(team) && !reserved.contains("忍冬"),
            (None, None) => !reserved.contains("忍冬") && !reserved.contains("铃兰"),
        };
        if operators.is_empty() && pair_available {
            let pair = candidates
                .iter()
                .find(|candidate| candidate.name == "忍冬" && candidate.elite >= 2)
                .zip(candidates.iter().find(|candidate| candidate.name == "铃兰"));
            if let Some((winter, suzuran)) = pair {
                let mut trial = assignment.clone();
                let pair_fits = [winter, suzuran].into_iter().all(|candidate| {
                    if used.contains(&candidate.name) {
                        return false;
                    }
                    let fits = instances.map_or_else(
                        || {
                            candidate_fits_facility_shift(
                                &trial,
                                blueprint,
                                &room.id,
                                candidate,
                                eligibility,
                            )
                        },
                        |instances| {
                            candidate_fits_rotation_shift(
                                &trial,
                                blueprint,
                                instances,
                                &room.id,
                                candidate,
                                eligibility,
                                mood_state,
                            )
                        },
                    );
                    if fits {
                        let mut current = trial.operators_in(&room.id).to_vec();
                        current.push((*candidate).clone());
                        trial.set_room(room.id.clone(), current);
                    }
                    fits
                });
                if pair_fits {
                    let mut ordinary = assignment.clone();
                    for candidate in candidates.iter().filter(|candidate| {
                        candidate.name != "铃兰"
                            && rotating_team.get(&candidate.name).map_or_else(
                                || !reserved.contains(&candidate.name),
                                |team| active_teams.contains(team),
                            )
                            && !used.contains(&candidate.name)
                    }) {
                        let fits = instances.map_or_else(
                            || {
                                candidate_fits_facility_shift(
                                    &ordinary,
                                    blueprint,
                                    &room.id,
                                    candidate,
                                    eligibility,
                                )
                            },
                            |instances| {
                                candidate_fits_rotation_shift(
                                    &ordinary,
                                    blueprint,
                                    instances,
                                    &room.id,
                                    candidate,
                                    eligibility,
                                    mood_state,
                                )
                            },
                        );
                        if fits {
                            let mut current = ordinary.operators_in(&room.id).to_vec();
                            current.push(candidate.clone());
                            ordinary.set_room(room.id.clone(), current);
                            if ordinary.operators_in(&room.id).len() == room.operator_capacity() {
                                break;
                            }
                        }
                    }
                    let hours = eligibility.map_or(24.0, |state| state.shift_hours);
                    if meeting_pair_score(
                        trial.operators_in(&room.id),
                        layout,
                        hours,
                        mood_state,
                        extra_recruit_slots,
                    )? > meeting_pair_score(
                        ordinary.operators_in(&room.id),
                        layout,
                        hours,
                        mood_state,
                        extra_recruit_slots,
                    )? {
                        operators = vec![winter.clone(), suzuran.clone()];
                        for member in &operators {
                            used.insert(member.name.clone());
                        }
                    }
                }
            }
        }
        if operators.len() == 1 && (operators[0].name == "忍冬" || operators[0].name == "铃兰")
        {
            let partner_name = if operators[0].name == "忍冬" {
                "铃兰"
            } else {
                "忍冬"
            };
            if let Some(partner) = candidates.iter().find(|candidate| {
                candidate.name == partner_name
                    && (partner_name != "忍冬" || candidate.elite >= 2)
                    && !used.contains(&candidate.name)
                    && rotating_team.get(&candidate.name).map_or_else(
                        || !reserved.contains(&candidate.name),
                        |team| active_teams.contains(team),
                    )
            }) {
                let pair_trial = assignment.clone();
                let pair_fits = instances.map_or_else(
                    || {
                        candidate_fits_facility_shift(
                            &pair_trial,
                            blueprint,
                            &room.id,
                            partner,
                            eligibility,
                        )
                    },
                    |instances| {
                        candidate_fits_rotation_shift(
                            &pair_trial,
                            blueprint,
                            instances,
                            &room.id,
                            partner,
                            eligibility,
                            mood_state,
                        )
                    },
                );
                if pair_fits {
                    let mut pair_members = operators.clone();
                    pair_members.push(partner.clone());
                    let mut ordinary = assignment.clone();
                    for candidate in candidates.iter().filter(|candidate| {
                        candidate.name != partner_name
                            && !used.contains(&candidate.name)
                            && rotating_team.get(&candidate.name).map_or_else(
                                || !reserved.contains(&candidate.name),
                                |team| active_teams.contains(team),
                            )
                    }) {
                        let fits = instances.map_or_else(
                            || {
                                candidate_fits_facility_shift(
                                    &ordinary,
                                    blueprint,
                                    &room.id,
                                    candidate,
                                    eligibility,
                                )
                            },
                            |instances| {
                                candidate_fits_rotation_shift(
                                    &ordinary,
                                    blueprint,
                                    instances,
                                    &room.id,
                                    candidate,
                                    eligibility,
                                    mood_state,
                                )
                            },
                        );
                        if fits {
                            ordinary.set_room(
                                room.id.clone(),
                                vec![operators[0].clone(), candidate.clone()],
                            );
                            break;
                        }
                    }
                    let hours = eligibility.map_or(24.0, |state| state.shift_hours);
                    if meeting_pair_score(
                        &pair_members,
                        layout,
                        hours,
                        mood_state,
                        extra_recruit_slots,
                    )? > meeting_pair_score(
                        ordinary.operators_in(&room.id),
                        layout,
                        hours,
                        mood_state,
                        extra_recruit_slots,
                    )? {
                        used.insert(partner.name.clone());
                        operators.push(partner.clone());
                    }
                }
            }
        }
        while operators.len() < room.operator_capacity() {
            let rotating = active_rotating.iter().find_map(|name| {
                candidates.iter().find(|op| {
                    op.name == name.as_str()
                        && !used.contains(&op.name)
                        && (op.name != "铃兰"
                            || operators
                                .iter()
                                .any(|member| member.name == "忍冬" && member.elite >= 2))
                        && instances.map_or_else(
                            || {
                                candidate_fits_facility_shift(
                                    assignment,
                                    blueprint,
                                    &room.id,
                                    op,
                                    eligibility,
                                )
                            },
                            |instances| {
                                candidate_fits_rotation_shift(
                                    assignment,
                                    blueprint,
                                    instances,
                                    &room.id,
                                    op,
                                    eligibility,
                                    mood_state,
                                )
                            },
                        )
                })
            });
            let replacement = candidates.iter().find(|op| {
                (op.name != "铃兰"
                    || operators
                        .iter()
                        .any(|member| member.name == "忍冬" && member.elite >= 2))
                    && !used.contains(&op.name)
                    && !rotating_team.contains_key(&op.name)
                    && !resting_reserved.contains(&op.name)
                    && !reserved.contains(&op.name)
                    && instances.map_or_else(
                        || {
                            candidate_fits_facility_shift(
                                assignment,
                                blueprint,
                                &room.id,
                                op,
                                eligibility,
                            )
                        },
                        |instances| {
                            candidate_fits_rotation_shift(
                                assignment,
                                blueprint,
                                instances,
                                &room.id,
                                op,
                                eligibility,
                                mood_state,
                            )
                        },
                    )
            });
            let Some(op) = rotating.or(replacement).cloned() else {
                break;
            };
            used.insert(op.name.clone());
            operators.push(op.clone());
        }
        if !operators.is_empty() {
            assignment.set_room(room.id.clone(), operators);
        }
    }
    Ok(())
}

#[cfg(test)]
fn assign_rotating_meetings(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_teams: &[TeamLabel],
    resting_team: TeamLabel,
    rotating_team: &HashMap<String, TeamLabel>,
    candidates: &[AssignedOperator],
    reserved: &HashSet<String>,
) -> Result<()> {
    assign_rotating_meetings_with_context(
        assignment,
        blueprint,
        active_teams,
        resting_team,
        rotating_team,
        candidates,
        reserved,
        None,
        None,
        None,
        &LayoutContext::default(),
    )
}

fn complete_rotating_meeting_teams(
    blueprint: &BaseBlueprint,
    peak_meeting_names: &HashSet<String>,
    rotating_team: &mut HashMap<String, TeamLabel>,
    meeting_pool: &[AssignedOperator],
    reserved: &mut HashSet<String>,
) -> Result<()> {
    let capacity = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::MeetingRoom)
        .map(|room| room.operator_capacity())
        .sum::<usize>();
    if capacity == 0 {
        return Ok(());
    }

    let mut unbound_peak: Vec<_> = peak_meeting_names
        .iter()
        .filter(|name| !rotating_team.contains_key(*name))
        .cloned()
        .collect();
    unbound_peak.sort();
    if peak_meeting_names.contains("忍冬") && peak_meeting_names.contains("铃兰") {
        if let Some(team) = rotating_team
            .get("忍冬")
            .copied()
            .or_else(|| rotating_team.get("铃兰").copied())
        {
            rotating_team.insert("忍冬".to_string(), team);
            rotating_team.insert("铃兰".to_string(), team);
            unbound_peak.retain(|name| name != "忍冬" && name != "铃兰");
        }
    }
    // 峰值只选中忍冬时，仍把可用铃兰作为同队的待比较搭档带入轮换；
    // 是否最终进房由当班完整双人收益决定。
    if peak_meeting_names.contains("忍冬")
        && !peak_meeting_names.contains("铃兰")
        && !rotating_team.contains_key("铃兰")
        && meeting_pool.iter().any(|operator| operator.name == "铃兰")
    {
        let winter_team = rotating_team.get("忍冬").copied();
        if let Some(team) = winter_team.or_else(|| {
            TeamLabel::ALL.into_iter().min_by_key(|team| {
                rotating_team
                    .values()
                    .filter(|assigned| *assigned == team)
                    .count()
            })
        }) {
            rotating_team.insert("忍冬".to_string(), team);
            rotating_team.insert("铃兰".to_string(), team);
            unbound_peak.retain(|name| name != "忍冬");
        }
    }
    // 峰值已选中的同室搭档先归入一队，避免均衡分队把两人拆开。
    if unbound_peak.iter().any(|name| name == "忍冬")
        && unbound_peak.iter().any(|name| name == "铃兰")
    {
        if let Some(team) = TeamLabel::ALL.into_iter().min_by_key(|team| {
            rotating_team
                .values()
                .filter(|assigned| *assigned == team)
                .count()
        }) {
            rotating_team.insert("忍冬".to_string(), team);
            rotating_team.insert("铃兰".to_string(), team);
            unbound_peak.retain(|name| name != "忍冬" && name != "铃兰");
        }
    }
    // 先按"每队人数最少者"轮转分摊 peak 成员，使有限候选均匀铺开；
    // 避免把候选集中到某队导致其他队缺员、每班出现可避免的空位。
    for name in unbound_peak {
        let team = TeamLabel::ALL
            .into_iter()
            .min_by_key(|team| {
                rotating_team
                    .values()
                    .filter(|assigned| *assigned == team)
                    .count()
            })
            .expect("TeamLabel::ALL is non-empty");
        rotating_team.insert(name, team);
    }

    // 候选池成员同样按最少队优先轮转，逐队补到 capacity 下限；
    // 若候选不足，保证人数少的队优先获得，而不是按固定队序填充。
    let mut remaining_pool: Vec<&AssignedOperator> = meeting_pool
        .iter()
        .filter(|operator| {
            !reserved.contains(&operator.name)
                && !rotating_team.contains_key(&operator.name)
                && operator.name != "铃兰"
        })
        .collect();
    // meeting_pool 已按线索速度评分降序（无技能者 0 分按名序）；保持该序，
    // 每次取评分最高的未用者，分配给当前人数最少的队（不是固定队序），
    // 保证技能者优先且队伍均衡，避免出现可避免的空位。
    loop {
        let least_team = TeamLabel::ALL
            .iter()
            .min_by_key(|team| {
                rotating_team
                    .values()
                    .filter(|assigned| assigned == team)
                    .count()
            })
            .copied();
        let Some(team) = least_team else {
            break;
        };
        let count = rotating_team
            .values()
            .filter(|assigned| assigned == &&team)
            .count();
        if count >= capacity {
            break;
        }
        let Some(index) = remaining_pool.iter().position(|operator| {
            !reserved.contains(&operator.name) && !rotating_team.contains_key(&operator.name)
        }) else {
            break;
        };
        // remove(index) 保持评分降序（swap_remove 会把末尾兜底者换到前面，
        // 破坏"技能者优先"顺序）。
        let operator = remaining_pool.remove(index);
        rotating_team.insert(operator.name.clone(), team);
        reserved.insert(operator.name.clone());
    }
    Ok(())
}

/// 回收站候选：回收站技能持有者按当前档位的技能加成降序，其余按姓名稳定排序。
///
/// 排序只是让结果可复现的占位规则：普通干员只带来 +0.01 效率，却要消耗心情，
/// 不表示这个人更适合进回收站。`excluded` 为 Plan 排除者。
fn recycle_candidates(
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    excluded: &HashSet<String>,
) -> Result<(Vec<AssignedOperator>, Vec<AssignedOperator>)> {
    let roster = operbox.roster();
    let mut skilled = Vec::new();
    let mut fallback = Vec::new();
    for name in roster
        .names()
        .filter(|name| !excluded.contains(name.as_str()))
    {
        let Some(progress) = roster.progress(name) else {
            continue;
        };
        let operator = AssignedOperator::from_progress(name, progress);
        let skill = crate::recycle::recycle_skill_efficiency(instances, table, &operator)?;
        if !skill.is_zero() {
            skilled.push((skill, operator));
        } else {
            fallback.push(operator);
        }
    }
    skilled.sort_by(|(a_skill, a), (b_skill, b)| {
        b_skill.cmp(a_skill).then_with(|| a.name.cmp(&b.name))
    });
    fallback.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((skilled.into_iter().map(|(_, op)| op).collect(), fallback))
}

/// 回收站队伍归属：在逐班填充之前冻结，口径同 `complete_rotating_meeting_teams`。
///
/// 候选按评分序逐个分给当前人数最少的队，直到每队补到回收站工位数。
/// 不引入第二套规则：每个人属于且仅属于一个队，每班只让活跃队的成员上岗，
/// 因此每人一个周期上两班、休一班；能不能上岗由逐班 ETA 准入决定。
/// 候选不足时有几个放几个，空位留空，不报错。
fn complete_rotating_recycle_teams(
    blueprint: &BaseBlueprint,
    rotating_team: &mut HashMap<String, TeamLabel>,
    recycle_pool: &[AssignedOperator],
) {
    let capacity = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Recycle)
        .map(|room| room.operator_capacity())
        .sum::<usize>();
    if capacity == 0 {
        return;
    }
    let team_count = |rotating_team: &HashMap<String, TeamLabel>, team: &TeamLabel| {
        rotating_team
            .values()
            .filter(|assigned| *assigned == team)
            .count()
    };
    let mut remaining: Vec<&AssignedOperator> = recycle_pool.iter().collect();
    loop {
        let Some(team) = TeamLabel::ALL
            .iter()
            .min_by_key(|team| team_count(rotating_team, team))
            .copied()
        else {
            break;
        };
        if team_count(rotating_team, &team) >= capacity || remaining.is_empty() {
            break;
        }
        // remove(0) 保持评分降序，技能者优先。
        let operator = remaining.remove(0);
        rotating_team.insert(operator.name.clone(), team);
    }
}

/// 每班只放活跃队的回收站技能干员；普通补位在全部工作房确定后统一处理。
#[allow(clippy::too_many_arguments)]
fn assign_rotating_recycle(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_teams: &[TeamLabel],
    rotating_team: &HashMap<String, TeamLabel>,
    skilled_candidates: &[AssignedOperator],
    eligibility: Option<&FacilityRotationEligibility<'_>>,
    instances: &OperatorInstances,
    mood_state: Option<(&HashMap<String, f64>, &HashMap<String, f64>)>,
) {
    let mut used = assignment.operator_names();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Recycle)
    {
        let mut operators = assignment.operators_in(&room.id).to_vec();
        // 第一阶段只消费已经按 ABC 队伍冻结的回收站技能干员。
        for operator in skilled_candidates {
            if operators.len() >= room.operator_capacity() {
                break;
            }
            let on_active_team = rotating_team
                .get(&operator.name)
                .is_some_and(|team| active_teams.contains(team));
            if !on_active_team || used.contains(&operator.name) {
                continue;
            }
            let mut trial = assignment.clone();
            trial.set_room(room.id.clone(), operators.clone());
            if candidate_fits_rotation_shift(
                &trial,
                blueprint,
                instances,
                &room.id,
                operator,
                eligibility,
                mood_state,
            ) {
                used.insert(operator.name.clone());
                operators.push(operator.clone());
            }
        }
        if !operators.is_empty() {
            assignment.set_room(room.id.clone(), operators);
        }
    }
}

/// 回放已有的逐班 ETA；回收站补位不能使其他已定岗位失去心情预算。
fn fallback_candidate_eta_fits(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
    initial_mood: f64,
    name: &str,
) -> bool {
    if validate_operator_cycle_mood(blueprint, instances, report, initial_mood, name).is_err() {
        return false;
    }
    let model = default_mood_model().expect("mood model loaded for rotation");
    let mut mood: HashMap<String, f64> = operbox
        .roster()
        .names()
        .map(|name| (name.to_string(), initial_mood.min(model.mood_cap)))
        .collect();
    let full_rest = report.profile == TimedRotationProfile::Abc12_12_12;
    for (index, shift) in report.shifts.iter().enumerate() {
        if let Some(action) = &shift.fiammetta {
            mood.insert(action.target.clone(), model.mood_cap);
        }
        let first = (index + 1 == report.shifts.len())
            .then(|| report.shifts.first())
            .flatten()
            .map(|first| (&first.assignment, first.duration_hours));
        let eta = shift_eta_with_instances(&model, blueprint, &shift.assignment, instances);
        for entry in eta.per_op.iter().filter(|entry| entry.name == name) {
            let next = first
                .map(|(assignment, hours)| {
                    let eta = shift_eta_with_instances(&model, blueprint, assignment, instances);
                    eta.per_op
                        .iter()
                        .find(|operator| operator.name == name)
                        .map_or(0.0, |operator| operator.drain_per_hour * hours)
                })
                .unwrap_or(0.0);
            if !fits_shift_budget(
                &model,
                mood.get(name).copied().unwrap_or(model.mood_cap),
                entry.drain_per_hour,
                shift.duration_hours,
                next,
            ) {
                return false;
            }
        }
        advance_rotation_mood(
            &model,
            blueprint,
            instances,
            &shift.assignment,
            shift.duration_hours,
            full_rest,
            &mut mood,
        );
    }
    true
}

fn fill_rotation_vacancies(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    initial_mood: f64,
    report: &mut TeamRotationReport,
) -> Result<()> {
    let originally_occupied: HashSet<String> = report
        .shifts
        .iter()
        .flat_map(|shift| shift.assignment.occupied_operator_names())
        .collect();
    let mut excluded = HashSet::new();
    for plan in &report.assignment_plans {
        excluded.extend(plan.excluded_operators.iter().cloned());
        excluded.extend(plan.anchors.iter().map(|anchor| anchor.operator.clone()));
        excluded.extend(
            plan.rotation_reserves
                .iter()
                .flat_map(|reserve| reserve.operators.iter().cloned()),
        );
    }
    excluded.extend(
        report
            .runtime_shift_binds
            .iter()
            .flat_map(|bind| bind.operators.iter().cloned()),
    );
    for shift in &report.shifts {
        if let Some(rest) = &shift.dorm_rest {
            excluded.extend(dorm_rest_members(rest).map(str::to_string));
        }
        if shift_works_in_trade(blueprint, &shift.assignment, SPHINX_NAME) {
            excluded.insert(ULPIANUS_NAME.to_string());
        }
    }
    excluded.insert(FIAMMETTA_NAME.to_string());
    let allowed_operbox = if report.profile == TimedRotationProfile::MainBackup24_24 {
        filter_operbox_for_24_24(operbox)
    } else {
        operbox.clone()
    };
    let roster = allowed_operbox.roster_excluding(&excluded);
    let mut candidates: Vec<_> = roster
        .names()
        .filter_map(|name| {
            roster
                .progress(name)
                .map(|progress| AssignedOperator::from_progress(name, progress))
        })
        .collect();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    for index in 0..report.shifts.len() {
        if report.shifts[index].transition == ShiftTransition::FiammettaOnly
            || blueprint
                .rooms
                .iter()
                .filter(|room| ordinary_workplace(room.kind))
                .all(|room| {
                    report.shifts[index].assignment.operators_in(&room.id).len()
                        >= room.operator_capacity()
                })
        {
            continue;
        }
        let team_for = |name: &str| {
            let memberships: Vec<_> = report
                .teams
                .iter()
                .filter(|team| team.operators.iter().any(|operator| operator == name))
                .map(|team| team.label)
                .collect();
            let active = &report.shifts[index].active_teams;
            match memberships.as_slice() {
                [team] if active.contains(team) => Some(*team),
                [] if !originally_occupied.contains(name) => report
                    .teams
                    .iter()
                    .filter(|team| {
                        active.contains(&team.label)
                            && report
                                .shifts
                                .iter()
                                .any(|shift| !shift.active_teams.contains(&team.label))
                    })
                    .min_by_key(|team| team.operators.len())
                    .map(|team| team.label),
                _ => None,
            }
        };
        let Some(plan) = report.assignment_plans.get(report.shifts[index].plan_index) else {
            return Err(Error::msg(
                "ordinary_fallback_admission: missing owning Plan",
            ));
        };
        let continuation_indices: Vec<_> = (index + 1..report.shifts.len())
            .take_while(|next| report.shifts[*next].transition == ShiftTransition::FiammettaOnly)
            .collect();
        let budget_safe: Vec<_> = report
            .shifts
            .iter()
            .flat_map(|shift| shift.assignment.work_position_operator_names(blueprint))
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|name| {
                fallback_candidate_eta_fits(
                    blueprint,
                    &allowed_operbox,
                    instances,
                    report,
                    initial_mood,
                    name,
                )
            })
            .collect();
        let eligible: Vec<_> = candidates
            .iter()
            .filter(|operator| team_for(&operator.name).is_some())
            .cloned()
            .collect();
        let mut assignment = report.shifts[index].assignment.clone();
        let placements = fill_workplace_vacancies(
            blueprint,
            &mut assignment,
            &eligible,
            instances,
            table,
            |trial, room, candidate| {
                if !crate::layout::plan_admits_workplace_candidate(plan, trial, room, candidate) {
                    return false;
                }
                let mut trial_report = report.clone();
                trial_report.shifts[index].assignment = trial.clone();
                let before_names = report.shifts[index].assignment.operator_names();
                for next in &continuation_indices {
                    for added_room in trial.iter_rooms() {
                        for added in added_room
                            .operators
                            .iter()
                            .filter(|operator| !before_names.contains(&operator.name))
                        {
                            let next_assignment = &mut trial_report.shifts[*next].assignment;
                            let Some(spec) = blueprint.room(&added_room.room_id) else {
                                return false;
                            };
                            if next_assignment
                                .occupied_operator_names()
                                .contains(&added.name)
                                || next_assignment.operators_in(&spec.id).len()
                                    >= spec.operator_capacity()
                            {
                                return false;
                            }
                            let mut operators = next_assignment.operators_in(&spec.id).to_vec();
                            operators.push(added.clone());
                            next_assignment.set_room(spec.id.clone(), operators);
                            if !crate::layout::ordinary_room_compatible(
                                blueprint,
                                next_assignment,
                                spec,
                                instances,
                                table,
                            ) {
                                return false;
                            }
                        }
                    }
                }
                if room.kind == FacilityKind::Factory {
                    let members = trial
                        .operators_in(&room.id)
                        .iter()
                        .map(|operator| crate::manufacture::ManuOperator {
                            name: operator.name.clone(),
                            elite: operator.elite,
                            buff_ids: instances
                                .resolve_manufacture_buff_ids(&operator.name, operator.tier()),
                            tags: instances.tags_for(&operator.name, operator.tier()),
                        })
                        .collect::<Vec<_>>();
                    let hours = report.shifts[index].duration_hours
                        + continuation_indices
                            .iter()
                            .map(|next| report.shifts[*next].duration_hours)
                            .sum::<f64>();
                    if !crate::search::manufacture_combo_allowed_for_shift(
                        &members,
                        table,
                        Some(hours),
                    ) {
                        return false;
                    }
                }
                fallback_candidate_eta_fits(
                    blueprint,
                    &allowed_operbox,
                    instances,
                    &trial_report,
                    initial_mood,
                    &candidate.name,
                ) && budget_safe.iter().all(|name| {
                    fallback_candidate_eta_fits(
                        blueprint,
                        &allowed_operbox,
                        instances,
                        &trial_report,
                        initial_mood,
                        name,
                    )
                })
            },
        )?;
        let memberships: Vec<_> = placements
            .iter()
            .filter_map(|(_, operator)| {
                team_for(&operator.name).map(|team| (operator.name.clone(), team))
            })
            .collect();
        report.shifts[index].assignment = assignment;
        for next in &continuation_indices {
            for (room_id, operator) in &placements {
                let mut operators = report.shifts[*next]
                    .assignment
                    .operators_in(room_id)
                    .to_vec();
                operators.push(operator.clone());
                report.shifts[*next]
                    .assignment
                    .set_room(room_id.clone(), operators);
            }
        }
        for (name, team) in memberships {
            if let Some(cohort) = report.teams.iter_mut().find(|cohort| cohort.label == team) {
                if !cohort.operators.contains(&name) {
                    cohort.operators.push(name);
                    cohort.operators.sort();
                }
            }
        }
    }
    Ok(())
}

fn peak_meeting_operators(blueprint: &BaseBlueprint, peak: &BaseAssignment) -> HashSet<String> {
    peak.rooms_of_kind(blueprint, FacilityKind::MeetingRoom)
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
        .collect()
}

/// 被 active dependency 钉进 shared 的会客室成员按全周期共享（required 语义），
/// 不得进入 rotating meeting cohort，否则 resting 班会因 shared 保留而重复出现。
fn rotating_meeting_names_excluding_shared(
    peak_meeting_names: HashSet<String>,
    shared_names: &HashSet<String>,
) -> HashSet<String> {
    peak_meeting_names
        .into_iter()
        .filter(|name| !shared_names.contains(name))
        .collect()
}

/// 把全部生产设施（贸易/制造/发电）按同类房间交替切成两半，尽量均衡负载。
fn split_production_facilities(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
    reserves: &[ResolvedRoleReserve],
) -> Result<[FacilityHalf; 2]> {
    let room_kind: HashMap<_, _> = blueprint
        .rooms
        .iter()
        .map(|room| (room.id.clone(), room.kind))
        .collect();
    let mut half_components = crate::layout::pack_production_components(
        blueprint,
        peak,
        binds.iter().map(|bind| bind.operators.as_slice()),
    )
    .map_err(Error::msg)?;

    for reserve in reserves
        .iter()
        .filter(|reserve| reserve.reuse_policy == ReserveReusePolicy::EveryEligibleHalf)
    {
        let has_target = |components: &[Vec<RoomId>]| {
            components.iter().any(|component| {
                component
                    .iter()
                    .any(|room| reserve.eligible_rooms.contains(room))
            })
        };
        let has_h1 = has_target(&half_components[0]);
        let has_h2 = has_target(&half_components[1]);
        if has_h1 && has_h2 {
            continue;
        }
        if !has_h1 && !has_h2 {
            return Err(Error::msg(format!(
                "rotation reserve {} has no production component in H1/H2",
                reserve.reserve_id
            )));
        }
        if reserve.require_pre_split_halves {
            return Err(Error::msg(format!(
                "rotation reserve {} was not naturally pre-split across H1/H2 before schedule",
                reserve.reserve_id
            )));
        }
        let source = usize::from(!has_h1);
        let target = 1 - source;
        let eligible_count = |component: &[RoomId]| {
            component
                .iter()
                .filter(|room| reserve.eligible_rooms.contains(room))
                .count()
        };
        let component_index =
            half_components[source]
                .iter()
                .enumerate()
                .find_map(|(index, component)| {
                    (eligible_count(component) > 0
                        && half_components[source].iter().enumerate().any(
                            |(other_index, other)| {
                                other_index != index && eligible_count(other) > 0
                            },
                        ))
                    .then_some(index)
                });
        let Some(component_index) = component_index else {
            return Err(Error::msg(format!(
                "rotation reserve {} eligible rooms are inseparable across H1/H2",
                reserve.reserve_id
            )));
        };
        let moved = half_components[source].remove(component_index);
        half_components[target].push(moved);
    }

    let mut halves: [FacilityHalf; 2] = Default::default();
    for (half_index, components) in half_components.into_iter().enumerate() {
        for component in components {
            for room in component {
                match room_kind[&room] {
                    FacilityKind::TradePost => halves[half_index].trade.push(room),
                    FacilityKind::Factory => halves[half_index].manu.push(room),
                    FacilityKind::PowerPlant => halves[half_index].power.push(room),
                    _ => unreachable!(),
                }
            }
        }
    }
    eprintln!(
        "[rotation-debug] production halves H1 trade={:?} manu={:?} power={:?}; H2 trade={:?} manu={:?} power={:?}",
        halves[0].trade.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
        halves[0].manu.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
        halves[0].power.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
        halves[1].trade.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
        halves[1].manu.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
        halves[1].power.iter().map(|room| room.0.as_str()).collect::<Vec<_>>(),
    );
    Ok(halves)
}

/// γ 替补半区：贸易沿用 core role 顺序，制造/发电站绑定搜索。
/// 约束（禁同站/禁同房）统一取自 peak Plan，与主路径 pipeline 同源。
#[allow(clippy::too_many_arguments)]
fn assign_gamma_half(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    coexist_assignment: &BaseAssignment,
    durin_dorm_planning: Option<u8>,
    pools: &ProductionPools,
    table: &SkillTable,
    options: &AssignBaseOptions,
    half: &FacilityHalf,
    required_manu_by_room: &HashMap<RoomId, Vec<String>>,
    plan: &AssignmentPlan,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let layout = resolve_base(
        blueprint,
        coexist_assignment,
        Some(instances),
        Some(table),
        options.mood,
        durin_dorm_planning,
    )?
    .layout_snapshot();
    let trade_pool = pools.trade.clone();
    assign_team_gamma_half(
        blueprint,
        instances,
        coexist_assignment,
        durin_dorm_planning,
        &trade_pool,
        &pools.manu,
        &pools.power,
        table,
        &layout,
        options,
        &half.trade,
        &half.manu,
        &half.power,
        required_manu_by_room,
        plan,
        assignment,
        used,
    )
}

fn reserve_target_for_half(
    blueprint: &BaseBlueprint,
    reserve: &ResolvedRoleReserve,
    half: &FacilityHalf,
) -> Result<Option<RoomId>> {
    let half_rooms = match reserve.facility {
        FacilityKind::TradePost => &half.trade,
        FacilityKind::Factory => &half.manu,
        FacilityKind::PowerPlant => &half.power,
        facility => {
            return Err(Error::msg(format!(
                "rotation reserve {} has unsupported facility {:?}",
                reserve.reserve_id, facility
            )))
        }
    };
    let mut targets: Vec<_> = reserve
        .eligible_rooms
        .iter()
        .filter(|room_id| half_rooms.contains(room_id))
        .cloned()
        .collect();
    targets.sort_by(|left, right| left.0.cmp(&right.0));
    if let Some(room_id) = targets.first() {
        if blueprint.room(room_id).is_none() {
            return Err(Error::msg(format!(
                "rotation reserve {} target room {} missing",
                reserve.reserve_id, room_id.0
            )));
        }
    }
    Ok(targets.into_iter().next())
}

fn place_rotation_reserve_at(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    reserve: &ResolvedRoleReserve,
    room_id: &RoomId,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let room = blueprint.room(room_id).ok_or_else(|| {
        Error::msg(format!(
            "rotation reserve {} target room {} missing",
            reserve.reserve_id, room_id.0
        ))
    })?;
    if room.kind != reserve.facility {
        return Err(Error::msg(format!(
            "rotation reserve {} target room {} has facility {:?}, expected {:?}",
            reserve.reserve_id, room_id.0, room.kind, reserve.facility
        )));
    }
    if !assignment.operators_in(room_id).is_empty()
        || reserve.operators.len() > room.operator_capacity()
        || reserve.operators.iter().any(|name| used.contains(name))
    {
        return Err(Error::msg(format!(
            "rotation reserve {} cannot occupy {}",
            reserve.reserve_id, room_id.0
        )));
    }
    let operators = reserve
        .operators
        .iter()
        .map(|name| {
            let progress = operbox.progress_of(name).ok_or_else(|| {
                Error::msg(format!(
                    "rotation reserve {} operator {} missing from operbox",
                    reserve.reserve_id, name
                ))
            })?;
            Ok(AssignedOperator::from_progress(name, progress))
        })
        .collect::<Result<Vec<_>>>()?;
    used.extend(reserve.operators.iter().cloned());
    assignment.set_room(room_id.clone(), operators);
    Ok(())
}

fn place_rotation_reserves_for_halves(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    reserves: &[ResolvedRoleReserve],
    h1: &FacilityHalf,
    assignment_h1: &mut BaseAssignment,
    used_h1: &mut HashSet<String>,
    h2: &FacilityHalf,
    assignment_h2: &mut BaseAssignment,
    used_h2: &mut HashSet<String>,
) -> Result<()> {
    for reserve in reserves {
        let target_h1 = reserve_target_for_half(blueprint, reserve, h1)?;
        let target_h2 = reserve_target_for_half(blueprint, reserve, h2)?;
        match reserve.reuse_policy {
            ReserveReusePolicy::Once => {
                if let Some(target) = target_h1 {
                    place_rotation_reserve_at(
                        blueprint,
                        operbox,
                        reserve,
                        &target,
                        assignment_h1,
                        used_h1,
                    )?;
                } else if let Some(target) = target_h2 {
                    place_rotation_reserve_at(
                        blueprint,
                        operbox,
                        reserve,
                        &target,
                        assignment_h2,
                        used_h2,
                    )?;
                } else {
                    return Err(Error::msg(format!(
                        "rotation reserve {} has no eligible H1/H2 target",
                        reserve.reserve_id
                    )));
                }
            }
            ReserveReusePolicy::EveryEligibleHalf => {
                let target_h1 = target_h1.ok_or_else(|| {
                    Error::msg(format!(
                        "rotation reserve {} missing eligible H1 target",
                        reserve.reserve_id
                    ))
                })?;
                let target_h2 = target_h2.ok_or_else(|| {
                    Error::msg(format!(
                        "rotation reserve {} missing eligible H2 target; eligible={:?}; H1 trade={:?}; H2 trade={:?}",
                        reserve.reserve_id, reserve.eligible_rooms, h1.trade, h2.trade
                    ))
                })?;
                place_rotation_reserve_at(
                    blueprint,
                    operbox,
                    reserve,
                    &target_h1,
                    assignment_h1,
                    used_h1,
                )?;
                place_rotation_reserve_at(
                    blueprint,
                    operbox,
                    reserve,
                    &target_h2,
                    assignment_h2,
                    used_h2,
                )?;
            }
        }
    }
    Ok(())
}

/// `rotation_reserves` 只保证菲亚后处理之前的基础 γ 编制。这里在两半都完成普通
/// fill 后校验一次，防止后续改动在基础 rotation 阶段悄悄覆盖已解析 cohort。
fn validate_rotation_reserves_before_postprocess(
    blueprint: &BaseBlueprint,
    reserves: &[ResolvedRoleReserve],
    h1: &FacilityHalf,
    assignment_h1: &BaseAssignment,
    h2: &FacilityHalf,
    assignment_h2: &BaseAssignment,
) -> Result<()> {
    let matches_reserve =
        |assignment: &BaseAssignment, room_id: &RoomId, reserve: &ResolvedRoleReserve| {
            let actual = assignment.operators_in(room_id);
            actual.len() == reserve.operators.len()
                && actual
                    .iter()
                    .all(|operator| reserve.operators.contains(&operator.name))
        };

    for reserve in reserves {
        let target_h1 = reserve_target_for_half(blueprint, reserve, h1)?;
        let target_h2 = reserve_target_for_half(blueprint, reserve, h2)?;
        let require_match =
            |half: &str, target: Option<RoomId>, assignment: &BaseAssignment| -> Result<()> {
                let target = target.ok_or_else(|| {
                    Error::msg(format!(
                        "rotation reserve {} missing {half} target before postprocess",
                        reserve.reserve_id
                    ))
                })?;
                if !matches_reserve(assignment, &target, reserve) {
                    return Err(Error::msg(format!(
                        "rotation reserve {} was overwritten in {half} room {} before postprocess",
                        reserve.reserve_id, target.0
                    )));
                }
                Ok(())
            };

        match reserve.reuse_policy {
            ReserveReusePolicy::Once => {
                if let Some(target) = target_h1 {
                    require_match("H1", Some(target), assignment_h1)?;
                } else {
                    require_match("H2", target_h2, assignment_h2)?;
                }
            }
            ReserveReusePolicy::EveryEligibleHalf => {
                require_match("H1", target_h1, assignment_h1)?;
                require_match("H2", target_h2, assignment_h2)?;
            }
        }
    }
    Ok(())
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

fn print_trade_rotation_debug(label: &str, assignment: &BaseAssignment) {
    let mut rooms: Vec<_> = assignment
        .iter_rooms()
        .filter(|room| room.room_id.0.starts_with("trade_"))
        .map(|room| {
            (
                room.room_id.0.as_str(),
                room.operators
                    .iter()
                    .map(|operator| operator.name.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    rooms.sort_by(|left, right| left.0.cmp(right.0));
    eprintln!("[rotation-debug] {label} trade={rooms:?}");
}

fn active_dependency_required_manu_by_room(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    dependencies: &[crate::layout::ActiveDependency],
    binds: &[RuntimeShiftBind],
) -> HashMap<RoomId, Vec<String>> {
    // 已在班次绑定里的 required 成员跟随其绑定组同上同下，不能再被当作 γ 替补必须带上的人，
    // 否则（例如多萝西既是莱茵挂件又在制造站）会在绑定组休息的班次单独上岗。
    let required: HashSet<&str> = dependencies
        .iter()
        .flat_map(|dependency| dependency.required.iter().map(String::as_str))
        .filter(|name| {
            !binds
                .iter()
                .any(|bind| bind.operators.iter().any(|member| member == name))
        })
        .collect();
    let mut by_room = HashMap::new();
    for room in peak.iter_rooms() {
        if !blueprint
            .room(&room.room_id)
            .is_some_and(|blueprint_room| blueprint_room.kind == FacilityKind::Factory)
        {
            continue;
        }
        let names: Vec<String> = room
            .operators
            .iter()
            .filter(|operator| required.contains(operator.name.as_str()))
            .map(|operator| operator.name.clone())
            .collect();
        if !names.is_empty() {
            by_room.insert(room.room_id.clone(), names);
        }
    }
    by_room
}

fn bound_control_operators_for_half(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
    half: &FacilityHalf,
) -> HashSet<String> {
    let room_for = |name: &str| peak.room_of_operator(name);
    binds
        .iter()
        .filter(|bind| {
            bind.operators.iter().any(|name| {
                room_for(name).is_some_and(|room| {
                    half.trade.contains(&room.room_id)
                        || half.manu.contains(&room.room_id)
                        || half.power.contains(&room.room_id)
                })
            })
        })
        .flat_map(|bind| &bind.operators)
        .filter(|name| {
            room_for(name).is_some_and(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|room| room.kind == FacilityKind::ControlCenter)
            })
        })
        .cloned()
        .collect()
}

struct GammaProductionContext {
    assignment: BaseAssignment,
}

/// γ 接替某半区时，该半区所属 bind 队正在休息；其绑定中枢 producer 不能继续
/// 出现在搜索上下文。未绑定 producer 允许进入 γ 候选，收益按实际在场结算
/// （ADR 0004 D6：轮换搜索 heuristic，不后置新建 cohort）。
fn gamma_production_coexist_seed(
    blueprint: &BaseBlueprint,
    shared: &BaseAssignment,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
    replaced_half: &FacilityHalf,
    active_half_assignment: &BaseAssignment,
) -> GammaProductionContext {
    let resting_control = bound_control_operators_for_half(blueprint, peak, binds, replaced_half);
    let mut seed = shared.clone();
    let active_control: Vec<_> = peak
        .control_operators()
        .into_iter()
        .filter(|operator| !resting_control.contains(&operator.name))
        .collect();
    if !active_control.is_empty() {
        seed.set_room("control", active_control);
    }
    merge_rooms(&mut seed, active_half_assignment);
    GammaProductionContext { assignment: seed }
}

fn validate_fiammetta_continuous_targets(
    blueprint: &BaseBlueprint,
    profile: TimedRotationProfile,
    shifts: &[TeamShiftResult],
) -> Result<()> {
    // These profiles keep a served target in every final production shift;
    // two-shift owner profiles have a different, local target lifetime.
    if !matches!(
        profile,
        TimedRotationProfile::Abc12_6_6
            | TimedRotationProfile::Abc12_12_12
            | TimedRotationProfile::Fiammetta8_8_4_4
    ) {
        return Ok(());
    }
    let targets: HashSet<_> = shifts
        .iter()
        .filter_map(|shift| shift.fiammetta.as_ref())
        .map(|action| action.target.as_str())
        .collect();
    for shift in shifts {
        let production_names: HashSet<_> = shift
            .assignment
            .iter_rooms()
            .filter(|room| {
                blueprint.room(&room.room_id).is_some_and(|room| {
                    matches!(
                        room.kind,
                        FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                    )
                })
            })
            .flat_map(|room| room.operators.iter().map(|operator| operator.name.as_str()))
            .collect();
        for target in &targets {
            if !production_names.contains(*target) {
                return Err(Error::msg(format!(
                    "fiammetta_continuous_target_present failed in shift {}: target {} missing",
                    shift.index + 1,
                    target
                )));
            }
        }
    }
    Ok(())
}

fn fiammetta_action_slots(profile: TimedRotationProfile, shift_count: usize) -> Vec<usize> {
    let count = match profile {
        TimedRotationProfile::MainBackup12_12 | TimedRotationProfile::MainBackup24_24 => 2,
        TimedRotationProfile::Abc12_12_12 => 3,
        TimedRotationProfile::Abc12_6_6 => 2,
        TimedRotationProfile::Fiammetta8_8_4_4 => 3,
        _ => 0,
    };
    (0..count.min(shift_count)).collect()
}

fn validate_fiammetta_action_policy(
    profile: TimedRotationProfile,
    shifts: &[TeamShiftResult],
) -> Result<()> {
    let slots = fiammetta_action_slots(profile, shifts.len());
    let mut targets = HashSet::new();
    for shift in shifts {
        if let Some(action) = &shift.fiammetta {
            if !slots.contains(&shift.index) {
                return Err(Error::msg(format!(
                    "fiammetta_action_slot failed in shift {}",
                    shift.index + 1
                )));
            }
            if !targets.insert(action.target.clone()) {
                return Err(Error::msg(format!(
                    "fiammetta_target_unique failed: target {} is repeated",
                    action.target
                )));
            }
            if !fiammetta_target_is_legal(profile, &action.target) {
                return Err(Error::msg(format!(
                    "fiammetta_target_invalid failed in shift {}: target {} is not a legal target",
                    shift.index + 1,
                    action.target
                )));
            }
        }
    }
    Ok(())
}

fn fiammetta_target_is_legal(profile: TimedRotationProfile, target: &str) -> bool {
    let _ = profile;
    matches!(target, "但书" | "可露希尔" | "清流" | "巫恋" | "龙舌兰")
}

fn first_fiammetta_assignment_target(
    profile: TimedRotationProfile,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
) -> Option<(String, RoomId)> {
    fiammetta_return_priority(blueprint)
        .into_iter()
        .filter(|target| fiammetta_target_is_legal(profile, target))
        .filter(|target| !used.contains(*target))
        .find_map(|target| {
            assignment.iter_rooms().find_map(|room| {
                let production = blueprint.room(&room.room_id).is_some_and(|room| {
                    matches!(
                        room.kind,
                        FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                    )
                });
                (production
                    && room
                        .operators
                        .iter()
                        .any(|operator| operator.name == target))
                .then(|| (target.to_string(), room.room_id.clone()))
            })
        })
}

fn validate_office_assignment_disjoint(
    blueprint: &BaseBlueprint,
    shift: &TeamShiftResult,
) -> Result<()> {
    let mut production_names = HashSet::new();
    let mut office_names = HashSet::new();
    for room in shift.assignment.iter_rooms() {
        let Some(spec) = blueprint.room(&room.room_id) else {
            continue;
        };
        for operator in &room.operators {
            match spec.kind {
                FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant => {
                    production_names.insert(operator.name.clone());
                }
                FacilityKind::Office => {
                    office_names.insert(operator.name.clone());
                }
                _ => {}
            }
        }
    }
    let mut conflicts: Vec<String> = office_names
        .intersection(&production_names)
        .cloned()
        .collect();
    if let Some(assistant) = &shift.assignment.training_assist {
        if office_names.contains(&assistant.name) {
            conflicts.push(assistant.name.clone());
        }
    }
    conflicts.sort();
    conflicts.dedup();
    if !conflicts.is_empty() {
        return Err(Error::msg(format!(
            "office_current_assignment_disjoint failed in shift {}: {:?}",
            shift.index + 1,
            conflicts
        )));
    }
    Ok(())
}

fn validate_office_shift_eta(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    shift: &TeamShiftResult,
) -> Result<()> {
    let model = default_mood_model()?;
    let eta = shift_eta_with_instances(&model, blueprint, &shift.assignment, instances);
    for room in shift
        .assignment
        .rooms_of_kind(blueprint, FacilityKind::Office)
    {
        for operator in &room.operators {
            let Some(detail) = eta
                .per_op
                .iter()
                .find(|detail| detail.name == operator.name && detail.room_id == room.room_id.0)
            else {
                continue;
            };
            if detail.drain_per_hour > 0.0
                && detail.drain_per_hour * shift.duration_hours > model.workable_mood() + 1e-9
            {
                return Err(Error::msg(format!(
                    "office_current_shift_eta failed in shift {} for {}",
                    shift.index + 1,
                    operator.name
                )));
            }
        }
    }
    Ok(())
}

fn validate_final_rotation_invariants(
    blueprint: &BaseBlueprint,
    shifts: &[TeamShiftResult],
    binds: &[RuntimeShiftBind],
    active_dependencies: &[crate::layout::ActiveDependency],
) -> Result<()> {
    for shift in shifts {
        validate_office_assignment_disjoint(blueprint, shift)?;
        let mut seen = HashSet::new();
        for room in shift.assignment.iter_rooms() {
            for operator in &room.operators {
                if !seen.insert(operator.name.clone()) {
                    return Err(Error::msg(format!(
                        "shift {} operator {} appears in multiple room slots",
                        shift.index + 1,
                        operator.name
                    )));
                }
            }
        }
        let control = shift.assignment.control_operators();
        let control_capacity = blueprint
            .rooms_of(FacilityKind::ControlCenter)
            .first()
            .map(|room| room.operator_capacity())
            .unwrap_or(0);
        if control.len() > control_capacity {
            return Err(Error::msg(format!(
                "shift {} control center has {} operators, exceeds capacity {}",
                shift.index + 1,
                control.len(),
                control_capacity
            )));
        }

        let present = shift.assignment.operator_names();
        for bind in binds {
            let flags: Vec<_> = bind
                .operators
                .iter()
                .map(|name| present.contains(name))
                .collect();
            if flags
                .first()
                .is_some_and(|first| flags.iter().any(|flag| flag != first))
            {
                let active: Vec<_> = bind
                    .operators
                    .iter()
                    .zip(&flags)
                    .filter(|(_, present)| **present)
                    .map(|(name, _)| name)
                    .collect();
                return Err(Error::msg(format!(
                    "shift {} bind {:?} does not move together; active={active:?}",
                    shift.index + 1,
                    bind.operators
                )));
            }
        }
        for dependency in active_dependencies {
            let consumer_active = dependency
                .consumers
                .iter()
                .any(|name| present.contains(name));
            if consumer_active {
                let missing: Vec<_> = dependency
                    .required
                    .iter()
                    .filter(|name| !present.contains(*name))
                    .collect();
                if !missing.is_empty() {
                    return Err(Error::msg(format!(
                        "shift {} active dependency {:?} missing required {:?}",
                        shift.index + 1,
                        dependency.consumers,
                        missing
                    )));
                }
            }
        }
    }

    for bind in binds {
        if shifts.len() != 3 {
            continue;
        }
        let active = shifts
            .iter()
            .filter(|shift| {
                let present = shift.assignment.operator_names();
                bind.operators.iter().all(|name| present.contains(name))
            })
            .count();
        let inactive = shifts.len().saturating_sub(active);
        if active != usize::from(bind.on_shifts) || inactive != usize::from(bind.off_shifts) {
            return Err(Error::msg(format!(
                "bind {:?} expected {} active/{} resting shifts, got {active}/{inactive}",
                bind.operators, bind.on_shifts, bind.off_shifts
            )));
        }
    }

    Ok(())
}

fn operator_is_in_normal_base(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    name: &str,
) -> bool {
    assignment.iter_rooms().any(|room| {
        blueprint.room(&room.room_id).is_some_and(|spec| {
            spec.kind != FacilityKind::MeetingRoom
                && room.operators.iter().any(|operator| operator.name == name)
        })
    }) && assignment
        .training_assist
        .as_ref()
        .is_none_or(|assist| assist.name != name)
}

fn shift_works_in_trade(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    name: &str,
) -> bool {
    assignment.iter_rooms().any(|room| {
        blueprint.room(&room.room_id).is_some_and(|spec| {
            spec.kind == FacilityKind::TradePost
                && room.operators.iter().any(|operator| operator.name == name)
        })
    })
}

fn dorm_rest_members(rest: &DormRestPlan) -> [&str; 3] {
    [
        rest.single_manager.as_str(),
        rest.group_manager.as_str(),
        rest.target.as_str(),
    ]
}

fn finalize_shift_dorms(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    shift: &mut TeamShiftResult,
) -> Result<()> {
    let has_action = shift.fiammetta.is_some();
    let mut preserved_by_room: HashMap<RoomId, Vec<AssignedOperator>> = blueprint
        .rooms_of(FacilityKind::Dormitory)
        .into_iter()
        .map(|room| {
            (
                room.id.clone(),
                shift.assignment.operators_in(&room.id).to_vec(),
            )
        })
        .collect();

    let recovery_members = shift
        .dorm_rest
        .as_ref()
        .map(dorm_rest_members)
        .unwrap_or(["", "", ""]);
    for occupants in preserved_by_room.values_mut() {
        occupants.retain(|operator| {
            operator.name != FIAMMETTA_NAME
                && !recovery_members.iter().any(|name| *name == operator.name)
        });
    }

    let all_non_dorm = shift.assignment.work_position_operator_names(blueprint);
    if has_action && all_non_dorm.contains(FIAMMETTA_NAME) {
        return Err(Error::msg(format!(
            "shift {} fiammetta_dorm_admission failed: Fiammetta is already assigned outside a dorm",
            shift.index + 1
        )));
    }

    let dorms = blueprint.rooms_of(FacilityKind::Dormitory);
    let fiammetta_room = if has_action {
        let progress = operbox
            .progress_of(FIAMMETTA_NAME)
            .ok_or_else(|| Error::msg("fiammetta action requires an owned Fiammetta operator"))?;
        let room = dorms
            .iter()
            .find(|room| {
                preserved_by_room.get(&room.id).map_or(0, Vec::len)
                    < room.dorm_bed_count().max(1) as usize
            })
            .ok_or_else(|| {
                Error::msg(format!(
                    "shift {} fiammetta_dorm_capacity failed: no bed for Fiammetta",
                    shift.index + 1
                ))
            })?;
        let occupants = preserved_by_room
            .get_mut(&room.id)
            .expect("dorm was collected");
        occupants.insert(0, AssignedOperator::from_progress(FIAMMETTA_NAME, progress));
        Some(room.id.clone())
    } else {
        None
    };

    if let Some(rest) = &shift.dorm_rest {
        let room = blueprint
            .room(&rest.room_id)
            .ok_or_else(|| Error::msg("dorm recovery room disappeared from blueprint"))?;
        let occupants = preserved_by_room
            .get_mut(&rest.room_id)
            .expect("dorm recovery room was collected");
        let capacity = room.dorm_bed_count().max(1) as usize;
        if occupants.len() + 3 > capacity {
            return Err(Error::msg(format!(
                "shift {} dorm_recovery_group_capacity failed",
                shift.index + 1
            )));
        }
        for name in dorm_rest_members(rest) {
            let progress = operbox.progress_of(name).ok_or_else(|| {
                Error::msg(format!(
                    "dorm recovery member {name} disappeared from operbox"
                ))
            })?;
            occupants.push(AssignedOperator::from_progress(name, progress));
        }
    }

    if shift_works_in_trade(blueprint, &shift.assignment, SPHINX_NAME)
        && !operator_is_in_normal_base(blueprint, &shift.assignment, ULPIANUS_NAME)
        && !preserved_by_room
            .values()
            .flatten()
            .any(|operator| operator.name == ULPIANUS_NAME)
    {
        let progress = operbox
            .progress_of(ULPIANUS_NAME)
            .ok_or_else(|| Error::msg("Sphinx trade assignment requires an owned Ulpianus"))?;
        let room = dorms
            .iter()
            .find(|room| {
                preserved_by_room.get(&room.id).map_or(0, Vec::len)
                    < room.dorm_bed_count().max(1) as usize
            })
            .ok_or_else(|| {
                Error::msg(format!(
                    "shift {} sphinx_ulpianus_presence failed: no dorm bed for Ulpianus",
                    shift.index + 1
                ))
            })?;
        preserved_by_room
            .get_mut(&room.id)
            .expect("dorm was collected")
            .push(AssignedOperator::from_progress(ULPIANUS_NAME, progress));
    }

    if shift_works_in_trade(blueprint, &shift.assignment, SPHINX_NAME)
        && !operator_is_in_normal_base(blueprint, &shift.assignment, ULPIANUS_NAME)
        && !preserved_by_room
            .values()
            .flatten()
            .any(|operator| operator.name == ULPIANUS_NAME)
    {
        return Err(Error::msg(format!(
            "shift {} sphinx_ulpianus_presence failed: Ulpianus is absent while Sphinx works in trade",
            shift.index + 1
        )));
    }

    for room in &dorms {
        let occupants = preserved_by_room
            .get_mut(&room.id)
            .expect("dorm was collected");
        shift
            .assignment
            .set_room(room.id.clone(), occupants.clone());
    }

    if let Some(room_id) = fiammetta_room {
        let first = shift
            .assignment
            .operators_in(&room_id)
            .first()
            .map(|operator| operator.name.as_str());
        if first != Some(FIAMMETTA_NAME) {
            return Err(Error::msg(format!(
                "shift {} fiammetta_dorm_first failed",
                shift.index + 1
            )));
        }
    }
    Ok(())
}

fn complete_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    initial_mood: f64,
    mut report: TeamRotationReport,
) -> Result<TeamRotationReport> {
    for shift in &mut report.shifts {
        let mut trial = shift.clone();
        match finalize_shift_dorms(blueprint, operbox, &mut trial) {
            Ok(()) => *shift = trial,
            Err(error) => report
                .warnings
                .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}")),
        }
    }
    if let Err(error) = fill_rotation_vacancies(
        blueprint,
        operbox,
        instances,
        table,
        initial_mood,
        &mut report,
    ) {
        return Err(Error::with_rotation(&report, error));
    }
    finalize_timed_rotation(blueprint, operbox, instances, table, initial_mood, report)
}

fn finalize_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    initial_mood: f64,
    mut report: TeamRotationReport,
) -> Result<TeamRotationReport> {
    let total_started = Instant::now();
    let cycle_hours: f64 = report.shifts.iter().map(|shift| shift.duration_hours).sum();
    if cycle_hours <= 0.0 {
        return Err(Error::msg("rotation cycle must have positive duration"));
    }
    let total_before = crate::profile::solver_perf_snapshot();
    let stage_started = Instant::now();
    let stage_before = total_before;
    for shift in &report.shifts {
        print_trade_rotation_debug(
            &format!("finalize shift {} input", shift.index + 1),
            &shift.assignment,
        );
    }
    let mut dorm_shifts = report.shifts.clone();
    let dorm_warnings = match dorm_finalizer::finalize_dorms(blueprint, operbox, &mut dorm_shifts) {
        Ok(warnings) => {
            report.shifts = dorm_shifts;
            warnings
                .into_iter()
                .map(dorm_finalizer::render_warning)
                .collect::<Vec<_>>()
        }
        Err(error) => {
            report
                .warnings
                .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
            Vec::new()
        }
    };
    let mood_model = default_mood_model()?;
    report.warnings.extend(
        fiammetta_events::retain_ready_actions(
            &mut report.shifts,
            &mood_model,
            blueprint,
            instances,
        )
        .into_iter()
        .map(|warning| format!("{FINAL_INVARIANT_WARNING_PREFIX} {warning}")),
    );
    for shift in &mut report.shifts {
        if shift.transition == ShiftTransition::FiammettaOnly && shift.fiammetta.is_none() {
            shift.transition = ShiftTransition::ReplaceRooms;
            shift.drone = drone_action(blueprint, shift.transition);
        }
    }
    let mut stage_after = crate::profile::solver_perf_snapshot();
    crate::profile::emit_solver_stage(
        "rotation_finalize",
        "dorm_prepare",
        "ok",
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    let stage_started = Instant::now();
    let stage_before = stage_after;
    for shift in &mut report.shifts {
        clear_production_efficiencies(blueprint, &mut shift.assignment);
        shift.efficiencies = if shift.transition == ShiftTransition::FiammettaOnly {
            let action = shift
                .fiammetta
                .as_ref()
                .expect("Fiammetta-only shift has action");
            match score_fiammetta_continuation(
                blueprint,
                &shift.assignment,
                instances,
                table,
                operbox.durin_dorm_planning_count(instances),
                &action.room_id,
            ) {
                Ok(scores) => scores,
                Err(error) => return Err(Error::with_rotation(&report, error)),
            }
        } else {
            match evaluate_base_assignment_efficiencies(
                blueprint,
                &shift.assignment,
                instances,
                table,
                shift.duration_hours,
                Some(operbox.durin_dorm_planning_count(instances)),
            ) {
                Ok(scores) => scores,
                Err(error) => return Err(Error::with_rotation(&report, error)),
            }
        };
        shift.weighted_trade = shift
            .efficiencies
            .weighted_trade_for_cycle(shift.duration_hours, cycle_hours);
        shift.weighted_manufacture = shift
            .efficiencies
            .weighted_manufacture_for_cycle(shift.duration_hours, cycle_hours);
        shift.weighted_power = shift
            .efficiencies
            .weighted_power_for_cycle(shift.duration_hours, cycle_hours);
        annotate_assignment(blueprint, &mut shift.assignment);
        print_trade_rotation_debug(
            &format!("finalize shift {} output", shift.index + 1),
            &shift.assignment,
        );
    }
    stage_after = crate::profile::solver_perf_snapshot();
    crate::profile::emit_solver_stage(
        "rotation_finalize",
        "rescore",
        "ok",
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    let stage_started = Instant::now();
    let stage_before = stage_after;
    report.daily = DailyTotals {
        trade: report.shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: report
            .shifts
            .iter()
            .map(|shift| shift.weighted_manufacture)
            .sum(),
        power: report.shifts.iter().map(|shift| shift.weighted_power).sum(),
        production: average_cycle_production(blueprint, &report.shifts),
    };
    let quality_warnings: Vec<_> = report
        .warnings
        .into_iter()
        .filter(|warning| warning.starts_with(FINAL_INVARIANT_WARNING_PREFIX))
        .collect();
    report.warnings = rotation_shortfall_warnings(blueprint, &report.shifts);
    report.warnings.extend(dorm_warnings);
    report.warnings.extend(quality_warnings);
    let cycle_rest_warnings = rotation_cycle_rest_warnings(blueprint, &report);
    report.warnings.extend(cycle_rest_warnings);
    let structure_result = validate_final_bind_cycle(blueprint, &report)
        .map_err(Error::msg)
        .and_then(|_| {
            if matches!(
                report.profile,
                TimedRotationProfile::Abc12_6_6
                    | TimedRotationProfile::Abc12_12_12
                    | TimedRotationProfile::Fiammetta8_8_4_4
            ) {
                validate_final_rotation_invariants(blueprint, &report.shifts, &[], &[])
            } else {
                validate_profile_rotation(blueprint, instances, &report)
            }
        });
    if let Err(error) = structure_result {
        report
            .warnings
            .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    }
    if let Err(error) = validate_mood_gap_decay_work_spans(
        blueprint,
        instances,
        table,
        report.profile,
        &report.shifts,
    ) {
        report
            .warnings
            .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    }
    if let Err(error) = validate_fiammetta_action_policy(report.profile, &report.shifts) {
        report
            .warnings
            .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    }
    if let Err(error) =
        validate_fiammetta_continuous_targets(blueprint, report.profile, &report.shifts)
    {
        report
            .warnings
            .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    }
    if let Err(error) = validate_cycle_mood(blueprint, instances, &report, initial_mood) {
        return Err(Error::with_rotation(&report, error));
    }
    stage_after = crate::profile::solver_perf_snapshot();
    crate::profile::emit_solver_stage(
        "rotation_finalize",
        "dorm_assign_and_totals",
        "ok",
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    let stage_started = Instant::now();
    let stage_before = stage_after;
    let invariant_result = assert_final_rotation_invariants_with_mood(
        blueprint,
        operbox,
        instances,
        table,
        &report,
        initial_mood,
    );
    stage_after = crate::profile::solver_perf_snapshot();
    crate::profile::emit_solver_stage(
        "rotation_finalize",
        "invariants",
        if invariant_result.is_ok() {
            "ok"
        } else {
            "error"
        },
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    if let Err(error) = invariant_result {
        // 最终不变量违规不拒绝输出：排班照常返回，违规作为诊断警告交给人工修正。
        report
            .warnings
            .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    }
    crate::profile::emit_solver_stage(
        "rotation_finalize",
        "total",
        "ok",
        total_started.elapsed(),
        total_before,
        stage_after,
    );
    Ok(report)
}

struct ProductionPools {
    trade: crate::pool::TradePool,
    manu: crate::pool::ManuPool,
    power: crate::pool::PowerPool,
}

fn operators_of(assignment: &BaseAssignment) -> Vec<String> {
    let mut names: Vec<String> = assignment
        .iter_rooms()
        .flat_map(|r| r.operators.iter().map(|o| o.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn merge_rooms(target: &mut BaseAssignment, source: &BaseAssignment) {
    for room in source.iter_rooms() {
        if room.operators.is_empty() {
            continue;
        }
        target.set_room_assignment(room.clone());
    }
}

fn clear_room(assignment: &mut BaseAssignment, room_id: &str) {
    assignment.remove_room(room_id);
}

fn clear_room_efficiency(assignment: &mut BaseAssignment, room_id: &RoomId) {
    assignment.clear_room_efficiency(room_id);
}

/// Compatibility order for callers that do not have a blueprint. Two-trade
/// layouts use this order; layout-aware callers must use
/// `fiammetta_return_priority`.
pub const FIAMMETTA_RETURN_PRIORITY: [&str; 5] = ["但书", "可露希尔", "巫恋", "龙舌兰", "清流"];

pub fn fiammetta_return_priority(blueprint: &BaseBlueprint) -> Vec<&'static str> {
    let trade_station_count = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::TradePost)
        .count();
    if trade_station_count == 1 {
        vec!["可露希尔", "巫恋", "龙舌兰", "但书", "清流"]
    } else if trade_station_count >= 3 {
        vec!["但书", "可露希尔", "清流", "巫恋", "龙舌兰"]
    } else {
        vec!["但书", "可露希尔", "巫恋", "龙舌兰", "清流"]
    }
}

fn has_multi_member_shift_bind(name: &str, shift_binds: &[RuntimeShiftBind]) -> bool {
    shift_binds.iter().any(|bind| {
        bind.operators.len() > 1 && bind.operators.iter().any(|operator| operator == name)
    })
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn apply_single_core_fiammetta_rotation_legacy(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
    teams: &[TeamAssignment],
    continuous_roles: &[crate::layout::ContinuousRole],
    shift_binds: &[RuntimeShiftBind],
    shifts: &mut [TeamShiftResult],
) -> Result<bool> {
    if !operbox.owns("菲亚梅塔") || shifts.is_empty() {
        return Ok(false);
    }
    if !matches!(
        profile,
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12
    ) {
        return Ok(false);
    }
    let cycle_hours: f64 = shifts.iter().map(|shift| shift.duration_hours).sum();
    let model = default_mood_model()?;
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let durin_plan = operbox.durin_dorm_planning_count(instances);

    for target in fiammetta_return_priority(blueprint) {
        if target == "巫恋" {
            let witch_room_exists = shifts.iter().any(|shift| {
                shift.assignment.iter_rooms().any(|room| {
                    blueprint.room(&room.room_id).is_some_and(|blueprint_room| {
                        blueprint_room.kind == FacilityKind::TradePost
                            && witch_core_station_ok(blueprint_room)
                            && room
                                .operators
                                .iter()
                                .any(|operator| operator.name == "巫恋")
                            && room
                                .operators
                                .iter()
                                .any(|operator| operator.name == "龙舌兰")
                    })
                })
            });
            if witch_room_exists && beta_tailor_names(&trade_pool, table).len() >= 2 {
                return Ok(false);
            }
            continue;
        }
        if !matches!(target, "但书" | "可露希尔" | "清流") {
            continue;
        }
        if target == "清流" && operbox.elite_of(target).is_some_and(|elite| elite == 0) {
            continue;
        }
        let room_id = continuous_roles
            .iter()
            .find(|role| role.operator == target)
            .map(|role| role.room_id.clone())
            .or_else(|| {
                shifts.iter().find_map(|shift| {
                    shift
                        .assignment
                        .room_of_operator(target)
                        .map(|room| room.room_id.clone())
                })
            });
        let Some(room_id) = room_id else {
            continue;
        };
        let Some(room_kind) = blueprint.room(&room_id).map(|room| room.kind) else {
            continue;
        };
        let mut candidate_shifts = shifts.to_vec();
        let mut refill_failed = false;
        for shift in &mut candidate_shifts {
            if shift.assignment.operator_names().contains(target) {
                continue;
            }
            if shift
                .assignment
                .operators_in(&room_id)
                .iter()
                .any(|operator| {
                    operator.name != target
                        && has_multi_member_shift_bind(&operator.name, shift_binds)
                })
            {
                refill_failed = true;
                break;
            }
            let mut shift_options = options.clone();
            shift_options.shift_hours = shift.duration_hours;
            let mut active_names = active_team_names(teams, &shift.active_teams);
            active_names.insert(target.to_string());
            let forbidden_names: Vec<String> = match room_kind {
                FacilityKind::TradePost => trade_pool
                    .entries
                    .iter()
                    .filter(|entry| !active_names.contains(&entry.name))
                    .map(|entry| entry.name.clone())
                    .collect(),
                FacilityKind::Factory => manu_pool
                    .entries
                    .iter()
                    .filter(|entry| !active_names.contains(&entry.name))
                    .map(|entry| entry.name.clone())
                    .collect(),
                _ => Vec::new(),
            };
            let refill = match room_kind {
                FacilityKind::TradePost => refill_trade_room_with_core(
                    blueprint,
                    &room_id,
                    target,
                    &trade_pool,
                    table,
                    instances,
                    &shift_options,
                    Some(durin_plan),
                    &forbidden_names,
                    &mut shift.assignment,
                ),
                FacilityKind::Factory if target == "清流" => refill_manu_room_with_core(
                    blueprint,
                    &room_id,
                    target,
                    &manu_pool,
                    table,
                    instances,
                    &shift_options,
                    Some(durin_plan),
                    &forbidden_names,
                    &mut shift.assignment,
                ),
                _ => Err(Error::msg("unsupported Fiammetta continuous core facility")),
            };
            if refill.is_err() {
                refill_failed = true;
                break;
            }
        }
        if refill_failed {
            continue;
        }

        let target_team = teams
            .iter()
            .find(|team| team.operators.iter().any(|operator| operator == target))
            .map(|team| team.label);
        let action_candidates: Vec<_> = if target_team.is_some() {
            candidate_shifts
                .iter()
                .enumerate()
                .filter_map(|(index, shift)| {
                    (Some(shift.resting_team) == target_team).then_some(index)
                })
                .chain(0..candidate_shifts.len())
                .collect()
        } else {
            (0..candidate_shifts.len()).collect()
        };
        let action_indices = fiammetta_ready_events(
            &candidate_shifts,
            target,
            &action_candidates,
            &model,
            blueprint,
            instances,
        );
        if action_indices.is_empty() {
            continue;
        }
        for index in action_indices {
            let action_room_id = candidate_shifts[index]
                .assignment
                .room_of_operator(target)
                .map(|room| room.room_id.clone())
                .ok_or_else(|| {
                    Error::msg(format!(
                        "Fiammetta target {target} missing after continuous refill"
                    ))
                })?;
            if !fiammetta_events::try_accept_action(
                &mut candidate_shifts,
                index,
                target,
                &action_room_id,
                &model,
                blueprint,
                instances,
            ) {
                continue;
            }
        }

        rescore_standard_shifts(
            blueprint,
            instances,
            table,
            durin_plan,
            cycle_hours,
            &mut candidate_shifts,
        )?;
        if validate_final_rotation_invariants(blueprint, &candidate_shifts, shift_binds, &[])
            .is_err()
        {
            continue;
        }
        shifts.clone_from_slice(&candidate_shifts);
        return Ok(true);
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn apply_profile_fiammetta_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
    teams: &[TeamAssignment],
    continuous_roles: &[crate::layout::ContinuousRole],
    shift_binds: &[RuntimeShiftBind],
    shifts: &mut [TeamShiftResult],
) -> Result<bool> {
    if !operbox.owns("菲亚梅塔") || shifts.is_empty() {
        return Ok(false);
    }
    let slots = fiammetta_action_slots(profile, shifts.len());
    if slots.is_empty() {
        return Ok(false);
    }
    let cycle_hours: f64 = shifts.iter().map(|shift| shift.duration_hours).sum();
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(instances), instances, table)?;
    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mut candidate_shifts = shifts.to_vec();
    let mut used_targets = HashSet::new();
    let mut applied = false;

    for slot in slots {
        let mut selected = false;
        for target in fiammetta_return_priority(blueprint) {
            if used_targets.contains(target)
                || !matches!(target, "但书" | "可露希尔" | "清流")
                || has_multi_member_shift_bind(target, shift_binds)
                || (target == "清流" && operbox.elite_of(target).is_some_and(|elite| elite == 0))
            {
                continue;
            }
            let Some(room_id) = continuous_roles
                .iter()
                .find(|role| role.operator == target)
                .map(|role| role.room_id.clone())
                .or_else(|| {
                    candidate_shifts.iter().find_map(|shift| {
                        shift
                            .assignment
                            .room_of_operator(target)
                            .map(|room| room.room_id.clone())
                    })
                })
            else {
                continue;
            };
            let Some(room_kind) = blueprint.room(&room_id).map(|room| room.kind) else {
                continue;
            };
            let mut trial = candidate_shifts.clone();
            let mut failed = false;
            for shift in &mut trial {
                if shift.assignment.operator_names().contains(target) {
                    continue;
                }
                if shift
                    .assignment
                    .operators_in(&room_id)
                    .iter()
                    .any(|operator| {
                        operator.name != target
                            && has_multi_member_shift_bind(&operator.name, shift_binds)
                    })
                {
                    failed = true;
                    break;
                }
                let mut shift_options = options.clone();
                shift_options.shift_hours = shift.duration_hours;
                let mut active_names = active_team_names(teams, &shift.active_teams);
                active_names.insert(target.to_string());
                let forbidden_names: Vec<String> = match room_kind {
                    FacilityKind::TradePost => trade_pool
                        .entries
                        .iter()
                        .filter(|entry| !active_names.contains(&entry.name))
                        .map(|entry| entry.name.clone())
                        .collect(),
                    FacilityKind::Factory => manu_pool
                        .entries
                        .iter()
                        .filter(|entry| !active_names.contains(&entry.name))
                        .map(|entry| entry.name.clone())
                        .collect(),
                    _ => Vec::new(),
                };
                let refill = match room_kind {
                    FacilityKind::TradePost => refill_trade_room_with_core(
                        blueprint,
                        &room_id,
                        target,
                        &trade_pool,
                        table,
                        instances,
                        &shift_options,
                        Some(durin_plan),
                        &forbidden_names,
                        &mut shift.assignment,
                    ),
                    FacilityKind::Factory if target == "清流" => refill_manu_room_with_core(
                        blueprint,
                        &room_id,
                        target,
                        &manu_pool,
                        table,
                        instances,
                        &shift_options,
                        Some(durin_plan),
                        &forbidden_names,
                        &mut shift.assignment,
                    ),
                    _ => Err(Error::msg("unsupported Fiammetta continuous core facility")),
                };
                if refill.is_err() {
                    failed = true;
                    break;
                }
            }
            if failed
                || trial
                    .iter()
                    .any(|shift| !shift.assignment.operator_names().contains(target))
            {
                continue;
            }
            let action_room_id = trial[slot]
                .assignment
                .room_of_operator(target)
                .map(|room| room.room_id.clone());
            let Some(action_room_id) = action_room_id else {
                continue;
            };
            let mood_model = default_mood_model()?;
            if !fiammetta_events::try_accept_action(
                &mut trial,
                slot,
                target,
                &action_room_id,
                &mood_model,
                blueprint,
                instances,
            ) {
                continue;
            }
            if validate_fiammetta_continuous_targets(blueprint, profile, &trial).is_err() {
                continue;
            }
            candidate_shifts = trial;
            used_targets.insert(target.to_string());
            applied = true;
            selected = true;
            break;
        }
        if !selected {
            break;
        }
    }
    if !applied {
        return Ok(false);
    }
    rescore_standard_shifts(
        blueprint,
        instances,
        table,
        durin_plan,
        cycle_hours,
        &mut candidate_shifts,
    )?;
    shifts.clone_from_slice(&candidate_shifts);
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn apply_witch_group_fiammetta_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
    shift_binds: &[RuntimeShiftBind],
    shifts: &mut [TeamShiftResult],
) -> Result<bool> {
    if !operbox.owns("菲亚梅塔") || shifts.is_empty() {
        return Ok(false);
    }
    if !matches!(
        profile,
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12
    ) {
        return Ok(false);
    }
    let Some(room_id) = shifts.iter().find_map(|shift| {
        shift.assignment.iter_rooms().find_map(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|blueprint_room| {
                    blueprint_room.kind == FacilityKind::TradePost
                        && blueprint_room.level == 3
                        && room
                            .operators
                            .iter()
                            .any(|operator| operator.name == "巫恋")
                        && room
                            .operators
                            .iter()
                            .any(|operator| operator.name == "龙舌兰")
                })
                .then(|| room.room_id.clone())
        })
    }) else {
        return Ok(false);
    };
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let tailors = beta_tailor_names(&trade_pool, table);
    if tailors.len() < 2 {
        return Ok(false);
    }
    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mood_model = default_mood_model()?;
    let mut ranked_tailors = Vec::new();
    for tailor in tailors {
        let mut assignment = shifts[0].assignment.clone();
        let mut shift_options = options.clone();
        shift_options.shift_hours = shifts[0].duration_hours;
        if let Ok(score) = refill_witch_room_with_tailor(
            blueprint,
            &room_id,
            &tailor,
            &trade_pool,
            table,
            instances,
            &shift_options,
            Some(durin_plan),
            &mut assignment,
        ) {
            ranked_tailors.push((tailor, score));
        }
    }
    ranked_tailors.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if ranked_tailors.len() < 2 {
        return Ok(false);
    }
    let tailor_a = ranked_tailors[0].0.clone();
    let tailor_b = ranked_tailors[1].0.clone();
    let mut candidate_shifts = shifts.to_vec();
    let shift_count = candidate_shifts.len();
    for (index, shift) in candidate_shifts.iter_mut().enumerate() {
        let tailor = if shift_count == 2 || index + 1 == shift_count {
            &tailor_b
        } else {
            &tailor_a
        };
        let mut shift_options = options.clone();
        shift_options.shift_hours = shift.duration_hours;
        refill_witch_room_with_tailor(
            blueprint,
            &room_id,
            tailor,
            &trade_pool,
            table,
            instances,
            &shift_options,
            Some(durin_plan),
            &mut shift.assignment,
        )?;
    }
    let cycle_hours: f64 = candidate_shifts
        .iter()
        .map(|shift| shift.duration_hours)
        .sum();
    let required_actions = ["巫恋", "龙舌兰"];
    if required_actions.len() > candidate_shifts.len() {
        return Ok(false);
    }
    for (index, target) in required_actions.into_iter().enumerate() {
        if !fiammetta_events::try_accept_action(
            &mut candidate_shifts,
            index,
            target,
            &room_id,
            &mood_model,
            blueprint,
            instances,
        ) {
            return Ok(false);
        }
    }
    rescore_standard_shifts(
        blueprint,
        instances,
        table,
        durin_plan,
        cycle_hours,
        &mut candidate_shifts,
    )?;
    if validate_final_rotation_invariants(blueprint, &candidate_shifts, shift_binds, &[]).is_err() {
        return Ok(false);
    }
    shifts.clone_from_slice(&candidate_shifts);
    Ok(true)
}

// ── 深海链 S2 短班入口 ──

const ABYSSAL_CONTROL_BETA_BUFF: &str = "control_mp_aegir2[010]";
const ABYSSAL_FORBID_SAME_ROOM_MANU_BUFFS: [&str; 3] = [
    "manu_prod_spd&power[000]",
    "manu_prod_spd&power[010]",
    "manu_prod_spd&power[020]",
];
const TAG_ABYSSAL: &str = "cc.g.abyssal";
#[cfg(test)]
const DAIFEEN: &str = "戴菲恩";
const WARMUP_STICKY_TRADE_OPERATORS: [&str; 2] = ["巫恋", "龙舌兰"];
const WARMUP_MANU_BUFF_PREFIX: &str = "manu_prod_spd_addition[";
const WARMUP_TRADE_BUFF_PREFIX: &str = "trade_ord_wt&cost[";

#[cfg(test)]
fn room_has_all(room: &RoomAssignment, names: &[&str]) -> bool {
    names
        .iter()
        .all(|name| room.operators.iter().any(|op| op.name == *name))
}

fn rooms_compatible_for_swap(
    blueprint: &BaseBlueprint,
    a: &RoomId,
    b: &RoomId,
    kind: FacilityKind,
) -> bool {
    if a == b {
        return true;
    }
    let Some(room_a) = blueprint.room(a) else {
        return false;
    };
    let Some(room_b) = blueprint.room(b) else {
        return false;
    };
    room_a.kind == kind
        && room_b.kind == kind
        && room_a.level == room_b.level
        && room_a.product == room_b.product
        && room_a.product.is_some()
}

#[cfg(test)]
fn trade_room_containing_group(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    names: &[&str],
) -> Option<RoomId> {
    assignment.iter_rooms().find_map(|room| {
        let bp_room = blueprint.room(&room.room_id)?;
        (bp_room.kind == FacilityKind::TradePost && room_has_all(room, names))
            .then(|| room.room_id.clone())
    })
}

fn swap_room_assignments(assignment: &mut BaseAssignment, a: &RoomId, b: &RoomId) -> bool {
    assignment.swap_room_contents(a, b)
}

fn room_containing_operator(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    name: &str,
    kind: FacilityKind,
) -> Option<RoomId> {
    assignment.iter_rooms().find_map(|room| {
        let bp_room = blueprint.room(&room.room_id)?;
        (bp_room.kind == kind && room.operators.iter().any(|op| op.name == name))
            .then(|| room.room_id.clone())
    })
}

fn is_warmup_operator(
    name: &str,
    tier: PromotionTier,
    kind: FacilityKind,
    instances: &OperatorInstances,
) -> bool {
    match kind {
        FacilityKind::TradePost => {
            if WARMUP_STICKY_TRADE_OPERATORS.contains(&name) {
                return true;
            }
            instances
                .resolve_trade_buff_ids(name, tier)
                .iter()
                .any(|buff_id| buff_id.starts_with(WARMUP_TRADE_BUFF_PREFIX))
        }
        FacilityKind::Factory => instances
            .resolve_manufacture_buff_ids(name, tier)
            .iter()
            .any(|buff_id| buff_id.starts_with(WARMUP_MANU_BUFF_PREFIX)),
        _ => false,
    }
}

/// 暖机类技能跨短班连续上岗时保持同一房间；只能在同设施/同等级/同产物房间之间交换。
fn align_warmup_rooms(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    assignment: &mut BaseAssignment,
    sticky_rooms: &mut HashMap<String, RoomId>,
) {
    let ops: Vec<(String, FacilityKind)> = assignment
        .iter_rooms()
        .filter_map(|room| {
            blueprint
                .room(&room.room_id)
                .map(|bp_room| (bp_room.kind, room.operators.as_slice()))
        })
        .filter(|(kind, _)| matches!(kind, FacilityKind::TradePost | FacilityKind::Factory))
        .flat_map(|(kind, ops)| ops.iter().map(move |op| (op.name.clone(), op.tier(), kind)))
        .filter(|(name, tier, kind)| is_warmup_operator(name, *tier, *kind, instances))
        .map(|(name, _, kind)| (name, kind))
        .collect();

    for (name, kind) in ops {
        let Some(current_room) = room_containing_operator(assignment, blueprint, &name, kind)
        else {
            continue;
        };
        let Some(anchor_room) = sticky_rooms.get(&name).cloned() else {
            sticky_rooms.insert(name, current_room);
            continue;
        };
        if current_room == anchor_room {
            continue;
        }
        if rooms_compatible_for_swap(blueprint, &current_room, &anchor_room, kind)
            && swap_room_assignments(assignment, &current_room, &anchor_room)
        {
            clear_room_efficiency(assignment, &current_room);
            clear_room_efficiency(assignment, &anchor_room);
        }
    }
}

struct AbyssalCandidate {
    assignment: BaseAssignment,
    gamma_ops: Vec<String>,
    control_producer: String,
}

/// 构造 S2 深海短班候选：至少三名深海猎手视作等价生产锚，按房间人数计数枚举。
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
    required_manu_by_room: &'a HashMap<RoomId, Vec<String>>,
    admitted_control_producer: Option<&'a str>,
}

fn abyssal_control_producer(
    operbox: &OperBox,
    instances: &OperatorInstances,
    used: &HashSet<String>,
) -> Option<String> {
    operbox
        .entries
        .iter()
        .filter(|entry| entry.own && !used.contains(&entry.name))
        .find(|entry| {
            let tier =
                PromotionTier::from_elite_rarity_level(entry.elite, entry.rarity, entry.level);
            instances
                .resolve_control_buff_ids(&entry.name, tier)
                .iter()
                .any(|buff_id| buff_id == ABYSSAL_CONTROL_BETA_BUFF)
        })
        .map(|entry| entry.name.clone())
}

fn owned_abyssal_hunters(
    operbox: &OperBox,
    instances: &OperatorInstances,
    used_ab: &HashSet<String>,
) -> Vec<String> {
    let mut hunters: Vec<_> = operbox
        .entries
        .iter()
        .filter(|entry| entry.own && !used_ab.contains(&entry.name))
        .filter(|entry| {
            let tier =
                PromotionTier::from_elite_rarity_level(entry.elite, entry.rarity, entry.level);
            instances
                .tags_for(&entry.name, tier)
                .iter()
                .any(|tag| tag == TAG_ABYSSAL)
                && !instances
                    .resolve_control_buff_ids(&entry.name, tier)
                    .iter()
                    .any(|buff_id| buff_id == ABYSSAL_CONTROL_BETA_BUFF)
        })
        .map(|entry| entry.name.clone())
        .collect();
    hunters.sort();
    hunters.dedup();
    hunters
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
    let Some(control_producer) = ctx.admitted_control_producer.map(str::to_string) else {
        return Vec::new();
    };

    let hunters = owned_abyssal_hunters(ctx.operbox, ctx.instances, ctx.used_ab);
    if hunters.len() < 3 {
        return Vec::new();
    }

    if ctx.mutable_manu_rooms.is_empty() {
        return Vec::new();
    }

    let mut base = ctx.shared.clone();
    merge_rooms(&mut base, ctx.beta);
    merge_rooms(&mut base, ctx.gamma_h1);

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
            let room_id = &ctx.mutable_manu_rooms[room_idx];
            let required = ctx
                .required_manu_by_room
                .get(room_id)
                .cloned()
                .unwrap_or_default();
            if count == 0 && required.is_empty() {
                continue;
            }
            let Some(room) = ctx.blueprint.room(room_id) else {
                ok = false;
                break;
            };
            let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
                ok = false;
                break;
            };
            let mut anchors: Vec<ManuPoolEntry> = required
                .iter()
                .filter_map(|name| ctx.manu_pool.entry(name).cloned())
                .collect();
            anchors.extend(
                hunter_entries[next_hunter..next_hunter + count]
                    .iter()
                    .cloned(),
            );
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
            .iter_rooms()
            .filter(|room| ctx.mutable_manu_rooms.contains(&room.room_id))
            .flat_map(|room| room.operators.iter())
            .map(|op| op.name.clone())
            .collect();
        gamma_ops.sort();
        gamma_ops.dedup();
        out.push(AbyssalCandidate {
            assignment: candidate,
            gamma_ops,
            control_producer: control_producer.clone(),
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

/// 从编排计划中提取体系绑定的实际中枢干员名。
fn system_control_operators(plan: &AssignmentPlan, peak_ctrl: &HashSet<String>) -> HashSet<String> {
    plan.anchors
        .iter()
        .filter(|anchor| {
            anchor.facility == FacilityKind::ControlCenter && peak_ctrl.contains(&anchor.operator)
        })
        .map(|anchor| anchor.operator.clone())
        .collect()
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
        if h1.contains(rid) {
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
    let peak_set: HashSet<String> = peak_ctrl.iter().cloned().collect();
    let system_names = system_control_operators(plan, &peak_set);
    let mut team_map: HashMap<TeamLabel, Vec<String>> = HashMap::new();

    // Step 1: 体系实际落位的中枢干员归属到对应队伍。
    for anchor in plan
        .anchors
        .iter()
        .filter(|anchor| anchor.facility == FacilityKind::ControlCenter)
    {
        let team = plan
            .activated
            .iter()
            .find(|system| system.system_id == anchor.system_id)
            .and_then(|system| system_control_team(system, h1))
            .unwrap_or(TeamLabel::Alpha);
        if peak_set.contains(&anchor.operator) {
            team_map
                .entry(team)
                .or_default()
                .push(anchor.operator.clone());
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

fn automatically_excluded_control_names(
    pool: &crate::pool::ControlPool,
    table: &SkillTable,
) -> Result<HashSet<String>> {
    pool.entries
        .iter()
        .map(|entry| {
            crate::response_dependency::control_buff_is_automatically_excluded(
                table,
                entry.buff_ids.iter().map(String::as_str),
            )
            .map(|excluded| excluded.then(|| entry.name.clone()))
        })
        .collect::<Result<Vec<_>>>()
        .map(|names| names.into_iter().flatten().collect())
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

fn assert_recycle_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
) -> Result<()> {
    let rooms: Vec<_> = blueprint.rooms_of(FacilityKind::Recycle);
    if rooms.is_empty() {
        return Ok(());
    }
    for shift in &report.shifts {
        let plan = report
            .assignment_plans
            .get(shift.plan_index)
            .ok_or_else(|| {
                Error::msg("assert-invariants recycle_admission failed: missing Plan")
            })?;
        for room in &rooms {
            let operators = shift.assignment.operators_in(&room.id);
            if operators.len() > room.operator_capacity() {
                return Err(Error::msg(format!(
                    "assert-invariants recycle_capacity failed in shift {} room {}",
                    shift.index + 1,
                    room.id.0
                )));
            }
            for operator in operators {
                if operbox.progress_of(&operator.name).is_none_or(|progress| {
                    AssignedOperator::from_progress(&operator.name, progress).tier()
                        != operator.tier()
                        || progress.elite != operator.elite
                        || progress.level != operator.level
                        || progress.rarity != operator.rarity
                }) || plan.excluded_operators.contains(&operator.name)
                {
                    return Err(Error::msg(format!(
                        "assert-invariants recycle_admission failed in shift {} for {}",
                        shift.index + 1,
                        operator.name
                    )));
                }
                let teams: Vec<_> = report
                    .teams
                    .iter()
                    .filter(|team| team.operators.contains(&operator.name))
                    .map(|team| team.label)
                    .collect();
                if teams.len() != 1
                    || !shift.active_teams.contains(&teams[0])
                    || !report
                        .shifts
                        .iter()
                        .any(|other| !other.active_teams.contains(&teams[0]))
                {
                    return Err(Error::msg(format!(
                        "assert-invariants recycle_team_membership failed in shift {} for {}",
                        shift.index + 1,
                        operator.name
                    )));
                }
            }
            let expected =
                crate::recycle::evaluate_recycle(room.level, operators, instances, table)?;
            let lines: Vec<_> = shift
                .efficiencies
                .room_lines
                .iter()
                .filter(|line| line.room_id == room.id.0)
                .collect();
            if lines.len() != 1
                || lines[0].operator_count != operators.len()
                || lines[0].base_efficiency != expected.base_efficiency
                || lines[0].operator_efficiency != expected.operator_efficiency
                || lines[0].total_efficiency != expected.total_efficiency
            {
                return Err(Error::msg(format!(
                    "assert-invariants recycle_efficiency failed in shift {} room {}",
                    shift.index + 1,
                    room.id.0
                )));
            }
        }
    }
    Ok(())
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
/// - 宿舍共享；普通办公室成员按每班效率独立选择，真实绑定成员随消费方体系同上同下。
/// - 中枢按 αβγ 队伍轮休重分配：每班只用活跃两队候选，体系中枢先 pin，再补满 5 人。
pub fn schedule_team_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    schedule_timed_rotation(
        blueprint,
        operbox,
        instances,
        table,
        options,
        TimedRotationProfile::default(),
    )
}

pub fn assert_final_rotation_invariants(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
) -> Result<()> {
    if report.profile == TimedRotationProfile::FiammettaWitch12_12 {
        return Err(Error::msg(
            "assert-invariants unsupported rotation profile fiammetta_witch_12_12: this product mode was removed",
        ));
    }
    assert_final_rotation_invariants_with_mood(blueprint, operbox, instances, table, report, 24.0)
}

fn assert_final_rotation_invariants_with_mood(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    initial_mood: f64,
) -> Result<()> {
    validate_final_bind_cycle(blueprint, report)
        .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    validate_fiammetta_action_policy(report.profile, &report.shifts)
        .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    let mood_model = default_mood_model()?;
    fiammetta_events::validate_actions(&report.shifts, &mood_model, blueprint, instances)
        .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    assert_final_control_rotation(blueprint, operbox, instances, report)?;
    super::control_rotation::assert_control_mood_mapping(blueprint, instances, table, report)?;
    if report.profile == TimedRotationProfile::MainBackup24_24 {
        assert_main_backup_24_24_final_state(blueprint, instances, table, report, initial_mood)?;
    }
    crate::layout::assert_support_combination_policy(&report.assignment_plans, &report.shifts)?;
    assert_recycle_rotation(blueprint, operbox, instances, table, report)?;
    for shift in &report.shifts {
        let plan = report
            .assignment_plans
            .get(shift.plan_index)
            .ok_or_else(|| {
                Error::msg("assert-invariants ordinary_fallback_admission failed: missing Plan")
            })?;
        let mut names = HashSet::new();
        for room in shift.assignment.iter_rooms() {
            for operator in &room.operators {
                if !names.insert(operator.name.clone()) {
                    return Err(Error::msg(format!(
                        "assert-invariants ordinary_fallback_admission failed: duplicate {} in shift {}",
                        operator.name, shift.index + 1,
                    )));
                }
            }
            if !blueprint
                .room(&room.room_id)
                .is_some_and(|room| ordinary_workplace(room.kind))
            {
                continue;
            }
            for operator in &room.operators {
                let progress = operbox.progress_of(&operator.name);
                if progress.is_none_or(|progress| {
                    progress.elite != operator.elite
                        || (operator.level != 0 && progress.level != operator.level)
                        || (operator.rarity != 0 && progress.rarity != operator.rarity)
                }) || plan.excluded_operators.contains(&operator.name)
                {
                    return Err(Error::msg(format!(
                        "assert-invariants ordinary_fallback_admission failed: invalid workplace member {} in shift {}",
                        operator.name, shift.index + 1,
                    )));
                }
            }
        }
    }
    let mut completed = report.clone();
    fill_rotation_vacancies(
        blueprint,
        operbox,
        instances,
        table,
        initial_mood,
        &mut completed,
    )
    .map_err(|error| {
        Error::msg(format!(
            "assert-invariants ordinary_fallback_complete failed: {error}"
        ))
    })?;
    for (before, after) in report.shifts.iter().zip(&completed.shifts) {
        for room in blueprint
            .rooms
            .iter()
            .filter(|room| ordinary_workplace(room.kind))
        {
            if before.assignment.operators_in(&room.id) != after.assignment.operators_in(&room.id) {
                return Err(Error::msg(format!(
                    "assert-invariants ordinary_fallback_complete failed in shift {} room {}: a legal vacancy remained after completion",
                    before.index + 1, room.id.0,
                )));
            }
        }
    }
    let cycle_hours: f64 = report.shifts.iter().map(|shift| shift.duration_hours).sum();
    if cycle_hours <= 0.0 {
        return Err(Error::msg(
            "assert-invariants rotation_cycle_weighting failed: empty or zero-duration cycle",
        ));
    }
    let expected_rest_warnings = rotation_cycle_rest_warnings(blueprint, report);
    let actual_rest_warnings: Vec<_> = report
        .warnings
        .iter()
        .filter(|warning| warning.starts_with(CYCLE_REST_WARNING_PREFIX))
        .collect();
    if actual_rest_warnings != expected_rest_warnings.iter().collect::<Vec<_>>() {
        return Err(Error::msg(format!(
            "assert-invariants rotation_cycle_rest_diagnostics failed: report={actual_rest_warnings:?} expected={expected_rest_warnings:?}"
        )));
    }
    let mut replay_shifts = report.shifts.clone();
    let expected_dorm_warnings =
        dorm_finalizer::finalize_dorms(blueprint, operbox, &mut replay_shifts)?
            .into_iter()
            .map(dorm_finalizer::render_warning)
            .collect::<Vec<_>>();
    let mut expected_warnings = rotation_shortfall_warnings(blueprint, &report.shifts);
    expected_warnings.extend(expected_dorm_warnings);
    expected_warnings.extend(expected_rest_warnings);
    let actual_warnings: Vec<_> = report
        .warnings
        .iter()
        .filter(|warning| !warning.starts_with(FINAL_INVARIANT_WARNING_PREFIX))
        .collect();
    if actual_warnings != expected_warnings.iter().collect::<Vec<_>>() {
        return Err(Error::msg(format!(
            "assert-invariants rotation_warnings_complete failed: report={actual_warnings:?} expected={expected_warnings:?}"
        )));
    }
    let mut expected = DailyTotals::default();
    for shift in &report.shifts {
        let scores = &shift.efficiencies;
        let trade = scores.weighted_trade_for_cycle(shift.duration_hours, cycle_hours);
        let manufacture = scores.weighted_manufacture_for_cycle(shift.duration_hours, cycle_hours);
        let power = scores.weighted_power_for_cycle(shift.duration_hours, cycle_hours);
        if (
            shift.weighted_trade,
            shift.weighted_manufacture,
            shift.weighted_power,
        ) != (trade, manufacture, power)
        {
            return Err(Error::msg(format!(
                "assert-invariants rotation_cycle_weighting failed in shift {}",
                shift.index + 1
            )));
        }
        expected.trade += trade;
        expected.manufacture += manufacture;
        expected.power += power;
    }
    if (
        report.daily.trade,
        report.daily.manufacture,
        report.daily.power,
    ) != (expected.trade, expected.manufacture, expected.power)
    {
        return Err(Error::msg(
            "assert-invariants rotation_cycle_weighting failed in daily totals",
        ));
    }
    let unresolved_trade_refs = table.validate_operator_refs(instances);
    if !unresolved_trade_refs.is_empty() {
        return Err(Error::msg(format!(
            "assert-invariants runtime_trade_skill_refs_resolve failed:\n{}",
            unresolved_trade_refs.join("\n")
        )));
    }
    let pool = build_control_pool_with_fillers(operbox, instances, table)?;
    let mood = default_mood_model()?;
    let peak_gold_flow_selected = report
        .peak_plan
        .selected_rules
        .iter()
        .find(|rule| rule.rule_id == "pozyomka_durin");
    let peak_has_split = peak_gold_flow_selected.is_some_and(|rule| {
        matches!(
            rule.alternative_id.as_str(),
            "two_trade_gold_flow" | "three_trade_gold_flow"
        )
    });
    let mut split_witness = false;
    for shift in &report.shifts {
        let plan = report
            .assignment_plans
            .get(shift.plan_index)
            .ok_or_else(|| {
                Error::msg("assert-invariants gold_flow_core_real_lines failed: missing Plan")
            })?;
        let gold_flow_selected = plan
            .selected_rules
            .iter()
            .find(|rule| rule.rule_id == "pozyomka_durin");
        let gold_flow_split =
            gold_flow_selected.and_then(|rule| match rule.alternative_id.as_str() {
                "two_trade_gold_flow" => Some(2),
                "three_trade_gold_flow" => Some(3),
                _ => None,
            });
        if gold_flow_selected.is_some() && blueprint.gold_manu_line_count() < 4 {
            return Err(Error::msg(format!(
                "assert-invariants gold_flow_core_real_lines failed in shift {}: core selected with {} real gold lines (need 4)",
                shift.index + 1,
                blueprint.gold_manu_line_count(),
            )));
        }
        for room in blueprint.rooms_of(FacilityKind::Factory) {
            let members = shift.assignment.operators_in(&room.id);
            if room.level == 3 && members.len() > room.operator_capacity() {
                return Err(Error::msg(format!(
                    "assert-invariants manufacture_capacity failed in shift {} room {}: {} exceeds {}",
                    shift.index + 1,
                    room.id.0,
                    members.len(),
                    room.operator_capacity()
                )));
            }
            let operators: Vec<_> = members
                .iter()
                .map(|member| crate::manufacture::ManuOperator {
                    name: member.name.clone(),
                    elite: member.elite,
                    buff_ids: instances.resolve_manufacture_buff_ids(&member.name, member.tier()),
                    tags: instances.tags_for(&member.name, member.tier()),
                })
                .collect();
            if !crate::search::manufacture_peers_compatible(&operators, table) {
                return Err(Error::msg(format!(
                    "assert-invariants manufacture_peer_group failed in shift {} room {}: {:?}",
                    shift.index + 1,
                    room.id.0,
                    members
                        .iter()
                        .map(|member| member.name.as_str())
                        .collect::<Vec<_>>()
                )));
            }
        }
        validate_office_assignment_disjoint(blueprint, shift)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
        validate_office_shift_eta(blueprint, instances, shift)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
        let mut expected_notes = shift.assignment.clone();
        annotate_assignment(blueprint, &mut expected_notes);
        let actual_note_rows: Vec<_> = shift
            .assignment
            .iter_rooms()
            .filter(|room| !room.notes.is_empty())
            .map(|room| (&room.room_id, &room.notes))
            .collect();
        let expected_note_rows: Vec<_> = expected_notes
            .iter_rooms()
            .filter(|room| !room.notes.is_empty())
            .map(|room| (&room.room_id, &room.notes))
            .collect();
        if actual_note_rows != expected_note_rows {
            return Err(Error::msg(format!(
                "assert-invariants room_notes_owner failed in shift {}",
                shift.index + 1
            )));
        }
        let violations = shift.assignment.structural_violations(blueprint);
        if !violations.is_empty() {
            return Err(Error::msg(format!(
                "assert-invariants structural failed in shift {}: {}",
                shift.index + 1,
                violations.join("; ")
            )));
        }
        let durin_count = assert_gold_flow_final_state(blueprint, shift, instances, table)?;
        if let Some(room_count) = gold_flow_split {
            if let Some([kirara_room, pozyomka_room, tuye_room]) =
                gold_flow_core_rooms_for_shift(blueprint, report, shift)?
            {
                for (name, room) in [
                    ("绮良", &kirara_room),
                    ("鸿雪", &pozyomka_room),
                    ("图耶", &tuye_room),
                ] {
                    if !shift
                        .assignment
                        .operators_in(room)
                        .iter()
                        .any(|operator| operator.name == name && operator.elite >= 2)
                    {
                        return Err(Error::msg(format!(
                            "assert-invariants gold_flow_split_admission failed in shift {}: core operator {} is not E2 in a gold TradePost",
                            shift.index + 1,
                            name
                        )));
                    }
                }
                if durin_count != 4 {
                    return Err(Error::msg(format!(
                        "assert-invariants gold_flow_group_durin_presence failed in shift {}: got {}",
                        shift.index + 1,
                        durin_count
                    )));
                }
                let valid = if room_count == 2 {
                    kirara_room != pozyomka_room
                        && kirara_room != tuye_room
                        && pozyomka_room == tuye_room
                } else {
                    kirara_room != pozyomka_room
                        && kirara_room != tuye_room
                        && pozyomka_room != tuye_room
                };
                if !valid {
                    return Err(Error::msg(format!(
                        "assert-invariants gold_flow_split_admission failed in shift {}",
                        shift.index + 1
                    )));
                }
                split_witness = true;
                validate_gold_flow_recommendation_anchors(
                    plan,
                    &kirara_room,
                    &pozyomka_room,
                    &tuye_room,
                    shift.index,
                )?;
            }
        }
        if shift_works_in_trade(blueprint, &shift.assignment, SPHINX_NAME)
            && !operator_is_in_normal_base(blueprint, &shift.assignment, ULPIANUS_NAME)
        {
            return Err(Error::msg(format!(
                "assert-invariants sphinx_ulpianus_presence failed in shift {}",
                shift.index + 1
            )));
        }
        if shift
            .efficiencies
            .room_lines
            .iter()
            .filter(|line| {
                blueprint
                    .room(&RoomId::new(line.room_id.clone()))
                    .is_some_and(|room| room.kind == FacilityKind::PowerPlant)
            })
            .any(|line| line.base_efficiency != Efficiency::ZERO)
        {
            return Err(Error::msg(format!(
                "assert-invariants power_station_base_zero failed in shift {}",
                shift.index + 1
            )));
        }
        let power_increments = shift
            .efficiencies
            .room_lines
            .iter()
            .filter(|line| {
                blueprint
                    .room(&RoomId::new(line.room_id.clone()))
                    .is_some_and(|room| room.kind == FacilityKind::PowerPlant)
            })
            .map(|line| line.power_efficiency);
        let expected_power = crate::power::aggregate_power_efficiency(power_increments);
        if shift.efficiencies.power_efficiency != expected_power {
            return Err(Error::msg(format!(
                "assert-invariants power_shared_base_once failed in shift {}: expected {}, got {}",
                shift.index + 1,
                expected_power,
                shift.efficiencies.power_efficiency,
            )));
        }
        let Some(action) = &shift.fiammetta else {
            continue;
        };
        let fiammetta_dorms: Vec<_> = blueprint
            .rooms_of(FacilityKind::Dormitory)
            .into_iter()
            .filter(|room| {
                shift
                    .assignment
                    .operators_in(&room.id)
                    .first()
                    .is_some_and(|operator| operator.name == FIAMMETTA_NAME)
            })
            .collect();
        if fiammetta_dorms.len() != 1 {
            return Err(Error::msg(format!(
                "assert-invariants fiammetta_dorm_first failed in shift {}",
                shift.index + 1
            )));
        }
        let target_in_room = shift
            .assignment
            .operators_in(&action.room_id)
            .iter()
            .any(|operator| operator.name == action.target);
        if !target_in_room {
            return Err(Error::msg(format!(
                "assert-invariants fiammetta_target_present failed in shift {}: target {} missing from room {}",
                shift.index + 1,
                action.target,
                action.room_id.0
            )));
        }
        let valid_room = blueprint.room(&action.room_id).is_some_and(|room| {
            matches!(
                room.kind,
                FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
            )
        });
        if !valid_room {
            return Err(Error::msg(format!(
                "assert-invariants fiammetta_action_room failed in shift {}: room {} is not production",
                shift.index + 1,
                action.room_id.0
            )));
        }
    }
    validate_fiammetta_continuous_targets(blueprint, report.profile, &report.shifts)
        .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    validate_cycle_mood(blueprint, instances, report, initial_mood)
        .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    for shift in &report.shifts {
        for room in blueprint.rooms_of(FacilityKind::MeetingRoom) {
            let members = shift.assignment.operators_in(&room.id);
            let suzuran = members.iter().any(|member| member.name == "铃兰");
            let winter = members
                .iter()
                .any(|member| member.name == "忍冬" && member.elite >= 2);
            let required = report
                .assignment_plans
                .get(shift.plan_index)
                .is_some_and(|plan| {
                    plan.anchors.iter().any(|anchor| {
                        anchor.operator == "铃兰" && anchor.facility == FacilityKind::MeetingRoom
                    })
                });
            if suzuran && !winter && !required {
                return Err(Error::msg(format!(
                    "assert-invariants meeting_suzuran_partner failed in shift {} room {}",
                    shift.index + 1,
                    room.id.0,
                )));
            }
        }
        let control = shift.assignment.control_operators();
        let control_capacity = blueprint
            .rooms_of(FacilityKind::ControlCenter)
            .first()
            .map(|room| room.operator_capacity())
            .unwrap_or(0);
        if control.len() > control_capacity {
            return Err(Error::msg(format!(
                "assert-invariants control_capacity failed in shift {}: {} exceeds {}",
                shift.index + 1,
                control.len(),
                control_capacity,
            )));
        }
        let operators = control
            .iter()
            .map(|op| {
                pool.entry(&op.name)
                    .map(|entry| entry.to_control_operator())
                    .ok_or_else(|| {
                        Error::msg(format!(
                            "assert-invariants missing control operator {}",
                            op.name
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let layout = crate::layout::resolve_control_input_layout(
            blueprint,
            &shift.assignment,
            instances,
            table,
            24.0,
            Some(operbox.durin_dorm_planning_count(instances)),
        )?;
        let result = solve_control(
            &ControlRoomInput {
                operators,
                mood: 24.0,
                layout: layout.clone(),
            },
            table,
        );
        if result.effect_ledger.has_conflicting_effect_sources() {
            return Err(Error::msg(format!(
                "assert-invariants control_effect_source_unique failed in shift {}",
                shift.index + 1
            )));
        }
        let final_layout = resolve_base(
            blueprint,
            &shift.assignment,
            Some(instances),
            Some(table),
            24.0,
            Some(operbox.durin_dorm_planning_count(instances)),
        )?
        .layout_snapshot();
        let cuisine_sources = crate::cross_facility::collect_global_atoms(
            blueprint,
            &shift.assignment,
            instances,
            table,
            &final_layout,
        )
        .into_iter()
        .filter(|entry| {
            matches!(
                &entry.atom.action,
                crate::types::Action::StateProduce { key, .. }
                    if GlobalResourceKey::parse(key) == Some(GlobalResourceKey::MonsterCuisine)
            )
        })
        .collect::<Vec<_>>();
        if cuisine_sources.iter().any(|entry| {
            entry.owner_name != "森西"
                || entry.owner_elite < 2
                || entry.room_kind != FacilityKind::Dormitory
                || entry.buff_id != "dorm_rec_bd_dungeon[000]"
                || entry.atom.phase != crate::types::Phase::StateWrite
                || entry.atom.selector != Some(crate::types::Selector::FacilityLevel)
                || !matches!(
                    &entry.atom.action,
                    crate::types::Action::StateProduce { amount, .. } if *amount == 1.0
                )
        }) {
            return Err(Error::msg(format!(
                "assert-invariants monster_cuisine_assignment_source failed in shift {}: invalid producer",
                shift.index + 1,
            )));
        }
        let expected_cuisine: f64 = blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::Dormitory)
            .filter(|room| {
                shift
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .any(|op| op.name == "森西" && op.elite >= 2)
            })
            .map(|room| f64::from(room.level))
            .sum();
        let actual_cuisine = final_layout.global.get(GlobalResourceKey::MonsterCuisine);
        if (actual_cuisine - expected_cuisine).abs() > 1e-9 {
            return Err(Error::msg(format!(
                "assert-invariants monster_cuisine_assignment_source failed in shift {}: expected {}, got {}",
                shift.index + 1,
                expected_cuisine,
                actual_cuisine,
            )));
        }
        for room in shift.assignment.iter_rooms() {
            if blueprint
                .room(&room.room_id)
                .is_none_or(|spec| spec.kind != FacilityKind::TradePost)
            {
                continue;
            }
            if room
                .operators
                .iter()
                .any(|operator| operator.name == "黑键")
                && room
                    .operators
                    .iter()
                    .any(|operator| operator.name == "巫恋")
            {
                return Err(Error::msg(format!(
                    "assert-invariants trade_blackkey_witch_disjoint failed in shift {} room {}",
                    shift.index + 1,
                    room.room_id.0,
                )));
            }
            let has_witch = room
                .operators
                .iter()
                .any(|operator| operator.name == "巫恋");
            let has_tequila = room
                .operators
                .iter()
                .any(|operator| operator.name == "龙舌兰");
            if has_witch && has_tequila {
                let level = blueprint
                    .room(&room.room_id)
                    .map(|spec| spec.level)
                    .unwrap_or_default();
                let valid_room = blueprint.room(&room.room_id).is_some_and(|spec| {
                    spec.level == 3
                        && spec.product
                            == Some(RoomProduct::Trade {
                                order: TradeOrderKind::Gold,
                            })
                });
                let operators = room
                    .operators
                    .iter()
                    .map(|operator| TradeOperator {
                        name: operator.name.clone(),
                        elite: operator.elite,
                        buff_ids: instances.resolve_trade_buff_ids(&operator.name, operator.tier()),
                        tags: instances.tags_for(&operator.name, operator.tier()),
                        compiled_atoms: Arc::from([]),
                    })
                    .collect::<Vec<_>>();
                let valid_rule = match_witch_group_shortcut(&operators, table, level).is_some();
                if !valid_room || !valid_rule {
                    return Err(Error::msg(format!(
                        "assert-invariants witch_tequila_admission failed in shift {} room {}",
                        shift.index + 1,
                        room.room_id.0,
                    )));
                }
            }
            let has_market_variant = room.operators.iter().any(|operator| {
                instances
                    .resolve_trade_buff_ids(&operator.name, operator.tier())
                    .iter()
                    .any(|buff_id| buff_id == "trade_ord_limit_count[000]")
            });
            let has_karlan_consumer = room.operators.iter().any(|operator| {
                instances
                    .tags_for(&operator.name, operator.tier())
                    .iter()
                    .any(|tag| tag == "cc.g.karlan")
            });
            if has_market_variant
                && has_karlan_consumer
                && final_layout.global_inject.karlan_precision().is_none()
            {
                return Err(Error::msg(format!(
                    "assert-invariants karlan_trade_requires_control_source failed in shift {} room {}",
                    shift.index + 1,
                    room.room_id.0,
                )));
            }
        }
        let control_values = [
            result.inject.trade_eff_pct(),
            result.office_hire_spd_pct,
            result.meeting_speed_pct,
            final_layout.global_inject.trade_eff_pct(),
            final_layout.control_office_hire_spd_pct,
            final_layout.control_meeting_speed_pct,
        ];
        if control_values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
            || (final_layout.control_office_hire_spd_pct - result.office_hire_spd_pct).abs() > 1e-9
            || (final_layout.control_meeting_speed_pct - result.meeting_speed_pct).abs() > 1e-9
        {
            return Err(Error::msg(format!(
                "assert-invariants control_effect_projection_finite failed in shift {}: pre=[trade {}, office {}, meeting {}] final=[trade {}, office {}, meeting {}]",
                shift.index + 1,
                result.inject.trade_eff_pct(), result.office_hire_spd_pct, result.meeting_speed_pct,
                final_layout.global_inject.trade_eff_pct(), final_layout.control_office_hire_spd_pct,
                final_layout.control_meeting_speed_pct,
            )));
        }
        let control_ops = control
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect::<Vec<_>>();
        let active_global_recovery_owners = mood
            .control_global_recovery
            .iter()
            .filter(|entry| {
                control_ops
                    .iter()
                    .any(|(name, elite)| name == &entry.name && *elite >= entry.elite)
            })
            .count();
        if active_global_recovery_owners > 1 {
            return Err(Error::msg(format!(
                "assert-invariants control_global_recovery_owner_unique failed in shift {}",
                shift.index + 1
            )));
        }
        let recovery = mood.control_global_recovery_summary(&control_ops);
        let smiley = mood.control_smiley_recovery_value(&control_ops);
        let control_smiley =
            mood.control_smiley_recovery_for_facility(FacilityKind::ControlCenter, &control_ops);
        let spread_smiley =
            mood.control_smiley_recovery_for_facility(FacilityKind::TradePost, &control_ops);
        if !recovery.effective_value.is_finite()
            || !recovery.faction_recovery_value.is_finite()
            || recovery.faction_recovery_value < 0.0
            || !smiley.is_finite()
            || smiley < 0.0
            || !control_smiley.is_finite()
            || (control_smiley - smiley).abs() > 1e-9
            || !spread_smiley.is_finite()
            || spread_smiley < 0.0
        {
            return Err(Error::msg(format!(
                "assert-invariants control_mood_recovery_consistent failed in shift {}",
                shift.index + 1
            )));
        }
    }
    if matches!(
        report.profile,
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12
    ) {
        let full_rest = report.profile == TimedRotationProfile::Abc12_12_12;
        for shift in &report.shifts {
            let resting: HashSet<&str> = report
                .teams
                .iter()
                .find(|team| team.label == shift.resting_team)
                .into_iter()
                .flat_map(|team| team.operators.iter().map(String::as_str))
                .collect();
            for room in blueprint.rooms_of(FacilityKind::Office) {
                if let Some(operator) = shift
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .find(|operator| resting.contains(operator.name.as_str()))
                {
                    return Err(Error::msg(format!(
                        "assert-invariants abc_resting_team_office_disjoint failed in shift {} for {}",
                        shift.index + 1,
                        operator.name
                    )));
                }
            }
        }
        let mut mood_state: HashMap<String, f64> = operbox
            .roster()
            .names()
            .map(|name| (name.to_string(), initial_mood.min(mood.mood_cap)))
            .collect();
        let cycles = 1;
        for cycle in 0..cycles {
            for (index, shift) in report.shifts.iter().enumerate() {
                let next_cycle_first = (!full_rest && index + 1 == report.shifts.len())
                    .then(|| {
                        report
                            .shifts
                            .first()
                            .map(|first| (&first.assignment, first.duration_hours))
                    })
                    .flatten();
                if let Some(name) = nonproduction_eta_failure(
                    &mood,
                    blueprint,
                    instances,
                    &shift.assignment,
                    &mood_state,
                    shift.duration_hours,
                    next_cycle_first,
                    None,
                ) {
                    return Err(Error::msg(format!(
                        "assert-invariants nonproduction_rotation_eta failed in cycle {} shift {} for {}",
                        cycle + 1, shift.index + 1, name
                    )));
                }
                advance_rotation_mood(
                    &mood,
                    blueprint,
                    instances,
                    &shift.assignment,
                    shift.duration_hours,
                    full_rest,
                    &mut mood_state,
                );
            }
        }
    }
    if peak_has_split && !split_witness {
        return Err(Error::msg(
            "assert-invariants gold_flow_split_admission failed: selected group has no working shift",
        ));
    }
    Ok(())
}

fn validate_gold_flow_recommendation_anchors(
    plan: &AssignmentPlan,
    kirara_room: &RoomId,
    pozyomka_room: &RoomId,
    tuye_room: &RoomId,
    shift_index: usize,
) -> Result<()> {
    for anchor in plan.anchors.iter().filter(|anchor| {
        anchor.system_id == "pozyomka_durin" && anchor.optional_trade_role.is_some()
    }) {
        let expected_room = match anchor.optional_trade_role.as_deref() {
            Some("witch" | "tequila") => kirara_room,
            Some("docus") => tuye_room,
            Some("closure") => pozyomka_room,
            _ => continue,
        };
        if anchor.room_id.as_ref() != Some(expected_room) {
            return Err(Error::msg(format!(
                "assert-invariants gold_flow_recommendation_geometry failed in shift {}: role {} operator {} is not co-located with its core room",
                shift_index + 1,
                anchor.optional_trade_role.as_deref().unwrap_or("unknown"),
                anchor.operator
            )));
        }
    }
    Ok(())
}

fn gold_flow_core_rooms_for_shift(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
    shift: &TeamShiftResult,
) -> Result<Option<[RoomId; 3]>> {
    let core_names = ["绮良", "鸿雪", "图耶"];
    let working = core_names.map(|name| operator_has_non_dorm_assignment(blueprint, shift, name));

    if matches!(
        report.profile,
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12
    ) {
        let has_core_bind = report.runtime_shift_binds.iter().any(|bind| {
            core_names
                .iter()
                .all(|name| bind.operators.iter().any(|operator| operator == name))
                && bind.on_shifts == 2
                && bind.off_shifts == 1
        });
        if !has_core_bind {
            return Err(Error::msg(format!(
                "assert-invariants gold_flow_shift_bind failed in shift {}: selected core has no synchronized two-on/one-off bind",
                shift.index + 1
            )));
        }
        let core_team = team_of_operator(report, core_names[0]).ok_or_else(|| {
            Error::msg(format!(
                "assert-invariants gold_flow_shift_bind failed in shift {}: missing core team",
                shift.index + 1
            ))
        })?;
        if core_names
            .iter()
            .skip(1)
            .any(|name| team_of_operator(report, name) != Some(core_team))
        {
            return Err(Error::msg(format!(
                "assert-invariants gold_flow_shift_bind failed in shift {}: core members are assigned to different teams",
                shift.index + 1
            )));
        }
        if shift.resting_team == core_team {
            if working.into_iter().any(|is_working| is_working) {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_shift_bind failed in shift {}: a core member works while the bound team rests",
                    shift.index + 1
                )));
            }
            return Ok(None);
        }
    } else if working.iter().all(|is_working| !is_working) {
        return Ok(None);
    }

    if working.iter().any(|is_working| !is_working) {
        return Err(Error::msg(format!(
            "assert-invariants gold_flow_shift_bind failed in shift {}: core members are split between work and rest",
            shift.index + 1
        )));
    }

    let [Some(kirara), Some(pozyomka), Some(tuye)] = core_names.map(|name| {
        blueprint
            .rooms_of(FacilityKind::TradePost)
            .into_iter()
            .filter(|room| {
                matches!(
                    room.product,
                    Some(RoomProduct::Trade {
                        order: TradeOrderKind::Gold
                    })
                )
            })
            .find(|room| {
                shift
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .any(|operator| operator.name == name)
            })
            .map(|room| room.id.clone())
    }) else {
        return Err(Error::msg(format!(
            "assert-invariants gold_flow_split_admission failed in shift {}: active core member is not in a gold TradePost",
            shift.index + 1
        )));
    };

    Ok(Some([kirara, pozyomka, tuye]))
}

fn operator_has_non_dorm_assignment(
    blueprint: &BaseBlueprint,
    shift: &TeamShiftResult,
    name: &str,
) -> bool {
    shift.assignment.iter_rooms().any(|room| {
        room.operators.iter().any(|operator| operator.name == name)
            && blueprint
                .room(&room.room_id)
                .is_some_and(|spec| spec.kind != FacilityKind::Dormitory)
    }) || shift
        .assignment
        .training_assist
        .as_ref()
        .is_some_and(|operator| operator.name == name)
}

fn assert_main_backup_24_24_final_state(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    initial_mood: f64,
) -> Result<()> {
    if report.shifts.len() != 2
        || report.assignment_plans.len() != 2
        || report.teams.len() != 2
        || report
            .teams
            .iter()
            .filter(|team| team.label == TeamLabel::Alpha)
            .count()
            != 1
        || report
            .teams
            .iter()
            .filter(|team| team.label == TeamLabel::Beta)
            .count()
            != 1
    {
        return Err(Error::msg(
            "assert-invariants main_backup_24_24_topology failed: requires two owners, teams and states",
        ));
    }
    let model = default_mood_model()?;
    let available = (initial_mood.min(model.mood_cap) - model.rest_threshold).max(0.0);
    let action_count = report
        .shifts
        .iter()
        .filter(|shift| shift.fiammetta.is_some())
        .count();
    let fiammetta_targets = main_backup_24_24_fiammetta_targets_from_report(report);
    if action_count > 2 || fiammetta_targets.len() != action_count {
        return Err(Error::msg(
            "assert-invariants main_backup_24_24_fiammetta_targets failed: action targets must be distinct and within the two-slot limit",
        ));
    }
    if action_count > 0 {
        for (index, shift) in report.shifts.iter().enumerate() {
            let Some(action) = shift.fiammetta.as_ref() else {
                continue;
            };
            if !blueprint
                .room(&action.room_id)
                .is_some_and(|room| room.kind == FacilityKind::TradePost)
            {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_fiammetta_trade_room failed in shift {} for {}",
                    index + 1,
                    action.target
                )));
            }
            if trade_post_room_for_operator(blueprint, &shift.assignment, &action.target).as_ref()
                != Some(&action.room_id)
            {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_fiammetta_target_assignment failed in shift {} for {}",
                    index + 1,
                    action.target
                )));
            }
            let plan = report
                .assignment_plans
                .get(shift.plan_index)
                .ok_or_else(|| {
                    Error::msg(format!(
                        "assert-invariants main_backup_24_24_plan_index failed in shift {}",
                        index + 1
                    ))
                })?;
            if has_multi_member_shift_bind(&action.target, &shift_binds_from_plan(plan)) {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_fiammetta_bind failed for {}: target is part of a multi-member bind",
                    action.target
                )));
            }
        }
        let mut repeated_shifts = Vec::with_capacity(report.shifts.len() * 2);
        for cycle in 0..2 {
            for shift in &report.shifts {
                let mut repeated = shift.clone();
                repeated.index += cycle * report.shifts.len();
                repeated_shifts.push(repeated);
            }
        }
        fiammetta_events::validate_actions(&repeated_shifts, &model, blueprint, instances)
            .map_err(|error| {
                Error::msg(format!(
                    "assert-invariants main_backup_24_24_fiammetta_ready failed: {error}"
                ))
            })?;
        for target in &fiammetta_targets {
            validate_operator_cycle_mood(blueprint, instances, report, initial_mood, target)
                .map_err(|error| {
                    Error::msg(format!(
                        "assert-invariants main_backup_24_24_fiammetta_mood failed: {error}"
                    ))
                })?;
        }
    }
    let mut cycle_workers = HashSet::new();
    for (index, shift) in report.shifts.iter().enumerate() {
        let (active, resting) = if index == 0 {
            (TeamLabel::Alpha, TeamLabel::Beta)
        } else {
            (TeamLabel::Beta, TeamLabel::Alpha)
        };
        if shift.index != index
            || shift.plan_index != index
            || !shift.duration_hours.is_finite()
            || (shift.duration_hours - 24.0).abs() > 1e-9
            || shift.active_teams.as_slice() != [active].as_slice()
            || shift.resting_team != resting
            || shift.transition != ShiftTransition::ReplaceRooms
        {
            return Err(Error::msg(format!(
                "assert-invariants main_backup_24_24_topology failed in shift {}",
                index + 1,
            )));
        }
        let plan = &report.assignment_plans[shift.plan_index];
        for anchor in plan
            .anchors
            .iter()
            .filter(|anchor| !anchor.proactive_support)
        {
            let present = blueprint.rooms.iter().any(|room| {
                room.kind == anchor.facility
                    && anchor.room_id.as_ref().is_none_or(|id| id == &room.id)
                    && shift
                        .assignment
                        .operators_in(&room.id)
                        .iter()
                        .any(|operator| {
                            operator.name == anchor.operator && operator.elite >= anchor.elite
                        })
            });
            if !present {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_plan_anchor failed in shift {} for {}",
                    index + 1,
                    anchor.operator,
                )));
            }
        }
        let active_members: HashSet<_> = report
            .teams
            .iter()
            .filter(|team| team.label == active)
            .flat_map(|team| team.operators.iter().map(String::as_str))
            .collect();
        let working_names = shift.assignment.work_position_operator_names(blueprint);
        for name in working_names {
            if !active_members.contains(name.as_str())
                || (!cycle_workers.insert(name.clone()) && !fiammetta_targets.contains(&name))
            {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_work_disjoint failed in shift {} for {}",
                    index + 1,
                    name,
                )));
            }
        }
        for bind in shift_binds_from_plan(plan) {
            if bind.operators.iter().any(|name| {
                !active_members.contains(name.as_str())
                    || !shift.assignment.operator_names().contains(name)
            }) {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_bind failed in shift {} for {:?}",
                    index + 1,
                    bind.operators,
                )));
            }
        }
        for room in &blueprint.rooms {
            if matches!(
                room.kind,
                FacilityKind::TradePost
                    | FacilityKind::Factory
                    | FacilityKind::PowerPlant
                    | FacilityKind::ControlCenter
            ) && shift.assignment.operators_in(&room.id).len() > room.operator_capacity()
            {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_capacity failed in shift {} room {}: exceeds capacity",
                    index + 1,
                    room.id.0,
                )));
            }
        }
        let eta = shift_eta_with_instances(&model, blueprint, &shift.assignment, instances);
        for operator in eta.per_op {
            if !operator.drain_per_hour.is_finite()
                || operator.drain_per_hour * shift.duration_hours > available + 1e-9
            {
                return Err(Error::msg(format!(
                    "assert-invariants main_backup_24_24_work_budget failed in shift {} for {}",
                    index + 1,
                    operator.name,
                )));
            }
        }
    }
    validate_mood_gap_decay_work_spans(blueprint, instances, table, report.profile, &report.shifts)
        .map_err(|error| {
            Error::msg(format!(
                "assert-invariants main_backup_24_24_manufacture_work failed: {error}",
            ))
        })?;
    Ok(())
}

fn main_backup_24_24_fiammetta_targets_from_report(report: &TeamRotationReport) -> HashSet<String> {
    if report.profile != TimedRotationProfile::MainBackup24_24 {
        return HashSet::new();
    }
    report
        .shifts
        .iter()
        .filter_map(|shift| shift.fiammetta.as_ref())
        .map(|action| action.target.clone())
        .collect()
}

fn assert_gold_flow_final_state(
    blueprint: &BaseBlueprint,
    shift: &TeamShiftResult,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<u8> {
    let assignment = &shift.assignment;
    let shift_index = shift.index;
    let resolved = resolve_base(
        blueprint,
        assignment,
        Some(instances),
        Some(table),
        24.0,
        None,
    )?;
    let layout = &resolved.layout;
    let present_durin: HashSet<_> = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind != FacilityKind::MeetingRoom)
        .flat_map(|room| assignment.operators_in(&room.id))
        .filter(|op| {
            assignment
                .training_assist
                .as_ref()
                .is_none_or(|assist| assist.name != op.name)
                && instances
                    .tags_for(&op.name, op.tier())
                    .iter()
                    .any(|tag| tag == crate::layout::TAG_DURIN)
        })
        .map(|op| op.name.as_str())
        .collect();
    let expected_durin = present_durin.len().min(4) as u8;
    if layout.durin_in_base != expected_durin {
        return Err(Error::msg(format!(
            "assert-invariants gold_flow_durin_presence failed in shift {}: expected {}, got {}",
            shift_index + 1,
            expected_durin,
            layout.durin_in_base
        )));
    }

    let mut expected_sources = Vec::new();
    for room in blueprint.rooms_of(FacilityKind::TradePost) {
        let operators = assignment.operators_in(&room.id);
        if matches!(
            room.product,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold
            })
        ) && operators
            .iter()
            .map(|op| crate::trade::gold_flow::ordered_trio_priority(&op.name))
            .filter(|priority| *priority < 3)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::msg(format!(
                "assert-invariants gold_flow_room_order failed in shift {} room {}",
                shift_index + 1,
                room.id.0
            )));
        }
        for operator in operators {
            let buffs = instances.resolve_trade_buff_ids(&operator.name, operator.tier());
            if operator.name == "鸿雪"
                && operator.elite >= 2
                && !(buffs.iter().any(|id| id == "trade_ord_line_durin[010]")
                    && buffs.iter().any(|id| id == "trade_ord_spd&gold[100]"))
            {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_pozi_skill_mapping failed in shift {}",
                    shift_index + 1
                )));
            }
            if operator.name == "绮良"
                && operator.elite >= 2
                && !buffs.iter().any(|id| id == "trade_ord_line_gold[010]")
            {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_kirara_skill_mapping failed in shift {}",
                    shift_index + 1
                )));
            }
            let real = blueprint.gold_manu_line_count();
            let amount: u32 = buffs
                .iter()
                .map(|id| match id.as_str() {
                    "trade_ord_line_durin[010]" => u32::from(expected_durin),
                    "trade_ord_line_gold[010]" => 2 * (real / 2),
                    "trade_ord_line_gold[000]" => 2 * (real / 4),
                    _ => 0,
                })
                .sum();
            if amount > 0 {
                expected_sources.push((
                    operator.name.clone(),
                    amount,
                    buffs.iter().any(|id| id == "trade_ord_line_durin[010]"),
                ));
            }
        }
    }
    let initial = blueprint
        .initial_global_pool()
        .get_u32(GlobalResourceKey::VirtualGoldLines);
    let expected_total = initial
        + expected_sources
            .iter()
            .map(|(_, amount, _)| amount)
            .sum::<u32>();
    let mut actual_sources = layout.trade_gold_line_sources.clone();
    actual_sources.sort();
    expected_sources.sort();
    if actual_sources != expected_sources
        || layout.global.get_u32(GlobalResourceKey::VirtualGoldLines) != expected_total
    {
        return Err(Error::msg(format!(
            "assert-invariants gold_flow_global_sources failed in shift {}: expected {:?} total {}, got {:?} total {}",
            shift_index + 1, expected_sources, expected_total,
            actual_sources, layout.global.get_u32(GlobalResourceKey::VirtualGoldLines)
        )));
    }
    for room in resolved
        .trade_rooms
        .iter()
        .filter(|room| room.order.is_gold())
    {
        if !room.operators.iter().any(|op| {
            op.buff_ids.iter().any(|id| {
                matches!(
                    id.as_str(),
                    "trade_ord_spd&gold[100]"
                        | "trade_ord_spd&gold[010]"
                        | "trade_ord_spd&gold[000]"
                )
            })
        }) {
            continue;
        }
        let input = TradeRoomInput {
            level: room.level,
            operators: room.operators.clone(),
            order_count: None,
            mood: 24.0,
            gold_production_lines: Some(blueprint.gold_manu_line_count()),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(room.layout.clone()),
            active_order_kind: room.order,
        };
        let mut chain = crate::trade::interpreter::TradeContext::from_room(&input);
        crate::trade::gold_flow::apply_gold_flow_chain(&mut chain, table);
        if let Some(line) = shift
            .efficiencies
            .room_lines
            .iter()
            .find(|line| line.room_id == room.id.0 && line.operator_count > 0)
        {
            let actual = crate::trade::solve_trade_with_shift(&input, table, shift.duration_hours)?;
            if line.trade_skill_efficiency != actual.efficiency.paper.skill_efficiency {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_report_efficiency failed in shift {} room {}: expected {}, got {}",
                    shift_index + 1,
                    room.id.0,
                    actual.efficiency.paper.skill_efficiency,
                    line.trade_skill_efficiency
                )));
            }
        }
        let mut virtual_lines = expected_total;
        for operator in &room.operators {
            if let Some((_, amount, _)) = expected_sources
                .iter()
                .find(|(name, _, _)| name == &operator.name)
            {
                virtual_lines = virtual_lines.saturating_sub(*amount);
            }
        }
        for (operator, settled) in room.operators.iter().zip(&chain.operators) {
            let real = blueprint.gold_manu_line_count();
            let mut expected_eff = 0.0;
            for id in &operator.buff_ids {
                let total = real + virtual_lines;
                match id.as_str() {
                    "trade_ord_line_gold[010]" => {
                        expected_eff += 5.0;
                        virtual_lines += 2 * (real / 2);
                    }
                    "trade_ord_line_gold[000]" => {
                        expected_eff += 5.0;
                        virtual_lines += 2 * (real / 4);
                    }
                    "trade_ord_line_durin[010]" => virtual_lines += u32::from(expected_durin),
                    "trade_ord_spd&gold[100]" => expected_eff += total as f64 * 5.0,
                    "trade_ord_spd&gold[010]" => expected_eff += 5.0 + (total / 2) as f64 * 15.0,
                    "trade_ord_spd&gold[000]" => expected_eff += 5.0 + (total / 4) as f64 * 15.0,
                    _ => {}
                }
            }
            if (settled.settled_eff - expected_eff).abs() > 1e-6 {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_reader_efficiency failed in shift {} room {} for {}: expected {}, got {}",
                    shift_index + 1, room.id.0, operator.name, expected_eff, settled.settled_eff
                )));
            }
        }
    }
    Ok(expected_durin)
}

/// N/A/B 三候选各自独立完整求解后的唯一策略价值。
///
/// 三项产物先按完整轮换周期归一为 daily.production，再按用户裁决的策略权重
/// 统一为可比较的值。该值只用于菲亚候选比较，不改变报告中的原始产物字段。
fn fiammetta_candidate_value(report: &TeamRotationReport) -> f64 {
    let production = &report.daily.production;
    production.lmd + production.pure_gold + production.battle_records * 1.25
}

/// A 分支 trade_core 偏好的 alternative id。按 D4 降级：单金单贸易站 153 布局优先
/// 可露希尔；其余布局优先但书；以候选干员实际拥有情况为准（但书任意练度或
/// 可露希尔精二）。返回 None 表示无合法单核心目标。
fn fiammetta_a_core_preference(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
) -> Option<(&'static str, &'static str)> {
    let single_gold_trade = blueprint.count_facility(FacilityKind::TradePost) == 1
        && blueprint.trade_station_scenario().gold_order_stations == 1;
    let closure_ready = operbox.elite_of("可露希尔").is_some_and(|elite| elite >= 2);
    let docus_ready = operbox.owns("但书");
    if single_gold_trade {
        if closure_ready {
            return Some(("trade_core", "single_trade_closure"));
        }
        if docus_ready {
            return Some(("trade_core", "single_trade_docus_fallback"));
        }
        return None;
    }
    if docus_ready {
        return Some(("trade_core", "multi_trade_docus"));
    }
    if closure_ready {
        return Some(("trade_core", "multi_trade_closure_fallback"));
    }
    None
}

/// B 分支资格（D5）：存在三级金单贸易站，且 `match_witch_group_shortcut` 能判定
/// 合法完整龙巫组（巫恋 + 龙舌兰 + 合法裁缝 β/α/白板）。合法组合判定完全复用
/// 既有 shortcut，不手搓第二套条件；条件不足直接放弃 B，不降级单人。
fn fiammetta_b_eligible(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<bool> {
    if !operbox.owns("巫恋") || !operbox.owns("龙舌兰") {
        return Ok(false);
    }
    let trade_pool = build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
    let Some(witch) = trade_pool.entry("巫恋") else {
        return Ok(false);
    };
    let Some(tequila) = trade_pool.entry("龙舌兰") else {
        return Ok(false);
    };
    let witch_op = witch.to_trade_operator();
    let tequila_op = tequila.to_trade_operator();
    let room_exists = blueprint
        .rooms
        .iter()
        .any(|room| room.kind == FacilityKind::TradePost && witch_core_station_ok(room));
    if !room_exists {
        return Ok(false);
    }
    let has_tailor = trade_pool.entries.iter().any(|entry| {
        entry.name != "巫恋"
            && entry.name != "龙舌兰"
            && match_witch_group_shortcut(
                &[
                    witch_op.clone(),
                    tequila_op.clone(),
                    entry.to_trade_operator(),
                ],
                table,
                3,
            )
            .is_some()
    });
    Ok(has_tailor)
}

/// A 分支资格（D5）：存在金单贸易站，且但书任意练度或可露希尔精二至少其一。
fn fiammetta_a_eligible(blueprint: &BaseBlueprint, operbox: &OperBox) -> bool {
    if blueprint.trade_station_scenario().gold_order_stations == 0 {
        return false;
    }
    operbox.owns("但书") || operbox.elite_of("可露希尔").is_some_and(|elite| elite >= 2)
}

fn reject_fiammetta_candidate(error: Error) -> Result<()> {
    let message = match &error {
        Error::Msg(message) | Error::PartialRotation { message, .. } => message.as_str(),
        _ => return Err(error),
    };
    // Only known candidate feasibility failures may be skipped. Invalid inputs,
    // unsupported activity and broken internal state retain their failure boundary.
    let candidate_failure = is_budget_rejection(&error)
        || message == "no ready fiammetta target action"
        || message.starts_with("fiammetta_ready failed")
        || [
            "control: NO_CANDIDATE_UNDER_POLICY",
            "control rotation ETA candidate budget exhausted",
            "nonproduction rotation ETA cannot cover",
            "fiammetta_continuous_target_present failed",
            "all peak prefix candidates are infeasible",
            "no solution under ",
        ]
        .iter()
        .any(|prefix| message.starts_with(prefix));
    if candidate_failure {
        Ok(())
    } else {
        Err(error)
    }
}

/// ABC 三班菲亚固定策略分支适配层。
///
/// N/A/B 各自完整求解并通过最终化后才比较；合法 N 在平局时优先。
/// 无合法 N 时继续独立 A/B，但未知错误不能被当作候选不可行吞掉。
/// `gamma_relaxed` 放行续命目标，A 的 `system_preferences` 偏好贸易核心。
pub fn schedule_abc_rotation_candidates(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
) -> Result<TeamRotationReport> {
    let shift_hours = match profile {
        TimedRotationProfile::Abc12_6_6 => [12.0, 6.0, 6.0],
        TimedRotationProfile::Abc12_12_12 => [12.0, 12.0, 12.0],
        _ => {
            return Err(Error::msg(
                "schedule_abc_rotation_candidates only supports ABC profiles",
            ))
        }
    };
    let a_eligible = fiammetta_a_eligible(blueprint, operbox);
    let b_eligible = fiammetta_b_eligible(blueprint, operbox, instances, table)?;
    if !options.fiammetta_enable || !operbox.owns(FIAMMETTA_NAME) || (!a_eligible && !b_eligible) {
        let mut baseline_options = options.clone();
        baseline_options.fiammetta_enable = false;
        let report = schedule_abc_rotation(
            blueprint,
            operbox,
            instances,
            table,
            &baseline_options,
            shift_hours,
            false,
            AbyssalShortShiftPolicy::default_for(profile),
            profile,
            &[],
            &[],
        )?;
        let report =
            complete_timed_rotation(blueprint, operbox, instances, table, options.mood, report)?;
        write_candidate_report(options.fiammetta_candidates_out.as_deref(), "N", &report);
        return Ok(report);
    }

    let mut baseline_options = options.clone();
    baseline_options.fiammetta_enable = false;
    let baseline_result = schedule_abc_rotation(
        blueprint,
        operbox,
        instances,
        table,
        &baseline_options,
        shift_hours,
        false,
        AbyssalShortShiftPolicy::default_for(profile),
        profile,
        &[],
        &[],
    )
    .and_then(|baseline| {
        complete_timed_rotation(blueprint, operbox, instances, table, options.mood, baseline)
    });
    let (mut winner, baseline_error) = match baseline_result {
        Ok(baseline) => {
            write_candidate_report(options.fiammetta_candidates_out.as_deref(), "N", &baseline);
            let value = fiammetta_candidate_value(&baseline);
            eprintln!(
                "[fiammetta-candidate] N {} trade={} manufacture={} production={:?} value={value:.3} targets=[]",
                profile.cli_name(),
                baseline.daily.trade,
                baseline.daily.manufacture,
                baseline.daily.production,
            );
            (Some(baseline), None)
        }
        Err(error) if is_budget_rejection(&error) => {
            eprintln!(
                "[fiammetta-candidate] N {} rejected reason={error}",
                profile.cli_name()
            );
            (None, Some(error))
        }
        Err(error) => return Err(error),
    };
    let baseline_for_tie = winner.clone();
    let mut winner_value = winner
        .as_ref()
        .map_or(f64::NEG_INFINITY, fiammetta_candidate_value);
    let mut winner_value_tied = false;
    let mut winner_targets = Vec::<String>::new();

    if a_eligible {
        let mut a_options = options.clone();
        if let Some((system, alternative)) = fiammetta_a_core_preference(blueprint, operbox) {
            a_options
                .system_preferences
                .insert(system.to_string(), alternative.to_string());
        }
        let relaxed: Vec<&str> = if operbox.owns("但书") {
            vec!["但书", "可露希尔"]
        } else {
            vec!["可露希尔"]
        };
        let a_targets: Vec<&str> = if operbox.owns("但书") {
            vec!["但书", "可露希尔"]
        } else {
            vec!["可露希尔"]
        };
        match schedule_abc_rotation(
            blueprint,
            operbox,
            instances,
            table,
            &a_options,
            shift_hours,
            false,
            AbyssalShortShiftPolicy::default_for(profile),
            profile,
            &relaxed,
            &[],
        ) {
            Ok(mut candidate) => {
                let attached = attach_fiammetta_target_actions(
                    blueprint,
                    operbox,
                    instances,
                    profile,
                    &a_targets,
                    &mut candidate.shifts,
                )
                .and_then(|_| {
                    complete_timed_rotation(
                        blueprint,
                        operbox,
                        instances,
                        table,
                        options.mood,
                        candidate,
                    )
                });
                match attached {
                    Ok(candidate) => {
                        write_candidate_report(
                            options.fiammetta_candidates_out.as_deref(),
                            "A",
                            &candidate,
                        );
                        let value = fiammetta_candidate_value(&candidate);
                        let targets = candidate_target_names(&candidate);
                        eprintln!(
                            "[fiammetta-candidate] A {} trade={} manufacture={} production={:?} value={value:.3} targets={targets:?}",
                            profile.cli_name(),
                            candidate.daily.trade,
                            candidate.daily.manufacture,
                            candidate.daily.production,
                        );
                        if value > winner_value + 1e-9 {
                            winner = Some(candidate);
                            winner_value = value;
                            winner_value_tied = false;
                            winner_targets = targets;
                        } else if (value - winner_value).abs() <= 1e-9 {
                            winner_value_tied = true;
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "[fiammetta-candidate] A {} rejected reason={error}",
                            profile.cli_name()
                        );
                        reject_fiammetta_candidate(error)?;
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "[fiammetta-candidate] A {} rejected reason={error}",
                    profile.cli_name()
                );
                reject_fiammetta_candidate(error)?;
            }
        }
    }

    if b_eligible {
        let mut b_options = options.clone();
        b_options.trade_role_preference = Some(TradeRolePreference::Witch);
        let relaxed: Vec<&str> = vec!["巫恋", "龙舌兰"];
        match schedule_abc_rotation(
            blueprint,
            operbox,
            instances,
            table,
            &b_options,
            shift_hours,
            false,
            AbyssalShortShiftPolicy::default_for(profile),
            profile,
            &relaxed,
            &["trade_core"],
        ) {
            Ok(mut candidate) => {
                let attached = attach_fiammetta_target_actions(
                    blueprint,
                    operbox,
                    instances,
                    profile,
                    &relaxed,
                    &mut candidate.shifts,
                )
                .and_then(|_| {
                    complete_timed_rotation(
                        blueprint,
                        operbox,
                        instances,
                        table,
                        options.mood,
                        candidate,
                    )
                });
                match attached {
                    Ok(candidate) => {
                        write_candidate_report(
                            options.fiammetta_candidates_out.as_deref(),
                            "B",
                            &candidate,
                        );
                        let value = fiammetta_candidate_value(&candidate);
                        let targets = candidate_target_names(&candidate);
                        eprintln!(
                            "[fiammetta-candidate] B {} trade={} manufacture={} production={:?} value={value:.3} targets={targets:?}",
                            profile.cli_name(),
                            candidate.daily.trade,
                            candidate.daily.manufacture,
                            candidate.daily.production,
                        );
                        if value > winner_value + 1e-9 {
                            winner = Some(candidate);
                            winner_value = value;
                            winner_value_tied = false;
                            winner_targets = targets;
                        } else if (value - winner_value).abs() <= 1e-9 {
                            winner_value_tied = true;
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "[fiammetta-candidate] B {} rejected reason={error}",
                            profile.cli_name()
                        );
                        reject_fiammetta_candidate(error)?;
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "[fiammetta-candidate] B {} rejected reason={error}",
                    profile.cli_name()
                );
                reject_fiammetta_candidate(error)?;
            }
        }
    }

    if let Some(baseline) = baseline_for_tie {
        if winner_value_tied && (winner_value - fiammetta_candidate_value(&baseline)).abs() <= 1e-9
        {
            winner = Some(baseline);
            winner_targets.clear();
        }
    }
    let winner = winner.ok_or_else(|| {
        baseline_error.unwrap_or_else(|| Error::msg("no legal baseline or Fiammetta candidate"))
    })?;
    let value = fiammetta_candidate_value(&winner);
    eprintln!(
        "[fiammetta-candidate] winner {} trade={} manufacture={} production={:?} value={value:.3} targets={winner_targets:?}",
        winner.profile.cli_name(),
        winner.daily.trade,
        winner.daily.manufacture,
        winner.daily.production,
    );
    Ok(winner)
}

fn candidate_target_names(report: &TeamRotationReport) -> Vec<String> {
    report
        .shifts
        .iter()
        .filter_map(|shift| shift.fiammetta.as_ref())
        .map(|action| action.target.clone())
        .collect()
}

/// 把最终化后的候选排班报告写入指定目录（N.json / A.json / B.json）。
/// 仅用于调试与对比；目录不存在时创建，写入失败不阻断主流程。
fn write_candidate_report(
    directory: Option<&std::path::Path>,
    name: &str,
    report: &TeamRotationReport,
) {
    let Some(directory) = directory else {
        return;
    };
    let Ok(()) = std::fs::create_dir_all(directory) else {
        return;
    };
    let value = fiammetta_candidate_value(report);
    let payload = serde_json::json!({
        "schema_version": 1,
        "candidate": name,
        "value_policy": {
            "lmd": 1.0,
            "pure_gold": 1.0,
            "battle_records": 1.25
        },
        "value": value,
        "daily_production": report.daily.production,
        "report": report
    });
    let Ok(json) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    let _ = std::fs::write(directory.join(format!("{name}.json")), json);
}

/// 对固定目标各写一次菲亚换心情动作，不改 assignment、不 refill。
/// 用 `fiammetta_ready_events` 校验（菲亚按自律速率攒满 + 目标当班在岗才 ready），ready 即每班
/// 在岗保证由 L1 shortcut 自然选中 + `validate_fiammetta_continuous_targets` 兜底。
fn attach_fiammetta_target_actions(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    profile: TimedRotationProfile,
    targets: &[&str],
    shifts: &mut [TeamShiftResult],
) -> Result<()> {
    if !operbox.owns(FIAMMETTA_NAME) {
        return Err(Error::msg(
            "fiammetta target actions require owned Fiammetta",
        ));
    }
    let slots = fiammetta_action_slots(profile, shifts.len());
    if slots.is_empty() || targets.is_empty() {
        return Ok(());
    }
    let mood_model = default_mood_model()?;
    let action_budget = slots.len();
    let mut used_slots = 0usize;
    let mut consumed_shifts = HashSet::new();
    for target in targets {
        if used_slots >= action_budget {
            break;
        }
        let Some(slot) = (0..shifts.len()).find(|slot| {
            !consumed_shifts.contains(slot)
                && shifts[*slot].assignment.iter_rooms().any(|room| {
                    blueprint.room(&room.room_id).is_some_and(|spec| {
                        matches!(spec.kind, FacilityKind::TradePost | FacilityKind::Factory)
                    }) && room
                        .operators
                        .iter()
                        .any(|operator| operator.name == *target)
                })
        }) else {
            continue;
        };
        let Some(room_id) = shifts[slot].assignment.iter_rooms().find_map(|room| {
            blueprint
                .room(&room.room_id)
                .filter(|spec| matches!(spec.kind, FacilityKind::TradePost | FacilityKind::Factory))
                .and_then(|_| {
                    room.operators
                        .iter()
                        .any(|operator| operator.name == *target)
                        .then(|| room.room_id.clone())
                })
        }) else {
            continue;
        };
        if !fiammetta_events::try_accept_action(
            shifts,
            slot,
            target,
            &room_id,
            &mood_model,
            blueprint,
            instances,
        ) {
            continue;
        }
        consumed_shifts.insert(slot);
        used_slots += 1;
    }
    if used_slots == 0 {
        return Err(Error::msg("no ready fiammetta target action"));
    }
    validate_fiammetta_continuous_targets(blueprint, profile, shifts)?;
    Ok(())
}

/// 周期心情预算在候选阶段仍是筛选条件（让合法候选优先于违规候选）；
/// 但所有候选都被它拒绝时不再拒绝输出：保留的完整排班带 `rotation_final_invariant:` 警告返回，
/// 交给人工修正。没有保留排班的错误（输入非法、无候选等）原样传播。
fn downgrade_cycle_mood_rejection(error: Error) -> Result<TeamRotationReport> {
    if !is_budget_rejection(&error) {
        return Err(error);
    }
    let Some(mut report) = error.partial_rotation_report() else {
        return Err(error);
    };
    report
        .warnings
        .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {error}"));
    Ok(report)
}

pub fn schedule_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
) -> Result<TeamRotationReport> {
    if profile == TimedRotationProfile::FiammettaWitch12_12 {
        return Err(Error::msg(
            "unsupported rotation profile fiammetta_witch_12_12: this product mode was removed; no fallback is performed",
        ));
    }
    let started = Instant::now();
    let before = crate::profile::solver_perf_snapshot();
    let result =
        schedule_timed_rotation_inner(blueprint, operbox, instances, table, options, profile)
            .or_else(downgrade_cycle_mood_rejection);
    let after = crate::profile::solver_perf_snapshot();
    crate::profile::emit_resolve_stage(
        "rotation_resolve",
        "total",
        if result.is_ok() { "ok" } else { "error" },
        started.elapsed(),
        before,
        after,
    );
    crate::profile::emit_solver_stage(
        "rotation",
        "total",
        if result.is_ok() { "ok" } else { "error" },
        started.elapsed(),
        before,
        after,
    );
    result
}

fn schedule_timed_rotation_inner(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
) -> Result<TeamRotationReport> {
    if matches!(
        profile,
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12
    ) {
        return schedule_abc_rotation_candidates(
            blueprint, operbox, instances, table, options, profile,
        );
    }
    let (report, fiammetta_24_diagnostic) = match profile {
        TimedRotationProfile::Abc12_6_6 => (
            schedule_abc_rotation(
                blueprint,
                operbox,
                instances,
                table,
                options,
                [12.0, 6.0, 6.0],
                options.fiammetta_enable,
                AbyssalShortShiftPolicy::Opportunistic,
                TimedRotationProfile::Abc12_6_6,
                &[],
                &[],
            )?,
            None,
        ),
        TimedRotationProfile::Abc12_12_12 => (
            schedule_abc_rotation(
                blueprint,
                operbox,
                instances,
                table,
                options,
                [12.0, 12.0, 12.0],
                options.fiammetta_enable,
                AbyssalShortShiftPolicy::Disabled,
                TimedRotationProfile::Abc12_12_12,
                &[],
                &[],
            )?,
            None,
        ),
        TimedRotationProfile::MainBackup12_12 => (
            schedule_main_backup_12_12(blueprint, operbox, instances, table, options)?,
            None,
        ),
        TimedRotationProfile::MainBackup24_24 => {
            schedule_main_backup_24_24(blueprint, operbox, instances, table, options)?
        }
        TimedRotationProfile::FiammettaWitch12_12 => {
            return Err(Error::msg(
                "unsupported rotation profile fiammetta_witch_12_12: this product mode was removed; no fallback is performed",
            ));
        }
        TimedRotationProfile::Fiammetta8_8_4_4 => {
            if !options.fiammetta_enable {
                return Err(Error::msg(
                    "fiammetta-8844 requires fiammetta_enable=true; disable conflicts with the explicit Fiammetta rotation profile",
                ));
            }
            (
                schedule_fiammetta_8_8_4_4(blueprint, operbox, instances, table, options)?,
                None,
            )
        }
        TimedRotationProfile::Abyssal7_5_7_5 => (
            schedule_abyssal_7_5_7_5(blueprint, operbox, instances, table, options)?,
            None,
        ),
    };
    let mut report =
        complete_timed_rotation(blueprint, operbox, instances, table, options.mood, report)?;
    if let Some(diagnostic) = fiammetta_24_diagnostic {
        append_main_backup_24_fiammetta_warning(&mut report, diagnostic);
    }
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbyssalShortShiftPolicy {
    Opportunistic,
    Disabled,
}

impl AbyssalShortShiftPolicy {
    fn default_for(profile: TimedRotationProfile) -> Self {
        match profile {
            TimedRotationProfile::Abc12_6_6 => Self::Opportunistic,
            TimedRotationProfile::Abc12_12_12 => Self::Disabled,
            _ => Self::Disabled,
        }
    }
}

struct AbcRotationEtaFailure {
    optional_control_role: Option<(String, String, String)>,
}

enum AbcControlCandidateFailure {
    Eta(String),
    Other(Error),
}

impl AbcRotationEtaFailure {
    fn for_operator(plan: &AssignmentPlan, failed: &str) -> Self {
        Self {
            optional_control_role: plan.anchors.iter().find_map(|anchor| {
                if anchor.operator != failed {
                    return None;
                }
                anchor.optional_control_role.as_ref().map(|role| {
                    (
                        anchor.system_id.clone(),
                        role.clone(),
                        anchor.operator.clone(),
                    )
                })
            }),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn schedule_abc_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: [f64; 3],
    enable_fiammetta_return: bool,
    abyssal_policy: AbyssalShortShiftPolicy,
    profile: TimedRotationProfile,
    gamma_relaxed: &[&str],
    skip_systems: &[&str],
) -> Result<TeamRotationReport> {
    let mut retry_options = options.clone();
    let mut reversed_halves = false;
    let mut control_team_attempt = 0;
    loop {
        let mut eta_failure = None;
        let result = schedule_abc_rotation_once(
            blueprint,
            operbox,
            instances,
            table,
            &retry_options,
            shift_hours,
            enable_fiammetta_return,
            abyssal_policy,
            profile,
            gamma_relaxed,
            skip_systems,
            reversed_halves,
            control_team_attempt,
            &mut eta_failure,
        );
        match result {
            Err(error @ Error::ControlRotationRequiredAnchorsExceedCapacity { .. })
                if !skip_systems.contains(&"pozyomka_durin") =>
            {
                let mut fallback_skip = skip_systems.to_vec();
                fallback_skip.push("pozyomka_durin");
                eprintln!(
                    "[rotation-fallback] retry ordinary trade Plan after the four-gold core made required control anchors infeasible: {error}"
                );
                return schedule_abc_rotation(
                    blueprint,
                    operbox,
                    instances,
                    table,
                    &retry_options,
                    shift_hours,
                    enable_fiammetta_return,
                    abyssal_policy,
                    profile,
                    gamma_relaxed,
                    &fallback_skip,
                );
            }
            Err(error) => {
                let Some(failure) = eta_failure else {
                    return Err(error);
                };
                if !reversed_halves {
                    reversed_halves = true;
                    eprintln!("[rotation-eta] retry ABC with reversed production halves: {error}");
                    continue;
                }
                if control_team_attempt < 3 {
                    control_team_attempt += 1;
                    reversed_halves = false;
                    eprintln!("[rotation-eta] replay full cycle with mood-cohort attempt {control_team_attempt}: {error}");
                    continue;
                }
                let Some(role) = failure.optional_control_role else {
                    return Err(error);
                };
                if !retry_options.skipped_optional_control_roles.insert(role) {
                    return Err(error);
                }
                reversed_halves = false;
                control_team_attempt = 0;
                eprintln!("[rotation-eta] retry peak Plan without optional control role: {error}");
            }
            result => return result,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn schedule_abc_rotation_once(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: [f64; 3],
    enable_fiammetta_return: bool,
    abyssal_policy: AbyssalShortShiftPolicy,
    profile: TimedRotationProfile,
    gamma_relaxed: &[&str],
    skip_systems: &[&str],
    reversed_halves: bool,
    control_team_attempt: usize,
    eta_failure: &mut Option<AbcRotationEtaFailure>,
) -> Result<TeamRotationReport> {
    let t0 = Instant::now();
    let m0 = crate::profile::solver_perf_snapshot();
    let cycle_hours: f64 = shift_hours.iter().sum();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mut peak_options = options.clone();
    peak_options.shift_hours = shift_hours[0] + shift_hours[1].max(shift_hours[2]);
    peak_options.registry_sustain_hours = Some(peak_options.shift_hours);
    let mut gamma_h1_options = options.clone();
    gamma_h1_options.shift_hours = shift_hours[1];
    let mut gamma_h2_options = options.clone();
    gamma_h2_options.shift_hours = shift_hours[2];

    // 1) 参考高峰班 + 编排计划 → 只取宿舍作为三班共享脚手架；办公室不钉死到 peak。
    // 中枢后续按 αβγ 队伍轮休重分配，不能从 peak 直接钉死。
    // 深海链因歌蕾蒂娅心情消耗 ~3.2/h（最长约 7h），不可进入 12h 主班；
    // 唯一入口是后续 S2 6h 短班的有/无深海双路径评分对比。
    let peak_skip: HashSet<String> = skip_systems.iter().map(|s| s.to_string()).collect();
    let peak_result = assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        &peak_options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
        &peak_skip,
    )?;
    let peak = peak_result.assignment;
    let mut peak_plan = peak_result.plan;
    peak_plan.resolve_actual_producer_dependencies(blueprint, &peak, instances, table)?;
    let mut shift_binds = shift_binds_from_plan(&peak_plan);
    let peak_office_names = peak_office_operators(blueprint, &peak);
    let rotating_anchor_names = bound_rotating_anchor_operators(blueprint, &peak, &shift_binds);
    let rotating_meeting_names = peak_meeting_operators(blueprint, &peak);
    let excluded_shared_names: HashSet<String> = peak_office_names
        .iter()
        .chain(&rotating_anchor_names)
        .cloned()
        .collect();
    let mood_model = default_mood_model()?;
    let peak_mood_eta = Some(shift_eta_with_instances(
        &mood_model,
        blueprint,
        &peak,
        instances,
    ));
    let mut shared = pinned_assignment_excluding(&peak, blueprint, &excluded_shared_names);
    pin_active_dependency_required(
        &mut shared,
        &peak,
        &peak_plan.active_dependencies,
        &shift_binds,
    )?;
    // 被 active dependency 钉进 shared 的会客室成员按全周期共享（required 语义），
    // 不得再进入 rotating meeting cohort，否则 resting 班会因 shared 保留而重复出现。
    let shared_names: HashSet<String> = operators_of(&shared).into_iter().collect();
    let rotating_meeting_names =
        rotating_meeting_names_excluding_shared(rotating_meeting_names, &shared_names);
    let mut scaffold_used: HashSet<String> = operators_of(&shared).into_iter().collect();
    scaffold_used.extend(peak_plan.excluded_operators.iter().cloned());
    let t1 = Instant::now();
    let m1 = crate::profile::solver_perf_snapshot();

    // 2) 以脚手架解算共享 layout；γ 生产搜索稍后按实际 bind 半区构造各自中枢上下文。
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
        power: build_power_pool_with_fillers(operbox, instances, table)?,
    };
    let control_pool = build_control_pool_with_fillers(operbox, instances, table)?;
    let automatically_excluded_control =
        automatically_excluded_control_names(&control_pool, table)?;

    // 班次绑定（上2休1）来自统一 plan，不再硬编码具体体系。
    // 薇薇安娜的骑士制造增益对普通候选同样有效；若 peak 同时选中薇薇安娜
    // 与砾，则把实际骑士消费者加入同班约束，避免切半时把增益来源与消费者拆开。
    if peak
        .control_operators()
        .iter()
        .any(|operator| operator.name == "薇薇安娜")
        && peak.iter_rooms().any(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|spec| spec.kind == FacilityKind::Factory)
                && room.operators.iter().any(|operator| operator.name == "砾")
        })
    {
        if let Some(bind) = shift_binds
            .iter_mut()
            .find(|bind| bind.operators.iter().any(|name| name == "薇薇安娜"))
        {
            if !bind.operators.iter().any(|name| name == "砾") {
                bind.operators.push("砾".to_string());
            }
        } else {
            shift_binds.push(RuntimeShiftBind {
                operators: vec!["薇薇安娜".to_string(), "砾".to_string()],
                on_shifts: 2,
                off_shifts: 1,
            });
        }
    }
    let bound_office_names = bound_office_operators(blueprint, &peak, &shift_binds);
    let mut halves =
        split_production_facilities(blueprint, &peak, &shift_binds, &peak_plan.rotation_reserves)?;
    // Retry the two intact halves in the other work/rest orientation. Gamma and
    // every dependent cohort are rebuilt below; no bind component is moved or split.
    if reversed_halves {
        halves.swap(0, 1);
    }
    let [h1, h2] = halves;
    let required_manu_by_room = active_dependency_required_manu_by_room(
        blueprint,
        &peak,
        &peak_plan.active_dependencies,
        &shift_binds,
    );

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
    let m2 = crate::profile::solver_perf_snapshot();

    // 4) γ 为替补：S2 接 H1、S3 接 H2，干员与 α/β 互斥（两次各自从剩余池取，可复用同人）。
    let mut used_ab = used.clone();
    used_ab.extend(bound_control_operators(blueprint, &peak, &shift_binds));
    // A bound office member must remain available for the office assignment;
    // do not let gamma production consume the other side of the bind first.
    used_ab.extend(bound_office_names.iter().cloned());
    // A bound rotating anchor (dorm / workshop / training-room support such as the
    // Rhine group) is placed per shift by `assign_rotating_anchors` and owned by one
    // team. Gamma production must not also take it, otherwise the member belongs to
    // two teams, works every shift and never rests.
    used_ab.extend(rotating_anchor_names.iter().cloned());

    let mut gamma_h1 = BaseAssignment::default();
    let mut used_g1 = used_ab.clone();
    let gamma_h1_context =
        gamma_production_coexist_seed(blueprint, &shared, &peak, &shift_binds, &h1, &beta);
    let mut gamma_h2 = BaseAssignment::default();
    let mut used_g2 = used_ab.clone();
    let gamma_h2_context =
        gamma_production_coexist_seed(blueprint, &shared, &peak, &shift_binds, &h2, &alpha);
    for relaxed in gamma_relaxed {
        // 候选分支的菲亚续命目标跨班复用：目标在 peak（S1）已上岗时，其原房会
        // 通过 coexist 延续到 S2/S3；γ 只在目标尚未在 coexist 在岗的班次放行它，
        // 避免跨班时在另一间贸易房重复落位（multiple room slots）。目标是各组合
        // 的唯一 core，不参与 shift_bind / rotation_reserves / active dependency
        // 的绑定闭包，也不出现在办公室/宿舍，移除无副作用。
        if !gamma_h1_context
            .assignment
            .operator_names()
            .contains(*relaxed)
        {
            used_g1.remove(*relaxed);
        }
        if !gamma_h2_context
            .assignment
            .operator_names()
            .contains(*relaxed)
        {
            used_g2.remove(*relaxed);
        }
    }
    place_rotation_reserves_for_halves(
        blueprint,
        operbox,
        &peak_plan.rotation_reserves,
        &h1,
        &mut gamma_h1,
        &mut used_g1,
        &h2,
        &mut gamma_h2,
        &mut used_g2,
    )?;
    let mut gamma_h1_coexist = gamma_h1_context.assignment.clone();
    merge_rooms(&mut gamma_h1_coexist, &gamma_h1);
    if !operator_is_in_normal_base(blueprint, &gamma_h1_coexist, ULPIANUS_NAME) {
        // Deepcolor's trade bonus has a final-state ULPianus presence requirement.
        // This gamma branch does not own a ULPianus placement, so it may only use
        // Deepcolor when the shared/coexisting assignment already proves presence.
        used_g1.insert(SPHINX_NAME.to_string());
    }
    assign_gamma_half(
        blueprint,
        instances,
        &gamma_h1_context.assignment,
        Some(durin_plan),
        &pools,
        table,
        &gamma_h1_options,
        &h1,
        &required_manu_by_room,
        &peak_plan,
        &mut gamma_h1,
        &mut used_g1,
    )?;
    print_trade_rotation_debug("gamma_h1 after fill", &gamma_h1);
    let mut gamma_h1_full_context = gamma_h1_context.assignment.clone();
    merge_rooms(&mut gamma_h1_full_context, &gamma_h1);
    let gamma_h1_layout = resolve_base(
        blueprint,
        &gamma_h1_full_context,
        Some(instances),
        Some(table),
        options.mood,
        Some(durin_plan),
    )?
    .layout_snapshot();

    if shift_hours[1] + shift_hours[2] >= MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS {
        for room_id in &h1.manu {
            for operator in gamma_h1.operators_in(room_id) {
                if pools.manu.entry(&operator.name).is_some_and(|entry| {
                    manufacture_operator_has_mood_gap_decay(&entry.to_manu_operator(), table)
                }) {
                    used_g2.insert(operator.name.clone());
                }
            }
        }
    }
    let mut gamma_h2_coexist = gamma_h2_context.assignment.clone();
    merge_rooms(&mut gamma_h2_coexist, &gamma_h2);
    if !operator_is_in_normal_base(blueprint, &gamma_h2_coexist, ULPIANUS_NAME) {
        used_g2.insert(SPHINX_NAME.to_string());
    }

    assign_gamma_half(
        blueprint,
        instances,
        &gamma_h2_context.assignment,
        Some(durin_plan),
        &pools,
        table,
        &gamma_h2_options,
        &h2,
        &required_manu_by_room,
        &peak_plan,
        &mut gamma_h2,
        &mut used_g2,
    )?;
    print_trade_rotation_debug("gamma_h2 after fill", &gamma_h2);
    validate_rotation_reserves_before_postprocess(
        blueprint,
        &peak_plan.rotation_reserves,
        &h1,
        &gamma_h1,
        &h2,
        &gamma_h2,
    )?;
    let t3 = Instant::now();
    let m3 = crate::profile::solver_perf_snapshot();

    // 中枢每班独立分配：体系绑定干员跟随生产队同上同下，散件均分三队，
    // 每班仅允许活跃两队的中枢干员入池（保证每人休息一班）。
    let peak_ctrl: Vec<String> = peak
        .control_operators()
        .iter()
        .map(|o| o.name.clone())
        .collect();
    let peak_ctrl_set: HashSet<String> = peak_ctrl.iter().cloned().collect();
    let mut system_ctrl_names = system_control_operators(&peak_plan, &peak_ctrl_set);
    system_ctrl_names.extend(
        shift_binds
            .iter()
            .flat_map(|bind| &bind.operators)
            .filter(|name| peak_ctrl.contains(name))
            .cloned(),
    );
    let mut team_ctrl = build_team_control_map(&peak_ctrl, &peak_plan, &h1);
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
    let entry_by_name: HashMap<String, crate::pool::ControlPoolEntry> = control_pool
        .entries
        .iter()
        .map(|entry| (entry.name.clone(), entry.clone()))
        .collect();
    // 深海猎手是 S2 短班的制造候选，不能作为中枢目录的 filler 被分进某个常驻队：
    // 否则 S2 恰是该队的休息班，猎手上岗会与休息队约束冲突。
    let abyssal_hunters: HashSet<String> =
        if abyssal_policy == AbyssalShortShiftPolicy::Opportunistic {
            owned_abyssal_hunters(operbox, instances, &used_ab)
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };
    let mut plugin_entries: Vec<_> = control_pool
        .entries
        .iter()
        .filter(|entry| !team_ctrl.values().any(|names| names.contains(&entry.name)))
        .filter(|entry| !abyssal_hunters.contains(&entry.name))
        .filter(|entry| !production_team_by_name.contains_key(&entry.name))
        .filter(|entry| !automatically_excluded_control.contains(&entry.name))
        .filter(|entry| !scaffold_used.contains(&entry.name))
        .filter(|entry| !bound_office_names.contains(&entry.name))
        .filter(|entry| !rotating_anchor_names.contains(&entry.name))
        .filter(|entry| !rotating_meeting_names.contains(&entry.name))
        .filter(|entry| !peak_plan.control_exclusions.contains(&entry.name))
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
    // 跨设施 shift bind 是最终归属约束，不能被中枢均衡策略再次拆开。
    for bind in &shift_binds {
        let Some(team) = bind
            .operators
            .iter()
            .find_map(|name| production_team_by_name.get(name).copied())
            .or_else(|| {
                bind.operators.iter().find_map(|name| {
                    team_ctrl.iter().find_map(|(team, names)| {
                        names.iter().any(|member| member == name).then_some(*team)
                    })
                })
            })
        else {
            continue;
        };
        for name in &bind.operators {
            if peak_ctrl.contains(name) {
                move_control_operator_to_team(&mut team_ctrl, name, team);
            }
        }
    }
    for names in team_ctrl.values_mut() {
        names.sort();
        names.dedup();
    }
    if control_team_attempt > 0 {
        // A bounded ordering heuristic, never a new bind: ordinary members of
        // one modeled faction are tried in one cohort before splitting them.
        let target = TeamLabel::ALL[control_team_attempt - 1];
        let mut fixed = system_ctrl_names.clone();
        fixed.extend(
            shift_binds
                .iter()
                .flat_map(|bind| bind.operators.iter())
                .cloned(),
        );
        for faction in &mood_model.control_faction_recovery {
            for name in &faction.members {
                if fixed.contains(name) || production_team_by_name.contains_key(name) {
                    continue;
                }
                if team_ctrl.values().any(|names| names.contains(name)) {
                    move_control_operator_to_team(&mut team_ctrl, name, target);
                }
            }
        }
    }
    let mut rotation_team_by_name = production_team_by_name.clone();
    for team in TeamLabel::ALL {
        for name in team_ctrl.get(&team).into_iter().flatten() {
            rotation_team_by_name.entry(name.clone()).or_insert(team);
        }
    }
    let bound_office_team =
        rotating_team_for_names(&shift_binds, &bound_office_names, &rotation_team_by_name);
    let rotating_anchor_team =
        rotating_team_for_names(&shift_binds, &rotating_anchor_names, &rotation_team_by_name);
    let mut rotating_meeting_team = rotating_team_for_names(
        &shift_binds,
        &rotating_meeting_names,
        &rotation_team_by_name,
    );
    let office_plan_excluded: HashSet<String> =
        peak_plan.excluded_operators.iter().cloned().collect();
    let mut office_ranked_pool = office_candidates(
        operbox,
        instances,
        table,
        &layout,
        options.mood,
        &office_plan_excluded,
    );
    let mut reserved: HashSet<String> = operators_of(&alpha)
        .into_iter()
        .chain(operators_of(&beta))
        .chain(operators_of(&gamma_h1))
        .chain(operators_of(&gamma_h2))
        .chain(system_ctrl_names.iter().cloned())
        .chain(operators_of(&shared))
        .chain(bound_office_names.iter().cloned())
        .chain(rotating_anchor_names.iter().cloned())
        .collect();
    let meeting_pool = meeting_candidates(
        operbox,
        &layout,
        &peak_plan.excluded_operators.iter().cloned().collect(),
    );
    complete_rotating_meeting_teams(
        blueprint,
        &rotating_meeting_names,
        &mut rotating_meeting_team,
        &meeting_pool,
        &mut reserved,
    )?;
    // 回收站队伍归属与会客室同一时点冻结：在中枢目录建立之前，
    // 且排除已预留给生产、中枢、办公室、会客室的成员。
    let mut recycle_excluded: HashSet<String> =
        peak_plan.excluded_operators.iter().cloned().collect();
    recycle_excluded.extend(bound_control_operators(blueprint, &peak, &shift_binds));
    recycle_excluded.extend(system_ctrl_names.iter().cloned());
    recycle_excluded.extend(reserved.iter().cloned());
    let (recycle_skill_pool, _) = if blueprint.count_facility(FacilityKind::Recycle) > 0 {
        recycle_candidates(operbox, instances, table, &recycle_excluded)?
    } else {
        (Vec::new(), Vec::new())
    };
    let mut rotating_recycle_team: HashMap<String, TeamLabel> = HashMap::new();
    complete_rotating_recycle_teams(blueprint, &mut rotating_recycle_team, &recycle_skill_pool);
    reserved.extend(rotating_recycle_team.keys().cloned());
    // Candidate directories do not reserve physical jobs. Actual bound and
    // rotating meeting roles are frozen before the control directory is built.
    for names in team_ctrl.values_mut() {
        names.retain(|name| {
            !rotating_meeting_team.contains_key(name) && !rotating_recycle_team.contains_key(name)
        });
    }
    let mut work_teams = production_team_by_name.clone();
    work_teams.extend(
        bound_office_team
            .iter()
            .map(|(name, team)| (name.clone(), *team)),
    );
    work_teams.extend(
        rotating_anchor_team
            .iter()
            .map(|(name, team)| (name.clone(), *team)),
    );
    work_teams.extend(
        rotating_meeting_team
            .iter()
            .map(|(name, team)| (name.clone(), *team)),
    );
    work_teams.extend(
        rotating_recycle_team
            .iter()
            .map(|(name, team)| (name.clone(), *team)),
    );
    let branch_producer = (abyssal_policy == AbyssalShortShiftPolicy::Opportunistic)
        .then(|| abyssal_control_producer(operbox, instances, &used_ab))
        .flatten()
        .filter(|name| !work_teams.contains_key(name) && !shared_names.contains(name));
    if let Some(producer) = &branch_producer {
        for names in team_ctrl.values_mut() {
            names.retain(|name| name != producer);
        }
    }
    let control_closure = Arc::new(ControlRotationClosure::new(
        blueprint,
        &control_pool,
        &peak_plan,
        &peak,
        &shared,
        &team_ctrl,
        &work_teams,
        &system_ctrl_names,
        branch_producer.as_deref(),
        &shift_binds,
        table,
    )?);
    for names in team_ctrl.values_mut() {
        names.clear();
    }
    for entry in &control_pool.entries {
        if let Some(team) = control_closure.team_of(&entry.name) {
            team_ctrl.entry(team).or_default().push(entry.name.clone());
        }
    }
    office_ranked_pool.retain(|candidate| {
        (!reserved.contains(&candidate.name)
            && !rotating_meeting_team.contains_key(&candidate.name)
            && !rotating_recycle_team.contains_key(&candidate.name))
            || bound_office_names.contains(&candidate.name)
    });

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
    for (name, team) in bound_office_team.iter().chain(&rotating_anchor_team) {
        match team {
            TeamLabel::Alpha => alpha_ops.push(name.clone()),
            TeamLabel::Beta => beta_ops.push(name.clone()),
            TeamLabel::Gamma => gamma_ops.push(name.clone()),
        }
    }
    for (name, team) in rotating_meeting_team
        .iter()
        .chain(rotating_recycle_team.iter())
    {
        match team {
            TeamLabel::Alpha => alpha_ops.push(name.clone()),
            TeamLabel::Beta => beta_ops.push(name.clone()),
            TeamLabel::Gamma => gamma_ops.push(name.clone()),
        }
    }
    alpha_ops.sort();
    alpha_ops.dedup();
    beta_ops.sort();
    beta_ops.dedup();
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

    // 5) 组装 ABC 三态（每班满编）并评分。默认时长仍为 12/6/6；
    // 菲亚 profile 会先按 16/4/4 构造，再由 profile finalizer 展开第一态。
    //
    //    S2 深海链优先路径：预占猎手 → γ fill 绕开 → 补第3人；满足准入后优先于普通 S2，深海候选内部再按效率选优。
    let shift_specs: [(f64, [TeamLabel; 2], TeamLabel, [&BaseAssignment; 2]); 3] = [
        (
            shift_hours[0],
            [TeamLabel::Alpha, TeamLabel::Beta],
            TeamLabel::Gamma,
            [&alpha, &beta],
        ),
        (
            shift_hours[1],
            [TeamLabel::Beta, TeamLabel::Gamma],
            TeamLabel::Alpha,
            [&beta, &gamma_h1],
        ),
        (
            shift_hours[2],
            [TeamLabel::Gamma, TeamLabel::Alpha],
            TeamLabel::Beta,
            [&gamma_h2, &alpha],
        ),
    ];

    let mut shifts: Vec<TeamShiftResult> = Vec::with_capacity(3);
    let mut control_options = options.clone();
    control_options.skip_standalone_control = true;
    let mut warmup_sticky_rooms: HashMap<String, RoomId> = HashMap::new();
    let mut rotation_mood: HashMap<String, f64> = operbox
        .roster()
        .names()
        .map(|name| (name.to_string(), options.mood.min(mood_model.mood_cap)))
        .collect();
    let full_rest = profile == TimedRotationProfile::Abc12_12_12;
    let mut meeting_worked_hours: HashMap<String, f64> = HashMap::new();
    for (index, (hours, active, resting, parts)) in shift_specs.into_iter().enumerate() {
        let shift_started = Instant::now();
        let shift_before = crate::profile::solver_perf_snapshot();
        // An off-shift may restore mood; crossing the cycle boundary cannot.
        let first_assignment = (index == 2).then(|| shifts[0].assignment.clone());
        let first_shift = first_assignment
            .as_ref()
            .map(|assignment| (assignment, shift_hours[0]));
        let first_consumption: HashMap<String, f64> = first_shift
            .map(|(assignment, duration)| {
                shift_eta_with_instances(&mood_model, blueprint, assignment, instances)
                    .per_op
                    .into_iter()
                    .map(|op| (op.name, op.drain_per_hour * duration))
                    .collect()
            })
            .unwrap_or_default();
        let admission = Some((&rotation_mood, &first_consumption));
        // 活跃两队的中枢干员名册
        let active_names: HashSet<String> = active
            .iter()
            .flat_map(|t| team_ctrl.get(t).cloned().unwrap_or_default())
            .collect();
        control_options.mood = control_mood_for_shift(&rotation_mood, &active_names, options.mood);
        // 从活跃两队池分配中枢；不足五人是不可行排班，不能静默留下空席。
        let mut assign_ctrl =
            |a: &mut BaseAssignment, used: &mut HashSet<String>, extra_control_pins: &[&str]| {
                let original = a.clone();
                let mut pending = VecDeque::from([HashSet::<String>::new()]);
                let mut attempted = HashSet::new();
                let mut last_failure = None;
                while let Some(rejected) = pending.pop_front() {
                    let mut key: Vec<_> = rejected.iter().cloned().collect();
                    key.sort();
                    if attempted.contains(&key) {
                        continue;
                    }
                    if attempted.len() >= 64 {
                        let mut budget_detail = String::new();
                        match last_failure.take() {
                            Some(AbcControlCandidateFailure::Eta(name)) => {
                                budget_detail = format!("; last ETA bottleneck={name}");
                                *eta_failure =
                                    Some(AbcRotationEtaFailure::for_operator(&peak_plan, &name));
                            }
                            Some(AbcControlCandidateFailure::Other(error)) => return Err(error),
                            None => {}
                        }
                        return Err(Error::msg(format!(
                            "control rotation ETA candidate budget exhausted in shift {}{}",
                            index + 1,
                            budget_detail
                        )));
                    }
                    attempted.insert(key);
                    *a = original.clone();
                    let control_admission =
                        control_closure.admission(index, &active, extra_control_pins)?;
                    let final_pool = control_admission.prepare(a, &rejected)?;
                    *used = a.operator_names();
                    let current_layout = crate::layout::resolve_control_input_layout(
                        blueprint,
                        a,
                        instances,
                        table,
                        control_options.mood,
                        Some(durin_plan),
                    )?;
                    let seed = a.clone();
                    let control_eta_rejection = std::sync::Mutex::new(None);
                    let fits_control = |names: &[String]| {
                        let mut trial = seed.clone();
                        trial.set_room(
                            "control",
                            names
                                .iter()
                                .map(|name| {
                                    let entry = final_pool
                                        .entry(name)
                                        .expect("search member from admitted pool");
                                    AssignedOperator::from_progress(name, entry.progress)
                                })
                                .collect(),
                        );
                        let failed = nonproduction_eta_failure(
                            &mood_model,
                            blueprint,
                            instances,
                            &trial,
                            &rotation_mood,
                            hours,
                            first_shift,
                            Some(FacilityKind::ControlCenter),
                        );
                        if let Some(name) = &failed {
                            *control_eta_rejection.lock().expect("local ETA state") =
                                Some(name.clone());
                        }
                        failed.is_none()
                    };
                    if let Err(error) = crate::layout::assign_control_with_saileach(
                        a,
                        &final_pool,
                        table,
                        &current_layout,
                        &control_options,
                        &control_admission.requirements(),
                        &HashSet::new(),
                        None,
                        control_admission.required_names().contains("琴柳"),
                        Some(&fits_control),
                        used,
                    ) {
                        if rejected.is_empty() {
                            *eta_failure = control_eta_rejection
                                .into_inner()
                                .expect("local ETA state")
                                .map(|name| AbcRotationEtaFailure::for_operator(&peak_plan, &name));
                            return Err(error);
                        }
                        last_failure = Some(AbcControlCandidateFailure::Other(error));
                        continue;
                    }
                    if let Err(error) = control_admission.validate(a) {
                        if rejected.is_empty() {
                            return Err(error);
                        }
                        last_failure = Some(AbcControlCandidateFailure::Other(error));
                        continue;
                    }
                    *used = a.operator_names();

                    let mut early_failure = nonproduction_eta_failure(
                        &mood_model,
                        blueprint,
                        instances,
                        a,
                        &rotation_mood,
                        hours,
                        first_shift,
                        Some(FacilityKind::ControlCenter),
                    );
                    if early_failure.is_none() {
                        let mut office_eta_failure = None;
                        let office_result = assign_office_for_shift_with_mood(
                            a,
                            blueprint,
                            &active_bound_office_names(&bound_office_team, &active),
                            &bound_office_names,
                            &office_ranked_pool
                                .iter()
                                .filter(|candidate| {
                                    control_closure
                                        .team_of(&candidate.name)
                                        .is_none_or(|team| active.contains(&team))
                                })
                                .cloned()
                                .collect::<Vec<_>>(),
                            hours,
                            Some(instances),
                            admission,
                            Some(&mut office_eta_failure),
                        );
                        match office_result {
                            Ok(()) => {
                                assign_rotating_meetings_with_context(
                                    a,
                                    blueprint,
                                    &active,
                                    resting,
                                    &rotating_meeting_team,
                                    &meeting_pool,
                                    &reserved,
                                    Some(&FacilityRotationEligibility {
                                        worked_hours: &meeting_worked_hours,
                                        shift_hours: hours,
                                    }),
                                    Some(instances),
                                    admission,
                                    &layout,
                                )?;
                                assign_rotating_recycle(
                                    a,
                                    blueprint,
                                    &active,
                                    &rotating_recycle_team,
                                    &recycle_skill_pool,
                                    Some(&FacilityRotationEligibility {
                                        worked_hours: &meeting_worked_hours,
                                        shift_hours: hours,
                                    }),
                                    instances,
                                    admission,
                                );
                            }
                            Err(_) if office_eta_failure.is_some() => {
                                early_failure = office_eta_failure;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    let failed = early_failure.or_else(|| {
                        nonproduction_eta_failure(
                            &mood_model,
                            blueprint,
                            instances,
                            a,
                            &rotation_mood,
                            hours,
                            first_shift,
                            None,
                        )
                    });
                    if let Some(failed) = failed {
                        let mut alternatives: Vec<String> = a
                            .control_operators()
                            .into_iter()
                            .map(|op| op.name)
                            .filter(|name| {
                                !control_admission.required_names().contains(name)
                                    && !rejected.contains(name)
                            })
                            .collect();
                        alternatives.sort();
                        alternatives.sort_by_key(|name| name != &failed);
                        for name in alternatives.into_iter().rev() {
                            let mut branch = rejected.clone();
                            branch.insert(name);
                            pending.push_front(branch);
                        }
                        last_failure = Some(AbcControlCandidateFailure::Eta(failed));
                        continue;
                    }
                    return Ok::<(), Error>(());
                }
                match last_failure {
                    Some(AbcControlCandidateFailure::Eta(name)) => {
                        *eta_failure = Some(AbcRotationEtaFailure::for_operator(&peak_plan, &name));
                        Err(Error::msg(format!(
                            "control rotation ETA cannot cover shift {} for {}",
                            index + 1,
                            name
                        )))
                    }
                    Some(AbcControlCandidateFailure::Other(error)) => Err(error),
                    None => Err(Error::msg(format!(
                        "control rotation has no candidates in shift {}",
                        index + 1
                    ))),
                }
            };

        let mut assignment;
        if index == 1 && abyssal_policy == AbyssalShortShiftPolicy::Opportunistic {
            // ── S2 深海候选优先 ──
            // 路径 A: 无深海（基线）
            let mut base = shared.clone();
            for part in parts {
                merge_rooms(&mut base, part);
            }
            assign_rotating_anchors(&mut base, &peak, &active, &rotating_anchor_team)?;
            clear_room(&mut base, "control");
            let mut base_used = base.operator_names();
            assign_ctrl(&mut base, &mut base_used, &[])?;
            let mut base_warmup_rooms = warmup_sticky_rooms.clone();
            align_warmup_rooms(blueprint, instances, &mut base, &mut base_warmup_rooms);
            clear_production_efficiencies(blueprint, &mut base);
            let score_base = evaluate_base_assignment_efficiencies(
                blueprint,
                &base,
                instances,
                table,
                hours,
                Some(durin_plan),
            )?;

            // 路径 B: 有深海（S2 短班候选）
            let alpha_blocked_ops: HashSet<String> = operators_of(&alpha).into_iter().collect();
            let abyssal_candidates = build_abyssal_s2_candidates(&AbyssalBuildCtx {
                operbox,
                instances,
                table,
                blueprint,
                layout: &gamma_h1_layout,
                options,
                manu_pool: &pools.manu,
                used_ab: &used_ab,
                blocked_ops: &alpha_blocked_ops,
                shared: &shared,
                beta: &beta,
                gamma_h1: &gamma_h1,
                mutable_manu_rooms: &h1.manu,
                required_manu_by_room: &required_manu_by_room,
                admitted_control_producer: branch_producer.as_deref(),
            });
            let mut best_abyssal: Option<(
                AbyssalCandidate,
                ShiftEfficiencies,
                HashMap<String, RoomId>,
            )> = None;
            for mut candidate in abyssal_candidates {
                assign_rotating_anchors(
                    &mut candidate.assignment,
                    &peak,
                    &active,
                    &rotating_anchor_team,
                )?;
                clear_room(&mut candidate.assignment, "control");
                let mut aby_used = candidate.assignment.operator_names();
                let control_producer = candidate.control_producer.clone();
                assign_ctrl(
                    &mut candidate.assignment,
                    &mut aby_used,
                    &[control_producer.as_str()],
                )?;
                let mut candidate_warmup_rooms = warmup_sticky_rooms.clone();
                align_warmup_rooms(
                    blueprint,
                    instances,
                    &mut candidate.assignment,
                    &mut candidate_warmup_rooms,
                );
                if !candidate
                    .assignment
                    .control_operators()
                    .iter()
                    .any(|operator| operator.name == candidate.control_producer)
                {
                    return Err(Error::msg(
                        "short-shift tagged manufacture candidate lost its control producer pin",
                    ));
                }
                clear_production_efficiencies(blueprint, &mut candidate.assignment);
                let score_aby = evaluate_base_assignment_efficiencies(
                    blueprint,
                    &candidate.assignment,
                    instances,
                    table,
                    hours,
                    Some(durin_plan),
                )?;
                let replace = best_abyssal.as_ref().is_none_or(|(_, best, _)| {
                    score_aby.manufacture_efficiency > best.manufacture_efficiency
                });
                if replace {
                    best_abyssal = Some((candidate, score_aby, candidate_warmup_rooms));
                }
            }
            let (best_assignment, best_scores, best_warmup_rooms) =
                if let Some((candidate, score_aby, candidate_warmup_rooms)) = best_abyssal {
                    let alpha_beta: HashSet<String> = teams
                        .iter()
                        .filter(|team| matches!(team.label, TeamLabel::Alpha | TeamLabel::Beta))
                        .flat_map(|team| team.operators.iter().cloned())
                        .collect();
                    if let Some(team) = teams.iter_mut().find(|team| team.label == TeamLabel::Gamma)
                    {
                        let mut ops = team.operators.clone();
                        ops.extend(candidate.gamma_ops.clone());
                        ops.sort();
                        ops.dedup();
                        ops.retain(|name| !alpha_beta.contains(name));
                        team.operators = ops;
                    }
                    (candidate.assignment, score_aby, candidate_warmup_rooms)
                } else {
                    (base, score_base, base_warmup_rooms)
                };
            assignment = best_assignment;
            print_trade_rotation_debug("S2 final candidate", &assignment);
            let scores = best_scores;
            warmup_sticky_rooms = best_warmup_rooms;
            record_meeting_usage_hours(blueprint, &assignment, &mut meeting_worked_hours, hours);
            let weighted_trade = scores.weighted_trade_for_cycle(hours, cycle_hours);
            let weighted_manufacture = scores.weighted_manufacture_for_cycle(hours, cycle_hours);
            let weighted_power = scores.weighted_power_for_cycle(hours, cycle_hours);
            shifts.push(TeamShiftResult {
                index,
                plan_index: 0,
                duration_hours: hours,
                active_teams: active.to_vec(),
                resting_team: resting,
                assignment,
                fiammetta: None,
                dorm_rest: None,
                drone: drone_action(blueprint, ShiftTransition::ReplaceRooms),
                transition: ShiftTransition::ReplaceRooms,
                efficiencies: scores,
                weighted_trade,
                weighted_manufacture,
                weighted_power,
            });
        } else {
            // ── S1 / S3 正常组装 ──
            assignment = shared.clone();
            for part in parts {
                merge_rooms(&mut assignment, part);
            }
            print_trade_rotation_debug("S1/S3 merged before extras", &assignment);
            assign_rotating_anchors(&mut assignment, &peak, &active, &rotating_anchor_team)?;
            clear_room(&mut assignment, "control");
            let mut s13_used = assignment.operator_names();
            assign_ctrl(&mut assignment, &mut s13_used, &[])?;
            record_meeting_usage_hours(blueprint, &assignment, &mut meeting_worked_hours, hours);
            align_warmup_rooms(
                blueprint,
                instances,
                &mut assignment,
                &mut warmup_sticky_rooms,
            );
            print_trade_rotation_debug("S1/S3 before final score", &assignment);
            clear_production_efficiencies(blueprint, &mut assignment);
            let scores = evaluate_base_assignment_efficiencies(
                blueprint,
                &assignment,
                instances,
                table,
                hours,
                Some(durin_plan),
            )?;
            let weighted_trade = scores.weighted_trade_for_cycle(hours, cycle_hours);
            let weighted_manufacture = scores.weighted_manufacture_for_cycle(hours, cycle_hours);
            let weighted_power = scores.weighted_power_for_cycle(hours, cycle_hours);
            shifts.push(TeamShiftResult {
                index,
                plan_index: 0,
                duration_hours: hours,
                active_teams: active.to_vec(),
                resting_team: resting,
                assignment,
                fiammetta: None,
                dorm_rest: None,
                drone: drone_action(blueprint, ShiftTransition::ReplaceRooms),
                transition: ShiftTransition::ReplaceRooms,
                efficiencies: scores,
                weighted_trade,
                weighted_manufacture,
                weighted_power,
            });
        }
        let assignment = &shifts.last().expect("shift just appended").assignment;
        if let Some(name) = nonproduction_eta_failure(
            &mood_model,
            blueprint,
            instances,
            assignment,
            &rotation_mood,
            hours,
            first_shift,
            None,
        ) {
            *eta_failure = Some(AbcRotationEtaFailure::for_operator(&peak_plan, &name));
            return Err(Error::msg(format!(
                "nonproduction rotation ETA cannot cover shift {} for {}",
                index + 1,
                name
            )));
        }
        advance_rotation_mood(
            &mood_model,
            blueprint,
            instances,
            assignment,
            hours,
            full_rest,
            &mut rotation_mood,
        );
        let shift_after = crate::profile::solver_perf_snapshot();
        crate::profile::emit_solver_stage(
            "rotation_abc",
            match index {
                0 => "shift_1",
                1 => "shift_2",
                _ => "shift_3",
            },
            "ok",
            shift_started.elapsed(),
            shift_before,
            shift_after,
        );
    }

    let mut report = TeamRotationReport {
        profile,
        assignment_plans: vec![peak_plan.clone()],
        peak_plan: peak_plan.clone(),
        runtime_shift_binds: shift_binds.clone(),
        peak_mood_eta,
        teams: teams.clone(),
        daily: DailyTotals {
            trade: shifts.iter().map(|shift| shift.weighted_trade).sum(),
            manufacture: shifts.iter().map(|shift| shift.weighted_manufacture).sum(),
            power: shifts.iter().map(|shift| shift.weighted_power).sum(),
            production: average_cycle_production(blueprint, &shifts),
        },
        warnings: rotation_shortfall_warnings(blueprint, &shifts),
        shifts,
        elapsed: t0.elapsed(),
    };

    // Publish actual work cohorts rather than the unused candidate directory.
    let actual_work: HashSet<_> = report
        .shifts
        .iter()
        .flat_map(|shift| shift.assignment.iter_rooms())
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|spec| spec.kind != FacilityKind::Dormitory)
        })
        .flat_map(|room| &room.operators)
        .map(|op| op.name.clone())
        .collect();
    for team in &mut report.teams {
        team.operators.retain(|name| actual_work.contains(name));
    }

    if enable_fiammetta_return {
        for shift in &report.shifts {
            print_trade_rotation_debug(
                &format!("before fiammetta shift {}", shift.index + 1),
                &shift.assignment,
            );
        }
        let applied_single_core = match apply_profile_fiammetta_rotation(
            blueprint,
            operbox,
            instances,
            table,
            options,
            profile,
            &report.teams,
            &report.peak_plan.continuous_roles,
            &report.runtime_shift_binds,
            &mut report.shifts,
        ) {
            Ok(applied) => applied,
            Err(error) => return Err(Error::with_rotation(&report, error)),
        };
        for shift in &report.shifts {
            print_trade_rotation_debug(
                &format!("after single-core fiammetta shift {}", shift.index + 1),
                &shift.assignment,
            );
        }
        let _applied_witch = if applied_single_core {
            false
        } else {
            match apply_witch_group_fiammetta_rotation(
                blueprint,
                operbox,
                instances,
                table,
                options,
                profile,
                &report.runtime_shift_binds,
                &mut report.shifts,
            ) {
                Ok(applied) => applied,
                Err(error) => return Err(Error::with_rotation(&report, error)),
            }
        };
        for shift in &report.shifts {
            print_trade_rotation_debug(
                &format!("after all fiammetta shift {}", shift.index + 1),
                &shift.assignment,
            );
        }
    }
    report.daily = DailyTotals {
        trade: report.shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: report
            .shifts
            .iter()
            .map(|shift| shift.weighted_manufacture)
            .sum(),
        power: report.shifts.iter().map(|shift| shift.weighted_power).sum(),
        production: average_cycle_production(blueprint, &report.shifts),
    };
    report.warnings = rotation_shortfall_warnings(blueprint, &report.shifts);
    let t4 = Instant::now();
    let m4 = crate::profile::solver_perf_snapshot();

    fn ms(a: Instant, b: Instant) -> f64 {
        a.duration_since(b).as_secs_f64() * 1000.0
    }

    eprintln!(
        "[计时] 轮换·peak班={:.2}ms  resolve+建池+切半={:.2}ms  γ替补={:.2}ms  三班评分={:.2}ms  轮换总计={:.2}ms",
        ms(t1, t0), ms(t2, t1), ms(t3, t2), ms(t4, t3), ms(t4, t0),
    );
    for (stage, elapsed, before, after) in [
        ("peak", t1.duration_since(t0), m0, m1),
        ("shared_pool_split", t2.duration_since(t1), m1, m2),
        ("gamma", t3.duration_since(t2), m2, m3),
        ("shifts_and_postprocess", t4.duration_since(t3), m3, m4),
        ("total", t4.duration_since(t0), m0, m4),
    ] {
        crate::profile::emit_solver_stage("rotation_abc", stage, "ok", elapsed, before, after);
    }

    report.elapsed = t4.duration_since(t0);
    Ok(report)
}

#[derive(Debug, Clone)]
struct OwnedRotationAssignment {
    assignment: BaseAssignment,
    plan: AssignmentPlan,
}

enum ProfileOwnerCandidate {
    Ready(OwnedRotationAssignment),
    Unavailable(&'static str),
}

#[derive(Debug, Clone)]
struct RotationStateSpec {
    owner_index: usize,
    duration_hours: f64,
    active_teams: Vec<TeamLabel>,
    resting_team: TeamLabel,
    fiammetta: Option<FiammettaShiftAction>,
    dorm_rest: Option<DormRestPlan>,
    transition: ShiftTransition,
}

fn sorted_assignment_names(assignment: &BaseAssignment) -> Vec<String> {
    let mut names: Vec<_> = assignment.operator_names().into_iter().collect();
    names.sort();
    names
}

fn active_team_names(teams: &[TeamAssignment], active: &[TeamLabel]) -> HashSet<String> {
    teams
        .iter()
        .filter(|team| active.contains(&team.label))
        .flat_map(|team| team.operators.iter().cloned())
        .collect()
}

fn solve_owned_peak_assignment(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: f64,
) -> Result<OwnedRotationAssignment> {
    let mut shift_options = options.clone();
    shift_options.shift_hours = shift_hours;
    let mut result = assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        &shift_options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
        &HashSet::new(),
    )?;
    result.plan.resolve_actual_producer_dependencies(
        blueprint,
        &result.assignment,
        instances,
        table,
    )?;
    Ok(OwnedRotationAssignment {
        assignment: result.assignment,
        plan: result.plan,
    })
}

fn solve_policy_disjoint_backup(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    main: &BaseAssignment,
    shift_hours: f64,
) -> Result<OwnedRotationAssignment> {
    // Two-team policy: the backup state is generated from the operators not
    // present anywhere in the complete main assignment.
    let unavailable = main.operator_names();
    let available = operbox.excluding(&unavailable);
    solve_owned_peak_assignment(
        blueprint,
        &available,
        instances,
        table,
        options,
        shift_hours,
    )
    .map_err(|error| {
        Error::msg(format!(
            "no solution under current-main complete-state-disjoint backup policy: {error}"
        ))
    })
}

fn trade_post_room_for_operator(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    operator: &str,
) -> Option<RoomId> {
    assignment.iter_rooms().find_map(|room| {
        (blueprint
            .room(&room.room_id)
            .is_some_and(|spec| spec.kind == FacilityKind::TradePost)
            && room
                .operators
                .iter()
                .any(|assigned| assigned.name == operator))
        .then(|| room.room_id.clone())
    })
}

fn main_backup_24_24_fiammetta_targets(
    blueprint: &BaseBlueprint,
    owner: &OwnedRotationAssignment,
) -> Vec<String> {
    let binds = shift_binds_from_plan(&owner.plan);
    fiammetta_return_priority(blueprint)
        .into_iter()
        .filter(|target| {
            fiammetta_target_is_legal(TimedRotationProfile::MainBackup24_24, target)
                && !has_multi_member_shift_bind(target, &binds)
        })
        .filter(|target| {
            trade_post_room_for_operator(blueprint, &owner.assignment, target).is_some()
        })
        .map(str::to_owned)
        .collect()
}

fn main_backup_24_24_fiammetta_actions(
    blueprint: &BaseBlueprint,
    main: &OwnedRotationAssignment,
    backup: &OwnedRotationAssignment,
    first_target: &str,
    second_target: &str,
) -> Option<[Option<FiammettaShiftAction>; 2]> {
    let first_room = trade_post_room_for_operator(blueprint, &main.assignment, first_target)?;
    let second_room = trade_post_room_for_operator(blueprint, &backup.assignment, second_target)?;
    Some([
        Some(FiammettaShiftAction {
            target: first_target.to_owned(),
            displaced: String::new(),
            room_id: first_room,
        }),
        Some(FiammettaShiftAction {
            target: second_target.to_owned(),
            displaced: String::new(),
            room_id: second_room,
        }),
    ])
}

fn main_backup_24_24_fiammetta_single_action(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    target: &str,
) -> Option<FiammettaShiftAction> {
    Some(FiammettaShiftAction {
        target: target.to_owned(),
        displaced: String::new(),
        room_id: trade_post_room_for_operator(blueprint, assignment, target)?,
    })
}

fn main_backup_24_24_probe_report(
    main: &OwnedRotationAssignment,
    backup: &OwnedRotationAssignment,
    actions: &[Option<FiammettaShiftAction>; 2],
) -> TeamRotationReport {
    let make_shift = |index: usize, owner: &OwnedRotationAssignment| TeamShiftResult {
        index,
        plan_index: index,
        duration_hours: 24.0,
        active_teams: vec![if index == 0 {
            TeamLabel::Alpha
        } else {
            TeamLabel::Beta
        }],
        resting_team: if index == 0 {
            TeamLabel::Beta
        } else {
            TeamLabel::Alpha
        },
        assignment: owner.assignment.clone(),
        fiammetta: actions[index].clone(),
        dorm_rest: None,
        drone: None,
        transition: ShiftTransition::ReplaceRooms,
        efficiencies: ShiftEfficiencies::default(),
        weighted_trade: Efficiency::ZERO,
        weighted_manufacture: Efficiency::ZERO,
        weighted_power: Efficiency::ZERO,
    };
    let assignment_plans = vec![main.plan.clone(), backup.plan.clone()];
    TeamRotationReport {
        profile: TimedRotationProfile::MainBackup24_24,
        peak_plan: main.plan.clone(),
        runtime_shift_binds: assignment_plans
            .iter()
            .flat_map(shift_binds_from_plan)
            .collect(),
        assignment_plans,
        peak_mood_eta: None,
        teams: vec![
            TeamAssignment {
                label: TeamLabel::Alpha,
                operators: sorted_assignment_names(&main.assignment),
            },
            TeamAssignment {
                label: TeamLabel::Beta,
                operators: sorted_assignment_names(&backup.assignment),
            },
        ],
        shifts: vec![make_shift(0, main), make_shift(1, backup)],
        daily: DailyTotals::default(),
        warnings: Vec::new(),
        elapsed: std::time::Duration::ZERO,
    }
}

fn validate_main_backup_24_24_fiammetta_candidate(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    options: &AssignBaseOptions,
    main: &OwnedRotationAssignment,
    backup: &OwnedRotationAssignment,
    actions: &[Option<FiammettaShiftAction>; 2],
) -> Result<()> {
    if !actions.iter().any(Option::is_some) {
        return Err(Error::msg(
            "fiammetta_24_24_candidate requires at least one action",
        ));
    }
    let targets: HashSet<_> = actions
        .iter()
        .filter_map(|action| action.as_ref().map(|action| action.target.as_str()))
        .collect();
    if targets.len() != actions.iter().filter(|action| action.is_some()).count() {
        return Err(Error::msg(
            "fiammetta_24_24_candidate requires distinct action targets",
        ));
    }
    let report = main_backup_24_24_probe_report(main, backup, actions);
    let mut repeated_shifts = Vec::with_capacity(report.shifts.len() * 2);
    for cycle in 0..2 {
        for shift in &report.shifts {
            let mut repeated = shift.clone();
            repeated.index += cycle * report.shifts.len();
            repeated_shifts.push(repeated);
        }
    }
    let model = default_mood_model()?;
    fiammetta_events::validate_actions(&repeated_shifts, &model, blueprint, instances)?;
    validate_cycle_mood(blueprint, instances, &report, options.mood)?;
    for action in actions.iter().flatten() {
        validate_operator_cycle_mood(blueprint, instances, &report, options.mood, &action.target)?;
    }
    Ok(())
}

/// 为单个 owner assignment 保留式补齐办公室空位。
///
/// - 不清除已有办公室成员（execute_plan / 体系可能已预占，如人间烟火桑葚）。
/// - 空槽位候选 = `office_candidates` 静态评分降序，排除 owner 已用、其他 owner
///   全 assignment、`plan.excluded_operators` 与 `plan.rotation_reserves`。
/// - 按当前 owner 的班次时长检查候选 ETA；不读取其他班次的办公室状态。
/// - 候选不足或全部不可持续时留空，缺额由 `rotation_shortfall_warnings` 报告。
#[allow(clippy::too_many_arguments)]
fn fill_owner_office_seats(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: f64,
    owner: &mut OwnedRotationAssignment,
    other_forbidden: &HashSet<String>,
) -> Result<()> {
    let has_office = blueprint
        .rooms
        .iter()
        .any(|room| room.kind == FacilityKind::Office);
    if !has_office {
        return Ok(());
    }
    let layout = crate::layout::resolve_base(
        blueprint,
        &owner.assignment,
        Some(instances),
        Some(table),
        options.mood,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?
    .layout_snapshot();
    let owner_used: HashSet<String> = owner.assignment.operator_names();
    let plan = &owner.plan;
    let mut excluded: HashSet<String> = owner_used
        .iter()
        .cloned()
        .chain(other_forbidden.iter().cloned())
        .chain(plan.excluded_operators.iter().cloned())
        .collect();
    for reserve in &plan.rotation_reserves {
        for operator in &reserve.operators {
            excluded.insert(operator.clone());
        }
    }
    let candidates = office_candidates(operbox, instances, table, &layout, options.mood, &excluded);
    assign_office_for_shift_with_mood(
        &mut owner.assignment,
        blueprint,
        &HashSet::new(),
        &HashSet::new(),
        &candidates,
        shift_hours,
        None,
        None,
        None,
    )?;
    Ok(())
}

fn repair_owner_meeting_seats(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    owner: &mut OwnedRotationAssignment,
    shift_hours: f64,
) -> Result<()> {
    let model = default_mood_model()?;
    let eta = shift_eta_with_instances(&model, blueprint, &owner.assignment, instances);
    let infeasible: HashSet<_> = eta
        .per_op
        .iter()
        .filter(|operator| {
            owner.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|spec| spec.kind == FacilityKind::MeetingRoom)
                    && room
                        .operators
                        .iter()
                        .any(|assigned| assigned.name == operator.name)
            }) && operator.drain_per_hour > 0.0
                && operator.drain_per_hour * shift_hours > model.workable_mood() + 1e-9
        })
        .map(|operator| operator.name.clone())
        .collect();
    if infeasible.is_empty() {
        return Ok(());
    }
    for name in &infeasible {
        if owner
            .plan
            .anchors
            .iter()
            .any(|anchor| anchor.operator == *name)
            || owner
                .plan
                .active_dependencies
                .iter()
                .any(|dependency| dependency.required.contains(name))
        {
            return Err(Error::msg(format!(
                "required meeting operator {name} cannot cover {shift_hours}h shift ETA"
            )));
        }
    }
    owner
        .assignment
        .remove_operators_where(|room_id, operator| {
            blueprint
                .room(room_id)
                .is_some_and(|spec| spec.kind == FacilityKind::MeetingRoom)
                && infeasible.contains(&operator.name)
        });
    let layout = resolve_base(
        blueprint,
        &owner.assignment,
        Some(instances),
        Some(table),
        24.0,
        Some(operbox.durin_dorm_planning_count(instances)),
    )?
    .layout_snapshot();
    let mut forbidden = owner.assignment.operator_names();
    forbidden.extend(owner.plan.excluded_operators.iter().cloned());
    forbidden.extend(
        owner
            .plan
            .rotation_reserves
            .iter()
            .flat_map(|reserve| reserve.operators.iter().cloned()),
    );
    let mut candidate_excluded = forbidden.clone();
    if owner.assignment.iter_rooms().any(|room| {
        blueprint
            .room(&room.room_id)
            .is_some_and(|spec| spec.kind == FacilityKind::MeetingRoom)
            && room
                .operators
                .iter()
                .any(|operator| operator.name == "忍冬" && operator.elite >= 2)
    }) {
        candidate_excluded.remove("忍冬");
    }
    let candidates = meeting_candidates(operbox, &layout, &candidate_excluded);
    let worked_hours = HashMap::new();
    assign_rotating_meetings_with_context(
        &mut owner.assignment,
        blueprint,
        &[],
        TeamLabel::Beta,
        &HashMap::new(),
        &candidates,
        &forbidden,
        Some(&FacilityRotationEligibility {
            worked_hours: &worked_hours,
            shift_hours,
        }),
        None,
        None,
        &layout,
    )
}

fn schedule_main_backup_12_12(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    schedule_main_backup_profile(
        TimedRotationProfile::MainBackup12_12,
        &[12.0, 12.0],
        blueprint,
        operbox,
        instances,
        table,
        options,
    )
    .map(|(report, _)| report)
}

fn schedule_main_backup_24_24(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<(TeamRotationReport, Option<MainBackup24FiammettaDiagnostic>)> {
    // 24/24 模式：过滤高心情消耗干员（drain >= 1.0）和德拉（互增撑不住24h）
    let filtered_operbox = filter_operbox_for_24_24(operbox);

    let mut profile_options = options.clone();
    profile_options.control_work_hours = Some(24.0);
    schedule_main_backup_profile(
        TimedRotationProfile::MainBackup24_24,
        &[24.0, 24.0],
        blueprint,
        &filtered_operbox,
        instances,
        table,
        &profile_options,
    )
}

#[derive(Debug)]
struct MainBackup24FiammettaDiagnostic {
    selected_targets: Vec<String>,
    search_path: Option<&'static str>,
    candidate_names: Vec<String>,
    rejection_counts: BTreeMap<&'static str, usize>,
    reentered_searches: usize,
}

fn append_main_backup_24_fiammetta_warning(
    report: &mut TeamRotationReport,
    diagnostic: MainBackup24FiammettaDiagnostic,
) {
    let final_actions: Vec<_> = report
        .shifts
        .iter()
        .enumerate()
        .filter_map(|(index, shift)| shift.fiammetta.as_ref().map(|action| (index, action)))
        .collect();
    let warning = match final_actions.as_slice() {
        [(shift_index, action)] => {
            let search_path = diagnostic.search_path.unwrap_or("ordinary_disjoint");
            Some(format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} fiammetta_24_24_targets: only one distinct target passed final checks; accepted_target={} accepted_shift={} accepted_room={} search_path={search_path}; candidate_names={:?}; rejected={:?}",
                action.target,
                shift_index + 1,
                action.room_id.0,
                diagnostic.candidate_names,
                diagnostic.rejection_counts
            ))
        }
        [] if !diagnostic.selected_targets.is_empty() => Some(format!(
            "{FINAL_INVARIANT_WARNING_PREFIX} fiammetta_24_24_targets: no selected action survived final checks; selected_targets={:?}; ordinary 24/24 assignments retained",
            diagnostic.selected_targets
        )),
        [] => Some(format!(
            "{FINAL_INVARIANT_WARNING_PREFIX} fiammetta_24_24_targets: NO_CANDIDATE_UNDER_POLICY; normal Trade candidate names={:?}; reentered backup searches={}; rejected={:?}; ordinary disjoint 24/24 assignments retained",
            diagnostic.candidate_names,
            diagnostic.reentered_searches,
            diagnostic.rejection_counts
        )),
        _ => None,
    };
    if let Some(warning) = warning {
        report.warnings.push(warning);
    }
}

/// 为 24/24 模式过滤 operbox：移除德克萨斯、拉普兰德，以及心情消耗增加≥0.5的干员
fn filter_operbox_for_24_24(operbox: &OperBox) -> OperBox {
    // 德拉特殊组合：两人互增 drain=1.9/h，24h 撑不住
    // drain_delta >= 0.5 的干员（从 mood_model.json 读取）
    const FORBIDDEN_NAMES_24_24: &[&str] = &[
        "德克萨斯",
        "拉普兰德",
        // operator_mood_skills 中 drain_delta >= 0.5 的干员
        "地灵",
        "夕",
        "怒潮凛冬",
        "惊蛰",
        "斥罪",
        "水灯心",
        "涤火杰西卡",
        "见行者",
        "重岳",
        "阿",
        "麒麟R夜刀",
        // todo 区特殊干员
        "歌蕾蒂娅",
    ];

    // entries 与 owned 必须同步：部分路径按 entry.own 筛候选，部分按 owned 判断拥有。
    let mut filtered_owned = operbox.owned.clone();
    for name in FORBIDDEN_NAMES_24_24 {
        filtered_owned.remove(*name);
    }
    let entries = operbox
        .entries
        .iter()
        .map(|entry| {
            let mut entry = entry.clone();
            if FORBIDDEN_NAMES_24_24.contains(&entry.name.as_str()) {
                entry.own = false;
            }
            entry
        })
        .collect();

    OperBox {
        entries,
        owned: filtered_owned,
    }
}

fn schedule_main_backup_profile(
    profile: TimedRotationProfile,
    state_hours: &[f64],
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<(TeamRotationReport, Option<MainBackup24FiammettaDiagnostic>)> {
    let started = Instant::now();
    let main_hours = state_hours[0];
    let backup_hours = state_hours[1];
    let fiammetta_enabled = options.fiammetta_enable
        && operbox.owns("菲亚梅塔")
        && !fiammetta_action_slots(profile, state_hours.len()).is_empty();
    let fiammetta_target_candidates =
        if profile == TimedRotationProfile::MainBackup24_24 && fiammetta_enabled {
            let normal_trade_pool =
                build_trade_pool(&operbox.trade_roster(instances), instances, table)?;
            fiammetta_return_priority(blueprint)
                .into_iter()
                .filter(|target| {
                    fiammetta_target_is_legal(TimedRotationProfile::MainBackup24_24, target)
                })
                .filter(|target| {
                    normal_trade_pool
                        .entries
                        .iter()
                        .any(|entry| entry.name == *target)
                })
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
    let mut main =
        solve_owned_peak_assignment(blueprint, operbox, instances, table, options, main_hours)?;
    fill_owner_office_seats(
        blueprint,
        operbox,
        instances,
        table,
        options,
        main_hours,
        &mut main,
        &HashSet::new(),
    )?;
    repair_owner_meeting_seats(blueprint, operbox, instances, table, &mut main, main_hours)?;
    let main_forbidden = main.assignment.operator_names();
    let mut selected_fiammetta_actions: Option<[Option<FiammettaShiftAction>; 2]> = None;
    let mut fiammetta_rejection_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut fiammetta_reentered_searches = 0;
    let mut fiammetta_action_search_path = None;
    let mut backup_candidate = None;

    if profile == TimedRotationProfile::MainBackup24_24 && fiammetta_enabled {
        let selected_main_targets = main_backup_24_24_fiammetta_targets(blueprint, &main);

        'reentered_targets: for first_target in &selected_main_targets {
            let mut backup_unavailable = main_forbidden.clone();
            backup_unavailable.remove(first_target);
            let candidate_operbox = operbox.excluding(&backup_unavailable);
            let candidate_trade_pool =
                build_trade_pool(&candidate_operbox.trade_roster(instances), instances, table)?;
            if !candidate_trade_pool
                .entries
                .iter()
                .any(|entry| entry.name == *first_target)
            {
                return Err(Error::msg(format!(
                    "fiammetta_24_24_candidate_pool failed: {first_target} was not preserved in the normal Trade pool"
                )));
            }
            eprintln!(
                "[fiammetta-24-24] reentered normal Trade candidate target={first_target}; candidate pool retained it"
            );

            fiammetta_reentered_searches += 1;
            let mut candidate_backup = match solve_owned_peak_assignment(
                blueprint,
                &candidate_operbox,
                instances,
                table,
                options,
                backup_hours,
            ) {
                Ok(assignment) => assignment,
                Err(error) => {
                    *fiammetta_rejection_counts
                        .entry("normal_backup_search_rejected_reentered_target")
                        .or_default() += 1;
                    eprintln!(
                        "[fiammetta-24-24] reentered target {first_target} rejected by normal backup search: {error}"
                    );
                    continue;
                }
            };
            if fill_owner_office_seats(
                blueprint,
                &candidate_operbox,
                instances,
                table,
                options,
                backup_hours,
                &mut candidate_backup,
                &main_forbidden,
            )
            .is_err()
                || repair_owner_meeting_seats(
                    blueprint,
                    &operbox.excluding(&main_forbidden),
                    instances,
                    table,
                    &mut candidate_backup,
                    backup_hours,
                )
                .is_err()
            {
                *fiammetta_rejection_counts
                    .entry("ordinary_support_fill_rejected_reentered_target")
                    .or_default() += 1;
                continue;
            }

            let selected_backup_targets =
                main_backup_24_24_fiammetta_targets(blueprint, &candidate_backup);
            eprintln!(
                "[fiammetta-24-24] normal backup Trade search result for reentered target={first_target}: legal selected targets={selected_backup_targets:?}"
            );
            for second_target in selected_backup_targets {
                if second_target == *first_target {
                    *fiammetta_rejection_counts
                        .entry("normal_trade_search_reused_first_target")
                        .or_default() += 1;
                    continue;
                }
                let Some(actions) = main_backup_24_24_fiammetta_actions(
                    blueprint,
                    &main,
                    &candidate_backup,
                    first_target,
                    &second_target,
                ) else {
                    *fiammetta_rejection_counts
                        .entry("normal_trade_search_did_not_select_target_in_own_shift")
                        .or_default() += 1;
                    continue;
                };
                if let Err(error) = validate_main_backup_24_24_fiammetta_candidate(
                    blueprint,
                    instances,
                    options,
                    &main,
                    &candidate_backup,
                    &actions,
                ) {
                    *fiammetta_rejection_counts
                        .entry("cycle_mood_or_ready_rejected_pair")
                        .or_default() += 1;
                    eprintln!(
                        "[fiammetta-24-24] pair {first_target} -> {second_target} rejected by cyclic mood / ready: {error}"
                    );
                    continue;
                }

                eprintln!(
                    "[fiammetta-24-24] selected normal Trade targets {first_target} -> {second_target}; each is assigned in its action shift and the full cycle passes mood / ready replay"
                );
                selected_fiammetta_actions = Some(actions);
                fiammetta_action_search_path = Some("backup_pool_reentry");
                backup_candidate = Some(candidate_backup);
                break 'reentered_targets;
            }

            if let Some(action) =
                main_backup_24_24_fiammetta_single_action(blueprint, &main.assignment, first_target)
            {
                let actions = [Some(action), None];
                match validate_main_backup_24_24_fiammetta_candidate(
                    blueprint,
                    instances,
                    options,
                    &main,
                    &candidate_backup,
                    &actions,
                ) {
                    Ok(()) => {
                        eprintln!(
                            "[fiammetta-24-24] selected one normal Trade target {first_target}; backup search result retained without forcing a second action"
                        );
                        selected_fiammetta_actions = Some(actions);
                        fiammetta_action_search_path = Some("backup_pool_reentry");
                        backup_candidate = Some(candidate_backup);
                        break 'reentered_targets;
                    }
                    Err(error) => {
                        *fiammetta_rejection_counts
                            .entry("single_reentered_target_rejected_by_cycle")
                            .or_default() += 1;
                        eprintln!(
                            "[fiammetta-24-24] single reentered target {first_target} rejected by cyclic mood / ready: {error}"
                        );
                    }
                }
            }
        }

        if backup_candidate.is_none() {
            let mut disjoint_backup = solve_policy_disjoint_backup(
                blueprint,
                operbox,
                instances,
                table,
                options,
                &main.assignment,
                backup_hours,
            )?;
            fill_owner_office_seats(
                blueprint,
                operbox,
                instances,
                table,
                options,
                backup_hours,
                &mut disjoint_backup,
                &main_forbidden,
            )?;
            let backup_operbox = operbox.excluding(&main_forbidden);
            repair_owner_meeting_seats(
                blueprint,
                &backup_operbox,
                instances,
                table,
                &mut disjoint_backup,
                backup_hours,
            )?;

            let disjoint_backup_targets =
                main_backup_24_24_fiammetta_targets(blueprint, &disjoint_backup);
            'disjoint_targets: for first_target in &selected_main_targets {
                for second_target in &disjoint_backup_targets {
                    if first_target == second_target {
                        continue;
                    }
                    let Some(actions) = main_backup_24_24_fiammetta_actions(
                        blueprint,
                        &main,
                        &disjoint_backup,
                        first_target,
                        second_target,
                    ) else {
                        continue;
                    };
                    if let Err(error) = validate_main_backup_24_24_fiammetta_candidate(
                        blueprint,
                        instances,
                        options,
                        &main,
                        &disjoint_backup,
                        &actions,
                    ) {
                        *fiammetta_rejection_counts
                            .entry("cycle_mood_or_ready_rejected_disjoint_pair")
                            .or_default() += 1;
                        eprintln!(
                            "[fiammetta-24-24] disjoint pair {first_target} -> {second_target} rejected by cyclic mood / ready: {error}"
                        );
                        continue;
                    }
                    eprintln!(
                        "[fiammetta-24-24] selected disjoint normal Trade targets {first_target} -> {second_target}; each is assigned in its action shift and the full cycle passes mood / ready replay"
                    );
                    selected_fiammetta_actions = Some(actions);
                    fiammetta_action_search_path = Some("ordinary_disjoint");
                    break 'disjoint_targets;
                }
            }

            if selected_fiammetta_actions.is_none() {
                'single_targets: for (index, candidates, assignment) in [
                    (0usize, &selected_main_targets, &main.assignment),
                    (
                        1usize,
                        &disjoint_backup_targets,
                        &disjoint_backup.assignment,
                    ),
                ] {
                    for target in candidates {
                        let Some(action) = main_backup_24_24_fiammetta_single_action(
                            blueprint, assignment, target,
                        ) else {
                            continue;
                        };
                        let mut actions = [None, None];
                        actions[index] = Some(action);
                        match validate_main_backup_24_24_fiammetta_candidate(
                            blueprint,
                            instances,
                            options,
                            &main,
                            &disjoint_backup,
                            &actions,
                        ) {
                            Ok(()) => {
                                selected_fiammetta_actions = Some(actions);
                                fiammetta_action_search_path = Some("ordinary_disjoint");
                                break 'single_targets;
                            }
                            Err(error) => {
                                *fiammetta_rejection_counts
                                    .entry("single_disjoint_target_rejected_by_cycle")
                                    .or_default() += 1;
                                eprintln!(
                                    "[fiammetta-24-24] single disjoint target {target} rejected by cyclic mood / ready: {error}"
                                );
                            }
                        }
                    }
                }
            }

            backup_candidate = Some(disjoint_backup);
        }
    }

    let selected_backup = backup_candidate.take();
    let backup_was_prepared = selected_backup.is_some();
    let mut backup = match selected_backup {
        Some(backup) => backup,
        None => solve_policy_disjoint_backup(
            blueprint,
            operbox,
            instances,
            table,
            options,
            &main.assignment,
            backup_hours,
        )?,
    };
    if !backup_was_prepared {
        fill_owner_office_seats(
            blueprint,
            operbox,
            instances,
            table,
            options,
            backup_hours,
            &mut backup,
            &main_forbidden,
        )?;
        let backup_operbox = operbox.excluding(&main_forbidden);
        repair_owner_meeting_seats(
            blueprint,
            &backup_operbox,
            instances,
            table,
            &mut backup,
            backup_hours,
        )?;
    }
    let teams = vec![
        TeamAssignment {
            label: TeamLabel::Alpha,
            operators: sorted_assignment_names(&main.assignment),
        },
        TeamAssignment {
            label: TeamLabel::Beta,
            operators: sorted_assignment_names(&backup.assignment),
        },
    ];
    let (first_fiammetta, second_fiammetta) = if profile == TimedRotationProfile::MainBackup24_24 {
        selected_fiammetta_actions
            .as_ref()
            .map(|actions| (actions[0].clone(), actions[1].clone()))
            .unwrap_or((None, None))
    } else {
        let first = fiammetta_enabled
            .then(|| {
                first_fiammetta_assignment_target(
                    profile,
                    blueprint,
                    &main.assignment,
                    &HashSet::new(),
                )
                .map(|(target, room_id)| FiammettaShiftAction {
                    target,
                    displaced: String::new(),
                    room_id,
                })
            })
            .flatten();
        let second = fiammetta_enabled
            .then(|| {
                let used = first
                    .as_ref()
                    .map(|action| std::iter::once(action.target.clone()).collect())
                    .unwrap_or_default();
                first_fiammetta_assignment_target(profile, blueprint, &backup.assignment, &used)
                    .map(|(target, room_id)| FiammettaShiftAction {
                        target,
                        displaced: String::new(),
                        room_id,
                    })
            })
            .flatten();
        (first, second)
    };
    let states = state_hours
        .iter()
        .enumerate()
        .map(|(index, duration_hours)| {
            let main_active = index % 2 == 0;
            RotationStateSpec {
                owner_index: index % 2,
                duration_hours: *duration_hours,
                active_teams: vec![if main_active {
                    TeamLabel::Alpha
                } else {
                    TeamLabel::Beta
                }],
                resting_team: if main_active {
                    TeamLabel::Beta
                } else {
                    TeamLabel::Alpha
                },
                fiammetta: if main_active {
                    first_fiammetta.clone()
                } else {
                    second_fiammetta.clone()
                },
                dorm_rest: None,
                transition: ShiftTransition::ReplaceRooms,
            }
        })
        .collect();
    let report = finalize_profile_rotation(
        profile,
        blueprint,
        operbox,
        instances,
        table,
        vec![main, backup],
        teams,
        states,
        started,
    )?;
    let fiammetta_24_diagnostic = (profile == TimedRotationProfile::MainBackup24_24
        && fiammetta_enabled)
        .then(|| MainBackup24FiammettaDiagnostic {
            selected_targets: selected_fiammetta_actions
                .as_ref()
                .into_iter()
                .flatten()
                .flatten()
                .map(|action| action.target.clone())
                .collect(),
            search_path: fiammetta_action_search_path,
            candidate_names: fiammetta_target_candidates,
            rejection_counts: fiammetta_rejection_counts,
            reentered_searches: fiammetta_reentered_searches,
        });
    Ok((report, fiammetta_24_diagnostic))
}

fn schedule_ordinary_profile_fallback(
    profile: TimedRotationProfile,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    reason: &str,
) -> Result<TeamRotationReport> {
    let state_hours: &[f64] = match profile {
        TimedRotationProfile::Abyssal7_5_7_5 => &[7.0, 5.0, 7.0, 5.0],
        _ => {
            return Err(Error::msg(
                "ordinary profile fallback requires a named special profile",
            ))
        }
    };
    let (mut report, _) = schedule_main_backup_profile(
        profile,
        state_hours,
        blueprint,
        operbox,
        instances,
        table,
        options,
    )?;
    report.warnings.push(format!(
        "{FINAL_INVARIANT_WARNING_PREFIX} {reason}; special claims unavailable, ordinary assignments retain requested {} timing",
        profile.display_name(),
    ));
    Ok(report)
}

fn schedule_fiammetta_8_8_4_4(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let started = Instant::now();
    let mut long_options = options.clone();
    long_options.shift_hours = 16.0;
    let raw = schedule_abc_rotation(
        blueprint,
        operbox,
        instances,
        table,
        &long_options,
        [16.0, 4.0, 4.0],
        false,
        AbyssalShortShiftPolicy::Disabled,
        TimedRotationProfile::Abc12_6_6,
        &[],
        &[],
    )?;
    let binds = raw.runtime_shift_binds.clone();
    let fiammetta_owner = raw.shifts[0].assignment.clone();
    let owner_assignments = vec![
        fiammetta_owner,
        raw.shifts[1].assignment.clone(),
        raw.shifts[2].assignment.clone(),
    ];
    let owners: Vec<_> = owner_assignments
        .into_iter()
        .map(|assignment| OwnedRotationAssignment {
            assignment,
            plan: raw.peak_plan.clone(),
        })
        .collect();
    let states = vec![
        RotationStateSpec {
            owner_index: 0,
            duration_hours: 8.0,
            active_teams: raw.shifts[0].active_teams.clone(),
            resting_team: raw.shifts[0].resting_team,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 0,
            duration_hours: 8.0,
            active_teams: raw.shifts[0].active_teams.clone(),
            resting_team: raw.shifts[0].resting_team,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 1,
            duration_hours: 4.0,
            active_teams: raw.shifts[1].active_teams.clone(),
            resting_team: raw.shifts[1].resting_team,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 2,
            duration_hours: 4.0,
            active_teams: raw.shifts[2].active_teams.clone(),
            resting_team: raw.shifts[2].resting_team,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
    ];
    let mut report = finalize_profile_rotation(
        TimedRotationProfile::Fiammetta8_8_4_4,
        blueprint,
        operbox,
        instances,
        table,
        owners,
        raw.teams,
        states,
        started,
    )?;
    report.runtime_shift_binds = binds;
    if options.fiammetta_enable {
        let applied = match apply_profile_fiammetta_rotation(
            blueprint,
            operbox,
            instances,
            table,
            options,
            TimedRotationProfile::Fiammetta8_8_4_4,
            &report.teams,
            &raw.peak_plan.continuous_roles,
            &report.runtime_shift_binds,
            &mut report.shifts,
        ) {
            Ok(applied) => applied,
            Err(error) => return Err(Error::with_rotation(&report, error)),
        };
        if !applied {
            report.warnings.push(format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} fiammetta-8844 has no accepted Fiammetta action; ordinary 8/8/4/4 assignments retained"
            ));
        }
        for shift in &mut report.shifts {
            if let Some(action) = shift.fiammetta.as_ref() {
                shift.transition = ShiftTransition::FiammettaOnly;
                shift.drone = None;
                shift.efficiencies = match score_fiammetta_continuation(
                    blueprint,
                    &shift.assignment,
                    instances,
                    table,
                    operbox.durin_dorm_planning_count(instances),
                    &action.room_id,
                ) {
                    Ok(scores) => scores,
                    Err(error) => return Err(Error::with_rotation(&report, error)),
                };
                shift.weighted_trade = shift.efficiencies.weighted_trade(shift.duration_hours);
                shift.weighted_manufacture = shift
                    .efficiencies
                    .weighted_manufacture(shift.duration_hours);
                shift.weighted_power = shift.efficiencies.weighted_power(shift.duration_hours);
            }
        }
        report.daily = DailyTotals {
            trade: report.shifts.iter().map(|shift| shift.weighted_trade).sum(),
            manufacture: report
                .shifts
                .iter()
                .map(|shift| shift.weighted_manufacture)
                .sum(),
            power: report.shifts.iter().map(|shift| shift.weighted_power).sum(),
            production: average_cycle_production(blueprint, &report.shifts),
        };
    }
    Ok(report)
}

fn choose_four(items: &[String]) -> Vec<Vec<String>> {
    fn visit(
        items: &[String],
        start: usize,
        current: &mut Vec<String>,
        out: &mut Vec<Vec<String>>,
    ) {
        if current.len() == 4 {
            out.push(current.clone());
            return;
        }
        let needed = 4 - current.len();
        for index in start..=items.len().saturating_sub(needed) {
            current.push(items[index].clone());
            visit(items, index + 1, current, out);
            current.pop();
        }
    }

    let mut out = Vec::new();
    if items.len() >= 4 {
        visit(items, 0, &mut Vec::new(), &mut out);
    }
    out
}

fn enumerate_capacity_counts(
    remaining: usize,
    capacities: &[usize],
    current: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if current.len() == capacities.len() {
        if remaining == 0 {
            out.push(current.clone());
        }
        return;
    }
    let capacity = capacities[current.len()];
    for count in 0..=remaining.min(capacity) {
        current.push(count);
        enumerate_capacity_counts(remaining - count, capacities, current, out);
        current.pop();
    }
}

fn augment_abyssal_profile_plan(
    plan: &mut AssignmentPlan,
    producer: &str,
    control_room_id: &RoomId,
    placements: &[(String, RoomId, Option<crate::types::RecipeKind>)],
    operbox: &OperBox,
) -> Result<()> {
    const SYSTEM_ID: &str = "timed_rotation_abyssal_7_5_7_5";
    let mut operators = vec![producer.to_string()];
    operators.extend(placements.iter().map(|(name, _, _)| name.clone()));
    plan.selected_rules.insert(
        0,
        SelectedRuleAlternative {
            rule_id: SYSTEM_ID.to_string(),
            alternative_id: "required_four_consumers".to_string(),
            conditional_pack_id: None,
            priority: i32::MAX,
            operators: operators.clone(),
        },
    );
    let producer_progress = operbox
        .progress_of(producer)
        .ok_or_else(|| Error::msg("abyssal profile producer disappeared from operbox"))?;
    let mut required = vec![SystemAnchor {
        system_id: SYSTEM_ID.to_string(),
        operator: producer.to_string(),
        proactive_support: false,
        optional_trade_role: None,
        optional_control_role: None,
        elite: producer_progress.elite,
        facility: FacilityKind::ControlCenter,
        room_id: Some(control_room_id.clone()),
        recipe: None,
        trade_order: None,
        work_mood: None,
        rest_facility: None,
        fill_policy: AnchorFillPolicy::Plain,
    }];
    for (operator, room_id, recipe) in placements {
        let progress = operbox.progress_of(operator).ok_or_else(|| {
            Error::msg(format!(
                "abyssal profile consumer {operator} disappeared from operbox"
            ))
        })?;
        required.push(SystemAnchor {
            system_id: SYSTEM_ID.to_string(),
            operator: operator.clone(),
            proactive_support: false,
            optional_trade_role: None,
            optional_control_role: None,
            elite: progress.elite,
            facility: FacilityKind::Factory,
            room_id: Some(room_id.clone()),
            recipe: *recipe,
            trade_order: None,
            work_mood: None,
            rest_facility: None,
            fill_policy: AnchorFillPolicy::ManufactureRecipe,
        });
    }
    // The planning preclaim has already forced ordinary selection around these
    // placements. Prepend the same claims so execute_plan owns and proves them.
    required.extend(plan.anchors.clone());
    plan.anchors = required;
    plan.shift_binds.push(ShiftBind {
        operators,
        on_shifts: 2,
        off_shifts: 2,
    });
    Ok(())
}

fn solve_abyssal_profile_main(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<ProfileOwnerCandidate> {
    let empty = HashSet::new();
    let Some(producer) = abyssal_control_producer(operbox, instances, &empty) else {
        return Ok(ProfileOwnerCandidate::Unavailable(
            "abyssal-7575 requires a control producer with the Abyssal capability",
        ));
    };
    let hunters = owned_abyssal_hunters(operbox, instances, &empty);
    if hunters.len() < 4 {
        return Ok(ProfileOwnerCandidate::Unavailable(
            "abyssal-7575 requires at least four tag-matched Abyssal factory consumers",
        ));
    }
    let Some(control_room) = blueprint
        .rooms
        .iter()
        .find(|room| room.kind == FacilityKind::ControlCenter)
    else {
        return Ok(ProfileOwnerCandidate::Unavailable(
            "abyssal-7575 requires a control center",
        ));
    };
    let factories: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Factory)
        .collect();
    let capacities: Vec<_> = factories
        .iter()
        .map(|room| room.operator_capacity())
        .collect();
    if capacities.iter().sum::<usize>() < 4 {
        return Ok(ProfileOwnerCandidate::Unavailable(
            "abyssal-7575 requires factory capacity for four consumers",
        ));
    }
    let mut distributions = Vec::new();
    enumerate_capacity_counts(4, &capacities, &mut Vec::new(), &mut distributions);
    let mut profile_options = options.clone();
    profile_options.shift_hours = 7.0;
    profile_options.skip_standalone_control = true;
    let mut best: Option<(OwnedRotationAssignment, Efficiency, Vec<String>)> = None;

    for group in choose_four(&hunters) {
        for counts in &distributions {
            let mut planning_seed = BaseAssignment::default();
            planning_seed.set_room(
                control_room.id.clone(),
                vec![AssignedOperator::from_progress(
                    &producer,
                    operbox.progress_of(&producer).expect("producer checked"),
                )],
            );
            let mut placements = Vec::new();
            let mut next = 0;
            for (room, count) in factories.iter().zip(counts) {
                if *count == 0 {
                    continue;
                }
                let names = &group[next..next + count];
                next += count;
                let operators: Vec<_> = names
                    .iter()
                    .map(|name| {
                        AssignedOperator::from_progress(
                            name,
                            operbox.progress_of(name).expect("consumer checked"),
                        )
                    })
                    .collect();
                planning_seed.set_room(room.id.clone(), operators);
                let recipe = match room.product.as_ref() {
                    Some(RoomProduct::Factory { recipe }) => Some(*recipe),
                    _ => None,
                };
                placements.extend(
                    names
                        .iter()
                        .map(|name| (name.clone(), room.id.clone(), recipe)),
                );
            }
            let producer_for_plan = producer.clone();
            let control_room_for_plan = control_room.id.clone();
            let placements_for_plan = placements.clone();
            let result = assign_shift_with_preclaimed_plan(
                blueprint,
                operbox,
                instances,
                table,
                &profile_options,
                AssignShiftMode::Peak,
                &planning_seed,
                &BaseAssignment::default(),
                &HashSet::new(),
                |plan| {
                    augment_abyssal_profile_plan(
                        plan,
                        &producer_for_plan,
                        &control_room_for_plan,
                        &placements_for_plan,
                        operbox,
                    )
                },
            );
            let Ok(mut result) = result else {
                continue;
            };
            result.plan.resolve_actual_producer_dependencies(
                blueprint,
                &result.assignment,
                instances,
                table,
            )?;
            if !profile_anchors_present(blueprint, &result.assignment, &result.plan) {
                continue;
            }
            let mut scored = result.assignment.clone();
            clear_production_efficiencies(blueprint, &mut scored);
            let scores = evaluate_base_assignment_efficiencies(
                blueprint,
                &scored,
                instances,
                table,
                7.0,
                operbox.durin_dorm_planning_count(instances).into(),
            )?;
            let stable = sorted_assignment_names(&result.assignment);
            let replace = best.as_ref().is_none_or(|(_, best_score, best_stable)| {
                scores.manufacture_efficiency > *best_score
                    || (scores.manufacture_efficiency == *best_score && stable < *best_stable)
            });
            if replace {
                best = Some((
                    OwnedRotationAssignment {
                        assignment: result.assignment,
                        plan: result.plan,
                    },
                    scores.manufacture_efficiency,
                    stable,
                ));
            }
        }
    }
    Ok(best.map_or(
        ProfileOwnerCandidate::Unavailable(
            "abyssal-7575 hard claims could not be combined with a complete legal assignment",
        ),
        |(owner, _, _)| ProfileOwnerCandidate::Ready(owner),
    ))
}

fn plan_abyssal_dorm_rest(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    main: &BaseAssignment,
    backup: &BaseAssignment,
) -> Result<Option<DormRestPlan>> {
    let model = default_mood_model()?;
    let producer = main
        .control_operators()
        .into_iter()
        .find(|operator| {
            instances
                .resolve_control_buff_ids(&operator.name, operator.tier())
                .iter()
                .any(|buff_id| buff_id == ABYSSAL_CONTROL_BETA_BUFF)
        })
        .ok_or_else(|| Error::msg("abyssal dorm plan lost its control producer"))?;
    let producer_eta = shift_eta_with_instances(&model, blueprint, main, instances)
        .per_op
        .into_iter()
        .find(|operator| operator.name == producer.name)
        .ok_or_else(|| Error::msg("abyssal dorm plan cannot resolve producer mood drain"))?;
    let required_recovery = producer_eta.drain_per_hour * 7.0;
    let active_backup: HashSet<_> = backup
        .iter_rooms()
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|room| room.kind != FacilityKind::Dormitory)
        })
        .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
        .collect();

    let mut single_managers = Vec::new();
    let mut group_managers = Vec::new();
    for (name, progress) in operbox
        .owned
        .iter()
        .filter(|(name, _)| name.as_str() != producer.name.as_str())
        .filter(|(name, _)| !active_backup.contains(*name))
    {
        let skills = model.dorm_skills(name, progress.elite);
        if let Some(value) = skills
            .iter()
            .filter(|skill| skill.is_single())
            .map(|skill| skill.value)
            .max_by(|left, right| left.total_cmp(right))
        {
            single_managers.push((name.clone(), progress.elite, value));
        }
        if let Some(value) = skills
            .iter()
            .filter_map(|skill| {
                let mut value = skill.is_group().then_some(skill.value);
                if skill.is_manager() {
                    if let Some(group) = skill.also_group {
                        value = Some(value.map_or(group, |current| current.max(group)));
                    }
                }
                value
            })
            .max_by(|left, right| left.total_cmp(right))
        {
            group_managers.push((name.clone(), progress.elite, value));
        }
    }
    single_managers.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    group_managers.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut best: Option<(DormRestPlan, f64, String)> = None;
    for single in &single_managers {
        for group in &group_managers {
            if single.0 == group.0 {
                continue;
            }
            for dorm in blueprint.rooms_of(FacilityKind::Dormitory) {
                let mut existing: Vec<_> = backup
                    .operators_in(&dorm.id)
                    .iter()
                    .filter(|operator| {
                        operator.name != single.0
                            && operator.name != group.0
                            && operator.name != producer.name
                    })
                    .cloned()
                    .collect();
                if existing.len() + 3 > dorm.dorm_bed_count().max(1) as usize {
                    continue;
                }
                let mut occupants = vec![
                    DormOccupant {
                        name: single.0.clone(),
                        elite: single.1,
                        drain_hint: 0.0,
                    },
                    DormOccupant {
                        name: group.0.clone(),
                        elite: group.1,
                        drain_hint: 0.0,
                    },
                    DormOccupant {
                        name: producer.name.clone(),
                        elite: producer.elite,
                        drain_hint: producer_eta.drain_per_hour,
                    },
                ];
                occupants.extend(existing.drain(..).map(|operator| DormOccupant {
                    name: operator.name,
                    elite: operator.elite,
                    drain_hint: 0.0,
                }));
                let rates = dorm_recovery_rates(&model, dorm.dorm_skill_level(), &occupants);
                let recovery = rates.get(&producer.name).copied().unwrap_or(0.0) * 5.0;
                if recovery + 1e-9 < required_recovery {
                    continue;
                }
                let stable = format!("{}:{}:{}", dorm.id.0, single.0, group.0);
                let replace = best.as_ref().is_none_or(|(_, best_recovery, best_stable)| {
                    recovery > *best_recovery
                        || (recovery == *best_recovery && stable < *best_stable)
                });
                if replace {
                    best = Some((
                        DormRestPlan {
                            room_id: dorm.id.clone(),
                            target: producer.name.clone(),
                            single_manager: single.0.clone(),
                            group_manager: group.0.clone(),
                        },
                        recovery,
                        stable,
                    ));
                }
            }
        }
    }
    Ok(best.map(|(plan, _, _)| plan))
}

fn schedule_abyssal_7_5_7_5(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let started = Instant::now();
    let mut main = match solve_abyssal_profile_main(blueprint, operbox, instances, table, options)?
    {
        ProfileOwnerCandidate::Ready(owner) => owner,
        ProfileOwnerCandidate::Unavailable(reason) => {
            return schedule_ordinary_profile_fallback(
                TimedRotationProfile::Abyssal7_5_7_5,
                blueprint,
                operbox,
                instances,
                table,
                options,
                reason,
            );
        }
    };
    fill_owner_office_seats(
        blueprint,
        operbox,
        instances,
        table,
        options,
        7.0,
        &mut main,
        &HashSet::new(),
    )?;
    let main_forbidden: HashSet<String> = main.assignment.operator_names();
    let mut backup = solve_policy_disjoint_backup(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &main.assignment,
        5.0,
    )?;
    fill_owner_office_seats(
        blueprint,
        operbox,
        instances,
        table,
        options,
        5.0,
        &mut backup,
        &main_forbidden,
    )?;
    let dorm_rest = plan_abyssal_dorm_rest(
        blueprint,
        operbox,
        instances,
        &main.assignment,
        &backup.assignment,
    )?;
    let teams = vec![
        TeamAssignment {
            label: TeamLabel::Alpha,
            operators: sorted_assignment_names(&main.assignment),
        },
        TeamAssignment {
            label: TeamLabel::Beta,
            operators: sorted_assignment_names(&backup.assignment),
        },
    ];
    let states = vec![
        RotationStateSpec {
            owner_index: 0,
            duration_hours: 7.0,
            active_teams: vec![TeamLabel::Alpha],
            resting_team: TeamLabel::Beta,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 1,
            duration_hours: 5.0,
            active_teams: vec![TeamLabel::Beta],
            resting_team: TeamLabel::Alpha,
            fiammetta: None,
            dorm_rest: dorm_rest.clone(),
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 0,
            duration_hours: 7.0,
            active_teams: vec![TeamLabel::Alpha],
            resting_team: TeamLabel::Beta,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 1,
            duration_hours: 5.0,
            active_teams: vec![TeamLabel::Beta],
            resting_team: TeamLabel::Alpha,
            fiammetta: None,
            dorm_rest: dorm_rest.clone(),
            transition: ShiftTransition::ReplaceRooms,
        },
    ];
    let mut report = finalize_profile_rotation(
        TimedRotationProfile::Abyssal7_5_7_5,
        blueprint,
        operbox,
        instances,
        table,
        vec![main, backup],
        teams,
        states,
        started,
    )?;
    if dorm_rest.is_none() {
        report.warnings.push(format!(
            "{FINAL_INVARIANT_WARNING_PREFIX} abyssal-7575 requires one single-target and one group dorm manager who can fully recover the control producer during each 5h rest; no unproven recovery group emitted"
        ));
    }
    Ok(report)
}

fn profile_anchors_present(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    plan: &AssignmentPlan,
) -> bool {
    plan.anchors
        .iter()
        .filter(|anchor| anchor.system_id.starts_with("timed_rotation_"))
        .all(|anchor| {
            let Some(room_id) = &anchor.room_id else {
                return false;
            };
            blueprint
                .room(room_id)
                .is_some_and(|room| room.kind == anchor.facility)
                && assignment
                    .operators_in(room_id)
                    .iter()
                    .any(|operator| operator.name == anchor.operator)
        })
}

fn continuation_efficiency(total: Efficiency, first: Efficiency) -> Efficiency {
    (total.scale_ratio(16, 1) - first.scale_ratio(8, 1)).scale_ratio(1, 8)
}

fn continuation_room_line(
    total: &RoomEfficiencyLine,
    first: &RoomEfficiencyLine,
) -> RoomEfficiencyLine {
    RoomEfficiencyLine {
        room_id: total.room_id.clone(),
        operator_count: total.operator_count,
        operator_efficiency: continuation_efficiency(
            total.operator_efficiency,
            first.operator_efficiency,
        ),
        base_efficiency: continuation_efficiency(total.base_efficiency, first.base_efficiency),
        station_efficiency: continuation_efficiency(
            total.station_efficiency,
            first.station_efficiency,
        ),
        global_efficiency: continuation_efficiency(
            total.global_efficiency,
            first.global_efficiency,
        ),
        total_efficiency: continuation_efficiency(total.total_efficiency, first.total_efficiency),
        final_efficiency: continuation_efficiency(total.final_efficiency, first.final_efficiency),
        order_multiplier: total.order_multiplier,
        equivalent_efficiency: continuation_efficiency(
            total.equivalent_efficiency,
            first.equivalent_efficiency,
        ),
        trade_output_per_day: total.trade_output_per_day,
        gold_consumption_per_day: total.gold_consumption_per_day,
        originium_shards_per_day: total.originium_shards_per_day,
        manufacture_unit_output_per_day: total.manufacture_unit_output_per_day,
        manufacture_output_per_day: total.manufacture_output_per_day,
        trade_equivalent_efficiency: continuation_efficiency(
            total.trade_equivalent_efficiency,
            first.trade_equivalent_efficiency,
        ),
        gold_equivalent_efficiency: continuation_efficiency(
            total.gold_equivalent_efficiency,
            first.gold_equivalent_efficiency,
        ),
        combined_output_value: continuation_efficiency(
            total.combined_output_value,
            first.combined_output_value,
        ),
        trade_efficiency: continuation_efficiency(total.trade_efficiency, first.trade_efficiency),
        trade_skill_efficiency: continuation_efficiency(
            total.trade_skill_efficiency,
            first.trade_skill_efficiency,
        ),
        manufacture_efficiency: continuation_efficiency(
            total.manufacture_efficiency,
            first.manufacture_efficiency,
        ),
        manufacture_skill_efficiency: continuation_efficiency(
            total.manufacture_skill_efficiency,
            first.manufacture_skill_efficiency,
        ),
        power_efficiency: continuation_efficiency(total.power_efficiency, first.power_efficiency),
        power_skill_efficiency: continuation_efficiency(
            total.power_skill_efficiency,
            first.power_skill_efficiency,
        ),
    }
}

fn score_fiammetta_continuation(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    durin_plan: u8,
    target_room: &RoomId,
) -> Result<ShiftEfficiencies> {
    let mut clean = assignment.clone();
    clear_production_efficiencies(blueprint, &mut clean);
    let first = evaluate_base_assignment_efficiencies(
        blueprint,
        &clean,
        instances,
        table,
        8.0,
        Some(durin_plan),
    )?;
    let total = evaluate_base_assignment_efficiencies(
        blueprint,
        &clean,
        instances,
        table,
        16.0,
        Some(durin_plan),
    )?;
    let first_by_room: HashMap<_, _> = first
        .room_lines
        .iter()
        .map(|line| (line.room_id.as_str(), line))
        .collect();
    let room_lines: Vec<_> = total
        .room_lines
        .iter()
        .map(|line| {
            let first_line = first_by_room
                .get(line.room_id.as_str())
                .expect("8h and 16h room sets match");
            if line.room_id == target_room.0 {
                (*first_line).clone()
            } else {
                continuation_room_line(line, first_line)
            }
        })
        .collect();
    Ok(ShiftEfficiencies {
        trade_efficiency: room_lines.iter().map(|line| line.trade_efficiency).sum(),
        manufacture_efficiency: room_lines
            .iter()
            .map(|line| line.manufacture_efficiency)
            .sum(),
        power_efficiency: crate::power::aggregate_power_efficiency(
            room_lines.iter().map(|line| line.power_efficiency),
        ),
        room_lines,
    })
}

#[allow(clippy::too_many_arguments)]
fn finalize_profile_rotation(
    profile: TimedRotationProfile,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    owners: Vec<OwnedRotationAssignment>,
    teams: Vec<TeamAssignment>,
    states: Vec<RotationStateSpec>,
    started: Instant,
) -> Result<TeamRotationReport> {
    if owners.is_empty() || states.is_empty() {
        return Err(Error::msg("timed rotation requires assignments and states"));
    }
    let cycle_hours: f64 = states.iter().map(|state| state.duration_hours).sum();
    if !cycle_hours.is_finite() || cycle_hours <= 0.0 {
        return Err(Error::msg(
            "timed rotation requires a finite positive cycle duration",
        ));
    }
    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mut shifts = Vec::with_capacity(states.len());
    let proposals: Vec<Option<FiammettaShiftAction>> =
        states.iter().map(|state| state.fiammetta.clone()).collect();
    for (index, state) in states.iter().enumerate() {
        let owner = owners.get(state.owner_index).ok_or_else(|| {
            Error::msg(format!(
                "rotation state {} references missing plan owner {}",
                index + 1,
                state.owner_index
            ))
        })?;
        let mut assignment = owner.assignment.clone();
        clear_production_efficiencies(blueprint, &mut assignment);
        let scores = if state.transition == ShiftTransition::FiammettaOnly {
            let action = state.fiammetta.as_ref().ok_or_else(|| {
                Error::msg("Fiammetta-only transition requires a scheduled action")
            })?;
            score_fiammetta_continuation(
                blueprint,
                &assignment,
                instances,
                table,
                durin_plan,
                &action.room_id,
            )?
        } else {
            evaluate_base_assignment_efficiencies(
                blueprint,
                &assignment,
                instances,
                table,
                state.duration_hours,
                Some(durin_plan),
            )?
        };
        shifts.push(TeamShiftResult {
            index,
            plan_index: state.owner_index,
            duration_hours: state.duration_hours,
            active_teams: state.active_teams.clone(),
            resting_team: state.resting_team,
            assignment,
            fiammetta: None,
            dorm_rest: state.dorm_rest.clone(),
            drone: drone_action(blueprint, state.transition),
            transition: state.transition,
            weighted_trade: scores.weighted_trade_for_cycle(state.duration_hours, cycle_hours),
            weighted_manufacture: scores
                .weighted_manufacture_for_cycle(state.duration_hours, cycle_hours),
            weighted_power: scores.weighted_power_for_cycle(state.duration_hours, cycle_hours),
            efficiencies: scores,
        });
    }
    retry_profile_fiammetta_slots(blueprint, instances, &proposals, &mut shifts)?;
    let daily = DailyTotals {
        trade: shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: shifts.iter().map(|shift| shift.weighted_manufacture).sum(),
        power: shifts.iter().map(|shift| shift.weighted_power).sum(),
        production: average_cycle_production(blueprint, &shifts),
    };
    let warnings = rotation_shortfall_warnings(blueprint, &shifts);
    let assignment_plans: Vec<_> = owners.iter().map(|owner| owner.plan.clone()).collect();
    let runtime_shift_binds = assignment_plans
        .iter()
        .flat_map(shift_binds_from_plan)
        .collect();
    let peak_plan = assignment_plans[0].clone();
    let peak_mood_eta = Some(shift_eta_with_instances(
        default_mood_model()?,
        blueprint,
        &owners[0].assignment,
        instances,
    ));
    let report = TeamRotationReport {
        profile,
        peak_plan,
        assignment_plans,
        runtime_shift_binds,
        peak_mood_eta,
        teams,
        shifts,
        daily,
        warnings,
        elapsed: started.elapsed(),
    };
    Ok(report)
}

fn retry_profile_fiammetta_slots(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    proposals: &[Option<FiammettaShiftAction>],
    shifts: &mut [TeamShiftResult],
) -> Result<()> {
    let has_proposal = proposals.iter().any(Option::is_some);
    if !has_proposal {
        return Ok(());
    }
    let model = default_mood_model()?;
    fiammetta_events::accept_proposals(shifts, proposals, &model, blueprint, instances);
    Ok(())
}

fn validate_mood_gap_decay_work_spans(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    profile: TimedRotationProfile,
    shifts: &[TeamShiftResult],
) -> Result<()> {
    let sensitive_names: HashSet<String> = shifts
        .iter()
        .flat_map(|shift| shift.assignment.iter_rooms())
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|room| room.kind == FacilityKind::Factory)
        })
        .flat_map(|room| &room.operators)
        .filter_map(|operator| {
            let manu_operator = crate::manufacture::ManuOperator {
                name: operator.name.clone(),
                elite: operator.elite,
                buff_ids: instances.resolve_manufacture_buff_ids(&operator.name, operator.tier()),
                tags: Vec::new(),
            };
            manufacture_operator_has_mood_gap_decay(&manu_operator, table)
                .then(|| operator.name.clone())
        })
        .collect();
    let works_in_factory = |shift: &TeamShiftResult, name: &str| {
        shift.assignment.iter_rooms().any(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|room| room.kind == FacilityKind::Factory)
                && room.operators.iter().any(|operator| operator.name == name)
        })
    };

    for name in sensitive_names {
        for start in 0..shifts.len() {
            let mut continuous_hours = 0.0;
            for offset in 0..shifts.len() {
                let shift = &shifts[(start + offset) % shifts.len()];
                if shift
                    .fiammetta
                    .as_ref()
                    .is_some_and(|action| action.target == name)
                {
                    continuous_hours = 0.0;
                }
                if !works_in_factory(shift, &name) {
                    continuous_hours = 0.0;
                    continue;
                }
                continuous_hours += shift.duration_hours;
                if continuous_hours >= MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS - 1e-9 {
                    return Err(Error::msg(format!(
                        "{name} is ineligible for {}: continuous manufacture work reaches {:.1}h",
                        profile.display_name(),
                        continuous_hours
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_profile_rotation(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
) -> Result<()> {
    for shift in &report.shifts {
        let plan = report
            .assignment_plans
            .get(shift.plan_index)
            .ok_or_else(|| Error::msg("rotation shift lost its plan owner"))?;
        let mut seen = HashSet::new();
        for room in shift.assignment.iter_rooms() {
            for operator in &room.operators {
                if !seen.insert(operator.name.clone()) {
                    return Err(Error::msg(format!(
                        "shift {} operator {} appears in multiple rooms",
                        shift.index + 1,
                        operator.name
                    )));
                }
            }
        }
        for room in &blueprint.rooms {
            if matches!(
                room.kind,
                FacilityKind::TradePost
                    | FacilityKind::Factory
                    | FacilityKind::PowerPlant
                    | FacilityKind::ControlCenter
            ) {
                let actual = shift.assignment.operators_in(&room.id).len();
                if actual > room.operator_capacity() {
                    return Err(Error::msg(format!(
                        "shift {} room {} has {actual} operators, exceeds capacity {}",
                        shift.index + 1,
                        room.id.0,
                        room.operator_capacity()
                    )));
                }
            } else if matches!(room.kind, FacilityKind::Office | FacilityKind::MeetingRoom) {
                // 办公室/会客室允许缺员（warnings 提示），但不得超过容量。
                let actual = shift.assignment.operators_in(&room.id).len();
                if actual > room.operator_capacity() {
                    return Err(Error::msg(format!(
                        "shift {} room {} exceeds capacity with {actual} operators",
                        shift.index + 1,
                        room.id.0
                    )));
                }
            }
        }
        if !profile_anchors_present(blueprint, &shift.assignment, plan) {
            return Err(Error::msg(format!(
                "shift {} does not satisfy its owning plan anchors",
                shift.index + 1
            )));
        }
        let present = shift.assignment.operator_names();
        for dependency in &plan.active_dependencies {
            if dependency
                .consumers
                .iter()
                .any(|consumer| present.contains(consumer))
            {
                let missing: Vec<_> = dependency
                    .required
                    .iter()
                    .filter(|required| !present.contains(*required))
                    .collect();
                if !missing.is_empty() {
                    return Err(Error::msg(format!(
                        "shift {} owner dependency {:?} missing {:?}",
                        shift.index + 1,
                        dependency.consumers,
                        missing
                    )));
                }
            }
        }
    }

    // 每个在岗普通成员必须恰好属于一个 Team；dorm_rest 引入、且不在任何
    // owner assignment 的临时宿管允许零标签（宿舍恢复元数据不新增 Team）。
    // 仅二班/深海启用：ABC/菲亚存在 shared 宿舍成员与菲亚事件覆盖，天然
    // 不属于 αβγ teams，不能套用该规则。
    if report.profile.is_two_team() {
        let fiammetta_targets = main_backup_24_24_fiammetta_targets_from_report(report);
        let team_by_name: HashMap<&str, Vec<TeamLabel>> = {
            let mut map: HashMap<&str, Vec<TeamLabel>> = HashMap::new();
            for team in &report.teams {
                for op in &team.operators {
                    map.entry(op.as_str()).or_default().push(team.label);
                }
            }
            map
        };
        for shift in &report.shifts[..2] {
            let dorm_names: HashSet<String> = shift
                .dorm_rest
                .as_ref()
                .map(|rest| {
                    [
                        rest.target.clone(),
                        rest.single_manager.clone(),
                        rest.group_manager.clone(),
                    ]
                    .into_iter()
                    .collect()
                })
                .unwrap_or_default();
            let fiammetta_dorm_action = shift.fiammetta.is_some()
                && blueprint
                    .rooms_of(FacilityKind::Dormitory)
                    .into_iter()
                    .any(|room| {
                        shift
                            .assignment
                            .operators_in(&room.id)
                            .iter()
                            .any(|operator| operator.name == FIAMMETTA_NAME)
                    });
            for operator in shift.assignment.operator_names() {
                let teams = team_by_name
                    .get(operator.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if operator == FIAMMETTA_NAME && fiammetta_dorm_action {
                    continue;
                }
                if fiammetta_targets.contains(&operator) {
                    let action_is_here = shift
                        .fiammetta
                        .as_ref()
                        .is_some_and(|action| action.target == operator);
                    let action_owner_matches =
                        shift.active_teams.iter().any(|team| teams.contains(team));
                    if action_is_here && !action_owner_matches {
                        return Err(Error::msg(format!(
                            "shift {} Fiammetta action target {operator} is not assigned to its action owner",
                            shift.index + 1
                        )));
                    }
                    continue;
                }
                match teams {
                    [] => {
                        if !dorm_names.contains(&operator) {
                            return Err(Error::msg(format!(
                                "shift {} operator {operator} belongs to no team",
                                shift.index + 1
                            )));
                        }
                    }
                    [_single] => {}
                    _ => {
                        return Err(Error::msg(format!(
                            "shift {} operator {operator} belongs to multiple teams",
                            shift.index + 1
                        )));
                    }
                }
            }
        }
    }

    if report.profile.is_two_team() {
        let fiammetta_targets = main_backup_24_24_fiammetta_targets_from_report(report);
        let alpha: HashSet<_> = report
            .teams
            .iter()
            .find(|team| team.label == TeamLabel::Alpha)
            .map(|team| team.operators.iter().cloned().collect())
            .unwrap_or_default();
        let beta: HashSet<_> = report
            .teams
            .iter()
            .find(|team| team.label == TeamLabel::Beta)
            .map(|team| team.operators.iter().cloned().collect())
            .unwrap_or_default();
        let alpha_work = report
            .shifts
            .first()
            .map(|shift| shift.assignment.work_position_operator_names(blueprint))
            .unwrap_or_default();
        let beta_work = report
            .shifts
            .get(1)
            .map(|shift| shift.assignment.work_position_operator_names(blueprint))
            .unwrap_or_default();
        let overlap: HashSet<_> = alpha
            .intersection(&beta)
            .filter(|operator| alpha_work.contains(*operator) && beta_work.contains(*operator))
            .cloned()
            .collect();
        let allowed_overlap: HashSet<_> = fiammetta_targets
            .intersection(&alpha_work)
            .filter(|operator| beta_work.contains(*operator))
            .cloned()
            .collect();
        if overlap != allowed_overlap {
            return Err(Error::msg(
                "two-team rotation violates work-position-disjoint policy",
            ));
        }
    }

    if report.profile == TimedRotationProfile::Abyssal7_5_7_5 {
        let producer_shift = &report.shifts[0];
        let producer = producer_shift
            .assignment
            .control_operators()
            .into_iter()
            .find(|operator| {
                instances
                    .resolve_control_buff_ids(&operator.name, operator.tier())
                    .iter()
                    .any(|buff_id| buff_id == ABYSSAL_CONTROL_BETA_BUFF)
            })
            .ok_or_else(|| Error::msg("abyssal profile lost its control producer"))?;
        let tagged_factory_count = producer_shift
            .assignment
            .rooms_of_kind(blueprint, FacilityKind::Factory)
            .flat_map(|room| &room.operators)
            .filter(|operator| {
                instances
                    .tags_for(&operator.name, operator.tier())
                    .iter()
                    .any(|tag| tag == TAG_ABYSSAL)
                    && operator.name != producer.name
            })
            .count();
        if tagged_factory_count != 4 {
            return Err(Error::msg(format!(
                "abyssal profile expected exactly four factory consumers, got {tagged_factory_count}"
            )));
        }
        for shift in report
            .shifts
            .iter()
            .filter(|shift| shift.duration_hours == 5.0)
        {
            let rest = shift.dorm_rest.as_ref().ok_or_else(|| {
                Error::msg("abyssal 5h recovery state lost its explicit dorm rest group")
            })?;
            if rest.target != producer.name
                || rest.single_manager == rest.group_manager
                || rest.single_manager == rest.target
                || rest.group_manager == rest.target
            {
                return Err(Error::msg("abyssal dorm rest group is not distinct"));
            }
            let room = blueprint
                .room(&rest.room_id)
                .ok_or_else(|| Error::msg("abyssal dorm rest room disappeared"))?;
            if room.kind != FacilityKind::Dormitory {
                return Err(Error::msg(
                    "abyssal dorm rest destination is not a dormitory",
                ));
            }
            let active_non_dorm: HashSet<_> = shift
                .assignment
                .iter_rooms()
                .filter(|assignment| {
                    blueprint
                        .room(&assignment.room_id)
                        .is_some_and(|room| room.kind != FacilityKind::Dormitory)
                })
                .flat_map(|assignment| {
                    assignment
                        .operators
                        .iter()
                        .map(|operator| operator.name.as_str())
                })
                .collect();
            if active_non_dorm.contains(rest.target.as_str())
                || active_non_dorm.contains(rest.single_manager.as_str())
                || active_non_dorm.contains(rest.group_manager.as_str())
            {
                return Err(Error::msg(
                    "abyssal dorm rest member is also active outside the dorm",
                ));
            }
        }
    }
    Ok(())
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
        default_layout_243_path, default_operbox_full_e2_path, default_operbox_gongsun_path,
        OperBoxEntry,
    };
    use crate::trade::TradeOrderKind;

    fn empty_rotation_report() -> TeamRotationReport {
        TeamRotationReport {
            profile: TimedRotationProfile::Abc12_12_12,
            peak_plan: crate::layout::AssignmentPlan::empty(),
            assignment_plans: vec![],
            runtime_shift_binds: vec![],
            peak_mood_eta: None,
            teams: vec![],
            shifts: vec![],
            daily: Default::default(),
            warnings: vec![],
            elapsed: Duration::from_millis(1),
        }
    }

    #[test]
    fn cycle_mood_rejection_with_report_downgrades_to_warning() {
        let message = "rotation_cycle_mood failed: 赫默 has no rest or recovery boundary";
        let error = Error::with_rotation(&empty_rotation_report(), message);
        let report = downgrade_cycle_mood_rejection(error).unwrap();
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].starts_with(FINAL_INVARIANT_WARNING_PREFIX));
        assert!(report.warnings[0].contains("赫默"));
    }

    #[test]
    fn non_budget_or_report_less_errors_keep_failing() {
        let other = Error::with_rotation(&empty_rotation_report(), "control: NO_CANDIDATE");
        assert!(downgrade_cycle_mood_rejection(other).is_err());
        let no_report = Error::msg("rotation_cycle_mood failed: 赫默 has no rest");
        assert!(downgrade_cycle_mood_rejection(no_report).is_err());
    }

    #[test]
    fn sui_control_mood_uses_active_operator_state() {
        let moods = HashMap::from([("令".to_string(), 11.5), ("夕".to_string(), 18.0)]);
        let active = HashSet::from(["令".to_string()]);
        assert_eq!(control_mood_for_shift(&moods, &active, 24.0), 11.5);
    }

    #[test]
    fn bound_peak_control_members_are_reserved_from_gamma_production() {
        let (blueprint, operbox, instances, table) = fixtures();
        let mut peak = BaseAssignment::default();
        peak.set_room(
            "control",
            vec![
                AssignedOperator::new("森蚺", 2),
                AssignedOperator::new("焰尾", 2),
                AssignedOperator::new("重岳", 2),
                AssignedOperator::new("未绑定中枢", 2),
            ],
        );
        peak.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("温蒂", 2),
                AssignedOperator::new("灰毫", 2),
            ],
        );
        peak.set_room("trade_1", vec![AssignedOperator::new("乌有", 2)]);
        let binds = vec![
            RuntimeShiftBind {
                operators: vec!["森蚺".into(), "温蒂".into()],
                on_shifts: 2,
                off_shifts: 1,
            },
            RuntimeShiftBind {
                operators: vec!["焰尾".into(), "灰毫".into()],
                on_shifts: 2,
                off_shifts: 1,
            },
            RuntimeShiftBind {
                operators: vec!["重岳".into(), "乌有".into()],
                on_shifts: 2,
                off_shifts: 1,
            },
        ];

        let reserved = bound_control_operators(&blueprint, &peak, &binds);
        assert_eq!(
            reserved,
            HashSet::from(["森蚺".to_string(), "焰尾".to_string(), "重岳".to_string()]),
            "自动化、红松和烟火等 exact bind 的中枢成员应走同一预留边界"
        );
        let pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        assert!(pool.entry("森蚺").is_some(), "森蚺原本确实是制造候选");
        let filtered = crate::pool::filter_manufacture_pool(&pool, &reserved);
        assert!(
            filtered.entry("森蚺").is_none(),
            "γ 生产池消费 used 后不得再次选择已绑定的 peak 中枢成员"
        );
    }

    #[test]
    fn sui_mood_propagates_work_and_rest_between_shifts() {
        let blueprint: BaseBlueprint = serde_json::from_str(
            r#"{"rooms":[{"id":"control","kind":"control_center","level":5}]}"#,
        )
        .expect("blueprint");
        let model = MoodModel::load_default().expect("mood model");
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("令", 2)]);
        let mut moods = HashMap::from([("令".to_string(), 24.0), ("夕".to_string(), 12.0)]);

        advance_sui_mood(
            &model,
            &blueprint,
            &assignment,
            &HashSet::from(["夕".to_string()]),
            6.0,
            &mut moods,
        );

        assert!(moods["令"] < 24.0, "在岗令应消耗心情");
        assert_eq!(moods["夕"], 24.0, "休息夕应按宿舍基础回复并封顶");
    }

    #[test]
    fn office_selection_recomputes_remaining_pool_for_each_shift() {
        let blueprint: BaseBlueprint = serde_json::from_str(
            r#"{"rooms":[
                {"id":"trade_1","kind":"trade_post","level":3},
                {"id":"office_any","kind":"office","level":3}
            ]}"#,
        )
        .expect("blueprint");
        let candidates = vec![
            AssignedOperator::new("办公室高效", 2),
            AssignedOperator::new("办公室次高效", 2),
        ];

        let mut first_shift = BaseAssignment::default();
        first_shift.set_room("trade_1", vec![candidates[0].clone()]);
        assign_office_for_shift(
            &mut first_shift,
            &blueprint,
            &HashSet::new(),
            &HashSet::new(),
            &candidates,
            6.0,
        )
        .expect("first office shift");
        assert_eq!(
            first_shift.operators_in(&RoomId::from("office_any"))[0].name,
            "办公室次高效"
        );

        let mut second_shift = BaseAssignment::default();
        second_shift.set_room("trade_1", vec![candidates[1].clone()]);
        assign_office_for_shift(
            &mut second_shift,
            &blueprint,
            &HashSet::new(),
            &HashSet::new(),
            &candidates,
            6.0,
        )
        .expect("second office shift");
        assert_eq!(
            second_shift.operators_in(&RoomId::from("office_any"))[0].name,
            "办公室高效"
        );
    }

    #[test]
    fn office_selection_skips_candidate_without_current_shift_eta() {
        let blueprint: BaseBlueprint = serde_json::from_str(
            r#"{"rooms":[
                {"id":"control","kind":"control_center","level":5},
                {"id":"office_any","kind":"office","level":3}
            ]}"#,
        )
        .expect("blueprint");
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            (0..5)
                .map(|index| AssignedOperator::new(format!("中枢{index}"), 0))
                .collect(),
        );
        let candidates = vec![
            AssignedOperator::new("地灵", 1),
            AssignedOperator::new("办公室替补", 2),
        ];

        assign_office_for_shift(
            &mut assignment,
            &blueprint,
            &HashSet::new(),
            &HashSet::new(),
            &candidates,
            12.0,
        )
        .expect("office replacement");
        assert_eq!(
            assignment.operators_in(&RoomId::from("office_any"))[0].name,
            "办公室替补"
        );

        let mut short_assignment = BaseAssignment::default();
        short_assignment.set_room(
            "control",
            (0..5)
                .map(|index| AssignedOperator::new(format!("中枢{index}"), 0))
                .collect(),
        );
        assign_office_for_shift(
            &mut short_assignment,
            &blueprint,
            &HashSet::new(),
            &HashSet::new(),
            &candidates[..1],
            6.0,
        )
        .expect("short office shift");
        assert_eq!(
            short_assignment.operators_in(&RoomId::from("office_any"))[0].name,
            "地灵"
        );
    }

    #[test]
    fn office_and_meeting_long_shift_reject_extra_mood_drains() {
        let office_blueprint: BaseBlueprint =
            serde_json::from_str(r#"{"rooms":[{"id":"office_any","kind":"office","level":3}]}"#)
                .expect("office blueprint");
        let meeting_blueprint: BaseBlueprint = serde_json::from_str(
            r#"{"rooms":[{"id":"meeting_any","kind":"meeting_room","level":3}]}"#,
        )
        .expect("meeting blueprint");
        let worked_hours = HashMap::new();
        let eligibility = FacilityRotationEligibility {
            worked_hours: &worked_hours,
            shift_hours: 12.0,
        };
        assert!(!candidate_fits_facility_shift(
            &BaseAssignment::default(),
            &office_blueprint,
            &RoomId::from("office_any"),
            &AssignedOperator::new("地灵", 1),
            Some(&eligibility),
        ));
        assert!(!candidate_fits_facility_shift(
            &BaseAssignment::default(),
            &meeting_blueprint,
            &RoomId::from("meeting_any"),
            &AssignedOperator::new("见行者", 2),
            Some(&eligibility),
        ));
    }

    #[test]
    fn office_candidates_score_from_support_registry_not_instances() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        let layout = resolve_base(
            &blueprint,
            &BaseAssignment::default(),
            Some(&instances),
            Some(&table),
            24.0,
            Some(0),
        )
        .unwrap()
        .layout_snapshot();
        let candidates =
            office_candidates(&operbox, &instances, &table, &layout, 24.0, &HashSet::new());
        let position = |name: &str| {
            candidates
                .iter()
                .position(|op| op.name == name)
                .unwrap_or_else(|| panic!("{name} missing from office candidates"))
        };
        // 斥罪（registry 固定 50%）必须排在闪击（instances 20%）之前；
        // 两者都必须在无技能兜底干员之前。
        let chizui = position("斥罪");
        let shanji = position("闪击");
        assert!(
            chizui < shanji,
            "斥罪(50%) must outrank 闪击(20%): candidates={:?}",
            candidates
                .iter()
                .take(10)
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            shanji < candidates.len().saturating_sub(1),
            "0-score operators must trail scored ones"
        );
    }

    #[test]
    fn rotating_nonproduction_anchor_follows_bound_cohort() {
        let mut peak = BaseAssignment::default();
        peak.set_room("dorm_any", vec![AssignedOperator::new("宿舍producer", 2)]);
        let teams = HashMap::from([("宿舍producer".to_string(), TeamLabel::Alpha)]);

        let mut active = BaseAssignment::default();
        assign_rotating_anchors(
            &mut active,
            &peak,
            &[TeamLabel::Alpha, TeamLabel::Beta],
            &teams,
        )
        .unwrap();
        assert!(active
            .operators_in(&RoomId::from("dorm_any"))
            .iter()
            .any(|operator| operator.name == "宿舍producer"));

        let mut resting = BaseAssignment::default();
        assign_rotating_anchors(
            &mut resting,
            &peak,
            &[TeamLabel::Beta, TeamLabel::Gamma],
            &teams,
        )
        .unwrap();
        assert!(resting.operators_in(&RoomId::from("dorm_any")).is_empty());
    }

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

    fn user_243_fixtures() -> (BaseBlueprint, OperBox, OperatorInstances, SkillTable) {
        let (_, operbox, instances, table) = fixtures();
        let blueprint = BaseBlueprint::load(&default_layout_243_path().unwrap()).unwrap();
        (blueprint, operbox, instances, table)
    }

    #[test]
    fn office_fill_excludes_plan_disabled_and_other_owner_members() {
        let (blueprint, operbox, instances, table) = fixtures();
        // 找到办公室评分最高的候选，验证把它同时放进 plan.excluded_operators
        // 与 backup forbidden 后不会进入 office。
        let mut owner = OwnedRotationAssignment {
            assignment: BaseAssignment::default(),
            plan: crate::layout::AssignmentPlan::empty(),
        };
        let layout = crate::layout::resolve_base(
            &blueprint,
            &owner.assignment,
            Some(&instances),
            Some(&table),
            24.0,
            Some(operbox.durin_dorm_planning_count(&instances)),
        )
        .unwrap()
        .layout_snapshot();
        let all_candidates =
            office_candidates(&operbox, &instances, &table, &layout, 24.0, &HashSet::new());
        let Some(top) = all_candidates.first() else {
            return; // 办公室候选为空时该 fixture 无法测禁用语义
        };
        let top_name = top.name.clone();
        owner.plan.excluded_operators.push(top_name.clone());
        // 同一干员也出现在 backup（其他 owner）全 assignment 中。
        let other_forbidden = HashSet::from([top_name.clone()]);
        fill_owner_office_seats(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            12.0,
            &mut owner,
            &other_forbidden,
        )
        .unwrap();
        for room in &blueprint.rooms {
            if room.kind == FacilityKind::Office {
                let names: Vec<_> = owner
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .map(|op| op.name.clone())
                    .collect();
                assert!(
                    !names.contains(&top_name),
                    "plan-disabled office candidate {top_name} must not be selected: {names:?}"
                );
            }
        }
    }

    #[test]
    fn timed_rotation_profile_parser_is_closed() {
        assert_eq!(
            "12/12/12".parse::<TimedRotationProfile>().unwrap(),
            TimedRotationProfile::Abc12_12_12
        );
        assert_eq!(
            "2".parse::<TimedRotationProfile>().unwrap(),
            TimedRotationProfile::MainBackup12_12
        );
        assert_eq!(
            "fiammetta-8844".parse::<TimedRotationProfile>().unwrap(),
            TimedRotationProfile::Fiammetta8_8_4_4
        );
        assert!("4".parse::<TimedRotationProfile>().is_err());
        assert!("5".parse::<TimedRotationProfile>().is_err());
    }

    #[test]
    fn abc_12_12_12_reuses_abc_team_rotation_for_a_36_hour_cycle() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                support_combinations_enable: true,
                ..Default::default()
            },
            TimedRotationProfile::Abc12_12_12,
        )
        .unwrap();

        assert_eq!(report.profile, TimedRotationProfile::Abc12_12_12);
        assert_eq!(
            report
                .shifts
                .iter()
                .map(|shift| shift.duration_hours)
                .collect::<Vec<_>>(),
            vec![12.0, 12.0, 12.0]
        );
        assert!(report
            .shifts
            .iter()
            .all(|shift| shift.active_teams.len() == 2));
        assert!(report.shifts.iter().all(|shift| shift.dorm_rest.is_none()));
        assert!(
            report
                .shifts
                .iter()
                .filter(|shift| shift.fiammetta.is_some())
                .count()
                <= 3
        );
        for shift in &report.shifts {
            assert_eq!(
                shift.weighted_trade,
                shift.efficiencies.trade_efficiency.scale_ratio(1, 3),
                "each 12h segment must contribute one third of the 36h cycle"
            );
            assert_eq!(
                shift.weighted_manufacture,
                shift.efficiencies.manufacture_efficiency.scale_ratio(1, 3)
            );
        }
        assert_eq!(
            report.daily.trade,
            report.shifts.iter().map(|shift| shift.weighted_trade).sum()
        );
    }

    #[test]
    fn fiammetta_second_segment_uses_cumulative_warmup_delta() {
        assert_eq!(
            continuation_efficiency(Efficiency::from_decimal(2.0), Efficiency::from_decimal(1.0),),
            Efficiency::from_decimal(3.0)
        );
        let first = RoomEfficiencyLine {
            room_id: "trade_1".into(),
            operator_count: 2,
            trade_efficiency: Efficiency::from_decimal(1.0),
            ..RoomEfficiencyLine::default()
        };
        let total = RoomEfficiencyLine {
            room_id: "trade_1".into(),
            operator_count: 3,
            trade_efficiency: Efficiency::from_decimal(2.0),
            ..RoomEfficiencyLine::default()
        };
        let continuation = continuation_room_line(&total, &first);
        assert_eq!(continuation.trade_efficiency, Efficiency::from_decimal(3.0));
        assert_eq!(continuation.operator_count, 3);
    }

    #[test]
    fn named_four_shift_profiles_fail_their_hard_preconditions() {
        let (blueprint, operbox, instances, table) = fixtures();
        let no_fiammetta = operbox.excluding(&HashSet::from(["菲亚梅塔".to_string()]));
        let fiammetta_error = schedule_timed_rotation(
            &blueprint,
            &no_fiammetta,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            TimedRotationProfile::Fiammetta8_8_4_4,
        )
        .unwrap_err();
        assert!(fiammetta_error.to_string().contains("requires an owned"));

        let excluded_abyssal: HashSet<_> = operbox
            .entries
            .iter()
            .filter(|entry| {
                let tier =
                    PromotionTier::from_elite_rarity_level(entry.elite, entry.rarity, entry.level);
                instances
                    .tags_for(&entry.name, tier)
                    .iter()
                    .any(|tag| tag == TAG_ABYSSAL)
            })
            .map(|entry| entry.name.clone())
            .collect();
        let no_abyssal = operbox.excluding(&excluded_abyssal);
        let abyssal_error = schedule_timed_rotation(
            &blueprint,
            &no_abyssal,
            &instances,
            &table,
            &AssignBaseOptions::default(),
            TimedRotationProfile::Abyssal7_5_7_5,
        )
        .unwrap_err();
        assert!(abyssal_error
            .to_string()
            .contains("requires a control producer"));
    }

    #[test]
    fn abyssal_dorm_rest_rejects_insufficient_five_hour_recovery() {
        let (_, operbox, instances, _) = fixtures();
        let blueprint: BaseBlueprint = serde_json::from_str(
            r#"{
                "rooms": [
                    {"id":"control","kind":"control_center","level":5},
                    {"id":"manu_1","kind":"factory","level":3,"product":{"factory":{"recipe":"gold"}}},
                    {"id":"manu_2","kind":"factory","level":3,"product":{"factory":{"recipe":"battle_record"}}},
                    {"id":"dorm_1","kind":"dormitory","level":1,"dorm_beds":5,"dorm_ambience_level":1}
                ]
            }"#,
        )
        .unwrap();
        let mut main = BaseAssignment::default();
        main.set_room(
            "control",
            vec![
                AssignedOperator::new("歌蕾蒂娅", 2),
                AssignedOperator::new("中枢填充1", 0),
                AssignedOperator::new("中枢填充2", 0),
                AssignedOperator::new("中枢填充3", 0),
                AssignedOperator::new("中枢填充4", 0),
            ],
        );
        main.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("乌尔比安", 2),
                AssignedOperator::new("斯卡蒂", 2),
            ],
        );
        main.set_room(
            "manu_2",
            vec![
                AssignedOperator::new("幽灵鲨", 2),
                AssignedOperator::new("安哲拉", 2),
            ],
        );

        let error = plan_abyssal_dorm_rest(
            &blueprint,
            &operbox,
            &instances,
            &main,
            &BaseAssignment::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("fully recover"));
    }

    #[test]
    fn timed_two_shift_builds_disjoint_complete_states() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            TimedRotationProfile::MainBackup12_12,
        )
        .unwrap();
        assert_eq!(report.profile, TimedRotationProfile::MainBackup12_12);
        assert_eq!(report.assignment_plans.len(), 2);
        assert_eq!(
            report
                .shifts
                .iter()
                .map(|shift| shift.duration_hours)
                .collect::<Vec<_>>(),
            vec![12.0, 12.0]
        );
        let main = report.shifts[0].assignment.operator_names();
        let backup = report.shifts[1].assignment.operator_names();
        assert!(main.is_disjoint(&backup));
        for shift in &report.shifts {
            assert_eq!(shift.assignment.control_operators().len(), 5);
            for room in &blueprint.rooms {
                if matches!(
                    room.kind,
                    FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                ) {
                    assert_eq!(
                        shift.assignment.operators_in(&room.id).len(),
                        room.operator_capacity()
                    );
                }
                if room.kind == FacilityKind::MeetingRoom {
                    assert_eq!(
                        shift.assignment.operators_in(&room.id).len(),
                        room.operator_capacity(),
                        "main-backup shift must fill the meeting room"
                    );
                }
            }
        }
        let maa = crate::export::build_from_team_rotation(
            &blueprint,
            &report,
            &crate::export::MaaExportOptions::for_blueprint(&blueprint),
        )
        .unwrap();
        assert_eq!(maa.plans.len(), 2);
        assert_eq!(maa.plan_times.as_deref(), Some("2班"));
    }

    #[test]
    fn timed_two_shift_office_fill_keeps_disjoint_states_and_warnings() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            TimedRotationProfile::MainBackup12_12,
        )
        .unwrap();
        // 主替互斥必须保持（main-fill-before-backup 后仍成立）。
        let main = report.shifts[0].assignment.operator_names();
        let backup = report.shifts[1].assignment.operator_names();
        assert!(main.is_disjoint(&backup), "main/backup must stay disjoint");
        // 办公室每班不超过容量；缺额必须与纯函数 warnings 一致。
        for shift in &report.shifts {
            for room in &blueprint.rooms {
                if room.kind == FacilityKind::Office {
                    let actual = shift.assignment.operators_in(&room.id).len();
                    assert!(
                        actual <= room.operator_capacity(),
                        "office {} over capacity: {actual}",
                        room.id.0
                    );
                }
            }
        }
        let expected = rotation_shortfall_warnings(&blueprint, &report.shifts);
        assert_eq!(
            report.warnings, expected,
            "report warnings must match pure function"
        );
        // 充足 roster 下应满编且无 warning（243 全精二 + 办公室容量 1）。
        let has_office = blueprint
            .rooms
            .iter()
            .any(|room| room.kind == FacilityKind::Office);
        if has_office {
            assert!(
                report.warnings.is_empty(),
                "full roster two-shift should fill office: {:?}",
                report.warnings
            );
        }
    }

    #[test]
    fn rotation_shortfall_warnings_distinguish_office_and_meeting() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        // 办公室留空 1 席、会客室只放 1 人（容量 2）。
        assignment.set_room("office", vec![]);
        assignment.set_room("meeting", vec![AssignedOperator::new("麦哲伦", 2)]);
        let shift = TeamShiftResult {
            index: 0,
            plan_index: 0,
            duration_hours: 12.0,
            active_teams: vec![TeamLabel::Alpha],
            resting_team: TeamLabel::Beta,
            assignment,
            fiammetta: None,
            dorm_rest: None,
            drone: None,
            transition: ShiftTransition::ReplaceRooms,
            efficiencies: ShiftEfficiencies::default(),
            weighted_trade: Efficiency::default(),
            weighted_manufacture: Efficiency::default(),
            weighted_power: Efficiency::default(),
        };
        let warnings = rotation_shortfall_warnings(&blueprint, &[shift]);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("办公室") && w.contains("office")),
            "office warning must be identifiable: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("会客室") && w.contains("meeting")),
            "meeting warning must be identifiable: {warnings:?}"
        );
    }

    #[test]
    fn abc_rotation_fills_meeting_room_with_rotating_meeting_skilled_cohort() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions::default(),
        )
        .unwrap();
        assert_eq!(report.shifts.len(), 3);
        let registry = crate::support_facility::SupportRegistry::load_default().unwrap();
        let meet_skilled: HashSet<String> = registry
            .entries
            .iter()
            .filter(|entry| entry.facility == crate::support_facility::SupportFacility::Meeting)
            .map(|entry| entry.operator.clone())
            .collect();
        for shift in &report.shifts {
            let names: Vec<_> = shift
                .assignment
                .operators_in(&RoomId::from("meeting"))
                .iter()
                .map(|op| op.name.clone())
                .collect();
            assert!(
                !names.is_empty(),
                "shift {} meeting room must not be empty",
                shift.index + 1
            );
            assert!(
                names.len() <= 2,
                "shift {} meeting room exceeds capacity: {names:?}",
                shift.index + 1
            );
            let skilled: Vec<_> = names
                .iter()
                .filter(|name| meet_skilled.contains(name.as_str()))
                .collect();
            assert!(
                !skilled.is_empty(),
                "shift {} meeting room should prefer clue-speed skilled operators: {names:?}",
                shift.index + 1
            );
        }
        let meeting_all: Vec<_> = report
            .shifts
            .iter()
            .flat_map(|shift| {
                shift
                    .assignment
                    .operators_in(&RoomId::from("meeting"))
                    .iter()
                    .map(|op| op.name.clone())
            })
            .collect();
        let cohort_slots = blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::MeetingRoom)
            .map(|room| room.operator_capacity())
            .sum::<usize>()
            * 3;
        assert!(
            meeting_all.len() >= cohort_slots - 1,
            "meeting cohort expected around {cohort_slots} total slots, got {}: {meeting_all:?}",
            meeting_all.len()
        );
        // 被 active dependency 钉进 shared 的会客室成员必须三班稳定出现（全周期共享），
        // 不得因 rotating cohort 轮换而在任何一班缺席。
        let shared_required: Vec<_> = report
            .shifts
            .iter()
            .flat_map(|shift| {
                shift
                    .assignment
                    .operators_in(&RoomId::from("meeting"))
                    .iter()
                    .map(|op| op.name.clone())
            })
            .collect();
        let all_three: HashSet<_> = shared_required
            .iter()
            .filter(|name| {
                report.shifts.iter().all(|shift| {
                    shift
                        .assignment
                        .operators_in(&RoomId::from("meeting"))
                        .iter()
                        .any(|op| &op.name == *name)
                })
            })
            .cloned()
            .collect();
        let shared_names: HashSet<String> = report.shifts[0]
            .assignment
            .operator_names()
            .into_iter()
            .filter(|name| {
                report
                    .shifts
                    .iter()
                    .all(|shift| shift.assignment.operator_names().contains(name))
            })
            .collect();
        for name in &all_three {
            assert!(
                shared_names.contains(name),
                "every-shift meeting member {name} must be a shared-scaffold member"
            );
        }
    }

    #[test]
    fn rotating_meeting_names_exclude_shared_required_members() {
        let peak = HashSet::from(["麦哲伦".to_string(), "见行者".to_string()]);
        let shared = HashSet::from(["麦哲伦".to_string(), "宿舍锚点".to_string()]);
        let rotating = rotating_meeting_names_excluding_shared(peak, &shared);
        assert_eq!(
            rotating,
            HashSet::from(["见行者".to_string()]),
            "shared-required meeting member must not enter rotating cohort"
        );
    }

    #[test]
    fn meeting_cohort_spreads_limited_candidates_across_all_teams() {
        let blueprint: BaseBlueprint =
            serde_json::from_str(r#"{"rooms":[{"id":"meeting","kind":"meeting_room","level":3}]}"#)
                .expect("blueprint");
        let pool = vec![
            AssignedOperator::new("A", 2),
            AssignedOperator::new("B", 2),
            AssignedOperator::new("C", 2),
        ];
        let mut rotating = HashMap::new();
        let mut reserved = HashSet::new();
        complete_rotating_meeting_teams(
            &blueprint,
            &HashSet::from(["A".to_string(), "B".to_string(), "C".to_string()]),
            &mut rotating,
            &pool,
            &mut reserved,
        )
        .unwrap();
        // 三名候选应均匀分摊到三队（每队 1 人），而不是集中到 Alpha/Beta。
        let by_team = |team: TeamLabel| {
            rotating
                .values()
                .filter(|assigned| **assigned == team)
                .count()
        };
        assert_eq!(by_team(TeamLabel::Alpha), 1, "rotating={rotating:?}");
        assert_eq!(by_team(TeamLabel::Beta), 1, "rotating={rotating:?}");
        assert_eq!(by_team(TeamLabel::Gamma), 1, "rotating={rotating:?}");

        // 每班两个活跃队都应能各提供一个会客室成员（容量 2 时第二席由 replacement 补齐）。
        let mut assignment = BaseAssignment::default();
        assign_rotating_meetings(
            &mut assignment,
            &blueprint,
            &[TeamLabel::Alpha, TeamLabel::Beta],
            TeamLabel::Gamma,
            &rotating,
            &pool,
            &reserved,
        )
        .unwrap();
        let names: Vec<_> = assignment
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert!(
            !names.is_empty(),
            "active teams must fill meeting: {names:?}"
        );
    }

    #[test]
    fn meeting_cohort_backfills_least_populated_team_first() {
        let blueprint: BaseBlueprint =
            serde_json::from_str(r#"{"rooms":[{"id":"meeting","kind":"meeting_room","level":3}]}"#)
                .expect("blueprint");
        let pool = vec![AssignedOperator::new("D", 2), AssignedOperator::new("E", 2)];
        // 已有 bind 成员在 Alpha，一个 peak 成员已分到 Beta（人数 1/1/0）。
        let mut rotating = HashMap::from([
            ("A".to_string(), TeamLabel::Alpha),
            ("B".to_string(), TeamLabel::Beta),
        ]);
        let mut reserved = HashSet::new();
        complete_rotating_meeting_teams(
            &blueprint,
            &HashSet::new(),
            &mut rotating,
            &pool,
            &mut reserved,
        )
        .unwrap();
        let by_team = |team: TeamLabel| {
            rotating
                .values()
                .filter(|assigned| **assigned == team)
                .count()
        };
        // 候选池两人：首补人数最少的 Gamma（1/1/0 → 1/1/1），第二人进入任一
        // 未满队（此时三队均衡，按队序取 Alpha）。
        assert_eq!(
            by_team(TeamLabel::Gamma),
            1,
            "least-populated team must be backfilled first: {rotating:?}"
        );
        assert!(
            by_team(TeamLabel::Alpha) <= 2 && by_team(TeamLabel::Beta) <= 2,
            "no team may exceed capacity: {rotating:?}"
        );
    }

    #[test]
    fn assign_rotating_meetings_replacement_never_uses_resting_team_members() {
        let blueprint: BaseBlueprint =
            serde_json::from_str(r#"{"rooms":[{"id":"meeting","kind":"meeting_room","level":3}]}"#)
                .expect("blueprint");
        let candidates = vec![
            AssignedOperator::new("休息队生产干员", 2),
            AssignedOperator::new("未编队空闲", 2),
        ];
        // cohort 中只有 Beta 队成员；本班活跃 Alpha+Gamma，Beta 休息。
        let rotating_team = HashMap::from([("Beta成员".to_string(), TeamLabel::Beta)]);
        let reserved = HashSet::from(["休息队生产干员".to_string(), "Alpha成员".to_string()]);

        let mut assignment = BaseAssignment::default();
        assign_rotating_meetings(
            &mut assignment,
            &blueprint,
            &[TeamLabel::Alpha, TeamLabel::Gamma],
            TeamLabel::Beta,
            &rotating_team,
            &candidates,
            &reserved,
        )
        .expect("meeting fill");
        let names: Vec<_> = assignment
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|op| op.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["未编队空闲"],
            "must pick idle unroster member: {names:?}"
        );
    }

    #[test]
    fn timed_fiammetta_profile_emits_event_boundary() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        if !operbox.owns("菲亚梅塔") {
            return;
        }
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            TimedRotationProfile::Fiammetta8_8_4_4,
        )
        .unwrap();
        assert_eq!(
            report
                .shifts
                .iter()
                .map(|shift| shift.duration_hours)
                .collect::<Vec<_>>(),
            vec![8.0, 8.0, 4.0, 4.0]
        );
        assert_eq!(report.shifts[1].transition, ShiftTransition::FiammettaOnly);
        let action = report.shifts[1].fiammetta.as_ref().unwrap();
        assert!(action.displaced.is_empty());
        assert!(report.shifts[1]
            .assignment
            .operators_in(&action.room_id)
            .iter()
            .any(|operator| operator.name == action.target));
        assert_eq!(
            report.shifts[0].assignment.operators_in(&action.room_id),
            report.shifts[1].assignment.operators_in(&action.room_id),
            "FiammettaOnly must use the re-searched target room owner"
        );
        let active_names = active_team_names(&report.teams, &report.shifts[1].active_teams);
        assert!(report.shifts[1]
            .assignment
            .operators_in(&action.room_id)
            .iter()
            .all(|operator| active_names.contains(&operator.name)));
        assert_eq!(
            report.daily.trade,
            report.shifts.iter().map(|shift| shift.weighted_trade).sum()
        );
        let maa = crate::export::build_from_team_rotation(
            &blueprint,
            &report,
            &crate::export::MaaExportOptions::for_blueprint(&blueprint),
        )
        .unwrap();
        assert_eq!(maa.plans.len(), 4);
        assert_eq!(maa.plan_times.as_deref(), Some("4班"));
        assert!(maa.plans[1].fiammetta.enable);
    }

    #[test]
    fn timed_abyssal_profile_owns_four_consumers_and_alternates() {
        let (blueprint, operbox, instances, table) = fixtures();
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
            TimedRotationProfile::Abyssal7_5_7_5,
        )
        .unwrap();
        assert_eq!(report.assignment_plans.len(), 2);
        assert_eq!(
            report
                .shifts
                .iter()
                .map(|shift| shift.duration_hours)
                .collect::<Vec<_>>(),
            vec![7.0, 5.0, 7.0, 5.0]
        );
        assert_eq!(
            serde_json::to_value(&report.shifts[0].assignment).unwrap(),
            serde_json::to_value(&report.shifts[2].assignment).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&report.shifts[1].assignment).unwrap(),
            serde_json::to_value(&report.shifts[3].assignment).unwrap()
        );
        assert!(report.assignment_plans[0]
            .selected_rules
            .iter()
            .any(|rule| rule.rule_id == "timed_rotation_abyssal_7_5_7_5"));
        assert_eq!(
            report.assignment_plans[0]
                .anchors
                .iter()
                .filter(|anchor| anchor.system_id == "timed_rotation_abyssal_7_5_7_5")
                .count(),
            5
        );
        assert!(shift_binds_from_plan(&report.assignment_plans[0])
            .iter()
            .any(|bind| bind.operators.len() == 5 && bind.on_shifts == 2 && bind.off_shifts == 2));
        let dorm_rest = report.shifts[1]
            .dorm_rest
            .as_ref()
            .expect("5h Abyssal recovery state has explicit dorm group");
        assert_ne!(dorm_rest.single_manager, dorm_rest.group_manager);
        let mood_model = MoodModel::load_default().unwrap();
        assert!(mood_model
            .dorm_skills(
                &dorm_rest.single_manager,
                operbox.elite_of(&dorm_rest.single_manager).unwrap(),
            )
            .iter()
            .any(|skill| skill.is_single()));
        assert!(mood_model
            .dorm_skills(
                &dorm_rest.group_manager,
                operbox.elite_of(&dorm_rest.group_manager).unwrap(),
            )
            .iter()
            .any(|skill| {
                skill.is_group() || (skill.is_manager() && skill.also_group.is_some())
            }));
        assert_eq!(report.shifts[3].dorm_rest.as_ref(), Some(dorm_rest));
        let maa = crate::export::build_from_team_rotation(
            &blueprint,
            &report,
            &crate::export::MaaExportOptions::for_blueprint(&blueprint),
        )
        .unwrap();
        assert_eq!(maa.plans.len(), 4);
        assert_eq!(maa.plan_times.as_deref(), Some("4班"));
        let dorm_index = blueprint
            .rooms_of(FacilityKind::Dormitory)
            .iter()
            .position(|room| room.id == dorm_rest.room_id)
            .unwrap();
        assert_eq!(
            &maa.plans[1].rooms.dormitory[dorm_index].operators[..3],
            [
                dorm_rest.single_manager.clone(),
                dorm_rest.group_manager.clone(),
                dorm_rest.target.clone(),
            ]
        );
        assert!(!maa.plans[1].rooms.dormitory[dorm_index].sort);
    }

    fn witch_rotation_reserve() -> ResolvedRoleReserve {
        ResolvedRoleReserve {
            system_id: "rosemary_perception".to_string(),
            reserve_id: "trade_cohort_c".to_string(),
            role_id: "witch".to_string(),
            facility: FacilityKind::TradePost,
            operators: vec!["巫恋".to_string(), "龙舌兰".to_string(), "柏喙".to_string()],
            eligible_rooms: vec![RoomId::from("trade_1"), RoomId::from("trade_2")],
            reuse_policy: ReserveReusePolicy::EveryEligibleHalf,
            require_pre_split_halves: true,
        }
    }

    #[test]
    fn rotation_reserve_reuses_exact_operator_set_in_both_halves() {
        let (blueprint, operbox, _, _) = fixtures();
        let h1 = FacilityHalf {
            trade: vec![RoomId::from("trade_1")],
            ..FacilityHalf::default()
        };
        let h2 = FacilityHalf {
            trade: vec![RoomId::from("trade_2")],
            ..FacilityHalf::default()
        };
        let reserve = witch_rotation_reserve();
        let mut assignment_h1 = BaseAssignment::default();
        let mut assignment_h2 = BaseAssignment::default();
        let mut used_h1 = HashSet::new();
        let mut used_h2 = HashSet::new();

        place_rotation_reserves_for_halves(
            &blueprint,
            &operbox,
            std::slice::from_ref(&reserve),
            &h1,
            &mut assignment_h1,
            &mut used_h1,
            &h2,
            &mut assignment_h2,
            &mut used_h2,
        )
        .unwrap();
        validate_rotation_reserves_before_postprocess(
            &blueprint,
            std::slice::from_ref(&reserve),
            &h1,
            &assignment_h1,
            &h2,
            &assignment_h2,
        )
        .unwrap();

        for assignment in [&assignment_h1, &assignment_h2] {
            let names: Vec<_> = assignment
                .operators_in(&RoomId::from("trade_1"))
                .iter()
                .chain(assignment.operators_in(&RoomId::from("trade_2")).iter())
                .map(|operator| operator.name.as_str())
                .collect();
            assert_eq!(names, vec!["巫恋", "龙舌兰", "柏喙"]);
        }
        assert_eq!(used_h1, used_h2);
    }

    #[test]
    fn rotation_reserve_pre_postprocess_validation_rejects_overwrite() {
        let (blueprint, operbox, _, _) = fixtures();
        let h1 = FacilityHalf {
            trade: vec![RoomId::from("trade_1")],
            ..FacilityHalf::default()
        };
        let h2 = FacilityHalf {
            trade: vec![RoomId::from("trade_2")],
            ..FacilityHalf::default()
        };
        let reserve = witch_rotation_reserve();
        let mut assignment_h1 = BaseAssignment::default();
        let mut assignment_h2 = BaseAssignment::default();
        place_rotation_reserves_for_halves(
            &blueprint,
            &operbox,
            std::slice::from_ref(&reserve),
            &h1,
            &mut assignment_h1,
            &mut HashSet::new(),
            &h2,
            &mut assignment_h2,
            &mut HashSet::new(),
        )
        .unwrap();
        assignment_h2.set_room(
            "trade_2",
            vec![AssignedOperator::new("被错误覆盖的干员", 2)],
        );

        let error = validate_rotation_reserves_before_postprocess(
            &blueprint,
            std::slice::from_ref(&reserve),
            &h1,
            &assignment_h1,
            &h2,
            &assignment_h2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("overwritten in H2"));
    }

    #[test]
    fn rotation_reserve_every_eligible_half_rejects_missing_target() {
        let (blueprint, operbox, _, _) = fixtures();
        let h1 = FacilityHalf {
            trade: vec![RoomId::from("trade_1")],
            ..FacilityHalf::default()
        };
        let h2 = FacilityHalf::default();
        let reserve = witch_rotation_reserve();

        let error = place_rotation_reserves_for_halves(
            &blueprint,
            &operbox,
            std::slice::from_ref(&reserve),
            &h1,
            &mut BaseAssignment::default(),
            &mut HashSet::new(),
            &h2,
            &mut BaseAssignment::default(),
            &mut HashSet::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing eligible H2 target"));
    }

    fn assert_pinus_rotation(
        report: &TeamRotationReport,
        blueprint: &BaseBlueprint,
        manufacturers: &[&str],
    ) {
        use crate::schedule::shift_bind::team_of_operator;

        let mut bound = vec!["焰尾", "薇薇安娜"];
        bound.extend_from_slice(manufacturers);
        let team = team_of_operator(report, "焰尾").expect("焰尾应进入轮换队");
        for name in &bound {
            assert_eq!(
                team_of_operator(report, name),
                Some(team),
                "红松核心应同队上二休一: {name}"
            );
        }

        let mut active_shifts = 0;
        for shift in &report.shifts {
            let active = shift.resting_team != team;
            active_shifts += usize::from(active);
            for name in &bound {
                if !active {
                    // 休息队成员可以住在宿舍；只检查他们不占工作岗位。
                    assert!(
                        !shift.assignment.is_work_position_operator(blueprint, name),
                        "休息班不应安排红松核心: {name}"
                    );
                    continue;
                }
                let room = shift
                    .assignment
                    .room_of_operator(name)
                    .unwrap_or_else(|| panic!("工作班缺少红松核心: {name}"));
                let room_blueprint = blueprint.room(&room.room_id).expect("room exists");
                if manufacturers.contains(name) {
                    assert!(
                        matches!(
                            room_blueprint.product,
                            Some(crate::layout::RoomProduct::Factory {
                                recipe: crate::types::RecipeKind::BattleRecord
                            })
                        ),
                        "红松制造成员只能进入作战记录站: {name}"
                    );
                } else {
                    assert_eq!(
                        room_blueprint.kind,
                        FacilityKind::ControlCenter,
                        "中枢双核心必须实际进入中枢: {name}"
                    );
                }
            }
        }
        assert_eq!(active_shifts, 2, "红松核心应工作两班、休息一班");
    }

    #[test]
    fn pinus_rotation_keeps_optional_gravel_with_viviana_when_selected() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
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
        if !report.shifts.iter().any(|shift| {
            shift
                .assignment
                .control_operators()
                .iter()
                .any(|op| op.name == "薇薇安娜")
        }) || !report.shifts.iter().any(|shift| {
            shift.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|spec| spec.kind == FacilityKind::Factory)
                    && room.operators.iter().any(|op| op.name == "砾")
            })
        }) {
            return;
        }
        let viviana_team = crate::schedule::shift_bind::team_of_operator(&report, "薇薇安娜")
            .expect("薇薇安娜应进入轮换队伍");
        assert_eq!(
            crate::schedule::shift_bind::team_of_operator(&report, "砾"),
            Some(viviana_team),
            "实际选中的砾应与薇薇安娜同班，才能稳定获得骑士制造加成"
        );
        for shift in &report.shifts {
            let viviana_present = shift
                .assignment
                .control_operators()
                .iter()
                .any(|op| op.name == "薇薇安娜");
            let gravel_present = shift.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|spec| spec.kind == FacilityKind::Factory)
                    && room.operators.iter().any(|op| op.name == "砾")
            });
            assert_eq!(viviana_present, gravel_present);
        }
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
        assignment.iter_rooms().any(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|bp| bp.kind == FacilityKind::TradePost)
                && names
                    .iter()
                    .all(|name| room.operators.iter().any(|op| op.name == *name))
        })
    }

    fn assert_three_trade_cohort_plan(
        report: &TeamRotationReport,
        blueprint: &BaseBlueprint,
    ) -> Vec<String> {
        let selected = report
            .peak_plan
            .selected_rules
            .iter()
            .find(|selected| selected.rule_id == "rosemary_perception")
            .expect("full Rosemary pack must select the Rosemary rule");
        assert_eq!(
            selected.conditional_pack_id.as_deref(),
            Some("three_trade_cohorts")
        );
        let reserve = report
            .peak_plan
            .rotation_reserves
            .iter()
            .find(|reserve| reserve.reserve_id == "trade_cohort_c")
            .expect("full Rosemary pack must resolve cohort C");
        assert_eq!(reserve.reuse_policy, ReserveReusePolicy::EveryEligibleHalf);
        assert_eq!(reserve.operators.len(), 3);
        assert!(reserve.operators.iter().any(|name| name == "巫恋"));
        assert!(reserve.operators.iter().any(|name| name == "龙舌兰"));
        assert!(reserve
            .operators
            .iter()
            .any(|name| name != "巫恋" && name != "龙舌兰"));

        assert!(trade_room_contains(
            &report.shifts[0].assignment,
            blueprint,
            &["但书"]
        ));
        assert!(trade_room_contains(
            &report.shifts[0].assignment,
            blueprint,
            &["可露希尔", "黑键"]
        ));
        let peak_names = report.shifts[0]
            .assignment
            .work_position_operator_names(blueprint);
        assert!(reserve
            .operators
            .iter()
            .all(|operator| !peak_names.contains(operator)));
        reserve.operators.clone()
    }

    #[test]
    fn team_rotation_full_e2_resolves_gamma_cohort_before_postprocess() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
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
        assert_three_trade_cohort_plan(&report, &blueprint);
    }

    #[test]
    fn team_rotation_three_trade_cohorts_keep_unique_tailor_out_of_peak() {
        let (blueprint, full, instances, table) = user_243_fixtures();
        let operbox = full.excluding(&HashSet::from([
            "卡夫卡".to_string(),
            "折光".to_string(),
            "明椒".to_string(),
            "渡桥".to_string(),
            "贝娜".to_string(),
        ]));
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
        let reserve = assert_three_trade_cohort_plan(&report, &blueprint);
        let mut sorted_reserve = reserve.clone();
        sorted_reserve.sort();
        assert_eq!(
            sorted_reserve,
            vec!["巫恋".to_string(), "柏喙".to_string(), "龙舌兰".to_string()]
        );
        assert!(report
            .peak_plan
            .anchors
            .iter()
            .all(|anchor| { !reserve.iter().any(|name| name == &anchor.operator) }));
    }

    #[test]
    fn team_rotation_frontend_layout_keeps_closure_and_docus_core() {
        let (mut blueprint, operbox, instances, table) = user_243_fixtures();
        if let Some(room) = blueprint
            .rooms
            .iter_mut()
            .find(|room| room.id.0 == "office_1")
        {
            room.id = RoomId::from("office");
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
        assert_three_trade_cohort_plan(&report, &blueprint);
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
        });

        assert!(!candidates.is_empty());
        for candidate in candidates {
            for room in candidate.assignment.iter_rooms() {
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
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
        ])
        .unwrap();
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
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
        ])
        .unwrap();
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
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
        ])
        .unwrap();
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
            required_manu_by_room: &HashMap::new(),
            admitted_control_producer: abyssal_control_producer(
                &operbox,
                &instances,
                &HashSet::new(),
            )
            .as_deref(),
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
            for room in shift.assignment.iter_rooms() {
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
        assert!(!report.daily.trade.is_zero());
        assert!(!report.daily.manufacture.is_zero());
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
                // 深海 S2 短班分支生产者是 admission-only pin，没有永久队伍归属，只允许出现在 S2。
                let Some(team) = team_by_name.get(&op.name).copied() else {
                    assert_eq!(
                        shift.index, 1,
                        "中枢干员 {} 缺少 α/β/γ 归属，只有 S2 短班分支生产者可以例外",
                        op.name
                    );
                    continue;
                };
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
            let names: Vec<String> = shift
                .assignment
                .control_operators()
                .into_iter()
                .map(|op| op.name)
                .collect();
            let layout = resolve_base(
                &blueprint,
                &shift.assignment,
                Some(&instances),
                Some(&table),
                24.0,
                None,
            )
            .unwrap()
            .layout_snapshot();
            let inject = control_inject_components_for_assignment(
                &layout,
                &blueprint,
                &shift.assignment,
                &instances,
            );

            assert!(
                inject.trade_eff_pct > 0.0,
                "每班中枢应产生实际贸易订单效率注入: {names:?}"
            );
            assert!(
                inject.gold_manu_eff_pct > 0.0,
                "每班中枢应产生实际赤金制造生产力注入: {names:?}"
            );
            assert!(
                inject.battle_record_manu_eff_pct > 0.0,
                "每班中枢应产生实际经验书制造生产力注入: {names:?}"
            );
            control_by_shift.push(names);
        }

        for names in &control_by_shift {
            assert_eq!(
                names.len(),
                5,
                "每班中枢仍应满编 5 人: {:?}",
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
            let has_abyssal_manu = shift.assignment.iter_rooms().any(|room| {
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
                    shift.efficiencies.manufacture_efficiency.as_f64() > 4.500,
                    "S2 深海候选应使用带歌蕾蒂娅和深海 tag 的最终布局重算制造分，got {}",
                    shift.efficiencies.manufacture_efficiency
                );
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
                for room in shift.assignment.iter_rooms() {
                    // 宿舍内的休息队成员是正常休息，不属于“上岗”。
                    if blueprint
                        .room(&room.room_id)
                        .is_some_and(|bp| bp.kind == FacilityKind::Dormitory)
                    {
                        continue;
                    }
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
                        if resting.operators.contains(&op.name) {
                            assert_eq!(
                                shift.fiammetta.as_ref().map(|action| action.target.as_str()),
                                Some(op.name.as_str()),
                                "S2 休息队成员只有声明式 continuous role 的菲亚回岗目标可以上岗: {}",
                                op.name
                            );
                        }
                    }
                }
            }
        }

        assert!(
            shifts_with_abyssal_manu == vec![1],
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
        let shared_dependency_names: HashSet<_> = report
            .peak_plan
            .active_dependencies
            .iter()
            .flat_map(|dependency| dependency.required.iter().cloned())
            .collect();

        for shift in &report.shifts {
            for room in shift.assignment.iter_rooms() {
                if blueprint.room(&room.room_id).is_some_and(|bp| {
                    matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office)
                }) {
                    continue;
                }
                for op in &room.operators {
                    if shared_dependency_names.contains(&op.name) {
                        continue;
                    }
                    // 深海 S2 短班分支生产者是 admission-only pin，没有永久队伍归属，只允许出现在 S2。
                    let Some(team) = team_by_name.get(&op.name).copied() else {
                        assert_eq!(
                            shift.index, 1,
                            "上岗干员 {} 缺少 α/β/γ 归属，只有 S2 短班分支生产者可以例外",
                            op.name
                        );
                        continue;
                    };
                    if team == shift.resting_team {
                        assert_eq!(
                            shift
                                .fiammetta
                                .as_ref()
                                .map(|action| action.target.as_str()),
                            Some(op.name.as_str()),
                            "shift {} 房间 {} 非菲亚目标的休息队干员 {} 被错误排回",
                            shift.index + 1,
                            room.room_id.0,
                            op.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn team_rotation_scores_match_current_shift_context() {
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
        let durin_plan = operbox.durin_dorm_planning_count(&instances);

        for shift in &report.shifts {
            let mut assignment = shift.assignment.clone();
            clear_production_efficiencies(&blueprint, &mut assignment);
            let recomputed = evaluate_base_assignment_efficiencies(
                &blueprint,
                &assignment,
                &instances,
                &table,
                shift.duration_hours,
                Some(durin_plan),
            )
            .unwrap();

            assert!(
                recomputed.trade_efficiency == shift.efficiencies.trade_efficiency,
                "shift {} trade score used a stale room snapshot: recomputed={} stored={}",
                shift.index + 1,
                recomputed.trade_efficiency,
                shift.efficiencies.trade_efficiency
            );
            assert!(
                recomputed.manufacture_efficiency == shift.efficiencies.manufacture_efficiency,
                "shift {} manufacture score used a stale room snapshot: recomputed={} stored={}",
                shift.index + 1,
                recomputed.manufacture_efficiency,
                shift.efficiencies.manufacture_efficiency
            );
            assert!(
                recomputed.power_efficiency == shift.efficiencies.power_efficiency,
                "shift {} power score used a stale room snapshot: recomputed={} stored={}",
                shift.index + 1,
                recomputed.power_efficiency,
                shift.efficiencies.power_efficiency
            );
        }
    }

    #[test]
    fn team_rotation_fiammetta_marks_s1_normal_search_target_at_cycle_boundary() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
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

        let actions: Vec<_> = report
            .shifts
            .iter()
            .filter_map(|shift| shift.fiammetta.as_ref().map(|action| (shift, action)))
            .collect();
        assert_eq!(actions.len(), 1, "每个 24h 周期只应安排一次菲亚回岗");

        let (shift, action) = actions[0];
        assert_eq!(shift.index, 0, "菲亚动作必须挂在换心情后的 S1");
        assert!(action.displaced.is_empty(), "菲亚不得直接顶替 S1 的候选");
        let source_room = blueprint
            .room(&action.room_id)
            .expect("菲亚回岗原房必须仍在 layout");
        assert_eq!(source_room.kind, FacilityKind::TradePost);
        assert!(matches!(
            source_room.product,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold
            })
        ));
        assert!(
            shift
                .assignment
                .operators_in(&action.room_id)
                .iter()
                .any(|op| op.name == action.target),
            "菲亚目标必须回到 peak 原房间"
        );
        assert_eq!(
            crate::schedule::team_of_operator(&report, &action.target),
            Some(TeamLabel::Alpha),
            "菲亚目标必须仍是 S1 正常搜索选中的上班成员"
        );
        assert!(
            report.shifts[2]
                .assignment
                .operator_names()
                .contains(&action.target),
            "S3 结束时必须由菲亚为下一周期 S1 的目标换心情"
        );
        let target_team = crate::schedule::team_of_operator(&report, &action.target)
            .expect("菲亚目标必须属于一个轮换队伍");
        for shift in &report.shifts {
            let target_team_active = shift.active_teams.contains(&target_team);
            assert_eq!(
                shift.assignment.operator_names().contains(&action.target),
                target_team_active,
                "默认 ABC 不得让菲亚目标跨越休息班持续工作: shift={} target={} active={target_team_active}",
                shift.index + 1,
                action.target
            );
        }
        for shift in &report.shifts {
            let names: Vec<_> = shift
                .assignment
                .iter_rooms()
                .flat_map(|room| room.operators.iter().map(|operator| &operator.name))
                .collect();
            assert_eq!(
                names.len(),
                names.iter().copied().collect::<HashSet<_>>().len(),
                "shift {} 不得因菲亚回岗产生重复干员",
                shift.index + 1
            );
            assert!(
                !blackkey_witch_same_trade_room(&shift.assignment, &blueprint),
                "shift {} 菲亚后仍必须保持黑键与巫恋禁同房",
                shift.index + 1
            );
        }
    }

    #[test]
    fn fiammetta_enable_false_suppresses_return_actions() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        assert!(
            operbox.owns("菲亚梅塔"),
            "fixture 必须拥有菲亚梅塔，否则本测试无法区分关闭与缺员"
        );
        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                fiammetta_enable: false,
                ..Default::default()
            },
            TimedRotationProfile::default(),
        )
        .unwrap();
        assert!(
            report.shifts.iter().all(|shift| shift.fiammetta.is_none()),
            "fiammetta_enable=false 时不得产生任何菲亚回岗动作"
        );
        let with_fiammetta = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                fiammetta_enable: true,
                ..Default::default()
            },
            TimedRotationProfile::default(),
        )
        .unwrap();
        assert!(
            with_fiammetta
                .shifts
                .iter()
                .any(|shift| shift.fiammetta.is_some()),
            "fiammetta_enable=true 时默认 ABC 应产生回岗动作"
        );
    }

    #[test]
    fn fiammetta_enable_false_rejects_8844_profile() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        let err = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                fiammetta_enable: false,
                ..Default::default()
            },
            TimedRotationProfile::Fiammetta8_8_4_4,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("fiammetta-8844 requires fiammetta_enable=true"),
            "禁用菲亚梅塔时显式 8844 profile 必须硬错误，实际: {err}"
        );
    }

    #[test]
    fn fiammetta_target_priority_matches_canonical_order() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        assert_eq!(
            fiammetta_return_priority(&blueprint_for_one_trade(&blueprint)),
            vec!["可露希尔", "巫恋", "龙舌兰", "但书", "清流"]
        );

        let bind = RuntimeShiftBind {
            operators: vec!["绑定成员".to_string(), "绑定队友".to_string()],
            on_shifts: 2,
            off_shifts: 1,
        };
        assert!(has_multi_member_shift_bind("绑定成员", &[bind]));

        let mut three_trade = blueprint.clone();
        three_trade.rooms.push(crate::layout::RoomBlueprint {
            id: RoomId::from("trade_extra"),
            kind: FacilityKind::TradePost,
            level: 3,
            product: Some(RoomProduct::Trade {
                order: crate::trade::input::TradeOrderKind::Gold,
            }),
            dorm_beds: None,
            dorm_ambience_level: None,
        });
        assert_eq!(
            fiammetta_return_priority(&three_trade),
            vec!["但书", "可露希尔", "清流", "巫恋", "龙舌兰"]
        );
    }

    fn blueprint_for_one_trade(blueprint: &BaseBlueprint) -> BaseBlueprint {
        let mut one_trade = blueprint.clone();
        let mut seen_trade = false;
        one_trade.rooms.retain(|room| {
            if room.kind != FacilityKind::TradePost {
                return true;
            }
            if seen_trade {
                false
            } else {
                seen_trade = true;
                true
            }
        });
        one_trade
    }

    #[test]
    fn team_rotation_without_fiammetta_does_not_create_return_action() {
        let (blueprint, operbox, instances, table) = user_243_fixtures();
        let no_fiammetta = operbox.excluding(&HashSet::from(["菲亚梅塔".to_string()]));
        let report = schedule_team_rotation(
            &blueprint,
            &no_fiammetta,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(report.shifts.iter().all(|shift| shift.fiammetta.is_none()));
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
        assert!(
            !report
                .peak_plan
                .registry_system_ids()
                .contains(&"syracusa_pair"),
            "叙拉古贸易队友不应再由 peak registry 认领"
        );
        assert!(
            !report
                .peak_plan
                .registry_system_ids()
                .contains(&"syracusa_cross_station"),
            "叙拉古自然效率关系不得出现在 peak registry"
        );
    }

    #[test]
    fn team_rotation_reports_peak_mood_eta() {
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

        let eta = report
            .peak_mood_eta
            .as_ref()
            .expect("peak 主力班必须输出 mood ETA");
        assert!(!eta.per_op.is_empty());
        assert!(eta.bottleneck.is_some());
        assert!(eta.eta_hours.is_some_and(|hours| hours > 0.0));
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
        let peak_has_blackkey = peak.iter_rooms().any(|r| {
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
            shift.assignment.iter_rooms().any(|room| {
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
                support_combinations_enable: true,
                ..Default::default()
            },
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )
        .unwrap();
        assert!(
            peak.iter_rooms()
                .any(|r| r.operators.iter().any(|o| o.name == "迷迭香")),
            "迷迭香链激活时 peak 应含迷迭香"
        );
        assert!(
            peak.iter_rooms()
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
                support_combinations_enable: true,
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
    fn team_rotation_pure_fireworks_rotates_office_trade_and_control_together() {
        use crate::schedule::shift_bind::team_of_operator;

        let (blueprint, full_operbox, instances, table) = fixtures();
        let operbox = OperBox::from_entries(
            full_operbox
                .entries
                .into_iter()
                // 与 pure_fireworks_declares_solver_candidate_requirement 同口径：
                // 去掉迷迭香和喀兰跨站成员，避免喀兰体系先占走贸易站。
                .filter(|entry| {
                    ![
                        "迷迭香",
                        "黑键",
                        "灵知",
                        "孑",
                        "银灰",
                        "崖心",
                        "讯使",
                        "角峰",
                    ]
                    .contains(&entry.name.as_str())
                })
                .collect(),
        )
        .unwrap();
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

        assert!(report
            .peak_plan
            .activated
            .iter()
            .any(|system| system.system_id == "human_fireworks_pure"));
        let bind = report
            .peak_plan
            .shift_binds
            .iter()
            .find(|bind| {
                bind.operators.iter().any(|name| name == "桑葚")
                    && bind.operators.iter().any(|name| name == "乌有")
            })
            .expect("纯人间烟火应产生包含桑葚和乌有的实际 shift bind");
        let control_name = ["重岳", "令"]
            .into_iter()
            .find(|name| bind.operators.iter().any(|bound| bound == name))
            .expect("纯人间烟火 bind 应包含实际入驻中枢的重岳或令");
        let team = team_of_operator(&report, "桑葚").expect("桑葚应归入轮换队伍");
        assert_eq!(
            report
                .teams
                .iter()
                .flat_map(|cohort| &cohort.operators)
                .filter(|name| name.as_str() == "桑葚")
                .count(),
            1,
            "绑定办公室成员只能归属一个 cohort"
        );
        for name in ["乌有", control_name] {
            assert_eq!(
                team_of_operator(&report, name),
                Some(team),
                "纯烟火成员应与桑葚同队: {name}"
            );
        }

        let in_facility = |shift: &TeamShiftResult, facility: FacilityKind, name: &str| {
            shift.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|bp| bp.kind == facility)
                    && room.operators.iter().any(|op| op.name == name)
            })
        };
        let mut active_count = 0;
        for shift in &report.shifts {
            assert_eq!(shift.assignment.control_operators().len(), 5);
            let mangberry_active = in_facility(shift, FacilityKind::Office, "桑葚");
            let wuyou_active = in_facility(shift, FacilityKind::TradePost, "乌有");
            let control_active = in_facility(shift, FacilityKind::ControlCenter, control_name);
            let resolve_fireworks = |assignment: &BaseAssignment| {
                resolve_base(
                    &blueprint,
                    assignment,
                    Some(&instances),
                    Some(&table),
                    24.0,
                    None,
                )
                .unwrap()
                .layout
                .global
                .get(crate::global_resource::GlobalResourceKey::HumanFireworks)
            };
            let actual_fireworks = resolve_fireworks(&shift.assignment);
            let office_id = blueprint
                .rooms
                .iter()
                .find(|room| room.kind == FacilityKind::Office)
                .unwrap()
                .id
                .clone();
            let mut counterfactual = shift.assignment.clone();
            counterfactual.set_room(
                office_id,
                if mangberry_active {
                    Vec::new()
                } else {
                    vec![AssignedOperator::new("桑葚", 2)]
                },
            );
            let counterfactual_fireworks = resolve_fireworks(&counterfactual);
            assert_eq!(
                (wuyou_active, control_active),
                (mangberry_active, mangberry_active),
                "纯烟火办公室、贸易和中枢成员必须同上同下"
            );

            if shift.resting_team == team {
                assert!(!mangberry_active, "桑葚休息班不得继续占用办公室");
                let offices: Vec<_> = shift
                    .assignment
                    .rooms_of_kind(&blueprint, FacilityKind::Office)
                    .collect();
                assert!(!offices.is_empty(), "布局应包含办公室");
                assert!(
                    offices.iter().all(|room| !room.operators.is_empty()),
                    "桑葚休息时办公室必须由其他干员补位"
                );
                assert!(offices.iter().all(|room| {
                    room.operators
                        .iter()
                        .all(|operator| operator.name != "桑葚")
                }));
                assert!(
                    (counterfactual_fireworks - actual_fireworks - 20.0).abs() < 0.001,
                    "休班替补不得暗含桑葚的 +20 烟火"
                );
            } else {
                active_count += 1;
                assert!(mangberry_active, "桑葚所属队当班时应进入办公室");
                assert!(
                    (actual_fireworks - counterfactual_fireworks - 20.0).abs() < 0.001,
                    "最终班次 resolve 必须看到桑葚办公室的 +20 烟火"
                );
            }
        }
        assert_eq!(active_count, 2, "纯烟火绑定组应上二休一");
    }

    #[test]
    fn team_rotation_perception_fireworks_keeps_full_core_together() {
        use crate::schedule::shift_bind::team_of_operator;

        let (blueprint, full_operbox, instances, table) = fixtures();
        let operbox = full_operbox.excluding(&HashSet::from([
            "焰尾".to_string(),
            "薇薇安娜".to_string(),
            "斩业星熊".to_string(),
            "诗怀雅".to_string(),
            "火龙S黑角".to_string(),
            "麒麟R夜刀".to_string(),
        ]));
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

        assert!(
            report.peak_plan.selected_rules.iter().any(|selected| {
                selected.rule_id == "human_fireworks_perception"
                    && selected.alternative_id == "actual_perception_core"
            }),
            "selected={:?}; registry={:?}; anchors={:?}",
            report.peak_plan.selected_rules,
            report.peak_plan.registry_system_ids(),
            report.peak_plan.anchors
        );
        assert!(!report
            .peak_plan
            .registry_system_ids()
            .contains(&"human_fireworks_perception"));
        assert!(!report
            .peak_plan
            .registry_system_ids()
            .contains(&"human_fireworks_pure"));
        let team = team_of_operator(&report, "乌有").expect("乌有应归入感知烟火 cohort");
        for name in ["重岳", "令"] {
            assert_eq!(team_of_operator(&report, name), Some(team));
        }

        let in_facility = |shift: &TeamShiftResult, facility: FacilityKind, name: &str| {
            shift.assignment.iter_rooms().any(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|bp| bp.kind == facility)
                    && room.operators.iter().any(|op| op.name == name)
            })
        };
        let mut active_count = 0;
        for shift in &report.shifts {
            assert_eq!(shift.assignment.control_operators().len(), 5);
            let wuyou = in_facility(shift, FacilityKind::TradePost, "乌有");
            let zhongyue = in_facility(shift, FacilityKind::ControlCenter, "重岳");
            let ling = in_facility(shift, FacilityKind::ControlCenter, "令");
            assert_eq!((zhongyue, ling), (wuyou, wuyou));
            assert!(
                !in_facility(shift, FacilityKind::Office, "桑葚"),
                "感知分支与纯烟火桑葚办公室分支必须互斥"
            );
            assert!(blueprint
                .rooms
                .iter()
                .filter(|room| room.kind == FacilityKind::Office)
                .all(|room| !shift.assignment.operators_in(&room.id).is_empty()));
            active_count += usize::from(wuyou);
        }
        assert_eq!(active_count, 2);
    }

    #[test]
    fn team_rotation_pinus_two_manufacturers_end_to_end() {
        let (blueprint, full_operbox, instances, table) = fixtures();
        let operbox = OperBox::from_entries(
            full_operbox
                .entries
                .into_iter()
                .filter(|entry| entry.name != "野鬃")
                .collect(),
        )
        .unwrap();
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

        assert!(report
            .peak_plan
            .activated
            .iter()
            .any(|system| system.system_id == "pinus_sylvestris"));
        assert_pinus_rotation(&report, &blueprint, &["灰毫", "远牙"]);
        assert!(
            report
                .teams
                .iter()
                .all(|team| !team.operators.iter().any(|name| name == "野鬃")),
            "未拥有的野鬃不得进入轮换"
        );
    }

    #[test]
    fn team_rotation_pinus_three_manufacturers_span_battle_record_rooms() {
        let (mut blueprint, operbox, instances, table) = fixtures();
        for room in &mut blueprint.rooms {
            if matches!(
                room.product,
                Some(crate::layout::RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::BattleRecord
                })
            ) {
                room.level = 2;
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

        let manufacturers = ["灰毫", "远牙", "野鬃"];
        assert_pinus_rotation(&report, &blueprint, &manufacturers);
        for shift in &report.shifts {
            if shift.assignment.iter_rooms().any(|room| {
                room.operators
                    .iter()
                    .any(|op| manufacturers.contains(&op.name.as_str()))
            }) {
                let occupied_rooms = shift
                    .assignment
                    .iter_rooms()
                    .filter(|room| {
                        room.operators
                            .iter()
                            .any(|op| manufacturers.contains(&op.name.as_str()))
                    })
                    .count();
                assert!(
                    occupied_rooms >= 2,
                    "标准 243 满配应证明红松成员允许跨作战记录站分布"
                );
            }
        }
    }

    #[test]
    fn team_rotation_binds_only_actual_syracusa_members() {
        use crate::operbox::default_operbox_full_e2_path;

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let base = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = base.excluding(&HashSet::from([
            "戴菲恩".to_string(),
            "推进之王".to_string(),
            "摩根".to_string(),
            "维娜·维多利亚".to_string(),
        ]));
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["但书", "八幡海铃", "伺夜", "贝洛内"] {
            assert!(operbox.owns(name), "full E2 fixture must own {name}");
        }

        let report = schedule_team_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 5,
                support_combinations_enable: true,
                ..Default::default()
            },
        )
        .unwrap();
        for forbidden in ["docus_syracusa", "syracusa_pair", "syracusa_cross_station"] {
            assert!(
                report
                    .peak_plan
                    .registry_claims
                    .iter()
                    .all(|claim| claim.system_id != forbidden),
                "叙拉古不得由历史 registry {forbidden} 激活: {:?}",
                report.peak_plan.registry_claims
            );
        }
        let peak_control: HashSet<_> = report
            .shifts
            .iter()
            .flat_map(|shift| shift.assignment.control_operators())
            .map(|op| op.name)
            .collect();
        assert!(
            peak_control.contains("八幡海铃"),
            "control={peak_control:?}"
        );
        let teams = operator_team_map(&report);
        let actual_syracusa: Vec<_> = ["伺夜", "贝洛内"]
            .into_iter()
            .filter(|name| teams.contains_key(*name))
            .collect();
        assert!(
            !actual_syracusa.is_empty(),
            "fixture 应至少自然选中一名叙拉古 consumer"
        );
        for name in &actual_syracusa {
            assert_eq!(
                teams.get(*name),
                teams.get("八幡海铃"),
                "实际入选的叙拉古贸易成员必须与八幡海铃同上同下: {name}"
            );
        }
        assert!(report.peak_plan.shift_binds.iter().any(|bind| {
            bind.operators.iter().any(|name| name == "八幡海铃")
                && actual_syracusa
                    .iter()
                    .all(|name| bind.operators.iter().any(|bound| bound == name))
        }));
        for name in ["伺夜", "贝洛内"]
            .into_iter()
            .filter(|name| !actual_syracusa.contains(name))
        {
            assert!(report
                .peak_plan
                .shift_binds
                .iter()
                .all(|bind| { !bind.operators.iter().any(|bound| bound == name) }));
        }
        let actual: Vec<_> = std::iter::once("八幡海铃")
            .chain(actual_syracusa.iter().copied())
            .collect();
        let mut active_shifts = 0;
        for shift in &report.shifts {
            assert_eq!(
                shift.assignment.control_operators().len(),
                5,
                "shift {} control center must stay full",
                shift.index + 1
            );
            let present: Vec<bool> = actual
                .iter()
                .map(|name| {
                    shift
                        .assignment
                        .iter_rooms()
                        .flat_map(|room| &room.operators)
                        .any(|operator| operator.name == *name)
                })
                .collect();
            assert!(
                present.iter().all(|flag| *flag == present[0]),
                "shift {} 实际叙拉古成员未同上同下: {:?}",
                shift.index + 1,
                present
            );
            active_shifts += usize::from(present[0]);
        }
        assert_eq!(active_shifts, 2, "实际叙拉古绑定组必须恰好上2休1");
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
        let _ = checked;
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
            for room in shift.assignment.iter_rooms() {
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
    fn team_rotation_252_automation_keeps_bound_control_out_of_gamma_production() {
        use crate::schedule::shift_bind::team_of_operator;

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
        assert!(report.peak_plan.selected_rules.iter().any(|selected| {
            selected.rule_id == "automation_group"
                && selected.alternative_id == "two_power_control_eunectes"
        }));

        let eunectes_team = team_of_operator(&report, "森蚺").expect("森蚺应归入自动化 cohort");
        assert_eq!(
            team_of_operator(&report, "Lancet-2"),
            Some(eunectes_team),
            "森蚺与 Lancet-2 必须属于同一 exact bind cohort"
        );
        for shift in &report.shifts {
            let raw_names: Vec<_> = shift
                .assignment
                .iter_rooms()
                .flat_map(|room| room.operators.iter().map(|operator| operator.name.as_str()))
                .collect();
            let unique_names: HashSet<_> = raw_names.iter().copied().collect();
            assert_eq!(
                raw_names.len(),
                unique_names.len(),
                "shift {} 每名干员必须全基建唯一",
                shift.index + 1
            );

            let facilities_for = |name: &str| -> Vec<FacilityKind> {
                shift
                    .assignment
                    .iter_rooms()
                    .filter(|room| room.operators.iter().any(|operator| operator.name == name))
                    .filter_map(|room| blueprint.room(&room.room_id).map(|room| room.kind))
                    .collect()
            };
            let active = shift.resting_team != eunectes_team;
            let eunectes_facilities = facilities_for("森蚺");
            if active {
                assert_eq!(
                    eunectes_facilities,
                    vec![FacilityKind::ControlCenter],
                    "shift {} 自动化工作班的森蚺只能在中枢",
                    shift.index + 1
                );
            } else {
                assert!(
                    eunectes_facilities.is_empty(),
                    "shift {} 自动化休息班不得安排森蚺",
                    shift.index + 1
                );
            }
            let lancet_facilities = facilities_for("Lancet-2");
            if active {
                assert_eq!(
                    lancet_facilities,
                    vec![FacilityKind::PowerPlant],
                    "shift {} 自动化工作班的 Lancet-2 只能在发电站",
                    shift.index + 1
                );
            }
            assert!(
                lancet_facilities
                    .iter()
                    .all(|facility| *facility != FacilityKind::Dormitory),
                "shift {} Lancet-2 休班不得进入宿舍",
                shift.index + 1
            );
        }
    }

    #[test]
    fn team_rotation_keeps_claimed_gnosis_karlan_chain_and_excludes_rin() {
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
            report
                .peak_plan
                .registry_claims
                .iter()
                .any(|claim| claim.system_id == "ling_jie_karlan"),
            "fixture 应由正式 System 认领灵知喀兰链"
        );
        let mut gnosis_shifts = 0;
        for shift in &report.shifts {
            if !shift
                .assignment
                .control_operators()
                .iter()
                .any(|op| op.name == "灵知")
            {
                continue;
            }
            gnosis_shifts += 1;
            assert!(
                shift.assignment.iter_rooms().any(|room| {
                    let Some(room_spec) = blueprint.room(&room.room_id) else {
                        return false;
                    };
                    room_spec.kind == FacilityKind::TradePost
                        && room.operators.iter().any(|operator| operator.name == "孑")
                        && room.operators.iter().any(|operator| {
                            instances
                                .tags_for(&operator.name, operator.tier())
                                .iter()
                                .any(|tag| tag == "cc.g.karlan")
                        })
                }),
                "灵知上岗班必须存在孑+谢拉格成员的实际贸易消费房"
            );
        }
        assert_eq!(gnosis_shifts, 2, "peak System 应按上2休1运行");
        assert!(
            report.shifts.iter().all(|shift| !shift
                .assignment
                .control_operators()
                .iter()
                .any(|op| op.name == "凛御银灰")),
            "自动 rotation 不得通过 plugin cohort、coverage repair 或 fallback 放行凛御银灰"
        );
    }

    #[test]
    fn team_rotation_feedback_010457_witch_group_keeps_trade_room() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for name in ["巫恋", "龙舌兰"] {
            assert!(operbox.owns(name), "full E2 fixture must own {name}");
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
            !witch_rooms.is_empty(),
            "feedback operbox should schedule witch group when its cohort is available"
        );
        let first_room = witch_rooms[0].1.clone();
        assert!(
            witch_rooms.iter().all(|(_, room)| *room == first_room),
            "巫恋/龙舌兰组跨班不应变更贸易站: {witch_rooms:?}"
        );
    }

    #[test]
    fn timed_fiammetta_witch_group_uses_aab_tailors_and_two_targets() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let full = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let operbox = full.excluding(&HashSet::from([
            "但书".to_string(),
            "可露希尔".to_string(),
            "清流".to_string(),
            "黑键".to_string(),
        ]));
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let trade_pool = build_trade_pool(&operbox.trade_roster(&instances), &instances, &table)
            .expect("trade pool");
        let beta_tailors: HashSet<_> = beta_tailor_names(&trade_pool, &table).into_iter().collect();
        assert!(beta_tailors.len() >= 2, "fixture must own two beta tailors");

        let report = schedule_timed_rotation(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &AssignBaseOptions {
                top_k: 20,
                ..Default::default()
            },
            TimedRotationProfile::Abc12_12_12,
        )
        .unwrap();

        let targets: HashSet<_> = report
            .shifts
            .iter()
            .filter_map(|shift| {
                shift
                    .fiammetta
                    .as_ref()
                    .map(|action| action.target.as_str())
            })
            .collect();
        assert_eq!(
            targets,
            HashSet::from(["巫恋", "龙舌兰"]),
            "teams={:?} active={:?} assignments={:?}",
            report.teams,
            report
                .shifts
                .iter()
                .map(|shift| shift.active_teams.clone())
                .collect::<Vec<_>>(),
            report
                .shifts
                .iter()
                .map(|shift| shift.assignment.operator_names())
                .collect::<Vec<_>>()
        );

        let mut rooms = Vec::new();
        let mut selected_tailors = Vec::new();
        for shift in &report.shifts {
            let room = shift
                .assignment
                .iter_rooms()
                .find(|room| {
                    room.operators
                        .iter()
                        .any(|operator| operator.name == "巫恋")
                        && room
                            .operators
                            .iter()
                            .any(|operator| operator.name == "龙舌兰")
                })
                .expect("Witch and Tequila must remain in one trade room");
            rooms.push(room.room_id.clone());
            selected_tailors.push(
                room.operators
                    .iter()
                    .find(|operator| beta_tailors.contains(&operator.name))
                    .expect("Witch group must use a beta tailor")
                    .name
                    .clone(),
            );
        }
        assert!(rooms.iter().all(|room| *room == rooms[0]));
        assert_eq!(selected_tailors[0], selected_tailors[1]);
        assert_ne!(selected_tailors[1], selected_tailors[2]);
    }

    #[test]
    fn team_rotation_warmup_manu_operator_keeps_factory_room() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        if !operbox.owns("阿罗玛") {
            return;
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

        let mut aroma_rooms = Vec::new();
        for shift in &report.shifts {
            if let Some(room_id) = room_containing_operator(
                &shift.assignment,
                &blueprint,
                "阿罗玛",
                FacilityKind::Factory,
            ) {
                aroma_rooms.push((shift.index, room_id));
            }
        }
        assert!(
            aroma_rooms.len() >= 2,
            "full E2 rotation should schedule 阿罗玛 in multiple shifts: {aroma_rooms:?}"
        );
        let first_room = aroma_rooms[0].1.clone();
        assert!(
            aroma_rooms.iter().all(|(_, room)| *room == first_room),
            "阿罗玛例行清扫跨班不应变更制造站: {aroma_rooms:?}"
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
            .iter_rooms()
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

    #[test]
    fn split_keeps_multi_room_production_bind_as_one_component() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut peak = BaseAssignment::default();
        peak.set_room("manu_1", vec![AssignedOperator::new("生产甲", 2)]);
        peak.set_room("manu_2", vec![AssignedOperator::new("生产乙", 2)]);
        peak.set_room("manu_3", vec![AssignedOperator::new("生产丙", 2)]);
        peak.set_room("control", vec![AssignedOperator::new("中枢甲", 2)]);
        let bind = RuntimeShiftBind {
            operators: ["生产甲", "生产乙", "生产丙", "中枢甲"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            on_shifts: 2,
            off_shifts: 1,
        };

        let [h1, h2] = split_production_facilities(&blueprint, &peak, &[bind], &[]).unwrap();
        let bound = [
            RoomId::from("manu_1"),
            RoomId::from("manu_2"),
            RoomId::from("manu_3"),
        ];
        assert!(
            bound.iter().all(|room| h1.manu.contains(room))
                || bound.iter().all(|room| h2.manu.contains(room)),
            "multi-room bind was split: h1={:?}, h2={:?}",
            h1.manu,
            h2.manu
        );
    }

    #[test]
    fn split_does_not_move_guarded_reserve_targets_between_halves() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut peak = BaseAssignment::default();
        let bound_rooms = ["manu_1", "manu_2", "manu_3", "manu_4", "power_1"];
        let operators: Vec<_> = bound_rooms
            .iter()
            .enumerate()
            .map(|(index, room)| {
                let name = format!("bound_{index}");
                peak.set_room(*room, vec![AssignedOperator::new(&name, 2)]);
                name
            })
            .collect();
        let bind = RuntimeShiftBind {
            operators,
            on_shifts: 2,
            off_shifts: 1,
        };
        let reserve = witch_rotation_reserve();

        let error = split_production_facilities(&blueprint, &peak, &[bind], &[reserve])
            .expect_err("schedule must not rescue a guarded pack by moving components");
        assert!(
            error
                .to_string()
                .contains("was not naturally pre-split across H1/H2 before schedule"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn gamma_coexist_excludes_bound_control_producer_when_its_half_rests() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut peak = BaseAssignment::default();
        peak.set_room("control", vec![AssignedOperator::new("八幡海铃", 2)]);
        peak.set_room("trade_1", vec![AssignedOperator::new("伺夜", 2)]);
        peak.set_room("trade_2", vec![AssignedOperator::new("古米", 2)]);
        let bind = RuntimeShiftBind {
            operators: vec!["八幡海铃".into(), "伺夜".into()],
            on_shifts: 2,
            off_shifts: 1,
        };
        let h1 = FacilityHalf {
            trade: vec![RoomId::from("trade_1")],
            ..Default::default()
        };
        let h2 = FacilityHalf {
            trade: vec![RoomId::from("trade_2")],
            ..Default::default()
        };
        let alpha = production_half_from_peak(&peak, &h1);
        let beta = production_half_from_peak(&peak, &h2);

        // 绑定 producer 所在半区休息时，不进入 γ 搜索上下文（bind 半区过滤保留）。
        let while_h1_rests = gamma_production_coexist_seed(
            &blueprint,
            &BaseAssignment::default(),
            &peak,
            std::slice::from_ref(&bind),
            &h1,
            &beta,
        );
        assert!(!while_h1_rests
            .assignment
            .control_operators()
            .iter()
            .any(|op| op.name == "八幡海铃"));
        let resting_layout = resolve_base(
            &blueprint,
            &while_h1_rests.assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap()
        .layout_snapshot();
        assert!(!resting_layout.global_inject.has_dynamic_trade_inject());

        // 绑定 producer 所在半区活跃时保留，注入按实际在场结算。
        let while_h1_active = gamma_production_coexist_seed(
            &blueprint,
            &BaseAssignment::default(),
            &peak,
            std::slice::from_ref(&bind),
            &h2,
            &alpha,
        );
        let active_layout = resolve_base(
            &blueprint,
            &while_h1_active.assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap()
        .layout_snapshot();
        assert!(active_layout.global_inject.has_dynamic_trade_inject());
        // 八幡海铃只按各贸易房站内叙拉古人数结算；trade_1 有伺夜 → 5%，空贸易房 → 0%。
        let in_room = std::collections::HashMap::from([("cc.g.siracusa".to_string(), 1)]);
        let empty = std::collections::HashMap::new();
        assert_eq!(
            active_layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(
                    &active_layout.trade_tagged_count_sum,
                    &in_room
                ),
            5.0
        );
        assert_eq!(
            active_layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(
                    &active_layout.trade_tagged_count_sum,
                    &empty
                ),
            0.0
        );

        // 未绑定 producer 允许进入 γ 候选（ADR 0004 D6：
        // 收益按最终实际在场结算，不后置新建 cohort）。
        let without_peak_bind = gamma_production_coexist_seed(
            &blueprint,
            &BaseAssignment::default(),
            &peak,
            &[],
            &h2,
            &alpha,
        );
        assert!(without_peak_bind
            .assignment
            .control_operators()
            .iter()
            .any(|op| op.name == "八幡海铃"));
    }

    #[test]
    fn gamma_unbound_consumer_benefit_settles_by_presence_without_bind() {
        // ADR 0004 D6：γ 班允许普通搜索新增未绑定叙拉古 consumer；
        // 不要求轮换层后置新建 cohort，注入按该班最终实际在场人数结算。
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut peak = BaseAssignment::default();
        peak.set_room("control", vec![AssignedOperator::new("八幡海铃", 2)]);
        peak.set_room("trade_1", vec![AssignedOperator::new("伺夜", 2)]);
        let bind = RuntimeShiftBind {
            operators: vec!["八幡海铃".into(), "伺夜".into()],
            on_shifts: 2,
            off_shifts: 1,
        };
        let h1 = FacilityHalf {
            trade: vec![RoomId::from("trade_1")],
            ..Default::default()
        };
        let h2 = FacilityHalf {
            trade: vec![RoomId::from("trade_2")],
            ..Default::default()
        };
        let alpha = production_half_from_peak(&peak, &h1);

        // γ 接替 H2 时：活跃半区 α 的绑定 producer 保留在 seed，
        // γ 再新增未绑定叙拉古成员（贝洛内）进 trade_2。
        let ctx = gamma_production_coexist_seed(
            &blueprint,
            &BaseAssignment::default(),
            &peak,
            std::slice::from_ref(&bind),
            &h2,
            &alpha,
        );
        let mut gamma = ctx.assignment;
        gamma.set_room("trade_2", vec![AssignedOperator::new("贝洛内", 2)]);
        let layout = resolve_base(
            &blueprint,
            &gamma,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap()
        .layout_snapshot();
        // 八幡海铃注入按各贸易房站内叙拉古人数结算：伺夜（trade_1）、贝洛内（trade_2）
        // 各贡献 5%，全站人数不得跨站串入统一全局值。
        let all_counts = &layout.trade_tagged_count_sum;
        let in_room = std::collections::HashMap::from([("cc.g.siracusa".to_string(), 1)]);
        assert_eq!(
            layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(all_counts, &in_room),
            5.0,
            "trade_1 站内 1 名叙拉古（伺夜）→ 5%"
        );
        assert_eq!(
            layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(all_counts, &in_room),
            5.0,
            "trade_2 站内 1 名叙拉古（贝洛内）同样只贡献 5%"
        );
        let empty = std::collections::HashMap::new();
        assert_eq!(
            layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(all_counts, &empty),
            0.0,
            "无叙拉古的贸易房不应获得注入（D6）"
        );
        // γ 搜索上下文是纯 assignment，不派生 bind/cohort：
        // 后置 bind 只能来自 peak_plan（shift_binds_from_plan 唯一生成）。
    }

    #[test]
    fn final_validator_rejects_duplicate_operator_across_rooms() {
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            vec![
                AssignedOperator::new("重复干员", 2),
                AssignedOperator::new("control-1", 2),
                AssignedOperator::new("control-2", 2),
                AssignedOperator::new("control-3", 2),
                AssignedOperator::new("control-4", 2),
            ],
        );
        assignment.set_room("manu_1", vec![AssignedOperator::new("重复干员", 2)]);
        let shift = TeamShiftResult {
            index: 2,
            plan_index: 0,
            duration_hours: 6.0,
            active_teams: vec![TeamLabel::Gamma, TeamLabel::Alpha],
            resting_team: TeamLabel::Beta,
            assignment,
            fiammetta: None,
            dorm_rest: None,
            drone: None,
            transition: ShiftTransition::ReplaceRooms,
            efficiencies: ShiftEfficiencies::default(),
            weighted_trade: Efficiency::default(),
            weighted_manufacture: Efficiency::default(),
            weighted_power: Efficiency::default(),
        };

        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let error = validate_final_rotation_invariants(&blueprint, &[shift], &[], &[]).unwrap_err();
        assert!(error.to_string().contains("appears in multiple room slots"));
    }

    #[test]
    fn mood_gap_decay_validator_rejects_adjacent_short_shifts() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let make_shift = |index: usize, duration_hours: f64, has_leadfoot: bool| {
            let mut assignment = BaseAssignment::default();
            if has_leadfoot {
                assignment.set_room(
                    "manu_1",
                    vec![AssignedOperator::from_progress(
                        "铅踝",
                        crate::roster::OperatorProgress::new(1, 60, 4),
                    )],
                );
            }
            TeamShiftResult {
                index,
                plan_index: 0,
                duration_hours,
                active_teams: vec![TeamLabel::Alpha, TeamLabel::Beta],
                resting_team: TeamLabel::Gamma,
                assignment,
                fiammetta: None,
                dorm_rest: None,
                drone: None,
                transition: ShiftTransition::ReplaceRooms,
                efficiencies: ShiftEfficiencies::default(),
                weighted_trade: Efficiency::default(),
                weighted_manufacture: Efficiency::default(),
                weighted_power: Efficiency::default(),
            }
        };

        let one_short_shift = vec![
            make_shift(0, 12.0, false),
            make_shift(1, 6.0, true),
            make_shift(2, 6.0, false),
        ];
        assert!(validate_mood_gap_decay_work_spans(
            &blueprint,
            &instances,
            &table,
            TimedRotationProfile::Abc12_6_6,
            &one_short_shift,
        )
        .is_ok());

        let adjacent_short_shifts = vec![
            make_shift(0, 12.0, false),
            make_shift(1, 6.0, true),
            make_shift(2, 6.0, true),
        ];
        let error = validate_mood_gap_decay_work_spans(
            &blueprint,
            &instances,
            &table,
            TimedRotationProfile::Abc12_6_6,
            &adjacent_short_shifts,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("continuous manufacture work reaches 12.0h"));
    }

    #[test]
    fn split_rejects_bind_member_missing_from_peak() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let peak = BaseAssignment::default();
        let bind = RuntimeShiftBind {
            operators: vec!["不存在".into(), "也不存在".into()],
            on_shifts: 2,
            off_shifts: 1,
        };
        let error = split_production_facilities(&blueprint, &peak, &[bind], &[]).unwrap_err();
        assert!(error.to_string().contains("missing from peak"));
    }

    #[test]
    fn active_dependency_requires_support_only_when_consumer_is_active() {
        let dependency = crate::layout::ActiveDependency {
            consumers: vec!["receiver".into()],
            required: vec!["support".into()],
        };
        let mut shifts = Vec::new();
        for index in 0..3 {
            let mut assignment = BaseAssignment::default();
            assignment.set_room(
                "control",
                (0..5)
                    .map(|slot| AssignedOperator::new(format!("control-{slot}"), 0))
                    .collect(),
            );
            assignment.set_room("meeting", vec![AssignedOperator::new("support", 0)]);
            if index != 1 {
                assignment.set_room("manu_1", vec![AssignedOperator::new("receiver", 2)]);
            }
            shifts.push(TeamShiftResult {
                index,
                plan_index: 0,
                duration_hours: if index == 0 { 12.0 } else { 6.0 },
                active_teams: vec![TeamLabel::Alpha, TeamLabel::Beta],
                resting_team: TeamLabel::Gamma,
                assignment,
                fiammetta: None,
                dorm_rest: None,
                drone: None,
                transition: ShiftTransition::ReplaceRooms,
                efficiencies: ShiftEfficiencies::default(),
                weighted_trade: Efficiency::default(),
                weighted_manufacture: Efficiency::default(),
                weighted_power: Efficiency::default(),
            });
        }

        assert!(validate_final_rotation_invariants(
            &BaseBlueprint::template_243_use_this().unwrap(),
            &shifts,
            &[],
            std::slice::from_ref(&dependency),
        )
        .is_ok());
        shifts[0].assignment.remove_room("meeting");
        assert!(validate_final_rotation_invariants(
            &BaseBlueprint::template_243_use_this().unwrap(),
            &shifts,
            &[],
            std::slice::from_ref(&dependency),
        )
        .is_err());
    }

    #[test]
    fn active_dependency_pins_only_declared_required_member_into_shared() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut peak = BaseAssignment::default();
        peak.set_room(
            "meeting",
            vec![
                AssignedOperator::new("required-support", 0),
                AssignedOperator::new("unrelated-old-member", 0),
            ],
        );
        let mut shared = pinned_assignment_excluding(&peak, &blueprint, &HashSet::new());
        assert!(shared.operators_in(&RoomId::from("meeting")).is_empty());

        pin_active_dependency_required(
            &mut shared,
            &peak,
            &[crate::layout::ActiveDependency {
                consumers: vec!["receiver".into()],
                required: vec!["required-support".into()],
            }],
            &[],
        )
        .unwrap();

        let names: Vec<_> = shared
            .operators_in(&RoomId::from("meeting"))
            .iter()
            .map(|operator| operator.name.as_str())
            .collect();
        assert_eq!(names, vec!["required-support"]);
    }

    #[test]
    fn continuous_trade_core_is_only_served_when_normal_s1_search_selects_it() {
        let (blueprint, full, instances, table) = user_243_fixtures();
        let options = AssignBaseOptions::default();

        let with_fiammetta =
            schedule_team_rotation(&blueprint, &full, &instances, &table, &options).unwrap();
        assert!(with_fiammetta
            .peak_plan
            .continuous_roles
            .iter()
            .any(|role| role.operator == "但书"));
        assert!(with_fiammetta
            .shifts
            .iter()
            .any(|shift| shift.fiammetta.as_ref().is_some_and(|action| {
                action.target != "菲亚梅塔"
                    && crate::schedule::operator_in_shift(&with_fiammetta, 0, &action.target)
                    && crate::schedule::operator_in_shift(&with_fiammetta, 2, &action.target)
            })));

        let without = full.excluding(&HashSet::from(["菲亚梅塔".to_string()]));
        let without_fiammetta =
            schedule_team_rotation(&blueprint, &without, &instances, &table, &options).unwrap();
        let active = without_fiammetta
            .shifts
            .iter()
            .filter(|shift| {
                crate::schedule::operator_in_shift(&without_fiammetta, shift.index, "但书")
            })
            .count();
        assert_eq!(
            active, 2,
            "无菲亚梅塔时必须显式遵守正常轮休，不能伪造连续上班"
        );
    }
}
