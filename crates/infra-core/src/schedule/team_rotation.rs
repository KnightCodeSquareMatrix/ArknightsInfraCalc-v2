use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
#[cfg(test)]
use crate::layout::RoomAssignment;
use crate::layout::{
    assign_control, assign_manu_room_with_anchors, assign_shift_with_plan_skip,
    assign_shift_with_preclaimed_plan, assign_team_gamma_half, pinned_assignment_excluding,
    resolve_base, ActivatedSystem, AnchorFillPolicy, AssignBaseOptions, AssignShiftMode,
    AssignedOperator, AssignmentPlan, BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext,
    ReserveReusePolicy, ResolvedRoleReserve, RoomId, RoomProduct, SelectedRuleAlternative,
    ShiftBind, SlotFill, SystemAnchor,
};
use crate::mood::{
    dorm_recovery_rates, shift_eta, shift_eta_with_instances, DormOccupant, MoodModel, ShiftEta,
};
use crate::operbox::OperBox;
use crate::pool::{
    add_jie_market_to_trade_pool, build_control_pool_with_fillers, build_manufacture_pool,
    build_power_pool, build_trade_pool, karlan_precision_active, ManuPool, ManuPoolEntry,
};
use crate::search::{
    control_efficiency_fill_sort_weight, control_entry_plugin_fill,
    control_inject_components_for_assignment, manufacture_operator_has_mood_gap_decay,
    MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS,
};
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

use super::base_rotation::{
    evaluate_base_assignment_efficiencies, RoomEfficiencyLine, ShiftEfficiencies,
};
use super::shift_bind::{shift_binds_from_plan, RuntimeShiftBind};

const HALF_HOUR_SAFETY_MARGIN: f64 = 0.5;

/// Closed set of timed-rotation products supported by the runtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimedRotationProfile {
    /// Backward-compatible αβγ rotation.
    #[default]
    #[serde(rename = "abc_12_6_6")]
    Abc12_6_6,
    /// Two complete, disjoint main/backup states.
    #[serde(rename = "main_backup_12_12")]
    MainBackup12_12,
    /// Split 16/4/4 at the Fiammetta event boundary.
    #[serde(rename = "fiammetta_8_8_4_4")]
    Fiammetta8_8_4_4,
    /// Required Abyssal main alternating with an independent backup.
    #[serde(rename = "abyssal_7_5_7_5")]
    Abyssal7_5_7_5,
}

impl TimedRotationProfile {
    pub const fn cli_name(self) -> &'static str {
        match self {
            Self::Abc12_6_6 => "3",
            Self::MainBackup12_12 => "2",
            Self::Fiammetta8_8_4_4 => "fiammetta-8844",
            Self::Abyssal7_5_7_5 => "abyssal-7575",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Abc12_6_6 => "ABC 三班 12/6/6",
            Self::MainBackup12_12 => "二班主替 12/12",
            Self::Fiammetta8_8_4_4 => "菲亚插班 8/8/4/4",
            Self::Abyssal7_5_7_5 => "深海四班 7/5/7/5",
        }
    }

    pub const fn plan_times(self) -> &'static str {
        match self {
            Self::Abc12_6_6 => "3班",
            Self::MainBackup12_12 => "2班",
            Self::Fiammetta8_8_4_4 | Self::Abyssal7_5_7_5 => "4班",
        }
    }

    pub const fn is_two_team(self) -> bool {
        matches!(self, Self::MainBackup12_12 | Self::Abyssal7_5_7_5)
    }
}

impl std::str::FromStr for TimedRotationProfile {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "3" | "abc" | "12/6/6" | "abc-1266" => Ok(Self::Abc12_6_6),
            "2" | "12/12" | "two-shift" | "main-backup" => Ok(Self::MainBackup12_12),
            "fiammetta-8844" | "8/8/4/4" => Ok(Self::Fiammetta8_8_4_4),
            "abyssal-7575" | "7/5/7/5" => Ok(Self::Abyssal7_5_7_5),
            "4" => Err(Error::msg(
                "rotation 4 requires a named profile: fiammetta-8844 or abyssal-7575",
            )),
            other => Err(Error::msg(format!(
                "unsupported rotation {other:?}; expected 2, 3, fiammetta-8844, or abyssal-7575"
            ))),
        }
    }
}

/// How the executor reaches a state from the preceding state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftTransition {
    #[default]
    ReplaceRooms,
    /// Run Fiammetta and restore the target room; all other rooms remain untouched.
    FiammettaOnly,
}

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

/// 菲亚梅塔在某个班次执行的一次主力回岗覆盖。
#[derive(Debug, Clone, Serialize)]
pub struct FiammettaShiftAction {
    /// 获得换心情并重新回到原岗位的主力。
    pub target: String,
    /// 被主力替换下岗、应进入休息区的原当班干员。
    pub displaced: String,
    /// 主力在 peak 班中的原房间，也是本次替换发生的房间。
    pub room_id: RoomId,
}

/// Explicit same-dorm recovery group confirmed by the schedule layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DormRestPlan {
    pub room_id: RoomId,
    pub target: String,
    pub single_manager: String,
    pub group_manager: String,
}

/// 无人机动作由 schedule 层决策：目标必须是实际产品为赤金的制造房；
/// 无合法赤金房或菲亚-only 班次时不启用。export 只负责序列化映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DroneAction {
    pub room_id: RoomId,
    pub order: &'static str,
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

/// 单个班次结果：当班两队合起来铺满全部设施。
#[derive(Debug, Clone, Serialize)]
pub struct TeamShiftResult {
    pub index: usize,
    /// Owning entry in `TeamRotationReport.assignment_plans`.
    pub plan_index: usize,
    pub duration_hours: f64,
    pub active_teams: Vec<TeamLabel>,
    pub resting_team: TeamLabel,
    pub assignment: BaseAssignment,
    /// 菲亚梅塔使休息队主力额外回岗的单次覆盖；没有可接受目标时为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fiammetta: Option<FiammettaShiftAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dorm_rest: Option<DormRestPlan>,
    /// 班次无人机动作；菲亚-only 或无赤金制造房时为 `None`（export 输出 `enable:false`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drone: Option<DroneAction>,
    pub transition: ShiftTransition,
    pub efficiencies: ShiftEfficiencies,
    /// 贸易效率按时长折算（三类各自独立，不混合量纲）。
    pub weighted_trade: Efficiency,
    /// 制造效率按时长折算。
    pub weighted_manufacture: Efficiency,
    /// 发电效率按时长折算。
    pub weighted_power: Efficiency,
}

/// 三类各自的每日加权产出（贸易/制造/发电分开，不相加）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct DailyTotals {
    pub trade: Efficiency,
    pub manufacture: Efficiency,
    pub power: Efficiency,
}

/// αβγ 三队轮换报告。
#[derive(Debug, Clone, Serialize)]
pub struct TeamRotationReport {
    pub profile: TimedRotationProfile,
    /// peak 班编排计划（只读；α/β 切半与 γ 贸易 role 填充均据此对齐）。
    pub peak_plan: AssignmentPlan,
    /// Every independently solved assignment keeps its own Plan owner.
    pub assignment_plans: Vec<AssignmentPlan>,
    /// 最高效率 peak 编制从满心情工作到首个瓶颈触发的最长时间。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_mood_eta: Option<ShiftEta>,
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
            peak.rooms.iter().any(|room| {
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
    peak.rooms
        .iter()
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|bp| bp.kind == FacilityKind::Office)
        })
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
            peak.rooms.iter().any(|room| {
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
            peak.rooms.iter().any(|room| {
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
        let Some(peak_room) = peak
            .rooms
            .iter()
            .find(|room| room.operators.iter().any(|operator| operator.name == *name))
        else {
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
fn pin_active_dependency_required(
    shared: &mut BaseAssignment,
    peak: &BaseAssignment,
    dependencies: &[crate::layout::ActiveDependency],
) -> Result<()> {
    let mut required: Vec<_> = dependencies
        .iter()
        .flat_map(|dependency| &dependency.required)
        .collect();
    required.sort();
    required.dedup();
    for name in required {
        let Some(peak_room) = peak
            .rooms
            .iter()
            .find(|room| room.operators.iter().any(|operator| operator.name == *name))
        else {
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

/// 办公室轮换候选：全 roster 按 `support_skill_registry` 静态办公室速度评分排序
/// （无技能干员 0 分，按名序兜底）。
///
/// 评分固定使用单人办公室语义（办公室一人房）与 24 小时、0 额外招募位和 shared
/// 脚手架 layout；`hire_spd_cost[210]` 等动态条件按注册表求值。这是静态 heuristic，
/// 不反映当班全局资源或短班时长，不声称办公室全局最优。与 `meeting_candidates` 同口径。
fn office_candidates(
    operbox: &OperBox,
    _instances: &OperatorInstances,
    _table: &SkillTable,
    layout: &LayoutContext,
    _mood: f64,
    excluded: &HashSet<String>,
) -> Vec<AssignedOperator> {
    let registry = crate::support_facility::SupportRegistry::load_default()
        .expect("support registry loads in office candidate build");
    let roster = operbox.roster();
    let mut candidates: Vec<(f64, AssignedOperator)> = roster
        .names()
        .filter(|name| !excluded.contains(name.as_str()))
        .filter_map(|name| {
            let progress = roster.progress(name)?;
            let score = crate::office::evaluate_office(
                &crate::support_facility::SupportRoomInput {
                    facility: crate::support_facility::SupportFacility::Office,
                    operators: vec![crate::support_facility::SupportOperator {
                        name: name.clone(),
                        elite: progress.elite,
                        level: progress.level,
                    }],
                    capacity: 1,
                    extra_recruit_slots: 0,
                    elapsed_hours: 24.0,
                    external_speed_bonus_pct: 0.0,
                    layout: layout.clone(),
                },
                &registry,
            )
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

/// 为当班办公室满编选人。
///
/// 候选来源是当前活跃的两个 cohort（D4：候选仅在所属 cohort 活跃时具备上岗资格）。
/// 排序规则：绑定成员始终优先且不受轮换限制（必须随消费方同上同下）；
/// 普通成员优先选本完整循环尚未上岗者（跨班次轮换），同一普通成员不会在多个班次
/// 连续复用导致心情超限。不保证普通成员个人上二休一，出勤受当班办公室席位约束。
/// 返回本班新上岗的普通成员，供调用方更新跨班次轮换集合。
fn assign_rotating_offices(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_teams: &[TeamLabel],
    rotating_team: &HashMap<String, TeamLabel>,
    bound_names: &HashSet<String>,
    candidates: &[AssignedOperator],
    used_office: &HashSet<String>,
) -> Result<Vec<String>> {
    let mut used = assignment.operator_names();
    // 活跃两队都是合法候选来源；绑定成员优先，其余普通成员按“本循环未上岗 → 效率”排序。
    let mut active_rotating: Vec<&String> = rotating_team
        .iter()
        .filter(|(_, team)| active_teams.contains(team))
        .map(|(name, _)| name)
        .collect();
    active_rotating.sort_by_key(|name| {
        let rank = candidates
            .iter()
            .position(|op| op.name == name.as_str())
            .unwrap_or(usize::MAX);
        (
            !bound_names.contains(*name),
            !used_office.contains(*name),
            rank,
        )
    });
    let mut newly_used = Vec::new();
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
    {
        let mut operators = assignment.operators_in(&room.id).to_vec();
        while operators.len() < room.operator_capacity() {
            let rotating = active_rotating
                .iter()
                .find(|name| {
                    !used.contains(name.as_str())
                        && bound_names.contains(name.as_str())
                        && !used_office.contains(name.as_str())
                })
                .or_else(|| {
                    active_rotating.iter().find(|name| {
                        !used.contains(name.as_str()) && bound_names.contains(name.as_str())
                    })
                })
                .or_else(|| {
                    active_rotating.iter().find(|name| {
                        !used.contains(name.as_str()) && !used_office.contains(name.as_str())
                    })
                })
                .or_else(|| {
                    active_rotating
                        .iter()
                        .find(|name| !used.contains(name.as_str()))
                });
            let replacement = candidates
                .iter()
                .find(|op| !used.contains(&op.name) && !rotating_team.contains_key(&op.name));
            let Some(op) = rotating
                .and_then(|name| candidates.iter().find(|op| op.name == name.as_str()))
                .or(replacement)
                .cloned()
            else {
                return Err(Error::msg(format!(
                    "office {} has no legal rotating operator or replacement",
                    room.id.0
                )));
            };
            if !bound_names.contains(&op.name) {
                newly_used.push(op.name.clone());
            }
            used.insert(op.name.clone());
            operators.push(op);
        }
        assignment.set_room(room.id.clone(), operators);
    }
    Ok(newly_used)
}

/// 把当班 assignment 中实际入驻的非绑定办公室成员记入跨班次轮换集合。
fn record_office_usage(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    bound_names: &HashSet<String>,
    used_office: &mut HashSet<String>,
) {
    for room in assignment.rooms.iter().filter(|room| {
        blueprint
            .room(&room.room_id)
            .is_some_and(|bp| bp.kind == FacilityKind::Office)
    }) {
        for operator in &room.operators {
            if !bound_names.contains(&operator.name) {
                used_office.insert(operator.name.clone());
            }
        }
    }
}

fn complete_rotating_office_teams(
    blueprint: &BaseBlueprint,
    peak_office_names: &HashSet<String>,
    rotating_team: &mut HashMap<String, TeamLabel>,
    office_pool: &[AssignedOperator],
    reserved: &mut HashSet<String>,
) -> Result<()> {
    let capacity = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Office)
        .map(|room| room.operator_capacity())
        .sum::<usize>();
    if capacity == 0 {
        return Ok(());
    }

    let mut unbound_peak: Vec<_> = peak_office_names
        .iter()
        .filter(|name| !rotating_team.contains_key(*name))
        .cloned()
        .collect();
    unbound_peak.sort();
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

    for team in TeamLabel::ALL {
        while rotating_team
            .values()
            .filter(|assigned| **assigned == team)
            .count()
            < capacity
        {
            let Some(operator) = office_pool.iter().find(|operator| {
                !reserved.contains(&operator.name) && !rotating_team.contains_key(&operator.name)
            }) else {
                return Err(Error::msg(format!(
                    "office cohort {team:?} has no legal rotating operator"
                )));
            };
            rotating_team.insert(operator.name.clone(), team);
            reserved.insert(operator.name.clone());
        }
    }
    Ok(())
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
    let registry = crate::support_facility::SupportRegistry::load_default()
        .expect("support registry loads in meeting candidate build");
    let roster = operbox.roster();
    let extra_recruit_slots = 0;
    let partner_sentinel = crate::support_facility::SupportOperator {
        name: "\0meeting-partner-sentinel".to_string(),
        elite: 0,
        level: 1,
    };
    let mut candidates: Vec<(f64, AssignedOperator)> = roster
        .names()
        .filter(|name| !excluded.contains(name.as_str()))
        .filter_map(|name| {
            let progress = roster.progress(name)?;
            let score = crate::meeting::evaluate_meeting(
                &crate::support_facility::SupportRoomInput {
                    facility: crate::support_facility::SupportFacility::Meeting,
                    operators: vec![
                        crate::support_facility::SupportOperator {
                            name: name.clone(),
                            elite: progress.elite,
                            level: progress.level,
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

/// 每班从活跃队中选会客室成员填满会客室（每间两人）。
///
/// 活跃队成员按会客室候选评分序选择；cohort 不足时兜底 replacement，但
/// replacement 必须排除休息队与所有已编队成员（`reserved`），避免休息队
/// 成员因 meeting 兜底而返岗破坏"每班两队上岗、一队休息"的不变量。
fn assign_rotating_meetings(
    assignment: &mut BaseAssignment,
    blueprint: &BaseBlueprint,
    active_teams: &[TeamLabel],
    resting_team: TeamLabel,
    rotating_team: &HashMap<String, TeamLabel>,
    candidates: &[AssignedOperator],
    reserved: &HashSet<String>,
) -> Result<()> {
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
        while operators.len() < room.operator_capacity() {
            let rotating = active_rotating
                .iter()
                .find(|name| !used.contains(name.as_str()));
            let replacement = candidates.iter().find(|op| {
                !used.contains(&op.name)
                    && !rotating_team.contains_key(&op.name)
                    && !resting_reserved.contains(&op.name)
                    && !reserved.contains(&op.name)
            });
            let Some(op) = rotating
                .and_then(|name| candidates.iter().find(|op| op.name == name.as_str()))
                .or_else(|| replacement)
                .cloned()
            else {
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
            !reserved.contains(&operator.name) && !rotating_team.contains_key(&operator.name)
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

fn peak_meeting_operators(blueprint: &BaseBlueprint, peak: &BaseAssignment) -> HashSet<String> {
    peak.rooms
        .iter()
        .filter(|room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|bp| bp.kind == FacilityKind::MeetingRoom)
        })
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

fn active_control_candidate_count(
    by_team: &HashMap<TeamLabel, Vec<String>>,
    active_teams: &[TeamLabel],
    unavailable: &HashSet<String>,
) -> usize {
    active_teams
        .iter()
        .flat_map(|team| by_team.get(team).into_iter().flatten())
        .filter(|name| !unavailable.contains(*name))
        .collect::<HashSet<_>>()
        .len()
}

fn least_available_control_team(
    by_team: &HashMap<TeamLabel, Vec<String>>,
    unavailable: &HashSet<String>,
) -> TeamLabel {
    TeamLabel::ALL
        .into_iter()
        .min_by_key(|team| active_control_candidate_count(by_team, &[*team], unavailable))
        .unwrap_or(TeamLabel::Alpha)
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
    Ok(halves)
}

/// γ 替补半区：贸易沿用 core role 顺序，制造/发电站绑定搜索。
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
    let mut trade_pool = pools.trade.clone();
    if karlan_precision_active(&layout.global_inject) {
        add_jie_market_to_trade_pool(&mut trade_pool, instances, table);
    }
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

fn bound_control_operators_for_half(
    blueprint: &BaseBlueprint,
    peak: &BaseAssignment,
    binds: &[RuntimeShiftBind],
    half: &FacilityHalf,
) -> HashSet<String> {
    let room_for = |name: &str| {
        peak.rooms
            .iter()
            .find(|room| room.operators.iter().any(|operator| operator.name == name))
    };
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

fn validate_final_rotation_invariants(
    shifts: &[TeamShiftResult],
    binds: &[RuntimeShiftBind],
    active_dependencies: &[crate::layout::ActiveDependency],
) -> Result<()> {
    for shift in shifts {
        let mut seen = HashSet::new();
        for room in &shift.assignment.rooms {
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
        if control.len() != 5 {
            return Err(Error::msg(format!(
                "shift {} control center has {} operators, expected 5",
                shift.index + 1,
                control.len()
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

fn clear_room_efficiency(assignment: &mut BaseAssignment, room_id: &RoomId) {
    if let Some(room) = assignment
        .rooms
        .iter_mut()
        .find(|room| room.room_id == *room_id)
    {
        room.efficiency = None;
    }
}

fn clear_production_efficiencies(blueprint: &BaseBlueprint, assignment: &mut BaseAssignment) {
    for room in &blueprint.rooms {
        if matches!(
            room.kind,
            FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
        ) {
            clear_room_efficiency(assignment, &room.id);
        }
    }
}

/// 当前轻量策略每个 24h αβγ 周期只安排一次菲亚梅塔回岗。
///
/// 顺序是公孙长乐确认的常规线性 fallback；布局动态排序和龙巫成组服务留给
/// 后续完整心情排班器。
/// 尚未迁移到声明式 continuous role 的 legacy fallback。
/// 但书/可露希尔只由 `AssignmentPlan.continuous_roles` 决定，不在这里保留第二事实源。
pub const FIAMMETTA_RETURN_PRIORITY: [&str; 3] = ["巫恋", "龙舌兰", "清流"];

fn fiammetta_target_names(
    continuous_roles: &[crate::layout::ContinuousRole],
    shift_binds: &[RuntimeShiftBind],
) -> Vec<String> {
    let mut target_names: Vec<String> = continuous_roles
        .iter()
        .map(|role| role.operator.clone())
        .collect();
    for name in FIAMMETTA_RETURN_PRIORITY {
        if shift_binds.iter().any(|bind| {
            bind.operators.len() > 1 && bind.operators.iter().any(|operator| operator == name)
        }) {
            continue;
        }
        if !target_names.iter().any(|target| target == name) {
            target_names.push(name.to_string());
        }
    }
    target_names
}

fn first_fiammetta_target(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    continuous_roles: &[crate::layout::ContinuousRole],
    shift_binds: &[RuntimeShiftBind],
) -> Option<(String, RoomId)> {
    fiammetta_target_names(continuous_roles, shift_binds)
        .into_iter()
        .find_map(|target| {
            assignment.rooms.iter().find_map(|room| {
                let room_blueprint = blueprint.room(&room.room_id)?;
                (matches!(
                    room_blueprint.kind,
                    FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
                ) && room
                    .operators
                    .iter()
                    .any(|operator| operator.name == target))
                .then(|| (target.clone(), room.room_id.clone()))
            })
        })
}

fn production_efficiency(
    efficiencies: &ShiftEfficiencies,
    kind: FacilityKind,
) -> Option<Efficiency> {
    match kind {
        FacilityKind::TradePost => Some(efficiencies.trade_efficiency),
        FacilityKind::Factory => Some(efficiencies.manufacture_efficiency),
        FacilityKind::PowerPlant => Some(efficiencies.power_efficiency),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_fiammetta_return(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    durin_plan: u8,
    peak: &BaseAssignment,
    teams: &[TeamAssignment],
    continuous_roles: &[crate::layout::ContinuousRole],
    shift_binds: &[RuntimeShiftBind],
    shifts: &mut [TeamShiftResult],
) -> Result<()> {
    if !operbox.owns("菲亚梅塔") {
        return Ok(());
    }

    for target_name in fiammetta_target_names(continuous_roles, shift_binds) {
        let Some(source_room) = peak
            .rooms
            .iter()
            .find(|room| room.operators.iter().any(|op| op.name == target_name))
        else {
            continue;
        };
        let Some(room_blueprint) = blueprint.room(&source_room.room_id) else {
            continue;
        };
        if !matches!(
            room_blueprint.kind,
            FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
        ) {
            continue;
        }
        let Some(target_op) = source_room
            .operators
            .iter()
            .find(|op| op.name == target_name)
            .cloned()
        else {
            continue;
        };
        let Some(target_team) = teams
            .iter()
            .find(|team| team.operators.iter().any(|name| name == &target_name))
            .map(|team| team.label)
        else {
            continue;
        };
        let Some(shift_index) = shifts
            .iter()
            .position(|shift| shift.resting_team == target_team)
        else {
            continue;
        };
        if shifts[shift_index]
            .assignment
            .operator_names()
            .contains(&target_name)
        {
            continue;
        }

        let hours = shifts[shift_index].duration_hours;
        let Some(current_room) = shifts[shift_index]
            .assignment
            .room_assignment(&source_room.room_id)
        else {
            continue;
        };

        let mut best: Option<(
            BaseAssignment,
            ShiftEfficiencies,
            AssignedOperator,
            Efficiency,
        )> = None;
        for slot in 0..current_room.operators.len() {
            let mut candidate = shifts[shift_index].assignment.clone();
            let Some(room) = candidate
                .rooms
                .iter_mut()
                .find(|room| room.room_id == source_room.room_id)
            else {
                continue;
            };
            let displaced = room.operators[slot].clone();
            room.operators[slot] = target_op.clone();
            room.efficiency = None;

            let Ok(scores) = evaluate_base_assignment_efficiencies(
                blueprint,
                &candidate,
                instances,
                table,
                hours,
                Some(durin_plan),
            ) else {
                continue;
            };
            let candidate_score = production_efficiency(&scores, room_blueprint.kind)
                .expect("production room kind checked above");
            let replace = best
                .as_ref()
                .is_none_or(|(_, _, _, best_score)| candidate_score > *best_score);
            if replace {
                best = Some((candidate, scores, displaced, candidate_score));
            }
        }

        let Some((assignment, scores, displaced, _)) = best else {
            continue;
        };
        let shift = &mut shifts[shift_index];
        shift.assignment = assignment;
        shift.weighted_trade = scores.weighted_trade(hours);
        shift.weighted_manufacture = scores.weighted_manufacture(hours);
        shift.weighted_power = scores.weighted_power(hours);
        shift.efficiencies = scores;
        shift.fiammetta = Some(FiammettaShiftAction {
            target: target_name,
            displaced: displaced.name,
            room_id: source_room.room_id.clone(),
        });
        break;
    }

    Ok(())
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
    assignment.rooms.iter().find_map(|room| {
        let bp_room = blueprint.room(&room.room_id)?;
        (bp_room.kind == FacilityKind::TradePost && room_has_all(room, names))
            .then(|| room.room_id.clone())
    })
}

fn swap_room_assignments(assignment: &mut BaseAssignment, a: &RoomId, b: &RoomId) -> bool {
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

fn room_containing_operator(
    assignment: &BaseAssignment,
    blueprint: &BaseBlueprint,
    name: &str,
    kind: FacilityKind,
) -> Option<RoomId> {
    assignment.rooms.iter().find_map(|room| {
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
        .rooms
        .iter()
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
        if rooms_compatible_for_swap(blueprint, &current_room, &anchor_room, kind) {
            if swap_room_assignments(assignment, &current_room, &anchor_room) {
                clear_room_efficiency(assignment, &current_room);
                clear_room_efficiency(assignment, &anchor_room);
            }
        }
    }
}

struct AbyssalCandidate {
    assignment: BaseAssignment,
    gamma_ops: Vec<String>,
    control_producer: String,
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
    let Some(control_producer) = abyssal_control_producer(ctx.operbox, ctx.instances, ctx.used_ab)
    else {
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

/// 从编排计划中提取体系绑定的中枢干员名（含 PickOne 候选人）。
fn system_control_operators(plan: &AssignmentPlan) -> HashSet<String> {
    let mut names = HashSet::new();
    names.extend(
        plan.anchors
            .iter()
            .filter(|anchor| anchor.facility == FacilityKind::ControlCenter)
            .map(|anchor| anchor.operator.clone()),
    );
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

/// 是否为 producer_rules 登记的 deferred_optional 动态 producer。
/// 这类 producer 只能由 peak 的 winner→bind 或普通中枢搜索自然选中，
/// 轮换层不得通过注入覆盖后置强塞（ADR 0004 D6）。
fn entry_is_deferred_dynamic_producer(
    entry: &crate::pool::ControlPoolEntry,
    table: &SkillTable,
) -> bool {
    crate::response_dependency::producer_rule_catalog(table)
        .map(|catalog| {
            entry.buff_ids.iter().any(|bid| {
                catalog.rules.iter().any(|rule| {
                    rule.admission
                        == crate::response_dependency::ProducerAdmission::DeferredOptional
                        && rule.source_buff_id == *bid
                })
            })
        })
        .unwrap_or(false)
}

fn control_replace_rank(
    op: &AssignedOperator,
    system_ctrl_names: &HashSet<String>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
) -> i32 {
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
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
    durin_plan: Option<u8>,
    final_pool: &crate::pool::ControlPool,
    control_exclusions: &HashSet<String>,
    system_ctrl_names: &HashSet<String>,
    entry_by_name: &HashMap<String, crate::pool::ControlPoolEntry>,
) -> Result<()> {
    let mut ops = assignment.control_operators();
    let assigned = assignment.operator_names();
    let dimensions: [(
        fn(&crate::pool::ControlPoolEntry) -> bool,
        fn(&crate::scoring::TradeManuEfficiencyComponents) -> f64,
    ); 3] = [
        (control_entry_trade_inject, |value| value.trade_eff_pct),
        (control_entry_manu_inject, |value| value.gold_manu_eff_pct),
        (control_entry_manu_inject, |value| {
            value.battle_record_manu_eff_pct
        }),
    ];
    let evaluate = |candidate_ops: &[AssignedOperator]| -> Result<_> {
        let mut candidate = assignment.clone();
        candidate.set_room(RoomId::from("control"), candidate_ops.to_vec());
        let layout = resolve_base(
            blueprint,
            &candidate,
            Some(instances),
            Some(table),
            mood,
            durin_plan,
        )?
        .layout_snapshot();
        Ok(control_inject_components_for_assignment(
            &layout, blueprint, &candidate, instances,
        ))
    };
    for (class, component) in dimensions {
        let current = evaluate(&ops)?;
        if component(&current) > 0.0 {
            continue;
        }
        let current_covered = [
            current.trade_eff_pct,
            current.gold_manu_eff_pct,
            current.battle_record_manu_eff_pct,
        ]
        .into_iter()
        .filter(|value| *value > 0.0)
        .count();
        let mut best: Option<(Vec<AssignedOperator>, usize, f64, f64, String)> = None;
        for candidate in final_pool
            .entries
            .iter()
            .filter(|entry| class(entry) && !control_exclusions.contains(&entry.name))
            .filter(|entry| !entry_is_deferred_dynamic_producer(entry, table))
        {
            if assigned.contains(&candidate.name)
                || ops.iter().any(|operator| operator.name == candidate.name)
            {
                continue;
            }
            for (drop_idx, drop) in ops.iter().enumerate() {
                if control_replace_rank(drop, system_ctrl_names, entry_by_name) < 0 {
                    continue;
                }
                let mut candidate_ops = ops.clone();
                candidate_ops[drop_idx] =
                    AssignedOperator::from_progress(&candidate.name, candidate.progress);
                let values = evaluate(&candidate_ops)?;
                let target = component(&values);
                if target <= 0.0 {
                    continue;
                }
                let covered = [
                    values.trade_eff_pct,
                    values.gold_manu_eff_pct,
                    values.battle_record_manu_eff_pct,
                ]
                .into_iter()
                .filter(|value| *value > 0.0)
                .count();
                if covered < current_covered {
                    continue;
                }
                let policy = values.trade_eff_pct
                    + values.gold_manu_eff_pct
                    + values.battle_record_manu_eff_pct;
                let replace = best.as_ref().is_none_or(
                    |(_, best_covered, best_target, best_policy, best_name)| {
                        covered > *best_covered
                            || (covered == *best_covered && target > *best_target)
                            || (covered == *best_covered
                                && target == *best_target
                                && policy > *best_policy)
                            || (covered == *best_covered
                                && target == *best_target
                                && policy == *best_policy
                                && candidate.name < *best_name)
                    },
                );
                if replace {
                    best = Some((
                        candidate_ops,
                        covered,
                        target,
                        policy,
                        candidate.name.clone(),
                    ));
                }
            }
        }
        if let Some((candidate_ops, ..)) = best {
            ops = candidate_ops;
        }
    }
    assignment.set_room(RoomId::from("control"), ops);
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
/// - 宿舍共享；所有办公室成员按 αβγ cohort 上二休一，绑定成员随消费方体系同上同下。
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

pub fn schedule_timed_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    profile: TimedRotationProfile,
) -> Result<TeamRotationReport> {
    match profile {
        TimedRotationProfile::Abc12_6_6 => schedule_abc_rotation(
            blueprint,
            operbox,
            instances,
            table,
            options,
            [12.0, 6.0, 6.0],
            true,
            AbyssalShortShiftPolicy::Opportunistic,
        ),
        TimedRotationProfile::MainBackup12_12 => {
            schedule_main_backup_12_12(blueprint, operbox, instances, table, options)
        }
        TimedRotationProfile::Fiammetta8_8_4_4 => {
            schedule_fiammetta_8_8_4_4(blueprint, operbox, instances, table, options)
        }
        TimedRotationProfile::Abyssal7_5_7_5 => {
            schedule_abyssal_7_5_7_5(blueprint, operbox, instances, table, options)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbyssalShortShiftPolicy {
    Opportunistic,
    Disabled,
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
) -> Result<TeamRotationReport> {
    let t0 = Instant::now();
    blueprint.validate()?;

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let mut peak_options = options.clone();
    peak_options.shift_hours = shift_hours[0] + shift_hours[1].max(shift_hours[2]);
    let mut gamma_h1_options = options.clone();
    gamma_h1_options.shift_hours = shift_hours[1];
    let mut gamma_h2_options = options.clone();
    gamma_h2_options.shift_hours = shift_hours[2];

    // 1) 参考高峰班 + 编排计划 → 只取宿舍作为三班共享脚手架；办公室随后按 cohort 重排。
    // 中枢后续按 αβγ 队伍轮休重分配，不能从 peak 直接钉死。
    // 深海链因歌蕾蒂娅心情消耗 ~3.2/h（最长约 7h），不可进入 12h 主班；
    // 唯一入口是后续 S2 6h 短班的有/无深海双路径评分对比。
    let peak_result = assign_shift_with_plan_skip(
        blueprint,
        operbox,
        instances,
        table,
        &peak_options,
        AssignShiftMode::Peak,
        &BaseAssignment::default(),
        &HashSet::new(),
    )?;
    let peak = peak_result.assignment;
    let mut peak_plan = peak_result.plan;
    peak_plan.resolve_actual_producer_dependencies(blueprint, &peak, instances, table)?;
    let shift_binds = shift_binds_from_plan(&peak_plan);
    let bound_office_names = bound_office_operators(blueprint, &peak, &shift_binds);
    let rotating_office_names = peak_office_operators(blueprint, &peak);
    let rotating_anchor_names = bound_rotating_anchor_operators(blueprint, &peak, &shift_binds);
    let rotating_meeting_names = peak_meeting_operators(blueprint, &peak);
    let excluded_shared_names: HashSet<String> = rotating_office_names
        .iter()
        .chain(&rotating_anchor_names)
        .cloned()
        .collect();
    let mood_model = MoodModel::load_default()?;
    let peak_mood_eta = Some(shift_eta_with_instances(
        &mood_model,
        blueprint,
        &peak,
        instances,
    ));
    let mut shared = pinned_assignment_excluding(&peak, blueprint, &excluded_shared_names);
    pin_active_dependency_required(&mut shared, &peak, &peak_plan.active_dependencies)?;
    // 被 active dependency 钉进 shared 的会客室成员按全周期共享（required 语义），
    // 不得再进入 rotating meeting cohort，否则 resting 班会因 shared 保留而重复出现。
    let shared_names: HashSet<String> = operators_of(&shared).into_iter().collect();
    let rotating_meeting_names =
        rotating_meeting_names_excluding_shared(rotating_meeting_names, &shared_names);
    let mut scaffold_used: HashSet<String> = operators_of(&shared).into_iter().collect();
    scaffold_used.extend(peak_plan.excluded_operators.iter().cloned());
    let t1 = Instant::now();

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
        power: build_power_pool(&operbox.power_roster(instances), instances, table)?,
    };
    let control_pool = build_control_pool_with_fillers(operbox, instances, table)?;

    // 班次绑定（上2休1）来自统一 plan，不再硬编码具体体系。
    let [h1, h2] =
        split_production_facilities(blueprint, &peak, &shift_binds, &peak_plan.rotation_reserves)?;

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
    let mut used_ab = used.clone();
    used_ab.extend(bound_control_operators(blueprint, &peak, &shift_binds));

    let mut gamma_h1 = BaseAssignment::default();
    let mut used_g1 = used_ab.clone();
    let gamma_h1_context =
        gamma_production_coexist_seed(blueprint, &shared, &peak, &shift_binds, &h1, &beta);
    let mut gamma_h2 = BaseAssignment::default();
    let mut used_g2 = used_ab.clone();
    let gamma_h2_context =
        gamma_production_coexist_seed(blueprint, &shared, &peak, &shift_binds, &h2, &alpha);
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
    assign_gamma_half(
        blueprint,
        instances,
        &gamma_h1_context.assignment,
        Some(durin_plan),
        &pools,
        table,
        &gamma_h1_options,
        &h1,
        &mut gamma_h1,
        &mut used_g1,
    )?;
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

    assign_gamma_half(
        blueprint,
        instances,
        &gamma_h2_context.assignment,
        Some(durin_plan),
        &pools,
        table,
        &gamma_h2_options,
        &h2,
        &mut gamma_h2,
        &mut used_g2,
    )?;
    validate_rotation_reserves_before_postprocess(
        blueprint,
        &peak_plan.rotation_reserves,
        &h1,
        &gamma_h1,
        &h2,
        &gamma_h2,
    )?;
    let t3 = Instant::now();

    // 中枢每班独立分配：体系绑定干员跟随生产队同上同下，散件均分三队，
    // 每班仅允许活跃两队的中枢干员入池（保证每人休息一班）。
    let peak_ctrl: Vec<String> = peak
        .control_operators()
        .iter()
        .map(|o| o.name.clone())
        .collect();
    let mut system_ctrl_names = system_control_operators(&peak_plan);
    system_ctrl_names.extend(
        peak_plan
            .control_candidate_requirements
            .iter()
            .flat_map(|requirement| &requirement.candidates)
            .filter(|name| peak_ctrl.contains(name))
            .cloned(),
    );
    system_ctrl_names.extend(
        shift_binds
            .iter()
            .flat_map(|bind| &bind.operators)
            .filter(|name| peak_ctrl.contains(name))
            .cloned(),
    );
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
    let mut rotating_office_team = rotating_team_for_names(
        &shift_binds,
        &rotating_office_names,
        &production_team_by_name,
    );
    let rotating_anchor_team = rotating_team_for_names(
        &shift_binds,
        &rotating_anchor_names,
        &production_team_by_name,
    );
    let mut rotating_meeting_team = rotating_team_for_names(
        &shift_binds,
        &rotating_meeting_names,
        &production_team_by_name,
    );
    let entry_by_name: HashMap<String, crate::pool::ControlPoolEntry> = control_pool
        .entries
        .iter()
        .map(|entry| (entry.name.clone(), entry.clone()))
        .collect();
    let mut plugin_entries: Vec<_> = control_pool
        .entries
        .iter()
        .filter(|entry| !team_ctrl.values().any(|names| names.contains(&entry.name)))
        .filter(|entry| !production_team_by_name.contains_key(&entry.name))
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
    // 跨设施 shift bind 是最终归属约束，不能被中枢均衡策略再次拆开。
    for bind in &shift_binds {
        let Some(team) = bind
            .operators
            .iter()
            .find_map(|name| production_team_by_name.get(name).copied())
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
    let unavailable_for_control: HashSet<String> = production_team_by_name
        .keys()
        .cloned()
        .chain(scaffold_used.iter().cloned())
        .chain(rotating_office_names.iter().cloned())
        .chain(rotating_anchor_names.iter().cloned())
        .collect();
    let active_pair_has_five = |by_team: &HashMap<TeamLabel, Vec<String>>| {
        TeamLabel::ALL.iter().all(|resting| {
            let active: Vec<_> = TeamLabel::ALL
                .iter()
                .filter(|team| *team != resting)
                .copied()
                .collect();
            active_control_candidate_count(by_team, &active, &unavailable_for_control) >= 5
        })
    };
    let mut assigned_control: HashSet<String> = team_ctrl.values().flatten().cloned().collect();
    let mut skillless_fillers: Vec<_> = control_pool
        .entries
        .iter()
        .filter(|entry| entry.buff_ids.is_empty())
        .filter(|entry| !assigned_control.contains(&entry.name))
        .filter(|entry| !production_team_by_name.contains_key(&entry.name))
        .filter(|entry| !scaffold_used.contains(&entry.name))
        .filter(|entry| !rotating_office_names.contains(&entry.name))
        .filter(|entry| !rotating_anchor_names.contains(&entry.name))
        .collect();
    skillless_fillers.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in skillless_fillers {
        if active_pair_has_five(&team_ctrl) {
            break;
        }
        let team = least_available_control_team(&team_ctrl, &unavailable_for_control);
        team_ctrl.entry(team).or_default().push(entry.name.clone());
        assigned_control.insert(entry.name.clone());
    }
    if !active_pair_has_five(&team_ctrl) {
        return Err(Error::msg(
            "control rotation cannot fill all five seats for every active team pair",
        ));
    }
    let office_pool = office_candidates(
        operbox,
        instances,
        table,
        &layout,
        options.mood,
        &peak_plan.excluded_operators.iter().cloned().collect(),
    );
    let mut reserved: HashSet<String> = operators_of(&alpha)
        .into_iter()
        .chain(operators_of(&beta))
        .chain(operators_of(&gamma_h1))
        .chain(operators_of(&gamma_h2))
        .chain(team_ctrl.values().flatten().cloned())
        .chain(operators_of(&shared))
        .chain(rotating_office_names.iter().cloned())
        .chain(rotating_anchor_names.iter().cloned())
        .collect();
    complete_rotating_office_teams(
        blueprint,
        &rotating_office_names,
        &mut rotating_office_team,
        &office_pool,
        &mut reserved,
    )?;
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
    for (name, team) in rotating_office_team.iter().chain(&rotating_anchor_team) {
        match team {
            TeamLabel::Alpha => alpha_ops.push(name.clone()),
            TeamLabel::Beta => beta_ops.push(name.clone()),
            TeamLabel::Gamma => gamma_ops.push(name.clone()),
        }
    }
    for (name, team) in &rotating_meeting_team {
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
    //    S2 深海链双路径对比：预占猎手 → γ fill 绕开 → 补第3人 → 评分，与无深海基线对比，选优胜。
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

    let mut shifts = Vec::with_capacity(3);
    let mut control_options = options.clone();
    control_options.skip_standalone_control = true;
    let mut sui_mood: HashMap<String, f64> = SUI_MOOD_OPERATORS
        .into_iter()
        .map(|name| (name.to_string(), options.mood.min(mood_model.mood_cap)))
        .collect();
    let mut warmup_sticky_rooms: HashMap<String, RoomId> = HashMap::new();
    // 跨班次办公室轮换：同一普通办公室成员在完整循环中只上岗一次，避免连续上班超限。
    let mut used_office: HashSet<String> = HashSet::new();
    for (index, (hours, active, resting, parts)) in shift_specs.into_iter().enumerate() {
        // 活跃两队的中枢干员名册
        let active_names: HashSet<String> = active
            .iter()
            .flat_map(|t| team_ctrl.get(t).cloned().unwrap_or_default())
            .collect();
        control_options.mood = control_mood_for_shift(&sui_mood, &active_names, options.mood);
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
            let previous = a.control_operators();
            let mut required: Vec<String> = active_names
                .iter()
                .filter(|name| system_ctrl_names.contains(*name))
                .cloned()
                .collect();
            required.extend(extra_control_pins.iter().map(|name| (*name).to_string()));
            required.sort();
            required.dedup();
            let mut ops = Vec::new();
            for name in &required {
                let progress = operbox.progress_of(name).unwrap_or_default();
                ops.push(AssignedOperator::from_progress(name, progress));
            }
            if ops.len() > 5 {
                return Err(Error::msg(format!(
                    "active required control anchors exceed capacity: {:?}",
                    required
                )));
            }
            for op in previous {
                if ops.len() >= 5 {
                    break;
                }
                if !required.contains(&op.name) {
                    ops.push(op);
                }
            }
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
            Ok::<(), Error>(())
        };

        // 从活跃两队池分配中枢；不足五人是不可行排班，不能静默留下空席。
        let assign_ctrl = |a: &mut BaseAssignment,
                           used: &mut HashSet<String>,
                           extra_control_pins: &[&str]| {
            pin_active_system_control(a, extra_control_pins)?;
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
            assign_control(
                a,
                &final_pool,
                table,
                &layout,
                &control_options,
                &[],
                &peak_plan.control_exclusions.iter().cloned().collect(),
                used,
            )?;
            let mut protected_control = system_ctrl_names.clone();
            protected_control.extend(extra_control_pins.iter().map(|name| (*name).to_string()));
            ensure_control_inject_coverage(
                a,
                blueprint,
                instances,
                table,
                control_options.mood,
                Some(durin_plan),
                &final_pool,
                &peak_plan.control_exclusions.iter().cloned().collect(),
                &protected_control,
                &entry_by_name,
            )?;
            *used = a.operator_names();

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
                    if peak_plan.control_exclusions.contains(&entry.name) {
                        continue;
                    }
                    ops.push(AssignedOperator::from_progress(&entry.name, entry.progress));
                    names.insert(entry.name.clone());
                }
                a.set_room(RoomId::from("control"), ops);
                *used = a.operator_names();
            }
            if a.control_operators().len() != 5 {
                return Err(Error::msg(format!(
                    "control rotation filled {} of 5 seats",
                    a.control_operators().len()
                )));
            }
            Ok::<(), Error>(())
        };

        let mut assignment;
        if index == 1 && abyssal_policy == AbyssalShortShiftPolicy::Opportunistic {
            // ── S2 双路径对比 ──
            // 路径 A: 无深海（基线）
            let mut base = shared.clone();
            for part in parts {
                merge_rooms(&mut base, part);
            }
            assign_rotating_anchors(&mut base, &peak, &active, &rotating_anchor_team)?;
            assign_rotating_offices(
                &mut base,
                blueprint,
                &active,
                &rotating_office_team,
                &bound_office_names,
                &office_pool,
                &used_office,
            )?;
            assign_rotating_meetings(
                &mut base,
                blueprint,
                &active,
                resting,
                &rotating_meeting_team,
                &meeting_pool,
                &reserved,
            )?;
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
                assign_rotating_offices(
                    &mut candidate.assignment,
                    blueprint,
                    &active,
                    &rotating_office_team,
                    &bound_office_names,
                    &office_pool,
                    &used_office,
                )?;
                assign_rotating_meetings(
                    &mut candidate.assignment,
                    blueprint,
                    &active,
                    resting,
                    &rotating_meeting_team,
                    &meeting_pool,
                    &reserved,
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
                    if score_aby.manufacture_efficiency > score_base.manufacture_efficiency {
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
                            ops.push(candidate.control_producer.clone());
                            ops.sort();
                            ops.dedup();
                            ops.retain(|name| !alpha_beta.contains(name));
                            team.operators = ops;
                        }
                        (candidate.assignment, score_aby, candidate_warmup_rooms)
                    } else {
                        (base, score_base, base_warmup_rooms)
                    }
                } else {
                    (base, score_base, base_warmup_rooms)
                };
            assignment = best_assignment;
            let scores = best_scores;
            warmup_sticky_rooms = best_warmup_rooms;
            record_office_usage(
                blueprint,
                &assignment,
                &bound_office_names,
                &mut used_office,
            );
            let weighted_trade = scores.weighted_trade(hours);
            let weighted_manufacture = scores.weighted_manufacture(hours);
            let weighted_power = scores.weighted_power(hours);
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
            assign_rotating_anchors(&mut assignment, &peak, &active, &rotating_anchor_team)?;
            assign_rotating_offices(
                &mut assignment,
                blueprint,
                &active,
                &rotating_office_team,
                &bound_office_names,
                &office_pool,
                &used_office,
            )?;
            record_office_usage(
                blueprint,
                &assignment,
                &bound_office_names,
                &mut used_office,
            );
            assign_rotating_meetings(
                &mut assignment,
                blueprint,
                &active,
                resting,
                &rotating_meeting_team,
                &meeting_pool,
                &reserved,
            )?;
            clear_room(&mut assignment, "control");
            let mut s13_used = assignment.operator_names();
            assign_ctrl(&mut assignment, &mut s13_used, &[])?;
            align_warmup_rooms(
                blueprint,
                instances,
                &mut assignment,
                &mut warmup_sticky_rooms,
            );
            clear_production_efficiencies(blueprint, &mut assignment);
            let scores = evaluate_base_assignment_efficiencies(
                blueprint,
                &assignment,
                instances,
                table,
                hours,
                Some(durin_plan),
            )?;
            let weighted_trade = scores.weighted_trade(hours);
            let weighted_manufacture = scores.weighted_manufacture(hours);
            let weighted_power = scores.weighted_power(hours);
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
        let resting_names: HashSet<String> = teams
            .iter()
            .find(|team| team.label == resting)
            .map(|team| team.operators.iter().cloned().collect())
            .unwrap_or_default();
        let assignment = &shifts.last().expect("shift just appended").assignment;
        advance_sui_mood(
            &mood_model,
            blueprint,
            assignment,
            &resting_names,
            hours,
            &mut sui_mood,
        );
    }

    if enable_fiammetta_return {
        apply_fiammetta_return(
            blueprint,
            operbox,
            instances,
            table,
            durin_plan,
            &peak,
            &teams,
            &peak_plan.continuous_roles,
            &shift_binds,
            &mut shifts,
        )?;
    }
    validate_final_rotation_invariants(&shifts, &shift_binds, &peak_plan.active_dependencies)?;
    validate_mood_gap_decay_work_spans(
        blueprint,
        instances,
        table,
        TimedRotationProfile::Abc12_6_6,
        &shifts,
    )?;
    let daily = DailyTotals {
        trade: shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: shifts.iter().map(|shift| shift.weighted_manufacture).sum(),
        power: shifts.iter().map(|shift| shift.weighted_power).sum(),
    };
    let t4 = Instant::now();

    fn ms(a: Instant, b: Instant) -> f64 {
        a.duration_since(b).as_secs_f64() * 1000.0
    }

    eprintln!(
        "[计时] 轮换·peak班={:.2}ms  resolve+建池+切半={:.2}ms  γ替补={:.2}ms  三班评分={:.2}ms  轮换总计={:.2}ms",
        ms(t1, t0), ms(t2, t1), ms(t3, t2), ms(t4, t3), ms(t4, t0),
    );

    Ok(TeamRotationReport {
        profile: TimedRotationProfile::Abc12_6_6,
        assignment_plans: vec![peak_plan.clone()],
        peak_plan,
        peak_mood_eta,
        teams,
        shifts,
        daily,
        elapsed: t4.duration_since(t0),
    })
}

#[derive(Debug, Clone)]
struct OwnedRotationAssignment {
    assignment: BaseAssignment,
    plan: AssignmentPlan,
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
    // Initial two-team policy: no operator assigned anywhere in the main state
    // may be selected by the backup solver. This is a policy restriction, not
    // a completeness-preserving reduction of all possible two-shift plans.
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
            "no solution under complete-state-disjoint backup policy: {error}"
        ))
    })
}

fn schedule_main_backup_12_12(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let started = Instant::now();
    let main = solve_owned_peak_assignment(blueprint, operbox, instances, table, options, 12.0)?;
    let backup = solve_policy_disjoint_backup(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &main.assignment,
        12.0,
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
            duration_hours: 12.0,
            active_teams: vec![TeamLabel::Alpha],
            resting_team: TeamLabel::Beta,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
        RotationStateSpec {
            owner_index: 1,
            duration_hours: 12.0,
            active_teams: vec![TeamLabel::Beta],
            resting_team: TeamLabel::Alpha,
            fiammetta: None,
            dorm_rest: None,
            transition: ShiftTransition::ReplaceRooms,
        },
    ];
    finalize_profile_rotation(
        TimedRotationProfile::MainBackup12_12,
        blueprint,
        operbox,
        instances,
        table,
        vec![main, backup],
        teams,
        states,
        started,
    )
}

fn schedule_fiammetta_8_8_4_4(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    if !operbox.owns("菲亚梅塔") {
        return Err(Error::msg(
            "fiammetta-8844 requires an owned Fiammetta operator",
        ));
    }
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
    )?;
    let binds = shift_binds_from_plan(&raw.peak_plan);
    let (target, room_id) = first_fiammetta_target(
        blueprint,
        &raw.shifts[0].assignment,
        &raw.peak_plan.continuous_roles,
        &binds,
    )
    .ok_or_else(|| {
        Error::msg(
            "fiammetta-8844 requires an eligible peak production target under the configured priority",
        )
    })?;

    let owners: Vec<_> = raw
        .shifts
        .iter()
        .map(|shift| OwnedRotationAssignment {
            assignment: shift.assignment.clone(),
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
            fiammetta: Some(FiammettaShiftAction {
                target,
                displaced: String::new(),
                room_id,
            }),
            dorm_rest: None,
            transition: ShiftTransition::FiammettaOnly,
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
    finalize_profile_rotation(
        TimedRotationProfile::Fiammetta8_8_4_4,
        blueprint,
        operbox,
        instances,
        table,
        owners,
        raw.teams,
        states,
        started,
    )
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
) -> Result<OwnedRotationAssignment> {
    let empty = HashSet::new();
    let producer = abyssal_control_producer(operbox, instances, &empty).ok_or_else(|| {
        Error::msg("abyssal-7575 requires a control producer with the Abyssal capability")
    })?;
    let hunters = owned_abyssal_hunters(operbox, instances, &empty);
    if hunters.len() < 4 {
        return Err(Error::msg(
            "abyssal-7575 requires at least four tag-matched Abyssal factory consumers",
        ));
    }
    let control_room = blueprint
        .rooms
        .iter()
        .find(|room| room.kind == FacilityKind::ControlCenter)
        .ok_or_else(|| Error::msg("abyssal-7575 requires a control center"))?;
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
        return Err(Error::msg(
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
    best.map(|(owner, _, _)| owner).ok_or_else(|| {
        Error::msg(
            "abyssal-7575 hard claims could not be combined with a complete legal assignment",
        )
    })
}

fn plan_abyssal_dorm_rest(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    main: &BaseAssignment,
    backup: &BaseAssignment,
) -> Result<DormRestPlan> {
    let model = MoodModel::load_default()?;
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
        .rooms
        .iter()
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
                if existing.len() + 3 > dorm.dorm_beds.unwrap_or(5).max(1) as usize {
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
    best.map(|(plan, _, _)| plan).ok_or_else(|| {
        Error::msg(
            "abyssal-7575 requires one single-target and one group dorm manager who can fully recover the control producer during each 5h rest",
        )
    })
}

fn schedule_abyssal_7_5_7_5(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport> {
    let started = Instant::now();
    let main = solve_abyssal_profile_main(blueprint, operbox, instances, table, options)?;
    let backup = solve_policy_disjoint_backup(
        blueprint,
        operbox,
        instances,
        table,
        options,
        &main.assignment,
        5.0,
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
            dorm_rest: Some(dorm_rest.clone()),
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
            dorm_rest: Some(dorm_rest),
            transition: ShiftTransition::ReplaceRooms,
        },
    ];
    finalize_profile_rotation(
        TimedRotationProfile::Abyssal7_5_7_5,
        blueprint,
        operbox,
        instances,
        table,
        vec![main, backup],
        teams,
        states,
        started,
    )
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
        trade_efficiency: continuation_efficiency(total.trade_efficiency, first.trade_efficiency),
        trade_skill_efficiency: continuation_efficiency(
            total.trade_skill_efficiency,
            first.trade_skill_efficiency,
        ),
        trade_display_efficiency: continuation_efficiency(
            total.trade_display_efficiency,
            first.trade_display_efficiency,
        ),
        manufacture_efficiency: continuation_efficiency(
            total.manufacture_efficiency,
            first.manufacture_efficiency,
        ),
        manufacture_skill_efficiency: continuation_efficiency(
            total.manufacture_skill_efficiency,
            first.manufacture_skill_efficiency,
        ),
        manufacture_display_efficiency: continuation_efficiency(
            total.manufacture_display_efficiency,
            first.manufacture_display_efficiency,
        ),
        power_efficiency: continuation_efficiency(total.power_efficiency, first.power_efficiency),
        power_skill_efficiency: continuation_efficiency(
            total.power_skill_efficiency,
            first.power_skill_efficiency,
        ),
        power_display_efficiency: continuation_efficiency(
            total.power_display_efficiency,
            first.power_display_efficiency,
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
        power_efficiency: room_lines.iter().map(|line| line.power_efficiency).sum(),
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
    let durin_plan = operbox.durin_dorm_planning_count(instances);
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
            fiammetta: state.fiammetta.clone(),
            dorm_rest: state.dorm_rest.clone(),
            drone: drone_action(blueprint, state.transition),
            transition: state.transition,
            weighted_trade: scores.weighted_trade(state.duration_hours),
            weighted_manufacture: scores.weighted_manufacture(state.duration_hours),
            weighted_power: scores.weighted_power(state.duration_hours),
            efficiencies: scores,
        });
    }
    let daily = DailyTotals {
        trade: shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: shifts.iter().map(|shift| shift.weighted_manufacture).sum(),
        power: shifts.iter().map(|shift| shift.weighted_power).sum(),
    };
    let assignment_plans: Vec<_> = owners.iter().map(|owner| owner.plan.clone()).collect();
    let peak_plan = assignment_plans[0].clone();
    let peak_mood_eta = Some(shift_eta_with_instances(
        &MoodModel::load_default()?,
        blueprint,
        &owners[0].assignment,
        instances,
    ));
    let report = TeamRotationReport {
        profile,
        peak_plan,
        assignment_plans,
        peak_mood_eta,
        teams,
        shifts,
        daily,
        elapsed: started.elapsed(),
    };
    validate_profile_rotation(blueprint, instances, &report)?;
    validate_continuous_work_spans(blueprint, instances, &report)?;
    validate_mood_gap_decay_work_spans(
        blueprint,
        instances,
        table,
        report.profile,
        &report.shifts,
    )?;
    Ok(report)
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
        .flat_map(|shift| &shift.assignment.rooms)
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
        shift.assignment.rooms.iter().any(|room| {
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
        for room in &shift.assignment.rooms {
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
                if actual != room.operator_capacity() {
                    return Err(Error::msg(format!(
                        "shift {} room {} has {actual} operators, expected {}",
                        shift.index + 1,
                        room.id.0,
                        room.operator_capacity()
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

    if report.profile.is_two_team() {
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
        if !alpha.is_disjoint(&beta) {
            return Err(Error::msg(
                "two-team rotation violates complete-state-disjoint policy",
            ));
        }
    }

    let mut binds = Vec::new();
    for plan in &report.assignment_plans {
        binds.extend(shift_binds_from_plan(plan));
    }
    for bind in binds {
        let Some(team) = report.teams.iter().find(|team| {
            bind.operators
                .iter()
                .all(|operator| team.operators.contains(operator))
        }) else {
            continue;
        };
        let expected: Vec<_> = report
            .shifts
            .iter()
            .map(|shift| shift.active_teams.contains(&team.label))
            .collect();
        for operator in &bind.operators {
            let actual: Vec<_> = report
                .shifts
                .iter()
                .map(|shift| shift.assignment.operator_names().contains(operator))
                .collect();
            if actual != expected {
                return Err(Error::msg(format!(
                    "profile bind {} presence {actual:?} does not match team {:?} topology {expected:?}",
                    operator, team.label
                )));
            }
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
            .rooms
            .iter()
            .filter(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|room| room.kind == FacilityKind::Factory)
            })
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
                .rooms
                .iter()
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

fn validate_continuous_work_spans(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
) -> Result<()> {
    let model = MoodModel::load_default()?;
    let eta_by_shift: Vec<_> = report
        .shifts
        .iter()
        .map(|shift| shift_eta_with_instances(&model, blueprint, &shift.assignment, instances))
        .collect();
    let names: HashSet<_> = eta_by_shift
        .iter()
        .flat_map(|eta| eta.per_op.iter().map(|operator| operator.name.clone()))
        .collect();
    let state_count = report.shifts.len();
    for name in names {
        for start in 0..state_count {
            let mut consumed = 0.0;
            for offset in 0..state_count {
                let index = (start + offset) % state_count;
                let shift = &report.shifts[index];
                if shift
                    .fiammetta
                    .as_ref()
                    .is_some_and(|action| action.target == name)
                {
                    consumed = 0.0;
                }
                let Some(operator) = eta_by_shift[index]
                    .per_op
                    .iter()
                    .find(|operator| operator.name == name)
                else {
                    // Current timed-rotation scope validates continuous work from a
                    // full rest boundary; it does not claim multi-cycle dorm optimum.
                    consumed = 0.0;
                    continue;
                };
                consumed += operator.drain_per_hour * shift.duration_hours;
                if consumed + operator.drain_per_hour * HALF_HOUR_SAFETY_MARGIN
                    > model.workable_mood() + 1e-9
                {
                    return Err(Error::msg(format!(
                        "{} is infeasible for {}: continuous mood use {:.2} exceeds {:.2} with 0.5h margin",
                        name,
                        report.profile.display_name(),
                        consumed,
                        model.workable_mood()
                    )));
                }
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
    fn rotating_office_uses_bound_member_then_replacement() {
        let blueprint: BaseBlueprint =
            serde_json::from_str(r#"{"rooms":[{"id":"office_any","kind":"office","level":3}]}"#)
                .expect("blueprint");
        let candidates = vec![
            AssignedOperator::new("体系办公室", 2),
            AssignedOperator::new("办公室替补", 2),
        ];
        let teams = HashMap::from([
            ("体系办公室".to_string(), TeamLabel::Alpha),
            ("办公室替补".to_string(), TeamLabel::Beta),
        ]);
        let bound = HashSet::from(["体系办公室".to_string()]);

        let mut active = BaseAssignment::default();
        assign_rotating_offices(
            &mut active,
            &blueprint,
            &[TeamLabel::Alpha, TeamLabel::Beta],
            &teams,
            &bound,
            &candidates,
            &HashSet::new(),
        )
        .expect("active office");
        assert_eq!(
            active.operators_in(&RoomId::from("office_any"))[0].name,
            "体系办公室"
        );

        let mut resting = BaseAssignment::default();
        assign_rotating_offices(
            &mut resting,
            &blueprint,
            &[TeamLabel::Beta, TeamLabel::Gamma],
            &teams,
            &bound,
            &candidates,
            &HashSet::new(),
        )
        .expect("replacement office");
        assert_eq!(
            resting.operators_in(&RoomId::from("office_any"))[0].name,
            "办公室替补"
        );
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

    #[test]
    fn control_coverage_does_not_count_production_or_shared_members() {
        let by_team = HashMap::from([
            (
                TeamLabel::Alpha,
                vec![
                    "可用甲".to_string(),
                    "生产占用".to_string(),
                    "共享占用".to_string(),
                ],
            ),
            (
                TeamLabel::Beta,
                vec!["可用乙".to_string(), "绑定办公室".to_string()],
            ),
        ]);
        let unavailable = HashSet::from([
            "生产占用".to_string(),
            "共享占用".to_string(),
            "绑定办公室".to_string(),
        ]);

        assert_eq!(
            active_control_candidate_count(
                &by_team,
                &[TeamLabel::Alpha, TeamLabel::Beta],
                &unavailable,
            ),
            2
        );
    }

    #[test]
    fn control_filler_targets_least_available_not_shortest_raw_roster() {
        let by_team = HashMap::from([
            (
                TeamLabel::Alpha,
                vec![
                    "占用甲".to_string(),
                    "占用乙".to_string(),
                    "占用丙".to_string(),
                ],
            ),
            (TeamLabel::Beta, vec!["可用乙".to_string()]),
            (TeamLabel::Gamma, vec!["可用丙".to_string()]),
        ]);
        let unavailable = HashSet::from([
            "占用甲".to_string(),
            "占用乙".to_string(),
            "占用丙".to_string(),
        ]);

        assert_eq!(
            least_available_control_team(&by_team, &unavailable),
            TeamLabel::Alpha,
            "A raw roster 最长但真实可用为0，filler必须优先投A"
        );
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
    fn timed_rotation_profile_parser_is_closed() {
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
                let room = shift
                    .assignment
                    .rooms
                    .iter()
                    .find(|room| room.operators.iter().any(|op| op.name == *name));
                if !active {
                    assert!(room.is_none(), "休息班不应安排红松核心: {name}");
                    continue;
                }
                let room = room.unwrap_or_else(|| panic!("工作班缺少红松核心: {name}"));
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
        let peak_names = report.shifts[0].assignment.operator_names();
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
            for room in &shift.assignment.rooms {
                if blueprint.room(&room.room_id).is_some_and(|bp| {
                    matches!(bp.kind, FacilityKind::Dormitory | FacilityKind::Office)
                }) {
                    continue;
                }
                for op in &room.operators {
                    if shared_dependency_names.contains(&op.name) {
                        continue;
                    }
                    let team = team_by_name
                        .get(&op.name)
                        .copied()
                        .unwrap_or_else(|| panic!("上岗干员 {} 缺少 α/β/γ 归属", op.name));
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
    fn team_rotation_fiammetta_returns_peak_core_and_rests_replacement() {
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
        assert_eq!(action.target, "但书");
        let peak_anchor = report
            .peak_plan
            .anchors
            .iter()
            .find(|anchor| anchor.operator == action.target)
            .expect("但书必须是 peak required anchor");
        assert_eq!(peak_anchor.room_id.as_ref(), Some(&action.room_id));
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
        assert!(
            !shift
                .assignment
                .operator_names()
                .contains(&action.displaced),
            "被替换者必须离开当班 assignment"
        );
        assert_eq!(
            crate::schedule::team_of_operator(&report, &action.target),
            Some(shift.resting_team),
            "菲亚回岗应是休息队主力的显式例外"
        );
        for shift in &report.shifts {
            let names: Vec<_> = shift
                .assignment
                .rooms
                .iter()
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
    fn team_rotation_pure_fireworks_rotates_office_trade_and_control_together() {
        use crate::schedule::shift_bind::team_of_operator;

        let (blueprint, full_operbox, instances, table) = fixtures();
        let operbox = OperBox::from_entries(
            full_operbox
                .entries
                .into_iter()
                .filter(|entry| entry.name != "迷迭香")
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
            shift.assignment.rooms.iter().any(|room| {
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
                    .rooms
                    .iter()
                    .filter(|room| {
                        blueprint
                            .room(&room.room_id)
                            .is_some_and(|bp| bp.kind == FacilityKind::Office)
                    })
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
    fn team_rotation_rotates_office_members_across_cohorts() {
        use crate::schedule::shift_bind::team_of_operator;

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

        let office_names: Vec<_> = report
            .shifts
            .iter()
            .flat_map(|shift| {
                shift
                    .assignment
                    .rooms
                    .iter()
                    .filter(|room| {
                        blueprint
                            .room(&room.room_id)
                            .is_some_and(|bp| bp.kind == FacilityKind::Office)
                    })
                    .flat_map(|room| room.operators.iter().map(|operator| operator.name.clone()))
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        assert_eq!(office_names.len(), 3, "单人办公室三班应准备三个轮换成员");
        for shift in &report.shifts {
            let office_operator = shift
                .assignment
                .rooms
                .iter()
                .filter(|room| {
                    blueprint
                        .room(&room.room_id)
                        .is_some_and(|bp| bp.kind == FacilityKind::Office)
                })
                .flat_map(|room| room.operators.iter())
                .next()
                .expect("每个班次办公室必须满编");
            let team = team_of_operator(&report, &office_operator.name)
                .expect("办公室轮换成员必须归属一个 cohort");
            assert!(
                shift.active_teams.contains(&team),
                "办公室成员 {} 所属 {:?} 在休息班仍上岗",
                office_operator.name,
                team
            );
        }
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
            shift.assignment.rooms.iter().any(|room| {
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
            if shift.assignment.rooms.iter().any(|room| {
                room.operators
                    .iter()
                    .any(|op| manufacturers.contains(&op.name.as_str()))
            }) {
                let occupied_rooms = shift
                    .assignment
                    .rooms
                    .iter()
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
                        .rooms
                        .iter()
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
                .rooms
                .iter()
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
                    .rooms
                    .iter()
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
    fn team_rotation_does_not_invent_karlan_control_producer() {
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

        let peak_has_karlan_consumer = report.peak_plan.shift_binds.iter().any(|bind| {
            ["灵知", "孑", "银灰"]
                .iter()
                .all(|name| bind.operators.iter().any(|op| op == name))
        });
        assert!(
            !peak_has_karlan_consumer,
            "本 fixture 的 peak 未形成喀兰消费链"
        );
        assert!(
            report.shifts.iter().all(|shift| !shift
                .assignment
                .control_operators()
                .iter()
                .any(|op| op.name == "灵知")),
            "未形成实际消费链时不得仅因 operbox 持有灵知而强塞中枢"
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
        assert_eq!(active_layout.global_inject.trade_eff_pct(), 5.0);

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
        // 八幡海铃注入 = 5 × 全贸易站叙拉古人数（伺夜 + 贝洛内 = 2）→ 10%。
        assert_eq!(
            layout.global_inject.trade_eff_pct(),
            10.0,
            "未绑定 consumer 的收益按实际在场结算（D6）"
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

        let error = validate_final_rotation_invariants(&[shift], &[], &[]).unwrap_err();
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
            &shifts,
            &[],
            std::slice::from_ref(&dependency),
        )
        .is_ok());
        shifts[0]
            .assignment
            .rooms
            .retain(|room| room.room_id.0 != "meeting");
        assert!(validate_final_rotation_invariants(
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
    fn continuous_trade_core_uses_plan_for_fiammetta_return() {
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
            .all(|shift| crate::schedule::operator_in_shift(&with_fiammetta, shift.index, "但书")));
        assert!(with_fiammetta.shifts.iter().any(|shift| shift
            .fiammetta
            .as_ref()
            .is_some_and(|action| action.target == "但书")));

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
