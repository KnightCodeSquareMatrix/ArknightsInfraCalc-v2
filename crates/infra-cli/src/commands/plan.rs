//! 用户主入口：`plan` = 账号分析 + αβγ 三队排班。

use std::fs;
use std::path::{Path, PathBuf};

use crate::output::{print_box_profile_report, print_team_rotation_text};
use infra_core::box_profile::{
    baseline_path_or_default, build_box_profile, BoxProfileOptions,
};
use infra_core::export::{build_from_team_rotation, MaaExportOptions};
use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::{AssignBaseOptions, BaseBlueprint};
use infra_core::operbox::{default_layout_243_path, OperBox};
use infra_core::schedule::schedule_team_rotation;
use infra_core::skill_table::{default_skill_table_path, SkillTable};
use infra_core::Error;

pub fn plan_cmd(args: &[String]) -> Result<(), Error> {
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

    let blueprint = BaseBlueprint::load(&layout_path)?;
    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;

    let baseline_path = baseline_path_or_default(baseline_path_from_args(args).as_deref())?;

    let layout_label = layout_path.to_string_lossy().into_owned();
    let operbox_label = operbox_path.to_string_lossy().into_owned();
    let owned = operbox.owned_count();

    // ── 1. 账号画像 / 分析 ──────────────────────────────────────────────
    let profile = build_box_profile(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &layout_label,
        &operbox_label,
        &BoxProfileOptions {
            top_k,
            baseline_operbox: Some(baseline_path),
            ..BoxProfileOptions::default()
        },
    )?;

    let profile_out = profile_out_path(args, &operbox_path);
    let profile_json = serde_json::to_string_pretty(&profile)?;
    fs::write(&profile_out, format!("{profile_json}\n"))?;
    eprintln!("profile JSON → {}", profile_out.display());

    // ── 2. αβγ 三队排班 ─────────────────────────────────────────────────
    let rotation = schedule_team_rotation(
        &blueprint,
        &operbox,
        &instances,
        &table,
        &AssignBaseOptions {
            top_k,
            ..AssignBaseOptions::default()
        },
    )?;

    if let Some(dir) = output_dir_from_args(args) {
        fs::create_dir_all(&dir)?;
        for shift in &rotation.shifts {
            let path = dir.join(format!("team_shift_{}.json", shift.index + 1));
            shift.assignment.save(&path)?;
            eprintln!("排班 assignment → {}", path.display());
        }
    }

    if let Some(maa_path) = maa_out_from_args(args) {
        let mut maa_opts = MaaExportOptions::for_blueprint(&blueprint);
        if let Some(title) = args
            .windows(2)
            .find(|w| w[0] == "--maa-title")
            .map(|w| w[1].clone())
        {
            maa_opts.title = title;
        }
        let schedule = build_from_team_rotation(&blueprint, &rotation, &maa_opts)?;
        schedule.save(&maa_path)?;
        eprintln!("MAA 排班 JSON → {}", maa_path.display());
    }

    // ── 3. 输出：分析 + 排班 → stdout；路径提示 → stderr ─────────────────
    let json_only = args.iter().any(|a| a == "--json");
    if json_only {
        print_box_profile_report(&profile);
        println!();
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
        println!("  αβγ 三队轮休排班");
        println!("══════════════════════════════════════");
        println!();
        print_team_rotation_text(&layout_label, &operbox_label, owned, &rotation)?;
    }

    Ok(())
}

fn operbox_path_from_args(args: &[String]) -> Result<PathBuf, Error> {
    args.windows(2)
        .find(|w| w[0] == "--operbox")
        .map(|w| PathBuf::from(&w[1]))
        .ok_or_else(|| {
            Error::msg("plan requires --operbox <path.json|.xlsx> (一图流练度表 xlsx 可直接传入)")
        })
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
    operbox_path
        .with_file_name(format!(
            "{}_profile.json",
            operbox_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("operbox")
        ))
}
