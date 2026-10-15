use std::collections::HashSet;

use crate::control::{solve_control, ControlRoomInput};
use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::context::LayoutContext;
use crate::mood::default_mood_model;
use crate::pool::{try_filter_standalone, ControlPool};
use crate::search::{
    search_control_combos_with_eligibility, ControlFillPolicy, ControlSearchOptions,
    MATATABI_CONSUMER_NAME,
};
use crate::skill_table::SkillTable;

use super::AssignBaseOptions;

fn assignment_has_matatabi_consumer(assignment: &BaseAssignment) -> bool {
    assignment.iter_rooms().any(|room| {
        room.operators
            .iter()
            .any(|op| op.name == MATATABI_CONSUMER_NAME)
    })
}

pub(crate) fn assign_control_with_saileach(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    candidate_requirements: &[crate::layout::ControlCandidateRequirement],
    excluded_control: &HashSet<String>,
    saileach_best_office_pct: Option<f64>,
    saileach_required: bool,
    eligibility: Option<&(dyn Fn(&[String]) -> bool + Sync)>,
    used: &mut HashSet<String>,
) -> Result<()> {
    assign_control_inner(
        assignment,
        pool,
        table,
        layout,
        options,
        candidate_requirements,
        None,
        Some(excluded_control),
        saileach_best_office_pct,
        saileach_required,
        eligibility,
        used,
    )?
    .then_some(())
    .ok_or_else(|| Error::msg("control: no legal fill combo"))
}

pub(crate) fn assign_control_matching_dynamic_set_with_saileach(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    candidate_requirements: &[crate::layout::ControlCandidateRequirement],
    required_all: &HashSet<String>,
    dynamic_excluded: &HashSet<String>,
    excluded_control: &HashSet<String>,
    saileach_best_office_pct: Option<f64>,
    saileach_required: bool,
    eligibility: Option<&(dyn Fn(&[String]) -> bool + Sync)>,
    used: &mut HashSet<String>,
) -> Result<bool> {
    let excluded: HashSet<_> = dynamic_excluded.union(excluded_control).cloned().collect();
    assign_control_inner(
        assignment,
        pool,
        table,
        layout,
        options,
        candidate_requirements,
        Some(required_all),
        Some(&excluded),
        saileach_best_office_pct,
        saileach_required,
        eligibility,
        used,
    )
}

#[allow(clippy::too_many_arguments)]
fn assign_control_inner(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    candidate_requirements: &[crate::layout::ControlCandidateRequirement],
    required_all: Option<&HashSet<String>>,
    excluded: Option<&HashSet<String>>,
    saileach_best_office_pct: Option<f64>,
    saileach_required: bool,
    eligibility: Option<&(dyn Fn(&[String]) -> bool + Sync)>,
    used: &mut HashSet<String>,
) -> Result<bool> {
    const MAX_CONTROL: usize = 5;
    let pinned: HashSet<String> = assignment
        .control_operators()
        .into_iter()
        .map(|o| o.name)
        .collect();
    if pinned.len() >= MAX_CONTROL {
        let operators = assignment
            .control_operators()
            .into_iter()
            .map(|operator| {
                pool.entry(&operator.name)
                    .map(|entry| entry.to_control_operator())
                    .ok_or_else(|| {
                        Error::msg(format!(
                            "control: pinned operator {} is absent from pool",
                            operator.name
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let mood_model = default_mood_model()?;
        let control_operators = operators
            .iter()
            .map(|operator| (operator.name.clone(), operator.elite))
            .collect::<Vec<_>>();
        if mood_model.control_global_recovery_has_conflict(&control_operators) {
            return Err(Error::msg(
                "control: pinned operators contain conflicting global recovery owners",
            ));
        }
        if solve_control(&ControlRoomInput::with_operators(operators), table)
            .effect_ledger
            .has_conflicting_effect_sources()
        {
            return Err(Error::msg(
                "control: pinned operators contain conflicting EffectKey sources",
            ));
        }
        let names: Vec<_> = pinned.iter().cloned().collect();
        return Ok(pinned.len() == MAX_CONTROL
            && required_all.is_none_or(|required| required.is_subset(&pinned))
            && candidate_requirements.iter().all(|group| {
                pinned
                    .iter()
                    .filter(|name| group.candidates.contains(name))
                    .count()
                    >= usize::from(group.min_count)
            })
            && excluded.is_none_or(|excluded| pinned.is_disjoint(excluded))
            && eligibility.is_none_or(|accept| accept(&names)));
    }
    if pool.entries.is_empty() {
        return Ok(required_all.is_none_or(HashSet::is_empty)
            && candidate_requirements
                .iter()
                .all(|group| group.min_count == 0));
    }
    let requirements: Vec<(HashSet<String>, u8)> = candidate_requirements
        .iter()
        .map(|requirement| {
            (
                requirement.candidates.iter().cloned().collect(),
                requirement.min_count,
            )
        })
        .collect();
    let mut must_include = pinned.clone();
    if let Some(required_all) = required_all {
        must_include.extend(required_all.iter().cloned());
    }
    let control_opts = ControlSearchOptions {
        max_operators: 5,
        top_k: options.top_k,
        mood: options.mood,
        layout: layout.clone(),
        matatabi_consumer_active: assignment_has_matatabi_consumer(assignment),
        must_include: must_include.clone(),
        candidate_requirements: requirements,
        fill_policy: ControlFillPolicy::LayeredFill,
        saileach_best_office_pct,
        saileach_required,
    };

    let base_pool =
        if options.skip_standalone_control || !pinned.is_empty() || required_all.is_some() {
            pool.clone()
        } else {
            let preferred = try_filter_standalone(pool, FacilityKind::ControlCenter, 1);
            if preferred.entries.len() >= MAX_CONTROL {
                preferred
            } else {
                pool.clone()
            }
        };
    let empty_excluded = HashSet::new();
    let filtered_pool = filter_control_pool_for_fill(
        &base_pool,
        table,
        used,
        &pinned,
        excluded.unwrap_or(&empty_excluded),
    )?;

    let combos =
        search_control_combos_with_eligibility(&filtered_pool, table, &control_opts, eligibility)?;
    let Some(hit) = pick_control_extending_pins(combos, &must_include, used, &|h| &h.names) else {
        if required_all.is_some() {
            return Ok(false);
        }
        return Err(Error::msg(
            format!(
                "control: NO_CANDIDATE_UNDER_POLICY LayeredFill; no combo extending pins after pool filter; pinned={:?} must_include={:?} pool={} filtered={} requirements={:?}",
                pinned,
                must_include,
                pool.entries.len(),
                filtered_pool.entries.len(),
                candidate_requirements.iter().map(|group| (&group.candidates, group.min_count)).collect::<Vec<_>>(),
            ),
        ));
    };
    let skillless_fallback_names: Vec<String> = hit
        .names
        .iter()
        .filter(|name| {
            !pinned.contains(*name)
                && pool
                    .entry(name.as_str())
                    .is_some_and(|entry| entry.buff_ids.is_empty())
        })
        .cloned()
        .collect();
    assignment
        .note_facts
        .control_skillless_fallback_names
        .clear();
    let control_id = RoomId::from("control");
    commit_control_combo(
        assignment,
        &control_id,
        &hit.names,
        |name| {
            pool.entry(name)
                .map(|e| AssignedOperator::from_progress(name, e.progress))
                .unwrap_or_else(|| AssignedOperator::new(name, 0))
        },
        used,
        &pinned,
    )?;
    assignment.note_facts.control_skillless_fallback_names = skillless_fallback_names;
    Ok(true)
}

fn filter_control_pool_for_fill(
    pool: &ControlPool,
    table: &SkillTable,
    used: &HashSet<String>,
    pinned: &HashSet<String>,
    excluded: &HashSet<String>,
) -> Result<ControlPool> {
    let automatically_excluded: HashSet<_> = pool
        .entries
        .iter()
        .map(|entry| {
            crate::response_dependency::control_buff_is_automatically_excluded(
                table,
                entry.buff_ids.iter().map(String::as_str),
            )
            .map(|excluded| (entry.name.as_str(), excluded))
        })
        .collect::<Result<_>>()?;
    Ok(ControlPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| {
                (pinned.contains(&e.name)
                    || (!excluded.contains(&e.name)
                        && !automatically_excluded
                            .iter()
                            .any(|(name, excluded)| *excluded && *name == e.name)))
                    && (!used.contains(&e.name) || pinned.contains(&e.name))
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    })
}

fn pick_control_extending_pins<T: Clone>(
    hits: impl IntoIterator<Item = T>,
    pinned: &HashSet<String>,
    used: &HashSet<String>,
    names_of: &impl Fn(&T) -> &[String],
) -> Option<T> {
    hits.into_iter().find(|h| {
        let names = names_of(h);
        pinned.iter().all(|p| names.contains(p))
            && names
                .iter()
                .all(|n| pinned.contains(n) || !used.contains(n))
    })
}

fn commit_control_combo(
    assignment: &mut BaseAssignment,
    room_id: &RoomId,
    names: &[String],
    operator_of: impl Fn(&str) -> AssignedOperator,
    used: &mut HashSet<String>,
    pinned: &HashSet<String>,
) -> Result<()> {
    let ops = names
        .iter()
        .map(|name| {
            if !pinned.contains(name) && !used.insert(name.clone()) {
                return Err(Error::msg(format!("control duplicate {name}")));
            }
            Ok(operator_of(name))
        })
        .collect::<Result<Vec<_>>>()?;
    assignment.set_room(room_id.clone(), ops);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::pool::build_control_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    #[test]
    fn final_control_pool_keeps_legal_operator_without_control_buff() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map([("阿米娅".to_string(), 2)].into_iter().collect());
        let mut pool = build_control_pool(&roster, &instances, &table).unwrap();
        pool.entries[0].buff_ids.clear();

        let filtered = filter_control_pool_for_fill(
            &pool,
            &table,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert_eq!(filtered.entries.len(), 1);
        assert!(filtered.entries[0].buff_ids.is_empty());

        let pinned = HashSet::from(["阿米娅".to_string()]);
        let filtered =
            filter_control_pool_for_fill(&pool, &table, &HashSet::new(), &pinned, &pinned).unwrap();
        assert_eq!(filtered.entries.len(), 1);
    }
}
