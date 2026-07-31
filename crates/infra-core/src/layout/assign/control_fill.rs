use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::context::LayoutContext;
use crate::pool::{try_filter_standalone, ControlPool};
use crate::search::{
    search_control_combos, ControlFillPolicy, ControlSearchOptions, MATATABI_CONSUMER_NAME,
};
use crate::skill_table::SkillTable;

use super::AssignBaseOptions;

fn assignment_has_matatabi_consumer(assignment: &BaseAssignment) -> bool {
    assignment.rooms.iter().any(|room| {
        room.operators
            .iter()
            .any(|op| op.name == MATATABI_CONSUMER_NAME)
    })
}

pub(crate) fn assign_control(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    candidate_requirements: &[crate::layout::ControlCandidateRequirement],
    excluded_control: &HashSet<String>,
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
        used,
    )?
    .then_some(())
    .ok_or_else(|| Error::msg("control: no legal fill combo"))
}

pub(crate) fn assign_control_matching_dynamic_set(
    assignment: &mut BaseAssignment,
    pool: &ControlPool,
    table: &SkillTable,
    layout: &LayoutContext,
    options: &AssignBaseOptions,
    candidate_requirements: &[crate::layout::ControlCandidateRequirement],
    required_all: &HashSet<String>,
    dynamic_excluded: &HashSet<String>,
    excluded_control: &HashSet<String>,
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
    used: &mut HashSet<String>,
) -> Result<bool> {
    const MAX_CONTROL: usize = 5;
    if pool.entries.is_empty() {
        return Ok(required_all.is_none_or(HashSet::is_empty));
    }
    let pinned: HashSet<String> = assignment
        .control_operators()
        .into_iter()
        .map(|o| o.name)
        .collect();
    if pinned.len() >= MAX_CONTROL {
        return Ok(required_all.is_none_or(|required| required.is_subset(&pinned)));
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
        used,
        &pinned,
        excluded.unwrap_or(&empty_excluded),
    );

    let combos = search_control_combos(&filtered_pool, table, &control_opts)?;
    let Some(hit) = pick_control_extending_pins(combos, &must_include, used, &|h| &h.names) else {
        if required_all.is_some() {
            return Ok(false);
        }
        return Err(Error::msg(
            "control: no combo extending pins after pool filter",
        ));
    };
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
    Ok(true)
}

fn filter_control_pool_for_fill(
    pool: &ControlPool,
    used: &HashSet<String>,
    pinned: &HashSet<String>,
    excluded: &HashSet<String>,
) -> ControlPool {
    ControlPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| {
                (pinned.contains(&e.name) || !excluded.contains(&e.name))
                    && (!used.contains(&e.name) || pinned.contains(&e.name))
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
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
    fn control_fill_skips_used_skillless_prefix_and_still_fills_five() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let entries: Vec<_> = (0..10)
            .map(|index| crate::pool::ControlPoolEntry {
                name: format!("普通候选{index:02}"),
                elite: 0,
                progress: crate::roster::OperatorProgress::default(),
                buff_ids: Vec::new(),
                tags: Vec::new(),
                tier: crate::layout::tier::OperatorTier::Standalone,
            })
            .collect();
        let pool = ControlPool {
            entries,
            skipped: Vec::new(),
        };
        let mut used: HashSet<String> = (0..5).map(|index| format!("普通候选{index:02}")).collect();
        let mut assignment = BaseAssignment::default();

        assign_control(
            &mut assignment,
            &pool,
            &table,
            &LayoutContext::default(),
            &AssignBaseOptions::default(),
            &[],
            &HashSet::new(),
            &mut used,
        )
        .unwrap();

        assert_eq!(assignment.control_operators().len(), 5);
        assert!(assignment.control_operators().iter().all(|op| !matches!(
            op.name.as_str(),
            "普通候选00" | "普通候选01" | "普通候选02" | "普通候选03" | "普通候选04"
        )));
    }

    #[test]
    fn final_control_pool_keeps_legal_operator_without_control_buff() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let roster = Roster::from_elite_map([("阿米娅".to_string(), 2)].into_iter().collect());
        let mut pool = build_control_pool(&roster, &instances, &table).unwrap();
        pool.entries[0].buff_ids.clear();

        let filtered =
            filter_control_pool_for_fill(&pool, &HashSet::new(), &HashSet::new(), &HashSet::new());

        assert_eq!(filtered.entries.len(), 1);
        assert!(filtered.entries[0].buff_ids.is_empty());

        let pinned = HashSet::from(["阿米娅".to_string()]);
        let filtered = filter_control_pool_for_fill(&pool, &HashSet::new(), &pinned, &pinned);
        assert_eq!(filtered.entries.len(), 1);
    }

    #[test]
    fn excluded_control_operator_is_not_reintroduced_by_fill() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let entries: Vec<_> = [
            "排除候选",
            "合法候选01",
            "合法候选02",
            "合法候选03",
            "合法候选04",
            "合法候选05",
        ]
        .into_iter()
        .map(|name| crate::pool::ControlPoolEntry {
            name: name.to_string(),
            elite: 0,
            progress: crate::roster::OperatorProgress::default(),
            buff_ids: Vec::new(),
            tags: Vec::new(),
            tier: crate::layout::tier::OperatorTier::Standalone,
        })
        .collect();
        let pool = ControlPool {
            entries,
            skipped: Vec::new(),
        };
        let mut assignment = BaseAssignment::default();
        let mut used = HashSet::new();
        let excluded = HashSet::from(["排除候选".to_string()]);

        assign_control(
            &mut assignment,
            &pool,
            &table,
            &LayoutContext::default(),
            &AssignBaseOptions::default(),
            &[],
            &excluded,
            &mut used,
        )
        .unwrap();

        assert_eq!(assignment.control_operators().len(), 5);
        assert!(assignment
            .control_operators()
            .iter()
            .all(|operator| operator.name != "排除候选"));
    }
}
