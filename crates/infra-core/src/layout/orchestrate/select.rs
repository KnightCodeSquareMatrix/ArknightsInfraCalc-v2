//! System 选型：产出 `AssignmentPlan`（不调 solve）。

use crate::error::{Error, Result};
use crate::instances::{default_instances_path, OperatorInstances};
use crate::layout::assignment::BaseAssignment;
use crate::layout::blueprint::BaseBlueprint;
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::SlotFillMode;
use crate::layout::tier::OperatorTier;
use crate::operbox::OperBox;
use crate::skill_table::{default_skill_table_path, SkillTable};

const ABYSSAL_CONTROL_BETA_BUFF: &str = "control_mp_aegir2[010]";
const ABYSSAL_TAG: &str = "cc.g.abyssal";

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

/// 使用调用方已经加载的机制数据构建计划；资源 gate（例如感知 >= 60）读取候选计划
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
        &std::collections::HashSet::new(),
        &std::collections::HashSet::new(),
        false,
        None,
        None,
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
    skipped_optional_control_roles: &std::collections::HashSet<(String, String, String)>,
    skipped_optional_trade_roles: &std::collections::HashSet<(String, String, String)>,
    support_combinations_enable: bool,
    registry_sustain_hours: Option<f64>,
    control_work_hours: Option<f64>,
    mode: AssignShiftMode,
    seed: &BaseAssignment,
    skip_system_ids: &std::collections::HashSet<String>,
    disabled_conditional_packs: &mut std::collections::HashSet<String>,
) -> Result<AssignmentPlan> {
    if crate::layout::blackkey_witch_same_trade_room(seed, blueprint) {
        return Err(Error::msg("trade seed cannot colocate 黑键 and 巫恋"));
    }

    let mut catalog = load_rule_catalog(&default_rule_catalog_path()?)?;
    validate_rule_preferences(&catalog, preferences)?;
    if seed.auto_rotation {
        super::rules::restrict_suggestion_catalog(&mut catalog);
    }
    if !support_combinations_enable && !seed.auto_rotation {
        for rule in &mut catalog.rules {
            for alternative in &mut rule.alternatives {
                alternative.roles.retain(|role| !role.proactive_support);
                for pack in &mut alternative.conditional_packs {
                    pack.roles.retain(|role| !role.proactive_support);
                }
            }
        }
    }
    for rule in &mut catalog.rules {
        for alternative in &mut rule.alternatives {
            for role in alternative.roles.iter_mut().chain(
                alternative
                    .conditional_packs
                    .iter_mut()
                    .flat_map(|pack| &mut pack.roles),
            ) {
                if role.min_count == 0
                    && role.facility == crate::layout::FacilityKind::ControlCenter
                {
                    role.candidates.retain(|candidate| {
                        !skipped_optional_control_roles.contains(&(
                            rule.id.clone(),
                            role.id.clone(),
                            candidate.name.clone(),
                        ))
                    });
                }
            }
        }
    }
    loop {
        for rule in &mut catalog.rules {
            for alternative in &mut rule.alternatives {
                for role in alternative.roles.iter_mut().chain(
                    alternative
                        .conditional_packs
                        .iter_mut()
                        .flat_map(|pack| &mut pack.roles),
                ) {
                    if !role.optional_trade_recommendation {
                        continue;
                    }
                    for (system_id, role_id, name) in skipped_optional_trade_roles {
                        if system_id == &rule.id && role_id == &role.id {
                            role.candidates.retain(|candidate| candidate.name != *name);
                        }
                    }
                }
            }
        }
        let (mut plan, scratch) = compile_plan_once(
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
            registry_sustain_hours,
            control_work_hours,
        )?;
        plan.support_combinations_enable = support_combinations_enable;
        plan.auto_rotation = seed.auto_rotation;
        if crate::layout::blackkey_witch_same_trade_room(&scratch, blueprint) {
            return Err(Error::msg("trade plan cannot colocate 黑键 and 巫恋"));
        }
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
    registry_sustain_hours: Option<f64>,
    control_work_hours: Option<f64>,
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
            shift_hours: seed.work_hours.unwrap_or(24.0),
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
    let mut mood_admission = registry_mood_admission(control_work_hours, instances, mood)?;
    let admit_all = |claim: &crate::layout::system::RegistrySystemClaim, trial: &BaseAssignment| {
        !seed.auto_rotation
            || (claim
                .slots
                .iter()
                .map(|slot| &slot.room_id)
                .collect::<std::collections::HashSet<_>>()
                .len()
                <= 1
                && !claim
                    .slots
                    .iter()
                    .any(|slot| slot.facility == crate::layout::FacilityKind::ControlCenter)
                && crate::layout::admission::assignment_admitted(
                    blueprint, trial, instances, table,
                ))
    };
    let mut admission = |claim: &crate::layout::system::RegistrySystemClaim,
                         trial: &BaseAssignment| {
        admit_all(claim, trial)
            && mood_admission
                .as_deref_mut()
                .is_none_or(|admit| admit(claim, trial))
    };
    let registry_claims = crate::layout::system::select_registry_systems_with_admission(
        blueprint,
        operbox,
        mode,
        &scratch,
        &used,
        &registry_skip,
        &compiled.claimed_exclusive_groups,
        registry_sustain_hours.filter(|_| mode == AssignShiftMode::Peak),
        &mut admission,
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
            shift_hours: seed.work_hours.unwrap_or(24.0),
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
    // Mujica is the lowest-priority complete control combination: any other
    // combination that can claim control, including ABC's deferred Abyssal
    // branch, must suppress it. Generic bonuses do not set this flag.
    let has_other_cross_station = registry_claims.iter().any(|claim| {
        claim.tier == OperatorTier::CrossStation
            && claim.system_id != "trade_core"
            && claim
                .slots
                .iter()
                .any(|slot| slot.facility == crate::layout::FacilityKind::ControlCenter)
    }) || compiled.activated.iter().chain(late.activated.iter()).any(
        |system| {
            system.tier == OperatorTier::CrossStation
                && system.system_id != "trade_core"
                && system.slots.iter().any(|slot| {
                    matches!(
                        slot,
                        super::plan::SlotFill::Fixed {
                            facility: crate::layout::FacilityKind::ControlCenter,
                            ..
                        } | super::plan::SlotFill::OptionalFixed {
                            facility: crate::layout::FacilityKind::ControlCenter,
                            ..
                        } | super::plan::SlotFill::PickOne {
                            facility: crate::layout::FacilityKind::ControlCenter,
                            ..
                        }
                    )
                })
        },
    ) || has_usable_abyssal_combination(operbox, instances, &scratch);
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
            shift_hours: seed.work_hours.unwrap_or(24.0),
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
                shift_hours: seed.work_hours.unwrap_or(24.0),
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

    // 有序集合：该列表会原样写入 rotation 输出，HashSet 的迭代顺序每次运行都不同。
    let mut control_exclusions: std::collections::BTreeSet<_> =
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
        support_combinations_enable: false,
        auto_rotation: seed.auto_rotation,
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
    plan.control_requirement_sources()?;
    Ok((plan, final_scratch))
}

fn registry_mood_admission<'a>(
    hours: Option<f64>,
    instances: &'a OperatorInstances,
    initial_mood: f64,
) -> Result<
    Option<
        Box<dyn FnMut(&crate::layout::system::RegistrySystemClaim, &BaseAssignment) -> bool + 'a>,
    >,
> {
    let Some(hours) = hours else {
        return Ok(None);
    };
    let model = crate::mood::default_mood_model()?;
    const ABYSSAL_CONTROL_DRAIN_BUFF: &str = "control_mp_aegir2[010]";
    Ok(Some(Box::new(move |claim, _trial| {
        let control_claims: Vec<_> = claim
            .slots
            .iter()
            .filter(|slot| slot.facility == crate::layout::FacilityKind::ControlCenter)
            .collect();
        if !control_claims
            .iter()
            .any(|slot| slot.fill != SlotFillMode::Search && !slot.operators.is_empty())
        {
            return true;
        }
        if control_claims
            .iter()
            .any(|slot| slot.fill == SlotFillMode::Search)
        {
            return true;
        }
        let available = initial_mood.min(model.mood_cap);
        control_claims
            .iter()
            .flat_map(|slot| &slot.operators)
            .all(|op| {
                // Abyssal drain depends on outside-dorm presence, which is not
                // a fixed per-operator fact at registry selection time.
                if instances
                    .resolve_control_buff_ids(&op.name, op.tier())
                    .iter()
                    .any(|buff_id| buff_id == ABYSSAL_CONTROL_DRAIN_BUFF)
                {
                    return true;
                }
                let room_ops = vec![(op.name.clone(), op.elite)];
                let inputs = crate::mood::DrainInputs {
                    name: &op.name,
                    elite: op.elite,
                    facility: crate::layout::FacilityKind::ControlCenter,
                    room_ops: &room_ops,
                    full_control: true,
                    control_ops: &[],
                    recipe: None,
                };
                crate::mood::fits_work_budget(
                    &model,
                    available,
                    crate::mood::operator_net_drain(&model, &inputs),
                    hours,
                )
            })
    })))
}

fn has_usable_abyssal_combination(
    operbox: &OperBox,
    instances: &OperatorInstances,
    seed: &BaseAssignment,
) -> bool {
    let used = seed.operator_names();
    let mut producer_available = false;
    let mut hunters = 0usize;
    for entry in operbox
        .entries
        .iter()
        .filter(|entry| entry.own && !used.contains(&entry.name))
    {
        let tier = crate::tier::PromotionTier::from_elite_rarity_level(
            entry.elite,
            entry.rarity,
            entry.level,
        );
        let control_buffs = instances.resolve_control_buff_ids(&entry.name, tier);
        if control_buffs
            .iter()
            .any(|buff| buff == ABYSSAL_CONTROL_BETA_BUFF)
        {
            producer_available = true;
        }
        let is_hunter = instances
            .tags_for(&entry.name, tier)
            .iter()
            .any(|tag| tag == ABYSSAL_TAG)
            && !control_buffs
                .iter()
                .any(|buff| buff == ABYSSAL_CONTROL_BETA_BUFF);
        if is_hunter {
            hunters += 1;
        }
    }
    // The control bonus is enabled once Gladia and three other Abyssal
    // hunters are available. Room and skill feasibility is resolved by S2.
    producer_available && hunters >= 3
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
