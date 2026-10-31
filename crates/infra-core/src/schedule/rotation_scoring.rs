use crate::layout::{BaseAssignment, BaseBlueprint, FacilityKind};

pub(super) fn clear_production_efficiencies(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
) {
    for room in &blueprint.rooms {
        if matches!(
            room.kind,
            FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
        ) {
            assignment.clear_room_efficiency(&room.id);
        }
    }
}
