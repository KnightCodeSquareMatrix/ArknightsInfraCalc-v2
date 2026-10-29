use std::collections::HashSet;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use crate::control::{
    solve_control, solve_control_with_trace, ControlEffectKey, ControlOperator, ControlRoomInput,
    ControlSettlementTrace, ControlTraceOutcome,
};
use crate::error::Result;
use crate::global_resource::{
    GlobalResourceKey, InjectMergeStatus, INJECT_FAMILY_MANU_GLOBAL_ALL,
    INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
use crate::layout::LayoutContext;
use crate::pool::{combinations_indices, ControlPool};
use crate::scoring::{
    evaluate_control_inject_policy, ScoringPolicyId, TradeManuEfficiencyComponents,
};
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, Phase, RecipeKind, Selector};

/// 木天蓼 consumer：贸易/制造侧的泰拉大陆调查团。
pub const MATATABI_CONSUMER_NAME: &str = "泰拉大陆调查团";

/// 中枢具名排序 policy 的完整分解。
///
/// 当前中枢排序策略（`ControlInjectRawSumV0`）：
/// ```text
/// sort_key = trade_inject + manu_gold_inject + manu_br_inject
/// ```
/// 这是局部注入排序策略：三项均为可解释效率百分比分量。它不是
/// 贸易/制造平衡公式；vpower、木天蓼、心情扣分也不在中枢 policy 里预支，
/// 而是分别在制造 resolve、调查团 consumer、轮换层体现。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ControlPolicyBreakdown {
    /// 具名局部排序策略；不是生产效率。
    pub policy: ScoringPolicyId,
    /// 贸易效率注入%
    pub trade_inject_pct: f64,
    /// 赤金制造效率注入%
    pub manu_gold_inject_pct: f64,
    /// 经验书制造效率注入%
    pub manu_br_inject_pct: f64,
    /// 当前注入排序 key = trade + gold + br；仅用于中枢候选排序。
    pub inject_subtotal: f64,
    /// 虚拟发电站数
    pub virtual_power: f64,
    /// 木天蓼点数
    pub matatabi: f64,
    /// 本班是否有调查团 consumer
    pub matatabi_consumer_active: bool,
    /// 心情消耗总和 (>0 的部分)
    pub mood_penalty: f64,
    /// 体系外散件排序分量（单走八幡海铃等；与 +7/+2 同层比较）。
    pub loose_piece_sort_component: f64,
    /// 心情补位排序分量（EW / 玛恩纳等；低于效率散件）。
    pub mood_fill_sort_component: f64,
    /// 线索补位排序分量（低于心情）。
    pub clue_fill_sort_component: f64,
    /// policy 排序键；中枢补位按 组合体系(pinned) → 散件 → 心情 → 线索。
    pub policy_sort_key: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlSearchHit {
    pub names: Vec<String>,
    pub trade_inject_pct: f64,
    pub manu_gold_inject_pct: f64,
    /// 具名 policy 明细
    pub breakdown: ControlPolicyBreakdown,
}

/// 中枢补位策略：`base_systems` 钉死组合体系后，剩余席位按
/// 散件（单走八幡海铃 / +7 / +2）→ 心情 → 线索填，而非热情贸易链。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlFillPolicy {
    #[default]
    InjectOnly,
    /// 公孙分层补位：注入散件 → 心情 → 线索。
    LayeredFill,
}

#[derive(Debug, Clone)]
pub struct ControlSearchOptions {
    pub max_operators: u8,
    pub top_k: usize,
    pub mood: f64,
    pub layout: LayoutContext,
    /// 本班编制是否已有调查团在贸易/制造上岗；无 consumer 时木天蓼不计正分。
    pub matatabi_consumer_active: bool,
    /// 组合必须包含这些干员（如 `base_systems` 已钉死的中枢位）。
    pub must_include: HashSet<String>,
    /// 候选组最低命中数；约束合法组合，不改变实际效率排序。
    pub candidate_requirements: Vec<(HashSet<String>, u8)>,
    pub fill_policy: ControlFillPolicy,
    /// 当前班次可用办公室的最高静态加成；仅用于琴柳的条件准入特判。
    pub saileach_best_office_pct: Option<f64>,
    /// Plan 是否硬要求琴柳。硬要求时不能由该特判移除。
    pub saileach_required: bool,
}

impl Default for ControlSearchOptions {
    fn default() -> Self {
        Self {
            max_operators: 5,
            top_k: 20,
            mood: 24.0,
            layout: LayoutContext::default(),
            matatabi_consumer_active: false,
            must_include: HashSet::new(),
            candidate_requirements: Vec::new(),
            fill_policy: ControlFillPolicy::default(),
            saileach_best_office_pct: None,
            saileach_required: false,
        }
    }
}

/// 热情/MyGO 经济链 buff：补位阶段排除，避免占满剩余中枢位。
pub fn control_passion_chain_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "control_dorm_bd[000]"
            | "control_mp_bd&trade[000]"
            | "control_prod_bd_spd[000]"
            | "control_prod_bd_spd[010]"
    )
}

fn control_resource_producer_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "control_mp_bd[000]"
            | "control_mp_bd[010]"
            | "control_mp_bd2[000]"
            | "control_mp_cost&bd2[010]"
    )
}

fn control_standalone_producer_buff(buff_id: &str) -> bool {
    control_passion_chain_buff(buff_id) || control_resource_producer_buff(buff_id)
}

fn control_efficiency_inject_buff(buff_id: &str) -> bool {
    buff_id.starts_with("control_prod_spd")
        || buff_id.starts_with("control_tra_spd")
        || buff_id.starts_with("control_token_prod_spd")
        || buff_id.starts_with("control_tra_limit&spd")
        || buff_id.starts_with("control_bd_spd")
        || matches!(
            buff_id,
            "control_token_tra_spd[000]" | "control_prod_tra_spd[000]"
        )
}

pub fn control_efficiency_fill_sort_weight(entry: &crate::pool::ControlPoolEntry) -> f64 {
    // 旧调用点仍需要稳定排序 API；实际候选优劣统一由组合 solver 决定。
    let _ = entry;
    0.0
}

fn control_mood_cost_buff(buff_id: &str) -> bool {
    buff_id == "control_mp_psk[000]"
        || buff_id == "control_mp_lonely[000]"
        || buff_id == "control_mp_expand_double[000]"
        || (buff_id.starts_with("control_mp_cost[") && !buff_id.contains('&'))
        || buff_id.starts_with("control_mp_cost&faction")
}

fn control_loose_piece_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "control_hire_spd&bd[000]" | "control_tra_limit&spd[000]"
    )
}

fn control_clue_buff(_buff_id: &str) -> bool {
    false
}

fn control_layered_fill_buff(buff_id: &str) -> bool {
    control_loose_piece_buff(buff_id)
        || control_mood_cost_buff(buff_id)
        || control_clue_buff(buff_id)
}

fn control_inject_sort_key(hit: &ControlSearchHit) -> f64 {
    hit.breakdown.policy_sort_key
}

#[derive(Debug)]
struct RankedControlSearchHit {
    hit: ControlSearchHit,
    trade_global_pct: f64,
    manufacture_global_pct: f64,
    office_global_pct: f64,
    meeting_global_pct: f64,
    replacement_preference: i32,
    suppressed_replacement_sources: usize,
    redundant_effect_sources: usize,
    redundant_global_recovery_sources: usize,
}

#[derive(Debug, Clone, Copy)]
struct ControlReplacementRule {
    source_buff_id: &'static str,
    family: &'static str,
    preference: i32,
}

const CONTROL_REPLACEMENT_RULES: [ControlReplacementRule; 4] = [
    ControlReplacementRule {
        source_buff_id: "control_prod_spd[1000]",
        family: INJECT_FAMILY_MANU_GLOBAL_ALL,
        preference: 2,
    },
    ControlReplacementRule {
        source_buff_id: "control_prod_spd[000]",
        family: INJECT_FAMILY_MANU_GLOBAL_ALL,
        preference: -2,
    },
    ControlReplacementRule {
        source_buff_id: "control_tra_spd[030]",
        family: INJECT_FAMILY_TRADE_GLOBAL_FLAT,
        preference: 1,
    },
    ControlReplacementRule {
        source_buff_id: "control_tra_spd[020]",
        family: INJECT_FAMILY_TRADE_GLOBAL_FLAT,
        preference: -1,
    },
];

fn control_replacement_candidate(operator: &ControlOperator) -> bool {
    CONTROL_REPLACEMENT_RULES.iter().any(|replacement| {
        operator
            .buff_ids
            .iter()
            .any(|buff_id| buff_id == replacement.source_buff_id)
    })
}

fn control_replacement_status(
    trace: &ControlSettlementTrace,
    replacement: ControlReplacementRule,
) -> (bool, bool) {
    let mut effective = false;
    let mut suppressed = false;
    for event in &trace.events {
        if event.source_buff_id != replacement.source_buff_id {
            continue;
        }
        let merge = match &event.outcome {
            ControlTraceOutcome::TradeInject {
                family,
                merge: Some(merge),
                ..
            }
            | ControlTraceOutcome::ManufactureInject {
                family,
                merge: Some(merge),
                ..
            } if family == replacement.family => merge,
            _ => continue,
        };
        match merge.status {
            InjectMergeStatus::Winner | InjectMergeStatus::TiedWinner => effective = true,
            InjectMergeStatus::Suppressed => suppressed = true,
            _ => {}
        }
    }
    (effective, suppressed)
}

fn control_replacement_rank(trace: &ControlSettlementTrace) -> (i32, usize) {
    let preference = CONTROL_REPLACEMENT_RULES
        .iter()
        .filter_map(|replacement| {
            let (effective, suppressed) = control_replacement_status(trace, *replacement);
            (effective && !suppressed).then_some(replacement.preference)
        })
        .sum();
    let suppressed_sources = CONTROL_REPLACEMENT_RULES
        .iter()
        .filter(|replacement| control_replacement_status(trace, **replacement).1)
        .count();
    (preference, suppressed_sources)
}

fn compare_ranked_control_hits(
    a: &RankedControlSearchHit,
    b: &RankedControlSearchHit,
) -> std::cmp::Ordering {
    let key = |hit: &RankedControlSearchHit| {
        let value = control_inject_sort_key(&hit.hit);
        if value.is_nan() {
            f64::NEG_INFINITY
        } else {
            value
        }
    };
    // Cross-domain effects are selected in their declared order. The legacy
    // raw inject key only resolves candidates equal in every migrated region.
    b.trade_global_pct
        .total_cmp(&a.trade_global_pct)
        .then_with(|| {
            b.manufacture_global_pct
                .total_cmp(&a.manufacture_global_pct)
        })
        .then_with(|| b.office_global_pct.total_cmp(&a.office_global_pct))
        .then_with(|| b.meeting_global_pct.total_cmp(&a.meeting_global_pct))
        .then_with(|| a.redundant_effect_sources.cmp(&b.redundant_effect_sources))
        .then_with(|| {
            a.suppressed_replacement_sources
                .cmp(&b.suppressed_replacement_sources)
        })
        .then_with(|| key(b).total_cmp(&key(a)))
        .then_with(|| b.replacement_preference.cmp(&a.replacement_preference))
        .then_with(|| {
            a.redundant_global_recovery_sources
                .cmp(&b.redundant_global_recovery_sources)
        })
        .then_with(|| a.hit.names.cmp(&b.hit.names))
}

pub fn control_entry_core_inject_fill(entry: &crate::pool::ControlPoolEntry) -> bool {
    if entry.name == "琴柳" {
        return false;
    }
    entry
        .buff_ids
        .iter()
        .any(|b| control_efficiency_inject_buff(b))
}

/// 返回该中枢候选会按贸易站实际标签人数动态结算的标签集合。
/// 识别依据是 atom 结构，不依赖干员名或具体倍率。
pub fn control_entry_dynamic_trade_tags(
    entry: &crate::pool::ControlPoolEntry,
    table: &SkillTable,
) -> HashSet<String> {
    let mut tags = HashSet::new();
    for buff_id in &entry.buff_ids {
        let Some(skill) = table.get(buff_id) else {
            continue;
        };
        for atom in &skill.atoms {
            if atom.phase != Phase::GlobalInject
                || !matches!(&atom.action, Action::GlobalInjectTradeEff { .. })
            {
                continue;
            }
            if let Some(
                Selector::TaggedCountInTradeSum { tag }
                | Selector::TaggedCountInCurrentTradeRoom { tag }
                | Selector::TradeStationsWithTaggedGte { tag, .. },
            ) = atom.selector.as_ref()
            {
                tags.insert(tag.clone());
            }
        }
    }
    tags
}

/// 返回编排规则明确分类为 deferred-optional 的贸易 producer 标签。
pub fn control_entry_deferred_trade_tags(
    entry: &crate::pool::ControlPoolEntry,
    table: &SkillTable,
) -> Result<HashSet<String>> {
    let rules = crate::response_dependency::deferred_producer_rules_for_buffs(
        table,
        entry.buff_ids.iter().map(String::as_str),
        "trade",
    )?;
    let rule_buffs: HashSet<_> = rules
        .iter()
        .map(|rule| rule.source_buff_id.as_str())
        .collect();
    let mut tags = HashSet::new();
    for buff_id in entry
        .buff_ids
        .iter()
        .filter(|buff_id| rule_buffs.contains(buff_id.as_str()))
    {
        let Some(skill) = table.get(buff_id) else {
            continue;
        };
        for atom in &skill.atoms {
            if let Some(
                Selector::TaggedCountInTradeSum { tag }
                | Selector::TaggedCountInCurrentTradeRoom { tag }
                | Selector::TradeStationsWithTaggedGte { tag, .. },
            ) = atom.selector.as_ref()
            {
                tags.insert(tag.clone());
            }
        }
    }
    Ok(tags)
}

pub fn control_entry_mood_cost_fill(entry: &crate::pool::ControlPoolEntry) -> bool {
    if entry.name == "琴柳" {
        return false;
    }
    if entry
        .buff_ids
        .iter()
        .any(|b| control_standalone_producer_buff(b))
    {
        return false;
    }
    entry.buff_ids.iter().any(|b| control_mood_cost_buff(b))
}

pub fn control_entry_plugin_fill(entry: &crate::pool::ControlPoolEntry) -> bool {
    if entry.name == "琴柳" {
        return false;
    }
    if entry
        .buff_ids
        .iter()
        .any(|b| control_standalone_producer_buff(b))
    {
        return false;
    }
    control_entry_core_inject_fill(entry) || control_entry_layered_fill(entry)
}

pub fn control_entry_layered_fill(entry: &crate::pool::ControlPoolEntry) -> bool {
    if entry.name == "琴柳" {
        return false;
    }
    if entry
        .buff_ids
        .iter()
        .any(|b| control_standalone_producer_buff(b) || control_efficiency_inject_buff(b))
    {
        return false;
    }
    entry.buff_ids.iter().any(|b| control_layered_fill_buff(b))
}

/// 体系外中枢补位分层：散件（单走八幡海铃）→ 心情（EW / 玛恩纳）→ 线索。

fn control_global_recovery_redundancy(
    global_recovery: crate::mood::ControlGlobalRecoverySummary,
) -> usize {
    global_recovery.redundant_owner_count
}

/// 中枢搜索排序：当前为贸易/制造注入%之和。
///
/// 公孙长乐口径下不需要跨贸易/制造平衡公式；贸易站赤金订单总效率、
/// 制造赤金效率、制造经验效率作为分量分别解释。这里保留裸加只作为
/// 当前中枢候选的局部排序策略，并通过 `scoring` 命名入口显式标记。
///
/// vpower、木天蓼、心情扣分不在此评分——它们分别在制造站 resolve、调查团 consumer、
/// 轮换层心情管理里体现。
///
/// 返回完整的 policy 分解；`.policy_sort_key` 只用于中枢候选排序。
fn evaluate_control_policy(
    result: &crate::control::ControlCenterResult,
    _operators: &[ControlOperator],
    _table: &SkillTable,
    _mood_model: &crate::mood::MoodModel,
    options: &ControlSearchOptions,
    _global_recovery: crate::mood::ControlGlobalRecoverySummary,
) -> ControlPolicyBreakdown {
    let mood_penalty: f64 = result
        .operator_mood_drains
        .values()
        .filter(|v| **v > 0.0)
        .sum();

    let trade_inject = result
        .inject
        .trade_eff_pct_with_tag_counts(&options.layout.trade_tagged_count_sum);
    let manu_gold = result.inject.manu_eff_for(RecipeKind::Gold);
    let manu_br = result.inject.manu_eff_for(RecipeKind::BattleRecord);
    let gold_line_count = options.layout.gold_manu_line_count.min(u32::from(u8::MAX)) as u8;
    let battle_record_line_count = options
        .layout
        .manufacture_station_count
        .saturating_sub(gold_line_count);
    let component_score = evaluate_control_inject_policy(TradeManuEfficiencyComponents {
        trade_eff_pct: trade_inject,
        gold_manu_eff_pct: manu_gold,
        battle_record_manu_eff_pct: manu_br,
        trade_station_count: options.layout.trade_station_count,
        gold_line_count,
        battle_record_line_count,
    });
    let inject_subtotal = component_score.sort_key_pct;

    let matatabi = result.global.get(GlobalResourceKey::Matatabi);
    let virtual_power = result.global.get(GlobalResourceKey::VirtualPower);

    ControlPolicyBreakdown {
        policy: component_score.policy,
        trade_inject_pct: trade_inject,
        manu_gold_inject_pct: manu_gold,
        manu_br_inject_pct: manu_br,
        inject_subtotal,
        virtual_power,
        matatabi,
        matatabi_consumer_active: options.matatabi_consumer_active,
        mood_penalty,
        policy_sort_key: inject_subtotal,
        ..ControlPolicyBreakdown::default()
    }
}

pub(crate) fn control_inject_policy_sort_key_for_assignment(
    layout: &LayoutContext,
    blueprint: &crate::layout::BaseBlueprint,
    assignment: &crate::layout::BaseAssignment,
    instances: &crate::instances::OperatorInstances,
) -> f64 {
    evaluate_control_inject_policy(control_inject_components_for_assignment(
        layout, blueprint, assignment, instances,
    ))
    .sort_key_pct
}

pub(crate) fn control_inject_components_for_assignment(
    layout: &LayoutContext,
    blueprint: &crate::layout::BaseBlueprint,
    assignment: &crate::layout::BaseAssignment,
    instances: &crate::instances::OperatorInstances,
) -> TradeManuEfficiencyComponents {
    let empty = std::collections::HashMap::new();
    let global_trade = layout
        .global_inject
        .trade_eff_pct_with_scoped_tag_counts(&layout.trade_tagged_count_sum, &empty);
    let current_room_increment: f64 = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::TradePost)
        .map(|room| {
            let mut counts = std::collections::HashMap::new();
            for operator in assignment.operators_in(&room.id) {
                let mut seen = HashSet::new();
                for tag in instances.tags_for(&operator.name, operator.tier()) {
                    if seen.insert(tag.clone()) {
                        *counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
            layout
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(&layout.trade_tagged_count_sum, &counts)
                - global_trade
        })
        .sum();
    let mut gold_manu = layout.global_inject.manu_eff_for(RecipeKind::Gold);
    let mut battle_record_manu = layout.global_inject.manu_eff_for(RecipeKind::BattleRecord);
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::Factory)
    {
        let Some(crate::layout::RoomProduct::Factory { recipe }) = room.product.as_ref() else {
            continue;
        };
        let tagged: f64 = assignment
            .operators_in(&room.id)
            .iter()
            .flat_map(|operator| {
                let tags = instances.tags_for(&operator.name, operator.tier());
                layout
                    .global_inject
                    .manu_tagged()
                    .iter()
                    .filter(move |rule| {
                        rule.recipe.is_none_or(|target| target == *recipe)
                            && tags.iter().any(|tag| tag == &rule.target_tag)
                    })
                    .map(|rule| rule.value)
            })
            .sum();
        match recipe {
            RecipeKind::Gold => gold_manu += tagged,
            RecipeKind::BattleRecord => battle_record_manu += tagged,
            RecipeKind::All | RecipeKind::Originium => {}
        }
    }
    TradeManuEfficiencyComponents {
        trade_eff_pct: global_trade + current_room_increment,
        gold_manu_eff_pct: gold_manu,
        battle_record_manu_eff_pct: battle_record_manu,
        trade_station_count: layout.trade_station_count,
        gold_line_count: layout.gold_manu_line_count.min(u32::from(u8::MAX)) as u8,
        battle_record_line_count: layout
            .manufacture_station_count
            .saturating_sub(layout.gold_manu_line_count.min(u32::from(u8::MAX)) as u8),
    }
}

pub(crate) fn control_inject_policy_sort_key_upper_bound(
    layout: &LayoutContext,
    blueprint: &crate::layout::BaseBlueprint,
    table: &SkillTable,
) -> f64 {
    let support_dependent = layout.global_inject.active_source_buffs().any(|buff_id| {
        table.get(buff_id).is_some_and(|skill| {
            skill.atoms.iter().any(|atom| {
                let policy_action = matches!(
                    &atom.action,
                    Action::GlobalInjectTradeEff { .. }
                        | Action::GlobalInjectManuEff { .. }
                        | Action::GlobalInjectManuTaggedEff { .. }
                );
                let condition_is_safe = matches!(
                    atom.condition.as_ref(),
                    None | Some(
                        Condition::MoodAbove { .. }
                            | Condition::MoodAboveOrEq { .. }
                            | Condition::MoodBelow { .. }
                            | Condition::MoodBelowOrEq { .. }
                            | Condition::OwnerEliteGte { .. }
                            | Condition::OwnerEliteBelow { .. }
                            | Condition::PartnerInRoom { .. }
                            | Condition::TagPresentInRoom { .. }
                            | Condition::PeerTagInRoom { .. }
                            | Condition::OwnerLacksBuff { .. }
                            | Condition::ExternalMomentumGteField {}
                            | Condition::FieldMomentumGtExternal {}
                    )
                );
                let selector_is_safe = matches!(
                    atom.selector.as_ref(),
                    None | Some(
                        Selector::TaggedCountInControl { .. }
                            | Selector::ControlOperatorCount
                            | Selector::TradeStationCount
                            | Selector::PowerStationCount
                            | Selector::TaggedCountInTradeSum { .. }
                            | Selector::TaggedCountInCurrentTradeRoom { .. }
                            | Selector::TradeStationsWithTaggedGte { .. }
                            | Selector::TaggedCountInManuSum { .. }
                    )
                );
                policy_action && (!condition_is_safe || !selector_is_safe)
            })
        })
    });
    if support_dependent {
        return f64::INFINITY;
    }
    let mut upper = layout.clone();
    let trade_rules: Vec<_> = upper.global_inject.trade_tagged().to_vec();
    let max_trade_slots: u8 = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::TradePost)
        .map(|room| room.operator_capacity().min(usize::from(u8::MAX)) as u8)
        .fold(0u8, u8::saturating_add);
    for rule in &trade_rules {
        match rule.count_scope {
            crate::global_resource::TradeTaggedCountScope::AllTradeRooms => {
                upper
                    .trade_tagged_count_sum
                    .entry(rule.target_tag.clone())
                    .and_modify(|count| *count = (*count).max(max_trade_slots))
                    .or_insert(max_trade_slots);
            }
            crate::global_resource::TradeTaggedCountScope::QualifiedTradeRooms { min } => {
                let rooms = blueprint
                    .rooms
                    .iter()
                    .filter(|room| {
                        room.kind == crate::layout::FacilityKind::TradePost
                            && room.operator_capacity() >= usize::from(min)
                    })
                    .count()
                    .min(usize::from(u8::MAX)) as u8;
                upper.trade_stations_tagged_gte.insert(
                    crate::layout::trade_station_tagged_gte_key(&rule.target_tag, min),
                    rooms,
                );
            }
            crate::global_resource::TradeTaggedCountScope::CurrentTradeRoom => {}
        }
    }
    upper
        .global_inject
        .refresh_qualified_trade_counts(&upper.trade_stations_tagged_gte);

    let max_manu_slots: u8 = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::Factory)
        .map(|room| room.operator_capacity().min(usize::from(u8::MAX)) as u8)
        .fold(0u8, u8::saturating_add);
    for rule in upper.global_inject.manu_count_scaled().to_vec() {
        upper
            .manu_tagged_count_sum
            .entry(rule.target_tag)
            .and_modify(|count| *count = (*count).max(max_manu_slots))
            .or_insert(max_manu_slots);
    }
    upper
        .global_inject
        .refresh_manu_count_scaled(&upper.manu_tagged_count_sum);

    let empty = std::collections::HashMap::new();
    let global_trade = upper
        .global_inject
        .trade_eff_pct_with_scoped_tag_counts(&upper.trade_tagged_count_sum, &empty);
    let current_room_increment: f64 = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::TradePost)
        .map(|room| {
            let count = room.operator_capacity().min(usize::from(u8::MAX)) as u8;
            let current: std::collections::HashMap<_, _> = trade_rules
                .iter()
                .filter(|rule| {
                    rule.count_scope
                        == crate::global_resource::TradeTaggedCountScope::CurrentTradeRoom
                })
                .map(|rule| (rule.target_tag.clone(), count))
                .collect();
            upper
                .global_inject
                .trade_eff_pct_with_scoped_tag_counts(&upper.trade_tagged_count_sum, &current)
                - global_trade
        })
        .sum();
    let mut gold_manu = upper.global_inject.manu_eff_for(RecipeKind::Gold);
    let mut battle_record_manu = upper.global_inject.manu_eff_for(RecipeKind::BattleRecord);
    for room in blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == crate::layout::FacilityKind::Factory)
    {
        let Some(crate::layout::RoomProduct::Factory { recipe }) = room.product.as_ref() else {
            continue;
        };
        let per_operator: f64 = upper
            .global_inject
            .manu_tagged()
            .iter()
            .filter(|rule| rule.recipe.is_none_or(|target| target == *recipe))
            .map(|rule| rule.value.max(0.0))
            .sum();
        let tagged = per_operator * room.operator_capacity() as f64;
        match recipe {
            RecipeKind::Gold => gold_manu += tagged,
            RecipeKind::BattleRecord => battle_record_manu += tagged,
            RecipeKind::All | RecipeKind::Originium => {}
        }
    }
    evaluate_control_inject_policy(TradeManuEfficiencyComponents {
        trade_eff_pct: global_trade + current_room_increment,
        gold_manu_eff_pct: gold_manu,
        battle_record_manu_eff_pct: battle_record_manu,
        trade_station_count: upper.trade_station_count,
        gold_line_count: upper.gold_manu_line_count.min(u32::from(u8::MAX)) as u8,
        battle_record_line_count: upper
            .manufacture_station_count
            .saturating_sub(upper.gold_manu_line_count.min(u32::from(u8::MAX)) as u8),
    })
    .sort_key_pct
}

/// Deterministic sequential policy: required groups, then trade, manufacture,
/// office, meeting, mood and ordinary fill. Each decision reads only the
/// selected prefix plus one candidate. Failure means this policy found no
/// completion, not that the full canonical assignment space is infeasible.
fn search_control_in_order(
    pool: &ControlPool,
    table: &SkillTable,
    options: &ControlSearchOptions,
    eligibility: Option<&(dyn Fn(&[String]) -> bool + Sync)>,
) -> Result<Vec<ControlSearchHit>> {
    let observe_control =
        std::env::var_os("ARKNIGHTS_INFRA_OBSERVE_CONTROL").is_some_and(|value| value == "1");
    macro_rules! observe {
        ($($arg:tt)*) => {
            if observe_control {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "[INFRA-CONTROL-OBSERVE] {}",
                    format_args!($($arg)*)
                );
            }
        };
    }
    let model = crate::mood::default_mood_model()?;
    let mut entries: Vec<_> =
        pool.entries
            .iter()
            .filter_map(|entry| {
                match crate::response_dependency::control_buff_is_automatically_excluded(
                    table,
                    entry.buff_ids.iter().map(String::as_str),
                ) {
                    Ok(excluded) => (!excluded || options.must_include.contains(&entry.name))
                        .then_some(Ok(entry)),
                    Err(error) => Some(Err(error)),
                }
            })
            .collect::<Result<_>>()?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.dedup_by(|a, b| a.name == b.name);
    let capacity = usize::from(options.max_operators.min(5));
    let mut selected: Vec<ControlOperator> = entries
        .iter()
        .filter(|entry| options.must_include.contains(&entry.name))
        .map(|entry| entry.to_control_operator())
        .collect();
    if selected.len() != options.must_include.len() || selected.len() > capacity {
        observe!("search_control initial_prefix={:?} must_include={:?} capacity={capacity} rejected=missing_or_over_capacity_required",
            selected.iter().map(|op| &op.name).collect::<Vec<_>>(),
            options.must_include.iter().collect::<std::collections::BTreeSet<_>>());
        return Ok(Vec::new());
    }
    let evaluate = |ops: &[ControlOperator]| {
        solve_control(
            &ControlRoomInput {
                operators: ops.to_vec(),
                mood: options.mood,
                layout: options.layout.clone(),
            },
            table,
        )
    };
    let legal = |ops: &[ControlOperator]| {
        if !crate::layout::admission::candidate_admitted(
            &options.layout,
            "control",
            ops.iter()
                .map(|op| {
                    (
                        op.name.clone(),
                        op.elite,
                        op.buff_ids.clone(),
                        op.tags.clone(),
                    )
                })
                .collect(),
            table,
            false,
        ) {
            return false;
        }
        let control: Vec<_> = ops.iter().map(|op| (op.name.clone(), op.elite)).collect();
        !model.control_global_recovery_has_conflict(&control)
            && !evaluate(ops).effect_ledger.has_conflicting_effect_sources()
            && (!ops.iter().any(|op| op.name == "琴柳")
                || options.saileach_required
                || options.saileach_best_office_pct.is_none_or(|best| {
                    let other: Vec<_> =
                        ops.iter().filter(|op| op.name != "琴柳").cloned().collect();
                    5.0 + best + evaluate(&other).office_hire_spd_pct < 30.0
                }))
    };
    if !legal(&selected) {
        observe!(
            "search_control initial_prefix={:?} rejected=illegal_required_prefix",
            selected.iter().map(|op| &op.name).collect::<Vec<_>>()
        );
        return Ok(Vec::new());
    }
    let group_count = |ops: &[ControlOperator], candidates: &HashSet<String>| {
        ops.iter()
            .filter(|op| candidates.contains(&op.name))
            .count()
    };
    // Necessary residual group checks, not a combination witness or extra bind.
    let groups_can_fit = |ops: &[ControlOperator]| {
        options
            .candidate_requirements
            .iter()
            .all(|(candidates, min)| {
                let deficit = usize::from(*min).saturating_sub(group_count(ops, candidates));
                deficit <= capacity.saturating_sub(ops.len())
                    && deficit
                        <= entries
                            .iter()
                            .filter(|entry| {
                                candidates.contains(&entry.name)
                                    && !ops.iter().any(|op| op.name == entry.name)
                            })
                            .count()
            })
    };
    let value = |result: &crate::control::ControlCenterResult, region: usize| match region {
        // Only static global ledger regions participate here. Dynamic producer
        // and count-scaled projections are resolved after the room context and
        // cannot decide an earlier control seat.
        0 => result
            .effect_ledger
            .value(ControlEffectKey::TRADE_GLOBAL_EFFICIENCY),
        1 => result
            .effect_ledger
            .value(ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY),
        // Runtime has no "mastery in progress" input. Training stays explicitly
        // Unsupported rather than gaining a guessed +5 candidate value.
        2 => 0.0,
        3 if options.layout.office_room_count > 0 => result.office_hire_spd_pct,
        4 if options.layout.meeting_room_count > 0 => result.meeting_speed_pct,
        _ => 0.0,
    };
    let accepts_complete = |ops: &[ControlOperator]| {
        if ops.len() != capacity {
            return true;
        }
        let names: Vec<_> = ops.iter().map(|op| op.name.clone()).collect();
        eligibility.is_none_or(|accept| accept(&names))
    };
    for (candidates, min_count) in &options.candidate_requirements {
        while group_count(&selected, candidates) < usize::from(*min_count) {
            let prefix = observe_control.then(|| {
                selected
                    .iter()
                    .map(|op| op.name.as_str())
                    .collect::<Vec<_>>()
            });
            if observe_control {
                let mut group_names: Vec<_> = candidates.iter().map(String::as_str).collect();
                group_names.sort_unstable();
                observe!("control_step stage=required_group prefix={prefix:?} group_candidates={group_names:?} min_count={min_count}");
            }
            let mut ranked = Vec::new();
            for entry in &entries {
                if !candidates.contains(&entry.name)
                    || selected.iter().any(|op| op.name == entry.name)
                {
                    continue;
                }
                let mut trial = selected.clone();
                trial.push(entry.to_control_operator());
                let trial_names = observe_control
                    .then(|| trial.iter().map(|op| op.name.as_str()).collect::<Vec<_>>());
                if trial.len() > capacity {
                    observe!("control_try stage=required_group prefix={prefix:?} candidate={} result=rejected_capacity trial={trial_names:?}", entry.name);
                    continue;
                }
                if !legal(&trial) {
                    observe!("control_try stage=required_group prefix={prefix:?} candidate={} result=rejected_legal_constraint trial={trial_names:?}", entry.name);
                    continue;
                }
                if !groups_can_fit(&trial) {
                    observe!("control_try stage=required_group prefix={prefix:?} candidate={} result=rejected_min_count_residual trial={trial_names:?}", entry.name);
                    continue;
                }
                if !accepts_complete(&trial) {
                    observe!("control_try stage=required_group prefix={prefix:?} candidate={} result=rejected_complete_eligibility trial={trial_names:?}", entry.name);
                    continue;
                }
                let result = evaluate(&trial);
                ranked.push((
                    *entry,
                    [
                        value(&result, 0),
                        value(&result, 1),
                        value(&result, 2),
                        value(&result, 3),
                        value(&result, 4),
                    ],
                ));
            }
            ranked.sort_by(|(a, av), (b, bv)| {
                bv.iter()
                    .zip(av)
                    .map(|(b, a)| b.total_cmp(a))
                    .find(|order| !order.is_eq())
                    .unwrap_or_else(|| a.name.cmp(&b.name))
            });
            let Some((entry, _)) = ranked.first() else {
                observe!("control_step stage=required_group prefix={prefix:?} result=no_candidate");
                return Ok(Vec::new());
            };
            observe!(
                "control_select stage=required_group prefix={prefix:?} candidate={} ranked={:?}",
                entry.name,
                ranked
                    .iter()
                    .map(|(candidate, scores)| (&candidate.name, scores))
                    .collect::<Vec<_>>()
            );
            selected.push(entry.to_control_operator());
        }
    }
    // Regional effective maxima. A later region never changes an earlier seat.
    for region in 0..5 {
        if selected.len() >= capacity {
            break;
        }
        let prefix = observe_control.then(|| {
            selected
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>()
        });
        let before = evaluate(&selected);
        let before_score = value(&before, region);
        observe!("control_step stage=regional region={region} prefix={prefix:?} current_score={before_score}");
        let mut best: Option<(&crate::pool::ControlPoolEntry, f64)> = None;
        for entry in &entries {
            if selected.iter().any(|op| op.name == entry.name) {
                continue;
            }
            let mut trial = selected.clone();
            trial.push(entry.to_control_operator());
            let trial_names = observe_control
                .then(|| trial.iter().map(|op| op.name.as_str()).collect::<Vec<_>>());
            if !legal(&trial) {
                observe!("control_try stage=regional region={region} prefix={prefix:?} candidate={} result=rejected_legal_constraint trial={trial_names:?}", entry.name);
                continue;
            }
            if !groups_can_fit(&trial) {
                observe!("control_try stage=regional region={region} prefix={prefix:?} candidate={} result=rejected_min_count_residual trial={trial_names:?}", entry.name);
                continue;
            }
            let result = evaluate(&trial);
            let score = value(&result, region);
            if score <= before_score {
                observe!("control_try stage=regional region={region} prefix={prefix:?} candidate={} result=no_improvement score={score} current={before_score} trial={trial_names:?}", entry.name);
                continue;
            }
            if !accepts_complete(&trial) {
                observe!("control_try stage=regional region={region} prefix={prefix:?} candidate={} result=rejected_complete_eligibility score={score} trial={trial_names:?}", entry.name);
                continue;
            }
            observe!("control_try stage=regional region={region} prefix={prefix:?} candidate={} result=eligible score={score} previous_best={:?} trial={trial_names:?}",
                entry.name,
                best.map(|(previous, score)| (&previous.name, score)));
            if best.is_none_or(|(previous, previous_value)| {
                score > previous_value || (score == previous_value && entry.name < previous.name)
            }) {
                best = Some((entry, score));
            }
        }
        if let Some((entry, score)) = best {
            observe!("control_select stage=regional region={region} prefix={prefix:?} candidate={} score={score}", entry.name);
            selected.push(entry.to_control_operator());
        } else {
            observe!(
                "control_select stage=regional region={region} prefix={prefix:?} candidate=none"
            );
        }
    }
    // The recovery family has a single owner. Its common five-facility
    // projection includes spread smileys but never individual drain or ETA.
    if selected.len() < capacity {
        let prefix = observe_control.then(|| {
            selected
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>()
        });
        observe!("control_step stage=global_recovery prefix={prefix:?}");
        let mut best = None;
        for entry in &entries {
            if selected.iter().any(|op| op.name == entry.name)
                || !model
                    .control_global_recovery
                    .iter()
                    .any(|source| source.name == entry.name && entry.elite >= source.elite)
            {
                continue;
            }
            let mut trial = selected.clone();
            trial.push(entry.to_control_operator());
            let trial_names = observe_control
                .then(|| trial.iter().map(|op| op.name.as_str()).collect::<Vec<_>>());
            if !legal(&trial) {
                observe!("control_try stage=global_recovery prefix={prefix:?} candidate={} result=rejected_legal_constraint trial={trial_names:?}", entry.name);
                continue;
            }
            if !groups_can_fit(&trial) {
                observe!("control_try stage=global_recovery prefix={prefix:?} candidate={} result=rejected_min_count_residual trial={trial_names:?}", entry.name);
                continue;
            }
            if !accepts_complete(&trial) {
                observe!("control_try stage=global_recovery prefix={prefix:?} candidate={} result=rejected_complete_eligibility trial={trial_names:?}", entry.name);
                continue;
            }
            let control: Vec<_> = trial.iter().map(|op| (op.name.clone(), op.elite)).collect();
            let values = [
                crate::layout::FacilityKind::Factory,
                crate::layout::FacilityKind::TradePost,
                crate::layout::FacilityKind::PowerPlant,
                crate::layout::FacilityKind::Office,
                crate::layout::FacilityKind::MeetingRoom,
            ]
            .map(|facility| model.control_uniform_recovery_for_facility(facility, &control));
            // Only a facility-by-facility dominance relation is meaningful:
            // trade/factory recovery cannot be traded against office recovery.
            // Non-dominating effects retain stable identity order.
            let is_better = best.as_ref().is_none_or(
                |(_, previous_values): &(&crate::pool::ControlPoolEntry, [f64; 5])| {
                    values
                        .iter()
                        .zip(previous_values)
                        .all(|(value, previous)| value >= previous)
                        && values
                            .iter()
                            .zip(previous_values)
                            .any(|(value, previous)| value > previous)
                },
            );
            observe!("control_try stage=global_recovery prefix={prefix:?} candidate={} result={} facility_recovery={values:?} trial={trial_names:?}", entry.name, if is_better { "policy_promoted" } else { "policy_not_promoted" });
            if is_better {
                best = Some((*entry, values));
            }
        }
        if let Some((entry, _)) = best {
            observe!(
                "control_select stage=global_recovery prefix={prefix:?} candidate={}",
                entry.name
            );
            selected.push(entry.to_control_operator());
        } else {
            observe!("control_select stage=global_recovery prefix={prefix:?} candidate=none");
        }
    }
    while selected.len() < capacity && selected.len() < entries.len() {
        let prefix = observe_control.then(|| {
            selected
                .iter()
                .map(|op| op.name.as_str())
                .collect::<Vec<_>>()
        });
        observe!("control_step stage=mood_and_ordinary_fill prefix={prefix:?}");
        let control: Vec<_> = selected
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect();
        let owner = model.control_global_recovery.iter().find(|source| {
            control
                .iter()
                .any(|(name, elite)| name == &source.name && *elite >= source.elite)
        });
        let mut ranked = Vec::new();
        for entry in &entries {
            if selected.iter().any(|op| op.name == entry.name) {
                continue;
            }
            let mut trial = selected.clone();
            trial.push(entry.to_control_operator());
            let trial_names = observe_control
                .then(|| trial.iter().map(|op| op.name.as_str()).collect::<Vec<_>>());
            if !legal(&trial) {
                observe!("control_try stage=mood_and_ordinary_fill prefix={prefix:?} candidate={} result=rejected_legal_constraint trial={trial_names:?}", entry.name);
                continue;
            }
            if !groups_can_fit(&trial) {
                observe!("control_try stage=mood_and_ordinary_fill prefix={prefix:?} candidate={} result=rejected_min_count_residual trial={trial_names:?}", entry.name);
                continue;
            }
            let after: Vec<_> = trial.iter().map(|op| (op.name.clone(), op.elite)).collect();
            let internal = model.control_uniform_recovery_for_facility(
                crate::layout::FacilityKind::ControlCenter,
                &after,
            ) - model.control_uniform_recovery_for_facility(
                crate::layout::FacilityKind::ControlCenter,
                &control,
            );
            // Named canonical preference consumes data, not a hard-coded score
            // or an individual net-drain objective.
            let preferred = owner.is_some_and(|source| {
                source.bonus_when_control_contains.as_deref() == Some(entry.name.as_str())
                    || (source.spread_smiley
                        && model
                            .control_smiley_recovery
                            .as_ref()
                            .is_some_and(|smiley| {
                                smiley.providers.iter().any(|provider| {
                                    provider.name == entry.name && entry.elite >= provider.elite
                                })
                            }))
            });
            let effect = if owner.is_some_and(|source| {
                source.bonus_when_control_contains.as_deref() == Some(entry.name.as_str())
            }) {
                owner.expect("owner").bonus_value
            } else {
                internal
            };
            if !accepts_complete(&trial) {
                observe!("control_try stage=mood_and_ordinary_fill prefix={prefix:?} candidate={} result=rejected_complete_eligibility preferred={preferred} effect={effect} trial={trial_names:?}", entry.name);
                continue;
            }
            observe!("control_try stage=mood_and_ordinary_fill prefix={prefix:?} candidate={} result=eligible preferred={preferred} effect={effect} trial={trial_names:?}", entry.name);
            ranked.push((*entry, preferred, effect.max(0.0)));
        }
        ranked.sort_by(|(a, ap, av), (b, bp, bv)| {
            bp.cmp(ap)
                .then_with(|| bv.total_cmp(av))
                .then_with(|| a.name.cmp(&b.name))
        });
        let Some((entry, ..)) = ranked.first() else {
            observe!(
                "control_step stage=mood_and_ordinary_fill prefix={prefix:?} result=no_candidate"
            );
            if options.layout.auto_rotation {
                break;
            }
            return Ok(Vec::new());
        };
        observe!("control_select stage=mood_and_ordinary_fill prefix={prefix:?} candidate={} ranked={:?}", entry.name, ranked.iter().map(|(candidate, preferred, effect)| (&candidate.name, preferred, effect)).collect::<Vec<_>>());
        selected.push(entry.to_control_operator());
    }
    if !options
        .candidate_requirements
        .iter()
        .all(|(candidates, min)| group_count(&selected, candidates) >= usize::from(*min))
    {
        observe!(
            "control_result selected={:?} rejected=final_min_count",
            selected.iter().map(|op| &op.name).collect::<Vec<_>>()
        );
        return Ok(Vec::new());
    }
    let mut names: Vec<_> = selected.iter().map(|op| op.name.clone()).collect();
    names.sort();
    if eligibility.is_some_and(|accept| !accept(&names)) {
        observe!("control_result selected={names:?} rejected=final_eligibility");
        return Ok(Vec::new());
    }
    observe!("control_result selected={names:?} accepted=true");
    let result = evaluate(&selected);
    let summary = model.control_global_recovery_summary(
        &selected
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect::<Vec<_>>(),
    );
    let mut legacy_options = options.clone();
    legacy_options.fill_policy = ControlFillPolicy::InjectOnly;
    let breakdown =
        evaluate_control_policy(&result, &selected, table, model, &legacy_options, summary);
    Ok(vec![ControlSearchHit {
        names,
        trade_inject_pct: breakdown.trade_inject_pct,
        manu_gold_inject_pct: breakdown.manu_gold_inject_pct,
        breakdown,
    }])
}

/// 中枢满编搜索；LayeredFill 使用顺序 policy，InjectOnly 保留兼容组合入口。
pub fn search_control_combos(
    pool: &ControlPool,
    table: &SkillTable,
    options: &ControlSearchOptions,
) -> Result<Vec<ControlSearchHit>> {
    search_control_combos_with_eligibility(pool, table, options, None)
}

/// Apply a caller-owned feasibility constraint before ranking/top-K. The public
/// search keeps its existing policy when no timed work budget is supplied.
pub(crate) fn search_control_combos_with_eligibility(
    pool: &ControlPool,
    table: &SkillTable,
    options: &ControlSearchOptions,
    eligibility: Option<&(dyn Fn(&[String]) -> bool + Sync)>,
) -> Result<Vec<ControlSearchHit>> {
    if options.fill_policy == ControlFillPolicy::LayeredFill {
        return search_control_in_order(pool, table, options, eligibility);
    }
    let leaf_started = Instant::now();
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Control);
    // InjectOnly retains its legacy bounded tail. LayeredFill above consumes
    // every admitted identity and does not assume empty buffs imply equivalence.
    let automatically_excluded: HashSet<_> = pool
        .entries
        .iter()
        .map(|entry| {
            crate::response_dependency::control_buff_is_automatically_excluded(
                table,
                entry.buff_ids.iter().map(String::as_str),
            )
            .map(|excluded| (entry.name.as_str(), excluded))
        })
        .collect::<Result<_>>()?;
    let is_automatically_excluded = |name: &str| {
        automatically_excluded
            .iter()
            .any(|(excluded_name, excluded)| *excluded && *excluded_name == name)
    };
    let mut entries: Vec<_> = pool
        .entries
        .iter()
        .filter(|entry| {
            (!is_automatically_excluded(&entry.name) || options.must_include.contains(&entry.name))
                && (!entry.buff_ids.is_empty()
                    || options.must_include.contains(&entry.name)
                    || options
                        .candidate_requirements
                        .iter()
                        .any(|(candidates, _)| candidates.contains(&entry.name)))
        })
        .collect();
    let fallback_slots = usize::from(options.max_operators.min(5));
    entries.extend(
        pool.entries
            .iter()
            .filter(|entry| {
                !is_automatically_excluded(&entry.name)
                    && entry.buff_ids.is_empty()
                    && !options.must_include.contains(&entry.name)
                    && !options
                        .candidate_requirements
                        .iter()
                        .any(|(candidates, _)| candidates.contains(&entry.name))
            })
            .take(fallback_slots),
    );
    let n = entries.len();
    if n == 0 {
        perf.finish(0, 0, 0, 0, false, Default::default());
        return Ok(Vec::new());
    }

    let max_k = options.max_operators.min(5).min(n as u8) as usize;
    let required_indices: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| options.must_include.contains(&entry.name))
        .map(|(index, _)| index)
        .collect();
    if required_indices.len() != options.must_include.len() || required_indices.len() > max_k {
        perf.finish(0, 0, 0, 0, false, Default::default());
        return Ok(Vec::new());
    }
    let optional_indices: Vec<_> = (0..n)
        .filter(|index| !required_indices.contains(index))
        .collect();
    let remaining = max_k - required_indices.len();
    let combos: Vec<Vec<usize>> = combinations_indices(optional_indices.len(), remaining)
        .map(|indices| {
            let mut combo = required_indices.clone();
            combo.extend(indices.into_iter().map(|index| optional_indices[index]));
            combo.sort_unstable();
            combo
        })
        .collect();
    let combinations_count = combos.len() as u64;
    perf.set_progress(
        combinations_count,
        combinations_count,
        0,
        Default::default(),
    );

    let layout = options.layout.clone();
    let mood = options.mood;
    let mood_model = crate::mood::default_mood_model()?;
    let prepare_elapsed = leaf_started.elapsed();

    let evaluate_started = Instant::now();
    perf.mark_live_started();
    let mut hits: Vec<RankedControlSearchHit> = combos
        .par_iter()
        .filter_map(|idxs| {
            let operators: Vec<_> = idxs
                .iter()
                .map(|i| entries[*i].to_control_operator())
                .collect();
            let mut names: Vec<String> = operators.iter().map(|o| o.name.clone()).collect();
            names.sort();
            if eligibility.is_some_and(|accept| !accept(&names)) {
                return None;
            }
            let input = ControlRoomInput {
                operators: operators.clone(),
                mood,
                layout: layout.clone(),
            };
            let (result, replacement_preference, suppressed_replacement_sources) =
                if operators.iter().any(control_replacement_candidate) {
                    let (result, trace) = solve_control_with_trace(&input, table);
                    let (preference, suppressed_sources) = control_replacement_rank(&trace);
                    (result, preference, suppressed_sources)
                } else {
                    (solve_control(&input, table), 0, 0)
                };
            if result.effect_ledger.has_conflicting_effect_sources() {
                return None;
            }
            if !options.saileach_required
                && operators.iter().any(|operator| operator.name == "琴柳")
            {
                if let Some(best_office_pct) = options.saileach_best_office_pct {
                    let other_control = solve_control(
                        &ControlRoomInput {
                            operators: operators
                                .iter()
                                .filter(|operator| operator.name != "琴柳")
                                .cloned()
                                .collect(),
                            mood,
                            layout: layout.clone(),
                        },
                        table,
                    );
                    if 5.0
                        + best_office_pct
                        + other_control
                            .effect_ledger
                            .value(ControlEffectKey::OFFICE_HIRE_SPEED)
                        >= 30.0
                    {
                        return None;
                    }
                }
            }
            let control_operators = operators
                .iter()
                .map(|operator| (operator.name.clone(), operator.elite))
                .collect::<Vec<_>>();
            let global_recovery = mood_model.control_global_recovery_summary(&control_operators);
            if mood_model.control_global_recovery_has_conflict(&control_operators) {
                return None;
            }
            let breakdown = evaluate_control_policy(
                &result,
                &operators,
                table,
                &mood_model,
                options,
                global_recovery,
            );
            Some(RankedControlSearchHit {
                hit: ControlSearchHit {
                    trade_inject_pct: result
                        .inject
                        .trade_eff_pct_with_tag_counts(&layout.trade_tagged_count_sum),
                    manu_gold_inject_pct: result.inject.manu_eff_for(RecipeKind::Gold),
                    names,
                    breakdown,
                },
                trade_global_pct: result
                    .effect_ledger
                    .value(ControlEffectKey::TRADE_GLOBAL_EFFICIENCY),
                manufacture_global_pct: result
                    .effect_ledger
                    .value(ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY),
                office_global_pct: if layout.office_room_count > 0 {
                    {
                        result
                            .effect_ledger
                            .value(ControlEffectKey::OFFICE_HIRE_SPEED)
                    }
                } else {
                    0.0
                },
                meeting_global_pct: if layout.meeting_room_count > 0 {
                    {
                        result
                            .effect_ledger
                            .value(ControlEffectKey::MEETING_CLUE_SPEED)
                    }
                } else {
                    0.0
                },
                replacement_preference,
                suppressed_replacement_sources,
                redundant_effect_sources: result.effect_ledger.redundant_effect_source_count(),
                redundant_global_recovery_sources: control_global_recovery_redundancy(
                    global_recovery,
                ),
            })
        })
        .collect();
    let evaluate_elapsed = evaluate_started.elapsed();

    let sort_started = Instant::now();
    if !options.candidate_requirements.is_empty() {
        hits.retain(|hit| {
            options
                .candidate_requirements
                .iter()
                .all(|(candidates, min_count)| {
                    hit.hit
                        .names
                        .iter()
                        .filter(|name| candidates.contains(*name))
                        .count()
                        >= usize::from(*min_count)
                })
        });
    }

    let combinations = combinations_count;
    let accepted_hits = hits.len() as u64;
    hits.sort_by(compare_ranked_control_hits);
    hits.truncate(options.top_k);
    let result: Vec<_> = hits.into_iter().map(|ranked| ranked.hit).collect();
    let sort_topk_elapsed = sort_started.elapsed();
    perf.finish(
        combinations,
        combinations,
        accepted_hits,
        result.len() as u64,
        false,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            evaluate: evaluate_elapsed,
            sort_topk: sort_topk_elapsed,
            ..Default::default()
        },
    );
    Ok(result)
}
