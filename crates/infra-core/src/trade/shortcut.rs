use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::skill_table::data_path;
use crate::skill_table::SkillTable;
use crate::trade::input::TradeOperator;
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
    let raw = std::fs::read_to_string(path)?;
    let file: TradeShortcutFile = serde_json::from_str(&raw)?;
    Ok(file.entries)
}

pub fn default_shortcuts_path() -> Result<std::path::PathBuf> {
    data_path("trade_shortcuts.json")
}

/// **L3 组合短路**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.7）：工具人表最优解查表。
/// 巫恋组（组合分类）> 可露希尔分档（order_eff 锚定）；同时作 `verify` 回归锚点。
pub fn resolve_trade_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
    order_eff_pre: f64,
    trade_level: u8,
) -> Option<TradeShortcutMatch> {
    if let Some(m) = match_witch_group_shortcut(ops, table) {
        return Some(m);
    }
    match_closure_shortcut(ops, table, order_eff_pre, trade_level)
}

pub fn match_witch_group_shortcut(
    ops: &[TradeOperator],
    table: &SkillTable,
) -> Option<TradeShortcutMatch> {
    let table_entries = load_trade_shortcuts(&default_shortcuts_path().ok()?).ok()?;
    let kind = classify_witch_room(ops, table)?;
    let id = match kind {
        WitchRoomKind::LongE2Docus => "gsl_witch_long_docus",
        WitchRoomKind::LongE2Beta => "gsl_witch_long_beta",
        WitchRoomKind::LongE2Alpha => "gsl_witch_long_alpha",
        WitchRoomKind::LongE2Blank => "gsl_witch_long_blank",
        WitchRoomKind::LongE0Blank => "gsl_witch_long0_blank",
        WitchRoomKind::BetaBlankNoLongE2 => "gsl_witch_beta_blank",
    };
    let entry = table_entries.into_iter().find(|e| e.id == id)?;
    Some(TradeShortcutMatch { entry })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WitchRoomKind {
    LongE2Docus,
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

    let long = find_op(ops, "龙舌兰");
    let long_e2 = long.is_some_and(|o| o.elite >= 2);
    let long_e0_only = long.is_some_and(|o| o.elite < 2);

    let has_docus = ops.iter().any(|o| o.name == "但书" && has_docus_buff(o, table));
    let has_beta = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_beta(o, table));
    let has_alpha = ops
        .iter()
        .any(|o| o.name != "巫恋" && has_tailor_alpha(o, table));

    if long_e2 && has_docus {
        return Some(WitchRoomKind::LongE2Docus);
    }
    if long_e2 && has_beta {
        return Some(WitchRoomKind::LongE2Beta);
    }
    if long_e2 && has_alpha && !has_beta {
        return Some(WitchRoomKind::LongE2Alpha);
    }
    if long_e2 && !has_beta && !has_alpha && !has_docus {
        return Some(WitchRoomKind::LongE2Blank);
    }
    if long_e0_only && !has_beta && !has_alpha && !has_docus {
        return Some(WitchRoomKind::LongE0Blank);
    }
    if !long_e2 && has_beta && !has_docus && has_blank_third(ops, table) {
        return Some(WitchRoomKind::BetaBlankNoLongE2);
    }
    None
}

fn find_op<'a>(ops: &'a [TradeOperator], name: &str) -> Option<&'a TradeOperator> {
    ops.iter().find(|o| o.name == name)
}

fn has_witch_e2(ops: &[TradeOperator], table: &SkillTable) -> bool {
    ops.iter().any(|o| {
        o.name == "巫恋"
            && o.elite >= 2
            && o.buff_ids.iter().any(|bid| has_vodfox_buff(bid, table))
    })
}

fn has_vodfox_buff(bid: &str, table: &SkillTable) -> bool {
    bid == "trade_ord_vodfox[000]"
        || table.get(bid).is_some_and(|s| {
            s.atoms
                .iter()
                .any(|a| matches!(a.action, Action::VodfoxAbsorb { .. }))
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
                        Action::ReplaceOrder {
                            order_type: ref t
                        } if t == "closure_special"
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
    let table_entries = load_trade_shortcuts(&default_shortcuts_path().ok()?).ok()?;
    let tiers: Vec<_> = table_entries
        .iter()
        .filter(|e| e.r#match.as_ref().is_some_and(|m| m.kind == "closure"))
        .collect();
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
            dominant_kind: SpecialOrderKind::NormalGold,
            gold_distribution: dist,
            mechanic_equiv_eff_pct: self.entry.gold_pct,
            gold_per_order_avg: gold_avg,
            minutes_per_gold: mpg,
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
    fn gsl_witch_long_docus_shortcut() {
        let table = table();
        let ops = vec![
            mk_op("巫恋", 2, vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"]),
            mk_op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            mk_op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            ),
        ];
        let m = match_witch_group_shortcut(&ops, &table).expect("match");
        assert_eq!(m.entry.id, "gsl_witch_long_docus");
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
            mk_op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
            mk_op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
        ];
        let m = resolve_trade_shortcut(&ops, &table, 134.0, 3).expect("match");
        assert_eq!(m.entry.id, "gsl_closure_tier90");
    }
}
