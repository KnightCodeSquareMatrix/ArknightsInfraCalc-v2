pub mod eff_ramp;
pub mod error;
pub mod profile;
pub mod global_resource;
pub mod instances;
pub mod operbox;
pub mod pool;
pub mod roster;
pub mod schedule;
pub mod search;
pub mod skill_table;
pub mod tier;
pub mod types;

pub mod control;
pub mod layout;
pub mod manufacture;
pub mod office;
pub mod power;
pub mod trade;

pub use error::{Error, Result};
pub use instances::{buff_stem, resolve_buff_ids, OperatorInstances};
pub use operbox::{default_operbox_gongsun_path, OperBox};
pub use pool::{
    build_control_pool, build_manufacture_pool, build_power_pool, build_trade_pool,
    filter_control_pool, filter_manufacture_pool, filter_trade_pool, jie_e0_trade_operator,
    jie_market_trade_operator,
    ControlPool, ControlPoolEntry, ManuPool, ManuPoolEntry, PowerPool, PowerPoolEntry, TradePool,
    TradePoolEntry, JIE_TRADE_NAME,
};
pub use power::{
    apply_power_to_layout, charge_ramp_from_buffs, solve_power, PowerOperator, PowerResult,
    PowerRoomInput,
};
pub use manufacture::{
    score_manu_composite, solve_manufacture, ManuCompositeScore, ManuLineScenario, ManuOperator,
    ManuProdBreakdown, ManuResult, ManuRoomInput, ManuSearchRecipeMode, ManuStorageBreakdown,
};
pub use roster::{OperatorProgress, Roster};
pub use schedule::{
    schedule_base_rotation_a_b_a, schedule_jie_remainder_shift_from_pool,
    schedule_meta_shift_from_pool, schedule_trade_rotation_a_b_a, schedule_trade_shift,
    score_base_assignment, BaseRotationReport, BaseShiftPlan, BaseShiftRole, ShiftScores,
    TradeRotationReport, TradeShiftPlan, TradeStationPlan, TradeStationRole,
    TRADE_STATIONS_PER_SHIFT, WORKERS_PER_SHIFT,
};
pub use search::{
    hit_closure_shortcut, hit_docus_solo_shortcut, hit_witch_shortcut, search_control_combos,
    search_manufacture_triples, search_power_assignment, search_power_top, search_trade_triples,
    search_trade_triples_filtered, ControlSearchHit, ControlSearchOptions, ManuSearchHit,
    ManuSearchOptions, ManuSearchReport, PowerSearchHit, PowerSearchOptions, PowerSearchReport,
    PowerStationAssignment, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
    TradeSearchReport,
};
pub use control::{
    apply_control_to_layout, solve_control, ControlCenterResult, ControlOperator, ControlRoomInput,
};
pub use layout::{
    assign_base_greedy, assign_shift, assignment_operator_names, pinned_assignment,
    rotating_workers, resolve_automation_group_1_layout, resolve_base,
    resolve_search_baseline_layout, AssignBaseOptions, AssignShiftMode, AssignedOperator,
    BaseAssignment, BaseBlueprint, FacilityKind, LayoutContext, ResolvedBase, RoomId, RoomProduct,
    SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub use global_resource::{
    GlobalInjectManifest, GlobalResourceConversion, GlobalResourceEntry, GlobalResourceKey,
    GlobalResourcePool, GlobalResourceTier, INJECT_FAMILY_MANU_GLOBAL_ALL,
    INJECT_FAMILY_TRADE_GLOBAL_FLAT, CONVERSIONS, REGISTRY,
};
pub use skill_table::SkillTable;
pub use tier::PromotionTier;
pub use types::*;
