use serde::{Deserialize, Serialize};

pub use crate::eff_ramp::EffRampStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    All,
    BattleRecord,
    Gold,
    Originium,
}

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
    /// After peer eff settles: zero other ops and credit owner (巫恋/佩佩·PeerEffAbsorb).
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
    /// 制造站同房干员总数（含自身；冬时·流程优化等）。
    RoomOperatorCount,
    FinalOrderLimit,
    /// `final_order_limit - facility_base_limit` (招商引资).
    LimitExcess,
    /// Trade post facility level (1–3); used by 佩佩/瑰盐·每级+1 上限.
    FacilityLevel,
    /// Count of room peers carrying `tag` (摩根/新约能天使).
    TaggedCountInRoom {
        tag: String,
    },
    /// Sum of per-operator `limit_contrib` (锏·冠军风采).
    LimitContribSum,
    MeetingMaxLevel,
    DormLevelSum,
    ManuRecipeKinds,
    EliteFacilityCount,
    SuiFacilityCount,
    /// 岁干员进驻设施数，带上限（重岳·知我为我，最多 5）。
    CappedSuiFacilityCount {
        max: u8,
    },
    DormOccupantCount,
    OrderGap,
    /// 当前订单数（孑·市井之道 per-order）。
    OrderCount,
    /// 全站 settled 效率之和（雪雉·天道酬勤分档）。
    PeerSettledEffSum,
    /// 同房他人 `skill_eff` 之和（不含 `layout_eff`；槐琥·配合意识）。
    PeerSkillEffSum,
    /// 同房他人 settled 效率之和（孑·压缩上限）。
    OtherOpsSettledEff,
    /// 基建内贸易站数量（清流/引星棘刺·再生能源等）。
    TradeStationCount,
    /// 基建内发电站数量（森蚺/温蒂·自动化等）。
    PowerStationCount,
    /// 基建内发电站作业平台数量（阿兰娜·机械精通等）。
    PlatformCountInPower,
    /// 控制中枢内携带 `tag` 的干员数（黑角·团队合作等）。
    TaggedCountInControl {
        tag: String,
    },
    /// 控制中枢进驻干员总数。
    ControlOperatorCount,
    /// 无人机上限（承曦·巡线框架）。
    DroneCap,
    /// 基建内莱茵生命干员数（娜斯提·造价高昂等；最多 5，不含副手）。
    RhineLifeInBase,
    /// 基建内除自身外莱茵生命干员数（缪尔赛思·生态科主任，最多 5）。
    RhineLifeInBaseExcludingSelf,
    /// 同房金属工艺类技能数量（苍苔·打工心得；含自身）。
    MetalFormulaSkillCountInRoom,
    /// 同房标准化类技能数量（水月·意识协议；标准化·α/β）。
    StandardSkillCountInRoom,
    /// 同房莱茵科技类技能数量（多萝西·源石技艺理论应用；莱茵·α/β/γ）。
    RhineSkillCountInRoom,
    /// 全基建设施等级合计（不含会客室；至简·绘图设计 → 工程机器人）。
    FacilityLevelSumExclMeeting,
    /// 训练室最高等级（维伊·手艺人；cap 通常 30%）。
    TrainingRoomLevel,
    Mood,
    /// 各贸易站同房 `tag` 干员数之和（戴菲恩/八幡海铃等中枢跨设施注入）。
    TaggedCountInTradeSum {
        tag: String,
    },
    /// 至少 `min` 名 `tag` 干员进驻的贸易站数量（凛御银灰·商业版图）。
    TradeStationsWithTaggedGte {
        tag: String,
        min: u8,
    },
    /// 各制造站同房 `tag` 干员数之和（涤火杰西卡/薇薇安娜等）。
    TaggedCountInManuSum {
        tag: String,
    },
    /// 中枢求解中 `state_pool` 资源量按 `div` 下取整（祥子/睦热情值阶梯）。
    StatePoolFloored {
        key: String,
        div: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    GoldDeliveryBelow {
        n: u8,
    },
    GoldDeliveryAbove {
        n: u8,
    },
    GoldOrderInvestEligible {},
    OrderHasTag {
        tag: String,
    },
    OrderNotHasTag {
        tag: String,
    },
    MoodAbove {
        n: u8,
    },
    MoodBelowOrEq {
        n: u8,
    },
    PartnerInRoom {
        name: String,
    },
    TagPresentInRoom {
        tag: String,
    },
    /// 同房存在带 `tag` 的**其他**干员（怪猎秘传交涉术/以身作则等）。
    PeerTagInRoom {
        tag: String,
    },
    OperatorInBase {
        name: String,
    },
    /// 干员进驻在发电站（森蚺·我寻思能行）。
    OperatorInPower {
        name: String,
    },
    /// 干员在训练室协助位（PhonoR-0·咒文共鸣）。
    OperatorInTraining {
        name: String,
    },
    /// 干员进驻在贸易站（烈夏·患难拍档等跨设施条件）。
    OperatorInTrade {
        name: String,
    },
    /// 其他发电站无作业平台（承曦·晨曦）。
    NoPlatformInOtherPower {},
    /// 其他作业平台进驻发电站（GALLUS²·鸡励机制）。
    OtherPlatformInPower {},
    /// 其他拉特兰干员进驻发电站（CONFESS-47·维护中）。
    OtherLateranoInPower {},
    /// 市井之道与天道酬勤不可单独互叠；无第三方 settled 贡献时天道酬勤不生效。
    TiandaoEffVarAllowed {},
    /// 制造站当前在产配方（仓库整备/剪辑/Vlog 等配方限定效果）。
    ActiveRecipe {
        kind: RecipeKind,
    },
    /// 持有者未绑定该 buff（冬时精0 科学改造 vs 精1 流程优化）。
    OwnerLacksBuff {
        buff_id: String,
    },
    /// 望·权变：外势 ≥ 实地（贸易站+发电站 ≥ 制造站）.
    ExternalMomentumGteField {},
    /// 望·权变：实地 > 外势.
    FieldMomentumGtExternal {},
    /// 发电站作业平台数量 ≥ n（布丁·超频）。
    PlatformCountGte {
        min: u8,
    },
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
    AddFlatEff {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
    },
    AddPerGapEff {
        rate: f64,
    },
    /// 订单分类标签（违约、裁缝 peak、特别订单等共用同一原语）。
    TagOrder {
        tag: String,
    },
    AddGoldDelivery {
        n: u8,
    },
    ReduceLimit {
        div: f64,
        min: i32,
    },
    AddLimitFromSelector {
        multiplier: f64,
    },
    /// 读取同房 `state_pool`，`floor(state) * multiplier` → 仓库贡献。
    AddLimitFromState {
        key: String,
        multiplier: f64,
    },
    AddFlatEffFromSelector {
        multiplier: f64,
        #[serde(default)]
        cap: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
    },
    AddLimitDelta {
        delta: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
    },
    StateProduce {
        key: String,
        amount: f64,
    },
    StateConsume {
        key: String,
        div: f64,
    },
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
    AddOrderLmdBonus {
        bonus: i32,
    },
    /// 同房他人 trade 效率归零；`rate_per_peer` 为每名他人向自身转移的效率%（巫恋 45，佩佩 0）。
    #[serde(alias = "vodfox_absorb")]
    PeerEffAbsorb {
        rate_per_peer: f64,
    },
    /// `floor(selector/step)*ret_per_step` capped (雪雉/锏 bucket).
    AddBucketEffFromSelector {
        step: f64,
        ret_per_step: f64,
        cap: f64,
    },
    /// 同房 `limit_contrib` 总和 × `rate`% → 站级生产力（红云·回收利用）。
    AddEffFromLimitContribSum {
        rate: f64,
    },
    /// 同房每人 `limit_contrib` 分段 × rate → 站级生产力（泡泡·大就是好）。
    AddEffFromLimitContribTiered {
        threshold: i32,
        low_rate: f64,
        high_rate: f64,
    },
    StateConvert {
        from: String,
        to: String,
        ratio: f64,
    },
    /// Read state, add `floor(state/div) * multiplier`% to owner eff (黑键/齐尔查克/泰拉调查团).
    StateConsumeToEff {
        key: String,
        div: f64,
        #[serde(default)]
        multiplier: Option<f64>,
    },
    /// 中枢注入：全贸易站订单效率 +%（同 `tag` 族取最高后合并）。
    GlobalInjectTradeEff {
        value: f64,
    },
    /// 中枢注入：全制造站生产力 +%（`recipe` 默认 All）。
    GlobalInjectManuEff {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
    },
    /// 中枢注入：灵知·精密计算。每名同贸易房谢拉格(`cc.g.karlan`)干员
    /// 订单获取效率 `eff_per_karlan`%、订单上限 `limit_per_karlan`（数值按房间组成在贸易域结算）。
    GlobalInjectKarlanPrecision {
        eff_per_karlan: f64,
        limit_per_karlan: i32,
    },
    /// 进驻后逐时爬升；制造纸面取 20h 效率算术平均（芬/阿罗玛等）。
    AddEffRamp {
        initial: f64,
        per_hour: f64,
        cap: f64,
        #[serde(default)]
        style: EffRampStyle,
    },
}

/// 技能 atom 影响范围：同房（默认）或全基建跨房间。
///
/// - `Room`（默认）：现有行为，per-room 求解执行。
/// - `Global`：跨房间效果，写入全局资源池供全基建共享。
///   由 [`crate::cross_facility`] 编排层统一执行，per-room 求解时跳过。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomScope {
    Room,
    Global,
}

impl Default for AtomScope {
    fn default() -> Self {
        Self::Room
    }
}

/// `#[serde(skip_serializing_if)]` helper for `AtomScope` defaults.
pub fn is_room_scope(s: &AtomScope) -> bool {
    *s == AtomScope::Room
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
    /// 影响范围：`Room`（默认）或 `Global`。
    /// Global 的 atom 由 cross_facility 编排层统一执行，per-room 求解会跳过。
    #[serde(default, skip_serializing_if = "is_room_scope")]
    pub scope: AtomScope,
}

/// 建池时预编译的单条 atom（含排序键，供 solve 时 k 路归并）。
#[derive(Debug, Clone)]
pub struct CompiledAtom {
    pub atom: EffectAtom,
    pub sort_key: (i32, i32),
    pub seq: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub skill_name: String,
    pub facility: String,
    pub tier: String,
    pub atoms: Vec<EffectAtom>,
}

/// 同房 StateWrite / skill_table JSON 沿用的状态键；与 [`crate::global_resource::GlobalResourceKey`] 同型。
pub type StateKey = crate::global_resource::GlobalResourceKey;
