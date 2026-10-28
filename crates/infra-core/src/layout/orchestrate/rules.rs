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
    /// 与同一房间的普通最优候选比较，未严格胜出则本 alternative 不成立。
    #[serde(default)]
    pub competitive: bool,
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

pub(crate) fn assert_support_combination_policy(
    plans: &[super::plan::AssignmentPlan],
    shifts: &[crate::schedule::TeamShiftResult],
) -> Result<()> {
    for shift in shifts {
        let plan = plans.get(shift.plan_index).ok_or_else(|| {
            Error::msg("assert-invariants support_combination_policy failed: missing Plan")
        })?;
        if plan.support_combinations_enable {
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
            let alternative_baseline = out.scratch.clone();
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
                    &alternative_baseline,
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
                candidate_assignment,
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
            for dependency in compile_active_dependencies(
                &alternative.active_dependencies,
                &candidate_roles,
                &candidate_anchors,
            ) {
                out.active_dependencies.push(dependency);
            }
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
                    shift_hours: 24.0,
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
    alternative_baseline: &BaseAssignment,
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
        alternative_baseline,
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
    alternative_baseline: &BaseAssignment,
) -> Option<RolePlacementState> {
    if role_index == roles.len() {
        return complete_domain_roles(rule_id, roles, ctx, state, alternative_baseline);
    }
    let role = &roles[role_index];
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
                optional_control_role: (role.min_count == 0
                    && role.facility == FacilityKind::ControlCenter)
                    .then(|| role.id.clone()),
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
            if role.fill_to_capacity || role.competitive {
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
            alternative_baseline,
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
            (!state.used.contains(&candidate.name) && progress.elite >= candidate.elite)
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

    if role.fill_to_capacity || role.competitive {
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
            trial.operators_in(&room.id).len() < role_room_capacity(room)
                && (!role.one_per_room || trial.operators_in(&room.id).is_empty())
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
            crate::search::evaluate_manufacture_room(&input, ctx.table, *recipe)
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
        if assignment.operators_in(&room.id).len() + pending >= role_room_capacity(room)
            || (role.one_per_room && (!assignment.operators_in(&room.id).is_empty() || pending > 0))
        {
            continue;
        }
        current.push((room.id.clone(), candidate, progress));
        if placement_is_valid(
            role,
            current,
            assignment,
            ctx,
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
    existing_constraints: &[SystemConstraint],
    new_constraints: &[SystemConstraint],
) -> bool {
    let mut trial = assignment.clone();
    for (room_id, candidate, progress) in placement {
        let Some(room) = ctx.blueprint.room(room_id) else {
            return false;
        };
        if trial.operators_in(room_id).len() >= role_room_capacity(room)
            || (role.one_per_room && !trial.operators_in(room_id).is_empty())
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
    alternative_baseline: &BaseAssignment,
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
        if !complete_and_compare_domain_role(
            rule_id,
            &roles[role_index],
            &room_id,
            ctx,
            &mut state.assignment,
            &mut state.used,
            &mut state.anchors,
            alternative_baseline,
        ) {
            return None;
        }
    }
    Some(state)
}

#[allow(clippy::too_many_arguments)]
fn complete_and_compare_domain_role(
    rule_id: &str,
    role: &RoleSpec,
    room_id: &crate::layout::blueprint::RoomId,
    ctx: &RuleCompileContext<'_>,
    assignment: &mut BaseAssignment,
    used: &mut HashSet<String>,
    anchors: &mut Vec<SystemAnchor>,
    alternative_baseline: &BaseAssignment,
) -> bool {
    let Some(room) = ctx.blueprint.room(room_id) else {
        return false;
    };
    match room.kind {
        FacilityKind::Factory => complete_manufacture_role(
            rule_id,
            role,
            room,
            ctx,
            assignment,
            used,
            anchors,
            alternative_baseline,
        ),
        FacilityKind::PowerPlant if role.competitive => {
            power_role_beats_unplanned(ctx, assignment, used, alternative_baseline)
        }
        _ => !role.competitive,
    }
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
    alternative_baseline: &BaseAssignment,
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
    let Ok(candidate_resolved) = crate::layout::resolve_base(
        ctx.blueprint,
        assignment,
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
        shift_hours: None,
        layout: Arc::new(candidate_resolved.layout),
        recipe_mode: crate::manufacture::ManuSearchRecipeMode::Single(recipe),
        must_include_name: Some(required[0].clone()),
        full_pool: true,
        bake_mode: crate::bake::BakeMode::Disabled,
    };
    let Ok(candidate_report) =
        crate::search::search_manufacture_triples(&candidate_pool, ctx.table, &candidate_options)
    else {
        return false;
    };
    let candidate = candidate_report.best;

    let mut operators = Vec::new();
    let mut candidate_used = used.clone();
    for name in &candidate.names {
        let Some(progress) = ctx.operbox.progress_of(name) else {
            return false;
        };
        operators.push(AssignedOperator::from_progress(name, progress));
        candidate_used.insert(name.clone());
    }
    let mut candidate_assignment = assignment.clone();
    candidate_assignment.set_room(room.id.clone(), operators.clone());
    if role.competitive {
        let baseline_used = alternative_baseline.operator_names();
        let Some(candidate_manu) =
            score_completed_manufacture(ctx, &candidate_assignment, &candidate_used)
        else {
            return false;
        };
        let Some(baseline_manu) =
            score_completed_manufacture(ctx, alternative_baseline, &baseline_used)
        else {
            return false;
        };
        let Some(candidate_power) =
            score_completed_power(ctx, &candidate_assignment, &candidate_used)
        else {
            return false;
        };
        let Some(baseline_power) = score_completed_power(ctx, alternative_baseline, &baseline_used)
        else {
            return false;
        };
        if candidate_manu <= baseline_manu || candidate_power < baseline_power {
            return false;
        }
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
                optional_control_role: None,
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

fn power_role_beats_unplanned(
    ctx: &RuleCompileContext<'_>,
    assignment: &BaseAssignment,
    used: &HashSet<String>,
    alternative_baseline: &BaseAssignment,
) -> bool {
    let baseline_used = alternative_baseline.operator_names();
    let Some(candidate_power) = score_completed_power(ctx, assignment, used) else {
        return false;
    };
    let Some(baseline_power) = score_completed_power(ctx, alternative_baseline, &baseline_used)
    else {
        return false;
    };
    // 最终判定要求 candidate_power > baseline_power。发电分不占优时结果必为 false，
    // 逐房制造评分是纯函数且无副作用，跳过它不改变结果，只省掉两次全组合制造搜索。
    if candidate_power <= baseline_power {
        return false;
    }
    let Some(candidate_manu) = score_completed_manufacture(ctx, assignment, used) else {
        return false;
    };
    let Some(baseline_manu) =
        score_completed_manufacture(ctx, alternative_baseline, &baseline_used)
    else {
        return false;
    };
    candidate_power > baseline_power && candidate_manu >= baseline_manu
}

fn score_completed_manufacture(
    ctx: &RuleCompileContext<'_>,
    assignment: &BaseAssignment,
    initially_used: &HashSet<String>,
) -> Option<crate::Efficiency> {
    let pool = crate::pool::build_manufacture_pool(
        &ctx.operbox.manufacture_roster(ctx.instances),
        ctx.instances,
        ctx.table,
    )
    .ok()?;
    let mut scratch = assignment.clone();
    let mut used = initially_used.clone();
    let mut rooms: Vec<_> = ctx
        .blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::Factory)
        .collect();
    rooms.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    for room in rooms {
        if !scratch.operators_in(&room.id).is_empty() {
            continue;
        }
        let Some(RoomProduct::Factory { recipe }) = room.product.as_ref() else {
            continue;
        };
        let resolved = crate::layout::resolve_base(
            ctx.blueprint,
            &scratch,
            Some(ctx.instances),
            Some(ctx.table),
            ctx.mood,
            None,
        )
        .ok()?;
        let sub = crate::pool::filter_manufacture_pool(&pool, &used);
        let options = crate::search::ManuSearchOptions {
            level: room.level,
            operator_capacity: room.operator_capacity(),
            top_k: 20,
            mood: ctx.mood,
            shift_hours: None,
            layout: Arc::new(resolved.layout),
            recipe_mode: crate::manufacture::ManuSearchRecipeMode::Single(*recipe),
            must_include_name: None,
            full_pool: true,
            bake_mode: crate::bake::BakeMode::Disabled,
        };
        let Ok(report) = crate::search::search_manufacture_triples(&sub, ctx.table, &options)
        else {
            continue;
        };
        let operators: Vec<_> = report
            .best
            .names
            .iter()
            .filter_map(|name| {
                ctx.operbox
                    .progress_of(name)
                    .map(|progress| AssignedOperator::from_progress(name, progress))
            })
            .collect();
        if operators.len() != report.best.names.len() {
            return None;
        }
        used.extend(report.best.names);
        scratch.set_room(room.id.clone(), operators);
    }
    let resolved = crate::layout::resolve_base(
        ctx.blueprint,
        &scratch,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    )
    .ok()?;
    resolved
        .manu_rooms
        .iter()
        .filter(|room| !room.operators.is_empty())
        .map(|room| {
            crate::manufacture::solve_manufacture(
                &crate::manufacture::ManuRoomInput {
                    level: room.level,
                    operators: room.operators.clone(),
                    active_recipe: room.recipe,
                    mood: ctx.mood,
                    layout: Arc::new(room.layout.clone()),
                },
                ctx.table,
            )
            .map(|result| result.final_efficiency)
            .ok()
        })
        .collect::<Option<Vec<_>>>()
        .map(|scores| scores.into_iter().sum())
}

fn score_completed_power(
    ctx: &RuleCompileContext<'_>,
    assignment: &BaseAssignment,
    initially_used: &HashSet<String>,
) -> Option<crate::Efficiency> {
    let pool = crate::pool::build_power_pool(
        &ctx.operbox.power_roster(ctx.instances),
        ctx.instances,
        ctx.table,
    )
    .ok()?;
    let mut scratch = assignment.clone();
    let mut used = initially_used.clone();
    let mut rooms: Vec<_> = ctx
        .blueprint
        .rooms
        .iter()
        .filter(|room| room.kind == FacilityKind::PowerPlant)
        .collect();
    rooms.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    for room in rooms {
        if !scratch.operators_in(&room.id).is_empty() {
            continue;
        }
        let mut best: Option<(crate::Efficiency, AssignedOperator)> = None;
        for entry in pool
            .entries
            .iter()
            .filter(|entry| !used.contains(&entry.name))
        {
            let operator = AssignedOperator::from_progress(&entry.name, entry.progress);
            let mut trial = scratch.clone();
            trial.set_room(room.id.clone(), vec![operator.clone()]);
            let resolved = crate::layout::resolve_base(
                ctx.blueprint,
                &trial,
                Some(ctx.instances),
                Some(ctx.table),
                ctx.mood,
                None,
            )
            .ok()?;
            let power = resolved
                .power_rooms
                .iter()
                .find(|candidate| candidate.id == room.id)?;
            let score = crate::power::solve_power(
                &crate::power::PowerRoomInput {
                    operator: power.operator.clone(),
                    mood: power.operator.work_mood.unwrap_or(ctx.mood),
                    shift_hours: 24.0,
                    layout: power.layout.clone(),
                },
                ctx.table,
            )
            .ok()?
            .final_efficiency;
            if best.as_ref().is_none_or(|(current, _)| score > *current) {
                best = Some((score, operator));
            }
        }
        if let Some((_, operator)) = best {
            used.insert(operator.name.clone());
            scratch.set_room(room.id.clone(), vec![operator]);
        }
    }
    let resolved = crate::layout::resolve_base(
        ctx.blueprint,
        &scratch,
        Some(ctx.instances),
        Some(ctx.table),
        ctx.mood,
        None,
    )
    .ok()?;
    let scores = resolved
        .power_rooms
        .iter()
        .map(|room| {
            crate::power::solve_power(
                &crate::power::PowerRoomInput {
                    operator: room.operator.clone(),
                    mood: room.operator.work_mood.unwrap_or(ctx.mood),
                    shift_hours: 24.0,
                    layout: room.layout.clone(),
                },
                ctx.table,
            )
            .map(|result| result.final_efficiency)
            .ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(crate::power::aggregate_power_efficiency(scores))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::blueprint::{BlueprintScenario, RoomId};
    use crate::operbox::OperBoxEntry;

    fn room(
        id: &str,
        kind: FacilityKind,
        level: u8,
        product: Option<RoomProduct>,
    ) -> RoomBlueprint {
        RoomBlueprint {
            id: RoomId::from(id),
            kind,
            level,
            product,
            dorm_beds: None,
            dorm_ambience_level: None,
        }
    }

    fn blueprint(rooms: Vec<RoomBlueprint>) -> BaseBlueprint {
        BaseBlueprint {
            template: None,
            drone_cap: 135,
            scenario: BlueprintScenario::default(),
            rooms,
        }
    }

    fn operbox(entries: &[(&str, u8)]) -> OperBox {
        OperBox::from_entries(
            entries
                .iter()
                .enumerate()
                .map(|(index, (name, elite))| {
                    let rarity = match *name {
                        "Lancet-2" => 1,
                        "清流" | "冬时" => 4,
                        _ => 6,
                    };
                    OperBoxEntry {
                        id: format!("test-{index}"),
                        name: (*name).to_string(),
                        elite: *elite,
                        level: if rarity <= 2 { 30 } else { 1 },
                        own: true,
                        potential: 0,
                        rarity,
                    }
                })
                .collect(),
        )
        .unwrap()
    }

    fn compile(
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        seed: &BaseAssignment,
    ) -> CompiledRules {
        compile_with_preferences(blueprint, operbox, seed, &HashMap::new())
    }

    fn compile_with_preferences(
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        seed: &BaseAssignment,
        preferences: &HashMap<String, String>,
    ) -> CompiledRules {
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let catalog = load_rule_catalog(&default_rule_catalog_path().unwrap()).unwrap();
        compile_catalog_range(
            &catalog,
            blueprint,
            operbox,
            &instances,
            &table,
            seed,
            &HashSet::new(),
            preferences,
            i32::MIN..=i32::MAX,
        )
    }

    fn compile_catalog(
        catalog: &RuleCatalog,
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        seed: &BaseAssignment,
    ) -> CompiledRules {
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        compile_catalog_range(
            catalog,
            blueprint,
            operbox,
            &instances,
            &table,
            seed,
            &HashSet::new(),
            &HashMap::new(),
            i32::MIN..=i32::MAX,
        )
    }

    fn manufacture_objective_catalog(candidates: &[&str]) -> RuleCatalog {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "rules": [{
                "id": "manufacture_objective_test",
                "priority": 20,
                "alternatives": [{
                    "id": "battle_record",
                    "roles": [{
                        "id": "wendy",
                        "facility": "factory",
                        "recipe": "battle_record",
                        "candidates": [{"name": "温蒂", "elite": 2}],
                        "room_group": "objective_room"
                    }, {
                        "id": "peer",
                        "facility": "factory",
                        "recipe": "battle_record",
                        "candidates": candidates.iter().map(|name| serde_json::json!({
                            "name": name,
                            "elite": 2
                        })).collect::<Vec<_>>(),
                        "min_count": 0,
                        "max_count": 1,
                        "selection": "manufacture_objective",
                        "room_group": "objective_room"
                    }]
                }]
            }]
        }))
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_catalog_range(
        catalog: &RuleCatalog,
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        instances: &OperatorInstances,
        table: &SkillTable,
        seed: &BaseAssignment,
        inherited_excluded: &HashSet<String>,
        preferences: &HashMap<String, String>,
        priority_range: std::ops::RangeInclusive<i32>,
    ) -> CompiledRules {
        compile_rules(
            catalog,
            &RuleCompileContext {
                blueprint,
                operbox,
                instances,
                table,
                mode: AssignShiftMode::Peak,
                mood: 24.0,
                preferences,
            },
            seed,
            inherited_excluded,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            priority_range,
        )
        .unwrap()
    }

    fn exclusion_catalog() -> RuleCatalog {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "rules": [
                {
                    "id": "high_exclusion",
                    "priority": 20,
                    "alternatives": [{
                        "id": "active",
                        "exclude_operators": ["桑葚"],
                        "roles": [{
                            "id": "owner",
                            "facility": "control_center",
                            "candidates": [{"name": "重岳", "elite": 0}]
                        }]
                    }]
                },
                {
                    "id": "later_candidate",
                    "priority": 18,
                    "alternatives": [{
                        "id": "revive",
                        "roles": [{
                            "id": "candidate",
                            "facility": "office",
                            "candidates": [{"name": "桑葚", "elite": 2}]
                        }]
                    }]
                }
            ]
        }))
        .unwrap()
    }

    fn selected<'a>(compiled: &'a CompiledRules, rule: &str) -> Option<&'a str> {
        compiled
            .selected
            .iter()
            .find(|selected| selected.rule_id == rule)
            .map(|selected| selected.alternative_id.as_str())
    }

    #[test]
    fn preferences_reject_unknown_rule_or_alternative() {
        let catalog = load_rule_catalog(&default_rule_catalog_path().unwrap()).unwrap();
        let unknown_rule = HashMap::from([("missing".to_string(), "anything".to_string())]);
        assert!(validate_rule_preferences(&catalog, &unknown_rule)
            .unwrap_err()
            .to_string()
            .contains("unknown orchestration rule"));

        let unknown_alternative =
            HashMap::from([("rosemary_perception".to_string(), "missing".to_string())]);
        assert!(validate_rule_preferences(&catalog, &unknown_alternative)
            .unwrap_err()
            .to_string()
            .contains("unknown alternative"));
    }

    #[test]
    fn alternative_exclusion_conflicting_with_seed_rejects_rule_generically() {
        let bp = blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room("office", FacilityKind::Office, 3, None),
        ]);
        let roster = operbox(&[("重岳", 0), ("桑葚", 2)]);
        let mut seed = BaseAssignment::default();
        seed.set_room("office", vec![AssignedOperator::new("桑葚", 2)]);
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let compiled = compile_catalog_range(
            &exclusion_catalog(),
            &bp,
            &roster,
            &instances,
            &table,
            &seed,
            &HashSet::new(),
            &HashMap::new(),
            i32::MIN..=i32::MAX,
        );

        assert_eq!(selected(&compiled, "high_exclusion"), None);
    }

    #[test]
    fn inherited_high_priority_exclusion_blocks_later_rule_generically() {
        let bp = blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room("office", FacilityKind::Office, 3, None),
        ]);
        let roster = operbox(&[("重岳", 0), ("桑葚", 2)]);
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let catalog = exclusion_catalog();
        let high = compile_catalog_range(
            &catalog,
            &bp,
            &roster,
            &instances,
            &table,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashMap::new(),
            19..=i32::MAX,
        );
        assert_eq!(selected(&high, "high_exclusion"), Some("active"));
        let inherited: HashSet<_> = high.excluded_operators.iter().cloned().collect();

        let later = compile_catalog_range(
            &catalog,
            &bp,
            &roster,
            &instances,
            &table,
            &high.scratch,
            &inherited,
            &HashMap::new(),
            i32::MIN..=18,
        );
        assert_eq!(selected(&later, "later_candidate"), None);
        assert!(!later.scratch.operator_names().contains("桑葚"));
    }

    #[test]
    fn inherited_exclusive_group_blocks_later_rule_generically() {
        let bp = blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room("office", FacilityKind::Office, 3, None),
        ]);
        let roster = operbox(&[("重岳", 0), ("桑葚", 2)]);
        let instances =
            OperatorInstances::load(&crate::instances::default_instances_path().unwrap()).unwrap();
        let table =
            SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap();
        let catalog: RuleCatalog = serde_json::from_value(serde_json::json!({
            "version": 1,
            "rules": [
                {
                    "id": "high_group",
                    "priority": 20,
                    "exclusive_group": "shared_path",
                    "alternatives": [{
                        "id": "active",
                        "roles": [{
                            "id": "owner",
                            "facility": "control_center",
                            "candidates": [{"name": "重岳", "elite": 0}]
                        }]
                    }]
                },
                {
                    "id": "late_group",
                    "priority": 18,
                    "exclusive_group": "shared_path",
                    "alternatives": [{
                        "id": "active",
                        "roles": [{
                            "id": "candidate",
                            "facility": "office",
                            "candidates": [{"name": "桑葚", "elite": 2}]
                        }]
                    }]
                }
            ]
        }))
        .unwrap();
        let high = compile_catalog_range(
            &catalog,
            &bp,
            &roster,
            &instances,
            &table,
            &BaseAssignment::default(),
            &HashSet::new(),
            &HashMap::new(),
            19..=i32::MAX,
        );
        assert_eq!(selected(&high, "high_group"), Some("active"));

        let late = compile_rules(
            &catalog,
            &RuleCompileContext {
                blueprint: &bp,
                operbox: &roster,
                instances: &instances,
                table: &table,
                mode: AssignShiftMode::Peak,
                mood: 24.0,
                preferences: &HashMap::new(),
            },
            &high.scratch,
            &high.excluded_operators.iter().cloned().collect(),
            &high.claimed_exclusive_groups,
            &HashSet::new(),
            &HashSet::new(),
            i32::MIN..=18,
        )
        .unwrap();

        assert_eq!(selected(&late, "late_group"), None);
        assert!(late.claimed_exclusive_groups.contains("shared_path"));
    }

    #[test]
    fn trade_core_uses_actual_station_count_and_order_only() {
        let one_gold = blueprint(vec![room(
            "any-trade-name",
            FacilityKind::TradePost,
            3,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold,
            }),
        )]);
        let roster = operbox(&[("可露希尔", 2), ("但书", 2)]);
        let one = compile(&one_gold, &roster, &BaseAssignment::default());
        assert_eq!(selected(&one, "trade_core"), Some("single_trade_closure"));
        assert_eq!(one.anchors[0].operator, "可露希尔");

        let multi_gold = blueprint(vec![
            room(
                "left",
                FacilityKind::TradePost,
                3,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
            ),
            room(
                "right",
                FacilityKind::TradePost,
                3,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Originium,
                }),
            ),
        ]);
        let multi = compile(&multi_gold, &roster, &BaseAssignment::default());
        assert_eq!(selected(&multi, "trade_core"), Some("multi_trade_docus"));
        assert!(multi.anchors.iter().any(|anchor| anchor.operator == "但书"));

        let multi_e0 = compile(
            &multi_gold,
            &operbox(&[("可露希尔", 2), ("但书", 0)]),
            &BaseAssignment::default(),
        );
        assert_eq!(selected(&multi_e0, "trade_core"), Some("multi_trade_docus"));
        assert!(multi_e0
            .anchors
            .iter()
            .any(|anchor| anchor.operator == "但书" && anchor.elite == 0));

        let all_originium = blueprint(vec![room(
            "only",
            FacilityKind::TradePost,
            3,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Originium,
            }),
        )]);
        let none = compile(&all_originium, &roster, &BaseAssignment::default());
        assert_eq!(selected(&none, "trade_core"), None);
    }

    #[test]
    fn trade_core_room_level_preferences_ignore_blueprint_order() {
        let mixed_levels = blueprint(vec![
            room(
                "high-first",
                FacilityKind::TradePost,
                3,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
            ),
            room(
                "low-last",
                FacilityKind::TradePost,
                2,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
            ),
        ]);

        let docus = compile(
            &mixed_levels,
            &operbox(&[("但书", 2), ("可露希尔", 2)]),
            &BaseAssignment::default(),
        );
        assert_eq!(selected(&docus, "trade_core"), Some("multi_trade_docus"));
        assert_eq!(docus.anchors[0].room_id.as_ref().unwrap().0, "low-last");

        let closure = compile(
            &mixed_levels,
            &operbox(&[("可露希尔", 2)]),
            &BaseAssignment::default(),
        );
        assert_eq!(
            selected(&closure, "trade_core"),
            Some("multi_trade_closure_fallback")
        );
        assert_eq!(closure.anchors[0].room_id.as_ref().unwrap().0, "high-first");
    }

    fn rosemary_blueprint() -> BaseBlueprint {
        blueprint(vec![
            room(
                "factory",
                FacilityKind::Factory,
                3,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold,
                }),
            ),
            room(
                "trade",
                FacilityKind::TradePost,
                3,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
            ),
            room("control", FacilityKind::ControlCenter, 3, None),
            room("dorm", FacilityKind::Dormitory, 5, None),
            room("office", FacilityKind::Office, 3, None),
            room("power-a", FacilityKind::PowerPlant, 3, None),
            room("power-b", FacilityKind::PowerPlant, 3, None),
        ])
    }

    fn rosemary_two_trade_blueprint() -> BaseBlueprint {
        let mut bp = rosemary_blueprint();
        bp.rooms.push(room(
            "trade-b",
            FacilityKind::TradePost,
            3,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold,
            }),
        ));
        bp
    }

    fn three_trade_cohort_operbox(include: &[&str]) -> OperBox {
        let entries = [
            ("但书", 2),
            ("可露希尔", 2),
            ("迷迭香", 2),
            ("黑键", 2),
            ("八幡海铃", 2),
            ("夕", 0),
            ("巫恋", 2),
            ("龙舌兰", 2),
            ("柏喙", 2),
            ("爱丽丝", 2),
            ("车尔尼", 2),
        ];
        let filtered: Vec<_> = entries
            .into_iter()
            .filter(|(name, _)| include.contains(name))
            .collect();
        operbox(&filtered)
    }

    #[test]
    fn rosemary_conditional_pack_reserves_full_three_trade_cohorts() {
        let bp = rosemary_two_trade_blueprint();
        let all = [
            "但书",
            "可露希尔",
            "迷迭香",
            "黑键",
            "八幡海铃",
            "夕",
            "巫恋",
            "龙舌兰",
            "柏喙",
            "爱丽丝",
            "车尔尼",
        ];
        let compiled = compile(
            &bp,
            &three_trade_cohort_operbox(&all),
            &BaseAssignment::default(),
        );
        let selected = compiled
            .selected
            .iter()
            .find(|selected| selected.rule_id == "rosemary_perception")
            .unwrap();
        assert_eq!(selected.alternative_id, "recruit_refresh");
        assert_eq!(
            selected.conditional_pack_id.as_deref(),
            Some("three_trade_cohorts")
        );
        let docus_room = compiled
            .scratch
            .iter_rooms()
            .find(|room| {
                room.operators
                    .iter()
                    .any(|operator| operator.name == "但书")
            })
            .unwrap();
        let blackkey_room = compiled
            .scratch
            .iter_rooms()
            .find(|room| {
                room.operators
                    .iter()
                    .any(|operator| operator.name == "黑键")
            })
            .unwrap();
        assert_ne!(docus_room.room_id, blackkey_room.room_id);
        assert!(blackkey_room
            .operators
            .iter()
            .any(|operator| operator.name == "可露希尔"));
        assert!(docus_room
            .operators
            .iter()
            .all(|operator| operator.name != "黑键" && operator.name != "可露希尔"));
        assert_eq!(compiled.rotation_reserves.len(), 1);
        let mut reserve_operators = compiled.rotation_reserves[0].operators.clone();
        reserve_operators.sort();
        assert_eq!(reserve_operators, vec!["巫恋", "柏喙", "龙舌兰"]);
        assert_eq!(compiled.rotation_reserves[0].eligible_rooms.len(), 2);
        assert_eq!(
            compiled.rotation_reserves[0].reuse_policy,
            ReserveReusePolicy::EveryEligibleHalf
        );
        assert!(compiled.rotation_reserves[0].require_pre_split_halves);
    }

    #[test]
    fn rosemary_conditional_pack_degrades_without_each_full_cohort_requirement() {
        let bp = rosemary_two_trade_blueprint();
        let all = [
            "但书",
            "可露希尔",
            "迷迭香",
            "黑键",
            "八幡海铃",
            "夕",
            "巫恋",
            "龙舌兰",
            "柏喙",
            "爱丽丝",
            "车尔尼",
        ];
        for missing in ["可露希尔", "巫恋", "龙舌兰", "柏喙"] {
            let include: Vec<_> = all
                .iter()
                .copied()
                .filter(|name| *name != missing)
                .collect();
            let compiled = compile(
                &bp,
                &three_trade_cohort_operbox(&include),
                &BaseAssignment::default(),
            );
            let selected = compiled
                .selected
                .iter()
                .find(|selected| selected.rule_id == "rosemary_perception")
                .unwrap_or_else(|| panic!("missing {missing} must not close Rosemary itself"));
            assert_eq!(selected.conditional_pack_id, None, "missing {missing}");
            assert!(compiled.rotation_reserves.is_empty(), "missing {missing}");
            assert!(
                compiled
                    .anchors
                    .iter()
                    .all(|anchor| anchor.operator != "可露希尔"),
                "missing {missing}: fallback must not force Closure"
            );
        }

        let inactive_names: Vec<_> = all.iter().copied().filter(|name| *name != "夕").collect();
        let inactive = compile(
            &bp,
            &three_trade_cohort_operbox(&inactive_names),
            &BaseAssignment::default(),
        );
        assert_eq!(selected(&inactive, "rosemary_perception"), None);
        assert!(inactive.rotation_reserves.is_empty());
        assert!(inactive
            .anchors
            .iter()
            .all(|anchor| anchor.operator != "可露希尔"));

        let mut constrained = bp.clone();
        constrained
            .rooms
            .iter_mut()
            .find(|room| room.id.0 == "trade")
            .unwrap()
            .level = 2;
        let capacity_fallback = compile(
            &constrained,
            &three_trade_cohort_operbox(&all),
            &BaseAssignment::default(),
        );
        assert_eq!(
            capacity_fallback
                .selected
                .iter()
                .find(|selected| selected.rule_id == "rosemary_perception")
                .unwrap()
                .conditional_pack_id,
            None,
            "C 必须能复用到两间目标房，任一容量不足都不能激活完整 pack"
        );
    }

    #[test]
    fn rosemary_gate_uses_resolved_perception_not_path_member_name() {
        let bp = rosemary_blueprint();
        let forty = operbox(&[("迷迭香", 2), ("黑键", 2), ("八幡海铃", 2)]);
        let inactive = compile(&bp, &forty, &BaseAssignment::default());
        assert_eq!(selected(&inactive, "rosemary_perception"), None);
        assert!(!inactive.skip_registry_ids.contains("human_fireworks_pure"));

        // 40 感知 + 塑心 E0（固定 5 点 SilentEcho）仍不能跨过 60 感知启动门槛：
        // 无声共鸣不是 Perception 资源。
        let forty_plus_suscentheart =
            operbox(&[("迷迭香", 2), ("黑键", 2), ("八幡海铃", 2), ("塑心", 0)]);
        let inactive_suscentheart =
            compile(&bp, &forty_plus_suscentheart, &BaseAssignment::default());
        assert_eq!(
            selected(&inactive_suscentheart, "rosemary_perception"),
            None,
            "塑心 SilentEcho 不参与 60 感知门槛（ADR 0004 D4）"
        );

        let fifty = operbox(&[("迷迭香", 2), ("黑键", 2), ("八幡海铃", 2), ("夕", 0)]);
        let below_gate = compile(&bp, &fifty, &BaseAssignment::default());
        assert_eq!(
            selected(&below_gate, "rosemary_perception"),
            None,
            "50 感知低于 60 启动门槛"
        );

        let sixty = operbox(&[
            ("迷迭香", 2),
            ("黑键", 2),
            ("八幡海铃", 2),
            ("夕", 0),
            ("爱丽丝", 2),
            ("车尔尼", 2),
        ]);
        let compiled = compile(&bp, &sixty, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "rosemary_perception"),
            Some("recruit_refresh")
        );
        assert!(compiled.skip_registry_ids.contains("human_fireworks_pure"));
        assert!(!compiled
            .skip_registry_ids
            .contains("human_fireworks_perception"));
        let names: HashSet<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "rosemary_perception")
            .map(|anchor| anchor.operator.as_str())
            .collect();
        assert!(["迷迭香", "黑键", "夕"]
            .into_iter()
            .all(|name| names.contains(name)));
        assert!(compiled.shift_binds.iter().any(|bind| {
            ["迷迭香", "黑键", "夕"]
                .into_iter()
                .all(|name| bind.operators.iter().any(|operator| operator == name))
        }));
        assert!(compiled
            .shift_binds
            .iter()
            .all(|bind| { !bind.operators.iter().any(|operator| operator == "八幡海铃") }));

        let with_fireworks = operbox(&[
            ("迷迭香", 2),
            ("黑键", 2),
            ("八幡海铃", 2),
            ("夕", 0),
            ("爱丽丝", 2),
            ("车尔尼", 2),
            ("重岳", 0),
            ("令", 2),
            ("乌有", 2),
        ]);
        let fireworks = compile(&bp, &with_fireworks, &BaseAssignment::default());
        assert_eq!(
            selected(&fireworks, "human_fireworks_perception"),
            Some("actual_perception_core")
        );
        assert!(fireworks.excluded_operators.contains(&"桑葚".to_string()));
        assert!(fireworks.shift_binds.iter().any(|bind| {
            ["重岳", "令", "乌有"]
                .iter()
                .all(|name| bind.operators.iter().any(|operator| operator == name))
        }));

        let mut blocked_seed = BaseAssignment::default();
        blocked_seed.set_room("office", vec![AssignedOperator::new("桑葚", 2)]);
        let blocked = compile(&bp, &with_fireworks, &blocked_seed);
        assert_eq!(
            selected(&blocked, "human_fireworks_perception"),
            None,
            "排除者已在 seed 时 alternative 必须整体不成立"
        );
    }

    fn pinus_blueprint() -> BaseBlueprint {
        blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room(
                "br-small",
                FacilityKind::Factory,
                2,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::BattleRecord,
                }),
            ),
            room(
                "br-other",
                FacilityKind::Factory,
                2,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::BattleRecord,
                }),
            ),
        ])
    }

    #[test]
    fn pinus_requires_both_cores_and_keeps_all_two_or_three_members() {
        let bp = pinus_blueprint();
        let two = operbox(&[("焰尾", 2), ("薇薇安娜", 2), ("灰毫", 2), ("远牙", 2)]);
        let two_plan = compile(&bp, &two, &BaseAssignment::default());
        assert_eq!(
            selected(&two_plan, "pinus_sylvestris"),
            Some("battle_record_members")
        );
        assert_eq!(
            two_plan
                .anchors
                .iter()
                .filter(|anchor| anchor.system_id == "pinus_sylvestris")
                .count(),
            4
        );

        let three = operbox(&[
            ("焰尾", 2),
            ("薇薇安娜", 2),
            ("灰毫", 2),
            ("远牙", 2),
            ("野鬃", 2),
        ]);
        let three_plan = compile(&bp, &three, &BaseAssignment::default());
        assert_eq!(
            selected(&three_plan, "pinus_sylvestris"),
            Some("battle_record_members")
        );
        assert_eq!(
            three_plan
                .anchors
                .iter()
                .filter(|anchor| ["灰毫", "远牙", "野鬃"].contains(&anchor.operator.as_str()))
                .count(),
            3,
            "两个二级作战记录站必须容纳全部三名实际拥有的红松制造成员"
        );
        let rooms: HashSet<_> = three_plan
            .anchors
            .iter()
            .filter(|anchor| ["灰毫", "远牙", "野鬃"].contains(&anchor.operator.as_str()))
            .filter_map(|anchor| anchor.room_id.as_ref().map(|room| room.0.as_str()))
            .collect();
        assert_eq!(rooms.len(), 2, "三级成员必须可跨作战记录站");
        assert_eq!(bp.count_facility(FacilityKind::Factory), 2);
        assert_eq!(
            bp.rooms
                .iter()
                .filter(|room| {
                    matches!(
                        room.product.as_ref(),
                        Some(RoomProduct::Factory { recipe }) if *recipe == RecipeKind::Gold
                    )
                })
                .count(),
            0,
            "赤金线不是红松激活前提"
        );

        let missing_core = operbox(&[("焰尾", 2), ("灰毫", 2), ("远牙", 2)]);
        assert_eq!(
            selected(
                &compile(&bp, &missing_core, &BaseAssignment::default()),
                "pinus_sylvestris"
            ),
            None
        );

        let insufficient_capacity = blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room(
                "only-br",
                FacilityKind::Factory,
                2,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::BattleRecord,
                }),
            ),
        ]);
        let blocked = compile(&insufficient_capacity, &three, &BaseAssignment::default());
        assert_eq!(selected(&blocked, "pinus_sylvestris"), None);
        assert!(blocked
            .anchors
            .iter()
            .all(|anchor| anchor.system_id != "pinus_sylvestris"));
    }

    fn two_power_blueprint() -> BaseBlueprint {
        blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room(
                "gold",
                FacilityKind::Factory,
                2,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold,
                }),
            ),
            room("power-a", FacilityKind::PowerPlant, 3, None),
            room("power-b", FacilityKind::PowerPlant, 3, None),
            room("workshop", FacilityKind::Workshop, 3, None),
        ])
    }

    #[test]
    fn automation_two_power_requires_wendy_and_resolves_lancet_state() {
        let bp = two_power_blueprint();
        let roster = operbox(&[
            ("清流", 1),
            ("温蒂", 2),
            ("森蚺", 2),
            ("Lancet-2", 0),
            ("承曦格雷伊", 2),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("two_power_control_eunectes")
        );
        let lancet = compiled
            .anchors
            .iter()
            .find(|anchor| anchor.operator == "Lancet-2")
            .unwrap();
        assert_eq!(lancet.work_mood, Some(0));
        assert_eq!(lancet.rest_facility, Some(FacilityKind::Workshop));
        assert!(compiled
            .anchors
            .iter()
            .any(|anchor| anchor.operator == "温蒂"));

        let without_wendy =
            operbox(&[("清流", 1), ("森蚺", 2), ("Lancet-2", 0), ("承曦格雷伊", 2)]);
        assert_eq!(
            selected(
                &compile(&bp, &without_wendy, &BaseAssignment::default()),
                "automation_group"
            ),
            None
        );

        let mut full_control = BaseAssignment::default();
        full_control.set_room(
            "control",
            (0..5)
                .map(|index| AssignedOperator::new(format!("occupied-{index}"), 0))
                .collect(),
        );
        assert_eq!(
            selected(&compile(&bp, &roster, &full_control), "automation_group"),
            None,
            "拥有森蚺+Lancet但中枢无空位时不得伪降级为制造森蚺"
        );
    }

    #[test]
    fn automation_three_power_fallback_still_has_required_wendy() {
        let bp = blueprint(vec![
            room("control", FacilityKind::ControlCenter, 3, None),
            room(
                "general",
                FacilityKind::Factory,
                3,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::BattleRecord,
                }),
            ),
            room(
                "gold",
                FacilityKind::Factory,
                3,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold,
                }),
            ),
            room(
                "other",
                FacilityKind::Factory,
                3,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::BattleRecord,
                }),
            ),
            room(
                "trade",
                FacilityKind::TradePost,
                3,
                Some(RoomProduct::Trade {
                    order: TradeOrderKind::Gold,
                }),
            ),
            room("power-a", FacilityKind::PowerPlant, 3, None),
            room("power-b", FacilityKind::PowerPlant, 3, None),
            room("power-c", FacilityKind::PowerPlant, 3, None),
        ]);
        let roster = operbox(&[("温蒂", 2), ("森蚺", 2), ("冬时", 2), ("承曦格雷伊", 2)]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("three_power_general")
        );
        let automation: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "automation_group")
            .collect();
        assert!(automation.iter().any(|anchor| anchor.operator == "温蒂"));
        assert_eq!(
            automation
                .iter()
                .filter(|anchor| anchor.facility == FacilityKind::Factory)
                .count(),
            3
        );
    }

    #[test]
    fn automation_two_power_without_lancet_or_qingliu_uses_general_eunectes_team() {
        let bp = two_power_blueprint();
        let roster = operbox(&[("温蒂", 2), ("森蚺", 2), ("冬时", 2), ("承曦格雷伊", 2)]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("two_power_factory_eunectes_general")
        );
        let factory_names: HashSet<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.system_id == "automation_group" && anchor.facility == FacilityKind::Factory
            })
            .map(|anchor| anchor.operator.as_str())
            .collect();
        assert_eq!(factory_names, HashSet::from(["温蒂", "森蚺"]));
    }

    #[test]
    fn automation_two_power_control_branch_uses_available_third_factory_slot() {
        let mut bp = two_power_blueprint();
        bp.rooms
            .iter_mut()
            .find(|room| room.id.0 == "gold")
            .unwrap()
            .level = 3;
        let roster = operbox(&[
            ("清流", 1),
            ("温蒂", 2),
            ("森蚺", 2),
            ("Lancet-2", 0),
            ("冬时", 2),
            ("异客", 2),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("two_power_control_eunectes")
        );
        let factory: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.system_id == "automation_group" && anchor.facility == FacilityKind::Factory
            })
            .collect();
        assert_eq!(factory.len(), 3);
        assert!(factory
            .iter()
            .all(|anchor| anchor.room_id == factory[0].room_id));
        assert!(factory.iter().any(|anchor| anchor.operator == "冬时"));
        assert!(factory.iter().all(|anchor| anchor.operator != "异客"));
    }

    #[test]
    fn automation_lancet_keeps_normal_power_efficiency_without_chengxi() {
        let roster = operbox(&[
            ("清流", 1),
            ("温蒂", 2),
            ("森蚺", 2),
            ("Lancet-2", 0),
            ("冬时", 1),
        ]);
        let compiled = compile(&two_power_blueprint(), &roster, &BaseAssignment::default());
        assert!(compiled
            .anchors
            .iter()
            .any(|anchor| anchor.operator == "Lancet-2" && anchor.work_mood.is_none()));
    }

    #[test]
    fn automation_two_power_prefers_level_two_gold_room() {
        let mut bp = two_power_blueprint();
        bp.rooms
            .iter_mut()
            .find(|room| room.id.0 == "gold")
            .unwrap()
            .level = 3;
        bp.rooms.push(room(
            "gold-low",
            FacilityKind::Factory,
            2,
            Some(RoomProduct::Factory {
                recipe: RecipeKind::Gold,
            }),
        ));
        bp.rooms.push(room(
            "gold-level-one",
            FacilityKind::Factory,
            1,
            Some(RoomProduct::Factory {
                recipe: RecipeKind::Gold,
            }),
        ));
        let roster = operbox(&[
            ("清流", 1),
            ("温蒂", 2),
            ("森蚺", 2),
            ("Lancet-2", 0),
            ("冬时", 1),
            ("异客", 2),
            ("承曦格雷伊", 2),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("two_power_control_eunectes")
        );
        let factory: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.system_id == "automation_group" && anchor.facility == FacilityKind::Factory
            })
            .collect();
        assert_eq!(factory.len(), 2);
        assert!(factory.iter().all(|anchor| anchor
            .room_id
            .as_ref()
            .is_some_and(|room| room.0 == "gold-low")));
        assert_eq!(
            compiled
                .anchors
                .iter()
                .find(|anchor| anchor.operator == "Lancet-2")
                .and_then(|anchor| anchor.work_mood),
            Some(0)
        );
    }

    #[test]
    fn automation_two_power_control_accepts_e1_winter_as_third_factory_candidate() {
        let mut bp = two_power_blueprint();
        bp.rooms
            .iter_mut()
            .find(|room| room.id.0 == "gold")
            .unwrap()
            .level = 3;
        let roster = operbox(&[
            ("清流", 1),
            ("温蒂", 2),
            ("森蚺", 2),
            ("Lancet-2", 0),
            ("冬时", 1),
            ("异客", 2),
            ("承曦格雷伊", 2),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "automation_group"),
            Some("two_power_control_eunectes")
        );
        let factory: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.system_id == "automation_group" && anchor.facility == FacilityKind::Factory
            })
            .collect();
        assert_eq!(factory.len(), 3);
        assert!(factory.iter().any(|anchor| anchor.operator == "冬时"));
        assert!(factory.iter().all(|anchor| anchor.operator != "异客"));
    }

    #[test]
    fn manufacture_objective_is_order_independent_and_enforces_boundaries() {
        let objective_blueprint =
            |level| {
                let mut rooms = vec![
                    room(
                        "factory",
                        FacilityKind::Factory,
                        level,
                        Some(RoomProduct::Factory {
                            recipe: RecipeKind::BattleRecord,
                        }),
                    ),
                    room("office", FacilityKind::Office, 3, None),
                ];
                rooms.extend((0..5).map(|index| {
                    room(&format!("power-{index}"), FacilityKind::PowerPlant, 3, None)
                }));
                blueprint(rooms)
            };
        let candidate = |compiled: &CompiledRules| {
            compiled
                .anchors
                .iter()
                .find(|anchor| {
                    anchor.system_id == "manufacture_objective_test" && anchor.operator != "温蒂"
                })
                .map(|anchor| anchor.operator.clone())
        };
        let level_two = objective_blueprint(2);
        let full = operbox(&[("温蒂", 2), ("冬时", 2), ("异客", 2)]);
        let winter_first = compile_catalog(
            &manufacture_objective_catalog(&["冬时", "异客"]),
            &level_two,
            &full,
            &BaseAssignment::default(),
        );
        let passenger_first = compile_catalog(
            &manufacture_objective_catalog(&["异客", "冬时"]),
            &level_two,
            &full,
            &BaseAssignment::default(),
        );
        assert_eq!(candidate(&winter_first).as_deref(), Some("异客"));
        assert_eq!(candidate(&passenger_first).as_deref(), Some("异客"));
        assert!(winter_first
            .anchors
            .iter()
            .all(|anchor| anchor.operator != "冬时"));

        let mut constrained_catalog = manufacture_objective_catalog(&["冬时", "异客"]);
        constrained_catalog.rules[0].alternatives[0]
            .relations
            .push(RelationSpec::ForbidSameRoom {
                a: "异客".to_string(),
                b: "温蒂".to_string(),
            });
        let constrained = compile_catalog(
            &constrained_catalog,
            &level_two,
            &full,
            &BaseAssignment::default(),
        );
        assert_eq!(candidate(&constrained).as_deref(), Some("冬时"));
        assert!(constrained
            .anchors
            .iter()
            .all(|anchor| anchor.operator != "异客"));

        let underleveled = operbox(&[("温蒂", 2), ("冬时", 2), ("异客", 1)]);
        let qualified = compile_catalog(
            &manufacture_objective_catalog(&["异客", "冬时"]),
            &level_two,
            &underleveled,
            &BaseAssignment::default(),
        );
        assert_eq!(candidate(&qualified).as_deref(), Some("冬时"));
        assert!(qualified
            .anchors
            .iter()
            .all(|anchor| anchor.operator != "异客"));

        let no_candidate = compile_catalog(
            &manufacture_objective_catalog(&["异客", "冬时"]),
            &level_two,
            &operbox(&[("温蒂", 2)]),
            &BaseAssignment::default(),
        );
        assert_eq!(candidate(&no_candidate), None);
        assert_eq!(
            selected(&no_candidate, "manufacture_objective_test"),
            Some("battle_record")
        );

        let no_slot = compile_catalog(
            &manufacture_objective_catalog(&["冬时", "异客"]),
            &objective_blueprint(1),
            &full,
            &BaseAssignment::default(),
        );
        assert_eq!(candidate(&no_slot), None);

        let missing_core = compile_catalog(
            &manufacture_objective_catalog(&["冬时", "异客"]),
            &level_two,
            &operbox(&[("冬时", 2), ("异客", 2)]),
            &BaseAssignment::default(),
        );
        assert_eq!(selected(&missing_core, "manufacture_objective_test"), None);

        let mut used = BaseAssignment::default();
        used.set_room("office", vec![AssignedOperator::new("冬时", 2)]);
        let conflict = compile_catalog(
            &manufacture_objective_catalog(&["冬时", "异客"]),
            &level_two,
            &full,
            &used,
        );
        assert_eq!(candidate(&conflict).as_deref(), Some("异客"));
        assert_eq!(
            conflict
                .anchors
                .iter()
                .filter(|anchor| anchor.operator == "冬时")
                .count(),
            0
        );
    }

    #[test]
    fn rhine_soft_candidate_uses_actual_support_rooms_and_one_way_dependency() {
        let bp = blueprint(vec![
            room(
                "gold",
                FacilityKind::Factory,
                1,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold,
                }),
            ),
            room("dorm", FacilityKind::Dormitory, 5, None),
            room("meeting", FacilityKind::MeetingRoom, 3, None),
        ]);
        let roster = operbox(&[("娜斯提", 2), ("塞雷娅", 0), ("麦哲伦", 0)]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert_eq!(
            selected(&compiled, "rhine_receivers"),
            Some("nasti_actual_support")
        );
        let rhine: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "rhine_receivers")
            .collect();
        assert_eq!(rhine.len(), 2, "receiver + 1 eligible support room");
        assert!(rhine.iter().all(|anchor| anchor.room_id.is_some()));
        let support_rooms: HashSet<_> = rhine
            .iter()
            .filter(|anchor| matches!(anchor.facility, FacilityKind::Dormitory))
            .filter_map(|anchor| anchor.room_id.as_ref())
            .collect();
        assert_eq!(
            support_rooms.len(),
            1,
            "one_per_room 必须选择一个 eligible 支持房"
        );
        assert!(compiled
            .shift_binds
            .iter()
            .all(|bind| { !bind.operators.iter().any(|name| name == "娜斯提") }));
        let dependency = compiled
            .active_dependencies
            .iter()
            .find(|dependency| dependency.consumers.iter().any(|name| name == "娜斯提"))
            .unwrap();
        assert_eq!(dependency.required.len(), 2);
        assert!(dependency.required.iter().any(|name| name == "塞雷娅"));
        assert!(dependency.required.iter().any(|name| name == "麦哲伦"));

        let mut occupied = BaseAssignment::default();
        occupied.set_room("dorm", vec![AssignedOperator::new("occupied-a", 0)]);
        occupied.set_room("meeting", vec![AssignedOperator::new("occupied-b", 0)]);
        let no_support = compile(&bp, &roster, &occupied);
        assert_eq!(
            selected(&no_support, "rhine_receivers"),
            None,
            "没有真实空余支持位时 receiver 不得激活"
        );
        let support_count = no_support
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.system_id == "rhine_receivers"
                    && matches!(
                        anchor.facility,
                        FacilityKind::Dormitory | FacilityKind::MeetingRoom
                    )
            })
            .count();
        assert_eq!(support_count, 0, "已占支持房不能被虚构为空位");

        let receiver_only = operbox(&[("娜斯提", 2)]);
        assert_eq!(
            selected(
                &compile(&bp, &receiver_only, &BaseAssignment::default()),
                "rhine_receivers"
            ),
            None,
            "没有可用支持成员时 receiver 不得激活"
        );
    }

    #[test]
    fn competitive_receiver_backtracks_and_is_stable_across_room_order() {
        let a_blocked = room(
            "a-blocked",
            FacilityKind::Factory,
            2,
            Some(RoomProduct::Factory {
                recipe: RecipeKind::Gold,
            }),
        );
        let b_open = room(
            "b-open",
            FacilityKind::Factory,
            1,
            Some(RoomProduct::Factory {
                recipe: RecipeKind::Gold,
            }),
        );
        let dorm = room("dorm", FacilityKind::Dormitory, 5, None);
        let meeting = room("meeting", FacilityKind::MeetingRoom, 3, None);
        let forward = blueprint(vec![
            a_blocked.clone(),
            b_open.clone(),
            dorm.clone(),
            meeting.clone(),
        ]);
        let reversed = blueprint(vec![meeting, dorm, b_open, a_blocked]);
        let roster = operbox(&[("娜斯提", 2), ("塞雷娅", 0), ("麦哲伦", 0)]);
        let mut seed = BaseAssignment::default();
        seed.set_room(
            "a-blocked",
            vec![AssignedOperator::new("already-assigned", 0)],
        );

        let forward_plan = compile(&forward, &roster, &seed);
        let reversed_plan = compile(&reversed, &roster, &seed);
        assert_eq!(
            selected(&forward_plan, "rhine_receivers"),
            Some("nasti_actual_support")
        );
        assert_eq!(
            selected(&reversed_plan, "rhine_receivers"),
            Some("nasti_actual_support")
        );
        assert!(forward_plan.anchors.iter().any(|anchor| {
            anchor.operator == "娜斯提"
                && anchor
                    .room_id
                    .as_ref()
                    .is_some_and(|room| room.0 == "b-open")
        }));
        assert_eq!(forward_plan.selected, reversed_plan.selected);
        assert_eq!(forward_plan.anchors, reversed_plan.anchors);
        assert!(forward_plan.activated.iter().any(|system| {
            system.system_id == "rhine_receivers" && system.tier == OperatorTier::CrossStation
        }));
    }

    #[test]
    fn rhine_muelsyse_receiver_uses_actual_support_rooms() {
        let bp = blueprint(vec![
            room("power", FacilityKind::PowerPlant, 3, None),
            room("dorm", FacilityKind::Dormitory, 5, None),
            room("meeting", FacilityKind::MeetingRoom, 3, None),
            room("workshop", FacilityKind::Workshop, 3, None),
        ]);
        let roster = operbox(&[("缪尔赛思", 2), ("塞雷娅", 0), ("麦哲伦", 0), ("赫默", 0)]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());

        assert_eq!(
            selected(&compiled, "rhine_receivers"),
            Some("muelsyse_actual_support")
        );
        let anchors: Vec<_> = compiled
            .anchors
            .iter()
            .filter(|anchor| anchor.system_id == "rhine_receivers")
            .collect();
        assert_eq!(anchors.len(), 3, "receiver plus two eligible support rooms");
        assert!(anchors.iter().any(|anchor| {
            anchor.operator == "缪尔赛思" && anchor.facility == FacilityKind::PowerPlant
        }));
        let dependency = compiled
            .active_dependencies
            .iter()
            .find(|dependency| dependency.consumers.iter().any(|name| name == "缪尔赛思"))
            .unwrap();
        assert_eq!(dependency.required.len(), 2);
        assert!(dependency
            .required
            .iter()
            .all(|name| { ["塞雷娅", "麦哲伦", "赫默"].contains(&name.as_str()) }));
    }

    #[test]
    fn rhine_dual_receivers_share_actual_support() {
        let bp = blueprint(vec![
            room(
                "gold",
                FacilityKind::Factory,
                1,
                Some(RoomProduct::Factory {
                    recipe: RecipeKind::Gold,
                }),
            ),
            room("power", FacilityKind::PowerPlant, 3, None),
            room("dorm", FacilityKind::Dormitory, 5, None),
            room("meeting", FacilityKind::MeetingRoom, 3, None),
            room("workshop", FacilityKind::Workshop, 3, None),
            room("training", FacilityKind::TrainingRoom, 3, None),
        ]);
        let roster = operbox(&[
            ("娜斯提", 2),
            ("缪尔赛思", 2),
            ("塞雷娅", 0),
            ("麦哲伦", 0),
            ("赫默", 0),
            ("白面鸮", 0),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());

        assert_eq!(
            selected(&compiled, "rhine_receivers"),
            Some("nasti_muelsyse_actual_support")
        );
        let dependency = compiled
            .active_dependencies
            .iter()
            .find(|dependency| {
                dependency.consumers.iter().any(|name| name == "娜斯提")
                    && dependency.consumers.iter().any(|name| name == "缪尔赛思")
            })
            .expect("双 receiver 应共享实际支持 dependency");
        assert_eq!(dependency.required.len(), 3);
        assert!(dependency.required.iter().all(|name| {
            ["塞雷娅", "麦哲伦", "赫默", "白面鸮"].contains(&name.as_str())
        }));
    }

    #[test]
    fn every_compiled_anchor_has_a_resolved_room() {
        let bp = rosemary_blueprint();
        let roster = operbox(&[
            ("可露希尔", 2),
            ("迷迭香", 2),
            ("黑键", 2),
            ("八幡海铃", 2),
            ("夕", 0),
        ]);
        let compiled = compile(&bp, &roster, &BaseAssignment::default());
        assert!(!compiled.anchors.is_empty());
        assert!(compiled
            .anchors
            .iter()
            .all(|anchor| anchor.room_id.is_some()));
    }
}
