use std::sync::Arc;

use crate::tier::PromotionTier;
use crate::layout::{LayoutContext, SharedLayout};
use crate::types::RecipeKind;

/// 制造站产线布局假设：同类产线用同一三人组（L1 搜索简化，不做 12 人分站排班）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ManuLineScenario {
    pub gold_lines: u8,
    pub battle_record_lines: u8,
    pub originium_lines: u8,
}

impl ManuLineScenario {
    /// 默认基准：4 条产线 = 2 赤金 + 2 经验（源石 0）。
    pub fn standard_four_lines() -> Self {
        Self {
            gold_lines: 2,
            battle_record_lines: 2,
            originium_lines: 0,
        }
    }

    pub fn total_lines(self) -> u8 {
        self.gold_lines
            .saturating_add(self.battle_record_lines)
            .saturating_add(self.originium_lines)
    }

    pub fn active_recipes(self) -> impl Iterator<Item = (RecipeKind, u8)> {
        [
            (RecipeKind::Gold, self.gold_lines),
            (RecipeKind::BattleRecord, self.battle_record_lines),
            (RecipeKind::Originium, self.originium_lines),
        ]
        .into_iter()
        .filter(|(_, lines)| *lines > 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ManuSearchRecipeMode {
    /// 单配方求值（调试 / 单线专精）。
    Single(RecipeKind),
    /// 按产线数加权求和（默认 2 金 + 2 经验）。
    Lines(ManuLineScenario),
}

impl Default for ManuSearchRecipeMode {
    fn default() -> Self {
        Self::Lines(ManuLineScenario::standard_four_lines())
    }
}

impl ManuSearchRecipeMode {
    pub fn single_gold() -> Self {
        Self::Single(RecipeKind::Gold)
    }
}

#[derive(Debug, Clone)]
pub struct ManuOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
}

impl ManuOperator {
    pub fn tier(&self) -> PromotionTier {
        PromotionTier::from_elite(self.elite)
    }

    pub fn new(name: impl Into<String>, elite: u8, buff_ids: Vec<String>) -> Self {
        Self {
            name: name.into(),
            elite,
            buff_ids,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManuRoomInput {
    pub level: u8,
    pub operators: Vec<ManuOperator>,
    pub active_recipe: RecipeKind,
    pub mood: f64,
    pub layout: SharedLayout,
}

impl ManuRoomInput {
    pub fn with_operators(level: u8, active_recipe: RecipeKind, operators: Vec<ManuOperator>) -> Self {
        Self {
            level,
            operators,
            active_recipe,
            mood: 24.0,
            layout: Arc::new(LayoutContext::default()),
        }
    }

    pub fn operator_names(&self) -> Vec<&str> {
        self.operators.iter().map(|o| o.name.as_str()).collect()
    }
}
