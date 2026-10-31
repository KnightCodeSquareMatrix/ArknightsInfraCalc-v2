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
    rooms: Vec<RoomAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub training_assist: Option<AssignedOperator>,
    /// 覆盖 blueprint.scenario.base_workforce。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_workforce: Vec<String>,
    #[serde(skip)]
    pub(crate) note_facts: crate::notes::NoteFacts,
    /// Construction diagnostics retained until the complete cycle is settled.
    #[serde(skip)]
    pub(crate) construction_warnings: Vec<String>,
    /// Execution fact submitted by dorm completion; wording never controls MAA.
    #[serde(skip)]
    pub(crate) dorm_autofill: bool,
    /// Room permutations committed by warmup continuity during construction.
    #[serde(skip)]
    pub(crate) rotation_room_moves: std::collections::HashMap<RoomId, RoomId>,
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

    /// 全部房间条目的只读视图，顺序即写入顺序。整班扫描（校验、结算、诊断）使用；
    /// 带明确意图的查询优先用 `room_of_operator`、`has_operator`、`rooms_of_kind`。
    pub fn iter_rooms(&self) -> impl Iterator<Item = &RoomAssignment> {
        self.rooms.iter()
    }

    /// 干员所在的房间条目；训练副手不在 rooms 内，返回 None。
    pub fn room_of_operator(&self, name: &str) -> Option<&RoomAssignment> {
        self.rooms
            .iter()
            .find(|room| room.operators.iter().any(|op| op.name == name))
    }

    /// 干员是否在任一房间里（不含独立的训练副手）。
    pub fn has_operator(&self, name: &str) -> bool {
        self.room_of_operator(name).is_some()
    }

    /// 蓝图中属于指定设施类型的房间条目，按 rooms 顺序；蓝图里不存在的房间被跳过。
    pub fn rooms_of_kind<'a>(
        &'a self,
        blueprint: &'a BaseBlueprint,
        kind: FacilityKind,
    ) -> impl Iterator<Item = &'a RoomAssignment> + 'a {
        self.rooms.iter().filter(move |room| {
            blueprint
                .room(&room.room_id)
                .is_some_and(|spec| spec.kind == kind)
        })
    }

    /// 整房移除：房间条目连同成员、效率、备注一起消失。
    pub fn remove_room(&mut self, room_id: &str) {
        self.rooms.retain(|room| room.room_id.0 != room_id);
    }

    /// 清掉某房间缓存的效率快照，成员不变。
    pub fn clear_room_efficiency(&mut self, room_id: &RoomId) {
        if let Some(room) = self.rooms.iter_mut().find(|room| &room.room_id == room_id) {
            room.efficiency = None;
        }
    }

    /// 清空全部房间的备注；备注只用于展示，不参与任何排班判断。
    pub fn clear_room_notes(&mut self) {
        for room in &mut self.rooms {
            room.notes.clear();
        }
    }

    /// 给指定房间追加一条备注；房间不存在时不创建。
    pub fn push_room_note(&mut self, room_id: &RoomId, note: Note) {
        if let Some(room) = self.rooms.iter_mut().find(|room| &room.room_id == room_id) {
            room.notes.push(note);
        }
    }

    /// 把满足条件的干员从所有房间移出，返回被移出的人数。
    /// 房间条目即使变空也保留，与此前各调用点 `retain` 的行为一致。
    pub fn remove_operators_where(
        &mut self,
        mut remove: impl FnMut(&RoomId, &AssignedOperator) -> bool,
    ) -> usize {
        let mut removed = 0;
        for room in &mut self.rooms {
            let before = room.operators.len();
            let room_id = &room.room_id;
            room.operators.retain(|operator| !remove(room_id, operator));
            removed += before - room.operators.len();
        }
        removed
    }

    /// 交换两个房间的成员与效率快照，房间条目顺序不变；任一房间不存在返回 false。
    pub fn swap_room_contents(&mut self, a: &RoomId, b: &RoomId) -> bool {
        let Some(ai) = self.rooms.iter().position(|room| &room.room_id == a) else {
            return false;
        };
        let Some(bi) = self.rooms.iter().position(|room| &room.room_id == b) else {
            return false;
        };
        if ai == bi {
            return true;
        }
        let (low, high) = if ai < bi { (ai, bi) } else { (bi, ai) };
        let (head, tail) = self.rooms.split_at_mut(high);
        let (left, right) = (&mut head[low], &mut tail[0]);
        std::mem::swap(&mut left.operators, &mut right.operators);
        std::mem::swap(&mut left.efficiency, &mut right.efficiency);
        true
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

    /// Normalize gold-flow entry order only in trade rooms, preserving other facilities' order.
    pub fn sort_trade_gold_flow_operators(&mut self, blueprint: &BaseBlueprint) {
        for room in &mut self.rooms {
            if blueprint
                .room(&room.room_id)
                .is_some_and(|blueprint_room| blueprint_room.kind == FacilityKind::TradePost)
                && crate::trade::gold_flow::has_ordered_gold_flow(
                    room.operators.iter().map(|operator| operator.name.as_str()),
                )
            {
                room.operators.sort_by_key(|operator| {
                    crate::trade::gold_flow::ordered_trio_priority(&operator.name)
                });
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

    /// 结构合法性的唯一检查：只看位置结构，不看技能、心情、体系或 Plan。
    /// 依次检查：房间存在于蓝图、同一房间不重复出现、成员数不超过房间容量
    /// （宿舍按床位）、同一干员只占一个位置（含独立的训练副手）。
    /// 返回全部违规，便于诊断；写入入口与终态 gate 共用本函数。
    pub fn structural_violations(&self, blueprint: &BaseBlueprint) -> Vec<String> {
        let mut violations = Vec::new();
        let mut seen_rooms = HashSet::new();
        let mut holders: HashMap<&str, &str> = HashMap::new();
        for room in &self.rooms {
            let room_id = room.room_id.0.as_str();
            if !seen_rooms.insert(room_id) {
                violations.push(format!("room_duplicated: {room_id}"));
            }
            let Some(spec) = blueprint.room(&room.room_id) else {
                if !room.operators.is_empty() {
                    violations.push(format!("room_unknown: {room_id}"));
                }
                continue;
            };
            let capacity = if spec.kind == FacilityKind::Dormitory {
                spec.dorm_bed_count() as usize
            } else {
                spec.operator_capacity()
            };
            if room.operators.len() > capacity {
                violations.push(format!(
                    "room_capacity: {room_id} has {} > {capacity}",
                    room.operators.len()
                ));
            }
            for operator in &room.operators {
                if let Some(first) = holders.insert(operator.name.as_str(), room_id) {
                    violations.push(format!(
                        "operator_unique: {} in {first} and {room_id}",
                        operator.name
                    ));
                }
            }
        }
        if let Some(assistant) = &self.training_assist {
            if let Some(room) = holders.get(assistant.name.as_str()) {
                violations.push(format!(
                    "operator_unique: {} in {room} and training_assist",
                    assistant.name
                ));
            }
        }
        violations
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
