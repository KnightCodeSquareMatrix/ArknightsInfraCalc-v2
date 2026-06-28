use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use infra_core::bake::{
    bake_catalogs, validate_baked_catalog, warm_runtime_baked_table, BakeGeneratorFingerprint,
    BakeOptions, BakeProgressEvent,
};
use infra_core::box_profile::{
    baseline_path_or_default, build_box_profile_from_current_probe, run_user_rotation_probe,
    BoxProfileOptions,
};
use infra_core::export::{build_from_team_rotation, MaaExportOptions};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{default_layout_243_path, default_operbox_full_e2_path, OperBox};
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

    let _bake_handle = spawn_background_bake();
    match warm_runtime_baked_table() {
        Ok(true) => eprintln!("infra-cli serve: baked combo table warmed"),
        Ok(false) => {}
        Err(err) => eprintln!("infra-cli serve: baked combo table warm failed: {err}"),
    }

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

fn spawn_background_bake() -> Option<thread::JoinHandle<()>> {
    let out_dir = PathBuf::from("data/baked");
    let generator = current_generator_fingerprint().ok()?;
    if validate_baked_catalog(&out_dir, &generator).is_ok() {
        return None;
    }
    eprintln!(
        "infra-cli serve: baked catalog missing or stale, baking in background -> {}",
        out_dir.display()
    );
    Some(thread::spawn(move || {
        let mut options = BakeOptions::default();
        options.out_dir = out_dir;
        options.generator = Some(generator);
        options.progress = Some(std::sync::Arc::new(print_bake_progress_line));
        if let Err(err) = bake_catalogs(&options) {
            eprintln!("infra-cli serve background bake failed: {err}");
        }
    }))
}

fn current_generator_fingerprint() -> Result<BakeGeneratorFingerprint, Error> {
    let path = std::env::current_exe()?;
    let bytes = fs::read(&path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    bytes.hash(&mut hasher);
    Ok(BakeGeneratorFingerprint {
        kind: "infra-cli-exe".to_string(),
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
        hash64: format!("{:016x}", hasher.finish()),
    })
}

fn print_bake_progress_line(event: BakeProgressEvent) {
    match event {
        BakeProgressEvent::Started {
            out_dir,
            operator_count,
            signature_count,
            worker_count,
        } => eprintln!(
            "[bake] start operators={} signatures={} rayon_workers={} -> {}",
            operator_count,
            signature_count,
            worker_count,
            out_dir.display()
        ),
        BakeProgressEvent::SignatureStarted {
            facility,
            signature_key,
            combo_count,
        } => eprintln!("[bake] {facility} {signature_key}: enumerating {combo_count} combos"),
        BakeProgressEvent::SignatureFinished {
            facility,
            signature_key,
            rows,
            elapsed_ms,
        } => eprintln!("[bake] {facility} {signature_key}: rows={rows} elapsed={elapsed_ms}ms"),
        BakeProgressEvent::Writing { path, rows } => {
            if let Some(rows) = rows {
                eprintln!("[bake] write {} rows={rows}", path.display());
            } else {
                eprintln!("[bake] write {}", path.display());
            }
        }
        BakeProgressEvent::Finished {
            combo_table_rows,
            elapsed_ms,
        } => eprintln!("[bake] done combo_table_rows={combo_table_rows} elapsed={elapsed_ms}ms"),
    }
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
    #[serde(default)]
    operbox: Option<PathBuf>,
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
    let operbox_path = params.operbox.unwrap_or(default_operbox_full_e2_path()?);
    let operbox = OperBox::load(&operbox_path)?;
    let baseline_path = baseline_path_or_default(params.baseline.as_deref())?;

    let layout_label = layout_path.to_string_lossy().into_owned();
    let operbox_label = operbox_path.to_string_lossy().into_owned();
    let current =
        run_user_rotation_probe(&blueprint, &operbox, &state.instances, &state.table, top)?;

    if let Some(dir) = params.output_dir.as_ref() {
        fs::create_dir_all(dir)?;
        for shift in &current.rotation.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
        }
    }

    if let Some(path) = params.maa_out.as_ref() {
        let mut maa_opts = MaaExportOptions::for_blueprint(&blueprint);
        if let Some(title) = params.maa_title.clone() {
            maa_opts.title = title;
        }
        let schedule = build_from_team_rotation(&blueprint, &current.rotation, &maa_opts)?;
        schedule.save(path)?;
    }

    if let Some(path) = params.profile_out.as_ref() {
        let profile = build_box_profile_from_current_probe(
            &current,
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
        write_pretty_json(path, &profile)?;
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
        daily_trade: current.rotation.daily.trade,
        daily_manu: current.rotation.daily.manu,
        daily_power: current.rotation.daily.power,
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
