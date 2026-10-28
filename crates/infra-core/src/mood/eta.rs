//! 班次 ETA：给定一套 assignment，算出每个在岗生产/功能干员的净消耗与可工作时长，
//! 班次 ETA = 所有在岗干员里最小的可工作时长（第一个耗尽心情的人决定换班）。
//!
//! 只统计工作设施（贸易/制造/发电/办公/中枢/会客）；宿舍是回复侧，不计入 ETA。
//! 跨设施组合由组内消耗最快者决定——这里自然通过「全局取 min」体现。

use serde::Serialize;

use crate::layout::{BaseAssignment, BaseBlueprint, FacilityKind, RoomProduct};
use crate::types::RecipeKind;

use super::drain::{operator_net_drain_with_sui, DrainInputs};
use super::{facility_key, MoodModel};

const ABYSSAL_CONTROL_RECOVERY_BUFF: &str = "control_mp_aegir1[000]";
const ABYSSAL_CONTROL_DRAIN_BUFF: &str = "control_mp_aegir2[010]";
const TAG_ABYSSAL: &str = "cc.g.abyssal";

/// 单个在岗干员的 ETA 明细。
#[derive(Debug, Clone, Serialize)]
pub struct OperatorEta {
    pub name: String,
    pub room_id: String,
    /// 净每小时心情消耗 `x`。
    pub drain_per_hour: f64,
    /// 满心情可工作时长（小时）；`x ≤ 0` 时为 `None`（在岗即回复，不成为瓶颈）。
    pub workable_hours: Option<f64>,
}

/// 一个班次的 ETA 报告。
#[derive(Debug, Clone, Serialize)]
pub struct ShiftEta {
    pub per_op: Vec<OperatorEta>,
    /// 瓶颈干员名（可工作时长最短者）；全员无限时为 `None`。
    pub bottleneck: Option<String>,
    /// 班次 ETA（小时）= 最短可工作时长；无有限瓶颈时为 `None`。
    pub eta_hours: Option<f64>,
}

fn recipe_key(recipe: RecipeKind) -> Option<&'static str> {
    match recipe {
        RecipeKind::Gold => Some("gold"),
        RecipeKind::BattleRecord => Some("battle_record"),
        RecipeKind::Originium => Some("originium"),
        RecipeKind::All => None,
    }
}

/// 计算一套 assignment 的班次 ETA。
pub fn shift_eta(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
) -> ShiftEta {
    shift_eta_inner(model, blueprint, assignment, None)
}

/// Contextual ETA used by scheduling. Operator capabilities and actual
/// outside-dorm presence are needed for conditional control-center mood rules.
pub fn shift_eta_with_instances(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &crate::instances::OperatorInstances,
) -> ShiftEta {
    shift_eta_inner(model, blueprint, assignment, Some(instances))
}

/// ETA with optional operator-instance context. `None` skips instance-driven
/// rules (sui / abyssal) and otherwise matches [`shift_eta_with_instances`].
pub fn shift_eta_in_context(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&crate::instances::OperatorInstances>,
) -> ShiftEta {
    shift_eta_inner(model, blueprint, assignment, instances)
}

impl ShiftEta {
    /// Per-operator detail for `name` in `room_id`, if that operator works there.
    pub fn operator_in_room(&self, name: &str, room_id: &str) -> Option<&OperatorEta> {
        self.per_op
            .iter()
            .find(|op| op.name == name && op.room_id == room_id)
    }
}

/// Rotation budget: an operator with `available_mood` may work `hours` at
/// `drain_per_hour` and still sit at the rest threshold plus `reserve_drain`
/// (mood consumed by a following shift) afterwards. Not clamped at the threshold.
pub fn fits_shift_budget(
    model: &MoodModel,
    available_mood: f64,
    drain_per_hour: f64,
    hours: f64,
    reserve_drain: f64,
) -> bool {
    available_mood + 1e-9 >= model.rest_threshold + drain_per_hour * hours + reserve_drain
}

/// Single-shift work budget: mood above the rest threshold (clamped at 0) must
/// cover `drain_per_hour * hours`.
pub fn fits_work_budget(
    model: &MoodModel,
    available_mood: f64,
    drain_per_hour: f64,
    hours: f64,
) -> bool {
    let budget = (available_mood - model.rest_threshold).max(0.0);
    drain_per_hour * hours <= budget + 1e-9
}

fn shift_eta_inner(
    model: &MoodModel,
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: Option<&crate::instances::OperatorInstances>,
) -> ShiftEta {
    // 中枢是否满 5 人 → 全局 −0.25。
    let full_control = assignment
        .iter_rooms()
        .find(|r| {
            blueprint
                .room(&r.room_id)
                .is_some_and(|bp| bp.kind == FacilityKind::ControlCenter)
        })
        .is_some_and(|r| r.operators.len() >= 5);

    let control_ops: Vec<(String, u8)> = assignment
        .control_operators()
        .iter()
        .map(|o| (o.name.clone(), o.elite))
        .collect();

    let abyssal_outside_dorm = instances.map_or(0, |instances| {
        assignment
            .iter_rooms()
            .filter(|room| {
                blueprint
                    .room(&room.room_id)
                    .is_some_and(|room| room.kind != FacilityKind::Dormitory)
            })
            .flat_map(|room| &room.operators)
            .filter(|operator| {
                instances
                    .tags_for(&operator.name, operator.tier())
                    .iter()
                    .any(|tag| tag == TAG_ABYSSAL)
            })
            .count()
    });
    let abyssal_control_recovery = instances.is_some_and(|instances| {
        assignment.control_operators().iter().any(|operator| {
            instances
                .resolve_control_buff_ids(&operator.name, operator.tier())
                .iter()
                .any(|buff_id| buff_id == ABYSSAL_CONTROL_RECOVERY_BUFF)
        })
    });

    let mut per_op: Vec<OperatorEta> = Vec::new();

    for room in assignment.iter_rooms() {
        let Some(bp) = blueprint.room(&room.room_id) else {
            continue;
        };
        // 只算工作设施（能映射到 mood 设施键的）；宿舍/训练/加工跳过。
        if facility_key(bp.kind).is_none() || bp.kind == FacilityKind::Dormitory {
            continue;
        }
        let recipe = match bp.product.as_ref() {
            Some(RoomProduct::Factory { recipe }) => recipe_key(*recipe),
            _ => None,
        };
        let room_ops: Vec<(String, u8)> = room
            .operators
            .iter()
            .map(|o| (o.name.clone(), o.elite))
            .collect();

        for op in &room.operators {
            let inputs = DrainInputs {
                name: &op.name,
                elite: op.elite,
                facility: bp.kind,
                room_ops: &room_ops,
                full_control,
                control_ops: &control_ops,
                recipe,
            };
            let sui_member = instances.is_some_and(|instances| {
                instances
                    .tags_for(&op.name, op.tier())
                    .iter()
                    .any(|tag| tag == "cc.g.sui")
            });
            let mut x = operator_net_drain_with_sui(model, &inputs, sui_member);
            if bp.kind == FacilityKind::ControlCenter {
                if abyssal_control_recovery {
                    x -= 0.05;
                }
                if instances.is_some_and(|instances| {
                    instances
                        .resolve_control_buff_ids(&op.name, op.tier())
                        .iter()
                        .any(|buff_id| buff_id == ABYSSAL_CONTROL_DRAIN_BUFF)
                }) {
                    x += abyssal_outside_dorm as f64 * 0.5;
                }
                x = x.max(0.0);
            }
            let hours = if x <= 0.0 {
                None
            } else {
                Some(model.workable_mood() / x)
            };
            per_op.push(OperatorEta {
                name: op.name.clone(),
                room_id: room.room_id.0.clone(),
                drain_per_hour: x,
                workable_hours: hours,
            });
        }
    }

    // 瓶颈：可工作时长最短者。
    let bottleneck_op = per_op
        .iter()
        .filter_map(|o| o.workable_hours.map(|h| (o.name.clone(), h)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let (bottleneck, eta_hours) = match bottleneck_op {
        Some((name, h)) => (Some(name), Some(h)),
        None => (None, None),
    };

    ShiftEta {
        per_op,
        bottleneck,
        eta_hours,
    }
}
