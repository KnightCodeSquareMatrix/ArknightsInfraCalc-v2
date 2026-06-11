//! **L2 域短路**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.6）：订单类型与分布 → 等效贸易效率%。
//! L1 `order_mechanic` phase 只打 tag / 替换订单；本模块算 `mechanic_equiv_eff_pct`。

use serde::Serialize;

use super::interpreter::{MechanicCaps, TradeContext};

const LMD_PER_GOLD: f64 = 500.0;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum SpecialOrderKind {
    NormalGold,
    ClosureSpecial,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoldDistribution {
    pub p2: f64,
    pub p3: f64,
    pub p4: f64,
}

impl GoldDistribution {
    pub fn regular_lv3() -> Self {
        Self {
            p2: 0.30,
            p3: 0.50,
            p4: 0.20,
        }
    }

    pub fn alpha_peak_lv3() -> Self {
        Self {
            p2: 0.15,
            p3: 0.30,
            p4: 0.55,
        }
    }

    pub fn beta_peak_lv3() -> Self {
        Self {
            p2: 0.05,
            p3: 0.10,
            p4: 0.85,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderMechanicResult {
    pub dominant_kind: SpecialOrderKind,
    pub gold_distribution: GoldDistribution,
    pub mechanic_equiv_eff_pct: f64,
    pub gold_per_order_avg: f64,
    pub minutes_per_gold: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_id: Option<String>,
}

impl OrderMechanicResult {
    pub fn effective_eff_multiplier(&self, order_eff_total_pct: f64) -> f64 {
        let eff = 1.0 + order_eff_total_pct / 100.0;
        let mech = 1.0 + self.mechanic_equiv_eff_pct / 100.0;
        eff * mech
    }
}

pub fn resolve_order_mechanic(ctx: &TradeContext, order_eff_total_pct: f64) -> OrderMechanicResult {
    let caps = ctx.mechanic_caps();
    let dist = if ctx.order_tags.iter().any(|t| t == "tailor_beta") {
        GoldDistribution::beta_peak_lv3()
    } else if ctx.order_tags.iter().any(|t| t == "tailor_alpha") {
        GoldDistribution::alpha_peak_lv3()
    } else {
        GoldDistribution::regular_lv3()
    };

    if caps.closure {
        return closure_result(order_eff_total_pct, &dist);
    }

    if ctx.replace_order.as_deref() == Some("eureka") {
        return eureka_result(order_eff_total_pct, &dist);
    }

    if ctx.replace_order.as_deref() == Some("pepe_exclusive") {
        return pepe_result(&dist);
    }

    let baseline_mpg = weighted_minutes_per_gold(&GoldDistribution::regular_lv3(), &MechanicCaps {
        law: false,
        breach_add: 0,
        closure: false,
    });
    let mpg = weighted_minutes_per_gold(&dist, &caps);
    let mut mechanic_equiv = if baseline_mpg > 0.0 && mpg > 0.0 {
        (baseline_mpg / mpg - 1.0) * 100.0
    } else {
        0.0
    };
    if ctx.order_lmd_bonus > 0 {
        let (_, lmd4) = tier_params(4);
        mechanic_equiv += dist.p4 * (ctx.order_lmd_bonus as f64 / lmd4) * 100.0;
    }
    let gold_avg = expected_gold_avg(&dist, &caps);

    OrderMechanicResult {
        dominant_kind: SpecialOrderKind::NormalGold,
        gold_distribution: dist,
        mechanic_equiv_eff_pct: mechanic_equiv,
        gold_per_order_avg: gold_avg,
        minutes_per_gold: mpg,
        shortcut_id: None,
    }
}

fn eureka_result(order_eff_total_pct: f64, dist: &GoldDistribution) -> OrderMechanicResult {
    let baseline_mpg = weighted_minutes_per_gold(dist, &MechanicCaps {
        law: false,
        breach_add: 0,
        closure: false,
    });
    let eureka_mpg = 144.0 / 2.0;
    let mechanic_equiv = if baseline_mpg > 0.0 {
        (baseline_mpg / eureka_mpg - 1.0) * 100.0
    } else {
        0.0
    };
    let _ = order_eff_total_pct;
    OrderMechanicResult {
        dominant_kind: SpecialOrderKind::NormalGold,
        gold_distribution: dist.clone(),
        mechanic_equiv_eff_pct: mechanic_equiv,
        gold_per_order_avg: 2.0,
        minutes_per_gold: eureka_mpg,
        shortcut_id: None,
    }
}

fn pepe_result(dist: &GoldDistribution) -> OrderMechanicResult {
    OrderMechanicResult {
        dominant_kind: SpecialOrderKind::NormalGold,
        gold_distribution: dist.clone(),
        mechanic_equiv_eff_pct: 0.0,
        gold_per_order_avg: 0.0,
        minutes_per_gold: 270.0,
        shortcut_id: None,
    }
}

fn closure_result(order_eff_total_pct: f64, dist: &GoldDistribution) -> OrderMechanicResult {
    let baseline_mpg = weighted_minutes_per_gold(dist, &MechanicCaps {
        law: false,
        breach_add: 0,
        closure: false,
    });
    let closure_mpg = 144.0 / 2.0;
    let mechanic_equiv = if baseline_mpg > 0.0 {
        (baseline_mpg / closure_mpg - 1.0) * 100.0
    } else {
        0.0
    };
    let _ = order_eff_total_pct;
    OrderMechanicResult {
        dominant_kind: SpecialOrderKind::ClosureSpecial,
        gold_distribution: dist.clone(),
        mechanic_equiv_eff_pct: mechanic_equiv,
        gold_per_order_avg: 2.0,
        minutes_per_gold: closure_mpg,
        shortcut_id: None,
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

fn apply_mechanics(base_gold: u8, base_lmd: f64, caps: &MechanicCaps) -> (f64, f64) {
    let mut gold = base_gold as i32;
    let mut lmd = base_lmd;
    let mut breach = false;
    if caps.law && gold < 4 {
        breach = true;
    }
    if breach && caps.breach_add > 0 {
        gold += caps.breach_add;
        lmd += caps.breach_add as f64 * LMD_PER_GOLD;
    }
    (gold as f64, lmd)
}

fn weighted_minutes_per_gold(dist: &GoldDistribution, caps: &MechanicCaps) -> f64 {
    let mut sum = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        let (dur, lmd) = tier_params(g);
        let (gold, _) = apply_mechanics(g, lmd, caps);
        sum += p * (dur / gold);
    }
    sum
}

fn expected_gold_avg(dist: &GoldDistribution, caps: &MechanicCaps) -> f64 {
    let mut sum = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        let (_, lmd) = tier_params(g);
        let (gold, _) = apply_mechanics(g, lmd, caps);
        sum += p * gold;
    }
    sum
}
