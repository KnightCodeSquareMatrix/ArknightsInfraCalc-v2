mod auto_rotation;
mod base_rotation;
pub use auto_rotation::{
    assert_auto_rotation_invariants, schedule_auto_rotation, AutoControlEffects,
    AutoRoomRecommendation, AutoRoomState, AutoRotationReport,
};
pub(crate) mod control_rotation;
mod cycle_mood;
mod dorm_finalizer;
mod fiammetta_events;
mod room_admission;
mod rotation_scoring;
mod rotation_types;
mod shift_bind;
mod team_rotation;

pub use base_rotation::evaluate_base_assignment_efficiencies;
pub use rotation_types::{
    AutoRotationTiming, DailyProduction, DailyTotals, DormRestPlan, DroneAction, FacilityHalf,
    FiammettaShiftAction, GenericRotationTiming, RoomEfficiencyLine, RotationRequest,
    RuntimeShiftBind, ShiftEfficiencies, ShiftTransition, TeamAssignment, TeamLabel,
    TeamRotationReport, TeamShiftResult, TimedRotationProfile, TimedRotationRequest,
};

/// 两种输出分别保留其领域合同，共用模式/时长分派入口。
pub enum RotationReport {
    Timed(TeamRotationReport),
    Auto(AutoRotationReport),
}

pub fn schedule_rotation(
    blueprint: &crate::layout::BaseBlueprint,
    operbox: &crate::operbox::OperBox,
    instances: &crate::instances::OperatorInstances,
    table: &crate::skill_table::SkillTable,
    options: &crate::layout::AssignBaseOptions,
    request: RotationRequest,
) -> crate::error::Result<RotationReport> {
    let _memo_scope = crate::search::ensure_search_memo_scope();
    match request {
        RotationRequest::Timed(request) => team_rotation::execute_timed_rotation_request(
            blueprint, operbox, instances, table, options, request,
        )
        .map(RotationReport::Timed),
        RotationRequest::Auto(timing) => {
            schedule_auto_rotation(blueprint, operbox, instances, table, options, timing)
                .map(RotationReport::Auto)
        }
    }
}
pub use shift_bind::{
    align_shift_binds_in_halves, bound_operator_names, operator_in_shift, resting_shift_index,
    shift_binds_from_plan, team_of_operator, verify_shift_binds,
};
pub use team_rotation::{
    assert_final_rotation_invariants, assert_final_rotation_invariants_for_request,
    collect_final_rotation_issues, fiammetta_return_priority, operator_team_map,
    schedule_team_rotation, schedule_timed_rotation_request, RotationIssue,
    FIAMMETTA_RETURN_PRIORITY,
};
