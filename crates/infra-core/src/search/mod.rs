mod control;
pub(crate) mod exact_key;
mod manufacture;
pub(crate) mod memo;
mod power;
mod role_pick;
mod trade;

pub use memo::{ensure_search_memo_scope, SearchMemoScopeGuard};

pub use control::{
    control_efficiency_fill_sort_weight, control_entry_core_inject_fill,
    control_entry_deferred_trade_tags, control_entry_dynamic_trade_tags,
    control_entry_layered_fill, control_entry_mood_cost_fill, control_entry_plugin_fill,
    search_control_combos, ControlFillPolicy, ControlPolicyBreakdown, ControlSearchHit,
    ControlSearchOptions, MATATABI_CONSUMER_NAME,
};
pub(crate) use control::{
    control_inject_policy_sort_key_for_assignment, control_inject_policy_sort_key_upper_bound,
    search_control_combos_with_eligibility,
};
pub(crate) use manufacture::{
    compare_manufacture_hits, evaluate_manufacture_room, manufacture_combo_allowed_for_shift,
    manufacture_operator_has_mood_gap_decay, manufacture_peers_compatible,
    MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS,
};
pub use manufacture::{
    search_manufacture_triples, ManuEfficiencyBreakdown, ManuSearchHit, ManuSearchOptions,
    ManuSearchReport,
};
pub use power::{
    search_power_assignment, search_power_top, PowerEfficiencyBreakdown, PowerSearchHit,
    PowerSearchOptions, PowerSearchReport, PowerStationAssignment, VIRTUAL_POWER_MANU_EQUIV,
};
pub use role_pick::{
    hit_docus_syracusa_shortcut, pick_docus_trade_hit, pick_trade_role_hit,
    pick_trade_role_hit_requiring,
};
pub(crate) use trade::is_global_trade_forbidden_pair;
pub use trade::{
    hit_blackkey_closure_shortcut, hit_closure_shortcut, hit_docus_solo_shortcut,
    hit_witch_shortcut, search_trade_triples, search_trade_triples_filtered, SearchTripleFilter,
    TradeEfficiencyBreakdown, TradeSearchHit, TradeSearchOptions, TradeSearchReport,
};
