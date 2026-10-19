mod assign;
mod assignment;
mod blueprint;
mod context;
mod orchestrate;
pub(crate) use orchestrate::assert_support_combination_policy;
mod resolve;
mod shift;
mod system;
pub mod tier;
mod vacancy_fill;
mod workforce;

pub(crate) use assign::assign_control_with_saileach;
pub(crate) use assign::assign_manu_room_with_anchors;
pub(crate) use assign::assign_shift_with_preclaimed_plan;
pub(crate) use assign::refill_manu_room_with_core;
pub(crate) use assign::refill_trade_room_with_core;
pub use assign::{
    assign_base_greedy, assign_power_rooms, assign_power_stations, assign_shift,
    assign_shift_with_plan, assign_shift_with_plan_skip, assign_team_gamma_half,
    assign_team_producer_rooms, assignment_operator_names, blackkey_witch_same_trade_room,
    explain_assignment_systems, pinned_assignment, pinned_assignment_excluding, rotating_workers,
    AssignBaseOptions, AssignShiftResult, TradeRolePreference,
};
pub(crate) use assign::{beta_tailor_names, refill_witch_room_with_tailor};
pub use assignment::{AssignedOperator, BaseAssignment, RoomAssignment, RoomEfficiencySnapshot};
pub use blueprint::{
    recycle_base_efficiency_millis, recycle_operator_capacity, BaseBlueprint, BlueprintScenario,
    FacilityKind, RoomBlueprint, RoomId, RoomProduct,
};
pub use context::{
    trade_station_tagged_gte_key, LayoutContext, SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub(crate) use orchestrate::pack_production_components;
pub use orchestrate::{
    build_plan, build_plan_with_runtime, execute_plan, ActivatedSystem, ActiveDependency,
    AnchorFillPolicy, AssignmentPlan, ContinuousRole, ControlCandidateRequirement, ExecuteResult,
    ReserveReusePolicy, ResolvedRoleReserve, SelectedRuleAlternative, ShiftBind, SlotFill,
    SystemAnchor, SystemConstraint,
};
pub(crate) use resolve::resolve_control_input_layout;
pub use resolve::{
    resolve_automation_group_1_layout, resolve_base, resolve_base_with_trace,
    resolve_search_baseline_layout, resolve_snhunt_baseline_layout,
    resolve_snhunt_elite2_baseline_layout, snhunt_control_assignment, snhunt_default_assignment,
    ResolveTrace, ResolvedBase, ResolvedManuRoom, ResolvedPowerRoom, ResolvedSupportRoom,
    ResolvedTradeRoom,
};
pub use shift::AssignShiftMode;
pub use system::{
    apply_registry_system_claim, claim_base_systems, default_base_systems_path, load_base_systems,
    select_registry_systems, RegistrySlotClaim, RegistrySystemClaim, SystemExplainEntry,
    SystemExplainReason, SystemExplainReport, SystemExplainStatus, SystemSlotExplain,
};
pub use tier::OperatorTier;
pub(crate) use vacancy_fill::plan_admits_workplace_candidate;
pub(crate) use vacancy_fill::{
    fill_workplace_vacancies, ordinary_room_compatible, ordinary_workplace,
};
pub use workforce::{
    is_elite_operator, is_platform_operator, WorkforceIndex, TAG_DURIN, TAG_ELITE_OPERATOR,
    TAG_KNIGHT, TAG_PINUS, TAG_RHINE,
};
