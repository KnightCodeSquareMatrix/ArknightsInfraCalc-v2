use crate::verify::{
    blackkey_closure_fixture, closure_fixture, docus_fixture, ling_jie_fixture,
    load_regression_cases, load_unit_anchor_cases, unit_fixture, witch_fixture,
};
use infra_core::skill_table::{data_path, default_skill_table_path, SkillTable};
use infra_core::trade::solve_trade_with_shift;
use infra_core::Error;

pub fn verify_cmd(args: &[String]) -> Result<(), Error> {
    let run_all = args.iter().any(|a| a == "--all");
    let case_id = args
        .windows(2)
        .find(|w| w[0] == "--case")
        .map(|w| w[1].as_str());
    if !run_all && case_id.is_none() {
        eprintln!("specify --case <id> or --all");
        return Ok(());
    }
    run_regressions(run_all, case_id)
}

pub(crate) fn run_all_regressions() -> Result<(), Error> {
    run_regressions(true, None)
}

fn run_regressions(run_all: bool, case_id: Option<&str>) -> Result<(), Error> {
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let cases = load_regression_cases(&data_path("REGRESSION_CASES.csv")?)?;

    let mut any_fail = false;
    for case in &cases {
        if !run_all {
            if let Some(id) = case_id {
                if case.case_id != id {
                    continue;
                }
            }
        }

        if !case.operators.starts_with("可露希尔")
            && !case.operators.starts_with("但书")
            && !case.operators.starts_with("黑键")
            && case.operators != "see_roster"
            && !case.expect_rule_id.starts_with("gsl_witch_")
            && !case.case_id.contains("ling_jie")
        {
            println!("skip {} (fixture not wired)", case.case_id);
            continue;
        }

        let input = if case.expect_rule_id == "gsl_blackkey_closure" {
            blackkey_closure_fixture(case.trade_level)
        } else if case.case_id.contains("ling_jie") {
            ling_jie_fixture(case.trade_level)
        } else if case.expect_rule_id.starts_with("gsl_witch_") {
            witch_fixture(&case.expect_rule_id, case.trade_level)
        } else if case.expect_rule_id.starts_with("gsl_docus_") {
            docus_fixture(&case.case_id, case.trade_level)
        } else {
            closure_fixture(&case.case_id, case.trade_level)
        };
        let result = solve_trade_with_shift(&input, &table, 24.0)?;
        let efficiency_ok =
            (result.efficiency.final_efficiency.as_f64() - case.expect_final_efficiency).abs()
                <= case.tolerance;
        let mechanic_ok = (result
            .order_mechanic
            .mechanic_equivalent_efficiency
            .as_f64()
            - case.expect_mechanic_equivalent_efficiency)
            .abs()
            <= case.tolerance;
        let rule_ok = if case.expect_rule_id == "none" {
            result.rule_id.is_none()
        } else {
            result.rule_id.as_deref() == Some(case.expect_rule_id.as_str())
        };

        if efficiency_ok && mechanic_ok && rule_ok {
            println!(
                "PASS {} total_efficiency={} mechanic_efficiency={} rule_id={:?}",
                case.case_id,
                result.efficiency.final_efficiency,
                result.order_mechanic.mechanic_equivalent_efficiency,
                result.rule_id,
            );
        } else {
            any_fail = true;
            eprintln!(
                "FAIL {} expected total_efficiency={} mechanic_efficiency={} rule_id={} got total_efficiency={} mechanic_efficiency={} rule_id={:?}",
                case.case_id,
                case.expect_final_efficiency,
                case.expect_mechanic_equivalent_efficiency,
                case.expect_rule_id,
                result.efficiency.final_efficiency,
                result.order_mechanic.mechanic_equivalent_efficiency,
                result.rule_id,
            );
        }
    }

    if verify_unit_anchors(&table, run_all, case_id)? {
        any_fail = true;
    }

    if any_fail {
        return Err(Error::msg("regression failures"));
    }
    Ok(())
}

fn verify_unit_anchors(
    table: &SkillTable,
    run_all: bool,
    case_id: Option<&str>,
) -> Result<bool, Error> {
    let cases = load_unit_anchor_cases(&data_path("UNIT_OUTPUT_ANCHORS.csv")?)?;
    let mut any_fail = false;
    for case in &cases {
        if !run_all {
            if let Some(id) = case_id {
                if case.case_id != id {
                    continue;
                }
            } else {
                return Ok(false);
            }
        }
        let input = unit_fixture(&case.fixture, case.trade_level);
        let result = solve_trade_with_shift(&input, table, 24.0)?;
        let u = &result.production.unit;
        let trade_tol = case.expect_unit_trade * case.tolerance_pct / 100.0;
        let gold_tol = if case.expect_gsl_unit_gold > 0.0 {
            case.expect_gsl_unit_gold * case.tolerance_pct / 100.0
        } else {
            0.0
        };
        let trade_ok = (u.unit_trade_per_day - case.expect_unit_trade).abs() <= trade_tol;
        let gold_ok = case.expect_gsl_unit_gold <= 0.0
            || (u.gsl_unit_gold() - case.expect_gsl_unit_gold).abs() <= gold_tol;
        let efficiency = &result.efficiency;
        let unit_multiplier = infra_core::Efficiency::from_decimal(
            case.expect_unit_trade / efficiency.production_basis.reference_unit_output_per_day,
        );
        let expected_final = if efficiency.applies_paper_efficiency {
            efficiency.paper.paper_efficiency * unit_multiplier
        } else {
            unit_multiplier
        };
        let final_tol = expected_final.as_f64().abs() * case.tolerance_pct / 100.0;
        let final_ok =
            (efficiency.final_efficiency.as_f64() - expected_final.as_f64()).abs() <= final_tol;
        let expected_daily =
            efficiency.production_basis.reference_unit_output_per_day * expected_final.as_f64();
        let daily_tol = expected_daily.abs() * case.tolerance_pct / 100.0;
        let daily_ok =
            (result.production.daily_at_shift.trade_lmd - expected_daily).abs() <= daily_tol;
        if trade_ok && gold_ok && final_ok && daily_ok {
            println!(
                "PASS {} unit_trade={:.2} gsl_gold={:.2} paper={:.4} final={:.6} daily={:.2}",
                case.case_id,
                u.unit_trade_per_day,
                u.gsl_unit_gold(),
                efficiency.paper.paper_efficiency,
                efficiency.final_efficiency,
                result.production.daily_at_shift.trade_lmd,
            );
        } else {
            any_fail = true;
            eprintln!(
                "FAIL {} expected unit_trade={} gsl_gold={} final={:.6} daily={:.2} got unit_trade={:.2} gsl_gold={:.2} final={:.6} daily={:.2}",
                case.case_id,
                case.expect_unit_trade,
                case.expect_gsl_unit_gold,
                expected_final,
                expected_daily,
                u.unit_trade_per_day,
                u.gsl_unit_gold(),
                efficiency.final_efficiency,
                result.production.daily_at_shift.trade_lmd,
            );
        }
    }
    Ok(any_fail)
}
