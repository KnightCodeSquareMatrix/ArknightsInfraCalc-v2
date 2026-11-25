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
            .resolve_recycle_buff_ids(
                &candidate.name,
                crate::roster::OperatorProgress::new(
                    candidate.elite,
                    candidate.level,
                    candidate.rarity,
                ),
            )
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

fn resolve_assignment_control(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<crate::control::ControlCenterResult> {
    let model = crate::mood::default_mood_model()?;
    let layout = crate::layout::resolve_control_input_layout(
        blueprint,
        assignment,
        instances,
        table,
        model.mood_cap,
        None,
    )?;
    let operators = assignment
        .control_operators()
        .iter()
        .map(|operator| crate::control::ControlOperator {
            name: operator.name.clone(),
            elite: operator.elite,
            buff_ids: instances.resolve_control_buff_ids(&operator.name, operator.tier()),
            tags: instances.tags_for(&operator.name, operator.tier()),
        })
        .collect();
    Ok(crate::control::solve_control(
        &crate::control::ControlRoomInput {
            operators,
            mood: model.mood_cap,
            layout,
        },
        table,
    ))
}

fn has_active_saileach_effect(settled: &crate::control::ControlCenterResult) -> bool {
    settled.effect_ledger.records().iter().any(|record| {
        record.source_operator == "琴柳"
            && record.key == crate::control::ControlEffectKey::OFFICE_CONDITIONAL_HIRE_SPEED
            && record.effective_value > 0.0
    })
}

/// 自动选人和随机补位都不能使中枢琴柳失效；机制仍由中枢解释器结算。
pub(crate) fn auto_saileach_assignment_compatible(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<bool> {
    if !assignment.auto_rotation
        || !assignment
            .control_operators()
            .iter()
            .any(|operator| operator.name == "琴柳")
    {
        return Ok(true);
    }
    Ok(has_active_saileach_effect(&resolve_assignment_control(
        blueprint, assignment, instances, table,
    )?))
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
        let control = members
            .iter()
            .map(|operator| (operator.name.clone(), operator.elite))
            .collect::<Vec<_>>();
        let Ok(model) = crate::mood::default_mood_model() else {
            return false;
        };
        let Ok(settled) = resolve_assignment_control(blueprint, assignment, instances, table)
        else {
            return false;
        };
        if assignment.auto_rotation
            && members.iter().any(|op| op.name == "琴柳")
            && !has_active_saileach_effect(&settled)
        {
            return false;
        }
        return !model.control_global_recovery_has_conflict(&control)
            && !settled.effect_ledger.has_conflicting_effect_sources()
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
        let skillless_order: &[bool] = if room.kind == FacilityKind::Recycle {
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
            if assignment.auto_rotation {
                &[false]
            } else {
                &[false, true]
            }
        } else if assignment.auto_rotation {
            &[false]
        } else {
            &[true, false]
        };
        for &skillless in skillless_order {
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
    if assignment.auto_rotation {
        let random_order = std::collections::hash_map::RandomState::new();
        let mut remaining: Vec<_> = candidates
            .iter()
            .filter(|operator| !used.contains(&operator.name))
            .collect();
        remaining.sort_by_key(|operator| {
            std::hash::BuildHasher::hash_one(&random_order, &operator.name)
        });
        for room in &blueprint.rooms {
            if !ordinary_workplace(room.kind) {
                continue;
            }
            let mut operators = assignment.operators_in(&room.id).to_vec();
            while operators.len() < room.operator_capacity() {
                let mut selected = None;
                for index in (0..remaining.len()).rev() {
                    let mut trial = assignment.clone();
                    let mut trial_operators = operators.clone();
                    trial_operators.push(remaining[index].clone());
                    trial.set_room(room.id.clone(), trial_operators);
                    if auto_saileach_assignment_compatible(blueprint, &trial, instances, table)? {
                        selected = Some(index);
                        break;
                    }
                }
                let Some(index) = selected else {
                    break;
                };
                // 被拒绝者保留给后续设施；本房没有兼容候选时保留缺位。
                let candidate = remaining.remove(index);
                used.insert(candidate.name.clone());
                operators.push(candidate.clone());
                assignment.fallback_operators.push(candidate.name.clone());
                placed.push((room.id.clone(), candidate.clone()));
            }
            if operators.len() != assignment.operators_in(&room.id).len() {
                assignment.set_room(room.id.clone(), operators);
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
