use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomId};
use crate::layout::context::LayoutContext;
use crate::pool::{try_filter_standalone, PowerPool};
use crate::search::{search_power_assignment, PowerSearchOptions};
use crate::skill_table::SkillTable;

use super::commit::power_efficiency_snapshot;
use super::AssignBaseOptions;

/// 填满蓝图全部空发电站（每站 1 人、贪心取可用最优）；跨班复用，受 `used` 约束。
pub fn assign_power_stations(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let room_ids: Vec<RoomId> = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::PowerPlant)
        .map(|r| r.id.clone())
        .collect();
    assign_power_rooms(
        blueprint, pool, table, layout, options, &room_ids, assignment, used,
    )
}

/// 填满指定发电站（每站 1 人、贪心取可用最优）；供三队轮换按半区分配。
#[allow(clippy::too_many_arguments)]
pub fn assign_power_rooms(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
) -> Result<()> {
    let total_stations = blueprint
        .rooms
        .iter()
        .filter(|r| r.kind == FacilityKind::PowerPlant)
        .count();
    if total_stations == 0 || rooms.is_empty() {
        return Ok(());
    }

    let power_opts = PowerSearchOptions {
        station_count: total_stations.min(255) as u8,
        mood: options.mood,
        shift_hours: options.shift_hours,
        layout: layout.clone(),
    };

    let empty_rooms: Vec<RoomId> = rooms
        .iter()
        .filter(|room_id| {
            blueprint
                .room(room_id)
                .is_some_and(|r| r.kind == FacilityKind::PowerPlant)
                && assignment.operators_in(room_id).is_empty()
        })
        .cloned()
        .collect();
    if empty_rooms.is_empty() {
        return Ok(());
    }

    let sub = filter_power_pool(pool, used);
    let sub = try_filter_standalone(&sub, FacilityKind::PowerPlant, 1);
    if sub.entries.is_empty() {
        return Err(Error::msg("power: no available operators"));
    }

    let mut opts = power_opts;
    opts.station_count = empty_rooms.len().min(255) as u8;
    let report = search_power_assignment(&sub, table, &opts)?;
    if report.assignments.len() != empty_rooms.len() {
        return Err(Error::msg(format!(
            "power: expected {} assignments, got {}",
            empty_rooms.len(),
            report.assignments.len()
        )));
    }

    for (room_id, station) in empty_rooms.iter().zip(report.assignments.iter()) {
        let op = pool
            .entry(&station.hit.name)
            .map(|e| AssignedOperator::from_progress(&station.hit.name, e.progress))
            .unwrap_or_else(|| AssignedOperator::new(&station.hit.name, 0));
        if !used.insert(station.hit.name.clone()) {
            return Err(Error::msg(format!(
                "power {}: duplicate {}",
                room_id.0, station.hit.name
            )));
        }
        assignment.set_room_with_efficiency(
            room_id.clone(),
            vec![op],
            Some(power_efficiency_snapshot(&station.hit)),
        );
    }
    Ok(())
}

fn filter_power_pool(pool: &PowerPool, exclude: &HashSet<String>) -> PowerPool {
    PowerPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| !exclude.contains(&e.name))
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}
