use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use super::plan_compute::{compute_plan, PlanComputeInput, PlanResources, RequestedOutputs};
use infra_core::bake::{
    bake_catalogs, default_baked_out_dir, set_runtime_bake_in_progress, validate_baked_catalog,
    warm_runtime_baked_table, BakeGeneratorFingerprint, BakeOptions, BakeProgressEvent,
};
use infra_core::box_profile::{baseline_path_or_default, BoxProfile};
use infra_core::export::MaaSchedule;
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{
    default_layout_243_path, default_operbox_full_e2_path, OperBox, OperBoxEntry,
};
use infra_core::schedule::{
    DailyTotals, RoomEfficiencyLine, ShiftEfficiencies, TeamLabel, TeamRotationReport,
    TimedRotationProfile,
};
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::training_advice::{
    build_player_training_advice, default_training_advice_knowledge_path,
    load_training_advice_knowledge, PlayerAdviceContext, PlayerTrainingAdviceReport,
    TrainingAdviceKnowledge,
};
use infra_core::Error;
use jsonschema::{Draft, JSONSchema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PLAN_SCHEMA_VERSION: u32 = 2;
const COMPAT_PLAN_SCHEMA_VERSION: u32 = 1;
const PROTOCOL_VERSION: u32 = 1;
const PLAN_COMPUTE_SCHEMA_V1_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v1.schema.json");
const PLAN_COMPUTE_SCHEMA_V2_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v2.schema.json");
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_OPERBOX_ENTRIES: usize = 1000;
const MAX_LAYOUT_ROOMS: usize = 64;
const MAX_LABEL_BYTES: usize = 200;
const MAX_TOP_K: usize = 100;

mod solver_build_info {
    include!(concat!(env!("OUT_DIR"), "/solver_build_info.rs"));
}

pub fn serve_cmd(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_serve_usage();
        return Ok(());
    }

    let state = ServeState {
        instances: OperatorInstances::load(&default_instances_path()?)?,
        table: SkillTable::load(&default_skill_table_path()?)?,
        training_advice_knowledge: load_training_advice_knowledge(
            &default_training_advice_knowledge_path()?,
        )?,
        plan_schema_v1: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V1_JSON)?,
        plan_schema_v2: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V2_JSON)?,
        plan_contract_sha256_v1: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V1_JSON),
        plan_contract_sha256_v2: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V2_JSON),
        solver_executable_sha256: solver_executable_sha256().unwrap_or_else(|err| {
            eprintln!("infra-cli serve: could not fingerprint executable: {err}");
            String::new()
        }),
        solver: SolverBuildInfo::current(),
    };

    let _bake_handle = spawn_background_bake();
    match warm_runtime_baked_table() {
        Ok(true) => eprintln!("infra-cli serve: baked combo table warmed"),
        Ok(false) => {}
        Err(err) => eprintln!("infra-cli serve: baked combo table warm failed: {err}"),
    }

    eprintln!("infra-cli serve ready: line-delimited JSON on stdin/stdout");
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout().lock();
    loop {
        let Some(frame) = read_ndjson_frame(&mut input, MAX_FRAME_BYTES)? else {
            break;
        };
        let response = match frame {
            NdjsonFrame::Line(line) if line.trim().is_empty() => continue,
            NdjsonFrame::Line(line) => handle_request(&state, &line),
            NdjsonFrame::TooLarge => coded_error_response(
                serde_json::Value::Null,
                0,
                "FRAME_TOO_LARGE",
                "protocol",
                format!("request frame exceeds {MAX_FRAME_BYTES} bytes"),
            ),
            NdjsonFrame::InvalidUtf8 => coded_error_response(
                serde_json::Value::Null,
                0,
                "INVALID_UTF8",
                "protocol",
                "request frame must be UTF-8".to_string(),
            ),
        };
        write_response(&mut stdout, response)?;
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
        "  plan: params {{ operbox, layout?, baseline?, top?, rotation?, profile_out?, maa_out?, output_dir?, maa_title? }}"
    );
    eprintln!("  plan.compute: params {{ schema_version, layout, operbox, labels?, options? }}");
}

enum NdjsonFrame {
    Line(String),
    TooLarge,
    InvalidUtf8,
}

fn read_ndjson_frame(
    reader: &mut impl BufRead,
    max_bytes: usize,
) -> io::Result<Option<NdjsonFrame>> {
    let mut bytes = Vec::new();
    let mut too_large = false;
    let mut saw_input = false;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_input {
                return Ok(None);
            }
            break;
        }
        saw_input = true;

        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !too_large {
            if bytes.len().saturating_add(consumed) > max_bytes {
                too_large = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&available[..consumed]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }

    if too_large {
        return Ok(Some(NdjsonFrame::TooLarge));
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    Ok(Some(match String::from_utf8(bytes) {
        Ok(line) => NdjsonFrame::Line(line),
        Err(_) => NdjsonFrame::InvalidUtf8,
    }))
}

fn write_response(
    stdout: &mut impl Write,
    response: ServeResponse<serde_json::Value>,
) -> Result<(), Error> {
    let id = response.id.clone();
    let mut encoded = serde_json::to_vec(&response)?;
    if encoded.len().saturating_add(1) > MAX_FRAME_BYTES {
        encoded = serde_json::to_vec(&coded_error_response(
            id,
            response.elapsed_ms,
            "RESPONSE_TOO_LARGE",
            "protocol",
            format!("response frame exceeds {MAX_FRAME_BYTES} bytes"),
        ))?;
    }
    stdout.write_all(&encoded)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

struct ServeState {
    instances: OperatorInstances,
    table: SkillTable,
    training_advice_knowledge: TrainingAdviceKnowledge,
    plan_schema_v1: JSONSchema,
    plan_schema_v2: JSONSchema,
    plan_contract_sha256_v1: String,
    plan_contract_sha256_v2: String,
    solver_executable_sha256: String,
    solver: SolverBuildInfo,
}

#[derive(Clone, Debug, Serialize)]
struct SolverBuildInfo {
    git_commit: Option<String>,
    built_at: Option<String>,
}

impl SolverBuildInfo {
    fn current() -> Self {
        Self {
            git_commit: solver_build_info::GIT_COMMIT.map(str::to_string),
            built_at: solver_build_info::BUILT_AT.map(str::to_string),
        }
    }
}

fn compile_plan_compute_schema(schema_json: &str) -> Result<JSONSchema, Error> {
    let schema: serde_json::Value = serde_json::from_str(schema_json)
        .map_err(|err| Error::msg(format!("invalid embedded plan.compute schema: {err}")))?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|err| Error::msg(format!("invalid embedded plan.compute schema: {err}")))
}

fn plan_contract_sha256(schema_json: &str) -> String {
    format!("{:x}", Sha256::digest(schema_json.as_bytes()))
}

fn solver_executable_sha256() -> Result<String, Error> {
    let path = std::env::current_exe()?;
    let bytes = fs::read(&path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_plan_compute_params(
    schema: &JSONSchema,
    params: &serde_json::Value,
    schema_version: u32,
) -> Result<(), Error> {
    let Err(errors) = schema.validate(params) else {
        return Ok(());
    };
    let details = errors
        .take(3)
        .map(|error| {
            let path = error.instance_path.to_string();
            let path = if path.is_empty() { "/" } else { &path };
            format!("{path}: {error}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(Error::msg(format!(
        "plan.compute params do not match schema v{schema_version}: {details}"
    )))
}

fn spawn_background_bake() -> Option<thread::JoinHandle<()>> {
    let out_dir = default_baked_out_dir().ok()?;
    let generator = current_generator_fingerprint().ok()?;
    if validate_baked_catalog(&out_dir, &generator).is_ok() {
        return None;
    }
    eprintln!(
        "infra-cli serve: baked catalog missing or stale, baking in background -> {}",
        out_dir.display()
    );
    set_runtime_bake_in_progress(true);
    Some(thread::spawn(move || {
        let mut options = BakeOptions::default();
        options.out_dir = out_dir;
        options.generator = Some(generator);
        options.progress = Some(std::sync::Arc::new(print_bake_progress_line));
        if let Err(err) = bake_catalogs(&options) {
            eprintln!("infra-cli serve background bake failed: {err}");
        }
        set_runtime_bake_in_progress(false);
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
    solver: SolverBuildInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ServeError>,
}

#[derive(Debug, Serialize)]
struct ServeError {
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<&'static str>,
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
    rotation: Option<TimedRotationProfile>,
    #[serde(default)]
    profile_out: Option<PathBuf>,
    #[serde(default)]
    maa_out: Option<PathBuf>,
    #[serde(default)]
    output_dir: Option<PathBuf>,
    #[serde(default)]
    maa_title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanComputeParams {
    schema_version: u32,
    layout: BaseBlueprint,
    operbox: Vec<OperBoxEntry>,
    #[serde(default)]
    labels: PlanComputeLabels,
    #[serde(default)]
    options: PlanComputeOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanComputeLabels {
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    operbox: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanComputeOptions {
    #[serde(default)]
    rotation: TimedRotationProfile,
    #[serde(default = "default_top_k")]
    top: usize,
    #[serde(default)]
    system_preferences: HashMap<String, String>,
    #[serde(default)]
    maa_title: Option<String>,
}

fn default_top_k() -> usize {
    20
}

#[derive(Debug, Serialize)]
struct PlanResult {
    layout: String,
    operbox: String,
    owned: usize,
    top: usize,
    rotation: TimedRotationProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maa_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dir: Option<String>,
    daily_trade_efficiency: infra_core::Efficiency,
    daily_manufacture_efficiency: infra_core::Efficiency,
    daily_power_efficiency: infra_core::Efficiency,
    shifts: Vec<infra_core::schedule::TeamShiftResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PlanComputeResult {
    schema_version: u32,
    profile: BoxProfile,
    rotation: RotationSummary,
    maa: MaaSchedule,
    training_advice: PlayerTrainingAdviceReport,
}

#[derive(Debug, Serialize)]
struct RotationSummary {
    profile: TimedRotationProfile,
    daily: DailyTotals,
    shifts: Vec<RotationShiftSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RotationShiftSummary {
    index: usize,
    duration_hours: f64,
    active_teams: Vec<TeamLabel>,
    resting_team: TeamLabel,
    efficiencies: ShiftEfficiencies,
    weighted_trade: infra_core::Efficiency,
    weighted_manufacture: infra_core::Efficiency,
    weighted_power: infra_core::Efficiency,
}

impl From<&TeamRotationReport> for RotationSummary {
    fn from(rotation: &TeamRotationReport) -> Self {
        Self {
            profile: rotation.profile,
            daily: rotation.daily.clone(),
            shifts: rotation
                .shifts
                .iter()
                .map(|shift| RotationShiftSummary {
                    index: shift.index,
                    duration_hours: shift.duration_hours,
                    active_teams: shift.active_teams.clone(),
                    resting_team: shift.resting_team,
                    efficiencies: shift.efficiencies.clone(),
                    weighted_trade: shift.weighted_trade,
                    weighted_manufacture: shift.weighted_manufacture,
                    weighted_power: shift.weighted_power,
                })
                .collect(),
            warnings: rotation.warnings.clone(),
        }
    }
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
    let (result, error_code, error_stage) = match request.method.as_str() {
        "plan" => (handle_plan(state, request.params), None, None),
        "plan.compute" => (
            handle_plan_compute(state, request.params),
            Some("PLAN_FAILED"),
            Some("plan.compute"),
        ),
        "ping" => (
            Ok(serde_json::json!({
                "pong": true,
                "protocol_version": PROTOCOL_VERSION,
                "plan_schema_version": COMPAT_PLAN_SCHEMA_VERSION,
                "supported_plan_schema_versions": [1, 2],
                "plan_contract_sha256": state.plan_contract_sha256_v1,
                "plan_contract_sha256_by_version": {
                    "1": state.plan_contract_sha256_v1,
                    "2": state.plan_contract_sha256_v2,
                },
                "solver_executable_sha256": state.solver_executable_sha256,
                "solver_git_commit": state.solver.git_commit,
                "solver_built_at": state.solver.built_at,
            })),
            None,
            None,
        ),
        other => (
            Err(Error::msg(format!("unknown serve method {other:?}"))),
            None,
            None,
        ),
    };
    match result {
        Ok(result) => ServeResponse {
            id,
            ok: true,
            elapsed_ms: start.elapsed().as_millis(),
            solver: state.solver.clone(),
            result: Some(result),
            error: None,
        },
        Err(err) => match (error_code, error_stage) {
            (Some(code), Some(stage)) => coded_error_response(
                id,
                start.elapsed().as_millis(),
                code,
                stage,
                err.to_string(),
            ),
            _ => error_response(id, start.elapsed().as_millis(), err.to_string()),
        },
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
        solver: SolverBuildInfo::current(),
        result: None,
        error: Some(ServeError {
            code: None,
            stage: None,
            message,
        }),
    }
}

fn coded_error_response(
    id: serde_json::Value,
    elapsed_ms: u128,
    code: &'static str,
    stage: &'static str,
    message: String,
) -> ServeResponse<serde_json::Value> {
    ServeResponse {
        id,
        ok: false,
        elapsed_ms,
        solver: SolverBuildInfo::current(),
        result: None,
        error: Some(ServeError {
            code: Some(code),
            stage: Some(stage),
            message,
        }),
    }
}

fn handle_plan_compute(
    state: &ServeState,
    params: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let schema_version = params
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| Error::msg("plan.compute params require schema_version"))?
        as u32;
    let schema = match schema_version {
        1 => &state.plan_schema_v1,
        2 => &state.plan_schema_v2,
        other => {
            return Err(Error::msg(format!(
                "unsupported plan schema version {other}; expected 1 or {PLAN_SCHEMA_VERSION}"
            )))
        }
    };
    validate_plan_compute_params(schema, &params, schema_version)?;
    let params: PlanComputeParams = serde_json::from_value(params)?;
    if params.options.top == 0 || params.options.top > MAX_TOP_K {
        return Err(Error::msg(format!("top must be between 1 and {MAX_TOP_K}")));
    }
    if params.layout.rooms.is_empty() || params.layout.rooms.len() > MAX_LAYOUT_ROOMS {
        return Err(Error::msg(format!(
            "layout rooms must contain between 1 and {MAX_LAYOUT_ROOMS} entries"
        )));
    }
    params.layout.validate()?;

    let layout_label = validated_label(
        params.labels.layout,
        params.layout.template.as_deref().unwrap_or("inline-layout"),
        "layout label",
    )?;
    let operbox_label = validated_label(params.labels.operbox, "inline-operbox", "operbox label")?;
    let operbox = inline_operbox(params.operbox)?;

    let computed = compute_plan(
        PlanResources {
            instances: &state.instances,
            table: &state.table,
        },
        PlanComputeInput {
            blueprint: &params.layout,
            operbox: &operbox,
            layout_label: &layout_label,
            operbox_label: &operbox_label,
            baseline_operbox: None,
            top_k: params.options.top,
            rotation_profile: params.options.rotation,
            system_preferences: &params.options.system_preferences,
            maa_title: params.options.maa_title.as_deref(),
        },
        RequestedOutputs {
            profile: true,
            maa: true,
        },
    )?;

    let profile = computed
        .profile
        .expect("profile is always requested by plan.compute");
    let maa = computed
        .maa
        .expect("MAA schedule is always requested by plan.compute");
    let training_advice = build_plan_compute_training_advice(
        &operbox,
        &params.layout,
        &state.training_advice_knowledge,
        Some(&computed.current.rotation),
    );
    serde_json::to_value(PlanComputeResult {
        schema_version: params.schema_version,
        profile,
        rotation: RotationSummary::from(&computed.current.rotation),
        maa,
        training_advice,
    })
    .map_err(Error::from)
}

fn build_plan_compute_training_advice(
    operbox: &OperBox,
    blueprint: &BaseBlueprint,
    knowledge: &TrainingAdviceKnowledge,
    rotation: Option<&TeamRotationReport>,
) -> PlayerTrainingAdviceReport {
    let mut context = PlayerAdviceContext::from_blueprint(blueprint);
    if let Some(rotation) = rotation {
        context.trade_average_efficiency_percent = average_station_skill_efficiency_percent(
            rotation
                .shifts
                .iter()
                .flat_map(|shift| shift.efficiencies.room_lines.iter()),
            |line| line.trade_efficiency,
            |line| line.trade_skill_efficiency,
        );
        context.manufacturing_average_efficiency_percent = average_station_skill_efficiency_percent(
            rotation
                .shifts
                .iter()
                .flat_map(|shift| shift.efficiencies.room_lines.iter()),
            |line| line.manufacture_efficiency,
            |line| line.manufacture_skill_efficiency,
        );
    }
    build_player_training_advice(operbox, knowledge, &context)
}

fn average_station_skill_efficiency_percent<'a>(
    lines: impl Iterator<Item = &'a RoomEfficiencyLine>,
    station_efficiency: impl Fn(&RoomEfficiencyLine) -> infra_core::Efficiency,
    skill_efficiency: impl Fn(&RoomEfficiencyLine) -> infra_core::Efficiency,
) -> Option<f64> {
    let values = lines
        .filter(|line| line.operator_count > 0 && !station_efficiency(line).is_zero())
        .map(|line| skill_efficiency(line).as_f64() * 100.0 / line.operator_count as f64)
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn validated_label(value: Option<String>, fallback: &str, field: &str) -> Result<String, Error> {
    let value = value.unwrap_or_else(|| fallback.to_string());
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::msg(format!("{field} must not be empty")));
    }
    if value.len() > MAX_LABEL_BYTES {
        return Err(Error::msg(format!(
            "{field} must not exceed {MAX_LABEL_BYTES} bytes"
        )));
    }
    Ok(value.to_string())
}

fn inline_operbox(mut entries: Vec<OperBoxEntry>) -> Result<OperBox, Error> {
    if entries.is_empty() || entries.len() > MAX_OPERBOX_ENTRIES {
        return Err(Error::msg(format!(
            "operbox must contain between 1 and {MAX_OPERBOX_ENTRIES} entries"
        )));
    }

    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.id = entry.id.trim().to_string();
        entry.name = entry.name.trim().to_string();
        if entry.id.is_empty() || entry.name.is_empty() {
            return Err(Error::msg(format!(
                "operbox entry {} must have non-empty id and name",
                index + 1
            )));
        }
        if !ids.insert(entry.id.clone()) {
            return Err(Error::msg(format!("duplicate operbox id {}", entry.id)));
        }
        if !names.insert(entry.name.clone()) {
            return Err(Error::msg(format!("duplicate operbox name {}", entry.name)));
        }
        if !(1..=6).contains(&entry.potential) {
            return Err(Error::msg(format!(
                "{} potential must be between 1 and 6",
                entry.name
            )));
        }
    }
    OperBox::from_entries(entries)
}

fn handle_plan(state: &ServeState, params: serde_json::Value) -> Result<serde_json::Value, Error> {
    let params: PlanParams = serde_json::from_value(params)?;
    let top = params.top.unwrap_or(20);
    let rotation_profile = params.rotation.unwrap_or_default();
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
    let system_preferences = std::collections::HashMap::new();
    let computed = compute_plan(
        PlanResources {
            instances: &state.instances,
            table: &state.table,
        },
        PlanComputeInput {
            blueprint: &blueprint,
            operbox: &operbox,
            layout_label: &layout_label,
            operbox_label: &operbox_label,
            baseline_operbox: Some(&baseline_path),
            top_k: top,
            rotation_profile,
            system_preferences: &system_preferences,
            maa_title: params.maa_title.as_deref(),
        },
        RequestedOutputs {
            profile: params.profile_out.is_some(),
            maa: params.maa_out.is_some(),
        },
    )?;
    let rotation = &computed.current.rotation;

    if let Some(dir) = params.output_dir.as_ref() {
        fs::create_dir_all(dir)?;
        for shift in &rotation.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
        }
    }

    if let Some(path) = params.maa_out.as_ref() {
        let schedule = computed
            .maa
            .as_ref()
            .expect("MAA schedule was requested when maa_out is present");
        schedule.save(path)?;
    }

    if let Some(path) = params.profile_out.as_ref() {
        let profile = computed
            .profile
            .as_ref()
            .expect("profile was requested when profile_out is present");
        write_pretty_json(path, &profile)?;
    }

    let result = PlanResult {
        layout: layout_label,
        operbox: operbox_label,
        owned: operbox.owned_count(),
        top,
        rotation: rotation_profile,
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
        daily_trade_efficiency: rotation.daily.trade,
        daily_manufacture_efficiency: rotation.daily.manufacture,
        daily_power_efficiency: rotation.daily.power,
        shifts: rotation.shifts.clone(),
        warnings: rotation.warnings.clone(),
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn operbox_entry(id: &str, name: &str) -> OperBoxEntry {
        OperBoxEntry {
            id: id.to_string(),
            name: name.to_string(),
            elite: 2,
            level: 90,
            own: true,
            potential: 6,
            rarity: 6,
        }
    }

    fn valid_compute_params() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "layout": {
                "template": "contract-test",
                "drone_cap": 135,
                "scenario": {},
                "rooms": [
                    { "id": "control", "kind": "control_center", "level": 3 }
                ]
            },
            "operbox": [
                {
                    "id": "char_009_12fce",
                    "name": "12F",
                    "elite": 0,
                    "level": 30,
                    "own": true,
                    "potential": 6,
                    "rarity": 2
                }
            ],
            "labels": {
                "layout": "contract-test",
                "operbox": "contract-test"
            },
            "options": {
                "rotation": "abc_12_6_6",
                "top": 20,
                "system_preferences": {},
                "maa_title": "Contract test"
            }
        })
    }

    #[test]
    fn bounded_reader_drains_oversized_frame_and_reads_next_line() {
        let mut input = Cursor::new(b"12345\n{}\n".to_vec());
        assert!(matches!(
            read_ndjson_frame(&mut input, 5).unwrap(),
            Some(NdjsonFrame::TooLarge)
        ));
        match read_ndjson_frame(&mut input, 5).unwrap() {
            Some(NdjsonFrame::Line(line)) => assert_eq!(line, "{}"),
            _ => panic!("expected the frame after the oversized line"),
        }
    }

    #[test]
    fn inline_operbox_trims_and_rejects_duplicate_names() {
        let operbox = inline_operbox(vec![operbox_entry(" a ", " 甲 ")]).unwrap();
        assert!(operbox.owns("甲"));

        let error =
            inline_operbox(vec![operbox_entry("a", "甲"), operbox_entry("b", "甲")]).unwrap_err();
        assert!(error.to_string().contains("duplicate operbox name"));
    }

    #[test]
    fn inline_operbox_rejects_impossible_progress() {
        let mut entry = operbox_entry("test", "弑君者");
        entry.rarity = 1;
        let error = inline_operbox(vec![entry]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "operbox entry 弑君者 has impossible progress: rarity 1 cannot reach elite 2"
        );
    }

    #[test]
    fn compute_params_reject_unknown_fields() {
        let value = serde_json::json!({
            "schema_version": 1,
            "layout": { "rooms": [] },
            "operbox": [],
            "unexpected": true,
        });
        assert!(serde_json::from_value::<PlanComputeParams>(value).is_err());
    }

    #[test]
    fn solver_executable_fingerprint_is_a_sha256_hex() {
        let fingerprint = solver_executable_sha256().unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn solver_build_info_uses_a_full_commit_and_utc_timestamp() {
        let info = SolverBuildInfo::current();
        let commit = info.git_commit.expect("test build has a Git commit");
        let built_at = info.built_at.expect("test build has a timestamp");
        assert_eq!(commit.len(), 40);
        assert!(commit.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(built_at.ends_with('Z'));
        assert_eq!(built_at.len(), 20);
    }

    #[test]
    fn plan_compute_schema_accepts_the_v1_contract() {
        let schema = compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V1_JSON).unwrap();
        let value = valid_compute_params();
        validate_plan_compute_params(&schema, &value, 1).unwrap();
        serde_json::from_value::<PlanComputeParams>(value).unwrap();
        assert_eq!(plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V2_JSON).len(), 64);
    }

    #[test]
    fn plan_compute_schema_v2_accepts_abc_12_12_12() {
        let schema = compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V2_JSON).unwrap();
        let mut value = valid_compute_params();
        value["schema_version"] = serde_json::json!(2);
        value["options"]["rotation"] = serde_json::json!("abc_12_12_12");
        validate_plan_compute_params(&schema, &value, 2).unwrap();
        serde_json::from_value::<PlanComputeParams>(value).unwrap();
    }

    #[test]
    fn plan_compute_schema_rejects_structural_mutations() {
        let schema = compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V1_JSON).unwrap();

        let mut unknown_room_field = valid_compute_params();
        unknown_room_field["layout"]["rooms"][0]["unexpected"] = serde_json::json!(true);

        let mut invalid_rotation = valid_compute_params();
        invalid_rotation["options"]["rotation"] = serde_json::json!("4");

        let mut invalid_product_shape = valid_compute_params();
        invalid_product_shape["layout"]["rooms"][0]["product"] =
            serde_json::json!({ "trade": { "recipe": "gold" } });

        let mut invalid_level = valid_compute_params();
        invalid_level["operbox"][0]["level"] = serde_json::json!(91);

        let mut invalid_top = valid_compute_params();
        invalid_top["options"]["top"] = serde_json::json!(0);

        for value in [
            unknown_room_field,
            invalid_rotation,
            invalid_product_shape,
            invalid_level,
            invalid_top,
        ] {
            assert!(validate_plan_compute_params(&schema, &value, 1).is_err());
        }
    }

    #[test]
    fn semantic_validation_remains_after_schema_admission() {
        let schema = compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V1_JSON).unwrap();
        let mut duplicate_rooms = valid_compute_params();
        duplicate_rooms["layout"]["rooms"] = serde_json::json!([
            { "id": "control", "kind": "control_center", "level": 3 },
            { "id": "control", "kind": "power_plant", "level": 3 }
        ]);
        validate_plan_compute_params(&schema, &duplicate_rooms, 1).unwrap();
        let params: PlanComputeParams = serde_json::from_value(duplicate_rooms).unwrap();
        assert!(params.layout.validate().is_err());

        let mut mismatched_product = valid_compute_params();
        mismatched_product["layout"]["rooms"][0] = serde_json::json!({
            "id": "trade_1",
            "kind": "trade_post",
            "level": 3,
            "product": { "factory": { "recipe": "gold" } }
        });
        validate_plan_compute_params(&schema, &mismatched_product, 1).unwrap();
        let params: PlanComputeParams = serde_json::from_value(mismatched_product).unwrap();
        assert!(params.layout.validate().is_err());
    }

    #[test]
    fn plan_compute_training_advice_consumes_the_inline_layout_gate() {
        let mut knowledge =
            load_training_advice_knowledge(&default_training_advice_knowledge_path().unwrap())
                .unwrap();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| entry.id == "bubble_group");
        knowledge.standalone.retain(|entry| {
            entry.product == infra_core::training_advice::ProductBucket::OriginiumShards
        });
        let operbox = inline_operbox(vec![operbox_entry("test", "12F")]).unwrap();
        let without_originium: BaseBlueprint = serde_json::from_value(serde_json::json!({
            "rooms": [
                { "id": "factory", "kind": "factory", "level": 3,
                  "product": { "factory": { "recipe": "gold" } } }
            ]
        }))
        .unwrap();
        let with_originium: BaseBlueprint = serde_json::from_value(serde_json::json!({
            "rooms": [
                { "id": "factory", "kind": "factory", "level": 3,
                  "product": { "factory": { "recipe": "originium" } } }
            ]
        }))
        .unwrap();

        let gated =
            build_plan_compute_training_advice(&operbox, &without_originium, &knowledge, None);
        assert_eq!(gated.context.has_originium_shard_factory, Some(false));
        assert!(gated.recommendations.iter().all(|item| {
            !matches!(item.operator.as_str(), "泡泡" | "火神")
                && !matches!(
                    item.operator.as_str(),
                    "褐果" | "炎熔" | "地灵" | "谬因" | "艾雅法拉" | "薄绿"
                )
        }));

        let enabled =
            build_plan_compute_training_advice(&operbox, &with_originium, &knowledge, None);
        assert_eq!(enabled.context.has_originium_shard_factory, Some(true));
        let bubble_names = enabled
            .recommendations
            .iter()
            .filter(|item| item.combination_id.as_deref() == Some("bubble_group"))
            .map(|item| item.operator.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bubble_names, vec!["泡泡", "火神"]);
        assert!(enabled.recommendations.iter().any(|item| {
            matches!(
                item.operator.as_str(),
                "褐果" | "炎熔" | "地灵" | "谬因" | "艾雅法拉" | "薄绿"
            )
        }));
    }

    #[test]
    fn t9_average_uses_skill_efficiency_per_worker_without_duration_weighting() {
        let lines = vec![
            RoomEfficiencyLine {
                room_id: "trade_1".to_string(),
                operator_count: 2,
                trade_efficiency: infra_core::Efficiency::from_decimal(1.8),
                trade_skill_efficiency: infra_core::Efficiency::from_decimal(0.8),
                ..RoomEfficiencyLine::default()
            },
            RoomEfficiencyLine {
                room_id: "trade_2".to_string(),
                operator_count: 3,
                trade_efficiency: infra_core::Efficiency::from_decimal(1.6),
                trade_skill_efficiency: infra_core::Efficiency::from_decimal(0.6),
                ..RoomEfficiencyLine::default()
            },
        ];

        let average = average_station_skill_efficiency_percent(
            lines.iter(),
            |line| line.trade_efficiency,
            |line| line.trade_skill_efficiency,
        )
        .unwrap();
        assert_eq!(average, 30.0);
    }
}
