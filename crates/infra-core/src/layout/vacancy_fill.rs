use std::collections::HashSet;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::pool::{
    build_control_pool, build_manufacture_pool, build_power_pool, build_trade_pool,
    filter_general_manufacture_search_pool,
};
use crate::roster::{OperatorProgress, Roster};
use crate::skill_table::SkillTable;
use crate::support_facility::{SupportFacility, SupportRegistry};

use super::{AssignedOperator, BaseAssignment, BaseBlueprint, FacilityKind, RoomBlueprint, RoomId};

pub(crate) fn ordinary_workplace(kind: FacilityKind) -> bool {
    matches!(
        kind,
        FacilityKind::ControlCenter
            | FacilityKind::TradePost
            | FacilityKind::Factory
            | FacilityKind::PowerPlant
            | FacilityKind::MeetingRoom
            | FacilityKind::Office
            | FacilityKind::Recycle
    )
}

fn candidate_is_skillless(
    kind: FacilityKind,
    candidate: &AssignedOperator,
    instances: &OperatorInstances,
    registry: &SupportRegistry,
) -> bool {
    let tier = candidate.tier();
    match kind {
        FacilityKind::ControlCenter => instances
            .resolve_control_buff_ids(&candidate.name, tier)
            .is_empty(),
        FacilityKind::TradePost => instances
            .resolve_trade_buff_ids(&candidate.name, tier)
            .is_empty(),
        FacilityKind::Factory => instances
            .resolve_manufacture_buff_ids(&candidate.name, tier)
            .is_empty(),
        FacilityKind::PowerPlant => instances
            .resolve_power_buff_ids(&candidate.name, tier)
            .is_empty(),
        FacilityKind::Recycle => instances
            .resolve_recycle_buff_ids(&candidate.name, tier)
            .is_empty(),
        FacilityKind::Office | FacilityKind::MeetingRoom => !registry.entries.iter().any(|entry| {
            entry.facility
                == if kind == FacilityKind::Office {
                    SupportFacility::Office
                } else {
                    SupportFacility::Meeting
                }
                && entry.operator == candidate.name
                && (candidate.elite > entry.min_elite
                    || (candidate.elite == entry.min_elite
                        && (candidate.level == 0 || candidate.level >= entry.min_level)))
        }),
        _ => false,
    }
}

fn modeled_candidates(
    kind: FacilityKind,
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    registry: &SupportRegistry,
) -> Result<HashSet<String>> {
    Ok(match kind {
        FacilityKind::ControlCenter => build_control_pool(roster, instances, table)?
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect(),
        FacilityKind::TradePost => build_trade_pool(roster, instances, table)?
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect(),
        FacilityKind::Factory => filter_general_manufacture_search_pool(&build_manufacture_pool(
            roster, instances, table,
        )?)
        .entries
        .into_iter()
        .map(|entry| entry.name)
        .collect(),
        FacilityKind::PowerPlant => build_power_pool(roster, instances, table)?
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect(),
        FacilityKind::Office | FacilityKind::MeetingRoom => roster
            .names()
            .filter(|name| {
                roster.progress(name).is_some_and(|progress| {
                    !candidate_is_skillless(
                        kind,
                        &AssignedOperator::from_progress(name.as_str(), progress),
                        instances,
                        registry,
                    )
                })
            })
            .cloned()
            .collect(),
        FacilityKind::Recycle => roster
            .names()
            .filter(|name| {
                roster.progress(name).is_some_and(|progress| {
                    crate::recycle::recycle_skill_efficiency(
                        instances,
                        table,
                        &AssignedOperator::from_progress(name.as_str(), progress),
                    )
                    .is_ok()
                })
            })
            .cloned()
            .collect(),
        _ => HashSet::new(),
    })
}

pub(crate) fn ordinary_room_compatible(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    room: &RoomBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> bool {
    let members = assignment.operators_in(&room.id);
    if room.kind == FacilityKind::TradePost {
        let operators: Vec<_> = members
            .iter()
            .map(|operator| {
                let buff_ids = instances.resolve_trade_buff_ids(&operator.name, operator.tier());
                crate::trade::input::TradeOperator {
                    name: operator.name.clone(),
                    elite: operator.elite,
                    compiled_atoms: crate::pool::compile_operator_atoms(&buff_ids, table),
                    buff_ids,
                    tags: instances.tags_for(&operator.name, operator.tier()),
                }
            })
            .collect();
        return !crate::layout::blackkey_witch_same_trade_room(assignment, blueprint)
            && !crate::trade::shortcut::trade_station_exclusive_violation(&operators, table)
            && !crate::trade::legality::PLAIN_FORBIDDEN_TRADE_PAIRS
                .iter()
                .any(|(left, right)| {
                    members.iter().any(|operator| operator.name == *left)
                        && members.iter().any(|operator| operator.name == *right)
                });
    }
    if room.kind == FacilityKind::Factory {
        let operators: Vec<_> = members
            .iter()
            .map(|operator| {
                let buff_ids =
                    instances.resolve_manufacture_buff_ids(&operator.name, operator.tier());
                crate::manufacture::ManuOperator {
                    name: operator.name.clone(),
                    elite: operator.elite,
                    compiled_atoms: Some(table.compile_manufacture_atoms(&buff_ids)),
                    buff_ids,
                    tags: instances.tags_for(&operator.name, operator.tier()),
                }
            })
            .collect();
        return crate::search::manufacture_peers_compatible(&operators, table);
    }
    if room.kind == FacilityKind::ControlCenter {
        let operators: Vec<_> = members
            .iter()
            .map(|operator| crate::control::ControlOperator {
                name: operator.name.clone(),
                elite: operator.elite,
                buff_ids: instances.resolve_control_buff_ids(&operator.name, operator.tier()),
                tags: instances.tags_for(&operator.name, operator.tier()),
            })
            .collect();
        let control = members
            .iter()
            .map(|operator| (operator.name.clone(), operator.elite))
            .collect::<Vec<_>>();
        let Ok(model) = crate::mood::default_mood_model() else {
            return false;
        };
        return !model.control_global_recovery_has_conflict(&control)
            && !crate::control::solve_control(
                &crate::control::ControlRoomInput::with_operators(operators),
                table,
            )
            .effect_ledger
            .has_conflicting_effect_sources()
            && assignment
                .control_rotation_admission
                .as_ref()
                .is_none_or(|admission| {
                    let names = members
                        .iter()
                        .map(|operator| &operator.name)
                        .collect::<HashSet<_>>();
                    admission
                        .required_names()
                        .iter()
                        .all(|name| names.contains(name))
                        && admission.requirements().iter().all(|group| {
                            let present = names
                                .iter()
                                .filter(|name| group.candidates.contains(name))
                                .count();
                            usize::from(group.min_count).saturating_sub(present)
                                <= room.operator_capacity().saturating_sub(members.len())
                        })
                        && (members.len() < room.operator_capacity()
                            || admission.validate(assignment).is_ok())
                });
    }
    true
}

pub(crate) fn fill_workplace_vacancies(
    blueprint: &BaseBlueprint,
    assignment: &mut BaseAssignment,
    candidates: &[AssignedOperator],
    instances: &OperatorInstances,
    table: &SkillTable,
    mut admits: impl FnMut(&BaseAssignment, &RoomBlueprint, &AssignedOperator) -> bool,
) -> Result<Vec<(RoomId, AssignedOperator)>> {
    let registry = SupportRegistry::load_default_shared()?;
    let roster = Roster::from_progress_map(
        candidates
            .iter()
            .map(|operator| {
                (
                    operator.name.clone(),
                    OperatorProgress::new(operator.elite, operator.level, operator.rarity),
                )
            })
            .collect(),
    );
    let mut used = assignment.occupied_operator_names();
    let mut placed = Vec::new();
    for room in &blueprint.rooms {
        if !ordinary_workplace(room.kind)
            || assignment.operators_in(&room.id).len() >= room.operator_capacity()
        {
            continue;
        }
        let modeled = modeled_candidates(room.kind, &roster, instances, table, &registry)?;
        // 回收技能优先于普通补位；其他设施保留原“无技能先补空位”的 policy。
        let mut ranked = candidates.iter().collect::<Vec<_>>();
        let skillless_order = if room.kind == FacilityKind::Recycle {
            let mut scored = ranked
                .into_iter()
                .map(|candidate| {
                    crate::recycle::recycle_skill_efficiency(instances, table, candidate)
                        .map(|score| (candidate, score))
                })
                .collect::<Result<Vec<_>>>()?;
            scored
                .sort_by(|(left, a), (right, b)| b.cmp(a).then_with(|| left.name.cmp(&right.name)));
            ranked = scored.into_iter().map(|(candidate, _)| candidate).collect();
            [false, true]
        } else {
            [true, false]
        };
        for skillless in skillless_order {
            for candidate in &ranked {
                if assignment.operators_in(&room.id).len() >= room.operator_capacity() {
                    break;
                }
                if used.contains(&candidate.name)
                    || instances.get(&candidate.name, candidate.tier()).is_none()
                    || candidate_is_skillless(room.kind, candidate, instances, &registry)
                        != skillless
                    || (!skillless && !modeled.contains(&candidate.name))
                    || (room.kind == FacilityKind::ControlCenter
                        && (crate::response_dependency::control_buff_is_automatically_excluded(
                            table,
                            instances
                                .resolve_control_buff_ids(&candidate.name, candidate.tier())
                                .iter()
                                .map(String::as_str),
                        )? || assignment
                            .control_rotation_admission
                            .as_ref()
                            .is_some_and(|admission| !admission.allows(&candidate.name))))
                {
                    continue;
                }
                let mut trial = assignment.clone();
                let mut operators = trial.operators_in(&room.id).to_vec();
                operators.push((*candidate).clone());
                trial.set_room(room.id.clone(), operators);
                if !ordinary_room_compatible(blueprint, &trial, room, instances, table)
                    || !admits(&trial, room, candidate)
                {
                    continue;
                }
                *assignment = trial;
                used.insert(candidate.name.clone());
                placed.push((room.id.clone(), (*candidate).clone()));
            }
        }
    }
    Ok(placed)
}

pub(crate) fn plan_admits_workplace_candidate(
    plan: &super::AssignmentPlan,
    assignment: &BaseAssignment,
    room: &RoomBlueprint,
    candidate: &AssignedOperator,
) -> bool {
    !(room.kind == FacilityKind::ControlCenter && plan.control_exclusions.contains(&candidate.name))
        && !plan.constraints.iter().any(|constraint| match constraint {
            super::SystemConstraint::ForbidSameRoom { a: left, b: right }
            | super::SystemConstraint::ForbidSameStation { a: left, b: right } => {
                assignment
                    .operators_in(&room.id)
                    .iter()
                    .any(|operator| operator.name == *left)
                    && assignment
                        .operators_in(&room.id)
                        .iter()
                        .any(|operator| operator.name == *right)
            }
            _ => false,
        })
}
