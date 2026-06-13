use crate::layout::LayoutContext;

#[derive(Debug, Clone)]
pub struct OfficeOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OfficeRoomInput {
    pub operators: Vec<OfficeOperator>,
    pub mood: f64,
    pub layout: LayoutContext,
}

impl OfficeRoomInput {
    pub fn with_operators(operators: Vec<OfficeOperator>) -> Self {
        Self {
            operators,
            mood: 24.0,
            layout: LayoutContext::default(),
        }
    }
}
