use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

use crate::layout::blueprint::RoomId;
use crate::roster::OperatorProgress;
use crate::tier::PromotionTier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignedOperator {
    pub name: String,
    pub elite: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub level: u32,
    /// 0 = unknown; tier fallback then uses `elite` only for compatibility.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub rarity: u8,
}

impl AssignedOperator {
    pub fn new(name: impl Into<String>, elite: u8) -> Self {
        Self {
            name: name.into(),
            elite,
            level: 0,
            rarity: 0,
        }
    }

    pub fn from_progress(name: impl Into<String>, progress: OperatorProgress) -> Self {
        Self {
            name: name.into(),
            elite: progress.elite,
            level: progress.level,
            rarity: progress.rarity,
        }
    }

    pub fn tier(&self) -> PromotionTier {
        if self.level > 0 || self.rarity > 0 {
            return PromotionTier::from_progress(OperatorProgress::new(
                self.elite,
                self.level,
                self.rarity,
            ));
        }
        PromotionTier::from_elite(self.elite)
    }
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoomEfficiencySnapshot {
    /// 贸易调试倍率 `effective_eff_multiplier`。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_score: f64,
    /// 贸易订单效率总量纲 `%`。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_pct: f64,
    /// 贸易技能效率 `%`，不含人头与全局注入。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_skill_pct: f64,
    /// 赤金侧等效效率 `%`，用于拆分展示。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_gold_pct: f64,
    /// 制造总效率 `%`，即搜索排序使用的 `prod_total`。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manu_prod_total: f64,
    /// 制造技能生产力 `%`，不含人头与全局注入。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manu_prod_skill: f64,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub manu_storage_limit: i32,
    /// 发电充能速度 `%`。
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub power_charge_speed_pct: f64,
}

impl RoomEfficiencySnapshot {
    pub fn is_trade(&self) -> bool {
        self.trade_score != 0.0 || self.trade_pct != 0.0 || self.trade_skill_pct != 0.0
    }

    pub fn is_manufacture(&self) -> bool {
        self.manu_prod_total != 0.0 || self.manu_prod_skill != 0.0
    }

    pub fn is_power(&self) -> bool {
        self.power_charge_speed_pct != 0.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomAssignment {
    pub room_id: RoomId,
    #[serde(default)]
    pub operators: Vec<AssignedOperator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency: Option<RoomEfficiencySnapshot>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseAssignment {
    #[serde(default)]
    pub rooms: Vec<RoomAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub training_assist: Option<AssignedOperator>,
    /// 覆盖 blueprint.scenario.base_workforce。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_workforce: Vec<String>,
}

impl BaseAssignment {
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| Error::msg(format!("assignment parse {}: {e}", path.display())))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| Error::msg(format!("assignment serialize: {e}")))?;
        std::fs::write(path, json)
            .map_err(|e| Error::msg(format!("assignment write {}: {e}", path.display())))
    }

    pub fn operators_in(&self, room_id: &RoomId) -> &[AssignedOperator] {
        self.rooms
            .iter()
            .find(|r| &r.room_id == room_id)
            .map(|r| r.operators.as_slice())
            .unwrap_or(&[])
    }

    pub fn room_assignment(&self, room_id: &RoomId) -> Option<&RoomAssignment> {
        self.rooms.iter().find(|r| &r.room_id == room_id)
    }

    pub fn efficiency_in(&self, room_id: &RoomId) -> Option<&RoomEfficiencySnapshot> {
        self.room_assignment(room_id)
            .and_then(|room| room.efficiency.as_ref())
    }

    pub fn set_room(&mut self, room_id: impl Into<RoomId>, operators: Vec<AssignedOperator>) {
        self.set_room_with_efficiency(room_id, operators, None);
    }

    pub fn set_room_with_efficiency(
        &mut self,
        room_id: impl Into<RoomId>,
        operators: Vec<AssignedOperator>,
        efficiency: Option<RoomEfficiencySnapshot>,
    ) {
        let room_id = room_id.into();
        if let Some(entry) = self.rooms.iter_mut().find(|r| r.room_id == room_id) {
            entry.operators = operators;
            entry.efficiency = efficiency;
            return;
        }
        self.rooms.push(RoomAssignment {
            room_id,
            operators,
            efficiency,
        });
    }

    pub fn set_room_assignment(&mut self, room: RoomAssignment) {
        if let Some(entry) = self.rooms.iter_mut().find(|r| r.room_id == room.room_id) {
            *entry = room;
            return;
        }
        self.rooms.push(room);
    }

    pub fn set_power_operator(&mut self, room_id: impl Into<RoomId>, op: AssignedOperator) {
        self.set_room(room_id, vec![op]);
    }

    pub fn control_operators(&self) -> Vec<AssignedOperator> {
        self.rooms
            .iter()
            .find(|r| r.room_id.0 == "control")
            .map(|r| r.operators.clone())
            .unwrap_or_default()
    }

    pub fn by_room_map(&self) -> HashMap<&str, &RoomAssignment> {
        self.rooms
            .iter()
            .map(|r| (r.room_id.0.as_str(), r))
            .collect()
    }

    pub fn dorm_occupant_count(&self) -> u8 {
        self.rooms
            .iter()
            .map(|r| r.operators.len())
            .sum::<usize>()
            .min(255) as u8
    }

    pub fn has_room_staffing(&self) -> bool {
        self.rooms.iter().any(|r| !r.operators.is_empty())
    }

    pub fn operator_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        for room in &self.rooms {
            for op in &room.operators {
                names.insert(op.name.clone());
            }
        }
        names
    }
}
