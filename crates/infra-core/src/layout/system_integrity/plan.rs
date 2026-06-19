//! 体系完整性判定输出：模拟 / 排班前的声明式计划（不含 trade% 评分）。

use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::shift::AssignShiftMode;

/// 迷迭香感知链档位（对齐 `docs/ROSEMARY_PERCEPTION_CHAIN.md` §4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosemaryTier {
    /// 夕 + 宿舍 producer + 絮雨
    Tier1,
    /// 夕 + 絮雨（无爱丽丝/车尔尼）
    Tier2,
    /// 仅核心 + 絮雨（无夕）
    Tier3,
    /// 核心 + 八幡海铃/焰狐龙梓兰等替代感知源（无絮雨）
    Tier3Substitute,
}

/// 班次绑定：迷迭香 + 黑键上 2 休 1（黑键贸站由贪心选型，不在此锚定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftBind {
    pub operators: Vec<String>,
    /// 相对 24h 周期内上班班次数（ABC 三队语境下由上层轮换消费）。
    pub on_shifts: u8,
    pub off_shifts: u8,
}

/// 体系锚点：只钉核心干员与设施，队友由后续散件贪心补齐。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemAnchor {
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    /// `None` = 该设施类型下首个空房（不绑 trade_1/trade_2）。
    pub room_id: Option<RoomId>,
}

/// 可选 producer slot（缺人时裁剪，不影响核心锚点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalProducer {
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosemaryPlan {
    pub system_id: String,
    pub priority: i32,
    pub tier: RosemaryTier,
    pub shift_mode: AssignShiftMode,
    pub anchors: Vec<SystemAnchor>,
    pub optional_producers: Vec<OptionalProducer>,
    pub shift_bind: ShiftBind,
    pub producers_present: Vec<String>,
    pub producers_missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    RecoveryShift,
    MissingOperator { name: String, need_elite: u8 },
    /// §8.2：无絮雨且无八幡海铃且无焰狐龙梓兰
    InsufficientPerceptionSources,
    /// §8.3：四发电等未验证布局
    UnsupportedLayout { power_stations: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosemaryVerdict {
    Activate(RosemaryPlan),
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluateResult {
    pub rosemary: RosemaryVerdict,
}

impl EvaluateResult {
    pub fn rosemary_plan(&self) -> Option<&RosemaryPlan> {
        match &self.rosemary {
            RosemaryVerdict::Activate(p) => Some(p),
            RosemaryVerdict::Skip(_) => None,
        }
    }
}
