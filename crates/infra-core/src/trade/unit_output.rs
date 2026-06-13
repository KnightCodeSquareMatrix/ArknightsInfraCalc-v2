//! **场景产量层**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.11）：单位产出 / 倍率 / 无人机。
//! 不新增 EffectAtom；由 L2 分布 + 订单效率% 与上班时长组合。

use serde::Serialize;

use super::interpreter::{MechanicCaps, TradeContext};
use super::order_mechanic::{
    unit_per_slot_per_day, GoldDistribution, OrderMechanicResult, OriginiumDistribution,
    SpecialOrderKind, PEPE_ORDER_DURATION_MIN, PEPE_ORDER_LMD,
};

pub use super::order_mechanic::baseline_unit_trade_lv3_regular;

/// 公孙长乐工具人表：无人机贸易产出折算系数。
pub const DRONE_TRADE_FACTOR: f64 = 0.685;
/// 工具人表「单位赤金产出」与内部日消耗量的换算（closure 锚点 20→2000）。
pub const GSL_GOLD_UNIT_SCALE: f64 = 100.0;

const MINUTES_PER_DAY: f64 = 1440.0;

#[derive(Debug, Clone, Serialize)]
pub struct TradeUnitOutput {
    /// 24h、效率总和=100% 时的日贸易产出（龙门币）。
    pub unit_trade_per_day: f64,
    /// 24h、效率总和=100% 时的日赤金消耗。
    pub unit_gold_per_day: f64,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeDailyYield {
    pub trade_lmd: f64,
    pub gold_spent: f64,
    pub drone_trade_lmd: f64,
    pub drone_gold_spent: f64,
    pub shift_hours: f64,
    pub eff_factor: f64,
}

/// 公孙公式：产出 = 效率总和 × (上班/24) × 加强单位产出；此处 `order_eff_total_pct` 即纸面效率总和%。
/// 佩佩独占单固定 cadence，`ignore_order_eff=true` 时仅按 `(shift/24)` 缩放。
pub fn daily_yield(
    unit: &TradeUnitOutput,
    order_eff_total_pct: f64,
    shift_hours: f64,
    ignore_order_eff: bool,
) -> TradeDailyYield {
    let eff_factor = if ignore_order_eff {
        shift_hours / 24.0
    } else {
        (order_eff_total_pct / 100.0) * (shift_hours / 24.0)
    };
    TradeDailyYield {
        trade_lmd: unit.unit_trade_per_day * eff_factor,
        gold_spent: unit.unit_gold_per_day * eff_factor,
        drone_trade_lmd: unit.drone_unit_trade_per_day * eff_factor,
        drone_gold_spent: unit.drone_unit_gold_per_day * eff_factor,
        shift_hours,
        eff_factor,
    }
}

pub fn compute_unit_output(
    ctx: &TradeContext,
    dist: &GoldDistribution,
    caps: &MechanicCaps,
    mechanic: &OrderMechanicResult,
) -> TradeUnitOutput {
    let (trade_per_slot_day, gold_per_slot_day, originium_per_slot_day) =
        match mechanic.dominant_kind {
            SpecialOrderKind::ClosureSpecial => {
                let (t, g) = closure_unit_per_slot_day();
                (t, g, 0.0)
            }
            SpecialOrderKind::PepeExclusive => {
                let (t, g) = pepe_unit_per_slot_day();
                (t, g, 0.0)
            }
            SpecialOrderKind::NormalOriginium => {
                let ori_dist = mechanic
                    .originium_distribution
                    .as_ref()
                    .expect("originium distribution");
                let (t, s) = super::order_mechanic::originium_unit_per_slot_per_day(ori_dist);
                (t, 0.0, s)
            }
            SpecialOrderKind::NormalGold if caps.closure => {
                let (t, g) = closure_unit_per_slot_day();
                (t, g, 0.0)
            }
            SpecialOrderKind::NormalGold => {
                let (t, g) = unit_per_slot_per_day(dist, caps, ctx.order_lmd_bonus);
                (t, g, 0.0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_table::SkillTable;
    use crate::trade::input::{TradeOperator, TradeRoomInput};
    use crate::trade::interpreter::TradeContext;
    use crate::trade::order_mechanic::{resolve_order_mechanic, SpecialOrderKind};
    use crate::trade::solve_trade;

    fn solve_unit(level: u8, operators: Vec<TradeOperator>) -> (TradeUnitOutput, f64) {
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let input = TradeRoomInput::with_operators(level, operators);
        let result = solve_trade(&input, &table).unwrap();
        (result.production.unit.clone(), result.order_eff_total)
    }

    #[test]
    fn closure_unit_output_near_gsl_anchor() {
        let table = SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let input = TradeRoomInput::with_operators(
            3,
            vec![TradeOperator::new(
                "可露希尔",
                2,
                vec!["trade_ord_closure[000]".into()],
            )],
        );
        let result = solve_trade(&input, &table).unwrap();
        let unit = &result.production.unit;
        let eff = result.order_eff_total;
        let daily = daily_yield(unit, eff, 24.0, false);
        assert!(
            (unit.unit_trade_per_day - 12_000.0).abs() < 800.0,
            "closure unit trade got {:.1}",
            unit.unit_trade_per_day
        );
        assert!(
            (unit.gsl_unit_gold() - 2_000.0).abs() < 200.0,
            "closure gsl gold got {:.1}",
            unit.gsl_unit_gold()
        );
        assert!(daily.drone_trade_lmd > 0.0);
    }

    #[test]
    fn docus_trade_level_monotonic() {
        let docus = |elite: u8| {
            TradeOperator::new(
                "但书",
                elite,
                if elite >= 2 {
                    vec![
                        "trade_ord_law[000]".into(),
                        "trade_ord_against[010]".into(),
                    ]
                } else {
                    vec![
                        "trade_ord_law[000]".into(),
                        "trade_ord_against[000]".into(),
                    ]
                },
            )
        };
        let (u3, _) = solve_unit(3, vec![docus(2)]);
        let (u2, _) = solve_unit(2, vec![docus(2)]);
        let (u1, _) = solve_unit(1, vec![docus(0)]);
        assert!(
            u1.unit_trade_per_day > u2.unit_trade_per_day
                && u2.unit_trade_per_day > u3.unit_trade_per_day,
            "lv1={:.0} lv2={:.0} lv3={:.0}",
            u1.unit_trade_per_day,
            u2.unit_trade_per_day,
            u3.unit_trade_per_day
        );
        assert!((u3.unit_trade_per_day - 15_929.2).abs() < 3_000.0);
        assert!((u2.unit_trade_per_day - 18_591.55).abs() < 3_500.0);
        assert!((u1.unit_trade_per_day - 20_000.0).abs() < 3_500.0);
    }

    #[test]
    fn pepe_exclusive_unit_and_ignores_paper_eff() {
        let table = SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let pepe = |extra: Vec<TradeOperator>| {
            let mut ops = vec![TradeOperator::new(
                "佩佩",
                2,
                vec![
                    "trade_ord_limit&trade&lv[000]".into(),
                    "trade_ord_pepe[000]".into(),
                ],
            )];
            ops.extend(extra);
            TradeRoomInput::with_operators(3, ops)
        };
        let solo = solve_trade(&pepe(vec![]), &table).unwrap();
        let expected_unit = PEPE_ORDER_LMD / PEPE_ORDER_DURATION_MIN * MINUTES_PER_DAY;
        assert!(
            (solo.production.unit.unit_trade_per_day - expected_unit).abs() < 0.01,
            "pepe unit {:.1}",
            solo.production.unit.unit_trade_per_day
        );
        assert!((solo.production.unit.unit_gold_per_day).abs() < 1e-6);
        assert_eq!(
            solo.order_mechanic.dominant_kind,
            SpecialOrderKind::PepeExclusive
        );

        let with_limit_tools = pepe(vec![
            TradeOperator::new("暗索", 0, vec!["trade_ord_limit&cost[000]".into()]),
            TradeOperator::new("桃金娘", 0, vec!["trade_ord_limit&cost[000]".into()]),
        ]);
        let with_limits = solve_trade(&with_limit_tools, &table).unwrap();
        assert!(
            (with_limits.effective_eff_multiplier - solo.effective_eff_multiplier).abs() < 1e-6,
            "pepe score unchanged with limit-only roommates"
        );
        assert!(
            (with_limits.production.daily_at_shift.trade_lmd - solo.production.daily_at_shift.trade_lmd)
                .abs()
                < 1.0,
            "daily lmd must not scale with paper eff"
        );

        let with_eff_tools = pepe(vec![
            TradeOperator::new(
                "能天使",
                2,
                vec!["trade_ord_spd[010]".into(), "trade_ord_spd[020]".into()],
            ),
            TradeOperator::new("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]".into()]),
        ]);
        assert!(
            solve_trade(&with_eff_tools, &table).is_err(),
            "pepe+效率人应被 trade_station_exclusive_violation 拒绝"
        );
    }

    #[test]
    fn drone_factor_applied() {
        let caps = MechanicCaps::default();
        let ctx = TradeContext::from_room(&TradeRoomInput::with_operators(3, vec![]));
        let mech = resolve_order_mechanic(&ctx, 3.0);
        let unit = compute_unit_output(&ctx, &mech.gold_distribution, &caps, &mech);
        assert!(
            (unit.drone_unit_trade_per_day - unit.unit_trade_per_day * DRONE_TRADE_FACTOR).abs()
                < 1e-6
        );
    }
}
