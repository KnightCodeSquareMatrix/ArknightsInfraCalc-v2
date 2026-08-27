use serde::Serialize;

use crate::error::Result;
use crate::skill_table::SkillTable;
use crate::trade::efficiency::{
    PaperTradeEfficiency, TradeEfficiency, GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY,
};
use crate::trade::input::{TradeOrderKind, TradeRoomInput};
use crate::trade::interpreter::{apply_trade_phases, TradeContext};
use crate::trade::order_mechanic::{self, OrderMechanicResult, SpecialOrderKind};
use crate::trade::shortcut;
use crate::trade::unit_output::{
    compute_unit_output, daily_yield, regular_trade_unit_output_per_day, TradeDailyYield,
    TradeUnitOutput,
};

#[derive(Debug, Clone, Serialize)]
pub struct OperatorMoodDrain {
    pub name: String,
    pub drain_delta_per_hour: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeResult {
    pub final_order_limit: i32,
    pub order_mechanic: OrderMechanicResult,
    pub efficiency: TradeEfficiency,
    pub rule_id: Option<String>,
    pub mood_drain: Vec<OperatorMoodDrain>,
    pub production: TradeProductionReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeProductionReport {
    pub unit: crate::trade::unit_output::TradeUnitOutput,
    pub daily_at_shift: TradeDailyYield,
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

fn build_production(
    unit: TradeUnitOutput,
    efficiency: &TradeEfficiency,
    shift_hours: f64,
) -> TradeProductionReport {
    TradeProductionReport {
        daily_at_shift: daily_yield(&unit, efficiency, shift_hours),
        unit,
    }
}

fn resolve_unit_output(
    ctx: &TradeContext,
    mechanic: &OrderMechanicResult,
    sc: Option<&shortcut::TradeShortcutMatch>,
    level: u8,
) -> Result<TradeUnitOutput> {
    let reference = if mechanic.order_kind.is_gold() {
        GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY
    } else {
        order_mechanic::baseline_unit_trade_lv3_originium()
    };
    let docus = mechanic.order_kind.is_gold() && ctx.mechanic_caps().law;
    let mut unit = if docus {
        let caps = ctx.mechanic_caps();
        compute_unit_output(ctx, &mechanic.gold_distribution, &caps, mechanic)
    } else {
        sc.and_then(|m| m.unit_output_from_anchor(reference))
            .unwrap_or_else(|| {
                let caps = ctx.mechanic_caps();
                compute_unit_output(ctx, &mechanic.gold_distribution, &caps, mechanic)
            })
    };

    let community_trade_unit = if docus {
        let docus_e2 = ctx
            .operators
            .iter()
            .find(|operator| operator.name == "但书")
            .is_some_and(|operator| operator.elite >= 2);
        let shortcut_id = if docus_e2 {
            "gsl_docus_solo"
        } else {
            "gsl_docus_solo_e0"
        };
        Some(shortcut::required_community_unit_trade_per_day_by_id(
            shortcut_id,
            level,
            reference,
        )?)
    } else {
        sc.and_then(|m| m.community_unit_trade_per_day(level, reference))
    };

    if let Some(unit_trade_per_day) = community_trade_unit {
        unit.replace_trade_unit_output(unit_trade_per_day, reference);
    } else if mechanic.order_kind.is_gold()
        && sc.is_none()
        && mechanic.dominant_kind == SpecialOrderKind::NormalGold
        && mechanic.mechanic_equivalent_efficiency.is_zero()
    {
        unit.replace_trade_unit_output(regular_trade_unit_output_per_day(level), reference);
    }
    if let Some(gsl_unit_gold) = sc.and_then(|matched| matched.entry.unit_gsl_gold_anchor) {
        unit.replace_gsl_gold_unit_output(gsl_unit_gold);
    }

    Ok(unit)
}

fn build_efficiency(
    paper: PaperTradeEfficiency,
    unit: &TradeUnitOutput,
    mechanic: &OrderMechanicResult,
    rule_id: Option<String>,
) -> TradeEfficiency {
    let reference = if mechanic.order_kind.is_gold() {
        GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY
    } else {
        order_mechanic::baseline_unit_trade_lv3_originium()
    };
    TradeEfficiency::new(
        paper,
        reference,
        unit.unit_trade_per_day,
        unit.gold_consumption_per_day * crate::trade::unit_output::GSL_GOLD_UNIT_SCALE,
        rule_id,
        !mechanic.ignores_order_eff(),
    )
}

pub fn solve_trade(input: &TradeRoomInput, table: &SkillTable) -> Result<TradeResult> {
    solve_trade_with_shift(input, table, 24.0)
}

/// 搜索热路径：调用方已做 `trade_station_exclusive_violation` 预筛时跳过重复校验。
pub(crate) fn solve_trade_with_shift_prevalidated(
    input: &TradeRoomInput,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<TradeResult> {
    solve_trade_with_shift_inner(input, table, shift_hours, false)
}

pub fn solve_trade_with_shift(
    input: &TradeRoomInput,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<TradeResult> {
    solve_trade_with_shift_inner(input, table, shift_hours, true)
}

fn solve_trade_with_shift_inner(
    input: &TradeRoomInput,
    table: &SkillTable,
    shift_hours: f64,
    check_exclusivity: bool,
) -> Result<TradeResult> {
    crate::profile::record_trade_solve();
    shortcut::ensure_trade_shortcuts_loaded()?;
    if input.active_order_kind == TradeOrderKind::Originium && input.level < 3 {
        return Err(crate::error::Error::msg(
            "originium trade orders require a level-3 trade post",
        ));
    }
    if check_exclusivity && shortcut::trade_station_exclusive_violation(&input.operators, table) {
        return Err(crate::error::Error::msg(
            "trade station violates mechanism exclusivity (docus / closure / witch)",
        ));
    }

    let mut ctx = TradeContext::from_room(input);
    apply_trade_phases(&mut ctx, table);

    let order_eff_base = ctx.order_eff_base();
    let siracusa_skill_eff = input
        .layout
        .global_inject
        .trade_eff_pct_for_family_with_scoped_tag_counts(
            "trade_siracusa_scaling",
            &input.layout.trade_tagged_count_sum,
            &current_room_tag_counts(&ctx),
        );
    let order_eff_skill = ctx.order_eff_skill() + siracusa_skill_eff;
    let order_eff_pre = ctx.order_eff_total();
    let order_eff_global = order_eff_pre - order_eff_base - order_eff_skill;
    let paper_efficiency = PaperTradeEfficiency::from_percent_points(
        order_eff_base,
        order_eff_skill,
        order_eff_global,
    );

    if input.active_order_kind.is_gold() {
        let sc = if check_exclusivity {
            shortcut::resolve_trade_shortcut(
                &input.operators,
                table,
                order_eff_pre,
                input.level,
                &input.layout.global_inject,
            )
        } else {
            shortcut::resolve_trade_shortcut_prevalidated(
                &input.operators,
                table,
                order_eff_pre,
                input.level,
                &input.layout.global_inject,
            )
        };
        if let Some(sc) = sc {
            let mechanic = sc.build_mechanic_result(input.level);
            let unit = resolve_unit_output(&ctx, &mechanic, Some(&sc), input.level)?;
            let efficiency = build_efficiency(
                paper_efficiency,
                &unit,
                &mechanic,
                Some(sc.entry.id.clone()),
            );
            let production = build_production(unit, &efficiency, shift_hours);
            return Ok(TradeResult {
                final_order_limit: ctx.final_order_limit,
                order_mechanic: mechanic,
                efficiency,
                rule_id: Some(sc.entry.id),
                mood_drain: mood_drain_from_ctx(&ctx),
                production,
            });
        }
    }

    let mechanic = order_mechanic::resolve_order_mechanic(&ctx, order_eff_pre);
    let unit = resolve_unit_output(&ctx, &mechanic, None, input.level)?;
    let efficiency = build_efficiency(paper_efficiency, &unit, &mechanic, None);
    let production = build_production(unit, &efficiency, shift_hours);

    Ok(TradeResult {
        final_order_limit: ctx.final_order_limit,
        order_mechanic: mechanic,
        efficiency,
        rule_id: None,
        mood_drain: mood_drain_from_ctx(&ctx),
        production,
    })
}

fn current_room_tag_counts(ctx: &TradeContext) -> std::collections::HashMap<String, u8> {
    let mut counts = std::collections::HashMap::new();
    for operator in &ctx.operators {
        for tag in &operator.tags {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::LayoutContext;
    use crate::skill_table::SkillTable;
    use crate::tier::PromotionTier;
    use crate::trade::input::{TradeOperator, TradeRoomInput};
    use crate::Efficiency;

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

    fn exusiai_e2_buffs(instances: &OperatorInstances) -> Vec<String> {
        instances.resolve_trade_buff_ids("能天使", PromotionTier::TierUp)
    }

    fn closure_tier90_room() -> TradeRoomInput {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let exu = exusiai_e2_buffs(&instances);
        room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("能天使", 2, exu.iter().map(String::as_str).collect()),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
                op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]),
            ],
        )
    }

    #[test]
    fn gsl_closure_tier90_regression() {
        let result = solve_trade(&closure_tier90_room(), &table()).unwrap();
        assert_eq!(result.rule_id.as_deref(), Some("gsl_closure_tier90"));
        assert!(
            ((result
                .order_mechanic
                .mechanic_equivalent_efficiency
                .as_f64()
                * 100.0)
                - 42.0)
                .abs()
                < 2.0
        );
        assert!(result.efficiency.final_efficiency > result.efficiency.paper.paper_efficiency);
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
        assert_eq!(result.rule_id.as_deref(), Some("gsl_closure_tier80"));
        assert!(result.efficiency.final_efficiency > result.efficiency.paper.paper_efficiency);
    }

    #[test]
    fn gsl_closure_tier60_regression() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let exu = exusiai_e2_buffs(&instances);
        let input = room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("能天使", 2, exu.iter().map(String::as_str).collect()),
                op("德克萨斯", 0, vec!["trade_ord_spd&cost_P[000]"]),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_eq!(result.rule_id.as_deref(), Some("gsl_closure_tier60"));
        assert!(result.efficiency.final_efficiency > result.efficiency.paper.paper_efficiency);
    }

    #[test]
    fn exusiai_e2_stepwise_buff_binding() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        assert_eq!(
            exusiai_e2_buffs(&instances),
            vec!["trade_ord_spd[020]".to_string()],
            "精2 物流专家应替换精0 企鹅物流·α，不得双挂 [010]+[020]"
        );
        let input = room(
            3,
            vec![
                op(
                    "能天使",
                    2,
                    exusiai_e2_buffs(&instances)
                        .iter()
                        .map(String::as_str)
                        .collect(),
                ),
                op("香草", 0, vec!["trade_ord_spd[000]"]),
                op("安德切尔", 0, vec!["trade_ord_spd[000]"]),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert!(
            (((result.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0) - 75.0).abs()
                < 4.0,
            "精2 能天使仅 +35%，加两名 20% 级工具人纸面≈75%，got {}",
            ((result.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0)
        );
    }

    #[test]
    fn lemuen_e2_stepwise_buff_binding() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let lemuen = instances.resolve_trade_buff_ids("蕾缪安", PromotionTier::TierUp);
        assert_eq!(
            lemuen,
            vec!["trade_ord_spd&multiPar[100]".to_string()],
            "精2 相伴应替换精0 订单分发·α，不得双挂 [000]+[100]"
        );
    }

    #[test]
    fn heidi_e2_stepwise_buff_binding() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        assert_eq!(
            instances.resolve_trade_buff_ids("海蒂", PromotionTier::Tier0),
            vec!["trade_ord_spd&multiPar[000]".to_string()],
            "精0 仅订单分发·α +20%"
        );
        assert_eq!(
            instances.resolve_trade_buff_ids("海蒂", PromotionTier::TierUp),
            vec!["trade_ord_spd[021]".to_string()],
            "精2 名流欢会应替换订单分发·α，不得双挂 [000]+[021]"
        );
    }

    #[test]
    fn kroos_e2_stepwise_buff_binding() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        assert_eq!(
            instances.resolve_trade_buff_ids("可颂", PromotionTier::TierUp),
            vec!["trade_ord_spd&limit[031]".to_string()]
        );
    }

    #[test]
    fn heidi_kroos_closure_not_tier90() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let heidi = instances.resolve_trade_buff_ids("海蒂", PromotionTier::TierUp);
        let kroos = instances.resolve_trade_buff_ids("可颂", PromotionTier::TierUp);
        let input = room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("海蒂", 2, heidi.iter().map(String::as_str).collect()),
                op("可颂", 2, kroos.iter().map(String::as_str).collect()),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_ne!(
            result.rule_id.as_deref(),
            Some("gsl_closure_tier90"),
            "海蒂(35%)+可颂(30%) 不得命中 90 档，got {:?} pre={}",
            result.rule_id,
            ((result.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0)
        );
    }

    #[test]
    fn lemuen_heidi_closure_not_tier90() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let lemuen = instances.resolve_trade_buff_ids("蕾缪安", PromotionTier::TierUp);
        let heidi = instances.resolve_trade_buff_ids("海蒂", PromotionTier::TierUp);
        let input = room(
            3,
            vec![
                op("可露希尔", 2, vec!["trade_ord_closure[000]"]),
                op("蕾缪安", 2, lemuen.iter().map(String::as_str).collect()),
                op("海蒂", 2, heidi.iter().map(String::as_str).collect()),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_ne!(
            result.rule_id.as_deref(),
            Some("gsl_closure_tier90"),
            "蕾缪安+海蒂 无 能天使，不得命中 90 档可露"
        );
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
            ("gsl_witch_long_alpha", 129.0, 38.0),
            ("gsl_witch_long_blank", 124.0, 33.0),
            ("gsl_witch_long0_blank", 108.0, 17.0),
            ("gsl_witch_beta_blank", 93.0, 0.0),
        ];
        for (id, _trade, gold) in cases {
            let result = solve_trade(&witch_room(id), &table).unwrap();
            assert_eq!(result.rule_id.as_deref(), Some(id), "shortcut for {id}");
            assert!(result.efficiency.final_efficiency >= result.efficiency.paper.paper_efficiency);
            assert!(
                ((result
                    .order_mechanic
                    .mechanic_equivalent_efficiency
                    .as_f64()
                    * 100.0)
                    - gold)
                    .abs()
                    < 0.5,
                "{id} gold got {}",
                (result
                    .order_mechanic
                    .mechanic_equivalent_efficiency
                    .as_f64()
                    * 100.0)
            );
        }
    }

    #[test]
    fn docus_solo_beats_efficiency_tools_without_tailor_group() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let exu_buffs = exusiai_e2_buffs(&instances);
        let exu: Vec<&str> = exu_buffs.iter().map(String::as_str).collect();
        let docus_e0_tools = room(
            3,
            vec![
                op(
                    "但书",
                    0,
                    vec!["trade_ord_law[000]", "trade_ord_against[000]"],
                ),
                op("能天使", 2, exu.clone()),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
            ],
        );
        let docus_e2_tools = room(
            3,
            vec![
                op(
                    "但书",
                    2,
                    vec!["trade_ord_law[000]", "trade_ord_against[010]"],
                ),
                op("能天使", 2, exu.clone()),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
            ],
        );
        let tools_only = room(
            3,
            vec![
                op("能天使", 2, exu),
                op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]),
                op("古米", 0, vec!["trade_ord_spd&cost[000]"]),
            ],
        );
        let witch_long_docus = room(
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
        );

        let r_e0 = solve_trade(&docus_e0_tools, &table).unwrap();
        let r_e2 = solve_trade(&docus_e2_tools, &table).unwrap();
        let r_tools = solve_trade(&tools_only, &table).unwrap();
        assert!(solve_trade(&witch_long_docus, &table).is_err());

        for (label, expected_rule, expected_unit, result) in [
            ("e0", "gsl_docus_solo_e0", 13_097.345, &r_e0),
            ("e2", "gsl_docus_solo", 15_929.2, &r_e2),
        ] {
            assert!(
                result
                    .rule_id
                    .as_deref()
                    .is_some_and(|s| s == expected_rule),
                "docus {label} tools shortcut {:?}",
                result.rule_id
            );
            assert!(
                (result.production.unit.unit_trade_per_day - expected_unit).abs() < 0.001,
                "docus {label} unit multiplier {:?}",
                result.efficiency
            );
            assert!(
                result.efficiency.final_efficiency
                    == result.efficiency.paper.paper_efficiency
                        * result.efficiency.production_basis.unit_output_multiplier,
                "docus {label} final efficiency {:?}",
                result.efficiency
            );
            assert!(
                (result.production.daily_at_shift.trade_lmd
                    - result.production.unit.unit_trade_per_day
                        * result.efficiency.total_efficiency.as_f64())
                .abs()
                    < 1e-6
            );
        }
        assert!(
            r_e0.production.unit.unit_trade_per_day > r_tools.production.unit.unit_trade_per_day,
            "docus e0 unit {:.0} vs tools {:.0}",
            r_e0.production.unit.unit_trade_per_day,
            r_tools.production.unit.unit_trade_per_day
        );
        assert!(r_e2.production.unit.unit_trade_per_day >= r_e0.production.unit.unit_trade_per_day);
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
        assert!(
            (result
                .order_mechanic
                .mechanic_equivalent_efficiency
                .as_f64()
                * 100.0)
                > 0.0
        );
    }

    #[test]
    fn control_amiya_injects_plus_seven_trade_eff() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let mut layout = LayoutContext::default();
        let amiya_buffs = instances.resolve_control_buff_ids("阿米娅", PromotionTier::Tier0);
        crate::control::apply_control_to_layout(
            &mut layout,
            &[crate::control::ControlOperator::new(
                "阿米娅",
                0,
                amiya_buffs,
            )],
            &table,
            24.0,
        );
        let mut input = TradeRoomInput::with_operators(
            3,
            vec![
                TradeOperator::new("能天使", 2, vec!["trade_ord_spd&limit[033]".into()]),
                TradeOperator::new("德克萨斯", 2, vec!["trade_ord_spd&limit[030]".into()]),
                TradeOperator::new("空弦", 2, vec!["trade_ord_spd&limit[040]".into()]),
            ],
        );
        input.layout = Arc::new(layout);
        let without = {
            let mut plain = input.clone();
            Arc::make_mut(&mut plain.layout).global_inject = Default::default();
            (solve_trade(&plain, &table)
                .unwrap()
                .efficiency
                .paper
                .paper_efficiency
                .as_f64()
                - 1.0)
                * 100.0
        };
        let with = (solve_trade(&input, &table)
            .unwrap()
            .efficiency
            .paper
            .paper_efficiency
            .as_f64()
            - 1.0)
            * 100.0;
        assert!((with - without - 7.0).abs() < 0.01);
    }

    #[test]
    fn karlan_silverash_jiaofeng_vs_jie_account() {
        use crate::instances::OperatorInstances;
        use crate::pool::build_trade_pool;
        use crate::roster::Roster;
        use crate::search::TradeSearchOptions;
        use crate::trade::input::{TradeOrderKind, TradeRoomInput};

        let table = table();
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map(
            [
                ("银灰".into(), 2),
                ("角峰".into(), 0),
                ("讯使".into(), 1),
                ("孑".into(), 0),
            ]
            .into_iter()
            .collect(),
        );
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();

        fn ops_from_pool(pool: &crate::pool::TradePool, names: &[&str]) -> Vec<TradeOperator> {
            names
                .iter()
                .map(|n| {
                    pool.entries
                        .iter()
                        .find(|e| e.name == *n)
                        .unwrap_or_else(|| panic!("missing {n}"))
                        .to_trade_operator()
                })
                .collect()
        }

        let opts = TradeSearchOptions::default();
        let base = TradeRoomInput {
            level: opts.trade_level,
            operators: vec![],
            order_count: None,
            mood: opts.mood,
            gold_production_lines: Some(opts.gold_production_lines),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: opts.layout.clone(),
            active_order_kind: TradeOrderKind::Gold,
        };

        // 孑精0（摊贩）替角峰：账号默认练度
        for (label, names) in [
            ("角峰", ["银灰", "角峰", "讯使"]),
            ("孑e0", ["银灰", "孑", "讯使"]),
        ] {
            let mut input = base.clone();
            input.operators = ops_from_pool(&pool, &names);
            let r = solve_trade_with_shift(&input, &table, 24.0).unwrap();
            eprintln!(
                "{label}: score={:.3} trade={:.1} limit={}",
                r.efficiency.final_efficiency.as_f64(),
                ((r.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0),
                r.final_order_limit,
            );
        }

        let mut with_jiao = base.clone();
        with_jiao.operators = ops_from_pool(&pool, &["银灰", "角峰", "讯使"]);
        let mut with_jie = base.clone();
        with_jie.operators = ops_from_pool(&pool, &["银灰", "孑", "讯使"]);
        let r_jiao = solve_trade_with_shift(&with_jiao, &table, 24.0).unwrap();
        let r_jie = solve_trade_with_shift(&with_jie, &table, 24.0).unwrap();
        assert!(
            r_jie.efficiency.final_efficiency.as_f64()
                > r_jiao.efficiency.final_efficiency.as_f64(),
            "精0孑+摊贩应高于角峰: jie={:.3} jiao={:.3}",
            r_jie.efficiency.final_efficiency.as_f64(),
            r_jiao.efficiency.final_efficiency.as_f64()
        );

        // 精英化孑：市井压上限 + 按现单数加%。纸面 trade% 虚高，score 未扣上限对产能影响（已知缺口）。
        let jie_e2 = TradeOperator::new(
            "孑",
            2,
            instances.resolve_trade_buff_ids("孑", crate::tier::PromotionTier::TierUp),
        );
        let silver = ops_from_pool(&pool, &["银灰"])[0].clone();
        let courier = ops_from_pool(&pool, &["讯使"])[0].clone();
        let mut input_e2 = base.clone();
        input_e2.operators = vec![silver, jie_e2, courier];
        let r_e2 = solve_trade_with_shift(&input_e2, &table, 24.0).unwrap();
        eprintln!(
            "孑e2(市井,纸面虚高): trade={:.1} limit={} — 游戏里精英化常倒扣，勿与 e0 同论",
            ((r_e2.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0),
            r_e2.final_order_limit
        );

        // 若当前订单数 = 上限（gap=0），摊贩经济失效，孑组合会输给角峰
        let mut full_jiao = with_jiao.clone();
        full_jiao.order_count = Some(20);
        let mut full_jie = with_jie.clone();
        full_jie.order_count = Some(18);
        let r_full_jiao = solve_trade_with_shift(&full_jiao, &table, 24.0).unwrap();
        let r_full_jie = solve_trade_with_shift(&full_jie, &table, 24.0).unwrap();
        eprintln!(
            "满单 gap=0: 角峰={:.1}% 孑={:.1}%",
            ((r_full_jiao.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0),
            ((r_full_jie.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0)
        );
        assert!(
            ((r_full_jiao.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0)
                > ((r_full_jie.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0),
            "满单时角峰应更高（摊贩无 gap）"
        );
    }

    #[test]
    fn jie_equiv_with_peers_35_limit_10() {
        use crate::trade::input::{TradeOrderKind, TradeRoomInput};
        use crate::trade::interpreter::{apply_trade_phases, TradeContext};

        let table = table();
        let peers = vec![
            op("a", 0, vec!["trade_ord_spd[000]"]), // +20
            op("b", 0, vec!["trade_ord_spd[010]"]), // +15
        ];
        for &oc in &[7i32, 8, 10] {
            let mut ops = peers.clone();
            ops.push(op("孑", 0, vec!["trade_ord_limit_diff[000]"]));
            let input = TradeRoomInput {
                level: 3,
                operators: ops,
                order_count: Some(oc),
                mood: 24.0,
                gold_production_lines: None,
                durin_virtual_lines: None,
                human_fireworks: None,
                layout: Default::default(),
                active_order_kind: TradeOrderKind::Gold,
            };
            let mut ctx = TradeContext::from_room(&input);
            apply_trade_phases(&mut ctx, &table);
            let jie = ctx.operators.iter().find(|o| o.name == "孑").unwrap();
            let r8 = solve_trade_with_shift(&input, &table, 8.0).unwrap();
            let r24 = solve_trade_with_shift(&input, &table, 24.0).unwrap();
            eprintln!(
                "e0 oc={oc} limit={} gap={} jie_var={:.0} total={:.0} score8={:.3} score24={:.3}",
                ctx.final_order_limit,
                ctx.order_gap(),
                jie.variable_eff,
                ((r8.efficiency.paper.paper_efficiency.as_f64() - 1.0) * 100.0),
                r8.efficiency.final_efficiency.as_f64(),
                r24.efficiency.final_efficiency.as_f64()
            );
        }
    }
}
