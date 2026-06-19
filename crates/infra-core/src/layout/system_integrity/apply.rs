//! 将 `evaluate_*` 产出的声明式计划写入 `BaseAssignment`（不含 trade search 评分）。

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};

use super::plan::{OptionalProducer, RosemaryPlan, SystemAnchor};

/// 迷迭香链计划：钉制造锚点 + 可选 producer（缺房/满员时静默跳过 optional）。
/// 黑键不进贸易锚点，由 resolve 后贸易贪心 + 感知 layout 自然选型。
pub fn apply_rosemary_plan(
    blueprint: &BaseBlueprint,
    plan: &RosemaryPlan,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for anchor in &plan.anchors {
        place_anchor(blueprint, anchor, assignment, used)?;
    }
    for producer in &plan.optional_producers {
        place_optional_producer(blueprint, producer, assignment, used);
    }
    Ok(())
}

fn place_anchor(
    blueprint: &BaseBlueprint,
    anchor: &SystemAnchor,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    if used.contains(&anchor.operator) {
        return Ok(());
    }
    let room_id = find_room(blueprint, assignment, anchor.facility, anchor.room_id.as_ref())
        .ok_or_else(|| {
            Error::msg(format!(
                "rosemary anchor {}: no {} room",
                anchor.operator,
                facility_label(anchor.facility)
            ))
        })?;
    if !used.insert(anchor.operator.clone()) {
        return Err(Error::msg(format!("rosemary duplicate {}", anchor.operator)));
    }
    if anchor.facility == FacilityKind::ControlCenter {
        let mut existing = assignment.control_operators();
        existing.push(AssignedOperator::new(&anchor.operator, anchor.elite));
        assignment.set_room(RoomId::from("control"), existing);
    } else {
        assignment.set_room(
            room_id,
            vec![AssignedOperator::new(&anchor.operator, anchor.elite)],
        );
    }
    Ok(())
}

fn place_optional_producer(
    blueprint: &BaseBlueprint,
    producer: &OptionalProducer,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    if used.contains(&producer.operator) {
        return;
    }
    let Some(room_id) = find_room(blueprint, assignment, producer.facility, None) else {
        return;
    };
    if producer.facility == FacilityKind::ControlCenter {
        let existing = assignment.control_operators();
        if existing.len() >= 5 {
            return;
        }
    }
    used.insert(producer.operator.clone());
    if producer.facility == FacilityKind::ControlCenter {
        let mut existing = assignment.control_operators();
        existing.push(AssignedOperator::new(&producer.operator, producer.elite));
        assignment.set_room(RoomId::from("control"), existing);
    } else {
        assignment.set_room(
            room_id,
            vec![AssignedOperator::new(&producer.operator, producer.elite)],
        );
    }
}

fn find_room(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    facility: FacilityKind,
    room_id: Option<&RoomId>,
) -> Option<RoomId> {
    if let Some(id) = room_id {
        if assignment.operators_in(id).is_empty() {
            return Some(id.clone());
        }
        return None;
    }
    blueprint.rooms.iter().find_map(|r| {
        if r.kind != facility {
            return None;
        }
        if facility == FacilityKind::ControlCenter {
            (assignment.operators_in(&r.id).len() < 5).then(|| r.id.clone())
        } else {
            assignment.operators_in(&r.id).is_empty().then(|| r.id.clone())
        }
    })
}

fn facility_label(kind: FacilityKind) -> &'static str {
    match kind {
        FacilityKind::ControlCenter => "control",
        FacilityKind::TradePost => "trade_post",
        FacilityKind::Factory => "factory",
        FacilityKind::PowerPlant => "power_plant",
        FacilityKind::Dormitory => "dormitory",
        FacilityKind::Office => "office",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::shift::AssignShiftMode;
    use crate::layout::system_integrity::{evaluate_rosemary, EvaluateContext};
    use crate::operbox::OperBox;

    #[test]
    fn apply_rosemary_plan_places_rosemary_factory_anchor_not_blackkey_trade() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let path = crate::skill_table::data_path("fixtures/243/operbox_full_e2.json").unwrap();
        let operbox = OperBox::load(&path).unwrap();
        let ctx = EvaluateContext::new(&blueprint, &operbox, AssignShiftMode::Peak);
        let plan = match evaluate_rosemary(&ctx) {
            crate::layout::system_integrity::RosemaryVerdict::Activate(p) => p,
            other => panic!("expected activate, got {other:?}"),
        };

        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        apply_rosemary_plan(&blueprint, &plan, &mut assignment, &mut used).unwrap();

        assert!(!assignment.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::TradePost)
                && r.operators.iter().any(|o| o.name == "黑键")
        }));
        assert!(assignment.rooms.iter().any(|r| {
            blueprint
                .rooms
                .iter()
                .any(|b| b.id == r.room_id && b.kind == FacilityKind::Factory)
                && r.operators.iter().any(|o| o.name == "迷迭香")
        }));
        assert!(assignment.control_operators().iter().any(|o| o.name == "夕"));
    }
}
