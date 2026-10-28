use std::collections::HashSet;

use crate::control::{solve_control, ControlRoomInput};
use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::context::LayoutContext;
use crate::mood::{default_mood_model, fits_work_budget, operator_net_drain, DrainInputs};
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
    let observe_control =
        std::env::var_os("ARKNIGHTS_INFRA_OBSERVE_CONTROL").is_some_and(|value| value == "1");
    macro_rules! observe {
        ($($arg:tt)*) => {
            if observe_control {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "[INFRA-CONTROL-OBSERVE] {}",
                    format_args!($($arg)*)
                );
            }
        };
    }
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
        let accepted = pinned.len() == MAX_CONTROL
            && required_all.is_none_or(|required| required.is_subset(&pinned))
            && candidate_requirements.iter().all(|group| {
                pinned
                    .iter()
                    .filter(|name| group.candidates.contains(name))
                    .count()
                    >= usize::from(group.min_count)
            })
            && excluded.is_none_or(|excluded| pinned.is_disjoint(excluded))
            && eligibility.is_none_or(|accept| accept(&names));
        if observe_control {
            let pinned_log: std::collections::BTreeSet<_> = pinned.iter().collect();
            let required_log: Option<std::collections::BTreeSet<_>> =
                required_all.map(|names| names.iter().collect());
            let excluded_log: Option<std::collections::BTreeSet<_>> =
                excluded.map(|names| names.iter().collect());
            observe!(
                "control_fill fixed_prefix={pinned_log:?} required_all={required_log:?} requirements={candidate_requirements:?} excluded={excluded_log:?} accepted={accepted}"
            );
        }
        return Ok(accepted);
    }
    if pool.entries.is_empty() {
        let accepted = required_all.is_none_or(HashSet::is_empty)
            && candidate_requirements
                .iter()
                .all(|group| group.min_count == 0);
        if observe_control {
            let required_log: Option<std::collections::BTreeSet<_>> =
                required_all.map(|names| names.iter().collect());
            observe!(
                "control_fill empty_pool required_all={required_log:?} requirements={candidate_requirements:?} accepted={accepted}"
            );
        }
        return Ok(accepted);
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
    let protected_mood_candidates: HashSet<String> = candidate_requirements
        .iter()
        .flat_map(|requirement| requirement.candidates.iter().cloned())
        .chain(pinned.iter().cloned())
        .chain(
            required_all
                .into_iter()
                .flat_map(|required| required.iter().cloned()),
        )
        .collect();
    let filtered_pool = filter_control_pool_for_mood(
        &filtered_pool,
        options.control_work_hours,
        options.mood,
        &protected_mood_candidates,
    )?;

    if observe_control {
        let mut pins_log: Vec<_> = pinned.iter().cloned().collect();
        pins_log.sort();
        let mut must_include_log: Vec<_> = must_include.iter().cloned().collect();
        must_include_log.sort();
        let mut excluded_log: Vec<_> = excluded
            .unwrap_or(&empty_excluded)
            .iter()
            .cloned()
            .collect();
        excluded_log.sort();
        let required_log: Option<std::collections::BTreeSet<_>> =
            required_all.map(|names| names.iter().collect());
        let identities: Vec<_> = filtered_pool
            .entries
            .iter()
            .map(|entry| &entry.name)
            .collect();
        observe!("control_fill pins={pins_log:?} must_include={must_include_log:?} required_all={required_log:?} excluded={excluded_log:?} requirements={:?} filtered_candidates={identities:?}",
            candidate_requirements
                .iter()
                .map(|group| (&group.candidates, group.min_count))
                .collect::<Vec<_>>());
    }

    let combos =
        search_control_combos_with_eligibility(&filtered_pool, table, &control_opts, eligibility)?;
    let Some(hit) = pick_control_extending_pins(combos, &must_include, used, &|h| &h.names) else {
        observe!("control_fill no_hit_after_policy");
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
    observe!("control_fill trial_assignment={:?}", hit.names);
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

/// Apply the fixed, per-operator control admission before combo search. The
/// control-wide reduction is the only shared context here; combo-specific
/// recovery remains owned by the final assignment check.
fn filter_control_pool_for_mood(
    pool: &ControlPool,
    work_hours: Option<f64>,
    initial_mood: f64,
    protected: &HashSet<String>,
) -> Result<ControlPool> {
    let Some(work_hours) = work_hours else {
        return Ok(pool.clone());
    };
    let model = default_mood_model()?;
    let entries = pool
        .entries
        .iter()
        .filter(|entry| {
            if protected.contains(&entry.name) {
                return true;
            }
            let room_ops = vec![(entry.name.clone(), entry.progress.elite)];
            let inputs = DrainInputs {
                name: &entry.name,
                elite: entry.progress.elite,
                facility: FacilityKind::ControlCenter,
                room_ops: &room_ops,
                full_control: true,
                control_ops: &[],
                recipe: None,
            };
            fits_work_budget(
                &model,
                initial_mood.min(model.mood_cap),
                operator_net_drain(&model, &inputs),
                work_hours,
            )
        })
        .cloned()
        .collect();
    Ok(ControlPool {
        entries,
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
