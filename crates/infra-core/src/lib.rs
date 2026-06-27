pub mod bake;
pub mod box_profile;
pub mod cross_facility;
pub mod eff_ramp;
pub mod error;
pub mod global_resource;
pub mod instances;
pub mod operbox;
pub mod pool;
pub mod profile;
pub mod roster;
pub mod schedule;
pub mod scoring;
pub mod search;
pub mod skill_table;
pub mod tier;
pub mod types;

pub mod control;
pub mod export;
pub mod layout;
pub mod manufacture;
pub mod office;
pub mod power;
pub mod trade;

pub use bake::{
    bake_catalogs, validate_baked_catalog, BakeGeneratorFingerprint, BakeOptions, BakeReport,
    BAKE_SCHEMA_VERSION,
};
pub use box_profile::{
    baseline_path_or_default, build_box_profile, render_box_profile_narrative, ActionKind,
    BoxProfile, BoxProfileOptions, GapSeverity, ProfileAction,
};
pub use control::{
    apply_control_to_layout, solve_control, ControlCenterResult, ControlOperator, ControlRoomInput,
};
pub use error::{Error, Result};
pub use export::{
    build_from_base_rotation, build_from_team_rotation, MaaExportOptions, MaaSchedule,
};
pub use global_resource::{
    GlobalInjectManifest, GlobalResourceConversion, GlobalResourceEntry, GlobalResourceKey,
    GlobalResourcePool, GlobalResourceTier, CONVERSIONS, INJECT_FAMILY_MANU_GLOBAL_ALL,
    INJECT_FAMILY_TRADE_GLOBAL_FLAT, REGISTRY,
};
pub use instances::{buff_stem, resolve_buff_ids, OperatorInstances};
pub use layout::{
    assign_base_greedy, assign_shift, assignment_operator_names, pinned_assignment,
    resolve_automation_group_1_layout, resolve_base, resolve_search_baseline_layout,
    rotating_workers, AssignBaseOptions, AssignShiftMode, AssignedOperator, BaseAssignment,
    BaseBlueprint, FacilityKind, LayoutContext, OperatorTier, ResolvedBase, RoomId, RoomProduct,
    SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub use manufacture::{
    score_manu_composite, solve_manufacture, ManuCompositeScore, ManuLineScenario, ManuOperator,
    ManuProdBreakdown, ManuResult, ManuRoomInput, ManuSearchRecipeMode, ManuStorageBreakdown,
};
pub use operbox::{
    default_layout_243_path, default_operbox_full_e2_path, default_operbox_gongsun_path,
    from_xlsx_path, OperBox,
};
pub use pool::{
    build_control_pool, build_manufacture_pool, build_power_pool, build_trade_pool,
    filter_control_pool, filter_manufacture_pool, filter_trade_pool, jie_e0_trade_operator,
    jie_market_trade_operator, ControlPool, ControlPoolEntry, ManuPool, ManuPoolEntry, PowerPool,
    PowerPoolEntry, TradePool, TradePoolEntry, JIE_TRADE_NAME,
};
pub use power::{
    apply_power_to_layout, charge_ramp_from_buffs, solve_power, PowerOperator, PowerResult,
    PowerRoomInput,
};
pub use roster::{OperatorProgress, Roster};
pub use schedule::{
    operator_team_map, schedule_base_rotation_a_b_a, schedule_jie_remainder_shift_from_pool,
    schedule_meta_shift_from_pool, schedule_team_rotation, schedule_trade_rotation_a_b_a,
    schedule_trade_shift, score_base_assignment, BaseRotationReport, BaseShiftPlan, BaseShiftRole,
    DailyTotals, ShiftScores, TeamAssignment, TeamLabel, TeamRotationReport, TeamShiftResult,
    TradeRotationReport, TradeShiftPlan, TradeStationPlan, TradeStationRole,
    TRADE_STATIONS_PER_SHIFT, WORKERS_PER_SHIFT,
};
pub use scoring::{
    current_control_inject_sort_score, ComponentScore, EffPct, ScoringPolicyId,
    TradeManuEfficiencyComponents,
};
pub use search::{
    hit_closure_shortcut, hit_docus_solo_shortcut, hit_witch_shortcut, search_control_combos,
    search_manufacture_triples, search_power_assignment, search_power_top, search_trade_triples,
    search_trade_triples_filtered, ControlSearchHit, ControlSearchOptions, ManuSearchHit,
    ManuSearchOptions, ManuSearchReport, PowerSearchHit, PowerSearchOptions, PowerSearchReport,
    PowerStationAssignment, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
    TradeSearchReport,
};
pub use skill_table::SkillTable;
pub use tier::PromotionTier;
pub use types::*;
