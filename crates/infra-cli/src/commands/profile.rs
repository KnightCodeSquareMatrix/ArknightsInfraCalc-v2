use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use infra_core::box_profile::{
    build_box_profile, default_schedule_export_path, run_layout_probe, BoxProfileOptions,
};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::{
    assign_base_greedy, resolve_base, AssignBaseOptions, BaseBlueprint,
};
use infra_core::manufacture::input::ManuSearchRecipeMode;
use infra_core::operbox::OperBox;
use infra_core::pool::{build_manufacture_pool, build_trade_pool};
use infra_core::profile::{hot_path_snapshot, reset_hot_path_counters, HotPathSnapshot};
use infra_core::schedule::schedule_team_rotation;
use infra_core::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, TradeSearchOptions,
};
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use infra_core::Error;

#[derive(Debug, Clone)]
struct PhaseTiming {
    name: &'static str,
    elapsed: Duration,
}

#[derive(Debug, Clone)]
struct WorkloadStats {
    trade_pool_ready: usize,
    trade_combinations: u64,
    trade_evaluated: u64,
    manu_pool_ready: usize,
    manu_combinations: u64,
    manu_evaluated: u64,
    rotation_shifts: usize,
    rotation_trade_peak: f64,
    rotation_trade_recovery: f64,
}

#[derive(Debug, Clone)]
struct RunProfile {
    label: String,
    total: Duration,
    phases: Vec<PhaseTiming>,
    counters: HotPathSnapshot,
    stats: WorkloadStats,
}

pub fn profile_cmd(args: &[String]) -> Result<(), Error> {
    match args.first().map(String::as_str) {
        Some("layout-full") => profile_layout_full_cmd(&args[1..]),
        Some("analyze-compare") => profile_analyze_compare_cmd(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  infra-cli profile layout-full [--layout <path>] [--operbox <path>] [--top <n>] [--runs <n>]\n  infra-cli profile analyze-compare [--layout <path>] [--operbox <path>] [--schedule <path>] [--runs <n>]"
            );
            Ok(())
        }
    }
}

fn profile_layout_full_cmd(args: &[String]) -> Result<(), Error> {
    let layout_path = args
        .windows(2)
        .find(|w| w[0] == "--layout")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from("data/layout/243_use_this_.json"));
    let operbox_path = args
        .windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from("data/schedule_243/operbox_ideal_e2.json"));
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(20);
    let runs = args
        .windows(2)
        .find(|w| w[0] == "--runs")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(3)
        .max(1);
    let label = args
        .windows(2)
        .find(|w| w[0] == "--label")
        .map(|w| w[1].as_str())
        .unwrap_or("run");

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    eprintln!("=== infra profile layout-full ===");
    eprintln!(
        "label={label} layout={} operbox={} owned={} top_k={} runs={}",
        layout_path.display(),
        operbox_path.display(),
        operbox.owned_count(),
        top_k,
        runs
    );

    let mut profiles = Vec::with_capacity(runs);
    let mut owned_labels = Vec::new();
    for i in 0..runs {
        let run_label = if runs == 1 {
            label.to_string()
        } else {
            let s = format!("{label}#{i}");
            owned_labels.push(s);
            owned_labels.last().unwrap().clone()
        };
        profiles.push(run_layout_full(
            &run_label,
            &blueprint,
            &operbox,
            &instances,
            &table,
            top_k,
        )?);
    }

    print_profile_report(&profiles);
    Ok(())
}

fn run_layout_full(
    label: &str,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    top_k: usize,
) -> Result<RunProfile, Error> {
    reset_hot_path_counters();
    let total_start = Instant::now();
    let mut phases = Vec::new();

    let t0 = Instant::now();
    let assignment = assign_base_greedy(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;
    phases.push(PhaseTiming {
        name: "assign_base_greedy",
        elapsed: t0.elapsed(),
    });

    let durin_plan = operbox.durin_dorm_planning_count(instances);
    let t1 = Instant::now();
    let resolved = resolve_base(
        blueprint,
        &assignment,
        Some(instances),
        Some(table),
        24.0,
        Some(durin_plan),
    )?;
    phases.push(PhaseTiming {
        name: "resolve_base",
        elapsed: t1.elapsed(),
    });
    let layout = Arc::new(resolved.layout_snapshot());

    let trade_scenario = blueprint.trade_station_scenario();
    let manu_scenario = blueprint.manu_line_scenario();
    let trade_order_mode = if trade_scenario.total_stations() == 0 {
        TradeSearchOrderMode::Single(TradeOrderKind::Gold)
    } else {
        TradeSearchOrderMode::Stations(trade_scenario)
    };
    let recipe_mode = ManuSearchRecipeMode::Lines(manu_scenario);

    let t2 = Instant::now();
    let trade_roster = operbox.trade_roster(instances);
    let trade_pool = build_trade_pool(&trade_roster, instances, table)?;
    phases.push(PhaseTiming {
        name: "build_trade_pool",
        elapsed: t2.elapsed(),
    });

    let t3 = Instant::now();
    let trade_report = search_trade_triples(
        &trade_pool,
        table,
        &TradeSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            gold_production_lines: blueprint.gold_manu_line_count(),
            order_mode: trade_order_mode,
            ..TradeSearchOptions::default()
        },
    )?;
    phases.push(PhaseTiming {
        name: "search_trade_triples",
        elapsed: t3.elapsed(),
    });

    let t4 = Instant::now();
    let manu_roster = operbox.manufacture_roster(instances);
    let manu_pool = build_manufacture_pool(&manu_roster, instances, table)?;
    phases.push(PhaseTiming {
        name: "build_manu_pool",
        elapsed: t4.elapsed(),
    });

    let t5 = Instant::now();
    let manu_report = search_manufacture_triples(
        &manu_pool,
        table,
        &ManuSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            recipe_mode,
            ..ManuSearchOptions::default()
        },
    )?;
    phases.push(PhaseTiming {
        name: "search_manufacture_triples",
        elapsed: t5.elapsed(),
    });

    let t6 = Instant::now();
    let rotation = schedule_team_rotation(
        blueprint,
        operbox,
        instances,
        table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;
    phases.push(PhaseTiming {
        name: "schedule_team_rotation",
        elapsed: t6.elapsed(),
    });

    let total = total_start.elapsed();
    let counters = hot_path_snapshot();
    let stats = WorkloadStats {
        trade_pool_ready: trade_pool.entries.len(),
        trade_combinations: trade_report.combinations,
        trade_evaluated: trade_report.evaluated,
        manu_pool_ready: manu_pool.entries.len(),
        manu_combinations: manu_report.combinations,
        manu_evaluated: manu_report.evaluated,
        rotation_shifts: rotation.shifts.len(),
        rotation_trade_peak: rotation
            .shifts
            .first()
            .map(|s| s.scores.trade_score)
            .unwrap_or(0.0),
        rotation_trade_recovery: rotation
            .shifts
            .get(1)
            .map(|s| s.scores.trade_score)
            .unwrap_or(0.0),
    };

    Ok(RunProfile {
        label: label.to_string(),
        total,
        phases,
        counters,
        stats,
    })
}

fn print_profile_report(runs: &[RunProfile]) {
    for run in runs {
        eprintln!();
        eprintln!("[{}] total={:.3}ms", run.label, ms(run.total));
        for phase in &run.phases {
            let pct = 100.0 * phase.elapsed.as_secs_f64() / run.total.as_secs_f64();
            eprintln!(
                "  {:<32} {:>8.3}ms ({:>5.1}%)",
                phase.name,
                ms(phase.elapsed),
                pct
            );
        }
        eprintln!("  hot_path:");
        eprintln!(
            "    shortcut_json_loads = {}",
            run.counters.shortcut_json_loads
        );
        eprintln!(
            "    exclusive_checks    = {}",
            run.counters.exclusive_checks
        );
        eprintln!("    trade_solves        = {}", run.counters.trade_solves);
        eprintln!("  workload:");
        eprintln!(
            "    trade pool ready={} combos={} evaluated={}",
            run.stats.trade_pool_ready, run.stats.trade_combinations, run.stats.trade_evaluated
        );
        eprintln!(
            "    manu  pool ready={} combos={} evaluated={}",
            run.stats.manu_pool_ready, run.stats.manu_combinations, run.stats.manu_evaluated
        );
        eprintln!(
            "    rotation shifts={} peak_trade={:.3} recovery_trade={:.3}",
            run.stats.rotation_shifts,
            run.stats.rotation_trade_peak,
            run.stats.rotation_trade_recovery
        );
    }

    if runs.len() > 1 {
        let avg_total: Duration =
            runs.iter().map(|r| r.total).sum::<Duration>() / runs.len() as u32;
        eprintln!();
        eprintln!(
            "aggregate: runs={} avg_total={:.3}ms min={:.3}ms max={:.3}ms",
            runs.len(),
            ms(avg_total),
            ms(runs.iter().map(|r| r.total).min().unwrap_or_default()),
            ms(runs.iter().map(|r| r.total).max().unwrap_or_default()),
        );
        let avg_shortcut: f64 = runs
            .iter()
            .map(|r| r.counters.shortcut_json_loads as f64)
            .sum::<f64>()
            / runs.len() as f64;
        let avg_exclusive: f64 = runs
            .iter()
            .map(|r| r.counters.exclusive_checks as f64)
            .sum::<f64>()
            / runs.len() as f64;
        let avg_solves: f64 = runs
            .iter()
            .map(|r| r.counters.trade_solves as f64)
            .sum::<f64>()
            / runs.len() as f64;
        eprintln!(
            "aggregate counters: shortcut_loads={avg_shortcut:.0} exclusive_checks={avg_exclusive:.0} trade_solves={avg_solves:.0}"
        );
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn profile_analyze_compare_cmd(args: &[String]) -> Result<(), Error> {
    let layout_path = args
        .windows(2)
        .find(|w| w[0] == "--layout")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from("data/fixtures/243/layout.json"));
    let operbox_path = args
        .windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from("data/operbox_knightcode.json"));
    let schedule_path = args
        .windows(2)
        .find(|w| w[0] == "--schedule")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| default_schedule_export_path().unwrap_or_else(|_| {
            PathBuf::from("data/fixtures/243/schedule_export.json")
        }));
    let runs = args
        .windows(2)
        .find(|w| w[0] == "--runs")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(3)
        .max(1);

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let layout_label = layout_path.to_string_lossy().into_owned();
    let operbox_label = operbox_path.to_string_lossy().into_owned();
    let options = BoxProfileOptions {
        top_k: 10,
        ..BoxProfileOptions::default()
    };

    eprintln!("=== profile analyze-compare (release) ===");
    eprintln!(
        "layout={} operbox={} owned={} schedule={} runs={}",
        layout_path.display(),
        operbox_path.display(),
        operbox.owned_count(),
        schedule_path.display(),
        runs
    );
    eprintln!("hybrid:  user team_rotation vs 公孙 schedule_export (full_e2 eval)");
    eprintln!("legacy:  2x full search probe (old analyze)");

    let mut hybrid_ms = Vec::new();
    let mut legacy_ms = Vec::new();

    for i in 0..runs {
        let t0 = Instant::now();
        let _ = build_box_profile(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &layout_label,
            &operbox_label,
            &BoxProfileOptions {
                top_k: 10,
                baseline_schedule: Some(schedule_path.clone()),
                ..options.clone()
            },
        )?;
        hybrid_ms.push(ms(t0.elapsed()));

        let t1 = Instant::now();
        let _ = run_layout_probe(&blueprint, &operbox, &instances, &table, 10)?;
        let _ = run_layout_probe(
            &blueprint,
            &OperBox::load(&infra_core::operbox::default_operbox_full_e2_path()?)?,
            &instances,
            &table,
            10,
        )?;
        legacy_ms.push(ms(t1.elapsed()));
        eprintln!(
            "  run {}: hybrid={:.1}ms legacy={:.1}ms speedup={:.1}x",
            i + 1,
            hybrid_ms[i],
            legacy_ms[i],
            legacy_ms[i] / hybrid_ms[i].max(0.001)
        );
    }

    let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let hybrid_avg = avg(&hybrid_ms);
    let legacy_avg = avg(&legacy_ms);
    eprintln!();
    eprintln!(
        "average: hybrid={hybrid_avg:.1}ms legacy={legacy_avg:.1}ms speedup={:.1}x",
        legacy_avg / hybrid_avg.max(0.001)
    );
    Ok(())
}
