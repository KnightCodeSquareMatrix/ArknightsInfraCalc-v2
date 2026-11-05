//! 声明式体系规则编译器。
//!
//! 这里没有“每个体系一个函数”。所有体系共享同一套 gate / role / relation 编译流程；
//! 体系差异只存在于 `data/orchestration_rules.json`。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::global_resource::GlobalResourceKey;
use crate::instances::OperatorInstances;
use crate::layout::assignment::{AssignedOperator, BaseAssignment};
use crate::layout::blueprint::{BaseBlueprint, FacilityKind, RoomBlueprint, RoomProduct};
use crate::layout::shift::AssignShiftMode;
use crate::layout::tier::OperatorTier;
use crate::operbox::OperBox;
use crate::skill_table::SkillTable;
use crate::trade::input::TradeOrderKind;
use crate::types::RecipeKind;

use super::plan::{
    ActivatedSystem, ActiveDependency, AnchorFillPolicy, ContinuousRole, ReserveReusePolicy,
    SelectedRuleAlternative, ShiftBind, SlotFill, SystemAnchor, SystemConstraint,
};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RuleCatalog {
    pub version: u32,
    pub rules: Vec<RuleSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RuleSpec {
    pub id: String,
    pub priority: i32,
    #[serde(default)]
    pub auto_only: bool,
    #[serde(default = "cross_station")]
    pub tier: OperatorTier,
    /// Only one selected rule in the same group may enter the plan.
    #[serde(default)]
    pub exclusive_group: Option<String>,
    #[serde(default)]
    pub skip_registry_ids: Vec<String>,
    /// 本规则未选中任何 alternative 时仍需关闭的 legacy registry 路径。
    #[serde(default)]
    pub skip_registry_ids_when_inactive: Vec<String>,
    pub alternatives: Vec<AlternativeSpec>,
}

fn cross_station() -> OperatorTier {
    OperatorTier::CrossStation
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AlternativeSpec {
    pub id: String,
    /// 仅在通用 `--prefer rule=alternative` 明确选择时参与候选。
    #[serde(default)]
    pub explicit_only: bool,
    #[serde(default)]
    pub gates: Vec<GateSpec>,
    #[serde(default)]
    pub roles: Vec<RoleSpec>,
    /// All factory rooms claimed by this alternative must have their full complement.
    #[serde(default)]
    pub require_factory_capacity: bool,
    /// 满足自身 gate 时优先事务性替换部分基础 roles；整包不可行则回退原 alternative。
    #[serde(default)]
    pub conditional_packs: Vec<ConditionalPackSpec>,
    #[serde(default)]
    pub relations: Vec<RelationSpec>,
    #[serde(default)]
    pub active_dependencies: Vec<ActiveDependencySpec>,
    /// alternative 激活后从本班所有普通候选中排除；用于表达路径级实际互斥。
    #[serde(default)]
    pub exclude_operators: Vec<String>,
    /// These control candidates are unavailable unless this alternative is selected.
    #[serde(default)]
    pub exclude_control_when_inactive: Vec<String>,
    #[serde(default)]
    pub bind_all: bool,
    /// 只绑定这些 role 的实际入选成员；用于区分体系 producer 与非 producer 路径收益位。
    #[serde(default)]
    pub bind_roles: Vec<String>,
    /// Independent room cohorts; binding the entire cross-station alternative would prevent splitting.
    #[serde(default)]
    pub bind_role_groups: Vec<Vec<String>>,
    #[serde(default = "two")]
    pub on_shifts: u8,
    #[serde(default = "one")]
    pub off_shifts: u8,
    #[serde(default)]
    pub continuous: bool,
    #[serde(default)]
    pub continuous_role_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConditionalPackSpec {
    pub id: String,
    #[serde(default)]
    pub gates: Vec<GateSpec>,
    #[serde(default)]
    pub replace_roles: Vec<String>,
    #[serde(default)]
    pub roles: Vec<RoleSpec>,
    #[serde(default)]
    pub relations: Vec<RelationSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ActiveDependencySpec {
    /// 这些 role 中任一实际入选成员在岗时触发依赖。
    pub consumer_roles: Vec<String>,
    /// 只要求这些 role 中本次实际入选的成员在基建内。
    pub required_roles: Vec<String>,
}

fn two() -> u8 {
    2
}

fn one() -> u8 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum GateSpec {
    FacilityCount {
        facility: FacilityKind,
        #[serde(default)]
        min: u8,
        max: Option<u8>,
    },
    TradeOrderCount {
        order: TradeOrderKind,
        #[serde(default)]
        min: u8,
        max: Option<u8>,
    },
    ManufactureRecipeCount {
        recipe: RecipeKind,
        #[serde(default)]
        min: u8,
        max: Option<u8>,
    },
    ResourceAtLeast {
        resource: String,
        value: f64,
    },
    OperatorAvailable {
        name: String,
        #[serde(default)]
        elite: u8,
    },
    OperatorUnavailable {
        name: String,
        #[serde(default)]
        elite: u8,
    },
    /// 复用正式贸易 role + solver 验证候选 assignment 剩余人员能否组成合法 cohort。
    TradeRoleFeasible {
        role: String,
        order: TradeOrderKind,
        #[serde(default)]
        all_matching_rooms: bool,
        /// 成功时把正式搜索选中的同一组成员写入 Plan，供 peak 预留与 rotation 消费。
        #[serde(default)]
        reserve_as: Option<String>,
        /// reserve 在 gamma half 中的复用方式；只有生成 reserve 时才有实际作用。
        #[serde(default)]
        reuse_policy: ReserveReusePolicy,
        /// reserve 必须在 exact bind 的自然 H1/H2 打包结果中两边均有目标。
        /// 不满足时整个 conditional pack 事务性降级，而不是留给 schedule 搬房或报错。
        #[serde(default)]
        require_pre_split_halves: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct CandidateSpec {
    pub name: String,
    #[serde(default)]
    pub elite: u8,
    #[serde(default)]
    pub fallback_if_no_other_candidates: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RoleSelection {
    #[default]
    Ordered,
    AllAvailable,
    ManufactureObjective,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RoomLevelPreference {
    #[default]
    BlueprintOrder,
    LowestLevel,
    #[serde(rename = "level_2_first")]
    Level2First,
    HighestLevel,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RoleSpec {
    pub id: String,
    #[serde(default)]
    pub proactive_support: bool,
    #[serde(default)]
    pub optional_trade_recommendation: bool,
    pub facility: FacilityKind,
    #[serde(default)]
    pub facilities: Vec<FacilityKind>,
    /// 这些设施允许与原有住员共享；one_per_room 仍限制本 role 每房一名。
    #[serde(default)]
    pub shared_room_facilities: Vec<FacilityKind>,
    #[serde(default)]
    pub recipe: Option<RecipeKind>,
    #[serde(default)]
    pub trade_order: Option<TradeOrderKind>,
    #[serde(default)]
    pub min_room_level: u8,
    #[serde(default)]
    pub room_level_preference: RoomLevelPreference,
    pub candidates: Vec<CandidateSpec>,
    #[serde(default = "one")]
    pub min_count: u8,
    #[serde(default = "one")]
    pub max_count: u8,
    #[serde(default)]
    pub selection: RoleSelection,
    #[serde(default)]
    pub same_room: bool,
    /// 具有相同 group 的多个 role 必须落在同一个房间。
    #[serde(default)]
    pub room_group: Option<String>,
    /// 每间房最多放置本 role 一名候选，适用于“真实空余支持位”。
    #[serde(default)]
    pub one_per_room: bool,
    /// 由对应生产域的统一 solver 补满已解析房间。
    #[serde(default)]
    pub fill_to_capacity: bool,
    /// 从主动支持依赖派生，开关过滤前保留；不是运行时支持承诺。
    #[serde(skip)]
    pub support_receiver: bool,
    #[serde(default)]
    pub work_mood: Option<u8>,
    /// Apply the explicit work mood only when this operator is already in the
    /// same resolved alternative.
    #[serde(default)]
    pub work_mood_if_operator_present: Option<String>,
    #[serde(default)]
    pub rest_facility: Option<FacilityKind>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum RelationSpec {
    ForbidSameRoom { a: String, b: String },
    ForbidSameStation { a: String, b: String },
}

pub(super) struct RuleCompileContext<'a> {
    pub blueprint: &'a BaseBlueprint,
    pub operbox: &'a OperBox,
    pub instances: &'a OperatorInstances,
    pub table: &'a SkillTable,
    pub mode: AssignShiftMode,
    pub mood: f64,
    pub shift_hours: f64,
    pub preferences: &'a HashMap<String, String>,
}

#[derive(Debug)]
pub(super) struct CompiledRules {
    pub activated: Vec<ActivatedSystem>,
    pub selected: Vec<SelectedRuleAlternative>,
    pub anchors: Vec<SystemAnchor>,
    pub constraints: Vec<SystemConstraint>,
    pub excluded_operators: Vec<String>,
    pub inactive_control_exclusions: Vec<String>,
    pub shift_binds: Vec<ShiftBind>,
    pub active_dependencies: Vec<ActiveDependency>,
    pub continuous_roles: Vec<ContinuousRole>,
    pub rotation_reserves: Vec<super::plan::ResolvedRoleReserve>,
    pub skip_registry_ids: HashSet<String>,
    pub claimed_exclusive_groups: HashSet<String>,
    pub scratch: BaseAssignment,
}

pub(super) fn load_rule_catalog(path: &Path) -> Result<RuleCatalog> {
    let raw = std::fs::read_to_string(path)?;
    let catalog: RuleCatalog = serde_json::from_str(&raw).map_err(|error| {
        Error::msg(format!(
            "orchestration rules parse {}: {error}",
            path.display()
        ))
    })?;
    if catalog.version != 1 {
        return Err(Error::msg(format!(
            "unsupported orchestration rules version {} in {}",
            catalog.version,
            path.display()
        )));
    }
    Ok(catalog)
}

pub(super) fn default_rule_catalog_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("orchestration_rules.json")
}

/// 仅显式 gate 检查 Plan 的来源，不在生产候选中重新证明协同。
pub(crate) fn assert_auto_rotation_plan_policy(
    plans: &[super::plan::AssignmentPlan],
) -> Result<()> {
    let mut catalog = load_rule_catalog(&default_rule_catalog_path()?)?;
    restrict_auto_rotation_catalog(&mut catalog);
    for plan in plans {
        if !plan.auto_rotation || !plan.registry_claims.is_empty() {
            return Err(Error::msg(
                "assert-invariants auto_plan_capability failed: registry or mode",
            ));
        }
        for selected in &plan.selected_rules {
            let allowed = catalog
                .rules
                .iter()
                .find(|rule| rule.id == selected.rule_id)
                .and_then(|rule| {
                    rule.alternatives
                        .iter()
                        .find(|alt| alt.id == selected.alternative_id)
                });
            if allowed.is_none_or(|alt| {
                selected
                    .conditional_pack_id
                    .as_ref()
                    .is_some_and(|id| !alt.conditional_packs.iter().any(|pack| pack.id == *id))
            }) {
                return Err(Error::msg(format!(
                    "assert-invariants auto_plan_capability failed: {}={}",
                    selected.rule_id, selected.alternative_id
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn assert_support_combination_policy(
    plans: &[super::plan::AssignmentPlan],
    shifts: &[crate::schedule::TeamShiftResult],
) -> Result<()> {
    for shift in shifts {
        let plan = plans.get(shift.plan_index).ok_or_else(|| {
            Error::msg("assert-invariants support_combination_policy failed: missing Plan")
        })?;
        if plan.support_combinations_enable || plan.auto_rotation {
            continue;
        }
        // Candidate lists overlap with ordinary domain winners; preserve role provenance.
        if let Some(anchor) = plan.anchors.iter().find(|anchor| anchor.proactive_support) {
            let present = anchor.room_id.as_ref().is_some_and(|room| {
                shift
                    .assignment
                    .operators_in(room)
                    .iter()
                    .any(|op| op.name == anchor.operator)
            });
            return Err(Error::msg(format!(
                "assert-invariants support_combination_policy failed in shift {}: proactive claim {} (present={present}) while disabled",
                shift.index + 1, anchor.operator,
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_rule_preferences(
    catalog: &RuleCatalog,
    preferences: &HashMap<String, String>,
) -> Result<()> {
    for (rule_id, alternative_id) in preferences {
        let rule = catalog
            .rules
            .iter()
            .find(|rule| rule.id == *rule_id)
            .ok_or_else(|| {
                Error::msg(format!("unknown orchestration rule in --prefer: {rule_id}"))
            })?;
        if !rule
            .alternatives
            .iter()
            .any(|alternative| alternative.id == *alternative_id)
        {
            return Err(Error::msg(format!(
                "unknown alternative in --prefer: {rule_id}={alternative_id}"
            )));
        }
    }
    Ok(())
}

pub(super) fn compile_rules(
    catalog: &RuleCatalog,
    ctx: &RuleCompileContext<'_>,
    seed: &BaseAssignment,
    inherited_excluded: &HashSet<String>,
    inherited_exclusive_groups: &HashSet<String>,
    externally_skipped: &HashSet<String>,
    disabled_conditional_packs: &HashSet<String>,
    priority_range: std::ops::RangeInclusive<i32>,
) -> Result<CompiledRules> {
    let mut rules: Vec<_> = catalog.rules.iter().collect();
    rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority));

    let mut out = CompiledRules {
        activated: Vec::new(),
        selected: Vec::new(),
        anchors: Vec::new(),
        constraints: Vec::new(),
        excluded_operators: Vec::new(),
        inactive_control_exclusions: Vec::new(),
        shift_binds: Vec::new(),
        active_dependencies: Vec::new(),
        continuous_roles: Vec::new(),
        rotation_reserves: Vec::new(),
        skip_registry_ids: HashSet::new(),
        claimed_exclusive_groups: inherited_exclusive_groups.clone(),
        scratch: seed.clone(),
    };
    // Keep actual placements separate from inherited exclusions. An alternative may repeat an
    // already inherited exclusion, but neither its roles nor later alternatives may select an
    // excluded operator.
    let mut used = seed.operator_names();
    let mut excluded = inherited_excluded.clone();

    if ctx.mode != AssignShiftMode::Peak {
        return Ok(out);
    }

    for rule in rules {
        if rule.auto_only && !seed.auto_rotation {
            continue;
        }
        if !priority_range.contains(&rule.priority) {
            continue;
        }
        if externally_skipped.contains(&rule.id) {
            out.skip_registry_ids
                .extend(rule.skip_registry_ids_when_inactive.iter().cloned());
            continue;
        }
        if rule
            .exclusive_group
            .as_ref()
            .is_some_and(|group| out.claimed_exclusive_groups.contains(group))
        {
            continue;
        }
        let mut alternatives: Vec<_> = rule.alternatives.iter().collect();
        if let Some(preferred) = ctx.preferences.get(&rule.id) {
            alternatives.sort_by_key(|alternative| (alternative.id != *preferred,));
        }

        let mut selected_alternative = false;
        for alternative in alternatives {
            if alternative.explicit_only && ctx.preferences.get(&rule.id) != Some(&alternative.id) {
                continue;
            }
            if !static_gates_match(&alternative.gates, ctx) {
                continue;
            }
            if alternative
                .exclude_operators
                .iter()
                .any(|name| used.contains(name))
            {
                continue;
            }
            let mut variants: Vec<(
                Vec<RoleSpec>,
                Vec<RelationSpec>,
                Option<(&str, &[GateSpec])>,
            )> = alternative
                .conditional_packs
                .iter()
                .filter(|pack| {
                    !disabled_conditional_packs.contains(&format!("{}:{}", rule.id, pack.id))
                })
                .filter(|pack| static_gates_match(&pack.gates, ctx))
                .map(|pack| {
                    (
                        roles_with_conditional_pack(&alternative.roles, pack),
                        alternative
                            .relations
                            .iter()
                            .chain(&pack.relations)
                            .cloned()
                            .collect(),
                        Some((pack.id.as_str(), pack.gates.as_slice())),
                    )
                })
                .collect();
            variants.push((
                alternative.roles.clone(),
                alternative.relations.clone(),
                None,
            ));
            let mut placed = None;
            for (candidate_roles, candidate_relations, conditional_pack) in variants {
                let mut candidate_assignment = out.scratch.clone();
                let mut candidate_used = used.clone();
                candidate_used.extend(excluded.iter().cloned());
                let mut candidate_anchors = Vec::new();
                let candidate_constraints = compile_relations(&candidate_relations);
                if !place_roles(
                    &rule.id,
                    &candidate_roles,
                    ctx,
                    &out.constraints,
                    &candidate_constraints,
                    &mut candidate_assignment,
                    &mut candidate_used,
                    &mut candidate_anchors,
                ) {
                    continue;
                }
                if alternative.require_factory_capacity
                    && candidate_anchors
                        .iter()
                        .filter(|anchor| anchor.facility == FacilityKind::Factory)
                        .filter_map(|anchor| anchor.room_id.as_ref())
                        .any(|room_id| {
                            ctx.blueprint.room(room_id).is_none_or(|room| {
                                candidate_assignment.operators_in(room_id).len()
                                    != room.operator_capacity()
                            })
                        })
                {
                    continue;
                }
                let mut candidate_reserves = Vec::new();
                if let Some((_, gates)) = conditional_pack {
                    let Some(reserves) =
                        dynamic_gates_match(gates, ctx, &candidate_assignment, &candidate_used)?
                    else {
                        continue;
                    };
                    candidate_reserves.extend(reserves);
                }
                let Some(reserves) = dynamic_gates_match(
                    &alternative.gates,
                    ctx,
                    &candidate_assignment,
                    &candidate_used,
                )?
                else {
                    continue;
                };
                candidate_reserves.extend(reserves);
                placed = Some((
                    candidate_assignment,
                    candidate_used,
                    candidate_anchors,
                    candidate_constraints,
                    candidate_roles,
                    conditional_pack.map(|(id, _)| id.to_string()),
                    candidate_reserves,
                ));
                break;
            }
            let Some((
                mut candidate_assignment,
                _candidate_used,
                candidate_anchors,
                candidate_constraints,
                candidate_roles,
                conditional_pack_id,
                candidate_reserves,
            )) = placed
            else {
                continue;
            };

            let candidate_dependencies = compile_active_dependencies(
                &alternative.active_dependencies,
                &candidate_roles,
                &candidate_anchors,
            );
            if seed.auto_rotation {
                let anchors: Vec<_> = out
                    .anchors
                    .iter()
                    .chain(&candidate_anchors)
                    .cloned()
                    .collect();
                let dependencies: Vec<_> = out
                    .active_dependencies
                    .iter()
                    .chain(&candidate_dependencies)
                    .cloned()
                    .collect();
                let Some(claims) = super::auto_support::claim_support_rooms(
                    &candidate_assignment,
                    &anchors,
                    &dependencies,
                    &seed.auto_dorm_support_rooms,
                ) else {
                    continue;
                };
                candidate_assignment.auto_dorm_support_rooms = claims;
            }
            let operators: Vec<String> = candidate_anchors
                .iter()
                .map(|anchor: &SystemAnchor| anchor.operator.clone())
                .collect();
            let slots = candidate_anchors
                .iter()
                .map(|anchor| SlotFill::Fixed {
                    operator: anchor.operator.clone(),
                    elite: anchor.elite,
                    facility: anchor.facility,
                    room_id: anchor.room_id.clone(),
                })
                .collect();
            out.scratch = candidate_assignment;
            used = out.scratch.operator_names();
            excluded.extend(alternative.exclude_operators.iter().cloned());
            let bound_operators = if alternative.bind_roles.is_empty() {
                operators.clone()
            } else {
                selected_operators_for_roles(
                    &alternative.bind_roles,
                    &candidate_roles,
                    &candidate_anchors,
                )
            };
            if (alternative.bind_all || !alternative.bind_roles.is_empty())
                && bound_operators.len() > 1
            {
                out.shift_binds.push(ShiftBind {
                    operators: bound_operators,
                    on_shifts: alternative.on_shifts,
                    off_shifts: alternative.off_shifts,
                });
            }
            for group in &alternative.bind_role_groups {
                let operators =
                    selected_operators_for_roles(group, &candidate_roles, &candidate_anchors);
                if operators.len() > 1 {
                    out.shift_binds.push(ShiftBind {
                        operators,
                        on_shifts: alternative.on_shifts,
                        off_shifts: alternative.off_shifts,
                    });
                }
            }
            out.active_dependencies.extend(candidate_dependencies);
            if alternative.continuous || !alternative.continuous_role_ids.is_empty() {
                let continuous_names: HashSet<_> = selected_operators_for_roles(
                    &alternative.continuous_role_ids,
                    &candidate_roles,
                    &candidate_anchors,
                )
                .into_iter()
                .collect();
                out.continuous_roles
                    .extend(candidate_anchors.iter().filter_map(|anchor| {
                        if !alternative.continuous && !continuous_names.contains(&anchor.operator) {
                            return None;
                        }
                        Some(ContinuousRole {
                            operator: anchor.operator.clone(),
                            facility: anchor.facility,
                            room_id: anchor.room_id.clone()?,
                        })
                    }));
            }
            out.rotation_reserves
                .extend(candidate_reserves.into_iter().map(|reserve| {
                    super::plan::ResolvedRoleReserve {
                        system_id: rule.id.clone(),
                        reserve_id: reserve.reserve_id,
                        role_id: reserve.role_id,
                        facility: FacilityKind::TradePost,
                        operators: reserve.operators,
                        eligible_rooms: reserve.eligible_rooms,
                        reuse_policy: reserve.reuse_policy,
                        require_pre_split_halves: reserve.require_pre_split_halves,
                    }
                }));
            out.anchors.extend(candidate_anchors);
            out.constraints.extend(candidate_constraints);
            out.excluded_operators
                .extend(alternative.exclude_operators.iter().cloned());
            if let Some(group) = &rule.exclusive_group {
                out.claimed_exclusive_groups.insert(group.clone());
            }
            out.selected.push(SelectedRuleAlternative {
                rule_id: rule.id.clone(),
                alternative_id: alternative.id.clone(),
                conditional_pack_id,
                priority: rule.priority,
                operators,
            });
            out.activated.push(ActivatedSystem {
                system_id: rule.id.clone(),
                priority: rule.priority,
                tier: rule.tier,
                slots,
            });
            out.skip_registry_ids
                .extend(rule.skip_registry_ids.iter().cloned());
            selected_alternative = true;
            break;
        }
        if !selected_alternative {
            out.skip_registry_ids
                .extend(rule.skip_registry_ids_when_inactive.iter().cloned());
            out.inactive_control_exclusions.extend(
                rule.alternatives.iter().flat_map(|alternative| {
                    alternative.exclude_control_when_inactive.iter().cloned()
                }),
            );
        }
    }

    Ok(out)
}

fn roles_with_conditional_pack(
    base_roles: &[RoleSpec],
    pack: &ConditionalPackSpec,
) -> Vec<RoleSpec> {
    let replaced: HashSet<&str> = pack.replace_roles.iter().map(String::as_str).collect();
    let mut roles = Vec::with_capacity(base_roles.len() + pack.roles.len());
    let mut inserted = false;
    for role in base_roles {
        if replaced.contains(role.id.as_str()) {
            if !inserted {
                roles.extend(pack.roles.iter().cloned());
                inserted = true;
            }
        } else {
            roles.push(role.clone());
        }
    }
    if !inserted {
        roles.extend(pack.roles.iter().cloned());
    }
    roles
}

fn compile_active_dependencies(
    specs: &[ActiveDependencySpec],
    roles: &[RoleSpec],
    anchors: &[SystemAnchor],
) -> Vec<ActiveDependency> {
    specs
        .iter()
        .filter_map(|spec| {
            let consumers = selected_operators_for_roles(&spec.consumer_roles, roles, anchors);
            let required = selected_operators_for_roles(&spec.required_roles, roles, anchors);
            (!consumers.is_empty() && !required.is_empty()).then_some(ActiveDependency {
                consumers,
                required,
            })
        })
        .collect()
}

fn selected_operators_for_roles(
    role_ids: &[String],
    roles: &[RoleSpec],
    anchors: &[SystemAnchor],
) -> Vec<String> {
    let role_candidates: HashMap<&str, HashSet<&str>> = roles
        .iter()
        .map(|role| {
            (
                role.id.as_str(),
                role.candidates
                    .iter()
                    .map(|candidate| candidate.name.as_str())
                    .collect(),
            )
        })
        .collect();
    let candidates: HashSet<&str> = role_ids
        .iter()
        .filter_map(|role_id| role_candidates.get(role_id.as_str()))
        .flatten()
        .copied()
        .collect();
    anchors
        .iter()
        .filter(|anchor| candidates.contains(anchor.operator.as_str()))
        .map(|anchor| anchor.operator.clone())
        .collect()
}

fn static_gates_match(gates: &[GateSpec], ctx: &RuleCompileContext<'_>) -> bool {
    gates.iter().all(|gate| match gate {
        GateSpec::FacilityCount { facility, min, max } => {
            in_range(ctx.blueprint.count_facility(*facility), *min, *max)
        }
        GateSpec::TradeOrderCount { order, min, max } => {
            let count = ctx
                .blueprint
                .rooms
                .iter()
                .filter(|room| {
                    matches!(room.product, Some(RoomProduct::Trade { order: actual }) if actual == *order)
                })
                .count()
                .min(u8::MAX as usize) as u8;
            in_range(count, *min, *max)
        }
        GateSpec::ManufactureRecipeCount { recipe, min, max } => {
            let count = ctx
                .blueprint
                .rooms
                .iter()
                .filter(|room| {
                    matches!(room.product, Some(RoomProduct::Factory { recipe: actual }) if actual == *recipe)
                })
                .count()
                .min(u8::MAX as usize) as u8;
            in_range(count, *min, *max)
        }
        GateSpec::ResourceAtLeast { .. } | GateSpec::TradeRoleFeasible { .. } => true,
        GateSpec::OperatorAvailable { name, elite } => ctx
            .operbox
            .progress_of(name)
            .is_some_and(|progress| progress.elite >= *elite),
        GateSpec::OperatorUnavailable { name, elite } => ctx
            .operbox
            .progress_of(name).is_none_or(|progress| progress.elite < *elite),
    })
}

fn in_range(value: u8, min: u8, max: Option<u8>) -> bool {
    value >= min && max.is_none_or(|max| value <= max)
}

struct GateReserve {
    reserve_id: String,
    role_id: String,
    operators: Vec<String>,
    eligible_rooms: Vec<crate::layout::blueprint::RoomId>,
    reuse_policy: ReserveReusePolicy,
    require_pre_split_halves: bool,
}

fn dynamic_gates_match(
    gates: &[GateSpec],
    ctx: &RuleCompileContext<'_>,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
) -> Result<Option<Vec<GateReserve>>> {
    if !gates.iter().any(|gate| {
        matches!(
            gate,
            GateSpec::ResourceAtLeast { .. } | GateSpec::TradeRoleFeasible { .. }
        )
    }) {
        return Ok(Some(Vec::new()));
    }
    let resolved = crate::layout::resolve_base(
        ctx.blueprint,
        assignment,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    )?;
    let mut reserves = Vec::new();
    for gate in gates {
        match gate {
            GateSpec::ResourceAtLeast { resource, value } => {
                if !GlobalResourceKey::parse(resource)
                    .is_some_and(|key| resolved.layout.global.get(key) >= *value)
                {
                    return Ok(None);
                }
            }
            GateSpec::TradeRoleFeasible {
                role,
                order,
                all_matching_rooms,
                reserve_as,
                reuse_policy,
                require_pre_split_halves,
            } => {
                let pool = crate::pool::build_trade_pool(
                    &ctx.operbox.trade_roster(ctx.instances),
                    ctx.instances,
                    ctx.table,
                )?;
                let mut matching_rooms: Vec<_> = ctx
                    .blueprint
                    .rooms
                    .iter()
                    .filter(|room| {
                        matches!(
                            room.product,
                            Some(RoomProduct::Trade { order: actual }) if actual == *order
                        )
                    })
                    .collect();
                matching_rooms.sort_by(|left, right| left.id.0.cmp(&right.id.0));
                if matching_rooms.is_empty() {
                    return Ok(None);
                }
                let options_for = |room: &RoomBlueprint| crate::search::TradeSearchOptions {
                    trade_level: room.level,
                    operator_capacity: room.operator_capacity(),
                    mood: ctx.mood,
                    top_k: 20,
                    gold_production_lines: resolved.gold_manu_line_count(),
                    layout: Arc::new(resolved.layout.clone()),
                    shift_hours: ctx.shift_hours,
                    order_mode: crate::trade::input::TradeSearchOrderMode::Single(*order),
                    bake_mode: crate::bake::BakeMode::Disabled,
                    full_pool: true,
                };
                let first_room = matching_rooms[0];
                if first_room.operator_capacity() < 3 {
                    return Ok(None);
                }
                let Ok(first_hit) = crate::search::pick_trade_role_hit(
                    role,
                    &pool,
                    ctx.table,
                    options_for(first_room),
                    &resolved.layout,
                    used,
                    20,
                ) else {
                    return Ok(None);
                };
                let reserved_operators = first_hit.names;
                let reserved_operator_set: HashSet<_> =
                    reserved_operators.iter().cloned().collect();
                let room_feasible = |room: &&RoomBlueprint| {
                    if room.operator_capacity() < 3 {
                        return false;
                    }
                    crate::search::pick_trade_role_hit_requiring(
                        role,
                        &pool,
                        ctx.table,
                        options_for(room),
                        &resolved.layout,
                        used,
                        20,
                        &reserved_operators,
                    )
                    .is_ok_and(|hit| {
                        hit.names.iter().cloned().collect::<HashSet<_>>() == reserved_operator_set
                    })
                };
                let feasible = if *all_matching_rooms {
                    matching_rooms.iter().all(room_feasible)
                } else {
                    matching_rooms.iter().any(room_feasible)
                };
                if !feasible {
                    return Ok(None);
                }
                if let Some(reserve_id) = reserve_as {
                    reserves.push(GateReserve {
                        reserve_id: reserve_id.clone(),
                        role_id: role.clone(),
                        operators: reserved_operators,
                        eligible_rooms: matching_rooms.iter().map(|room| room.id.clone()).collect(),
                        reuse_policy: *reuse_policy,
                        require_pre_split_halves: *require_pre_split_halves,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(Some(reserves))
}

#[allow(clippy::too_many_arguments)]
fn place_roles(
    rule_id: &str,
    roles: &[RoleSpec],
    ctx: &RuleCompileContext<'_>,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    anchors: &mut Vec<SystemAnchor>,
) -> bool {
    let initial = RolePlacementState {
        assignment: assignment.clone(),
        used: used.clone(),
        anchors: anchors.clone(),
        room_groups: HashMap::new(),
        pending_domain_roles: Vec::new(),
    };
    let Some(resolved) = place_role_recursive(
        rule_id,
        roles,
        0,
        ctx,
        existing_constraints,
        new_constraints,
        initial,
    ) else {
        return false;
    };
    *assignment = resolved.assignment;
    *used = resolved.used;
    *anchors = resolved.anchors;
    true
}

#[derive(Clone)]
struct RolePlacementState {
    assignment: BaseAssignment,
    used: HashSet<String>,
    anchors: Vec<SystemAnchor>,
    room_groups: HashMap<String, crate::layout::blueprint::RoomId>,
    /// `(role index, room)`；只在所有声明角色落位后执行 domain completion。
    pending_domain_roles: Vec<(usize, crate::layout::blueprint::RoomId)>,
}

type RolePlacement<'a> = Vec<(
    crate::layout::blueprint::RoomId,
    &'a CandidateSpec,
    crate::roster::OperatorProgress,
)>;

#[allow(clippy::too_many_arguments)]
fn place_role_recursive(
    rule_id: &str,
    roles: &[RoleSpec],
    role_index: usize,
    ctx: &RuleCompileContext<'_>,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
    state: RolePlacementState,
) -> Option<RolePlacementState> {
    if role_index == roles.len() {
        return complete_domain_roles(rule_id, roles, ctx, state);
    }
    let role = &roles[role_index];
    // A constant dorm role can be inherited by the second state. It remains a
    // real Plan anchor and participates in ordinary dependency compilation.
    if state.assignment.auto_rotation
        && role.proactive_support
        && role.facility == FacilityKind::Dormitory
    {
        let inherited = ctx
            .blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::Dormitory)
            .find_map(|room| {
                state
                    .assignment
                    .operators_in(&room.id)
                    .iter()
                    .find(|operator| {
                        role.candidates.iter().any(|candidate| {
                            candidate.name == operator.name && operator.elite >= candidate.elite
                        })
                    })
                    .map(|operator| (room.id.clone(), operator.clone()))
            });
        if let Some((room_id, operator)) = inherited {
            let mut next = state.clone();
            next.anchors.push(SystemAnchor {
                system_id: rule_id.into(),
                operator: operator.name.clone(),
                proactive_support: true,
                optional_trade_role: None,
                elite: operator.elite,
                facility: FacilityKind::Dormitory,
                room_id: Some(room_id),
                recipe: None,
                trade_order: None,
                work_mood: None,
                rest_facility: role.rest_facility,
                fill_policy: AnchorFillPolicy::Plain,
            });
            return place_role_recursive(
                rule_id,
                roles,
                role_index + 1,
                ctx,
                existing_constraints,
                new_constraints,
                next,
            );
        }
    }
    for placement in
        enumerate_role_placements(role, ctx, &state, existing_constraints, new_constraints)
    {
        let mut next = state.clone();
        let work_mood = role.work_mood.filter(|_| {
            role.work_mood_if_operator_present
                .as_ref()
                .is_none_or(|required| next.used.contains(required))
        });
        if let Some(group) = &role.room_group {
            if let Some((room_id, _, _)) = placement.first() {
                next.room_groups
                    .entry(group.clone())
                    .or_insert_with(|| room_id.clone());
            }
        }
        for (room_id, candidate, progress) in placement {
            let mut operators = next.assignment.operators_in(&room_id).to_vec();
            operators.push(
                AssignedOperator::from_progress(&candidate.name, progress)
                    .with_work_mood(work_mood),
            );
            next.assignment.set_room(room_id.clone(), operators);
            next.used.insert(candidate.name.clone());
            next.anchors.push(SystemAnchor {
                system_id: rule_id.to_string(),
                operator: candidate.name.clone(),
                proactive_support: role.proactive_support,
                optional_trade_role: role.optional_trade_recommendation.then(|| role.id.clone()),
                elite: progress.elite,
                facility: ctx
                    .blueprint
                    .room(&room_id)
                    .map(|room| room.kind)
                    .unwrap_or(role.facility),
                room_id: Some(room_id.clone()),
                recipe: role.recipe,
                trade_order: role.trade_order,
                work_mood,
                rest_facility: role.rest_facility,
                fill_policy: match role.facility {
                    FacilityKind::TradePost => AnchorFillPolicy::TradeRole { role_id: None },
                    FacilityKind::Factory => AnchorFillPolicy::ManufactureRecipe,
                    _ => AnchorFillPolicy::Plain,
                },
            });
            if role.fill_to_capacity || role.support_receiver {
                next.pending_domain_roles.push((role_index, room_id));
            }
        }
        if let Some(resolved) = place_role_recursive(
            rule_id,
            roles,
            role_index + 1,
            ctx,
            existing_constraints,
            new_constraints,
            next,
        ) {
            return Some(resolved);
        }
    }
    None
}

fn enumerate_role_placements<'a>(
    role: &'a RoleSpec,
    ctx: &RuleCompileContext<'_>,
    state: &RolePlacementState,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
) -> Vec<RolePlacement<'a>> {
    let mut available: Vec<_> = role
        .candidates
        .iter()
        .filter_map(|candidate| {
            let progress = ctx.operbox.progress_of(&candidate.name)?;
            (!state.used.contains(&candidate.name)
                && progress.elite >= candidate.elite
                && std::iter::once(role.facility)
                    .chain(role.facilities.iter().copied())
                    .any(|facility| ctx.operbox.admits_facility(&candidate.name, facility)))
            .then_some((candidate, progress))
        })
        .collect();
    if matches!(role.selection, RoleSelection::ManufactureObjective) {
        available = manufacture_objective_candidates(
            role,
            ctx,
            state,
            existing_constraints,
            new_constraints,
            available,
        );
    }
    if available
        .iter()
        .any(|(candidate, _)| !candidate.fallback_if_no_other_candidates)
    {
        available.retain(|(candidate, _)| !candidate.fallback_if_no_other_candidates);
    }
    match role.selection {
        RoleSelection::Ordered => available.truncate(role.max_count as usize),
        RoleSelection::ManufactureObjective => available.truncate(role.max_count as usize),
        RoleSelection::AllAvailable if available.len() > role.max_count as usize => {
            return Vec::new();
        }
        RoleSelection::AllAvailable => {}
    }
    if available.len() < role.min_count as usize {
        return Vec::new();
    }
    let exact_count =
        matches!(role.selection, RoleSelection::AllAvailable).then_some(available.len());
    let rooms = matching_rooms(role, ctx, state);
    if role.same_room || role.room_group.is_some() {
        let mut placements: Vec<_> = rooms
            .into_iter()
            .filter_map(|room| {
                let available_slots = role_room_capacity(room)
                    .saturating_sub(state.assignment.operators_in(&room.id).len());
                let selected_count = available
                    .len()
                    .min(role.max_count as usize)
                    .min(available_slots);
                if selected_count < role.min_count as usize
                    || exact_count.is_some_and(|count| selected_count != count)
                {
                    return None;
                }
                let placement: RolePlacement<'a> = available[..selected_count]
                    .iter()
                    .map(|(candidate, progress)| (room.id.clone(), *candidate, *progress))
                    .collect();
                placement_is_valid(
                    role,
                    &placement,
                    &state.assignment,
                    ctx,
                    &state.used,
                    existing_constraints,
                    new_constraints,
                )
                .then_some(placement)
            })
            .collect();
        if placements.is_empty() && role.min_count == 0 {
            return vec![Vec::new()];
        }
        append_optional_trade_omission(role, &mut placements);
        return placements;
    }

    if role.fill_to_capacity || role.support_receiver {
        let max_count = available.len().min(role.max_count as usize);
        let min_count = exact_count.unwrap_or(role.min_count as usize);
        let mut placements = Vec::new();
        for count in (min_count..=max_count).rev() {
            let mut current = Vec::new();
            enumerate_distributed_placements(
                role,
                &available[..count],
                0,
                &rooms,
                &state.assignment,
                ctx,
                &state.used,
                existing_constraints,
                new_constraints,
                &mut current,
                &mut placements,
            );
            if !placements.is_empty() {
                break;
            }
        }
        append_optional_trade_omission(role, &mut placements);
        return placements;
    }

    let mut placement = Vec::new();
    let mut trial = state.assignment.clone();
    for (candidate, progress) in available {
        let Some(room) = rooms.iter().copied().find(|room| {
            ctx.operbox.admits_facility(&candidate.name, room.kind)
                && trial.operators_in(&room.id).len()
                    < role_placement_capacity(
                        role,
                        room,
                        ctx,
                        &trial,
                        &state.used,
                        &candidate.name,
                        existing_constraints,
                        new_constraints,
                    )
                && (!role.one_per_room
                    || if role.shared_room_facilities.contains(&room.kind) {
                        !placement.iter().any(|(id, _, _)| id == &room.id)
                    } else {
                        trial.operators_in(&room.id).is_empty()
                    })
                && room_allows_operator(
                    &candidate.name,
                    room,
                    &trial,
                    existing_constraints,
                    new_constraints,
                )
        }) else {
            break;
        };
        placement.push((room.id.clone(), candidate, progress));
        let mut operators = trial.operators_in(&room.id).to_vec();
        operators.push(AssignedOperator::from_progress(&candidate.name, progress));
        trial.set_room(room.id.clone(), operators);
        if placement.len() >= role.max_count as usize {
            break;
        }
    }
    let mut placements = if placement.len() < role.min_count as usize
        || exact_count.is_some_and(|count| placement.len() != count)
    {
        Vec::new()
    } else {
        vec![placement]
    };
    append_optional_trade_omission(role, &mut placements);
    placements
}

fn append_optional_trade_omission<'a>(role: &RoleSpec, placements: &mut Vec<RolePlacement<'a>>) {
    if role.optional_trade_recommendation
        && role.min_count == 0
        && !placements.iter().any(Vec::is_empty)
    {
        placements.push(Vec::new());
    }
}

fn manufacture_objective_candidates<'a>(
    role: &'a RoleSpec,
    ctx: &RuleCompileContext<'_>,
    state: &RolePlacementState,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
    available: Vec<(&'a CandidateSpec, crate::roster::OperatorProgress)>,
) -> Vec<(&'a CandidateSpec, crate::roster::OperatorProgress)> {
    let Some(room) = matching_rooms(role, ctx, state).into_iter().next() else {
        return Vec::new();
    };
    let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
        return Vec::new();
    };
    let required: Vec<_> = state
        .assignment
        .operators_in(&room.id)
        .iter()
        .map(|operator| operator.name.clone())
        .collect();
    if required.len() >= room.operator_capacity() {
        return Vec::new();
    }
    let Ok(resolved) = crate::layout::resolve_base(
        ctx.blueprint,
        &state.assignment,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    ) else {
        return Vec::new();
    };
    let layout = Arc::new(resolved.layout);
    let eligible: Vec<_> = available
        .into_iter()
        .filter(|(candidate, progress)| {
            let placement = vec![(room.id.clone(), *candidate, *progress)];
            placement_is_valid(
                role,
                &placement,
                &state.assignment,
                ctx,
                &state.used,
                existing_constraints,
                new_constraints,
            )
        })
        .collect();
    let mut ranked: Vec<_> = eligible
        .into_iter()
        .filter_map(|candidate| {
            let operators: Option<Vec<_>> = required
                .iter()
                .chain(std::iter::once(&candidate.0.name))
                .map(|name| {
                    let progress = ctx.operbox.progress_of(name)?;
                    let tier = crate::tier::PromotionTier::from_progress(progress);
                    let buff_ids = ctx.instances.resolve_manufacture_buff_ids(name, tier);
                    (!buff_ids.is_empty()).then(|| crate::manufacture::ManuOperator {
                        compiled_atoms: Some(ctx.table.compile_manufacture_atoms(&buff_ids)),
                        name: name.clone(),
                        elite: progress.elite,
                        buff_ids,
                        tags: ctx.instances.tags_for(name, tier),
                    })
                })
                .collect();
            let input = crate::manufacture::ManuRoomInput {
                level: room.level,
                operators: operators?,
                active_recipe: *recipe,
                mood: ctx.mood,
                layout: Arc::clone(&layout),
            };
            crate::search::evaluate_manufacture_room(&input, ctx.table, *recipe, ctx.shift_hours)
                .map(|hit| (candidate, hit))
        })
        .collect();
    ranked.sort_by(|left, right| crate::search::compare_manufacture_hits(&left.1, &right.1));
    ranked.into_iter().map(|(candidate, _)| candidate).collect()
}

fn matching_rooms<'a>(
    role: &RoleSpec,
    ctx: &'a RuleCompileContext<'_>,
    state: &RolePlacementState,
) -> Vec<&'a RoomBlueprint> {
    let grouped_room = role
        .room_group
        .as_ref()
        .and_then(|group| state.room_groups.get(group));
    let mut rooms: Vec<_> = ctx
        .blueprint
        .rooms
        .iter()
        .filter(|room| {
            grouped_room.is_none_or(|required| &room.id == required)
                && role_matches_room(role, room)
        })
        .collect();
    rooms.sort_by(|left, right| {
        (left.kind != role.facility)
            .cmp(&(right.kind != role.facility))
            .then_with(|| match role.room_level_preference {
                RoomLevelPreference::BlueprintOrder => left.id.0.cmp(&right.id.0),
                RoomLevelPreference::Level2First => (right.level == 2)
                    .cmp(&(left.level == 2))
                    .then_with(|| left.id.0.cmp(&right.id.0)),
                RoomLevelPreference::LowestLevel => left
                    .level
                    .cmp(&right.level)
                    .then_with(|| left.id.0.cmp(&right.id.0)),
                RoomLevelPreference::HighestLevel => right
                    .level
                    .cmp(&left.level)
                    .then_with(|| left.id.0.cmp(&right.id.0)),
            })
    });
    rooms
}

#[allow(clippy::too_many_arguments)]
fn enumerate_distributed_placements<'a>(
    role: &RoleSpec,
    candidates: &[(&'a CandidateSpec, crate::roster::OperatorProgress)],
    candidate_index: usize,
    rooms: &[&RoomBlueprint],
    assignment: &BaseAssignment,
    ctx: &RuleCompileContext<'_>,
    used: &HashSet<String>,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
    current: &mut RolePlacement<'a>,
    out: &mut Vec<RolePlacement<'a>>,
) {
    if candidate_index == candidates.len() {
        out.push(current.clone());
        return;
    }
    let (candidate, progress) = candidates[candidate_index];
    for room in rooms {
        let pending = current
            .iter()
            .filter(|(room_id, _, _)| room_id == &room.id)
            .count();
        if assignment.operators_in(&room.id).len() + pending
            >= role_placement_capacity(
                role,
                room,
                ctx,
                assignment,
                used,
                &candidate.name,
                existing_constraints,
                new_constraints,
            )
            || (role.one_per_room
                && (pending > 0
                    || (!role.shared_room_facilities.contains(&room.kind)
                        && !assignment.operators_in(&room.id).is_empty())))
        {
            continue;
        }
        current.push((room.id.clone(), candidate, progress));
        if placement_is_valid(
            role,
            current,
            assignment,
            ctx,
            used,
            existing_constraints,
            new_constraints,
        ) {
            enumerate_distributed_placements(
                role,
                candidates,
                candidate_index + 1,
                rooms,
                assignment,
                ctx,
                used,
                existing_constraints,
                new_constraints,
                current,
                out,
            );
        }
        current.pop();
    }
}

fn placement_is_valid(
    role: &RoleSpec,
    placement: &RolePlacement<'_>,
    assignment: &BaseAssignment,
    ctx: &RuleCompileContext<'_>,
    used: &HashSet<String>,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
) -> bool {
    let mut trial = assignment.clone();
    let mut role_rooms = HashSet::new();
    for (room_id, candidate, progress) in placement {
        let Some(room) = ctx.blueprint.room(room_id) else {
            return false;
        };
        if !ctx.operbox.admits_facility(&candidate.name, room.kind)
            || trial.operators_in(room_id).len()
                >= role_placement_capacity(
                    role,
                    room,
                    ctx,
                    &trial,
                    used,
                    &candidate.name,
                    existing_constraints,
                    new_constraints,
                )
            || (role.one_per_room
                && (!role_rooms.insert(room_id)
                    || (!role.shared_room_facilities.contains(&room.kind)
                        && !assignment.operators_in(room_id).is_empty())))
            || !room_allows_operator(
                &candidate.name,
                room,
                &trial,
                existing_constraints,
                new_constraints,
            )
        {
            return false;
        }
        let mut operators = trial.operators_in(room_id).to_vec();
        operators.push(AssignedOperator::from_progress(&candidate.name, *progress));
        trial.set_room(room_id.clone(), operators);
    }
    true
}

fn complete_domain_roles(
    rule_id: &str,
    roles: &[RoleSpec],
    ctx: &RuleCompileContext<'_>,
    mut state: RolePlacementState,
) -> Option<RolePlacementState> {
    state.pending_domain_roles.sort_by(|left, right| {
        left.1
             .0
            .cmp(&right.1 .0)
            .then_with(|| roles[left.0].id.cmp(&roles[right.0].id))
    });
    state
        .pending_domain_roles
        .dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    for (role_index, room_id) in state.pending_domain_roles.clone() {
        if !complete_domain_role(
            rule_id,
            &roles[role_index],
            &room_id,
            ctx,
            &mut state.assignment,
            &mut state.used,
            &mut state.anchors,
            roles[role_index].support_receiver,
        ) {
            return None;
        }
    }
    Some(state)
}

#[allow(clippy::too_many_arguments)]
fn complete_domain_role(
    rule_id: &str,
    role: &RoleSpec,
    room_id: &crate::layout::blueprint::RoomId,
    ctx: &RuleCompileContext<'_>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    anchors: &mut Vec<SystemAnchor>,
    optional_receiver: bool,
) -> bool {
    let Some(room) = ctx.blueprint.room(room_id) else {
        return false;
    };
    let required: Vec<_> = assignment
        .operators_in(room_id)
        .iter()
        .map(|op| op.name.clone())
        .collect();
    let selected = match room.kind {
        FacilityKind::TradePost
            if role.fill_to_capacity || optional_receiver || assignment.auto_rotation =>
        {
            if !complete_trade_role(
                rule_id,
                room,
                ctx,
                assignment,
                used,
                anchors,
                optional_receiver,
            ) {
                return false;
            }
            assignment
                .operators_in(room_id)
                .iter()
                .map(|op| op.name.clone())
                .collect()
        }
        FacilityKind::Factory => {
            if !complete_manufacture_role(
                rule_id,
                role,
                room,
                ctx,
                assignment,
                used,
                anchors,
                optional_receiver,
            ) {
                return false;
            }
            assignment
                .operators_in(room_id)
                .iter()
                .map(|op| op.name.clone())
                .collect()
        }
        FacilityKind::PowerPlant if optional_receiver => {
            let Some(selected) = select_power_role(ctx, room, assignment, used) else {
                return false;
            };
            selected
        }
        _ => return !optional_receiver,
    };
    required.iter().all(|name| selected.contains(name))
}

#[allow(clippy::too_many_arguments)]
fn complete_manufacture_role(
    rule_id: &str,
    role: &RoleSpec,
    room: &RoomBlueprint,
    ctx: &RuleCompileContext<'_>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    anchors: &mut Vec<SystemAnchor>,
    optional_receiver: bool,
) -> bool {
    let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
        return false;
    };
    let recipe = *recipe;
    let required: Vec<String> = assignment
        .operators_in(&room.id)
        .iter()
        .map(|operator| operator.name.clone())
        .collect();
    if required.len() != 1 {
        return false;
    }
    let Ok(pool) = crate::pool::build_manufacture_pool(
        &ctx.operbox.manufacture_roster(ctx.instances),
        ctx.instances,
        ctx.table,
    ) else {
        return false;
    };

    let mut candidate_excluded = used.clone();
    for name in &required {
        candidate_excluded.remove(name);
    }
    let candidate_pool = crate::pool::filter_manufacture_pool(&pool, &candidate_excluded);
    // 挂件是临时可兑现的环境，受益者必须由普通完整组合搜索选出。
    // 清掉本房预选成员，避免未入选的受益者残留在基内人数等结算事实中。
    let mut search_assignment = assignment.clone();
    if optional_receiver {
        search_assignment.set_room(room.id.clone(), Vec::new());
    }
    let Ok(candidate_resolved) = crate::layout::resolve_base(
        ctx.blueprint,
        &search_assignment,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    ) else {
        return false;
    };
    let candidate_options = crate::search::ManuSearchOptions {
        level: room.level,
        operator_capacity: room.operator_capacity(),
        top_k: 20,
        mood: ctx.mood,
        shift_hours: Some(ctx.shift_hours),
        layout: Arc::new(candidate_resolved.layout),
        recipe_mode: crate::manufacture::ManuSearchRecipeMode::Single(recipe),
        must_include_name: (!optional_receiver).then(|| required[0].clone()),
        full_pool: true,
        bake_mode: crate::bake::BakeMode::Disabled,
    };
    let Ok(candidate_report) =
        crate::search::search_manufacture_triples(&candidate_pool, ctx.table, &candidate_options)
    else {
        return false;
    };
    let candidate = candidate_report.best;
    if candidate.names.len() != room.operator_capacity() {
        return false;
    }

    let mut operators = Vec::new();
    for name in &candidate.names {
        let Some(progress) = ctx.operbox.progress_of(name) else {
            return false;
        };
        operators.push(AssignedOperator::from_progress(name, progress));
    }

    for (name, operator) in candidate.names.iter().zip(&operators) {
        used.insert(name.clone());
        if !anchors
            .iter()
            .any(|anchor| anchor.operator == *name && anchor.room_id.as_ref() == Some(&room.id))
        {
            anchors.push(SystemAnchor {
                system_id: rule_id.to_string(),
                operator: name.clone(),
                proactive_support: role.proactive_support,
                optional_trade_role: None,
                elite: operator.elite,
                facility: FacilityKind::Factory,
                room_id: Some(room.id.clone()),
                recipe: Some(recipe),
                trade_order: None,
                work_mood: role.work_mood,
                rest_facility: role.rest_facility,
                fill_policy: AnchorFillPolicy::ManufactureRecipe,
            });
        }
    }
    assignment.set_room(room.id.clone(), operators);
    true
}

fn select_power_role(
    ctx: &RuleCompileContext<'_>,
    room: &RoomBlueprint,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
) -> Option<Vec<String>> {
    let required = assignment.operators_in(&room.id);
    if required.len() != 1 {
        return None;
    }
    let Ok(pool) =
        crate::pool::build_power_pool_with_fillers(ctx.operbox, ctx.instances, ctx.table)
    else {
        return None;
    };
    let mut excluded = used.clone();
    excluded.remove(&required[0].name);
    let pool = crate::layout::assign::filter_power_pool(&pool, &excluded);
    let mut search_assignment = assignment.clone();
    search_assignment.set_room(room.id.clone(), Vec::new());
    let Ok(resolved) = crate::layout::resolve_base(
        ctx.blueprint,
        &search_assignment,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    ) else {
        return None;
    };
    let report = crate::search::search_power_assignment(
        &pool,
        ctx.table,
        &crate::search::PowerSearchOptions {
            station_count: 1,
            mood: ctx.mood,
            shift_hours: ctx.shift_hours,
            layout: resolved.layout,
        },
    )
    .ok()?;
    Some(vec![report.assignments.first()?.hit.name.clone()])
}

fn role_matches_room(role: &RoleSpec, room: &RoomBlueprint) -> bool {
    if room.level < role.min_room_level
        || (room.kind != role.facility && !role.facilities.contains(&room.kind))
    {
        return false;
    }
    if role.recipe.is_some_and(|required| {
        !matches!(room.product, Some(RoomProduct::Factory { recipe }) if recipe == required)
    }) {
        return false;
    }
    if role.trade_order.is_some_and(
        |required| !matches!(room.product, Some(RoomProduct::Trade { order }) if order == required),
    ) {
        return false;
    }
    true
}

/// 主动支持只使用回收技能岗位之外的真实空位；技能干员仍由设施填房 owner 选入。
fn role_placement_capacity(
    role: &RoleSpec,
    room: &RoomBlueprint,
    ctx: &RuleCompileContext<'_>,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    proposed: &str,
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
) -> usize {
    let capacity = role_room_capacity(room);
    if !role.proactive_support || room.kind != FacilityKind::Recycle {
        return capacity;
    }
    let occupied = assignment.occupied_operator_names();
    let reserved = ctx
        .operbox
        .owned
        .iter()
        .filter(|(name, progress)| {
            name.as_str() != proposed
                && !used.contains(*name)
                && !occupied.contains(*name)
                && room_allows_operator(
                    name,
                    room,
                    assignment,
                    existing_constraints,
                    new_constraints,
                )
                && !ctx
                    .instances
                    .resolve_recycle_buff_ids(
                        name,
                        crate::tier::PromotionTier::from_progress(**progress),
                    )
                    .is_empty()
        })
        .count();
    capacity.saturating_sub(reserved)
}

fn role_room_capacity(room: &RoomBlueprint) -> usize {
    if room.kind == FacilityKind::Dormitory {
        room.dorm_bed_count() as usize
    } else {
        room.operator_capacity()
    }
}

fn room_allows_operator(
    operator: &str,
    room: &RoomBlueprint,
    assignment: &BaseAssignment,
    existing: &[SystemConstraint],
    new: &[SystemConstraint],
) -> bool {
    existing
        .iter()
        .chain(new)
        .filter_map(|constraint| match constraint {
            SystemConstraint::ForbidSameRoom { a, b }
            | SystemConstraint::ForbidSameStation { a, b } => Some((a.as_str(), b.as_str())),
            SystemConstraint::RequireFacility { .. } => None,
        })
        .all(|(a, b)| {
            let peer = if operator == a {
                Some(b)
            } else if operator == b {
                Some(a)
            } else {
                None
            };
            peer.is_none_or(|peer| {
                !assignment
                    .operators_in(&room.id)
                    .iter()
                    .any(|assigned| assigned.name == peer)
            })
        })
}

fn compile_relations(relations: &[RelationSpec]) -> Vec<SystemConstraint> {
    relations
        .iter()
        .map(|relation| match relation {
            RelationSpec::ForbidSameRoom { a, b } => SystemConstraint::ForbidSameRoom {
                a: a.clone(),
                b: b.clone(),
            },
            RelationSpec::ForbidSameStation { a, b } => SystemConstraint::ForbidSameStation {
                a: a.clone(),
                b: b.clone(),
            },
        })
        .collect()
}

/// Preserve optional beneficiary provenance before removing proactive placement roles.
/// Hard cores without an explicit support dependency remain mandatory.
pub(super) fn configure_support_admission(catalog: &mut RuleCatalog, enabled: bool) {
    for rule in &mut catalog.rules {
        for alternative in &mut rule.alternatives {
            let support_roles: HashSet<_> = alternative
                .roles
                .iter()
                .chain(
                    alternative
                        .conditional_packs
                        .iter()
                        .flat_map(|pack| &pack.roles),
                )
                .filter(|role| role.proactive_support)
                .map(|role| role.id.clone())
                .collect();
            let receivers: HashSet<_> = alternative
                .active_dependencies
                .iter()
                .filter(|dependency| {
                    dependency
                        .required_roles
                        .iter()
                        .any(|id| support_roles.contains(id))
                })
                .flat_map(|dependency| dependency.consumer_roles.iter().cloned())
                .collect();
            for role in alternative.roles.iter_mut().chain(
                alternative
                    .conditional_packs
                    .iter_mut()
                    .flat_map(|pack| &mut pack.roles),
            ) {
                role.support_receiver = !role.proactive_support && receivers.contains(&role.id);
            }
            if !enabled {
                alternative.roles.retain(|role| !role.proactive_support);
                for pack in &mut alternative.conditional_packs {
                    pack.roles.retain(|role| !role.proactive_support);
                }
            }
        }
    }
}

/// Product capability restriction happens before role placement. Tier alone is
/// insufficient: it also labels legal single-station and building-count rules.
pub(super) fn restrict_auto_rotation_catalog(catalog: &mut RuleCatalog) {
    for rule in &mut catalog.rules {
        rule.alternatives
            .retain(|alternative| match rule.id.as_str() {
                "trade_core" => true,
                _ if rule.auto_only => true,
                "automation_group" => !alternative
                    .roles
                    .iter()
                    .any(|role| role.facility == FacilityKind::ControlCenter),
                "rhine_receivers" => {
                    alternative
                        .roles
                        .iter()
                        .filter(|role| !role.proactive_support)
                        .count()
                        == 1
                }
                _ => alternative
                    .roles
                    .iter()
                    .all(|role| role.facility == FacilityKind::ControlCenter),
            });
        if rule.id == "rhine_receivers" {
            for alternative in &mut rule.alternatives {
                for role in &mut alternative.roles {
                    if role.proactive_support {
                        role.min_count = 1;
                        role.max_count = 1;
                        role.facilities.clear();
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_trade_role(
    rule_id: &str,
    room: &RoomBlueprint,
    ctx: &RuleCompileContext<'_>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    anchors: &mut Vec<SystemAnchor>,
    optional_receiver: bool,
) -> bool {
    let Some(RoomProduct::Trade { order }) = room.product else {
        return false;
    };
    let required: Vec<_> = assignment
        .operators_in(&room.id)
        .iter()
        .map(|op| op.name.clone())
        .collect();
    let Ok(pool) = crate::pool::build_trade_pool(
        &ctx.operbox.trade_roster(ctx.instances),
        ctx.instances,
        ctx.table,
    ) else {
        return false;
    };
    let mut search_assignment = assignment.clone();
    let mut search_used = used.clone();
    if optional_receiver {
        search_assignment.set_room(room.id.clone(), Vec::new());
        for name in &required {
            search_used.remove(name);
        }
    }
    let Ok(layout) = crate::layout::resolve_base(
        ctx.blueprint,
        &search_assignment,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    ) else {
        return false;
    };
    let options = crate::layout::AssignBaseOptions {
        auto_rotation: assignment.auto_rotation,
        mood: ctx.mood,
        shift_hours: ctx.shift_hours,
        ..Default::default()
    };
    let Ok(hit) = crate::layout::assign::trade_fill::pick_trade_meta_then_plain(
        &pool,
        ctx.table,
        &layout.layout,
        ctx.blueprint.gold_manu_line_count(),
        &options,
        order,
        room.level,
        &mut search_used,
        if optional_receiver { &[] } else { &required },
        optional_receiver,
    ) else {
        return false;
    };
    let mut operators = Vec::new();
    for name in &hit.names {
        let Some(progress) = ctx.operbox.progress_of(name) else {
            return false;
        };
        operators.push(AssignedOperator::from_progress(name, progress));
        used.insert(name.clone());
        if required.contains(name) {
            continue;
        }
        anchors.push(SystemAnchor {
            system_id: rule_id.into(),
            operator: name.clone(),
            elite: progress.elite,
            proactive_support: false,
            optional_trade_role: None,
            facility: FacilityKind::TradePost,
            room_id: Some(room.id.clone()),
            recipe: None,
            trade_order: Some(order),
            work_mood: None,
            rest_facility: None,
            fill_policy: AnchorFillPolicy::Plain,
        });
    }
    assignment.set_room(room.id.clone(), operators);
    true
}
