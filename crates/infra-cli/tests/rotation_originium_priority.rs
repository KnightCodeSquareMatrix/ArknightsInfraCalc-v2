use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("infra-cli must be nested under the workspace crates directory")
        .to_path_buf()
}

fn write_temp_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "infra-cli-originium-priority-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create temporary output directory");
    directory
}

fn write_h2_layout(directory: &Path) -> PathBuf {
    let path = directory.join("two_originium_h2_layout.json");
    let root = workspace_root();
    let source =
        fs::read_to_string(root.join("data/fixtures/243/one_originium_121212_layout.json"))
            .expect("read base originium layout");
    let mut layout: serde_json::Value =
        serde_json::from_str(&source).expect("parse base originium layout");
    layout["rooms"]
        .as_array_mut()
        .expect("layout rooms array")
        .push(serde_json::json!({
            "id": "manu_5",
            "kind": "factory",
            "level": 3,
            "product": { "factory": { "recipe": "originium" } }
        }));
    for room in layout["rooms"].as_array_mut().expect("layout rooms array") {
        match room["id"].as_str() {
            Some("manu_4") => {
                room["product"] = serde_json::json!({ "factory": { "recipe": "gold" } });
            }
            _ => {}
        }
    }
    fs::write(
        &path,
        serde_json::to_vec_pretty(&layout).expect("serialize H2 layout"),
    )
    .expect("write H2 layout");
    path
}

fn write_two_originium_layout(directory: &Path) -> PathBuf {
    let path = write_h2_layout(directory);
    let mut layout: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read H2 layout")).expect("parse H2 layout");
    for room in layout["rooms"].as_array_mut().expect("layout rooms array") {
        if room["id"] == "manu_4" {
            room["product"] = serde_json::json!({ "factory": { "recipe": "originium" } });
        }
    }
    fs::write(
        &path,
        serde_json::to_vec_pretty(&layout).expect("serialize two originium layout"),
    )
    .expect("write two originium layout");
    path
}

fn write_operbox_variant(
    directory: &Path,
    file_name: &str,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> PathBuf {
    let path = directory.join(file_name);
    let root = workspace_root();
    let source = fs::read_to_string(root.join("data/fixtures/243/operbox_full_e2.json"))
        .expect("read full operbox");
    let mut operbox: serde_json::Value = serde_json::from_str(&source).expect("parse full operbox");
    mutate(&mut operbox);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&operbox).expect("serialize operbox variant"),
    )
    .expect("write operbox variant");
    path
}

fn entry<'a>(operbox: &'a mut serde_json::Value, name: &str) -> &'a mut serde_json::Value {
    operbox
        .as_array_mut()
        .expect("operbox array")
        .iter_mut()
        .find(|operator| operator["name"] == name)
        .unwrap_or_else(|| panic!("operator {name} is missing from the fixture"))
}

fn run_plan_with(
    directory: &Path,
    layout: &Path,
    operbox: &Path,
    rotation: &str,
    gate: bool,
    output_prefix: &str,
) -> std::process::Output {
    let root = workspace_root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_infra-cli"));
    command
        .current_dir(&root)
        .args(["plan", "--no-perf", "--operbox"])
        .arg(operbox)
        .args(["--layout"])
        .arg(layout)
        .args(["--rotation", rotation, "--profile-out"])
        .arg(directory.join(format!("{output_prefix}-profile.json")))
        .arg("--rotation-out")
        .arg(directory.join(format!("{output_prefix}-rotation.json")))
        .arg("--maa-out")
        .arg(directory.join(format!("{output_prefix}-maa.json")))
        .arg("--json");
    if gate {
        command.arg("--assert-invariants");
    }
    let output = command.output().expect("run infra-cli plan");
    assert!(
        gate || output.status.success(),
        "ordinary plan should still emit a rotation\nstatus={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn run_plan(directory: &Path, layout: &Path, rotation: &str) -> std::process::Output {
    run_plan_with(
        directory,
        layout,
        &workspace_root().join("data/fixtures/243/operbox_full_e2.json"),
        rotation,
        true,
        "plan",
    )
}

fn room_index(layout_path: &Path, room_id: &str) -> usize {
    let layout: serde_json::Value =
        serde_json::from_slice(&fs::read(layout_path).expect("read MAA ordering layout"))
            .expect("parse MAA ordering layout");
    layout["rooms"]
        .as_array()
        .expect("layout rooms array")
        .iter()
        .filter(|room| room["kind"] == "factory")
        .position(|room| room["id"] == room_id)
        .unwrap_or_else(|| panic!("factory room {room_id} is missing from layout"))
}

fn read_json(path: &Path) -> serde_json::Value {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!("parse {}: {error}", path.display());
    })
}

fn room_operator_names<'a>(
    rotation: &'a serde_json::Value,
    shift_index: usize,
    room_id: &str,
) -> Vec<&'a str> {
    rotation["shifts"]
        .as_array()
        .expect("rotation shifts array")
        .iter()
        .find(|shift| shift["index"].as_u64() == Some(shift_index as u64))
        .unwrap_or_else(|| panic!("shift {shift_index} is missing"))["assignment"]["rooms"]
        .as_array()
        .expect("assignment rooms array")
        .iter()
        .find(|room| room["room_id"].as_str() == Some(room_id))
        .and_then(|room| room["operators"].as_array())
        .unwrap_or_else(|| panic!("room {room_id} is missing from shift {shift_index}"))
        .iter()
        .map(|operator| operator["name"].as_str().expect("operator name"))
        .collect()
}

fn is_originium_pair_room(names: &[&str]) -> bool {
    names.contains(&"泡泡") && names.contains(&"火神")
}

fn assert_plan_timing_and_teams(rotation: &serde_json::Value, durations: &[f64]) {
    let shifts = rotation["shifts"]
        .as_array()
        .expect("rotation shifts array");
    assert_eq!(shifts.len(), 3);
    let actual: Vec<f64> = shifts
        .iter()
        .map(|shift| shift["duration_hours"].as_f64().expect("duration_hours"))
        .collect();
    assert_eq!(actual, durations);
    assert_eq!(shifts[0]["index"], 0);
    let teams: Vec<(Vec<&str>, &str)> = shifts
        .iter()
        .map(|shift| {
            (
                shift["active_teams"]
                    .as_array()
                    .expect("active_teams")
                    .iter()
                    .map(|team| team.as_str().expect("team label"))
                    .collect(),
                shift["resting_team"].as_str().expect("resting_team"),
            )
        })
        .collect();
    assert_eq!(
        teams,
        vec![
            (vec!["alpha", "beta"], "gamma"),
            (vec!["beta", "gamma"], "alpha"),
            (vec!["gamma", "alpha"], "beta"),
        ]
    );
}

fn assert_pair_success(
    directory: &Path,
    layout_path: &Path,
    rotation_profile: &str,
    source_room: &str,
    working: [usize; 2],
    resting: usize,
) {
    let cli_rotation = if rotation_profile == "abc_12_6_6" {
        "12/6/6"
    } else {
        "12/12/12"
    };
    let output = run_plan(directory, layout_path, cli_rotation);
    assert!(
        !output.stderr.is_empty() || !output.stdout.is_empty(),
        "plan produced no diagnostics"
    );

    let profile = read_json(&directory.join("plan-profile.json"));
    let rotation = read_json(&directory.join("plan-rotation.json"));
    let maa = read_json(&directory.join("plan-maa.json"));

    assert_eq!(profile["rotation_profile"], rotation_profile);
    assert_eq!(rotation["profile"], rotation_profile);
    let durations = if rotation_profile == "abc_12_6_6" {
        vec![12.0, 6.0, 6.0]
    } else {
        vec![12.0, 12.0, 12.0]
    };
    assert_plan_timing_and_teams(&rotation, &durations);
    let shifts = rotation["shifts"]
        .as_array()
        .expect("rotation shifts array");

    let s1 = room_operator_names(&rotation, 0, source_room);
    let s2 = room_operator_names(&rotation, 1, source_room);
    let s3 = room_operator_names(&rotation, 2, source_room);
    assert!(working.contains(&0), "test must list S1 as a working shift");
    let by_shift = [(0usize, &s1), (1usize, &s2), (2usize, &s3)];
    for (shift_index, names) in by_shift {
        let expected = working.contains(&shift_index);
        assert_eq!(
            is_originium_pair_room(names),
            expected,
            "shift {shift_index} pair expectation failed for room {source_room}: {names:?}"
        );
    }
    assert!(
        !working.contains(&resting),
        "working and resting shifts must differ"
    );

    let plans = rotation["assignment_plans"]
        .as_array()
        .expect("assignment_plans array");
    for shift_index in working {
        let plan_index = shifts[shift_index]["plan_index"]
            .as_u64()
            .expect("shift plan_index") as usize;
        let anchors = plans[plan_index]["anchors"]
            .as_array()
            .expect("owning Plan anchors");
        for name in ["泡泡", "火神"] {
            assert!(
                anchors.iter().any(|anchor| {
                    anchor["system_id"] == "rotation_originium_pair"
                        && anchor["operator"] == name
                        && anchor["room_id"] == source_room
                        && anchor["facility"] == "factory"
                        && anchor["recipe"] == "originium"
                        && anchor["fill_policy"]["kind"] == "manufacture_recipe"
                }),
                "shift {shift_index} owning Plan is missing {name} anchor: {anchors:?}"
            );
        }
    }

    let warnings = rotation["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        !warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("rotation_final_invariant"))),
        "unexpected final invariant warning: {warnings:?}"
    );

    assert_eq!(maa["planTimes"], "3班");
    let maa_plans = maa["plans"].as_array().expect("MAA plans array");
    assert_eq!(maa_plans.len(), 3);
    let source_index = room_index(layout_path, source_room);
    let products: Vec<&str> = maa_plans[0]["rooms"]["manufacture"]
        .as_array()
        .expect("MAA manufacture slots")
        .iter()
        .map(|slot| slot["product"].as_str().expect("manufacture product"))
        .collect();
    assert_eq!(products[source_index], "Originium Shard");
    let third_members: Vec<Vec<&str>> = [&s1, &s2, &s3]
        .into_iter()
        .filter(|names| is_originium_pair_room(names))
        .map(|names| {
            names
                .iter()
                .copied()
                .filter(|name| !["泡泡", "火神"].contains(name))
                .collect()
        })
        .collect();
    assert!(
        third_members.iter().all(|members| members.len() == 1),
        "each working pair room must have exactly one third member: {third_members:?}"
    );
    for (shift_index, expected_names) in [(0usize, &s1), (1usize, &s2), (2usize, &s3)] {
        let exported = maa_plans[shift_index]["rooms"]["manufacture"][source_index]["operators"]
            .as_array()
            .expect("MAA source room operators")
            .iter()
            .map(|name| name.as_str().expect("MAA operator name"))
            .collect::<Vec<_>>();
        assert_eq!(
            exported, *expected_names,
            "MAA manufacture[{source_index}] must match shift {shift_index} {source_room}"
        );
    }

    fs::remove_dir_all(&directory).expect("remove temporary output directory");
}

fn assert_pair_inactive(directory: &Path, layout: &Path, operbox: &Path, rotation_profile: &str) {
    let cli_rotation = if rotation_profile == "abc_12_6_6" {
        "12/6/6"
    } else {
        "12/12/12"
    };
    let output = run_plan_with(directory, layout, operbox, cli_rotation, true, "inactive");
    assert!(
        output.status.success(),
        "inactive pair plan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rotation = read_json(&directory.join("inactive-rotation.json"));
    assert_eq!(rotation["profile"], rotation_profile);
    let warnings = rotation["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        !warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("originium pair"))),
        "inactive pair must fall back silently: {warnings:?}"
    );
    let plans = rotation["assignment_plans"]
        .as_array()
        .expect("assignment_plans array");
    assert!(
        plans.iter().all(
            |plan| plan["anchors"].as_array().is_none_or(|anchors| anchors
                .iter()
                .all(|anchor| anchor["system_id"] != "rotation_originium_pair"))
        ),
        "inactive pair must not emit preclaim anchors"
    );
}

#[test]
fn originium_pair_short_third_member_is_a_warning_not_a_hard_gate() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = write_operbox_variant(&directory, "pair-only.json", |operbox| {
        for operator in operbox.as_array_mut().expect("operbox array") {
            let name = operator["name"].as_str().expect("operator name");
            if !["泡泡", "火神"].contains(&name) {
                operator["own"] = serde_json::json!(false);
            }
        }
    });

    let ordinary = run_plan_with(&directory, &layout, &operbox, "12/12/12", false, "short");
    assert!(
        ordinary.status.success(),
        "ordinary plan should retain a short frozen room\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ordinary.stdout),
        String::from_utf8_lossy(&ordinary.stderr)
    );
    let rotation = read_json(&directory.join("short-rotation.json"));
    assert_plan_timing_and_teams(&rotation, &[12.0, 12.0, 12.0]);
    let s1 = room_operator_names(&rotation, 0, "manu_4");
    let s2 = room_operator_names(&rotation, 2, "manu_4");
    assert_eq!(s1, vec!["泡泡", "火神"]);
    assert_eq!(s2, vec!["泡泡", "火神"]);
    let warnings = rotation["warnings"].as_array().expect("warnings array");
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|warning| warning.contains("制造站 manu_4 缺 1 名"))
    }));
    assert!(!warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|warning| warning.contains("originium pair preclaim failed"))
    }));

    let maa = read_json(&directory.join("short-maa.json"));
    let source_index = room_index(&layout, "manu_4");
    for (shift_index, expected) in [(0usize, ["泡泡", "火神"]), (2usize, ["泡泡", "火神"])]
    {
        let exported: Vec<&str> = maa["plans"][shift_index]["rooms"]["manufacture"][source_index]
            ["operators"]
            .as_array()
            .expect("MAA source room operators")
            .iter()
            .map(|name| name.as_str().expect("MAA operator name"))
            .collect();
        assert_eq!(exported, expected);
    }

    let gated = run_plan_with(
        &directory,
        &layout,
        &operbox,
        "12/12/12",
        true,
        "short-gated",
    );
    assert!(
        gated.status.success(),
        "the pair invariant must not turn ordinary short staffing into a hard failure\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&gated.stdout),
        String::from_utf8_lossy(&gated.stderr)
    );

    fs::remove_dir_all(&directory).expect("remove temporary output directory");
}

#[test]
fn abc_12_6_6_h1_originium_pair_reaches_12h_shift() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    assert_pair_success(&directory, &layout, "abc_12_6_6", "manu_4", [0, 2], 1);
}

#[test]
fn abc_12_6_6_h2_originium_pair_reaches_12h_shift() {
    let directory = write_temp_dir();
    let layout = write_h2_layout(&directory);
    assert_pair_success(&directory, &layout, "abc_12_6_6", "manu_5", [0, 2], 1);
}

#[test]
fn abc_12_12_12_h1_originium_pair_keeps_two_work_one_rest() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    assert_pair_success(&directory, &layout, "abc_12_12_12", "manu_4", [0, 2], 1);
}

#[test]
fn abc_12_12_12_h2_originium_pair_keeps_two_work_one_rest() {
    let directory = write_temp_dir();
    let layout = write_h2_layout(&directory);
    assert_pair_success(&directory, &layout, "abc_12_12_12", "manu_5", [0, 2], 1);
}

#[test]
fn originium_pair_uses_first_legal_room_in_blueprint_order() {
    let directory = write_temp_dir();
    let layout = write_two_originium_layout(&directory);
    let output = run_plan(&directory, &layout, "12/6/6");
    assert!(
        output.status.success(),
        "complete plan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rotation = read_json(&directory.join("plan-rotation.json"));
    for shift_index in [0usize, 2] {
        let first = room_operator_names(&rotation, shift_index, "manu_4");
        let second = room_operator_names(&rotation, shift_index, "manu_5");
        assert!(is_originium_pair_room(&first));
        assert!(!is_originium_pair_room(&second));
    }
    fs::remove_dir_all(&directory).expect("remove temporary output directory");
}

#[test]
fn originium_pair_stays_inactive_when_bubble_is_not_e1() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = write_operbox_variant(&directory, "bubble-e0.json", |operbox| {
        let bubble = entry(operbox, "泡泡");
        bubble["elite"] = serde_json::json!(0);
        bubble["level"] = serde_json::json!(45);
    });
    assert_pair_inactive(&directory, &layout, &operbox, "abc_12_6_6");
}

#[test]
fn originium_pair_stays_inactive_when_vulcan_is_not_e2() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = write_operbox_variant(&directory, "vulcan-e1.json", |operbox| {
        let vulcan = entry(operbox, "火神");
        vulcan["elite"] = serde_json::json!(1);
        vulcan["level"] = serde_json::json!(70);
    });
    assert_pair_inactive(&directory, &layout, &operbox, "abc_12_12_12");
}

#[test]
fn originium_pair_stays_inactive_without_vulcan() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = write_operbox_variant(&directory, "no-vulcan.json", |operbox| {
        entry(operbox, "火神")["own"] = serde_json::json!(false);
    });
    assert_pair_inactive(&directory, &layout, &operbox, "abc_12_6_6");
}

#[test]
fn originium_pair_stays_inactive_without_bubble() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = write_operbox_variant(&directory, "no-bubble.json", |operbox| {
        entry(operbox, "泡泡")["own"] = serde_json::json!(false);
    });
    assert_pair_inactive(&directory, &layout, &operbox, "abc_12_12_12");
}

#[test]
fn originium_pair_stays_inactive_without_a_legal_originium_factory() {
    let directory = write_temp_dir();
    let path = directory.join("no-originium-layout.json");
    let root = workspace_root();
    let source =
        fs::read_to_string(root.join("data/fixtures/243/one_originium_121212_layout.json"))
            .expect("read base originium layout");
    let mut layout: serde_json::Value =
        serde_json::from_str(&source).expect("parse base originium layout");
    for room in layout["rooms"].as_array_mut().expect("layout rooms array") {
        if room["id"] == "manu_4" {
            room["product"] = serde_json::json!({ "factory": { "recipe": "gold" } });
        }
    }
    fs::write(
        &path,
        serde_json::to_vec_pretty(&layout).expect("serialize layout"),
    )
    .expect("write layout");
    assert_pair_inactive(
        &directory,
        &path,
        &root.join("data/fixtures/243/operbox_full_e2.json"),
        "abc_12_12_12",
    );
}

#[test]
fn originium_pair_does_not_apply_to_fiammetta_profile() {
    let directory = write_temp_dir();
    let layout = workspace_root().join("data/fixtures/243/one_originium_121212_layout.json");
    let operbox = workspace_root().join("data/fixtures/243/operbox_full_e2.json");
    let output = run_plan_with(
        &directory,
        &layout,
        &operbox,
        "fiammetta-8844",
        true,
        "fiammetta",
    );
    assert!(
        output.status.success(),
        "non-ABC plan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rotation = read_json(&directory.join("fiammetta-rotation.json"));
    assert_eq!(rotation["profile"], "fiammetta_8_8_4_4");
    let plans = rotation["assignment_plans"]
        .as_array()
        .expect("assignment_plans array");
    assert!(plans.iter().all(
        |plan| plan["anchors"].as_array().is_none_or(|anchors| anchors
            .iter()
            .all(|anchor| anchor["system_id"] != "rotation_originium_pair"))
    ));
    let warnings = rotation["warnings"].as_array().cloned().unwrap_or_default();
    assert!(!warnings.iter().any(|warning| warning
        .as_str()
        .is_some_and(|warning| warning.contains("originium pair"))));

    let maa = read_json(&directory.join("fiammetta-maa.json"));
    assert_eq!(maa["planTimes"], "4班");
    assert_eq!(maa["plans"].as_array().expect("MAA plans").len(), 4);
    fs::remove_dir_all(&directory).expect("remove temporary output directory");
}
