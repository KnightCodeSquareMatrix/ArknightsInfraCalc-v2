mod assign;
mod assignment;
mod blueprint;
mod context;
mod resolve;
mod shift;
mod system;
mod workforce;

pub use shift::AssignShiftMode;
pub use assign::{
    assign_base_greedy, assign_shift, assignment_operator_names, pinned_assignment,
    rotating_workers, AssignBaseOptions,
};
pub use system::{claim_base_systems, default_base_systems_path, load_base_systems};
pub use assignment::{AssignedOperator, BaseAssignment, RoomAssignment};
pub use blueprint::{
    BaseBlueprint, BlueprintScenario, FacilityKind, RoomBlueprint, RoomId, RoomProduct,
};
pub use context::{
    trade_station_tagged_gte_key, LayoutContext, SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub use resolve::{
    resolve_automation_group_1_layout, resolve_base, resolve_search_baseline_layout,
    resolve_snhunt_baseline_layout, resolve_snhunt_elite2_baseline_layout,
    snhunt_control_assignment, snhunt_default_assignment, ResolvedBase, ResolvedManuRoom,
    ResolvedPowerRoom, ResolvedTradeRoom,
};
pub use workforce::{
    is_elite_operator, is_platform_operator, WorkforceIndex, TAG_DURIN, TAG_ELITE_OPERATOR,
};
