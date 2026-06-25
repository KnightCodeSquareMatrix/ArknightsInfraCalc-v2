use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::global_resource::GlobalResourceKey;
use crate::global_resource::GlobalResourcePool;
use crate::manufacture::input::ManuLineScenario;
use crate::trade::input::{TradeOrderKind, TradeStationScenario};
use crate::types::RecipeKind;

/// 稳定房间键：可与游戏 slot 对齐，或使用语义名 `trade_1`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoomId(pub String);

impl RoomId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for RoomId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacilityKind {
    ControlCenter,
    TradePost,
    Factory,
    PowerPlant,
    Dormitory,
    Office,
    MeetingRoom,
    TrainingRoom,
    Workshop,
}

/// 生产类房间的在产配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomProduct {
    Trade { order: TradeOrderKind },
    Factory { recipe: RecipeKind },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomBlueprint {
    pub id: RoomId,
    pub kind: FacilityKind,
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<RoomProduct>,
    /// 宿舍床位数；缺省时按等级查表。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dorm_beds: Option<u8>,
    /// “每间宿舍每级”技能读取的有效宿舍等级；缺省时兼容旧布局的 dorm_beds，再退回建筑等级。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dorm_ambience_level: Option<u8>,
}

impl RoomBlueprint {
    pub fn dorm_skill_level(&self) -> u8 {
        self.dorm_ambience_level
            .or(self.dorm_beds)
            .unwrap_or(self.level)
            .min(5)
    }
}

/// 场景假设：无法从物理蓝图单独推出的聚合量（宿管精二设施数等）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlueprintScenario {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 无进驻编制时的回退；有 assignment 时由 WorkforceIndex 按真言「精英小队」规则统计。
    pub elite_facility_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sui_facility_count: Option<u8>,
    /// 无宿舍进驻编制时的默认宿舍人数（黑键/乌有链）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dorm_occupant_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_workforce: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub initial_global: HashMap<GlobalResourceKey, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseBlueprint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default = "default_drone_cap")]
    pub drone_cap: u32,
    #[serde(default)]
    pub scenario: BlueprintScenario,
    pub rooms: Vec<RoomBlueprint>,
}

fn default_drone_cap() -> u32 {
    135
}

impl BaseBlueprint {
    pub fn room(&self, id: &RoomId) -> Option<&RoomBlueprint> {
        self.rooms.iter().find(|r| &r.id == id)
    }

    pub fn rooms_of(&self, kind: FacilityKind) -> Vec<&RoomBlueprint> {
        self.rooms.iter().filter(|r| r.kind == kind).collect()
    }

    pub fn count_facility(&self, kind: FacilityKind) -> u8 {
        self.rooms
            .iter()
            .filter(|r| r.kind == kind)
            .count()
            .min(255) as u8
    }

    pub fn manu_recipe_kinds(&self) -> u8 {
        let kinds: HashSet<RecipeKind> = self
            .rooms
            .iter()
            .filter_map(|r| match (&r.kind, &r.product) {
                (FacilityKind::Factory, Some(RoomProduct::Factory { recipe })) => Some(*recipe),
                _ => None,
            })
            .collect();
        kinds.len().min(255) as u8
    }

    /// 贸易站订单分布（由蓝图在产贸易站推导）。
    pub fn trade_station_scenario(&self) -> TradeStationScenario {
        let mut gold_order_stations = 0u8;
        let mut originium_order_stations = 0u8;
        for room in &self.rooms {
            if room.kind != FacilityKind::TradePost {
                continue;
            }
            let Some(RoomProduct::Trade { order }) = room.product.as_ref() else {
                continue;
            };
            match order {
                TradeOrderKind::Gold => gold_order_stations = gold_order_stations.saturating_add(1),
                TradeOrderKind::Originium => {
                    originium_order_stations = originium_order_stations.saturating_add(1)
                }
            }
        }
        TradeStationScenario {
            gold_order_stations,
            originium_order_stations,
        }
    }

    /// 制造站产线分布（由蓝图在产制造站推导）。
    pub fn manu_line_scenario(&self) -> ManuLineScenario {
        let mut gold_lines = 0u8;
        let mut battle_record_lines = 0u8;
        let mut originium_lines = 0u8;
        for room in &self.rooms {
            if room.kind != FacilityKind::Factory {
                continue;
            }
            let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
                continue;
            };
            match recipe {
                RecipeKind::Gold => gold_lines = gold_lines.saturating_add(1),
                RecipeKind::BattleRecord => {
                    battle_record_lines = battle_record_lines.saturating_add(1)
                }
                RecipeKind::Originium => originium_lines = originium_lines.saturating_add(1),
                RecipeKind::All => {}
            }
        }
        ManuLineScenario {
            gold_lines,
            battle_record_lines,
            originium_lines,
        }
    }

    pub fn gold_manu_line_count(&self) -> u32 {
        self.rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::Factory)
            .filter(|r| {
                matches!(
                    r.product,
                    Some(RoomProduct::Factory {
                        recipe: RecipeKind::Gold
                    })
                )
            })
            .count() as u32
    }

    pub fn meeting_max_level(&self) -> u8 {
        self.rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::MeetingRoom)
            .map(|r| r.level)
            .max()
            .unwrap_or(0)
    }

    pub fn training_room_max_level(&self) -> u8 {
        self.rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::TrainingRoom)
            .map(|r| r.level)
            .max()
            .unwrap_or(0)
    }

    pub fn dorm_level_sum(&self) -> u16 {
        self.rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::Dormitory)
            .map(|r| u16::from(r.dorm_skill_level()))
            .sum()
    }

    /// 至简「绘图设计」：除会客室外每间设施每级 +1 工程机器人（上限 64）。
    pub fn facility_level_sum_excl_meeting(&self) -> u16 {
        self.rooms
            .iter()
            .filter(|r| r.kind != FacilityKind::MeetingRoom)
            .map(|r| u16::from(r.level))
            .sum()
    }

    pub fn dorm_bed_capacity(&self) -> u8 {
        self.rooms
            .iter()
            .filter(|r| r.kind == FacilityKind::Dormitory)
            .map(|r| r.dorm_beds.unwrap_or_else(|| default_dorm_beds(r.level)))
            .sum::<u8>()
    }

    pub fn initial_global_pool(&self) -> GlobalResourcePool {
        let mut pool = GlobalResourcePool::new();
        for (key, value) in &self.scenario.initial_global {
            pool.set(*key, *value);
        }
        pool
    }

    pub fn load_template(name: &str) -> Result<Self> {
        let path = crate::skill_table::data_path(&format!("layout/{name}.json"))?;
        Self::load(&path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let blueprint: BaseBlueprint = serde_json::from_str(&raw)?;
        blueprint.validate()?;
        Ok(blueprint)
    }

    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for room in &self.rooms {
            if !ids.insert(room.id.0.clone()) {
                return Err(Error::msg(format!("duplicate room id {}", room.id.0)));
            }
            if room.level == 0 {
                return Err(Error::msg(format!("room {} level must be >= 1", room.id.0)));
            }
            if matches!(room.kind, FacilityKind::Dormitory) {
                if matches!(room.dorm_beds, Some(0)) {
                    return Err(Error::msg(format!(
                        "dorm room {} dorm_beds must be >= 1",
                        room.id.0
                    )));
                }
                if matches!(room.dorm_ambience_level, Some(0)) {
                    return Err(Error::msg(format!(
                        "dorm room {} dorm_ambience_level must be >= 1",
                        room.id.0
                    )));
                }
            }
            match room.kind {
                FacilityKind::TradePost => {
                    let Some(RoomProduct::Trade { .. }) = room.product else {
                        return Err(Error::msg(format!(
                            "trade room {} requires product.trade",
                            room.id.0
                        )));
                    };
                }
                FacilityKind::Factory => {
                    let Some(RoomProduct::Factory { .. }) = room.product else {
                        return Err(Error::msg(format!(
                            "factory room {} requires product.factory",
                            room.id.0
                        )));
                    };
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 旧版 243c（3 贸易：2 金 + 1 源石）；怪猎 `snhunt` 等同结构仍用此模板。
    pub fn template_243c() -> Result<Self> {
        Self::load_template("243c")
    }

    /// 公孙 243 事实布局：`data/layout/243_use_this_.json`（2 金贸、2 金 + 2 经验制造）。
    pub fn template_243_use_this() -> Result<Self> {
        Self::load_template("243_use_this_")
    }

    pub fn template_252_auto1() -> Result<Self> {
        Self::load_template("252_auto1")
    }

    /// 怪猎联动基准：物理布局同 243c；木天蓼由中枢火龙S黑角 + 麒麟R夜刀在 `resolve` 编制中产出。
    pub fn template_snhunt() -> Result<Self> {
        Self::load_template("snhunt")
    }
}

pub fn default_dorm_beds(level: u8) -> u8 {
    match level {
        1 => 4,
        2 => 5,
        3 => 5,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_243_use_this_loads_and_validates() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        assert_eq!(bp.training_room_max_level(), 0);
        assert_eq!(bp.count_facility(FacilityKind::TradePost), 2);
        assert_eq!(bp.count_facility(FacilityKind::Factory), 4);
        assert_eq!(bp.manu_recipe_kinds(), 2);
        assert_eq!(bp.dorm_level_sum(), 20);
        let trade = bp.trade_station_scenario();
        assert_eq!(trade.gold_order_stations, 2);
        assert_eq!(trade.originium_order_stations, 0);
        let manu = bp.manu_line_scenario();
        assert_eq!(manu.gold_lines, 2);
        assert_eq!(manu.battle_record_lines, 2);
    }

    #[test]
    fn template_252_auto1_has_two_trade() {
        let bp = BaseBlueprint::template_252_auto1().unwrap();
        assert_eq!(bp.count_facility(FacilityKind::TradePost), 2);
        assert_eq!(bp.count_facility(FacilityKind::PowerPlant), 3);
    }

    #[test]
    fn scenario_derived_from_rooms() {
        let bp = BaseBlueprint::template_243_use_this().unwrap();
        let trade = bp.trade_station_scenario();
        assert_eq!(trade.gold_order_stations, 2);
        assert_eq!(trade.originium_order_stations, 0);
        let manu = bp.manu_line_scenario();
        assert_eq!(manu.gold_lines, 2);
        assert_eq!(manu.battle_record_lines, 2);
        assert_eq!(bp.gold_manu_line_count(), 2);
    }

    #[test]
    fn dorm_level_sum_uses_skill_level_not_building_level() {
        let bp = BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: Default::default(),
            rooms: vec![
                RoomBlueprint {
                    id: RoomId::new("dorm_1"),
                    kind: FacilityKind::Dormitory,
                    level: 3,
                    product: None,
                    dorm_beds: Some(5),
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::new("dorm_2"),
                    kind: FacilityKind::Dormitory,
                    level: 3,
                    product: None,
                    dorm_beds: Some(5),
                    dorm_ambience_level: Some(4),
                },
            ],
        };

        assert_eq!(bp.dorm_level_sum(), 9);
    }
}
