//! Arknights base mechanics, search, layout assignment, timed scheduling, and export types.
//!
//! # Main entrypoints
//!
//! - Load runtime inputs with [`SkillTable`], [`OperatorInstances`], [`OperBox`], and
//!   [`BaseBlueprint`].
//! - Build one shift with [`assign_shift`] or [`assign_base_greedy`], then inspect derived
//!   context with [`resolve_base`]. The [`layout`] module owns Plan execution and facility fill.
//! - Build a named or generic timed schedule with [`schedule_timed_rotation_request`].
//!   [`schedule_team_rotation`] is the default ABC convenience wrapper.
//! - Evaluate facility candidates through [`search`] and the [`trade`], [`manufacture`],
//!   [`power`], and [`control`] domain modules.
//! - Export an already solved rotation with [`build_from_team_rotation`] and [`MaaSchedule`].
//! - Build account analysis through [`box_profile`] and training recommendations through
//!   [`training_advice`].
//!
//! # Boundaries
//!
//! This crate owns mechanisms and solver results. `infra-cli` owns transport, file loading, and
//! output formatting; exporters do not recompute mechanisms or choose a different assignment.
//! Canonical domain rules live in the repository `docs/` owners rather than in crate-level API
//! prose.
//!
//! Fallible public operations use [`Result`] and [`Error`]. Direct I/O and data-format conversions
//! can retain typed sources; domain validation, unsupported states, and contextualized failures use
//! [`Error::Msg`].

pub mod bake;
pub mod box_profile;
pub mod candidate;
pub mod cross_facility;
pub mod eff_ramp;
pub mod efficiency;
pub mod error;
pub mod global_resource;
pub mod instances;
pub mod mood;
pub mod operbox;
pub mod pool;
pub mod profile;
pub mod recycle;
pub mod response_dependency;
pub mod roster;
pub mod schedule;
pub mod scoring;
pub mod search;
pub mod skill_table;
pub mod support_facility;
pub mod tier;
pub mod training_advice;
pub mod types;

pub mod control;
pub mod export;
pub mod layout;
pub mod manufacture;
pub mod meeting;
pub mod notes;
pub mod office;
pub mod power;
pub mod trade;

pub use bake::{
    bake_and_publish_catalogs, bake_catalogs, current_exe_generator_fingerprint,
    default_baked_out_dir, load_complete_baked_trade_catalog, resolved_baked_catalog_dir,
    validate_baked_catalog, verify_baked_catalog_responses, warm_runtime_baked_table,
    BakeGeneratorFingerprint, BakeMode, BakeOptions, BakeProgressEvent, BakeReport,
    BakedTradeRowId, CompleteBakedTradeCatalog, CompleteBakedTradeRow, CompleteBakedTradeRows,
    BAKE_SCHEMA_VERSION,
};
pub use box_profile::{
    baseline_path_or_default, build_box_profile, render_box_profile_narrative, ActionKind,
    BoxProfile, BoxProfileOptions, GapSeverity, ProfileAction,
};
pub use candidate::{CandidateSource, CandidateStationKind, TeamCandidate, TeamMetric};
pub use control::{
    apply_control_to_layout, solve_control, ControlCenterResult, ControlOperator, ControlRoomInput,
};
pub use efficiency::Efficiency;
pub use error::{Error, Result};
pub use export::{build_from_team_rotation, MaaExportOptions, MaaSchedule};
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
    BaseBlueprint, FacilityKind, LayoutContext, OperatorTier, ResolvedBase, ResolvedSupportRoom,
    RoomId, RoomProduct, SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT,
};
pub use manufacture::{
    evaluate_manufacture_lines, solve_manufacture, ManuLineEfficiency, ManuLineScenario,
    ManuOperator, ManuProdBreakdown, ManuResult, ManuRoomInput, ManuSearchRecipeMode,
    ManuStorageBreakdown,
};
pub use meeting::evaluate_meeting;
pub use mood::{
    dorm_recovery_rates, facility_key as mood_facility_key, operator_net_drain, shift_eta,
    workable_hours, DormOccupant, DrainInputs, MoodModel, OperatorEta, ShiftEta,
};
pub use notes::Note;
pub use office::evaluate_office;
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
    aggregate_power_efficiency, apply_power_to_layout, charge_ramp_from_buffs, solve_power,
    PowerOperator, PowerResult, PowerRoomInput,
};
pub use response_dependency::{
    build_response_dependency_report, build_response_dependency_report_for_blueprint,
    default_producer_rules_path, deferred_producer_rules_for_buffs, load_producer_rule_catalog,
    producer_rule_catalog, resolve_assignment_producer_dependencies, DependencyScenario,
    DependencyScope, DomainDependencyContributor, DomainDependencyInput, DomainDependencyInputDecl,
    DomainInputSource, ProducerAdmission, ProducerRule, ProducerRuleCatalog,
    ResolvedProducerDependency, ResourceClosureEdge, ResourceClosureEdgeKind,
    ResourceConversionDependency, ResourceEquivalenceClass, ResourceReachableRange,
    ResourceReadFormula, ResourceReverseClosure, ResourceValueDomainFact, ResponseDependencyReport,
    ResponseDependencyRow, ResponseField, ScheduleDependencyRelation,
    UnresolvedDelegatedDependency,
};
pub use roster::{OperatorProgress, Roster};
pub use schedule::{
    evaluate_base_assignment_efficiencies, operator_team_map, schedule_team_rotation,
    schedule_timed_rotation_request, DailyTotals, DormRestPlan, GenericRotationTiming,
    ShiftEfficiencies, ShiftTransition, TeamAssignment, TeamLabel, TeamRotationReport,
    TeamShiftResult, TimedRotationProfile, TimedRotationRequest,
};
pub use scoring::{
    evaluate_control_inject_policy, evaluate_trade_manufacture_weighted_efficiency, EffPct,
    PolicyEvaluation, ScoringPolicyId, TradeManuEfficiencyComponents,
    TradeManufactureEfficiencyWeights, TradeManufactureWeightedEfficiencyEvaluation,
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
pub use support_facility::{
    OperatorMoodDelta, SupportContribution, SupportFacility, SupportNotice, SupportOperator,
    SupportRegistry, SupportRoomInput, SupportRoomResult,
};
pub use tier::PromotionTier;
pub use training_advice::{
    build_player_training_advice, build_training_advice, build_training_advice_rag_input,
    default_training_advice_knowledge_path, default_training_recommendations_path,
    load_training_advice_knowledge, load_training_recommendation_rules,
    render_training_advice_answer, validate_training_advice_knowledge, AcquisitionInfo,
    AcquisitionKind, AcquisitionMode, AcquisitionPolicy, BlockedRuleReport,
    CombinationMemberProgress, CombinationProgress, CombinationProgressState, CombinationScale,
    CompletionAlternative, EvidenceRef, EvidenceSnippet, MemberBenefit, MemberProgress, MemberRole,
    MissingCoreGroupReport, NewbieProgress, OperatorAdviceItem, OperatorTrainingState,
    PickOneCoreSlot, PlayerAction, PlayerAdviceContext, PlayerRecommendation,
    PlayerRecommendationPriority, PlayerRecommendationReason, PlayerTrainingAdviceReport,
    RecommendationAction, RecommendationPriority, RequiredCoreGroup, ReviewStatus, RuleAdmission,
    RuleKind, RuleMatch, RuleMember, RuleReview, RuleScope, TrainingAdviceBundle,
    TrainingAdviceFact, TrainingAdviceKnowledge, TrainingAdviceOptions, TrainingAdviceRagInput,
    TrainingAdviceReport, TrainingAdviceSummary, TrainingRecommendationRules, TrainingRule,
    TrainingTarget, MAX_PLAYER_RECOMMENDATIONS,
};
pub use types::*;
