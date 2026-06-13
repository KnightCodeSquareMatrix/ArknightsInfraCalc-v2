use std::sync::Arc;

pub use crate::layout::{LayoutContext, SharedLayout, DEFAULT_DORM_OCCUPANT_COUNT};

/// 兼容旧名；新代码请用 [`LayoutContext`]。
pub type TradeLayoutContext = LayoutContext;

use crate::tier::PromotionTier;
use crate::types::{CompiledAtom, RecipeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeOrderKind {
    #[default]
    Gold,
    Originium,
}

impl TradeOrderKind {
    pub fn as_recipe_kind(self) -> RecipeKind {
        match self {
            Self::Gold => RecipeKind::Gold,
            Self::Originium => RecipeKind::Originium,
        }
    }

    pub fn is_gold(self) -> bool {
        matches!(self, Self::Gold)
    }
}

/// 贸易站排班假设：同类订单站共用同一三人组（L1 搜索简化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct TradeStationScenario {
    pub gold_order_stations: u8,
    pub originium_order_stations: u8,
}

impl TradeStationScenario {
    /// 默认基准：3 站 = 2 赤金订单 + 1 源石（固源岩）订单。
    pub fn standard_three_stations() -> Self {
        Self {
            gold_order_stations: 2,
            originium_order_stations: 1,
        }
    }

    pub fn total_stations(self) -> u8 {
        self.gold_order_stations
            .saturating_add(self.originium_order_stations)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TradeSearchOrderMode {
    /// 单订单类型求值（调试 / 专精站）。
    Single(TradeOrderKind),
    /// 按站数加权求和（默认 2 赤金 + 1 源石）。
    Stations(TradeStationScenario),
}

impl Default for TradeSearchOrderMode {
    fn default() -> Self {
        Self::Stations(TradeStationScenario::standard_three_stations())
    }
}

#[derive(Debug, Clone)]
pub struct TradeOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// 建池时预编译；手动构造的干员用空切片。
    pub compiled_atoms: Arc<[CompiledAtom]>,
}

impl Default for TradeOperator {
    fn default() -> Self {
        Self::new("", 0, Vec::new())
    }
}

impl TradeOperator {
    pub fn tier(&self) -> PromotionTier {
        PromotionTier::from_elite(self.elite)
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    pub fn new(name: impl Into<String>, elite: u8, buff_ids: Vec<String>) -> Self {
        Self {
            name: name.into(),
            elite,
            buff_ids,
            tags: Vec::new(),
            compiled_atoms: Arc::from([]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TradeRoomInput {
    pub level: u8,
    pub operators: Vec<TradeOperator>,
    pub order_count: Option<i32>,
    pub mood: f64,
    /// 基建内制造站赤金真实生产线数（默认 0；搜索常用 4）。
    pub gold_production_lines: Option<u32>,
    /// 杜林族干员提供的虚拟赤金线（鸿雪精2「际崖居民」）。
    pub durin_virtual_lines: Option<u32>,
    /// 进驻贸易站前已积累的人间烟火（铎铃心情链）。
    pub human_fireworks: Option<f64>,
    pub layout: SharedLayout,
    /// 当前贸易站处理的订单类型（赤金 / 固源岩源石订单）。
    pub active_order_kind: TradeOrderKind,
}

impl TradeRoomInput {
    pub fn operator_names(&self) -> Vec<&str> {
        self.operators.iter().map(|o| o.name.as_str()).collect()
    }

    pub fn with_operators(level: u8, operators: Vec<TradeOperator>) -> Self {
        Self {
            level,
            operators,
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(LayoutContext::default()),
            active_order_kind: TradeOrderKind::Gold,
        }
    }

    pub fn with_order_kind(mut self, kind: TradeOrderKind) -> Self {
        self.active_order_kind = kind;
        self
    }
}
