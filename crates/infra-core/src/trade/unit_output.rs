//! **场景产量层**（见 `docs/机制/EFFECT_ATOM_DESIGN.md` §8.11）：单位产出 / 倍率 / 无人机。
//! 不新增 EffectAtom；由 L2 分布 + 订单效率% 与上班时长组合。

use serde::Serialize;

use crate::efficiency::Efficiency;

use super::efficiency::{TradeEfficiency, GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY};
use super::interpreter::{MechanicCaps, TradeContext};
use super::order_mechanic::{
    unit_per_slot_per_day, GoldDistribution, OrderMechanicResult, SpecialOrderKind,
    PEPE_ORDER_DURATION_MIN, PEPE_ORDER_LMD,
};

pub use super::order_mechanic::baseline_unit_trade_lv3_regular;

/// 公孙长乐工具人表：无人机贸易产出折算系数。
pub const DRONE_TRADE_FACTOR: f64 = 0.685;
/// 工具人表「单位赤金产出」与内部日消耗量的换算（closure 锚点 20→2000）。
pub const GSL_GOLD_UNIT_SCALE: f64 = 100.0;
pub const GOLD_OUTPUT_REFERENCE_PER_DAY: f64 = 10_000.0;

pub fn regular_trade_unit_output_per_day(level: u8) -> f64 {
    match level {
        1 => 10_000.0,
        2 => 10_141.0,
        _ => GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY,
    }
}

const MINUTES_PER_DAY: f64 = 1440.0;

#[derive(Debug, Clone, Serialize)]
pub struct TradeUnitOutput {
    /// 24h、效率总和=100% 时的日贸易产出（龙门币）。
    pub unit_trade_per_day: f64,
    /// 24h、效率总和=100% 时的日赤金消耗。
    pub unit_gold_per_day: f64,
    /// 24h、效率总和=100% 时的特殊赤金生产能力；普通贸易订单为 0。
    #[serde(skip_serializing)]
    pub gold_consumption_per_day: f64,
    /// 24h、效率总和=100% 时的日固源岩消耗（源石订单轨）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unit_originium_per_day: f64,
    /// 相对 3 级站常规分布（无机制）的倍率。
    pub multiplier_vs_lv3_regular: f64,
    pub drone_unit_trade_per_day: f64,
    pub drone_unit_gold_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub drone_unit_originium_per_day: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl TradeUnitOutput {
    /// 与公孙工具人表对齐的赤金单位产出。
    pub fn gsl_unit_gold(&self) -> f64 {
        self.unit_gold_per_day * GSL_GOLD_UNIT_SCALE
    }

    pub fn replace_trade_unit_output(&mut self, unit_trade_per_day: f64, reference: f64) {
        self.unit_trade_per_day = unit_trade_per_day;
        self.multiplier_vs_lv3_regular = if reference > 0.0 {
            unit_trade_per_day / reference
        } else {
            1.0
        };
        self.drone_unit_trade_per_day = unit_trade_per_day * DRONE_TRADE_FACTOR;
    }

    pub fn replace_gsl_gold_unit_output(&mut self, gsl_unit_gold: f64) {
        self.unit_gold_per_day = gsl_unit_gold / GSL_GOLD_UNIT_SCALE;
        self.gold_consumption_per_day = self.unit_gold_per_day;
        self.drone_unit_gold_per_day = self.unit_gold_per_day * DRONE_TRADE_FACTOR;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeDailyYield {
    pub trade_lmd: f64,
    pub gold_spent: f64,
    /// 新口径：该贸易站对应的赤金日产出（特殊贸易组合时非零）。
    #[serde(skip_serializing)]
    pub gold_consumption: f64,
    pub drone_trade_lmd: f64,
    pub drone_gold_spent: f64,
    pub shift_hours: f64,
    pub paper_efficiency: Efficiency,
    #[serde(skip_serializing)]
    pub final_efficiency: Efficiency,
    pub total_efficiency: Efficiency,
    pub time_factor: f64,
}

/// 统一产出口径：产出 = 单位日产出 × 总效率 × (上班/24)。
pub fn daily_yield(
    unit: &TradeUnitOutput,
    efficiency: &TradeEfficiency,
    shift_hours: f64,
) -> TradeDailyYield {
    let time_factor = shift_hours / 24.0;
    let total_factor = efficiency.total_efficiency.as_f64() * time_factor;
    let paper_factor = if efficiency.applies_paper_efficiency {
        efficiency.paper.paper_efficiency.as_f64()
    } else {
        1.0
    };
    TradeDailyYield {
        trade_lmd: unit.unit_trade_per_day * total_factor,
        gold_spent: unit.unit_gold_per_day * paper_factor * time_factor,
        gold_consumption: unit.gold_consumption_per_day * GSL_GOLD_UNIT_SCALE * total_factor,
        drone_trade_lmd: unit.unit_trade_per_day * total_factor * DRONE_TRADE_FACTOR,
        drone_gold_spent: unit.drone_unit_gold_per_day * paper_factor * time_factor,
        shift_hours,
        paper_efficiency: efficiency.paper.paper_efficiency,
        final_efficiency: efficiency.total_efficiency,
        total_efficiency: efficiency.total_efficiency,
        time_factor,
    }
}

pub fn compute_unit_output(
    ctx: &TradeContext<'_>,
    dist: &GoldDistribution,
    caps: &MechanicCaps,
    mechanic: &OrderMechanicResult,
) -> TradeUnitOutput {
    let (trade_per_slot_day, gold_per_slot_day, originium_per_slot_day, gold_consumption_per_day) =
        match mechanic.dominant_kind {
            SpecialOrderKind::ClosureSpecial => {
                let (t, g) = closure_unit_per_slot_day();
                (t, g, 0.0, g)
            }
            SpecialOrderKind::PepeExclusive => {
                let (t, g) = pepe_unit_per_slot_day();
                (t, g, 0.0, 0.0)
            }
            SpecialOrderKind::NormalOriginium => {
                let ori_dist = mechanic
                    .originium_distribution
                    .as_ref()
                    .expect("originium distribution");
                let (t, s) = super::order_mechanic::originium_unit_per_slot_per_day(ori_dist);
                (t, 0.0, s, 0.0)
            }
            SpecialOrderKind::NormalGold if caps.closure => {
                let (t, g) = closure_unit_per_slot_day();
                (t, g, 0.0, g)
            }
            SpecialOrderKind::NormalGold => {
                let (t, g) = unit_per_slot_per_day(dist, caps, ctx.order_lmd_bonus);
                (t, g, 0.0, 0.0)
            }
        };

    let baseline_lv3 = if mechanic.order_kind == crate::trade::input::TradeOrderKind::Originium {
        super::order_mechanic::baseline_unit_trade_lv3_originium()
    } else {
        let vanilla = MechanicCaps::default();
        unit_per_slot_per_day(&GoldDistribution::for_trade_level(3), &vanilla, 0).0
    };
    let multiplier = if baseline_lv3 > 0.0 {
        trade_per_slot_day / baseline_lv3
    } else {
        1.0
    };

    TradeUnitOutput {
        unit_trade_per_day: trade_per_slot_day,
        unit_gold_per_day: gold_per_slot_day,
        gold_consumption_per_day,
        unit_originium_per_day: originium_per_slot_day,
        multiplier_vs_lv3_regular: multiplier,
        drone_unit_trade_per_day: trade_per_slot_day * DRONE_TRADE_FACTOR,
        drone_unit_gold_per_day: gold_per_slot_day * DRONE_TRADE_FACTOR,
        drone_unit_originium_per_day: originium_per_slot_day * DRONE_TRADE_FACTOR,
    }
}

/// 佩佩特别独占订单：4:30:00，0 赤金，1000 龙门币（PRTS §慧眼独到脚注）.
fn pepe_unit_per_slot_day() -> (f64, f64) {
    (
        PEPE_ORDER_LMD / PEPE_ORDER_DURATION_MIN * MINUTES_PER_DAY,
        0.0,
    )
}

/// 可露希尔特别订单：2:24:00，2 赤金，1200 龙门币（公孙表单位产出锚点）.
fn closure_unit_per_slot_day() -> (f64, f64) {
    let dur = 144.0;
    let lmd = 1200.0;
    let gold = 2.0;
    (lmd / dur * MINUTES_PER_DAY, gold / dur * MINUTES_PER_DAY)
}
