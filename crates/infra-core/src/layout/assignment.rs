use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::efficiency::Efficiency;
use crate::error::{Error, Result};

use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::notes::Note;
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
    /// 仅当体系显式要求特殊工作状态时覆盖本班全局心情。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_mood: Option<u8>,
}

impl AssignedOperator {
    pub fn new(name: impl Into<String>, elite: u8) -> Self {
        Self {
            name: name.into(),
            elite,
            level: 0,
            rarity: 0,
            work_mood: None,
        }
    }

    pub fn from_progress(name: impl Into<String>, progress: OperatorProgress) -> Self {
        Self {
            name: name.into(),
            elite: progress.elite,
            level: progress.level,
            rarity: progress.rarity,
            work_mood: None,
        }
    }

    pub fn with_work_mood(mut self, mood: Option<u8>) -> Self {
        self.work_mood = mood;
        self
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

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero_efficiency(v: &Efficiency) -> bool {
    v.is_zero()
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoomEfficiencySnapshot {
    /// 统一口径的基础效率：100% + 非 0 心情入驻人数 × 1%。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_base_efficiency: Efficiency,
    /// 完整总效率：基础 + 技能 + 中枢/全局。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_paper_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_occupancy_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_control_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_total_efficiency: Efficiency,
    /// 社区加强单位产出相对三级普通贸易站的倍率。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_unit_output_multiplier: Efficiency,
    /// 旧贸易倍率后效率，仅保留供旧快照兼容；排班与 policy 不消费该字段。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    #[serde(skip_serializing)]
    pub trade_final_efficiency: Efficiency,
    /// 旧倍率后效率反算的等效技能加成；新路径仅用于审计兼容。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_equivalent_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_equivalent_trade_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_equivalent_gold_efficiency: Efficiency,
    /// 1:1 贸易/赤金归一化后的单房诊断值，不参与默认排序。
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_combined_output_value: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub trade_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    #[serde(skip_serializing)]
    pub gold_consumption_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    #[serde(skip_serializing)]
    pub trade_originium_shards_per_day: f64,
    /// 命中的社区规则或 shortcut ID，用于产出审计。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub trade_mechanic_equivalent_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    #[serde(skip_serializing)]
    pub manufacture_final_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_base_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_occupancy_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_global_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_skill_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    pub manufacture_total_efficiency: Efficiency,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manufacture_unit_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub manufacture_output_per_day: f64,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub manufacture_storage_limit: i32,
    #[serde(default, skip_serializing_if = "is_zero_efficiency")]
    #[serde(skip_serializing)]
    pub power_final_efficiency: Efficiency,
}

impl RoomEfficiencySnapshot {
    pub fn is_trade(&self) -> bool {
        !self.trade_total_efficiency.is_zero()
    }

    pub fn is_manufacture(&self) -> bool {
        !self.manufacture_total_efficiency.is_zero()
    }

    pub fn is_power(&self) -> bool {
        !self.power_final_efficiency.is_zero()
    }

    /// New room projections may reuse only snapshots carrying their direct
    /// station components. Older assignments are recalculated by the solver.
    pub fn has_trade_breakdown(&self) -> bool {
        !self.trade_paper_efficiency.is_zero()
            && !self.trade_occupancy_efficiency.is_zero()
            && !self.trade_total_efficiency.is_zero()
            && !self.trade_unit_output_multiplier.is_zero()
    }

    pub fn has_manufacture_breakdown(&self) -> bool {
        !self.manufacture_total_efficiency.is_zero()
            && !self.manufacture_occupancy_efficiency.is_zero()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomAssignment {
    pub room_id: RoomId,
    #[serde(default)]
    pub operators: Vec<AssignedOperator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency: Option<RoomEfficiencySnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
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
    #[serde(skip)]
    pub(crate) note_facts: crate::notes::NoteFacts,
    /// Frozen scheduling inputs for the final-state control gate; not a second
    /// selected-members list and never part of the public assignment protocol.
    #[serde(skip)]
    pub(crate) control_rotation_admission:
        Option<crate::schedule::control_rotation::ControlShiftAdmission>,
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
            entry.notes.clear();
            return;
        }
        self.rooms.push(RoomAssignment {
            room_id,
            operators,
            efficiency,
            notes: Vec::new(),
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

    /// Normalize presentation and export order without changing room membership.
    pub fn sort_room_operators_stably(&mut self) {
        for room in &mut self.rooms {
            room.operators.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| right.elite.cmp(&left.elite))
                    .then_with(|| right.level.cmp(&left.level))
                    .then_with(|| right.rarity.cmp(&left.rarity))
                    .then_with(|| left.work_mood.cmp(&right.work_mood))
            });
            if crate::trade::gold_flow::has_ordered_gold_flow(
                room.operators.iter().map(|op| op.name.as_str()),
            ) {
                room.operators
                    .sort_by_key(|op| crate::trade::gold_flow::ordered_trio_priority(&op.name));
            }
        }
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

    /// 占用口径：任意房间（含宿舍）成员加独立的训练副手。
    pub fn occupied_operator_names(&self) -> HashSet<String> {
        let mut names = self.operator_names();
        if let Some(assistant) = &self.training_assist {
            names.insert(assistant.name.clone());
        }
        names
    }

    /// 工作岗位口径的唯一 owner：除宿舍外所有房间（中枢、贸易、制造、发电、会客室、
    /// 办公室、加工站、训练室）的成员，加独立的 `training_assist` 训练副手。
    /// 不读取队伍标签、shift_bind、required 或豁免；未知房间不计入。
    pub fn work_position_operators<'a>(
        &'a self,
        blueprint: &'a BaseBlueprint,
    ) -> impl Iterator<Item = &'a AssignedOperator> + 'a {
        self.rooms
            .iter()
            .filter(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|spec| spec.kind != FacilityKind::Dormitory)
            })
            .flat_map(|room| room.operators.iter())
            .chain(self.training_assist.iter())
    }

    pub fn work_position_operator_names(&self, blueprint: &BaseBlueprint) -> HashSet<String> {
        self.work_position_operators(blueprint)
            .map(|operator| operator.name.clone())
            .collect()
    }

    pub fn is_work_position_operator(&self, blueprint: &BaseBlueprint, name: &str) -> bool {
        self.work_position_operators(blueprint)
            .any(|operator| operator.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_snapshot_exposes_total_efficiency_separately_from_legacy_final() {
        let snapshot = RoomEfficiencySnapshot {
            trade_final_efficiency: Efficiency::from_decimal(1.907),
            trade_total_efficiency: Efficiency::from_decimal(1.230),
            ..RoomEfficiencySnapshot::default()
        };
        assert_eq!(
            snapshot.trade_total_efficiency,
            Efficiency::from_millis(1230)
        );
        assert_eq!(
            snapshot.trade_final_efficiency,
            Efficiency::from_millis(1907)
        );
    }

    #[test]
    fn trade_snapshot_roundtrip_preserves_audit_fields() {
        let snapshot = RoomEfficiencySnapshot {
            trade_paper_efficiency: Efficiency::from_decimal(1.230),
            trade_unit_output_multiplier: Efficiency::from_decimal(1.550),
            trade_final_efficiency: Efficiency::from_decimal(1.907),
            trade_total_efficiency: Efficiency::from_decimal(1.230),
            trade_equivalent_skill_efficiency: Efficiency::from_decimal(0.807),
            trade_rule_id: Some("gsl_docus_solo".to_string()),
            ..RoomEfficiencySnapshot::default()
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: RoomEfficiencySnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(
            decoded.trade_paper_efficiency,
            Efficiency::from_decimal(1.230)
        );
        assert_eq!(
            decoded.trade_unit_output_multiplier,
            Efficiency::from_decimal(1.550)
        );
        assert_eq!(
            decoded.trade_total_efficiency,
            Efficiency::from_decimal(1.230)
        );
        assert_eq!(decoded.trade_final_efficiency, Efficiency::ZERO);
        assert_eq!(decoded.trade_rule_id.as_deref(), Some("gsl_docus_solo"));
    }

    #[test]
    fn room_operator_order_is_stable_without_changing_membership() {
        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "trade_1",
            vec![
                AssignedOperator::new("银灰", 2),
                AssignedOperator::new("但书", 2),
                AssignedOperator::new("孑", 2),
            ],
        );

        assignment.sort_room_operators_stably();

        assert_eq!(
            assignment
                .operators_in(&RoomId::from("trade_1"))
                .iter()
                .map(|operator| operator.name.as_str())
                .collect::<Vec<_>>(),
            vec!["但书", "孑", "银灰"]
        );
    }
}
