use serde::Serialize;

use crate::error::Result;
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::interpreter::{apply_trade_phases, TradeContext};
use crate::trade::order_mechanic::{self, OrderMechanicResult};
use crate::trade::shortcut;

#[derive(Debug, Clone, Serialize)]
pub struct OperatorMoodDrain {
    pub name: String,
    pub drain_delta_per_hour: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeResult {
    pub order_eff_base: f64,
    pub order_eff_skill: f64,
    pub order_eff_total: f64,
    pub order_eff_pre_shortcut: f64,
    pub final_order_limit: i32,
    pub order_mechanic: OrderMechanicResult,
    pub effective_eff_multiplier: f64,
    pub trade_shortcut: Option<String>,
    pub mood_drain: Vec<OperatorMoodDrain>,
}

fn mood_drain_from_ctx(ctx: &TradeContext) -> Vec<OperatorMoodDrain> {
    ctx.mood_drain_summary()
        .into_iter()
        .map(|(name, drain_delta_per_hour)| OperatorMoodDrain {
            name,
            drain_delta_per_hour,
        })
        .collect()
}

pub fn solve_trade(input: &TradeRoomInput, table: &SkillTable) -> Result<TradeResult> {
    let mut ctx = TradeContext::from_room(input);
    apply_trade_phases(&mut ctx, table);

    let order_eff_base = ctx.order_eff_base();
    let order_eff_skill = ctx.order_eff_skill();
    let order_eff_pre = ctx.order_eff_total();

    if let Some(sc) =
        shortcut::resolve_trade_shortcut(&input.operators, table, order_eff_pre, input.level)
    {
        let mechanic = sc.build_mechanic_result(input.level);
        let order_eff_total = sc.entry.trade_pct;
        let order_eff_skill_adj = order_eff_total - order_eff_base;
        return Ok(TradeResult {
            order_eff_base,
            order_eff_skill: order_eff_skill_adj,
            order_eff_total,
            order_eff_pre_shortcut: order_eff_pre,
            final_order_limit: ctx.final_order_limit,
            effective_eff_multiplier: sc.effective_multiplier(),
            order_mechanic: mechanic,
            trade_shortcut: Some(sc.entry.id),
            mood_drain: mood_drain_from_ctx(&ctx),
        });
    }

    let mechanic = order_mechanic::resolve_order_mechanic(&ctx, order_eff_pre);
    let effective_eff_multiplier = mechanic.effective_eff_multiplier(order_eff_pre);

    Ok(TradeResult {
        order_eff_base,
        order_eff_skill,
        order_eff_total: order_eff_pre,
        order_eff_pre_shortcut: order_eff_pre,
        final_order_limit: ctx.final_order_limit,
        order_mechanic: mechanic,
        effective_eff_multiplier,
        trade_shortcut: None,
        mood_drain: mood_drain_from_ctx(&ctx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_table::SkillTable;
    use crate::trade::input::{TradeOperator, TradeRoomInput};

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    fn op(name: &str, elite: u8, buff_ids: Vec<&str>) -> TradeOperator {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    }

    fn room(level: u8, operators: Vec<TradeOperator>) -> TradeRoomInput {
        TradeRoomInput::with_operators(level, operators)
    }

    fn closure_tier90_room() -> TradeRoomInput {
        room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
                op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]),
            ],
        )
    }

    #[test]
    fn gsl_closure_tier90_regression() {
        let result = solve_trade(&closure_tier90_room(), &table()).unwrap();
        assert_eq!(result.trade_shortcut.as_deref(), Some("gsl_closure_tier90"));
        assert!((result.order_eff_pre_shortcut - 134.0).abs() < 1.0);
        assert!((result.order_eff_total - 135.0).abs() < 2.0);
        assert!((result.order_mechanic.mechanic_equiv_eff_pct - 42.0).abs() < 2.0);
    }

    #[test]
    fn gsl_closure_tier80_regression() {
        let input = room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
                op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_eq!(result.trade_shortcut.as_deref(), Some("gsl_closure_tier80"));
        assert!((result.order_eff_total - 124.0).abs() < 2.0);
    }

    #[test]
    fn gsl_closure_tier60_regression() {
        let input = room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("能天使", 2, vec!["trade_ord_spd[010]", "trade_ord_spd[020]"]),
                op("德克萨斯", 0, vec!["trade_ord_spd&cost_P[000]"]),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_eq!(result.trade_shortcut.as_deref(), Some("gsl_closure_tier60"));
        assert!((result.order_eff_total - 100.0).abs() < 2.0);
    }

    fn witch_room(shortcut_id: &str) -> TradeRoomInput {
        match shortcut_id {
            "gsl_witch_long_beta" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
                    op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
                ],
            ),
            "gsl_witch_long_docus" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
                    op(
                        "但书",
                        2,
                        vec!["trade_ord_law[000]", "trade_ord_against[010]"],
                    ),
                ],
            ),
            "gsl_witch_long_alpha" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
                    op("折光", 0, vec!["trade_ord_wt&cost[002]"]),
                ],
            ),
            "gsl_witch_long_blank" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
                    op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
                ],
            ),
            "gsl_witch_long0_blank" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("龙舌兰", 0, vec!["trade_ord_long[000]"]),
                    op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
                ],
            ),
            "gsl_witch_beta_blank" => room(
                3,
                vec![
                    op(
                        "巫恋",
                        2,
                        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
                    ),
                    op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]),
                    op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
                ],
            ),
            _ => panic!("unknown witch shortcut {shortcut_id}"),
        }
    }

    #[test]
    fn gsl_witch_regressions() {
        let table = table();
        let cases = [
            ("gsl_witch_long_beta", 138.0, 46.0),
            ("gsl_witch_long_docus", 178.0, 33.5),
            ("gsl_witch_long_alpha", 129.0, 38.0),
            ("gsl_witch_long_blank", 124.0, 33.0),
            ("gsl_witch_long0_blank", 108.0, 17.0),
            ("gsl_witch_beta_blank", 93.0, 0.0),
        ];
        for (id, trade, gold) in cases {
            let result = solve_trade(&witch_room(id), &table).unwrap();
            assert_eq!(
                result.trade_shortcut.as_deref(),
                Some(id),
                "shortcut for {id}"
            );
            assert!(
                (result.order_eff_total - trade).abs() < 0.5,
                "{id} trade got {}",
                result.order_eff_total
            );
            assert!(
                (result.order_mechanic.mechanic_equiv_eff_pct - gold).abs() < 0.5,
                "{id} gold got {}",
                result.order_mechanic.mechanic_equiv_eff_pct
            );
        }
    }

    #[test]
    fn docus_breach_mechanic() {
        let input = room(
            3,
            vec![op(
                "但书",
                2,
                vec!["trade_ord_law[000]", "trade_ord_against[010]"],
            )],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert!(result.order_mechanic.mechanic_equiv_eff_pct > 0.0);
    }
}
