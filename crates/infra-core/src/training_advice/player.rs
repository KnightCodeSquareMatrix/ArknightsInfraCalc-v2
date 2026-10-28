use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::layout::BaseBlueprint;
use crate::operbox::OperBox;
use crate::tier::PromotionTier;

use super::knowledge::{
    AcquisitionInfo, ChoiceGroup, CombinationKnowledgeEntry, CombinationScale, CombinationTier,
    ConditionKind, EfficiencyFact, KnowledgeCondition, KnowledgeRole, KnowledgeTarget,
    ProductBucket, TargetKind, TrainingAdviceKnowledge,
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
                target: report_target_for(operbox, &member.operator, &member.target),
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
                            target: report_target_for(operbox, &member.operator, &member.target),
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
            if entry.id == "bubble_group" && context.has_originium_shard_factory != Some(true) {
                continue;
            }
            if !matches!(
                member.progress,
                MemberProgress::Missing | MemberProgress::OwnedNeedsTraining
            ) {
                continue;
            }
            if member.role == KnowledgeRole::Core && (!important_side_owned || core_gaps > 2) {
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
                    target: report_target_for(operbox, &member.operator, &member.target),
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
        if conditions.iter().any(|condition| {
            condition.condition.kind != ConditionKind::SameStation
                && condition.status != PlayerConditionStatus::Satisfied
        }) {
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

    let bubble_pair = ["泡泡", "火神"];
    let bubble_gaps = bubble_pair
        .iter()
        .filter(|operator| merged.contains_key(**operator))
        .copied()
        .collect::<Vec<_>>();
    let mut selected_order = Vec::new();
    let mut bubble_handled = false;
    for operator in &order {
        if bubble_pair.contains(&operator.as_str()) && bubble_gaps.len() == 2 {
            if bubble_handled {
                continue;
            }
            bubble_handled = true;
            if selected_order.len() + 2 <= MAX_PLAYER_RECOMMENDATIONS {
                for bubble_operator in &order {
                    if bubble_pair.contains(&bubble_operator.as_str()) {
                        selected_order.push(bubble_operator.clone());
                    }
                }
            }
            continue;
        }
        if selected_order.len() < MAX_PLAYER_RECOMMENDATIONS {
            selected_order.push(operator.clone());
        }
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
        if !selected_order.contains(&mandatory) {
            if selected_order.len() == MAX_PLAYER_RECOMMENDATIONS {
                if let Some(index) = selected_order
                    .iter()
                    .rposition(|operator| !bubble_pair.contains(&operator.as_str()))
                {
                    selected_order.remove(index);
                }
            }
            selected_order.push(mandatory);
        }
    }

    selected_order
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
        TargetKind::DeriveFromSkillBinding => {
            if PromotionTier::from_progress(progress) == PromotionTier::TierUp {
                MemberProgress::Ready
            } else {
                MemberProgress::OwnedNeedsTraining
            }
        }
        TargetKind::NeedsReview => MemberProgress::NeedsReview,
    }
}

fn report_target_for(
    operbox: &OperBox,
    operator: &str,
    target: &KnowledgeTarget,
) -> KnowledgeTarget {
    if target.kind != TargetKind::DeriveFromSkillBinding {
        return target.clone();
    }
    let Some(progress) = operbox.progress_of(operator) else {
        return target.clone();
    };
    match progress.rarity {
        1 | 2 => KnowledgeTarget {
            kind: TargetKind::Explicit,
            elite: Some(0),
            level: Some(30),
        },
        3 | 4 => KnowledgeTarget {
            kind: TargetKind::Explicit,
            elite: Some(1),
            level: None,
        },
        _ => KnowledgeTarget {
            kind: TargetKind::Explicit,
            elite: Some(2),
            level: None,
        },
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
