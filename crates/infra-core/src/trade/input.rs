use crate::tier::PromotionTier;

#[derive(Debug, Clone, Default)]
pub struct TradeLayoutContext {
    pub meeting_max_level: u8,
    pub dorm_level_sum: u16,
    pub manu_recipe_kinds: u8,
    pub elite_facility_count: u8,
    pub sui_facility_count: u8,
    /// 全基建宿舍进驻人数合计（黑键/乌有状态链）.
    pub dorm_occupant_count: u8,
    /// 森西宿舍写入的魔物料理层数（齐尔查克贸易链）.
    pub monster_cuisine_layers: u8,
    /// Operators working anywhere in base (赫德雷·白手起家).
    pub base_workforce: Vec<String>,
}

impl TradeLayoutContext {
    /// 243c Lv3 公孙长乐基准：布局技能与常见基建联动干员在岗。
    pub fn search_baseline() -> Self {
        Self {
            meeting_max_level: 3,
            dorm_level_sum: 12,
            manu_recipe_kinds: 4,
            elite_facility_count: 6,
            sui_facility_count: 2,
            dorm_occupant_count: 20,
            monster_cuisine_layers: 3,
            base_workforce: vec!["伺夜".into(), "乌尔比安".into()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct TradeOperator {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
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
    pub layout: TradeLayoutContext,
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
            layout: TradeLayoutContext::default(),
        }
    }
}
