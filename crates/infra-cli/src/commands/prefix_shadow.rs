use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use infra_core::instances::OperatorInstances;
use infra_core::layout::{
    AssignBaseOptions, BaseAssignment, BaseBlueprint, RoomEfficiencySnapshot,
};
use infra_core::operbox::OperBox;
use infra_core::response_dependency::{
    producer_rule_catalog, resolve_assignment_producer_dependencies, ProducerAdmission,
    ResolvedProducerDependency,
};
use infra_core::schedule::{
    schedule_timed_rotation_request, DailyTotals, ShiftEfficiencies, TeamRotationReport,
    TimedRotationProfile, TimedRotationRequest,
};
use infra_core::scoring::{ScoringPolicyId, TradeManufactureEfficiencyWeights};
use infra_core::skill_table::SkillTable;
use infra_core::Error;
use serde::Serialize;

pub(super) fn prefix_shadow_out_from_args(args: &[String]) -> Result<Option<PathBuf>, Error> {
    let Some(index) = args.iter().position(|arg| arg == "--prefix-shadow-out") else {
        return Ok(None);
    };
    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| Error::msg("missing --prefix-shadow-out <file.json>"))?;
    Ok(Some(PathBuf::from(value)))
}

pub(super) struct PrefixShadowInput<'a> {
    pub path: &'a Path,
    pub layout_label: &'a str,
    pub operbox_label: &'a str,
    pub blueprint: &'a BaseBlueprint,
    pub operbox: &'a OperBox,
    pub instances: &'a OperatorInstances,
    pub table: &'a SkillTable,
    pub baseline_options: &'a AssignBaseOptions,
    pub rotation_request: TimedRotationRequest,
    pub baseline: &'a TeamRotationReport,
}

pub(super) fn run_prefix_shadow(input: PrefixShadowInput<'_>) -> bool {
    handle_prefix_shadow_result(write_prefix_shadow_report(input))
}

fn handle_prefix_shadow_result(result: Result<(), Error>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("prefix shadow failed: {error}");
            false
        }
    }
}

fn write_prefix_shadow_report(input: PrefixShadowInput<'_>) -> Result<(), Error> {
    let mut candidate_options = input.baseline_options.clone();
    candidate_options.dynamic_producer_policy =
        ScoringPolicyId::TradeManufactureWeightedEfficiencyV1;

    let started = Instant::now();
    let candidate = schedule_timed_rotation_request(
        input.blueprint,
        input.operbox,
        input.instances,
        input.table,
        &candidate_options,
        input.rotation_request,
    );
    let candidate_elapsed = started.elapsed();

    let baseline = PolicyRunSummary {
        policy: ScoringPolicyId::ControlInjectRawSumV0,
        weights: None,
        elapsed_ms: duration_millis(input.baseline.elapsed),
        daily: &input.baseline.daily,
    };
    let candidate = match candidate {
        Ok(candidate) => match compare_rotations(
            input.blueprint,
            input.instances,
            input.table,
            input.baseline,
            &candidate,
        ) {
            Ok(comparison) => CandidateOutcome::Completed {
                policy: ScoringPolicyId::TradeManufactureWeightedEfficiencyV1,
                weights: candidate_options.trade_manufacture_weights,
                elapsed_ms: duration_millis(candidate_elapsed),
                daily: candidate.daily.clone(),
                comparison,
            },
            Err(error) => CandidateOutcome::Error {
                stage: "comparison",
                policy: ScoringPolicyId::TradeManufactureWeightedEfficiencyV1,
                weights: candidate_options.trade_manufacture_weights,
                elapsed_ms: duration_millis(candidate_elapsed),
                error: error.to_string(),
            },
        },
        Err(error) => CandidateOutcome::Error {
            stage: "schedule",
            policy: ScoringPolicyId::TradeManufactureWeightedEfficiencyV1,
            weights: candidate_options.trade_manufacture_weights,
            elapsed_ms: duration_millis(candidate_elapsed),
            error: error.to_string(),
        },
    };
    let report = PrefixShadowReport {
        schema_version: 1,
        mode: "dynamic_producer_prefix_shadow",
        guarantee: GuaranteeScope {
            peak_prefix: "BEST_FOUND",
            search_space: "one ControlInjectRawSumV0-filled control combo per dynamic producer presence set, followed by ordered room fill",
            rotation: "evaluated, not optimized by the prefix comparator",
        },
        inputs: ShadowInputs {
            layout: input.layout_label,
            operbox: input.operbox_label,
            rotation: input.rotation_request.profile(),
        },
        baseline,
        candidate,
    };

    if let Some(parent) = input.path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(input.path, serde_json::to_string_pretty(&report)? + "\n")?;
    eprintln!("prefix shadow JSON -> {}", input.path.display());
    Ok(())
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[derive(Serialize)]
struct PrefixShadowReport<'a> {
    schema_version: u32,
    mode: &'static str,
    guarantee: GuaranteeScope,
    inputs: ShadowInputs<'a>,
    baseline: PolicyRunSummary<'a>,
    candidate: CandidateOutcome,
}

#[derive(Serialize)]
struct GuaranteeScope {
    peak_prefix: &'static str,
    search_space: &'static str,
    rotation: &'static str,
}

#[derive(Serialize)]
struct ShadowInputs<'a> {
    layout: &'a str,
    operbox: &'a str,
    rotation: TimedRotationProfile,
}

#[derive(Serialize)]
struct PolicyRunSummary<'a> {
    policy: ScoringPolicyId,
    #[serde(skip_serializing_if = "Option::is_none")]
    weights: Option<TradeManufactureEfficiencyWeights>,
    elapsed_ms: u64,
    daily: &'a DailyTotals,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CandidateOutcome {
    Completed {
        policy: ScoringPolicyId,
        weights: TradeManufactureEfficiencyWeights,
        elapsed_ms: u64,
        daily: DailyTotals,
        comparison: RotationComparison,
    },
    Error {
        stage: &'static str,
        policy: ScoringPolicyId,
        weights: TradeManufactureEfficiencyWeights,
        elapsed_ms: u64,
        error: String,
    },
}

#[derive(Serialize)]
struct RotationComparison {
    assignment_changed: bool,
    constraint_validation: &'static str,
    baseline_dynamic_producer_shifts: usize,
    candidate_dynamic_producer_shifts: usize,
    daily_delta_millis: EfficiencyDelta,
    shifts: Vec<ShiftComparison>,
}

#[derive(Serialize)]
struct ShiftComparison {
    index: usize,
    assignment_changed: bool,
    room_changes: Vec<RoomChange>,
    baseline_efficiencies: ShiftEfficiencies,
    candidate_efficiencies: ShiftEfficiencies,
    efficiency_delta_millis: EfficiencyDelta,
    baseline_dependencies: Vec<ResolvedProducerDependency>,
    candidate_dependencies: Vec<ResolvedProducerDependency>,
}

#[derive(Serialize)]
struct RoomChange {
    room_id: String,
    baseline_operators: Vec<String>,
    candidate_operators: Vec<String>,
    baseline_efficiency: Option<RoomEfficiencySnapshot>,
    candidate_efficiency: Option<RoomEfficiencySnapshot>,
}

#[derive(Serialize)]
struct EfficiencyDelta {
    trade: i32,
    manufacture: i32,
    power: i32,
}

fn compare_rotations(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    baseline: &TeamRotationReport,
    candidate: &TeamRotationReport,
) -> Result<RotationComparison, Error> {
    if baseline.shifts.len() != candidate.shifts.len() {
        return Err(Error::msg(format!(
            "prefix shadow shift count mismatch: baseline={} candidate={}",
            baseline.shifts.len(),
            candidate.shifts.len()
        )));
    }

    let mut baseline_dynamic_producer_shifts = 0;
    let mut candidate_dynamic_producer_shifts = 0;
    let dynamic_rule_ids: HashSet<_> = producer_rule_catalog(table)?
        .rules
        .iter()
        .filter(|rule| rule.admission == ProducerAdmission::DeferredOptional)
        .map(|rule| rule.id.as_str())
        .collect();
    let mut shifts = Vec::with_capacity(baseline.shifts.len());
    for (baseline_shift, candidate_shift) in baseline.shifts.iter().zip(&candidate.shifts) {
        let room_changes = room_changes(&baseline_shift.assignment, &candidate_shift.assignment);
        let baseline_dependencies = resolve_assignment_producer_dependencies(
            blueprint,
            &baseline_shift.assignment,
            instances,
            table,
        )?;
        let candidate_dependencies = resolve_assignment_producer_dependencies(
            blueprint,
            &candidate_shift.assignment,
            instances,
            table,
        )?;
        baseline_dynamic_producer_shifts += usize::from(
            baseline_dependencies
                .iter()
                .any(|dependency| dynamic_rule_ids.contains(dependency.rule_id.as_str())),
        );
        candidate_dynamic_producer_shifts += usize::from(
            candidate_dependencies
                .iter()
                .any(|dependency| dynamic_rule_ids.contains(dependency.rule_id.as_str())),
        );
        shifts.push(ShiftComparison {
            index: baseline_shift.index,
            assignment_changed: !room_changes.is_empty(),
            room_changes,
            baseline_efficiencies: baseline_shift.efficiencies.clone(),
            candidate_efficiencies: candidate_shift.efficiencies.clone(),
            efficiency_delta_millis: efficiency_delta(
                &baseline_shift.efficiencies,
                &candidate_shift.efficiencies,
            ),
            baseline_dependencies,
            candidate_dependencies,
        });
    }

    Ok(RotationComparison {
        assignment_changed: shifts.iter().any(|shift| shift.assignment_changed),
        constraint_validation:
            "candidate schedule completed; shadow performed no independent constraint audit",
        baseline_dynamic_producer_shifts,
        candidate_dynamic_producer_shifts,
        daily_delta_millis: EfficiencyDelta {
            trade: candidate.daily.trade.millis() - baseline.daily.trade.millis(),
            manufacture: candidate.daily.manufacture.millis() - baseline.daily.manufacture.millis(),
            power: candidate.daily.power.millis() - baseline.daily.power.millis(),
        },
        shifts,
    })
}

fn efficiency_delta(
    baseline: &ShiftEfficiencies,
    candidate: &ShiftEfficiencies,
) -> EfficiencyDelta {
    EfficiencyDelta {
        trade: candidate.trade_efficiency.millis() - baseline.trade_efficiency.millis(),
        manufacture: candidate.manufacture_efficiency.millis()
            - baseline.manufacture_efficiency.millis(),
        power: candidate.power_efficiency.millis() - baseline.power_efficiency.millis(),
    }
}

fn room_changes(baseline: &BaseAssignment, candidate: &BaseAssignment) -> Vec<RoomChange> {
    let room_ids: BTreeSet<_> = baseline
        .iter_rooms()
        .chain(candidate.iter_rooms())
        .map(|room| room.room_id.0.clone())
        .collect();
    room_ids
        .into_iter()
        .filter_map(|room_id| {
            let room_id_value = infra_core::layout::RoomId::from(room_id.as_str());
            let baseline_room = baseline.room_assignment(&room_id_value);
            let candidate_room = candidate.room_assignment(&room_id_value);
            let baseline_operators = baseline_room
                .map(|room| room.operators.iter().map(|op| op.name.clone()).collect())
                .unwrap_or_default();
            let candidate_operators = candidate_room
                .map(|room| room.operators.iter().map(|op| op.name.clone()).collect())
                .unwrap_or_default();
            (baseline_operators != candidate_operators).then(|| RoomChange {
                room_id,
                baseline_operators,
                candidate_operators,
                baseline_efficiency: baseline_room.and_then(|room| room.efficiency.clone()),
                candidate_efficiency: candidate_room.and_then(|room| room.efficiency.clone()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use infra_core::layout::AssignedOperator;

    #[test]
    fn room_diff_only_reports_changed_staffing() {
        let mut baseline = BaseAssignment::default();
        baseline.set_room("control", vec![AssignedOperator::new("阿米娅", 2)]);
        baseline.set_room("trade_1", vec![AssignedOperator::new("伺夜", 2)]);
        let mut candidate = baseline.clone();
        candidate.set_room("control", vec![AssignedOperator::new("八幡海铃", 2)]);

        let changes = room_changes(&baseline, &candidate);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].room_id, "control");
        assert_eq!(changes[0].baseline_operators, ["阿米娅"]);
        assert_eq!(changes[0].candidate_operators, ["八幡海铃"]);
    }

    #[test]
    fn shadow_output_flag_requires_a_path() {
        let error = prefix_shadow_out_from_args(&["--prefix-shadow-out".into()]).unwrap_err();
        assert!(error
            .to_string()
            .contains("missing --prefix-shadow-out <file.json>"));
        let error = prefix_shadow_out_from_args(&["--prefix-shadow-out".into(), "--json".into()])
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("missing --prefix-shadow-out <file.json>"));
    }

    #[test]
    fn shadow_runtime_errors_are_non_fatal() {
        handle_prefix_shadow_result(Err(Error::msg("read-only output path")));
    }

    #[test]
    fn candidate_error_records_stage_without_a_fake_comparison() {
        let value = serde_json::to_value(CandidateOutcome::Error {
            stage: "schedule",
            policy: ScoringPolicyId::TradeManufactureWeightedEfficiencyV1,
            weights: TradeManufactureEfficiencyWeights::default(),
            elapsed_ms: 12,
            error: "infeasible".to_string(),
        })
        .unwrap();
        assert_eq!(value["status"], "error");
        assert_eq!(value["stage"], "schedule");
        assert!(value.get("comparison").is_none());
    }
}
