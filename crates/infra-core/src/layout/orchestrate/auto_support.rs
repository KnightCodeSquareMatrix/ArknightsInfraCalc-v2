//! 自动建议只登记显式宿舍角色的归属，不发现潜在技能关系。
use std::collections::BTreeMap;

use super::plan::{ActiveDependency, SystemAnchor};
use crate::layout::{BaseAssignment, FacilityKind, RoomId};

pub(crate) fn claim_support_rooms(
    assignment: &BaseAssignment,
    anchors: &[SystemAnchor],
    dependencies: &[ActiveDependency],
    inherited: &BTreeMap<String, RoomId>,
) -> Option<BTreeMap<String, RoomId>> {
    let mut rooms = inherited.clone();
    let mut receivers = BTreeMap::new();
    let mut supports = BTreeMap::new();
    for dependency in dependencies {
        for support in &dependency.required {
            let Some(anchor) = anchors.iter().find(|anchor| {
                anchor.operator == *support
                    && anchor.proactive_support
                    && anchor.facility == FacilityKind::Dormitory
            }) else {
                continue;
            };
            let dorm = anchor.room_id.as_ref()?;
            if !assignment
                .operators_in(dorm)
                .iter()
                .any(|op| op.name == *support)
            {
                return None;
            }
            for receiver in &dependency.consumers {
                let room = assignment
                    .iter_rooms()
                    .find(|room| room.operators.iter().any(|op| op.name == *receiver))?;
                if receivers
                    .insert(support.clone(), receiver.clone())
                    .is_some_and(|old| old != *receiver)
                    || supports
                        .insert(receiver.clone(), support.clone())
                        .is_some_and(|old| old != *support)
                    || rooms.get(support).is_some_and(|old| *old != room.room_id)
                {
                    return None;
                }
                rooms.insert(support.clone(), room.room_id.clone());
            }
        }
    }
    Some(rooms)
}
