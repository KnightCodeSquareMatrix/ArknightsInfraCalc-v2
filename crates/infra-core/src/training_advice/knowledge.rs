use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::skill_table::data_path;
use crate::skill_table::{default_skill_table_path, SkillTable};
use crate::tier::PromotionTier;

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
    #[serde(default)]
    pub rarity: Option<u8>,
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
    #[serde(default)]
    pub scale: CombinationScale,
    pub members: Vec<CombinationMember>,
    #[serde(default)]
    pub dynamic_skill_members: Vec<DynamicSkillMembers>,
    pub completion: CompletionSpec,
    #[serde(default)]
    pub evidence: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSkillMembers {
    pub skill_name_contains: String,
    pub facility: String,
    pub role: KnowledgeRole,
    #[serde(default)]
    pub choice_group_id: Option<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
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
    #[serde(default)]
    pub alternatives: Vec<CompletionAlternative>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionAlternative {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombinationScale {
    #[default]
    Small,
    System,
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
    let mut knowledge: TrainingAdviceKnowledge = serde_json::from_str(&raw).map_err(|error| {
        Error::msg(format!(
            "training advice knowledge parse {}: {error}",
            path.display()
        ))
    })?;
    if knowledge
        .combinations
        .iter()
        .any(|entry| !entry.dynamic_skill_members.is_empty())
    {
        let instances = OperatorInstances::load(&default_instances_path()?)?;
        let table = SkillTable::load(&default_skill_table_path()?)?;
        expand_dynamic_skill_members(&mut knowledge, &instances, &table)?;
    }
    validate_training_advice_knowledge(&knowledge)?;
    Ok(knowledge)
}

fn expand_dynamic_skill_members(
    knowledge: &mut TrainingAdviceKnowledge,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<()> {
    for entry in &mut knowledge.combinations {
        for selector in entry.dynamic_skill_members.clone() {
            let mut candidates = BTreeMap::<String, PromotionTier>::new();
            for (_, instance) in instances.iter() {
                let Some(binding) = instance.facilities.get(&selector.facility) else {
                    continue;
                };
                if !binding.buff_ids.iter().any(|buff_id| {
                    table.get(buff_id).is_some_and(|skill| {
                        skill.skill_name.contains(&selector.skill_name_contains)
                    })
                }) {
                    continue;
                }
                let Some(tier) = PromotionTier::parse(&instance.tier) else {
                    continue;
                };
                candidates
                    .entry(instance.name.clone())
                    .and_modify(|current| {
                        if tier == PromotionTier::Tier0 {
                            *current = tier;
                        }
                    })
                    .or_insert(tier);
            }

            for (operator, tier) in candidates {
                if selector.exclude.contains(&operator) {
                    continue;
                }
                if !entry
                    .members
                    .iter()
                    .any(|member| member.operator == operator)
                {
                    entry.members.push(CombinationMember {
                        operator: operator.clone(),
                        role: selector.role,
                        target: match tier {
                            PromotionTier::Tier0 => KnowledgeTarget {
                                kind: TargetKind::Explicit,
                                elite: Some(0),
                                level: None,
                            },
                            PromotionTier::TierUp => KnowledgeTarget {
                                kind: TargetKind::DeriveFromSkillBinding,
                                elite: None,
                                level: None,
                            },
                        },
                    });
                }
                if let Some(group_id) = selector.choice_group_id.as_deref() {
                    let group = entry
                        .completion
                        .choice_groups
                        .iter_mut()
                        .chain(
                            entry
                                .completion
                                .alternatives
                                .iter_mut()
                                .flat_map(|alternative| alternative.choice_groups.iter_mut()),
                        )
                        .find(|group| group.id == group_id)
                        .ok_or_else(|| {
                            Error::msg(format!(
                                "combination {} dynamic selector references unknown choice group {}",
                                entry.id, group_id
                            ))
                        })?;
                    if !group.members.contains(&operator) {
                        group.members.push(operator);
                    }
                }
            }
        }
    }
    Ok(())
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
    for selector in &entry.dynamic_skill_members {
        validate_name(
            &selector.skill_name_contains,
            "dynamic skill member selector",
        )?;
        validate_name(&selector.facility, "dynamic skill member facility")?;
        for operator in &selector.exclude {
            validate_name(operator, "dynamic skill member exclusion")?;
        }
    }

    validate_completion_shape(
        entry,
        &entry.completion.required,
        &entry.completion.choice_groups,
        &member_names,
        "base",
    )?;
    let mut alternative_ids = HashSet::new();
    for (index, alternative) in entry.completion.alternatives.iter().enumerate() {
        validate_completion_shape(
            entry,
            &alternative.required,
            &alternative.choice_groups,
            &member_names,
            &format!("alternative {index}"),
        )?;
        if alternative.required.is_empty() && alternative.choice_groups.is_empty() {
            return Err(Error::msg(format!(
                "combination {} alternative {index} must not be empty",
                entry.id
            )));
        }
        for group in &alternative.choice_groups {
            if !alternative_ids.insert(group.id.as_str()) {
                return Err(Error::msg(format!(
                    "combination {} repeats choice group {}",
                    entry.id, group.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_completion_shape(
    entry: &CombinationKnowledgeEntry,
    required: &[String],
    groups: &[ChoiceGroup],
    member_names: &HashSet<&str>,
    label: &str,
) -> Result<()> {
    let mut completion_names = HashSet::new();
    for member in required {
        validate_completion_member(entry, member, member_names)?;
        if !completion_names.insert(member.as_str()) {
            return Err(Error::msg(format!(
                "combination {} {label} repeats completion member {member}",
                entry.id
            )));
        }
    }
    let mut group_ids = HashSet::new();
    for group in groups {
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
            validate_completion_member(entry, member, member_names)?;
            if !group_members.insert(member.as_str()) || !completion_names.insert(member.as_str()) {
                return Err(Error::msg(format!(
                    "combination {} {label} repeats choice member {member}",
                    entry.id
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
