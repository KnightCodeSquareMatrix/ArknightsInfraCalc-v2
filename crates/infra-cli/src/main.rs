use std::env;
use std::process::ExitCode;

use std::path::PathBuf;

use csv::ReaderBuilder;
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::pool::{build_trade_pool, PoolSkip};
use infra_core::roster::Roster;
use infra_core::search::{search_trade_triples, TradeSearchOptions};
use infra_core::skill_table::{data_path, default_skill_table_path, SkillTable};
use infra_core::trade::{solve_trade, TradeOperator, TradeRoomInput};
use infra_core::Error;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "verify" => verify_cmd(&args[2..])?,
        "pool" => pool_cmd(&args[2..])?,
        "search" => search_cmd(&args[2..])?,
        _ => print_usage(),
    }
    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  infra-cli verify --case <case_id>");
    eprintln!("  infra-cli verify --all");
    eprintln!("  infra-cli pool --trade [--roster <path>]");
    eprintln!("  infra-cli search trade [--roster <path>] [--top <n>]");
}

fn roster_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    if let Some(path) = args
        .windows(2)
        .find(|w| w[0] == "--roster")
        .map(|w| w[1].as_str())
    {
        return Ok(PathBuf::from(path));
    }
    Ok(data_path("roster.csv")?)
}

fn load_trade_context(roster_path: &std::path::Path) -> Result<(Roster, OperatorInstances, SkillTable), Error> {
    let roster = Roster::load_csv_for_facility(roster_path, "trade")?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    Ok((roster, instances, table))
}

fn pool_cmd(args: &[String]) -> Result<(), Error> {
    if !args.iter().any(|a| a == "--trade") {
        eprintln!("specify --trade");
        return Ok(());
    }
    let roster_path = roster_path_from_args(args)?;
    let (roster, instances, table) = load_trade_context(&roster_path)?;
    let pool = build_trade_pool(&roster, &instances, &table)?;
    let stats = pool.stats();

    println!(
        "trade pool: ready={} skipped={} C(ready,3)={}",
        stats.ready, stats.skipped, stats.combinations_3
    );
    for entry in &pool.entries {
        println!(
            "  ready  {} e{} hint={:.0} mechanic={} buffs={}",
            entry.name,
            entry.elite,
            entry.flat_eff_hint,
            entry.is_mechanic,
            entry.buff_ids.len()
        );
    }
    for (name, elite, reason) in &pool.skipped {
        let detail = match reason {
            PoolSkip::NoTradeBinding => "no trade binding".to_string(),
            PoolSkip::UnmodeledBuff(id) => format!("unmodeled {id}"),
        };
        println!("  skip   {name} e{elite}: {detail}");
    }
    Ok(())
}

fn search_cmd(args: &[String]) -> Result<(), Error> {
    if args.first().map(String::as_str) != Some("trade") {
        eprintln!("usage: infra-cli search trade [--roster <path>] [--top <n>]");
        return Ok(());
    }
    let roster_path = roster_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(5);
    let (roster, instances, table) = load_trade_context(&roster_path)?;
    let pool = build_trade_pool(&roster, &instances, &table)?;
    let report = search_trade_triples(
        &pool,
        &table,
        &TradeSearchOptions {
            top_k,
            ..TradeSearchOptions::default()
        },
    )?;

    let rate = if report.elapsed.as_secs_f64() > 0.0 {
        report.evaluated as f64 / report.elapsed.as_secs_f64()
    } else {
        0.0
    };
    println!(
        "combinations={} evaluated={} elapsed={:.2?} ({:.0} eval/s)",
        report.combinations, report.evaluated, report.elapsed, rate
    );
    for (i, hit) in report.top.iter().enumerate() {
        println!(
            "  #{:<2} score={:.3} trade={:.1} gold={:.1} shortcut={:?} ops={:?}",
            i + 1,
            hit.score,
            hit.trade_pct,
            hit.gold_pct,
            hit.shortcut,
            hit.names
        );
    }
    Ok(())
}

fn verify_cmd(args: &[String]) -> Result<(), Error> {
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
            && case.operators != "see_roster"
            && !case.expect_shortcut.starts_with("gsl_witch_")
        {
            println!("skip {} (fixture not wired)", case.case_id);
            continue;
        }

        let input = if case.expect_shortcut.starts_with("gsl_witch_") {
            witch_fixture(&case.expect_shortcut, case.trade_level)
        } else {
            closure_fixture(&case.case_id, case.trade_level)
        };
        let result = solve_trade(&input, &table)?;
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

    if any_fail {
        return Err(Error::msg("regression failures"));
    }
    Ok(())
}

#[derive(Debug)]
struct RegressionCase {
    case_id: String,
    expect_trade_pct: f64,
    expect_gold_pct: f64,
    expect_shortcut: String,
    tolerance: f64,
    trade_level: u8,
    operators: String,
}

fn load_regression_cases(path: &std::path::Path) -> Result<Vec<RegressionCase>, Error> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        out.push(RegressionCase {
            case_id: rec[0].to_string(),
            expect_shortcut: rec[1].to_string(),
            operators: rec[2].to_string(),
            trade_level: rec[3].parse().unwrap_or(3),
            expect_trade_pct: rec[4].parse().unwrap_or(0.0),
            expect_gold_pct: rec[5].parse().unwrap_or(0.0),
            tolerance: rec[7].parse().unwrap_or(0.5),
        });
    }
    Ok(out)
}

fn witch_fixture(shortcut_id: &str, level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let shamare = op(
        "巫恋",
        2,
        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
    );
    let long_e2 = op("龙舌兰", 2, vec!["trade_ord_long[010]"]);
    let long_e0 = op("龙舌兰", 0, vec!["trade_ord_long[000]"]);
    let kafka_beta = op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]);
    let docus = op(
        "但书",
        2,
        vec!["trade_ord_law[000]", "trade_ord_against[010]"],
    );
    let zheguang_alpha = op("折光", 0, vec!["trade_ord_wt&cost[002]"]);
    let blank = op("古米", 0, vec!["trade_ord_spd&cost[000]"]);

    let operators = match shortcut_id {
        "gsl_witch_long_beta" => vec![shamare, long_e2, kafka_beta],
        "gsl_witch_long_docus" => vec![shamare, long_e2, docus],
        "gsl_witch_long_alpha" => vec![shamare, long_e2, zheguang_alpha],
        "gsl_witch_long_blank" => vec![shamare, long_e2, blank],
        "gsl_witch_long0_blank" => vec![shamare, long_e0, blank],
        "gsl_witch_beta_blank" => vec![shamare, kafka_beta, blank],
        _ => vec![shamare, long_e2, blank],
    };

    TradeRoomInput::with_operators(level, operators)
}

fn closure_fixture(case_id: &str, level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let closure = op("可露希尔", 2, vec!["trade_ord_closure[000]"]);
    let exusiai = op(
        "能天使",
        2,
        vec!["trade_ord_spd[010]", "trade_ord_spd[020]"],
    );
    let texas_e2 = op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]);
    let texas_e0 = op("德克萨斯", 0, vec!["trade_ord_spd&cost_P[000]"]);
    let lappland = op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]);

    let operators = match case_id {
        "reg_gsl_closure_tier90" => vec![closure, exusiai, texas_e2, lappland],
        "reg_gsl_closure_tier80" => vec![closure, texas_e2, lappland],
        "reg_gsl_closure_tier60" => vec![closure, exusiai, texas_e0],
        _ => vec![closure, exusiai, texas_e2, lappland],
    };

    TradeRoomInput::with_operators(level, operators)
}

