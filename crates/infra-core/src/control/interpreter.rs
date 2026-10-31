use std::collections::HashMap;

use crate::control::input::ControlRoomInput;
use crate::control::{ControlEffectKey, ControlEffectLedger};
use crate::global_resource::GlobalResourcePool;
use crate::global_resource::{
    GlobalInjectManifest, InjectMergeExplanation, ManuInjectCandidate, ManuInjectCandidateKind,
    TradeTaggedCountScope, INJECT_FAMILY_MANU_GLOBAL_ALL, INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
use crate::layout::trade_station_tagged_gte_key;
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, EffectAtom, Phase, RecipeKind, Selector, StateKey};

#[derive(Debug, Clone, Default)]
struct ControlOperatorRuntime {
    name: String,
    buff_ids: Vec<String>,
    tags: Vec<String>,
    mood_drain_delta: f64,
}

#[derive(Debug, Clone)]
struct ControlContext {
    operators: Vec<ControlOperatorRuntime>,
    layout: crate::layout::LayoutContext,
    mood: f64,
    state_pool: HashMap<StateKey, f64>,
    inject: GlobalInjectManifest,
    effect_ledger: ControlEffectLedger,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlCenterResult {
    pub global: GlobalResourcePool,
    pub inject: GlobalInjectManifest,
    /// 中枢干员 L1 心情消耗增量（按名；排班非目标，仅供机制记账）。
    pub operator_mood_drains: HashMap<String, f64>,
    /// 冻结后的中枢全局效果合并事实。
    pub effect_ledger: ControlEffectLedger,
    /// 中枢对人力办公室的联络速度注入。
    pub office_hire_spd_pct: f64,
    /// 中枢对会客室的线索搜集速度注入。
    pub meeting_speed_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlConditionStatus {
    Passed,
    Failed,
    UnhandledAssumedTrue,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ControlTraceEvent {
    pub source_operator: String,
    pub source_buff_id: String,
    pub atom_id: String,
    pub phase: Phase,
    pub phase_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
    pub condition_status: ControlConditionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<Selector>,
    pub action: Action,
    pub outcome: ControlTraceOutcome,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlTraceOutcome {
    ConditionNotMet,
    StateWrite {
        key: StateKey,
        delta: f64,
        before: f64,
        after: f64,
    },
    TradeInject {
        family: String,
        candidate_value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        merge: Option<InjectMergeExplanation>,
    },
    ManufactureInject {
        family: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
        candidate_value: f64,
        candidate_kind: ManuInjectCandidateKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        merge: Option<InjectMergeExplanation>,
    },
    DeferredToTradeRoom {
        family: String,
        target_tag: String,
        value_per_operator: f64,
        count_scope: String,
    },
    DeferredToManufactureRoom {
        target_tag: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        recipe: Option<RecipeKind>,
        value: f64,
    },
    SpecialRule {
        rule: String,
    },
    MoodApplied,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ControlStateValue {
    pub key: StateKey,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ControlSettlementTrace {
    pub events: Vec<ControlTraceEvent>,
    pub final_state: Vec<ControlStateValue>,
}

pub fn solve_control(input: &ControlRoomInput, table: &SkillTable) -> ControlCenterResult {
    solve_control_inner(input, table, false).0
}

pub fn solve_control_with_trace(
    input: &ControlRoomInput,
    table: &SkillTable,
) -> (ControlCenterResult, ControlSettlementTrace) {
    let (result, trace) = solve_control_inner(input, table, true);
    (result, trace.expect("trace requested"))
}

fn solve_control_inner(
    input: &ControlRoomInput,
    table: &SkillTable,
    trace_enabled: bool,
) -> (ControlCenterResult, Option<ControlSettlementTrace>) {
    let mut ctx = ControlContext {
        operators: input
            .operators
            .iter()
            .map(|o| ControlOperatorRuntime {
                name: o.name.clone(),
                buff_ids: o.buff_ids.clone(),
                tags: o.tags.clone(),
                mood_drain_delta: 0.0,
            })
            .collect(),
        layout: input.layout.clone(),
        mood: input.mood,
        state_pool: input.layout.global.to_room_state(),
        inject: GlobalInjectManifest::default(),
        effect_ledger: ControlEffectLedger::default(),
    };
    for operator in &ctx.operators {
        for buff_id in &operator.buff_ids {
            ctx.inject.record_active_source_buff(buff_id);
        }
    }

    let atoms = collect_control_atoms(&ctx.operators, table);
    let mut trace_events = trace_enabled.then(Vec::new);
    for (atom, atom_id, owner, source_buff_id) in atoms {
        let condition_status = evaluate_condition(&atom.condition, &ctx, &owner);
        if condition_status == ControlConditionStatus::Failed {
            if let Some(events) = &mut trace_events {
                events.push(ControlTraceEvent {
                    source_operator: owner,
                    source_buff_id,
                    atom_id,
                    phase: atom.phase,
                    phase_order: atom.phase_order,
                    condition: atom.condition.clone(),
                    condition_status,
                    selector: atom.selector.clone(),
                    action: atom.action.clone(),
                    outcome: ControlTraceOutcome::ConditionNotMet,
                });
            }
            continue;
        }
        let outcome = apply_atom(
            &mut ctx,
            atom,
            &atom_id,
            &owner,
            &source_buff_id,
            trace_enabled,
        );
        if let Some(events) = &mut trace_events {
            events.push(ControlTraceEvent {
                source_operator: owner,
                source_buff_id,
                atom_id,
                phase: atom.phase,
                phase_order: atom.phase_order,
                condition: atom.condition.clone(),
                condition_status,
                selector: atom.selector.clone(),
                action: atom.action.clone(),
                outcome: outcome.unwrap_or(ControlTraceOutcome::Ignored),
            });
        }
    }

    ctx.effect_ledger.freeze();
    if let Some(events) = &mut trace_events {
        finalize_inject_trace(events, &ctx.inject);
    }
    let final_state = trace_enabled.then(|| {
        crate::global_resource::GlobalResourceKey::ALL
            .iter()
            .filter_map(|key| {
                let value = ctx.state_pool.get(key).copied().unwrap_or(0.0);
                (value != 0.0).then_some(ControlStateValue { key: *key, value })
            })
            .collect::<Vec<_>>()
    });

    let mut global = input.layout.global.clone();
    for (key, value) in ctx.state_pool {
        if value != 0.0 {
            global.set(key, value);
        }
    }

    let operator_mood_drains = ctx
        .operators
        .iter()
        .map(|o| (o.name.clone(), o.mood_drain_delta))
        .collect();

    (
        ControlCenterResult {
            global,
            inject: ctx.inject,
            operator_mood_drains,
            office_hire_spd_pct: ctx.effect_ledger.value(ControlEffectKey::OFFICE_HIRE_SPEED),
            meeting_speed_pct: ctx
                .effect_ledger
                .value(ControlEffectKey::MEETING_CLUE_SPEED),
            effect_ledger: ctx.effect_ledger,
        },
        trace_events.map(|events| ControlSettlementTrace {
            events,
            final_state: final_state.unwrap_or_default(),
        }),
    )
}

fn collect_control_atoms<'a>(
    ops: &[ControlOperatorRuntime],
    table: &'a SkillTable,
) -> Vec<(&'a EffectAtom, String, String, String)> {
    let mut atoms = Vec::new();
    for op in ops {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else {
                continue;
            };
            if skill.facility != "control" {
                continue;
            }
            for atom in &skill.atoms {
                let Some(atom_id) = atom.atom_id.as_deref() else {
                    continue;
                };
                atoms.push((atom, atom_id.to_string(), op.name.clone(), bid.clone()));
            }
        }
    }
    atoms.sort_by(|(a, _, _, _), (b, _, _, _)| {
        let pa = a.phase.sort_key();
        let pb = b.phase.sort_key();
        pa.cmp(&pb).then(a.phase_order.cmp(&b.phase_order))
    });
    atoms
}

fn evaluate_condition(
    cond: &Option<Condition>,
    ctx: &ControlContext,
    owner: &str,
) -> ControlConditionStatus {
    let Some(cond) = cond else {
        return ControlConditionStatus::Passed;
    };
    let matched = match cond {
        Condition::MoodAbove { n } => Some(ctx.mood > *n as f64),
        Condition::MoodAboveOrEq { n } => Some(ctx.mood >= *n as f64),
        Condition::MoodBelow { n } => Some(ctx.mood < *n as f64),
        Condition::MoodBelowOrEq { n } => Some(ctx.mood <= *n as f64),
        Condition::OperatorInBase { name } => {
            Some(ctx.layout.base_workforce.iter().any(|n| n == name))
        }
        Condition::OperatorInPower { name } => {
            Some(ctx.layout.power_workforce.iter().any(|n| n == name))
        }
        Condition::OperatorInTraining { name } => {
            Some(ctx.layout.training_assist.iter().any(|n| n == name))
        }
        Condition::NoPlatformInOtherPower {} => Some(!ctx.layout.other_power_has_platform),
        Condition::OtherPlatformInPower {} => Some(ctx.layout.other_platform_in_power),
        Condition::OtherLateranoInPower {} => Some(ctx.layout.other_laterano_in_power),
        Condition::PartnerInRoom { name } => Some(ctx.operators.iter().any(|o| o.name == *name)),
        Condition::TagPresentInRoom { tag } => Some(
            ctx.operators
                .iter()
                .any(|o| o.tags.iter().any(|t| t == tag)),
        ),
        Condition::PeerTagInRoom { tag } => Some(
            ctx.operators
                .iter()
                .any(|o| o.name != owner && o.tags.iter().any(|t| t == tag)),
        ),
        Condition::ExternalMomentumGteField {} => {
            Some(ctx.layout.external_momentum() >= ctx.layout.field_momentum())
        }
        Condition::FieldMomentumGtExternal {} => {
            Some(ctx.layout.field_momentum() > ctx.layout.external_momentum())
        }
        Condition::OfficeHireSpeedBelow { n } => Some(
            5.0 + ctx.layout.control_office_hire_spd_pct + ctx.layout.office_hire_spd_pct
                < *n as f64,
        ),
        Condition::PlatformCountGte { min } => Some(ctx.layout.platform_count_in_power >= *min),
        _ => None,
    };
    match matched {
        Some(true) => ControlConditionStatus::Passed,
        Some(false) => ControlConditionStatus::Failed,
        None => ControlConditionStatus::UnhandledAssumedTrue,
    }
}

fn scaled_inject_value(ctx: &ControlContext, atom: &EffectAtom, base: f64) -> f64 {
    if atom.selector.is_some() {
        resolve_selector_value(ctx, atom.selector.as_ref()) * base
    } else {
        base
    }
}

fn apply_atom(
    ctx: &mut ControlContext,
    atom: &EffectAtom,
    atom_id: &str,
    owner: &str,
    source_buff_id: &str,
    trace_enabled: bool,
) -> Option<ControlTraceOutcome> {
    match atom.phase {
        Phase::StateWrite => apply_state_write(ctx, atom, trace_enabled),
        Phase::GlobalInject => {
            apply_global_inject(ctx, atom, atom_id, owner, source_buff_id, trace_enabled)
        }
        Phase::Mood => {
            apply_mood_action(ctx, &atom.action, owner);
            trace_enabled.then_some(ControlTraceOutcome::MoodApplied)
        }
        _ => trace_enabled.then_some(ControlTraceOutcome::Ignored),
    }
}

fn apply_mood_action(ctx: &mut ControlContext, action: &Action, owner: &str) {
    match action {
        Action::MoodDrainDelta { delta, scope } => match scope {
            crate::types::MoodDrainScope::SelfOp => {
                if let Some(op) = ctx.operators.iter_mut().find(|o| o.name == owner) {
                    op.mood_drain_delta += delta;
                }
            }
            crate::types::MoodDrainScope::RoomOperators => {
                for op in &mut ctx.operators {
                    op.mood_drain_delta += delta;
                }
            }
        },
        Action::MoodDrainDeltaForOperator { operator, delta } => {
            if let Some(op) = ctx.operators.iter_mut().find(|op| op.name == *operator) {
                op.mood_drain_delta += delta;
            }
        }
        Action::MoodDrainPerStateStep {
            key,
            step_size,
            delta_per_step,
            scope,
        } => {
            let Some(sk) = StateKey::parse(key) else {
                return;
            };
            let state = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            let steps = if *step_size > 0.0 {
                (state / step_size).floor()
            } else {
                0.0
            };
            let delta = steps * delta_per_step;
            match scope {
                crate::types::MoodDrainScope::SelfOp => {
                    if let Some(op) = ctx.operators.iter_mut().find(|o| o.name == owner) {
                        op.mood_drain_delta += delta;
                    }
                }
                crate::types::MoodDrainScope::RoomOperators => {
                    for op in &mut ctx.operators {
                        op.mood_drain_delta += delta;
                    }
                }
            }
        }
        Action::ClearMoodDrain { scope } => match scope {
            crate::types::MoodDrainScope::SelfOp => {
                if let Some(op) = ctx.operators.iter_mut().find(|o| o.name == owner) {
                    op.mood_drain_delta = 0.0;
                }
            }
            crate::types::MoodDrainScope::RoomOperators => {
                for op in &mut ctx.operators {
                    op.mood_drain_delta = 0.0;
                }
            }
        },
        _ => {}
    }
}

fn apply_state_write(
    ctx: &mut ControlContext,
    atom: &EffectAtom,
    trace_enabled: bool,
) -> Option<ControlTraceOutcome> {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            let Some(sk) = StateKey::parse(key) else {
                return trace_enabled.then_some(ControlTraceOutcome::Ignored);
            };
            let scale = resolve_selector_value(ctx, atom.selector.as_ref());
            let add = if atom.selector.is_some() {
                scale * amount
            } else {
                *amount
            };
            let before = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            let after = before + add;
            ctx.state_pool.insert(sk, after);
            trace_enabled.then_some(ControlTraceOutcome::StateWrite {
                key: sk,
                delta: add,
                before,
                after,
            })
        }
        _ => trace_enabled.then_some(ControlTraceOutcome::Ignored),
    }
}

fn apply_global_inject(
    ctx: &mut ControlContext,
    atom: &EffectAtom,
    atom_id: &str,
    owner: &str,
    source_buff_id: &str,
    trace_enabled: bool,
) -> Option<ControlTraceOutcome> {
    ctx.inject.record_active_source_buff(source_buff_id);
    let family = atom.tag.as_deref().unwrap_or(match &atom.action {
        Action::GlobalInjectTradeEff { .. } => INJECT_FAMILY_TRADE_GLOBAL_FLAT,
        Action::GlobalInjectManuEff { .. } => INJECT_FAMILY_MANU_GLOBAL_ALL,
        Action::GlobalInjectManuTaggedEff { .. } => "manu_tagged",
        Action::GlobalInjectOfficeHireSpeed { .. } => "office_hire_speed",
        Action::GlobalInjectMeetingSpeed { .. } => "meeting_speed",
        _ => "default",
    });
    match &atom.action {
        Action::GlobalInjectOfficeHireSpeed { value } => {
            ctx.effect_ledger.record_max(
                owner,
                source_buff_id,
                atom_id,
                ControlEffectKey::OFFICE_HIRE_SPEED,
                *value,
            );
            return trace_enabled.then_some(ControlTraceOutcome::SpecialRule {
                rule: "control_office_hire_speed".to_string(),
            });
        }
        Action::GlobalInjectMeetingSpeed { value } => {
            ctx.effect_ledger.record_max(
                owner,
                source_buff_id,
                atom_id,
                ControlEffectKey::MEETING_CLUE_SPEED,
                *value,
            );
            return trace_enabled.then_some(ControlTraceOutcome::SpecialRule {
                rule: "control_meeting_speed".to_string(),
            });
        }
        Action::GlobalInjectTradeEff { value } => {
            let tagged_rule = match atom.selector.as_ref() {
                Some(Selector::TaggedCountInTradeSum { tag }) => Some((
                    tag,
                    ctx.layout
                        .trade_tagged_count_sum
                        .get(tag)
                        .copied()
                        .unwrap_or(0),
                    TradeTaggedCountScope::AllTradeRooms,
                )),
                Some(Selector::TaggedCountInCurrentTradeRoom { tag }) => {
                    Some((tag, 0, TradeTaggedCountScope::CurrentTradeRoom))
                }
                Some(Selector::TradeStationsWithTaggedGte { tag, min }) => Some((
                    tag,
                    ctx.layout
                        .trade_stations_tagged_gte
                        .get(&trade_station_tagged_gte_key(tag, *min))
                        .copied()
                        .unwrap_or(0),
                    TradeTaggedCountScope::QualifiedTradeRooms { min: *min },
                )),
                _ => None,
            };
            if let Some((tag, resolved_count, count_scope)) = tagged_rule {
                ctx.inject.record_trade_tagged(
                    owner,
                    source_buff_id,
                    family,
                    tag,
                    *value,
                    resolved_count,
                    count_scope,
                );
                if !trace_enabled {
                    return None;
                }
                if matches!(count_scope, TradeTaggedCountScope::CurrentTradeRoom) {
                    return Some(ControlTraceOutcome::DeferredToTradeRoom {
                        family: family.to_string(),
                        target_tag: tag.to_string(),
                        value_per_operator: *value,
                        count_scope: "current_trade_room".to_string(),
                    });
                }
                return Some(ControlTraceOutcome::TradeInject {
                    family: family.to_string(),
                    candidate_value: f64::from(resolved_count) * *value,
                    merge: None,
                });
            } else {
                let v = scaled_inject_value(ctx, atom, *value);
                if v != 0.0 {
                    ctx.inject.record_trade(family, v);
                    ctx.effect_ledger.record_max(
                        owner,
                        source_buff_id,
                        atom_id,
                        ControlEffectKey::TRADE_GLOBAL_EFFICIENCY,
                        v,
                    );
                }
                return trace_enabled.then_some(ControlTraceOutcome::TradeInject {
                    family: family.to_string(),
                    candidate_value: v,
                    merge: None,
                });
            }
        }
        Action::GlobalInjectManuEff { value, recipe } => {
            if let Some(Selector::TaggedCountInManuSum { tag }) = atom.selector.as_ref() {
                let resolved_count = ctx
                    .layout
                    .manu_tagged_count_sum
                    .get(tag)
                    .copied()
                    .unwrap_or(0);
                ctx.inject.record_manu_count_scaled(
                    owner,
                    source_buff_id,
                    family,
                    tag,
                    *recipe,
                    *value,
                    resolved_count,
                );
                return trace_enabled.then_some(ControlTraceOutcome::ManufactureInject {
                    family: family.to_string(),
                    recipe: *recipe,
                    candidate_value: f64::from(resolved_count) * *value,
                    candidate_kind: ManuInjectCandidateKind::CountScaled,
                    merge: None,
                });
            } else {
                let v = scaled_inject_value(ctx, atom, *value);
                if v != 0.0 {
                    ctx.inject.record_manu(family, *recipe, v);
                    if recipe.is_none_or(|recipe| recipe == RecipeKind::All) && v > 0.0 {
                        ctx.effect_ledger.record_max(
                            owner,
                            source_buff_id,
                            atom_id,
                            ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY,
                            v,
                        );
                    }
                }
                return trace_enabled.then_some(ControlTraceOutcome::ManufactureInject {
                    family: family.to_string(),
                    recipe: *recipe,
                    candidate_value: v,
                    candidate_kind: ManuInjectCandidateKind::Static,
                    merge: None,
                });
            }
        }
        Action::GlobalInjectManuTaggedEff {
            value,
            target_tag,
            recipe,
        } => {
            let v = scaled_inject_value(ctx, atom, *value);
            if v != 0.0 {
                ctx.inject
                    .record_manu_tagged(source_buff_id, target_tag, *recipe, v);
            }
            return trace_enabled.then_some(ControlTraceOutcome::DeferredToManufactureRoom {
                target_tag: target_tag.clone(),
                recipe: *recipe,
                value: v,
            });
        }
        Action::GlobalInjectKarlanPrecision {
            eff_per_karlan,
            limit_per_karlan,
        } => {
            ctx.inject
                .record_karlan_precision(*eff_per_karlan, *limit_per_karlan);
            return trace_enabled.then_some(ControlTraceOutcome::SpecialRule {
                rule: "karlan_precision".to_string(),
            });
        }
        _ => {}
    }
    trace_enabled.then_some(ControlTraceOutcome::Ignored)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceInjectTarget {
    Trade,
    Manufacture(Option<RecipeKind>),
}

#[derive(Debug, Clone)]
struct TraceInjectCandidate {
    target: TraceInjectTarget,
    family: String,
    value: f64,
    manu_kind: Option<ManuInjectCandidateKind>,
}

fn finalize_inject_trace(events: &mut [ControlTraceEvent], inject: &GlobalInjectManifest) {
    // Atom events are captured in execution order; family classification waits until every
    // candidate is known so max, ties and additive adjustments use the complete set.
    let candidates = events
        .iter()
        .map(|event| trace_inject_candidate(&event.outcome))
        .collect::<Vec<_>>();
    for (index, event) in events.iter_mut().enumerate() {
        let Some(candidate) = candidates[index].as_ref() else {
            continue;
        };
        let explanation = match candidate.target {
            TraceInjectTarget::Trade => {
                let tied = candidates.iter().enumerate().any(|(other_index, other)| {
                    other_index != index
                        && other.as_ref().is_some_and(|other| {
                            other.target == candidate.target
                                && other.family == candidate.family
                                && (other.value - candidate.value).abs() < f64::EPSILON
                        })
                });
                inject.explain_resolved_trade_candidate(&candidate.family, candidate.value, tied)
            }
            TraceInjectTarget::Manufacture(recipe) => {
                let group = candidates
                    .iter()
                    .enumerate()
                    .filter_map(|(event_index, other)| {
                        let other = other.as_ref()?;
                        if other.target == TraceInjectTarget::Manufacture(recipe)
                            && other.family == candidate.family
                        {
                            Some((
                                event_index,
                                ManuInjectCandidate {
                                    kind: other.manu_kind?,
                                    value: other.value,
                                },
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                let position = group
                    .iter()
                    .position(|(event_index, _)| *event_index == index)
                    .expect("current manufacture candidate in group");
                GlobalInjectManifest::explain_manu_candidates(
                    &group
                        .iter()
                        .map(|(_, candidate)| *candidate)
                        .collect::<Vec<_>>(),
                )[position]
                    .clone()
            }
        };
        match &mut event.outcome {
            ControlTraceOutcome::TradeInject { merge, .. }
            | ControlTraceOutcome::ManufactureInject { merge, .. } => {
                *merge = Some(explanation);
            }
            _ => {}
        }
    }
}

fn trace_inject_candidate(outcome: &ControlTraceOutcome) -> Option<TraceInjectCandidate> {
    match outcome {
        ControlTraceOutcome::TradeInject {
            family,
            candidate_value,
            ..
        } => Some(TraceInjectCandidate {
            target: TraceInjectTarget::Trade,
            family: family.clone(),
            value: *candidate_value,
            manu_kind: None,
        }),
        ControlTraceOutcome::ManufactureInject {
            family,
            recipe,
            candidate_value,
            candidate_kind,
            ..
        } => Some(TraceInjectCandidate {
            target: TraceInjectTarget::Manufacture(*recipe),
            family: family.clone(),
            value: *candidate_value,
            manu_kind: Some(*candidate_kind),
        }),
        _ => None,
    }
}

fn resolve_selector_value(ctx: &ControlContext, selector: Option<&Selector>) -> f64 {
    match selector {
        Some(Selector::TaggedCountInControl { tag }) => ctx
            .operators
            .iter()
            .filter(|o| o.tags.iter().any(|t| t == tag))
            .count() as f64,
        Some(Selector::ControlOperatorCount) => ctx.operators.len() as f64,
        Some(Selector::SuiFacilityCount) => f64::from(ctx.layout.sui_facility_count),
        Some(Selector::CappedSuiFacilityCount { max }) => {
            f64::from(ctx.layout.sui_facility_count.min(*max))
        }
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::PlatformCountInPower) => f64::from(ctx.layout.platform_count_in_power),
        Some(Selector::TaggedCountInTradeSum { tag }) => {
            f64::from(*ctx.layout.trade_tagged_count_sum.get(tag).unwrap_or(&0))
        }
        Some(Selector::TaggedCountInCurrentTradeRoom { .. }) => 0.0,
        Some(Selector::TradeStationsWithTaggedGte { tag, min }) => f64::from(
            *ctx.layout
                .trade_stations_tagged_gte
                .get(&trade_station_tagged_gte_key(tag, *min))
                .unwrap_or(&0),
        ),
        Some(Selector::TaggedCountInManuSum { tag }) => {
            f64::from(*ctx.layout.manu_tagged_count_sum.get(tag).unwrap_or(&0))
        }
        Some(Selector::StatePoolFloored { key, div }) => {
            let Some(sk) = StateKey::parse(key) else {
                return 0.0;
            };
            let v = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            if *div <= 0.0 {
                0.0
            } else {
                (v / div).floor()
            }
        }
        _ => 0.0,
    }
}
