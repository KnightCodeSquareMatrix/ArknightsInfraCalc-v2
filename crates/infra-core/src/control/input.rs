use crate::tier::PromotionTier;
use crate::layout::LayoutContext;

#[derive(Debug, Clone)]
pub struct ControlOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
}

impl ControlOperator {
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
pub struct ControlRoomInput {
    pub operators: Vec<ControlOperator>,
    pub mood: f64,
    pub layout: LayoutContext,
}

impl ControlRoomInput {
    pub fn with_operators(operators: Vec<ControlOperator>) -> Self {
        Self {
            operators,
            mood: 24.0,
            layout: LayoutContext::default(),
        }
    }
}
