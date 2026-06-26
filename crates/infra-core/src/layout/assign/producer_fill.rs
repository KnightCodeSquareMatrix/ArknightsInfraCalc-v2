use std::collections::HashSet;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::operbox::OperBox;

const SENXI_DORM_CUISINE_BUFF: &str = "dorm_rec_bd_dungeon[000]";
const SPHINX_NAME: &str = "深巡";
const URRBIAN_NAME: &str = "乌尔比安";

/// 感知链 producer 落位（非编排 System）：黑键/迷迭香在盒时堆感知源，供 resolve + 贪心消费。
pub(super) fn assign_perception_producers(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    if !operbox.owns("黑键") || !operbox.owns("迷迭香") {
        return Ok(());
    }
    if operbox.owns("夕") && !used.contains("夕") {
        let control = assignment.control_operators();
        if control.len() < 5 {
            let progress = operbox.progress_of("夕").unwrap_or_default();
            let mut ops = control;
            ops.push(AssignedOperator::from_progress("夕", progress));
            used.insert("夕".into());
            assignment.set_room(RoomId::from("control"), ops);
        }
    }
    if operbox.elite_of("絮雨").unwrap_or(0) >= 2 && !used.contains("絮雨") {
        for room in blueprint.rooms_of(FacilityKind::Office) {
            if !assignment.operators_in(&room.id).is_empty() {
                continue;
            }
            used.insert("絮雨".into());
            let op = operbox
                .progress_of("絮雨")
                .map(|progress| AssignedOperator::from_progress("絮雨", progress))
                .unwrap_or_else(|| AssignedOperator::new("絮雨", 2));
            assignment.set_room(room.id.clone(), vec![op]);
            break;
        }
    }
    for name in ["爱丽丝", "车尔尼"] {
        if operbox.elite_of(name).unwrap_or(0) < 2 || used.contains(name) {
            continue;
        }
        let Some(room) = blueprint
            .rooms_of(FacilityKind::Dormitory)
            .into_iter()
            .find(|r| assignment.operators_in(&r.id).is_empty())
        else {
            continue;
        };
        used.insert(name.into());
        let op = operbox
            .progress_of(name)
            .map(|progress| AssignedOperator::from_progress(name, progress))
            .unwrap_or_else(|| AssignedOperator::new(name, 2));
        assignment.set_room(room.id.clone(), vec![op]);
    }
    Ok(())
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
