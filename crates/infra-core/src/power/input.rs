use crate::tier::PromotionTier;
use crate::layout::LayoutContext;

#[derive(Debug, Clone)]
pub struct PowerOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
}

impl PowerOperator {
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
pub struct PowerRoomInput {
    pub operator: PowerOperator,
    pub mood: f64,
    /// 上班时长（小时）；空构·技术交流爬升用。
    pub shift_hours: f64,
    pub layout: LayoutContext,
}

impl PowerRoomInput {
    pub fn with_operator(operator: PowerOperator) -> Self {
        Self {
            operator,
            mood: 24.0,
            shift_hours: 24.0,
            layout: LayoutContext::default(),
        }
    }
}
