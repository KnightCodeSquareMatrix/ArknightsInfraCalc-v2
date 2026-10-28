use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use crate::types::RecipeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectMergeStatus {
    Winner,
    TiedWinner,
    Suppressed,
    Accumulated,
    NoEffect,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InjectMergeExplanation {
    pub status: InjectMergeStatus,
    pub effective_value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuInjectCandidateKind {
    Static,
    CountScaled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManuInjectCandidate {
    pub kind: ManuInjectCandidateKind,
    pub value: f64,
}

/// 灵知·精密计算：每名同贸易房谢拉格干员的订单效率/上限增量规则。
/// 控制域只携带系数；最终值在贸易域按房间内 `cc.g.karlan` 计数结算。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct KarlanPrecision {
    pub eff_per_karlan: f64,
    pub limit_per_karlan: i32,
}

/// 中枢 `GlobalInject` 汇总：同 `inject_family` 取最高，不同族相加。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GlobalInjectManifest {
    trade_by_family: HashMap<String, f64>,
    manu_all_by_family: HashMap<String, f64>,
    manu_gold_by_family: HashMap<String, f64>,
    manu_battle_record_by_family: HashMap<String, f64>,
    manu_originium_by_family: HashMap<String, f64>,
    trade_tagged: Vec<TaggedTradeInject>,
    manu_tagged: Vec<TaggedManuInject>,
    manu_count_scaled: Vec<TaggedManuCountInject>,
    /// 当前中枢编制实际激活的机制 buff；供 L3 capability gate 使用。
    active_source_buffs: BTreeSet<String>,
    /// 灵知·精密计算规则（单一中枢，不与自身叠加）。
    karlan_precision: Option<KarlanPrecision>,
    /// 同一实例按配方复用首次原算法结算值；制造输入变更时全槽失效。
    #[serde(skip)]
    manu_eff_cache: [OnceLock<f64>; 4],
}

impl PartialEq for GlobalInjectManifest {
    fn eq(&self, other: &Self) -> bool {
        self.trade_by_family == other.trade_by_family
            && self.manu_all_by_family == other.manu_all_by_family
            && self.manu_gold_by_family == other.manu_gold_by_family
            && self.manu_battle_record_by_family == other.manu_battle_record_by_family
            && self.manu_originium_by_family == other.manu_originium_by_family
            && self.trade_tagged == other.trade_tagged
            && self.manu_tagged == other.manu_tagged
            && self.manu_count_scaled == other.manu_count_scaled
            && self.active_source_buffs == other.active_source_buffs
            && self.karlan_precision == other.karlan_precision
    }
}

/// 中枢按贸易站实际标签人数结算的延迟注入规则。
///
/// 控制中枢先于贸易站搜索，因此不能在中枢求值时把标签人数冻结为 0。
/// `resolved_count` 只保存当前完整 layout 的展示快照；最终贸易房结算通过
/// [`GlobalInjectManifest::trade_eff_pct_with_scoped_tag_counts`] 分别提供全站与当前房计数。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaggedTradeInject {
    pub source_operator: String,
    pub source_buff_id: String,
    pub family: String,
    pub target_tag: String,
    pub value_per_operator: f64,
    pub resolved_count: u8,
    pub count_scope: TradeTaggedCountScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TradeTaggedCountScope {
    AllTradeRooms,
    CurrentTradeRoom,
    QualifiedTradeRooms { min: u8 },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaggedManuInject {
    pub source_buff_id: String,
    pub target_tag: String,
    pub recipe: Option<RecipeKind>,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaggedManuCountInject {
    pub source_operator: String,
    pub source_buff_id: String,
    pub family: String,
    pub target_tag: String,
    pub recipe: Option<RecipeKind>,
    pub value_per_operator: f64,
    pub resolved_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManuInjectSlot {
    All,
    Gold,
    BattleRecord,
    Originium,
}

impl GlobalInjectManifest {
    /// Bake 的静态基线门禁：保留全部业务字段、浮点 bit 和实际求和遍历顺序。
    /// 这里只比较输入，不改变注入合并或求和机制。
    pub(crate) fn exact_bake_compatible(&self, other: &Self) -> bool {
        let same_float =
            |a: f64, b: f64| a.is_finite() && b.is_finite() && a.to_bits() == b.to_bits();
        let same_map = |a: &HashMap<String, f64>, b: &HashMap<String, f64>| {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|((ak, av), (bk, bv))| ak == bk && same_float(*av, *bv))
        };
        self == other
            && same_map(&self.trade_by_family, &other.trade_by_family)
            && same_map(&self.manu_all_by_family, &other.manu_all_by_family)
            && same_map(&self.manu_gold_by_family, &other.manu_gold_by_family)
            && same_map(
                &self.manu_battle_record_by_family,
                &other.manu_battle_record_by_family,
            )
            && same_map(
                &self.manu_originium_by_family,
                &other.manu_originium_by_family,
            )
            && self
                .trade_tagged
                .iter()
                .zip(&other.trade_tagged)
                .all(|(a, b)| same_float(a.value_per_operator, b.value_per_operator))
            && self
                .manu_tagged
                .iter()
                .zip(&other.manu_tagged)
                .all(|(a, b)| same_float(a.value, b.value))
            && self
                .manu_count_scaled
                .iter()
                .zip(&other.manu_count_scaled)
                .all(|(a, b)| same_float(a.value_per_operator, b.value_per_operator))
            && match (self.karlan_precision, other.karlan_precision) {
                (Some(a), Some(b)) => same_float(a.eff_per_karlan, b.eff_per_karlan),
                (None, None) => true,
                _ => false,
            }
    }

    pub fn trade_eff_pct(&self) -> f64 {
        self.trade_eff_pct_by(|rule| rule.resolved_count)
    }

    /// 以调用方提供的贸易标签人数结算动态规则；同 `family` 仍取最高，
    /// 不同族相加，与静态 `record_trade` 语义一致。
    pub fn trade_eff_pct_with_tag_counts(&self, counts: &HashMap<String, u8>) -> f64 {
        self.trade_eff_pct_with_scoped_tag_counts(counts, &HashMap::new())
    }

    pub fn trade_eff_pct_with_scoped_tag_counts(
        &self,
        all_trade_counts: &HashMap<String, u8>,
        current_room_counts: &HashMap<String, u8>,
    ) -> f64 {
        self.trade_eff_pct_with_current_room_count(all_trade_counts, |tag| {
            current_room_counts.get(tag).copied().unwrap_or(0)
        })
    }

    /// Resolve current-room counts without constructing a per-room tag map.
    pub fn trade_eff_pct_with_current_room_count(
        &self,
        all_trade_counts: &HashMap<String, u8>,
        current_room_count: impl Fn(&str) -> u8,
    ) -> f64 {
        self.trade_eff_pct_by(|rule| match rule.count_scope {
            TradeTaggedCountScope::AllTradeRooms => {
                all_trade_counts.get(&rule.target_tag).copied().unwrap_or(0)
            }
            TradeTaggedCountScope::CurrentTradeRoom => current_room_count(&rule.target_tag),
            TradeTaggedCountScope::QualifiedTradeRooms { .. } => rule.resolved_count,
        })
    }

    /// Return one trade injection family using the same scoped count rules as
    /// [`Self::trade_eff_pct_with_scoped_tag_counts`].
    pub fn trade_eff_pct_for_family_with_scoped_tag_counts(
        &self,
        family: &str,
        all_trade_counts: &HashMap<String, u8>,
        current_room_counts: &HashMap<String, u8>,
    ) -> f64 {
        self.trade_eff_pct_for_family_with_current_room_count(family, all_trade_counts, |tag| {
            current_room_counts.get(tag).copied().unwrap_or(0)
        })
    }

    pub fn trade_eff_pct_for_family_with_current_room_count(
        &self,
        family: &str,
        all_trade_counts: &HashMap<String, u8>,
        current_room_count: impl Fn(&str) -> u8,
    ) -> f64 {
        let static_value = self.trade_by_family.get(family).copied().unwrap_or(0.0);
        self.trade_tagged
            .iter()
            .filter(|rule| rule.family == family)
            .map(|rule| match rule.count_scope {
                TradeTaggedCountScope::AllTradeRooms => {
                    all_trade_counts.get(&rule.target_tag).copied().unwrap_or(0)
                }
                TradeTaggedCountScope::CurrentTradeRoom => current_room_count(&rule.target_tag),
                TradeTaggedCountScope::QualifiedTradeRooms { .. } => rule.resolved_count,
            })
            .map(f64::from)
            .zip(
                self.trade_tagged
                    .iter()
                    .filter(|rule| rule.family == family)
                    .map(|rule| rule.value_per_operator),
            )
            .map(|(count, value)| count * value)
            .fold(static_value, f64::max)
    }

    fn trade_eff_pct_by(&self, count_of: impl Fn(&TaggedTradeInject) -> u8) -> f64 {
        if self.trade_tagged.is_empty() {
            return self.trade_by_family.values().sum();
        }
        let mut by_family = self.trade_by_family.clone();
        for rule in &self.trade_tagged {
            let value = f64::from(count_of(rule)) * rule.value_per_operator;
            let entry = by_family.entry(rule.family.clone()).or_insert(0.0);
            *entry = entry.max(value);
        }
        by_family.values().sum()
    }

    pub fn explain_resolved_trade_candidate(
        &self,
        family: &str,
        candidate_value: f64,
        tied: bool,
    ) -> InjectMergeExplanation {
        let mut effective_value = self.trade_by_family.get(family).copied().unwrap_or(0.0);
        for rule in self
            .trade_tagged
            .iter()
            .filter(|rule| rule.family == family)
        {
            effective_value =
                effective_value.max(f64::from(rule.resolved_count) * rule.value_per_operator);
        }
        InjectMergeExplanation {
            status: max_merge_status(candidate_value, effective_value, tied),
            effective_value,
        }
    }

    pub fn trade_global_flat_eff_pct(&self) -> f64 {
        self.trade_by_family
            .get(INJECT_FAMILY_TRADE_GLOBAL_FLAT)
            .copied()
            .unwrap_or(0.0)
    }

    pub fn manu_global_flat_eff_pct(&self) -> f64 {
        let value = self
            .manu_all_by_family
            .get(INJECT_FAMILY_MANU_GLOBAL_ALL)
            .copied()
            .unwrap_or(0.0);
        // The generic control-center effect is +2%. Hoshiguma the
        // Breacher's conditional +3% occupies the same max-of-family slot,
        // but is a cross-operator skill and must remain in displayed output.
        if (value - 2.0).abs() < f64::EPSILON {
            value
        } else {
            0.0
        }
    }

    pub fn manu_eff_for(&self, recipe: RecipeKind) -> f64 {
        let index = match recipe {
            RecipeKind::All => 0,
            RecipeKind::Gold => 1,
            RecipeKind::BattleRecord => 2,
            RecipeKind::Originium => 3,
        };
        *self.manu_eff_cache[index].get_or_init(|| self.manu_eff_for_uncached(recipe))
    }

    fn manu_eff_for_uncached(&self, recipe: RecipeKind) -> f64 {
        let mut all_by_family = self.manu_all_by_family.clone();
        let mut recipe_by_family = match recipe {
            RecipeKind::All => HashMap::new(),
            RecipeKind::Gold => self.manu_gold_by_family.clone(),
            RecipeKind::BattleRecord => self.manu_battle_record_by_family.clone(),
            RecipeKind::Originium => self.manu_originium_by_family.clone(),
        };
        for rule in &self.manu_count_scaled {
            let value = f64::from(rule.resolved_count) * rule.value_per_operator;
            let map = if rule.recipe.is_none_or(|target| target == RecipeKind::All) {
                &mut all_by_family
            } else if rule.recipe == Some(recipe) {
                &mut recipe_by_family
            } else {
                continue;
            };
            let entry = map.entry(rule.family.clone()).or_insert(0.0);
            *entry = entry.max(value);
        }
        all_by_family.values().sum::<f64>() + recipe_by_family.values().sum::<f64>()
    }

    pub fn explain_manu_candidates(
        candidates: &[ManuInjectCandidate],
    ) -> Vec<InjectMergeExplanation> {
        let effective_value = settle_manu_candidates(candidates, &[]);
        let static_value = settle_static_candidates(candidates, &[]);
        candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let status = if candidate.value == 0.0 {
                    InjectMergeStatus::NoEffect
                } else {
                    match candidate.kind {
                        ManuInjectCandidateKind::Static if candidate.value < 0.0 => {
                            if (settle_manu_candidates(candidates, &[index]) - effective_value)
                                .abs()
                                >= f64::EPSILON
                            {
                                InjectMergeStatus::Accumulated
                            } else {
                                InjectMergeStatus::Suppressed
                            }
                        }
                        ManuInjectCandidateKind::Static => {
                            if (settle_manu_candidates(candidates, &[index]) - effective_value)
                                .abs()
                                >= f64::EPSILON
                            {
                                InjectMergeStatus::Winner
                            } else {
                                let tie_group = candidates
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(other_index, other)| {
                                        (other.kind == ManuInjectCandidateKind::Static
                                            && (other.value - candidate.value).abs() < f64::EPSILON)
                                            .then_some(other_index)
                                    })
                                    .collect::<Vec<_>>();
                                if tie_group.len() > 1
                                    && tie_group.iter().all(|member_index| {
                                        (settle_manu_candidates(candidates, &[*member_index])
                                            - effective_value)
                                            .abs()
                                            < f64::EPSILON
                                    })
                                    && (settle_manu_candidates(candidates, &tie_group)
                                        - effective_value)
                                        .abs()
                                        >= f64::EPSILON
                                {
                                    InjectMergeStatus::TiedWinner
                                } else {
                                    InjectMergeStatus::Suppressed
                                }
                            }
                        }
                        ManuInjectCandidateKind::CountScaled => {
                            let max_count_scaled = candidates
                                .iter()
                                .filter(|other| other.kind == ManuInjectCandidateKind::CountScaled)
                                .map(|other| other.value)
                                .fold(static_value, f64::max);
                            if candidate.value >= static_value
                                && (candidate.value - max_count_scaled).abs() < f64::EPSILON
                            {
                                let ties = candidates
                                    .iter()
                                    .filter(|other| {
                                        other.kind == ManuInjectCandidateKind::CountScaled
                                            && (other.value - candidate.value).abs() < f64::EPSILON
                                    })
                                    .count();
                                if ties > 1 {
                                    InjectMergeStatus::TiedWinner
                                } else {
                                    InjectMergeStatus::Winner
                                }
                            } else {
                                InjectMergeStatus::Suppressed
                            }
                        }
                    }
                };
                InjectMergeExplanation {
                    status,
                    effective_value,
                }
            })
            .collect()
    }

    pub fn record_trade(&mut self, family: &str, value: f64) {
        let entry = self
            .trade_by_family
            .entry(family.to_string())
            .or_insert(0.0);
        *entry = entry.max(value);
    }

    pub fn record_trade_tagged(
        &mut self,
        source_operator: &str,
        source_buff_id: &str,
        family: &str,
        target_tag: &str,
        value_per_operator: f64,
        resolved_count: u8,
        count_scope: TradeTaggedCountScope,
    ) {
        self.trade_tagged.push(TaggedTradeInject {
            source_operator: source_operator.to_string(),
            source_buff_id: source_buff_id.to_string(),
            family: family.to_string(),
            target_tag: target_tag.to_string(),
            value_per_operator,
            resolved_count,
            count_scope,
        });
    }

    pub fn trade_tagged(&self) -> &[TaggedTradeInject] {
        &self.trade_tagged
    }

    pub fn refresh_qualified_trade_counts(&mut self, counts: &HashMap<String, u8>) {
        for rule in &mut self.trade_tagged {
            let TradeTaggedCountScope::QualifiedTradeRooms { min } = rule.count_scope else {
                continue;
            };
            let key = crate::layout::trade_station_tagged_gte_key(&rule.target_tag, min);
            rule.resolved_count = counts.get(&key).copied().unwrap_or(0);
        }
    }

    pub fn has_dynamic_trade_inject(&self) -> bool {
        !self.trade_tagged.is_empty()
    }

    pub fn record_manu(&mut self, family: &str, recipe: Option<RecipeKind>, value: f64) {
        self.invalidate_manu_eff_cache();
        let map = match manu_inject_slot(recipe) {
            ManuInjectSlot::All => &mut self.manu_all_by_family,
            ManuInjectSlot::Gold => &mut self.manu_gold_by_family,
            ManuInjectSlot::BattleRecord => &mut self.manu_battle_record_by_family,
            ManuInjectSlot::Originium => &mut self.manu_originium_by_family,
        };
        let entry = map.entry(family.to_string()).or_insert(0.0);
        if value >= 0.0 {
            *entry = entry.max(value);
        } else {
            *entry += value;
        }
    }

    pub fn record_manu_tagged(
        &mut self,
        source_buff_id: &str,
        target_tag: &str,
        recipe: Option<RecipeKind>,
        value: f64,
    ) {
        self.manu_tagged.push(TaggedManuInject {
            source_buff_id: source_buff_id.to_string(),
            target_tag: target_tag.to_string(),
            recipe,
            value,
        });
    }

    pub fn manu_tagged(&self) -> &[TaggedManuInject] {
        &self.manu_tagged
    }

    pub fn record_manu_count_scaled(
        &mut self,
        source_operator: &str,
        source_buff_id: &str,
        family: &str,
        target_tag: &str,
        recipe: Option<RecipeKind>,
        value_per_operator: f64,
        resolved_count: u8,
    ) {
        self.invalidate_manu_eff_cache();
        self.manu_count_scaled.push(TaggedManuCountInject {
            source_operator: source_operator.to_string(),
            source_buff_id: source_buff_id.to_string(),
            family: family.to_string(),
            target_tag: target_tag.to_string(),
            recipe,
            value_per_operator,
            resolved_count,
        });
    }

    pub fn manu_count_scaled(&self) -> &[TaggedManuCountInject] {
        &self.manu_count_scaled
    }

    pub fn refresh_manu_count_scaled(&mut self, counts: &HashMap<String, u8>) {
        self.invalidate_manu_eff_cache();
        for rule in &mut self.manu_count_scaled {
            rule.resolved_count = counts.get(&rule.target_tag).copied().unwrap_or(0);
        }
    }

    fn invalidate_manu_eff_cache(&mut self) {
        for cached in &mut self.manu_eff_cache {
            cached.take();
        }
    }

    /// 记录灵知·精密计算；单一中枢不叠加，取订单上限增益更大的一条。
    pub fn record_karlan_precision(&mut self, eff_per_karlan: f64, limit_per_karlan: i32) {
        let cand = KarlanPrecision {
            eff_per_karlan,
            limit_per_karlan,
        };
        self.karlan_precision = Some(match self.karlan_precision {
            Some(cur) if cur.limit_per_karlan >= limit_per_karlan => cur,
            _ => cand,
        });
    }

    pub fn karlan_precision(&self) -> Option<KarlanPrecision> {
        self.karlan_precision
    }

    pub fn record_active_source_buff(&mut self, buff_id: &str) {
        self.active_source_buffs.insert(buff_id.to_string());
    }

    pub fn has_active_source_buff(&self, buff_id: &str) -> bool {
        self.active_source_buffs.contains(buff_id)
    }

    pub fn active_source_buffs(&self) -> impl Iterator<Item = &str> {
        self.active_source_buffs.iter().map(String::as_str)
    }
}

fn manu_inject_slot(recipe: Option<RecipeKind>) -> ManuInjectSlot {
    match recipe {
        None | Some(RecipeKind::All) => ManuInjectSlot::All,
        Some(RecipeKind::Gold) => ManuInjectSlot::Gold,
        Some(RecipeKind::BattleRecord) => ManuInjectSlot::BattleRecord,
        Some(RecipeKind::Originium) => ManuInjectSlot::Originium,
    }
}

fn max_merge_status(candidate_value: f64, effective_value: f64, tied: bool) -> InjectMergeStatus {
    if candidate_value == 0.0 {
        return InjectMergeStatus::NoEffect;
    }
    if (candidate_value - effective_value).abs() < f64::EPSILON {
        if tied {
            InjectMergeStatus::TiedWinner
        } else {
            InjectMergeStatus::Winner
        }
    } else {
        InjectMergeStatus::Suppressed
    }
}

fn settle_manu_candidates(candidates: &[ManuInjectCandidate], skipped: &[usize]) -> f64 {
    let mut effective = settle_static_candidates(candidates, skipped);
    for (index, candidate) in candidates.iter().enumerate() {
        if skipped.contains(&index) || candidate.kind != ManuInjectCandidateKind::CountScaled {
            continue;
        }
        effective = effective.max(candidate.value);
    }
    effective
}

fn settle_static_candidates(candidates: &[ManuInjectCandidate], skipped: &[usize]) -> f64 {
    let mut effective = 0.0_f64;
    for (index, candidate) in candidates.iter().enumerate() {
        if skipped.contains(&index) || candidate.kind != ManuInjectCandidateKind::Static {
            continue;
        }
        if candidate.value >= 0.0 {
            effective = effective.max(candidate.value);
        } else {
            effective += candidate.value;
        }
    }
    effective
}

/// 默认注入族：全贸易站 flat %（阿米娅/诗怀雅/明椒/阿斯卡纶等）。
pub const INJECT_FAMILY_TRADE_GLOBAL_FLAT: &str = "trade_global_flat";

/// 默认注入族：全制造站 flat %（凯尔希/Mon3tr 等）。
pub const INJECT_FAMILY_MANU_GLOBAL_ALL: &str = "manu_global_all";
