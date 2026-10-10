use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::skill_table::data_path;

use super::types::{MemberRole, ReviewStatus, RuleKind, TrainingRecommendationRules, TrainingRule};

pub fn default_training_recommendations_path() -> Result<PathBuf> {
    data_path("training_recommendations.json")
}

pub fn load_training_recommendation_rules(path: &Path) -> Result<TrainingRecommendationRules> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        Error::msg(format!(
            "training recommendations read {}: {e}",
            path.display()
        ))
    })?;
    let rules: TrainingRecommendationRules = serde_json::from_str(&raw).map_err(|e| {
        Error::msg(format!(
            "training recommendations parse {}: {e}",
            path.display()
        ))
    })?;
    validate_rules(&rules)?;
    Ok(rules)
}

pub fn validate_rules(rules: &TrainingRecommendationRules) -> Result<()> {
    if rules.version != 2 {
        return Err(Error::msg(format!(
            "training recommendations version must be 2, got {}",
            rules.version
        )));
    }

    let mut ids = HashSet::new();
    for rule in &rules.rules {
        if !ids.insert(rule.id.clone()) {
            return Err(Error::msg(format!("duplicate rule id: {}", rule.id)));
        }
        validate_rule(rule)?;
    }
    Ok(())
}

fn validate_rule(rule: &TrainingRule) -> Result<()> {
    if rule.members.is_empty() {
        return Err(Error::msg(format!("rule {} has no members", rule.id)));
    }
    let mut member_names = HashSet::new();
    for member in &rule.members {
        if !member_names.insert(member.operator.as_str()) {
            return Err(Error::msg(format!(
                "rule {} has duplicate member {}",
                rule.id, member.operator
            )));
        }
    }

    let hard = matches!(rule.kind, RuleKind::System | RuleKind::Combo);
    if matches!(rule.kind, RuleKind::Standalone | RuleKind::SoftCombo)
        && (!rule.admission.required_core.is_empty()
            || !rule.admission.pick_one_core.is_empty()
            || !rule.admission.required_core_groups.is_empty())
    {
        return Err(Error::msg(format!(
            "rule {} kind {:?} must not declare hard core admission",
            rule.id, rule.kind
        )));
    }

    if hard
        && rule.admission.required_core.is_empty()
        && rule.admission.pick_one_core.is_empty()
        && rule.admission.required_core_groups.is_empty()
    {
        return Err(Error::msg(format!(
            "rule {} kind {:?} requires admission core",
            rule.id, rule.kind
        )));
    }

    let mut admission_names = HashSet::new();
    for name in &rule.admission.required_core {
        register_admission_name(rule, &mut admission_names, name)?;
        let member = rule.members.iter().find(|m| m.operator == *name);
        match member {
            Some(m) if matches!(m.role, MemberRole::Core) => {}
            Some(m) => {
                return Err(Error::msg(format!(
                    "rule {} required_core {} has role {:?}",
                    rule.id, name, m.role
                )));
            }
            None => {
                return Err(Error::msg(format!(
                    "rule {} required_core {} missing from members",
                    rule.id, name
                )));
            }
        }
    }

    for slot in &rule.admission.pick_one_core {
        if slot.candidates.is_empty() {
            return Err(Error::msg(format!(
                "rule {} pick_one slot {} has no candidates",
                rule.id, slot.label
            )));
        }
        for candidate in &slot.candidates {
            register_admission_name(rule, &mut admission_names, candidate)?;
        }
        validate_core_candidates(rule, "pick_one", &slot.candidates)?;
    }

    for group in &rule.admission.required_core_groups {
        if group.required_count < 2 || group.required_count > group.candidates.len() {
            return Err(Error::msg(format!(
                "rule {} required core group {} count {} invalid for {} candidates",
                rule.id,
                group.label,
                group.required_count,
                group.candidates.len()
            )));
        }
        for candidate in &group.candidates {
            register_admission_name(rule, &mut admission_names, candidate)?;
        }
        validate_core_candidates(rule, "required group", &group.candidates)?;
    }

    if rule.review.status == ReviewStatus::NeedsReview && rule.review.conflicts.is_empty() {
        return Err(Error::msg(format!(
            "rule {} needs_review but conflicts empty",
            rule.id
        )));
    }

    Ok(())
}

fn register_admission_name<'a>(
    rule: &TrainingRule,
    names: &mut HashSet<&'a str>,
    name: &'a str,
) -> Result<()> {
    if !names.insert(name) {
        return Err(Error::msg(format!(
            "rule {} repeats admission candidate {}",
            rule.id, name
        )));
    }
    Ok(())
}

fn validate_core_candidates(
    rule: &TrainingRule,
    slot_kind: &str,
    candidates: &[String],
) -> Result<()> {
    for candidate in candidates {
        let member = rule.members.iter().find(|m| m.operator == *candidate);
        match member {
            Some(member) if matches!(member.role, MemberRole::Core) => {}
            Some(member) => {
                return Err(Error::msg(format!(
                    "rule {} {} candidate {} has role {:?}",
                    rule.id, slot_kind, candidate, member.role
                )));
            }
            None => {
                return Err(Error::msg(format!(
                    "rule {} {} candidate {} missing from members",
                    rule.id, slot_kind, candidate
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training_advice::types::*;

    #[test]
    fn default_rules_are_v2_and_valid() {
        let rules =
            load_training_recommendation_rules(&default_training_recommendations_path().unwrap())
                .unwrap();
        assert_eq!(rules.version, 2);
        assert!(!rules.rules.is_empty());
    }

    #[test]
    fn natural_syracusa_members_are_not_a_training_core_rule() {
        let rules =
            load_training_recommendation_rules(&default_training_recommendations_path().unwrap())
                .unwrap();
        for forbidden in ["docus_syracusa", "syracusa_pair", "syracusa_cross_station"] {
            assert!(rules.rules.iter().all(|rule| {
                rule.id != forbidden && rule.source_system_id.as_deref() != Some(forbidden)
            }));
        }
    }

    #[test]
    fn standalone_with_hard_core_is_rejected() {
        let rules = TrainingRecommendationRules {
            version: 2,
            acquisition_policy: AcquisitionPolicy::default(),
            rules: vec![TrainingRule {
                id: "bad".to_string(),
                kind: RuleKind::Standalone,
                scope: RuleScope::Independent,
                label: "bad".to_string(),
                source_system_id: None,
                admission: RuleAdmission {
                    required_core: vec!["清流".to_string()],
                    pick_one_core: Vec::new(),
                    required_core_groups: Vec::new(),
                },
                members: vec![RuleMember {
                    operator: "清流".to_string(),
                    role: MemberRole::Core,
                    target: TrainingTarget {
                        elite: 1,
                        level: None,
                        skill_id: None,
                        skill_name: None,
                    },
                    priority: RecommendationPriority::P0,
                    acquisition: AcquisitionMode::Policy,
                    rarity: Some(4),
                    benefit: None,
                }],
                evidence: Vec::new(),
                review: RuleReview::default(),
            }],
        };
        assert!(validate_rules(&rules).is_err());
    }

    #[test]
    fn invalid_required_core_group_count_is_rejected() {
        let mut rules = TrainingRecommendationRules {
            version: 2,
            acquisition_policy: AcquisitionPolicy::default(),
            rules: vec![TrainingRule {
                id: "bad_group".to_string(),
                kind: RuleKind::System,
                scope: RuleScope::CrossStation,
                label: "bad group".to_string(),
                source_system_id: None,
                admission: RuleAdmission {
                    required_core: Vec::new(),
                    pick_one_core: Vec::new(),
                    required_core_groups: vec![RequiredCoreGroup {
                        label: "group".to_string(),
                        candidates: vec!["甲".to_string(), "乙".to_string()],
                        required_count: 3,
                    }],
                },
                members: vec![member_for_group("甲"), member_for_group("乙")],
                evidence: Vec::new(),
                review: RuleReview::default(),
            }],
        };
        assert!(validate_rules(&rules).is_err());
        rules.rules[0].admission.required_core_groups[0].required_count = 2;
        assert!(validate_rules(&rules).is_ok());
        rules.rules[0].admission.required_core_groups[0].candidates[1] = "甲".to_string();
        assert!(validate_rules(&rules).is_err());
    }

    #[test]
    fn duplicate_rule_members_are_rejected() {
        let mut rules =
            load_training_recommendation_rules(&default_training_recommendations_path().unwrap())
                .unwrap();
        let duplicate = rules.rules[0].members[0].clone();
        rules.rules[0].members.push(duplicate);
        assert!(validate_rules(&rules).is_err());
    }

    #[test]
    fn externally_reviewed_rules_are_confirmed_with_skill_targets() {
        let rules =
            load_training_recommendation_rules(&default_training_recommendations_path().unwrap())
                .unwrap();
        let expected = [
            ("blackkey_closure", "可露希尔", 2, "特别订单"),
            ("penguin_exusiai_lemuen", "能天使", 2, "物流专家"),
            ("penguin_exusiai_lemuen", "蕾缪安", 2, "相伴"),
            ("penguin_texlap_e0", "德克萨斯", 0, "恩怨"),
            ("penguin_texlap_e0", "拉普兰德", 2, "醉翁之意·β"),
        ];
        for (rule_id, operator, elite, skill_name) in expected {
            let rule = rules.rules.iter().find(|rule| rule.id == rule_id).unwrap();
            assert_eq!(rule.review.status, ReviewStatus::Confirmed);
            assert!(rule.review.conflicts.is_empty());
            let member = rule
                .members
                .iter()
                .find(|member| member.operator == operator)
                .unwrap();
            assert_eq!(member.target.elite, elite);
            assert_eq!(member.target.skill_name.as_deref(), Some(skill_name));
        }

        let expected_members = [
            ("blackkey_closure", vec!["可露希尔"]),
            ("penguin_exusiai_lemuen", vec!["能天使", "蕾缪安"]),
            ("penguin_texlap_e0", vec!["德克萨斯", "拉普兰德"]),
        ];
        for (rule_id, expected) in expected_members {
            let rule = rules.rules.iter().find(|rule| rule.id == rule_id).unwrap();
            let members = rule
                .members
                .iter()
                .map(|member| member.operator.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            let expected = expected
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(members, expected);
            assert_eq!(
                rule.admission
                    .required_core
                    .iter()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>(),
                expected
            );
            assert!(rule.admission.pick_one_core.is_empty());
            assert!(rule.admission.required_core_groups.is_empty());
        }
    }

    fn member_for_group(name: &str) -> RuleMember {
        RuleMember {
            operator: name.to_string(),
            role: MemberRole::Core,
            target: TrainingTarget {
                elite: 2,
                level: None,
                skill_id: None,
                skill_name: None,
            },
            priority: RecommendationPriority::P0,
            acquisition: AcquisitionMode::OwnedOnly,
            rarity: None,
            benefit: None,
        }
    }
}
