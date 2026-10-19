use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::{BaseAssignment, BaseBlueprint, FacilityKind};
use crate::skill_table::SkillTable;

use super::base_rotation::evaluate_base_assignment_efficiencies;
use super::TeamShiftResult;

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

// Ordinary full-window scoring only; continuation windows retain their own policy.
pub(super) fn rescore_standard_shifts(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    durin_plan: u8,
    cycle_hours: f64,
    shifts: &mut [TeamShiftResult],
) -> Result<()> {
    for shift in shifts {
        clear_production_efficiencies(blueprint, &mut shift.assignment);
        shift.efficiencies = evaluate_base_assignment_efficiencies(
            blueprint,
            &shift.assignment,
            instances,
            table,
            shift.duration_hours,
            Some(durin_plan),
        )?;
        shift.weighted_trade = shift
            .efficiencies
            .weighted_trade_for_cycle(shift.duration_hours, cycle_hours);
        shift.weighted_manufacture = shift
            .efficiencies
            .weighted_manufacture_for_cycle(shift.duration_hours, cycle_hours);
        shift.weighted_power = shift
            .efficiencies
            .weighted_power_for_cycle(shift.duration_hours, cycle_hours);
    }
    Ok(())
}
