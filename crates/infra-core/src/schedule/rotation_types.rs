use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::layout::{AssignmentPlan, BaseAssignment, RoomId};
use crate::mood::ShiftEta;

/// 单房间直接效率快照（用于 CLI 逐房展示）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RoomEfficiencyLine {
    pub room_id: String,
    pub operator_count: usize,
    /// 干员技能提供的直接效率；发电站包含技能驱动的爬升增量。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub operator_efficiency: Efficiency,
    /// 基础效率：100% + 非 0 心情入驻人数 × 1%。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub base_efficiency: Efficiency,
    /// 兼容字段：进驻增量与全局加成的合计，不参与总效率再次求和。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub station_efficiency: Efficiency,
    /// 中枢/全局效率；贸易与制造统一使用该分量。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub global_efficiency: Efficiency,
    /// Paper total before an order-specific multiplier is applied.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub total_efficiency: Efficiency,
    /// Legacy compatibility field; current direct efficiency is `total_efficiency`.
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    #[serde(skip_serializing)]
    pub final_efficiency: Efficiency,
    /// Trade order multiplier; 1.000 for non-trade facilities.
    #[serde(default)]
    pub order_multiplier: Efficiency,
    /// 等效效率：从总效率扣除设施基准与兼容分量，仅供解释。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub equivalent_efficiency: Efficiency,
    /// 贸易/赤金分别计算的单位时间产出。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    #[serde(skip_serializing)]
    pub gold_consumption_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    #[serde(skip_serializing)]
    pub originium_shards_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manufacture_unit_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manufacture_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_equivalent_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub gold_equivalent_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub combined_output_value: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub power_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub power_skill_efficiency: Efficiency,
}

fn is_zero_efficiency(v: &Efficiency) -> bool {
    v.is_zero()
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShiftEfficiencies {
    pub trade_efficiency: Efficiency,
    pub manufacture_efficiency: Efficiency,
    pub power_efficiency: Efficiency,
    /// 各房间效率明细（按 SHIFT_STATION_ORDER 顺序）。
    pub room_lines: Vec<RoomEfficiencyLine>,
}

impl ShiftEfficiencies {
    /// 贸易效率按时长折算（不与制造/发电混合量纲）。
    pub fn weighted_trade(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.trade_efficiency, shift_hours)
    }
    /// 制造效率按时长折算。
    pub fn weighted_manufacture(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.manufacture_efficiency, shift_hours)
    }
    /// 发电效率按时长折算。
    pub fn weighted_power(&self, shift_hours: f64) -> Efficiency {
        weighted_for_hours(self.power_efficiency, shift_hours)
    }

    pub fn weighted_trade_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.trade_efficiency, shift_hours, cycle_hours)
    }

    pub fn weighted_manufacture_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.manufacture_efficiency, shift_hours, cycle_hours)
    }

    pub fn weighted_power_for_cycle(&self, shift_hours: f64, cycle_hours: f64) -> Efficiency {
        weighted_for_cycle(self.power_efficiency, shift_hours, cycle_hours)
    }
}

fn weighted_for_hours(efficiency: Efficiency, hours: f64) -> Efficiency {
    weighted_for_cycle(efficiency, hours, 24.0)
}

fn weighted_for_cycle(efficiency: Efficiency, hours: f64, cycle_hours: f64) -> Efficiency {
    let minutes = (hours * 60.0).round() as i64;
    let cycle_minutes = (cycle_hours * 60.0).round() as i64;
    efficiency.scale_ratio(minutes, cycle_minutes)
}

/// 一组须同队、同休的轮换干员（运行期，来自统一 plan）。
#[derive(Debug, Clone)]
pub struct RuntimeShiftBind {
    pub operators: Vec<String>,
    /// αβγ 周期内上岗班次数（当前固定 12h+6h+6h → 2）。
    pub on_shifts: u8,
    pub off_shifts: u8,
}

/// Supported timed-rotation profiles plus a hidden legacy wire tombstone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimedRotationProfile {
    /// Backward-compatible αβγ rotation.
    #[default]
    #[serde(rename = "abc_12_6_6")]
    Abc12_6_6,
    /// ABC rotation with a 36-hour cycle of three 12-hour shifts.
    #[serde(rename = "abc_12_12_12")]
    Abc12_12_12,
    /// Two complete, disjoint main/backup states.
    #[serde(rename = "main_backup_12_12")]
    MainBackup12_12,
    /// Two complete, disjoint 24-hour main/backup states over a 48-hour cycle.
    #[serde(rename = "main_backup_24_24")]
    MainBackup24_24,
    /// An ordinary two-team topology whose ordered durations are supplied separately.
    #[serde(rename = "generic_main_backup")]
    GenericMainBackup,
    /// An ordinary αβγ topology whose ordered durations are supplied separately.
    #[serde(rename = "generic_abc")]
    GenericAbc,
    /// Legacy wire token retained only so older requests can be explicitly rejected.
    #[doc(hidden)]
    #[serde(rename = "fiammetta_witch_12_12")]
    FiammettaWitch12_12,
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
            Self::Abc12_12_12 => "abc-121212",
            Self::MainBackup12_12 => "2",
            Self::MainBackup24_24 => "main-backup-2424",
            Self::GenericMainBackup => "main-backup-custom",
            Self::GenericAbc => "abc-custom",
            Self::FiammettaWitch12_12 => "fiammetta-witch-1212 (removed)",
            Self::Fiammetta8_8_4_4 => "fiammetta-8844",
            Self::Abyssal7_5_7_5 => "abyssal-7575",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Abc12_6_6 => "ABC 三班 12/6/6",
            Self::Abc12_12_12 => "ABC 三班 12/12/12",
            Self::MainBackup12_12 => "二班主替 12/12",
            Self::MainBackup24_24 => "二班主替 24/24",
            Self::GenericMainBackup => "二班主替自定义时长",
            Self::GenericAbc => "ABC 三班自定义时长",
            Self::FiammettaWitch12_12 => "已删除的龙巫二班",
            Self::Fiammetta8_8_4_4 => "菲亚插班 8/8/4/4",
            Self::Abyssal7_5_7_5 => "深海四班 7/5/7/5",
        }
    }

    pub const fn plan_times(self) -> &'static str {
        match self {
            Self::Abc12_6_6 => "3班",
            Self::Abc12_12_12 => "3班",
            Self::MainBackup12_12 | Self::MainBackup24_24 => "2班",
            Self::GenericMainBackup => "2班",
            Self::GenericAbc => "3班",
            Self::FiammettaWitch12_12 => "不支持",
            Self::Fiammetta8_8_4_4 | Self::Abyssal7_5_7_5 => "4班",
        }
    }

    pub const fn maa_uses_rest_team_dorm_autofill(self) -> bool {
        matches!(
            self,
            Self::MainBackup12_12 | Self::MainBackup24_24 | Self::GenericMainBackup
        )
    }

    pub const fn is_ordinary_abc(self) -> bool {
        matches!(self, Self::Abc12_6_6 | Self::Abc12_12_12 | Self::GenericAbc)
    }

    pub const fn is_ordinary_main_backup(self) -> bool {
        matches!(
            self,
            Self::MainBackup12_12 | Self::MainBackup24_24 | Self::GenericMainBackup
        )
    }

    pub const fn is_two_team(self) -> bool {
        matches!(
            self,
            Self::MainBackup12_12
                | Self::MainBackup24_24
                | Self::GenericMainBackup
                | Self::Abyssal7_5_7_5
        )
    }
}

/// Explicit timing input for the ordinary two-team or αβγ rotation topologies.
/// Legacy named profiles remain plain strings on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimedRotationRequest {
    Named(TimedRotationProfile),
    Generic(GenericRotationTiming),
}

/// Ordered durations for an ordinary rotation topology.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "topology", rename_all = "snake_case")]
pub enum GenericRotationTiming {
    MainBackup { shift_hours: [f64; 2] },
    Abc { shift_hours: [f64; 3] },
}

impl Default for TimedRotationRequest {
    fn default() -> Self {
        Self::Named(TimedRotationProfile::default())
    }
}

impl PartialEq<TimedRotationProfile> for TimedRotationRequest {
    fn eq(&self, other: &TimedRotationProfile) -> bool {
        matches!(self, Self::Named(profile) if profile == other)
    }
}

impl From<TimedRotationProfile> for TimedRotationRequest {
    fn from(profile: TimedRotationProfile) -> Self {
        Self::Named(profile)
    }
}

impl TimedRotationRequest {
    pub fn profile(&self) -> TimedRotationProfile {
        match self {
            Self::Named(profile) => *profile,
            Self::Generic(GenericRotationTiming::MainBackup { .. }) => {
                TimedRotationProfile::GenericMainBackup
            }
            Self::Generic(GenericRotationTiming::Abc { .. }) => TimedRotationProfile::GenericAbc,
        }
    }

    pub fn generic_timing(&self) -> Option<&GenericRotationTiming> {
        match self {
            Self::Named(_) => None,
            Self::Generic(timing) => Some(timing),
        }
    }

    /// Preserve the established profile strategy for named sequences supplied
    /// through the generic syntax; other duration sequences stay generic.
    pub fn preserve_named_timing(self) -> Self {
        match self {
            Self::Generic(GenericRotationTiming::MainBackup { shift_hours })
                if shift_hours == [12.0, 12.0] =>
            {
                Self::Named(TimedRotationProfile::MainBackup12_12)
            }
            Self::Generic(GenericRotationTiming::MainBackup { shift_hours })
                if shift_hours == [24.0, 24.0] =>
            {
                Self::Named(TimedRotationProfile::MainBackup24_24)
            }
            Self::Generic(GenericRotationTiming::Abc { shift_hours })
                if shift_hours == [12.0, 6.0, 6.0] =>
            {
                Self::Named(TimedRotationProfile::Abc12_6_6)
            }
            Self::Generic(GenericRotationTiming::Abc { shift_hours })
                if shift_hours == [12.0, 12.0, 12.0] =>
            {
                Self::Named(TimedRotationProfile::Abc12_12_12)
            }
            request => request,
        }
    }
}

impl std::str::FromStr for TimedRotationRequest {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let Some((topology, durations)) = value.trim().split_once('@') else {
            return value.parse::<TimedRotationProfile>().map(Self::Named);
        };
        let parsed: Vec<f64> = durations
            .split('/')
            .map(|part| {
                part.trim()
                    .parse::<f64>()
                    .map_err(|_| Error::msg(format!("invalid rotation duration {part:?}")))
            })
            .collect::<Result<_>>()?;
        let request = match topology.trim().to_ascii_lowercase().as_str() {
            "main-backup" | "main_backup" => {
                let shift_hours: [f64; 2] = parsed.try_into().map_err(|durations: Vec<f64>| {
                    Error::msg(format!(
                        "main-backup rotation requires 2 ordered durations, got {}",
                        durations.len()
                    ))
                })?;
                Self::Generic(GenericRotationTiming::MainBackup { shift_hours })
            }
            "abc" => {
                let shift_hours: [f64; 3] = parsed.try_into().map_err(|durations: Vec<f64>| {
                    Error::msg(format!(
                        "ABC rotation requires 3 ordered durations, got {}",
                        durations.len()
                    ))
                })?;
                Self::Generic(GenericRotationTiming::Abc { shift_hours })
            }
            _ => {
                return Err(Error::msg(format!(
                    "unsupported rotation topology {topology:?}; expected main-backup or abc"
                )))
            }
        };
        Ok(request.preserve_named_timing())
    }
}

impl std::str::FromStr for TimedRotationProfile {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "3" | "abc" | "12/6/6" | "abc-1266" => Ok(Self::Abc12_6_6),
            "abc-121212" | "12/12/12" | "abc-12-12-12" => Ok(Self::Abc12_12_12),
            "2" | "12/12" | "two-shift" | "main-backup" => Ok(Self::MainBackup12_12),
            "main_backup_24_24" | "main-backup-2424" | "24/24" | "24-24" => {
                Ok(Self::MainBackup24_24)
            }
            "fiammetta-witch-1212" | "witch-1212" | "fiammetta_witch_12_12" => Err(
                Error::msg(
                    "unsupported rotation profile fiammetta_witch_12_12: this product mode was removed; no fallback is performed",
                ),
            ),
            "fiammetta-8844" | "8/8/4/4" => Ok(Self::Fiammetta8_8_4_4),
            "abyssal-7575" | "7/5/7/5" => Ok(Self::Abyssal7_5_7_5),
            "4" => Err(Error::msg(
                "rotation 4 requires a named profile: fiammetta-8844 or abyssal-7575",
            )),
            other => Err(Error::msg(format!(
                "unsupported rotation {other:?}; expected 2, 3, 12/12/12, 24/24, fiammetta-8844, or abyssal-7575"
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

/// 菲亚梅塔在某个班次边界执行的一次心情交换。
#[derive(Debug, Clone, Serialize)]
pub struct FiammettaShiftAction {
    /// 获得换心情，并由下一班正常搜索重新选中的干员。
    pub target: String,
    /// 旧导出兼容字段。菲亚不再直接替换当前岗位，因此为空。
    pub displaced: String,
    /// 目标由下一班正常搜索选中的工作房。
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
    /// 本班消费开始前执行的菲亚梅塔心情交换；没有可接受目标时为 `None`。
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
    /// 按完整轮换周期折算的 24h 平均产物。
    pub production: DailyProduction,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DailyProduction {
    pub lmd: f64,
    pub pure_gold: f64,
    pub battle_records: f64,
    pub orundum: f64,
    pub originium_shards: f64,
}

/// αβγ 三队轮换报告。
#[derive(Debug, Clone, Serialize)]
pub struct TeamRotationReport {
    pub profile: TimedRotationProfile,
    /// peak 班编排计划（只读；α/β 切半与 γ 贸易 role 填充均据此对齐）。
    pub peak_plan: AssignmentPlan,
    /// Every independently solved assignment keeps its own Plan owner.
    pub assignment_plans: Vec<AssignmentPlan>,
    /// Peak-dependent bind closures used while constructing this final rotation.
    #[serde(skip)]
    pub runtime_shift_binds: Vec<RuntimeShiftBind>,
    /// 最高效率 peak 编制从满心情工作到首个瓶颈触发的最长时间。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_mood_eta: Option<ShiftEta>,
    pub teams: Vec<TeamAssignment>,
    pub shifts: Vec<TeamShiftResult>,
    /// 三类各自按完整轮换周期时长归一到 24h 的加权产出。
    pub daily: DailyTotals,
    /// 报告级质量诊断（缺员、整周期无休班等）；空时省略。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub elapsed: Duration,
}

/// 生产设施一个半区（trade/manu/power 各一组完整房间）。
#[derive(Debug, Clone, Default)]
pub struct FacilityHalf {
    pub trade: Vec<RoomId>,
    pub manu: Vec<RoomId>,
    pub power: Vec<RoomId>,
}

impl FacilityHalf {
    pub(crate) fn contains(&self, room: &RoomId) -> bool {
        self.trade.iter().any(|candidate| candidate == room)
            || self.manu.iter().any(|candidate| candidate == room)
            || self.power.iter().any(|candidate| candidate == room)
    }
}
