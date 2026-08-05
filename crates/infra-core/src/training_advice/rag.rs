use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::types::{
    BlockedRuleReport, EvidenceRef, EvidenceSnippet, MemberRole, OperatorAdviceItem,
    OperatorTrainingState, RecommendationAction, TrainingAdviceFact, TrainingAdviceRagInput,
    TrainingAdviceReport,
};

const MAX_EXCERPT_CHARS: usize = 1600;

pub fn build_training_advice_rag_input(
    report: &TrainingAdviceReport,
    workspace_root: &Path,
) -> TrainingAdviceRagInput {
    let fact_skeleton = build_fact_skeleton(report);
    let source_requests = collect_source_requests(report);
    let mut evidence_snippets = Vec::new();
    let mut unavailable_source_refs = Vec::new();

    for (_, (source_ref, keywords)) in source_requests {
        if !is_markdown_source(&source_ref.path) {
            continue;
        }
        let Some(path) = resolve_workspace_source(workspace_root, &source_ref.path) else {
            unavailable_source_refs.push(source_ref);
            continue;
        };
        let Ok(markdown) = std::fs::read_to_string(path) else {
            unavailable_source_refs.push(source_ref);
            continue;
        };
        let Some(excerpt) = extract_markdown_snippet(
            &markdown,
            source_ref.heading.as_deref(),
            &keywords.into_iter().collect::<Vec<_>>(),
        ) else {
            unavailable_source_refs.push(source_ref);
            continue;
        };
        evidence_snippets.push(EvidenceSnippet {
            source_ref,
            excerpt,
        });
    }

    TrainingAdviceRagInput {
        schema_version: 1,
        fact_skeleton,
        evidence_snippets,
        unavailable_source_refs,
        guardrails: vec![
            "不得新增 report 中不存在的干员。".to_string(),
            "不得修改 action、target 或 display_priority。".to_string(),
            "不得把 conditional 表述为当前可直接培养。".to_string(),
            "不得把 blocked 或 review 表述为已成立的确定事实。".to_string(),
        ],
    }
}

pub fn render_training_advice_answer(report: &TrainingAdviceReport) -> String {
    let facts = build_fact_skeleton(report);
    let mut train = Vec::new();
    let mut conditional = Vec::new();
    let mut blocked = Vec::new();
    let mut review = Vec::new();
    let mut ready = Vec::new();
    for fact in &facts {
        match fact.action {
            RecommendationAction::Train => train.push(fact),
            RecommendationAction::AcquireThenTrain => conditional.push(fact),
            RecommendationAction::Blocked => blocked.push(fact),
            RecommendationAction::Review => review.push(fact),
            RecommendationAction::Ready => ready.push(fact),
        }
    }

    let mut lines = vec![
        "# 基建练卡建议".to_string(),
        String::new(),
        format!("账号：{}", report.operbox_label),
        format!(
            "当前可练 {}；获取后可练 {}；暂缓体系 {}；待复核 {}；已达标 {}。",
            train.len(),
            conditional.len(),
            blocked.len(),
            review.len(),
            ready.len()
        ),
    ];
    append_answer_section(&mut lines, "当前可练", &train);
    append_answer_section(&mut lines, "获取后可练", &conditional);
    append_answer_section(&mut lines, "暂缓体系", &blocked);
    append_answer_section(&mut lines, "待复核", &review);
    append_answer_section(&mut lines, "已达标", &ready);
    lines.join("\n") + "\n"
}

fn append_answer_section(lines: &mut Vec<String>, title: &str, facts: &[&TrainingAdviceFact]) {
    lines.push(String::new());
    lines.push(format!("## {title}（{}）", facts.len()));
    lines.push(String::new());
    if facts.is_empty() {
        lines.push("- 无".to_string());
        return;
    }
    lines.extend(facts.iter().map(|fact| format!("- {}", fact.text)));
}

fn build_fact_skeleton(report: &TrainingAdviceReport) -> Vec<TrainingAdviceFact> {
    let mut facts = Vec::new();
    for item in report
        .now
        .iter()
        .chain(&report.conditional)
        .chain(&report.ready)
        .chain(&report.review)
    {
        facts.push(operator_fact(item));
    }
    for blocked in &report.blocked {
        facts.push(blocked_fact(blocked));
    }
    facts
}

fn operator_fact(item: &OperatorAdviceItem) -> TrainingAdviceFact {
    let labels = unique_strings(item.matches.iter().map(|matched| matched.label.as_str()));
    let roles = unique_strings(item.matches.iter().map(|matched| role_label(matched.role)));
    let current = item
        .current
        .as_ref()
        .map(format_state)
        .unwrap_or_else(|| "未拥有".to_string());
    let action = match item.action {
        RecommendationAction::Train => "当前可直接培养。",
        RecommendationAction::AcquireThenTrain => "获取后再培养。",
        RecommendationAction::Ready => "已达到目标练度。",
        RecommendationAction::Review => "命中的规则待人工复核，不能作为确定推荐。",
        RecommendationAction::Blocked => "当前规则尚未准入。",
    };
    let review_note = if item.needs_review && item.action != RecommendationAction::Review {
        "另有待复核规则命中，不能据此改写当前行动。"
    } else {
        ""
    };
    let plan_notes = unique_strings(
        item.matches
            .iter()
            .filter_map(|matched| matched.plan_note.as_deref()),
    );
    let plan_note = if plan_notes.is_empty() {
        String::new()
    } else {
        format!("条件计划：{}", plan_notes.join("；"))
    };
    let text = format!(
        "{:?}：{}，当前{}，目标{}。所属：{}；角色：{}。{}{}{}",
        item.display_priority,
        item.operator,
        current,
        format_state(&item.target),
        labels.join("、"),
        roles.join("、"),
        action,
        review_note,
        plan_note
    );

    TrainingAdviceFact {
        action: item.action,
        operator: Some(item.operator.clone()),
        rule_id: None,
        priority: Some(item.display_priority),
        text,
        source_refs: item.source_refs.clone(),
    }
}

fn blocked_fact(blocked: &BlockedRuleReport) -> TrainingAdviceFact {
    let mut missing = blocked.missing_core.clone();
    for group in &blocked.missing_core_groups {
        let owned = if group.owned.is_empty() {
            "无".to_string()
        } else {
            group.owned.join("、")
        };
        missing.push(format!(
            "核心组「{}」至少 {} 人（已有：{}；候选：{}）",
            group.label,
            group.required_count,
            owned,
            group.candidates.join("、")
        ));
    }
    let deferred = if blocked.deferred_members.is_empty() {
        "无".to_string()
    } else {
        blocked.deferred_members.join("、")
    };
    let (action, text) = if blocked.needs_review {
        (
            RecommendationAction::Review,
            format!(
                "{}命中待人工复核规则；按当前规则草案仍缺少：{}。已拥有且未达标的成员：{}。以上不能作为确定推荐。",
                blocked.label,
                missing.join("、"),
                deferred
            ),
        )
    } else {
        (
            RecommendationAction::Blocked,
            format!(
                "{}尚未准入；缺少：{}。已拥有且未达标、当前暂缓：{}。",
                blocked.label,
                missing.join("、"),
                deferred
            ),
        )
    };
    TrainingAdviceFact {
        action,
        operator: None,
        rule_id: Some(blocked.rule_id.clone()),
        priority: None,
        text,
        source_refs: blocked.source_refs.clone(),
    }
}

fn collect_source_requests(
    report: &TrainingAdviceReport,
) -> BTreeMap<String, (EvidenceRef, BTreeSet<String>)> {
    let mut requests = BTreeMap::new();
    for item in report
        .now
        .iter()
        .chain(&report.conditional)
        .chain(&report.ready)
        .chain(&report.review)
    {
        for source_ref in &item.source_refs {
            let (_, keywords) = source_request(&mut requests, source_ref);
            keywords.insert(item.operator.clone());
            for matched in &item.matches {
                keywords.insert(matched.label.clone());
                keywords.insert(matched.rule_id.clone());
            }
        }
    }
    for blocked in &report.blocked {
        for source_ref in &blocked.source_refs {
            let (_, keywords) = source_request(&mut requests, source_ref);
            keywords.insert(blocked.label.clone());
            keywords.insert(blocked.rule_id.clone());
            keywords.extend(blocked.owned_core.iter().cloned());
            keywords.extend(blocked.deferred_members.iter().cloned());
        }
    }
    requests
}

fn source_request<'a>(
    requests: &'a mut BTreeMap<String, (EvidenceRef, BTreeSet<String>)>,
    source_ref: &EvidenceRef,
) -> &'a mut (EvidenceRef, BTreeSet<String>) {
    requests
        .entry(source_key(source_ref))
        .or_insert_with(|| (source_ref.clone(), BTreeSet::new()))
}

fn source_key(source_ref: &EvidenceRef) -> String {
    format!(
        "{}::{}",
        source_ref.path,
        source_ref.heading.as_deref().unwrap_or("")
    )
}

fn is_markdown_source(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn resolve_workspace_source(workspace_root: &Path, source: &str) -> Option<PathBuf> {
    let root = workspace_root.canonicalize().ok()?;
    let source = Path::new(source);
    if source.is_absolute() {
        return None;
    }
    let candidate = root.join(source).canonicalize().ok()?;
    candidate.starts_with(&root).then_some(candidate)
}

fn extract_markdown_snippet(
    markdown: &str,
    requested_heading: Option<&str>,
    keywords: &[String],
) -> Option<String> {
    let lines = markdown.lines().collect::<Vec<_>>();
    let start = requested_heading
        .and_then(|heading| find_heading(&lines, heading))
        .or_else(|| find_keyword_section(&lines, keywords))?;
    let end = section_end(&lines, start);
    let excerpt = lines[start..end].join("\n").trim().to_string();
    if excerpt.is_empty() {
        return None;
    }
    Some(excerpt.chars().take(MAX_EXCERPT_CHARS).collect())
}

fn find_heading(lines: &[&str], requested: &str) -> Option<usize> {
    let requested = requested.trim();
    lines.iter().position(|line| {
        heading(line).is_some_and(|(_, title)| title.trim().eq_ignore_ascii_case(requested))
    })
}

fn find_keyword_section(lines: &[&str], keywords: &[String]) -> Option<usize> {
    let hit = lines.iter().position(|line| {
        keywords
            .iter()
            .filter(|keyword| !keyword.is_empty())
            .any(|keyword| line.contains(keyword))
    })?;
    (0..=hit)
        .rev()
        .find(|index| heading(lines[*index]).is_some())
        .or(Some(hit))
}

fn section_end(lines: &[&str], start: usize) -> usize {
    let Some((level, _)) = heading(lines[start]) else {
        return (start + 8).min(lines.len());
    };
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| {
            heading(line)
                .filter(|(next_level, _)| *next_level <= level)
                .map(|_| index)
        })
        .unwrap_or(lines.len())
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if level == 0 || !line[level..].starts_with(' ') {
        return None;
    }
    Some((level, line[level + 1..].trim()))
}

fn format_state(state: &OperatorTrainingState) -> String {
    let mut text = if state.elite == 0 {
        "E0".to_string()
    } else {
        format!("精{}", state.elite)
    };
    if let Some(level) = state.level {
        text.push_str(&format!("/{level}级"));
    }
    text
}

fn role_label(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Core => "核心",
        MemberRole::Important => "重要成员",
        MemberRole::Hanger => "挂件",
        MemberRole::Independent => "独立",
    }
}

fn unique_strings<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training_advice::{
        RecommendationPriority, RuleKind, RuleMatch, TrainingAdviceSummary,
    };

    #[test]
    fn extracts_requested_heading_without_crossing_next_peer() {
        let markdown =
            "# 文档\n\n## 目标\n\n需要的内容。\n\n### 子节\n\n细节。\n\n## 其他\n\n不能进入。\n";
        let snippet = extract_markdown_snippet(markdown, Some("目标"), &[]).unwrap();
        assert!(snippet.contains("需要的内容"));
        assert!(snippet.contains("细节"));
        assert!(!snippet.contains("不能进入"));
    }

    #[test]
    fn keyword_retrieval_starts_at_containing_section() {
        let markdown = "# 文档\n\n## 无关\n\n别的内容。\n\n## 清流\n\n清流需要精一。\n";
        let snippet = extract_markdown_snippet(markdown, None, &["清流".to_string()]).unwrap();
        assert!(snippet.starts_with("## 清流"));
        assert!(snippet.contains("清流需要精一"));
        assert!(!snippet.contains("别的内容"));
    }

    #[test]
    fn workspace_source_rejects_absolute_and_parent_escape() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        assert!(resolve_workspace_source(root, "/etc/passwd").is_none());
        assert!(resolve_workspace_source(root, "../Cargo.toml").is_none());
        assert!(resolve_workspace_source(
            root,
            "docs/ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md"
        )
        .is_some());
    }

    #[test]
    fn rag_input_contains_only_report_facts_and_sources() {
        let source_ref = EvidenceRef {
            path: "/outside/secret.md".to_string(),
            heading: None,
        };
        let target = OperatorTrainingState {
            elite: 1,
            level: None,
        };
        let item = OperatorAdviceItem {
            operator: "清流".to_string(),
            action: RecommendationAction::Train,
            display_priority: RecommendationPriority::P0,
            current: Some(OperatorTrainingState {
                elite: 0,
                level: Some(1),
            }),
            target: target.clone(),
            matches: vec![RuleMatch {
                rule_id: "clear_stream".to_string(),
                kind: RuleKind::Standalone,
                label: "清流散件".to_string(),
                role: MemberRole::Independent,
                priority: RecommendationPriority::P0,
                target,
                benefit: None,
                source_refs: vec![source_ref.clone()],
                needs_review: false,
                plan_note: Some("核心组仍需 1 人。".to_string()),
            }],
            source_refs: vec![source_ref.clone()],
            needs_review: false,
        };
        let report = TrainingAdviceReport {
            schema_version: 2,
            operbox_label: "test".to_string(),
            summary: TrainingAdviceSummary {
                owned: 1,
                modelled_owned: 1,
                now_count: 1,
                conditional_count: 0,
                blocked_count: 0,
                review_count: 0,
            },
            now: vec![item],
            conditional: Vec::new(),
            blocked: Vec::new(),
            ready: Vec::new(),
            review: Vec::new(),
            source_refs: vec![source_ref.clone()],
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();

        let rag_input = build_training_advice_rag_input(&report, root);
        assert_eq!(rag_input.fact_skeleton.len(), 1);
        assert_eq!(rag_input.fact_skeleton[0].operator.as_deref(), Some("清流"));
        assert!(rag_input.fact_skeleton[0].text.contains("当前可直接培养"));
        assert!(rag_input.fact_skeleton[0].text.contains("核心组仍需 1 人"));
        assert!(rag_input.evidence_snippets.is_empty());
        assert_eq!(rag_input.unavailable_source_refs.len(), 1);
        assert_eq!(rag_input.unavailable_source_refs[0].path, source_ref.path);

        let answer = render_training_advice_answer(&report);
        assert!(answer.contains("## 当前可练（1）"));
        assert!(answer.contains("清流"));
        assert!(answer.contains("## 获取后可练（0）\n\n- 无"));
        assert!(!answer.contains("/outside/secret.md"));
    }

    #[test]
    fn needs_review_blocked_fact_is_not_a_deterministic_blocked_claim() {
        let fact = blocked_fact(&BlockedRuleReport {
            rule_id: "review_rule".to_string(),
            kind: RuleKind::System,
            label: "待复核体系".to_string(),
            missing_core: vec!["核心".to_string()],
            missing_core_groups: Vec::new(),
            owned_core: Vec::new(),
            deferred_members: vec!["挂件".to_string()],
            conditional_acquire: Vec::new(),
            source_refs: Vec::new(),
            needs_review: true,
        });
        assert_eq!(fact.action, RecommendationAction::Review);
        assert!(fact.text.contains("待人工复核"));
        assert!(fact.text.contains("不能作为确定推荐"));
        assert!(!fact.text.contains("尚未准入"));
    }
}
