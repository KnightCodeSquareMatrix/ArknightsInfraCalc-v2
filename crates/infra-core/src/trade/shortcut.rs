use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::global_resource::GlobalInjectManifest;
use crate::skill_table::data_path;
use crate::skill_table::SkillTable;
use crate::trade::input::{TradeOperator, TradeOrderKind};
use crate::trade::order_mechanic::{GoldDistribution, OrderMechanicResult, SpecialOrderKind};
use crate::types::Action;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutTailorTier {
    Regular,
    Alpha,
    Beta,
    Docus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutMatchRule {
    pub kind: String,
    #[serde(default)]
    pub station_trade_pct: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeShortcutEntry {
    pub id: String,
    pub label: String,
    pub trade_pct: f64,
    pub gold_pct: f64,
    #[serde(default = "default_tailor_tier")]
    pub tailor_tier: ShortcutTailorTier,
    #[serde(default)]
    pub r#match: Option<ShortcutMatchRule>,
    /// 公孙工具人表单位贸易产出（L3 产量锚，L2 未展开巫恋核等时使用）。
    #[serde(default)]
    pub unit_trade_anchor: Option<f64>,
    #[serde(default)]
    pub unit_gsl_gold_anchor: Option<f64>,
}

fn default_tailor_tier() -> ShortcutTailorTier {
    ShortcutTailorTier::Regular
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TradeShortcutFile {
    pub entries: Vec<TradeShortcutEntry>,
}

#[derive(Debug, Clone)]
pub struct TradeShortcutMatch {
    pub entry: TradeShortcutEntry,
}

pub fn load_trade_shortcuts(path: &Path) -> Result<Vec<TradeShortcutEntry>> {
    crate::profile::record_shortcut_json_load();
    let raw = std::fs::read_to_string(path)?;
    let file: TradeShortcutFile = serde_json::from_str(&raw)?;
    Ok(file.entries)
}

pub fn default_shortcuts_path() -> Result<std::path::PathBuf> {
    data_path("trade_shortcuts.json")
}

pub(crate) struct TradeShortcutCache {
    entries: Vec<TradeShortcutEntry>,
    by_id: HashMap<String, usize>,
    closure_indices: Vec<usize>,
}

impl TradeShortcutCache {
    fn build(entries: Vec<TradeShortcutEntry>) -> Self {
        let mut by_id = HashMap::new();
        let mut closure_indices = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            by_id.insert(entry.id.clone(), i);
            if entry
                .r#match
                .as_ref()
                .is_some_and(|m| m.kind == "closure")
            {
                closure_indices.push(i);
            }
        }
        Self {
            entries,
            by_id,
            closure_indices,
        }
    }

    pub(crate) fn get_by_id(&self, id: &str) -> Option<&TradeShortcutEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    fn closure_entries(&self) -> impl Iterator<Item = &TradeShortcutEntry> {
        self.closure_indices
            .iter()
            .map(|&i| &self.entries[i])
    }
}

static TRADE_SHORTCUT_CACHE: OnceLock<Option<TradeShortcutCache>> = OnceLock::new();

pub(crate) fn trade_shortcut_cache() -> Option<&'static TradeShortcutCache> {
    TRADE_SHORTCUT_CACHE
        .get_or_init(|| {
            let path = default_shortcuts_path().ok()?;
            let entries = load_trade_shortcuts(&path).ok()?;
            Some(TradeShortcutCache::build(entries))
        })
        .as_ref()
}

/// 巫恋核 / 龙舌兰投资 / 裁缝 α/β（与但书、可露希尔互斥的「巫恋侧」机制）。
pub fn room_has_witch_side_group(ops: &[TradeOperator], table: &SkillTable) -> bool {
    if has_witch_e2(ops, table) {
        return true;
    }
    if ops
        .iter()
        .any(|o| o.name == "龙舌兰" && has_long_invest_buff(o, table))
    {
        return true;
    }
    ops.iter()
        .any(|o| has_tailor_beta(o, table) || has_tailor_alpha(o, table))
}

/// 但书（合同法/违约）与巫恋侧机制 **不得同站**。
pub fn docus_tailor_exclusive_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    room_has_docus_mechanic(ops, table) && room_has_witch_side_group(ops, table)
}

/// 佩佩独占站：同房不得进驻提供订单获取效率% 的干员（工具人位应填上限/心情等）。
pub fn pepe_station_trade_eff_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    if !room_has_pepe_exclusive(ops, table) {
        return false;
    }
    ops.iter()
        .any(|o| o.name != "佩佩" && operator_constant_trade_flat_eff(o, table) > 0.0)
}

/// 贸易站同房互斥（公孙长乐）：违约 / 特别订单 / 巫恋低语 / 佩佩+效率人 不得混排。
pub fn trade_station_exclusive_violation(ops: &[TradeOperator], table: &SkillTable) -> bool {
    crate::profile::record_exclusive_check();
    let docus = room_has_docus_mechanic(ops, table);
    let closure = has_closure(ops, table);
    let witch = has_witch_e2(ops, table);

    if docus_tailor_exclusive_violation(ops, table) {
        return true;
    }
    if pepe_station_trade_eff_violation(ops, table) {
        return true;
    }
    if docus && closure {
        return true;
    }
    if witch && closure {
        return true;
    }
    false
}

/// 但书 + 工具人三人站（搜索/轮换均为 C(n,3)）。
pub fn is_docus_solo_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.len() >= 3
        && room_has_docus_mechanic(ops, table)
        && !room_has_witch_side_group(ops, table)
}

/// 叙拉古但书链段 consumer：但书 + 伺夜 + 贝洛内（无巫恋侧）。
pub fn is_docus_syracusa_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    is_docus_solo_station(ops, table)
        && ops.iter().any(|o| o.name == "伺夜")
        && ops.iter().any(|o| o.name == "贝洛内")
}

/// 链段 producer 已满足（中枢八幡海铃 E2）。
pub fn docus_syracusa_segment_active(inject: &GlobalInjectManifest) -> bool {
    inject.haru_e2_in_control()
}

/// 可露希尔特别订单站（不含但书/巫恋低语）。
pub fn is_closure_station(ops: &[TradeOperator], table: &SkillTable) -> bool {
    has_closure(ops, table) && !has_witch_e2(ops, table) && !room_has_docus_mechanic(ops, table)
}

/// 公孙：但书单走最终效率 ≈ 纸面工具效率 × 1.55 → `gold_pct=55` 固定，`trade_pct=order_eff_pre`。
pub const DOCUS_MECHANIC_GOLD_PCT: f64 = 55.0;

/// **L3 组合短路**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.7）：工具人表最优解查表。
pub fn resolve_trade_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, true)
}

/// 搜索热路径：调用方已做互斥预筛时跳过重复校验。
pub(crate) fn resolve_trade_shortcut_prevalidated(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    resolve_trade_shortcut_inner(ops, table, order_eff_pre, trade_level, inject, false)
}

fn resolve_trade_shortcut_inner(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
    inject: &GlobalInjectManifest,
    check_exclusivity: bool,
) -> Option<TradeShortcutMatch> {
    if check_exclusivity && trade_station_exclusive_violation(ops, table) {
        return None;
    }
    if let Some(m) = crate::trade::segment::match_registered_trade_segment(ops, table, inject) {
        return Some(m);
    }
    if let Some(m) = match_docus_solo_shortcut(ops, table, order_eff_pre) {
        return Some(m);
    }
    if let Some(m) = match_witch_group_shortcut(ops, table) {
        return Some(m);
    }
    match_closure_shortcut(ops, table, order_eff_pre, trade_level)
}

const JIE_MARKET_BUFF: &str = "trade_ord_limit_count[000]";
const KARLAN_TAG: &str = "cc.g.karlan";

/// 灵知 E2 + 精1 孑 + 银灰 + 另一名 `cc.g.karlan` 贸易干员；需中枢精密计算已激活。
pub fn match_ling_jie_shortcut(
    ops: &[TradeOperator],
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    if inject.karlan_precision().is_none() {
        return None;
    }
    let has_jie_market = ops.iter().any(|o| {
        o.name == "孑" && o.buff_ids.iter().any(|b| b == JIE_MARKET_BUFF)
    });
    if !has_jie_market {
        return None;
    }
    if !ops.iter().any(|o| o.name == "银灰") {
        return None;
    }
    let other_karlan = ops.iter().any(|o| {
        o.name != "孑"
            && o.name != "银灰"
            && o.tags.iter().any(|t| t == KARLAN_TAG)
    });
    if !other_karlan {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id("gsl_ling_jie_yaxin")?.clone();
    Some(TradeShortcutMatch { entry })
}

pub fn match_docus_solo_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
) -> Option<TradeShortcutMatch> {
    if !is_docus_solo_station(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let mut entry = cache.get_by_id("gsl_docus_solo")?.clone();
    entry.trade_pct = order_eff_pre;
    Some(TradeShortcutMatch { entry })
}

/// 叙拉古但书链段：委托 `trade/segment` 注册表（保留 API 供测试对照）。
pub fn match_docus_syracusa_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    inject: &GlobalInjectManifest,
) -> Option<TradeShortcutMatch> {
    crate::trade::segment::match_registered_trade_segment(ops, table, inject).filter(|m| {
        m.entry.id == "gsl_docus_syracusa"
    })
}

pub fn match_witch_group_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let kind = classify_witch_room(ops, table)?;
    let id = match kind {
        WitchRoomKind::LongE2Beta => "gsl_witch_long_beta",
        WitchRoomKind::LongE2Alpha => "gsl_witch_long_alpha",
        WitchRoomKind::LongE2Blank => "gsl_witch_long_blank",
        WitchRoomKind::LongE0Blank => "gsl_witch_long0_blank",
        WitchRoomKind::BetaBlankNoLongE2 => "gsl_witch_beta_blank",
    };
    let cache = trade_shortcut_cache()?;
    let entry = cache.get_by_id(id)?.clone();
    Some(TradeShortcutMatch { entry })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WitchRoomKind {
    LongE2Beta,
    LongE2Alpha,
    LongE2Blank,
    LongE0Blank,
    BetaBlankNoLongE2,
}

fn classify_witch_room(ops: &[TradeOperator], table: &SkillTable) -> Option<WitchRoomKind> {
    if !has_witch_e2(ops, table) {
        return None;
    }
    // 但书单走 / 可露希尔：同房有则不进巫恋核 L3。
    if room_has_docus_mechanic(ops, table) || has_closure(ops, table) {
        return None;
    }

    let long = find_op(ops, "龙舌兰");
    let long_e2 = long.is_some_and(|o| o.elite >= 2);
    let long_e0_only = long.is_some_and(|o| o.elite < 2);

    let has_beta = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_beta(o, table));
    let has_alpha = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_alpha(o, table));

    if long_e2 && has_beta {
        return Some(WitchRoomKind::LongE2Beta);
    }
    if long_e2 && has_alpha && !has_beta {
        return Some(WitchRoomKind::LongE2Alpha);
    }
    if long_e2 && !has_beta && !has_alpha {
        return Some(WitchRoomKind::LongE2Blank);
    }
    if long_e0_only && !has_beta && !has_alpha {
        return Some(WitchRoomKind::LongE0Blank);
    }
    if !long_e2 && has_beta && has_blank_third(ops, table) {
        return Some(WitchRoomKind::BetaBlankNoLongE2);
    }
    None
}

fn room_has_docus_mechanic(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_docus_buff(o, table))
}

fn room_has_pepe_exclusive(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| has_pepe_exclusive_buff(o, table))
}

fn has_pepe_exclusive_buff(op: &TradeOperator, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid.starts_with("trade_ord_pepe")
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "pepe_exclusive"
                    )
                })
            })
    })
}

/// `constant` 阶段 `AddFlatEff` 之和（佩佩站互斥判定用）。
fn operator_constant_trade_flat_eff(op: &TradeOperator, table: &SkillTable) -> f64 {
    let mut flat = 0.0;
    for bid in &op.buff_ids {
        let Some(skill) = table.get(bid) else {
            continue;
        };
        for atom in &skill.atoms {
            if atom.phase == crate::types::Phase::Constant {
                if let Action::AddFlatEff { value, .. } = atom.action {
                    flat += value;
                }
            }
        }
    }
    flat
}

fn has_long_invest_buff(op: &TradeOperator, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid.starts_with("trade_ord_long")
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(a.action, Action::AddOrderLmdBonus { .. })
                        || matches!(
                            a.action,
                            Action::TagOrder { ref tag } if tag == "long_invest"
                        )
                })
            })
    })
}

fn find_op<'a>(ops: &'a [TradeOperator], name: &str) -> Option<&'a TradeOperator> {
    ops.iter().find(|o| o.name == name)
}

fn has_witch_e2(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| {
        o.name == "巫恋"
            && o.elite >= 2
            && o.buff_ids.iter().any(|bid| has_witch_peer_absorb(bid, table))
    })
}

/// 巫恋·低语：`PeerEffAbsorb` 且 `rate_per_peer > 0`（与佩佩 rate=0 区分）。
fn has_witch_peer_absorb(bid: &str, table: &SkillTable) -> bool {
    peer_absorb_rate(bid, table).is_some_and(|r| r > 0.0)
}

fn peer_absorb_rate(bid: &str, table: &SkillTable) -> Option<f64> {
    table.get(bid).and_then(|s| {
        s.atoms.iter().find_map(|a| match a.action {
            Action::PeerEffAbsorb { rate_per_peer } => Some(rate_per_peer),
            _ => None,
        })
    })
}

fn has_docus_buff(op: &TradeOperator, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        bid == "trade_ord_law[000]"
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "breach"
                    )
                })
            })
    })
}

fn is_tailor_beta_id(bid: &str) -> bool {
    matches!(
        bid,
        "trade_ord_wt&cost[010]" | "trade_ord_wt&cost[011]" | "trade_ord_wt&cost[012]"
    )
}

fn is_tailor_alpha_id(bid: &str) -> bool {
    matches!(
        bid,
        "trade_ord_wt&cost[000]"
            | "trade_ord_wt&cost[001]"
            | "trade_ord_wt&cost[002]"
            | "trade_ord_wt&cost[003]"
    )
}

fn has_tailor_beta(op: &TradeOperator, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        is_tailor_beta_id(bid)
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "tailor_beta"
                    )
                })
            })
    })
}

fn has_tailor_alpha(op: &TradeOperator, table: &SkillTable) -> bool {
    op.buff_ids.iter().any(|bid| {
        is_tailor_alpha_id(bid)
            || table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "tailor_alpha"
                    )
                })
            })
    })
}

fn is_mechanic_filler(op: &TradeOperator, table: &SkillTable) -> bool {
    if op.name == "巫恋" || op.name == "龙舌兰" {
        return false;
    }
    !has_tailor_beta(op, table)
        && !has_tailor_alpha(op, table)
        && !has_docus_buff(op, table)
}

fn has_blank_third(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| is_mechanic_filler(o, table))
}

fn has_closure(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|op| {
        if op.elite < 2 {
            return false;
        }
        op.buff_ids.iter().any(|bid| {
            table.get(bid).is_some_and(|s| {
                s.atoms.iter().any(|a| {
                    matches!(
                        a.action,
                        Action::TagOrder { ref tag } if tag == "closure_special"
                    )
                })
            })
        })
    })
}

fn match_closure_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    _trade_level: u8,
) -> Option<TradeShortcutMatch> {
    if !has_closure(ops, table) {
        return None;
    }
    // 低语清零：同房有精二巫恋时不得按可露希尔分档短路。
    if has_witch_e2(ops, table) || room_has_docus_mechanic(ops, table) {
        return None;
    }
    let cache = trade_shortcut_cache()?;
    let tiers: Vec<_> = cache.closure_entries().collect();
    let best = tiers.iter().min_by(|a, b| {
        let da = (order_eff_pre - closure_tier(a) as f64).abs();
        let db = (order_eff_pre - closure_tier(b) as f64).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    if (order_eff_pre - closure_tier(best) as f64).abs() > 25.0 {
        return None;
    }
    Some(TradeShortcutMatch {
        entry: (*best).clone(),
    })
}

fn closure_tier(entry: &TradeShortcutEntry) -> i32 {
    station_trade_pct_anchor(entry)
}

fn station_trade_pct_anchor(entry: &TradeShortcutEntry) -> i32 {
    entry
        .r#match
        .as_ref()
        .and_then(|m| m.station_trade_pct)
        .unwrap_or(0)
}

fn distribution_for_tier(tier: ShortcutTailorTier, level: u8) -> GoldDistribution {
    if level < 3 {
        return GoldDistribution::regular_lv3();
    }
    match tier {
        ShortcutTailorTier::Beta => GoldDistribution::beta_peak_lv3(),
        ShortcutTailorTier::Alpha => GoldDistribution::alpha_peak_lv3(),
        ShortcutTailorTier::Regular | ShortcutTailorTier::Docus => GoldDistribution::regular_lv3(),
    }
}

fn tier_params(gold: u8) -> (f64, f64) {
    match gold {
        2 => (144.0, 1000.0),
        3 => (210.0, 1500.0),
        4 => (276.0, 2000.0),
        _ => (144.0, 1000.0),
    }
}

fn long_invest_bonus_avg(tier: ShortcutTailorTier, dist: &GoldDistribution) -> f64 {
    match tier {
        ShortcutTailorTier::Beta => dist.p4 * 500.0,
        ShortcutTailorTier::Regular if dist.p4 > 0.0 => dist.p4 * 250.0,
        _ => 0.0,
    }
}

fn expected_from_dist(dist: &GoldDistribution, long_bonus: f64) -> (f64, f64) {
    let mut gold = 0.0;
    let mut mpg_weighted = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        let (dur, lmd) = tier_params(g);
        let lmd_adj = lmd + if g == 4 { long_bonus } else { 0.0 };
        gold += p * g as f64;
        mpg_weighted += p * (dur / g as f64);
        let _ = lmd_adj;
    }
    (gold, mpg_weighted)
}

impl TradeShortcutMatch {
    pub fn unit_output_from_anchor(&self, baseline_unit_trade: f64) -> Option<crate::trade::unit_output::TradeUnitOutput> {
        let ut = self.entry.unit_trade_anchor?;
        let ug = self.entry.unit_gsl_gold_anchor.unwrap_or(0.0);
        let unit_gold = ug / crate::trade::unit_output::GSL_GOLD_UNIT_SCALE;
        let mult = if baseline_unit_trade > 0.0 {
            ut / baseline_unit_trade
        } else {
            1.0
        };
        Some(crate::trade::unit_output::TradeUnitOutput {
            unit_trade_per_day: ut,
            unit_gold_per_day: unit_gold,
            unit_originium_per_day: 0.0,
            multiplier_vs_lv3_regular: mult,
            drone_unit_trade_per_day: ut * crate::trade::unit_output::DRONE_TRADE_FACTOR,
            drone_unit_gold_per_day: unit_gold * crate::trade::unit_output::DRONE_TRADE_FACTOR,
            drone_unit_originium_per_day: 0.0,
        })
    }

    pub fn effective_multiplier(&self) -> f64 {
        let trade = 1.0 + self.entry.trade_pct / 100.0;
        let gold = 1.0 + self.entry.gold_pct / 100.0;
        trade * gold
    }

    pub fn build_mechanic_result(&self, trade_level: u8) -> OrderMechanicResult {
        let dist = distribution_for_tier(self.entry.tailor_tier, trade_level);
        let long_avg = long_invest_bonus_avg(self.entry.tailor_tier, &dist);
        let (gold_avg, mpg) = expected_from_dist(&dist, long_avg);

        OrderMechanicResult {
            order_kind: TradeOrderKind::Gold,
            dominant_kind: SpecialOrderKind::NormalGold,
            gold_distribution: dist,
            originium_distribution: None,
            mechanic_equiv_eff_pct: self.entry.gold_pct,
            gold_per_order_avg: gold_avg,
            originium_per_order_avg: 0.0,
            minutes_per_gold: mpg,
            minutes_per_originium_shard: 0.0,
            shortcut_id: Some(self.entry.id.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_table::default_skill_table_path;

    fn table() -> SkillTable {
        SkillTable::load(&default_skill_table_path().unwrap()).unwrap()
    }

    fn mk_op(name: &str, elite: u8, buff_ids: Vec<&str>) -> TradeOperator {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    }

    #[test]
    fn gsl_witch_long_beta_shortcut() {
        let table = table();
        let ops = vec![
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            mk_op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
        ];
        let m = match_witch_group_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_witch_long_beta");
        assert!((m.entry.trade_pct - 138.0).abs() < 0.01);
        assert!((m.effective_multiplier() - 2.38 * 1.46).abs() < 0.03);
    }

    #[test]
    fn docus_and_tailor_group_are_mutually_exclusive() {
        let table = table();
        let witch_long_docus = vec![
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
        ];
        assert!(docus_tailor_exclusive_violation(&witch_long_docus, &table));
        assert!(match_witch_group_shortcut(&witch_long_docus, &table).is_none());

        let docus_solo = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        assert!(is_docus_solo_station(&docus_solo, &table));
        assert!(!docus_tailor_exclusive_violation(&docus_solo, &table));
    }

    #[test]
    fn gsl_witch_beta_blank_shortcut() {
        let table = table();
        let ops = vec![
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
            mk_op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
        ];
        let m = match_witch_group_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_witch_beta_blank");
        assert!((m.entry.trade_pct - 93.0).abs() < 0.01);
    }

    #[test]
    fn gsl_closure_tier90_still_works() {
        let table = table();
        let ops = vec![
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("能天使", 2, vec!["trade_ord_spd[020]"]),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        let m = resolve_trade_shortcut(&ops, &table, 114.0, 3, &GlobalInjectManifest::default()).expect("match");
        assert_eq!(m.entry.id, "gsl_closure_tier90");
    }

    #[test]
    fn closure_and_witch_e2_are_mutually_exclusive() {
        let table = table();
        let mix = vec![
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("银灰", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
        ];
        assert!(trade_station_exclusive_violation(&mix, &table));
        assert!(resolve_trade_shortcut(&mix, &table, 93.0, 3, &GlobalInjectManifest::default()).is_none());
        assert!(match_closure_shortcut(&mix, &table, 134.0, 3).is_none());
        assert!(match_witch_group_shortcut(&mix, &table).is_none());
    }

    #[test]
    fn witch_blank_without_long_uses_no_closure_shortcut() {
        let table = table();
        let ops = vec![
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("银灰", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
            mk_op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
        ];
        assert!(!trade_station_exclusive_violation(&ops, &table));
        assert!(resolve_trade_shortcut(&ops, &table, 93.0, 3, &GlobalInjectManifest::default()).is_none());
    }

    #[test]
    fn gsl_docus_syracusa_requires_haru_and_trio() {
        use crate::instances::{default_instances_path, OperatorInstances};
        use crate::pool::build_trade_pool;
        use crate::roster::Roster;

        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [("但书", 2), ("伺夜", 2), ("贝洛内", 2)]
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        let mk = |names: &[&str]| -> Vec<TradeOperator> {
            names
                .iter()
                .map(|n| pool.entry(n).unwrap().to_trade_operator())
                .collect()
        };
        let trio = mk(&["但书", "伺夜", "贝洛内"]);

        let mut with_haru = GlobalInjectManifest::default();
        with_haru.record_haru_e2_in_control();
        let m = resolve_trade_shortcut(&trio, &table, 80.0, 3, &with_haru).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_syracusa");
        assert!((m.entry.trade_pct - 90.0).abs() < 0.01);

        let without_haru = GlobalInjectManifest::default();
        let m2 = resolve_trade_shortcut(&trio, &table, 80.0, 3, &without_haru).expect("match");
        assert_eq!(m2.entry.id, "gsl_docus_solo");
        assert!((m2.entry.trade_pct - 80.0).abs() < 0.01);
    }

    #[test]
    fn gsl_docus_syracusa_falls_back_when_trio_incomplete() {
        let table = table();
        let ops = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op("维娜·维多利亚", 2, vec!["trade_ord_spd&par[001]"]),
            mk_op("泰拉大陆调查团", 2, vec!["trade_ord_spd&limit&bd[000]"]),
        ];
        let mut inject = GlobalInjectManifest::default();
        inject.record_haru_e2_in_control();
        let pre = 55.0;
        let m = resolve_trade_shortcut(&ops, &table, pre, 3, &inject).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_solo");
        assert!((m.entry.trade_pct - pre).abs() < 0.01);
    }

    #[test]
    fn gsl_docus_solo_uses_pre_eff_times_155() {
        let table = table();
        let ops = vec![
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
            mk_op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        let pre = 83.0;
        let m = resolve_trade_shortcut(&ops, &table, pre, 3, &GlobalInjectManifest::default()).expect("match");
        assert_eq!(m.entry.id, "gsl_docus_solo");
        assert!((m.entry.trade_pct - pre).abs() < 0.01);
        assert!((m.effective_multiplier() - (1.0 + pre / 100.0) * 1.55).abs() < 0.02);
    }

    #[test]
    fn docus_and_closure_are_mutually_exclusive() {
        let table = table();
        let mix = vec![
            mk_op("但书", 2, vec!["trade_ord_law[000]", "trade_ord_against[010]"]),
            mk_op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
            mk_op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
        ];
        assert!(trade_station_exclusive_violation(&mix, &table));
    }

    #[test]
    fn gsl_ling_jie_accepts_karlan_peer_besides_yaxin() {
        use crate::global_resource::GlobalInjectManifest;
        let mut inject = GlobalInjectManifest::default();
        inject.record_karlan_precision(-15.0, 6);
        let ops = vec![
            TradeOperator {
                name: "孑".into(),
                elite: 1,
                buff_ids: vec!["trade_ord_limit_count[000]".into()],
                tags: vec![],
                compiled_atoms: std::sync::Arc::from([]),
            },
            TradeOperator {
                name: "银灰".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.karlan".into()],
                compiled_atoms: std::sync::Arc::from([]),
            },
            TradeOperator {
                name: "角峰".into(),
                elite: 0,
                buff_ids: vec![],
                tags: vec!["cc.g.karlan".into()],
                compiled_atoms: std::sync::Arc::from([]),
            },
        ];
        let m = match_ling_jie_shortcut(&ops, &inject).expect("match");
        assert_eq!(m.entry.id, "gsl_ling_jie_yaxin");
        assert!((m.entry.trade_pct - 125.0).abs() < f64::EPSILON);
    }

    #[test]
    fn gsl_ling_jie_rejects_without_second_karlan() {
        use crate::global_resource::GlobalInjectManifest;
        let mut inject = GlobalInjectManifest::default();
        inject.record_karlan_precision(-15.0, 6);
        let ops = vec![
            TradeOperator {
                name: "孑".into(),
                elite: 1,
                buff_ids: vec!["trade_ord_limit_count[000]".into()],
                tags: vec![],
                compiled_atoms: std::sync::Arc::from([]),
            },
            TradeOperator {
                name: "银灰".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.karlan".into()],
                compiled_atoms: std::sync::Arc::from([]),
            },
            TradeOperator {
                name: "能天使".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec![],
                compiled_atoms: std::sync::Arc::from([]),
            },
        ];
        assert!(match_ling_jie_shortcut(&ops, &inject).is_none());
    }
}
