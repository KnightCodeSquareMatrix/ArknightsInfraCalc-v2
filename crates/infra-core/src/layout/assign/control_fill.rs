use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::context::LayoutContext;
use crate::pool::{try_filter_standalone, ControlPool};
use crate::search::{
    control_entry_plugin_fill, search_control_combos, ControlFillPolicy, ControlSearchOptions,
    MATATABI_CONSUMER_NAME,
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
    used: &mut HashSet<String>,
) -> Result<()> {
    const MAX_CONTROL: usize = 5;
    if pool.entries.is_empty() {
        return Ok(());
    }
    let pinned: HashSet<String> = assignment
        .control_operators()
        .into_iter()
        .map(|o| o.name)
        .collect();
    if pinned.len() >= MAX_CONTROL {
        return Ok(());
    }

    let control_opts = ControlSearchOptions {
        max_operators: 5,
        top_k: options.top_k,
        mood: options.mood,
        layout: layout.clone(),
        matatabi_consumer_active: assignment_has_matatabi_consumer(assignment),
        must_include: pinned.clone(),
        fill_policy: ControlFillPolicy::HrAndMood,
    };

    let base_pool = if options.skip_standalone_control || !pinned.is_empty() {
        pool.clone()
    } else {
        try_filter_standalone(pool, FacilityKind::ControlCenter, 1)
    };
    let filtered_pool =
        filter_control_pool_for_fill(&base_pool, used, &pinned, control_opts.fill_policy);

    let hit = if pinned.is_empty() {
        let combos = search_control_combos(&filtered_pool, table, &control_opts)?;
        pick_cached_or_rescan_control(
            &combos,
            &pinned,
            used,
            || search_control_combos(&filtered_pool, table, &control_opts),
            |h| &h.names,
            "control: no disjoint combo after pool filter",
        )?
    } else {
        let combos = search_control_combos(&filtered_pool, table, &control_opts)?;
        pick_control_extending_pins(combos.iter().cloned(), &pinned, used, &|h| &h.names)
            .ok_or_else(|| Error::msg("control: no combo extending pinned after pool filter"))?
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
    )
}

fn filter_control_pool_for_fill(
    pool: &ControlPool,
    used: &HashSet<String>,
    pinned: &HashSet<String>,
    fill_policy: ControlFillPolicy,
) -> ControlPool {
    ControlPool {
        entries: pool
            .entries
            .iter()
            .filter(|e| {
                (!used.contains(&e.name) || pinned.contains(&e.name))
                    && (fill_policy != ControlFillPolicy::HrAndMood
                        || pinned.contains(&e.name)
                        || control_entry_plugin_fill(e))
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn pick_cached_or_rescan_control<T, F>(
    cached: &[T],
    pinned: &HashSet<String>,
    used: &HashSet<String>,
    rescan: F,
    names_of: impl Fn(&T) -> &[String],
    err: &str,
) -> Result<T>
where
    T: Clone,
    F: FnOnce() -> Result<Vec<T>>,
{
    if let Some(hit) = pick_control_extending_pins(cached.iter().cloned(), pinned, used, &names_of)
    {
        return Ok(hit);
    }
    let fresh = rescan()?;
    pick_control_extending_pins(fresh, pinned, used, &names_of).ok_or_else(|| Error::msg(err))
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
