//! 赤金生产线链式结算（贸易站内按进驻顺序迭代）。
//!
//! **L2 域短路**（见 `docs/EFFECT_ATOM_DESIGN.md` §8.5）：`skill_table` 中对应 buff 的
//! `atoms` 为空是刻意委托，不在此重复建模为 EffectAtom。
//!
//! 真实制造赤金线数由 `TradeRoomInput::gold_production_lines` 注入；
//! 虚拟线由绮良/鸿雪·杜林等技能在同站按顺序累加。

use crate::skill_table::SkillTable;
use crate::trade::interpreter::TradeContext;

/// 在 `PeerAbsorb` 之前执行：按干员进驻顺序结算赤金链效率。
pub fn apply_gold_flow_chain(ctx: &mut TradeContext, table: &SkillTable) {
    let real = ctx.real_gold_lines;
    let mut virtual_lines = ctx.virtual_gold_lines;
    let names: Vec<String> = ctx.operators.iter().map(|o| o.name.clone()).collect();

    for name in &names {
        let buff_ids: Vec<String> = ctx
            .operators
            .iter()
            .find(|o| o.name == *name)
            .map(|o| o.buff_ids.clone())
            .unwrap_or_default();

        let mut extra = 0.0f64;
        for bid in &buff_ids {
            if table.get(bid).is_none() {
                continue;
            }
            let total = real + virtual_lines;
            match bid.as_str() {
                "trade_ord_line_gold[010]" => {
                    extra += 5.0;
                    virtual_lines += (real / 2) * 2;
                }
                "trade_ord_line_gold[000]" => {
                    extra += 5.0;
                    virtual_lines += (real / 4) * 2;
                }
                "trade_ord_line_durin[010]" => {
                    virtual_lines += ctx.durin_virtual_lines;
                }
                "trade_ord_spd&gold[100]" => {
                    extra += total as f64 * 5.0;
                }
                "trade_ord_spd&gold[010]" => {
                    extra += 5.0 + (total / 2) as f64 * 15.0;
                }
                "trade_ord_spd&gold[000]" => {
                    extra += 5.0 + (total / 4) as f64 * 15.0;
                }
                _ => {}
            }
        }

        if extra > 0.0 {
            if let Some(idx) = ctx.operators.iter().position(|o| o.name == *name) {
                ctx.operators[idx].settled_eff += extra;
            }
        }
        ctx.virtual_gold_lines = virtual_lines;
    }
}

#[cfg(test)]
mod tests {
    use crate::skill_table::default_skill_table_path;
    use crate::skill_table::SkillTable;
    use crate::trade::input::{TradeOperator, TradeRoomInput};
    use crate::trade::interpreter::{apply_trade_phases, TradeContext};

    fn table() -> SkillTable {
        SkillTable::load(&default_skill_table_path().unwrap()).unwrap()
    }

    #[test]
    fn hongxue_spd_per_gold_line() {
        let table = table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "鸿雪".into(),
                elite: 0,
                buff_ids: vec!["trade_ord_spd&gold[100]".into()],
            tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: Some(4),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let hx = ctx.operators.first().unwrap();
        assert!((hx.settled_eff - 20.0).abs() < 0.01, "eff={}", hx.settled_eff);
    }

    #[test]
    fn kira_line_then_hongxue_chain_four_manu() {
        let table = table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "绮良".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_line_gold[010]".into()],
                tags: vec![],
                },
                TradeOperator {
                    name: "鸿雪".into(),
                    elite: 2,
                    buff_ids: vec![
                        "trade_ord_line_durin[010]".into(),
                        "trade_ord_spd&gold[100]".into(),
                    ],
            tags: vec![],
        },
                TradeOperator {
                    name: "图耶".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&gold[010]".into()],
                tags: vec![],
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: Some(4),
            durin_virtual_lines: Some(4),
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let skill_total: f64 = ctx
            .operators
            .iter()
            .map(|o| o.settled_eff + o.variable_eff)
            .sum();
        assert!(
            (skill_total - 160.0).abs() < 1.0,
            "gold-flow chain total={skill_total}"
        );
    }
}
