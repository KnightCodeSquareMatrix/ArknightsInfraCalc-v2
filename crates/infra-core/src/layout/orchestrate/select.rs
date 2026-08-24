//! System 选型：产出 `AssignmentPlan`（不调 solve）。

use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::select_registry_systems;
use crate::layout::system::SlotFillMode;
use crate::layout::tier::OperatorTier;
use crate::operbox::OperBox;
use crate::skill_table::{default_skill_table_path, SkillTable};

use super::plan::{registry_as_activated, AssignmentPlan};
use super::rules::{
    compile_rules, default_rule_catalog_path, load_rule_catalog, validate_rule_preferences,
    RuleCatalog, RuleCompileContext,
};

/// 根据 operbox / 蓝图 / 班次模式 / 种子编制构建编排计划。
/// `skip_system_ids`：静默跳过指定体系。
pub fn build_plan(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &std::collections::HashSet<String>,
) -> Result<AssignmentPlan> {
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let table = SkillTable::load(&default_skill_table_path()?)?;
    build_plan_with_runtime(
        blueprint,
        operbox,
        &instances,
        &table,
        24.0,
        &std::collections::HashMap::new(),
        mode,
        seed,
        skip_system_ids,
    )
}

/// 使用调用方已经加载的机制数据构建计划；资源 gate（例如感知 >= 50）读取候选计划
/// 的真实 resolve 结果。
#[allow(clippy::too_many_arguments)]
pub fn build_plan_with_runtime(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
    preferences: &std::collections::HashMap<String, String>,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &std::collections::HashSet<String>,
) -> Result<AssignmentPlan> {
    let mut disabled_conditional_packs = std::collections::HashSet::new();
    build_plan_with_runtime_and_disabled_packs(
        blueprint,
        operbox,
        instances,
        table,
        mood,
        preferences,
        mode,
        seed,
        skip_system_ids,
        &mut disabled_conditional_packs,
    )
}

/// 使用调用方持有的 conditional-pack 禁用集合构建计划；用于最终生产填充暴露新的
/// exact-presence 关系后，事务性重编译同一个 Plan。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_plan_with_runtime_and_disabled_packs(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
    preferences: &std::collections::HashMap<String, String>,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &std::collections::HashSet<String>,
    disabled_conditional_packs: &mut std::collections::HashSet<String>,
) -> Result<AssignmentPlan> {
    if mode != AssignShiftMode::Peak {
        return Ok(AssignmentPlan::recovery(mode));
    }

    let catalog = load_rule_catalog(&default_rule_catalog_path()?)?;
    validate_rule_preferences(&catalog, preferences)?;
    loop {
        let (plan, scratch) = compile_plan_once(
            &catalog,
            blueprint,
            operbox,
            instances,
            table,
            mood,
            preferences,
            mode,
            seed,
            skip_system_ids,
            disabled_conditional_packs,
        )?;
        let Some(pack_key) = guarded_pack_violation(blueprint, &scratch, &plan)? else {
            return Ok(plan);
        };
        if !disabled_conditional_packs.insert(pack_key.clone()) {
            return Err(Error::msg(format!(
                "conditional pack {pack_key} remained rotation-infeasible after downgrade"
            )));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_plan_once(
    catalog: &RuleCatalog,
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    mood: f64,
    preferences: &std::collections::HashMap<String, String>,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &std::collections::HashSet<String>,
    disabled_conditional_packs: &std::collections::HashSet<String>,
) -> Result<(AssignmentPlan, BaseAssignment)> {
    // priority >= 19 是用户确认的硬/主要体系；先于 legacy registry 编译。
    let mut compiled = compile_rules(
        catalog,
        &RuleCompileContext {
            blueprint,
            operbox,
            instances,
            table,
            mode,
            mood,
            preferences,
        },
        seed,
        &std::collections::HashSet::new(),
        &std::collections::HashSet::new(),
        skip_system_ids,
        disabled_conditional_packs,
        19..=i32::MAX,
    )?;
    let mut registry_skip = skip_system_ids.clone();
    registry_skip.extend(compiled.skip_registry_ids.iter().cloned());

    // Registry only sees the already resolved rule plan, so capacity and operator ownership have
    // a single ordering source.
    let mut scratch = compiled.scratch.clone();
    let mut used = scratch.operator_names();
    let inherited_excluded: std::collections::HashSet<_> =
        compiled.excluded_operators.iter().cloned().collect();
    used.extend(inherited_excluded.iter().cloned());
    let registry_claims = select_registry_systems(
        blueprint,
        operbox,
        mode,
        &scratch,
        &used,
        &registry_skip,
        &compiled.claimed_exclusive_groups,
    );
    for claim in &registry_claims {
        crate::layout::system::apply_registry_system_claim(
            blueprint,
            claim,
            &mut scratch,
            &mut used,
        )?;
    }
    // priority <= 18 的软候选只读取主要体系和 registry 都落位后的真实空位。
    let late = compile_rules(
        catalog,
        &RuleCompileContext {
            blueprint,
            operbox,
            instances,
            table,
            mode,
            mood,
            preferences,
        },
        &scratch,
        &inherited_excluded,
        &compiled
            .claimed_exclusive_groups
            .iter()
            .cloned()
            .chain(
                registry_claims
                    .iter()
                    .filter_map(|claim| claim.exclusive_group.clone()),
            )
            .collect(),
        &registry_skip,
        disabled_conditional_packs,
        i32::MIN..=18,
    )?;
    let mut late = late;
    let has_other_cross_station = registry_claims
        .iter()
        .any(|claim| claim.tier == OperatorTier::CrossStation && claim.system_id != "trade_core")
        || compiled
            .activated
            .iter()
            .chain(late.activated.iter())
            .any(|system| {
                system.tier == OperatorTier::CrossStation && system.system_id != "trade_core"
            });
    let mut mujica_catalog = catalog.clone();
    mujica_catalog
        .rules
        .retain(|rule| rule.id == "mujica_fallback");
    let mujica_inactive = compile_rules(
        &mujica_catalog,
        &RuleCompileContext {
            blueprint,
            operbox,
            instances,
            table,
            mode,
            mood,
            preferences: &std::collections::HashMap::new(),
        },
        &late.scratch,
        &inherited_excluded,
        &compiled
            .claimed_exclusive_groups
            .iter()
            .cloned()
            .chain(
                registry_claims
                    .iter()
                    .filter_map(|claim| claim.exclusive_group.clone()),
            )
            .collect(),
        &registry_skip,
        disabled_conditional_packs,
        i32::MIN..=i32::MAX,
    )?;
    late.inactive_control_exclusions
        .extend(mujica_inactive.inactive_control_exclusions);

    if !has_other_cross_station {
        let mut mujica_preferences = preferences.clone();
        mujica_preferences.insert("mujica_fallback".to_string(), "full_mujica".to_string());
        let mujica = compile_rules(
            &mujica_catalog,
            &RuleCompileContext {
                blueprint,
                operbox,
                instances,
                table,
                mode,
                mood,
                preferences: &mujica_preferences,
            },
            &late.scratch,
            &inherited_excluded,
            &compiled
                .claimed_exclusive_groups
                .iter()
                .cloned()
                .chain(
                    registry_claims
                        .iter()
                        .filter_map(|claim| claim.exclusive_group.clone()),
                )
                .collect(),
            &registry_skip,
            disabled_conditional_packs,
            i32::MIN..=i32::MAX,
        )?;
        late.activated.extend(mujica.activated);
        late.selected.extend(mujica.selected);
        late.anchors.extend(mujica.anchors);
        late.constraints.extend(mujica.constraints);
        late.excluded_operators.extend(mujica.excluded_operators);
        late.inactive_control_exclusions
            .extend(mujica.inactive_control_exclusions);
        late.shift_binds.extend(mujica.shift_binds);
        late.active_dependencies.extend(mujica.active_dependencies);
        late.continuous_roles.extend(mujica.continuous_roles);
        late.rotation_reserves.extend(mujica.rotation_reserves);
        late.skip_registry_ids.extend(mujica.skip_registry_ids);
        late.claimed_exclusive_groups
            .extend(mujica.claimed_exclusive_groups);
        late.scratch = mujica.scratch;
    }
    let final_scratch = late.scratch.clone();
    compiled.activated.extend(late.activated);
    compiled.selected.extend(late.selected);
    compiled.anchors.extend(late.anchors);
    compiled.constraints.extend(late.constraints);
    compiled.excluded_operators.extend(late.excluded_operators);
    compiled
        .inactive_control_exclusions
        .extend(late.inactive_control_exclusions);
    compiled.shift_binds.extend(late.shift_binds);
    compiled
        .active_dependencies
        .extend(late.active_dependencies);
    compiled.continuous_roles.extend(late.continuous_roles);
    compiled.rotation_reserves.extend(late.rotation_reserves);
    let mut activated: Vec<_> = registry_claims.iter().map(registry_as_activated).collect();
    activated.extend(compiled.activated);
    let control_candidate_requirements = registry_claims
        .iter()
        .flat_map(|claim| &claim.slots)
        .filter(|slot| {
            slot.facility == crate::layout::FacilityKind::ControlCenter
                && slot.fill == SlotFillMode::Search
                && slot.required_count > 0
        })
        .map(|slot| super::plan::ControlCandidateRequirement {
            candidates: slot.operators.iter().map(|op| op.name.clone()).collect(),
            min_count: slot.required_count,
        })
        .collect();

    let mut control_exclusions: std::collections::HashSet<_> =
        compiled.inactive_control_exclusions.into_iter().collect();
    control_exclusions
        .extend(crate::layout::system::control_exclusions_for_unclaimed_systems(&registry_claims));
    for claim in &registry_claims {
        for operator in &claim.control_allowed_operators {
            control_exclusions.remove(operator);
        }
    }
    for anchor in compiled
        .anchors
        .iter()
        .filter(|anchor| anchor.facility == crate::layout::FacilityKind::ControlCenter)
    {
        control_exclusions.remove(&anchor.operator);
    }
    control_exclusions.extend(compiled.excluded_operators.iter().cloned());
    let plan = AssignmentPlan {
        mode,
        activated,
        registry_claims,
        selected_rules: compiled.selected,
        anchors: compiled.anchors,
        producers: Vec::new(),
        constraints: compiled.constraints,
        excluded_operators: compiled.excluded_operators,
        control_exclusions: control_exclusions.into_iter().collect(),
        degradations: Vec::new(),
        shift_binds: compiled.shift_binds,
        active_dependencies: compiled.active_dependencies,
        resolved_producer_dependencies: Vec::new(),
        continuous_roles: compiled.continuous_roles,
        rotation_reserves: compiled.rotation_reserves,
        control_candidate_requirements,
    };
    Ok((plan, final_scratch))
}

pub(crate) fn guarded_pack_violation(
    blueprint: &BaseBlueprint,
    scratch: &BaseAssignment,
    plan: &AssignmentPlan,
) -> Result<Option<String>> {
    let guarded: Vec<_> = plan
        .rotation_reserves
        .iter()
        .filter(|reserve| {
            reserve.require_pre_split_halves
                && reserve.reuse_policy == super::plan::ReserveReusePolicy::EveryEligibleHalf
        })
        .collect();
    if guarded.is_empty() {
        return Ok(None);
    }
    let halves = super::pack_production_components(
        blueprint,
        scratch,
        plan.shift_binds
            .iter()
            .map(|bind| bind.operators.as_slice()),
    )
    .map_err(Error::msg)?;
    for reserve in guarded {
        let eligible_count = |component: &[crate::layout::RoomId]| {
            component
                .iter()
                .filter(|room| reserve.eligible_rooms.contains(room))
                .count()
        };
        let has_target = |components: &[Vec<crate::layout::RoomId>]| {
            components
                .iter()
                .any(|component| eligible_count(component) > 0)
        };
        if has_target(&halves[0]) && has_target(&halves[1]) {
            continue;
        }
        let selected = plan
            .selected_rules
            .iter()
            .find(|selected| selected.rule_id == reserve.system_id)
            .ok_or_else(|| {
                Error::msg(format!(
                    "rotation reserve {} has no selected rule owner {}",
                    reserve.reserve_id, reserve.system_id
                ))
            })?;
        let pack_id = selected.conditional_pack_id.as_deref().ok_or_else(|| {
            Error::msg(format!(
                "guarded rotation reserve {} is not owned by a conditional pack",
                reserve.reserve_id
            ))
        })?;
        return Ok(Some(format!("{}:{pack_id}", reserve.system_id)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::orchestrate::{
        ReserveReusePolicy, ResolvedRoleReserve, SelectedRuleAlternative, ShiftBind,
    };
    use crate::layout::{AssignedOperator, RoomId};

    #[test]
    fn guarded_pack_rejects_two_targets_naturally_packed_into_one_half() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        let bound_rooms = ["manu_1", "manu_2", "manu_3", "manu_4", "power_1"];
        let bound_operators: Vec<_> = bound_rooms
            .iter()
            .enumerate()
            .map(|(index, room)| {
                let name = format!("bound_{index}");
                assignment.set_room(*room, vec![AssignedOperator::new(&name, 2)]);
                name
            })
            .collect();

        let mut plan = AssignmentPlan::recovery(AssignShiftMode::Peak);
        plan.selected_rules.push(SelectedRuleAlternative {
            rule_id: "guarded_rule".to_string(),
            alternative_id: "base".to_string(),
            conditional_pack_id: Some("guarded_pack".to_string()),
            priority: 1,
            operators: Vec::new(),
        });
        plan.shift_binds.push(ShiftBind {
            operators: bound_operators,
            on_shifts: 2,
            off_shifts: 1,
        });
        plan.rotation_reserves.push(ResolvedRoleReserve {
            system_id: "guarded_rule".to_string(),
            reserve_id: "reserve".to_string(),
            role_id: "role".to_string(),
            facility: crate::layout::FacilityKind::TradePost,
            operators: Vec::new(),
            eligible_rooms: vec![RoomId::from("trade_1"), RoomId::from("trade_2")],
            reuse_policy: ReserveReusePolicy::EveryEligibleHalf,
            require_pre_split_halves: true,
        });

        let halves = crate::layout::pack_production_components(
            &blueprint,
            &assignment,
            plan.shift_binds
                .iter()
                .map(|bind| bind.operators.as_slice()),
        )
        .unwrap();
        let trade_target_count = |components: &[Vec<RoomId>]| {
            components
                .iter()
                .flatten()
                .filter(|room| room.0 == "trade_1" || room.0 == "trade_2")
                .count()
        };
        assert_eq!(
            [
                trade_target_count(&halves[0]),
                trade_target_count(&halves[1])
            ],
            [0, 2],
            "counterexample requires two independent trade targets in one natural half"
        );
        assert_eq!(
            guarded_pack_violation(&blueprint, &assignment, &plan).unwrap(),
            Some("guarded_rule:guarded_pack".to_string())
        );
    }
}
