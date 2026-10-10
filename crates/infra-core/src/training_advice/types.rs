use serde::{Deserialize, Serialize};

use crate::roster::OperatorProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RecommendationPriority {
    P0,
    P1,
    P2,
    Info,
}

impl RecommendationPriority {
    pub fn rank(self) -> u8 {
        match self {
            Self::P0 => 0,
            Self::P1 => 1,
            Self::P2 => 2,
            Self::Info => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    System,
    Combo,
    Standalone,
    SoftCombo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    SameStation,
    CrossStation,
    ControlCenter,
    Independent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    Core,
    Important,
    Hanger,
    Independent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AcquisitionMode {
    OwnedOnly,
    SuggestAcquire,
    #[default]
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ReviewStatus {
    #[default]
    Confirmed,
    NeedsReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationAction {
    Train,
    AcquireThenTrain,
    Ready,
    Blocked,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingTarget {
    pub elite: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
}

impl TrainingTarget {
    pub fn is_met_by(&self, progress: OperatorProgress) -> bool {
        if progress.elite > self.elite {
            return true;
        }
        if progress.elite < self.elite {
            return false;
        }
        self.level.is_none_or(|level| progress.level >= level)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorTrainingState {
    pub elite: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
}

impl From<OperatorProgress> for OperatorTrainingState {
    fn from(value: OperatorProgress) -> Self {
        Self {
            elite: value.elite,
            level: Some(value.level),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickOneCoreSlot {
    pub label: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredCoreGroup {
    pub label: String,
    pub candidates: Vec<String>,
    pub required_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleAdmission {
    #[serde(default)]
    pub required_core: Vec<String>,
    #[serde(default)]
    pub pick_one_core: Vec<PickOneCoreSlot>,
    #[serde(default)]
    pub required_core_groups: Vec<RequiredCoreGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberBenefit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMember {
    pub operator: String,
    pub role: MemberRole,
    pub target: TrainingTarget,
    pub priority: RecommendationPriority,
    #[serde(default)]
    pub acquisition: AcquisitionMode,
    /// Optional rarity hint for acquisition policy when the operator is unowned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rarity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benefit: Option<MemberBenefit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleReview {
    #[serde(default)]
    pub status: ReviewStatus,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRule {
    pub id: String,
    pub kind: RuleKind,
    pub scope: RuleScope,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_system_id: Option<String>,
    #[serde(default)]
    pub admission: RuleAdmission,
    pub members: Vec<RuleMember>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub review: RuleReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquisitionPolicy {
    #[serde(default = "default_rarity_le")]
    pub default_rarity_le: u8,
    #[serde(default)]
    pub named_exceptions: Vec<String>,
}

fn default_rarity_le() -> u8 {
    4
}

impl Default for AcquisitionPolicy {
    fn default() -> Self {
        Self {
            default_rarity_le: default_rarity_le(),
            named_exceptions: vec!["苍苔".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecommendationRules {
    pub version: u32,
    #[serde(default)]
    pub acquisition_policy: AcquisitionPolicy,
    #[serde(default)]
    pub rules: Vec<TrainingRule>,
}

#[derive(Debug, Clone, Default)]
pub struct TrainingAdviceOptions {
    pub operbox_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceReport {
    pub schema_version: u32,
    pub operbox_label: String,
    pub summary: TrainingAdviceSummary,
    pub now: Vec<OperatorAdviceItem>,
    pub conditional: Vec<OperatorAdviceItem>,
    pub blocked: Vec<BlockedRuleReport>,
    pub ready: Vec<OperatorAdviceItem>,
    pub review: Vec<OperatorAdviceItem>,
    pub source_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceSummary {
    pub owned: usize,
    pub modelled_owned: usize,
    pub now_count: usize,
    pub conditional_count: usize,
    pub blocked_count: usize,
    pub review_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorAdviceItem {
    pub operator: String,
    pub action: RecommendationAction,
    pub display_priority: RecommendationPriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<OperatorTrainingState>,
    pub target: OperatorTrainingState,
    pub matches: Vec<RuleMatch>,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub needs_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMatch {
    pub rule_id: String,
    pub kind: RuleKind,
    pub label: String,
    pub role: MemberRole,
    pub priority: RecommendationPriority,
    pub target: OperatorTrainingState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benefit: Option<MemberBenefit>,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub needs_review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedRuleReport {
    pub rule_id: String,
    pub kind: RuleKind,
    pub label: String,
    pub missing_core: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_core_groups: Vec<MissingCoreGroupReport>,
    pub owned_core: Vec<String>,
    #[serde(default)]
    pub deferred_members: Vec<String>,
    #[serde(default)]
    pub conditional_acquire: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub needs_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingCoreGroupReport {
    pub label: String,
    pub required_count: usize,
    pub owned: Vec<String>,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceBundle {
    pub report: TrainingAdviceReport,
    pub rag_input: TrainingAdviceRagInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceRagInput {
    pub schema_version: u32,
    pub fact_skeleton: Vec<TrainingAdviceFact>,
    pub evidence_snippets: Vec<EvidenceSnippet>,
    pub unavailable_source_refs: Vec<EvidenceRef>,
    pub guardrails: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAdviceFact {
    pub action: RecommendationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<RecommendationPriority>,
    pub text: String,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceSnippet {
    pub source_ref: EvidenceRef,
    pub excerpt: String,
}
