use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn custom_rules(evidence_path: &str, heading: Option<&str>) -> String {
    let heading = heading
        .map(|value| format!(r#", "heading": {value:?}"#))
        .unwrap_or_default();
    format!(
        r#"{{
  "version": 2,
  "acquisition_policy": {{"default_rarity_le": 4, "named_exceptions": []}},
  "rules": [{{
    "id": "custom_tequila",
    "kind": "standalone",
    "scope": "independent",
    "label": "龙舌兰测试规则",
    "admission": {{"required_core": [], "pick_one_core": []}},
    "members": [{{
      "operator": "龙舌兰",
      "role": "independent",
      "target": {{"elite": 1}},
      "priority": "P0",
      "acquisition": "owned_only",
      "rarity": 5
    }}],
    "evidence": [{{"path": {evidence_path:?}{heading}}}],
    "review": {{"status": "confirmed", "conflicts": []}}
  }}]
}}"#
    )
}

fn run_explain(rules_path: &Path) -> serde_json::Value {
    let root = workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_infra-cli"))
        .current_dir(&root)
        .args([
            "advice",
            "--operbox",
            "data/fixtures/training_advice/witch_only_tequila.json",
            "--rules",
        ])
        .arg(rules_path)
        .arg("--explain")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_answer() -> String {
    let root = workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_infra-cli"))
        .current_dir(&root)
        .args([
            "advice",
            "--operbox",
            "data/fixtures/training_advice/witch_ready_untrained.json",
            "--answer",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run_default_json(explain: bool) -> serde_json::Value {
    let root = workspace_root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_infra-cli"));
    command.current_dir(&root).args([
        "advice",
        "--operbox",
        "data/fixtures/training_advice/witch_ready_untrained.json",
    ]);
    if explain {
        command.arg("--explain");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_player_json_with_context(layout: Option<&str>, context: Option<&str>) -> serde_json::Value {
    let root = workspace_root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_infra-cli"));
    command.current_dir(&root).args([
        "advice",
        "--operbox",
        "data/fixtures/training_advice/witch_ready_untrained.json",
        "--player",
    ]);
    if let Some(layout) = layout {
        command.args(["--layout", layout]);
    }
    if let Some(context) = context {
        command.args(["--player-context", context]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_player_json(layout: Option<&str>) -> serde_json::Value {
    run_player_json_with_context(layout, None)
}

#[test]
fn custom_rules_explain_reads_workspace_markdown_but_rejects_absolute_sources() {
    let directory = std::env::temp_dir().join(format!(
        "infra-cli-advice-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::create_dir_all(&directory).unwrap();

    let valid_rules = directory.join("valid.json");
    fs::write(
        &valid_rules,
        custom_rules("docs/练卡推荐规则.md", Some("RAG 解释层")),
    )
    .unwrap();
    let valid = run_explain(&valid_rules);
    assert_eq!(valid["report"]["now"][0]["operator"], "龙舌兰");
    assert_eq!(
        valid["rag_input"]["evidence_snippets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(valid["rag_input"]["unavailable_source_refs"]
        .as_array()
        .unwrap()
        .is_empty());

    let absolute_rules = directory.join("absolute.json");
    fs::write(&absolute_rules, custom_rules("/etc/passwd.md", None)).unwrap();
    let absolute = run_explain(&absolute_rules);
    assert!(absolute["rag_input"]["evidence_snippets"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        absolute["rag_input"]["unavailable_source_refs"][0]["path"],
        "/etc/passwd.md"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn answer_mode_prints_deterministic_sections_without_changing_json_modes() {
    let answer = run_answer();
    let report = run_default_json(false);
    let bundle = run_default_json(true);
    assert_eq!(bundle["report"], report);
    assert_eq!(report["schema_version"], 2);
    assert!(report.get("report").is_none());
    assert!(bundle.get("rag_input").is_some());

    assert!(answer.starts_with("# 基建练卡建议\n"));
    assert!(answer.contains(&format!(
        "## 当前可练（{}）",
        report["now"].as_array().unwrap().len()
    )));
    assert!(answer.contains(&format!(
        "## 获取后可练（{}）",
        report["conditional"].as_array().unwrap().len()
    )));
    assert!(answer.contains(&format!(
        "## 暂缓体系（{}）",
        report["blocked"].as_array().unwrap().len()
    )));
    assert!(answer.contains(&format!(
        "## 待复核（{}）",
        report["review"].as_array().unwrap().len()
    )));
    assert!(answer.contains(&format!(
        "## 已达标（{}）",
        report["ready"].as_array().unwrap().len()
    )));
    for fact in bundle["rag_input"]["fact_skeleton"].as_array().unwrap() {
        let text = fact["text"].as_str().unwrap();
        assert!(
            answer.contains(&format!("- {text}")),
            "missing fact: {text}"
        );
    }
    assert!(answer.contains("巫恋"));
    assert!(answer.contains("龙舌兰"));
    assert!(answer.contains("卡夫卡"));
    assert!(!answer.trim_start().starts_with('{'));
}

#[test]
fn player_mode_exposes_three_sections_and_layout_gates_originium() {
    let unknown = run_player_json(None);
    assert_eq!(unknown["schema_version"], 2);
    assert_eq!(unknown["newbie_section_status"], "shown");
    assert!(unknown["incomplete_newbie"].as_array().unwrap().len() > 10);
    assert!(!unknown["combinations"].as_array().unwrap().is_empty());
    assert_eq!(unknown["recommendations"].as_array().unwrap().len(), 10);
    assert!(unknown["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["operator"] != "薄绿" && item["operator"] != "火神"));

    let originium_path = std::env::temp_dir().join(format!(
        "infra-cli-player-originium-{}.json",
        std::process::id()
    ));
    fs::write(
        &originium_path,
        r#"{"rooms":[{"id":"factory","kind":"factory","level":3,"product":{"factory":{"recipe":"originium"}}}]}"#,
    )
    .unwrap();
    let originium = run_player_json(originium_path.to_str());
    fs::remove_file(originium_path).unwrap();
    assert_eq!(originium["context"]["has_originium_shard_factory"], true);
    assert!(originium["combinations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == "bubble_group"));
}

#[test]
fn player_context_applies_t9_without_hiding_the_incomplete_inventory() {
    let context_path = std::env::temp_dir().join(format!(
        "infra-cli-player-context-{}.json",
        std::process::id()
    ));
    fs::write(
        &context_path,
        r#"{"trade_average_efficiency_percent":30.1,"manufacturing_average_efficiency_percent":25.1}"#,
    )
    .unwrap();
    let report = run_player_json_with_context(None, context_path.to_str());
    fs::remove_file(context_path).unwrap();

    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["newbie_section_status"], "skipped_by_efficiency");
    assert!(report["incomplete_newbie"].as_array().unwrap().len() > 10);
    assert!(report["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["reason"] != "newbie_required"));
    assert_eq!(report["context"]["trade_average_efficiency_percent"], 30.1);
    assert_eq!(
        report["context"]["manufacturing_average_efficiency_percent"],
        25.1
    );
}
