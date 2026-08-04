use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::BaseBlueprint;
use crate::operbox::OperBox;

use super::knowledge::{
    AcquisitionInfo, ChoiceGroup, CombinationKnowledgeEntry, CombinationScale, CombinationTier,
    EfficiencyFact, KnowledgeCondition, KnowledgeRole, KnowledgeTarget, ProductBucket, TargetKind,
    TrainingAdviceKnowledge,
};
use super::types::OperatorTrainingState;

pub const MAX_PLAYER_RECOMMENDATIONS: usize = 10;

const PLAYER_COMBINATION_ORDER: [&str; 17] = [
    "tequila_group",
    "siracusa",
    "exusiai_group",
    "automation",
    "metalwork",
    "vermeil",
    "red_pine",
    "abyssal_hunters",
    "perception_information",
    "mizuki_standardization",
    "bubble_group",
    "rhine_lab",
    "karlan_trade",
    "glasgow",
    "pozyomka_durin",
    "penguin_logistics",
    "human_fireworks",
];

fn player_combination_order(id: &str) -> usize {
    PLAYER_COMBINATION_ORDER
        .iter()
        .position(|candidate| *candidate == id)
        .unwrap_or(usize::MAX)
}

fn is_player_combination_visible(id: &str) -> bool {
    PLAYER_COMBINATION_ORDER.contains(&id)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerAdviceContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_originium_shard_factory: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meeting_room_max_level: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormitory_level_sum: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engineering_robot_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_average_efficiency_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturing_average_efficiency_percent: Option<f64>,
}

impl PlayerAdviceContext {
    pub fn from_blueprint(blueprint: &BaseBlueprint) -> Self {
        let mut context = Self::default();
        context.apply_blueprint(blueprint);
        context
    }

    pub fn apply_blueprint(&mut self, blueprint: &BaseBlueprint) {
        self.has_originium_shard_factory = Some(blueprint.manu_line_scenario().originium_lines > 0);
        self.meeting_room_max_level = Some(blueprint.meeting_max_level());
        self.dormitory_level_sum = Some(blueprint.dorm_level_sum());
        self.engineering_robot_count = Some(blueprint.facility_level_sum_excl_meeting().min(64));
    }

    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            (
                "trade_average_efficiency_percent",
                self.trade_average_efficiency_percent,
            ),
            (
                "manufacturing_average_efficiency_percent",
                self.manufacturing_average_efficiency_percent,
            ),
        ] {
            if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(Error::msg(format!(
                    "player advice context {label} must be finite and non-negative"
                )));
            }
        }
        Ok(())
    }

    fn skips_newbie_by_efficiency(&self) -> bool {
        self.trade_average_efficiency_percent
            .is_some_and(|value| value > 30.0)
            && self
                .manufacturing_average_efficiency_percent
                .is_some_and(|value| value > 25.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerTrainingAdviceReport {
    pub schema_version: u32,
    pub context: PlayerAdviceContext,
    pub newbie_section_status: PlayerNewbieSectionStatus,
    pub incomplete_newbie: Vec<NewbieProgress>,
    pub combinations: Vec<CombinationProgress>,
    pub recommendations: Vec<PlayerRecommendation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerNewbieSectionStatus {
    Shown,
    Complete,
    SkippedByEfficiency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAction {
    Acquire,
    Train,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberProgress {
    Missing,
    OwnedNeedsTraining,
    Ready,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewbieProgress {
    pub operator: String,
    pub product: ProductBucket,
    pub action: PlayerAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<OperatorTrainingState>,
    pub target: KnowledgeTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquisition: Option<AcquisitionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombinationProgressState {
    Complete,
    NeedsTraining,
    MissingCore,
    MissingImportant,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinationProgress {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<ProductBucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumer_products: Vec<ProductBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<CombinationTier>,
    pub scale: CombinationScale,
    pub facilities: Vec<String>,
    pub state: CombinationProgressState,
    pub completed_slots: usize,
    pub total_slots: usize,
    pub completion_percent: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_core: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untrained_core: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_important: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untrained_important: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_alternative: Option<usize>,
    pub members: Vec<CombinationMemberProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinationMemberProgress {
    pub operator: String,
    pub role: KnowledgeRole,
    pub progress: MemberProgress,
    pub owned: bool,
    pub target_met: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<OperatorTrainingState>,
    pub target: KnowledgeTarget,
    pub counts_toward_completion: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerRecommendationPriority {
    NewbieFourStarEliteOne,
    FlagshipNewbie,
    OtherNewbie,
    OwnedTailor,
    AutomationMustTrain,
    SmallHighEfficiencyCore,
    SmallHighEfficiencyImportant,
    SystemSingleCoreGap,
    OtherImportant,
    LowerPriorityCore,
    HighEfficiencyStandalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerRecommendationReason {
    NewbieRequired,
    CombinationCore,
    CombinationImportant,
    Standalone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRecommendation {
    pub operator: String,
    pub action: PlayerAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<OperatorTrainingState>,
    pub target: KnowledgeTarget,
    pub priority: PlayerRecommendationPriority,
    pub priority_rank: u16,
    pub reason: PlayerRecommendationReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<ProductBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combination_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combination_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency: Option<EfficiencyFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<PlayerConditionEvaluation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquisition: Option<AcquisitionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerConditionStatus {
    Satisfied,
    Unsatisfied,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConditionEvaluation {
    pub condition: KnowledgeCondition,
    pub status: PlayerConditionStatus,
}

pub fn build_player_training_advice(
    operbox: &OperBox,
    knowledge: &TrainingAdviceKnowledge,
    context: &PlayerAdviceContext,
) -> PlayerTrainingAdviceReport {
    let incomplete_newbie = build_incomplete_newbie(operbox, knowledge);
    let newbie_section_status = if incomplete_newbie.is_empty() {
        PlayerNewbieSectionStatus::Complete
    } else if context.skips_newbie_by_efficiency() {
        PlayerNewbieSectionStatus::SkippedByEfficiency
    } else {
        PlayerNewbieSectionStatus::Shown
    };
    let mut visible_entries = knowledge
        .combinations
        .iter()
        .filter(|entry| is_player_combination_visible(&entry.id))
        .collect::<Vec<_>>();
    visible_entries.sort_by_key(|entry| player_combination_order(&entry.id));
    let combinations = visible_entries
        .iter()
        .map(|entry| build_combination_progress(operbox, entry))
        .collect::<Vec<_>>();
    let recommendations = build_recommendations(
        operbox,
        knowledge,
        &incomplete_newbie,
        &visible_entries,
        &combinations,
        context,
        newbie_section_status,
    );
    PlayerTrainingAdviceReport {
        schema_version: 2,
        context: context.clone(),
        newbie_section_status,
        incomplete_newbie,
        combinations,
        recommendations,
    }
}

fn build_incomplete_newbie(
    operbox: &OperBox,
    knowledge: &TrainingAdviceKnowledge,
) -> Vec<NewbieProgress> {
    knowledge
        .newbie
        .iter()
        .filter_map(|entry| {
            let progress = progress_for(operbox, &entry.operator, &entry.target);
            match progress {
                MemberProgress::Ready => None,
                MemberProgress::Missing | MemberProgress::OwnedNeedsTraining => {
                    Some(NewbieProgress {
                        operator: entry.operator.clone(),
                        product: entry.product,
                        action: action_for(progress),
                        current: current_progress(operbox, &entry.operator),
                        target: entry.target.clone(),
                        acquisition: (!operbox.owns(&entry.operator))
                            .then(|| entry.acquisition.clone())
                            .flatten(),
                    })
                }
                MemberProgress::NeedsReview => None,
            }
        })
        .collect()
}

fn build_combination_progress(
    operbox: &OperBox,
    entry: &CombinationKnowledgeEntry,
) -> CombinationProgress {
    let status_by_name = entry
        .members
        .iter()
        .map(|member| {
            (
                member.operator.as_str(),
                progress_for(operbox, &member.operator, &member.target),
            )
        })
        .collect::<HashMap<_, _>>();
    let selected = select_completion_shape(entry, &status_by_name);
    let selected_names = selected.names.iter().copied().collect::<HashSet<_>>();
    let completed_slots = selected
        .names
        .iter()
        .filter(|name| status_by_name.get(**name) == Some(&MemberProgress::Ready))
        .count();
    let total_slots = selected.names.len();
    let completion_percent = if total_slots == 0 {
        100
    } else {
        ((completed_slots * 100) / total_slots) as u8
    };
    let selected_members = entry
        .members
        .iter()
        .filter(|member| selected_names.contains(member.operator.as_str()))
        .collect::<Vec<_>>();
    let has_missing_core = selected_members.iter().any(|member| {
        member.role == KnowledgeRole::Core
            && status_by_name.get(member.operator.as_str()) == Some(&MemberProgress::Missing)
    });
    let has_missing_important = selected_members.iter().any(|member| {
        member.role == KnowledgeRole::Important
            && status_by_name.get(member.operator.as_str()) == Some(&MemberProgress::Missing)
    });
    let has_review = selected_members.iter().any(|member| {
        status_by_name.get(member.operator.as_str()) == Some(&MemberProgress::NeedsReview)
    });
    let has_untrained_core = selected_members.iter().any(|member| {
        member.role == KnowledgeRole::Core
            && status_by_name.get(member.operator.as_str())
                == Some(&MemberProgress::OwnedNeedsTraining)
    });
    let has_training_gap = selected_members.iter().any(|member| {
        status_by_name.get(member.operator.as_str()) == Some(&MemberProgress::OwnedNeedsTraining)
    });
    let state = if has_review {
        CombinationProgressState::NeedsReview
    } else if has_missing_core {
        CombinationProgressState::MissingCore
    } else if has_untrained_core {
        CombinationProgressState::NeedsTraining
    } else if has_missing_important {
        CombinationProgressState::MissingImportant
    } else if has_training_gap {
        CombinationProgressState::NeedsTraining
    } else {
        CombinationProgressState::Complete
    };

    let gap_names = |role: KnowledgeRole, progress: MemberProgress| {
        entry
            .members
            .iter()
            .filter(|member| {
                selected_names.contains(member.operator.as_str())
                    && member.role == role
                    && status_by_name.get(member.operator.as_str()) == Some(&progress)
            })
            .map(|member| member.operator.clone())
            .collect::<Vec<_>>()
    };

    CombinationProgress {
        id: entry.id.clone(),
        name: entry.name.clone(),
        product: entry.product,
        consumer_products: entry.consumer_products.clone(),
        tier: entry.tier,
        scale: entry.scale,
        facilities: entry.facilities.clone(),
        state,
        completed_slots,
        total_slots,
        completion_percent,
        missing_core: gap_names(KnowledgeRole::Core, MemberProgress::Missing),
        untrained_core: gap_names(KnowledgeRole::Core, MemberProgress::OwnedNeedsTraining),
        missing_important: gap_names(KnowledgeRole::Important, MemberProgress::Missing),
        untrained_important: gap_names(
            KnowledgeRole::Important,
            MemberProgress::OwnedNeedsTraining,
        ),
        selected_alternative: selected.alternative,
        members: entry
            .members
            .iter()
            .map(|member| CombinationMemberProgress {
                operator: member.operator.clone(),
                role: member.role,
                progress: status_by_name[member.operator.as_str()],
                owned: current_progress(operbox, &member.operator).is_some(),
                target_met: status_by_name[member.operator.as_str()] == MemberProgress::Ready,
                current: current_progress(operbox, &member.operator),
                target: member.target.clone(),
                counts_toward_completion: selected_names.contains(member.operator.as_str()),
            })
            .collect(),
    }
}

struct SelectedCompletion<'a> {
    alternative: Option<usize>,
    names: Vec<&'a str>,
}

fn select_completion_shape<'a>(
    entry: &'a CombinationKnowledgeEntry,
    statuses: &HashMap<&str, MemberProgress>,
) -> SelectedCompletion<'a> {
    let mut variants = Vec::new();
    if entry.completion.alternatives.is_empty() {
        variants.push((None, Vec::new(), Vec::new()));
    } else {
        for (index, alternative) in entry.completion.alternatives.iter().enumerate() {
            variants.push((
                Some(index),
                alternative.required.iter().map(String::as_str).collect(),
                alternative.choice_groups.iter().collect(),
            ));
        }
    }

    variants
        .into_iter()
        .map(|(alternative, extra_required, extra_groups)| {
            let mut names = entry
                .completion
                .required
                .iter()
                .map(String::as_str)
                .chain(extra_required)
                .collect::<Vec<_>>();
            for group in entry.completion.choice_groups.iter().chain(extra_groups) {
                names.extend(select_choice_members(group, statuses));
            }
            let ready = names
                .iter()
                .filter(|name| statuses.get(**name) == Some(&MemberProgress::Ready))
                .count();
            let owned = names
                .iter()
                .filter(|name| {
                    matches!(
                        statuses.get(**name),
                        Some(MemberProgress::Ready | MemberProgress::OwnedNeedsTraining)
                    )
                })
                .count();
            (Reverse((ready, owned)), alternative, names)
        })
        .min_by_key(|(score, alternative, _)| (*score, *alternative))
        .map(|(_, alternative, names)| SelectedCompletion { alternative, names })
        .expect("at least one completion variant")
}

fn select_choice_members<'a>(
    group: &'a ChoiceGroup,
    statuses: &HashMap<&str, MemberProgress>,
) -> Vec<&'a str> {
    let mut members = group.members.iter().map(String::as_str).collect::<Vec<_>>();
    members.sort_by_key(|name| match statuses.get(name) {
        Some(MemberProgress::Ready) => 0,
        Some(MemberProgress::OwnedNeedsTraining) => 1,
        Some(MemberProgress::Missing) => 2,
        Some(MemberProgress::NeedsReview) | None => 3,
    });
    members.truncate(group.minimum);
    members
}

fn build_recommendations(
    operbox: &OperBox,
    knowledge: &TrainingAdviceKnowledge,
    incomplete_newbie: &[NewbieProgress],
    combination_entries: &[&CombinationKnowledgeEntry],
    combinations: &[CombinationProgress],
    context: &PlayerAdviceContext,
    newbie_section_status: PlayerNewbieSectionStatus,
) -> Vec<PlayerRecommendation> {
    let mut candidates = Vec::new();
    let tequila_group_enabled = operbox.owns("巫恋") && operbox.owns("龙舌兰");
    let tequila_group_operators: HashSet<&str> = knowledge
        .combinations
        .iter()
        .find(|entry| entry.id == "tequila_group")
        .map(|entry| {
            entry
                .members
                .iter()
                .map(|member| member.operator.as_str())
                .chain(
                    entry
                        .completion
                        .choice_groups
                        .iter()
                        .flat_map(|group| group.members.iter().map(String::as_str)),
                )
                .collect()
        })
        .unwrap_or_else(|| HashSet::from(["巫恋", "龙舌兰"]));
    let tequila_candidate_blocked =
        |operator: &str| !tequila_group_enabled && tequila_group_operators.contains(operator);
    if newbie_section_status == PlayerNewbieSectionStatus::Shown {
        for (index, item) in incomplete_newbie.iter().enumerate() {
            if tequila_candidate_blocked(&item.operator) {
                continue;
            }
            let entry = knowledge
                .newbie
                .iter()
                .find(|entry| entry.operator == item.operator)
                .expect("newbie progress comes from knowledge");
            let (priority, base_rank) =
                newbie_priority(entry.rarity, &item.operator, &item.target, item.action);
            candidates.push(Candidate {
                stable_order: index,
                recommendation: PlayerRecommendation {
                    operator: item.operator.clone(),
                    action: item.action,
                    current: item.current.clone(),
                    target: item.target.clone(),
                    priority,
                    priority_rank: base_rank,
                    reason: PlayerRecommendationReason::NewbieRequired,
                    product: Some(item.product),
                    combination_id: None,
                    combination_name: None,
                    efficiency: None,
                    conditions: Vec::new(),
                    acquisition: item.acquisition.clone(),
                },
            });
        }
    }

    for (combo_index, (entry, progress)) in combination_entries.iter().zip(combinations).enumerate()
    {
        let selected = progress
            .members
            .iter()
            .filter(|member| member.counts_toward_completion)
            .collect::<Vec<_>>();
        let core_gaps = selected
            .iter()
            .filter(|member| {
                member.role == KnowledgeRole::Core
                    && matches!(
                        member.progress,
                        MemberProgress::Missing | MemberProgress::OwnedNeedsTraining
                    )
            })
            .count();
        let core_ready = core_gaps == 0;
        let important_side_owned = selected
            .iter()
            .all(|member| member.role != KnowledgeRole::Important || member.owned);

        if entry.id == "automation" && progress.state == CombinationProgressState::Complete {
            if let Some(member) = progress
                .members
                .iter()
                .find(|member| member.operator == "承曦格雷伊")
            {
                if matches!(
                    member.progress,
                    MemberProgress::Missing | MemberProgress::OwnedNeedsTraining
                ) {
                    candidates.push(Candidate {
                        stable_order: 10_000 + combo_index * 100,
                        recommendation: PlayerRecommendation {
                            operator: member.operator.clone(),
                            action: action_for(member.progress),
                            current: member.current.clone(),
                            target: member.target.clone(),
                            priority: PlayerRecommendationPriority::AutomationMustTrain,
                            priority_rank: 45,
                            reason: PlayerRecommendationReason::CombinationImportant,
                            product: entry.product,
                            combination_id: Some(entry.id.clone()),
                            combination_name: Some(entry.name.clone()),
                            efficiency: None,
                            conditions: Vec::new(),
                            acquisition: None,
                        },
                    });
                }
            }
        }

        for (member_index, member) in selected.into_iter().enumerate() {
            if tequila_candidate_blocked(&member.operator) {
                continue;
            }
            if entry.id == "bubble_group"
                && member.operator == "火神"
                && context.has_originium_shard_factory != Some(true)
            {
                continue;
            }
            if !matches!(
                member.progress,
                MemberProgress::Missing | MemberProgress::OwnedNeedsTraining
            ) {
                continue;
            }
            if member.role == KnowledgeRole::Core
                && member.progress == MemberProgress::Missing
                && (!important_side_owned || core_gaps > 2)
            {
                continue;
            }
            let tailor = is_tailor(entry, &member.operator);
            let classification = if entry.id == "human_fireworks" && member.operator == "乌有" {
                Some((PlayerRecommendationPriority::LowerPriorityCore, 75))
            } else if tailor && member.progress == MemberProgress::OwnedNeedsTraining {
                Some((PlayerRecommendationPriority::OwnedTailor, 40))
            } else if tailor {
                None
            } else if entry.tier == Some(CombinationTier::HighEfficiency)
                && entry.scale == CombinationScale::Small
                && member.role == KnowledgeRole::Core
            {
                Some((PlayerRecommendationPriority::SmallHighEfficiencyCore, 50))
            } else if entry.tier == Some(CombinationTier::HighEfficiency)
                && entry.scale == CombinationScale::Small
                && member.role == KnowledgeRole::Important
            {
                Some((
                    PlayerRecommendationPriority::SmallHighEfficiencyImportant,
                    55,
                ))
            } else if entry.scale == CombinationScale::System
                && member.role == KnowledgeRole::Core
                && core_gaps == 1
            {
                Some((PlayerRecommendationPriority::SystemSingleCoreGap, 60))
            } else if member.role == KnowledgeRole::Important && core_ready {
                Some((PlayerRecommendationPriority::OtherImportant, 70))
            } else {
                None
            };
            let Some((priority, base_rank)) = classification else {
                continue;
            };
            candidates.push(Candidate {
                stable_order: 10_000 + combo_index * 100 + member_index,
                recommendation: PlayerRecommendation {
                    operator: member.operator.clone(),
                    action: action_for(member.progress),
                    current: member.current.clone(),
                    target: member.target.clone(),
                    priority,
                    priority_rank: base_rank,
                    reason: if member.role == KnowledgeRole::Core {
                        PlayerRecommendationReason::CombinationCore
                    } else {
                        PlayerRecommendationReason::CombinationImportant
                    },
                    product: entry.product,
                    combination_id: Some(entry.id.clone()),
                    combination_name: Some(entry.name.clone()),
                    efficiency: None,
                    conditions: Vec::new(),
                    acquisition: None,
                },
            });
        }
    }

    for (index, entry) in knowledge.standalone.iter().enumerate() {
        if tequila_candidate_blocked(&entry.operator) {
            continue;
        }
        if entry.product == ProductBucket::OriginiumShards
            && context.has_originium_shard_factory != Some(true)
        {
            continue;
        }
        let progress = progress_for(operbox, &entry.operator, &entry.target);
        if !matches!(
            progress,
            MemberProgress::Missing | MemberProgress::OwnedNeedsTraining
        ) {
            continue;
        }
        let conditions = entry
            .conditions
            .iter()
            .map(|condition| PlayerConditionEvaluation {
                condition: condition.clone(),
                status: evaluate_condition(context, condition),
            })
            .collect::<Vec<_>>();
        if conditions
            .iter()
            .any(|condition| condition.status != PlayerConditionStatus::Satisfied)
        {
            continue;
        }
        candidates.push(Candidate {
            stable_order: 20_000 + index,
            recommendation: PlayerRecommendation {
                operator: entry.operator.clone(),
                action: action_for(progress),
                current: current_progress(operbox, &entry.operator),
                target: entry.target.clone(),
                priority: PlayerRecommendationPriority::HighEfficiencyStandalone,
                priority_rank: 80,
                reason: PlayerRecommendationReason::Standalone,
                product: Some(entry.product),
                combination_id: None,
                combination_name: None,
                efficiency: entry.efficiency.clone(),
                conditions,
                acquisition: None,
            },
        });
    }

    candidates.sort_by_key(|candidate| {
        (
            candidate.recommendation.priority_rank,
            candidate.stable_order,
        )
    });
    let mut merged = BTreeMap::new();
    let mut order = Vec::new();
    for candidate in candidates {
        if merged.contains_key(&candidate.recommendation.operator) {
            continue;
        }
        order.push(candidate.recommendation.operator.clone());
        merged.insert(
            candidate.recommendation.operator.clone(),
            candidate.recommendation,
        );
    }

    let mandatory = order
        .iter()
        .find(|operator| {
            merged.get(*operator).is_some_and(|item| {
                item.priority == PlayerRecommendationPriority::AutomationMustTrain
            })
        })
        .cloned();
    if let Some(mandatory) = mandatory {
        if !order[..order.len().min(MAX_PLAYER_RECOMMENDATIONS)].contains(&mandatory) {
            order.truncate(MAX_PLAYER_RECOMMENDATIONS - 1);
            order.push(mandatory);
        } else {
            order.truncate(MAX_PLAYER_RECOMMENDATIONS);
        }
    } else {
        order.truncate(MAX_PLAYER_RECOMMENDATIONS);
    }

    order
        .into_iter()
        .filter_map(|operator| merged.remove(&operator))
        .collect()
}

fn evaluate_condition(
    context: &PlayerAdviceContext,
    condition: &KnowledgeCondition,
) -> PlayerConditionStatus {
    let satisfied = match condition.key.as_str() {
        "has_originium_shard_factory" => context.has_originium_shard_factory,
        "meeting_room_max_level_at_least" => compare_u64(
            context.meeting_room_max_level.map(u64::from),
            condition.value.as_ref(),
        ),
        "dormitory_level_sum_at_least" => compare_u64(
            context.dormitory_level_sum.map(u64::from),
            condition.value.as_ref(),
        ),
        "engineering_robot_count_at_least" => compare_u64(
            context.engineering_robot_count.map(u64::from),
            condition.value.as_ref(),
        ),
        _ => None,
    };
    match satisfied {
        Some(true) => PlayerConditionStatus::Satisfied,
        Some(false) => PlayerConditionStatus::Unsatisfied,
        None => PlayerConditionStatus::Unknown,
    }
}

fn compare_u64(actual: Option<u64>, expected: Option<&serde_json::Value>) -> Option<bool> {
    Some(actual? >= expected?.as_u64()?)
}

struct Candidate {
    stable_order: usize,
    recommendation: PlayerRecommendation,
}

fn newbie_priority(
    rarity: Option<u8>,
    operator: &str,
    target: &KnowledgeTarget,
    action: PlayerAction,
) -> (PlayerRecommendationPriority, u16) {
    if action == PlayerAction::Train
        && rarity == Some(4)
        && target.kind == TargetKind::Explicit
        && target.elite == Some(1)
    {
        return (PlayerRecommendationPriority::NewbieFourStarEliteOne, 10);
    }
    if ["但书", "可露希尔", "巫恋", "龙舌兰"].contains(&operator) {
        return (PlayerRecommendationPriority::FlagshipNewbie, 20);
    }
    (PlayerRecommendationPriority::OtherNewbie, 75)
}

fn is_tailor(entry: &CombinationKnowledgeEntry, operator: &str) -> bool {
    entry
        .completion
        .choice_groups
        .iter()
        .chain(
            entry
                .completion
                .alternatives
                .iter()
                .flat_map(|alternative| alternative.choice_groups.iter()),
        )
        .any(|group| {
            group.id.contains("tailor") && group.members.iter().any(|name| name == operator)
        })
}

fn progress_for(operbox: &OperBox, operator: &str, target: &KnowledgeTarget) -> MemberProgress {
    if matches!(
        target.kind,
        TargetKind::DeriveFromSkillBinding | TargetKind::NeedsReview
    ) {
        return MemberProgress::NeedsReview;
    }
    let Some(progress) = operbox.progress_of(operator) else {
        return MemberProgress::Missing;
    };
    match target.kind {
        TargetKind::Explicit => {
            let elite = target.elite.expect("validated explicit target");
            if progress.elite > elite
                || (progress.elite == elite
                    && target.level.is_none_or(|level| progress.level >= level))
            {
                MemberProgress::Ready
            } else {
                MemberProgress::OwnedNeedsTraining
            }
        }
        TargetKind::NoRequirement => MemberProgress::Ready,
        TargetKind::DeriveFromSkillBinding | TargetKind::NeedsReview => unreachable!(),
    }
}

fn action_for(progress: MemberProgress) -> PlayerAction {
    match progress {
        MemberProgress::Missing => PlayerAction::Acquire,
        MemberProgress::OwnedNeedsTraining => PlayerAction::Train,
        MemberProgress::Ready | MemberProgress::NeedsReview => {
            unreachable!("ready/review entries are not actionable")
        }
    }
}

fn current_progress(operbox: &OperBox, operator: &str) -> Option<OperatorTrainingState> {
    operbox.progress_of(operator).map(Into::into)
}

#[cfg(test)]
mod tests {
    use crate::operbox::OperBoxEntry;

    use super::*;

    fn load_knowledge() -> TrainingAdviceKnowledge {
        super::super::load_training_advice_knowledge(
            &super::super::default_training_advice_knowledge_path().unwrap(),
        )
        .unwrap()
    }

    fn operbox(entries: &[(&str, u8, u32, u8)]) -> OperBox {
        OperBox::from_entries(
            entries
                .iter()
                .enumerate()
                .map(|(index, (name, elite, level, rarity))| OperBoxEntry {
                    id: format!("test_{index}"),
                    name: (*name).to_string(),
                    elite: *elite,
                    level: *level,
                    own: true,
                    potential: 1,
                    rarity: *rarity,
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn report_contains_three_frontend_sections_and_caps_recommendations() {
        let report = build_player_training_advice(
            &operbox(&[]),
            &load_knowledge(),
            &PlayerAdviceContext::default(),
        );
        assert!(!report.incomplete_newbie.is_empty());
        assert!(!report.combinations.is_empty());
        assert_eq!(report.recommendations.len(), MAX_PLAYER_RECOMMENDATIONS);
        assert_eq!(report.recommendations[0].operator, "但书");
        assert!(report
            .recommendations
            .iter()
            .all(|item| item.operator != "薄绿"));
    }

    #[test]
    fn player_combination_output_uses_requested_order_and_hides_legacy_groups() {
        let report = build_player_training_advice(
            &operbox(&[]),
            &load_knowledge(),
            &PlayerAdviceContext::default(),
        );
        let ids = report
            .combinations
            .iter()
            .map(|combo| combo.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "tequila_group",
                "siracusa",
                "exusiai_group",
                "automation",
                "metalwork",
                "vermeil",
                "red_pine",
                "abyssal_hunters",
                "perception_information",
                "mizuki_standardization",
                "bubble_group",
                "rhine_lab",
                "karlan_trade",
                "glasgow",
                "pozyomka_durin",
                "penguin_logistics",
                "human_fireworks",
            ]
        );
        assert!(!ids.contains(&"lungmen_control"));
        assert!(!ids.contains(&"deepcolor_ulpianus"));
        assert!(!ids.contains(&"kaltsit_omertosa"));
    }

    #[test]
    fn automation_core_ready_always_recommends_gray() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| entry.id == "automation");
        knowledge.standalone.clear();
        let report = build_player_training_advice(
            &operbox(&[
                ("温蒂", 2, 1, 6),
                ("清流", 1, 1, 4),
                ("承曦格雷伊", 1, 40, 5),
                ("冬时", 1, 1, 5),
            ]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        let gray = report
            .recommendations
            .iter()
            .find(|item| item.operator == "承曦格雷伊")
            .expect("automation should recommend gray once its core can start");
        assert_eq!(
            gray.priority,
            PlayerRecommendationPriority::AutomationMustTrain
        );
        assert_eq!(gray.priority_rank, 45);

        let not_startable = build_player_training_advice(
            &operbox(&[
                ("温蒂", 2, 1, 6),
                ("清流", 1, 1, 4),
                ("承曦格雷伊", 1, 40, 5),
            ]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(not_startable
            .recommendations
            .iter()
            .all(|item| item.priority != PlayerRecommendationPriority::AutomationMustTrain));
    }

    #[test]
    fn automation_must_train_reserves_a_slot_at_the_recommendation_cap() {
        let report = build_player_training_advice(
            &operbox(&[
                ("温蒂", 2, 1, 6),
                ("清流", 1, 1, 4),
                ("承曦格雷伊", 1, 40, 5),
                ("冬时", 1, 1, 5),
            ]),
            &load_knowledge(),
            &PlayerAdviceContext::default(),
        );

        assert_eq!(report.recommendations.len(), MAX_PLAYER_RECOMMENDATIONS);
        assert!(report
            .recommendations
            .iter()
            .any(|item| item.operator == "承曦格雷伊"
                && item.priority == PlayerRecommendationPriority::AutomationMustTrain));
    }

    #[test]
    fn equal_priority_combination_recommendations_follow_player_combination_order() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| matches!(entry.id.as_str(), "siracusa" | "karlan_trade"));
        knowledge.standalone.clear();
        let report = build_player_training_advice(
            &operbox(&[
                ("八幡海铃", 2, 1, 6),
                ("贝洛内", 2, 1, 5),
                ("银灰", 2, 1, 6),
                ("孑", 1, 1, 4),
            ]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        let names = report
            .recommendations
            .iter()
            .map(|item| item.operator.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["伺夜", "灵知"]);
        assert!(report
            .recommendations
            .iter()
            .all(|item| item.priority == PlayerRecommendationPriority::SystemSingleCoreGap));
    }

    #[test]
    fn human_fireworks_core_is_lower_priority_than_other_system_core_gaps() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| matches!(entry.id.as_str(), "siracusa" | "human_fireworks"));
        knowledge.standalone.clear();
        let report = build_player_training_advice(
            &operbox(&[
                ("八幡海铃", 2, 1, 6),
                ("贝洛内", 0, 1, 5),
                ("乌有", 0, 5, 6),
            ]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        let names = report
            .recommendations
            .iter()
            .map(|item| item.operator.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["伺夜", "乌有"]);
        let uyou = report
            .recommendations
            .iter()
            .find(|item| item.operator == "乌有")
            .unwrap();
        assert_eq!(
            uyou.priority,
            PlayerRecommendationPriority::LowerPriorityCore
        );
        assert_eq!(uyou.priority_rank, 75);
    }

    #[test]
    fn owned_tailor_is_prioritized_after_flagship_newbie() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        let report = build_player_training_advice(
            &operbox(&[("巫恋", 2, 1, 5), ("龙舌兰", 2, 1, 5), ("卡夫卡", 0, 1, 5)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        let tailor = report
            .recommendations
            .iter()
            .find(|item| item.operator == "卡夫卡")
            .unwrap();
        assert_eq!(tailor.priority, PlayerRecommendationPriority::OwnedTailor);
    }

    #[test]
    fn tequila_group_recommendations_require_both_core_operators_owned() {
        let mut knowledge = load_knowledge();
        knowledge
            .newbie
            .retain(|entry| matches!(entry.operator.as_str(), "巫恋" | "龙舌兰"));
        knowledge
            .combinations
            .retain(|combo| combo.id == "tequila_group");
        knowledge.standalone.clear();

        let one_core = build_player_training_advice(
            &operbox(&[("巫恋", 0, 1, 5), ("卡夫卡", 0, 1, 5)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(one_core.recommendations.iter().all(|item| {
            !matches!(item.operator.as_str(), "巫恋" | "龙舌兰" | "卡夫卡")
        }));
        assert!(one_core
            .incomplete_newbie
            .iter()
            .any(|item| item.operator == "龙舌兰"));
        let one_core_tequila = one_core
            .combinations
            .iter()
            .find(|combo| combo.id == "tequila_group")
            .expect("tequila group remains visible");
        assert!(one_core_tequila.members.iter().any(
            |member| member.operator == "龙舌兰" && member.progress == MemberProgress::Missing
        ));

        let other_core = build_player_training_advice(
            &operbox(&[("龙舌兰", 0, 1, 5), ("卡夫卡", 0, 1, 5)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(other_core.recommendations.iter().all(|item| {
            !matches!(item.operator.as_str(), "巫恋" | "龙舌兰" | "卡夫卡")
        }));

        let both_core = build_player_training_advice(
            &operbox(&[
                ("巫恋", 0, 1, 5),
                ("龙舌兰", 0, 1, 5),
                ("卡夫卡", 0, 1, 5),
                ("柏喙", 0, 1, 5),
            ]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(both_core
            .recommendations
            .iter()
            .any(|item| item.operator == "巫恋"));
        assert!(both_core
            .recommendations
            .iter()
            .any(|item| item.operator == "龙舌兰"));
        assert_eq!(
            both_core
                .recommendations
                .iter()
                .filter(|item| matches!(item.operator.as_str(), "卡夫卡" | "柏喙"))
                .count(),
            1
        );
    }

    #[test]
    fn originium_and_bubble_recommendations_require_known_originium_factory() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|combo| combo.id == "bubble_group");
        let box_ = operbox(&[]);
        let without = build_player_training_advice(
            &box_,
            &knowledge,
            &PlayerAdviceContext {
                has_originium_shard_factory: Some(false),
                ..PlayerAdviceContext::default()
            },
        );
        let with = build_player_training_advice(
            &box_,
            &knowledge,
            &PlayerAdviceContext {
                has_originium_shard_factory: Some(true),
                ..PlayerAdviceContext::default()
            },
        );
        assert!(without
            .recommendations
            .iter()
            .all(|item| item.operator != "薄绿" && item.operator != "火神"));
        assert!(with.recommendations.iter().any(|item| {
            item.operator == "薄绿" || item.combination_id.as_deref() == Some("bubble_group")
        }));
    }

    #[test]
    fn completed_combo_reports_member_ownership_and_training() {
        let report = build_player_training_advice(
            &operbox(&[("巫恋", 2, 1, 5), ("龙舌兰", 2, 1, 5), ("卡夫卡", 2, 1, 5)]),
            &load_knowledge(),
            &PlayerAdviceContext::default(),
        );
        let combo = report
            .combinations
            .iter()
            .find(|combo| combo.id == "tequila_group")
            .unwrap();
        assert_eq!(combo.state, CombinationProgressState::Complete);
        assert_eq!(combo.completion_percent, 100);
        assert_eq!(combo.total_slots, 3);
        assert_eq!(
            combo
                .members
                .iter()
                .find(|member| member.operator == "卡夫卡")
                .unwrap()
                .progress,
            MemberProgress::Ready
        );
    }

    #[test]
    fn t9_skips_newbie_actions_but_keeps_the_incomplete_inventory() {
        let mut knowledge = load_knowledge();
        knowledge.combinations.clear();
        knowledge.standalone.clear();
        let high_efficiency = PlayerAdviceContext {
            trade_average_efficiency_percent: Some(30.1),
            manufacturing_average_efficiency_percent: Some(25.1),
            ..PlayerAdviceContext::default()
        };
        let report = build_player_training_advice(&operbox(&[]), &knowledge, &high_efficiency);

        assert_eq!(
            report.newbie_section_status,
            PlayerNewbieSectionStatus::SkippedByEfficiency
        );
        assert!(!report.incomplete_newbie.is_empty());
        assert!(report.recommendations.is_empty());

        let at_threshold = build_player_training_advice(
            &operbox(&[]),
            &knowledge,
            &PlayerAdviceContext {
                trade_average_efficiency_percent: Some(30.0),
                manufacturing_average_efficiency_percent: Some(25.0),
                ..PlayerAdviceContext::default()
            },
        );
        assert_eq!(
            at_threshold.newbie_section_status,
            PlayerNewbieSectionStatus::Shown
        );
        assert_eq!(
            at_threshold.recommendations.len(),
            MAX_PLAYER_RECOMMENDATIONS
        );
    }

    #[test]
    fn missing_system_core_requires_the_important_side_to_be_owned() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| entry.id == "siracusa");
        knowledge.standalone.clear();

        let important_missing = build_player_training_advice(
            &operbox(&[("八幡海铃", 2, 1, 6)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(important_missing
            .recommendations
            .iter()
            .all(|item| item.operator != "伺夜"));

        let important_owned = build_player_training_advice(
            &operbox(&[("八幡海铃", 2, 1, 6), ("贝洛内", 0, 1, 5)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(important_owned
            .recommendations
            .iter()
            .any(|item| { item.operator == "伺夜" && item.action == PlayerAction::Acquire }));
    }

    #[test]
    fn owned_untrained_core_takes_state_precedence_over_missing_important() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge
            .combinations
            .retain(|entry| entry.id == "siracusa");
        knowledge.standalone.clear();
        let report = build_player_training_advice(
            &operbox(&[("伺夜", 0, 1, 6), ("八幡海铃", 2, 1, 6)]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        let progress = &report.combinations[0];

        assert_eq!(progress.state, CombinationProgressState::NeedsTraining);
        assert_eq!(progress.untrained_core, vec!["伺夜"]);
        assert_eq!(progress.missing_important, vec!["贝洛内"]);
        assert!(report
            .recommendations
            .iter()
            .any(|item| { item.operator == "伺夜" && item.action == PlayerAction::Train }));
    }

    #[test]
    fn standalone_conditions_require_known_satisfied_context() {
        let mut knowledge = load_knowledge();
        knowledge.newbie.clear();
        knowledge.combinations.clear();
        knowledge.standalone.retain(|entry| {
            matches!(
                entry.id.as_str(),
                "luck_trade" | "vigil_trade" | "archetto_trade" | "minimalist_general"
            )
        });
        let unknown = build_player_training_advice(
            &operbox(&[]),
            &knowledge,
            &PlayerAdviceContext::default(),
        );
        assert!(unknown.recommendations.is_empty());

        let known = build_player_training_advice(
            &operbox(&[]),
            &knowledge,
            &PlayerAdviceContext {
                meeting_room_max_level: Some(3),
                dormitory_level_sum: Some(20),
                engineering_robot_count: Some(64),
                ..PlayerAdviceContext::default()
            },
        );
        let names = known
            .recommendations
            .iter()
            .map(|item| item.operator.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["伺夜", "空弦", "至简"]);
        assert!(known.recommendations.iter().all(|item| {
            item.reason == PlayerRecommendationReason::Standalone
                && item.product.is_some()
                && item.efficiency.is_some()
                && item
                    .conditions
                    .iter()
                    .all(|condition| condition.status == PlayerConditionStatus::Satisfied)
        }));
        assert!(!names.contains(&"吉星"));
    }

    #[test]
    fn context_rejects_invalid_efficiency_values() {
        assert!(PlayerAdviceContext {
            trade_average_efficiency_percent: Some(-0.1),
            ..PlayerAdviceContext::default()
        }
        .validate()
        .is_err());
    }
}
