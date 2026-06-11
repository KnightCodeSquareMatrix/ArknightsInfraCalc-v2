use std::collections::HashMap;

use crate::skill_table::SkillTable;
use crate::trade::gold_flow::apply_gold_flow_chain;
use crate::trade::input::TradeLayoutContext;
use crate::types::{Action, Condition, EffectAtom, Phase, Selector, StateKey};
#[derive(Debug, Clone, Default)]
pub struct OperatorRuntime {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    pub settled_eff: f64,
    pub direct_eff: f64,
    pub limit_contrib: i32,
    pub variable_eff: f64,
    /// Skill-driven modifier to hourly mood drain (negative = slower drain).
    pub mood_drain_delta: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TradeContext {
    pub operators: Vec<OperatorRuntime>,
    pub facility_level: u8,
    pub layout: TradeLayoutContext,
    pub facility_base_limit: i32,
    pub limit_gross: i32,
    pub limit_compression: i32,
    pub final_order_limit: i32,
    pub order_count: i32,
    pub mood: f64,
    pub state_pool: HashMap<StateKey, f64>,
    pub order_tags: Vec<String>,
    pub replace_order: Option<String>,
    pub breach_gold_add: i32,
    pub law_active: bool,
    /// Flat LMD bonus on eligible high-tier gold orders (e.g. 龙舌兰·投资).
    pub order_lmd_bonus: i32,
    pub real_gold_lines: u32,
    pub virtual_gold_lines: u32,
    pub durin_virtual_lines: u32,
}

#[derive(Debug, Clone)]
pub struct MechanicCaps {
    pub law: bool,
    pub breach_add: i32,
    pub closure: bool,
}

impl TradeContext {
    pub fn from_room(input: &crate::trade::input::TradeRoomInput) -> Self {
        let facility_base_limit = facility_base_limit(input.level);
        let operators = input
            .operators
            .iter()
            .map(|o| OperatorRuntime {
                name: o.name.clone(),
                elite: o.elite,
                buff_ids: o.buff_ids.clone(),
                tags: o.tags.clone(),
                ..Default::default()
            })
            .collect();
        let order_count = input.order_count.unwrap_or(facility_base_limit);
        let mut ctx = Self {
            operators,
            facility_level: input.level,
            layout: input.layout.clone(),
            facility_base_limit,
            final_order_limit: facility_base_limit,
            order_count,
            mood: input.mood,
            real_gold_lines: input.gold_production_lines.unwrap_or(0),
            durin_virtual_lines: input.durin_virtual_lines.unwrap_or(0),
            ..Default::default()
        };
        if let Some(fw) = input.human_fireworks {
            ctx.state_pool
                .insert(crate::types::StateKey::HumanFireworks, fw);
        }
        if input.layout.monster_cuisine_layers > 0 {
            ctx.state_pool.insert(
                crate::types::StateKey::MonsterCuisine,
                f64::from(input.layout.monster_cuisine_layers),
            );
        }
        ctx
    }

    pub fn order_gap(&self) -> i32 {
        (self.final_order_limit - self.order_count).max(0)
    }

    pub fn other_ops_direct_eff(&self, exclude: &str) -> f64 {
        self.operators
            .iter()
            .filter(|o| o.name != exclude)
            .map(|o| o.direct_eff)
            .sum()
    }

    pub fn order_eff_base(&self) -> f64 {
        self.operators.len() as f64
    }

    pub fn order_eff_skill(&self) -> f64 {
        self.operators
            .iter()
            .map(|o| o.settled_eff + o.variable_eff)
            .sum()
    }

    pub fn order_eff_total(&self) -> f64 {
        self.order_eff_base() + self.order_eff_skill()
    }

    pub fn mechanic_caps(&self) -> MechanicCaps {
        MechanicCaps {
            law: self.law_active,
            breach_add: self.breach_gold_add,
            closure: self.replace_order.as_deref() == Some("closure_special"),
        }
    }

    pub fn mood_drain_summary(&self) -> Vec<(String, f64)> {
        self.operators
            .iter()
            .map(|o| (o.name.clone(), o.mood_drain_delta))
            .collect()
    }
}

pub fn facility_base_limit(level: u8) -> i32 {
    match level {
        1 => 6,
        2 => 9,
        3 => 12,
        _ => 12,
    }
}

pub fn collect_atoms<'a>(
    ops: &[OperatorRuntime],
    table: &'a SkillTable,
) -> Vec<(&'a EffectAtom, String)> {
    let mut atoms = Vec::new();
    for op in ops {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else { continue };
            for atom in &skill.atoms {
                atoms.push((atom, op.name.clone()));
            }
        }
    }
    atoms.sort_by(|(a, _), (b, _)| {
        let pa = a.phase.sort_key();
        let pb = b.phase.sort_key();
        pa.cmp(&pb).then(a.phase_order.cmp(&b.phase_order))
    });
    atoms
}

fn recompute_limit(ctx: &mut TradeContext) {
    ctx.limit_gross = ctx.operators.iter().map(|o| o.limit_contrib).sum();
    ctx.final_order_limit =
        (ctx.facility_base_limit + ctx.limit_gross - ctx.limit_compression).max(1);
}

pub fn apply_trade_phases(ctx: &mut TradeContext, table: &SkillTable) {
    let names: Vec<String> = ctx.operators.iter().map(|o| o.name.clone()).collect();
    let atoms = {
        let ops = ctx.operators.clone();
        collect_atoms(&ops, table)
    };

    let peer_absorb_key = crate::types::Phase::PeerAbsorb.sort_key();
    let mut last_phase_group = 0i32;
    let mut gold_flow_done = false;
    for (atom, owner) in atoms {
        let phase_group = atom.phase.sort_key();
        if !gold_flow_done && phase_group >= peer_absorb_key {
            apply_gold_flow_chain(ctx, table);
            gold_flow_done = true;
        }
        if phase_group > crate::types::Phase::Limit.sort_key()
            && last_phase_group <= crate::types::Phase::Limit.sort_key()
        {
            recompute_limit(ctx);
        }
        last_phase_group = phase_group;

        if !condition_met(&atom.condition, ctx, &owner, &names) {
            continue;
        }
        apply_atom(ctx, &atom, &owner);
    }
    if !gold_flow_done {
        apply_gold_flow_chain(ctx, table);
    }

    recompute_limit(ctx);
}

fn condition_met(
    cond: &Option<Condition>,
    ctx: &TradeContext,
    _owner: &str,
    names: &[String],
) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::GoldDeliveryBelow { n } => default_gold_delivery(ctx) < *n as f64,
        Condition::GoldDeliveryAbove { n } => default_gold_delivery(ctx) > *n as f64,
        Condition::GoldOrderInvestEligible {} => {
            default_gold_delivery(ctx) > 3.0
                && !ctx.order_tags.iter().any(|t| t == "breach")
        }
        Condition::OrderHasTag { tag } => ctx.order_tags.iter().any(|t| t == tag),
        Condition::OrderNotHasTag { tag } => !ctx.order_tags.iter().any(|t| t == tag),
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::PartnerInRoom { name } => names.iter().any(|n| n == name),
        Condition::TagPresentInRoom { tag } => ctx
            .operators
            .iter()
            .any(|o| o.tags.iter().any(|t| t == tag)),
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
    }
}

fn default_gold_delivery(ctx: &TradeContext) -> f64 {
    if ctx.replace_order.as_deref() == Some("closure_special") {
        return 2.0;
    }
    3.0
}

fn apply_atom(ctx: &mut TradeContext, atom: &EffectAtom, owner: &str) {
    match atom.phase {
        Phase::StateWrite => apply_state_write(ctx, atom, owner),
        Phase::Constant | Phase::PeerShare | Phase::EffVar | Phase::OrderVar | Phase::LimitVar => {
            apply_eff_action(ctx, atom, owner);
        }
        Phase::Limit => apply_limit_action(ctx, atom, owner),
        Phase::OrderMechanic => {
            apply_order_mechanic(ctx, atom);
            if matches!(atom.action, Action::AddFlatEff { .. }) {
                apply_eff_action(ctx, atom, owner);
            }
        }
        Phase::GlobalInject => {}
        Phase::PeerAbsorb => apply_peer_absorb(ctx, &atom.action, owner),
        Phase::Mood => apply_mood_action(ctx, &atom.action, owner),
    }
}

fn apply_peer_absorb(ctx: &mut TradeContext, action: &Action, owner: &str) {
    let Action::VodfoxAbsorb { rate_per_peer } = action else {
        return;
    };
    let peer_count = ctx
        .operators
        .iter()
        .filter(|o| o.name != owner)
        .count();
    for op in &mut ctx.operators {
        if op.name != owner {
            op.settled_eff = 0.0;
            op.direct_eff = 0.0;
            op.variable_eff = 0.0;
        }
    }
    if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
        ctx.operators[idx].settled_eff += peer_count as f64 * rate_per_peer;
    }
}

fn apply_mood_action(ctx: &mut TradeContext, action: &Action, owner: &str) {
    match action {
        Action::MoodDrainDelta { delta, scope } => match scope {
            crate::types::MoodDrainScope::SelfOp => {
                if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
                    ctx.operators[idx].mood_drain_delta += delta;
                }
            }
            crate::types::MoodDrainScope::RoomOperators => {
                for op in &mut ctx.operators {
                    op.mood_drain_delta += delta;
                }
            }
        },
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
            if *step_size <= 0.0 {
                return;
            }
            let steps = (state / step_size).floor();
            let delta = steps * delta_per_step;
            match scope {
                crate::types::MoodDrainScope::SelfOp => {
                    if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
                        ctx.operators[idx].mood_drain_delta += delta;
                    }
                }
                crate::types::MoodDrainScope::RoomOperators => {
                    for op in &mut ctx.operators {
                        op.mood_drain_delta += delta;
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_state_write(ctx: &mut TradeContext, atom: &EffectAtom, _owner: &str) {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            if let Some(sk) = StateKey::parse(key) {
                let scale = resolve_selector_value(ctx, atom.selector.as_ref(), _owner);
                let add = if atom.selector.is_some() {
                    scale * amount
                } else {
                    *amount
                };
                *ctx.state_pool.entry(sk).or_insert(0.0) += add;
            }
        }
        Action::StateConvert { from, to, ratio } => {
            let (Some(from_k), Some(to_k)) = (StateKey::parse(from), StateKey::parse(to)) else {
                return;
            };
            let src = ctx.state_pool.get(&from_k).copied().unwrap_or(0.0);
            *ctx.state_pool.entry(to_k).or_insert(0.0) += src * ratio;
        }
        _ => {}
    }
}

fn apply_eff_action(ctx: &mut TradeContext, atom: &EffectAtom, owner: &str) {
    let idx = ctx.operators.iter().position(|o| o.name == owner);
    let Some(idx) = idx else { return };
    let value = resolve_eff_value(ctx, atom, owner);
    match atom.phase {
        Phase::OrderVar | Phase::LimitVar | Phase::EffVar
            if matches!(
                atom.action,
                Action::AddPerGapEff { .. }
                    | Action::AddFlatEffFromSelector { .. }
                    | Action::AddBucketEffFromSelector { .. }
            ) =>
        {
            ctx.operators[idx].variable_eff += value;
        }
        _ => {
            ctx.operators[idx].settled_eff += value;
            if matches!(
                atom.action,
                Action::AddFlatEff { .. } | Action::AddFlatEffFromSelector { .. }
            ) && atom.selector.is_none()
            {
                ctx.operators[idx].direct_eff += value;
            }
        }
    }
}

fn resolve_eff_value(ctx: &TradeContext, atom: &EffectAtom, owner: &str) -> f64 {
    match &atom.action {
        Action::AddFlatEff { value } => *value,
        Action::AddPerGapEff { rate } => *rate * ctx.order_gap() as f64,
        Action::AddFlatEffFromSelector { multiplier, cap } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            let mut v = base * multiplier;
            if let Some(c) = cap {
                v = v.min(*c);
            }
            v
        }
        Action::AddBucketEffFromSelector {
            step,
            ret_per_step,
            cap,
        } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            if *step <= 0.0 {
                0.0
            } else {
                let buckets = (base / step).floor();
                (buckets * ret_per_step).min(*cap)
            }
        }
        Action::StateConsumeToEff { key, div } => {
            let Some(sk) = StateKey::parse(key) else {
                return 0.0;
            };
            let state = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            if *div <= 0.0 {
                0.0
            } else {
                (state / div).floor()
            }
        }
        _ => 0.0,
    }
}

fn resolve_selector_value(ctx: &TradeContext, selector: Option<&Selector>, owner: &str) -> f64 {
    match selector {
        Some(Selector::FinalOrderLimit) => ctx.final_order_limit as f64,
        Some(Selector::LimitExcess) => {
            (ctx.final_order_limit - ctx.facility_base_limit).max(0) as f64
        }
        Some(Selector::FacilityLevel) => f64::from(ctx.facility_level),
        Some(Selector::TaggedCountInRoom { tag }) => ctx
            .operators
            .iter()
            .filter(|o| o.tags.iter().any(|t| t == tag))
            .count() as f64,
        Some(Selector::LimitContribSum) => ctx
            .operators
            .iter()
            .map(|o| o.limit_contrib)
            .sum::<i32>() as f64,
        Some(Selector::MeetingMaxLevel) => f64::from(ctx.layout.meeting_max_level),
        Some(Selector::DormLevelSum) => f64::from(ctx.layout.dorm_level_sum),
        Some(Selector::ManuRecipeKinds) => f64::from(ctx.layout.manu_recipe_kinds),
        Some(Selector::EliteFacilityCount) => f64::from(ctx.layout.elite_facility_count),
        Some(Selector::SuiFacilityCount) => f64::from(ctx.layout.sui_facility_count),
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::OrderGap) => ctx.order_gap() as f64,
        Some(Selector::OtherOpsDirectEff) => ctx.other_ops_direct_eff(owner),
        Some(Selector::OtherOpsTotalEff) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .map(|o| o.settled_eff + o.variable_eff + o.direct_eff)
            .sum(),
        Some(Selector::RoomPeerCount) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .count() as f64,
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::GoldDeliveryCount) => default_gold_delivery(ctx),
        None => 0.0,
    }
}

fn apply_limit_action(ctx: &mut TradeContext, atom: &EffectAtom, owner: &str) {
    match &atom.action {
        Action::ReduceLimit { div, min } => {
            let eff = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            let reduce = (eff / div).ceil() as i32;
            ctx.limit_compression += reduce.max(*min);
        }
        Action::AddLimitDelta { delta } => {
            if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
                ctx.operators[idx].limit_contrib += delta;
            }
        }
        Action::AddLimitFromSelector { multiplier } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
                ctx.operators[idx].limit_contrib += (base * multiplier).round() as i32;
            }
        }
        _ => {}
    }
}

fn apply_order_mechanic(ctx: &mut TradeContext, atom: &EffectAtom) {
    match &atom.action {
        Action::TagOrder { tag } => {
            if !ctx.order_tags.contains(tag) {
                ctx.order_tags.push(tag.clone());
            }
            if tag == "breach" {
                ctx.law_active = true;
            }
        }
        Action::AddGoldDelivery { n } => {
            ctx.breach_gold_add = ctx.breach_gold_add.max(*n as i32);
        }
        Action::ReplaceOrder { order_type } => {
            ctx.replace_order = Some(order_type.clone());
        }
        Action::AddOrderLmdBonus { bonus } => {
            ctx.order_lmd_bonus += bonus;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_table::SkillTable;
    use crate::trade::input::{TradeLayoutContext, TradeOperator, TradeRoomInput};
    fn load_table() -> SkillTable {
        let path = crate::skill_table::default_skill_table_path().expect("path");
        SkillTable::load(&path).expect("load")
    }

    /// 同房挂件：仅提供 peer 计数，干员名与机制无关。
    fn trade_peer(name: &str, buff_id: &str) -> TradeOperator {
        TradeOperator::new(name, 0, vec![buff_id.into()])
    }

    #[test]
    fn closure_flat_eff() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "subject".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_closure[000]".into()],
            tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert!((ctx.order_eff_skill() - 10.0).abs() < 0.01);
        assert_eq!(ctx.replace_order.as_deref(), Some("closure_special"));
    }

    #[test]
    fn jie_per_gap() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "subject".into(),
                elite: 0,
                buff_ids: vec!["trade_ord_limit_diff[000]".into()],
            tags: vec![],
            }],
            order_count: Some(8),
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        // gap = 12 - 8 = 4 → 4 * 4 = 16
        assert!((ctx.order_eff_skill() - 16.0).abs() < 0.01);
    }

    #[test]
    fn huoshao_peer_share_two_peers() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "subject".into(),
                    elite: 2,
                    buff_ids: vec![
                        "trade_cost[000]".into(),
                        "trade_ord_spd&share[000]".into(),
                    ],
            tags: vec![],
        },
                trade_peer("peer_a", "trade_ord_spd[000]"),
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let subject = &ctx.operators[0];
        let eff = subject.settled_eff + subject.variable_eff;
        assert!((eff - 30.0).abs() < 0.01, "eff={eff}");
    }

    #[test]
    fn huoshao_nuanchang_room_mood_drain() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "subject".into(),
                    elite: 0,
                    buff_ids: vec!["trade_cost[000]".into()],
                tags: vec![],
                },
                trade_peer("peer_a", "trade_ord_spd[000]"),
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        for op in &ctx.operators {
            assert!(
                (op.mood_drain_delta + 0.1).abs() < 0.001,
                "{} mood={}",
                op.name,
                op.mood_drain_delta
            );
        }
    }

    #[test]
    fn jixing_peer_share_alpha_two_peers() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "subject".into(),
                    elite: 0,
                    buff_ids: vec!["trade_ord_spd&share[001]".into()],
                tags: vec![],
                },
                trade_peer("peer_a", "trade_ord_spd[000]"),
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let subject = &ctx.operators[0];
        let eff = subject.settled_eff + subject.variable_eff;
        assert!((eff - 20.0).abs() < 0.01, "eff={eff}");
    }

    #[test]
    fn vodfox_zeros_peers_and_absorbs_45_per_peer() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "巫恋".into(),
                    elite: 2,
                    buff_ids: vec![
                        "trade_ord_vodfox[000]".into(),
                        "trade_ord_wt&cost[000]".into(),
                    ],
            tags: vec![],
        },
                trade_peer("peer_a", "trade_ord_spd&cost[000]"),
                trade_peer("peer_b", "trade_ord_spd&cost[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let shamare = ctx.operators.iter().find(|o| o.name == "巫恋").unwrap();
        assert!((shamare.settled_eff - 90.0).abs() < 0.01);
        for peer in ctx.operators.iter().filter(|o| o.name != "巫恋") {
            assert!((peer.settled_eff + peer.variable_eff).abs() < 0.01);
        }
        assert!((ctx.order_eff_total() - 93.0).abs() < 0.01);
        let shamare_mood = shamare.mood_drain_delta;
        assert!(shamare_mood.abs() < 0.001, "巫恋 mood={shamare_mood}");
        for peer in ctx.operators.iter().filter(|o| o.name != "巫恋") {
            assert!((peer.mood_drain_delta - 0.25).abs() < 0.001);
        }
    }

    #[test]
    fn duoling_bashansheshui_mood_with_human_fireworks() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "铎铃".into(),
                    elite: 0,
                    buff_ids: vec!["trade_cost&bd2[000]".into()],
                tags: vec![],
                },
                trade_peer("peer_a", "trade_ord_spd[000]"),
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        ctx.state_pool
            .insert(StateKey::HumanFireworks, 35.0);
        apply_trade_phases(&mut ctx, &table);
        for op in &ctx.operators {
            assert!(
                (op.mood_drain_delta + 0.13).abs() < 0.001,
                "{} mood={}",
                op.name,
                op.mood_drain_delta
            );
        }
    }

    #[test]
    fn duoling_wanlichuanshu_stronger_fireworks_scaling() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "铎铃".into(),
                elite: 2,
                buff_ids: vec!["trade_cost&bd2[001]".into()],
            tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        ctx.state_pool
            .insert(StateKey::HumanFireworks, 25.0);
        apply_trade_phases(&mut ctx, &table);
        let duoling = ctx.operators.first().unwrap();
        assert!(
            (duoling.mood_drain_delta + 0.14).abs() < 0.001,
            "mood={}",
            duoling.mood_drain_delta
        );
    }

    #[test]
    fn shiye_variable_excess_limit_with_silverash() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "琳琅诗怀雅".into(),
                    elite: 2,
                    buff_ids: vec![
                        "trade_ord_spd[000]".into(),
                        "trade_ord_spd_variable[000]".into(),
                    ],
            tags: vec![],
        },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                tags: vec![],
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let shiye = ctx.operators.iter().find(|o| o.name == "琳琅诗怀雅").unwrap();
        let skill = shiye.settled_eff + shiye.variable_eff;
        assert!((skill - 36.0).abs() < 0.01, "skill={skill}");
    }

    #[test]
    fn vigil_meeting_layout_bonus() {
        let table = load_table();
        let mut layout = TradeLayoutContext::default();
        layout.meeting_max_level = 3;
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "伺夜".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd&meet[000]".into()],
                tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert!((ctx.order_eff_skill() - 40.0).abs() < 0.01);
    }

    #[test]
    fn sphinx_ext_with_urrbian_in_base() {
        let table = load_table();
        let mut layout = TradeLayoutContext::default();
        layout.base_workforce = vec!["乌尔比安".into()];
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "深巡".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_ext[001]".into()],
                tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert!((ctx.order_eff_skill() - 40.0).abs() < 0.01);
    }

    #[test]
    fn orchd2_counts_snhunt_tag_in_room() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "焰狐龙梓兰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_orchd2[000]".into()],
                    tags: vec!["cc.g.snhunt".into()],
                },
                TradeOperator {
                    name: "雷狼龙S空爆".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd3&catap2[000]".into()],
                    tags: vec!["cc.g.snhunt".into()],
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let zilan = ctx.operators.iter().find(|o| o.name == "焰狐龙梓兰").unwrap();
        assert!((zilan.settled_eff - 40.0).abs() < 0.01);
        assert_eq!(ctx.final_order_limit, 15);
    }

    #[test]
    fn heijian_silent_echo_from_dorm_occupants() {
        let table = load_table();
        let mut layout = TradeLayoutContext::default();
        layout.dorm_occupant_count = 12;
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "黑键".into(),
                elite: 0,
                buff_ids: vec![
                    "trade_ord_spd_bd[000]".into(),
                    "trade_ord_spd_bd_n1[000]".into(),
                ],
                tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let heijian = ctx.operators.first().unwrap();
        assert!((heijian.settled_eff - 3.0).abs() < 0.01);
    }

    #[test]
    fn qiearchuck_monster_cuisine_from_layout() {
        let table = load_table();
        let mut layout = TradeLayoutContext::default();
        layout.monster_cuisine_layers = 3;
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "齐尔查克".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_bd[100]".into()],
                tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let qie = ctx.operators.first().unwrap();
        assert!((qie.settled_eff - 3.0).abs() < 0.01);
    }

    #[test]
    fn taojinnang_negotiation_limit_and_self_mood() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "subject".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_limit&cost[000]".into()],
            tags: vec![],
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert_eq!(ctx.final_order_limit, 12 + 5);
        let tao = ctx.operators.first().unwrap();
        assert!((tao.mood_drain_delta + 0.25).abs() < 0.001);
    }
}
