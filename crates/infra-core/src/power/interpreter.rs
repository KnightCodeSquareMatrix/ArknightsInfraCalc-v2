use std::collections::HashMap;

use crate::power::input::PowerRoomInput;
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, EffectAtom, Phase, Selector, StateKey};

#[derive(Debug, Clone, Default)]
pub struct PowerOperatorRuntime {
    pub name: String,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    pub charge_speed_pct: f64,
    pub mood_drain_delta: f64,
}

#[derive(Debug, Clone)]
pub struct PowerContext {
    pub operator: PowerOperatorRuntime,
    pub layout: crate::layout::LayoutContext,
    pub mood: f64,
    pub shift_hours: f64,
    pub state_pool: HashMap<StateKey, f64>,
}

impl PowerContext {
    pub fn from_room(input: &PowerRoomInput) -> Self {
        Self {
            operator: PowerOperatorRuntime {
                name: input.operator.name.clone(),
                buff_ids: input.operator.buff_ids.clone(),
                tags: input.operator.tags.clone(),
                ..Default::default()
            },
            layout: input.layout.clone(),
            mood: input.mood,
            shift_hours: input.shift_hours,
            state_pool: input.layout.global.to_room_state(),
        }
    }
}

pub fn apply_power_phases(ctx: &mut PowerContext, table: &SkillTable) {
    let atoms = collect_power_atoms(&ctx.operator, table);
    for (atom, owner) in atoms {
        if !condition_met(&atom.condition, ctx, &owner) {
            continue;
        }
        apply_atom(ctx, &atom, &owner);
    }
}

fn collect_power_atoms<'a>(
    op: &PowerOperatorRuntime,
    table: &'a SkillTable,
) -> Vec<(&'a EffectAtom, String)> {
    let mut atoms = Vec::new();
    for bid in &op.buff_ids {
        let Some(skill) = table.get(bid) else { continue };
        if skill.facility != "power" {
            continue;
        }
        for atom in &skill.atoms {
            atoms.push((atom, op.name.clone()));
        }
    }
    atoms.sort_by(|(a, _), (b, _)| {
        let pa = a.phase.sort_key();
        let pb = b.phase.sort_key();
        pa.cmp(&pb).then(a.phase_order.cmp(&b.phase_order))
    });
    atoms
}

fn condition_met(cond: &Option<Condition>, ctx: &PowerContext, owner: &str) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
        Condition::OperatorInTraining { name } => {
            ctx.layout.training_assist.iter().any(|n| n == name)
        }
        Condition::NoPlatformInOtherPower {} => !ctx.layout.other_power_has_platform,
        Condition::OtherPlatformInPower {} => ctx.layout.other_platform_in_power,
        Condition::OtherLateranoInPower {} => ctx.layout.other_laterano_in_power,
        Condition::OperatorInPower { name } => ctx.layout.power_workforce.iter().any(|n| n == name),
        Condition::PartnerInRoom { name } => owner == name,
        Condition::TagPresentInRoom { tag } => ctx.operator.tags.iter().any(|t| t == tag),
        _ => false,
    }
}

fn apply_atom(ctx: &mut PowerContext, atom: &EffectAtom, owner: &str) {
    match atom.phase {
        Phase::StateWrite => apply_state_write(ctx, atom),
        Phase::Constant | Phase::EffVar => apply_charge_action(ctx, atom, owner),
        Phase::Mood => apply_mood_action(ctx, &atom.action, owner),
        _ => {}
    }
}

fn apply_charge_action(ctx: &mut PowerContext, atom: &EffectAtom, _owner: &str) {
    let value = resolve_charge_value(ctx, atom);
    ctx.operator.charge_speed_pct += value;
}

fn resolve_charge_value(ctx: &PowerContext, atom: &EffectAtom) -> f64 {
    match &atom.action {
        Action::AddFlatEff { value, .. } => *value,
        Action::AddFlatEffFromSelector {
            multiplier, cap, ..
        } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref());
            let mut v = base * multiplier;
            if let Some(c) = cap {
                v = v.min(*c);
            }
            v
        }
        _ => 0.0,
    }
}

fn resolve_selector_value(ctx: &PowerContext, selector: Option<&Selector>) -> f64 {
    match selector {
        Some(Selector::DroneCap) => ctx.layout.drone_cap as f64,
        Some(Selector::DormLevelSum) => f64::from(ctx.layout.dorm_level_sum),
        Some(Selector::RhineLifeInBase) => f64::from(ctx.layout.rhine_life_in_base),
        Some(Selector::PowerStationCount) => {
            f64::from(ctx.layout.effective_power_station_count())
        }
        Some(Selector::Mood) => ctx.mood,
        None => 0.0,
        _ => 0.0,
    }
}

fn apply_state_write(ctx: &mut PowerContext, atom: &EffectAtom) {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            if let Some(sk) = StateKey::parse(key) {
                *ctx.state_pool.entry(sk).or_insert(0.0) += amount;
            }
        }
        _ => {}
    }
}

fn apply_mood_action(ctx: &mut PowerContext, action: &Action, _owner: &str) {
    if let Action::MoodDrainDelta { delta, scope } = action {
        match scope {
            crate::types::MoodDrainScope::SelfOp => {
                ctx.operator.mood_drain_delta += delta;
            }
            crate::types::MoodDrainScope::RoomOperators => {
                ctx.operator.mood_drain_delta += delta;
            }
        }
    }
}
