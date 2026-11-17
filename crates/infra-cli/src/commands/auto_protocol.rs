//! 自动轮换的 Worker 协议投影；不改变 core 的独立两态报告。

use infra_core::box_profile::{
    build_box_profile_from_metrics, manu_report_from_assignment, trade_report_from_assignment,
    BoxProfileMetrics, BoxProfileOptions,
};
use infra_core::export::{build_from_auto_rotation, MaaExportOptions};
use infra_core::layout::{
    AssignBaseOptions, AssignmentPlan, BaseAssignment, BaseBlueprint, FacilityKind, RoomId,
};
use infra_core::operbox::OperBox;
use infra_core::pool::{build_manufacture_pool, build_trade_pool};
use infra_core::schedule::{
    assert_auto_rotation_invariants, average_assignment_production,
    evaluate_base_assignment_efficiencies, schedule_auto_rotation, AutoFiammettaConfig,
    AutoRoomState, AutoRotationTiming, DailyTotals, RotationRequest, ShiftEfficiencies,
    TimedRotationProfile, TimedRotationRequest,
};
use infra_core::tier::PromotionTier;
use infra_core::training_advice::TrainingAdviceKnowledge;
use infra_core::{Efficiency, Error};
use serde::{Deserialize, Serialize};

use super::plan_compute::{PlanComputeOptions, PlanResources};
use super::serve::{build_training_advice_from_room_lines, PlanComputeResult};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum PlanRotationRequest {
    Auto(AutoRequest),
    AutoDefault(AutoDefaultRequest),
    Timed(TimedRotationRequest),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AutoDefaultRequest {
    Auto,
}

impl Default for PlanRotationRequest {
    fn default() -> Self {
        Self::Timed(TimedRotationRequest::default())
    }
}

impl PlanRotationRequest {
    pub(super) fn to_core(&self) -> RotationRequest {
        match self {
            Self::Timed(request) => RotationRequest::Timed(*request),
            Self::Auto(request) => RotationRequest::Auto(AutoRotationTiming {
                shift_hours: request.shift_hours,
            }),
            Self::AutoDefault(_) => RotationRequest::Auto(AutoRotationTiming {
                shift_hours: [24.0, 24.0],
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AutoRequest {
    #[serde(rename = "topology")]
    _topology: AutoTopology,
    pub shift_hours: [f64; 2],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AutoTopology {
    Auto,
}

#[derive(Serialize)]
struct AutoShiftSummary<'a> {
    index: usize,
    duration_hours: f64,
    // 沿用公共两班展示结构，不生成活动队、休息队或定时转换。
    assignment: &'a BaseAssignment,
    efficiencies: &'a ShiftEfficiencies,
    weighted_trade: Efficiency,
    weighted_manufacture: Efficiency,
    weighted_power: Efficiency,
}

#[derive(Serialize)]
struct AutoRoomSummary<'a> {
    room_id: &'a RoomId,
    facility: FacilityKind,
    main_missing_seats: usize,
    backup_missing_seats: usize,
    main_complete: bool,
    backup_complete: bool,
    main: &'a AutoRoomState,
    backup: &'a AutoRoomState,
}

#[derive(Serialize)]
struct AutoMetadata<'a> {
    mode: &'static str,
    timing: AutoRotationTiming,
    complete: bool,
    rooms: Vec<AutoRoomSummary<'a>>,
    constant_dorm_support: Vec<(&'a RoomId, Vec<&'a str>)>,
    assignment_plans: &'a [AssignmentPlan],
    fiammetta: &'a AutoFiammettaConfig,
    maa_execution_complete: bool,
}

#[derive(Serialize)]
struct AutoRotationSummary<'a> {
    // 仅为网站现有两班展示键，实际求解模式始终由 auto_rotation 标识。
    profile: TimedRotationProfile,
    daily: &'a DailyTotals,
    daily_is_estimate: bool,
    shifts: Vec<AutoShiftSummary<'a>>,
    warnings: &'a [String],
    auto_rotation: AutoMetadata<'a>,
}

pub(super) fn compute_auto_result(
    resources: PlanResources<'_>,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    labels: (&str, &str),
    options: &PlanComputeOptions<PlanRotationRequest>,
    timing: AutoRotationTiming,
    knowledge: &TrainingAdviceKnowledge,
) -> Result<serde_json::Value, Error> {
    let _memo_scope = infra_core::search::ensure_search_memo_scope();
    let _trace = infra_core::profile::ensure_solver_perf_trace(labels.0, labels.1);
    let report = schedule_auto_rotation(
        blueprint,
        operbox,
        resources.instances,
        resources.table,
        &AssignBaseOptions {
            top_k: options.top,
            system_preferences: options.system_preferences.clone(),
            fiammetta_enable: options.fiammetta_enable,
            support_combinations_enable: options.support_combinations_enable,
            ..AssignBaseOptions::default()
        },
        timing,
    )?;
    if options.assert_invariants {
        assert_auto_rotation_invariants(
            blueprint,
            operbox,
            resources.instances,
            resources.table,
            &report,
            options.fiammetta_enable,
        )?;
    }
    let durin = operbox.durin_dorm_planning_count(resources.instances);
    let assignments = [&report.main_assignment, &report.backup_assignment];
    let scores = assignments
        .iter()
        .zip(timing.shift_hours)
        .map(|(assignment, hours)| {
            evaluate_base_assignment_efficiencies(
                blueprint,
                assignment,
                resources.instances,
                resources.table,
                hours,
                Some(durin),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let total_hours: f64 = timing.shift_hours.iter().sum();
    let shifts: Vec<_> = assignments
        .iter()
        .zip(&scores)
        .zip(timing.shift_hours)
        .enumerate()
        .map(
            |(index, ((assignment, efficiencies), hours))| AutoShiftSummary {
                index,
                duration_hours: hours,
                assignment,
                efficiencies,
                weighted_trade: efficiencies.weighted_trade_for_cycle(hours, total_hours),
                weighted_manufacture: efficiencies
                    .weighted_manufacture_for_cycle(hours, total_hours),
                weighted_power: efficiencies.weighted_power_for_cycle(hours, total_hours),
            },
        )
        .collect();
    let daily = DailyTotals {
        trade: shifts.iter().map(|shift| shift.weighted_trade).sum(),
        manufacture: shifts.iter().map(|shift| shift.weighted_manufacture).sum(),
        power: shifts.iter().map(|shift| shift.weighted_power).sum(),
        production: average_assignment_production(
            blueprint,
            &timing
                .shift_hours
                .into_iter()
                .zip(&scores)
                .collect::<Vec<_>>(),
        ),
    };
    let display_profile = if timing.shift_hours == [24.0, 24.0] {
        TimedRotationProfile::MainBackup24_24
    } else {
        TimedRotationProfile::GenericMainBackup
    };
    let trade_report = trade_report_from_assignment(
        blueprint,
        &report.main_assignment,
        resources.instances,
        resources.table,
        durin,
    )?;
    let manu_report = manu_report_from_assignment(
        blueprint,
        &report.main_assignment,
        resources.instances,
        resources.table,
        durin,
    )?;
    let profile = build_box_profile_from_metrics(
        &BoxProfileMetrics {
            owned: operbox.owned_count(),
            tier_up_owned: operbox
                .owned
                .values()
                .filter(|tier| PromotionTier::is_tier_up(**tier))
                .count(),
            trade_pool_ready: build_trade_pool(
                &operbox.trade_roster(resources.instances),
                resources.instances,
                resources.table,
            )?
            .stats(3)
            .ready,
            manufacture_pool_ready: build_manufacture_pool(
                &operbox.manufacture_roster(resources.instances),
                resources.instances,
                resources.table,
            )?
            .stats(3)
            .ready,
            trade_report: &trade_report,
            manu_report: &manu_report,
            daily: &daily,
            daily_is_estimate: true,
            rotation_profile: display_profile,
        },
        blueprint,
        operbox,
        resources.instances,
        resources.table,
        labels.0,
        labels.1,
        &BoxProfileOptions::default(),
    )?;
    let mut maa_options = MaaExportOptions::for_blueprint(blueprint);
    if let Some(title) = &options.maa_title {
        maa_options.title = title.clone();
    }
    let maa = build_from_auto_rotation(blueprint, &report, &maa_options)?;
    let training_advice = build_training_advice_from_room_lines(
        operbox,
        blueprint,
        knowledge,
        scores.iter().flat_map(|score| score.room_lines.iter()),
    );
    let metadata = AutoMetadata {
        mode: report.mode,
        timing,
        complete: report.complete,
        rooms: report
            .rooms
            .iter()
            .map(|room| AutoRoomSummary {
                room_id: &room.room_id,
                facility: room.facility,
                main_missing_seats: room.main.missing_seats,
                backup_missing_seats: room.backup.missing_seats,
                main_complete: room.main.complete,
                backup_complete: room.backup.complete,
                main: &room.main,
                backup: &room.backup,
            })
            .collect(),
        constant_dorm_support: report
            .constant_dorm_support
            .iter()
            .map(|(id, operators)| {
                (
                    id,
                    operators
                        .iter()
                        .map(|operator| operator.name.as_str())
                        .collect(),
                )
            })
            .collect(),
        assignment_plans: &report.assignment_plans,
        fiammetta: &report.fiammetta,
        maa_execution_complete: false,
    };
    serde_json::to_value(PlanComputeResult {
        schema_version: 8,
        profile: Some(profile),
        maa: Some(maa),
        training_advice: Some(training_advice),
        error: None,
        rotation: Some(AutoRotationSummary {
            profile: display_profile,
            daily: &daily,
            daily_is_estimate: true,
            shifts,
            warnings: &report.warnings,
            auto_rotation: metadata,
        }),
    })
    .map_err(Error::from)
}
