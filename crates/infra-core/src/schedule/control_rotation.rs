//! Frozen ABC control admission. Candidate ownership is decided before any
//! shift is solved; physical occupancy is read from that shift's assignment.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::layout::{
    AssignedOperator, AssignmentPlan, BaseAssignment, BaseBlueprint, ControlCandidateRequirement,
    FacilityKind, RoomId,
};
use crate::operbox::OperBox;
use crate::pool::{ControlPool, ControlPoolEntry};
use crate::skill_table::SkillTable;

use super::rotation_types::RuntimeShiftBind;
use super::rotation_types::{TeamLabel, TeamRotationReport, TimedRotationProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkScope {
    Team(TeamLabel),
    Shared,
    ShortBranch,
}

impl WorkScope {
    fn active(self, active: &[TeamLabel], short_branch: bool) -> bool {
        match self {
            Self::Team(team) => active.contains(&team),
            Self::Shared => true,
            Self::ShortBranch => short_branch,
        }
    }
}

#[derive(Debug, Clone)]
enum CandidateOrigin {
    Ordinary,
    PlanRequired,
    Group {
        claim_index: usize,
        slot_index: usize,
    },
    ShortBranch,
}

#[derive(Debug, Clone)]
struct Candidate {
    entry: ControlPoolEntry,
    scope: WorkScope,
    origin: CandidateOrigin,
}

#[derive(Debug, Clone)]
struct ResolvedRequirement {
    claim_index: usize,
    slot_index: usize,
    scope: WorkScope,
    requirement: ControlCandidateRequirement,
}

#[derive(Debug, Clone)]
pub(crate) struct ControlRotationClosure {
    candidates: HashMap<String, Candidate>,
    required: HashSet<String>,
    requirements: Vec<ResolvedRequirement>,
    capacity: usize,
    room_id: RoomId,
}

impl ControlRotationClosure {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        blueprint: &BaseBlueprint,
        pool: &ControlPool,
        plan: &AssignmentPlan,
        peak: &BaseAssignment,
        shared: &BaseAssignment,
        team_ctrl: &HashMap<TeamLabel, Vec<String>>,
        production_teams: &HashMap<String, TeamLabel>,
        required_names: &HashSet<String>,
        branch_producer: Option<&str>,
        binds: &[RuntimeShiftBind],
        table: &SkillTable,
    ) -> Result<Self> {
        let room = blueprint
            .rooms_of(FacilityKind::ControlCenter)
            .into_iter()
            .next()
            .ok_or_else(|| Error::msg("control_rotation_admission: missing control room"))?;
        let shared_control: HashSet<_> = shared
            .control_operators()
            .into_iter()
            .map(|op| op.name)
            .collect();
        let mut closure = Self {
            candidates: HashMap::new(),
            required: required_names.union(&shared_control).cloned().collect(),
            requirements: Vec::new(),
            capacity: room.operator_capacity(),
            room_id: room.id.clone(),
        };
        for team in TeamLabel::ALL {
            for name in team_ctrl.get(&team).into_iter().flatten() {
                let entry = pool.entry(name).ok_or_else(|| {
                    Error::msg(format!(
                        "control_rotation_admission: missing runtime entry for {name}"
                    ))
                })?;
                let is_required = closure.required.contains(name);
                if !is_required
                    && (plan.control_exclusions.contains(name)
                        || crate::response_dependency::control_buff_is_automatically_excluded(
                            table,
                            entry.buff_ids.iter().map(String::as_str),
                        )?)
                {
                    continue;
                }
                if closure
                    .candidates
                    .insert(
                        name.clone(),
                        Candidate {
                            entry: entry.clone(),
                            scope: WorkScope::Team(team),
                            origin: if is_required {
                                CandidateOrigin::PlanRequired
                            } else {
                                CandidateOrigin::Ordinary
                            },
                        },
                    )
                    .is_some()
                {
                    return Err(Error::msg(format!(
                        "control_rotation_membership: duplicate team for {name}"
                    )));
                }
            }
        }
        for name in &shared_control {
            let entry = pool.entry(name).ok_or_else(|| {
                Error::msg(format!(
                    "control_rotation_admission: missing shared entry for {name}"
                ))
            })?;
            closure.candidates.insert(
                name.clone(),
                Candidate {
                    entry: entry.clone(),
                    scope: WorkScope::Shared,
                    origin: CandidateOrigin::PlanRequired,
                },
            );
        }
        for name in &closure.required {
            if !closure.candidates.contains_key(name) {
                return Err(Error::msg(format!(
                    "control_plan_requirement: required {name} has no frozen scope"
                )));
            }
        }
        let bound_names: HashSet<_> = binds
            .iter()
            .flat_map(|bind| bind.operators.iter())
            .collect();
        let actual_peak = peak.operator_names();
        for ((claim_index, slot_index), requirement) in plan
            .control_requirement_sources()?
            .into_iter()
            .zip(&plan.control_candidate_requirements)
        {
            let claim = &plan.registry_claims[claim_index];
            let peak_members: Vec<_> = peak
                .control_operators()
                .into_iter()
                .filter(|op| requirement.candidates.contains(&op.name))
                .collect();
            if peak_members.len() < usize::from(requirement.min_count) {
                return Err(Error::msg(format!(
                    "control_plan_requirement: peak violates {}/slot {}",
                    claim.system_id, slot_index
                )));
            }
            // Actual non-control roles own the work scope when available. Pure
            // control groups inherit their preassigned actual peak cohort.
            let mut owners: HashSet<_> = claim
                .slots
                .iter()
                .filter(|slot| slot.facility != FacilityKind::ControlCenter)
                .flat_map(|slot| &slot.operators)
                .filter(|op| actual_peak.contains(&op.name))
                .filter_map(|op| production_teams.get(&op.name).copied())
                .collect();
            if owners.is_empty() {
                owners.extend(
                    peak_members
                        .iter()
                        .filter_map(|op| closure.candidates.get(&op.name))
                        .filter_map(|candidate| match candidate.scope {
                            WorkScope::Team(team) => Some(team),
                            _ => None,
                        }),
                );
            }
            let scope = if peak_members
                .iter()
                .all(|op| shared_control.contains(&op.name))
            {
                WorkScope::Shared
            } else if owners.len() == 1 {
                WorkScope::Team(*owners.iter().next().expect("one owner"))
            } else {
                return Err(Error::msg(format!(
                    "control_requirement_scope_unresolved: {}/slot {} has {} cohorts",
                    claim.system_id,
                    slot_index,
                    owners.len()
                )));
            };
            for name in &requirement.candidates {
                let Some(candidate) = closure.candidates.get_mut(name) else {
                    if !production_teams.contains_key(name)
                        && !shared.operator_names().contains(name)
                        && !plan.control_exclusions.contains(name)
                        && pool.entry(name).is_some_and(|entry| {
                            crate::response_dependency::control_buff_is_automatically_excluded(
                                table,
                                entry.buff_ids.iter().map(String::as_str),
                            )
                            .ok()
                                == Some(false)
                        })
                    {
                        return Err(Error::msg(format!("control_requirement_scope_unresolved: group member {name} has no admitted or unavailable role")));
                    }
                    continue;
                };
                if candidate.scope != scope
                    && (closure.required.contains(name)
                        || bound_names.contains(name)
                        || matches!(candidate.origin, CandidateOrigin::Group { .. }))
                {
                    return Err(Error::msg(format!(
                        "control_requirement_scope_unresolved: conflicting ownership for {name}"
                    )));
                }
                candidate.scope = scope;
                candidate.origin = CandidateOrigin::Group {
                    claim_index,
                    slot_index,
                };
            }
            closure.requirements.push(ResolvedRequirement {
                claim_index,
                slot_index,
                scope,
                requirement: requirement.clone(),
            });
        }
        if let Some(name) = branch_producer {
            if closure.required.contains(name) || production_teams.contains_key(name) {
                return Err(Error::msg(format!(
                    "control_rotation_admission: short-branch producer {name} has a fixed role"
                )));
            }
            let entry = pool.entry(name).ok_or_else(|| {
                Error::msg(format!(
                    "control_rotation_admission: missing branch entry for {name}"
                ))
            })?;
            closure.candidates.insert(
                name.to_string(),
                Candidate {
                    entry: entry.clone(),
                    scope: WorkScope::ShortBranch,
                    origin: CandidateOrigin::ShortBranch,
                },
            );
        }
        Ok(closure)
    }

    pub(crate) fn team_of(&self, name: &str) -> Option<TeamLabel> {
        match self.candidates.get(name)?.scope {
            WorkScope::Team(team) => Some(team),
            // A short-branch producer is an admission-only pin. It has no
            // permanent cohort and must not be materialized as Gamma.
            WorkScope::ShortBranch => None,
            WorkScope::Shared => None,
        }
    }

    pub(crate) fn admission(
        self: &Arc<Self>,
        index: usize,
        active: &[TeamLabel],
        extra: &[&str],
    ) -> Result<ControlShiftAdmission> {
        if !extra.is_empty()
            && (index != 1
                || extra.iter().any(|name| {
                    self.candidates
                        .get(*name)
                        .is_none_or(|candidate| candidate.scope != WorkScope::ShortBranch)
                }))
        {
            return Err(Error::msg(
                "control_rotation_admission: unregistered short-branch pin",
            ));
        }
        Ok(ControlShiftAdmission {
            closure: Arc::clone(self),
            index,
            active: active.to_vec(),
            extra: extra.iter().map(|name| (*name).to_string()).collect(),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ControlShiftAdmission {
    closure: Arc<ControlRotationClosure>,
    index: usize,
    active: Vec<TeamLabel>,
    extra: HashSet<String>,
}

impl ControlShiftAdmission {
    pub(crate) fn allows(&self, name: &str) -> bool {
        self.closure
            .candidates
            .get(name)
            .is_some_and(|candidate| match candidate.scope {
                WorkScope::ShortBranch => self.extra.contains(name),
                scope => scope.active(&self.active, false),
            })
    }

    pub(crate) fn requirements(&self) -> Vec<ControlCandidateRequirement> {
        self.closure
            .requirements
            .iter()
            .filter(|group| group.scope.active(&self.active, false))
            .map(|group| group.requirement.clone())
            .collect()
    }

    pub(crate) fn required_names(&self) -> HashSet<String> {
        self.closure
            .required
            .iter()
            .filter(|name| self.allows(name))
            .chain(&self.extra)
            .cloned()
            .collect()
    }

    pub(crate) fn prepare(
        &self,
        assignment: &mut BaseAssignment,
        rejected: &HashSet<String>,
    ) -> Result<ControlPool> {
        let required = self.required_names();
        if required.len() > self.closure.capacity {
            return Err(Error::ControlRotationRequiredAnchorsExceedCapacity {
                required: required.len(),
                capacity: self.closure.capacity,
            });
        }
        let occupied: HashSet<_> = assignment
            .iter_rooms()
            .filter(|room| room.room_id != self.closure.room_id)
            .flat_map(|room| &room.operators)
            .map(|op| op.name.as_str())
            .chain(assignment.training_assist.iter().map(|op| op.name.as_str()))
            .collect();
        if let Some(name) = required
            .iter()
            .find(|name| occupied.contains(name.as_str()) || rejected.contains(*name))
        {
            return Err(Error::msg(format!(
                "control_rotation_admission: required {name} is occupied or rejected"
            )));
        }
        let mut entries: Vec<_> = self
            .closure
            .candidates
            .values()
            .filter(|candidate| {
                self.allows(&candidate.entry.name)
                    && !occupied.contains(candidate.entry.name.as_str())
                    && !rejected.contains(&candidate.entry.name)
            })
            .map(|candidate| candidate.entry.clone())
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut required: Vec<_> = required.into_iter().collect();
        required.sort();
        let operators = required
            .iter()
            .map(|name| {
                let candidate = self
                    .closure
                    .candidates
                    .get(name)
                    .expect("required candidate registered");
                AssignedOperator::from_progress(name, candidate.entry.progress)
            })
            .collect();
        assignment.set_room(self.closure.room_id.clone(), operators);
        assignment.control_rotation_admission = Some(self.clone());
        Ok(ControlPool {
            entries,
            skipped: Vec::new(),
        })
    }

    pub(crate) fn validate(&self, assignment: &BaseAssignment) -> Result<()> {
        let operators = assignment.operators_in(&self.closure.room_id);
        let names: HashSet<_> = operators.iter().map(|op| op.name.clone()).collect();
        if operators.len() > self.closure.capacity || names.len() != operators.len() {
            return Err(Error::msg(format!(
                "control_rotation_admission: shift {} capacity overflow or identity mismatch",
                self.index + 1,
            )));
        }
        for op in operators {
            if !self.allows(&op.name) {
                return Err(Error::msg(format!(
                    "control_rotation_membership: {} not admitted in shift {}",
                    op.name,
                    self.index + 1
                )));
            }
            let entry = &self.closure.candidates[&op.name].entry;
            if (op.elite, op.level, op.rarity)
                != (
                    entry.progress.elite,
                    entry.progress.level,
                    entry.progress.rarity,
                )
            {
                return Err(Error::msg(format!(
                    "control_admission_source: runtime progress changed for {}",
                    op.name
                )));
            }
        }
        if !self.required_names().is_subset(&names) {
            return Err(Error::msg(
                "control_plan_requirement: missing required anchor",
            ));
        }
        for group in self.requirements() {
            if names
                .iter()
                .filter(|name| group.candidates.contains(name))
                .count()
                < usize::from(group.min_count)
            {
                return Err(Error::msg(format!(
                    "control_plan_requirement: shift {} violates min_count {} for {:?}",
                    self.index + 1,
                    group.min_count,
                    group.candidates
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn assert_final_control_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    report: &TeamRotationReport,
) -> Result<()> {
    for shift in &report.shifts {
        let Some(admission) = &shift.assignment.control_rotation_admission else {
            if matches!(
                report.profile,
                TimedRotationProfile::Abc12_6_6
                    | TimedRotationProfile::Abc12_12_12
                    | TimedRotationProfile::Fiammetta8_8_4_4
            ) {
                return Err(Error::msg(
                    "assert-invariants control_rotation_admission: missing frozen runtime inputs",
                ));
            }
            continue;
        };
        if admission.active != shift.active_teams {
            return Err(Error::msg(
                "assert-invariants control_rotation_membership failed: changed shift scope",
            ));
        }
        admission
            .validate(&shift.assignment)
            .map_err(|error| Error::msg(format!("assert-invariants {error}")))?;
        let plan = report
            .assignment_plans
            .get(shift.plan_index)
            .ok_or_else(|| {
                Error::msg("assert-invariants control_plan_requirement: missing Plan")
            })?;
        for group in &admission.closure.requirements {
            let slot = plan
                .registry_claims
                .get(group.claim_index)
                .and_then(|claim| claim.slots.get(group.slot_index))
                .ok_or_else(|| {
                    Error::msg("assert-invariants control_plan_requirement: missing source slot")
                })?;
            let names: Vec<_> = slot.operators.iter().map(|op| op.name.clone()).collect();
            if names != group.requirement.candidates
                || slot.required_count != group.requirement.min_count
            {
                return Err(Error::msg(
                    "assert-invariants control_plan_requirement: changed source slot",
                ));
            }
        }
        for op in shift.assignment.operators_in(&admission.closure.room_id) {
            let entry = &admission.closure.candidates[&op.name].entry;
            let progress = operbox.progress_of(&op.name).ok_or_else(|| {
                Error::msg("assert-invariants control_admission_source: unowned member")
            })?;
            let actual_buffs = instances.resolve_control_buff_ids(&op.name, op.tier());
            let actual_tags = instances.tags_for(&op.name, op.tier());
            if progress != entry.progress
                || actual_buffs != entry.buff_ids
                || actual_tags != entry.tags
            {
                return Err(Error::msg(format!(
                    "assert-invariants control_admission_source: changed facts for {}: progress={:?}/{:?} buffs={:?}/{:?} tags={:?}/{:?}",
                    op.name, entry.progress, progress, entry.buff_ids, actual_buffs, entry.tags, actual_tags
                )));
            }
            if let CandidateOrigin::Group {
                claim_index,
                slot_index,
            } = entry_origin(admission, &op.name)
            {
                if plan
                    .registry_claims
                    .get(*claim_index)
                    .and_then(|claim| claim.slots.get(*slot_index))
                    .is_none()
                {
                    return Err(Error::msg(
                        "assert-invariants control_admission_source: missing group provenance",
                    ));
                }
            }
            if let Some(team) = admission.closure.team_of(&op.name) {
                let memberships: Vec<_> = report
                    .teams
                    .iter()
                    .filter(|cohort| cohort.operators.contains(&op.name))
                    .map(|cohort| cohort.label)
                    .collect();
                if memberships != vec![team] || !shift.active_teams.contains(&team) {
                    return Err(Error::msg(format!(
                        "assert-invariants control_rotation_membership: invalid cohort for {}",
                        op.name
                    )));
                }
            }
        }
        // Reconstruct capacity from the runtime blueprint, not the frozen copy.
        if blueprint
            .room(&admission.closure.room_id)
            .is_none_or(|room| room.operator_capacity() != admission.closure.capacity)
        {
            return Err(Error::msg(
                "assert-invariants control_rotation_admission: changed capacity",
            ));
        }
    }
    Ok(())
}

/// Data-contract witnesses for the sources repaired in this change. These run
/// in the final Rust gate, using the same model and instances as the report.
pub(crate) fn assert_control_mood_mapping(
    blueprint: &BaseBlueprint,
    instances: &OperatorInstances,
    table: &SkillTable,
    report: &TeamRotationReport,
) -> Result<()> {
    let model = crate::mood::default_mood_model()?;
    let rainbow = model
        .control_faction_recovery
        .iter()
        .find(|entry| entry.id == "rainbow_six")
        .ok_or_else(|| {
            Error::msg("assert-invariants control_mood_projection: missing rainbow mapping")
        })?;
    let providers: HashSet<_> = rainbow.providers.iter().map(String::as_str).collect();
    let members: HashSet<_> = rainbow.members.iter().map(String::as_str).collect();
    if providers != HashSet::from(["灰烬", "战车", "闪击", "霜华"])
        || members
            != HashSet::from([
                "灰烬",
                "战车",
                "闪击",
                "霜华",
                "艾拉",
                "医生",
                "双月",
                "导火索",
            ])
        || providers.len() != rainbow.providers.len()
        || members.len() != rainbow.members.len()
        || (rainbow.per_member - 0.05).abs() > 1e-9
    {
        return Err(Error::msg(
            "assert-invariants control_mood_projection: provider/member contract mismatch",
        ));
    }
    let smiley = model.control_smiley_recovery.as_ref().ok_or_else(|| {
        Error::msg("assert-invariants control_mood_projection: missing smiley mapping")
    })?;
    if smiley
        .providers
        .iter()
        .any(|provider| provider.name == "三角初华")
    {
        return Err(Error::msg(
            "assert-invariants control_mood_projection: dorm recovery treated as control smiley",
        ));
    }
    let delegated = [
        "control_mp_cost[000]",
        "control_mp_cost[001]",
        "control_mp_cost[002]",
        "control_mp_cost[003]",
        "control_mp_cost[004]",
        "control_mp_cost[005]",
        "control_mp_cost[006]",
        "control_mp_cost[009]",
        "control_mp_cost[011]",
        "control_mp_cost[013]",
        "control_mp_cost[014]",
        "control_mp_cost&faction[900]",
    ];
    for id in delegated {
        if table
            .get(id)
            .is_none_or(|skill| skill.facility != "control" || !skill.atoms.is_empty())
        {
            return Err(Error::msg(format!(
                "assert-invariants control_mood_projection: missing delegated source {id}"
            )));
        }
    }
    let witness = ["灰烬", "战车", "闪击", "医生", "双月"].map(|name| (name.to_string(), 0));
    if (model.control_faction_recovery_value(&witness) - 0.75).abs() > 1e-9
        || model
            .control_smiley_recovery_value(&[("三角初华".to_string(), 2)])
            .abs()
            > 1e-9
        || model
            .control_smiley_recovery_for_facility(
                FacilityKind::Dormitory,
                &[("玛恩纳".to_string(), 2), ("临光".to_string(), 0)],
            )
            .abs()
            > 1e-9
    {
        return Err(Error::msg("assert-invariants control_mood_projection: provider/member or target scope witness failed"));
    }
    for shift in &report.shifts {
        let actual: Vec<_> = shift
            .assignment
            .control_operators()
            .iter()
            .map(|op| (op.name.clone(), op.elite))
            .collect();
        for room in &blueprint.rooms {
            if crate::mood::facility_key(room.kind).is_none() {
                continue;
            }
            let projection = model.control_uniform_recovery_for_facility(room.kind, &actual);
            if !projection.is_finite() || projection < 0.0 {
                return Err(Error::msg(format!(
                    "assert-invariants control_mood_projection: invalid shift {} target {}",
                    shift.index + 1,
                    room.id.0
                )));
            }
        }
        for op in shift.assignment.control_operators() {
            if instances
                .resolve_control_buff_ids(&op.name, op.tier())
                .iter()
                .any(|id| delegated.contains(&id.as_str()) && table.get(id).is_none())
            {
                return Err(Error::msg(
                    "assert-invariants control_mood_projection: unresolved actual source",
                ));
            }
        }
    }
    Ok(())
}

fn entry_origin<'a>(admission: &'a ControlShiftAdmission, name: &str) -> &'a CandidateOrigin {
    &admission.closure.candidates[name].origin
}
