mod commands;
mod output;
mod verify;

use std::env;
use std::process::ExitCode;
use std::sync::Arc;

use std::path::PathBuf;

use commands::{layout_cmd, profile_cmd, verify_cmd};
use output::{
    emit_bench, emit_pool, emit_schedule, emit_trade_search, emit_trade_yield, BenchMeta, OutputOptions,
    PoolSummary, SearchMeta, TradeYieldRow,
};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::operbox::OperBox;
use infra_core::manufacture::ManuSearchRecipeMode;
use infra_core::pool::{build_manufacture_pool, build_trade_pool};
use infra_core::roster::Roster;
use infra_core::schedule::schedule_trade_rotation_a_b_a;
use infra_core::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, TradeSearchOptions,
};
use infra_core::skill_table::{data_path, default_skill_table_path, SkillTable};
use infra_core::layout::LayoutContext;
use infra_core::trade::input::TradeSearchOrderMode;
use infra_core::trade::solve_trade_with_shift;
use infra_core::types::RecipeKind;
use verify::unit_fixture;
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
        "schedule" => schedule_cmd(&args[2..])?,
        "trade" => trade_cmd(&args[2..])?,
        "bench" => bench_cmd(&args[2..])?,
        "layout" => layout_cmd(&args[2..])?,
        "profile" => profile_cmd(&args[2..])?,
        _ => print_usage(),
    }
    Ok(())
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  infra-cli verify --case <case_id>");
    eprintln!("  infra-cli verify --all");
    eprintln!("  infra-cli pool --trade [--manufacture] [--roster <path>] [--operbox <path>] [-o <file.csv>] [--text]");
    eprintln!("  infra-cli search trade [--roster <path>] [--operbox <path>] [--top <n>] [-o <file.csv>] [--text|--json]");
    eprintln!("  infra-cli bench --operbox <path> [--recipe gold|battle_record|originium] [--top <n>] [-o <file.csv>] [--text]");
    eprintln!("      (default manufacture: 4 lines = 2 gold + 2 battle_record; --recipe = single-line debug)");
    eprintln!("  infra-cli schedule rotation --operbox <path> [--layout-baseline] [-o <file.csv>] [--text|--json]");
    eprintln!("  infra-cli layout test --layout <path> --operbox <path> [--assignment <path>] [--top <n>] [-o <file.csv>] [--text]");
    eprintln!("  infra-cli layout eval --layout <path> --operbox <path> --assignment <path> [--text]");
    eprintln!("  infra-cli layout rotation --layout <path> --operbox <path> [--top <n>] [--output-dir <dir>] [-o <file.csv>] [--text|--json]");
    eprintln!("  infra-cli profile layout-full [--layout <path>] [--operbox <path>] [--top <n>] [--runs <n>] [--label <name>]");
    eprintln!("  infra-cli trade yield <fixture> [--level <n>] [--shift <hours>] [-o <file.csv>] [--text]");
    eprintln!();
    eprintln!("Output: CSV by default (UTF-8 BOM when writing to file). Use --text for human-readable stderr.");
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

fn operbox_path_optional(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
}

fn load_trade_roster(args: &[String], instances: &OperatorInstances) -> Result<Roster, Error> {
    if let Some(path) = operbox_path_optional(args) {
        let operbox = OperBox::load(&path)?;
        return Ok(operbox.trade_roster(instances));
    }
    let roster_path = roster_path_from_args(args)?;
    Roster::load_csv_for_facility(&roster_path, "trade")
}

fn load_trade_context(args: &[String]) -> Result<(Roster, OperatorInstances, SkillTable), Error> {
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let roster = load_trade_roster(args, &instances)?;
    Ok((roster, instances, table))
}

fn pool_cmd(args: &[String]) -> Result<(), Error> {
    let trade = args.iter().any(|a| a == "--trade");
    let manufacture = args.iter().any(|a| a == "--manufacture");
    if !trade && !manufacture {
        eprintln!("specify --trade and/or --manufacture");
        return Ok(());
    }
    let out = OutputOptions::from_args(args);
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    if let Some(ob_path) = operbox_path_optional(args) {
        let operbox = OperBox::load(&ob_path)?;
        let ob_str = ob_path.to_string_lossy();
        if trade {
            let roster = operbox.trade_roster(&instances);
            emit_trade_pool(&out, &roster, &instances, &table, Some(ob_str.as_ref()), operbox.owned_count())?;
        }
        if manufacture {
            let roster = operbox.manufacture_roster(&instances);
            emit_manufacture_pool(&out, &roster, &instances, &table, Some(ob_str.as_ref()), operbox.owned_count())?;
        }
        return Ok(());
    }

    if trade {
        let roster_path = roster_path_from_args(args)?;
        let roster = Roster::load_csv_for_facility(&roster_path, "trade")?;
        emit_trade_pool(&out, &roster, &instances, &table, None, 0)?;
    }
    if manufacture {
        eprintln!("manufacture pool requires --operbox (no default roster)");
    }
    Ok(())
}

fn emit_trade_pool(
    out: &OutputOptions,
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    operbox: Option<&str>,
    owned: usize,
) -> Result<(), Error> {
    let pool = build_trade_pool(roster, instances, table)?;
    let stats = pool.stats();
    emit_pool(
        out,
        &PoolSummary {
            facility: "trade",
            operbox,
            owned: operbox.map(|_| owned),
            roster_size: roster.len(),
            ready: stats.ready,
            skipped: stats.skipped,
            combinations_3: stats.combinations_3,
        },
        &pool.skipped,
    )
}

fn emit_manufacture_pool(
    out: &OutputOptions,
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    operbox: Option<&str>,
    owned: usize,
) -> Result<(), Error> {
    let pool = build_manufacture_pool(roster, instances, table)?;
    let stats = pool.stats();
    emit_pool(
        out,
        &PoolSummary {
            facility: "manufacture",
            operbox,
            owned: operbox.map(|_| owned),
            roster_size: roster.len(),
            ready: stats.ready,
            skipped: stats.skipped,
            combinations_3: stats.combinations_3,
        },
        &pool.skipped,
    )
}

fn bench_cmd(args: &[String]) -> Result<(), Error> {
    let out = OutputOptions::from_args(args);
    let operbox_path = operbox_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(3);
    let recipe_mode = args
        .windows(2)
        .find(|w| w[0] == "--recipe")
        .map(|w| w[1].as_str())
        .map(|s| parse_recipe(s).map(ManuSearchRecipeMode::Single))
        .transpose()?
        .unwrap_or_else(ManuSearchRecipeMode::default);

    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let layout = Arc::new({
        let mut l = LayoutContext::search_baseline();
        l.apply_durin_dorm_planning(operbox.durin_dorm_planning_count(&instances));
        l
    });
    let ob_str = operbox_path.to_string_lossy();

    let manu_scenario_label = match recipe_mode {
        ManuSearchRecipeMode::Single(r) => format!("single:{r:?}"),
        ManuSearchRecipeMode::Lines(s) => format!(
            "{} lines ({} gold + {} battle_record + {} originium)",
            s.total_lines(),
            s.gold_lines,
            s.battle_record_lines,
            s.originium_lines
        ),
    };

    let trade_roster = operbox.trade_roster(&instances);
    let trade_pool = build_trade_pool(&trade_roster, &instances, &table)?;
    let trade_stats = trade_pool.stats();
    let trade_report = search_trade_triples(
        &trade_pool,
        &table,
        &TradeSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            ..TradeSearchOptions::default()
        },
    )?;
    let trade_mode_label = match trade_report.order_mode {
        TradeSearchOrderMode::Single(kind) => format!("single:{kind:?}"),
        TradeSearchOrderMode::Stations(s) => format!(
            "{} stations ({} gold + {} originium)",
            s.total_stations(),
            s.gold_order_stations,
            s.originium_order_stations
        ),
    };

    let manu_roster = operbox.manufacture_roster(&instances);
    let manu_pool = build_manufacture_pool(&manu_roster, &instances, &table)?;
    let manu_stats = manu_pool.stats();
    let manu_report = search_manufacture_triples(
        &manu_pool,
        &table,
        &ManuSearchOptions {
            top_k,
            recipe_mode,
            layout: Arc::clone(&layout),
            ..ManuSearchOptions::default()
        },
    )?;

    emit_bench(
        &out,
        &BenchMeta {
            operbox: ob_str.as_ref(),
            owned: operbox.owned_count(),
            layout: None,
            manufacture_scenario: &manu_scenario_label,
            trade_order_mode: &trade_mode_label,
        },
        PoolSummary {
            facility: "trade",
            operbox: None,
            owned: None,
            roster_size: trade_roster.len(),
            ready: trade_stats.ready,
            skipped: trade_stats.skipped,
            combinations_3: trade_stats.combinations_3,
        },
        &trade_report,
        PoolSummary {
            facility: "manufacture",
            operbox: None,
            owned: None,
            roster_size: manu_roster.len(),
            ready: manu_stats.ready,
            skipped: manu_stats.skipped,
            combinations_3: manu_stats.combinations_3,
        },
        &manu_report,
    )
}

fn parse_recipe(s: &str) -> Result<RecipeKind, Error> {
    match s {
        "gold" => Ok(RecipeKind::Gold),
        "battle_record" => Ok(RecipeKind::BattleRecord),
        "originium" => Ok(RecipeKind::Originium),
        other => Err(Error::msg(format!(
            "unknown recipe {other:?}; use gold|battle_record|originium"
        ))),
    }
}

fn operbox_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    args.windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .ok_or_else(|| Error::msg("missing --operbox <path>"))
}

fn schedule_cmd(args: &[String]) -> Result<(), Error> {
    if args.first().map(String::as_str) != Some("rotation") {
        eprintln!("usage: infra-cli schedule rotation --operbox <path> [--layout-baseline] [-o <file.csv>] [--text|--json]");
        return Ok(());
    }
    let out = OutputOptions::from_args(args);
    let operbox_path = operbox_path_from_args(args)?;
    let layout_baseline = args.iter().any(|a| a == "--layout-baseline");

    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let mut options = TradeSearchOptions::default();
    if layout_baseline {
        options.layout = Arc::new(LayoutContext::search_baseline());
    }
    Arc::make_mut(&mut options.layout)
        .apply_durin_dorm_planning(operbox.durin_dorm_planning_count(&instances));

    let report = schedule_trade_rotation_a_b_a(&operbox, &instances, &table, &options)?;
    emit_schedule(&out, operbox.owned_count(), &report)
}

fn search_cmd(args: &[String]) -> Result<(), Error> {
    if args.first().map(String::as_str) != Some("trade") {
        eprintln!("usage: infra-cli search trade [--roster <path>] [--operbox <path>] [--top <n>] [-o <file.csv>] [--text|--json]");
        return Ok(());
    }
    let out = OutputOptions::from_args(args);
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(5);
    let (roster, instances, table) = load_trade_context(args)?;
    let operbox_meta = operbox_path_optional(args)
        .map(|ob_path| OperBox::load(&ob_path).map(|ob| (ob.owned_count(), roster.len())))
        .transpose()?;
    let pool = build_trade_pool(&roster, &instances, &table)?;
    let report = search_trade_triples(
        &pool,
        &table,
        &TradeSearchOptions {
            top_k,
            ..TradeSearchOptions::default()
        },
    )?;
    let order_mode_label = match report.order_mode {
        TradeSearchOrderMode::Single(kind) => format!("single:{kind:?}"),
        TradeSearchOrderMode::Stations(s) => format!(
            "{} stations ({} gold + {} originium)",
            s.total_stations(),
            s.gold_order_stations,
            s.originium_order_stations
        ),
    };
    let (owned, trade_roster) = operbox_meta
        .map(|(o, r)| (Some(o), Some(r)))
        .unwrap_or((None, None));
    emit_trade_search(
        &out,
        &SearchMeta {
            operbox_owned: owned,
            trade_roster,
            combinations: report.combinations,
            evaluated: report.evaluated,
            elapsed: report.elapsed,
            order_mode_label: &order_mode_label,
        },
        &report,
    )
}

fn trade_cmd(args: &[String]) -> Result<(), Error> {
    if args.first().map(String::as_str) != Some("yield") {
        eprintln!("usage: infra-cli trade yield <fixture> [--level <n>] [--shift <hours>] [-o <file.csv>] [--text]");
        return Ok(());
    }
    let out = OutputOptions::from_args(args);
    let fixture = args.get(1).map(String::as_str).unwrap_or("");
    let level = args
        .windows(2)
        .find(|w| w[0] == "--level")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(3);
    let shift = args
        .windows(2)
        .find(|w| w[0] == "--shift")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(24.0);
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let input = unit_fixture(fixture, level);
    let result = solve_trade_with_shift(&input, &table, shift)?;
    let u = &result.production.unit;
    let d = &result.production.daily_at_shift;
    emit_trade_yield(
        &out,
        &TradeYieldRow {
            fixture,
            level,
            shift_hours: shift,
            paper_eff_pct: result.order_eff_total,
            shortcut: result.trade_shortcut.as_deref(),
            unit_trade: u.unit_trade_per_day,
            unit_gsl_gold: u.gsl_unit_gold(),
            multiplier: u.multiplier_vs_lv3_regular,
            daily_trade: d.trade_lmd,
            daily_gold: d.gold_spent,
            drone_trade: d.drone_trade_lmd,
            pre_shortcut: result.order_eff_pre_shortcut,
        },
    )
}

