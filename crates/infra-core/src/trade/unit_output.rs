//! **场景产量层**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.11）：单位产出 / 倍率 / 无人机。
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
    ctx: &TradeContext,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::instances::OperatorInstances;
    use crate::skill_table::SkillTable;
    use crate::trade::input::{TradeOperator, TradeRoomInput};
    use crate::trade::interpreter::TradeContext;
    use crate::trade::order_mechanic::{resolve_order_mechanic, SpecialOrderKind};
    use crate::trade::solve_trade;

    fn solve_unit(level: u8, operators: Vec<TradeOperator>) -> (TradeUnitOutput, Efficiency) {
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let input = TradeRoomInput::with_operators(level, operators);
        let result = solve_trade(&input, &table).unwrap();
        (
            result.production.unit.clone(),
            result.efficiency.final_efficiency,
        )
    }

    #[test]
    fn regular_trade_unit_output_keeps_lv2_community_anchor() {
        let (unit, _) = solve_unit(2, vec![]);
        assert_eq!(regular_trade_unit_output_per_day(2), 10_141.0);
        assert_eq!(unit.unit_trade_per_day, 10_141.0);
    }

    #[test]
    fn closure_unit_output_near_gsl_anchor() {
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
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
        let daily = &result.production.daily_at_shift;
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
        assert!((daily.gold_consumption - 2_060.0).abs() < 250.0);
        assert!(daily.drone_trade_lmd > 0.0);
    }

    #[test]
    fn closure_with_exusiai_and_lemuen_reports_unified_12h_values() {
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let operator = |name: &str| {
            TradeOperator::new(
                name,
                2,
                instances.resolve_trade_buff_ids(name, crate::tier::PromotionTier::TierUp),
            )
        };
        let input = TradeRoomInput {
            level: 3,
            operators: vec![operator("可露希尔"), operator("能天使"), operator("蕾缪安")],
            order_count: None,
            mood: 24.0,
            gold_production_lines: Some(2),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(crate::layout::LayoutContext::search_baseline()),
            active_order_kind: crate::trade::input::TradeOrderKind::Gold,
        };
        let result = crate::trade::solve_trade_with_shift(&input, &table, 12.0).unwrap();
        println!("{:#?}", result);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("total_efficiency"));
        assert!(!json.contains("final_efficiency"));
        assert_eq!(
            result.efficiency.total_efficiency,
            result.efficiency.paper.paper_efficiency
        );
        assert!((result.efficiency.trade_equivalent_efficiency.as_f64() - 1.226).abs() < 0.001);
        assert!((result.efficiency.gold_equivalent_efficiency.as_f64() - 0.386).abs() < 0.001);
        assert!(
            (result.production.daily_at_shift.trade_lmd
                - 12_000.0 * result.efficiency.total_efficiency.as_f64() * 0.5)
                .abs()
                < 0.01
        );
        assert!(
            (result.production.daily_at_shift.gold_consumption
                - 2_000.0 * result.efficiency.total_efficiency.as_f64() * 0.5)
                .abs()
                < 0.01
        );
    }

    #[test]
    fn docus_trade_level_monotonic() {
        let docus = |elite: u8| {
            TradeOperator::new(
                "但书",
                elite,
                if elite >= 2 {
                    vec!["trade_ord_law[000]".into(), "trade_ord_against[010]".into()]
                } else {
                    vec!["trade_ord_law[000]".into(), "trade_ord_against[000]".into()]
                },
            )
        };
        let (u3, _) = solve_unit(3, vec![docus(2)]);
        let (u2, _) = solve_unit(2, vec![docus(2)]);
        let (u1, _) = solve_unit(1, vec![docus(0)]);
        assert!(u1.unit_trade_per_day < u2.unit_trade_per_day);
        assert!(u3.unit_trade_per_day < u2.unit_trade_per_day);
        assert!((u3.unit_trade_per_day - 15_929.2).abs() < 0.01);
        assert!((u2.unit_trade_per_day - 18_591.55).abs() < 0.01);
        assert!((u1.unit_trade_per_day - 15_000.0).abs() < 0.01);
    }

    #[test]
    fn docus_e0_uses_per_level_unit_output_mapping() {
        let docus_e0 = TradeOperator::new(
            "但书",
            0,
            vec!["trade_ord_law[000]".into(), "trade_ord_against[000]".into()],
        );
        let expected = [(1, 15_000.0), (2, 14_366.197), (3, 13_097.345)];
        for (level, unit_output) in expected {
            let (unit, _) = solve_unit(level, vec![docus_e0.clone()]);
            assert!((unit.unit_trade_per_day - unit_output).abs() < 0.001);
        }
    }

    #[test]
    fn witch_alpha_unit_output_uses_tequila_elite_mapping() {
        let witch = TradeOperator::new(
            "巫恋",
            2,
            vec![
                "trade_ord_vodfox[000]".into(),
                "trade_ord_wt&cost[000]".into(),
            ],
        );
        let tailor = TradeOperator::new("柏喙", 0, vec!["trade_ord_wt&cost[000]".into()]);
        let (e0, _) = solve_unit(
            3,
            vec![
                witch.clone(),
                TradeOperator::new("龙舌兰", 0, vec!["trade_ord_long[000]".into()]),
                tailor.clone(),
            ],
        );
        assert!((e0.unit_trade_per_day - 11_192.9).abs() < 0.001);
        assert!((e0.gsl_unit_gold() - 837.56).abs() < 0.001);

        let (e2, _) = solve_unit(
            3,
            vec![
                witch,
                TradeOperator::new("龙舌兰", 2, vec!["trade_ord_long[010]".into()]),
                tailor,
            ],
        );
        assert!((e2.unit_trade_per_day - 12_030.46).abs() < 0.001);
        assert!((e2.gsl_unit_gold() - 1_675.127).abs() < 0.001);
    }

    #[test]
    fn lv3_originium_trade_uses_fixed_orundum_unit_output() {
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let input = TradeRoomInput {
            level: 3,
            operators: Vec::new(),
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(crate::layout::LayoutContext::default()),
            active_order_kind: crate::trade::input::TradeOrderKind::Originium,
        };
        let result = solve_trade(&input, &table).unwrap();
        assert_eq!(result.production.unit.unit_trade_per_day, 240.0);
        assert_eq!(result.production.unit.unit_originium_per_day, 24.0);
        let mut invalid = input;
        invalid.level = 2;
        assert!(solve_trade(&invalid, &table).is_err());
    }

    #[test]
    fn pepe_exclusive_unit_and_ignores_paper_eff() {
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
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
            (with_limits.efficiency.final_efficiency.as_f64()
                - solo.efficiency.final_efficiency.as_f64())
            .abs()
                < 1e-6,
            "pepe score unchanged with limit-only roommates"
        );
        assert!(
            (with_limits.production.daily_at_shift.trade_lmd
                - solo.production.daily_at_shift.trade_lmd)
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
