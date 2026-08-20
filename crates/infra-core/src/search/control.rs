use std::collections::HashSet;
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use crate::control::{
    solve_control, solve_control_with_trace, ControlOperator, ControlRoomInput,
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
    replacement_preference: i32,
    suppressed_replacement_sources: usize,
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
    key(b)
        .total_cmp(&key(a))
        .then_with(|| b.replacement_preference.cmp(&a.replacement_preference))
        .then_with(|| {
            a.suppressed_replacement_sources
                .cmp(&b.suppressed_replacement_sources)
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
            match atom.selector.as_ref() {
                Some(
                    Selector::TaggedCountInTradeSum { tag }
                    | Selector::TaggedCountInCurrentTradeRoom { tag }
                    | Selector::TradeStationsWithTaggedGte { tag, .. },
                ) => {
                    tags.insert(tag.clone());
                }
                _ => {}
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
            match atom.selector.as_ref() {
                Some(
                    Selector::TaggedCountInTradeSum { tag }
                    | Selector::TaggedCountInCurrentTradeRoom { tag }
                    | Selector::TradeStationsWithTaggedGte { tag, .. },
                ) => {
                    tags.insert(tag.clone());
                }
                _ => {}
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

#[derive(Debug, Clone, Copy, Default)]
struct ControlFillTierScores {
    loose_piece: f64,
    mood: f64,
    clue: f64,
}

impl ControlFillTierScores {
    fn layered_sort_bonus(self) -> f64 {
        // 散件与 +7/+2 同层按数值比较；心情和线索只在高层同分时补空。
        self.loose_piece + self.mood * 0.01 + self.clue * 0.0001
    }
}

/// 体系外中枢补位分层：散件（单走八幡海铃）→ 心情（EW / 玛恩纳）→ 线索。
fn control_layered_fill_scores(
    operators: &[ControlOperator],
    table: &SkillTable,
) -> ControlFillTierScores {
    let mut scores = ControlFillTierScores::default();
    for op in operators {
        for bid in &op.buff_ids {
            match bid.as_str() {
                // 跨设施入口本身不预支人工估值；实际收益由完整编制的 solver 结算。
                "control_tra_limit&spd[000]" | "control_hire_spd&bd[000]" => {}
                // 中枢内全员心情 +0.05/h；EW / 玛恩纳等也归心情层。
                "control_mp_cost[007]"
                | "control_mp_cost[010]"
                | "control_mp_cost[012]"
                | "control_mp_psk[000]"
                | "control_mp_lonely[000]"
                | "control_mp_expand_double[000]" => scores.mood += 5.0,
                // 宿舍全员心情 +0.05/h（低于中枢内恢复）
                "control_dorm_rec2[000]" => scores.mood += 2.0,
                _ => {
                    let Some(skill) = table.get(bid) else {
                        continue;
                    };
                    if skill.facility != "control" || !skill.atoms.is_empty() {
                        continue;
                    }
                    if bid.starts_with("control_mp_cost[") && !bid.contains('&') {
                        scores.mood += 5.0;
                    } else if control_clue_buff(bid) {
                        scores.clue += 1.0;
                    }
                }
            };
        }
    }
    scores
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
    operators: &[ControlOperator],
    table: &SkillTable,
    options: &ControlSearchOptions,
) -> ControlPolicyBreakdown {
    let mood_penalty: f64 = result
        .operator_mood_drains
        .values()
        .filter(|v| **v > 0.0)
        .sum();

    if options.fill_policy == ControlFillPolicy::LayeredFill {
        let fill_scores = control_layered_fill_scores(operators, table);
        let trade_inject = result
            .inject
            .trade_eff_pct_with_tag_counts(&options.layout.trade_tagged_count_sum);
        let manu_gold = result.inject.manu_eff_for(RecipeKind::Gold);
        let manu_br = result.inject.manu_eff_for(RecipeKind::BattleRecord);
        let component_score = evaluate_control_inject_policy(TradeManuEfficiencyComponents {
            trade_eff_pct: trade_inject,
            gold_manu_eff_pct: manu_gold,
            battle_record_manu_eff_pct: manu_br,
            trade_station_count: options.layout.trade_station_count,
            gold_line_count: options.layout.gold_manu_line_count.min(u32::from(u8::MAX)) as u8,
            battle_record_line_count: options
                .layout
                .manufacture_station_count
                .saturating_sub(options.layout.gold_manu_line_count.min(u32::from(u8::MAX)) as u8),
        });
        return ControlPolicyBreakdown {
            policy: component_score.policy,
            trade_inject_pct: trade_inject,
            manu_gold_inject_pct: manu_gold,
            manu_br_inject_pct: manu_br,
            inject_subtotal: component_score.sort_key_pct,
            loose_piece_sort_component: fill_scores.loose_piece,
            mood_fill_sort_component: fill_scores.mood,
            clue_fill_sort_component: fill_scores.clue,
            mood_penalty,
            policy_sort_key: component_score.sort_key_pct + fill_scores.layered_sort_bonus(),
            ..ControlPolicyBreakdown::default()
        };
    }

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

/// 中枢满编组合搜索；可用人数不足上限时使用全部可用角色。
pub fn search_control_combos(
    pool: &ControlPool,
    table: &SkillTable,
    options: &ControlSearchOptions,
) -> Result<Vec<ControlSearchHit>> {
    // Skill-less operators are equivalent for control output. Keep only enough
    // stable-name fallbacks to fill the room, avoiding C(account_size, 5).
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
        return Ok(Vec::new());
    }

    let start = Instant::now();
    let max_k = options.max_operators.min(5).min(n as u8) as usize;
    let required_indices: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| options.must_include.contains(&entry.name))
        .map(|(index, _)| index)
        .collect();
    if required_indices.len() != options.must_include.len() || required_indices.len() > max_k {
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

    let layout = options.layout.clone();
    let mood = options.mood;

    let mut hits: Vec<RankedControlSearchHit> = combos
        .par_iter()
        .filter_map(|idxs| {
            let operators: Vec<_> = idxs
                .iter()
                .map(|i| entries[*i].to_control_operator())
                .collect();
            let mut names: Vec<String> = operators.iter().map(|o| o.name.clone()).collect();
            names.sort();
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
            let breakdown = evaluate_control_policy(&result, &operators, table, options);
            Some(RankedControlSearchHit {
                hit: ControlSearchHit {
                    trade_inject_pct: result
                        .inject
                        .trade_eff_pct_with_tag_counts(&layout.trade_tagged_count_sum),
                    manu_gold_inject_pct: result.inject.manu_eff_for(RecipeKind::Gold),
                    names,
                    breakdown,
                },
                replacement_preference,
                suppressed_replacement_sources,
            })
        })
        .collect();

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

    hits.sort_by(compare_ranked_control_hits);
    hits.truncate(options.top_k);
    let _elapsed = start.elapsed();
    Ok(hits.into_iter().map(|ranked| ranked.hit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{ControlOperator, ControlRoomInput, ControlTraceEvent};
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::pool::build_control_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn monhun_control_ops(
        table: &SkillTable,
    ) -> (crate::control::ControlCenterResult, Vec<ControlOperator>) {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("火龙S黑角", 2), ("麒麟R夜刀", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, table).unwrap();
        let ops: Vec<ControlOperator> = ["火龙S黑角", "麒麟R夜刀"]
            .iter()
            .map(|n| pool.entry(n).unwrap().to_control_operator())
            .collect();
        let result = solve_control(
            &ControlRoomInput {
                operators: ops.clone(),
                mood: 24.0,
                layout: LayoutContext::default(),
            },
            table,
        );
        (result, ops)
    }

    #[test]
    fn producer_subset_upper_bound_covers_every_dynamic_scope() {
        let blueprint = crate::layout::BaseBlueprint {
            template: None,
            drone_cap: 0,
            scenario: crate::layout::BlueprintScenario::default(),
            rooms: vec![
                crate::layout::RoomBlueprint {
                    id: crate::layout::RoomId::from("trade"),
                    kind: crate::layout::FacilityKind::TradePost,
                    level: 3,
                    product: Some(crate::layout::RoomProduct::Trade {
                        order: crate::trade::input::TradeOrderKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                crate::layout::RoomBlueprint {
                    id: crate::layout::RoomId::from("gold"),
                    kind: crate::layout::FacilityKind::Factory,
                    level: 3,
                    product: Some(crate::layout::RoomProduct::Factory {
                        recipe: RecipeKind::Gold,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                crate::layout::RoomBlueprint {
                    id: crate::layout::RoomId::from("battle"),
                    kind: crate::layout::FacilityKind::Factory,
                    level: 3,
                    product: Some(crate::layout::RoomProduct::Factory {
                        recipe: RecipeKind::BattleRecord,
                    }),
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
            ],
        };
        let mut layout = LayoutContext::default();
        layout.global_inject.record_trade_tagged(
            "all",
            "all_buff",
            "all_family",
            "all_tag",
            5.0,
            0,
            crate::global_resource::TradeTaggedCountScope::AllTradeRooms,
        );
        layout.global_inject.record_trade_tagged(
            "current",
            "current_buff",
            "current_family",
            "current_tag",
            10.0,
            0,
            crate::global_resource::TradeTaggedCountScope::CurrentTradeRoom,
        );
        layout.global_inject.record_trade_tagged(
            "threshold",
            "threshold_buff",
            "threshold_family",
            "threshold_tag",
            10.0,
            0,
            crate::global_resource::TradeTaggedCountScope::QualifiedTradeRooms { min: 3 },
        );
        layout.global_inject.record_manu_count_scaled(
            "count",
            "count_buff",
            "count_family",
            "count_tag",
            None,
            5.0,
            0,
        );
        layout
            .global_inject
            .record_manu_tagged("tagged_buff", "tagged_tag", None, 7.0);

        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        assert_eq!(
            control_inject_policy_sort_key_upper_bound(&layout, &blueprint, &table),
            157.0
        );
    }

    #[test]
    fn producer_subset_pruning_disables_for_unresolved_support_inputs() {
        let blueprint = crate::layout::BaseBlueprint::template_243_use_this().unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        for buff_id in ["control_token_prod_spd[000]", "control_mp_bd&trade[000]"] {
            let mut layout = LayoutContext::default();
            layout.global_inject.record_active_source_buff(buff_id);
            assert!(
                control_inject_policy_sort_key_upper_bound(&layout, &blueprint, &table)
                    .is_infinite(),
                "{buff_id} reads support/state that is unresolved before the full prefix"
            );
        }
    }

    #[test]
    fn control_search_uses_named_policy_sort_key() {
        let hit = ControlSearchHit {
            names: vec!["a".into()],
            trade_inject_pct: 7.0,
            manu_gold_inject_pct: 5.0,
            breakdown: ControlPolicyBreakdown {
                policy: ScoringPolicyId::ControlInjectRawSumV0,
                policy_sort_key: 12.0,
                inject_subtotal: 12.0,
                ..Default::default()
            },
        };
        assert_eq!(control_inject_sort_key(&hit), 12.0);
        assert_eq!(hit.breakdown.policy, ScoringPolicyId::ControlInjectRawSumV0);
        let json = serde_json::to_value(&hit).unwrap();
        assert_eq!(
            json["breakdown"]["policy"],
            serde_json::json!("ControlInjectRawSumV0")
        );
        assert_eq!(json["breakdown"]["policy_sort_key"], 12.0);
        assert!(json.get("score").is_none());
        assert!(json["breakdown"].get("total_score").is_none());
    }

    #[test]
    fn control_score_is_pure_inject_sum() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let (result, ops) = monhun_control_ops(&table);
        assert!(result.global.get(GlobalResourceKey::Matatabi) > 0.0);

        let without = evaluate_control_policy(
            &result,
            &ops,
            &table,
            &ControlSearchOptions {
                matatabi_consumer_active: false,
                ..Default::default()
            },
        );
        let with = evaluate_control_policy(
            &result,
            &ops,
            &table,
            &ControlSearchOptions {
                matatabi_consumer_active: true,
                ..Default::default()
            },
        );
        // 纯注入评分：matatabi consumer 不影响 score（木天蓼不在中枢评分里预支）
        assert!(
            (with.policy_sort_key - without.policy_sort_key).abs() < 0.001,
            "matatabi 不应影响中枢注入评分: with={} without={}",
            with.policy_sort_key,
            without.policy_sort_key,
        );
        assert!(with.matatabi > 0.0, "matatabi 仍应在 breakdown 中记录");
        assert!(
            result
                .operator_mood_drains
                .get("麒麟R夜刀")
                .copied()
                .unwrap_or(0.0)
                > 0.0,
            "夜刀应记账心情消耗"
        );
    }

    #[test]
    fn control_plugin_fill_excludes_unclaimed_system_producers() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [
                ("阿米娅", 2),
                ("诗怀雅", 2),
                ("八幡海铃", 2),
                ("焰尾", 2),
                ("薇薇安娜", 2),
                ("三角初华", 2),
                ("若叶睦", 2),
                ("丰川祥子", 2),
            ]
            .into_iter()
            .map(|(n, e)| (n.to_string(), e))
            .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        for name in ["阿米娅", "诗怀雅", "八幡海铃", "焰尾", "薇薇安娜"] {
            let entry = pool.entry(name).unwrap();
            assert!(
                control_entry_plugin_fill(entry),
                "{name} should be allowed as control plugin fill"
            );
        }

        for name in ["三角初华", "若叶睦", "丰川祥子"] {
            let entry = pool.entry(name).unwrap();
            assert!(
                !control_entry_plugin_fill(entry),
                "{name} is a system producer and should not be inserted as standalone control fill"
            );
        }
    }

    #[test]
    fn deferred_dynamic_branch_is_driven_by_producer_rule_catalog() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("八幡海铃", 2), ("戴菲恩", 2), ("凛御银灰", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();
        assert_eq!(
            control_entry_deferred_trade_tags(pool.entry("八幡海铃").unwrap(), &table).unwrap(),
            HashSet::from(["cc.g.siracusa".to_string()])
        );
        assert_eq!(
            control_entry_deferred_trade_tags(pool.entry("戴菲恩").unwrap(), &table).unwrap(),
            HashSet::from(["cc.g.glasgow".to_string()])
        );
        assert_eq!(
            control_entry_deferred_trade_tags(pool.entry("凛御银灰").unwrap(), &table).unwrap(),
            HashSet::from(["cc.g.karlan".to_string()])
        );
    }

    #[test]
    fn control_layered_fill_prefers_efficiency_piece_over_mood() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("Mon3tr", 2), ("玛恩纳", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();
        assert!(control_entry_plugin_fill(pool.entry("Mon3tr").unwrap()));
        assert!(control_entry_plugin_fill(pool.entry("玛恩纳").unwrap()));

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 1,
                top_k: 1,
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(hits[0].names, vec!["Mon3tr".to_string()]);
        assert!(
            hits[0].breakdown.inject_subtotal > hits[0].breakdown.mood_fill_sort_component * 0.01,
            "efficiency pieces should outrank pure mood fill: {:?}",
            hits[0].breakdown
        );
    }

    #[test]
    fn control_search_prefers_declared_replacements_on_equal_outputs() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("Mon3tr", 2), ("凯尔希", 2), ("阿斯卡纶", 2), ("明椒", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 2,
                top_k: 1,
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            hits[0].names,
            vec!["Mon3tr".to_string(), "阿斯卡纶".to_string()]
        );
    }

    #[test]
    fn control_replacement_preference_requires_effective_family_representatives() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let preference_for = |operators: &[(&str, u8)]| {
            let roster = Roster::from_elite_map(
                operators
                    .iter()
                    .map(|(name, elite)| ((*name).to_string(), *elite))
                    .collect(),
            );
            let pool = build_control_pool(&roster, &instances, &table).unwrap();
            let input = ControlRoomInput::with_operators(
                operators
                    .iter()
                    .map(|(name, _)| pool.entry(name).unwrap().to_control_operator())
                    .collect(),
            );
            let (_, trace) = solve_control_with_trace(&input, &table);
            control_replacement_rank(&trace)
        };

        assert_eq!(preference_for(&[("Mon3tr", 2)]), (2, 0));
        assert_eq!(preference_for(&[("Mon3tr", 0)]), (0, 0));
        assert_eq!(
            preference_for(&[("斩业星熊", 2), ("Mon3tr", 2), ("诗怀雅", 0)]),
            (0, 1),
            "suppressed Mon3tr inject must not receive replacement preference"
        );
        assert_eq!(
            preference_for(&[("Mon3tr", 2), ("凯尔希", 2)]),
            (0, 0),
            "tied family representatives retain their opposing stable preferences"
        );
        assert_eq!(
            preference_for(&[("阿斯卡纶", 2), ("明椒", 2)]),
            (0, 0),
            "equal trade-family representatives retain their opposing stable preferences"
        );
    }

    #[test]
    fn control_search_excludes_a_replacement_source_suppressed_by_a_stronger_family_winner() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            ["Mon3tr", "斩业星熊", "诗怀雅", "玛恩纳", "维什戴尔"]
                .into_iter()
                .map(|name| (name.to_string(), 2))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 4,
                top_k: 1,
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(hits[0].manu_gold_inject_pct, 3.0);
        assert!(hits[0].names.iter().any(|name| name == "斩业星熊"));
        assert!(hits[0].names.iter().any(|name| name == "诗怀雅"));
        assert!(!hits[0].names.iter().any(|name| name == "Mon3tr"));
    }

    fn ranked_hit_for_test(
        name: &str,
        policy_sort_key: f64,
        replacement_preference: i32,
        suppressed_replacement_sources: usize,
    ) -> RankedControlSearchHit {
        RankedControlSearchHit {
            hit: ControlSearchHit {
                names: vec![name.to_string()],
                trade_inject_pct: 0.0,
                manu_gold_inject_pct: 0.0,
                breakdown: ControlPolicyBreakdown {
                    policy_sort_key,
                    ..Default::default()
                },
            },
            replacement_preference,
            suppressed_replacement_sources,
        }
    }

    #[test]
    fn control_comparator_is_total_and_puts_nan_in_the_lowest_score_tier() {
        let high = ranked_hit_for_test("high", 10.0, 0, 0);
        let low = ranked_hit_for_test("low", 1.0, 0, 0);
        let nan = ranked_hit_for_test("nan", f64::NAN, 0, 0);

        assert_eq!(
            compare_ranked_control_hits(&high, &low),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_ranked_control_hits(&low, &nan),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_ranked_control_hits(&high, &nan),
            std::cmp::Ordering::Less
        );
        for (left, right) in [(&high, &low), (&low, &nan), (&high, &nan)] {
            assert_eq!(
                compare_ranked_control_hits(left, right),
                compare_ranked_control_hits(right, left).reverse()
            );
        }

        let mut hits = vec![nan, high, low];
        hits.sort_by(compare_ranked_control_hits);
        assert_eq!(
            hits.into_iter()
                .map(|hit| hit.hit.names[0].clone())
                .collect::<Vec<_>>(),
            vec!["high".to_string(), "low".to_string(), "nan".to_string()]
        );
    }

    #[test]
    fn control_comparator_secondary_keys_are_transitive() {
        let candidates = vec![
            ranked_hit_for_test("a", 10.0, 2, 0),
            ranked_hit_for_test("b", 10.0, 0, 0),
            ranked_hit_for_test("c", 10.0, 0, 1),
            ranked_hit_for_test("d", 10.0, 0, 1),
            ranked_hit_for_test("e", 9.0, 2, 0),
            ranked_hit_for_test("f", f64::NAN, 99, 0),
        ];
        for a in &candidates {
            for b in &candidates {
                for c in &candidates {
                    let ab = compare_ranked_control_hits(a, b);
                    let bc = compare_ranked_control_hits(b, c);
                    if ab != std::cmp::Ordering::Greater && bc != std::cmp::Ordering::Greater {
                        assert_ne!(
                            compare_ranked_control_hits(a, c),
                            std::cmp::Ordering::Greater,
                            "comparator must be transitive"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn control_replacement_provenance_does_not_mix_winner_and_suppressed_atoms() {
        let replacement = CONTROL_REPLACEMENT_RULES[0];
        let event = |status| ControlTraceEvent {
            source_operator: "Mon3tr".to_string(),
            source_buff_id: replacement.source_buff_id.to_string(),
            phase: Phase::GlobalInject,
            phase_order: 10,
            condition: None,
            condition_status: crate::control::ControlConditionStatus::Passed,
            selector: None,
            action: Action::GlobalInjectManuEff {
                value: 2.0,
                recipe: None,
            },
            outcome: ControlTraceOutcome::ManufactureInject {
                family: replacement.family.to_string(),
                recipe: None,
                candidate_value: 2.0,
                candidate_kind: crate::global_resource::ManuInjectCandidateKind::Static,
                merge: Some(crate::global_resource::InjectMergeExplanation {
                    status,
                    effective_value: 2.0,
                }),
            },
        };
        let trace = ControlSettlementTrace {
            events: vec![
                event(InjectMergeStatus::Winner),
                event(InjectMergeStatus::Suppressed),
            ],
            final_state: Vec::new(),
        };

        assert_eq!(
            control_replacement_status(&trace, replacement),
            (true, true)
        );
        assert_eq!(control_replacement_rank(&trace), (0, 1));
    }

    #[test]
    fn control_layered_fill_allows_haru_as_loose_piece_but_not_resource_producer() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("八幡海铃", 0), ("三角初华", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        assert!(control_entry_plugin_fill(pool.entry("八幡海铃").unwrap()));
        assert!(!control_entry_plugin_fill(pool.entry("三角初华").unwrap()));
    }

    #[test]
    fn control_search_fills_to_capacity_when_pool_is_large_enough() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            ["阿米娅", "诗怀雅", "玛恩纳", "Mon3tr", "明椒", "焰尾"]
                .into_iter()
                .map(|name| (name.to_string(), 2))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 5,
                top_k: 20,
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit.names.len() == 5));
    }

    #[test]
    fn control_search_enforces_candidate_count_without_list_order_pick() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            ["重岳", "令", "Mon3tr", "玛恩纳", "阿米娅", "诗怀雅"]
                .into_iter()
                .map(|name| (name.to_string(), 2))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();
        let candidates = HashSet::from(["重岳".to_string(), "令".to_string()]);

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 5,
                top_k: 20,
                candidate_requirements: vec![(candidates.clone(), 1)],
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| {
            hit.names
                .iter()
                .filter(|name| candidates.contains(*name))
                .count()
                >= 1
        }));
    }

    #[test]
    fn control_search_uses_every_operator_when_pool_is_under_capacity() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            ["阿米娅", "诗怀雅", "玛恩纳"]
                .into_iter()
                .map(|name| (name.to_string(), 2))
                .collect(),
        );
        let pool = build_control_pool(&roster, &instances, &table).unwrap();

        let hits = search_control_combos(
            &pool,
            &table,
            &ControlSearchOptions {
                max_operators: 5,
                top_k: 20,
                fill_policy: ControlFillPolicy::LayeredFill,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].names.len(), 3);
    }
}
