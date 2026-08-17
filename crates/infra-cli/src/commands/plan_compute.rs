use std::collections::HashMap;
use std::path::Path;

use infra_core::box_profile::{
    build_box_profile_from_current_probe, run_user_rotation_probe_with_profile_and_preferences,
    BoxProfile, BoxProfileOptions, LayoutProbe,
};
use infra_core::export::{build_from_team_rotation, MaaExportOptions, MaaSchedule};
use infra_core::instances::OperatorInstances;
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::OperBox;
use infra_core::schedule::TimedRotationProfile;
use infra_core::skill_table::SkillTable;
use infra_core::Error;

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
    pub rotation_profile: TimedRotationProfile,
    pub system_preferences: &'a HashMap<String, String>,
    pub maa_title: Option<&'a str>,
    pub fiammetta_enable: bool,
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

pub(super) fn compute_plan(
    resources: PlanResources<'_>,
    input: PlanComputeInput<'_>,
    requested: RequestedOutputs,
) -> Result<ComputedPlan, Error> {
    let current = run_user_rotation_probe_with_profile_and_preferences(
        input.blueprint,
        input.operbox,
        resources.instances,
        resources.table,
        input.top_k,
        input.rotation_profile,
        input.system_preferences,
        input.fiammetta_enable,
    )?;

    let profile = requested
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
                    rotation_profile: input.rotation_profile,
                    system_preferences: input.system_preferences.clone(),
                    fiammetta_enable: input.fiammetta_enable,
                    ..BoxProfileOptions::default()
                },
            )
        })
        .transpose()?;

    let maa = requested
        .maa
        .then(|| {
            let mut options = MaaExportOptions::for_blueprint(input.blueprint);
            if input.fiammetta_enable {
                options.enable_gongsun_fiammetta_priority();
            }
            if let Some(title) = input.maa_title {
                options.title = title.to_string();
            }
            build_from_team_rotation(input.blueprint, &current.rotation, &options)
        })
        .transpose()?;

    Ok(ComputedPlan {
        current,
        profile,
        maa,
    })
}
