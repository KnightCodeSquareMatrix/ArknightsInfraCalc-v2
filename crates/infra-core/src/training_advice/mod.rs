mod evaluate;
mod knowledge;
mod player;
mod rag;
mod rules;
mod types;

pub use evaluate::build_training_advice;
pub use knowledge::{
    default_training_advice_knowledge_path, load_training_advice_knowledge,
    validate_training_advice_knowledge, AcquisitionInfo, AcquisitionKind, ChoiceGroup,
    CombinationKnowledgeEntry, CombinationMember, CombinationScale, CombinationTier,
    CompletionAlternative, CompletionSpec, ConditionKind, EfficiencyFact, EfficiencyUnit,
    KnowledgeRole, KnowledgeSource, KnowledgeTarget, NewbieKnowledgeEntry, ProductBucket,
    StandaloneKnowledgeEntry, TargetKind, TrainingAdviceKnowledge,
};
pub use player::{
    build_player_training_advice, CombinationMemberProgress, CombinationProgress,
    CombinationProgressState, MemberProgress, NewbieProgress, PlayerAction, PlayerAdviceContext,
    PlayerRecommendation, PlayerRecommendationPriority, PlayerRecommendationReason,
    PlayerTrainingAdviceReport, MAX_PLAYER_RECOMMENDATIONS,
};
pub use rag::{build_training_advice_rag_input, render_training_advice_answer};
pub use rules::{default_training_recommendations_path, load_training_recommendation_rules};
pub use types::{
    AcquisitionMode, AcquisitionPolicy, BlockedRuleReport, EvidenceRef, EvidenceSnippet,
    MemberBenefit, MemberRole, MissingCoreGroupReport, OperatorAdviceItem, OperatorTrainingState,
    PickOneCoreSlot, RecommendationAction, RecommendationPriority, RequiredCoreGroup, ReviewStatus,
    RuleAdmission, RuleKind, RuleMatch, RuleMember, RuleReview, RuleScope, TrainingAdviceBundle,
    TrainingAdviceFact, TrainingAdviceOptions, TrainingAdviceRagInput, TrainingAdviceReport,
    TrainingAdviceSummary, TrainingRecommendationRules, TrainingRule, TrainingTarget,
};
