use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::output::{
    emit_bench, emit_team_rotation, print_box_profile_report, print_team_rotation_text, BenchMeta,
    OutputFormat, OutputOptions, PoolSummary,
};
use infra_core::box_profile::{baseline_path_or_default, build_box_profile, BoxProfileOptions};
use infra_core::export::{build_from_team_rotation, MaaExportOptions, MaaSchedule};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::{
    assign_base_greedy, explain_assignment_systems, resolve_base, resolve_base_with_trace,
    AssignBaseOptions, AssignShiftMode, BaseAssignment, BaseBlueprint,
};
use infra_core::manufacture::input::ManuRoomInput;
use infra_core::manufacture::solve_manufacture;
use infra_core::manufacture::ManuSearchRecipeMode;
use infra_core::operbox::OperBox;
use infra_core::pool::{build_manufacture_pool, build_trade_pool};
use infra_core::power::{solve_power, PowerRoomInput};
use infra_core::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, TradeSearchOptions,
};
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::trade::input::{TradeOrderKind, TradeRoomInput, TradeSearchOrderMode};
use infra_core::trade::solve_trade_with_shift;
use infra_core::Error;

use super::prefix_shadow::{prefix_shadow_out_from_args, run_prefix_shadow, PrefixShadowInput};

pub fn layout_cmd(args: &[String]) -> Result<(), Error> {
    match args.first().map(String::as_str) {
        Some("test") => layout_test_cmd(&args[1..]),
        Some("analyze") => layout_analyze_cmd(&args[1..]),
        Some("eval") => layout_eval_cmd(&args[1..]),
        Some("team-rotation") => layout_team_rotation_cmd(&args[1..]),
        _ => {
            eprintln!("       --enable-support-combinations: enable proactive attachment combinations (default off)");
            eprintln!(
                "usage: infra-cli layout test --layout <path> --operbox <path> [--assignment <path>] [--top <n>] [--prefer <system=alternative>] [-o <file.csv>] [--text] [--explain-systems]"
            );
            eprintln!(
                "       infra-cli layout analyze --layout <path> --operbox <path> [--baseline <operbox>] [--top <n>] [-o profile.json] [--json]"
            );
            eprintln!(
                "       infra-cli layout eval --layout <path> --operbox <path> --assignment <path> [--text | --explain-json]"
            );
            eprintln!(
                "       infra-cli layout team-rotation --layout <path> --operbox <path> [--top <n>] [--rotation <named|main-backup@H1/H2|abc@H1/H2/H3>] [--no-fiammetta] [--prefix-shadow-out <file.json>] [--output-dir <dir>] [--maa-out <file.json>] [--maa-title <title>] [-o <file.csv>] [--text|--json]"
            );
            Err(Error::msg(format!(
                "unknown layout command {:?}",
                args.first().map(String::as_str)
            )))
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

fn fiammetta_candidates_out_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--fiammetta-candidates-out")
        .map(|w| PathBuf::from(&w[1]))
}

fn system_preferences_from_args(
    args: &[String],
) -> Result<std::collections::HashMap<String, String>, Error> {
    let mut preferences = std::collections::HashMap::new();
    for pair in args.windows(2).filter(|pair| pair[0] == "--prefer") {
        let Some((system, alternative)) = pair[1].split_once('=') else {
            return Err(Error::msg(format!(
                "invalid --prefer {}; expected system=alternative",
                pair[1]
            )));
        };
        if system.is_empty() || alternative.is_empty() {
            return Err(Error::msg(format!(
                "invalid --prefer {}; expected system=alternative",
                pair[1]
            )));
        }
        preferences.insert(system.to_string(), alternative.to_string());
    }
    Ok(preferences)
}

fn maa_export_options(args: &[String], blueprint: &BaseBlueprint) -> MaaExportOptions {
    let mut opts = MaaExportOptions::for_blueprint(blueprint);
    if !args.iter().any(|arg| arg == "--no-fiammetta") {
        opts.enable_fiammetta_priority_for_blueprint(blueprint);
    }
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

fn layout_team_rotation_cmd(args: &[String]) -> Result<(), Error> {
    let rotation_request = super::timed_rotation_request_from_args(args)?;
    let out = OutputOptions::from_args(args);
    let layout_path = layout_path_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let top_k = args
        .windows(2)
        .find(|w| w[0] == "--top")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(20);
    let system_preferences = system_preferences_from_args(args)?;
    let fiammetta_enable = !args.iter().any(|arg| arg == "--no-fiammetta");

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let options = AssignBaseOptions {
        top_k,
        system_preferences,
        fiammetta_enable,
        support_combinations_enable: args
            .iter()
            .any(|arg| arg == "--enable-support-combinations"),
        fiammetta_candidates_out: fiammetta_candidates_out_from_args(args),
        ..AssignBaseOptions::default()
    };
    let report = infra_core::schedule::schedule_timed_rotation_request(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &options,
        rotation_request.clone(),
    )?;

    if let Some(path) = prefix_shadow_out_from_args(args)? {
        run_prefix_shadow(PrefixShadowInput {
            path: &path,
            layout_label: &layout_path.to_string_lossy(),
            operbox_label: &operbox_path.to_string_lossy(),
            blueprint: &blueprint,
            operbox: &operbox,
            instances: &instances,
            table: &table,
            baseline_options: &options,
            rotation_request,
            baseline: &report,
        });
    }

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
            support_combinations_enable: args
                .iter()
                .any(|arg| arg == "--enable-support-combinations"),
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
        system_preferences: system_preferences_from_args(args)?,
        support_combinations_enable: args
            .iter()
            .any(|arg| arg == "--enable-support-combinations"),
        ..AssignBaseOptions::default()
    };
    let assignment = if let Some(path) = assignment_path_from_args(args) {
        match seed_assignment {
            Some(assignment) => assignment,
            None => BaseAssignment::load(&path)?,
        }
    } else {
        assign_base_greedy(&blueprint, &operbox, &instances, &table, &options)?
    };

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
    let explain_json = args.iter().any(|a| a == "--explain-json");
    if text && explain_json {
        return Err(Error::msg(
            "--text and --explain-json are mutually exclusive",
        ));
    }
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

    let (resolved, settlement_trace) = if explain_json {
        let (resolved, trace) = resolve_base_with_trace(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            Some(durin_plan),
        )?;
        (resolved, Some(trace))
    } else {
        (
            resolve_base(
                &blueprint,
                &assignment,
                Some(&instances),
                Some(&table),
                24.0,
                Some(durin_plan),
            )?,
            None,
        )
    };

    if text {
        eprintln!(
            "layout eval layout={} operbox={} assignment={} durin_in_base={}",
            layout_path.display(),
            operbox_path.display(),
            assignment_path.display(),
            resolved.layout.durin_in_base
        );
    }

    let mut trade_total = infra_core::Efficiency::ZERO;
    let mut trade_explain = Vec::new();
    for room in &resolved.trade_rooms {
        if room.operators.is_empty() {
            if explain_json {
                trade_explain.push(serde_json::json!({
                    "room_id": room.id.0.clone(),
                    "status": "empty",
                    "level": room.level,
                    "order": room.order,
                    "operators": [],
                }));
            }
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
        trade_total += result.efficiency.total_efficiency;
        if explain_json {
            trade_explain.push(serde_json::json!({
                "room_id": room.id.0.clone(),
                "status": "settled",
                "level": room.level,
                "order": room.order,
                "operators": room.operators.iter().map(|operator| operator.name.as_str()).collect::<Vec<_>>(),
                "result": &result,
            }));
        }
        if text {
            let names: Vec<_> = room.operators.iter().map(|o| o.name.as_str()).collect();
            eprintln!(
                "  trade {} {:?} ops={:?} total_efficiency={} base_efficiency={} global_efficiency={} skill_efficiency={} trade_output_per_day={:.2} gold_consumption_per_day={:.2} trade_equivalent={} gold_equivalent={} rule_id={:?}",
                room.id.0,
                room.order,
                names,
                result.efficiency.total_efficiency,
                result.efficiency.paper.base_efficiency,
                result.efficiency.paper.control_efficiency,
                result.efficiency.paper.skill_efficiency,
                result.production.unit.unit_trade_per_day
                    * result.efficiency.total_efficiency.as_f64(),
                result.production.unit.gold_consumption_per_day
                    * infra_core::trade::unit_output::GSL_GOLD_UNIT_SCALE
                    * result.efficiency.total_efficiency.as_f64(),
                result.efficiency.trade_equivalent_efficiency,
                result.efficiency.gold_equivalent_efficiency,
                result.rule_id
            );
        }
    }

    let mut manu_total = infra_core::Efficiency::ZERO;
    let mut manu_explain = Vec::new();
    for room in &resolved.manu_rooms {
        if room.operators.is_empty() {
            if explain_json {
                manu_explain.push(serde_json::json!({
                    "room_id": room.id.0.clone(),
                    "status": "empty",
                    "level": room.level,
                    "recipe": room.recipe,
                    "operators": [],
                }));
            }
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
        manu_total += result.final_efficiency;
        if explain_json {
            manu_explain.push(serde_json::json!({
                "room_id": room.id.0.clone(),
                "status": "settled",
                "level": room.level,
                "recipe": room.recipe,
                "operators": room.operators.iter().map(|operator| operator.name.as_str()).collect::<Vec<_>>(),
                "result": &result,
            }));
        }
        if text {
            let names: Vec<_> = room.operators.iter().map(|o| o.name.as_str()).collect();
            eprintln!(
                "  manu {} {:?} ops={:?} total_efficiency={} storage={}",
                room.id.0, room.recipe, names, result.final_efficiency, result.storage_limit
            );
        }
    }

    let mut power_explain = Vec::new();
    let mut power_total = infra_core::Efficiency::ZERO;
    if explain_json {
        let mut power_results = Vec::new();
        for room in &resolved.power_rooms {
            let result = solve_power(
                &PowerRoomInput {
                    operator: room.operator.clone(),
                    mood: room.operator.work_mood.unwrap_or(24.0),
                    shift_hours: 24.0,
                    layout: room.layout.clone(),
                },
                &table,
            )?;
            power_results.push(result.clone());
            power_explain.push(serde_json::json!({
                "room_id": room.id.0.clone(),
                "status": "settled",
                "operator": room.operator.name.clone(),
                "result": result,
            }));
        }
        power_total = infra_core::aggregate_power_efficiency(
            power_results.iter().map(|result| result.final_efficiency),
        );
    }

    if text {
        for room in resolved.office_rooms.iter().chain(&resolved.meeting_rooms) {
            if let Some(result) = &room.result {
                eprintln!(
                    "  support {} {:?} skill_speed={} external_speed={} total_speed={} unsupported={}",
                    room.id.0,
                    result.facility,
                    result.skill_speed_bonus_pct,
                    result.external_speed_bonus_pct,
                    result.total_speed_bonus_pct,
                    result.unsupported.len()
                );
            } else {
                eprintln!("  support {} autofill=true", room.id.0);
            }
        }
    }

    if explain_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "mode": "fixed_assignment_settlement",
                "inputs": {
                    "layout": layout_path,
                    "operbox": operbox_path,
                    "assignment": assignment_path,
                    "mood": 24.0,
                    "shift_hours": 24.0,
                },
                "control_trace": settlement_trace.and_then(|trace| trace.control),
                "rooms": {
                    "trade": trade_explain,
                    "manufacture": manu_explain,
                    "power": power_explain,
                    "office": resolved.office_rooms.iter().map(|room| serde_json::json!({
                        "room_id": room.id.0.clone(),
                        "result": &room.result,
                        "autofill": room.autofill,
                    })).collect::<Vec<_>>(),
                    "meeting": resolved.meeting_rooms.iter().map(|room| serde_json::json!({
                        "room_id": room.id.0.clone(),
                        "result": &room.result,
                        "autofill": room.autofill,
                    })).collect::<Vec<_>>(),
                },
                "totals": {
                    "trade_efficiency": trade_total,
                    "manufacture_efficiency": manu_total,
                    "power_efficiency": power_total,
                },
                "durin_in_base": resolved.layout.durin_in_base,
            }))?
        );
    } else if text {
        eprintln!(
            "  total trade_efficiency={} manufacture_efficiency={}",
            trade_total, manu_total
        );
    } else {
        println!(
            "{}",
            serde_json::json!({
                "trade_efficiency": trade_total,
                "manufacture_efficiency": manu_total,
                "durin_in_base": resolved.layout.durin_in_base,
                "office": resolved.office_rooms.iter().map(|room| serde_json::json!({
                    "room_id": room.id.0.clone(),
                    "result": &room.result,
                    "autofill": room.autofill,
                })).collect::<Vec<_>>(),
                "meeting": resolved.meeting_rooms.iter().map(|room| serde_json::json!({
                    "room_id": room.id.0.clone(),
                    "result": &room.result,
                    "autofill": room.autofill,
                })).collect::<Vec<_>>(),
            })
        );
    }
    Ok(())
}
