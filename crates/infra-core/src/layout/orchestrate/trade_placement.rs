//! Plan-owned legal room variants. Default role ordering is a search order,
//! while explicit seed/registry placements and role geometry remain constraints.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::layout::{BaseAssignment, BaseBlueprint, FacilityKind, RoomId};

use super::rules::{role_matches_room, roles_with_conditional_pack, RuleCatalog};
use super::{AssignmentPlan, SlotFill, SystemConstraint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TradePlacementDomain {
    pub operators: Vec<String>,
    pub rooms: Vec<RoomId>,
    pub distinct_group: Option<String>,
}

pub(super) fn retain_trade_domains(
    catalog: &RuleCatalog,
    blueprint: &BaseBlueprint,
    plan: &mut AssignmentPlan,
) {
    let mut groups: BTreeMap<String, TradePlacementDomain> = BTreeMap::new();
    for selected in &plan.selected_rules {
        let Some(alternative) = catalog
            .rules
            .iter()
            .find(|rule| rule.id == selected.rule_id)
            .and_then(|rule| {
                rule.alternatives
                    .iter()
                    .find(|alternative| alternative.id == selected.alternative_id)
            })
        else {
            continue;
        };
        let roles = selected
            .conditional_pack_id
            .as_ref()
            .and_then(|id| {
                alternative
                    .conditional_packs
                    .iter()
                    .find(|pack| &pack.id == id)
            })
            .map_or_else(
                || alternative.roles.clone(),
                |pack| roles_with_conditional_pack(&alternative.roles, pack),
            );
        let declared: HashSet<_> = roles
            .iter()
            .flat_map(|role| role.candidates.iter().map(|candidate| &candidate.name))
            .collect();
        for role in roles
            .iter()
            .filter(|role| role.facility == FacilityKind::TradePost)
        {
            let core: Vec<_> = plan
                .anchors
                .iter()
                .filter(|anchor| {
                    anchor.system_id == selected.rule_id
                        && anchor.facility == FacilityKind::TradePost
                        && role
                            .candidates
                            .iter()
                            .any(|candidate| candidate.name == anchor.operator)
                })
                .collect();
            if core.is_empty() {
                continue;
            }
            let mut members: Vec<_> = core.iter().map(|anchor| anchor.operator.clone()).collect();
            if role.fill_to_capacity || role.support_receiver {
                members.extend(
                    plan.anchors
                        .iter()
                        .filter(|anchor| {
                            anchor.system_id == selected.rule_id
                                && anchor.facility == FacilityKind::TradePost
                                && !declared.contains(&anchor.operator)
                                && core
                                    .iter()
                                    .any(|consumer| consumer.room_id == anchor.room_id)
                        })
                        .map(|anchor| anchor.operator.clone()),
                );
            }
            let rooms: Vec<_> = blueprint
                .rooms
                .iter()
                .filter(|room| {
                    room.kind == FacilityKind::TradePost && role_matches_room(role, room)
                })
                .map(|room| room.id.clone())
                .collect();
            let shared = role.room_group.as_ref().cloned().or_else(|| {
                (role.same_room || role.fill_to_capacity || role.support_receiver)
                    .then(|| role.id.clone())
            });
            for member in members {
                let key = format!(
                    "{}:{}",
                    selected.rule_id,
                    shared.as_ref().unwrap_or(&member)
                );
                let group = groups.entry(key).or_insert_with(|| TradePlacementDomain {
                    operators: Vec::new(),
                    rooms: rooms.clone(),
                    distinct_group: role
                        .one_per_room
                        .then(|| format!("{}:{}", selected.rule_id, role.id)),
                });
                group.rooms.retain(|room| rooms.contains(room));
                if !group.operators.contains(&member) {
                    group.operators.push(member);
                }
            }
        }
    }
    plan.trade_placement_domains = groups.into_values().collect();
}

impl AssignmentPlan {
    pub(crate) fn trade_placement_variants(
        &self,
        blueprint: &BaseBlueprint,
        seed: &BaseAssignment,
    ) -> Vec<Self> {
        if self.auto_rotation
            || self.mode != crate::layout::AssignShiftMode::Peak
            || self.trade_placement_domains.is_empty()
        {
            return vec![self.clone()];
        }
        let mut domains = self.trade_placement_domains.clone();
        let seed_names = seed.operator_names();
        for domain in &mut domains {
            domain.operators.retain(|name| {
                self.anchors.iter().any(|anchor| {
                    &anchor.operator == name && anchor.facility == FacilityKind::TradePost
                })
            });
            let original = domain
                .operators
                .first()
                .and_then(|name| self.anchors.iter().find(|anchor| &anchor.operator == name))
                .and_then(|anchor| anchor.room_id.as_ref());
            if domain
                .operators
                .iter()
                .any(|name| seed_names.contains(name))
            {
                domain.rooms.retain(|room| Some(room) == original);
            }
            domain
                .rooms
                .sort_by_key(|room| (Some(room) != original, room.0.clone()));
        }
        domains.retain(|domain| !domain.operators.is_empty());
        let moving: HashSet<_> = domains
            .iter()
            .flat_map(|domain| domain.operators.iter().cloned())
            .collect();
        let mut occupants: HashMap<RoomId, Vec<String>> = blueprint
            .rooms
            .iter()
            .filter(|room| room.kind == FacilityKind::TradePost)
            .map(|room| {
                (
                    room.id.clone(),
                    seed.operators_in(&room.id)
                        .iter()
                        .filter(|op| !moving.contains(&op.name))
                        .map(|op| op.name.clone())
                        .collect(),
                )
            })
            .collect();
        for anchor in self.anchors.iter().filter(|anchor| {
            anchor.facility == FacilityKind::TradePost && !moving.contains(&anchor.operator)
        }) {
            if let Some(room) = &anchor.room_id {
                let names = occupants.entry(room.clone()).or_default();
                if !names.contains(&anchor.operator) {
                    names.push(anchor.operator.clone());
                }
            }
        }
        let mut variants = Vec::new();
        enumerate(
            self,
            blueprint,
            &domains,
            0,
            &mut occupants,
            &mut HashMap::new(),
            &mut variants,
        );
        if variants.is_empty() {
            vec![self.clone()]
        } else {
            variants
        }
    }
}

fn enumerate(
    plan: &AssignmentPlan,
    blueprint: &BaseBlueprint,
    domains: &[TradePlacementDomain],
    index: usize,
    occupants: &mut HashMap<RoomId, Vec<String>>,
    placed: &mut HashMap<String, RoomId>,
    out: &mut Vec<AssignmentPlan>,
) {
    if index == domains.len() {
        let mut variant = plan.clone();
        for anchor in &mut variant.anchors {
            if let Some(room) = placed.get(&anchor.operator) {
                anchor.room_id = Some(room.clone());
            }
        }
        for role in &mut variant.continuous_roles {
            if let Some(room) = placed.get(&role.operator) {
                role.room_id = room.clone();
            }
        }
        for system in &mut variant.activated {
            for slot in &mut system.slots {
                if let SlotFill::Fixed {
                    operator,
                    room_id,
                    facility,
                    ..
                } = slot
                {
                    if *facility == FacilityKind::TradePost {
                        if let Some(room) = placed.get(operator) {
                            *room_id = Some(room.clone());
                        }
                    }
                }
            }
        }
        out.push(variant);
        return;
    }
    let domain = &domains[index];
    for room in &domain.rooms {
        let names = occupants.get(room).cloned().unwrap_or_default();
        if blueprint
            .room(room)
            .is_none_or(|spec| names.len() + domain.operators.len() > spec.operator_capacity())
        {
            continue;
        }
        if domain.distinct_group.as_ref().is_some_and(|group| {
            domains[..index].iter().any(|other| {
                other.distinct_group.as_ref() == Some(group)
                    && other.operators.first().and_then(|name| placed.get(name)) == Some(room)
            })
        }) {
            continue;
        }
        let mut combined = names.clone();
        combined.extend(domain.operators.iter().cloned());
        if plan.constraints.iter().any(|constraint| match constraint {
            SystemConstraint::ForbidSameRoom { a, b }
            | SystemConstraint::ForbidSameStation { a, b } => {
                combined.contains(a) && combined.contains(b)
            }
            _ => false,
        }) {
            continue;
        }
        occupants.insert(room.clone(), combined);
        for name in &domain.operators {
            placed.insert(name.clone(), room.clone());
        }
        enumerate(plan, blueprint, domains, index + 1, occupants, placed, out);
        for name in &domain.operators {
            placed.remove(name);
        }
        occupants.insert(room.clone(), names);
    }
}
