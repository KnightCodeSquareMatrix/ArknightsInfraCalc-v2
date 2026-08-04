use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::skill_table::data_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceKnowledge {
    pub schema_version: u32,
    #[serde(default)]
    pub newbie: Vec<NewbieKnowledgeEntry>,
    #[serde(default)]
    pub combinations: Vec<CombinationKnowledgeEntry>,
    #[serde(default)]
    pub standalone: Vec<StandaloneKnowledgeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewbieKnowledgeEntry {
    pub operator: String,
    pub product: ProductBucket,
    pub target: KnowledgeTarget,
    #[serde(default)]
    pub acquisition: Option<AcquisitionInfo>,
    #[serde(default)]
    pub evidence: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinationKnowledgeEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub product: Option<ProductBucket>,
    #[serde(default)]
    pub consumer_products: Vec<ProductBucket>,
    #[serde(default)]
    pub tier: Option<CombinationTier>,
    pub facilities: Vec<String>,
    pub members: Vec<CombinationMember>,
    pub completion: CompletionSpec,
    #[serde(default)]
    pub evidence: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinationMember {
    pub operator: String,
    pub role: KnowledgeRole,
    pub target: KnowledgeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionSpec {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub choice_groups: Vec<ChoiceGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceGroup {
    pub id: String,
    pub label: String,
    pub minimum: usize,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandaloneKnowledgeEntry {
    pub id: String,
    pub operator: String,
    pub product: ProductBucket,
    pub target: KnowledgeTarget,
    #[serde(default)]
    pub efficiency: Option<EfficiencyFact>,
    #[serde(default)]
    pub conditions: Vec<KnowledgeCondition>,
    #[serde(default)]
    pub evidence: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeTarget {
    pub kind: TargetKind,
    #[serde(default)]
    pub elite: Option<u8>,
    #[serde(default)]
    pub level: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Explicit,
    NoRequirement,
    DeriveFromSkillBinding,
    NeedsReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductBucket {
    Trade,
    Gold,
    Experience,
    GeneralManufacturing,
    OriginiumShards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRole {
    Core,
    Important,
    Secondary,
    Hanger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombinationTier {
    HighEfficiency,
    LowEfficiency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquisitionInfo {
    pub kind: AcquisitionKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionKind {
    Shop,
    PublicRecruitment,
    Event,
    RedeemCode,
    IntegratedStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfficiencyFact {
    pub value: f64,
    pub unit: EfficiencyUnit,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EfficiencyUnit {
    OrderPercent,
    ProductionPercent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCondition {
    pub kind: ConditionKind,
    pub key: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    Layout,
    Facility,
    SameStation,
    Dormitory,
    EngineeringRobots,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSource {
    pub path: String,
    #[serde(default)]
    pub heading: Option<String>,
}

pub fn default_training_advice_knowledge_path() -> Result<PathBuf> {
    data_path("training_advice_knowledge.json")
}

pub fn load_training_advice_knowledge(path: &Path) -> Result<TrainingAdviceKnowledge> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        Error::msg(format!(
            "training advice knowledge read {}: {error}",
            path.display()
        ))
    })?;
    let knowledge: TrainingAdviceKnowledge = serde_json::from_str(&raw).map_err(|error| {
        Error::msg(format!(
            "training advice knowledge parse {}: {error}",
            path.display()
        ))
    })?;
    validate_training_advice_knowledge(&knowledge)?;
    Ok(knowledge)
}

pub fn validate_training_advice_knowledge(knowledge: &TrainingAdviceKnowledge) -> Result<()> {
    if knowledge.schema_version != 1 {
        return Err(Error::msg(format!(
            "training advice knowledge schema_version must be 1, got {}",
            knowledge.schema_version
        )));
    }

    let mut ids = HashSet::new();
    for entry in &knowledge.newbie {
        validate_name(&entry.operator, "newbie operator")?;
        validate_explicit_target(&entry.target, "newbie target")?;
        validate_acquisition(entry.acquisition.as_ref(), "newbie acquisition")?;
        validate_sources(&entry.evidence, "newbie evidence")?;
    }
    for entry in &knowledge.combinations {
        validate_id(&mut ids, &entry.id, "combination")?;
        validate_name(&entry.name, "combination name")?;
        if entry.facilities.is_empty()
            || entry.facilities.iter().any(|value| value.trim().is_empty())
        {
            return Err(Error::msg(format!(
                "combination {} facilities must contain a non-empty value",
                entry.id
            )));
        }
        validate_combination(entry)?;
        validate_sources(&entry.evidence, "combination evidence")?;
    }
    for entry in &knowledge.standalone {
        validate_id(&mut ids, &entry.id, "standalone")?;
        validate_name(&entry.operator, "standalone operator")?;
        validate_explicit_target(&entry.target, "standalone target")?;
        if let Some(efficiency) = &entry.efficiency {
            if !efficiency.value.is_finite() || efficiency.value < 0.0 {
                return Err(Error::msg(format!(
                    "standalone {} efficiency value must be finite and non-negative",
                    entry.id
                )));
            }
        }
        for condition in &entry.conditions {
            validate_name(&condition.key, "condition key")?;
            validate_name(&condition.description, "condition description")?;
        }
        validate_sources(&entry.evidence, "standalone evidence")?;
    }
    Ok(())
}

fn validate_combination(entry: &CombinationKnowledgeEntry) -> Result<()> {
    if entry.members.is_empty() {
        return Err(Error::msg(format!(
            "combination {} must contain members",
            entry.id
        )));
    }
    let mut member_names = HashSet::new();
    for member in &entry.members {
        validate_name(&member.operator, "combination member")?;
        if !member_names.insert(member.operator.as_str()) {
            return Err(Error::msg(format!(
                "combination {} has duplicate member {}",
                entry.id, member.operator
            )));
        }
        validate_target(&member.target, "combination member target")?;
    }

    let mut completion_names = HashSet::new();
    for required in &entry.completion.required {
        validate_completion_member(entry, required, &member_names)?;
        if !completion_names.insert(required.as_str()) {
            return Err(Error::msg(format!(
                "combination {} repeats completion member {}",
                entry.id, required
            )));
        }
    }
    let mut group_ids = HashSet::new();
    for group in &entry.completion.choice_groups {
        validate_id(&mut group_ids, &group.id, "choice group")?;
        validate_name(&group.label, "choice group label")?;
        if group.minimum == 0 || group.minimum > group.members.len() {
            return Err(Error::msg(format!(
                "combination {} choice group {} has invalid minimum {}",
                entry.id, group.id, group.minimum
            )));
        }
        let mut group_members = HashSet::new();
        for member in &group.members {
            validate_completion_member(entry, member, &member_names)?;
            if !group_members.insert(member.as_str()) || !completion_names.insert(member.as_str()) {
                return Err(Error::msg(format!(
                    "combination {} repeats choice member {}",
                    entry.id, member
                )));
            }
        }
    }
    Ok(())
}

fn validate_completion_member(
    entry: &CombinationKnowledgeEntry,
    member: &str,
    members: &HashSet<&str>,
) -> Result<()> {
    validate_name(member, "completion member")?;
    if !members.contains(member) {
        return Err(Error::msg(format!(
            "combination {} completion references unknown member {}",
            entry.id, member
        )));
    }
    Ok(())
}

fn validate_target(target: &KnowledgeTarget, context: &str) -> Result<()> {
    match target.kind {
        TargetKind::Explicit => validate_explicit_target(target, context),
        TargetKind::NoRequirement
        | TargetKind::DeriveFromSkillBinding
        | TargetKind::NeedsReview => {
            if target.elite.is_some() || target.level.is_some() {
                return Err(Error::msg(format!(
                    "{context} with {:?} kind must not declare elite or level",
                    target.kind
                )));
            }
            Ok(())
        }
    }
}

fn validate_explicit_target(target: &KnowledgeTarget, context: &str) -> Result<()> {
    let elite = target
        .elite
        .ok_or_else(|| Error::msg(format!("{context} explicit target requires elite")))?;
    if elite > 2 {
        return Err(Error::msg(format!("{context} elite must be 0, 1, or 2")));
    }
    if target.level.is_some_and(|level| level == 0) {
        return Err(Error::msg(format!("{context} level must be positive")));
    }
    Ok(())
}

fn validate_acquisition(acquisition: Option<&AcquisitionInfo>, context: &str) -> Result<()> {
    if let Some(acquisition) = acquisition {
        validate_name(&acquisition.detail, context)?;
    }
    Ok(())
}

fn validate_sources(sources: &[KnowledgeSource], context: &str) -> Result<()> {
    for source in sources {
        validate_name(&source.path, context)?;
        let path = Path::new(&source.path);
        if path.has_root()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(Error::msg(format!(
                "{context} path must be a relative repository path: {}",
                source.path
            )));
        }
        if source
            .heading
            .as_ref()
            .is_some_and(|heading| heading.trim().is_empty())
        {
            return Err(Error::msg(format!("{context} heading must not be empty")));
        }
    }
    Ok(())
}

fn validate_id(seen: &mut HashSet<String>, id: &str, context: &str) -> Result<()> {
    validate_name(id, context)?;
    if !seen.insert(id.to_string()) {
        return Err(Error::msg(format!("duplicate {context} id: {id}")));
    }
    Ok(())
}

fn validate_name(value: &str, context: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::msg(format!("{context} must not be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_knowledge_is_v1_and_valid() {
        let knowledge =
            load_training_advice_knowledge(&default_training_advice_knowledge_path().unwrap())
                .unwrap();
        assert_eq!(knowledge.schema_version, 1);
        assert!(!knowledge.newbie.is_empty());
        assert!(!knowledge.combinations.is_empty());
        assert!(!knowledge.standalone.is_empty());
    }

    #[test]
    fn explicit_targets_require_a_valid_elite() {
        let mut target = KnowledgeTarget {
            kind: TargetKind::Explicit,
            elite: None,
            level: None,
        };
        assert!(validate_target(&target, "target").is_err());
        target.elite = Some(3);
        assert!(validate_target(&target, "target").is_err());
        target.elite = Some(1);
        target.level = Some(0);
        assert!(validate_target(&target, "target").is_err());
    }

    #[test]
    fn combination_completion_must_reference_unique_members() {
        let mut knowledge = sample_knowledge();
        knowledge.combinations[0].completion.required = vec!["不存在".to_string()];
        assert!(validate_training_advice_knowledge(&knowledge).is_err());

        knowledge = sample_knowledge();
        knowledge.combinations[0].completion.required = vec!["甲".to_string(), "甲".to_string()];
        assert!(validate_training_advice_knowledge(&knowledge).is_err());
    }

    #[test]
    fn absolute_and_parent_source_paths_are_rejected() {
        let mut knowledge = sample_knowledge();
        knowledge.newbie[0].evidence[0].path = "/outside.md".to_string();
        assert!(validate_training_advice_knowledge(&knowledge).is_err());

        knowledge = sample_knowledge();
        knowledge.newbie[0].evidence[0].path = "../outside.md".to_string();
        assert!(validate_training_advice_knowledge(&knowledge).is_err());

        knowledge = sample_knowledge();
        knowledge.newbie[0].evidence[0].path = "C:outside.md".to_string();
        assert!(validate_training_advice_knowledge(&knowledge).is_err());
    }

    fn sample_knowledge() -> TrainingAdviceKnowledge {
        TrainingAdviceKnowledge {
            schema_version: 1,
            newbie: vec![NewbieKnowledgeEntry {
                operator: "新手".to_string(),
                product: ProductBucket::Trade,
                target: KnowledgeTarget {
                    kind: TargetKind::Explicit,
                    elite: Some(0),
                    level: None,
                },
                acquisition: None,
                evidence: vec![KnowledgeSource {
                    path: "docs/example.md".to_string(),
                    heading: None,
                }],
            }],
            combinations: vec![CombinationKnowledgeEntry {
                id: "combo".to_string(),
                name: "组合".to_string(),
                product: Some(ProductBucket::Trade),
                consumer_products: Vec::new(),
                tier: Some(CombinationTier::HighEfficiency),
                facilities: vec!["trade_station".to_string()],
                members: vec![CombinationMember {
                    operator: "甲".to_string(),
                    role: KnowledgeRole::Core,
                    target: KnowledgeTarget {
                        kind: TargetKind::Explicit,
                        elite: Some(2),
                        level: None,
                    },
                }],
                completion: CompletionSpec {
                    required: vec!["甲".to_string()],
                    choice_groups: Vec::new(),
                },
                evidence: Vec::new(),
            }],
            standalone: vec![StandaloneKnowledgeEntry {
                id: "standalone".to_string(),
                operator: "散件".to_string(),
                product: ProductBucket::OriginiumShards,
                target: KnowledgeTarget {
                    kind: TargetKind::Explicit,
                    elite: Some(1),
                    level: None,
                },
                efficiency: Some(EfficiencyFact {
                    value: 30.0,
                    unit: EfficiencyUnit::ProductionPercent,
                    note: None,
                }),
                conditions: Vec::new(),
                evidence: Vec::new(),
            }],
        }
    }
}
