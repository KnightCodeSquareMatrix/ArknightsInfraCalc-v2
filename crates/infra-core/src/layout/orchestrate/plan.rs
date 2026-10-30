//! 编排计划：声明式描述「启用哪些体系、各 slot 如何填」，不含 trade/manu solve 评分。

use crate::layout::blueprint::{FacilityKind, RoomId};
use crate::layout::shift::AssignShiftMode;
use crate::layout::system::{RegistrySystemClaim, SlotFillMode};
use crate::layout::tier::OperatorTier;
use crate::trade::input::TradeOrderKind;
use crate::types::RecipeKind;
use std::collections::HashSet;

/// 兼容 registry claim 的单个 slot 落位方式；声明式 Rule 直接产出 `SystemAnchor`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotFill {
    Fixed {
        operator: String,
        elite: u8,
        facility: FacilityKind,
        room_id: Option<RoomId>,
    },
    PickOne {
        candidates: Vec<String>,
        elite: u8,
        facility: FacilityKind,
        room_id: Option<RoomId>,
    },
    /// 可选 producer：缺房 / 缺人时 execute 静默跳过。
    OptionalFixed {
        operator: String,
        elite: u8,
        facility: FacilityKind,
    },
}

/// 已选中的成套体系（来自 `base_systems` 认领）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActivatedSystem {
    pub system_id: String,
    pub priority: i32,
    pub tier: OperatorTier,
    pub slots: Vec<SlotFill>,
}

// ── 当前声明式体系语义类型 ─────────────────────────────────────────────

/// anchor 填充策略：决定 fill 阶段如何补齐 anchor 房的队友。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnchorFillPolicy {
    /// 贸易 anchor：走 trade role / L3 shortcut 补齐。
    TradeRole { role_id: Option<String> },
    /// 制造 anchor：按 recipe 约束搜索补齐。
    ManufactureRecipe,
    /// 普通搜索补齐（must_include = 核心）。
    Plain,
}

/// 体系锚点：钉核心干员与已解析设施/房间，队友由 fill 阶段补齐。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SystemAnchor {
    pub system_id: String,
    pub operator: String,
    #[serde(skip)]
    pub(crate) proactive_support: bool,
    /// Recommendation provenance used only for bounded Plan retry when its required trade fill fails.
    #[serde(skip)]
    pub(crate) optional_trade_role: Option<String>,
    pub elite: u8,
    pub facility: FacilityKind,
    /// `None` = 该设施类型首个空房，不绑定具体 `room_id`。
    pub room_id: Option<RoomId>,
    /// 制造 anchor 的配方约束；非制造设施为 `None`。
    pub recipe: Option<RecipeKind>,
    /// 贸易 anchor 的订单约束；非贸易设施为 `None`。
    pub trade_order: Option<TradeOrderKind>,
    /// 单个干员的特殊工作心情；例如 Lancet-2 红脸上班。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_mood: Option<u8>,
    /// 该绑定组休息时的非宿舍去向；由轮换导出通用消费。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_facility: Option<FacilityKind>,
    pub fill_policy: AnchorFillPolicy,
}

/// 通用规则编译器已经选定的有限方案。后续阶段只消费结果，不再重跑体系选型。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SelectedRuleAlternative {
    pub rule_id: String,
    pub alternative_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_pack_id: Option<String>,
    pub priority: i32,
    pub operators: Vec<String>,
}

/// 只提供全局 / 跨设施资源的 producer slot（蓝本 `OptionalProducer`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProducerSlot {
    pub system_id: String,
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    /// 缺人时裁剪，不拖死核心。
    pub optional: bool,
}

/// 不占房但影响补齐搜索的约束（补 pairwise；现仅有系统级 `exclusive_group`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemConstraint {
    /// 两名干员禁止同房（迷迭香 ↔ 清流+温蒂）。
    ForbidSameRoom { a: String, b: String },
    /// 两名干员禁止同贸易站（黑键 ↔ 巫恋）。
    ForbidSameStation { a: String, b: String },
    /// 体系要求蓝图存在某设施。
    RequireFacility { facility: FacilityKind },
}

/// 降级阶梯结果（蓝本 `RosemaryTier` + `producers_present/missing`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DegradationLadder {
    pub system_id: String,
    /// 满配 / 档1 / 档2 / 档3 / 替代感知源。
    pub tier_label: String,
    /// priority-by-tier：高档保持高优先，低档降至核心体系之下。
    pub priority: i32,
    pub producers_present: Vec<String>,
    pub producers_missing: Vec<String>,
}

/// exact 同班绑定：plan 供轮换消费，不承担进编责任。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ShiftBind {
    pub operators: Vec<String>,
    /// 0 表示只绑定同队，不声明固定工作班次数。
    pub on_shifts: u8,
    /// 0 表示不声明固定休息班次数。
    pub off_shifts: u8,
}

/// 单向在岗依赖：任一 consumer 在岗时，实际入选的 required 成员必须在基建内。
/// 当前轮换器把 required 作为全周期共享岗位；consumer 休息时不反向要求 required 离岗。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActiveDependency {
    pub consumers: Vec<String>,
    pub required: Vec<String>,
}

/// 需要在所有可持续工作班次优先回岗的已解析角色；轮换层只负责通用恢复策略。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContinuousRole {
    pub operator: String,
    pub facility: FacilityKind,
    pub room_id: RoomId,
}

/// 已解析 reserve 在相邻 gamma half 中的复用策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReserveReusePolicy {
    /// 只在按 H1、H2 顺序遇到的首个合法 half 中使用一次。
    #[default]
    Once,
    /// H1、H2 都必须有合法目标，并在两边复用同一组实际成员。
    EveryEligibleHalf,
}

/// 由正式 role/solver 解析但不进入 peak 的轮换 cohort；peak fill 必须预留，rotation
/// 在声明的目标房间中复用同一组实际成员。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedRoleReserve {
    pub system_id: String,
    pub reserve_id: String,
    pub role_id: String,
    pub facility: FacilityKind,
    pub operators: Vec<String>,
    pub eligible_rooms: Vec<RoomId>,
    pub reuse_policy: ReserveReusePolicy,
    /// 该 reserve 只能在既有 exact bind 的自然 H1/H2 打包结果中两边都有目标时成立。
    /// 计划编译器在提交 conditional pack 前消费此门禁；schedule 不负责事后改写选型。
    pub require_pre_split_halves: bool,
}

/// 按 exact bind 连通分量打包生产房，作为计划可行性与 schedule 切半的共同事实源。
/// 含贸易站的闭包先按贸易站数均衡分入两半，再按总房间负载决胜；纯制造/发电
/// 闭包随后按总房间负载均衡。reserve 尚未参与此过程；需要两半复用的 conditional
/// pack 必须先证明自然打包结果已经在 H1/H2 各有一个合法目标，不能依靠 schedule
/// 事后搬动分量来挽救选型。
pub(crate) fn pack_production_components<'a>(
    blueprint: &crate::layout::BaseBlueprint,
    assignment: &crate::layout::BaseAssignment,
    binds: impl IntoIterator<Item = &'a [String]>,
) -> Result<[Vec<Vec<RoomId>>; 2], String> {
    let production_rooms: Vec<RoomId> = blueprint
        .rooms
        .iter()
        .filter(|room| {
            matches!(
                room.kind,
                FacilityKind::TradePost | FacilityKind::Factory | FacilityKind::PowerPlant
            )
        })
        .map(|room| room.id.clone())
        .collect();
    let mut components: Vec<Vec<RoomId>> = production_rooms
        .iter()
        .cloned()
        .map(|room| vec![room])
        .collect();

    for operators in binds {
        let mut bound_rooms = Vec::new();
        for name in operators {
            let room = assignment
                .iter_rooms()
                .find(|room| room.operators.iter().any(|operator| &operator.name == name))
                .ok_or_else(|| format!("shift bind operator {name} missing from peak"))?;
            if production_rooms.contains(&room.room_id) && !bound_rooms.contains(&room.room_id) {
                bound_rooms.push(room.room_id.clone());
            }
        }
        if bound_rooms.len() < 2 {
            continue;
        }
        let mut merged = Vec::new();
        components.retain(|component| {
            if component.iter().any(|room| bound_rooms.contains(room)) {
                merged.extend(component.iter().cloned());
                false
            } else {
                true
            }
        });
        merged.sort_by(|left, right| left.0.cmp(&right.0));
        merged.dedup();
        components.push(merged);
    }

    let trade_room_count = |component: &[RoomId]| {
        component
            .iter()
            .filter(|room_id| {
                blueprint
                    .room(room_id)
                    .is_some_and(|room| room.kind == FacilityKind::TradePost)
            })
            .count()
    };
    let component_key = |component: &[RoomId]| {
        component
            .iter()
            .map(|room_id| room_id.0.clone())
            .collect::<Vec<_>>()
    };
    components.sort_by(|left, right| {
        trade_room_count(right)
            .cmp(&trade_room_count(left))
            .then_with(|| right.len().cmp(&left.len()))
            .then_with(|| component_key(left).cmp(&component_key(right)))
    });
    let mut halves: [Vec<Vec<RoomId>>; 2] = Default::default();
    for component in components {
        let load = |items: &[Vec<RoomId>]| items.iter().map(Vec::len).sum::<usize>();
        let trade_load = |items: &[Vec<RoomId>]| {
            items
                .iter()
                .map(|component| trade_room_count(component))
                .sum::<usize>()
        };
        let target = if trade_room_count(&component) > 0 {
            let left_trade = trade_load(&halves[0]);
            let right_trade = trade_load(&halves[1]);
            if left_trade == right_trade {
                usize::from(load(&halves[1]) < load(&halves[0]))
            } else {
                usize::from(right_trade < left_trade)
            }
        } else {
            usize::from(load(&halves[1]) < load(&halves[0]))
        };
        halves[target].push(component);
    }
    Ok(halves)
}

/// 中枢搜索候选组约束：由 solver 在候选中至少选择 `min_count` 人。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ControlCandidateRequirement {
    pub candidates: Vec<String>,
    pub min_count: u8,
}

/// 单班进驻前的编排计划；`execute_plan` 将其落成 `BaseAssignment`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AssignmentPlan {
    #[serde(skip)]
    pub(crate) support_combinations_enable: bool,
    pub mode: AssignShiftMode,
    /// 计划阶段已确定的规则 alternatives 与兼容 registry claims 的统一视图。
    pub activated: Vec<ActivatedSystem>,
    /// `base_systems.json` 认领明细（`execute_plan` 落位用）。
    pub registry_claims: Vec<RegistrySystemClaim>,
    /// 声明式规则编译结果；一条 rule 最多选择一个 finite alternative。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_rules: Vec<SelectedRuleAlternative>,
    /// 规则编译器与兼容 registry 汇合后的实际 required placements；room 已解析。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<SystemAnchor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producers: Vec<ProducerSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<SystemConstraint>,
    /// 已选 alternative 明确排除的干员；fill / rotation 不得从普通候选重新引入。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_operators: Vec<String>,
    /// 只作用于中枢普通补位的路径 admission；不占用其它设施的候选资格。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_exclusions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradations: Vec<DegradationLadder>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shift_binds: Vec<ShiftBind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_dependencies: Vec<ActiveDependency>,
    /// hard Plan 与 winner 实际贡献规范化后的统一排班依赖输出。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_producer_dependencies: Vec<crate::response_dependency::ResolvedProducerDependency>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continuous_roles: Vec<ContinuousRole>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rotation_reserves: Vec<ResolvedRoleReserve>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_candidate_requirements: Vec<ControlCandidateRequirement>,
}

impl AssignmentPlan {
    /// 空 Peak 计划：无体系、无 anchor、无约束。
    pub(crate) fn empty() -> Self {
        Self {
            support_combinations_enable: false,
            mode: AssignShiftMode::Peak,
            activated: Vec::new(),
            registry_claims: Vec::new(),
            selected_rules: Vec::new(),
            anchors: Vec::new(),
            producers: Vec::new(),
            constraints: Vec::new(),
            excluded_operators: Vec::new(),
            control_exclusions: Vec::new(),
            degradations: Vec::new(),
            shift_binds: Vec::new(),
            active_dependencies: Vec::new(),
            resolved_producer_dependencies: Vec::new(),
            continuous_roles: Vec::new(),
            rotation_reserves: Vec::new(),
            control_candidate_requirements: Vec::new(),
        }
    }

    /// Recover exact provenance from the retained, ordered registry claims.
    /// A Search group is never matched by the members chosen in a final shift.
    pub(crate) fn control_requirement_sources(&self) -> crate::error::Result<Vec<(usize, usize)>> {
        let sources: Vec<_> = self
            .registry_claims
            .iter()
            .enumerate()
            .flat_map(|(claim_index, claim)| {
                claim
                    .slots
                    .iter()
                    .enumerate()
                    .filter_map(move |(slot_index, slot)| {
                        (slot.facility == FacilityKind::ControlCenter
                            && slot.fill == crate::layout::system::SlotFillMode::Search
                            && slot.required_count > 0)
                            .then_some((claim_index, slot_index))
                    })
            })
            .collect();
        if sources.len() != self.control_candidate_requirements.len() {
            return Err(crate::error::Error::msg(
                "control_requirement_scope_unresolved: missing registry provenance",
            ));
        }
        for ((claim_index, slot_index), requirement) in
            sources.iter().zip(&self.control_candidate_requirements)
        {
            let slot = &self.registry_claims[*claim_index].slots[*slot_index];
            let candidates: Vec<_> = slot.operators.iter().map(|op| op.name.clone()).collect();
            if candidates != requirement.candidates || slot.required_count != requirement.min_count
            {
                return Err(crate::error::Error::msg(format!(
                    "control_requirement_scope_unresolved: changed group in {}/slot {}",
                    self.registry_claims[*claim_index].system_id, slot_index
                )));
            }
        }
        Ok(sources)
    }

    /// 把声明式 hard 关系与 winner 的实际 producer/consumer 统一为排班事实。
    pub fn resolve_actual_producer_dependencies(
        &mut self,
        blueprint: &crate::layout::BaseBlueprint,
        assignment: &crate::layout::BaseAssignment,
        instances: &crate::instances::OperatorInstances,
        table: &crate::skill_table::SkillTable,
    ) -> crate::error::Result<()> {
        use crate::response_dependency::{
            resolve_assignment_producer_dependencies, ResolvedProducerDependency,
            ScheduleDependencyRelation,
        };

        self.derive_registry_binds_from_actual_assignment(assignment);
        let room_ids_for = |names: &[String]| {
            let mut rooms: Vec<_> = assignment
                .iter_rooms()
                .filter(|room| {
                    room.operators
                        .iter()
                        .any(|operator| names.contains(&operator.name))
                })
                .map(|room| room.room_id.clone())
                .collect();
            rooms.sort_by(|left, right| left.0.cmp(&right.0));
            rooms.dedup();
            rooms
        };
        let source_rule_for = |members: &[String]| {
            self.selected_rules
                .iter()
                .find(|selected| {
                    members
                        .iter()
                        .all(|member| selected.operators.contains(member))
                })
                .map(|selected| selected.rule_id.clone())
                .or_else(|| {
                    self.registry_claims
                        .iter()
                        .find(|claim| {
                            let operators: HashSet<_> = claim
                                .slots
                                .iter()
                                .flat_map(|slot| &slot.operators)
                                .map(|operator| operator.name.as_str())
                                .collect();
                            members
                                .iter()
                                .all(|member| operators.contains(member.as_str()))
                        })
                        .map(|claim| claim.system_id.clone())
                })
                .or_else(|| {
                    self.activated
                        .iter()
                        .find(|system| {
                            let operators: HashSet<_> = system
                                .slots
                                .iter()
                                .flat_map(|slot| match slot {
                                    SlotFill::Fixed { operator, .. }
                                    | SlotFill::OptionalFixed { operator, .. } => {
                                        vec![operator.as_str()]
                                    }
                                    SlotFill::PickOne { candidates, .. } => {
                                        candidates.iter().map(String::as_str).collect()
                                    }
                                })
                                .collect();
                            members
                                .iter()
                                .all(|member| operators.contains(member.as_str()))
                        })
                        .map(|system| system.system_id.clone())
                })
                .unwrap_or_else(|| "plan".to_string())
        };

        let mut dependencies = Vec::new();
        for bind in &self.shift_binds {
            dependencies.push(ResolvedProducerDependency {
                rule_id: source_rule_for(&bind.operators),
                source_buff_id: String::new(),
                producers: Vec::new(),
                consumers: bind.operators.clone(),
                target_facility: "multiple".to_string(),
                target_rooms: room_ids_for(&bind.operators),
                relation: ScheduleDependencyRelation::ExactPresence,
                on_shifts: bind.on_shifts,
                off_shifts: bind.off_shifts,
                effective_contribution: None,
            });
        }
        for dependency in &self.active_dependencies {
            let mut members = dependency.consumers.clone();
            members.extend(dependency.required.iter().cloned());
            dependencies.push(ResolvedProducerDependency {
                rule_id: source_rule_for(&members),
                source_buff_id: String::new(),
                producers: dependency.required.clone(),
                consumers: dependency.consumers.clone(),
                target_facility: "multiple".to_string(),
                target_rooms: room_ids_for(&members),
                relation: ScheduleDependencyRelation::RequiresPresence,
                on_shifts: 0,
                off_shifts: 0,
                effective_contribution: None,
            });
        }

        let dynamic =
            resolve_assignment_producer_dependencies(blueprint, assignment, instances, table)?;
        for dependency in &dynamic {
            if dependency.relation != ScheduleDependencyRelation::ExactPresence {
                continue;
            }
            let mut operators = dependency.producers.clone();
            operators.extend(dependency.consumers.iter().cloned());
            operators.sort();
            operators.dedup();
            if operators.len() > 1
                && !self.shift_binds.iter().any(|bind| {
                    operators
                        .iter()
                        .all(|operator| bind.operators.contains(operator))
                })
            {
                self.shift_binds.push(ShiftBind {
                    operators,
                    on_shifts: dependency.on_shifts,
                    off_shifts: dependency.off_shifts,
                });
            }
        }
        dependencies.extend(dynamic);
        dependencies.sort_by(|left, right| {
            left.rule_id
                .cmp(&right.rule_id)
                .then_with(|| left.producers.cmp(&right.producers))
                .then_with(|| left.consumers.cmp(&right.consumers))
        });
        self.resolved_producer_dependencies = dependencies;
        Ok(())
    }

    fn derive_registry_binds_from_actual_assignment(
        &mut self,
        assignment: &crate::layout::BaseAssignment,
    ) {
        let actual = assignment.operator_names();
        for claim in self.registry_claims.iter().filter(|claim| claim.bind_all) {
            let mut operators: Vec<_> = claim
                .slots
                .iter()
                .flat_map(|slot| &slot.operators)
                .map(|operator| operator.name.clone())
                .filter(|name| actual.contains(name))
                .collect();
            operators.sort();
            operators.dedup();
            if operators.len() < 2
                || self.shift_binds.iter().any(|bind| {
                    bind.operators.len() == operators.len()
                        && operators
                            .iter()
                            .all(|operator| bind.operators.contains(operator))
                })
            {
                continue;
            }
            self.shift_binds.push(ShiftBind {
                operators,
                on_shifts: claim.on_shifts,
                off_shifts: claim.off_shifts,
            });
        }
    }

    /// 已选 `base_systems` 体系 id（按 priority 降序）。
    pub fn registry_system_ids(&self) -> Vec<&str> {
        let mut claims: Vec<_> = self.registry_claims.iter().collect();
        claims.sort_by_key(|c| std::cmp::Reverse(c.priority));
        claims.into_iter().map(|c| c.system_id.as_str()).collect()
    }

    /// 编排认领的贸易站房间（meta 锚点；余站由 `assign_trade_remainder` 填）。
    pub fn registry_trade_room_ids(&self) -> HashSet<RoomId> {
        self.registry_claims
            .iter()
            .flat_map(|c| &c.slots)
            .filter(|s| s.facility == FacilityKind::TradePost)
            .map(|s| s.room_id.clone())
            .collect()
    }

    /// 编排认领的全部干员名（registry fixed/pick 落位结果）。
    pub fn registry_operator_names(&self) -> HashSet<String> {
        self.registry_claims
            .iter()
            .flat_map(|c| &c.slots)
            .flat_map(|s| s.operators.iter().map(|o| o.name.clone()))
            .collect()
    }

    /// peak 切半后，registry 贸易 meta 房间与干员应落在 α 或 β（不在 γ 替补池）。
    pub fn verify_registry_trade_in_alpha_beta(
        &self,
        alpha: &crate::layout::BaseAssignment,
        beta: &crate::layout::BaseAssignment,
    ) -> Result<(), String> {
        for claim in &self.registry_claims {
            for slot in &claim.slots {
                if slot.facility != FacilityKind::TradePost {
                    continue;
                }
                if slot.fill == SlotFillMode::Search {
                    // Search slots declare eligible cross-station members, not required anchors.
                    // Only operators selected into peak are expected to appear in an alpha/beta
                    // slice; absence is valid and must not fail rotation verification.
                    continue;
                }
                let room = &slot.room_id;
                let alpha_ops = alpha.operators_in(room);
                let beta_ops = beta.operators_in(room);
                if alpha_ops.is_empty() && beta_ops.is_empty() {
                    return Err(format!(
                        "registry trade {} @ {} missing from α/β",
                        claim.system_id, room.0
                    ));
                }
                for op in &slot.operators {
                    let in_ab = alpha_ops.iter().any(|a| a.name == op.name)
                        || beta_ops.iter().any(|a| a.name == op.name);
                    if !in_ab {
                        return Err(format!(
                            "registry operator {} @ {} not in α/β slice",
                            op.name, room.0
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn registry_as_activated(claim: &RegistrySystemClaim) -> ActivatedSystem {
    let slots = claim
        .slots
        .iter()
        .filter(|slot| slot.fill != SlotFillMode::Search)
        .flat_map(|slot| {
            slot.operators.iter().map(|op| SlotFill::Fixed {
                operator: op.name.clone(),
                elite: op.elite,
                facility: slot.facility,
                room_id: Some(slot.room_id.clone()),
            })
        })
        .collect();
    ActivatedSystem {
        system_id: claim.system_id.clone(),
        priority: claim.priority,
        tier: claim.tier,
        slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{AssignedOperator, BaseAssignment, BaseBlueprint};

    fn set_trade_group(assignment: &mut BaseAssignment, names: &[&str]) {
        assignment.set_room(
            "trade_1",
            names
                .iter()
                .map(|name| AssignedOperator::new(*name, 2))
                .collect(),
        );
    }

    fn resolve(plan: &mut AssignmentPlan, blueprint: &BaseBlueprint, assignment: &BaseAssignment) {
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().unwrap(),
        )
        .unwrap();
        let table = crate::skill_table::SkillTable::load(
            &crate::skill_table::default_skill_table_path().unwrap(),
        )
        .unwrap();
        plan.resolve_actual_producer_dependencies(blueprint, assignment, &instances, &table)
            .unwrap();
    }

    #[test]
    fn resolved_dynamic_trade_dependencies_follow_buff_selectors() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("八幡海铃", 2)]);
        set_trade_group(&mut assignment, &["伺夜"]);
        let mut single = AssignmentPlan::empty();
        resolve(&mut single, &blueprint, &assignment);
        assert_eq!(single.shift_binds.len(), 1);
        assert_eq!(single.resolved_producer_dependencies.len(), 1);
        let dependency = &single.resolved_producer_dependencies[0];
        assert_eq!(dependency.source_buff_id, "control_tra_limit&spd2[000]");
        assert_eq!(dependency.producers, vec!["八幡海铃"]);
        assert_eq!(dependency.consumers, vec!["伺夜"]);

        assignment.set_room("trade_2", vec![AssignedOperator::new("贝洛内", 2)]);
        let mut cross_station = AssignmentPlan::empty();
        resolve(&mut cross_station, &blueprint, &assignment);
        assert_eq!(cross_station.shift_binds.len(), 1);
        for name in ["八幡海铃", "伺夜", "贝洛内"] {
            assert!(cross_station.shift_binds[0]
                .operators
                .contains(&name.to_string()));
        }
    }

    #[test]
    fn excluded_threshold_producer_does_not_create_dependency() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("凛御银灰", 2)]);
        set_trade_group(&mut assignment, &["银灰", "崖心", "讯使"]);
        assignment.set_room("trade_2", vec![AssignedOperator::new("角峰", 2)]);

        let mut plan = AssignmentPlan::empty();
        resolve(&mut plan, &blueprint, &assignment);

        assert!(plan.resolved_producer_dependencies.is_empty());
    }

    #[test]
    fn natural_blacksteel_response_is_explained_without_exact_bind() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("涤火杰西卡", 2)]);
        assignment.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("水月", 2),
                AssignedOperator::new("香草", 2),
                AssignedOperator::new("杰西卡", 2),
            ],
        );

        let mut plan = AssignmentPlan::empty();
        resolve(&mut plan, &blueprint, &assignment);

        assert!(plan.shift_binds.is_empty());
        let dependency = plan
            .resolved_producer_dependencies
            .iter()
            .find(|dependency| dependency.source_buff_id == "control_bd_spd[000]")
            .unwrap();
        assert_eq!(
            dependency.relation,
            crate::response_dependency::ScheduleDependencyRelation::None
        );
        assert!(!dependency.consumers.is_empty());
    }

    #[test]
    fn declared_plan_bind_is_normalized_without_name_inference() {
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = BaseAssignment::default();
        assignment.set_room("control", vec![AssignedOperator::new("规则成员A", 2)]);
        set_trade_group(&mut assignment, &["规则成员B"]);
        let mut plan = AssignmentPlan::empty();
        plan.shift_binds.push(ShiftBind {
            operators: vec!["规则成员A".to_string(), "规则成员B".to_string()],
            on_shifts: 2,
            off_shifts: 1,
        });
        resolve(&mut plan, &blueprint, &assignment);
        assert_eq!(plan.resolved_producer_dependencies.len(), 1);
        assert_eq!(plan.resolved_producer_dependencies[0].rule_id, "plan");
    }
}
