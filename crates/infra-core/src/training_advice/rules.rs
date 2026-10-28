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
