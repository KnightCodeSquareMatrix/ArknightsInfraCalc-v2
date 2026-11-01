use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::plan_compute::{
    compute_plan, compute_plan_for_plan_compute, inline_operbox, PlanComputeAttempt,
    PlanComputeInput, PlanComputeParams, PlanResources, RequestedOutputs,
};
use infra_core::box_profile::{baseline_path_or_default, BoxProfile};
use infra_core::export::MaaSchedule;
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{default_layout_243_path, default_operbox_full_e2_path, OperBox};
use infra_core::schedule::{
    DailyTotals, RoomEfficiencyLine, ShiftEfficiencies, TeamLabel, TeamRotationReport,
    TimedRotationProfile, TimedRotationRequest,
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

const PLAN_SCHEMA_VERSION: u32 = 7;
const COMPAT_PLAN_SCHEMA_VERSION: u32 = 1;
const PROTOCOL_VERSION: u32 = 1;
const PLAN_COMPUTE_SCHEMA_V1_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v1.schema.json");
const PLAN_COMPUTE_SCHEMA_V2_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v2.schema.json");
const PLAN_COMPUTE_SCHEMA_V3_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v3.schema.json");
const PLAN_COMPUTE_SCHEMA_V4_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v4.schema.json");
const PLAN_COMPUTE_SCHEMA_V5_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v5.schema.json");
const PLAN_COMPUTE_SCHEMA_V6_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v6.schema.json");
const PLAN_COMPUTE_SCHEMA_V7_JSON: &str =
    include_str!("../../../../contracts/plan-compute-v7.schema.json");
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
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
        plan_schema_v3: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V3_JSON)?,
        plan_schema_v4: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V4_JSON)?,
        plan_schema_v5: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V5_JSON)?,
        plan_schema_v6: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V6_JSON)?,
        plan_schema_v7: compile_plan_compute_schema(PLAN_COMPUTE_SCHEMA_V7_JSON)?,
        plan_contract_sha256_v1: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V1_JSON),
        plan_contract_sha256_v2: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V2_JSON),
        plan_contract_sha256_v3: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V3_JSON),
        plan_contract_sha256_v4: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V4_JSON),
        plan_contract_sha256_v5: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V5_JSON),
        plan_contract_sha256_v6: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V6_JSON),
        plan_contract_sha256_v7: plan_contract_sha256(PLAN_COMPUTE_SCHEMA_V7_JSON),
        solver_executable_sha256: solver_executable_sha256().unwrap_or_else(|err| {
            eprintln!("infra-cli serve: could not fingerprint executable: {err}");
            String::new()
        }),
        solver: SolverBuildInfo::current(),
    };

    {
        let _trace = infra_core::profile::start_solver_perf_trace("serve_startup", "bake_warm");
        match infra_core::bake::warm_runtime_baked_table() {
            Ok(true) => eprintln!("infra-cli serve: baked combo table warmed"),
            Ok(false) => {}
            Err(err) => eprintln!("infra-cli serve: baked combo table warm failed: {err}"),
        }
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
    eprintln!(
        "  plan.compute v7: options.rotation accepts a legacy profile or topology + shift_hours"
    );
    eprintln!("  plan: support_combinations_enable boolean, default false");
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
    plan_schema_v3: JSONSchema,
    plan_schema_v4: JSONSchema,
    plan_schema_v5: JSONSchema,
    plan_schema_v6: JSONSchema,
    plan_schema_v7: JSONSchema,
    plan_contract_sha256_v1: String,
    plan_contract_sha256_v2: String,
    plan_contract_sha256_v3: String,
    plan_contract_sha256_v4: String,
    plan_contract_sha256_v5: String,
    plan_contract_sha256_v6: String,
    plan_contract_sha256_v7: String,
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

pub(super) fn validate_recorded_plan_compute_params(
    params: &serde_json::Value,
) -> Result<(), Error> {
    let schema_version = params
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| Error::msg("plan.compute params require schema_version"))?
        as u32;
    let schema_json = match schema_version {
        1 => PLAN_COMPUTE_SCHEMA_V1_JSON,
        2 => PLAN_COMPUTE_SCHEMA_V2_JSON,
        3 => PLAN_COMPUTE_SCHEMA_V3_JSON,
        4 => PLAN_COMPUTE_SCHEMA_V4_JSON,
        5 => PLAN_COMPUTE_SCHEMA_V5_JSON,
        6 => PLAN_COMPUTE_SCHEMA_V6_JSON,
        7 => PLAN_COMPUTE_SCHEMA_V7_JSON,
        other => {
            return Err(Error::msg(format!(
                "unsupported plan schema version {other}; expected 1 through {PLAN_SCHEMA_VERSION}"
            )))
        }
    };
    let schema = compile_plan_compute_schema(schema_json)?;
    validate_plan_compute_params(&schema, params, schema_version)
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
    #[serde(default)]
    support_combinations_enable: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<BoxProfile>,
    rotation: Option<RotationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maa: Option<MaaSchedule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    training_advice: Option<PlayerTrainingAdviceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<PlanComputeResultError>,
}

#[derive(Debug, Serialize)]
struct PlanComputeResultError {
    code: &'static str,
    stage: &'static str,
    message: String,
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
    assignment: infra_core::layout::BaseAssignment,
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
                    assignment: shift.assignment.clone(),
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
    let _trace = infra_core::profile::start_solver_perf_trace("request", "unparsed");
    let request: ServeRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(err) => {
            infra_core::profile::emit_solver_stage(
                "serve_request",
                "request_parse",
                "error",
                start.elapsed(),
                infra_core::profile::solver_perf_snapshot(),
                infra_core::profile::solver_perf_snapshot(),
            );
            return error_response(
                serde_json::Value::Null,
                start.elapsed().as_millis(),
                format!("invalid request json: {err}"),
            );
        }
    };
    let id = request.id.clone();
    let request_label = id.to_string();
    infra_core::profile::update_solver_perf_trace_labels(
        "request",
        &format!("{}:{request_label}", request.method),
    );
    let plan_compute_request = request.method == "plan.compute";
    let plan_compute_asserts = request
        .params
        .get("options")
        .and_then(|options| options.get("assert_invariants"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let plan_compute_schema_version = request
        .params
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .map(|version| version as u32);
    let request_before = infra_core::profile::solver_perf_snapshot();
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
                "supported_plan_schema_versions": [1, 2, 3, 4, 5, 6, 7],
                "auto_rotation": {
                    "entry": "plan --rotation auto@H1/H2",
                    "fiammetta_policy": "static_priority",
                    "continuous_targets": true,
                    "exchange_cycle": true,
                },
                "plan_contract_sha256": state.plan_contract_sha256_v1,
                "plan_contract_sha256_by_version": {
                    "1": state.plan_contract_sha256_v1,
                    "2": state.plan_contract_sha256_v2,
                    "3": state.plan_contract_sha256_v3,
                    "4": state.plan_contract_sha256_v4,
                    "5": state.plan_contract_sha256_v5,
                    "6": state.plan_contract_sha256_v6,
                    "7": state.plan_contract_sha256_v7,
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
    let request_status = match &result {
        Ok(value)
            if request.method == "plan.compute"
                && value.get("error").is_some_and(|error| !error.is_null()) =>
        {
            "error"
        }
        Ok(_) => "ok",
        Err(_) => "error",
    };
    infra_core::profile::emit_solver_stage(
        "serve_request",
        "request_total",
        request_status,
        start.elapsed(),
        request_before,
        infra_core::profile::solver_perf_snapshot(),
    );
    match result {
        Ok(result) => {
            let nested_error_message = (request.method == "plan.compute")
                .then(|| result.get("error"))
                .flatten()
                .filter(|error| !error.is_null())
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            if let Some(message) = nested_error_message {
                ServeResponse {
                    id,
                    ok: false,
                    elapsed_ms: start.elapsed().as_millis(),
                    solver: state.solver.clone(),
                    result: Some(result),
                    error: Some(ServeError {
                        code: Some("PLAN_FAILED"),
                        stage: Some("plan.compute"),
                        message,
                    }),
                }
            } else {
                ServeResponse {
                    id,
                    ok: true,
                    elapsed_ms: start.elapsed().as_millis(),
                    solver: state.solver.clone(),
                    result: Some(result),
                    error: None,
                }
            }
        }
        Err(err) => match (error_code, error_stage) {
            (Some(code), Some(stage)) if plan_compute_request && !plan_compute_asserts => {
                let message = err.to_string();
                coded_error_response_with_result(
                    id,
                    start.elapsed().as_millis(),
                    code,
                    stage,
                    message.clone(),
                    Some(plan_compute_error_result(
                        plan_compute_schema_version,
                        message,
                    )),
                )
            }
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
    coded_error_response_with_result(id, elapsed_ms, code, stage, message, None)
}

fn coded_error_response_with_result(
    id: serde_json::Value,
    elapsed_ms: u128,
    code: &'static str,
    stage: &'static str,
    message: String,
    result: Option<serde_json::Value>,
) -> ServeResponse<serde_json::Value> {
    ServeResponse {
        id,
        ok: false,
        elapsed_ms,
        solver: SolverBuildInfo::current(),
        result,
        error: Some(ServeError {
            code: Some(code),
            stage: Some(stage),
            message,
        }),
    }
}

fn plan_compute_error_result(schema_version: Option<u32>, message: String) -> serde_json::Value {
    let mut result = serde_json::json!({
        "error": {
            "code": "PLAN_FAILED",
            "stage": "plan.compute",
            "message": message,
        },
        "rotation": null,
    });
    if let Some(schema_version) = schema_version {
        result["schema_version"] = serde_json::json!(schema_version);
    }
    result
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
        3 => &state.plan_schema_v3,
        4 => &state.plan_schema_v4,
        5 => &state.plan_schema_v5,
        6 => &state.plan_schema_v6,
        7 => &state.plan_schema_v7,
        other => {
            return Err(Error::msg(format!(
                "unsupported plan schema version {other}; expected 1 through {PLAN_SCHEMA_VERSION}"
            )))
        }
    };
    reject_removed_rotation(
        params
            .get("options")
            .and_then(|options| options.get("rotation")),
    )?;
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

    let attempt = compute_plan_for_plan_compute(
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
            rotation_request: params.options.rotation,
            system_preferences: &params.options.system_preferences,
            maa_title: params.options.maa_title.as_deref(),
            fiammetta_candidates_out: None,
            fiammetta_enable: params.options.fiammetta_enable,
            support_combinations_enable: params.options.support_combinations_enable,
            assert_invariants: params.options.assert_invariants,
        },
        RequestedOutputs {
            profile: true,
            maa: true,
        },
    );
    let result = match attempt {
        PlanComputeAttempt::Complete(computed) => {
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
            PlanComputeResult {
                schema_version: params.schema_version,
                profile: Some(profile),
                rotation: Some(RotationSummary::from(&computed.current.rotation)),
                maa: Some(maa),
                training_advice: Some(training_advice),
                error: None,
            }
        }
        PlanComputeAttempt::Failed(failure) if !params.options.assert_invariants => {
            let rotation_report = failure
                .current
                .as_ref()
                .map(|current| &current.rotation)
                .or(failure.rotation.as_ref());
            let rotation = rotation_report.map(RotationSummary::from);
            let training_advice = failure.current.as_ref().map(|current| {
                build_plan_compute_training_advice(
                    &operbox,
                    &params.layout,
                    &state.training_advice_knowledge,
                    Some(&current.rotation),
                )
            });
            let message = failure.error.to_string();
            let result = PlanComputeResult {
                schema_version: params.schema_version,
                profile: failure.profile,
                rotation,
                maa: failure.maa,
                training_advice,
                error: Some(PlanComputeResultError {
                    code: "PLAN_FAILED",
                    stage: "plan.compute",
                    message,
                }),
            };
            return serde_json::to_value(result).map_err(Error::from);
        }
        PlanComputeAttempt::Failed(failure) => return Err(failure.error),
    };
    serde_json::to_value(result).map_err(Error::from)
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

fn handle_plan(state: &ServeState, params: serde_json::Value) -> Result<serde_json::Value, Error> {
    reject_removed_rotation(params.get("rotation"))?;
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
            rotation_request: TimedRotationRequest::Named(rotation_profile),
            system_preferences: &system_preferences,
            maa_title: params.maa_title.as_deref(),
            fiammetta_candidates_out: None,
            fiammetta_enable: true,
            support_combinations_enable: params.support_combinations_enable,
            assert_invariants: false,
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

fn reject_removed_rotation(rotation: Option<&serde_json::Value>) -> Result<(), Error> {
    if rotation.and_then(serde_json::Value::as_str) == Some("fiammetta_witch_12_12") {
        return Err(Error::msg(
            "unsupported rotation profile fiammetta_witch_12_12: this product mode was removed; no fallback is performed",
        ));
    }
    Ok(())
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
