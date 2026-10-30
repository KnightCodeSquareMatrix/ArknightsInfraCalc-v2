use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::global_resource::CONVERSIONS;
use crate::instances::OperatorInstances;
use crate::layout::{BaseAssignment, BaseBlueprint, FacilityKind, RoomId};
use crate::skill_table::SkillTable;
use crate::types::{Action, AtomScope, Condition, Selector};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyScope {
    RoomLocal,
    SameFacility,
    CrossFacility,
    GlobalLayout,
    RuntimeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseField {
    Efficiency,
    LimitOrStorage,
    UnitOutput,
    Mood,
    StateResource,
    GlobalInject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerAdmission {
    DeferredOptional,
    ExcludedAutomatic,
    PlanRequired,
    NormalObservation,
    PolicyManaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleDependencyRelation {
    ExactPresence,
    RequiresPresence,
    None,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProducerRule {
    pub id: String,
    pub source_buff_id: String,
    pub admission: ProducerAdmission,
    pub target_facility: String,
    pub schedule_relation: ScheduleDependencyRelation,
    #[serde(default = "default_on_shifts")]
    pub on_shifts: u8,
    #[serde(default = "default_off_shifts")]
    pub off_shifts: u8,
}

fn default_on_shifts() -> u8 {
    2
}

fn default_off_shifts() -> u8 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProducerRuleCatalog {
    pub version: u32,
    pub rules: Vec<ProducerRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedProducerDependency {
    pub rule_id: String,
    pub source_buff_id: String,
    pub producers: Vec<String>,
    pub consumers: Vec<String>,
    pub target_facility: String,
    pub target_rooms: Vec<RoomId>,
    pub relation: ScheduleDependencyRelation,
    pub on_shifts: u8,
    pub off_shifts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_contribution: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseDependencyRow {
    pub skill_id: String,
    pub source_facility: String,
    pub target_facility: String,
    pub atom_id: Option<String>,
    pub selector: Option<String>,
    pub selector_scope: Option<DependencyScope>,
    pub condition: Option<String>,
    pub condition_scope: Option<DependencyScope>,
    pub action_dependency: String,
    pub action_scope: DependencyScope,
    pub reads_resources: Vec<String>,
    pub writes_resources: Vec<String>,
    pub atom_scope: String,
    pub response_field: ResponseField,
    pub requires_external_state: bool,
    #[serde(skip)]
    atom_position: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceConversionDependency {
    pub from: String,
    pub to: String,
    pub from_per: f64,
    pub to_per: f64,
    pub provider_buff_id: String,
    pub converter_buff_id: String,
    pub activation: String,
    pub operation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClosureEdgeKind {
    AtomProduce,
    AtomConvert,
    RegistryConversion,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceClosureEdge {
    pub kind: ResourceClosureEdgeKind,
    pub from: Option<String>,
    pub to: String,
    pub skill_id: Option<String>,
    pub atom_id: Option<String>,
    pub source_facility: Option<String>,
    pub provider_buff_id: Option<String>,
    pub converter_buff_id: Option<String>,
    pub requires_same_shift_activation: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceReverseClosure {
    pub target_facility: String,
    pub response_field: ResponseField,
    pub seed_resources: Vec<String>,
    pub resources: Vec<String>,
    pub edges: Vec<ResourceClosureEdge>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceReadFormula {
    FloorDivideMultiplier {
        div: f64,
        multiplier: f64,
    },
    FloorStepDelta {
        step_size: f64,
        delta_per_step: f64,
    },
    FloorThenRoundMultiplier {
        multiplier: f64,
    },
    LinearConvert {
        ratio: f64,
    },
    IntegerTruncateSaturatingAdd {
        cap: u64,
        action_multiplier: f64,
        requires_physical_base: bool,
    },
    DeclaredConsume {
        div: f64,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceValueDomainFact {
    pub skill_id: String,
    pub atom_id: Option<String>,
    pub target_facility: String,
    pub response_field: ResponseField,
    pub resource: String,
    pub formula: ResourceReadFormula,
    pub requires_producer_range_analysis: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedDelegatedDependency {
    pub facility: String,
    pub mechanism: String,
    pub resource: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainInputSource {
    Blueprint,
    GlobalResource,
    LayoutStatistic,
    CandidateRow,
    RoomContext,
}

#[derive(Debug, Clone, Copy)]
pub struct DomainDependencyInputDecl {
    pub name: &'static str,
    pub source: DomainInputSource,
    pub external_signature: bool,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainDependencyInput {
    pub name: String,
    pub source: DomainInputSource,
    pub external_signature: bool,
    pub scenario_values: Option<Vec<u32>>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainDependencyContributor {
    pub mechanism: String,
    pub target_facility: String,
    pub response_field: ResponseField,
    pub inputs: Vec<DomainDependencyInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceReachableRange {
    pub resource: String,
    pub min: f64,
    pub max: Option<f64>,
    pub integer_valued: bool,
    pub scope: String,
    pub unresolved_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceEquivalenceClass {
    pub resource: String,
    pub min_inclusive: u32,
    pub max_inclusive: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyScenario {
    pub template: Option<String>,
    pub room_count: usize,
    pub gold_manu_line_count: u32,
    pub initial_virtual_gold_lines: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseDependencyReport {
    pub coverage: String,
    pub scenario: Option<DependencyScenario>,
    pub skill_count: usize,
    pub atom_count: usize,
    pub external_atom_count: usize,
    pub by_target_facility: BTreeMap<String, usize>,
    pub external_by_target_facility: BTreeMap<String, usize>,
    pub dependency_edges_by_scope: BTreeMap<DependencyScope, usize>,
    pub by_response_field: BTreeMap<ResponseField, usize>,
    pub resource_conversions: Vec<ResourceConversionDependency>,
    pub resource_reverse_closures: Vec<ResourceReverseClosure>,
    pub resource_value_domains: Vec<ResourceValueDomainFact>,
    pub unresolved_delegated_dependencies: Vec<UnresolvedDelegatedDependency>,
    pub domain_dependency_contributors: Vec<DomainDependencyContributor>,
    pub resource_reachable_ranges: Vec<ResourceReachableRange>,
    pub resource_equivalence_classes: Vec<ResourceEquivalenceClass>,
    pub rows: Vec<ResponseDependencyRow>,
}

static PRODUCER_RULE_CATALOG: OnceLock<std::result::Result<ProducerRuleCatalog, String>> =
    OnceLock::new();

pub fn default_producer_rules_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("producer_rules.json")
}

pub fn load_producer_rule_catalog(path: &Path, table: &SkillTable) -> Result<ProducerRuleCatalog> {
    let raw = std::fs::read_to_string(path)?;
    let catalog: ProducerRuleCatalog = serde_json::from_str(&raw)
        .map_err(|error| Error::msg(format!("producer rules parse {}: {error}", path.display())))?;
    validate_producer_rule_catalog(&catalog, table).map_err(Error::msg)?;
    Ok(catalog)
}

pub fn producer_rule_catalog(table: &SkillTable) -> Result<&'static ProducerRuleCatalog> {
    let catalog = PRODUCER_RULE_CATALOG
        .get_or_init(|| {
            let path = default_producer_rules_path().map_err(|error| error.to_string())?;
            let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            serde_json::from_str(&raw)
                .map_err(|error| format!("producer rules parse {}: {error}", path.display()))
        })
        .as_ref()
        .map_err(|message| Error::msg(message.clone()))?;
    validate_producer_rule_catalog(catalog, table).map_err(Error::msg)?;
    Ok(catalog)
}

fn validate_producer_rule_catalog(
    catalog: &ProducerRuleCatalog,
    table: &SkillTable,
) -> std::result::Result<(), String> {
    if catalog.version != 1 {
        return Err(format!(
            "unsupported producer rules version {}, expected 1",
            catalog.version
        ));
    }
    let mut ids = BTreeSet::new();
    let mut buff_ids = BTreeSet::new();
    for rule in &catalog.rules {
        if !matches!(
            rule.admission,
            ProducerAdmission::DeferredOptional | ProducerAdmission::ExcludedAutomatic
        ) {
            return Err(format!(
                "producer rule {} uses unsupported admission {:?}",
                rule.id, rule.admission
            ));
        }
        if !ids.insert(rule.id.as_str()) {
            return Err(format!("duplicate producer rule id {}", rule.id));
        }
        if !buff_ids.insert(rule.source_buff_id.as_str()) {
            return Err(format!(
                "duplicate producer source buff {}",
                rule.source_buff_id
            ));
        }
        let skill = table.get(&rule.source_buff_id).ok_or_else(|| {
            format!(
                "producer rule {} references unknown buff {}",
                rule.id, rule.source_buff_id
            )
        })?;
        let matching_atoms: Vec<_> = skill
            .atoms
            .iter()
            .filter(|atom| {
                (dynamic_workforce_selector(atom.selector.as_ref()).is_some()
                    || matches!(atom.action, Action::GlobalInjectKarlanPrecision { .. }))
                    && target_facility(&skill.facility, &atom.action, atom.scope)
                        == rule.target_facility
            })
            .collect();
        if matching_atoms.is_empty() {
            return Err(format!(
                "producer rule {} does not match a dynamic workforce atom targeting {}",
                rule.id, rule.target_facility
            ));
        }
        if matching_atoms.iter().any(|atom| {
            matches!(
                atom.action,
                Action::GlobalInjectTradeEff { value }
                    | Action::GlobalInjectManuEff { value, .. }
                    if value < 0.0
            )
        }) {
            return Err(format!(
                "producer rule {} has a negative dynamic inject unsupported by safe upper-bound pruning",
                rule.id
            ));
        }
    }

    let expected: BTreeSet<&str> = table
        .skills()
        .iter()
        .filter(|skill| {
            skill.atoms.iter().any(|atom| {
                matches!(
                    atom.action,
                    Action::GlobalInjectTradeEff { .. } | Action::GlobalInjectManuEff { .. }
                ) && dynamic_workforce_selector(atom.selector.as_ref()).is_some()
                    || matches!(atom.action, Action::GlobalInjectKarlanPrecision { .. })
            })
        })
        .map(|skill| skill.id.as_str())
        .collect();
    if buff_ids != expected {
        let missing: Vec<_> = expected.difference(&buff_ids).copied().collect();
        let extra: Vec<_> = buff_ids.difference(&expected).copied().collect();
        return Err(format!(
            "producer rule coverage mismatch: missing={missing:?} extra={extra:?}"
        ));
    }
    Ok(())
}

fn dynamic_workforce_selector(selector: Option<&Selector>) -> Option<(&str, Option<u8>)> {
    match selector? {
        Selector::TaggedCountInTradeSum { tag }
        | Selector::TaggedCountInCurrentTradeRoom { tag }
        | Selector::TaggedCountInManuSum { tag } => Some((tag, None)),
        Selector::TradeStationsWithTaggedGte { tag, min } => Some((tag, Some(*min))),
        _ => None,
    }
}

pub fn deferred_producer_rules_for_buffs<'a>(
    table: &SkillTable,
    buff_ids: impl IntoIterator<Item = &'a str>,
    target_facility: &str,
) -> Result<Vec<ProducerRule>> {
    let requested: BTreeSet<&str> = buff_ids.into_iter().collect();
    Ok(producer_rule_catalog(table)?
        .rules
        .iter()
        .filter(|rule| {
            matches!(
                rule.admission,
                ProducerAdmission::DeferredOptional | ProducerAdmission::ExcludedAutomatic
            ) && rule.target_facility == target_facility
                && requested.contains(rule.source_buff_id.as_str())
        })
        .cloned()
        .collect())
}

pub fn control_buff_is_automatically_excluded<'a>(
    table: &SkillTable,
    buff_ids: impl IntoIterator<Item = &'a str>,
) -> Result<bool> {
    let requested: BTreeSet<&str> = buff_ids.into_iter().collect();
    if producer_rule_catalog(table)?.rules.iter().any(|rule| {
        rule.admission == ProducerAdmission::ExcludedAutomatic
            && requested.contains(rule.source_buff_id.as_str())
    }) {
        return Ok(true);
    }
    // Precision is considered only by an explicit optional prefix, never by
    // ordinary control fill. The prefix does not reserve trade teammates.
    Ok(control_buffs_have_karlan_precision(
        table,
        requested.iter().copied(),
    ))
}

pub(crate) fn control_buffs_have_karlan_precision<'a>(
    table: &SkillTable,
    buff_ids: impl IntoIterator<Item = &'a str>,
) -> bool {
    buff_ids.into_iter().any(|buff_id| {
        table.get(buff_id).is_some_and(|skill| {
            skill
                .atoms
                .iter()
                .any(|atom| matches!(atom.action, Action::GlobalInjectKarlanPrecision { .. }))
        })
    })
}

/// Selection policy witness: the real market variant and a Karlan peer
/// survived trade selection in the same room. This does not place either one.
pub(crate) fn karlan_precision_consumer_rooms(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
) -> Vec<RoomId> {
    blueprint
        .rooms
        .iter()
        .filter(|room| {
            room.kind == FacilityKind::TradePost && {
                let operators = assignment.operators_in(&room.id);
                operators.iter().any(|op| {
                    instances
                        .resolve_trade_buff_ids(&op.name, op.tier())
                        .iter()
                        .any(|buff| buff == "trade_ord_limit_count[000]")
                }) && operators.iter().any(|op| {
                    instances
                        .tags_for(&op.name, op.tier())
                        .iter()
                        .any(|tag| tag == "cc.g.karlan")
                })
            }
        })
        .map(|room| room.id.clone())
        .collect()
}

pub fn resolve_assignment_producer_dependencies(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
) -> Result<Vec<ResolvedProducerDependency>> {
    let catalog = producer_rule_catalog(table)?;
    let mut resolved = Vec::new();
    for rule in &catalog.rules {
        if rule.admission == ProducerAdmission::ExcludedAutomatic {
            continue;
        }
        let mut producers: Vec<_> = assignment
            .control_operators()
            .into_iter()
            .filter(|operator| {
                instances
                    .resolve_control_buff_ids(&operator.name, operator.tier())
                    .iter()
                    .any(|buff_id| buff_id == &rule.source_buff_id)
            })
            .map(|operator| operator.name)
            .collect();
        producers.sort();
        producers.dedup();
        producers.truncate(1);
        if producers.is_empty() {
            continue;
        }

        let skill = table.get(&rule.source_buff_id).ok_or_else(|| {
            Error::msg(format!(
                "resolved producer rule {} missing skill {}",
                rule.id, rule.source_buff_id
            ))
        })?;
        if control_buffs_have_karlan_precision(table, [rule.source_buff_id.as_str()]) {
            let target_rooms = karlan_precision_consumer_rooms(blueprint, assignment, instances);
            if target_rooms.is_empty() {
                continue;
            }
            // Bind the market converter and actual Karlan recipients. An
            // independent third operator (e.g. Docus with Fiammetta coverage)
            // may keep working while this synergy rests.
            let mut consumers: Vec<_> = target_rooms
                .iter()
                .flat_map(|room| {
                    assignment
                        .operators_in(room)
                        .iter()
                        .filter(|op| {
                            instances
                                .tags_for(&op.name, op.tier())
                                .iter()
                                .any(|tag| tag == "cc.g.karlan")
                                || instances
                                    .resolve_trade_buff_ids(&op.name, op.tier())
                                    .iter()
                                    .any(|buff| buff == "trade_ord_limit_count[000]")
                        })
                        .map(|op| op.name.clone())
                })
                .collect();
            consumers.sort();
            consumers.dedup();
            resolved.push(ResolvedProducerDependency {
                rule_id: rule.id.clone(),
                source_buff_id: rule.source_buff_id.clone(),
                producers,
                consumers,
                target_facility: rule.target_facility.clone(),
                target_rooms,
                relation: rule.schedule_relation,
                on_shifts: rule.on_shifts,
                off_shifts: rule.off_shifts,
                effective_contribution: None,
            });
            continue;
        }
        let mut consumers = BTreeSet::new();
        let mut target_rooms = Vec::new();
        let mut contribution = 0.0;
        for atom in &skill.atoms {
            if target_facility(&skill.facility, &atom.action, atom.scope) != rule.target_facility {
                continue;
            }
            let Some((tag, threshold)) = dynamic_workforce_selector(atom.selector.as_ref()) else {
                continue;
            };
            let facility = match rule.target_facility.as_str() {
                "trade" => FacilityKind::TradePost,
                "manufacture" => FacilityKind::Factory,
                other => {
                    return Err(Error::msg(format!(
                        "producer rule {} has unsupported target facility {other}",
                        rule.id
                    )))
                }
            };
            let value = match atom.action {
                Action::GlobalInjectTradeEff { value }
                | Action::GlobalInjectManuEff { value, .. } => value,
                _ => continue,
            };
            for room in blueprint.rooms.iter().filter(|room| room.kind == facility) {
                let tagged: Vec<_> = assignment
                    .operators_in(&room.id)
                    .iter()
                    .filter(|operator| {
                        instances
                            .tags_for(&operator.name, operator.tier())
                            .iter()
                            .any(|candidate| candidate == tag)
                    })
                    .map(|operator| operator.name.clone())
                    .collect();
                if threshold.is_some_and(|min| tagged.len() < usize::from(min)) {
                    continue;
                }
                if tagged.is_empty() {
                    continue;
                }
                contribution += if threshold.is_some() {
                    value
                } else {
                    value * tagged.len() as f64
                };
                if !target_rooms.contains(&room.id) {
                    target_rooms.push(room.id.clone());
                }
                consumers.extend(tagged);
            }
        }
        if consumers.is_empty() || contribution == 0.0 {
            continue;
        }
        target_rooms.sort_by(|left, right| left.0.cmp(&right.0));
        resolved.push(ResolvedProducerDependency {
            rule_id: rule.id.clone(),
            source_buff_id: rule.source_buff_id.clone(),
            producers,
            consumers: consumers.into_iter().collect(),
            target_facility: rule.target_facility.clone(),
            target_rooms,
            relation: rule.schedule_relation,
            on_shifts: rule.on_shifts,
            off_shifts: rule.off_shifts,
            effective_contribution: Some(contribution),
        });
    }
    resolved.sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
    Ok(resolved)
}

pub fn build_response_dependency_report(table: &SkillTable) -> ResponseDependencyReport {
    build_response_dependency_report_inner(table, None)
}

pub fn build_response_dependency_report_for_blueprint(
    table: &SkillTable,
    blueprint: &BaseBlueprint,
) -> ResponseDependencyReport {
    build_response_dependency_report_inner(table, Some(blueprint))
}

fn build_response_dependency_report_inner(
    table: &SkillTable,
    blueprint: Option<&BaseBlueprint>,
) -> ResponseDependencyReport {
    let mut rows = Vec::new();
    for skill in table.skills() {
        for (atom_position, atom) in skill.atoms.iter().enumerate() {
            let selector = atom.selector.as_ref().map(selector_dependency);
            let condition = atom.condition.as_ref().map(condition_dependency);
            let action = action_dependency(&atom.action, atom.scope);
            let atom_scope = match atom.scope {
                AtomScope::Room => "room",
                AtomScope::Global => "global",
            };
            let requires_external_state = atom.scope == AtomScope::Global
                || selector.is_some_and(|(_, scope)| scope != DependencyScope::RoomLocal)
                || condition.is_some_and(|(_, scope)| scope != DependencyScope::RoomLocal)
                || action.1 != DependencyScope::RoomLocal;
            rows.push(ResponseDependencyRow {
                skill_id: skill.id.clone(),
                source_facility: skill.facility.clone(),
                target_facility: target_facility(&skill.facility, &atom.action, atom.scope)
                    .to_string(),
                atom_id: atom.atom_id.clone(),
                selector: selector.map(|(name, _)| name.to_string()),
                selector_scope: selector.map(|(_, scope)| scope),
                condition: condition.map(|(name, _)| name.to_string()),
                condition_scope: condition.map(|(_, scope)| scope),
                action_dependency: action.0.to_string(),
                action_scope: action.1,
                reads_resources: read_resources(atom),
                writes_resources: written_resources(atom),
                atom_scope: atom_scope.to_string(),
                response_field: response_field(&atom.action),
                requires_external_state,
                atom_position,
            });
        }
    }

    let mut by_target_facility = BTreeMap::new();
    let mut by_dependency_scope = BTreeMap::new();
    let mut by_response_field = BTreeMap::new();
    let mut external_by_target_facility = BTreeMap::new();
    for row in &rows {
        *by_target_facility
            .entry(row.target_facility.clone())
            .or_insert(0) += 1;
        for scope in [
            row.selector_scope,
            row.condition_scope,
            Some(row.action_scope),
        ]
        .into_iter()
        .flatten()
        {
            *by_dependency_scope.entry(scope).or_insert(0) += 1;
        }
        *by_response_field.entry(row.response_field).or_insert(0) += 1;
        if row.requires_external_state {
            *external_by_target_facility
                .entry(row.target_facility.clone())
                .or_insert(0) += 1;
        }
    }

    let resource_conversions: Vec<_> = CONVERSIONS
        .iter()
        .map(|conversion| ResourceConversionDependency {
            from: conversion.from.id().to_string(),
            to: conversion.to.id().to_string(),
            from_per: conversion.from_per,
            to_per: conversion.to_per,
            provider_buff_id: conversion.provider_buff_id.to_string(),
            converter_buff_id: conversion.converter_buff_id.to_string(),
            activation: "provider_and_converter_active_in_same_shift".to_string(),
            operation: "destructive_floor_conversion".to_string(),
        })
        .collect();
    let resource_value_domains = build_resource_value_domains(table);
    let resource_reverse_closures = build_resource_reverse_closures(&rows, &resource_conversions);
    let domain_dependency_contributors = build_domain_dependency_contributors(blueprint);
    let resource_reachable_ranges = build_resource_reachable_ranges(
        table,
        blueprint,
        &resource_reverse_closures,
        &resource_conversions,
    );
    let resource_equivalence_classes =
        build_resource_equivalence_classes(&resource_reachable_ranges, &resource_value_domains);

    ResponseDependencyReport {
        coverage: "effect_atom_plus_global_conversion_registry".to_string(),
        scenario: blueprint.map(|blueprint| DependencyScenario {
            template: blueprint.template.clone(),
            room_count: blueprint.rooms.len(),
            gold_manu_line_count: blueprint.gold_manu_line_count(),
            initial_virtual_gold_lines: blueprint
                .scenario
                .initial_global
                .get(&crate::global_resource::GlobalResourceKey::VirtualGoldLines)
                .copied()
                .unwrap_or(0.0)
                .max(0.0)
                .min(u32::MAX as f64) as u32,
        }),
        skill_count: table.skills().len(),
        atom_count: rows.len(),
        external_atom_count: rows
            .iter()
            .filter(|row| row.requires_external_state)
            .count(),
        by_target_facility,
        external_by_target_facility,
        dependency_edges_by_scope: by_dependency_scope,
        by_response_field,
        resource_conversions,
        resource_reverse_closures,
        resource_value_domains,
        unresolved_delegated_dependencies: domain_dependency_contributors
            .iter()
            .flat_map(|contributor| {
                contributor
                    .inputs
                    .iter()
                    .filter(|input| input.external_signature && input.scenario_values.is_none())
                    .map(|input| UnresolvedDelegatedDependency {
                        facility: contributor.target_facility.clone(),
                        mechanism: contributor.mechanism.clone(),
                        resource: input.name.clone(),
                        reason: "domain contributor has no finite scenario domain".to_string(),
                    })
            })
            .collect(),
        domain_dependency_contributors,
        resource_reachable_ranges,
        resource_equivalence_classes,
        rows,
    }
}

fn build_domain_dependency_contributors(
    blueprint: Option<&BaseBlueprint>,
) -> Vec<DomainDependencyContributor> {
    let inputs = crate::trade::gold_flow::dependency_inputs()
        .iter()
        .map(|decl| {
            let scenario_values = blueprint.and_then(|blueprint| match decl.name {
                "real_gold_lines" => Some(vec![blueprint.gold_manu_line_count()]),
                "virtual_gold_lines" => Some(vec![blueprint
                    .scenario
                    .initial_global
                    .get(&crate::global_resource::GlobalResourceKey::VirtualGoldLines)
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
                    .min(u32::MAX as f64)
                    as u32]),
                "durin_virtual_lines" => Some((0..=4).collect()),
                _ => None,
            });
            DomainDependencyInput {
                name: decl.name.to_string(),
                source: decl.source,
                external_signature: decl.external_signature,
                scenario_values,
                note: decl.note.to_string(),
            }
        })
        .collect();
    vec![DomainDependencyContributor {
        mechanism: "gold_flow".to_string(),
        target_facility: "trade".to_string(),
        response_field: ResponseField::Efficiency,
        inputs,
    }]
}

fn build_resource_reachable_ranges(
    _table: &SkillTable,
    blueprint: Option<&BaseBlueprint>,
    closures: &[ResourceReverseClosure],
    _conversions: &[ResourceConversionDependency],
) -> Vec<ResourceReachableRange> {
    let mut resources: BTreeSet<String> = closures
        .iter()
        .flat_map(|closure| closure.resources.iter().cloned())
        .collect();
    resources.extend([
        "real_gold_lines".to_string(),
        "virtual_gold_lines".to_string(),
        "durin_virtual_lines".to_string(),
    ]);

    resources
        .into_iter()
        .map(|resource| {
            let scenario_bound = blueprint.and_then(|blueprint| match resource.as_str() {
                "real_gold_lines" => {
                    let value = blueprint.gold_manu_line_count() as f64;
                    Some((value, value, true))
                }
                "virtual_gold_lines" => {
                    let value = blueprint
                        .scenario
                        .initial_global
                        .get(&crate::global_resource::GlobalResourceKey::VirtualGoldLines)
                        .copied()
                        .unwrap_or(0.0)
                        .max(0.0);
                    Some((value, value, true))
                }
                "durin_virtual_lines" => Some((0.0, 4.0, true)),
                _ => None,
            });
            match scenario_bound {
                Some((min, max, integer_valued)) => ResourceReachableRange {
                    resource,
                    min,
                    max: Some(max),
                    integer_valued,
                    scope: "scenario_external_input".to_string(),
                    unresolved_reasons: Vec::new(),
                },
                None => ResourceReachableRange {
                    resource,
                    min: 0.0,
                    max: None,
                    integer_valued: false,
                    scope: if blueprint.is_some() {
                        "scenario_effect_resource".to_string()
                    } else {
                        "unbound_model".to_string()
                    },
                    unresolved_reasons: vec![
                        "operator owner cardinality and all producer paths are not yet compiled"
                            .to_string(),
                    ],
                },
            }
        })
        .collect()
}

fn build_resource_equivalence_classes(
    ranges: &[ResourceReachableRange],
    _facts: &[ResourceValueDomainFact],
) -> Vec<ResourceEquivalenceClass> {
    let mut classes = Vec::new();
    for range in ranges {
        let Some(max) = range.max else {
            continue;
        };
        if !range.integer_valued || range.min < 0.0 || max > u32::MAX as f64 {
            continue;
        }
        let min = range.min as u32;
        let max = max as u32;
        for value in min..=max {
            classes.push(ResourceEquivalenceClass {
                resource: range.resource.clone(),
                min_inclusive: value,
                max_inclusive: value,
            });
        }
    }
    classes
}

fn build_resource_reverse_closures(
    rows: &[ResponseDependencyRow],
    conversions: &[ResourceConversionDependency],
) -> Vec<ResourceReverseClosure> {
    let mut seeds: BTreeMap<(String, ResponseField), BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        if row.target_facility == "global_resource" || row.reads_resources.is_empty() {
            continue;
        }
        seeds
            .entry((
                row.target_facility.clone(),
                closure_response_field(row.response_field),
            ))
            .or_default()
            .extend(row.reads_resources.iter().cloned());
    }

    seeds
        .into_iter()
        .map(|((target_facility, response_field), seed_resources)| {
            let mut resources = seed_resources.clone();
            let mut queue: VecDeque<_> = seed_resources.iter().cloned().collect();
            let mut edges = BTreeMap::new();
            while let Some(resource) = queue.pop_front() {
                for row in rows
                    .iter()
                    .filter(|row| row.writes_resources.contains(&resource))
                {
                    let kind = if row.reads_resources.is_empty() {
                        ResourceClosureEdgeKind::AtomProduce
                    } else {
                        ResourceClosureEdgeKind::AtomConvert
                    };
                    let predecessors: Vec<Option<String>> = if row.reads_resources.is_empty() {
                        vec![None]
                    } else {
                        row.reads_resources.iter().cloned().map(Some).collect()
                    };
                    for from in predecessors {
                        if let Some(predecessor) = &from {
                            if resources.insert(predecessor.clone()) {
                                queue.push_back(predecessor.clone());
                            }
                        }
                        let atom_key = row.atom_id.clone().unwrap_or_else(|| {
                            format!("legacy_atom_position:{}", row.atom_position)
                        });
                        let key = (
                            kind,
                            from.clone(),
                            resource.clone(),
                            Some(row.skill_id.clone()),
                            Some(atom_key),
                            None,
                            None,
                        );
                        edges.entry(key).or_insert_with(|| ResourceClosureEdge {
                            kind,
                            from,
                            to: resource.clone(),
                            skill_id: Some(row.skill_id.clone()),
                            atom_id: row.atom_id.clone(),
                            source_facility: Some(row.source_facility.clone()),
                            provider_buff_id: None,
                            converter_buff_id: None,
                            requires_same_shift_activation: false,
                        });
                    }
                }
                for conversion in conversions.iter().filter(|edge| edge.to == resource) {
                    if resources.insert(conversion.from.clone()) {
                        queue.push_back(conversion.from.clone());
                    }
                    let key = (
                        ResourceClosureEdgeKind::RegistryConversion,
                        Some(conversion.from.clone()),
                        resource.clone(),
                        None,
                        None,
                        Some(conversion.provider_buff_id.clone()),
                        Some(conversion.converter_buff_id.clone()),
                    );
                    edges.entry(key).or_insert_with(|| ResourceClosureEdge {
                        kind: ResourceClosureEdgeKind::RegistryConversion,
                        from: Some(conversion.from.clone()),
                        to: resource.clone(),
                        skill_id: None,
                        atom_id: None,
                        source_facility: None,
                        provider_buff_id: Some(conversion.provider_buff_id.clone()),
                        converter_buff_id: Some(conversion.converter_buff_id.clone()),
                        requires_same_shift_activation: true,
                    });
                }
            }
            ResourceReverseClosure {
                target_facility,
                response_field,
                seed_resources: seed_resources.into_iter().collect(),
                resources: resources.into_iter().collect(),
                edges: edges.into_values().collect(),
            }
        })
        .collect()
}

fn build_resource_value_domains(table: &SkillTable) -> Vec<ResourceValueDomainFact> {
    let mut facts = Vec::new();
    for skill in table.skills() {
        for atom in &skill.atoms {
            let target = target_facility(&skill.facility, &atom.action, atom.scope).to_string();
            let field = closure_response_field(response_field(&atom.action));
            if let Some(Selector::StatePoolFloored { key, div }) = atom.selector.as_ref() {
                let multiplier = match atom.action {
                    Action::AddFlatEffFromSelector { multiplier, .. } => multiplier,
                    Action::GlobalInjectTradeEff { value }
                    | Action::GlobalInjectManuEff { value, .. }
                    | Action::GlobalInjectManuTaggedEff { value, .. } => value,
                    _ => 1.0,
                };
                facts.push(ResourceValueDomainFact {
                    skill_id: skill.id.clone(),
                    atom_id: atom.atom_id.clone(),
                    target_facility: target.clone(),
                    response_field: field,
                    resource: normalize_resource(key),
                    formula: ResourceReadFormula::FloorDivideMultiplier {
                        div: *div,
                        multiplier,
                    },
                    requires_producer_range_analysis: true,
                });
            }
            let (resource, formula) = match &atom.action {
                Action::StateConsumeToEff {
                    key,
                    div,
                    multiplier,
                } => (
                    key,
                    ResourceReadFormula::FloorDivideMultiplier {
                        div: *div,
                        multiplier: multiplier.unwrap_or(1.0),
                    },
                ),
                Action::MoodDrainPerStateStep {
                    key,
                    step_size,
                    delta_per_step,
                    ..
                } => (
                    key,
                    ResourceReadFormula::FloorStepDelta {
                        step_size: *step_size,
                        delta_per_step: *delta_per_step,
                    },
                ),
                Action::AddLimitFromState { key, multiplier } => (
                    key,
                    ResourceReadFormula::FloorThenRoundMultiplier {
                        multiplier: *multiplier,
                    },
                ),
                Action::StateConvert { from, ratio, .. } => {
                    (from, ResourceReadFormula::LinearConvert { ratio: *ratio })
                }
                Action::StateConsume { key, div } => {
                    (key, ResourceReadFormula::DeclaredConsume { div: *div })
                }
                _ => continue,
            };
            facts.push(ResourceValueDomainFact {
                skill_id: skill.id.clone(),
                atom_id: atom.atom_id.clone(),
                target_facility: target,
                response_field: field,
                resource: normalize_resource(resource),
                formula,
                requires_producer_range_analysis: true,
            });
        }
    }
    for skill in table.skills() {
        for atom in &skill.atoms {
            if atom.selector != Some(Selector::PowerStationCount) {
                continue;
            }
            facts.push(ResourceValueDomainFact {
                skill_id: skill.id.clone(),
                atom_id: atom.atom_id.clone(),
                target_facility: target_facility(&skill.facility, &atom.action, atom.scope)
                    .to_string(),
                response_field: closure_response_field(response_field(&atom.action)),
                resource: "virtual_power".to_string(),
                formula: ResourceReadFormula::IntegerTruncateSaturatingAdd {
                    cap: 255,
                    action_multiplier: selector_action_multiplier(&atom.action),
                    requires_physical_base: true,
                },
                requires_producer_range_analysis: true,
            });
        }
    }
    facts
}

fn closure_response_field(field: ResponseField) -> ResponseField {
    match field {
        ResponseField::GlobalInject => ResponseField::Efficiency,
        other => other,
    }
}

fn selector_action_multiplier(action: &Action) -> f64 {
    match action {
        Action::AddFlatEffFromSelector { multiplier, .. } => *multiplier,
        Action::GlobalInjectTradeEff { value }
        | Action::GlobalInjectManuEff { value, .. }
        | Action::GlobalInjectManuTaggedEff { value, .. } => *value,
        _ => 1.0,
    }
}

fn read_resources(atom: &crate::types::EffectAtom) -> Vec<String> {
    let mut resources = Vec::new();
    if let Some(Selector::StatePoolFloored { key, .. }) = atom.selector.as_ref() {
        resources.push(normalize_resource(key));
    }
    if atom.selector == Some(Selector::PowerStationCount) {
        resources.push("virtual_power".to_string());
    }
    let action_resource = match &atom.action {
        Action::AddLimitFromState { key, .. }
        | Action::MoodDrainPerStateStep { key, .. }
        | Action::StateConsume { key, .. }
        | Action::StateConsumeToEff { key, .. } => Some(normalize_resource(key)),
        Action::StateConvert { from, .. } => Some(normalize_resource(from)),
        _ => None,
    };
    if let Some(resource) = action_resource {
        if !resources.contains(&resource) {
            resources.push(resource);
        }
    }
    resources
}

fn written_resources(atom: &crate::types::EffectAtom) -> Vec<String> {
    match &atom.action {
        Action::StateProduce { key, .. } => vec![normalize_resource(key)],
        Action::StateConvert { to, .. } => vec![normalize_resource(to)],
        _ => Vec::new(),
    }
}

fn normalize_resource(key: &str) -> String {
    crate::global_resource::GlobalResourceKey::parse(key)
        .map(|resource| resource.id().to_string())
        .unwrap_or_else(|| key.to_ascii_lowercase())
}

fn target_facility<'a>(source: &'a str, action: &Action, atom_scope: AtomScope) -> &'a str {
    match action {
        Action::GlobalInjectTradeEff { .. } | Action::GlobalInjectKarlanPrecision { .. } => "trade",
        Action::GlobalInjectManuEff { .. } | Action::GlobalInjectManuTaggedEff { .. } => {
            "manufacture"
        }
        Action::GlobalInjectOfficeHireSpeed { .. } => "office",
        Action::GlobalInjectMeetingSpeed { .. } => "meeting",
        Action::StateProduce { .. } if atom_scope == AtomScope::Global => "global_resource",
        _ => source,
    }
}

fn action_dependency(action: &Action, atom_scope: AtomScope) -> (&'static str, DependencyScope) {
    use DependencyScope::*;
    match action {
        Action::AddPerGapEff { .. } => ("order_gap", RuntimeState),
        Action::AddLimitFromState { .. }
        | Action::MoodDrainPerStateStep { .. }
        | Action::StateConsume { .. }
        | Action::StateConvert { .. }
        | Action::StateConsumeToEff { .. } => ("state_pool", RuntimeState),
        Action::GlobalInjectTradeEff { .. }
        | Action::GlobalInjectManuEff { .. }
        | Action::GlobalInjectManuTaggedEff { .. }
        | Action::GlobalInjectKarlanPrecision { .. }
        | Action::GlobalInjectOfficeHireSpeed { .. }
        | Action::GlobalInjectMeetingSpeed { .. } => ("global_inject", CrossFacility),
        Action::StateProduce { .. } if atom_scope == AtomScope::Global => {
            ("global_resource_produce", CrossFacility)
        }
        Action::StateProduce { .. } => ("room_state_produce", RoomLocal),
        Action::AddFlatEff { .. }
        | Action::TagOrder { .. }
        | Action::AddGoldDelivery { .. }
        | Action::ReduceLimit { .. }
        | Action::AddLimitFromSelector { .. }
        | Action::AddFlatEffFromSelector { .. }
        | Action::AddLimitDelta { .. }
        | Action::MoodDrainDelta { .. }
        | Action::MoodDrainDeltaForOperator { .. }
        | Action::ClearMoodDrain { .. }
        | Action::AddOrderLmdBonus { .. }
        | Action::PeerEffAbsorb { .. }
        | Action::AddBucketEffFromSelector { .. }
        | Action::AddEffFromLimitContribSum { .. }
        | Action::AddEffFromLimitContribTiered { .. }
        | Action::AddEffRamp { .. } => ("no_implicit_input", RoomLocal),
    }
}

fn response_field(action: &Action) -> ResponseField {
    match action {
        Action::AddFlatEff { .. }
        | Action::AddPerGapEff { .. }
        | Action::PeerEffAbsorb { .. }
        | Action::AddBucketEffFromSelector { .. }
        | Action::AddEffFromLimitContribSum { .. }
        | Action::AddEffFromLimitContribTiered { .. }
        | Action::StateConsumeToEff { .. }
        | Action::AddFlatEffFromSelector { .. }
        | Action::AddEffRamp { .. } => ResponseField::Efficiency,
        Action::ReduceLimit { .. }
        | Action::AddLimitFromSelector { .. }
        | Action::AddLimitFromState { .. }
        | Action::AddLimitDelta { .. } => ResponseField::LimitOrStorage,
        Action::TagOrder { .. }
        | Action::AddGoldDelivery { .. }
        | Action::AddOrderLmdBonus { .. } => ResponseField::UnitOutput,
        Action::MoodDrainDelta { .. }
        | Action::MoodDrainDeltaForOperator { .. }
        | Action::MoodDrainPerStateStep { .. }
        | Action::ClearMoodDrain { .. } => ResponseField::Mood,
        Action::StateProduce { .. } | Action::StateConsume { .. } | Action::StateConvert { .. } => {
            ResponseField::StateResource
        }
        Action::GlobalInjectTradeEff { .. }
        | Action::GlobalInjectManuEff { .. }
        | Action::GlobalInjectManuTaggedEff { .. }
        | Action::GlobalInjectKarlanPrecision { .. }
        | Action::GlobalInjectOfficeHireSpeed { .. }
        | Action::GlobalInjectMeetingSpeed { .. } => ResponseField::GlobalInject,
    }
}

fn selector_dependency(selector: &Selector) -> (&'static str, DependencyScope) {
    use DependencyScope::*;
    match selector {
        Selector::GoldDeliveryCount => ("gold_delivery_count", RuntimeState),
        Selector::OtherOpsDirectEff => ("other_ops_direct_eff", RoomLocal),
        Selector::OtherOpsTotalEff => ("other_ops_total_eff", RoomLocal),
        Selector::RoomPeerCount => ("room_peer_count", RoomLocal),
        Selector::RoomOperatorCount => ("room_operator_count", RoomLocal),
        Selector::FinalOrderLimit => ("final_order_limit", RuntimeState),
        Selector::LimitExcess => ("limit_excess", RuntimeState),
        Selector::FacilityLevel => ("facility_level", RoomLocal),
        Selector::FacilityLevelMinusOne => ("facility_level_minus_one", RoomLocal),
        Selector::TaggedCountInRoom { .. } => ("tagged_count_in_room", RoomLocal),
        Selector::LimitContribSum => ("limit_contrib_sum", RoomLocal),
        Selector::MeetingMaxLevel => ("meeting_max_level", CrossFacility),
        Selector::DormLevelSum => ("dorm_level_sum", CrossFacility),
        Selector::ManuRecipeKinds => ("manu_recipe_kinds", CrossFacility),
        Selector::EliteFacilityCount => ("elite_facility_count", GlobalLayout),
        Selector::SuiFacilityCount => ("sui_facility_count", GlobalLayout),
        Selector::CappedSuiFacilityCount { .. } => ("capped_sui_facility_count", GlobalLayout),
        Selector::DormOccupantCount => ("dorm_occupant_count", CrossFacility),
        Selector::OrderGap => ("order_gap", RuntimeState),
        Selector::OrderCount => ("order_count", RuntimeState),
        Selector::PeerSettledEffSum => ("peer_settled_eff_sum", RoomLocal),
        Selector::PeerSkillEffSum => ("peer_skill_eff_sum", RoomLocal),
        Selector::OtherOpsSettledEff => ("other_ops_settled_eff", RoomLocal),
        Selector::TradeStationCount => ("trade_station_count", CrossFacility),
        Selector::PowerStationCount => ("power_station_count", CrossFacility),
        Selector::PlatformCountInPower => ("platform_count_in_power", CrossFacility),
        Selector::TaggedCountInControl { .. } => ("tagged_count_in_control", CrossFacility),
        Selector::ControlOperatorCount => ("control_operator_count", CrossFacility),
        Selector::DroneCap => ("drone_cap", GlobalLayout),
        Selector::RhineLifeInBase => ("rhine_life_in_base", GlobalLayout),
        Selector::RhineLifeInBaseExcludingSelf => {
            ("rhine_life_in_base_excluding_self", GlobalLayout)
        }
        Selector::MetalFormulaSkillCountInRoom => ("metal_formula_skill_count_in_room", RoomLocal),
        Selector::StandardSkillCountInRoom => ("standard_skill_count_in_room", RoomLocal),
        Selector::RhineSkillCountInRoom => ("rhine_skill_count_in_room", RoomLocal),
        Selector::FacilityLevelSumExclMeeting => ("facility_level_sum_excl_meeting", GlobalLayout),
        Selector::TrainingRoomLevel => ("training_room_level", CrossFacility),
        Selector::Mood => ("mood", RuntimeState),
        Selector::OwnerMoodGap => ("owner_mood_gap", RuntimeState),
        Selector::TaggedCountInTradeSum { .. } => ("tagged_count_in_trade_sum", SameFacility),
        Selector::TaggedCountInCurrentTradeRoom { .. } => {
            ("tagged_count_in_current_trade_room", RoomLocal)
        }
        Selector::TradeStationsWithTaggedGte { .. } => {
            ("trade_stations_with_tagged_gte", SameFacility)
        }
        Selector::TaggedCountInManuSum { .. } => ("tagged_count_in_manu_sum", SameFacility),
        Selector::StatePoolFloored { .. } => ("state_pool_floored", RuntimeState),
    }
}

fn condition_dependency(condition: &Condition) -> (&'static str, DependencyScope) {
    use DependencyScope::*;
    match condition {
        Condition::GoldDeliveryBelow { .. } => ("gold_delivery_below", RuntimeState),
        Condition::GoldDeliveryAbove { .. } => ("gold_delivery_above", RuntimeState),
        Condition::GoldOrderInvestEligible { .. } => ("gold_order_invest_eligible", RuntimeState),
        Condition::OrderHasTag { .. } => ("order_has_tag", RuntimeState),
        Condition::OrderNotHasTag { .. } => ("order_not_has_tag", RuntimeState),
        Condition::MoodAbove { .. } => ("mood_above", RuntimeState),
        Condition::MoodAboveOrEq { .. } => ("mood_above_or_eq", RuntimeState),
        Condition::MoodBelow { .. } => ("mood_below", RuntimeState),
        Condition::MoodBelowOrEq { .. } => ("mood_below_or_eq", RuntimeState),
        Condition::OwnerMoodGapAbove { .. } => ("owner_mood_gap_above", RuntimeState),
        Condition::OwnerEliteGte { .. } => ("owner_elite_gte", RoomLocal),
        Condition::OwnerEliteBelow { .. } => ("owner_elite_below", RoomLocal),
        Condition::PartnerInRoom { .. } => ("partner_in_room", RoomLocal),
        Condition::TagPresentInRoom { .. } => ("tag_present_in_room", RoomLocal),
        Condition::PeerTagInRoom { .. } => ("peer_tag_in_room", RoomLocal),
        Condition::OperatorInBase { .. } => ("operator_in_base", GlobalLayout),
        Condition::OperatorInPower { .. } => ("operator_in_power", CrossFacility),
        Condition::OperatorInTraining { .. } => ("operator_in_training", CrossFacility),
        Condition::OperatorInTrade { .. } => ("operator_in_trade", CrossFacility),
        Condition::NoPlatformInOtherPower { .. } => ("no_platform_in_other_power", SameFacility),
        Condition::OtherPlatformInPower { .. } => ("other_platform_in_power", SameFacility),
        Condition::OtherLateranoInPower { .. } => ("other_laterano_in_power", SameFacility),
        Condition::TiandaoEffVarAllowed { .. } => ("tiandao_eff_var_allowed", RoomLocal),
        Condition::ActiveRecipe { .. } => ("active_recipe", RoomLocal),
        Condition::OwnerLacksBuff { .. } => ("owner_lacks_buff", RoomLocal),
        Condition::ExternalMomentumGteField { .. } => ("external_momentum_gte_field", GlobalLayout),
        Condition::FieldMomentumGtExternal { .. } => ("field_momentum_gt_external", GlobalLayout),
        Condition::OfficeHireSpeedBelow { .. } => ("office_hire_speed_below", CrossFacility),
        Condition::PlatformCountGte { .. } => ("platform_count_gte", CrossFacility),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_table::default_skill_table_path;

    #[test]
    fn producer_rule_catalog_exhaustively_classifies_dynamic_workforce_injects() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let catalog = producer_rule_catalog(&table).unwrap();
        let buffs: BTreeSet<_> = catalog
            .rules
            .iter()
            .map(|rule| rule.source_buff_id.as_str())
            .collect();
        assert_eq!(
            buffs,
            BTreeSet::from([
                "control_bd_spd[000]",
                "control_tra_limit&spd[000]",
                "control_tra_limit&spd2[000]",
                "control_tra_limit&spd3[000]",
                "control_tra_limit&spd[010]",
            ])
        );
        assert!(catalog.rules.iter().all(|rule| {
            !rule.id.contains("八幡")
                && !rule.id.contains("戴菲恩")
                && !rule.id.contains("银灰")
                && !rule.id.contains("杰西卡")
        }));
    }

    #[test]
    fn karlan_rule_keeps_consumer_discovery_but_excludes_automatic_admission() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let rules =
            deferred_producer_rules_for_buffs(&table, ["control_tra_limit&spd3[000]"], "trade")
                .unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "trade_karlan_station");
        assert_eq!(rules[0].admission, ProducerAdmission::ExcludedAutomatic);
        assert!(
            control_buff_is_automatically_excluded(&table, ["control_tra_limit&spd3[000]"])
                .unwrap()
        );
        assert!(
            control_buff_is_automatically_excluded(&table, ["control_tra_limit&spd[000]"]).unwrap()
        );
        assert!(
            !control_buff_is_automatically_excluded(&table, ["control_tra_limit&spd2[000]"])
                .unwrap()
        );
    }

    #[test]
    fn producer_rule_catalog_rejects_unimplemented_admission() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let mut catalog = producer_rule_catalog(&table).unwrap().clone();
        catalog.rules[0].admission = ProducerAdmission::PlanRequired;
        assert!(validate_producer_rule_catalog(&catalog, &table)
            .unwrap_err()
            .contains("unsupported admission"));
    }

    #[test]
    fn report_finds_external_trade_and_manufacture_dependencies() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let report = build_response_dependency_report(&table);
        assert!(report.atom_count > 0);
        assert!(report.external_atom_count > 0);
        assert!(report.rows.iter().any(|row| {
            row.target_facility == "trade"
                && row.selector.as_deref() == Some("tagged_count_in_current_trade_room")
        }));
        assert!(report.resource_conversions.iter().any(|conversion| {
            conversion.from == "dream"
                && conversion.to == "perception"
                && conversion.provider_buff_id == "dorm_rec_bd_n1_n2[000]"
                && conversion.converter_buff_id == "dorm_rec_bd_n1_n2[000]"
        }));
        assert!(report.rows.iter().any(|row| {
            row.skill_id == "trade_ord_spd_bd[010]"
                && row
                    .reads_resources
                    .iter()
                    .any(|resource| resource == "silent_echo")
        }));
        assert!(report.rows.iter().any(|row| {
            row.target_facility == "manufacture"
                && row.selector.as_deref() == Some("tagged_count_in_manu_sum")
        }));
        let trade_efficiency = report
            .resource_reverse_closures
            .iter()
            .find(|closure| {
                closure.target_facility == "trade"
                    && closure.response_field == ResponseField::Efficiency
            })
            .expect("trade efficiency resource closure");
        for resource in [
            "silent_echo",
            "perception",
            "passion",
            "dream",
            "musical_section",
            "memory_fragment",
        ] {
            assert!(
                trade_efficiency
                    .resources
                    .iter()
                    .any(|item| item == resource),
                "trade closure missing {resource}"
            );
        }
        assert!(trade_efficiency.edges.iter().any(|edge| {
            edge.kind == ResourceClosureEdgeKind::RegistryConversion
                && edge.from.as_deref() == Some("dream")
                && edge.to == "perception"
                && edge.requires_same_shift_activation
        }));
        let manufacture_efficiency = report
            .resource_reverse_closures
            .iter()
            .find(|closure| {
                closure.target_facility == "manufacture"
                    && closure.response_field == ResponseField::Efficiency
            })
            .expect("manufacture efficiency resource closure");
        for resource in ["thought_chain_ring", "perception", "virtual_power"] {
            assert!(
                manufacture_efficiency
                    .resources
                    .iter()
                    .any(|item| item == resource),
                "manufacture closure missing {resource}"
            );
        }
        assert!(report.resource_value_domains.iter().any(|fact| {
            fact.skill_id == "trade_ord_spd_bd[010]"
                && fact.resource == "silent_echo"
                && matches!(
                    fact.formula,
                    ResourceReadFormula::FloorDivideMultiplier {
                        div: 2.0,
                        multiplier: 1.0
                    }
                )
        }));
        assert!(report.resource_value_domains.iter().any(|fact| {
            fact.skill_id == "control_prod_bd_spd[000]"
                && fact.resource == "passion"
                && matches!(
                    fact.formula,
                    ResourceReadFormula::FloorDivideMultiplier {
                        div: 20.0,
                        multiplier: 0.5
                    }
                )
        }));
        assert!(report.resource_value_domains.iter().any(|fact| {
            fact.resource == "virtual_power"
                && matches!(
                    fact.formula,
                    ResourceReadFormula::IntegerTruncateSaturatingAdd {
                        cap: 255,
                        requires_physical_base: true,
                        ..
                    }
                )
        }));
        for resource in [
            "real_gold_lines",
            "virtual_gold_lines",
            "durin_virtual_lines",
        ] {
            assert!(report
                .unresolved_delegated_dependencies
                .iter()
                .any(|item| { item.mechanism == "gold_flow" && item.resource == resource }));
        }
    }

    #[test]
    fn response_dependency_keeps_stable_atom_id() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let report = build_response_dependency_report(&table);
        let row = report
            .rows
            .iter()
            .find(|row| row.skill_id == "control_mp_psk[000]")
            .expect("Pinus control response rows");
        assert_eq!(
            row.atom_id.as_deref(),
            Some("control_mp_psk[000]:pinus_battle_record")
        );
        assert!(report
            .resource_reverse_closures
            .iter()
            .flat_map(|closure| &closure.edges)
            .any(
                |edge| edge.kind == ResourceClosureEdgeKind::RegistryConversion
                    && edge.atom_id.is_none()
            ));
    }

    #[test]
    fn legacy_atoms_without_ids_keep_distinct_internal_closure_edges() {
        let row = |skill_id: &str,
                   atom_position: usize,
                   reads_resources: Vec<String>,
                   writes_resources: Vec<String>| ResponseDependencyRow {
            skill_id: skill_id.to_string(),
            source_facility: "trade".to_string(),
            target_facility: "trade".to_string(),
            atom_id: None,
            selector: None,
            selector_scope: None,
            condition: None,
            condition_scope: None,
            action_dependency: "test".to_string(),
            action_scope: DependencyScope::RoomLocal,
            reads_resources,
            writes_resources,
            atom_scope: "room".to_string(),
            response_field: ResponseField::Efficiency,
            requires_external_state: false,
            atom_position,
        };
        let rows = vec![
            row("consumer", 0, vec!["legacy_resource".to_string()], vec![]),
            row(
                "legacy_multi",
                0,
                vec![],
                vec!["legacy_resource".to_string()],
            ),
            row(
                "legacy_multi",
                1,
                vec![],
                vec!["legacy_resource".to_string()],
            ),
        ];
        let closures = build_resource_reverse_closures(&rows, &[]);
        let edges = &closures[0].edges;
        assert_eq!(
            edges
                .iter()
                .filter(|edge| edge.skill_id.as_deref() == Some("legacy_multi"))
                .count(),
            2
        );
        assert!(edges
            .iter()
            .filter(|edge| edge.skill_id.as_deref() == Some("legacy_multi"))
            .all(|edge| edge.atom_id.is_none()));
    }

    #[test]
    fn standard_243_compiles_finite_gold_flow_external_domains() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();
        let report = build_response_dependency_report_for_blueprint(&table, &blueprint);
        let scenario = report.scenario.as_ref().expect("scenario metadata");
        assert_eq!(scenario.gold_manu_line_count, 2);
        assert_eq!(scenario.initial_virtual_gold_lines, 0);

        let gold_flow = report
            .domain_dependency_contributors
            .iter()
            .find(|contributor| contributor.mechanism == "gold_flow")
            .expect("gold flow contributor");
        let values = |name: &str| {
            gold_flow
                .inputs
                .iter()
                .find(|input| input.name == name)
                .and_then(|input| input.scenario_values.clone())
                .expect("finite scenario input")
        };
        assert_eq!(values("real_gold_lines"), vec![2]);
        assert_eq!(values("virtual_gold_lines"), vec![0]);
        assert_eq!(values("durin_virtual_lines"), vec![0, 1, 2, 3, 4]);
        assert!(report.unresolved_delegated_dependencies.is_empty());

        let class_count = |resource: &str| {
            report
                .resource_equivalence_classes
                .iter()
                .filter(|class| class.resource == resource)
                .count()
        };
        assert_eq!(class_count("real_gold_lines"), 1);
        assert_eq!(class_count("virtual_gold_lines"), 1);
        assert_eq!(class_count("durin_virtual_lines"), 5);
        assert!(report
            .resource_reachable_ranges
            .iter()
            .filter(|range| !matches!(
                range.resource.as_str(),
                "real_gold_lines" | "virtual_gold_lines" | "durin_virtual_lines"
            ))
            .all(|range| range.max.is_none()));
    }

    #[test]
    fn global_inject_targets_production_facility() {
        assert_eq!(
            target_facility(
                "control",
                &Action::GlobalInjectTradeEff { value: 7.0 },
                AtomScope::Room,
            ),
            "trade"
        );
        assert_eq!(
            target_facility(
                "control",
                &Action::GlobalInjectManuEff {
                    value: 2.0,
                    recipe: None,
                },
                AtomScope::Room,
            ),
            "manufacture"
        );
        let injects = [
            Action::GlobalInjectTradeEff { value: 7.0 },
            Action::GlobalInjectManuEff {
                value: 2.0,
                recipe: None,
            },
            Action::GlobalInjectManuTaggedEff {
                value: 3.0,
                target_tag: "test".to_string(),
                recipe: None,
            },
            Action::GlobalInjectKarlanPrecision {
                eff_per_karlan: 5.0,
                limit_per_karlan: 1,
            },
        ];
        for action in injects {
            assert_eq!(
                action_dependency(&action, AtomScope::Room).1,
                DependencyScope::CrossFacility
            );
        }
    }

    #[test]
    fn action_implicit_inputs_are_never_false_local() {
        assert_eq!(
            action_dependency(&Action::AddPerGapEff { rate: 1.0 }, AtomScope::Room).1,
            DependencyScope::RuntimeState
        );
        assert_eq!(
            action_dependency(
                &Action::StateConsumeToEff {
                    key: "Matatabi".to_string(),
                    div: 1.0,
                    multiplier: None,
                },
                AtomScope::Room,
            )
            .1,
            DependencyScope::RuntimeState
        );
        let implicit_state_readers = [
            Action::AddLimitFromState {
                key: "Perception".to_string(),
                multiplier: 1.0,
            },
            Action::MoodDrainPerStateStep {
                key: "HumanFireworks".to_string(),
                step_size: 1.0,
                delta_per_step: -0.1,
                scope: crate::types::MoodDrainScope::SelfOp,
            },
            Action::StateConsume {
                key: "Perception".to_string(),
                div: 1.0,
            },
            Action::StateConvert {
                from: "Perception".to_string(),
                to: "SilentEcho".to_string(),
                ratio: 1.0,
            },
        ];
        for action in implicit_state_readers {
            assert_eq!(
                action_dependency(&action, AtomScope::Room).1,
                DependencyScope::RuntimeState
            );
        }
        assert_eq!(
            target_facility(
                "dorm",
                &Action::StateProduce {
                    key: "MonsterCuisine".to_string(),
                    amount: 1.0,
                },
                AtomScope::Global,
            ),
            "global_resource"
        );
        assert_eq!(
            target_facility(
                "trade",
                &Action::StateConsumeToEff {
                    key: "SilentEcho".to_string(),
                    div: 2.0,
                    multiplier: None,
                },
                AtomScope::Room,
            ),
            "trade"
        );
    }
}
