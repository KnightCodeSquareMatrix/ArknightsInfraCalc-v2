use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use infra_core::box_profile::{baseline_path_or_default, build_box_profile, BoxProfileOptions};
use infra_core::export::{build_from_team_rotation, MaaExportOptions};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::{AssignBaseOptions, BaseBlueprint};
use infra_core::operbox::{default_layout_243_path, OperBox};
use infra_core::schedule::schedule_team_rotation;
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::Error;
use serde::{Deserialize, Serialize};

pub fn serve_cmd(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_serve_usage();
        return Ok(());
    }

    let state = ServeState {
        instances: OperatorInstances::load(&default_instances_path()?)?,
        table: SkillTable::load(&default_skill_table_path()?)?,
    };

    eprintln!("infra-cli serve ready: line-delimited JSON on stdin/stdout");
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_request(&state, &line);
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

fn print_serve_usage() {
    eprintln!("Usage:");
    eprintln!("  infra-cli serve");
    eprintln!("Protocol:");
    eprintln!("  stdin:  one JSON request per line");
    eprintln!("  stdout: one JSON response per line");
    eprintln!("Methods:");
    eprintln!(
        "  plan: params {{ operbox, layout?, baseline?, top?, profile_out?, maa_out?, output_dir?, maa_title? }}"
    );
}

struct ServeState {
    instances: OperatorInstances,
    table: SkillTable,
}

#[derive(Debug, Deserialize)]
struct ServeRequest {
    #[serde(default)]
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ServeResponse<T: Serialize> {
    id: serde_json::Value,
    ok: bool,
    elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ServeError>,
}

#[derive(Debug, Serialize)]
struct ServeError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct PlanParams {
    operbox: PathBuf,
    #[serde(default)]
    layout: Option<PathBuf>,
    #[serde(default)]
    baseline: Option<PathBuf>,
    #[serde(default)]
    top: Option<usize>,
    #[serde(default)]
    profile_out: Option<PathBuf>,
    #[serde(default)]
    maa_out: Option<PathBuf>,
    #[serde(default)]
    output_dir: Option<PathBuf>,
    #[serde(default)]
    maa_title: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanResult {
    layout: String,
    operbox: String,
    owned: usize,
    top: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maa_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dir: Option<String>,
    daily_trade: f64,
    daily_manu: f64,
    daily_power: f64,
}

fn handle_request(state: &ServeState, line: &str) -> ServeResponse<serde_json::Value> {
    let start = Instant::now();
    let request: ServeRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(err) => {
            return error_response(
                serde_json::Value::Null,
                start.elapsed().as_millis(),
                format!("invalid request json: {err}"),
            );
        }
    };
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "plan" => handle_plan(state, request.params),
        "ping" => Ok(serde_json::json!({ "pong": true })),
        other => Err(Error::msg(format!("unknown serve method {other:?}"))),
    };
    match result {
        Ok(result) => ServeResponse {
            id,
            ok: true,
            elapsed_ms: start.elapsed().as_millis(),
            result: Some(result),
            error: None,
        },
        Err(err) => error_response(id, start.elapsed().as_millis(), err.to_string()),
    }
}

fn error_response(
    id: serde_json::Value,
    elapsed_ms: u128,
    message: String,
) -> ServeResponse<serde_json::Value> {
    ServeResponse {
        id,
        ok: false,
        elapsed_ms,
        result: None,
        error: Some(ServeError { message }),
    }
}

fn handle_plan(state: &ServeState, params: serde_json::Value) -> Result<serde_json::Value, Error> {
    let params: PlanParams = serde_json::from_value(params)?;
    let top = params.top.unwrap_or(20);
    let layout_path = match params.layout {
        Some(path) => path,
        None => default_layout_243_path()?,
    };
    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&params.operbox)?;
    let baseline_path = baseline_path_or_default(params.baseline.as_deref())?;

    let layout_label = layout_path.to_string_lossy().into_owned();
    let operbox_label = params.operbox.to_string_lossy().into_owned();
    let profile = build_box_profile(
        &blueprint,
        &operbox,
        &state.instances,
        &state.table,
        &layout_label,
        &operbox_label,
        &BoxProfileOptions {
            top_k: top,
            baseline_operbox: Some(baseline_path),
            ..BoxProfileOptions::default()
        },
    )?;

    if let Some(path) = params.profile_out.as_ref() {
        write_pretty_json(path, &profile)?;
    }

    let rotation = schedule_team_rotation(
        &blueprint,
        &operbox,
        &state.instances,
        &state.table,
        &AssignBaseOptions {
            top_k: top,
            ..AssignBaseOptions::default()
        },
    )?;

    if let Some(dir) = params.output_dir.as_ref() {
        fs::create_dir_all(dir)?;
        for shift in &rotation.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
        }
    }

    if let Some(path) = params.maa_out.as_ref() {
        let mut maa_opts = MaaExportOptions::for_blueprint(&blueprint);
        if let Some(title) = params.maa_title.clone() {
            maa_opts.title = title;
        }
        let schedule = build_from_team_rotation(&blueprint, &rotation, &maa_opts)?;
        schedule.save(path)?;
    }

    let result = PlanResult {
        layout: layout_label,
        operbox: operbox_label,
        owned: operbox.owned_count(),
        top,
        profile_out: params
            .profile_out
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        maa_out: params
            .maa_out
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        output_dir: params
            .output_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        daily_trade: rotation.daily.trade,
        daily_manu: rotation.daily.manu,
        daily_power: rotation.daily.power,
    };
    serde_json::to_value(result).map_err(Error::from)
}

fn write_pretty_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{json}\n"))?;
    Ok(())
}
