mod base_rotation;
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
    DailyProduction, DailyTotals, DormRestPlan, DroneAction, FacilityHalf, FiammettaShiftAction,
    GenericRotationTiming, RoomEfficiencyLine, RuntimeShiftBind, ShiftEfficiencies,
    ShiftTransition, TeamAssignment, TeamLabel, TeamRotationReport, TeamShiftResult,
    TimedRotationProfile, TimedRotationRequest,
};
pub use shift_bind::{
    align_shift_binds_in_halves, bound_operator_names, operator_in_shift, resting_shift_index,
    shift_binds_from_plan, team_of_operator, verify_shift_binds,
};
pub use team_rotation::{
    assert_final_rotation_invariants, assert_final_rotation_invariants_for_request,
    fiammetta_return_priority, operator_team_map, schedule_team_rotation,
    schedule_timed_rotation_request, FIAMMETTA_RETURN_PRIORITY,
};
