//! 用户主入口：`plan` = 账号分析 + αβγ 三队排班。

use std::fs;
use std::path::{Path, PathBuf};

use super::plan_compute::{compute_plan, PlanComputeInput, PlanResources, RequestedOutputs};
use crate::output::{print_box_profile_report, print_team_rotation_text};
use infra_core::box_profile::baseline_path_or_default;
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::AssignBaseOptions;
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::{default_layout_243_path, default_operbox_full_e2_path, OperBox};
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::Error;

use super::prefix_shadow::{prefix_shadow_out_from_args, run_prefix_shadow, PrefixShadowInput};

pub fn plan_cmd(args: &[String]) -> Result<(), Error> {
    let rotation_profile = super::timed_rotation_profile_from_args(args)?;
    let operbox_path = operbox_path_from_args(args)?;
    let layout_path = match layout_path_from_args(args) {
        Some(p) => p,
        None => default_layout_243_path()?,
    };
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

    let baseline_path = baseline_path_or_default(baseline_path_from_args(args).as_deref())?;

    let layout_label = layout_path.to_string_lossy().into_owned();
    let operbox_label = operbox_path.to_string_lossy().into_owned();
    let owned = operbox.owned_count();

    let maa_out = maa_out_from_args(args);
    let maa_title = args
        .windows(2)
        .find(|w| w[0] == "--maa-title")
        .map(|w| w[1].as_str());
    let computed = compute_plan(
        PlanResources {
            instances: &instances,
            table: &table,
        },
        PlanComputeInput {
            blueprint: &blueprint,
            operbox: &operbox,
            layout_label: &layout_label,
            operbox_label: &operbox_label,
            baseline_operbox: Some(&baseline_path),
            top_k,
            rotation_profile,
            system_preferences: &system_preferences,
            maa_title,
            fiammetta_enable,
        },
        RequestedOutputs {
            profile: true,
            maa: maa_out.is_some(),
        },
    )?;
    let profile = computed
        .profile
        .as_ref()
        .expect("profile was requested for plan command");
    let rotation = &computed.current.rotation;

    if let Some(path) = prefix_shadow_out_from_args(args)? {
        let shadow_options = AssignBaseOptions {
            top_k,
            system_preferences: system_preferences.clone(),
            fiammetta_enable,
            ..AssignBaseOptions::default()
        };
        run_prefix_shadow(PrefixShadowInput {
            path: &path,
            layout_label: &layout_label,
            operbox_label: &operbox_label,
            blueprint: &blueprint,
            operbox: &operbox,
            instances: &instances,
            table: &table,
            baseline_options: &shadow_options,
            profile: rotation_profile,
            baseline: rotation,
        });
    }

    let profile_out = profile_out_path(args, &operbox_path);
    let profile_json = serde_json::to_string_pretty(&profile)?;
    ensure_parent_dir(&profile_out)?;
    fs::write(&profile_out, format!("{profile_json}\n"))?;
    eprintln!("profile JSON → {}", profile_out.display());

    if let Some(dir) = output_dir_from_args(args) {
        fs::create_dir_all(&dir)?;
        for shift in &rotation.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
            eprintln!("排班 assignment → {}", path.display());
        }
    }

    if let Some(maa_path) = maa_out {
        let schedule = computed
            .maa
            .as_ref()
            .expect("MAA schedule was requested when --maa-out is present");
        schedule.save(&maa_path)?;
        eprintln!("MAA 排班 JSON → {}", maa_path.display());
    }

    // ── 3. 输出：分析 + 排班 → stdout；路径提示 → stderr ─────────────────
    let json_only = args.iter().any(|a| a == "--json");
    if json_only {
        print!("{profile_json}");
    } else {
        eprintln!("layout={layout_label}");
        eprintln!("operbox={operbox_label} owned={owned}");
        println!();
        println!("╔══════════════════════════════════════╗");
        println!("║  infra-cli plan · 243 基建方案         ║");
        println!("╚══════════════════════════════════════╝");
        println!();
        print_box_profile_report(&profile);
        println!();
        println!("══════════════════════════════════════");
        println!("  {}", rotation.profile.display_name());
        println!("══════════════════════════════════════");
        println!();
        print_team_rotation_text(&layout_label, &operbox_label, owned, &rotation)?;
    }

    Ok(())
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

fn operbox_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    Ok(args
        .windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or(default_operbox_full_e2_path()?))
}

fn layout_path_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--layout")
        .map(|w| PathBuf::from(&w[1]))
}

fn baseline_path_from_args(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|w| w[0] == "--baseline")
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

fn profile_out_path(args: &[String], operbox_path: &Path) -> PathBuf {
    if let Some(p) = args
        .windows(2)
        .find(|w| w[0] == "--profile-out")
        .map(|w| PathBuf::from(&w[1]))
    {
        return p;
    }
    if let Some(dir) = output_dir_from_args(args) {
        return dir.join("box_profile.json");
    }
    operbox_path.with_file_name(format!(
        "{}_profile.json",
        operbox_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("operbox")
    ))
}

fn ensure_parent_dir(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}
