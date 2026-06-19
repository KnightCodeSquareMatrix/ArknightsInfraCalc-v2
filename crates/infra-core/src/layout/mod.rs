mod assign;
mod assignment;
mod blueprint;
mod context;
mod orchestrate;
mod resolve;
mod shift;
mod system;
mod system_integrity;
mod workforce;

pub use shift::AssignShiftMode;
pub use assign::{
    assign_base_greedy, assign_power_rooms, assign_power_stations, assign_shift,
    assign_shift_with_plan, assign_team_gamma_half, assign_team_producer_rooms,
    assignment_operator_names, blackkey_witch_same_trade_room, pinned_assignment,
    rotating_workers, AssignBaseOptions, AssignShiftResult,
};
pub use system::{
    apply_registry_system_claim, claim_base_systems, default_base_systems_path, load_base_systems,
    select_registry_systems, RegistrySlotClaim, RegistrySystemClaim,
};
pub use system_integrity::{
    apply_rosemary_plan, evaluate_rosemary, evaluate_systems, EvaluateContext, EvaluateResult,
    OptionalProducer, RosemaryPlan, RosemaryTier, RosemaryVerdict, ShiftBind, SkipReason,
    SystemAnchor,
};
pub use assignment::{AssignedOperator, BaseAssignment, RoomAssignment};
pub use blueprint::{
    BaseBlueprint, BlueprintScenario, FacilityKind, RoomBlueprint, RoomId, RoomProduct,
};
pub use context::{
    trade_station_tagged_gte_key, LayoutContext, SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub use orchestrate::{
    build_plan, execute_plan, ActivatedSystem, AssignmentPlan, ExecuteResult, SlotFill,
};
pub use resolve::{
    resolve_automation_group_1_layout, resolve_base, resolve_search_baseline_layout,
    resolve_snhunt_baseline_layout, resolve_snhunt_elite2_baseline_layout,
    snhunt_control_assignment, snhunt_default_assignment, ResolvedBase, ResolvedManuRoom,
    ResolvedPowerRoom, ResolvedTradeRoom,
};
pub use workforce::{
    is_elite_operator, is_platform_operator, WorkforceIndex, TAG_DURIN, TAG_ELITE_OPERATOR,
    TAG_KNIGHT, TAG_PINUS, TAG_RHINE,
};
