//! **L2 域短路**（见 `docs/机制/EFFECT_ATOM_DESIGN.md` §8.6）：订单类型与分布 → 等效贸易效率%。
//! L1 `order_mechanic` phase 只打 tag；本模块算分布、单位产出与 `mechanic_equiv_eff_pct`。

use serde::Serialize;

use crate::efficiency::Efficiency;

use super::interpreter::{MechanicCaps, TradeContext};
use crate::trade::input::TradeOrderKind;

const LMD_PER_GOLD: f64 = 500.0;
/// 公孙表：但书在 2/3 金订单上额外 +1000 龙门币（4 金不变）。
pub const DOCUS_SUB4_LMD_BONUS: f64 = 1200.0;

/// PRTS：特别独占订单 4:30:00，0 赤金，1000 龙门币；不受订单获取效率影响。
pub const PEPE_ORDER_DURATION_MIN: f64 = 270.0;
pub const PEPE_ORDER_LMD: f64 = 1000.0;

const MINUTES_PER_DAY: f64 = 1440.0;

/// 24h、固定 cadence 下的日贸易产出（与纸面 trade% 无关）。
pub fn pepe_unit_trade_per_day() -> f64 {
    PEPE_ORDER_LMD / PEPE_ORDER_DURATION_MIN * MINUTES_PER_DAY
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum SpecialOrderKind {
    NormalGold,
    NormalOriginium,
    ClosureSpecial,
    PepeExclusive,
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

    /// 贸易站等级决定可出现订单档位（公孙：2 级 2/3 金为 60%/40%，无 4 金；1 级仅 2 金）。
    pub fn for_trade_level(level: u8) -> Self {
        match level {
            1 => Self {
                p2: 1.0,
                p3: 0.0,
                p4: 0.0,
            },
            2 => Self {
                p2: 0.60,
                p3: 0.40,
                p4: 0.0,
            },
            _ => Self::regular_lv3(),
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

    /// 裁剪当前等级不可出现的档位。
    pub fn clamp_to_trade_level(self, level: u8) -> Self {
        let mut d = self;
        if level < 3 {
            d.p4 = 0.0;
            let s = d.p2 + d.p3;
            if s > 0.0 {
                d.p2 /= s;
                d.p3 /= s;
            }
        }
        if level < 2 {
            d.p3 = 0.0;
            d.p2 = 1.0;
        }
        d
    }
}

/// 三级贸易站“开采协力”固定订单：2 固源岩 → 20 合成玉，耗时 2h。
#[derive(Debug, Clone, Serialize)]
pub struct OriginiumDistribution {
    pub shards_per_order: u8,
    pub orundum_per_order: f64,
    pub minutes_per_order: f64,
    pub lmd_per_order: f64,
}

impl OriginiumDistribution {
    pub fn fixed_lv3() -> Self {
        Self {
            shards_per_order: 2,
            orundum_per_order: 20.0,
            minutes_per_order: 120.0,
            lmd_per_order: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderMechanicResult {
    pub order_kind: TradeOrderKind,
    pub dominant_kind: SpecialOrderKind,
    pub gold_distribution: GoldDistribution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub originium_distribution: Option<OriginiumDistribution>,
    pub mechanic_equivalent_efficiency: Efficiency,
    pub gold_per_order_avg: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub originium_per_order_avg: f64,
    pub minutes_per_gold: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub minutes_per_originium_shard: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_id: Option<String>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl OrderMechanicResult {
    pub fn ignores_order_eff(&self) -> bool {
        self.dominant_kind == SpecialOrderKind::PepeExclusive
    }
}

pub fn resolve_order_mechanic(
    ctx: &TradeContext<'_>,
    order_eff_total_pct: f64,
) -> OrderMechanicResult {
    if ctx.active_order_kind == TradeOrderKind::Originium {
        return originium_result(ctx.facility_level);
    }

    let caps = ctx.mechanic_caps();
    let level = ctx.facility_level;

    let dist = if ctx.order_tags.iter().any(|t| t == "tailor_beta") {
        GoldDistribution::beta_peak_lv3().clamp_to_trade_level(level)
    } else if ctx.order_tags.iter().any(|t| t == "tailor_alpha") {
        GoldDistribution::alpha_peak_lv3().clamp_to_trade_level(level)
    } else {
        GoldDistribution::for_trade_level(level)
    };

    if caps.closure {
        return closure_result(order_eff_total_pct, &dist);
    }

    if ctx.order_tags.iter().any(|t| t == "eureka") {
        return eureka_result(order_eff_total_pct, &dist);
    }

    if ctx.order_tags.iter().any(|t| t == "pepe_exclusive") {
        return pepe_result(&dist);
    }

    let vanilla = MechanicCaps {
        law: false,
        breach_add: 0,
        closure: false,
    };
    let baseline_mpg =
        weighted_minutes_per_gold(&GoldDistribution::for_trade_level(level), &vanilla, 0);
    let mpg = weighted_minutes_per_gold(&dist, &caps, ctx.order_lmd_bonus);
    let mut mechanic_equiv = if baseline_mpg > 0.0 && mpg > 0.0 {
        (baseline_mpg / mpg - 1.0) * 100.0
    } else {
        0.0
    };
    if ctx.order_lmd_bonus > 0 {
        let (_, lmd4, _) = order_lmd_and_gold(4, &caps, ctx.order_lmd_bonus);
        mechanic_equiv += dist.p4 * (ctx.order_lmd_bonus as f64 / lmd4) * 100.0;
    }
    let gold_avg = expected_gold_avg(&dist, &caps, ctx.order_lmd_bonus);

    OrderMechanicResult {
        order_kind: TradeOrderKind::Gold,
        dominant_kind: SpecialOrderKind::NormalGold,
        gold_distribution: dist,
        originium_distribution: None,
        mechanic_equivalent_efficiency: Efficiency::from_percent_points(mechanic_equiv),
        gold_per_order_avg: gold_avg,
        originium_per_order_avg: 0.0,
        minutes_per_gold: mpg,
        minutes_per_originium_shard: 0.0,
        shortcut_id: None,
    }
}

pub fn order_lmd_and_gold(
    base_gold: u8,
    caps: &MechanicCaps,
    long_lmd_bonus: i32,
) -> (f64, f64, f64) {
    let mut lmd = tier_params(base_gold).1;
    let mut gold = base_gold as i32;
    let mut breach = false;
    if caps.law && gold < 4 {
        breach = true;
        lmd += DOCUS_SUB4_LMD_BONUS;
    }
    if breach && caps.breach_add > 0 {
        gold += caps.breach_add;
        lmd += caps.breach_add as f64 * LMD_PER_GOLD;
    }
    if base_gold == 4 && long_lmd_bonus > 0 {
        lmd += long_lmd_bonus as f64;
    }
    let final_gold = gold.clamp(2, 4) as u8;
    let dur = tier_params(final_gold).0;
    (dur, lmd, gold as f64)
}

fn eureka_result(order_eff_total_pct: f64, dist: &GoldDistribution) -> OrderMechanicResult {
    let baseline_mpg = weighted_minutes_per_gold(dist, &MechanicCaps::default(), 0);
    let eureka_mpg = 144.0 / 2.0;
    let mechanic_equiv = if baseline_mpg > 0.0 {
        (baseline_mpg / eureka_mpg - 1.0) * 100.0
    } else {
        0.0
    };
    let _ = order_eff_total_pct;
    OrderMechanicResult {
        order_kind: TradeOrderKind::Gold,
        dominant_kind: SpecialOrderKind::NormalGold,
        gold_distribution: dist.clone(),
        originium_distribution: None,
        mechanic_equivalent_efficiency: Efficiency::from_percent_points(mechanic_equiv),
        gold_per_order_avg: 2.0,
        originium_per_order_avg: 0.0,
        minutes_per_gold: eureka_mpg,
        minutes_per_originium_shard: 0.0,
        shortcut_id: None,
    }
}

fn pepe_result(dist: &GoldDistribution) -> OrderMechanicResult {
    let baseline = baseline_unit_trade_lv3_regular();
    let pepe_unit = pepe_unit_trade_per_day();
    let mechanic_equiv = if baseline > 0.0 {
        (pepe_unit / baseline - 1.0) * 100.0
    } else {
        0.0
    };
    OrderMechanicResult {
        order_kind: TradeOrderKind::Gold,
        dominant_kind: SpecialOrderKind::PepeExclusive,
        gold_distribution: dist.clone(),
        originium_distribution: None,
        mechanic_equivalent_efficiency: Efficiency::from_percent_points(mechanic_equiv),
        gold_per_order_avg: 0.0,
        originium_per_order_avg: 0.0,
        minutes_per_gold: PEPE_ORDER_DURATION_MIN,
        minutes_per_originium_shard: 0.0,
        shortcut_id: None,
    }
}

fn closure_result(order_eff_total_pct: f64, dist: &GoldDistribution) -> OrderMechanicResult {
    let baseline_mpg = weighted_minutes_per_gold(dist, &MechanicCaps::default(), 0);
    let closure_mpg = 144.0 / 2.0;
    let mechanic_equiv = if baseline_mpg > 0.0 {
        (baseline_mpg / closure_mpg - 1.0) * 100.0
    } else {
        0.0
    };
    let _ = order_eff_total_pct;
    OrderMechanicResult {
        order_kind: TradeOrderKind::Gold,
        dominant_kind: SpecialOrderKind::ClosureSpecial,
        gold_distribution: dist.clone(),
        originium_distribution: None,
        mechanic_equivalent_efficiency: Efficiency::from_percent_points(mechanic_equiv),
        gold_per_order_avg: 2.0,
        originium_per_order_avg: 0.0,
        minutes_per_gold: closure_mpg,
        minutes_per_originium_shard: 0.0,
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

fn weighted_minutes_per_gold(
    dist: &GoldDistribution,
    caps: &MechanicCaps,
    long_lmd_bonus: i32,
) -> f64 {
    let mut sum = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        if p <= 0.0 {
            continue;
        }
        let (dur, _, gold) = order_lmd_and_gold(g, caps, long_lmd_bonus);
        if gold > 0.0 {
            sum += p * (dur / gold);
        }
    }
    sum
}

fn expected_gold_avg(dist: &GoldDistribution, caps: &MechanicCaps, long_lmd_bonus: i32) -> f64 {
    let mut sum = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        if p <= 0.0 {
            continue;
        }
        let (_, _, gold) = order_lmd_and_gold(g, caps, long_lmd_bonus);
        sum += p * gold;
    }
    sum
}

fn expected_originium_avg(dist: &OriginiumDistribution) -> f64 {
    dist.orundum_per_order
}

pub fn originium_order_params(shards: u8) -> (f64, f64, f64) {
    let dist = OriginiumDistribution::fixed_lv3();
    (
        dist.minutes_per_order,
        dist.lmd_per_order,
        f64::from(shards),
    )
}

fn originium_result(_level: u8) -> OrderMechanicResult {
    let dist = OriginiumDistribution::fixed_lv3();
    let mps = dist.minutes_per_order / f64::from(dist.shards_per_order);
    let originium_avg = expected_originium_avg(&dist);
    OrderMechanicResult {
        order_kind: TradeOrderKind::Originium,
        dominant_kind: SpecialOrderKind::NormalOriginium,
        gold_distribution: GoldDistribution::for_trade_level(3),
        originium_distribution: Some(dist),
        mechanic_equivalent_efficiency: Efficiency::ZERO,
        gold_per_order_avg: 0.0,
        originium_per_order_avg: originium_avg,
        minutes_per_gold: 0.0,
        minutes_per_originium_shard: mps,
        shortcut_id: None,
    }
}

pub fn originium_unit_per_slot_per_day(dist: &OriginiumDistribution) -> (f64, f64) {
    let orders_per_day = MINUTES_PER_DAY / dist.minutes_per_order;
    (
        dist.orundum_per_order * orders_per_day,
        f64::from(dist.shards_per_order) * orders_per_day,
    )
}

/// 3 级站常规源石订单分布、纸面 100% 时的日贸易产出（倍率分母）。
pub fn baseline_unit_trade_lv3_originium() -> f64 {
    originium_unit_per_slot_per_day(&OriginiumDistribution::fixed_lv3()).0
}

/// 3 级站常规分布、无机制、纸面 100% 时的日贸易产出（倍率分母）。
pub fn baseline_unit_trade_lv3_regular() -> f64 {
    unit_per_slot_per_day(
        &GoldDistribution::for_trade_level(3),
        &MechanicCaps::default(),
        0,
    )
    .0
}

/// 24h、纸面 100% 下按分布加权得到的 (日龙门币, 日赤金)。
pub fn unit_per_slot_per_day(
    dist: &GoldDistribution,
    caps: &MechanicCaps,
    order_lmd_bonus: i32,
) -> (f64, f64) {
    let mut lmd_rate = 0.0;
    let mut gold_rate = 0.0;
    for (g, p) in [(2u8, dist.p2), (3, dist.p3), (4, dist.p4)] {
        if p <= 0.0 {
            continue;
        }
        let (dur, lmd, gold) = order_lmd_and_gold(g, caps, order_lmd_bonus);
        lmd_rate += p * (lmd / dur);
        gold_rate += p * (gold / dur);
    }
    (lmd_rate * MINUTES_PER_DAY, gold_rate * MINUTES_PER_DAY)
}
