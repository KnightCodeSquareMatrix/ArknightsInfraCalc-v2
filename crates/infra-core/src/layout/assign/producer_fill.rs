use std::collections::HashSet;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::operbox::OperBox;

const SENXI_DORM_CUISINE_BUFF: &str = "dorm_rec_bd_dungeon[000]";
const SPHINX_NAME: &str = "深巡";
const URRBIAN_NAME: &str = "乌尔比安";

/// 落位统一 plan 的体系 producer（感知链：夕中枢 / 絮雨办公室 / 爱丽丝·车尔尼宿舍）。
///
/// producer 由 `build_plan` 经 `evaluate_systems` 产出为 `ProducerSlot`（仅在拥有且达练度时
/// 出现），本函数按设施落位：中枢补位（满 5 跳过）、办公室/宿舍取首个空房。用真实 progress
/// 落位以保持效率不变；不在此重复 owns/elite 判定（已由体系层 gate）。
pub(super) fn place_system_producers(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    producers: &[crate::layout::orchestrate::ProducerSlot],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    for producer in producers {
        let name = producer.operator.as_str();
        if used.contains(name) {
            continue;
        }
        match producer.facility {
            FacilityKind::ControlCenter => {
                let control = assignment.control_operators();
                if control.len() >= 5 {
                    continue;
                }
                let op = operbox
                    .progress_of(name)
                    .map(|p| AssignedOperator::from_progress(name, p))
                    .unwrap_or_else(|| AssignedOperator::new(name, producer.elite));
                let mut ops = control;
                ops.push(op);
                used.insert(name.into());
                assignment.set_room(RoomId::from("control"), ops);
            }
            facility => {
                let Some(room) = blueprint
                    .rooms_of(facility)
                    .into_iter()
                    .find(|r| assignment.operators_in(&r.id).is_empty())
                else {
                    continue;
                };
                let op = operbox
                    .progress_of(name)
                    .map(|p| AssignedOperator::from_progress(name, p))
                    .unwrap_or_else(|| AssignedOperator::new(name, producer.elite));
                used.insert(name.into());
                assignment.set_room(room.id.clone(), vec![op]);
            }
        }
    }
}

pub(super) fn assign_dorm_producers(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    for room in blueprint.rooms_of(FacilityKind::Dormitory) {
        if !assignment.operators_in(&room.id).is_empty() {
            continue;
        }
        let Some((name, progress)) = best_dorm_producer(operbox, instances, used) else {
            continue;
        };
        used.insert(name.clone());
        assignment.set_room(
            room.id.clone(),
            vec![AssignedOperator::from_progress(name, progress)],
        );
    }
    Ok(())
}

/// 落位统一 plan 的体系 anchor（核心干员入房 + 计 used，队友留给后续 fill 补齐）。
///
/// 代码化体系层（如迷迭香）与 registry 汇合到 `AssignmentPlan.anchors` 后由本函数消费：
/// 迷迭香制造 anchor 落「首个空 factory」或指定 `room_id`；黑键不在此（走贸易贪心）。
/// producer（夕/絮雨/爱丽丝/车尔尼）已由 `place_system_producers` 落位，不在此重复。
pub(super) fn place_system_anchors(
    blueprint: &BaseBlueprint,
    anchors: &[crate::layout::orchestrate::SystemAnchor],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    for anchor in anchors {
        if used.contains(&anchor.operator) {
            continue;
        }
        let room_id = match &anchor.room_id {
            Some(id) if assignment.operators_in(id).is_empty() => Some(id.clone()),
            Some(_) => None,
            None => blueprint.rooms.iter().find_map(|r| {
                (r.kind == anchor.facility && assignment.operators_in(&r.id).is_empty())
                    .then(|| r.id.clone())
            }),
        };
        let Some(room_id) = room_id else {
            continue;
        };
        used.insert(anchor.operator.clone());
        assignment.set_room(
            room_id,
            vec![AssignedOperator::new(&anchor.operator, anchor.elite)],
        );
    }
}

pub(crate) fn assign_sphinx_urrbian_dorm_anchor(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    if !operbox.owns(SPHINX_NAME) || used.contains(URRBIAN_NAME) {
        return;
    }
    let Some(progress) = operbox.progress_of(URRBIAN_NAME) else {
        return;
    };
    let Some(room) = blueprint
        .rooms_of(FacilityKind::Dormitory)
        .into_iter()
        .find(|room| assignment.operators_in(&room.id).is_empty())
    else {
        return;
    };

    used.insert(URRBIAN_NAME.into());
    assignment.set_room(
        room.id.clone(),
        vec![AssignedOperator::from_progress(URRBIAN_NAME, progress)],
    );
}

pub(crate) fn cleanup_unused_sphinx_urrbian_dorm_anchor(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) {
    let sphinx_used = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::TradePost)
        .any(|room| {
            assignment
                .operators_in(&room.id)
                .iter()
                .any(|op| op.name == SPHINX_NAME)
        });
    if sphinx_used {
        return;
    }

    for room in &blueprint.rooms {
        if room.kind != FacilityKind::Dormitory {
            continue;
        }
        let ops = assignment.operators_in(&room.id);
        if ops.len() == 1 && ops[0].name == URRBIAN_NAME {
            assignment.set_room(room.id.clone(), Vec::new());
            used.remove(URRBIAN_NAME);
        }
    }
}

fn best_dorm_producer(
    operbox: &OperBox,
    instances: &OperatorInstances,
    used: &HashSet<String>,
) -> Option<(String, crate::roster::OperatorProgress)> {
    let mut best: Option<(String, crate::roster::OperatorProgress)> = None;
    for (name, progress) in &operbox.owned {
        if used.contains(name) || progress.elite < 2 {
            continue;
        }
        let tier = crate::tier::PromotionTier::from_progress(*progress);
        let buffs = instances.resolve_dorm_buff_ids(name, tier);
        if !buffs.iter().any(|b| b == SENXI_DORM_CUISINE_BUFF) {
            continue;
        }
        let replace = best.as_ref().is_none_or(|(_, best)| {
            progress.elite > best.elite
                || (progress.elite == best.elite && progress.level > best.level)
        });
        if replace {
            best = Some((name.clone(), *progress));
        }
    }
    best
}
