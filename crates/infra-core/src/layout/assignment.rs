use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

use crate::layout::blueprint::RoomId;
use crate::tier::PromotionTier;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignedOperator {
    pub name: String,
    pub elite: u8,
}

impl AssignedOperator {
    pub fn new(name: impl Into<String>, elite: u8) -> Self {
        Self {
            name: name.into(),
            elite,
        }
    }

    pub fn tier(&self) -> PromotionTier {
        PromotionTier::from_elite(self.elite)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomAssignment {
    pub room_id: RoomId,
    #[serde(default)]
    pub operators: Vec<AssignedOperator>,
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

    pub fn operators_in(&self, room_id: &RoomId) -> &[AssignedOperator] {        self.rooms
            .iter()
            .find(|r| &r.room_id == room_id)
            .map(|r| r.operators.as_slice())
            .unwrap_or(&[])
    }

    pub fn set_room(&mut self, room_id: impl Into<RoomId>, operators: Vec<AssignedOperator>) {
        let room_id = room_id.into();
        if let Some(entry) = self.rooms.iter_mut().find(|r| r.room_id == room_id) {
            entry.operators = operators;
            return;
        }
        self.rooms.push(RoomAssignment {
            room_id,
            operators,
        });
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
}
