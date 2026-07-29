use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::instances::OperatorInstances;
use crate::operbox::OperBox;
use crate::tier::PromotionTier;
use crate::Result;

use super::types::{
    AcquisitionMode, BlockedRuleReport, EvidenceRef, MissingCoreGroupReport, OperatorAdviceItem,
    OperatorTrainingState, RecommendationAction, ReviewStatus, RuleKind, RuleMatch, RuleMember,
    TrainingAdviceOptions, TrainingAdviceReport, TrainingAdviceSummary,
    TrainingRecommendationRules, TrainingRule, TrainingTarget,
};

pub fn build_training_advice(
    operbox: &OperBox,
    instances: &OperatorInstances,
    rules: &TrainingRecommendationRules,
    options: &TrainingAdviceOptions,
) -> Result<TrainingAdviceReport> {
    let mut confirmed: BTreeMap<String, OperatorAdviceItem> = BTreeMap::new();
    let mut review: BTreeMap<String, OperatorAdviceItem> = BTreeMap::new();
    let mut blocked = Vec::new();

    for rule in &rules.rules {
        let outcome = evaluate_rule(operbox, rules, rule);
        for item in outcome
            .now
            .into_iter()
            .chain(outcome.conditional)
            .chain(outcome.ready)
        {
            merge_item(&mut confirmed, item);
        }
        for item in outcome.review {
            merge_item(&mut review, item);
        }
        blocked.extend(outcome.blocked);
    }

    for operator in review.keys().cloned().collect::<Vec<_>>() {
        if let Some(existing) = confirmed.get_mut(&operator) {
            let review_item = review
                .remove(&operator)
                .expect("review key came from the same map");
            merge_review_context(existing, &review_item);
        }
    }

    let mut now = BTreeMap::new();
    let mut conditional = BTreeMap::new();
    let mut ready = BTreeMap::new();
    for (operator, item) in confirmed {
        match item.action {
            RecommendationAction::Train => {
                now.insert(operator, item);
            }
            RecommendationAction::AcquireThenTrain => {
                conditional.insert(operator, item);
            }
            RecommendationAction::Ready => {
                ready.insert(operator, item);
            }
            RecommendationAction::Review | RecommendationAction::Blocked => {}
        }
    }

    let now = sorted_items(now);
    let conditional = sorted_items(conditional);
    let ready = sorted_items(ready);
    let review = sorted_items(review);
    blocked.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));

    let mut source_refs = BTreeMap::new();
    for item in now.iter().chain(&conditional).chain(&ready).chain(&review) {
        collect_evidence(&mut source_refs, &item.source_refs);
    }
    for rule in &blocked {
        collect_evidence(&mut source_refs, &rule.source_refs);
    }

    Ok(TrainingAdviceReport {
        schema_version: 2,
        operbox_label: options
            .operbox_label
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        summary: TrainingAdviceSummary {
            owned: operbox.owned_count(),
            modelled_owned: modelled_owned_count(operbox, instances),
            now_count: now.len(),
            conditional_count: conditional.len(),
            blocked_count: blocked.len(),
            review_count: review.len(),
        },
        now,
        conditional,
        blocked,
        ready,
        review,
        source_refs: source_refs.into_values().collect(),
    })
}

struct RuleOutcome {
    now: Vec<OperatorAdviceItem>,
    conditional: Vec<OperatorAdviceItem>,
    ready: Vec<OperatorAdviceItem>,
    review: Vec<OperatorAdviceItem>,
    blocked: Vec<BlockedRuleReport>,
}

fn evaluate_rule(
    operbox: &OperBox,
    rules: &TrainingRecommendationRules,
    rule: &TrainingRule,
) -> RuleOutcome {
    let needs_review = rule.review.status == ReviewStatus::NeedsReview;
    let hard_admission = matches!(rule.kind, RuleKind::System | RuleKind::Combo);
    let admission = if hard_admission {
        evaluate_admission(operbox, rule)
    } else {
        AdmissionState {
            admitted: true,
            owned_core: Vec::new(),
            missing_core: Vec::new(),
            missing_core_groups: Vec::new(),
        }
    };

    if hard_admission && !admission.admitted {
        return blocked_outcome(operbox, rules, rule, &admission, needs_review);
    }

    let mut outcome = empty_outcome();
    for member in &rule.members {
        if let Some(item) = evaluate_member(operbox, rules, rule, member, needs_review, None) {
            push_by_action(&mut outcome, item);
        }
    }
    outcome
}

fn blocked_outcome(
    operbox: &OperBox,
    rules: &TrainingRecommendationRules,
    rule: &TrainingRule,
    admission: &AdmissionState,
    needs_review: bool,
) -> RuleOutcome {
    let mut deferred = BTreeSet::new();
    let mut conditional_acquire = BTreeSet::new();
    let mut outcome = empty_outcome();
    let owns_any_member = rule.members.iter().any(|member| {
        let Some(progress) = operbox.progress_of(&member.operator) else {
            return false;
        };
        if !member.target.is_met_by(progress) {
            deferred.insert(member.operator.clone());
        }
        true
    });
    let follow_up = deferred.iter().cloned().collect::<Vec<_>>();

    for missing in &admission.missing_core {
        if let Some(member) = find_member(rule, missing) {
            if can_suggest_acquire(operbox, rules, member) {
                conditional_acquire.insert(missing.clone());
                let plan = Some(conditional_plan(&format!("缺失核心 {missing}"), &follow_up));
                if let Some(item) =
                    evaluate_member(operbox, rules, rule, member, needs_review, plan)
                {
                    push_by_action(&mut outcome, item);
                }
            }
        }
    }

    for group in &admission.missing_core_groups {
        let missing_count = group.required_count.saturating_sub(group.owned.len());
        for candidate in &group.candidates {
            if operbox.owns(candidate) {
                continue;
            }
            let Some(member) = find_member(rule, candidate) else {
                continue;
            };
            if can_suggest_acquire(operbox, rules, member) {
                conditional_acquire.insert(candidate.clone());
                let plan = Some(conditional_plan(
                    &format!(
                        "核心组「{}」候选 {}（仍需 {} 人）",
                        group.label, candidate, missing_count
                    ),
                    &follow_up,
                ));
                if let Some(item) =
                    evaluate_member(operbox, rules, rule, member, needs_review, plan)
                {
                    push_by_action(&mut outcome, item);
                }
            }
        }
    }

    if owns_any_member || !conditional_acquire.is_empty() {
        outcome.blocked.push(BlockedRuleReport {
            rule_id: rule.id.clone(),
            kind: rule.kind,
            label: rule.label.clone(),
            missing_core: admission.missing_core.clone(),
            missing_core_groups: admission.missing_core_groups.clone(),
            owned_core: admission.owned_core.clone(),
            deferred_members: deferred.into_iter().collect(),
            conditional_acquire: conditional_acquire.into_iter().collect(),
            source_refs: rule.evidence.clone(),
            needs_review,
        });
    }
    outcome
}

fn conditional_plan(missing: &str, follow_up: &[String]) -> String {
    if follow_up.is_empty() {
        return format!("先获取并培养{missing}。");
    }
    format!(
        "先获取并培养{missing}，完成后再培养已拥有且未达标的 {}。",
        follow_up.join("、")
    )
}

fn evaluate_member(
    operbox: &OperBox,
    rules: &TrainingRecommendationRules,
    rule: &TrainingRule,
    member: &RuleMember,
    needs_review: bool,
    plan_note: Option<String>,
) -> Option<OperatorAdviceItem> {
    let match_item = RuleMatch {
        rule_id: rule.id.clone(),
        kind: rule.kind,
        label: rule.label.clone(),
        role: member.role,
        priority: member.priority,
        target: target_state(&member.target),
        benefit: member.benefit.clone(),
        source_refs: rule.evidence.clone(),
        needs_review,
        plan_note,
    };

    match operbox.progress_of(&member.operator) {
        Some(progress) if member.target.is_met_by(progress) => {
            let action = if needs_review {
                RecommendationAction::Review
            } else {
                RecommendationAction::Ready
            };
            Some(item(
                member,
                action,
                Some(progress.into()),
                match_item,
                rule,
                needs_review,
            ))
        }
        Some(progress) => {
            let action = if needs_review {
                RecommendationAction::Review
            } else {
                RecommendationAction::Train
            };
            Some(item(
                member,
                action,
                Some(progress.into()),
                match_item,
                rule,
                needs_review,
            ))
        }
        None => {
            if !can_suggest_acquire(operbox, rules, member) {
                return None;
            }
            let action = if needs_review {
                RecommendationAction::Review
            } else {
                RecommendationAction::AcquireThenTrain
            };
            Some(item(member, action, None, match_item, rule, needs_review))
        }
    }
}

fn item(
    member: &RuleMember,
    action: RecommendationAction,
    current: Option<OperatorTrainingState>,
    match_item: RuleMatch,
    rule: &TrainingRule,
    needs_review: bool,
) -> OperatorAdviceItem {
    OperatorAdviceItem {
        operator: member.operator.clone(),
        action,
        display_priority: member.priority,
        current,
        target: target_state(&member.target),
        matches: vec![match_item],
        source_refs: rule.evidence.clone(),
        needs_review,
    }
}

fn push_by_action(outcome: &mut RuleOutcome, item: OperatorAdviceItem) {
    match item.action {
        RecommendationAction::Train => outcome.now.push(item),
        RecommendationAction::AcquireThenTrain => outcome.conditional.push(item),
        RecommendationAction::Ready => outcome.ready.push(item),
        RecommendationAction::Review => outcome.review.push(item),
        RecommendationAction::Blocked => {}
    }
}

fn empty_outcome() -> RuleOutcome {
    RuleOutcome {
        now: Vec::new(),
        conditional: Vec::new(),
        ready: Vec::new(),
        review: Vec::new(),
        blocked: Vec::new(),
    }
}

struct AdmissionState {
    admitted: bool,
    owned_core: Vec<String>,
    missing_core: Vec<String>,
    missing_core_groups: Vec<MissingCoreGroupReport>,
}

fn evaluate_admission(operbox: &OperBox, rule: &TrainingRule) -> AdmissionState {
    let mut owned_core = BTreeSet::new();
    let mut missing_core = BTreeSet::new();
    let mut missing_core_groups = Vec::new();

    for name in &rule.admission.required_core {
        if operbox.owns(name) {
            owned_core.insert(name.clone());
        } else {
            missing_core.insert(name.clone());
        }
    }

    for slot in &rule.admission.pick_one_core {
        let owned: Vec<_> = slot
            .candidates
            .iter()
            .filter(|c| operbox.owns(c))
            .cloned()
            .collect();
        if owned.is_empty() {
            missing_core_groups.push(MissingCoreGroupReport {
                label: slot.label.clone(),
                required_count: 1,
                owned: Vec::new(),
                candidates: slot.candidates.clone(),
            });
        } else {
            owned_core.extend(owned);
        }
    }

    for group in &rule.admission.required_core_groups {
        let owned: Vec<_> = group
            .candidates
            .iter()
            .filter(|candidate| operbox.owns(candidate))
            .cloned()
            .collect();
        owned_core.extend(owned.iter().cloned());
        if owned.len() < group.required_count {
            missing_core_groups.push(MissingCoreGroupReport {
                label: group.label.clone(),
                required_count: group.required_count,
                owned,
                candidates: group.candidates.clone(),
            });
        }
    }

    AdmissionState {
        admitted: missing_core.is_empty() && missing_core_groups.is_empty(),
        owned_core: owned_core.into_iter().collect(),
        missing_core: missing_core.into_iter().collect(),
        missing_core_groups,
    }
}

fn find_member<'a>(rule: &'a TrainingRule, name: &str) -> Option<&'a RuleMember> {
    rule.members.iter().find(|m| m.operator == name)
}

fn can_suggest_acquire(
    operbox: &OperBox,
    rules: &TrainingRecommendationRules,
    member: &RuleMember,
) -> bool {
    match member.acquisition {
        AcquisitionMode::OwnedOnly => false,
        AcquisitionMode::SuggestAcquire => true,
        AcquisitionMode::Policy => {
            if rules
                .acquisition_policy
                .named_exceptions
                .iter()
                .any(|n| n == &member.operator)
            {
                return true;
            }
            let rarity = member.rarity.or_else(|| {
                operbox
                    .entries
                    .iter()
                    .find(|e| e.name == member.operator)
                    .map(|e| e.rarity)
                    .filter(|&r| r > 0)
            });
            matches!(rarity, Some(r) if r <= rules.acquisition_policy.default_rarity_le)
        }
    }
}

fn target_state(target: &TrainingTarget) -> OperatorTrainingState {
    OperatorTrainingState {
        elite: target.elite,
        level: target.level,
    }
}

fn collect_evidence(map: &mut BTreeMap<String, EvidenceRef>, refs: &[EvidenceRef]) {
    for r in refs {
        let key = format!("{}::{}", r.path, r.heading.as_deref().unwrap_or(""));
        map.entry(key).or_insert_with(|| r.clone());
    }
}

fn merge_item(map: &mut BTreeMap<String, OperatorAdviceItem>, incoming: OperatorAdviceItem) {
    map.entry(incoming.operator.clone())
        .and_modify(|existing| merge_operator_item(existing, &incoming))
        .or_insert(incoming);
}

fn merge_operator_item(existing: &mut OperatorAdviceItem, incoming: &OperatorAdviceItem) {
    let matches = merge_matches(&existing.matches, &incoming.matches);
    let source_refs = merge_evidence(&existing.source_refs, &incoming.source_refs);
    let needs_review = existing.needs_review || incoming.needs_review;
    let current = existing
        .current
        .clone()
        .or_else(|| incoming.current.clone());

    let better_action = action_rank(incoming.action) < action_rank(existing.action);
    let better_priority = incoming.display_priority.rank() < existing.display_priority.rank();

    if better_action || (incoming.action == existing.action && better_priority) {
        existing.action = incoming.action;
        existing.target = incoming.target.clone();
        if incoming.current.is_some() {
            existing.current = incoming.current.clone();
        }
    }

    if better_priority {
        existing.display_priority = incoming.display_priority;
    }

    existing.matches = matches;
    existing.source_refs = source_refs;
    existing.needs_review = needs_review;
    if existing.current.is_none() {
        existing.current = current;
    }
}

fn merge_review_context(existing: &mut OperatorAdviceItem, review: &OperatorAdviceItem) {
    existing.matches = merge_matches(&existing.matches, &review.matches);
    existing.source_refs = merge_evidence(&existing.source_refs, &review.source_refs);
    existing.needs_review = true;
}

fn action_rank(action: RecommendationAction) -> u8 {
    match action {
        RecommendationAction::Train => 0,
        RecommendationAction::AcquireThenTrain => 1,
        RecommendationAction::Review => 2,
        RecommendationAction::Ready => 3,
        RecommendationAction::Blocked => 4,
    }
}

fn merge_matches(a: &[RuleMatch], b: &[RuleMatch]) -> Vec<RuleMatch> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in a.iter().chain(b.iter()) {
        let key = format!(
            "{}:{}:{}",
            m.rule_id,
            format!("{:?}", m.role),
            format!("{:?}", m.priority)
        );
        if seen.insert(key) {
            out.push(m.clone());
        }
    }
    out
}

fn merge_evidence(a: &[EvidenceRef], b: &[EvidenceRef]) -> Vec<EvidenceRef> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in a.iter().chain(b.iter()) {
        let key = format!("{}::{}", r.path, r.heading.as_deref().unwrap_or(""));
        if seen.insert(key) {
            out.push(r.clone());
        }
    }
    out
}

fn sorted_items(map: BTreeMap<String, OperatorAdviceItem>) -> Vec<OperatorAdviceItem> {
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| {
        a.display_priority
            .rank()
            .cmp(&b.display_priority.rank())
            .then_with(|| a.operator.cmp(&b.operator))
    });
    out
}

fn modelled_owned_count(operbox: &OperBox, instances: &OperatorInstances) -> usize {
    operbox
        .owned
        .keys()
        .filter(|name| {
            instances.get(name, PromotionTier::Tier0).is_some()
                || instances.get(name, PromotionTier::TierUp).is_some()
        })
        .count()
}

#[cfg(test)]
mod tests {
    use crate::operbox::{OperBox, OperBoxEntry};
    use crate::{instances::default_instances_path, OperatorInstances};

    use super::super::types::*;
    use super::*;

    fn entry(name: &str, elite: u8, own: bool, rarity: u8) -> OperBoxEntry {
        OperBoxEntry {
            id: name.to_string(),
            name: name.to_string(),
            elite,
            level: 1,
            own,
            potential: 0,
            rarity,
        }
    }

    fn operbox(entries: Vec<OperBoxEntry>) -> OperBox {
        OperBox::from_entries(entries).unwrap()
    }

    fn member(
        name: &str,
        role: MemberRole,
        elite: u8,
        priority: RecommendationPriority,
        acquisition: AcquisitionMode,
        rarity: Option<u8>,
    ) -> RuleMember {
        RuleMember {
            operator: name.to_string(),
            role,
            target: TrainingTarget {
                elite,
                level: None,
                skill_id: None,
                skill_name: None,
            },
            priority,
            acquisition,
            rarity,
            benefit: None,
        }
    }

    fn sample_rules() -> TrainingRecommendationRules {
        TrainingRecommendationRules {
            version: 2,
            acquisition_policy: AcquisitionPolicy {
                default_rarity_le: 4,
                named_exceptions: vec!["苍苔".to_string()],
            },
            rules: vec![
                TrainingRule {
                    id: "standalone_clear".to_string(),
                    kind: RuleKind::Standalone,
                    scope: RuleScope::Independent,
                    label: "清流散件".to_string(),
                    source_system_id: None,
                    admission: RuleAdmission::default(),
                    members: vec![member(
                        "清流",
                        MemberRole::Independent,
                        1,
                        RecommendationPriority::P0,
                        AcquisitionMode::Policy,
                        Some(4),
                    )],
                    evidence: vec![EvidenceRef {
                        path: "docs/散件.md".to_string(),
                        heading: Some("清流".to_string()),
                    }],
                    review: RuleReview::default(),
                },
                TrainingRule {
                    id: "witch_long_beta".to_string(),
                    kind: RuleKind::System,
                    scope: RuleScope::SameStation,
                    label: "巫恋裁缝核".to_string(),
                    source_system_id: Some("witch_long_beta".to_string()),
                    admission: RuleAdmission {
                        required_core: vec!["巫恋".to_string(), "龙舌兰".to_string()],
                        pick_one_core: vec![PickOneCoreSlot {
                            label: "裁缝β第三人".to_string(),
                            candidates: vec!["卡夫卡".to_string(), "柏喙".to_string()],
                        }],
                        required_core_groups: Vec::new(),
                    },
                    members: vec![
                        member(
                            "巫恋",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P0,
                            AcquisitionMode::OwnedOnly,
                            Some(5),
                        ),
                        member(
                            "龙舌兰",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P0,
                            AcquisitionMode::OwnedOnly,
                            Some(5),
                        ),
                        member(
                            "卡夫卡",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P0,
                            AcquisitionMode::OwnedOnly,
                            Some(5),
                        ),
                        member(
                            "柏喙",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P0,
                            AcquisitionMode::OwnedOnly,
                            Some(5),
                        ),
                    ],
                    evidence: vec![EvidenceRef {
                        path: "docs/巫恋.md".to_string(),
                        heading: None,
                    }],
                    review: RuleReview::default(),
                },
                TrainingRule {
                    id: "combo_with_hanger".to_string(),
                    kind: RuleKind::Combo,
                    scope: RuleScope::SameStation,
                    label: "能天使蕾缪安二人组".to_string(),
                    source_system_id: None,
                    admission: RuleAdmission {
                        required_core: vec!["能天使".to_string(), "蕾缪安".to_string()],
                        pick_one_core: Vec::new(),
                        required_core_groups: Vec::new(),
                    },
                    members: vec![
                        member(
                            "能天使",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P1,
                            AcquisitionMode::OwnedOnly,
                            Some(6),
                        ),
                        member(
                            "蕾缪安",
                            MemberRole::Core,
                            2,
                            RecommendationPriority::P1,
                            AcquisitionMode::OwnedOnly,
                            Some(6),
                        ),
                        member(
                            "芬",
                            MemberRole::Hanger,
                            1,
                            RecommendationPriority::P2,
                            AcquisitionMode::Policy,
                            Some(3),
                        ),
                    ],
                    evidence: vec![],
                    review: RuleReview::default(),
                },
                TrainingRule {
                    id: "soft_hongyun".to_string(),
                    kind: RuleKind::SoftCombo,
                    scope: RuleScope::Independent,
                    label: "红云回收利用".to_string(),
                    source_system_id: None,
                    admission: RuleAdmission::default(),
                    members: vec![member(
                        "红云",
                        MemberRole::Independent,
                        1,
                        RecommendationPriority::P1,
                        AcquisitionMode::Policy,
                        Some(4),
                    )],
                    evidence: vec![],
                    review: RuleReview::default(),
                },
                TrainingRule {
                    id: "review_case".to_string(),
                    kind: RuleKind::Standalone,
                    scope: RuleScope::Independent,
                    label: "待复核".to_string(),
                    source_system_id: None,
                    admission: RuleAdmission::default(),
                    members: vec![member(
                        "槐琥",
                        MemberRole::Independent,
                        2,
                        RecommendationPriority::P1,
                        AcquisitionMode::OwnedOnly,
                        Some(5),
                    )],
                    evidence: vec![EvidenceRef {
                        path: "vault/review.md".to_string(),
                        heading: None,
                    }],
                    review: RuleReview {
                        status: ReviewStatus::NeedsReview,
                        conflicts: vec!["conflicting source".to_string()],
                    },
                },
            ],
        }
    }

    fn report(box_: OperBox, rules: TrainingRecommendationRules) -> TrainingAdviceReport {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        build_training_advice(
            &box_,
            &instances,
            &rules,
            &TrainingAdviceOptions {
                operbox_label: Some("test".to_string()),
            },
        )
        .unwrap()
    }

    #[test]
    fn partial_combo_does_not_train_owned_half_core() {
        let report = report(operbox(vec![entry("龙舌兰", 0, true, 5)]), sample_rules());
        assert!(report
            .blocked
            .iter()
            .any(|b| b.rule_id == "witch_long_beta"));
        assert!(report.now.iter().all(|r| r.operator != "龙舌兰"));
    }

    #[test]
    fn untouched_hard_rules_are_not_reported_as_blocked() {
        let report = report(operbox(vec![]), sample_rules());
        assert!(report.blocked.is_empty());
        assert!(report
            .source_refs
            .iter()
            .all(|source| source.path != "docs/巫恋.md"));
    }

    #[test]
    fn complete_combo_undertrained_emits_train() {
        let report = report(
            operbox(vec![
                entry("巫恋", 1, true, 5),
                entry("龙舌兰", 2, true, 5),
                entry("卡夫卡", 1, true, 5),
            ]),
            sample_rules(),
        );
        assert!(report.now.iter().any(|r| {
            r.operator == "巫恋"
                && r.display_priority == RecommendationPriority::P0
                && r.action == RecommendationAction::Train
        }));
    }

    #[test]
    fn hanger_not_trained_when_core_missing() {
        let report = report(operbox(vec![entry("芬", 0, true, 3)]), sample_rules());
        assert!(report.now.iter().all(|r| r.operator != "芬"));
        assert!(report
            .blocked
            .iter()
            .any(|b| b.rule_id == "combo_with_hanger"
                && b.deferred_members.iter().any(|n| n == "芬")));
    }

    #[test]
    fn hanger_trained_when_core_complete() {
        let report = report(
            operbox(vec![
                entry("能天使", 2, true, 6),
                entry("蕾缪安", 2, true, 6),
                entry("芬", 0, true, 3),
            ]),
            sample_rules(),
        );
        assert!(report
            .now
            .iter()
            .any(|r| r.operator == "芬" && r.action == RecommendationAction::Train));
    }

    #[test]
    fn low_star_unowned_suggests_acquire() {
        let report = report(operbox(vec![]), sample_rules());
        assert!(report.conditional.iter().any(|r| {
            r.operator == "清流" && r.action == RecommendationAction::AcquireThenTrain
        }));
    }

    #[test]
    fn soft_combo_independent_of_teammates() {
        let report = report(operbox(vec![entry("红云", 0, true, 4)]), sample_rules());
        assert!(report
            .now
            .iter()
            .any(|r| r.operator == "红云" && r.action == RecommendationAction::Train));
    }

    #[test]
    fn multi_rule_keeps_all_matches_and_best_priority() {
        let mut rules = sample_rules();
        rules.rules.push(TrainingRule {
            id: "clear_as_system".to_string(),
            kind: RuleKind::System,
            scope: RuleScope::Independent,
            label: "清流体系".to_string(),
            source_system_id: None,
            admission: RuleAdmission {
                required_core: vec!["清流".to_string()],
                pick_one_core: Vec::new(),
                required_core_groups: Vec::new(),
            },
            members: vec![member(
                "清流",
                MemberRole::Core,
                2,
                RecommendationPriority::P1,
                AcquisitionMode::OwnedOnly,
                Some(4),
            )],
            evidence: vec![],
            review: RuleReview::default(),
        });
        let report = report(operbox(vec![entry("清流", 0, true, 4)]), rules);
        let recs: Vec<_> = report.now.iter().filter(|r| r.operator == "清流").collect();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].display_priority, RecommendationPriority::P0);
        assert!(recs[0]
            .matches
            .iter()
            .any(|m| m.rule_id == "standalone_clear"));
        assert!(recs[0]
            .matches
            .iter()
            .any(|m| m.rule_id == "clear_as_system"));
    }

    #[test]
    fn cross_bucket_merge_keeps_ready_and_train_matches() {
        let mut rules = sample_rules();
        rules.rules.push(TrainingRule {
            id: "clear_upgrade".to_string(),
            kind: RuleKind::System,
            scope: RuleScope::Independent,
            label: "清流升级".to_string(),
            source_system_id: None,
            admission: RuleAdmission {
                required_core: vec!["清流".to_string()],
                pick_one_core: Vec::new(),
                required_core_groups: Vec::new(),
            },
            members: vec![member(
                "清流",
                MemberRole::Core,
                2,
                RecommendationPriority::P1,
                AcquisitionMode::OwnedOnly,
                Some(4),
            )],
            evidence: vec![],
            review: RuleReview::default(),
        });

        let report = report(operbox(vec![entry("清流", 1, true, 4)]), rules);
        let item = report
            .now
            .iter()
            .find(|item| item.operator == "清流")
            .unwrap();
        assert_eq!(item.action, RecommendationAction::Train);
        assert_eq!(item.display_priority, RecommendationPriority::P0);
        assert_eq!(item.target.elite, 2);
        assert_eq!(item.matches.len(), 2);
        assert!(item.matches.iter().any(|m| m.target.elite == 1));
        assert!(item.matches.iter().any(|m| m.target.elite == 2));
        assert!(report.ready.iter().all(|item| item.operator != "清流"));
    }

    #[test]
    fn needs_review_goes_to_review_bucket() {
        let report = report(operbox(vec![entry("槐琥", 1, true, 5)]), sample_rules());
        assert!(report
            .review
            .iter()
            .any(|r| r.operator == "槐琥" && r.needs_review));
        assert!(report.now.iter().all(|r| r.operator != "槐琥"));
    }

    #[test]
    fn confirmed_result_absorbs_review_context_without_changing_action() {
        let mut rules = sample_rules();
        rules.rules.push(TrainingRule {
            id: "confirmed_huaihu".to_string(),
            kind: RuleKind::Standalone,
            scope: RuleScope::Independent,
            label: "槐琥散件".to_string(),
            source_system_id: None,
            admission: RuleAdmission::default(),
            members: vec![member(
                "槐琥",
                MemberRole::Independent,
                1,
                RecommendationPriority::P2,
                AcquisitionMode::OwnedOnly,
                Some(5),
            )],
            evidence: vec![],
            review: RuleReview::default(),
        });

        let report = report(operbox(vec![entry("槐琥", 1, true, 5)]), rules);
        let item = report
            .ready
            .iter()
            .find(|item| item.operator == "槐琥")
            .unwrap();
        assert_eq!(item.action, RecommendationAction::Ready);
        assert_eq!(item.target.elite, 1);
        assert!(item.needs_review);
        assert_eq!(item.matches.len(), 2);
        assert!(report.review.iter().all(|item| item.operator != "槐琥"));
    }

    #[test]
    fn missing_core_with_suggest_acquire_is_conditional() {
        let mut rules = sample_rules();
        // make 柏喙 suggest_acquire as low-star stand-in for test
        for m in &mut rules.rules[1].members {
            if m.operator == "柏喙" {
                m.acquisition = AcquisitionMode::SuggestAcquire;
                m.rarity = Some(4);
            }
        }
        let report = report(
            operbox(vec![entry("巫恋", 2, true, 5), entry("龙舌兰", 2, true, 5)]),
            rules,
        );
        assert!(report
            .blocked
            .iter()
            .any(|b| b.rule_id == "witch_long_beta"));
        assert!(report
            .conditional
            .iter()
            .any(|r| r.operator == "柏喙" && r.action == RecommendationAction::AcquireThenTrain));
        assert!(report.now.iter().all(|r| r.operator != "巫恋"));
    }

    #[test]
    fn required_core_group_enforces_n_of_m_admission() {
        let rules = TrainingRecommendationRules {
            version: 2,
            acquisition_policy: AcquisitionPolicy::default(),
            rules: vec![TrainingRule {
                id: "red_pine".to_string(),
                kind: RuleKind::System,
                scope: RuleScope::CrossStation,
                label: "红松林".to_string(),
                source_system_id: None,
                admission: RuleAdmission {
                    required_core: vec!["焰尾".to_string(), "薇薇安娜".to_string()],
                    pick_one_core: Vec::new(),
                    required_core_groups: vec![RequiredCoreGroup {
                        label: "红松制造成员".to_string(),
                        candidates: vec![
                            "灰毫".to_string(),
                            "远牙".to_string(),
                            "野鬃".to_string(),
                        ],
                        required_count: 2,
                    }],
                },
                members: vec![
                    member(
                        "焰尾",
                        MemberRole::Core,
                        2,
                        RecommendationPriority::P0,
                        AcquisitionMode::OwnedOnly,
                        Some(6),
                    ),
                    member(
                        "薇薇安娜",
                        MemberRole::Core,
                        2,
                        RecommendationPriority::P0,
                        AcquisitionMode::OwnedOnly,
                        Some(6),
                    ),
                    member(
                        "灰毫",
                        MemberRole::Core,
                        2,
                        RecommendationPriority::P0,
                        AcquisitionMode::OwnedOnly,
                        Some(5),
                    ),
                    member(
                        "远牙",
                        MemberRole::Core,
                        2,
                        RecommendationPriority::P0,
                        AcquisitionMode::OwnedOnly,
                        Some(6),
                    ),
                    member(
                        "野鬃",
                        MemberRole::Core,
                        2,
                        RecommendationPriority::P0,
                        AcquisitionMode::OwnedOnly,
                        Some(5),
                    ),
                ],
                evidence: Vec::new(),
                review: RuleReview::default(),
            }],
        };

        let blocked = report(
            operbox(vec![
                entry("焰尾", 2, true, 6),
                entry("薇薇安娜", 2, true, 6),
                entry("灰毫", 1, true, 5),
            ]),
            rules.clone(),
        );
        assert!(blocked.now.is_empty());
        let missing = &blocked.blocked[0].missing_core_groups[0];
        assert_eq!(missing.required_count, 2);
        assert_eq!(missing.owned, vec!["灰毫"]);

        let admitted = report(
            operbox(vec![
                entry("焰尾", 2, true, 6),
                entry("薇薇安娜", 2, true, 6),
                entry("灰毫", 1, true, 5),
                entry("远牙", 1, true, 6),
            ]),
            rules,
        );
        assert!(admitted.blocked.is_empty());
        assert!(admitted.now.iter().any(|item| item.operator == "灰毫"));
        assert!(admitted.now.iter().any(|item| item.operator == "远牙"));
    }

    #[test]
    fn required_core_group_conditional_plan_preserves_missing_count() {
        let rules = TrainingRecommendationRules {
            version: 2,
            acquisition_policy: AcquisitionPolicy::default(),
            rules: vec![TrainingRule {
                id: "low_star_group".to_string(),
                kind: RuleKind::Combo,
                scope: RuleScope::SameStation,
                label: "低星核心组".to_string(),
                source_system_id: None,
                admission: RuleAdmission {
                    required_core: Vec::new(),
                    pick_one_core: Vec::new(),
                    required_core_groups: vec![RequiredCoreGroup {
                        label: "三选二".to_string(),
                        candidates: vec!["甲".to_string(), "乙".to_string(), "丙".to_string()],
                        required_count: 2,
                    }],
                },
                members: ["甲", "乙", "丙"]
                    .into_iter()
                    .map(|name| {
                        member(
                            name,
                            MemberRole::Core,
                            1,
                            RecommendationPriority::P1,
                            AcquisitionMode::Policy,
                            Some(3),
                        )
                    })
                    .collect(),
                evidence: Vec::new(),
                review: RuleReview::default(),
            }],
        };

        let empty = report(operbox(Vec::new()), rules.clone());
        assert_eq!(empty.conditional.len(), 3);
        assert!(empty
            .conditional
            .iter()
            .all(|item| item.matches.iter().any(|matched| {
                matched
                    .plan_note
                    .as_deref()
                    .is_some_and(|note| note.contains("仍需 2 人"))
            })));

        let partial = report(operbox(vec![entry("甲", 0, true, 3)]), rules);
        assert_eq!(partial.conditional.len(), 2);
        assert!(partial
            .conditional
            .iter()
            .all(|item| item.matches.iter().any(|matched| {
                matched
                    .plan_note
                    .as_deref()
                    .is_some_and(|note| note.contains("仍需 1 人"))
            })));
    }

    #[test]
    fn report_schema_version_is_two() {
        let report = report(operbox(vec![entry("清流", 0, true, 4)]), sample_rules());
        assert_eq!(report.schema_version, 2);
    }

    #[test]
    fn default_rules_load_and_filter_standalone_fixture() {
        let box_ = OperBox::from_json(include_str!(
            "../../../../data/fixtures/training_advice/standalone_e1_four_star.json"
        ))
        .unwrap();
        let rules = crate::training_advice::load_training_recommendation_rules(
            &crate::training_advice::default_training_recommendations_path().unwrap(),
        )
        .unwrap();
        assert_eq!(rules.version, 2);
        let report = report(box_, rules);
        let actual: std::collections::HashMap<_, _> = report
            .now
            .iter()
            .map(|rec| (rec.operator.as_str(), rec.display_priority))
            .collect();
        for name in [
            "石英",
            "清流",
            "砾",
            "断罪者",
            "Castle-3",
            "缠丸",
            "安比尔",
            "斑点",
            "霜叶",
            "白雪",
            "空弦",
            "吉星",
            "槐琥",
        ] {
            assert!(actual.contains_key(name), "missing {name} in {:?}", actual);
        }
        let ready: std::collections::HashSet<_> = report
            .ready
            .iter()
            .map(|rec| rec.operator.as_str())
            .collect();
        for name in ["慕斯", "红豆"] {
            assert!(ready.contains(name), "missing {name} in ready {:?}", ready);
            assert!(
                !actual.contains_key(name),
                "{name} should not be in now.train"
            );
        }
    }
}
