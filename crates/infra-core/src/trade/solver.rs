use serde::Serialize;

use crate::error::Result;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;
use crate::trade::efficiency::{
    PaperTradeEfficiency, TradeEfficiency, GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY,
};
use crate::trade::input::{TradeOrderKind, TradeRoomInput, TradeRoomView};
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

fn mood_drain_from_ctx(ctx: &TradeContext<'_>) -> Vec<OperatorMoodDrain> {
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
    ctx: &TradeContext<'_>,
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
            .is_some_and(|operator| {
                PromotionTier::from_elite(operator.elite) == PromotionTier::TierUp
            });
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
    solve_trade_with_shift_prevalidated_view(&input.as_view(), table, shift_hours)
}

pub(crate) fn solve_trade_with_shift_prevalidated_view(
    input: &TradeRoomView<'_>,
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
    solve_trade_with_shift_inner(&input.as_view(), table, shift_hours, true)
}

fn solve_trade_with_shift_inner(
    input: &TradeRoomView<'_>,
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
    if check_exclusivity
        && shortcut::trade_station_exclusive_violation_refs(&input.operators, table)
    {
        return Err(crate::error::Error::msg(
            "trade station violates mechanism exclusivity (docus / closure / witch)",
        ));
    }

    let mut ctx = TradeContext::from_view(input);
    apply_trade_phases(&mut ctx, table);

    let order_eff_base = ctx.order_eff_base();
    let siracusa_skill_eff = input
        .layout
        .global_inject
        .trade_eff_pct_for_family_with_current_room_count(
            "trade_siracusa_scaling",
            &input.layout.trade_tagged_count_sum,
            |tag| ctx.current_room_tag_count(tag),
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
            shortcut::resolve_trade_shortcut_refs(
                &input.operators,
                table,
                order_eff_pre,
                input.level,
                &input.layout.global_inject,
            )
        } else {
            shortcut::resolve_trade_shortcut_prevalidated_refs(
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
