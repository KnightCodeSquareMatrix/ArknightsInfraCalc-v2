use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use crate::control::{solve_control, ControlRoomInput};
use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::global_resource::GlobalResourceKey;
use crate::instances::OperatorInstances;
use crate::layout::{
    assign_manu_room_with_anchors, assign_shift_with_plan_skip, assign_shift_with_preclaimed_plan,
    assign_team_gamma_half, beta_tailor_names, fill_workplace_vacancies, ordinary_workplace,
    pinned_assignment_excluding, refill_manu_room_with_core, refill_trade_room_with_core,
    refill_witch_room_with_tailor, resolve_base, ActivatedSystem, AnchorFillPolicy,
    AssignBaseOptions, AssignShiftMode, AssignedOperator, AssignmentPlan, BaseAssignment,
    BaseBlueprint, FacilityKind, LayoutContext, ReserveReusePolicy, ResolvedRoleReserve, RoomId,
    RoomProduct, SelectedRuleAlternative, ShiftBind, SlotFill, SystemAnchor, TradeRolePreference,
};
use crate::mood::{
    default_mood_model, dorm_recovery_rates, shift_eta_with_instances, DormOccupant, MoodModel,
};
use crate::notes::annotate_assignment;
use crate::operbox::OperBox;
use crate::pool::{
    build_control_pool_with_fillers, build_manufacture_pool, build_power_pool_with_fillers,
    build_trade_pool, ManuPool, ManuPoolEntry,
};
use crate::search::control_efficiency_fill_sort_weight;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput};
use crate::trade::legality::witch_core_station_ok;
use crate::trade::shortcut::match_witch_group_shortcut;
use crate::trade::solve_trade_with_shift;

use super::base_rotation::evaluate_base_assignment_efficiencies;
use super::control_rotation::{assert_final_control_rotation, ControlRotationClosure};
use super::cycle_mood::validate_cycle_mood;
use super::dorm_finalizer;
use super::fiammetta_events;
use super::rotation_scoring::clear_production_efficiencies;
use super::rotation_types::{
    validate_rotation_duration_hours, DailyProduction, DailyTotals, DormRestPlan, DroneAction,
    FacilityHalf, FiammettaShiftAction, RoomEfficiencyLine, RuntimeShiftBind, ShiftEfficiencies,
    ShiftTransition, TeamAssignment, TeamLabel, TeamRotationReport, TeamShiftResult,
    TimedRotationProfile,
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
const ORIGINIUM_PAIR_SYSTEM_ID: &str = "rotation_originium_pair";
const ORIGINIUM_PAIR_WARNING: &str = "originium pair preclaim failed";
const ORIGINIUM_PAIR_OPERATORS: [&str; 2] = ["泡泡", "火神"];

#[derive(Debug, Clone)]
enum OriginiumPairPreclaim {
    Inactive,
    Active(RoomId),
}

struct OriginiumPairContext {
    preclaim: OriginiumPairPreclaim,
    warning: Option<String>,
}

impl OriginiumPairContext {
    /// The selected preclaim room is the scheduling fact.  Consumers must not
    /// rediscover it from "both operators happen to be in this room" after a
    /// later stage has moved one of them.
    fn from_peak_plan(plan: &AssignmentPlan) -> Result<Self> {
        let rooms: Vec<_> = plan
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID)
            .filter_map(|anchor| anchor.room_id.clone())
            .collect();
        let mut unique = rooms.clone();
        unique.sort_by(|left, right| left.0.cmp(&right.0));
        unique.dedup();
        match unique.as_slice() {
            [] => Ok(Self {
                preclaim: OriginiumPairPreclaim::Inactive,
                warning: None,
            }),
            [room_id]
                if rooms.len() == ORIGINIUM_PAIR_OPERATORS.len()
                    && ORIGINIUM_PAIR_OPERATORS.iter().all(|name| {
                        plan.anchors.iter().any(|anchor| {
                            anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID
                                && anchor.operator == *name
                                && anchor.room_id.as_ref() == Some(room_id)
                        })
                    }) =>
            {
                Ok(Self {
                    preclaim: OriginiumPairPreclaim::Active(room_id.clone()),
                    warning: None,
                })
            }
            _ => Err(Error::msg(format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: preclaim Plan has inconsistent room anchors"
            ))),
        }
    }

    fn for_ordinary_rotation(blueprint: &BaseBlueprint, operbox: &OperBox) -> Result<Self> {
        let eligible = operbox.elite_of("泡泡").is_some_and(|elite| elite >= 1)
            && operbox.elite_of("火神").is_some_and(|elite| elite >= 2);
        if !eligible {
            return Ok(Self {
                preclaim: OriginiumPairPreclaim::Inactive,
                warning: None,
            });
        }
        // A layout without a legal room is not a failed preference: the composition
        // cannot apply, so ordinary scheduling remains the normal result and stays silent.
        let Some(room_id) = Self::legal_room(blueprint) else {
            return Ok(Self {
                preclaim: OriginiumPairPreclaim::Inactive,
                warning: None,
            });
        };
        Ok(Self {
            preclaim: OriginiumPairPreclaim::Active(room_id),
            warning: None,
        })
    }

    fn room_id(&self) -> Option<&RoomId> {
        match &self.preclaim {
            OriginiumPairPreclaim::Inactive => None,
            OriginiumPairPreclaim::Active(room_id) => Some(room_id),
        }
    }

    fn matches_room(&self, room_id: &RoomId) -> bool {
        self.room_id() == Some(room_id)
    }

    fn legal_room(blueprint: &BaseBlueprint) -> Option<RoomId> {
        blueprint
            .rooms
            .iter()
            .find(|room| {
                room.kind == FacilityKind::Factory
                    && room.level == 3
                    && room.operator_capacity() >= 3
                    && matches!(
                        room.product.as_ref(),
                        Some(RoomProduct::Factory {
                            recipe: crate::types::RecipeKind::Originium
                        })
                    )
            })
            .map(|room| room.id.clone())
    }

    fn warning(&self) -> Option<String> {
        self.warning.clone()
    }
}

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

fn abc_control_work_hours(team: TeamLabel, shift_hours: [f64; 3]) -> f64 {
    match team {
        TeamLabel::Alpha => shift_hours[0] + shift_hours[2],
        TeamLabel::Beta => shift_hours[0] + shift_hours[1],
        TeamLabel::Gamma => shift_hours[1] + shift_hours[2],
    }
}

/// Freeze optional ABC control candidates with one-person, full-control mood
/// admission before the shift loop. Required policy members remain governed by
/// Plan admission; final cycle diagnostics never rebuild these cohorts.
fn freeze_abc_control_roster(
    team_ctrl: &mut HashMap<TeamLabel, Vec<String>>,
    pool: &crate::pool::ControlPool,
    shift_hours: [f64; 3],
    initial_mood: f64,
    model: &MoodModel,
    protected: &HashSet<String>,
) {
    for (team, names) in team_ctrl.iter_mut() {
        let work_hours = abc_control_work_hours(*team, shift_hours);
        names.retain(|name| {
            if protected.contains(name) {
                return true;
            }
            let Some(entry) = pool.entry(name) else {
                return true;
            };
            crate::mood::control_roster_fits(
                model,
                &entry.name,
                entry.progress.elite,
                initial_mood,
                work_hours,
            )
        });
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
            shared.construction_warnings.push(format!(
                "active dependency required operator {name} missing from peak"
            ));
            continue;
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
    shift_hours: f64,
    excluded: &HashSet<String>,
) -> Vec<AssignedOperator> {
    crate::office::rank_candidates(operbox, layout, _mood, shift_hours, excluded)
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

fn assign_office_for_shift(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_bound_names: &HashSet<String>,
    bound_names: &HashSet<String>,
    candidates: &[AssignedOperator],
) -> Result<()> {
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
            assignment.construction_warnings.push(format!(
                "active bound office operator {name} is already occupied in the current shift"
            ));
            continue;
        }
        let Some(candidate) = available_candidates
            .iter()
            .find(|candidate| candidate.name == name.as_str())
        else {
            assignment.construction_warnings.push(format!(
                "active bound office operator {name} is unavailable in the current shift"
            ));
            continue;
        };
        let Some((room_id, _)) = office_rooms
            .iter()
            .find(|(room_id, capacity)| assignment.operators_in(room_id).len() < *capacity)
        else {
            assignment.construction_warnings.push(format!(
                "active bound office operator {name} has no available office seat"
            ));
            continue;
        };
        let mut operators = assignment.operators_in(room_id).to_vec();
        operators.push(candidate.clone());
        assignment.set_room(room_id.clone(), operators);
        used.insert(candidate.name.clone());
    }

    crate::office::fill_vacancies(
        assignment,
        blueprint,
        &available_candidates,
        |_, _, _, candidate| !bound_names.contains(&candidate.name),
    );
    Ok(())
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
    mood_state: Option<&HashMap<String, f64>>,
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
                        .and_then(|mood| mood.get(&member.name).copied())
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
    shift_hours: f64,
    mood_state: Option<&HashMap<String, f64>>,
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
                    let mut current = trial.operators_in(&room.id).to_vec();
                    current.push((*candidate).clone());
                    trial.set_room(room.id.clone(), current);
                    true
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
                        let mut current = ordinary.operators_in(&room.id).to_vec();
                        current.push(candidate.clone());
                        ordinary.set_room(room.id.clone(), current);
                        if ordinary.operators_in(&room.id).len() == room.operator_capacity() {
                            break;
                        }
                    }
                    if meeting_pair_score(
                        trial.operators_in(&room.id),
                        layout,
                        shift_hours,
                        mood_state,
                        extra_recruit_slots,
                    )? > meeting_pair_score(
                        ordinary.operators_in(&room.id),
                        layout,
                        shift_hours,
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
                {
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
                        ordinary.set_room(
                            room.id.clone(),
                            vec![operators[0].clone(), candidate.clone()],
                        );
                        break;
                    }
                    if meeting_pair_score(
                        &pair_members,
                        layout,
                        shift_hours,
                        mood_state,
                        extra_recruit_slots,
                    )? > meeting_pair_score(
                        ordinary.operators_in(&room.id),
                        layout,
                        shift_hours,
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
        && !reserved.contains("铃兰")
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
            used.insert(operator.name.clone());
            operators.push(operator.clone());
        }
        if !operators.is_empty() {
            assignment.set_room(room.id.clone(), operators);
        }
    }
}

struct RotationVacancyCommit {
    assignment: BaseAssignment,
    placements: Vec<(RoomId, AssignedOperator)>,
    continuation_indices: Vec<usize>,
    memberships: Vec<(String, TeamLabel)>,
}

/// Read-only trial of the existing ordinary fill policy for one shift.
fn rotation_vacancy_commit(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    index: usize,
) -> Result<Option<RotationVacancyCommit>> {
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
    let roster = operbox.roster_excluding(&excluded);
    let mut candidates: Vec<_> = roster
        .names()
        .filter_map(|name| {
            roster
                .progress(name)
                .map(|progress| AssignedOperator::from_progress(name, progress))
        })
        .collect();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
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
        return Ok(None);
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
            true
        },
    )?;
    let memberships: Vec<_> = placements
        .iter()
        .filter_map(|(_, operator)| {
            team_for(&operator.name).map(|team| (operator.name.clone(), team))
        })
        .collect();
    if placements.is_empty() {
        return Ok(None);
    }
    Ok(Some(RotationVacancyCommit {
        assignment,
        placements,
        continuation_indices,
        memberships,
    }))
}

fn fill_rotation_vacancies(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &mut TeamRotationReport,
) -> Result<()> {
    for index in 0..report.shifts.len() {
        let Some(RotationVacancyCommit {
            assignment,
            placements,
            continuation_indices,
            memberships,
        }) = rotation_vacancy_commit(blueprint, operbox, instances, table, report, index)?
        else {
            continue;
        };
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
    originium_pair: &OriginiumPairContext,
    issues: &mut Vec<String>,
) -> Result<[FacilityHalf; 2]> {
    let room_kind: HashMap<_, _> = blueprint
        .rooms
        .iter()
        .map(|room| (room.id.clone(), room.kind))
        .collect();
    let present = peak.operator_names();
    let construction_binds: Vec<Vec<String>> = binds
        .iter()
        .map(|bind| {
            for name in &bind.operators {
                if !present.contains(name) {
                    issues.push(format!("shift bind operator {name} missing from peak"));
                }
            }
            bind.operators
                .iter()
                .filter(|name| present.contains(*name))
                .cloned()
                .collect()
        })
        .collect();
    let mut half_components = crate::layout::pack_production_components(
        blueprint,
        peak,
        construction_binds.iter().map(Vec::as_slice),
    )
    .map_err(Error::msg)?;
    if originium_pair.room_id().is_some_and(|room_id| {
        !half_components
            .iter()
            .flatten()
            .any(|component| component.contains(room_id))
    }) {
        issues.push(format!(
            "{ORIGINIUM_PAIR_SYSTEM_ID}: frozen pair room {} is absent from production halves",
            originium_pair.room_id().expect("checked preclaim room").0
        ));
    }

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
            issues.push(format!(
                "rotation reserve {} has no production component in H1/H2",
                reserve.reserve_id
            ));
            continue;
        }
        if reserve.require_pre_split_halves {
            issues.push(format!(
                "rotation reserve {} was not naturally pre-split across H1/H2 before schedule",
                reserve.reserve_id
            ));
            continue;
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
            issues.push(format!(
                "rotation reserve {} eligible rooms are inseparable across H1/H2",
                reserve.reserve_id
            ));
            continue;
        };
        let moved_component = &half_components[source][component_index];
        if originium_pair
            .room_id()
            .is_some_and(|room_id| moved_component.contains(room_id))
        {
            issues.push(format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: rotation reserve {} would move the frozen pair room component",
                reserve.reserve_id
            ));
            continue;
        }
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
        return Err(Error::schedule_quality(format!(
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
) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    for reserve in reserves {
        let mut trial_h1 = assignment_h1.clone();
        let mut trial_h2 = assignment_h2.clone();
        let mut trial_used_h1 = used_h1.clone();
        let mut trial_used_h2 = used_h2.clone();
        let outcome = (|| -> Result<()> {
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
                            &mut trial_h1,
                            &mut trial_used_h1,
                        )?;
                    } else if let Some(target) = target_h2 {
                        place_rotation_reserve_at(
                            blueprint,
                            operbox,
                            reserve,
                            &target,
                            &mut trial_h2,
                            &mut trial_used_h2,
                        )?;
                    } else {
                        return Err(Error::schedule_quality(format!(
                            "rotation reserve {} has no eligible H1/H2 target",
                            reserve.reserve_id
                        )));
                    }
                }
                ReserveReusePolicy::EveryEligibleHalf => {
                    let target_h1 = target_h1.ok_or_else(|| {
                        Error::schedule_quality(format!(
                            "rotation reserve {} missing eligible H1 target",
                            reserve.reserve_id
                        ))
                    })?;
                    let target_h2 = target_h2.ok_or_else(|| {
                    Error::schedule_quality(format!(
                        "rotation reserve {} missing eligible H2 target; eligible={:?}; H1 trade={:?}; H2 trade={:?}",
                        reserve.reserve_id, reserve.eligible_rooms, h1.trade, h2.trade
                    ))
                })?;
                    place_rotation_reserve_at(
                        blueprint,
                        operbox,
                        reserve,
                        &target_h1,
                        &mut trial_h1,
                        &mut trial_used_h1,
                    )?;
                    place_rotation_reserve_at(
                        blueprint,
                        operbox,
                        reserve,
                        &target_h2,
                        &mut trial_h2,
                        &mut trial_used_h2,
                    )?;
                }
            }
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                *assignment_h1 = trial_h1;
                *assignment_h2 = trial_h2;
                *used_h1 = trial_used_h1;
                *used_h2 = trial_used_h2;
            }
            Err(Error::ScheduleQuality(message)) => issues.push(message),
            Err(error) => return Err(error),
        }
    }
    Ok(issues)
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
                    Error::schedule_quality(format!(
                        "rotation reserve {} missing {half} target before postprocess",
                        reserve.reserve_id
                    ))
                })?;
                if !matches_reserve(assignment, &target, reserve) {
                    return Err(Error::schedule_quality(format!(
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
            | TimedRotationProfile::GenericAbc
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

/// Generic two/three-shift requests use one action slot for each state longer
/// than 12 hours.  Named profiles retain their established slot policy above.
fn fiammetta_action_slots_for_shifts(
    profile: TimedRotationProfile,
    shifts: &[TeamShiftResult],
) -> Vec<usize> {
    if matches!(
        profile,
        TimedRotationProfile::GenericMainBackup | TimedRotationProfile::GenericAbc
    ) {
        return shifts
            .iter()
            .enumerate()
            .filter_map(|(index, shift)| (shift.duration_hours > 12.0).then_some(index))
            .collect();
    }
    fiammetta_action_slots(profile, shifts.len())
}

fn validate_fiammetta_action_policy(
    profile: TimedRotationProfile,
    shifts: &[TeamShiftResult],
) -> Result<()> {
    let slots = fiammetta_action_slots_for_shifts(profile, shifts);
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
            fiammetta_assignment_target_room(blueprint, assignment, target)
                .map(|room| (target.to_string(), room))
        })
}

pub(crate) fn fiammetta_assignment_target_room(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    target: &str,
) -> Option<RoomId> {
    assignment.iter_rooms().find_map(|room| {
        let production = blueprint.room(&room.room_id).is_some_and(|spec| {
            matches!(
                spec.kind,
                FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
            )
        });
        (production
            && room
                .operators
                .iter()
                .any(|operator| operator.name == target))
        .then(|| room.room_id.clone())
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
        return Err(Error::FiammettaCandidateUnavailable(format!(
            "shift {} fiammetta_dorm_admission failed: Fiammetta is already assigned outside a dorm",
            shift.index + 1
        )));
    }

    let dorms = blueprint.rooms_of(FacilityKind::Dormitory);
    let fiammetta_room = if has_action {
        let progress = operbox.progress_of(FIAMMETTA_NAME).ok_or_else(|| {
            Error::FiammettaCandidateUnavailable(
                "fiammetta action requires an owned Fiammetta operator".into(),
            )
        })?;
        let room = dorms
            .iter()
            .find(|room| {
                preserved_by_room.get(&room.id).map_or(0, Vec::len)
                    < room.dorm_bed_count().max(1) as usize
            })
            .ok_or_else(|| {
                Error::FiammettaCandidateUnavailable(format!(
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
            .ok_or_else(|| Error::schedule_quality("dorm recovery target is not a dormitory"))?;
        let capacity = room.dorm_bed_count().max(1) as usize;
        if occupants.len() + 3 > capacity {
            return Err(Error::schedule_quality(format!(
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
                Error::schedule_quality(format!(
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
        return Err(Error::schedule_quality(format!(
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
            return Err(Error::FiammettaCandidateUnavailable(format!(
                "shift {} fiammetta_dorm_first failed",
                shift.index + 1
            )));
        }
    }
    Ok(())
}

/// Private construction state; only settlement publishes a public report.
#[derive(Clone)]
struct RotationDraft {
    report: TeamRotationReport,
    prepared: bool,
}
impl std::ops::Deref for RotationDraft {
    type Target = TeamRotationReport;
    fn deref(&self) -> &Self::Target {
        &self.report
    }
}
impl std::ops::DerefMut for RotationDraft {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.report
    }
}

fn complete_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    initial_mood: f64,
    fiammetta_enable: bool,
    mut report: RotationDraft,
) -> Result<TeamRotationReport> {
    let preserve_actions = report.shifts.iter().any(|shift| shift.fiammetta.is_some())
        && (report.profile.is_ordinary_abc()
            || report.profile == TimedRotationProfile::MainBackup24_24
            || report.profile == TimedRotationProfile::Fiammetta8_8_4_4);
    prepare_rotation_state(
        blueprint,
        operbox,
        instances,
        table,
        &mut report,
        preserve_actions,
    )?;
    finalize_timed_rotation(
        blueprint,
        operbox,
        instances,
        table,
        initial_mood,
        fiammetta_enable,
        report,
    )
}

/// The only mutable completion owner. Specialized action-dependent candidates
/// must retain their promised actions; ordinary proposals are confirmed first.
fn prepare_rotation_state(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &mut RotationDraft,
    preserve_actions: bool,
) -> Result<()> {
    if report.prepared {
        return Ok(());
    }
    let state = report;
    let report = &mut state.report;
    let model = default_mood_model()?;
    // Ordinary proposals have no cross-team dependency; confirm them before
    // any new workplace uses their recovery. Specialized candidates validate
    // their promised actions before reaching this boundary.
    if preserve_actions {
        fiammetta_events::validate_actions(&report.shifts, &model, blueprint, instances)
            .map_err(|error| Error::FiammettaCandidateUnavailable(error.to_string()))?;
    } else {
        report.warnings.extend(
            fiammetta_events::retain_ready_actions(
                &mut report.shifts,
                &model,
                blueprint,
                instances,
            )
            .into_iter()
            .map(|message| format!("{FINAL_INVARIANT_WARNING_PREFIX} {message}")),
        );
    }
    for shift in &mut report.shifts {
        if shift.transition == ShiftTransition::FiammettaOnly && shift.fiammetta.is_none() {
            shift.transition = ShiftTransition::ReplaceRooms;
            shift.drone = drone_action(blueprint, shift.transition);
        }
        let mut trial = shift.clone();
        match finalize_shift_dorms(blueprint, operbox, &mut trial) {
            Ok(()) => *shift = trial,
            Err(error @ Error::FiammettaCandidateUnavailable(_)) if preserve_actions => {
                return Err(error)
            }
            Err(Error::FiammettaCandidateUnavailable(message)) => {
                shift.fiammetta = None;
                shift.transition = ShiftTransition::ReplaceRooms;
                shift.drone = drone_action(blueprint, shift.transition);
                report.warnings.push(format!(
                    "{FINAL_INVARIANT_WARNING_PREFIX} {message}; action omitted"
                ));
            }
            Err(Error::ScheduleQuality(message)) => report
                .warnings
                .push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {message}")),
            Err(error) => return Err(error),
        }
    }
    if let Err(error) = fill_rotation_vacancies(blueprint, operbox, instances, table, report) {
        return Err(error);
    }
    for shift in &mut report.shifts {
        // Vacancy completion can append a gold-flow member after its reader. Normalize
        // before dorm finalization also consumes this order; preserve dorm order itself.
        shift.assignment.sort_trade_gold_flow_operators(blueprint);
        print_trade_rotation_debug(
            &format!("finalize shift {} input", shift.index + 1),
            &shift.assignment,
        );
    }
    let dorm_warnings = dorm_finalizer::finalize_dorms(blueprint, operbox, &mut report.shifts)?
        .into_iter()
        .map(dorm_finalizer::render_warning)
        .collect::<Vec<_>>();
    report.warnings.extend(dorm_warnings);
    if preserve_actions {
        fiammetta_events::validate_actions(&report.shifts, &model, blueprint, instances)
            .map_err(|error| Error::FiammettaCandidateUnavailable(error.to_string()))?;
    }
    state.prepared = true;
    Ok(())
}

fn rotation_dorm_warnings(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    shifts: &[TeamShiftResult],
) -> Result<Vec<String>> {
    Ok(dorm_finalizer::warnings(blueprint, operbox, shifts)?
        .into_iter()
        .map(dorm_finalizer::render_warning)
        .collect())
}

fn finalize_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    initial_mood: f64,
    fiammetta_enable: bool,
    report: RotationDraft,
) -> Result<TeamRotationReport> {
    let mut report = report.report;
    let total_started = Instant::now();
    let cycle_hours =
        validate_rotation_duration_hours(report.shifts.iter().map(|shift| shift.duration_hours))?;
    let total_before = crate::profile::solver_perf_snapshot();
    let stage_started = Instant::now();
    let stage_before = total_before;
    let mut stage_after;
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
                Err(error) => return Err(error),
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
                Err(error) => return Err(error),
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
    let mut construction_warnings: Vec<_> = report
        .warnings
        .into_iter()
        .filter(|warning| warning.starts_with(FINAL_INVARIANT_WARNING_PREFIX))
        .collect();
    construction_warnings.extend(
        report
            .shifts
            .iter()
            .flat_map(|shift| &shift.assignment.construction_warnings)
            .map(|message| format!("rotation_construction: {message}")),
    );
    report.warnings = rotation_shortfall_warnings(blueprint, &report.shifts);
    match rotation_dorm_warnings(blueprint, operbox, &report.shifts) {
        Ok(warnings) => report.warnings.extend(warnings),
        Err(Error::ScheduleQuality(message)) => {
            construction_warnings.push(format!("{FINAL_INVARIANT_WARNING_PREFIX} {message}"))
        }
        Err(error) => return Err(error),
    }
    report
        .warnings
        .extend(rotation_cycle_rest_warnings(blueprint, &report));
    let issues = collect_final_rotation_issues(
        blueprint,
        operbox,
        instances,
        table,
        &report,
        initial_mood,
        Some(fiammetta_enable),
    );
    report.warnings.extend(construction_warnings);
    report.warnings.extend(issues.into_iter().map(|issue| {
        format!(
            "{FINAL_INVARIANT_WARNING_PREFIX} {}: {}",
            issue.rule, issue.message
        )
    }));
    let mut seen = HashSet::new();
    report
        .warnings
        .retain(|message| seen.insert(message.clone()));
    stage_after = crate::profile::solver_perf_snapshot();
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
    originium_pair: &OriginiumPairContext,
    shifts: &mut [TeamShiftResult],
) -> Result<bool> {
    if !operbox.owns("菲亚梅塔") || shifts.is_empty() {
        return Ok(false);
    }
    let slots = fiammetta_action_slots_for_shifts(profile, shifts);
    if slots.is_empty() {
        return Ok(false);
    }
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
            if room_kind == FacilityKind::Factory
                && (originium_pair.matches_room(&room_id)
                    || candidate_shifts.iter().any(|shift| {
                        originium_pair_room_has_pair(blueprint, &shift.assignment, &room_id)
                    }))
            {
                continue;
            }
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
const WARMUP_STICKY_TRADE_OPERATORS: [&str; 2] = ["巫恋", "龙舌兰"];
const WARMUP_MANU_BUFF_PREFIX: &str = "manu_prod_spd_addition[";
const WARMUP_TRADE_BUFF_PREFIX: &str = "trade_ord_wt&cost[";

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

fn originium_pair_room_has_pair(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    room_id: &RoomId,
) -> bool {
    let operators = assignment.operators_in(room_id);
    ORIGINIUM_PAIR_OPERATORS
        .iter()
        .all(|name| operators.iter().any(|operator| operator.name == *name))
        && blueprint.room(room_id).is_some_and(|room| {
            room.kind == FacilityKind::Factory
                && matches!(
                    room.product.as_ref(),
                    Some(RoomProduct::Factory {
                        recipe: crate::types::RecipeKind::Originium
                    })
                )
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
    originium_pair: &OriginiumPairContext,
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
        if originium_pair.matches_room(&current_room)
            || originium_pair.matches_room(&anchor_room)
            || originium_pair_room_has_pair(blueprint, assignment, &current_room)
            || originium_pair_room_has_pair(blueprint, assignment, &anchor_room)
        {
            continue;
        }
        if rooms_compatible_for_swap(blueprint, &current_room, &anchor_room, kind)
            && swap_room_assignments(assignment, &current_room, &anchor_room)
        {
            // Compose the committed room permutation so Plan consumers retain
            // the original anchor identity through compatible warmup moves.
            for room in [&current_room, &anchor_room] {
                assignment
                    .rotation_room_moves
                    .entry(room.clone())
                    .or_insert_with(|| room.clone());
            }
            for destination in assignment.rotation_room_moves.values_mut() {
                if *destination == current_room {
                    *destination = anchor_room.clone();
                } else if *destination == anchor_room {
                    *destination = current_room.clone();
                }
            }
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
    originium_pair_room: Option<&'a RoomId>,
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
        compiled_atoms: Some(table.compile_manufacture_atoms(&buff_ids)),
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

    // The originium pair is a cross-shift hard anchor.  Its first-shift room
    // must survive the abyssal rebuild instead of being cleared as mutable H1.
    let mutable_manu_rooms: Vec<RoomId> = ctx
        .mutable_manu_rooms
        .iter()
        .filter(|room_id| ctx.originium_pair_room != Some(*room_id))
        .cloned()
        .collect();
    if mutable_manu_rooms.is_empty() {
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
        mutable_manu_rooms.len(),
        &mut Vec::new(),
        &mut count_vectors,
    );

    let mut out = Vec::new();
    for counts in count_vectors {
        let mut candidate = base.clone();
        for room_id in &mutable_manu_rooms {
            clear_room(&mut candidate, room_id.0.as_str());
        }
        let mut used = candidate.operator_names();
        used.extend(ctx.blocked_ops.iter().cloned());
        let mut next_hunter = 0;
        let mut ok = true;
        for (room_idx, count) in counts.iter().copied().enumerate() {
            let room_id = &mutable_manu_rooms[room_idx];
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
            .filter(|room| mutable_manu_rooms.contains(&room.room_id))
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

fn collect_recycle_rotation_issues(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
) -> Vec<RotationIssue> {
    let mut issues = Vec::new();
    macro_rules! check {
        ($rule:literal, $body:block) => {
            if let Err(error) = (|| -> Result<()> {
                $body;
                Ok(())
            })() {
                issues.push(RotationIssue {
                    rule: $rule,
                    message: error.to_string(),
                });
            }
        };
    }
    let rooms: Vec<_> = blueprint.rooms_of(FacilityKind::Recycle);
    if rooms.is_empty() {
        return issues;
    }
    for shift in &report.shifts {
        for room in &rooms {
            let operators = shift.assignment.operators_in(&room.id);
            check!("recycle_capacity", {
                if operators.len() > room.operator_capacity() {
                    return Err(Error::msg(format!(
                        "assert-invariants recycle_capacity failed in shift {} room {}",
                        shift.index + 1,
                        room.id.0
                    )));
                }
            });
            check!("recycle_admission", {
                let plan = report
                    .assignment_plans
                    .get(shift.plan_index)
                    .ok_or_else(|| {
                        Error::msg("assert-invariants recycle_admission failed: missing Plan")
                    })?;
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
                }
            });
            check!("recycle_team", {
                for operator in operators {
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
            });
            check!("recycle_support_priority", {
                let plan = report
                    .assignment_plans
                    .get(shift.plan_index)
                    .ok_or_else(|| Error::msg("recycle_support_priority: missing Plan"))?;
                let support_names: HashSet<_> = plan
                    .anchors
                    .iter()
                    .filter(|anchor| {
                        anchor.proactive_support && anchor.facility == FacilityKind::Recycle
                    })
                    .filter(|anchor| anchor.room_id.as_ref() == Some(&room.id))
                    .filter(|anchor| operators.iter().any(|op| op.name == anchor.operator))
                    .map(|anchor| anchor.operator.as_str())
                    .collect();
                if support_names.is_empty() {
                    return Ok(());
                }
                let mut protected = HashSet::new();
                for owner in &report.assignment_plans {
                    protected.extend(owner.excluded_operators.iter().map(String::as_str));
                    protected.extend(owner.anchors.iter().map(|anchor| anchor.operator.as_str()));
                    protected.extend(
                        owner
                            .rotation_reserves
                            .iter()
                            .flat_map(|reserve| reserve.operators.iter().map(String::as_str)),
                    );
                }
                protected.extend(
                    report
                        .runtime_shift_binds
                        .iter()
                        .flat_map(|bind| bind.operators.iter().map(String::as_str)),
                );
                for state in &report.shifts {
                    if let Some(rest) = &state.dorm_rest {
                        protected.extend(dorm_rest_members(rest));
                    }
                }
                let occupied = shift.assignment.occupied_operator_names();
                for (name, progress) in &operbox.owned {
                    if occupied.contains(name) || protected.contains(name.as_str()) {
                        continue;
                    }
                    let teams: Vec<_> = report
                        .teams
                        .iter()
                        .filter(|team| team.operators.contains(name))
                        .collect();
                    if teams
                        .iter()
                        .any(|team| !shift.active_teams.contains(&team.label))
                    {
                        continue;
                    }
                    let candidate = AssignedOperator::from_progress(name, *progress);
                    if crate::recycle::recycle_skill_efficiency(instances, table, &candidate)?
                        .is_zero()
                    {
                        continue;
                    }
                    // 核对真实可用技能人能否替换一名已选无回收技能支持者；不重新选人或修 assignment。
                    for support in &support_names {
                        let mut trial = shift.assignment.clone();
                        let mut members = operators
                            .iter()
                            .filter(|op| op.name != *support)
                            .cloned()
                            .collect::<Vec<_>>();
                        members.push(candidate.clone());
                        trial.set_room(room.id.clone(), members);
                        if crate::layout::plan_admits_workplace_candidate(
                            plan, &trial, room, &candidate,
                        ) {
                            return Err(Error::msg(format!(
                                "recycle_support_priority: shift {} support {} occupies an available skilled seat for {}",
                                shift.index + 1, support, name
                            )));
                        }
                    }
                }
            });
            check!("recycle_efficiency", {
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
            });
        }
    }
    issues
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
    schedule_timed_rotation_request(
        blueprint,
        operbox,
        instances,
        table,
        options,
        TimedRotationProfile::default().into(),
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
    assert_final_rotation_invariants_with_mood(
        blueprint, operbox, instances, table, report, 24.0, None,
    )
}

pub fn assert_final_rotation_invariants_for_request(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    fiammetta_enable: bool,
) -> Result<()> {
    if report.profile == TimedRotationProfile::FiammettaWitch12_12 {
        return Err(Error::msg(
            "assert-invariants unsupported rotation profile fiammetta_witch_12_12: this product mode was removed",
        ));
    }
    assert_final_rotation_invariants_with_mood(
        blueprint,
        operbox,
        instances,
        table,
        report,
        24.0,
        Some(fiammetta_enable),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationIssue {
    pub rule: &'static str,
    pub message: String,
}

/// Terminal checks read the completed state. Product warnings and explicit
/// acceptance consume the same facts; each independent rule is evaluated.
pub fn collect_final_rotation_issues(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    initial_mood: f64,
    fiammetta_enable: Option<bool>,
) -> Vec<RotationIssue> {
    let mut issues = Vec::new();
    macro_rules! check {
        ($name:literal, $body:block) => {
            if let Err(error) = (|| -> Result<()> {
                $body;
                Ok(())
            })() {
                issues.push(RotationIssue {
                    rule: $name,
                    message: error.to_string(),
                });
            }
        };
    }
    check!("rotation_duration_input", {
        validate_rotation_duration_hours(report.shifts.iter().map(|shift| shift.duration_hours))?;
    });
    check!("structure", {
        if report.profile.is_ordinary_abc()
            || report.profile == TimedRotationProfile::Fiammetta8_8_4_4
        {
            validate_final_rotation_invariants(blueprint, &report.shifts, &[], &[])?;
        } else {
            validate_profile_structure(blueprint, report)?;
        }
    });
    check!("profile_team_membership", {
        validate_profile_team_membership(blueprint, report)?;
    });
    check!("profile_work_disjoint", {
        validate_profile_work_disjoint(blueprint, report)?;
    });
    check!("abyssal_structure", {
        validate_abyssal_profile_structure(blueprint, instances, report)?;
    });
    for shift in &report.shifts {
        check!("assignment_structure", {
            let violations = shift.assignment.structural_violations(blueprint);
            if !violations.is_empty() {
                return Err(Error::msg(format!(
                    "shift {}: {}",
                    shift.index + 1,
                    violations.join("; ")
                )));
            }
        });
        check!("plan_anchors", {
            let plan = report
                .assignment_plans
                .get(shift.plan_index)
                .ok_or_else(|| {
                    Error::msg(format!(
                        "shift {}: cannot evaluate anchors without Plan",
                        shift.index + 1
                    ))
                })?;
            for anchor in &plan.anchors {
                let Some(original_room) = &anchor.room_id else {
                    return Err(Error::msg("anchor has unresolved room"));
                };
                let room_id = shift
                    .assignment
                    .rotation_room_moves
                    .get(original_room)
                    .unwrap_or(original_room);
                if !rooms_compatible_for_swap(blueprint, original_room, room_id, anchor.facility) {
                    return Err(Error::msg(
                        "anchor room move is incompatible with its original placement",
                    ));
                }
                let active = !report.profile.is_ordinary_abc()
                    && report.profile != TimedRotationProfile::Fiammetta8_8_4_4
                    || report.teams.iter().any(|team| {
                        shift.active_teams.contains(&team.label)
                            && team.operators.contains(&anchor.operator)
                    })
                    || !report
                        .teams
                        .iter()
                        .any(|team| team.operators.contains(&anchor.operator));
                if active
                    && !if anchor.facility == FacilityKind::Dormitory {
                        blueprint
                            .rooms_of(FacilityKind::Dormitory)
                            .iter()
                            .any(|room| {
                                shift
                                    .assignment
                                    .operators_in(&room.id)
                                    .iter()
                                    .any(|op| op.name == anchor.operator)
                            })
                    } else {
                        shift
                            .assignment
                            .operators_in(room_id)
                            .iter()
                            .any(|op| op.name == anchor.operator)
                    }
                {
                    return Err(Error::msg(format!(
                        "shift {}: required {} missing from {}",
                        shift.index + 1,
                        anchor.operator,
                        room_id.0
                    )));
                }
            }
        });
    }
    check!("team_roster", {
        let mut owners: HashMap<&str, Vec<TeamLabel>> = HashMap::new();
        for team in &report.teams {
            let mut names = HashSet::new();
            for name in &team.operators {
                if !names.insert(name) {
                    return Err(Error::msg(format!(
                        "team {:?} has duplicate member {name}",
                        team.label
                    )));
                }
                owners.entry(name).or_default().push(team.label);
            }
        }
        for (name, teams) in owners {
            let action_target = report.shifts.iter().any(|shift| {
                shift
                    .fiammetta
                    .as_ref()
                    .is_some_and(|action| action.target == name)
            });
            if teams.len() > 1 && !action_target {
                return Err(Error::msg(format!(
                    "operator {name} belongs to multiple teams without an accepted recovery action"
                )));
            }
        }
        if report.profile.is_two_team() {
            let present: HashSet<_> = report
                .shifts
                .iter()
                .flat_map(|shift| shift.assignment.operator_names())
                .collect();
            for team in &report.teams {
                let mut names = HashSet::new();
                for name in &team.operators {
                    if !names.insert(name) || !present.contains(name) {
                        return Err(Error::msg(format!(
                            "team {:?} has duplicate or absent member {name}",
                            team.label
                        )));
                    }
                }
            }
        }
    });
    check!("rotation_cycle_mood", {
        validate_cycle_mood(blueprint, instances, report, initial_mood)?;
    });
    check!("originium", {
        assert_final_originium_pair(blueprint, operbox, report)?;
    });
    check!("bind", {
        validate_final_bind_cycle(blueprint, report)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    });
    check!("fiammetta_enabled", {
        if fiammetta_enable == Some(false) {
            if let Some((shift_index, action)) =
                report.shifts.iter().enumerate().find_map(|(index, shift)| {
                    shift.fiammetta.as_ref().map(|action| (index, action))
                })
            {
                return Err(Error::msg(format!(
                "assert-invariants fiammetta_disabled failed in shift {}: action for {} is present while disabled",
                shift_index + 1,
                action.target
            )));
            }
        }
    });
    check!("action_policy", {
        validate_fiammetta_action_policy(report.profile, &report.shifts)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    });
    check!("action_ready", {
        let mood_model = default_mood_model()?;
        fiammetta_events::validate_actions(&report.shifts, &mood_model, blueprint, instances)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
    });
    check!("control", {
        assert_final_control_rotation(blueprint, operbox, instances, report)?;
    });
    issues.extend(
        super::control_rotation::collect_control_mood_mapping_issues(
            blueprint, instances, table, report,
        ),
    );
    check!("main_24", {
        if report.profile == TimedRotationProfile::MainBackup24_24 {
            assert_main_backup_24_24_final_state(blueprint, instances, report)?;
        }
    });
    check!("trade", {
        assert_rotation_trade_final_efficiency(blueprint, operbox, instances, table, report)?;
    });
    check!("support", {
        crate::layout::assert_support_combination_policy(&report.assignment_plans, &report.shifts)?;
    });
    issues.extend(collect_recycle_rotation_issues(
        blueprint, operbox, instances, table, report,
    ));
    check!("ordinary_admission", {
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
    });
    check!("ordinary_complete", {
        for index in 0..report.shifts.len() {
            if let Some(commit) =
                rotation_vacancy_commit(blueprint, operbox, instances, table, report, index)?
            {
                let room = &commit.placements[0].0;
                return Err(Error::msg(format!("assert-invariants ordinary_fallback_complete failed in shift {} room {}: a legal vacancy remained after completion", index + 1, room.0)));
            }
        }
    });
    check!("diagnostics", {
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
    });
    check!("dorm_autofill", {
        for (index, shift) in report.shifts.iter().enumerate() {
            let previous = (index + report.shifts.len() - 1) % report.shifts.len();
            let previous_work = report.shifts[previous]
                .assignment
                .work_position_operator_names(blueprint);
            let current_work = shift.assignment.work_position_operator_names(blueprint);
            let dorm_names: HashSet<_> = shift
                .assignment
                .rooms_of_kind(blueprint, FacilityKind::Dormitory)
                .flat_map(|room| room.operators.iter().map(|operator| operator.name.as_str()))
                .collect();
            let expected = previous_work
                .difference(&current_work)
                .any(|name| !dorm_names.contains(name.as_str()));
            if shift.assignment.dorm_autofill != expected {
                return Err(Error::msg(format!(
                    "assert-invariants dorm_autofill failed in shift {}: report={} expected={expected}",
                    index + 1, shift.assignment.dorm_autofill
                )));
            }
        }
    });
    check!("warning_completeness", {
        let expected_rest_warnings = rotation_cycle_rest_warnings(blueprint, report);
        let expected_dorm_warnings = rotation_dorm_warnings(blueprint, operbox, &report.shifts)?;
        let mut expected_warnings = rotation_shortfall_warnings(blueprint, &report.shifts);
        expected_warnings.extend(expected_dorm_warnings);
        expected_warnings.extend(expected_rest_warnings);
        let actual_warnings: Vec<_> = report
            .warnings
            .iter()
            .filter(|warning| {
                !warning.starts_with(FINAL_INVARIANT_WARNING_PREFIX)
                    && !warning.starts_with("rotation_construction:")
            })
            .collect();
        if actual_warnings != expected_warnings.iter().collect::<Vec<_>>() {
            return Err(Error::msg(format!(
            "assert-invariants rotation_warnings_complete failed: report={actual_warnings:?} expected={expected_warnings:?}"
        )));
        }
    });
    check!("weighting", {
        let cycle_hours = validate_rotation_duration_hours(
            report.shifts.iter().map(|shift| shift.duration_hours),
        )?;
        let mut expected = DailyTotals::default();
        for shift in &report.shifts {
            let scores = &shift.efficiencies;
            let trade = scores.weighted_trade_for_cycle(shift.duration_hours, cycle_hours);
            let manufacture =
                scores.weighted_manufacture_for_cycle(shift.duration_hours, cycle_hours);
            let power = scores.weighted_power_for_cycle(shift.duration_hours, cycle_hours);
            check!("shift_weighting", {
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
            });
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
    });
    check!("runtime_refs", {
        let unresolved_trade_refs = table.validate_operator_refs(instances);
        if !unresolved_trade_refs.is_empty() {
            return Err(Error::msg(format!(
                "assert-invariants runtime_trade_skill_refs_resolve failed:\n{}",
                unresolved_trade_refs.join("\n")
            )));
        }
    });
    check!("mechanisms", {
        let pool = build_control_pool_with_fillers(operbox, instances, table);
        let mood = default_mood_model();
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
            let plan = report.assignment_plans.get(shift.plan_index);
            let gold_flow_selected = plan.and_then(|plan| {
                plan.selected_rules
                    .iter()
                    .find(|rule| rule.rule_id == "pozyomka_durin")
            });
            let gold_flow_split =
                gold_flow_selected.and_then(|rule| match rule.alternative_id.as_str() {
                    "two_trade_gold_flow" => Some(2),
                    "three_trade_gold_flow" => Some(3),
                    _ => None,
                });
            check!("gold_flow_core", {
                if plan.is_none() {
                    return Err(Error::msg("cannot evaluate gold flow anchors without Plan"));
                }
                if gold_flow_selected.is_some() && blueprint.gold_manu_line_count() < 4 {
                    return Err(Error::msg(format!(
                "assert-invariants gold_flow_core_real_lines failed in shift {}: core selected with {} real gold lines (need 4)",
                shift.index + 1,
                blueprint.gold_manu_line_count(),
            )));
                }
            });
            for room in blueprint.rooms_of(FacilityKind::Factory) {
                check!("manufacture_capacity", {
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
                });
                check!("manufacture_peers", {
                    let members = shift.assignment.operators_in(&room.id);
                    let operators: Vec<_> = members
                        .iter()
                        .map(|member| crate::manufacture::ManuOperator {
                            name: member.name.clone(),
                            elite: member.elite,
                            compiled_atoms: Some(
                                table.compile_manufacture_atoms(
                                    &instances
                                        .resolve_manufacture_buff_ids(&member.name, member.tier()),
                                ),
                            ),
                            buff_ids: instances
                                .resolve_manufacture_buff_ids(&member.name, member.tier()),
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
                });
            }
            check!("office", {
                validate_office_assignment_disjoint(blueprint, shift)
                    .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
            });
            check!("notes", {
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
            });
            check!("structure", {
                let violations = shift.assignment.structural_violations(blueprint);
                if !violations.is_empty() {
                    return Err(Error::msg(format!(
                        "assert-invariants structural failed in shift {}: {}",
                        shift.index + 1,
                        violations.join("; ")
                    )));
                }
            });
            check!("gold_flow", {
                assert_gold_flow_final_state(blueprint, shift, instances, table)?;
            });
            check!("gold_flow_geometry", {
                let durin_count = gold_flow_durin_count(blueprint, &shift.assignment, instances);
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
                            plan.ok_or_else(|| {
                                Error::msg("cannot evaluate gold flow anchors without Plan")
                            })?,
                            &kirara_room,
                            &pozyomka_room,
                            &tuye_room,
                            shift.index,
                        )?;
                    }
                }
            });
            check!("sphinx", {
                if shift_works_in_trade(blueprint, &shift.assignment, SPHINX_NAME)
                    && !operator_is_in_normal_base(blueprint, &shift.assignment, ULPIANUS_NAME)
                {
                    return Err(Error::msg(format!(
                        "assert-invariants sphinx_ulpianus_presence failed in shift {}",
                        shift.index + 1
                    )));
                }
            });
            check!("power", {
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
            });
            check!("power_aggregate", {
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
            });
            check!("action_placement", {
                if shift.fiammetta.is_some() {
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
                }
            });
            check!("action_target_present", {
                if let Some(action) = &shift.fiammetta {
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
                }
            });
            check!("action_room", {
                if let Some(action) = &shift.fiammetta {
                    let valid_room = blueprint.room(&action.room_id).is_some_and(|room| {
                        matches!(
                            room.kind,
                            FacilityKind::TradePost
                                | FacilityKind::Factory
                                | FacilityKind::PowerPlant
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
            });
        }
        check!("continuous_targets", {
            validate_fiammetta_continuous_targets(blueprint, report.profile, &report.shifts)
                .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
        });
        for shift in &report.shifts {
            check!("meeting", {
                for room in blueprint.rooms_of(FacilityKind::MeetingRoom) {
                    let members = shift.assignment.operators_in(&room.id);
                    let suzuran = members.iter().any(|member| member.name == "铃兰");
                    let winter = members
                        .iter()
                        .any(|member| member.name == "忍冬" && member.elite >= 2);
                    let plan = report
                        .assignment_plans
                        .get(shift.plan_index)
                        .ok_or_else(|| {
                            Error::msg("cannot evaluate meeting partner without Plan")
                        })?;
                    let required = plan.anchors.iter().any(|anchor| {
                        anchor.operator == "铃兰" && anchor.facility == FacilityKind::MeetingRoom
                    });
                    if suzuran && !winter && !required {
                        return Err(Error::msg(format!(
                            "assert-invariants meeting_suzuran_partner failed in shift {} room {}",
                            shift.index + 1,
                            room.id.0,
                        )));
                    }
                }
            });
            let control = shift.assignment.control_operators();
            let control_capacity = blueprint
                .rooms_of(FacilityKind::ControlCenter)
                .first()
                .map(|room| room.operator_capacity())
                .unwrap_or(0);
            check!("control_capacity", {
                if control.len() > control_capacity {
                    return Err(Error::msg(format!(
                        "assert-invariants control_capacity failed in shift {}: {} exceeds {}",
                        shift.index + 1,
                        control.len(),
                        control_capacity,
                    )));
                }
            });
            let control_result = (|| -> Result<_> {
                let pool = pool.as_ref().map_err(|error| {
                    Error::msg(format!("cannot evaluate control pool: {error}"))
                })?;
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
                Ok(solve_control(
                    &ControlRoomInput {
                        operators,
                        mood: 24.0,
                        layout: layout.clone(),
                    },
                    table,
                ))
            })();
            check!("control_evaluation", {
                control_result
                    .as_ref()
                    .map_err(|error| Error::msg(error.to_string()))?;
            });
            check!("control_effect_sources", {
                let result = control_result.as_ref().map_err(|error| {
                    Error::msg(format!("cannot evaluate control sources: {error}"))
                })?;
                if result.effect_ledger.has_conflicting_effect_sources() {
                    return Err(Error::msg(format!(
                        "assert-invariants control_effect_source_unique failed in shift {}",
                        shift.index + 1
                    )));
                }
            });
            let final_layout = resolve_base(
                blueprint,
                &shift.assignment,
                Some(instances),
                Some(table),
                24.0,
                Some(operbox.durin_dorm_planning_count(instances)),
            )
            .map(|resolved| resolved.layout_snapshot());
            check!("cuisine", {
                let final_layout = final_layout.as_ref().map_err(|error| {
                    Error::msg(format!(
                        "cannot evaluate cuisine without resolved layout: {error}"
                    ))
                })?;
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
            });
            check!("producer_consumer", {
                let plan = report
                    .assignment_plans
                    .get(shift.plan_index)
                    .ok_or_else(|| {
                        Error::msg("cannot evaluate producer/consumer dependencies without Plan")
                    })?;
                for dependency in plan
                    .resolved_producer_dependencies
                    .iter()
                    .filter(|dependency| dependency.rule_id == "trade_karlan_precision")
                {
                    let source_active = shift
                        .assignment
                        .control_operators()
                        .iter()
                        .any(|op| dependency.producers.contains(&op.name));
                    if source_active {
                        let same_trade_room = blueprint.rooms.iter().any(|room| {
                            room.kind == FacilityKind::TradePost
                                && dependency.consumers.iter().all(|name| {
                                    shift
                                        .assignment
                                        .operators_in(&room.id)
                                        .iter()
                                        .any(|op| &op.name == name)
                                })
                        });
                        if !same_trade_room
                            || crate::response_dependency::karlan_precision_consumer_rooms(
                                blueprint,
                                &shift.assignment,
                                instances,
                            )
                            .is_empty()
                        {
                            return Err(Error::msg(format!(
                        "assert-invariants karlan_precision_selected_team failed in shift {}: producer without its selected trade room",
                        shift.index + 1,
                    )));
                        }
                    }
                }
            });
            for room in shift.assignment.iter_rooms() {
                if blueprint
                    .room(&room.room_id)
                    .is_none_or(|spec| spec.kind != FacilityKind::TradePost)
                {
                    continue;
                }
                check!("trade_blackkey_witch", {
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
                });
                check!("trade_witch_admission", {
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
                                buff_ids: instances
                                    .resolve_trade_buff_ids(&operator.name, operator.tier()),
                                tags: instances.tags_for(&operator.name, operator.tier()),
                                compiled_atoms: Arc::from([]),
                            })
                            .collect::<Vec<_>>();
                        let valid_rule =
                            match_witch_group_shortcut(&operators, table, level).is_some();
                        if !valid_room || !valid_rule {
                            return Err(Error::msg(format!(
                            "assert-invariants witch_tequila_admission failed in shift {} room {}",
                            shift.index + 1,
                            room.room_id.0,
                        )));
                        }
                    }
                });
            }
            check!("karlan_source", {
                let final_layout = final_layout.as_ref().map_err(|error| {
                    Error::msg(format!(
                        "cannot evaluate Karlan source without resolved layout: {error}"
                    ))
                })?;
                for room in shift.assignment.iter_rooms().filter(|room| {
                    blueprint
                        .room(&room.room_id)
                        .is_some_and(|spec| spec.kind == FacilityKind::TradePost)
                }) {
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
            });
            check!("control_projection", {
                let result = control_result.as_ref().map_err(|error| {
                    Error::msg(format!("cannot evaluate control projection: {error}"))
                })?;
                let final_layout = final_layout.as_ref().map_err(|error| {
                    Error::msg(format!(
                        "cannot evaluate control projection without resolved layout: {error}"
                    ))
                })?;
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
                    || (final_layout.control_office_hire_spd_pct - result.office_hire_spd_pct).abs()
                        > 1e-9
                    || (final_layout.control_meeting_speed_pct - result.meeting_speed_pct).abs()
                        > 1e-9
                {
                    return Err(Error::msg(format!(
                "assert-invariants control_effect_projection_finite failed in shift {}: pre=[trade {}, office {}, meeting {}] final=[trade {}, office {}, meeting {}]",
                shift.index + 1,
                result.inject.trade_eff_pct(), result.office_hire_spd_pct, result.meeting_speed_pct,
                final_layout.global_inject.trade_eff_pct(), final_layout.control_office_hire_spd_pct,
                final_layout.control_meeting_speed_pct,
            )));
                }
            });
            check!("control_recovery", {
                let mood = mood
                    .as_ref()
                    .map_err(|error| Error::msg(format!("cannot evaluate mood model: {error}")))?;
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
            });
            check!("control_recovery_values", {
                let mood = mood
                    .as_ref()
                    .map_err(|error| Error::msg(format!("cannot evaluate mood model: {error}")))?;
                let control_ops = control
                    .iter()
                    .map(|op| (op.name.clone(), op.elite))
                    .collect::<Vec<_>>();
                let recovery = mood.control_global_recovery_summary(&control_ops);
                let smiley = mood.control_smiley_recovery_value(&control_ops);
                let control_smiley = mood.control_smiley_recovery_for_facility(
                    FacilityKind::ControlCenter,
                    &control_ops,
                );
                let spread_smiley = mood
                    .control_smiley_recovery_for_facility(FacilityKind::TradePost, &control_ops);
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
            });
        }
        check!("abc_office_rest", {
            if report.profile.is_ordinary_abc() {
                for shift in &report.shifts {
                    let resting_team = report
                        .teams
                        .iter()
                        .find(|team| team.label == shift.resting_team)
                        .ok_or_else(|| {
                            Error::msg("cannot evaluate ABC office rest without resting Team")
                        })?;
                    let resting: HashSet<&str> =
                        resting_team.operators.iter().map(String::as_str).collect();
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
            }
        });
        check!("gold_flow_split_witness", {
            if peak_has_split && !split_witness {
                return Err(Error::msg(
            "assert-invariants gold_flow_split_admission failed: selected group has no working shift",
        ));
            }
        });
    });
    issues
}

fn assert_final_rotation_invariants_with_mood(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
    initial_mood: f64,
    fiammetta_enable: Option<bool>,
) -> Result<()> {
    let issues = collect_final_rotation_issues(
        blueprint,
        operbox,
        instances,
        table,
        report,
        initial_mood,
        fiammetta_enable,
    );
    let expected_mood_warnings: Vec<_> = issues
        .iter()
        .filter(|issue| issue.rule == "rotation_cycle_mood")
        .map(|issue| {
            format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} {}: {}",
                issue.rule, issue.message
            )
        })
        .collect();
    let actual_mood_warnings: Vec<_> = report
        .warnings
        .iter()
        .filter(|warning| {
            warning.starts_with(&format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} rotation_cycle_mood:"
            ))
        })
        .cloned()
        .collect();
    if actual_mood_warnings != expected_mood_warnings {
        return Err(Error::msg(
            "assert-invariants rotation_mood_diagnostics failed",
        ));
    }
    let assignment_issues: Vec<_> = issues
        .iter()
        .filter(|issue| issue.rule != "rotation_cycle_mood")
        .collect();
    if assignment_issues.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(
            assignment_issues
                .iter()
                .map(|issue| format!("assert-invariants {}: {}", issue.rule, issue.message))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }
}

/// Recompute the final Trade efficiency from each final assignment without using
/// the report's snapshots or the normal aggregate evaluator. This keeps the
/// final report check independent of the value it is proving.
fn assert_rotation_trade_final_efficiency(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
) -> Result<()> {
    for shift in &report.shifts {
        let expected = if shift.transition == ShiftTransition::FiammettaOnly {
            let action = shift.fiammetta.as_ref().ok_or_else(|| {
                Error::msg(format!(
                    "assert-invariants rotation_trade_final_efficiency: FiammettaOnly shift {} has no action",
                    shift.index + 1
                ))
            })?;
            let first = direct_trade_final_facts(
                blueprint,
                operbox,
                instances,
                table,
                &shift.assignment,
                8.0,
            )?;
            let total = direct_trade_final_facts(
                blueprint,
                operbox,
                instances,
                table,
                &shift.assignment,
                16.0,
            )?;
            total
                .into_iter()
                .map(|(room_id, (count, total_final, total_paper))| {
                    let (_, first_final, first_paper) = first
                        .get(&room_id)
                        .copied()
                        .ok_or_else(|| {
                            Error::msg(format!(
                                "assert-invariants rotation_trade_final_efficiency: room {} missing 8h fact in shift {}",
                                room_id,
                                shift.index + 1
                            ))
                        })?;
                    let (final_efficiency, paper_efficiency) =
                        if room_id == action.room_id.0 {
                            (first_final, first_paper)
                        } else {
                            (
                                continuation_efficiency(total_final, first_final),
                                continuation_efficiency(total_paper, first_paper),
                            )
                        };
                    Ok((room_id, (count, final_efficiency, paper_efficiency)))
                })
                .collect::<Result<HashMap<_, _>>>()?
        } else {
            direct_trade_final_facts(
                blueprint,
                operbox,
                instances,
                table,
                &shift.assignment,
                shift.duration_hours,
            )?
        };

        let mut actual = HashMap::new();
        for line in &shift.efficiencies.room_lines {
            if blueprint
                .room(&RoomId::new(line.room_id.clone()))
                .is_some_and(|room| room.kind == FacilityKind::TradePost)
            {
                if actual.insert(line.room_id.clone(), line).is_some() {
                    return Err(Error::msg(format!(
                        "assert-invariants rotation_trade_final_efficiency: duplicate room {} in shift {}",
                        line.room_id,
                        shift.index + 1
                    )));
                }
            }
        }
        if actual.len() != expected.len()
            || actual.len() != blueprint.rooms_of(FacilityKind::TradePost).len()
        {
            return Err(Error::msg(format!(
                "assert-invariants rotation_trade_final_efficiency: incomplete roomset in shift {}",
                shift.index + 1
            )));
        }
        let mut expected_sum = Efficiency::ZERO;
        for (room_id, (count, final_efficiency, paper_efficiency)) in expected {
            let line = actual.get(&room_id).ok_or_else(|| {
                Error::msg(format!(
                    "assert-invariants rotation_trade_final_efficiency: missing room {} in shift {}",
                    room_id,
                    shift.index + 1
                ))
            })?;
            if line.operator_count != count
                || line.trade_efficiency != final_efficiency
                || line.final_efficiency != final_efficiency
                || line.total_efficiency != paper_efficiency
            {
                return Err(Error::msg(format!(
                    "assert-invariants rotation_trade_final_efficiency: room {} mismatch in shift {} (count {}, trade {}, final {}, paper {}) {}",
                    room_id,
                    shift.index + 1,
                    line.operator_count,
                    line.trade_efficiency,
                    line.final_efficiency,
                    line.total_efficiency,
                    paper_efficiency,
                )));
            }
            expected_sum += final_efficiency;
        }
        if shift.efficiencies.trade_efficiency != expected_sum {
            return Err(Error::msg(format!(
                "assert-invariants rotation_trade_final_efficiency: domain sum mismatch in shift {} (got {}, expected {})",
                shift.index + 1,
                shift.efficiencies.trade_efficiency,
                expected_sum
            )));
        }
    }
    Ok(())
}

fn direct_trade_final_facts(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    assignment: &BaseAssignment,
    shift_hours: f64,
) -> Result<HashMap<String, (usize, Efficiency, Efficiency)>> {
    let mut clean = assignment.clone();
    clear_production_efficiencies(blueprint, &mut clean);
    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let resolved = resolve_base(
        blueprint,
        &clean,
        Some(instances),
        Some(table),
        24.0,
        Some(durin_plan),
    )?;
    let mut facts = HashMap::new();
    let gold_production_lines = resolved.gold_manu_line_count();
    for room in &resolved.trade_rooms {
        let (final_efficiency, paper_efficiency) = if room.operators.is_empty() {
            (Efficiency::ZERO, Efficiency::ZERO)
        } else {
            let input = TradeRoomInput {
                level: room.level,
                operators: room.operators.clone(),
                order_count: None,
                mood: 24.0,
                gold_production_lines: Some(gold_production_lines),
                durin_virtual_lines: None,
                human_fireworks: None,
                layout: Arc::new(room.layout.clone()),
                active_order_kind: room.order,
            };
            let result = solve_trade_with_shift(&input, table, shift_hours)?;
            (
                result.efficiency.final_efficiency,
                result.efficiency.paper.paper_efficiency,
            )
        };
        if facts
            .insert(
                room.id.0.clone(),
                (room.operators.len(), final_efficiency, paper_efficiency),
            )
            .is_some()
        {
            return Err(Error::msg(format!(
                "assert-invariants rotation_trade_final_efficiency: duplicate resolved room {}",
                room.id.0
            )));
        }
    }
    Ok(facts)
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

    if report.profile.is_ordinary_abc() {
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

fn assert_final_originium_pair(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    report: &TeamRotationReport,
) -> Result<()> {
    if !(report.profile.is_ordinary_abc() || report.profile.is_ordinary_main_backup()) {
        return Ok(());
    }
    if !(operbox.elite_of("泡泡").is_some_and(|elite| elite >= 1)
        && operbox.elite_of("火神").is_some_and(|elite| elite >= 2))
    {
        return Ok(());
    }
    let originium_pair = OriginiumPairContext::from_peak_plan(&report.peak_plan)?;
    let Some(expected_room) = OriginiumPairContext::legal_room(blueprint) else {
        if originium_pair.room_id().is_some() {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: peak Plan preclaims a room when no legal originium factory exists"
            )));
        }
        return Ok(());
    };
    let Some(room_id) = originium_pair.room_id() else {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: eligible pair and legal room exist but peak Plan has no preclaim anchors"
        )));
    };
    if room_id != &expected_room {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: preclaim room {} is not the first legal room {}",
            room_id.0, expected_room.0
        )));
    }
    let room_spec = blueprint.room(room_id).ok_or_else(|| {
        Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: frozen room {} disappeared",
            room_id.0
        ))
    })?;
    if room_spec.kind != FacilityKind::Factory
        || room_spec.level != 3
        || room_spec.operator_capacity() < 3
        || !matches!(
            room_spec.product.as_ref(),
            Some(RoomProduct::Factory {
                recipe: crate::types::RecipeKind::Originium
            })
        )
    {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: room {} is not a legal level-3 originium factory",
            room_id.0
        )));
    }
    if report
        .peak_plan
        .anchors
        .iter()
        .filter(|anchor| anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID)
        .count()
        != 2
    {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: peak Plan must contain exactly two pair anchors"
        )));
    }
    for name in ORIGINIUM_PAIR_OPERATORS {
        let team_memberships = report
            .teams
            .iter()
            .filter(|team| team.operators.iter().any(|member| member == name))
            .count();
        let alpha_memberships = report
            .teams
            .iter()
            .filter(|team| {
                team.label == TeamLabel::Alpha && team.operators.iter().any(|member| member == name)
            })
            .count();
        if team_memberships != 1
            || alpha_memberships != 1
            || team_of_operator(report, name) != Some(TeamLabel::Alpha)
        {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: {name} must belong to alpha exactly once"
            )));
        }
        let peak_anchor =
            report.peak_plan.anchors.iter().find(|anchor| {
                anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID && anchor.operator == name
            });
        if !peak_anchor.is_some_and(|anchor| {
            anchor.room_id.as_ref() == Some(room_id)
                && anchor.facility == FacilityKind::Factory
                && anchor.recipe == Some(crate::types::RecipeKind::Originium)
                && anchor.fill_policy == AnchorFillPolicy::ManufactureRecipe
        }) {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: peak Plan lacks the expected anchor for {name} in {}",
                room_id.0
            )));
        }
    }

    let expected_shift_count = if report.profile.is_ordinary_abc() {
        3
    } else {
        2
    };
    if report.shifts.len() != expected_shift_count
        || report
            .shifts
            .iter()
            .any(|shift| shift.index >= expected_shift_count)
        || report
            .shifts
            .iter()
            .map(|shift| shift.index)
            .collect::<HashSet<_>>()
            .len()
            != expected_shift_count
    {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: topology requires {expected_shift_count} uniquely indexed shifts"
        )));
    }
    let mut alpha_working_shifts = 0;
    let mut alpha_maximum_shifts = 0usize;
    let max_duration = report
        .shifts
        .iter()
        .map(|shift| shift.duration_hours)
        .max_by(f64::total_cmp)
        .ok_or_else(|| Error::msg("rotation has no shifts"))?;

    for shift in &report.shifts {
        let alpha_active = shift.active_teams.contains(&TeamLabel::Alpha);
        if report.profile.is_ordinary_abc() {
            if shift.active_teams.len() != 2
                || shift.active_teams.contains(&shift.resting_team)
                || !TeamLabel::ALL.contains(&shift.resting_team)
            {
                return Err(Error::msg(format!(
                    "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: ABC shift {} has inconsistent active/resting teams",
                    shift.index + 1
                )));
            }
        } else if shift.active_teams.len() != 1
            || shift.active_teams.contains(&shift.resting_team)
            || !TeamLabel::ALL.contains(&shift.resting_team)
        {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: main/backup shift {} has inconsistent active/resting teams",
                shift.index + 1
            )));
        }
        let pair_members_in_room = ORIGINIUM_PAIR_OPERATORS
            .iter()
            .filter(|name| {
                shift
                    .assignment
                    .operators_in(room_id)
                    .iter()
                    .any(|operator| operator.name == **name)
            })
            .count();
        let pair_any_working = ORIGINIUM_PAIR_OPERATORS
            .iter()
            .any(|name| operator_has_non_dorm_assignment(blueprint, shift, name));
        let pair_working = ORIGINIUM_PAIR_OPERATORS
            .iter()
            .all(|name| operator_has_non_dorm_assignment(blueprint, shift, name));
        if alpha_active {
            alpha_working_shifts += 1;
            if !pair_working || pair_members_in_room != ORIGINIUM_PAIR_OPERATORS.len() {
                return Err(Error::msg(format!(
                    "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: alpha shift {} must work both anchors together in frozen room {}",
                    shift.index + 1,
                    room_id.0
                )));
            }
            if shift.duration_hours.total_cmp(&max_duration).is_eq() {
                alpha_maximum_shifts += 1;
            }
            let plan = report.assignment_plans.get(shift.plan_index).ok_or_else(|| {
                Error::msg(format!(
                    "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed in shift {}: missing owning Plan",
                    shift.index + 1
                ))
            })?;
            for name in ORIGINIUM_PAIR_OPERATORS {
                let anchor_matches = plan.anchors.iter().any(|anchor| {
                    anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID
                        && anchor.operator == name
                        && anchor.room_id.as_ref() == Some(room_id)
                        && anchor.facility == FacilityKind::Factory
                        && anchor.recipe == Some(crate::types::RecipeKind::Originium)
                        && anchor.fill_policy == AnchorFillPolicy::ManufactureRecipe
                });
                if !anchor_matches {
                    return Err(Error::msg(format!(
                        "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed in shift {}: owning Plan lacks anchor for {name} in room {}",
                        shift.index + 1,
                        room_id.0
                    )));
                }
            }
        } else if pair_any_working || pair_members_in_room > 0 {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: pair appears in work/rest topology mismatch in shift {}",
                shift.index + 1
            )));
        }
        if let Some(room) = shift.assignment.room_assignment(room_id) {
            for name in ORIGINIUM_PAIR_OPERATORS {
                if room
                    .operators
                    .iter()
                    .filter(|operator| operator.name == name)
                    .count()
                    > 1
                {
                    return Err(Error::msg(format!(
                        "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed in shift {}: duplicate {name} in room {}",
                        shift.index + 1,
                        room_id.0
                    )));
                }
            }
        }
    }
    let expected_alpha_work_shifts = if report.profile.is_ordinary_abc() {
        2
    } else {
        1
    };
    let expected_bind = if report.profile.is_ordinary_abc() {
        (2, 1)
    } else {
        (1, 1)
    };
    let pair_has_topology_bind = report.runtime_shift_binds.iter().any(|bind| {
        bind.operators.len() == ORIGINIUM_PAIR_OPERATORS.len()
            && ORIGINIUM_PAIR_OPERATORS
                .iter()
                .all(|name| bind.operators.iter().any(|operator| operator == name))
            && (bind.on_shifts, bind.off_shifts) == expected_bind
    });
    if !pair_has_topology_bind {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: pair has no topology bind with on/off {:?}",
            expected_bind
        )));
    }
    let maximum_shift_count = report
        .shifts
        .iter()
        .filter(|shift| shift.duration_hours.total_cmp(&max_duration).is_eq())
        .count();
    let expected_alpha_maximum_shifts = if report.profile.is_ordinary_abc() {
        maximum_shift_count.min(expected_alpha_work_shifts)
    } else {
        1
    };
    if alpha_working_shifts != expected_alpha_work_shifts
        || alpha_maximum_shifts != expected_alpha_maximum_shifts
    {
        return Err(Error::msg(format!(
            "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed: alpha must cover its {expected_alpha_work_shifts} topology work shift(s) and as many longest shift(s) as topology permits; got work={alpha_working_shifts}, longest={alpha_maximum_shifts}, expected_longest={expected_alpha_maximum_shifts}"
        )));
    }

    for shift in report
        .shifts
        .iter()
        .filter(|shift| shift.active_teams.contains(&TeamLabel::Alpha))
    {
        let plan = report.assignment_plans.get(shift.plan_index).ok_or_else(|| {
            Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed in shift {}: missing owning Plan",
                shift.index + 1
            ))
        })?;
        if plan
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID)
            .count()
            < 2
        {
            return Err(Error::msg(format!(
                "assert-invariants {ORIGINIUM_PAIR_SYSTEM_ID} failed in shift {}: incomplete owner anchors",
                shift.index + 1
            )));
        }
    }
    Ok(())
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
    report: &TeamRotationReport,
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
            if (!active_members.contains(name.as_str()) && !fiammetta_targets.contains(&name))
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
                (!active_members.contains(name.as_str()) && !fiammetta_targets.contains(name))
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
    }
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

fn gold_flow_durin_count(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> u8 {
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
    present_durin.len().min(4) as u8
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
    let expected_durin = gold_flow_durin_count(blueprint, assignment, instances);
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
        let initial_efficiencies: Vec<_> =
            chain.operators.iter().map(|op| op.settled_eff).collect();
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
        for ((operator, settled), initial_efficiency) in room
            .operators
            .iter()
            .zip(&chain.operators)
            .zip(initial_efficiencies)
        {
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
            let chain_efficiency = settled.settled_eff - initial_efficiency;
            if (chain_efficiency - expected_eff).abs() > 1e-6 {
                return Err(Error::msg(format!(
                    "assert-invariants gold_flow_reader_efficiency failed in shift {} room {} for {}: expected {}, got {}",
                    shift_index + 1, room.id.0, operator.name, expected_eff, chain_efficiency
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

fn validate_relaxed_action_dependencies(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
    relaxed: &[&str],
) -> Result<()> {
    for target in relaxed {
        let owners = report
            .teams
            .iter()
            .filter(|team| team.operators.iter().any(|name| name == target))
            .count();
        let works = report
            .shifts
            .iter()
            .filter(|shift| {
                shift
                    .assignment
                    .work_position_operator_names(blueprint)
                    .contains(*target)
            })
            .count();
        if owners > 1 || works == report.shifts.len() {
            if !report.shifts.iter().any(|shift| {
                shift
                    .fiammetta
                    .as_ref()
                    .is_some_and(|action| action.target == *target)
            }) {
                return Err(Error::FiammettaCandidateUnavailable(format!(
                    "relaxed target {target} has no accepted action"
                )));
            }
        }
    }
    Ok(())
}

fn reject_fiammetta_candidate(error: Error) -> Result<()> {
    match error {
        Error::FiammettaCandidateUnavailable(_) => Ok(()),
        error => Err(error),
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
    schedule_abc_rotation_candidates_with_timing(
        blueprint,
        operbox,
        instances,
        table,
        options,
        shift_hours,
        profile,
        None,
    )
}

fn schedule_abc_rotation_candidates_with_timing(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: [f64; 3],
    profile: TimedRotationProfile,
    input_order_offset: Option<usize>,
) -> Result<TeamRotationReport> {
    let _memo_scope = crate::search::ensure_search_memo_scope();
    let a_eligible = fiammetta_a_eligible(blueprint, operbox);
    let b_eligible = fiammetta_b_eligible(blueprint, operbox, instances, table)?;
    let finish_candidate = |mut report: RotationDraft, fiammetta_enable: bool| {
        if let Some(offset) = input_order_offset {
            restore_input_shift_order(&mut report, offset)?;
        }
        complete_timed_rotation(
            blueprint,
            operbox,
            instances,
            table,
            options.mood,
            fiammetta_enable,
            report,
        )
    };
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
            true,
        )
        .and_then(|report| finish_candidate(report, false))?;
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
        true,
    )
    .and_then(|report| finish_candidate(report, false));
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
            true,
        )
        .and_then(|mut candidate| {
            attach_fiammetta_target_actions(
                blueprint,
                operbox,
                instances,
                profile,
                &a_targets,
                &mut candidate.shifts,
            )
            .and_then(|_| {
                if let Some(offset) = input_order_offset {
                    restore_input_shift_order(&mut candidate, offset)?;
                }
                prepare_rotation_state(blueprint, operbox, instances, table, &mut candidate, true)?;
                validate_relaxed_action_dependencies(blueprint, &candidate, &a_targets)?;
                finalize_timed_rotation(
                    blueprint,
                    operbox,
                    instances,
                    table,
                    options.mood,
                    true,
                    candidate,
                )
            })
        }) {
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
            true,
        )
        .and_then(|mut candidate| {
            attach_fiammetta_target_actions(
                blueprint,
                operbox,
                instances,
                profile,
                &relaxed,
                &mut candidate.shifts,
            )
            .and_then(|_| {
                if let Some(offset) = input_order_offset {
                    restore_input_shift_order(&mut candidate, offset)?;
                }
                prepare_rotation_state(blueprint, operbox, instances, table, &mut candidate, true)?;
                validate_relaxed_action_dependencies(blueprint, &candidate, &relaxed)?;
                finalize_timed_rotation(
                    blueprint,
                    operbox,
                    instances,
                    table,
                    options.mood,
                    true,
                    candidate,
                )
            })
        }) {
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
    let slots = fiammetta_action_slots_for_shifts(profile, shifts);
    if slots.is_empty() || targets.is_empty() {
        return Ok(());
    }
    let mood_model = default_mood_model()?;
    let action_budget = slots.len();
    let action_indices = if matches!(
        profile,
        TimedRotationProfile::GenericMainBackup | TimedRotationProfile::GenericAbc
    ) {
        slots
    } else {
        (0..shifts.len()).collect()
    };
    let mut used_slots = 0usize;
    let mut consumed_shifts = HashSet::new();
    for target in targets {
        if used_slots >= action_budget {
            break;
        }
        let Some(slot) = action_indices.iter().copied().find(|slot| {
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
        return Err(Error::FiammettaCandidateUnavailable(
            "no ready fiammetta target action".into(),
        ));
    }
    validate_fiammetta_continuous_targets(blueprint, profile, shifts)
        .map_err(|error| Error::FiammettaCandidateUnavailable(error.to_string()))?;
    Ok(())
}

pub fn schedule_timed_rotation_request(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    request: super::rotation_types::TimedRotationRequest,
) -> Result<TeamRotationReport> {
    match super::schedule_rotation(
        blueprint,
        operbox,
        instances,
        table,
        options,
        super::RotationRequest::Timed(request),
    )? {
        super::RotationReport::Timed(report) => Ok(report),
        super::RotationReport::Auto(_) => unreachable!("typed timed request produces timed report"),
    }
}

pub(crate) fn execute_timed_rotation_request(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    request: super::rotation_types::TimedRotationRequest,
) -> Result<TeamRotationReport> {
    let _memo_scope = crate::search::ensure_search_memo_scope();
    request.validate_duration_input()?;
    let request = request.preserve_named_timing();
    let started = Instant::now();
    let before = crate::profile::solver_perf_snapshot();
    let result = match request {
        super::rotation_types::TimedRotationRequest::Named(profile) => {
            schedule_timed_rotation_named(blueprint, operbox, instances, table, options, profile)
        }
        super::rotation_types::TimedRotationRequest::Generic(
            super::rotation_types::GenericRotationTiming::MainBackup { shift_hours },
        ) => {
            schedule_generic_main_backup(blueprint, operbox, instances, table, options, shift_hours)
        }
        super::rotation_types::TimedRotationRequest::Generic(
            super::rotation_types::GenericRotationTiming::Abc { shift_hours },
        ) => schedule_generic_abc(blueprint, operbox, instances, table, options, shift_hours),
    };
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

fn schedule_timed_rotation_named(
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
    schedule_timed_rotation_inner(blueprint, operbox, instances, table, options, profile)
}

fn schedule_timed_rotation_inner(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
) -> Result<TeamRotationReport> {
    let (report, fiammetta_24_diagnostic) = match profile {
        TimedRotationProfile::Abc12_6_6 | TimedRotationProfile::Abc12_12_12 => {
            return schedule_abc_rotation_candidates(
                blueprint, operbox, instances, table, options, profile,
            );
        }
        TimedRotationProfile::MainBackup12_12 => (
            schedule_main_backup_12_12(blueprint, operbox, instances, table, options)?,
            None,
        ),
        TimedRotationProfile::MainBackup24_24 => {
            schedule_main_backup_24_24(blueprint, operbox, instances, table, options)?
        }
        TimedRotationProfile::GenericMainBackup | TimedRotationProfile::GenericAbc => {
            return Err(Error::msg(
                "generic rotation profiles require an explicit ordered timing request",
            ));
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
    let mut report = complete_timed_rotation(
        blueprint,
        operbox,
        instances,
        table,
        options.mood,
        options.fiammetta_enable,
        report,
    )?;
    if let Some(diagnostic) = fiammetta_24_diagnostic {
        append_main_backup_24_fiammetta_warning(&mut report, diagnostic);
    }
    Ok(report)
}

fn first_longest_shift_index(shift_hours: &[f64]) -> usize {
    let mut longest = 0;
    for index in 1..shift_hours.len() {
        if shift_hours[index].total_cmp(&shift_hours[longest]).is_gt() {
            longest = index;
        }
    }
    longest
}

fn rotate_shift_hours_to_alpha<const N: usize>(shift_hours: [f64; N]) -> ([f64; N], usize) {
    let offset = first_longest_shift_index(&shift_hours);
    (
        std::array::from_fn(|index| shift_hours[(offset + index) % N]),
        offset,
    )
}

/// For ABC, alpha works states 0 and 2 and rests state 1.  When two shifts
/// share the maximum duration, put the rest state on the shorter shift so the
/// alpha-owned pair covers both tied maxima.  Equal-all-shift input keeps the
/// original phase for deterministic compatibility.
fn rotate_abc_shift_hours_to_alpha(shift_hours: [f64; 3]) -> ([f64; 3], usize) {
    let maximum = shift_hours
        .iter()
        .copied()
        .max_by(f64::total_cmp)
        .expect("ABC has three shift durations");
    let mut best_offset = 0usize;
    let mut best_score = (0usize, false, 0usize);
    for offset in 0..shift_hours.len() {
        let ordered: [f64; 3] = std::array::from_fn(|index| shift_hours[(offset + index) % 3]);
        let alpha_maxima = [ordered[0], ordered[2]]
            .into_iter()
            .filter(|hours| hours.total_cmp(&maximum).is_eq())
            .count();
        let score = (
            alpha_maxima,
            ordered[0].total_cmp(&maximum).is_eq(),
            usize::MAX - offset,
        );
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }
    (
        std::array::from_fn(|index| shift_hours[(best_offset + index) % 3]),
        best_offset,
    )
}

fn restore_input_shift_order(
    report: &mut TeamRotationReport,
    rotation_offset: usize,
) -> Result<()> {
    let count = report.shifts.len();
    if count == 0 || rotation_offset >= count {
        return Err(Error::msg(
            "generic rotation produced an invalid cyclic shift order",
        ));
    }
    let mut ordered: Vec<Option<TeamShiftResult>> = (0..count).map(|_| None).collect();
    for mut shift in report.shifts.drain(..) {
        if shift.index >= count {
            return Err(Error::msg(
                "generic rotation produced an out-of-range shift index",
            ));
        }
        let input_index = (rotation_offset + shift.index) % count;
        shift.index = input_index;
        ordered[input_index] = Some(shift);
    }
    report.shifts = ordered
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Error::msg("generic rotation omitted an input shift"))?;
    Ok(())
}

fn schedule_generic_main_backup(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    input_hours: [f64; 2],
) -> Result<TeamRotationReport> {
    let (ordered_hours, alpha_offset) = rotate_shift_hours_to_alpha(input_hours);
    let (mut report, _) = schedule_main_backup_profile(
        TimedRotationProfile::GenericMainBackup,
        &ordered_hours,
        blueprint,
        operbox,
        instances,
        table,
        options,
    )?;
    restore_input_shift_order(&mut report, alpha_offset)?;
    complete_timed_rotation(
        blueprint,
        operbox,
        instances,
        table,
        options.mood,
        options.fiammetta_enable,
        report,
    )
}

fn schedule_generic_abc(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    input_hours: [f64; 3],
) -> Result<TeamRotationReport> {
    let (ordered_hours, alpha_offset) = rotate_abc_shift_hours_to_alpha(input_hours);
    schedule_abc_rotation_candidates_with_timing(
        blueprint,
        operbox,
        instances,
        table,
        options,
        ordered_hours,
        TimedRotationProfile::GenericAbc,
        Some(alpha_offset),
    )
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
    enable_originium_pair: bool,
) -> Result<RotationDraft> {
    match schedule_abc_rotation_once(
        blueprint,
        operbox,
        instances,
        table,
        options,
        shift_hours,
        enable_fiammetta_return,
        abyssal_policy,
        profile,
        gamma_relaxed,
        skip_systems,
        enable_originium_pair,
    ) {
        Err(error @ Error::ControlRotationRequiredAnchorsExceedCapacity { .. })
            if !skip_systems.contains(&"pozyomka_durin") =>
        {
            // Preserve the separate optional four-gold Plan fallback. This is
            // capacity admission, not a mood-driven roster rebuild.
            let mut fallback_skip = skip_systems.to_vec();
            fallback_skip.push("pozyomka_durin");
            eprintln!(
                "[rotation-fallback] ordinary trade Plan after the four-gold core made required control anchors infeasible: {error}"
            );
            schedule_abc_rotation_once(
                blueprint,
                operbox,
                instances,
                table,
                options,
                shift_hours,
                enable_fiammetta_return,
                abyssal_policy,
                profile,
                gamma_relaxed,
                &fallback_skip,
                enable_originium_pair,
            )
        }
        result => result,
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
    enable_originium_pair: bool,
) -> Result<RotationDraft> {
    let t0 = Instant::now();
    let mut stage_perf = crate::profile::start_solver_stage("rotation_abc", "total");
    let mut peak_perf = crate::profile::start_solver_stage("rotation_abc", "peak");
    let cycle_hours: f64 = shift_hours.iter().sum();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mut originium_pair = if enable_originium_pair {
        OriginiumPairContext::for_ordinary_rotation(blueprint, operbox)?
    } else {
        OriginiumPairContext {
            preclaim: OriginiumPairPreclaim::Inactive,
            warning: None,
        }
    };
    let mut peak_options = options.clone();
    peak_options.shift_hours = shift_hours[0] + shift_hours[1].max(shift_hours[2]);
    peak_options.registry_sustain_hours = Some(peak_options.shift_hours);
    peak_options.preclaimed_factory_room = originium_pair.room_id().cloned();
    let mut gamma_h1_options = options.clone();
    gamma_h1_options.shift_hours = shift_hours[1];
    let mut gamma_h2_options = options.clone();
    gamma_h2_options.shift_hours = shift_hours[2];

    // 1) 参考高峰班 + 编排计划 → 只取宿舍作为三班共享脚手架；办公室不钉死到 peak。
    // 中枢后续按 αβγ 队伍轮休重分配，不能从 peak 直接钉死。
    // 深海链因歌蕾蒂娅心情消耗 ~3.2/h（最长约 7h），不可进入 12h 主班；
    // 唯一入口是后续 S2 6h 短班的有/无深海双路径评分对比。
    let peak_skip: HashSet<String> = skip_systems.iter().map(|s| s.to_string()).collect();
    let peak_result = if let Some(room_id) = originium_pair.room_id().cloned() {
        match assign_originium_pair_peak(
            blueprint,
            operbox,
            instances,
            table,
            &peak_options,
            &peak_skip,
            &room_id,
            2,
            1,
        ) {
            Ok(result) => result,
            Err(error) if originium_pair_preclaim_failure_allows_fallback(&error) => {
                originium_pair.preclaim = OriginiumPairPreclaim::Inactive;
                peak_options.preclaimed_factory_room = None;
                originium_pair.warning = Some(format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} {ORIGINIUM_PAIR_WARNING}: {error}; ordinary peak retained"
            ));
                assign_shift_with_plan_skip(
                    blueprint,
                    operbox,
                    instances,
                    table,
                    &peak_options,
                    AssignShiftMode::Peak,
                    &BaseAssignment::default(),
                    &peak_skip,
                )?
            }
            Err(error) => return Err(error),
        }
    } else {
        assign_shift_with_plan_skip(
            blueprint,
            operbox,
            instances,
            table,
            &peak_options,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
            &peak_skip,
        )?
    };
    let peak = peak_result.assignment;
    let mut peak_plan = peak_result.plan;
    peak_plan.resolve_actual_producer_dependencies(blueprint, &peak, instances, table)?;
    if originium_pair.room_id().is_some() {
        let plan_pair = OriginiumPairContext::from_peak_plan(&peak_plan)?;
        if plan_pair.room_id() != originium_pair.room_id() {
            return Err(Error::msg(format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: peak Plan lost its frozen preclaim room"
            )));
        }
    }
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
    peak_perf.finish();
    drop(peak_perf);
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
    let mut construction_issues = Vec::new();
    let mut halves = split_production_facilities(
        blueprint,
        &peak,
        &shift_binds,
        &peak_plan.rotation_reserves,
        &originium_pair,
        &mut construction_issues,
    )?;
    // The preclaimed pair belongs to Alpha; preserve that initial placement.
    if let Some(room_id) = originium_pair.room_id() {
        if let Some(alpha_half) = halves.iter().position(|half| half.manu.contains(room_id)) {
            if alpha_half != 0 {
                halves.swap(0, alpha_half);
            }
        }
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
    if let Err(message) = peak_plan.verify_registry_trade_in_alpha_beta(&alpha, &beta) {
        construction_issues.push(message);
    }
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
    // Peak meeting members keep their cohort; gamma must not claim a second
    // working role before the meeting cohorts are frozen.
    used_ab.extend(rotating_meeting_names.iter().cloned());

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
    construction_issues.extend(place_rotation_reserves_for_halves(
        blueprint,
        operbox,
        &peak_plan.rotation_reserves,
        &h1,
        &mut gamma_h1,
        &mut used_g1,
        &h2,
        &mut gamma_h2,
        &mut used_g2,
    )?);
    let mut gamma_h1_coexist = gamma_h1_context.assignment.clone();
    merge_rooms(&mut gamma_h1_coexist, &gamma_h1);
    if !operator_is_in_normal_base(blueprint, &gamma_h1_coexist, ULPIANUS_NAME) {
        // Deepcolor's trade bonus has a final-state ULPianus presence requirement.
        // This gamma branch does not own a ULPianus placement, so it may only use
        // Deepcolor when the shared/coexisting assignment already proves presence.
        used_g1.insert(SPHINX_NAME.to_string());
    }
    let mut gamma_h1_perf = crate::profile::start_solver_stage("rotation_abc", "gamma_h1");
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

    gamma_h1_perf.finish();
    drop(gamma_h1_perf);

    let mut gamma_h2_coexist = gamma_h2_context.assignment.clone();
    merge_rooms(&mut gamma_h2_coexist, &gamma_h2);
    if !operator_is_in_normal_base(blueprint, &gamma_h2_coexist, ULPIANUS_NAME) {
        used_g2.insert(SPHINX_NAME.to_string());
    }

    let mut gamma_h2_perf = crate::profile::start_solver_stage("rotation_abc", "gamma_h2");
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
    gamma_h2_perf.finish();
    drop(gamma_h2_perf);
    print_trade_rotation_debug("gamma_h2 after fill", &gamma_h2);
    if let Err(error) = validate_rotation_reserves_before_postprocess(
        blueprint,
        &peak_plan.rotation_reserves,
        &h1,
        &gamma_h1,
        &h2,
        &gamma_h2,
    ) {
        match error {
            Error::ScheduleQuality(message) => construction_issues.push(message),
            error => return Err(error),
        }
    }
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
        shift_hours[0],
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
    let mut protected_control_names = system_ctrl_names.clone();
    protected_control_names.extend(shared.control_operators().into_iter().map(|op| op.name));
    protected_control_names.extend(
        peak_plan
            .control_candidate_requirements
            .iter()
            .flat_map(|requirement| requirement.candidates.iter().cloned()),
    );
    protected_control_names.extend(
        peak_plan
            .registry_claims
            .iter()
            .flat_map(|claim| &claim.slots)
            .filter(|slot| slot.facility == FacilityKind::ControlCenter)
            .flat_map(|slot| slot.operators.iter().map(|operator| operator.name.clone())),
    );
    freeze_abc_control_roster(
        &mut team_ctrl,
        &control_pool,
        shift_hours,
        options.mood,
        &mood_model,
        &protected_control_names,
    );
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
    for (index, (hours, active, resting, parts)) in shift_specs.into_iter().enumerate() {
        let shift_started = Instant::now();
        let shift_before = crate::profile::solver_perf_snapshot();
        let scoring_mood = Some(&rotation_mood);
        // Control candidates were admitted and frozen before entering the shift loop.
        control_options.mood = options.mood;
        let assign_ctrl =
            |a: &mut BaseAssignment, used: &mut HashSet<String>, extra_control_pins: &[&str]| {
                let control_admission =
                    control_closure.admission(index, &active, extra_control_pins)?;
                let final_pool = match control_admission.prepare(a, &HashSet::new()) {
                    Ok(pool) => Some(pool),
                    Err(Error::ScheduleQuality(message)) => {
                        a.construction_warnings.push(message);
                        a.control_rotation_admission = Some(control_admission.clone());
                        None
                    }
                    Err(error @ Error::ControlRotationRequiredAnchorsExceedCapacity { .. })
                        if !peak_plan
                            .registry_claims
                            .iter()
                            .any(|claim| claim.system_id == "pozyomka_durin") =>
                    {
                        a.construction_warnings.push(error.to_string());
                        a.control_rotation_admission = Some(control_admission.clone());
                        None
                    }
                    Err(error) => return Err(error),
                };
                *used = a.operator_names();
                let current_layout = crate::layout::resolve_control_input_layout(
                    blueprint,
                    a,
                    instances,
                    table,
                    control_options.mood,
                    Some(durin_plan),
                )?;
                if let Some(final_pool) = final_pool {
                    crate::layout::assign_control_with_saileach(
                        a,
                        &final_pool,
                        table,
                        &current_layout,
                        &control_options,
                        &control_admission.requirements(),
                        &HashSet::new(),
                        None,
                        control_admission.required_names().contains("琴柳"),
                        None,
                        used,
                    )?;
                }
                if let Err(error) = control_admission.validate(a) {
                    a.construction_warnings.push(error.to_string());
                }
                {
                    assign_office_for_shift(
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
                    )?;
                    assign_rotating_meetings_with_context(
                        a,
                        blueprint,
                        &active,
                        resting,
                        &rotating_meeting_team,
                        &meeting_pool,
                        &reserved,
                        hours,
                        scoring_mood,
                        &layout,
                    )?;
                    assign_rotating_recycle(
                        a,
                        blueprint,
                        &active,
                        &rotating_recycle_team,
                        &recycle_skill_pool,
                    );
                    Ok(())
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
            align_warmup_rooms(
                blueprint,
                instances,
                &mut base,
                &mut base_warmup_rooms,
                &originium_pair,
            );
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
            let mut abyssal_perf =
                crate::profile::start_solver_stage("rotation_abc", "abyssal_backup");
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
                originium_pair_room: originium_pair.room_id(),
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
                    &originium_pair,
                );
                if !candidate
                    .assignment
                    .control_operators()
                    .iter()
                    .any(|operator| operator.name == candidate.control_producer)
                {
                    continue;
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
            abyssal_perf.finish();
            drop(abyssal_perf);
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

            align_warmup_rooms(
                blueprint,
                instances,
                &mut assignment,
                &mut warmup_sticky_rooms,
                &originium_pair,
            );
            let scores = ShiftEfficiencies::default();
            let weighted_trade = Efficiency::ZERO;
            let weighted_manufacture = Efficiency::ZERO;
            let weighted_power = Efficiency::ZERO;
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

    let mut report = RotationDraft {
        prepared: false,
        report: TeamRotationReport {
            profile,
            assignment_plans: vec![peak_plan.clone()],
            peak_plan: peak_plan.clone(),
            runtime_shift_binds: shift_binds.clone(),
            peak_mood_eta,
            teams: teams.clone(),
            daily: DailyTotals::default(),
            warnings: rotation_shortfall_warnings(blueprint, &shifts),
            shifts,
            elapsed: t0.elapsed(),
        },
    };
    report.warnings.extend(
        construction_issues
            .into_iter()
            .map(|message| format!("{FINAL_INVARIANT_WARNING_PREFIX} {message}")),
    );
    if let Some(warning) = originium_pair.warning() {
        report.warnings.push(warning);
    }

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
    for team in &mut report.report.teams {
        team.operators
            .retain(|name| actual_work.contains(name) || rotating_anchor_team.contains_key(name));
    }

    if enable_fiammetta_return {
        for shift in &report.report.shifts {
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
            &report.report.teams,
            &report.report.peak_plan.continuous_roles,
            &report.report.runtime_shift_binds,
            &originium_pair,
            &mut report.report.shifts,
        ) {
            Ok(applied) => applied,
            Err(error) => return Err(error),
        };
        for shift in &report.report.shifts {
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
                &report.report.runtime_shift_binds,
                &mut report.report.shifts,
            ) {
                Ok(applied) => applied,
                Err(error) => return Err(error),
            }
        };
        for shift in &report.report.shifts {
            print_trade_rotation_debug(
                &format!("after all fiammetta shift {}", shift.index + 1),
                &shift.assignment,
            );
        }
    }
    let quality_warnings: Vec<_> = std::mem::take(&mut report.report.warnings)
        .into_iter()
        .filter(|warning| warning.starts_with(FINAL_INVARIANT_WARNING_PREFIX))
        .collect();
    report.warnings = rotation_shortfall_warnings(blueprint, &report.report.shifts);
    report.warnings.extend(quality_warnings);
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
        ("shared_pool_split", t2.duration_since(t1), m1, m2),
        ("gamma", t3.duration_since(t2), m2, m3),
        ("shifts_and_postprocess", t4.duration_since(t3), m3, m4),
    ] {
        crate::profile::emit_solver_stage("rotation_abc", stage, "ok", elapsed, before, after);
    }

    report.elapsed = t4.duration_since(t0);
    stage_perf.finish();
    Ok(report)
}

fn originium_pair_operator(
    operbox: &OperBox,
    name: &str,
    room_id: &RoomId,
) -> Result<AssignedOperator> {
    let progress = operbox.progress_of(name).ok_or_else(|| {
        Error::msg(format!(
            "{ORIGINIUM_PAIR_SYSTEM_ID}: {name} is not owned for room {}",
            room_id.0
        ))
    })?;
    Ok(AssignedOperator::from_progress(name, progress))
}

fn originium_pair_preclaim_failure_allows_fallback(error: &Error) -> bool {
    match error {
        // The pair preclaim is a preference layered on the ordinary peak plan.
        // Its hardware is retained in the failed assignment, so only the
        // manufacture completion errors of that anchored room are optional.
        Error::OptionalRotationPlacementInfeasible { .. } => true,
        _ => false,
    }
}

fn assign_originium_pair_peak(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    skip_systems: &HashSet<String>,
    room_id: &RoomId,
    on_shifts: u8,
    off_shifts: u8,
) -> Result<crate::layout::AssignShiftResult> {
    let mut stage_perf = crate::profile::start_solver_stage("rotation_abc", "originium_pair");
    let result = (|| {
        let mut planning_seed = BaseAssignment::default();
        planning_seed.set_room(
            room_id.clone(),
            ORIGINIUM_PAIR_OPERATORS
                .iter()
                .map(|name| originium_pair_operator(operbox, name, room_id))
                .collect::<Result<Vec<_>>>()?,
        );
        let execution_seed = BaseAssignment::default();
        let room = blueprint.room(room_id).ok_or_else(|| {
            Error::msg(format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: target room {} disappeared",
                room_id.0
            ))
        })?;
        if room.kind != FacilityKind::Factory
            || room.level != 3
            || !matches!(
                room.product.as_ref(),
                Some(RoomProduct::Factory {
                    recipe: crate::types::RecipeKind::Originium
                })
            )
            || room.operator_capacity() < 3
        {
            return Err(Error::msg(format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: target room {} is not a legal originium factory",
                room_id.0
            )));
        }
        let result = crate::layout::assign::assign_shift_with_preclaimed_four_gold_comparison(
        blueprint,
        operbox,
        instances,
        table,
        options,
        AssignShiftMode::Peak,
        &planning_seed,
        &execution_seed,
        skip_systems,
        |plan| {
            for name in ORIGINIUM_PAIR_OPERATORS {
                let elite = operbox.elite_of(name).ok_or_else(|| {
                    Error::msg(format!(
                        "{ORIGINIUM_PAIR_SYSTEM_ID}: missing elite for {name}"
                    ))
                })?;
                if plan.anchors.iter().any(|anchor| {
                    anchor.system_id == ORIGINIUM_PAIR_SYSTEM_ID && anchor.operator == name
                }) {
                    continue;
                }
                plan.anchors.push(SystemAnchor {
                    system_id: ORIGINIUM_PAIR_SYSTEM_ID.to_string(),
                    operator: name.to_string(),
                    proactive_support: false,
                    optional_trade_role: None,
                    elite,
                    facility: FacilityKind::Factory,
                    room_id: Some(room_id.clone()),
                    recipe: Some(crate::types::RecipeKind::Originium),
                    trade_order: None,
                    work_mood: None,
                    rest_facility: None,
                    fill_policy: AnchorFillPolicy::ManufactureRecipe,
                });
            }
            if !plan.shift_binds.iter().any(|bind| {
                bind.operators.len() == ORIGINIUM_PAIR_OPERATORS.len()
                    && ORIGINIUM_PAIR_OPERATORS
                        .iter()
                        .all(|name| bind.operators.iter().any(|member| member == name))
            }) {
                plan.shift_binds.push(ShiftBind {
                    operators: ORIGINIUM_PAIR_OPERATORS
                        .iter()
                        .map(|name| (*name).to_string())
                        .collect(),
                    on_shifts,
                    off_shifts,
                });
            }
            Ok(())
        },
    )
    .map_err(|error| match error {
        Error::ManufactureAnchoredCompletionInfeasible {
            room_id: failed_room,
            anchor_count: 2,
        } if failed_room == room_id.0 => Error::OptionalRotationPlacementInfeasible {
            message: format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: target room {} could not be completed with its two preclaimed anchors",
                room_id.0
            ),
        },
        error => error,
    })?;
        let room = result.assignment.operators_in(room_id);
        let missing: Vec<_> = ORIGINIUM_PAIR_OPERATORS
            .iter()
            .filter(|name| !room.iter().any(|operator| operator.name == **name))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(Error::OptionalRotationPlacementInfeasible {
            message: format!(
                "{ORIGINIUM_PAIR_SYSTEM_ID}: assignment pipeline displaced preclaimed pair in room {}: missing {missing:?}",
                room_id.0
            ),
        });
        }
        Ok(result)
    })();
    stage_perf.record_result(&result);
    result
}

#[derive(Debug, Clone)]
pub(crate) struct OwnedRotationAssignment {
    pub(crate) assignment: BaseAssignment,
    pub(crate) plan: AssignmentPlan,
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
    solve_owned_peak_seed(
        blueprint,
        operbox,
        instances,
        table,
        options,
        shift_hours,
        &BaseAssignment::default(),
    )
}

pub(crate) fn solve_owned_peak_seed(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: f64,
    seed: &BaseAssignment,
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
        seed,
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

fn solve_owned_peak_with_originium_pair(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    shift_hours: f64,
    pair: &OriginiumPairContext,
) -> Result<OwnedRotationAssignment> {
    let Some(room_id) = pair.room_id() else {
        let mut ordinary_options = options.clone();
        ordinary_options.shift_hours = shift_hours;
        ordinary_options.preclaimed_factory_room = None;
        return solve_owned_peak_assignment(
            blueprint,
            operbox,
            instances,
            table,
            &ordinary_options,
            shift_hours,
        );
    };
    let mut peak_options = options.clone();
    peak_options.shift_hours = shift_hours;
    peak_options.preclaimed_factory_room = Some(room_id.clone());
    let result = assign_originium_pair_peak(
        blueprint,
        operbox,
        instances,
        table,
        &peak_options,
        &HashSet::new(),
        room_id,
        1,
        1,
    )?;
    let mut plan = result.plan;
    plan.resolve_actual_producer_dependencies(blueprint, &result.assignment, instances, table)?;
    if OriginiumPairContext::from_peak_plan(&plan)?.room_id() != Some(room_id) {
        return Err(Error::msg(format!(
            "{ORIGINIUM_PAIR_SYSTEM_ID}: main/backup peak Plan lost its frozen preclaim room"
        )));
    }
    Ok(OwnedRotationAssignment {
        assignment: result.assignment,
        plan,
    })
}

pub(crate) fn solve_policy_disjoint_backup(
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
    let unavailable = if options.auto_rotation {
        main.work_position_operator_names(blueprint)
    } else {
        main.operator_names()
    };
    let available = operbox.excluding(&unavailable);
    if options.auto_rotation {
        let mut seed = BaseAssignment::default();
        seed.auto_rotation = true;
        seed.work_hours = Some(shift_hours);
        seed.auto_dorm_support_rooms = main.auto_dorm_support_rooms.clone();
        for room in blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::Dormitory)
        {
            seed.set_room(room.id.clone(), main.operators_in(&room.id).to_vec());
        }
        // 主力已完成并冻结；备中枢只使用剩余工作成员。
        super::auto_rotation::select_control(
            blueprint, &available, instances, table, options, &mut seed,
        )?;
        return solve_owned_peak_seed(
            blueprint,
            &available,
            instances,
            table,
            options,
            shift_hours,
            &seed,
        );
    }
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
) -> RotationDraft {
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
    let teams = [main, backup]
        .into_iter()
        .enumerate()
        .map(|(index, owner)| TeamAssignment {
            label: if index == 0 {
                TeamLabel::Alpha
            } else {
                TeamLabel::Beta
            },
            operators: sorted_assignment_names(&owner.assignment)
                .into_iter()
                .filter(|name| {
                    !actions.iter().enumerate().any(|(action_index, action)| {
                        action_index != index
                            && action.as_ref().is_some_and(|action| action.target == *name)
                    })
                })
                .collect(),
        })
        .collect();
    RotationDraft {
        prepared: false,
        report: TeamRotationReport {
            profile: TimedRotationProfile::MainBackup24_24,
            peak_plan: main.plan.clone(),
            runtime_shift_binds: assignment_plans
                .iter()
                .flat_map(shift_binds_from_plan)
                .collect(),
            assignment_plans,
            peak_mood_eta: None,
            teams,
            shifts: vec![make_shift(0, main), make_shift(1, backup)],
            daily: DailyTotals::default(),
            warnings: Vec::new(),
            elapsed: std::time::Duration::ZERO,
        },
    }
}

fn validate_main_backup_24_24_fiammetta_candidate(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    main: &OwnedRotationAssignment,
    backup: &OwnedRotationAssignment,
    actions: &[Option<FiammettaShiftAction>; 2],
) -> Result<(
    OwnedRotationAssignment,
    OwnedRotationAssignment,
    Vec<TeamAssignment>,
)> {
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
    let mut report = main_backup_24_24_probe_report(main, backup, actions);
    prepare_rotation_state(blueprint, operbox, instances, table, &mut report, true)?;
    let mut repeated_shifts = Vec::with_capacity(report.shifts.len() * 2);
    for cycle in 0..2 {
        for shift in &report.shifts {
            let mut repeated = shift.clone();
            repeated.index += cycle * report.shifts.len();
            repeated_shifts.push(repeated);
        }
    }
    let model = default_mood_model()?;
    fiammetta_events::validate_actions(&repeated_shifts, &model, blueprint, instances)
        .map_err(|error| Error::FiammettaCandidateUnavailable(error.to_string()))?;
    Ok((
        OwnedRotationAssignment {
            assignment: report.shifts[0].assignment.clone(),
            plan: main.plan.clone(),
        },
        OwnedRotationAssignment {
            assignment: report.shifts[1].assignment.clone(),
            plan: backup.plan.clone(),
        },
        report.report.teams,
    ))
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
    let candidates = office_candidates(
        operbox,
        instances,
        table,
        &layout,
        options.mood,
        shift_hours,
        &excluded,
    );
    assign_office_for_shift(
        &mut owner.assignment,
        blueprint,
        &HashSet::new(),
        &HashSet::new(),
        &candidates,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]

fn schedule_main_backup_12_12(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<RotationDraft> {
    let mut profile_options = options.clone();
    profile_options.control_work_hours = Some(12.0);
    schedule_main_backup_profile(
        TimedRotationProfile::MainBackup12_12,
        &[12.0, 12.0],
        blueprint,
        operbox,
        instances,
        table,
        &profile_options,
    )
    .map(|(report, _)| report)
}

fn schedule_main_backup_24_24(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<(RotationDraft, Option<MainBackup24FiammettaDiagnostic>)> {
    let mut profile_options = options.clone();
    profile_options.control_work_hours = Some(24.0);
    schedule_main_backup_profile(
        TimedRotationProfile::MainBackup24_24,
        &[24.0, 24.0],
        blueprint,
        operbox,
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

fn schedule_main_backup_profile(
    profile: TimedRotationProfile,
    state_hours: &[f64],
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<(RotationDraft, Option<MainBackup24FiammettaDiagnostic>)> {
    let started = Instant::now();
    let main_hours = state_hours[0];
    let backup_hours = state_hours[1];
    let owner_options = |hours: f64| {
        let mut owner_options = options.clone();
        if profile == TimedRotationProfile::GenericMainBackup {
            owner_options.control_work_hours = Some(hours);
        }
        owner_options
    };
    let main_options = owner_options(main_hours);
    let backup_options = owner_options(backup_hours);
    let mut originium_pair = if profile.is_ordinary_main_backup() {
        OriginiumPairContext::for_ordinary_rotation(blueprint, operbox)?
    } else {
        OriginiumPairContext {
            preclaim: OriginiumPairPreclaim::Inactive,
            warning: None,
        }
    };
    let fiammetta_slots: Vec<usize> = if matches!(
        profile,
        TimedRotationProfile::GenericMainBackup | TimedRotationProfile::GenericAbc
    ) {
        state_hours
            .iter()
            .enumerate()
            .filter_map(|(index, hours)| (*hours > 12.0).then_some(index))
            .collect()
    } else {
        fiammetta_action_slots(profile, state_hours.len())
    };
    let fiammetta_enabled =
        options.fiammetta_enable && operbox.owns("菲亚梅塔") && !fiammetta_slots.is_empty();
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
    let mut main = match solve_owned_peak_with_originium_pair(
        blueprint,
        operbox,
        instances,
        table,
        &main_options,
        main_hours,
        &originium_pair,
    ) {
        Ok(main) => main,
        Err(error)
            if originium_pair.room_id().is_some()
                && originium_pair_preclaim_failure_allows_fallback(&error) =>
        {
            let room_id = originium_pair
                .room_id()
                .cloned()
                .expect("preclaim failure has a target room");
            originium_pair.preclaim = OriginiumPairPreclaim::Inactive;
            originium_pair.warning = Some(format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} {ORIGINIUM_PAIR_WARNING}: {} could not complete room {}; ordinary main retained",
                error, room_id.0
            ));
            let mut ordinary_options = main_options.clone();
            ordinary_options.preclaimed_factory_room = None;
            solve_owned_peak_assignment(
                blueprint,
                operbox,
                instances,
                table,
                &ordinary_options,
                main_hours,
            )?
        }
        Err(error) => return Err(error),
    };
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
    let main_forbidden = main.assignment.operator_names();
    let mut prepared_teams = None;
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
                *fiammetta_rejection_counts
                    .entry("target_absent_from_normal_trade_pool")
                    .or_default() += 1;
                continue;
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
                &backup_options,
                backup_hours,
            ) {
                Ok(assignment) => assignment,
                Err(error @ (Error::NoCandidate(_) | Error::FiammettaCandidateUnavailable(_))) => {
                    *fiammetta_rejection_counts
                        .entry("normal_backup_search_rejected_reentered_target")
                        .or_default() += 1;
                    eprintln!(
                        "[fiammetta-24-24] reentered target {first_target} rejected by normal backup search: {error}"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            fill_owner_office_seats(
                blueprint,
                &candidate_operbox,
                instances,
                table,
                options,
                backup_hours,
                &mut candidate_backup,
                &main_forbidden,
            )?;
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
                let (prepared_main, prepared_backup, teams) =
                    match validate_main_backup_24_24_fiammetta_candidate(
                        blueprint,
                        operbox,
                        instances,
                        table,
                        &main,
                        &candidate_backup,
                        &actions,
                    ) {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            *fiammetta_rejection_counts
                                .entry("action_rejected_pair")
                                .or_default() += 1;
                            eprintln!(
                        "[fiammetta-24-24] pair {first_target} -> {second_target} rejected by action validation: {error}"
                    );
                            reject_fiammetta_candidate(error)?;
                            continue;
                        }
                    };
                prepared_teams = Some(teams);
                main = prepared_main;
                candidate_backup = prepared_backup;

                eprintln!(
                    "[fiammetta-24-24] selected normal Trade targets {first_target} -> {second_target}; each is assigned in its action shift and the actions pass ready / placement / bind replay"
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
                    operbox,
                    instances,
                    table,
                    &main,
                    &candidate_backup,
                    &actions,
                ) {
                    Ok((prepared_main, prepared_backup, teams)) => {
                        prepared_teams = Some(teams);
                        main = prepared_main;
                        candidate_backup = prepared_backup;
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
                            .entry("single_reentered_action_rejected")
                            .or_default() += 1;
                        eprintln!("[fiammetta-24-24] single reentered target rejected: {error}");
                        reject_fiammetta_candidate(error)?;
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
                &backup_options,
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
                    let (prepared_main, prepared_backup, teams) =
                        match validate_main_backup_24_24_fiammetta_candidate(
                            blueprint,
                            operbox,
                            instances,
                            table,
                            &main,
                            &disjoint_backup,
                            &actions,
                        ) {
                            Ok(prepared) => prepared,
                            Err(error) => {
                                *fiammetta_rejection_counts
                                    .entry("action_rejected_disjoint_pair")
                                    .or_default() += 1;
                                eprintln!(
                            "[fiammetta-24-24] disjoint pair {first_target} -> {second_target} rejected by action validation: {error}"
                        );
                                reject_fiammetta_candidate(error)?;
                                continue;
                            }
                        };
                    prepared_teams = Some(teams);
                    main = prepared_main;
                    disjoint_backup = prepared_backup;
                    eprintln!(
                        "[fiammetta-24-24] selected disjoint normal Trade targets {first_target} -> {second_target}; each is assigned in its action shift and the actions pass ready / placement / bind replay"
                    );
                    selected_fiammetta_actions = Some(actions);
                    fiammetta_action_search_path = Some("ordinary_disjoint");
                    break 'disjoint_targets;
                }
            }

            if selected_fiammetta_actions.is_none() {
                let single_main_assignment = main.assignment.clone();
                let single_backup_assignment = disjoint_backup.assignment.clone();
                'single_targets: for (index, candidates, assignment) in [
                    (0usize, &selected_main_targets, &single_main_assignment),
                    (1usize, &disjoint_backup_targets, &single_backup_assignment),
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
                            operbox,
                            instances,
                            table,
                            &main,
                            &disjoint_backup,
                            &actions,
                        ) {
                            Ok((prepared_main, prepared_backup, teams)) => {
                                prepared_teams = Some(teams);
                                main = prepared_main;
                                disjoint_backup = prepared_backup;
                                selected_fiammetta_actions = Some(actions);
                                fiammetta_action_search_path = Some("ordinary_disjoint");
                                break 'single_targets;
                            }
                            Err(error) => {
                                *fiammetta_rejection_counts
                                    .entry("single_disjoint_action_rejected")
                                    .or_default() += 1;
                                eprintln!(
                                    "[fiammetta-24-24] single disjoint target rejected: {error}"
                                );
                                reject_fiammetta_candidate(error)?;
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
        None => {
            let backup = solve_policy_disjoint_backup(
                blueprint,
                operbox,
                instances,
                table,
                &backup_options,
                &main.assignment,
                backup_hours,
            )?;
            backup
        }
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
    }
    let teams = prepared_teams.unwrap_or_else(|| {
        vec![
            TeamAssignment {
                label: TeamLabel::Alpha,
                operators: sorted_assignment_names(&main.assignment),
            },
            TeamAssignment {
                label: TeamLabel::Beta,
                operators: sorted_assignment_names(&backup.assignment),
            },
        ]
    });
    let (first_fiammetta, second_fiammetta) = if profile == TimedRotationProfile::MainBackup24_24 {
        selected_fiammetta_actions
            .as_ref()
            .map(|actions| (actions[0].clone(), actions[1].clone()))
            .unwrap_or((None, None))
    } else {
        let first = if fiammetta_enabled && fiammetta_slots.contains(&0) {
            {
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
            }
        } else {
            None
        };
        let second = if fiammetta_enabled && fiammetta_slots.contains(&1) {
            {
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
            }
        } else {
            None
        };
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
    let owners = vec![main, backup];
    let mut report = build_rotation_report_shell(
        profile, blueprint, instances, owners, teams, states, started,
    )?;
    report.prepared = selected_fiammetta_actions.is_some();
    if let Some(warning) = originium_pair.warning() {
        report.warnings.push(warning);
    }
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
) -> Result<RotationDraft> {
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
) -> Result<RotationDraft> {
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
        false,
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
    let mut report = build_rotation_report_shell(
        TimedRotationProfile::Fiammetta8_8_4_4,
        blueprint,
        instances,
        owners,
        raw.teams.clone(),
        states,
        started,
    )?;
    report.runtime_shift_binds = binds;
    prepare_rotation_state(blueprint, operbox, instances, table, &mut report, false)?;
    let ordinary = report.clone();
    report.prepared = false;
    if options.fiammetta_enable {
        let inactive_originium_pair = OriginiumPairContext {
            preclaim: OriginiumPairPreclaim::Inactive,
            warning: None,
        };
        let applied = match apply_profile_fiammetta_rotation(
            blueprint,
            operbox,
            instances,
            table,
            options,
            TimedRotationProfile::Fiammetta8_8_4_4,
            &report.report.teams,
            &raw.peak_plan.continuous_roles,
            &report.report.runtime_shift_binds,
            &inactive_originium_pair,
            &mut report.report.shifts,
        ) {
            Ok(applied) => applied,
            Err(error) => return Err(error),
        };
        if !applied {
            report.warnings.push(format!(
                "{FINAL_INVARIANT_WARNING_PREFIX} fiammetta-8844 has no accepted Fiammetta action; ordinary 8/8/4/4 assignments retained"
            ));
        }
        for shift in &mut report.report.shifts {
            if shift.fiammetta.is_some() {
                shift.transition = ShiftTransition::FiammettaOnly;
                shift.drone = None;
            }
        }
    }
    if let Err(error) =
        prepare_rotation_state(blueprint, operbox, instances, table, &mut report, true)
    {
        match error {
            Error::FiammettaCandidateUnavailable(message) => {
                report = ordinary;
                report.warnings.push(format!("{FINAL_INVARIANT_WARNING_PREFIX} fiammetta-8844 candidate unavailable: {message}; ordinary state retained"));
            }
            error => return Err(error),
        }
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
) -> Result<RotationDraft> {
    let started = Instant::now();
    let mut main_options = options.clone();
    main_options.control_work_hours = Some(7.0);
    let mut main =
        match solve_abyssal_profile_main(blueprint, operbox, instances, table, &main_options)? {
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
    let mut backup_options = options.clone();
    backup_options.control_work_hours = Some(5.0);
    let mut backup = solve_policy_disjoint_backup(
        blueprint,
        operbox,
        instances,
        table,
        &backup_options,
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
    let mut report = build_rotation_report_shell(
        TimedRotationProfile::Abyssal7_5_7_5,
        blueprint,
        instances,
        vec![main, backup],
        teams,
        states,
        started,
    )?;
    if dorm_rest.is_none() {
        report.warnings.push(format!(
            "{FINAL_INVARIANT_WARNING_PREFIX} abyssal-7575 has no available single-target/group dorm manager placement; no dorm rest group emitted"
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
fn build_rotation_report_shell(
    profile: TimedRotationProfile,
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    owners: Vec<OwnedRotationAssignment>,
    teams: Vec<TeamAssignment>,
    states: Vec<RotationStateSpec>,
    started: Instant,
) -> Result<RotationDraft> {
    if owners.is_empty() || states.is_empty() {
        return Err(Error::msg("timed rotation requires assignments and states"));
    }
    let mut shifts = Vec::with_capacity(states.len());
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
        shifts.push(TeamShiftResult {
            index,
            plan_index: state.owner_index,
            duration_hours: state.duration_hours,
            active_teams: state.active_teams.clone(),
            resting_team: state.resting_team,
            assignment,
            fiammetta: state.fiammetta.clone(),
            dorm_rest: state.dorm_rest.clone(),
            drone: drone_action(blueprint, state.transition),
            transition: state.transition,
            efficiencies: ShiftEfficiencies::default(),
            weighted_trade: Efficiency::ZERO,
            weighted_manufacture: Efficiency::ZERO,
            weighted_power: Efficiency::ZERO,
        });
    }
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
    let report = RotationDraft {
        prepared: false,
        report: TeamRotationReport {
            profile,
            peak_plan,
            assignment_plans,
            runtime_shift_binds,
            peak_mood_eta,
            teams,
            shifts,
            daily: DailyTotals::default(),
            warnings: Vec::new(),
            elapsed: started.elapsed(),
        },
    };
    Ok(report)
}

fn validate_profile_structure(
    blueprint: &BaseBlueprint,
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

    Ok(())
}

fn validate_profile_team_membership(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
) -> Result<()> {
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
        for shift in &report.shifts {
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
            for operator in shift.assignment.work_position_operator_names(blueprint) {
                let teams = team_by_name
                    .get(operator.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if operator == FIAMMETTA_NAME && fiammetta_dorm_action {
                    continue;
                }
                if fiammetta_targets.contains(&operator) {
                    let action_owner = report.shifts.iter().find_map(|action_shift| {
                        action_shift
                            .fiammetta
                            .as_ref()
                            .filter(|action| action.target == operator)
                            .and_then(|_| action_shift.active_teams.first().copied())
                    });
                    if teams.len() != 1 || action_owner != teams.first().copied() {
                        return Err(Error::msg(format!(
                            "shift {} Fiammetta action target {operator} must belong only to its action owner",
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
                    [single] if shift.active_teams.contains(single) => {}
                    [_] => {
                        return Err(Error::msg(format!(
                            "shift {} operator {operator} belongs to an inactive team",
                            shift.index + 1
                        )))
                    }
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

    Ok(())
}

fn validate_profile_work_disjoint(
    blueprint: &BaseBlueprint,
    report: &TeamRotationReport,
) -> Result<()> {
    if report.profile.is_two_team() {
        let fiammetta_targets = main_backup_24_24_fiammetta_targets_from_report(report);
        for label in [TeamLabel::Alpha, TeamLabel::Beta] {
            if !report.teams.iter().any(|team| team.label == label) {
                return Err(Error::msg(format!(
                    "cannot evaluate work disjointness without {label:?} Team"
                )));
            }
        }
        let alpha_work = report
            .shifts
            .first()
            .map(|shift| shift.assignment.work_position_operator_names(blueprint))
            .ok_or_else(|| Error::msg("cannot evaluate work disjointness without first shift"))?;
        let beta_work = report
            .shifts
            .get(1)
            .map(|shift| shift.assignment.work_position_operator_names(blueprint))
            .ok_or_else(|| Error::msg("cannot evaluate work disjointness without second shift"))?;
        let overlap: HashSet<_> = alpha_work.intersection(&beta_work).cloned().collect();
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

    Ok(())
}

fn validate_abyssal_profile_structure(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
) -> Result<()> {
    if report.profile == TimedRotationProfile::Abyssal7_5_7_5
        && report
            .peak_plan
            .selected_rules
            .iter()
            .any(|rule| rule.rule_id == "timed_rotation_abyssal_7_5_7_5")
    {
        let producer_shift = report.shifts.first().ok_or_else(|| {
            Error::msg("cannot evaluate abyssal profile structure without first shift")
        })?;
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

pub(crate) fn assert_assignment_identity(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &BaseAssignment,
    plan: &AssignmentPlan,
) -> Result<()> {
    let violations = assignment.structural_violations(blueprint);
    if !violations.is_empty() {
        return Err(Error::msg(format!(
            "assert-invariants assignment_structure failed: {}",
            violations.join("; ")
        )));
    }
    for room in assignment.iter_rooms() {
        for operator in &room.operators {
            let progress = operbox.progress_of(&operator.name);
            if progress.is_none_or(|progress| {
                progress.elite != operator.elite
                    || (operator.level != 0 && progress.level != operator.level)
                    || (operator.rarity != 0 && progress.rarity != operator.rarity)
            }) || plan.excluded_operators.contains(&operator.name)
            {
                return Err(Error::msg(format!(
                    "assert-invariants ordinary_fallback_admission failed: invalid member {}",
                    operator.name
                )));
            }
        }
    }
    Ok(())
}
