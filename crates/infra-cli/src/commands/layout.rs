use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::output::{
    emit_base_rotation, emit_bench, emit_team_rotation, print_base_rotation_text,
    print_box_profile_report, print_team_rotation_text, BenchMeta, OutputFormat, OutputOptions,
    PoolSummary,
};
use infra_core::box_profile::{baseline_path_or_default, build_box_profile, BoxProfileOptions};
use infra_core::export::{
    build_from_base_rotation, build_from_team_rotation, MaaExportOptions, MaaSchedule,
};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::{
    assign_base_greedy, assign_shift_with_plan_and_trace, explain_assignment_systems, resolve_base,
    AssignBaseOptions, AssignShiftMode, BaseAssignment, BaseBlueprint,
};
use infra_core::manufacture::input::ManuRoomInput;
use infra_core::manufacture::solve_manufacture;
use infra_core::manufacture::ManuSearchRecipeMode;
use infra_core::operbox::OperBox;
use infra_core::pool::{build_manufacture_pool, build_trade_pool};
use infra_core::schedule::{schedule_base_rotation_a_b_a, schedule_team_rotation};
use infra_core::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, TradeSearchOptions,
};
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::trade::input::{TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use infra_core::trade::solve_trade_with_shift;
use infra_core::Error;

pub fn layout_cmd(args: &[String]) -> Result<(), Error> {
    match args.first().map(String::as_str) {
        Some("test") => layout_test_cmd(&args[1..]),
        Some("analyze") => layout_analyze_cmd(&args[1..]),
        Some("eval") => layout_eval_cmd(&args[1..]),
        Some("rotation") => layout_rotation_cmd(&args[1..]),
        Some("team-rotation") => layout_team_rotation_cmd(&args[1..]),
        _ => {
            eprintln!(
                "usage: infra-cli layout test --layout <path> --operbox <path> [--assignment <path>] [--top <n>] [-o <file.csv>] [--text] [--explain-systems] [--debug-manufacture-trace]"
            );
            eprintln!(
                "       infra-cli layout analyze --layout <path> --operbox <path> [--baseline <operbox>] [--top <n>] [-o profile.json] [--json]"
            );
            eprintln!(
                "       infra-cli layout eval --layout <path> --operbox <path> --assignment <path> [--text]"
            );
            eprintln!(
                "       infra-cli layout rotation --layout <path> --operbox <path> [--top <n>] [--output-dir <dir>] [--maa-out <file.json>] [-o <file.csv>] [--text|--json]"
            );
            eprintln!(
                "       infra-cli layout team-rotation --layout <path> --operbox <path> [--top <n>] [--output-dir <dir>] [--maa-out <file.json>] [--maa-title <title>] [-o <file.csv>] [--text|--json]"
            );
            Ok(())
        }
    }
}

fn layout_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    args.windows(2)
        .find(|w| w[0] == "--layout")
        .map(|w| PathBuf::from(&w[1]))
        .ok_or_else(|| Error::msg("missing --layout <path>"))
}

fn operbox_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    args.windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .ok_or_else(|| Error::msg("missing --operbox <path>"))
}

fn assignment_path_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--assignment")
        .map(|w| PathBuf::from(&w[1]))
}

fn output_dir_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--output-dir")
        .map(|w| PathBuf::from(&w[1]))
}

fn maa_out_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--maa-out")
        .map(|w| PathBuf::from(&w[1]))
}

fn debug_manufacture_trace_path_from_args(args: &[String]) -> Option<Option<PathBuf>> {
    args.iter()
        .position(|arg| arg == "--debug-manufacture-trace")
        .map(|idx| {
            let next = args.get(idx + 1);
            if let Some(value) = next {
                if !value.starts_with('-') {
                    return Some(PathBuf::from(value));
                }
            }
            None
        })
}

#[derive(Debug, serde::Serialize)]
struct ManufactureTraceDebugReport<'a> {
    schema: &'static str,
    layout: &'a str,
    operbox: &'a str,
    manufacture_traces: &'a [infra_core::layout::ManufactureSystemCandidateTrace],
    team_candidates: &'a [infra_core::TeamCandidate],
}

fn emit_manufacture_trace_debug_report(
    path: Option<&Path>,
    report: &ManufactureTraceDebugReport<'_>,
) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(report)?;
    if let Some(path) = path {
        fs::write(path, json + "\n")?;
        eprintln!("manufacture trace JSON → {}", path.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn maa_export_options(args: &[String], blueprint: &BaseBlueprint) -> MaaExportOptions {
    let mut opts = MaaExportOptions::for_blueprint(blueprint);
    if let Some(title) = args
        .windows(2)
        .find(|w| w[0] == "--maa-title")
        .map(|w| w[1].clone())
    {
        opts.title = title;
    }
    opts
}

fn write_maa_schedule(path: &Path, schedule: &MaaSchedule) -> Result<(), Error> {
    schedule.save(path)?;
    Ok(())
}

fn emit_maa_hint(path: &Path, layout: &str, operbox: &str, owned: usize) {
    eprintln!();
    eprintln!("MAA 排班 JSON 已写入: {}", path.display());
    eprintln!("  layout={layout} operbox={operbox} owned={owned}");
    eprintln!("  导入 MAA：任务设置 → 基建换班 → 自定义模式 → 选择该 JSON（plan_index 从 0 起）");
}

fn should_emit_primary_output(out: &OutputOptions, wrote_maa: bool) -> bool {
    if !wrote_maa {
        return true;
    }
    out.path.is_some() || out.format != OutputFormat::Csv
}

fn layout_rotation_cmd(args: &[String]) -> Result<(), Error> {
    eprintln!(
        "警告: `layout rotation`（A-B-A）已废弃，请改用 `layout team-rotation`（αβγ ABC 轮换）。见 docs/SCHEDULE_ROTATION.md"
    );
    let out = OutputOptions::from_args(args);
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(20);

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let report = schedule_base_rotation_a_b_a(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;

    if let Some(dir) = output_dir_from_args(args) {
        write_rotation_assignments(&dir, &report)?;
    }

    let layout_str = layout_path.to_string_lossy();
    let operbox_str = operbox_path.to_string_lossy();
    let owned = operbox.owned_count();
    let maa_path = maa_out_from_args(args);

    if let Some(ref maa_path) = maa_path {
        let maa_opts = maa_export_options(args, &blueprint);
        let schedule = build_from_base_rotation(&blueprint, &report, &maa_opts)?;
        write_maa_schedule(maa_path, &schedule)?;
        if out.format != OutputFormat::Text {
            print_base_rotation_text(layout_str.as_ref(), operbox_str.as_ref(), owned, &report)?;
        }
        emit_maa_hint(maa_path, layout_str.as_ref(), operbox_str.as_ref(), owned);
    }

    if should_emit_primary_output(&out, maa_path.is_some()) {
        emit_base_rotation(
            &out,
            layout_str.as_ref(),
            operbox_str.as_ref(),
            owned,
            &report,
        )?;
    }
    Ok(())
}

fn write_rotation_assignments(
    dir: &Path,
    report: &infra_core::schedule::BaseRotationReport,
) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    for shift in &report.shifts {
        let role = match shift.role {
            infra_core::schedule::BaseShiftRole::Peak => "peak",
            infra_core::schedule::BaseShiftRole::Recovery => "recovery",
        };
        let reuse = shift
            .reused_from_shift
            .map(|i| format!("_reuse{}", i + 1))
            .unwrap_or_default();
        let path = dir.join(format!(
            "assignment_{}_{}{}.json",
            shift.index + 1,
            role,
            reuse
        ));
        shift.assignment.save(&path)?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

fn layout_team_rotation_cmd(args: &[String]) -> Result<(), Error> {
    let out = OutputOptions::from_args(args);
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(20);

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let report = schedule_team_rotation(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;

    if let Some(dir) = output_dir_from_args(args) {
        fs::create_dir_all(&dir)?;
        for shift in &report.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
            eprintln!("wrote {}", path.display());
        }
    }

    let layout_str = layout_path.to_string_lossy();
    let operbox_str = operbox_path.to_string_lossy();
    let owned = operbox.owned_count();
    let maa_path = maa_out_from_args(args);

    if let Some(ref maa_path) = maa_path {
        let maa_opts = maa_export_options(args, &blueprint);
        let schedule = build_from_team_rotation(&blueprint, &report, &maa_opts)?;
        write_maa_schedule(maa_path, &schedule)?;
        if out.format != OutputFormat::Text {
            print_team_rotation_text(layout_str.as_ref(), operbox_str.as_ref(), owned, &report)?;
        }
        emit_maa_hint(maa_path, layout_str.as_ref(), operbox_str.as_ref(), owned);
    }

    if should_emit_primary_output(&out, maa_path.is_some()) {
        emit_team_rotation(
            &out,
            layout_str.as_ref(),
            operbox_str.as_ref(),
            owned,
            &report,
        )?;
    }
    Ok(())
}

fn baseline_path_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--baseline")
        .map(|w| PathBuf::from(&w[1]))
}

fn profile_out_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--profile-out")
        .map(|w| PathBuf::from(&w[1]))
        .or_else(|| {
            args.windows(2)
                .find(|w| w[0] == "-o")
                .map(|w| PathBuf::from(&w[1]))
        })
}

fn layout_analyze_cmd(args: &[String]) -> Result<(), Error> {
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(10);
    let json_only = args.iter().any(|a| a == "--json");

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let baseline_path = baseline_path_or_default(baseline_path_from_args(args).as_deref())?;

    let profile = build_box_profile(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &layout_path.to_string_lossy(),
        &operbox_path.to_string_lossy(),
        &BoxProfileOptions {
            top_k,
            baseline_operbox: Some(baseline_path),
            ..BoxProfileOptions::default()
        },
    )?;

    if let Some(out_path) = profile_out_from_args(args) {
        let json = serde_json::to_string_pretty(&profile)?;
        std::fs::write(&out_path, json + "\n")?;
        eprintln!("profile JSON → {}", out_path.display());
    }

    if json_only {
        print!("{}", serde_json::to_string_pretty(&profile)?);
    } else {
        print_box_profile_report(&profile);
    }

    Ok(())
}

fn layout_test_cmd(args: &[String]) -> Result<(), Error> {
    let out = OutputOptions::from_args(args);
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let explain_systems = args.iter().any(|a| a == "--explain-systems");
    let debug_manufacture_trace = debug_manufacture_trace_path_from_args(args);
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(3);

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let seed_assignment = if let Some(path) = assignment_path_from_args(args) {
        Some(BaseAssignment::load(&path)?)
    } else {
        None
    };

    if debug_manufacture_trace.is_some() && seed_assignment.is_some() {
        return Err(Error::msg(
            "--debug-manufacture-trace requires generating a fresh assignment; omit --assignment",
        ));
    }

    if explain_systems {
        let empty_seed = BaseAssignment::default();
        let seed = seed_assignment.as_ref().unwrap_or(&empty_seed);
        let report = explain_assignment_systems(&blueprint, &operbox, AssignShiftMode::Peak, seed);
        if out.format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        print_system_explain_report(&report);
    }

    let options = AssignBaseOptions {
        top_k,
        ..AssignBaseOptions::default()
    };
    let mut manufacture_trace_report = None;
    let assignment = if let Some(path) = assignment_path_from_args(args) {
        match seed_assignment {
            Some(assignment) => assignment,
            None => BaseAssignment::load(&path)?,
        }
    } else if debug_manufacture_trace.is_some() {
        let result = assign_shift_with_plan_and_trace(
            &blueprint,
            &operbox,
            &instances,
            &table,
            &options,
            AssignShiftMode::Peak,
            &BaseAssignment::default(),
        )?;
        manufacture_trace_report = Some(result.manufacture_traces);
        result.assignment
    } else {
        assign_base_greedy(&blueprint, &operbox, &instances, &table, &options)?
    };

    if let Some(path) = debug_manufacture_trace {
        let layout_str = layout_path.to_string_lossy();
        let operbox_str = operbox_path.to_string_lossy();
        let traces = manufacture_trace_report.unwrap_or_default();
        let team_candidates: Vec<infra_core::TeamCandidate> = traces
            .iter()
            .map(infra_core::TeamCandidate::from_manufacture_system_trace)
            .collect();
        let report = ManufactureTraceDebugReport {
            schema: "manufacture_trace_debug_v0",
            layout: layout_str.as_ref(),
            operbox: operbox_str.as_ref(),
            manufacture_traces: &traces,
            team_candidates: &team_candidates,
        };
        emit_manufacture_trace_debug_report(path.as_deref(), &report)?;
        if path.is_none() {
            return Ok(());
        }
    }

    let durin_plan = operbox.durin_dorm_planning_count(&instances);
    let resolved = resolve_base(
        &blueprint,
        &assignment,
        Some(&instances),
        Some(&table),
        24.0,
        Some(durin_plan),
    )?;
    let layout = Arc::new(resolved.layout_snapshot());

    let trade_scenario = blueprint.trade_station_scenario();
    let manu_scenario = blueprint.manu_line_scenario();
    let trade_order_mode = if trade_scenario.total_stations() == 0 {
        TradeSearchOrderMode::Single(TradeOrderKind::Gold)
    } else {
        TradeSearchOrderMode::Stations(trade_scenario)
    };
    let recipe_mode = ManuSearchRecipeMode::Lines(manu_scenario);

    let trade_mode_label = match trade_order_mode {
        TradeSearchOrderMode::Single(kind) => format!("single:{kind:?}"),
        TradeSearchOrderMode::Stations(s) => format!(
            "{} stations ({} gold + {} originium)",
            s.total_stations(),
            s.gold_order_stations,
            s.originium_order_stations
        ),
    };
    let manu_scenario_label = format!(
        "{} lines ({} gold + {} battle_record + {} originium)",
        manu_scenario.total_lines(),
        manu_scenario.gold_lines,
        manu_scenario.battle_record_lines,
        manu_scenario.originium_lines
    );

    let trade_roster = operbox.trade_roster(&instances);
    let trade_pool = build_trade_pool(&trade_roster, &instances, &table)?;
    let trade_stats = trade_pool.stats(3);
    let trade_report = search_trade_triples(
        &trade_pool,
        &table,
        &TradeSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            gold_production_lines: blueprint.gold_manu_line_count(),
            order_mode: trade_order_mode,
            ..TradeSearchOptions::default()
        },
    )?;

    let manu_roster = operbox.manufacture_roster(&instances);
    let manu_pool = build_manufacture_pool(&manu_roster, &instances, &table)?;
    let manu_stats = manu_pool.stats(3);
    let manu_report = search_manufacture_triples(
        &manu_pool,
        &table,
        &ManuSearchOptions {
            top_k,
            layout: Arc::clone(&layout),
            recipe_mode,
            ..ManuSearchOptions::default()
        },
    )?;

    let layout_str = layout_path.to_string_lossy();
    let operbox_str = operbox_path.to_string_lossy();
    emit_bench(
        &out,
        &BenchMeta {
            operbox: operbox_str.as_ref(),
            owned: operbox.owned_count(),
            layout: Some(layout_str.as_ref()),
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

fn print_system_explain_report(report: &infra_core::layout::SystemExplainReport) {
    eprintln!("system explain mode={:?}", report.mode);
    for entry in &report.systems {
        let status = match entry.status {
            infra_core::layout::SystemExplainStatus::Selected => "selected",
            infra_core::layout::SystemExplainStatus::Skipped => "skipped",
        };
        let reason = entry
            .reason
            .as_ref()
            .map(|r| format!(" reason={} {}", r.code, r.message))
            .unwrap_or_default();
        eprintln!(
            "  [{status}] {} tier={:?} priority={}{}",
            entry.system_id, entry.tier, entry.priority, reason
        );
        for slot in &entry.slots {
            let slot_status = match slot.status {
                infra_core::layout::SystemExplainStatus::Selected => "selected",
                infra_core::layout::SystemExplainStatus::Skipped => "skipped",
            };
            let room = slot
                .resolved_room_id
                .as_ref()
                .map(|id| id.0.as_str())
                .or(slot.room_id.as_deref())
                .unwrap_or("-");
            let ops = if slot.operators.is_empty() {
                "-".to_string()
            } else {
                slot.operators.join("+")
            };
            let reason = slot
                .reason
                .as_ref()
                .map(|r| format!(" reason={} {}", r.code, r.message))
                .unwrap_or_default();
            eprintln!(
                "      [{slot_status}] {} room={} optional={} ops={}{}",
                slot.facility, room, slot.optional, ops, reason
            );
        }
    }
}

fn layout_eval_cmd(args: &[String]) -> Result<(), Error> {
    let text = args.iter().any(|a| a == "--text");
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let assignment_path = assignment_path_from_args(args)
        .ok_or_else(|| Error::msg("layout eval requires --assignment <path>"))?;

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let assignment = BaseAssignment::load(&assignment_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let durin_plan = operbox.durin_dorm_planning_count(&instances);

    let resolved = resolve_base(
        &blueprint,
        &assignment,
        Some(&instances),
        Some(&table),
        24.0,
        Some(durin_plan),
    )?;

    if text {
        eprintln!(
            "layout eval layout={} operbox={} assignment={} durin_in_base={}",
            layout_path.display(),
            operbox_path.display(),
            assignment_path.display(),
            resolved.layout.durin_in_base
        );
    }

    let mut trade_total = 0.0;
    for room in &resolved.trade_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let input = TradeRoomInput {
            level: room.level,
            operators: room.operators.clone(),
            order_count: None,
            mood: 24.0,
            gold_production_lines: Some(resolved.gold_manu_line_count()),
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(room.layout.clone()),
            active_order_kind: room.order,
        };
        let result = solve_trade_with_shift(&input, &table, 24.0)?;
        trade_total += result.effective_eff_multiplier;
        if text {
            let names: Vec<_> = room.operators.iter().map(|o| o.name.as_str()).collect();
            eprintln!(
                "  trade {} {:?} ops={:?} score={:.3} trade%={:.1} gold%={:.1} shortcut={:?}",
                room.id.0,
                room.order,
                names,
                result.effective_eff_multiplier,
                result.order_eff_total,
                result.order_mechanic.mechanic_equiv_eff_pct,
                result.trade_shortcut
            );
        }
    }

    let mut manu_total = 0.0;
    for room in &resolved.manu_rooms {
        if room.operators.is_empty() {
            continue;
        }
        let input = ManuRoomInput {
            level: room.level,
            operators: room.operators.clone(),
            active_recipe: room.recipe,
            mood: 24.0,
            layout: Arc::new(room.layout.clone()),
        };
        let result = solve_manufacture(&input, &table)?;
        manu_total += result.prod_total;
        if text {
            let names: Vec<_> = room.operators.iter().map(|o| o.name.as_str()).collect();
            eprintln!(
                "  manu {} {:?} ops={:?} prod%={:.1} storage={}",
                room.id.0, room.recipe, names, result.prod_total, result.storage_limit
            );
        }
    }

    if text {
        eprintln!(
            "  total trade_score={:.3} manu_prod_sum={:.1}",
            trade_total, manu_total
        );
    } else {
        println!(
            "{{\"trade_score\":{trade_total:.6},\"manu_prod_sum\":{manu_total:.6},\"durin_in_base\":{}}}",
            resolved.layout.durin_in_base
        );
    }
    Ok(())
}
