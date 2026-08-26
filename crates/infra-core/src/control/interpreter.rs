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
            &atom,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::input::ControlOperator;
    use crate::global_resource::GlobalResourceKey;
    use crate::instances::{default_instances_path, resolve_buff_ids, OperatorInstances};
    use crate::layout::LayoutContext;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    fn instances() -> OperatorInstances {
        OperatorInstances::load(&default_instances_path().unwrap()).unwrap()
    }

    fn control_op(name: &str, elite: u8) -> ControlOperator {
        let inst = instances();
        let tier = PromotionTier::from_elite(elite);
        let binding = inst
            .get(name, tier)
            .and_then(|i| i.facilities.get("control"));
        let tier0 = inst
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("control"));
        let buff_ids = binding
            .map(|b| resolve_buff_ids(tier, b, tier0))
            .unwrap_or_default();
        let tags = inst
            .get(name, tier)
            .map(|i| i.tags.clone())
            .unwrap_or_default();
        ControlOperator {
            name: name.into(),
            elite,
            buff_ids,
            tags,
        }
    }

    #[test]
    fn amiya_global_trade_plus_seven() {
        let input = ControlRoomInput::with_operators(vec![control_op("阿米娅", 0)]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ledger_records_stable_control_atom_id() {
        let result = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("阿米娅", 0)]),
            &table(),
        );
        let record = result
            .effect_ledger
            .records()
            .iter()
            .find(|record| record.source_buff_id == "control_tra_spd[000]")
            .expect("trade ledger record");
        assert_eq!(record.atom_id, "control_tra_spd[000]:trade_efficiency");
    }

    #[test]
    fn office_hire_speed_uses_max_and_saileach_is_conditional_unique_effect() {
        let base = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("焰狐龙梓兰", 2)]),
            &table(),
        );
        assert_eq!(base.office_hire_spd_pct, 10.0);

        let max = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("焰狐龙梓兰", 2),
                control_op("八幡海铃", 2),
            ]),
            &table(),
        );
        assert_eq!(max.office_hire_spd_pct, 10.0);

        let saileach = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("琴柳", 2)]),
            &table(),
        );
        assert_eq!(saileach.office_hire_spd_pct, 20.0);

        let mut layout = LayoutContext::default();
        layout.office_hire_spd_pct = 25.0;
        let inactive = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("琴柳", 2)],
                mood: 24.0,
                layout,
            },
            &table(),
        );
        assert_eq!(inactive.office_hire_spd_pct, 0.0);
    }

    #[test]
    fn trade_global_flat_takes_max_not_sum() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("阿米娅", 0),
            control_op("诗怀雅", 0),
        ]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn starbear_with_lgd_peer_manu_plus_three() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("斩业星熊", 2),
            control_op("诗怀雅", 0),
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 3.0).abs() < f64::EPSILON,
            "manu={}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn ledger_marks_star_bear_as_global_manu_winner_over_kaltsit() {
        let result = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("斩业星熊", 2),
                control_op("陈", 2),
                control_op("凯尔希", 2),
            ]),
            &table(),
        );
        assert_eq!(
            result
                .effect_ledger
                .value(ControlEffectKey::MANUFACTURE_GLOBAL_PRODUCTIVITY),
            3.0
        );
        let kaltsit = result
            .effect_ledger
            .records()
            .iter()
            .find(|record| record.source_operator == "凯尔希")
            .expect("凯尔希全制造 atom");
        assert_eq!(
            kaltsit.status,
            crate::control::ControlEffectStatus::Suppressed
        );
    }

    #[test]
    fn haru_trade_tagged_inject_is_delayed_and_resolves_current_trade_room() {
        let result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("八幡海铃", 2)],
                mood: 24.0,
                layout: LayoutContext::default(),
            },
            &table(),
        );
        assert!(result.inject.has_dynamic_trade_inject());
        assert_eq!(result.inject.trade_tagged().len(), 1);
        assert_eq!(
            result.inject.trade_tagged()[0].count_scope,
            TradeTaggedCountScope::CurrentTradeRoom
        );

        // 八幡海铃只按各贸易房站内叙拉古人数结算；全站 3 名不得跨站串入统一全局值。
        let all_counts = std::collections::HashMap::from([("cc.g.siracusa".to_string(), 3)]);
        for (in_room, expected) in [(0, 0.0), (1, 5.0), (2, 10.0)] {
            let room = if in_room == 0 {
                std::collections::HashMap::new()
            } else {
                std::collections::HashMap::from([("cc.g.siracusa".to_string(), in_room)])
            };
            assert_eq!(
                result
                    .inject
                    .trade_eff_pct_with_scoped_tag_counts(&all_counts, &room),
                expected,
                "站内 {in_room} 名叙拉古 → {expected}%"
            );
        }
    }

    #[test]
    fn daifeen_trade_inject_is_delayed_to_the_current_trade_room() {
        let mut layout = LayoutContext::default();
        layout
            .trade_tagged_count_sum
            .insert("cc.g.glasgow".to_string(), 3);
        let result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("戴菲恩", 2)],
                mood: 24.0,
                layout,
            },
            &table(),
        );

        assert!(result.inject.has_dynamic_trade_inject());
        assert_eq!(result.inject.trade_eff_pct(), 0.0);
        assert_eq!(result.inject.trade_tagged().len(), 1);
        assert_eq!(
            result.inject.trade_tagged()[0].count_scope,
            TradeTaggedCountScope::CurrentTradeRoom
        );
    }

    #[test]
    fn silver_trade_threshold_is_preserved_as_a_dynamic_rule() {
        let mut layout = LayoutContext::default();
        layout
            .trade_stations_tagged_gte
            .insert(trade_station_tagged_gte_key("cc.g.karlan", 3), 2);
        let result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("凛御银灰", 2)],
                mood: 24.0,
                layout,
            },
            &table(),
        );
        assert_eq!(result.inject.trade_eff_pct(), 20.0);
        assert!(matches!(
            result.inject.trade_tagged()[0].count_scope,
            TradeTaggedCountScope::QualifiedTradeRooms { min: 3 }
        ));
    }

    #[test]
    fn jessica_manufacture_count_is_delayed_to_actual_workforce() {
        let mut layout = LayoutContext::default();
        layout
            .manu_tagged_count_sum
            .insert("cc.g.blacksteel".to_string(), 2);
        let result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("涤火杰西卡", 2)],
                mood: 24.0,
                layout,
            },
            &table(),
        );
        assert_eq!(
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold),
            10.0
        );
        assert_eq!(result.inject.manu_count_scaled().len(), 1);
    }

    #[test]
    fn mon3tr_global_manu_plus_two() {
        let input = ControlRoomInput::with_operators(vec![control_op("Mon3tr", 2)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn kaltsit_global_manu_plus_two() {
        let input = ControlRoomInput::with_operators(vec![control_op("凯尔希", 2)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn saria_virtual_power_when_lancet_in_power() {
        let mut layout = LayoutContext::default();
        layout.power_workforce = vec!["Lancet-2".into()];
        let input = ControlRoomInput {
            operators: vec![control_op("森蚺", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!((result.global.get(GlobalResourceKey::VirtualPower) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn matatabi_from_monhun_ops() {
        let layout = LayoutContext::default();
        let input = ControlRoomInput {
            operators: vec![control_op("火龙S黑角", 0), control_op("麒麟R夜刀", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        // 黑角 2×2 + 夜刀 8
        assert!((result.global.get(GlobalResourceKey::Matatabi) - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn kirin_yato_stamina_mood_drain() {
        let input = ControlRoomInput::with_operators(vec![control_op("麒麟R夜刀", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.operator_mood_drains["麒麟R夜刀"] - 0.5).abs() < f64::EPSILON,
            "mood={}",
            result.operator_mood_drains["麒麟R夜刀"]
        );
    }

    #[test]
    fn monhun_tier2_global_inject_requires_peer() {
        let solo = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("火龙S黑角", 2)]),
            &table(),
        );
        assert!(solo.inject.trade_eff_pct().abs() < f64::EPSILON);

        let duo = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("火龙S黑角", 2),
                control_op("麒麟R夜刀", 2),
            ]),
            &table(),
        );
        assert!((duo.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
        assert!(
            (duo.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn monhun_trade_global_flat_max_with_amiya() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("阿米娅", 0),
            control_op("火龙S黑角", 2),
            control_op("麒麟R夜刀", 2),
        ]);
        let result = solve_control(&input, &table());
        assert!((result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ling_e2_produces_human_fireworks_at_high_mood() {
        let input = ControlRoomInput::with_operators(vec![control_op("令", 2)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::HumanFireworks) - 15.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::HumanFireworks)
        );
    }

    #[test]
    fn chongyue_sui_facility_produces_fireworks() {
        let mut layout = LayoutContext::default();
        layout.sui_facility_count = 2;
        let input = ControlRoomInput {
            operators: vec![control_op("重岳", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::HumanFireworks) - 10.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::HumanFireworks)
        );
    }

    #[test]
    fn tucha_usaut_drink_from_tagged_peer_in_control() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("战车", 2),
            ControlOperator {
                name: "凛冬".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.usaut".into()],
            },
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::UsautDrink) - 1.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::UsautDrink)
        );
    }

    #[test]
    fn wang_quanbian_trade_when_external_gte_field() {
        let mut layout = LayoutContext::default();
        layout.trade_station_count = 2;
        layout.power_station_count = 3;
        layout.manufacture_station_count = 4;
        let input = ControlRoomInput {
            operators: vec![control_op("望", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.trade_eff_pct() - 7.0).abs() < f64::EPSILON,
            "243-like 5>=4 → trade +7%, got {}",
            result.inject.trade_eff_pct()
        );
        assert!(
            result
                .inject
                .manu_eff_for(crate::types::RecipeKind::Gold)
                .abs()
                < f64::EPSILON,
            "manu inject should be 0, got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn wang_quanbian_manu_when_field_gt_external() {
        let mut layout = LayoutContext::default();
        layout.trade_station_count = 1;
        layout.power_station_count = 1;
        layout.manufacture_station_count = 4;
        let input = ControlRoomInput {
            operators: vec![control_op("望", 0)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(result.inject.trade_eff_pct().abs() < f64::EPSILON);
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON,
            "2<4 → manu +2%, got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn kjera_precision_calculation_records_karlan_rule() {
        let input = ControlRoomInput::with_operators(vec![control_op("灵知", 2)]);
        let result = solve_control(&input, &table());
        let kp = result
            .inject
            .karlan_precision()
            .expect("精密计算应写入 manifest");
        assert!((kp.eff_per_karlan + 15.0).abs() < f64::EPSILON);
        assert_eq!(kp.limit_per_karlan, 6);
    }

    #[test]
    fn xi_perception_when_mood_above_twelve() {
        let input = ControlRoomInput::with_operators(vec![control_op("夕", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::Perception) - 10.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::Perception)
        );
        assert!(
            (result.operator_mood_drains["夕"] - 0.5).abs() < f64::EPSILON,
            "mood={}",
            result.operator_mood_drains["夕"]
        );
    }

    #[test]
    fn sui_mood_boundary_at_twelve() {
        let layout = LayoutContext::default();
        let ling = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("令", 2)],
                mood: 12.0,
                layout: layout.clone(),
            },
            &table(),
        );
        assert_eq!(ling.global.get(GlobalResourceKey::Perception), 10.0);
        assert_eq!(ling.global.get(GlobalResourceKey::HumanFireworks), 0.0);

        let xi = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("夕", 2)],
                mood: 12.0,
                layout,
            },
            &table(),
        );
        assert_eq!(xi.global.get(GlobalResourceKey::Perception), 10.0);
        assert_eq!(xi.global.get(GlobalResourceKey::HumanFireworks), 0.0);
    }

    #[test]
    fn ash_intelligence_reserve_from_rainbow_peer() {
        let input = ControlRoomInput::with_operators(vec![
            control_op("灰烬", 2),
            ControlOperator {
                name: "闪击".into(),
                elite: 2,
                buff_ids: vec![],
                tags: vec!["cc.g.rainbow".into()],
            },
        ]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::IntelligenceReserve) - 2.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::IntelligenceReserve)
        );
    }

    #[test]
    fn muzu_passion_trade_and_mood_step() {
        let input = ControlRoomInput::with_operators(vec![control_op("若叶睦", 0)]);
        let result = solve_control(&input, &table());
        assert!(
            (result.global.get(GlobalResourceKey::Passion) - 20.0).abs() < f64::EPSILON,
            "got {}",
            result.global.get(GlobalResourceKey::Passion)
        );
        assert!(
            (result.inject.trade_eff_pct() - 2.0).abs() < f64::EPSILON,
            "trade inject {}",
            result.inject.trade_eff_pct()
        );
        assert!(
            (result.operator_mood_drains["若叶睦"] - 0.02).abs() < f64::EPSILON,
            "mood {}",
            result.operator_mood_drains["若叶睦"]
        );
    }

    #[test]
    fn wakamugi_control_data_produces_passion_and_e2_pair_mood_delta() {
        let e0 = solve_control(
            &ControlRoomInput::with_operators(vec![control_op("祐天寺若麦", 0)]),
            &table(),
        );
        assert_eq!(e0.global.get(GlobalResourceKey::Passion), 10.0);
        assert_eq!(e0.operator_mood_drains["祐天寺若麦"], 0.0);
        assert_eq!(e0.meeting_speed_pct, 5.0);

        let e2 = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("祐天寺若麦", 2),
                control_op("丰川祥子", 2),
            ]),
            &table(),
        );
        assert_eq!(e2.global.get(GlobalResourceKey::Passion), 10.0);
        assert_eq!(e2.operator_mood_drains["祐天寺若麦"], 0.0);
        assert_eq!(e2.operator_mood_drains["丰川祥子"], 0.05);
        assert_eq!(e2.meeting_speed_pct, 5.0);
        assert_eq!(
            e2.effect_ledger
                .records()
                .iter()
                .filter(|record| record.key == ControlEffectKey::MEETING_CLUE_SPEED)
                .map(|record| record.status)
                .collect::<Vec<_>>(),
            vec![
                crate::control::ControlEffectStatus::TiedWinner,
                crate::control::ControlEffectStatus::TiedWinner,
            ]
        );
    }

    #[test]
    fn mujica_control_effects_use_actual_member_tiers() {
        let result = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("若叶睦", 2),
                control_op("三角初华", 2),
                control_op("丰川祥子", 2),
                control_op("祐天寺若麦", 2),
                control_op("八幡海铃", 2),
            ]),
            &table(),
        );

        assert_eq!(
            result.global.get(GlobalResourceKey::Passion),
            60.0,
            "初华按默认 20 名宿舍成员提供 20，睦/若麦/海铃分别提供 20/10/10"
        );
        assert_eq!(result.office_hire_spd_pct, 10.0);
        assert_eq!(result.meeting_speed_pct, 5.0);
        assert_eq!(
            result.operator_mood_drains["若叶睦"], 0.0,
            "E2 睦应在祥子同中枢时清除自身热情消耗"
        );
        assert_eq!(
            result.operator_mood_drains["丰川祥子"], 0.10,
            "祥子在热情 60 时自身 +0.05，若麦 E2 同中枢再 +0.05"
        );
        assert_eq!(
            result.inject.manu_eff_for(RecipeKind::Gold),
            4.0,
            "祥子 E2 的固定 +1 与热情 60 的 +3 应同时结算"
        );
    }

    #[test]
    fn pudding_manu_inject_when_two_platforms() {
        let mut layout = LayoutContext::default();
        layout.platform_count_in_power = 2;
        let input = ControlRoomInput {
            operators: vec![control_op("布丁", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            (result.inject.manu_eff_for(crate::types::RecipeKind::Gold) - 2.0).abs() < f64::EPSILON,
            "got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn pinus_knight_is_resolved_in_manufacture_room() {
        use crate::layout::TAG_PINUS;

        let mut layout = LayoutContext::default();
        layout
            .manu_tagged_count_sum
            .insert(TAG_PINUS.to_string(), 3);
        let input = ControlRoomInput {
            operators: vec![control_op("焰尾", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            result
                .inject
                .manu_eff_for(crate::types::RecipeKind::BattleRecord)
                .abs()
                < f64::EPSILON,
            "焰尾红松经验加成按制造房内同伴数结算，不走 generic inject: {}",
            result
                .inject
                .manu_eff_for(crate::types::RecipeKind::BattleRecord)
        );
        assert!(
            result
                .inject
                .manu_eff_for(crate::types::RecipeKind::Gold)
                .abs()
                < f64::EPSILON,
            "gold inject {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn vivi_knight_micro_is_resolved_in_manufacture_room() {
        use crate::layout::TAG_KNIGHT;

        let mut layout = LayoutContext::default();
        layout
            .manu_tagged_count_sum
            .insert(TAG_KNIGHT.to_string(), 1);
        let input = ControlRoomInput {
            operators: vec![control_op("薇薇安娜", 2)],
            mood: 24.0,
            layout,
        };
        let result = solve_control(&input, &table());
        assert!(
            result
                .inject
                .manu_eff_for(crate::types::RecipeKind::Gold)
                .abs()
                < f64::EPSILON,
            "got {}",
            result.inject.manu_eff_for(crate::types::RecipeKind::Gold)
        );
    }

    #[test]
    fn trace_is_behavior_neutral_and_exposes_passion_state_and_inject() {
        let mut layout = LayoutContext::default();
        layout.global.set(StateKey::Passion, 40.0);
        let input = ControlRoomInput {
            operators: vec![
                control_op("若叶睦", 2),
                control_op("丰川祥子", 2),
                control_op("三角初华", 2),
            ],
            mood: 24.0,
            layout,
        };
        let normal = solve_control(&input, &table());
        let (traced, trace) = solve_control_with_trace(&input, &table());
        assert_eq!(normal, traced);
        assert!(trace
            .final_state
            .iter()
            .any(|state| state.key == StateKey::Passion && state.value == 80.0));
        assert!(trace.events.iter().any(|event| {
            event.source_operator == "丰川祥子"
                && matches!(
                    &event.outcome,
                    ControlTraceOutcome::ManufactureInject {
                        family,
                        recipe: Some(RecipeKind::Gold),
                        candidate_value,
                        merge: Some(InjectMergeExplanation {
                            status: crate::global_resource::InjectMergeStatus::Winner,
                            ..
                        }),
                        ..
                    } if family == "manu_gold_passion" && *candidate_value == 4.0
                )
        }));
    }

    #[test]
    fn trace_marks_max_family_winner_tie_and_suppression() {
        let tied_input = ControlRoomInput::with_operators(vec![
            control_op("阿米娅", 0),
            control_op("诗怀雅", 0),
        ]);
        let (_, tied_trace) = solve_control_with_trace(&tied_input, &table());
        let tied_count = tied_trace
            .events
            .iter()
            .filter(|event| {
                matches!(
                    &event.outcome,
                    ControlTraceOutcome::TradeInject {
                        merge: Some(InjectMergeExplanation {
                            status: crate::global_resource::InjectMergeStatus::TiedWinner,
                            ..
                        }),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(tied_count, 2);

        let max_input = ControlRoomInput::with_operators(vec![
            control_op("斩业星熊", 2),
            control_op("Mon3tr", 2),
            control_op("诗怀雅", 0),
        ]);
        let (_, max_trace) = solve_control_with_trace(&max_input, &table());
        let status_for = |name: &str| {
            max_trace.events.iter().find_map(|event| {
                if event.source_operator != name {
                    return None;
                }
                match &event.outcome {
                    ControlTraceOutcome::ManufactureInject {
                        family,
                        merge: Some(merge),
                        ..
                    } if family == INJECT_FAMILY_MANU_GLOBAL_ALL => Some(merge.status),
                    _ => None,
                }
            })
        };
        assert_eq!(
            status_for("斩业星熊"),
            Some(crate::global_resource::InjectMergeStatus::Winner)
        );
        assert_eq!(
            status_for("Mon3tr"),
            Some(crate::global_resource::InjectMergeStatus::Suppressed)
        );
    }

    #[test]
    fn trace_distinguishes_failed_condition_and_room_deferred_inject() {
        let hoshiguma = ControlRoomInput::with_operators(vec![control_op("斩业星熊", 2)]);
        let (_, failed_trace) = solve_control_with_trace(&hoshiguma, &table());
        assert!(failed_trace.events.iter().any(|event| {
            event.source_operator == "斩业星熊"
                && event.condition_status == ControlConditionStatus::Failed
                && matches!(event.outcome, ControlTraceOutcome::ConditionNotMet)
        }));

        let daifeen = ControlRoomInput::with_operators(vec![control_op("戴菲恩", 2)]);
        let (_, deferred_trace) = solve_control_with_trace(&daifeen, &table());
        assert!(deferred_trace.events.iter().any(|event| {
            matches!(
                event.outcome,
                ControlTraceOutcome::DeferredToTradeRoom { .. }
            )
        }));
    }

    #[test]
    fn trace_marks_unhandled_condition_assumed_true_without_changing_behavior() {
        let path = std::env::temp_dir().join(format!(
            "infra-control-unhandled-condition-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "skills": [{
                    "id": "control_debug_unhandled[000]",
                    "skill_name": "debug unhandled condition",
                    "facility": "control",
                    "tier": "tier_0",
                    "atoms": [{
                        "id": "control_debug_unhandled[000]:passion",
                        "condition": {"gold_delivery_below": {"n": 1}},
                        "action": {
                            "kind": "state_produce",
                            "params": {"key": "Passion", "amount": 7.0}
                        },
                        "phase": "state_write",
                        "phase_order": 10
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let custom_table = SkillTable::load(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let input = ControlRoomInput::with_operators(vec![ControlOperator {
            name: "debug operator".to_string(),
            elite: 0,
            buff_ids: vec!["control_debug_unhandled[000]".to_string()],
            tags: vec![],
        }]);
        let (result, trace) = solve_control_with_trace(&input, &custom_table);
        assert_eq!(result.global.get(StateKey::Passion), 7.0);
        assert!(trace.events.iter().any(|event| {
            event.condition_status == ControlConditionStatus::UnhandledAssumedTrue
                && matches!(
                    event.outcome,
                    ControlTraceOutcome::StateWrite { delta: 7.0, .. }
                )
        }));
    }
}
