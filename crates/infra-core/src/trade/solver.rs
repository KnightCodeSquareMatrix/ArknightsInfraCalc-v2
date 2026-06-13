use serde::Serialize;

use crate::error::Result;
use crate::skill_table::SkillTable;
use crate::trade::input::TradeRoomInput;
use crate::trade::interpreter::{apply_trade_phases, TradeContext};
use crate::trade::order_mechanic::{self, OrderMechanicResult};
use crate::trade::shortcut;
use crate::trade::unit_output::{baseline_unit_trade_lv3_regular, compute_unit_output, daily_yield, TradeDailyYield};

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
    ctx: &TradeContext,
    mechanic: &OrderMechanicResult,
    order_eff_total: f64,
    shift_hours: f64,
) -> TradeProductionReport {
    let caps = ctx.mechanic_caps();
    let unit = compute_unit_output(ctx, &mechanic.gold_distribution, &caps, mechanic);
    TradeProductionReport {
        daily_at_shift: daily_yield(
            &unit,
            order_eff_total,
            shift_hours,
            mechanic.ignores_order_eff(),
        ),
        unit,
    }
}

fn build_production_shortcut(
    ctx: &TradeContext,
    mechanic: &OrderMechanicResult,
    sc: &shortcut::TradeShortcutMatch,
    order_eff_total: f64,
    shift_hours: f64,
) -> TradeProductionReport {
    let base = baseline_unit_trade_lv3_regular();
    let unit = sc
        .unit_output_from_anchor(base)
        .unwrap_or_else(|| {
            let caps = ctx.mechanic_caps();
            compute_unit_output(ctx, &mechanic.gold_distribution, &caps, mechanic)
        });
    TradeProductionReport {
        daily_at_shift: daily_yield(
            &unit,
            order_eff_total,
            shift_hours,
            mechanic.ignores_order_eff(),
        ),
        unit,
    }
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
    if check_exclusivity && shortcut::trade_station_exclusive_violation(&input.operators, table) {
        return Err(crate::error::Error::msg(
            "trade station violates mechanism exclusivity (docus / closure / witch)",
        ));
    }

    let mut ctx = TradeContext::from_room(input);
    apply_trade_phases(&mut ctx, table);

    let order_eff_base = ctx.order_eff_base();
    let order_eff_skill = ctx.order_eff_skill();
    let order_eff_pre = ctx.order_eff_total();

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
            let order_eff_total = sc.entry.trade_pct;
            let order_eff_skill_adj = order_eff_total - order_eff_base;
            let production =
                build_production_shortcut(&ctx, &mechanic, &sc, order_eff_total, shift_hours);
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
                production,
            });
        }
    }

    let mechanic = order_mechanic::resolve_order_mechanic(&ctx, order_eff_pre);
    let effective_eff_multiplier = mechanic.effective_eff_multiplier(order_eff_pre);
    let production = build_production(&ctx, &mechanic, order_eff_pre, shift_hours);

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
        production,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::skill_table::SkillTable;
    use crate::layout::LayoutContext;
    use crate::trade::input::{TradeOperator, TradeRoomInput};
    use crate::tier::PromotionTier;

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
        assert_eq!(result.trade_shortcut.as_deref(), Some("gsl_closure_tier90"));
        assert!((result.order_eff_pre_shortcut - 114.0).abs() < 1.0);
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
        assert_eq!(result.trade_shortcut.as_deref(), Some("gsl_closure_tier60"));
        assert!((result.order_eff_total - 100.0).abs() < 2.0);
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
            (result.order_eff_pre_shortcut - 75.0).abs() < 4.0,
            "精2 能天使仅 +35%，加两名 20% 级工具人纸面≈75%，got {}",
            result.order_eff_pre_shortcut
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
            instances.resolve_trade_buff_ids("海蒂", PromotionTier::TierUp),
            vec!["trade_ord_spd[021]".to_string()]
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
            result.trade_shortcut.as_deref(),
            Some("gsl_closure_tier90"),
            "海蒂(35%)+可颂(30%) 不得命中 90 档，got {:?} pre={}",
            result.trade_shortcut,
            result.order_eff_pre_shortcut
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
                op(
                    "蕾缪安",
                    2,
                    lemuen.iter().map(String::as_str).collect(),
                ),
                op("海蒂", 2, heidi.iter().map(String::as_str).collect()),
            ],
        );
        let result = solve_trade(&input, &table()).unwrap();
        assert_ne!(
            result.trade_shortcut.as_deref(),
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

        for (label, result) in [("e0", &r_e0), ("e2", &r_e2)] {
            assert!(
                result
                    .trade_shortcut
                    .as_deref()
                    .is_some_and(|s| s == "gsl_docus_solo"),
                "docus {label} tools shortcut {:?}",
                result.trade_shortcut
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
        assert!(result.order_mechanic.mechanic_equiv_eff_pct > 0.0);
    }

    #[test]
    fn control_amiya_injects_plus_seven_trade_eff() {
        let table = table();
        let instances =
            OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let mut layout = LayoutContext::default();
        let amiya_buffs = instances.resolve_control_buff_ids("阿米娅", PromotionTier::Tier0);
        crate::control::apply_control_to_layout(
            &mut layout,
            &[crate::control::ControlOperator::new("阿米娅", 0, amiya_buffs)],
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
            solve_trade(&plain, &table).unwrap().order_eff_total
        };
        let with = solve_trade(&input, &table).unwrap().order_eff_total;
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
        let roster = Roster::from_elite_map([
            ("银灰".into(), 2),
            ("角峰".into(), 0),
            ("讯使".into(), 1),
            ("孑".into(), 0),
        ]
        .into_iter()
        .collect());
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
                r.effective_eff_multiplier,
                r.order_eff_total,
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
            r_jie.effective_eff_multiplier > r_jiao.effective_eff_multiplier,
            "精0孑+摊贩应高于角峰: jie={:.3} jiao={:.3}",
            r_jie.effective_eff_multiplier,
            r_jiao.effective_eff_multiplier
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
            r_e2.order_eff_total,
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
            r_full_jiao.order_eff_total, r_full_jie.order_eff_total
        );
        assert!(
            r_full_jiao.order_eff_total > r_full_jie.order_eff_total,
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
                r8.order_eff_total,
                r8.effective_eff_multiplier,
                r24.effective_eff_multiplier
            );
        }
    }
}
