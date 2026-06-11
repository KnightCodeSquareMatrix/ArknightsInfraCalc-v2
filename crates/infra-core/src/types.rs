use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    StateWrite,
    Constant,
    PeerShare,
    Limit,
    /// After limit recompute: eff from net order-limit gain (招商引资).
    LimitVar,
    OrderVar,
    EffVar,
    /// After peer eff settles: zero other ops and credit owner (巫恋·低语).
    PeerAbsorb,
    OrderMechanic,
    GlobalInject,
    Mood,
}

impl Phase {
    pub fn sort_key(self) -> i32 {
        match self {
            Self::StateWrite => 10,
            Self::Constant => 20,
            Self::PeerShare => 35,
            Self::Limit => 40,
            Self::LimitVar => 55,
            Self::OrderVar => 50,
            Self::EffVar => 60,
            Self::PeerAbsorb => 70,
            Self::OrderMechanic => 90,
            Self::GlobalInject => 30,
            Self::Mood => 95,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selector {
    GoldDeliveryCount,
    OtherOpsDirectEff,
    OtherOpsTotalEff,
    RoomPeerCount,
    FinalOrderLimit,
    /// `final_order_limit - facility_base_limit` (招商引资).
    LimitExcess,
    /// Trade post facility level (1–3); used by 佩佩/瑰盐·每级+1 上限.
    FacilityLevel,
    /// Count of room peers carrying `tag` (摩根/新约能天使).
    TaggedCountInRoom { tag: String },
    /// Sum of per-operator `limit_contrib` (锏·冠军风采).
    LimitContribSum,
    MeetingMaxLevel,
    DormLevelSum,
    ManuRecipeKinds,
    EliteFacilityCount,
    SuiFacilityCount,
    DormOccupantCount,
    OrderGap,
    Mood,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    GoldDeliveryBelow { n: u8 },
    GoldDeliveryAbove { n: u8 },
    GoldOrderInvestEligible {},
    OrderHasTag { tag: String },
    OrderNotHasTag { tag: String },
    MoodAbove { n: u8 },
    MoodBelowOrEq { n: u8 },
    PartnerInRoom { name: String },
    TagPresentInRoom { tag: String },
    OperatorInBase { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoodDrainScope {
    #[serde(rename = "self")]
    SelfOp,
    RoomOperators,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "params")]
pub enum Action {
    AddFlatEff { value: f64 },
    AddPerGapEff { rate: f64 },
    TagOrder { tag: String },
    AddGoldDelivery { n: u8 },
    ReplaceOrder { order_type: String },
    ReduceLimit { div: f64, min: i32 },
    AddLimitFromSelector { multiplier: f64 },
    AddFlatEffFromSelector {
        multiplier: f64,
        #[serde(default)]
        cap: Option<f64>,
    },
    AddLimitDelta { delta: i32 },
    StateProduce { key: String, amount: f64 },
    StateConsume { key: String, div: f64 },
    MoodDrainDelta {
        delta: f64,
        scope: MoodDrainScope,
    },
    /// Per `floor(state / step_size)` steps, apply `delta_per_step` to mood drain (铎铃·人间烟火).
    MoodDrainPerStateStep {
        key: String,
        step_size: f64,
        delta_per_step: f64,
        scope: MoodDrainScope,
    },
    AddOrderLmdBonus { bonus: i32 },
    VodfoxAbsorb { rate_per_peer: f64 },
    /// `floor(selector/step)*ret_per_step` capped (雪雉/锏 bucket).
    AddBucketEffFromSelector {
        step: f64,
        ret_per_step: f64,
        cap: f64,
    },
    StateConvert {
        from: String,
        to: String,
        ratio: f64,
    },
    /// Read state, add `floor(state/div)`% to owner eff (黑键/齐尔查克).
    StateConsumeToEff { key: String, div: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectAtom {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<Selector>,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub phase: Phase,
    pub phase_order: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub skill_name: String,
    pub facility: String,
    pub tier: String,
    pub atoms: Vec<EffectAtom>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateKey {
    HumanFireworks,
    Perception,
    SilentEcho,
    MonsterCuisine,
}

impl StateKey {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "HumanFireworks" | "人间烟火" => Some(Self::HumanFireworks),
            "Perception" | "感知信息" => Some(Self::Perception),
            "SilentEcho" | "无声共鸣" => Some(Self::SilentEcho),
            "MonsterCuisine" | "魔物料理" => Some(Self::MonsterCuisine),
            _ => None,
        }
    }
}
