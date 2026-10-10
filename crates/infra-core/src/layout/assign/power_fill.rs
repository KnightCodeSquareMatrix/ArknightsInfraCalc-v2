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
    assign_power_rooms_with_instances(
        blueprint, pool, table, layout, options, rooms, assignment, used, None, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assign_power_stations_with_instances(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    instances: &crate::instances::OperatorInstances,
) -> Result<()> {
    let rooms: Vec<_> = blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::PowerPlant)
        .map(|room| room.id.clone())
        .collect();
    assign_power_rooms_with_instances(
        blueprint,
        pool,
        table,
        layout,
        options,
        &rooms,
        assignment,
        used,
        Some(instances),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assign_power_rooms_with_instances(
    blueprint: &BaseBlueprint,
    pool: &PowerPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    rooms: &[RoomId],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    instances: Option<&crate::instances::OperatorInstances>,
    coexist_assignment: Option<&BaseAssignment>,
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

    let mut full_pool = filter_power_pool(pool, used);
    full_pool.entries.retain(|entry| !entry.buff_ids.is_empty());
    let sub = try_filter_standalone(&full_pool, FacilityKind::PowerPlant, empty_rooms.len());
    let sub = if sub.entries.len() < empty_rooms.len() {
        full_pool
    } else {
        sub
    };
    let mut opts = power_opts;
    opts.station_count = empty_rooms.len().min(255) as u8;
    let report = search_power_assignment(&sub, table, &opts)?;
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
    if report.assignments.len() < empty_rooms.len() {
        let mood = crate::mood::default_mood_model()?;
        let mut fillers: Vec<_> = pool
            .entries
            .iter()
            .filter(|entry| entry.buff_ids.is_empty())
            .collect();
        // Preserve scarce trade/manufacture skills while filling an unskilled power seat.
        fillers.sort_by_key(|entry| {
            let productive = instances.is_some_and(|instances| {
                let tier = crate::tier::PromotionTier::from_progress(entry.progress);
                !instances
                    .resolve_trade_buff_ids(&entry.name, tier)
                    .is_empty()
                    || !instances
                        .resolve_manufacture_buff_ids(&entry.name, tier)
                        .is_empty()
            });
            (productive, entry.name.as_str())
        });
        for room_id in empty_rooms.iter().skip(report.assignments.len()) {
            let mut placed = false;
            for entry in &fillers {
                if used.contains(&entry.name) {
                    continue;
                }
                let op = AssignedOperator::from_progress(&entry.name, entry.progress);
                let mut trial = coexist_assignment.cloned().unwrap_or_default();
                for room in &assignment.rooms {
                    trial.set_room(room.room_id.clone(), room.operators.clone());
                }
                trial.set_room(room_id.clone(), vec![op.clone()]);
                let eta = crate::mood::shift_eta_in_context(&mood, blueprint, &trial, instances);
                let Some(candidate_eta) = eta.operator_in_room(&entry.name, &room_id.0) else {
                    continue;
                };
                if !crate::mood::fits_work_budget(
                    &mood,
                    options.mood,
                    candidate_eta.drain_per_hour,
                    options.shift_hours,
                ) {
                    continue;
                }
                // Settlement reads the final assignment, including empty power buff IDs.
                assignment.set_room(room_id.clone(), vec![op]);
                used.insert(entry.name.clone());
                placed = true;
                break;
            }
            if !placed {
                return Err(Error::msg(format!(
                    "power: no eligible spare operator for {} after skill search",
                    room_id.0,
                )));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::blueprint::RoomBlueprint;
    use crate::layout::tier::OperatorTier;
    use crate::pool::PowerPoolEntry;
    use crate::roster::OperatorProgress;

    fn power_entry(name: &str, skill_id: &str, hint: f64) -> PowerPoolEntry {
        PowerPoolEntry {
            name: name.to_string(),
            elite: 2,
            progress: OperatorProgress::elite_only(2),
            buff_ids: vec![skill_id.to_string()],
            tags: vec![],
            flat_charge_hint: hint,
            has_l2_delegate: false,
            tier: OperatorTier::Standalone,
        }
    }

    #[test]
    fn power_filter_falls_back_when_whitelist_cannot_fill_all_rooms() {
        let blueprint = BaseBlueprint {
            template: Some("test_243_power".to_string()),
            drone_cap: 135,
            scenario: Default::default(),
            rooms: vec![
                RoomBlueprint {
                    id: RoomId::from("power_1"),
                    kind: FacilityKind::PowerPlant,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("power_2"),
                    kind: FacilityKind::PowerPlant,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
                RoomBlueprint {
                    id: RoomId::from("power_3"),
                    kind: FacilityKind::PowerPlant,
                    level: 3,
                    product: None,
                    dorm_beds: None,
                    dorm_ambience_level: None,
                },
            ],
        };
        let pool = PowerPool {
            entries: vec![
                power_entry("承曦格雷伊", "power_rec_drone[000]", 0.0),
                power_entry("格雷伊", "power_rec_spd[020]", 20.0),
                power_entry("Friston-3", "power_rec_spd[000]", 10.0),
            ],
            skipped: vec![],
        };
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_power_stations(
            &blueprint,
            &pool,
            &table,
            &LayoutContext::default(),
            &AssignBaseOptions::default(),
            &mut assignment,
            &mut used,
        )
        .unwrap();

        assert_eq!(assignment.rooms.len(), 3);
        assert_eq!(used.len(), 3);
        assert!(used.contains("Friston-3"));
    }

    #[test]
    fn power_filter_prefers_ordinary_chargers_before_friston() {
        let blueprint = BaseBlueprint {
            template: Some("test_243_power".to_string()),
            drone_cap: 135,
            scenario: Default::default(),
            rooms: vec![RoomBlueprint {
                id: RoomId::from("power_1"),
                kind: FacilityKind::PowerPlant,
                level: 3,
                product: None,
                dorm_beds: None,
                dorm_ambience_level: None,
            }],
        };
        let mut friston = power_entry("Friston-3", "power_rec_spd[000]", 10.0);
        friston.buff_ids.push("power_rec_spd_P[000]".to_string());
        let pool = PowerPool {
            entries: vec![
                friston,
                power_entry("阿消", "power_rec_spd[010]", 15.0),
                power_entry("布丁", "power_rec_spd[010]", 15.0),
            ],
            skipped: vec![],
        };
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let mut layout = LayoutContext::default();
        layout.base_workforce = vec!["凯尔希".to_string()];
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();

        assign_power_stations(
            &blueprint,
            &pool,
            &table,
            &layout,
            &AssignBaseOptions::default(),
            &mut assignment,
            &mut used,
        )
        .unwrap();

        let ops = assignment.operators_in(&RoomId::from("power_1"));
        assert_eq!(ops.len(), 1);
        assert_ne!(
            ops[0].name, "Friston-3",
            "Friston-3 + 凯尔希只有 15%，常规发电补位应优先普通 15% 散件"
        );
    }
}
