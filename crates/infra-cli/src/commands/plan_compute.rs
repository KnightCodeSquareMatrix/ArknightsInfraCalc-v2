use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use infra_core::box_profile::{
    build_box_profile_from_current_probe, run_user_rotation_probe_with_request_and_preferences,
    BoxProfile, BoxProfileOptions, LayoutProbe,
};
use infra_core::export::{build_from_team_rotation, MaaExportOptions, MaaSchedule};
use infra_core::instances::OperatorInstances;
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{OperBox, OperBoxEntry};
use infra_core::schedule::{assert_final_rotation_invariants_for_request, TimedRotationRequest};
use infra_core::skill_table::SkillTable;
use infra_core::Error;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanComputeParams {
    pub schema_version: u32,
    pub layout: BaseBlueprint,
    pub operbox: Vec<OperBoxEntry>,
    #[serde(default)]
    pub labels: PlanComputeLabels,
    #[serde(default)]
    pub options: PlanComputeOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanComputeLabels {
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub operbox: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanComputeOptions {
    #[serde(default)]
    pub rotation: TimedRotationRequest,
    #[serde(default = "default_top_k")]
    pub top: usize,
    #[serde(default)]
    pub system_preferences: HashMap<String, String>,
    #[serde(default)]
    pub maa_title: Option<String>,
    #[serde(default = "default_fiammetta_enable")]
    pub fiammetta_enable: bool,
    #[serde(default)]
    pub support_combinations_enable: bool,
    #[serde(default)]
    pub assert_invariants: bool,
}

fn default_top_k() -> usize {
    20
}

fn default_fiammetta_enable() -> bool {
    true
}

pub(super) fn inline_operbox(mut entries: Vec<OperBoxEntry>) -> Result<OperBox, Error> {
    inline_operbox_inner(&mut entries, false)
}

pub(super) fn inline_operbox_for_debug_rerun(
    mut entries: Vec<OperBoxEntry>,
) -> Result<OperBox, Error> {
    inline_operbox_inner(&mut entries, true)
}

fn inline_operbox_inner(
    entries: &mut Vec<OperBoxEntry>,
    plan_compute_compat: bool,
) -> Result<OperBox, Error> {
    const MAX_OPERBOX_ENTRIES: usize = 1000;

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
    if plan_compute_compat {
        OperBox::from_entries_for_plan_compute_compat(std::mem::take(entries))
    } else {
        OperBox::from_entries(std::mem::take(entries))
    }
}

pub(super) struct PlanResources<'a> {
    pub instances: &'a OperatorInstances,
    pub table: &'a SkillTable,
}

pub(super) struct PlanComputeInput<'a> {
    pub blueprint: &'a BaseBlueprint,
    pub operbox: &'a OperBox,
    pub layout_label: &'a str,
    pub operbox_label: &'a str,
    pub baseline_operbox: Option<&'a Path>,
    pub top_k: usize,
    pub rotation_request: TimedRotationRequest,
    pub system_preferences: &'a HashMap<String, String>,
    pub maa_title: Option<&'a str>,
    pub fiammetta_candidates_out: Option<PathBuf>,
    pub fiammetta_enable: bool,
    pub support_combinations_enable: bool,
    pub assert_invariants: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RequestedOutputs {
    pub profile: bool,
    pub maa: bool,
}

pub(super) struct ComputedPlan {
    pub current: LayoutProbe,
    pub profile: Option<BoxProfile>,
    pub maa: Option<MaaSchedule>,
}

pub(super) struct PlanComputeFailure {
    pub current: Option<LayoutProbe>,
    pub rotation: Option<infra_core::schedule::TeamRotationReport>,
    pub profile: Option<BoxProfile>,
    pub maa: Option<MaaSchedule>,
    pub error: Error,
}

pub(super) enum PlanComputeAttempt {
    Complete(ComputedPlan),
    Failed(PlanComputeFailure),
}

impl PlanComputeAttempt {
    fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
}

pub(super) fn compute_plan(
    resources: PlanResources<'_>,
    input: PlanComputeInput<'_>,
    requested: RequestedOutputs,
) -> Result<ComputedPlan, Error> {
    match compute_plan_attempt(resources, input, requested) {
        PlanComputeAttempt::Complete(computed) => Ok(computed),
        PlanComputeAttempt::Failed(failure) => Err(failure.error),
    }
}

pub(super) fn compute_plan_for_plan_compute(
    resources: PlanResources<'_>,
    input: PlanComputeInput<'_>,
    requested: RequestedOutputs,
) -> PlanComputeAttempt {
    compute_plan_attempt(resources, input, requested)
}

fn compute_plan_attempt(
    resources: PlanResources<'_>,
    input: PlanComputeInput<'_>,
    requested: RequestedOutputs,
) -> PlanComputeAttempt {
    let _memo_scope = infra_core::search::ensure_search_memo_scope();
    let _trace =
        infra_core::profile::ensure_solver_perf_trace(input.layout_label, input.operbox_label);
    let total_started = Instant::now();
    let total_before = infra_core::profile::solver_perf_snapshot();
    let result = compute_plan_inner(resources, input, requested);
    let total_after = infra_core::profile::solver_perf_snapshot();
    infra_core::profile::emit_solver_stage(
        "plan_compute",
        "total",
        if result.is_complete() { "ok" } else { "error" },
        total_started.elapsed(),
        total_before,
        total_after,
    );
    result
}

fn compute_plan_inner(
    resources: PlanResources<'_>,
    input: PlanComputeInput<'_>,
    requested: RequestedOutputs,
) -> PlanComputeAttempt {
    let stage_started = Instant::now();
    let stage_before = infra_core::profile::solver_perf_snapshot();
    let current_result = run_user_rotation_probe_with_request_and_preferences(
        input.blueprint,
        input.operbox,
        resources.instances,
        resources.table,
        input.top_k,
        input.rotation_request.clone(),
        input.system_preferences,
        input.fiammetta_enable,
        input.support_combinations_enable,
        input.fiammetta_candidates_out.clone(),
    );
    let stage_after = infra_core::profile::solver_perf_snapshot();
    infra_core::profile::emit_resolve_stage(
        "plan_compute_resolve",
        "current_probe",
        if current_result.is_ok() {
            "ok"
        } else {
            "error"
        },
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    infra_core::profile::emit_solver_stage(
        "plan_compute",
        "current_probe",
        if current_result.is_ok() {
            "ok"
        } else {
            "error"
        },
        stage_started.elapsed(),
        stage_before,
        stage_after,
    );
    let current = match current_result {
        Ok(current) => current,
        Err(error) => {
            return PlanComputeAttempt::Failed(PlanComputeFailure {
                current: None,
                rotation: error.partial_rotation_report(),
                profile: None,
                maa: None,
                error,
            });
        }
    };

    if input.assert_invariants {
        let stage_started = Instant::now();
        let stage_before = infra_core::profile::solver_perf_snapshot();
        let invariant_result = assert_final_rotation_invariants_for_request(
            input.blueprint,
            input.operbox,
            resources.instances,
            resources.table,
            &current.rotation,
            input.fiammetta_enable,
        );
        let stage_after = infra_core::profile::solver_perf_snapshot();
        infra_core::profile::emit_solver_stage(
            "plan_compute",
            "explicit_invariants",
            if invariant_result.is_ok() {
                "ok"
            } else {
                "error"
            },
            stage_started.elapsed(),
            stage_before,
            stage_after,
        );
        if let Err(error) = invariant_result {
            return PlanComputeAttempt::Failed(PlanComputeFailure {
                current: Some(current),
                rotation: None,
                profile: None,
                maa: None,
                error,
            });
        }
    }

    let stage_started = Instant::now();
    let stage_before = infra_core::profile::solver_perf_snapshot();
    let profile_result = requested
        .profile
        .then(|| {
            build_box_profile_from_current_probe(
                &current,
                input.blueprint,
                input.operbox,
                resources.instances,
                resources.table,
                input.layout_label,
                input.operbox_label,
                &BoxProfileOptions {
                    top_k: input.top_k,
                    baseline_operbox: input.baseline_operbox.map(Path::to_path_buf),
                    rotation_profile: input.rotation_request.profile(),
                    rotation_request: Some(input.rotation_request),
                    system_preferences: input.system_preferences.clone(),
                    fiammetta_enable: input.fiammetta_enable,
                    support_combinations_enable: input.support_combinations_enable,
                    ..BoxProfileOptions::default()
                },
            )
        })
        .transpose();
    let stage_after = infra_core::profile::solver_perf_snapshot();
    if requested.profile {
        infra_core::profile::emit_solver_stage(
            "plan_compute",
            "profile_baseline",
            if profile_result.is_ok() {
                "ok"
            } else {
                "error"
            },
            stage_started.elapsed(),
            stage_before,
            stage_after,
        );
    }
    let profile = match profile_result {
        Ok(profile) => profile,
        Err(error) => {
            return PlanComputeAttempt::Failed(PlanComputeFailure {
                current: Some(current),
                rotation: None,
                profile: None,
                maa: None,
                error,
            });
        }
    };

    let stage_started = Instant::now();
    let stage_before = infra_core::profile::solver_perf_snapshot();
    let maa_result = requested
        .maa
        .then(|| {
            let mut options = MaaExportOptions::for_blueprint(input.blueprint);
            if input.fiammetta_enable {
                options.enable_fiammetta_priority_for_blueprint(input.blueprint);
            }
            if let Some(title) = input.maa_title {
                options.title = title.to_string();
            }
            build_from_team_rotation(input.blueprint, &current.rotation, &options)
        })
        .transpose();
    let stage_after = infra_core::profile::solver_perf_snapshot();
    if requested.maa {
        infra_core::profile::emit_solver_stage(
            "plan_compute",
            "maa_build",
            if maa_result.is_ok() { "ok" } else { "error" },
            stage_started.elapsed(),
            stage_before,
            stage_after,
        );
    }
    let maa = match maa_result {
        Ok(maa) => maa,
        Err(error) => {
            return PlanComputeAttempt::Failed(PlanComputeFailure {
                current: Some(current),
                rotation: None,
                profile,
                maa: None,
                error,
            });
        }
    };

    PlanComputeAttempt::Complete(ComputedPlan {
        current,
        profile,
        maa,
    })
}
