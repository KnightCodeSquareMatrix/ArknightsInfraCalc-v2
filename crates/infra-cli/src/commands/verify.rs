use crate::verify::{
    blackkey_closure_fixture, closure_fixture, docus_fixture, ling_jie_fixture,
    load_regression_cases, load_unit_anchor_cases, unit_fixture, witch_fixture,
};
use infra_core::skill_table::{data_path, default_skill_table_path, SkillTable};
use infra_core::trade::solve_trade_with_shift;
use infra_core::Error;

pub fn verify_cmd(args: &[String]) -> Result<(), Error> {
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let cases = load_regression_cases(&data_path("REGRESSION_CASES.csv")?)?;

    let run_all = args.iter().any(|a| a == "--all");
    let case_id = args
        .windows(2)
        .find(|w| w[0] == "--case")
        .map(|w| w[1].as_str());

    let mut any_fail = false;
    for case in &cases {
        if !run_all {
            if let Some(id) = case_id {
                if case.case_id != id {
                    continue;
                }
            } else {
                eprintln!("specify --case <id> or --all");
                return Ok(());
            }
        }

        if !case.operators.starts_with("可露希尔")
            && !case.operators.starts_with("但书")
            && !case.operators.starts_with("黑键")
            && case.operators != "see_roster"
            && !case.expect_shortcut.starts_with("gsl_witch_")
            && !case.expect_shortcut.starts_with("gsl_ling_jie_")
        {
            println!("skip {} (fixture not wired)", case.case_id);
            continue;
        }

        let input = if case.expect_shortcut == "gsl_blackkey_closure" {
            blackkey_closure_fixture(case.trade_level)
        } else if case.expect_shortcut.starts_with("gsl_ling_jie_") {
            ling_jie_fixture(case.trade_level)
        } else if case.expect_shortcut.starts_with("gsl_witch_") {
            witch_fixture(&case.expect_shortcut, case.trade_level)
        } else if case.expect_shortcut.starts_with("gsl_docus_") {
            docus_fixture(&case.case_id, case.trade_level)
        } else {
            closure_fixture(&case.case_id, case.trade_level)
        };
        let result = solve_trade_with_shift(&input, &table, 24.0)?;
        let trade_ok = (result.order_eff_total - case.expect_trade_pct).abs() <= case.tolerance;
        let gold_ok = (result.order_mechanic.mechanic_equiv_eff_pct - case.expect_gold_pct).abs()
            <= case.tolerance;
        let shortcut_ok = result.trade_shortcut.as_deref() == Some(case.expect_shortcut.as_str());

        if trade_ok && gold_ok && shortcut_ok {
            println!(
                "PASS {} trade={:.1} gold={:.1} shortcut={:?} pre={:.1}",
                case.case_id,
                result.order_eff_total,
                result.order_mechanic.mechanic_equiv_eff_pct,
                result.trade_shortcut,
                result.order_eff_pre_shortcut
            );
        } else {
            any_fail = true;
            eprintln!(
                "FAIL {} expected trade={} gold={} shortcut={} got trade={:.1} gold={:.1} shortcut={:?} pre={:.1}",
                case.case_id,
                case.expect_trade_pct,
                case.expect_gold_pct,
                case.expect_shortcut,
                result.order_eff_total,
                result.order_mechanic.mechanic_equiv_eff_pct,
                result.trade_shortcut,
                result.order_eff_pre_shortcut
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
        if trade_ok && gold_ok {
            println!(
                "PASS {} unit_trade={:.1} gsl_gold={:.1} mult={:.3}",
                case.case_id,
                u.unit_trade_per_day,
                u.gsl_unit_gold(),
                u.multiplier_vs_lv3_regular
            );
        } else {
            any_fail = true;
            eprintln!(
                "FAIL {} expected unit_trade={} gsl_gold={} got unit_trade={:.1} gsl_gold={:.1}",
                case.case_id,
                case.expect_unit_trade,
                case.expect_gsl_unit_gold,
                u.unit_trade_per_day,
                u.gsl_unit_gold()
            );
        }
    }
    Ok(any_fail)
}
