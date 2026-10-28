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
