use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::layout::BaseBlueprint;
use crate::operbox::OperBox;

use super::knowledge::{
    AcquisitionInfo, ChoiceGroup, CombinationKnowledgeEntry, CombinationScale, CombinationTier,
    KnowledgeRole, KnowledgeTarget, ProductBucket, TargetKind, TrainingAdviceKnowledge,
};
use super::types::OperatorTrainingState;

pub const MAX_PLAYER_RECOMMENDATIONS: usize = 10;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerAdviceContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_originium_shard_factory: Option<bool>,
}

impl PlayerAdviceContext {
    pub fn from_blueprint(blueprint: &BaseBlueprint) -> Self {
        Self {
            has_originium_shard_factory: Some(blueprint.manu_line_scenario().originium_lines > 0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerTrainingAdviceReport {
    pub schema_version: u32,
    pub context: PlayerAdviceContext,
    pub incomplete_newbie: Vec<NewbieProgress>,
    pub combinations: Vec<CombinationProgress>,
    pub recommendations: Vec<PlayerRecommendation>,
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
    SmallHighEfficiencyCore,
    SmallHighEfficiencyImportant,
    SystemSingleCoreGap,
    OtherImportant,
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
    pub combination_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combination_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquisition: Option<AcquisitionInfo>,
}

pub fn build_player_training_advice(
    operbox: &OperBox,
    knowledge: &TrainingAdviceKnowledge,
    context: &PlayerAdviceContext,
) -> PlayerTrainingAdviceReport {
    let incomplete_newbie = build_incomplete_newbie(operbox, knowledge);
    let combinations = knowledge
        .combinations
        .iter()
        .map(|entry| build_combination_progress(operbox, entry))
        .collect::<Vec<_>>();
    let recommendations = build_recommendations(
        operbox,
        knowledge,
        &incomplete_newbie,
        &combinations,
        context,
    );
    PlayerTrainingAdviceReport {
        schema_version: 1,
        context: context.clone(),
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
    let has_training_gap = selected_members.iter().any(|member| {
        status_by_name.get(member.operator.as_str()) == Some(&MemberProgress::OwnedNeedsTraining)
    });
    let state = if has_review {
        CombinationProgressState::NeedsReview
    } else if has_missing_core {
        CombinationProgressState::MissingCore
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
    combinations: &[CombinationProgress],
    context: &PlayerAdviceContext,
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
                combination_id: None,
                combination_name: None,
                acquisition: item.acquisition.clone(),
            },
        });
    }

    for (combo_index, (entry, progress)) in
        knowledge.combinations.iter().zip(combinations).enumerate()
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
            let tailor = is_tailor(entry, &member.operator);
            let classification = if tailor && member.progress == MemberProgress::OwnedNeedsTraining
            {
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
                    combination_id: Some(entry.id.clone()),
                    combination_name: Some(entry.name.clone()),
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
                combination_id: None,
                combination_name: None,
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
        if order.len() == MAX_PLAYER_RECOMMENDATIONS {
            break;
        }
    }
    order
        .into_iter()
        .filter_map(|operator| merged.remove(&operator))
        .collect()
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
            },
        );
        let with = build_player_training_advice(
            &box_,
            &knowledge,
            &PlayerAdviceContext {
                has_originium_shard_factory: Some(true),
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
}
