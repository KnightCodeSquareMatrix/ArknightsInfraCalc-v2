use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{OperBox, OperBoxEntry};
use infra_core::schedule::{TeamRotationReport, TimedRotationProfile};
use infra_core::skill_table::{data_path, default_skill_table_path, SkillTable};
use infra_core::Error;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::plan_compute::{
    compute_plan, inline_operbox_for_debug_rerun, PlanComputeInput, PlanComputeParams,
    PlanResources, RequestedOutputs,
};

use crate::output::emit_debug_json;

const RERUN_SCHEMA_VERSION: u32 = 1;
const FINGERPRINT_SCOPE: &str = "current_runtime_catalogs_v1";
const RUNTIME_CATALOGS: &[&str] = &[
    "skill_table.json",
    "operator_instances.json",
    "orchestration_rules.json",
    "base_systems.json",
    "trade_segments.json",
    "trade_shortcuts.json",
    "producer_rules.json",
    "standalone_roster.json",
    "support_skill_registry.json",
    "baked/manifest.json",
];

static NEXT_STAGE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
struct DebugBundleEnvelope {
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "startedAt", default)]
    started_at: Option<String>,
    #[serde(default)]
    layout: Option<Value>,
    #[serde(default)]
    operbox: Option<Value>,
    #[serde(rename = "inputSummary", default)]
    input_summary: Option<InputSummary>,
    #[serde(rename = "serveRequest", default)]
    serve_request: Option<RecordedRequest>,
    #[serde(rename = "serveResponse", default)]
    serve_response: Option<RecordedResponse>,
}

#[derive(Debug, Deserialize)]
struct InputSummary {
    #[serde(rename = "sourceName", default)]
    source_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordedRequest {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RecordedResponse {
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct OptionWithOrigin<T: Serialize> {
    value: T,
    origin: &'static str,
}

#[derive(Debug, Serialize)]
struct RerunInputSummary {
    layout_rooms: usize,
    operbox_entries: usize,
    owned_operators: usize,
    legacy_progress_normalized_entries: usize,
    layout_sha256: String,
    operbox_sha256: String,
    top_k: OptionWithOrigin<usize>,
    rotation: OptionWithOrigin<TimedRotationProfile>,
}

#[derive(Debug, Serialize)]
struct SourceSummary {
    bundle_version: Option<String>,
    started_at: Option<String>,
    source_name: Option<String>,
    request_method: String,
    original_runtime_fingerprint: &'static str,
}

#[derive(Debug, Serialize)]
struct CurrentRuntimeFingerprint {
    scope: &'static str,
    cli_version: &'static str,
    executable: FingerprintEntry,
    catalogs: Vec<FingerprintEntry>,
}

#[derive(Debug, Serialize)]
struct FingerprintEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct BehaviorProjection {
    rotation: String,
    daily_trade: f64,
    daily_manufacture: f64,
    daily_power: f64,
    shifts: Vec<ShiftProjection>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct ShiftProjection {
    index: usize,
    duration_hours: f64,
    transition: String,
    rooms: Vec<RoomProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RoomProjection {
    room_id: String,
    operators: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProjectionComparison {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    local_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<BehaviorProjection>,
}

#[derive(Debug, Serialize)]
struct DebugRerunReport {
    schema_version: u32,
    mode: &'static str,
    bundle_sha256: String,
    source: SourceSummary,
    current_runtime: CurrentRuntimeFingerprint,
    inputs: RerunInputSummary,
    projection: BehaviorProjection,
    comparison: ProjectionComparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_directory: Option<String>,
    rotation: TeamRotationReport,
}

#[derive(Debug, Serialize)]
struct DebugRerunSummary<'a> {
    schema_version: u32,
    mode: &'static str,
    bundle_sha256: &'a str,
    source: PublicSourceSummary<'a>,
    current_runtime: &'a CurrentRuntimeFingerprint,
    inputs: &'a RerunInputSummary,
    comparison: PublicComparison<'a>,
    private_report_written: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_directory: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct PublicSourceSummary<'a> {
    bundle_version: Option<&'a str>,
    request_method: &'a str,
    original_runtime_fingerprint: &'static str,
}

#[derive(Debug, Serialize)]
struct PublicComparison<'a> {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    local_sha256: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_sha256: Option<&'a str>,
}

pub fn debug_cmd(args: &[String]) -> Result<(), Error> {
    match args.first().map(String::as_str) {
        Some("rerun") => rerun_cmd(&args[1..]),
        _ => Err(Error::msg(
            "usage: infra-cli debug rerun --bundle <debug-bundle.json> [--top <n>] [--rotation <profile>] [--room <room_id> | --operator <name>] [--shift <1-based>] [--output-dir <new-dir>] [--pretty]",
        )),
    }
}

fn rerun_cmd(args: &[String]) -> Result<(), Error> {
    let bundle_path = required_path(args, "--bundle")?;
    let output_dir = optional_value(args, "--output-dir").map(PathBuf::from);
    if (optional_value(args, "--room").is_some() || optional_value(args, "--operator").is_some())
        && output_dir.is_none()
    {
        return Err(Error::msg(
            "--room/--operator contain private assignment data and require --output-dir",
        ));
    }
    let raw_bundle = fs::read(&bundle_path).map_err(|error| {
        Error::msg(format!(
            "debug bundle read {}: {error}",
            bundle_path.display()
        ))
    })?;
    let bundle: DebugBundleEnvelope = serde_json::from_slice(&raw_bundle).map_err(|error| {
        Error::msg(format!(
            "debug bundle parse {}: {error}",
            bundle_path.display()
        ))
    })?;
    validate_supported_bundle(&bundle)?;

    let bundle_dir = bundle_path.parent().unwrap_or_else(|| Path::new("."));
    let request = bundle
        .serve_request
        .as_ref()
        .ok_or_else(|| Error::msg("debug bundle missing serveRequest"))?;
    let layout_value = if request.method == "plan.compute" {
        select_compute_snapshot(
            bundle.layout.as_ref(),
            request.params.get("layout"),
            &bundle_dir.join("layout.json"),
            "layout",
        )?
    } else {
        select_snapshot(
            bundle.layout.as_ref(),
            &bundle_dir.join("layout.json"),
            "layout",
        )?
    };
    let operbox_value = if request.method == "plan.compute" {
        select_compute_snapshot(
            bundle.operbox.as_ref(),
            request.params.get("operbox"),
            &bundle_dir.join("operbox.json"),
            "operbox",
        )?
    } else {
        select_snapshot(
            bundle.operbox.as_ref(),
            &bundle_dir.join("operbox.json"),
            "operbox",
        )?
    };
    let (
        blueprint,
        operbox,
        legacy_progress_normalized_entries,
        system_preferences,
        fiammetta_enable,
        support_combinations_enable,
        labels,
    ) = if request.method == "plan.compute" {
        let params: PlanComputeParams = serde_json::from_value(request.params.clone())
            .map_err(|error| Error::msg(format!("plan.compute params parse: {error}")))?;
        if !(1..=6).contains(&params.schema_version) {
            return Err(Error::msg(format!(
                "unsupported plan schema version {}; expected 1 through 6",
                params.schema_version
            )));
        }
        let labels = (
            params
                .labels
                .layout
                .unwrap_or_else(|| "inline-layout".to_string()),
            params
                .labels
                .operbox
                .unwrap_or_else(|| "inline-operbox".to_string()),
        );
        let options = params.options;
        let mut entries = params.operbox;
        let legacy_progress_normalized_entries =
            normalize_plan_compute_operbox_compat(&mut entries);
        (
            params.layout,
            inline_operbox_for_debug_rerun(entries)?,
            legacy_progress_normalized_entries,
            options.system_preferences,
            options.fiammetta_enable,
            options.support_combinations_enable,
            labels,
        )
    } else {
        let blueprint: BaseBlueprint = serde_json::from_value(layout_value.clone())
            .map_err(|error| Error::msg(format!("debug bundle layout parse: {error}")))?;
        let mut entries: Vec<OperBoxEntry> = serde_json::from_value(operbox_value.clone())
            .map_err(|error| Error::msg(format!("debug bundle operbox parse: {error}")))?;
        let legacy_progress_normalized_entries = normalize_legacy_operbox_progress(&mut entries);
        (
            blueprint,
            OperBox::from_entries(entries)?,
            legacy_progress_normalized_entries,
            std::collections::HashMap::new(),
            true,
            request
                .params
                .get("support_combinations_enable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            ("legacy-layout".to_string(), "legacy-operbox".to_string()),
        )
    };

    let top_k = rerun_top_k(args, &bundle)?;
    let rotation = rerun_rotation(args, &bundle)?;
    if rotation.value == TimedRotationProfile::FiammettaWitch12_12 {
        return Err(Error::msg(
            "unsupported rotation profile fiammetta_witch_12_12: this product mode was removed; no fallback is performed",
        ));
    }
    let current_runtime = current_runtime_fingerprint();
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    let computed = compute_plan(
        PlanResources {
            instances: &instances,
            table: &table,
        },
        PlanComputeInput {
            blueprint: &blueprint,
            operbox: &operbox,
            layout_label: &labels.0,
            operbox_label: &labels.1,
            baseline_operbox: None,
            top_k: top_k.value,
            rotation_profile: rotation.value,
            system_preferences: &system_preferences,
            maa_title: None,
            fiammetta_candidates_out: None,
            fiammetta_enable,
            support_combinations_enable,
            assert_invariants: false,
        },
        RequestedOutputs {
            profile: false,
            maa: false,
        },
    )?;
    let report = computed.current.rotation;

    let projection = projection_from_report(&report)?;
    let expected_projection = bundle
        .serve_response
        .as_ref()
        .and_then(|response| response.result.as_ref())
        .map(projection_from_recorded_result);
    let comparison = compare_projection(
        &projection,
        expected_projection,
        top_k.origin == "explicit_override" || rotation.origin == "explicit_override",
    );
    let focus = focused_projection(args, &report)?;
    let artifact_directory = output_dir
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let inputs = RerunInputSummary {
        layout_rooms: blueprint.rooms.len(),
        operbox_entries: operbox.entries.len(),
        owned_operators: operbox.owned_count(),
        legacy_progress_normalized_entries,
        layout_sha256: canonical_sha256(&layout_value)?,
        operbox_sha256: canonical_sha256(&operbox_value)?,
        top_k,
        rotation,
    };
    let rerun = DebugRerunReport {
        schema_version: RERUN_SCHEMA_VERSION,
        mode: "current_runtime_rerun",
        bundle_sha256: sha256_bytes(&raw_bundle),
        source: SourceSummary {
            bundle_version: bundle.version,
            started_at: bundle.started_at,
            source_name: bundle.input_summary.and_then(|summary| summary.source_name),
            request_method: bundle
                .serve_request
                .as_ref()
                .map(|request| request.method.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            original_runtime_fingerprint: "unknown",
        },
        current_runtime,
        inputs,
        projection,
        comparison,
        focus,
        artifact_directory,
        rotation: report,
    };

    if let Some(output_dir) = output_dir {
        write_rerun_artifacts(&output_dir, &layout_value, &operbox_value, &rerun)?;
    }
    let summary = DebugRerunSummary {
        schema_version: rerun.schema_version,
        mode: rerun.mode,
        bundle_sha256: &rerun.bundle_sha256,
        source: PublicSourceSummary {
            bundle_version: rerun.source.bundle_version.as_deref(),
            request_method: &rerun.source.request_method,
            original_runtime_fingerprint: rerun.source.original_runtime_fingerprint,
        },
        current_runtime: &rerun.current_runtime,
        inputs: &rerun.inputs,
        comparison: PublicComparison {
            status: rerun.comparison.status,
            reason: rerun.comparison.reason.as_deref(),
            local_sha256: &rerun.comparison.local_sha256,
            expected_sha256: rerun.comparison.expected_sha256.as_deref(),
        },
        private_report_written: rerun.artifact_directory.is_some(),
        artifact_directory: rerun.artifact_directory.as_deref(),
    };
    emit_debug_json(&summary, args.iter().any(|arg| arg == "--pretty"))
}

fn normalize_legacy_operbox_progress(entries: &mut [OperBoxEntry]) -> usize {
    let mut normalized = 0;
    for entry in entries {
        let max_elite = match entry.rarity {
            1 | 2 => 0,
            3 => 1,
            4..=6 => 2,
            _ => continue,
        };
        let old = (entry.elite, entry.level);
        entry.elite = entry.elite.min(max_elite);
        if !entry.own {
            normalized += usize::from(old != (entry.elite, entry.level));
            continue;
        }
        let max_level = match (entry.rarity, entry.elite) {
            (1 | 2, 0) => 30,
            (3, 0) => 40,
            (3, 1) => 55,
            (4, 0) => 45,
            (4, 1) => 60,
            (4, 2) => 70,
            (5, 0) => 50,
            (5, 1) => 70,
            (5, 2) => 80,
            (6, 0) => 50,
            (6, 1) => 80,
            (6, 2) => 90,
            _ => unreachable!(),
        };
        entry.level = entry.level.clamp(1, max_level);
        normalized += usize::from(old != (entry.elite, entry.level));
    }
    normalized
}

fn normalize_plan_compute_operbox_compat(entries: &mut [OperBoxEntry]) -> usize {
    let mut normalized = 0;
    for entry in entries {
        // Historical plan.compute payloads used rarity=1 as an unknown-rarity
        // sentinel while preserving the real elite and level fields. Record the
        // compatibility case; the dedicated replay constructor accepts it
        // without demoting E1/E2 operators.
        if entry.rarity <= 2 && (entry.elite > 0 || entry.level > 30) {
            normalized += 1;
        }
    }
    normalized
}

fn validate_supported_bundle(bundle: &DebugBundleEnvelope) -> Result<(), Error> {
    let version = bundle
        .version
        .as_deref()
        .ok_or_else(|| Error::msg("debug bundle missing version"))?;
    if version != "beta-test-bundle-v2-next-serve" {
        return Err(Error::msg(format!(
            "unsupported debug bundle version {version:?}"
        )));
    }
    let request = bundle
        .serve_request
        .as_ref()
        .ok_or_else(|| Error::msg("debug bundle missing serveRequest"))?;
    if request.method != "plan" && request.method != "plan.compute" {
        return Err(Error::msg(format!(
            "debug rerun supports method=plan and method=plan.compute bundles; got {:?}",
            request.method
        )));
    }
    let response = bundle
        .serve_response
        .as_ref()
        .ok_or_else(|| Error::msg("debug bundle missing serveResponse"))?;
    if !response.ok {
        return Err(Error::msg(
            "debug rerun currently supports successful source invocations only",
        ));
    }
    Ok(())
}

fn select_snapshot(inline: Option<&Value>, sibling: &Path, label: &str) -> Result<Value, Error> {
    let sibling_value = if sibling.exists() {
        let raw = fs::read(sibling).map_err(|error| {
            Error::msg(format!(
                "{label} sibling read {}: {error}",
                sibling.display()
            ))
        })?;
        Some(serde_json::from_slice::<Value>(&raw).map_err(|error| {
            Error::msg(format!(
                "{label} sibling parse {}: {error}",
                sibling.display()
            ))
        })?)
    } else {
        None
    };
    match (inline, sibling_value) {
        (Some(inline), Some(sibling_snapshot)) => {
            let inline_hash = canonical_sha256(inline)?;
            let sibling_hash = canonical_sha256(&sibling_snapshot)?;
            if inline_hash != sibling_hash {
                return Err(Error::msg(format!(
                    "debug bundle {label} conflicts with sibling {}: inline={inline_hash} sibling={sibling_hash}",
                    sibling.display()
                )));
            }
            Ok(inline.clone())
        }
        (Some(inline), None) => Ok(inline.clone()),
        (None, Some(sibling)) => Ok(sibling),
        (None, None) => Err(Error::msg(format!(
            "debug bundle missing {label}; sibling {} not found",
            sibling.display()
        ))),
    }
}

fn select_compute_snapshot(
    inline: Option<&Value>,
    request: Option<&Value>,
    sibling: &Path,
    label: &str,
) -> Result<Value, Error> {
    if let (Some(inline), Some(request)) = (inline, request) {
        if canonical_sha256(inline)? != canonical_sha256(request)? {
            return Err(Error::msg(format!(
                "plan.compute {label} conflicts between bundle and request"
            )));
        }
        return Ok(inline.clone());
    }
    if let Some(value) = inline.or(request) {
        return Ok(value.clone());
    }
    select_snapshot(None, sibling, label)
}

fn rerun_top_k(
    args: &[String],
    bundle: &DebugBundleEnvelope,
) -> Result<OptionWithOrigin<usize>, Error> {
    if let Some(value) = optional_value(args, "--top") {
        return Ok(OptionWithOrigin {
            value: value
                .parse()
                .map_err(|_| Error::msg(format!("invalid --top {value:?}")))?,
            origin: "explicit_override",
        });
    }
    if let Some(top) = bundle
        .serve_request
        .as_ref()
        .and_then(|request| {
            request.params.get("top").or_else(|| {
                request
                    .params
                    .get("options")
                    .and_then(|options| options.get("top"))
            })
        })
        .and_then(Value::as_u64)
    {
        return Ok(OptionWithOrigin {
            value: usize::try_from(top).map_err(|_| Error::msg("recorded top exceeds usize"))?,
            origin: "recorded_request",
        });
    }
    Ok(OptionWithOrigin {
        value: 20,
        origin: "historical_default",
    })
}

fn rerun_rotation(
    args: &[String],
    bundle: &DebugBundleEnvelope,
) -> Result<OptionWithOrigin<TimedRotationProfile>, Error> {
    if let Some(value) = optional_value(args, "--rotation") {
        return Ok(OptionWithOrigin {
            value: value.parse()?,
            origin: "explicit_override",
        });
    }
    if let Some(value) = bundle.serve_request.as_ref().and_then(|request| {
        request.params.get("rotation").or_else(|| {
            request
                .params
                .get("options")
                .and_then(|options| options.get("rotation"))
        })
    }) {
        // serve 侧协议名（serde wire name）只通过 serde 反序列化，不走 CLI FromStr 别名。
        let profile = TimedRotationProfile::deserialize(value)?;
        return Ok(OptionWithOrigin {
            value: profile,
            origin: "recorded_request",
        });
    }
    Ok(OptionWithOrigin {
        value: TimedRotationProfile::default(),
        origin: "historical_default",
    })
}

fn projection_from_report(report: &TeamRotationReport) -> Result<BehaviorProjection, Error> {
    let rotation = serde_json::to_value(report.profile)?
        .as_str()
        .ok_or_else(|| Error::msg("rotation profile did not serialize as string"))?
        .to_string();
    let mut shifts = report
        .shifts
        .iter()
        .map(|shift| {
            let transition = serde_json::to_value(shift.transition)?
                .as_str()
                .ok_or_else(|| Error::msg("shift transition did not serialize as string"))?
                .to_string();
            let mut rooms = shift
                .assignment
                .iter_rooms()
                .map(|room| RoomProjection {
                    room_id: room.room_id.0.clone(),
                    operators: room
                        .operators
                        .iter()
                        .map(|operator| operator.name.clone())
                        .collect(),
                })
                .collect::<Vec<_>>();
            rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
            Ok(ShiftProjection {
                index: shift.index,
                duration_hours: shift.duration_hours,
                transition,
                rooms,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    shifts.sort_by_key(|shift| shift.index);
    Ok(BehaviorProjection {
        rotation,
        daily_trade: report.daily.trade.as_f64(),
        daily_manufacture: report.daily.manufacture.as_f64(),
        daily_power: report.daily.power.as_f64(),
        shifts,
    })
}

fn projection_from_recorded_result(result: &Value) -> Result<BehaviorProjection, Error> {
    if result.get("rotation").and_then(Value::as_object).is_some() {
        return projection_from_plan_compute_result(result);
    }
    let (rotation, shifts_value, daily_trade, daily_manufacture, daily_power) = (
        required_str(result, "rotation")?.to_string(),
        result
            .get("shifts")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::msg("recorded result missing shifts array"))?,
        required_f64(result, "daily_trade_efficiency")?,
        required_f64(result, "daily_manufacture_efficiency")?,
        required_f64(result, "daily_power_efficiency")?,
    );
    let mut shifts = Vec::with_capacity(shifts_value.len());
    for shift in shifts_value {
        let index = required_u64(shift, "index")? as usize;
        let duration_hours = required_f64(shift, "duration_hours")?;
        let transition = required_str(shift, "transition")?.to_string();
        let room_values = shift
            .get("assignment")
            .and_then(|assignment| assignment.get("rooms"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                Error::msg(format!("recorded shift {index} missing assignment.rooms"))
            })?;
        let mut rooms = Vec::with_capacity(room_values.len());
        for room in room_values {
            let room_id = required_str(room, "room_id")?.to_string();
            let operators = room
                .get("operators")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::msg(format!("recorded room {room_id} missing operators")))?
                .iter()
                .map(|operator| required_str(operator, "name").map(str::to_string))
                .collect::<Result<Vec<_>, Error>>()?;
            rooms.push(RoomProjection { room_id, operators });
        }
        rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
        shifts.push(ShiftProjection {
            index,
            duration_hours,
            transition,
            rooms,
        });
    }
    shifts.sort_by_key(|shift| shift.index);
    Ok(BehaviorProjection {
        rotation,
        daily_trade,
        daily_manufacture,
        daily_power,
        shifts,
    })
}

fn projection_from_plan_compute_result(result: &Value) -> Result<BehaviorProjection, Error> {
    let rotation_result = result
        .get("rotation")
        .ok_or_else(|| Error::msg("recorded result missing rotation"))?;
    let rotation = required_str(rotation_result, "profile")?.to_string();
    let shifts_value = rotation_result
        .get("shifts")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::msg("recorded rotation missing shifts array"))?;
    let plans = result
        .get("maa")
        .and_then(|maa| maa.get("plans"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::msg("recorded plan.compute result missing maa.plans"))?;
    let daily = rotation_result
        .get("daily")
        .ok_or_else(|| Error::msg("recorded rotation missing daily"))?;
    let mut shifts = Vec::with_capacity(shifts_value.len());
    for shift in shifts_value {
        let index = required_u64(shift, "index")? as usize;
        let plan = plans
            .get(index)
            .ok_or_else(|| Error::msg(format!("recorded MAA result missing plan {index}")))?;
        shifts.push(ShiftProjection {
            index,
            duration_hours: required_f64(shift, "duration_hours")?,
            // plan.compute exposes the stable MAA assignment but not this internal transition enum.
            transition: "replace_rooms".to_string(),
            rooms: maa_rooms(plan)?,
        });
    }
    shifts.sort_by_key(|shift| shift.index);
    Ok(BehaviorProjection {
        rotation,
        daily_trade: required_f64(daily, "trade")?,
        daily_manufacture: required_f64(daily, "manufacture")?,
        daily_power: required_f64(daily, "power")?,
        shifts,
    })
}

fn maa_rooms(plan: &Value) -> Result<Vec<RoomProjection>, Error> {
    let rooms = plan
        .get("rooms")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::msg("recorded MAA plan missing rooms"))?;
    let mut projections = Vec::new();
    for (category, values) in rooms {
        let values = values
            .as_array()
            .ok_or_else(|| Error::msg(format!("recorded MAA room {category} is not an array")))?;
        for (index, value) in values.iter().enumerate() {
            let operators = value
                .get("operators")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    Error::msg(format!("recorded MAA room {category} missing operators"))
                })?
                .iter()
                .map(|operator| {
                    operator
                        .as_str()
                        .map(str::to_string)
                        .or_else(|| {
                            operator
                                .get("name")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .ok_or_else(|| Error::msg("recorded MAA operator is not a name"))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            projections.push(RoomProjection {
                room_id: maa_room_id(category, index),
                operators,
            });
        }
    }
    projections.sort_by(|left, right| left.room_id.cmp(&right.room_id));
    Ok(projections)
}

fn maa_room_id(category: &str, index: usize) -> String {
    match category {
        "control" => "control".to_string(),
        "trading" => format!("trade_{}", index + 1),
        "manufacture" => format!("manu_{}", index + 1),
        "power" => format!("power_{}", index + 1),
        "dormitory" => format!("dorm_{}", index + 1),
        "meeting" => "meeting".to_string(),
        "hire" => "office".to_string(),
        "training" => "training_room".to_string(),
        other => format!("{other}_{}", index + 1),
    }
}

fn compare_projection(
    local: &BehaviorProjection,
    expected: Option<Result<BehaviorProjection, Error>>,
    inputs_overridden: bool,
) -> ProjectionComparison {
    let local_sha256 = canonical_sha256(&serde_json::to_value(local).unwrap_or(Value::Null))
        .unwrap_or_else(|_| "unavailable".to_string());
    match expected {
        None => ProjectionComparison {
            status: "not_available",
            reason: Some("recorded serveResponse.result is absent".to_string()),
            local_sha256,
            expected_sha256: None,
            expected: None,
        },
        Some(Err(error)) => ProjectionComparison {
            status: "not_comparable",
            reason: Some(error.to_string()),
            local_sha256,
            expected_sha256: None,
            expected: None,
        },
        Some(Ok(expected)) => {
            let expected_sha256 =
                canonical_sha256(&serde_json::to_value(&expected).unwrap_or(Value::Null)).ok();
            if inputs_overridden {
                return ProjectionComparison {
                    status: "not_comparable",
                    reason: Some("--top or --rotation overrides the recorded request".to_string()),
                    local_sha256,
                    expected_sha256,
                    expected: Some(expected),
                };
            }
            ProjectionComparison {
                status: if local == &expected {
                    "match"
                } else {
                    "different"
                },
                reason: (local != &expected)
                    .then(|| "stable rotation/assignment projection differs".to_string()),
                local_sha256,
                expected_sha256,
                expected: Some(expected),
            }
        }
    }
}

fn focused_projection(
    args: &[String],
    report: &TeamRotationReport,
) -> Result<Option<Value>, Error> {
    let room = optional_value(args, "--room");
    let operator = optional_value(args, "--operator");
    if room.is_some() && operator.is_some() {
        return Err(Error::msg("--room and --operator are mutually exclusive"));
    }
    let shift_filter = optional_value(args, "--shift")
        .map(|value| {
            let one_based: usize = value
                .parse()
                .map_err(|_| Error::msg(format!("invalid --shift {value:?}")))?;
            if one_based == 0 {
                return Err(Error::msg("--shift is 1-based"));
            }
            Ok(one_based - 1)
        })
        .transpose()?;
    if shift_filter.is_some() && room.is_none() && operator.is_none() {
        return Err(Error::msg("--shift requires --room or --operator"));
    }
    if let Some(room_id) = room {
        let shifts = report
            .shifts
            .iter()
            .filter(|shift| shift_filter.is_none_or(|index| shift.index == index))
            .map(|shift| {
                let room = shift
                    .assignment
                    .iter_rooms()
                    .find(|room| room.room_id.0 == room_id);
                let anchors = report
                    .assignment_plans
                    .get(shift.plan_index)
                    .map(|plan| {
                        plan.anchors
                            .iter()
                            .filter(|anchor| {
                                anchor
                                    .room_id
                                    .as_ref()
                                    .is_some_and(|resolved| resolved.0 == room_id)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({
                    "shift": shift.index + 1,
                    "plan_index": shift.plan_index,
                    "room": room,
                    "plan_anchors": anchors,
                })
            })
            .collect::<Vec<_>>();
        let matched = shifts.iter().any(|shift| !shift["room"].is_null());
        return Ok(Some(json!({
            "kind": "room",
            "room_id": room_id,
            "matched": matched,
            "shifts": shifts,
        })));
    }
    if let Some(name) = operator {
        let team = report
            .teams
            .iter()
            .find(|team| team.operators.iter().any(|operator| operator == &name));
        let shifts = report
            .shifts
            .iter()
            .filter(|shift| shift_filter.is_none_or(|index| shift.index == index))
            .map(|shift| {
                let room = shift.assignment.room_of_operator(&name);
                json!({
                    "shift": shift.index + 1,
                    "plan_index": shift.plan_index,
                    "working_room": room.map(|room| &room.room_id),
                    "resting_team": shift.resting_team,
                })
            })
            .collect::<Vec<_>>();
        let selected_rules = report
            .peak_plan
            .selected_rules
            .iter()
            .filter(|rule| rule.operators.iter().any(|operator| operator == &name))
            .collect::<Vec<_>>();
        let anchors = report
            .peak_plan
            .anchors
            .iter()
            .filter(|anchor| anchor.operator == name)
            .collect::<Vec<_>>();
        let producer_dependencies = report
            .peak_plan
            .resolved_producer_dependencies
            .iter()
            .filter(|dependency| {
                dependency
                    .producers
                    .iter()
                    .any(|operator| operator == &name)
                    || dependency
                        .consumers
                        .iter()
                        .any(|operator| operator == &name)
            })
            .collect::<Vec<_>>();
        return Ok(Some(json!({
            "kind": "operator",
            "operator": name,
            "matched": team.is_some() || shifts.iter().any(|shift| !shift["working_room"].is_null()),
            "team": team,
            "peak_plan": {
                "selected_rules": selected_rules,
                "anchors": anchors,
                "excluded": report.peak_plan.excluded_operators.iter().any(|operator| operator == &name),
                "producer_dependencies": producer_dependencies,
            },
            "shifts": shifts,
        })));
    }
    Ok(None)
}

fn current_runtime_fingerprint() -> CurrentRuntimeFingerprint {
    let executable = match std::env::current_exe() {
        Ok(path) => fingerprint_path("infra-cli", &path),
        Err(error) => FingerprintEntry {
            name: "infra-cli".to_string(),
            resolved_path: None,
            sha256: None,
            error: Some(error.to_string()),
        },
    };
    let catalogs = RUNTIME_CATALOGS
        .iter()
        .map(|name| match data_path(name) {
            Ok(path) => fingerprint_path(name, &path),
            Err(error) => FingerprintEntry {
                name: (*name).to_string(),
                resolved_path: None,
                sha256: None,
                error: Some(error.to_string()),
            },
        })
        .collect();
    CurrentRuntimeFingerprint {
        scope: FINGERPRINT_SCOPE,
        cli_version: env!("CARGO_PKG_VERSION"),
        executable,
        catalogs,
    }
}

fn fingerprint_path(name: &str, path: &Path) -> FingerprintEntry {
    match fs::read(path) {
        Ok(bytes) => FingerprintEntry {
            name: name.to_string(),
            resolved_path: Some(path.to_string_lossy().into_owned()),
            sha256: Some(sha256_bytes(&bytes)),
            error: None,
        },
        Err(error) => FingerprintEntry {
            name: name.to_string(),
            resolved_path: Some(path.to_string_lossy().into_owned()),
            sha256: None,
            error: Some(error.to_string()),
        },
    }
}

fn canonical_sha256(value: &Value) -> Result<String, Error> {
    let normalized = canonicalize(value);
    Ok(sha256_bytes(&serde_json::to_vec(&normalized)?))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let ordered = values
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        _ => value.clone(),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_rerun_artifacts(
    output_dir: &Path,
    layout: &Value,
    operbox: &Value,
    report: &DebugRerunReport,
) -> Result<(), Error> {
    if output_dir.exists() {
        return Err(Error::msg(format!(
            "debug output directory already exists: {}",
            output_dir.display()
        )));
    }
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(
        ".infra-debug-rerun-{}-{}",
        std::process::id(),
        NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    if stage.exists() {
        return Err(Error::msg(format!(
            "debug staging directory collision: {}",
            stage.display()
        )));
    }
    create_private_dir(&stage)?;
    let result = (|| {
        write_private_json(&stage.join("layout.json"), layout)?;
        write_private_json(&stage.join("operbox.json"), operbox)?;
        for shift in &report.rotation.shifts {
            write_private_json(
                &stage.join(format!("team_shift_{}.json", shift.index + 1)),
                &shift.assignment,
            )?;
        }
        write_private_json(&stage.join("rerun-report.json"), report)?;
        write_private_json(
            &stage.join("complete.json"),
            &json!({
                "schema_version": RERUN_SCHEMA_VERSION,
                "private_evidence": true,
                "complete": true,
            }),
        )?;
        fs::rename(&stage, output_dir)?;
        Ok(())
    })();
    if result.is_err() && stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn create_private_dir(path: &Path) -> Result<(), Error> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn required_path(args: &[String], flag: &str) -> Result<PathBuf, Error> {
    optional_value(args, flag)
        .map(PathBuf::from)
        .ok_or_else(|| Error::msg(format!("missing {flag} <path>")))
}

fn optional_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg(format!("missing string field {key}")))
}

fn required_u64(value: &Value, key: &str) -> Result<u64, Error> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::msg(format!("missing integer field {key}")))
}

fn required_f64(value: &Value, key: &str) -> Result<f64, Error> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| Error::msg(format!("missing numeric field {key}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_hash_ignores_object_key_order_but_not_array_order() {
        let left = json!({"b": 2, "a": [{"y": 2, "x": 1}, 3]});
        let right: Value = serde_json::from_str(r#"{"a":[{"x":1,"y":2},3],"b":2}"#).unwrap();
        let reordered_array = json!({"a": [3, {"x": 1, "y": 2}], "b": 2});
        assert_eq!(
            canonical_sha256(&left).unwrap(),
            canonical_sha256(&right).unwrap()
        );
        assert_ne!(
            canonical_sha256(&left).unwrap(),
            canonical_sha256(&reordered_array).unwrap()
        );
    }

    #[test]
    fn recorded_projection_is_order_independent_for_rooms() {
        let recorded = json!({
            "rotation": "abc_12_6_6",
            "daily_trade_efficiency": 1.0,
            "daily_manufacture_efficiency": 2.0,
            "daily_power_efficiency": 3.0,
            "shifts": [{
                "index": 0,
                "duration_hours": 12.0,
                "transition": "replace_rooms",
                "assignment": {"rooms": [
                    {"room_id": "trade_2", "operators": [{"name": "B"}]},
                    {"room_id": "trade_1", "operators": [{"name": "A"}]}
                ]}
            }]
        });
        let projection = projection_from_recorded_result(&recorded).unwrap();
        assert_eq!(projection.shifts[0].rooms[0].room_id, "trade_1");
        assert_eq!(projection.shifts[0].rooms[1].room_id, "trade_2");
    }

    #[test]
    fn plan_compute_projection_reads_rotation_and_maa_assignments() {
        let recorded = json!({
            "rotation": {
                "profile": "abc_12_6_6",
                "daily": {"trade": 1.0, "manufacture": 2.0, "power": 3.0},
                "shifts": [{"index": 0, "duration_hours": 12.0}]
            },
            "maa": {
                "plans": [{
                    "rooms": {
                        "control": [{"operators": ["斩业星熊", "夕"]}],
                        "trading": [{"operators": ["A"]}],
                        "manufacture": [],
                        "power": []
                    }
                }]
            }
        });
        let projection = projection_from_recorded_result(&recorded).unwrap();
        assert_eq!(projection.rotation, "abc_12_6_6");
        assert_eq!(projection.daily_trade, 1.0);
        assert_eq!(
            projection.shifts[0]
                .rooms
                .iter()
                .find(|room| room.room_id == "control")
                .unwrap()
                .operators,
            vec!["斩业星熊", "夕"]
        );
        assert_eq!(
            projection.shifts[0]
                .rooms
                .iter()
                .find(|room| room.room_id == "trade_1")
                .unwrap()
                .operators,
            vec!["A"]
        );
    }

    #[test]
    fn inline_snapshot_conflict_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "infra-debug-conflict-{}-{}",
            std::process::id(),
            NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let sibling = root.join("layout.json");
        fs::write(&sibling, br#"{"value":2}"#).unwrap();
        let error = select_snapshot(Some(&json!({"value": 1})), &sibling, "layout").unwrap_err();
        assert!(error.to_string().contains("conflicts with sibling"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_inline_snapshot_uses_fixed_sibling() {
        let root = std::env::temp_dir().join(format!(
            "infra-debug-fallback-{}-{}",
            std::process::id(),
            NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let sibling = root.join("layout.json");
        fs::write(&sibling, br#"{"value":2}"#).unwrap();
        assert_eq!(
            select_snapshot(None, &sibling, "layout").unwrap(),
            json!({"value": 2})
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rerun_rotation_uses_serde_wire_names_from_recorded_request() {
        for (wire, expected) in [
            ("abc_12_6_6", TimedRotationProfile::Abc12_6_6),
            ("abc_12_12_12", TimedRotationProfile::Abc12_12_12),
            ("main_backup_12_12", TimedRotationProfile::MainBackup12_12),
            (
                "fiammetta_witch_12_12",
                TimedRotationProfile::FiammettaWitch12_12,
            ),
            ("fiammetta_8_8_4_4", TimedRotationProfile::Fiammetta8_8_4_4),
            ("abyssal_7_5_7_5", TimedRotationProfile::Abyssal7_5_7_5),
        ] {
            let bundle = DebugBundleEnvelope {
                version: None,
                started_at: None,
                layout: None,
                operbox: None,
                input_summary: None,
                serve_request: Some(RecordedRequest {
                    method: "plan".to_string(),
                    params: json!({ "rotation": wire }),
                }),
                serve_response: None,
            };
            let rotation = rerun_rotation(&[], &bundle).expect("wire name resolves");
            assert_eq!(rotation.value, expected);
            assert_eq!(rotation.origin, "recorded_request");
        }
    }

    #[test]
    fn rerun_rotation_cli_override_keeps_from_str_aliases() {
        let bundle = DebugBundleEnvelope {
            version: None,
            started_at: None,
            layout: None,
            operbox: None,
            input_summary: None,
            serve_request: None,
            serve_response: None,
        };
        let rotation = rerun_rotation(&["--rotation".into(), "2".into()], &bundle)
            .expect("cli alias resolves");
        assert_eq!(rotation.value, TimedRotationProfile::MainBackup12_12);
        assert_eq!(rotation.origin, "explicit_override");
    }

    #[test]
    fn explicit_override_disables_recorded_projection_comparison() {
        let projection = BehaviorProjection {
            rotation: "abc_12_6_6".to_string(),
            daily_trade: 1.0,
            daily_manufacture: 2.0,
            daily_power: 3.0,
            shifts: vec![],
        };
        let comparison = compare_projection(&projection, Some(Ok(projection.clone())), true);
        assert_eq!(comparison.status, "not_comparable");
        assert!(comparison.reason.unwrap().contains("overrides"));
    }

    #[test]
    fn plan_compute_compat_keeps_elite_when_rarity_is_unknown_sentinel() {
        let mut entries = vec![OperBoxEntry {
            id: "char_4208_wintim".to_string(),
            name: "冬时".to_string(),
            elite: 1,
            level: 60,
            own: true,
            potential: 6,
            rarity: 1,
        }];
        assert_eq!(normalize_plan_compute_operbox_compat(&mut entries), 1);
        assert_eq!(entries[0].elite, 1);
        assert_eq!(entries[0].level, 60);
        assert_eq!(entries[0].rarity, 1);
    }

    #[test]
    fn legacy_operbox_progress_is_normalized_only_for_invalid_entries() {
        let mut entries = vec![
            OperBoxEntry {
                id: "12f".to_string(),
                name: "12F".to_string(),
                elite: 2,
                level: 90,
                own: true,
                potential: 0,
                rarity: 2,
            },
            OperBoxEntry {
                id: "normal".to_string(),
                name: "能天使".to_string(),
                elite: 2,
                level: 90,
                own: true,
                potential: 0,
                rarity: 6,
            },
            OperBoxEntry {
                id: "unowned".to_string(),
                name: "未拥有".to_string(),
                elite: 0,
                level: 0,
                own: false,
                potential: 0,
                rarity: 6,
            },
        ];

        assert_eq!(normalize_legacy_operbox_progress(&mut entries), 1);
        assert_eq!((entries[0].elite, entries[0].level), (0, 30));
        assert_eq!((entries[1].elite, entries[1].level), (2, 90));
        assert_eq!((entries[2].elite, entries[2].level), (0, 0));
    }

    #[cfg(unix)]
    #[test]
    fn private_artifacts_use_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "infra-debug-permissions-{}-{}",
            std::process::id(),
            NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_dir(&root).unwrap();
        let file = root.join("private.json");
        write_private_json(&file, &json!({"private": true})).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn public_summary_omits_private_rotation_and_focus() {
        let runtime = CurrentRuntimeFingerprint {
            scope: FINGERPRINT_SCOPE,
            cli_version: "test",
            executable: FingerprintEntry {
                name: "infra-cli".to_string(),
                resolved_path: None,
                sha256: Some("abc".to_string()),
                error: None,
            },
            catalogs: vec![],
        };
        let inputs = RerunInputSummary {
            layout_rooms: 1,
            operbox_entries: 1,
            owned_operators: 1,
            legacy_progress_normalized_entries: 0,
            layout_sha256: "layout".to_string(),
            operbox_sha256: "operbox".to_string(),
            top_k: OptionWithOrigin {
                value: 20,
                origin: "recorded_request",
            },
            rotation: OptionWithOrigin {
                value: TimedRotationProfile::default(),
                origin: "historical_default",
            },
        };
        let summary = DebugRerunSummary {
            schema_version: RERUN_SCHEMA_VERSION,
            mode: "current_runtime_rerun",
            bundle_sha256: "bundle",
            source: PublicSourceSummary {
                bundle_version: Some("beta-test-bundle-v2-next-serve"),
                request_method: "plan",
                original_runtime_fingerprint: "unknown",
            },
            current_runtime: &runtime,
            inputs: &inputs,
            comparison: PublicComparison {
                status: "match",
                reason: None,
                local_sha256: "local",
                expected_sha256: Some("expected"),
            },
            private_report_written: true,
            artifact_directory: Some("private"),
        };
        let value = serde_json::to_value(summary).unwrap();
        assert!(value.get("rotation").is_none());
        assert!(value.get("projection").is_none());
        assert!(value.get("focus").is_none());
        assert!(value["source"].get("source_name").is_none());
    }
}
