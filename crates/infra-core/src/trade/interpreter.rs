use std::collections::HashMap;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::global_resource::GlobalResourceKey;
use crate::layout::SharedLayout;
use crate::skill_table::SkillTable;
use crate::trade::gold_flow::apply_gold_flow_chain;
use crate::types::{Action, CompiledAtom, Condition, EffectAtom, Phase, Selector, StateKey};
#[derive(Debug, Clone, Default)]
pub struct OperatorRuntime<'a> {
    pub name: &'a str,
    pub elite: u8,
    pub buff_ids: &'a [String],
    pub tags: &'a [String],
    pub compiled_atoms: &'a [CompiledAtom],
    pub settled_eff: f64,
    pub direct_eff: f64,
    pub limit_contrib: i32,
    pub variable_eff: f64,
    /// Skill-driven modifier to hourly mood drain (negative = slower drain).
    pub mood_drain_delta: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TradeContext<'a> {
    pub operators: SmallVec<[OperatorRuntime<'a>; 5]>,
    pub facility_level: u8,
    pub layout: SharedLayout,
    pub facility_base_limit: i32,
    pub limit_gross: i32,
    pub limit_compression: i32,
    pub final_order_limit: i32,
    pub order_count: i32,
    pub mood: f64,
    pub state_pool: HashMap<StateKey, f64>,
    pub order_tags: Vec<String>,
    pub breach_gold_add: i32,
    pub law_active: bool,
    /// Flat LMD bonus on eligible high-tier gold orders (e.g. 龙舌兰·投资).
    pub order_lmd_bonus: i32,
    pub real_gold_lines: u32,
    pub virtual_gold_lines: u32,
    pub durin_virtual_lines: u32,
    pub active_order_kind: crate::trade::input::TradeOrderKind,
}

#[derive(Debug, Clone, Default)]
pub struct MechanicCaps {
    pub law: bool,
    pub breach_add: i32,
    pub closure: bool,
}

impl<'a> TradeContext<'a> {
    pub fn from_room(input: &'a crate::trade::input::TradeRoomInput) -> Self {
        let facility_base_limit = facility_base_limit(input.level);
        let operators = input
            .operators
            .iter()
            .map(|o| OperatorRuntime {
                name: &o.name,
                elite: o.elite,
                buff_ids: &o.buff_ids,
                tags: &o.tags,
                compiled_atoms: &o.compiled_atoms,
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
            real_gold_lines: input
                .gold_production_lines
                .unwrap_or(input.layout.gold_manu_line_count),
            virtual_gold_lines: input
                .layout
                .global
                .get_u32(GlobalResourceKey::VirtualGoldLines),
            durin_virtual_lines: input
                .durin_virtual_lines
                .unwrap_or_else(|| input.layout.durin_virtual_lines()),
            active_order_kind: input.active_order_kind,
            state_pool: input.layout.global.to_room_state(),
            ..Default::default()
        };
        if let Some(fw) = input.human_fireworks {
            ctx.state_pool.insert(StateKey::HumanFireworks, fw);
        }
        ctx.seed_karlan_precision();
        ctx
    }

    /// 灵知·精密计算（控制中枢 → 贸易房）：每名同房谢拉格干员
    /// 订单获取效率 / 订单上限按系数预置。作为相位循环前的初值落到该干员身上——
    /// 故市井之道 `ReduceLimit` 读 `other_ops_settled_eff` 时已含 −效率，
    /// `recompute_limit` 汇总 `limit_contrib` 时已含 +上限，交互与游戏一致。
    fn seed_karlan_precision(&mut self) {
        let Some(kp) = self.layout.global_inject.karlan_precision() else {
            return;
        };
        for op in &mut self.operators {
            if op.tags.iter().any(|t| t == KARLAN_TAG) {
                op.settled_eff += kp.eff_per_karlan;
                op.limit_contrib += kp.limit_per_karlan;
            }
        }
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

    pub fn other_ops_settled_eff(&self, exclude: &str) -> f64 {
        self.operators
            .iter()
            .filter(|o| o.name != exclude)
            .map(|o| o.settled_eff)
            .sum()
    }

    pub fn peer_settled_eff_sum(&self) -> f64 {
        self.operators.iter().map(|o| o.settled_eff).sum()
    }

    pub fn order_eff_base(&self) -> f64 {
        if self.mood <= 0.0 {
            0.0
        } else {
            self.operators.len() as f64
        }
    }

    pub fn order_eff_skill(&self) -> f64 {
        self.operators
            .iter()
            .map(|o| o.settled_eff + o.variable_eff)
            .sum()
    }

    pub fn order_eff_total(&self) -> f64 {
        self.order_eff_base()
            + self.order_eff_skill()
            + self
                .layout
                .global_inject
                .trade_eff_pct_with_current_room_count(&self.layout.trade_tagged_count_sum, |tag| {
                    self.current_room_tag_count(tag)
                })
    }

    /// Count every tag occurrence, matching the former per-room u8 map.
    pub fn current_room_tag_count(&self, tag: &str) -> u8 {
        let mut count = 0u8;
        for operator in &self.operators {
            for candidate in operator.tags {
                if candidate == tag {
                    count += 1;
                }
            }
        }
        count
    }

    pub fn mechanic_caps(&self) -> MechanicCaps {
        MechanicCaps {
            law: self.law_active,
            breach_add: self.breach_gold_add,
            closure: order_has_tag(self, "closure_special"),
        }
    }

    pub fn mood_drain_summary(&self) -> Vec<(String, f64)> {
        self.operators
            .iter()
            .map(|o| (o.name.to_owned(), o.mood_drain_delta))
            .collect()
    }
}

pub fn facility_base_limit(level: u8) -> i32 {
    match level {
        1 => 6,
        2 => 9,
        3 => 10,
        _ => 10,
    }
}

fn recompute_limit(ctx: &mut TradeContext<'_>) {
    ctx.limit_gross = ctx.operators.iter().map(|o| o.limit_contrib).sum();
    ctx.final_order_limit =
        (ctx.facility_base_limit + ctx.limit_gross - ctx.limit_compression).max(1);
}

pub fn apply_trade_phases(ctx: &mut TradeContext<'_>, table: &SkillTable) {
    // Hand-built operators may lack prepared atoms. Only those operators need
    // temporary Arc owners; the table caches compilation, including empty results.
    let missing: SmallVec<[(usize, Arc<[CompiledAtom]>); 5]> = ctx
        .operators
        .iter()
        .enumerate()
        .filter(|(_, op)| op.compiled_atoms.is_empty() && !op.buff_ids.is_empty())
        .map(|(idx, op)| (idx, table.compile_operator_atoms(op.buff_ids)))
        .collect();
    // Copy references out of the runtime so applying an atom can mutate ctx.
    let sources: SmallVec<[(&str, &[CompiledAtom]); 5]> = ctx
        .operators
        .iter()
        .enumerate()
        .map(|(idx, op)| {
            let atoms = missing
                .iter()
                .find(|(owner_idx, _)| *owner_idx == idx)
                .map_or(op.compiled_atoms, |(_, atoms)| atoms.as_ref());
            (op.name, atoms)
        })
        .collect();
    let mut inline_heads = [0usize; 5];
    let mut spilled_heads = if sources.len() > inline_heads.len() {
        vec![0usize; sources.len()]
    } else {
        Vec::new()
    };
    let heads = if sources.len() > inline_heads.len() {
        spilled_heads.as_mut_slice()
    } else {
        &mut inline_heads[..sources.len()]
    };

    let peer_absorb_key = Phase::PeerAbsorb.sort_key();
    let mut last_phase_group = 0i32;
    let mut gold_flow_done = false;
    loop {
        // The key retains phase/order, owner input order, then stable atom seq.
        let mut pick = None;
        let mut best_key = None;
        for (owner_idx, (_, atoms)) in sources.iter().enumerate() {
            let Some(compiled) = atoms.get(heads[owner_idx]) else {
                continue;
            };
            let key = (compiled.sort_key, owner_idx, compiled.seq);
            if best_key.is_none_or(|best| key < best) {
                best_key = Some(key);
                pick = Some(owner_idx);
            }
        }
        let Some(owner_idx) = pick else { break };
        let (owner_name, atoms) = sources[owner_idx];
        let atom = &atoms[heads[owner_idx]].atom;
        heads[owner_idx] += 1;
        let phase_group = atom.phase.sort_key();
        if !gold_flow_done && phase_group >= peer_absorb_key && ctx.active_order_kind.is_gold() {
            apply_gold_flow_chain(ctx, table);
            gold_flow_done = true;
        }
        if phase_group > Phase::Limit.sort_key() && last_phase_group <= Phase::Limit.sort_key() {
            recompute_limit(ctx);
        }
        last_phase_group = phase_group;

        if condition_met(&atom.condition, ctx, owner_name) {
            apply_atom(ctx, atom, owner_name);
        }
    }
    if !gold_flow_done && ctx.active_order_kind.is_gold() {
        apply_gold_flow_chain(ctx, table);
    }
    recompute_limit(ctx);
}

fn condition_met(cond: &Option<Condition>, ctx: &TradeContext<'_>, owner: &str) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::GoldDeliveryBelow { n } => {
            ctx.active_order_kind.is_gold() && default_gold_delivery(ctx) < *n as f64
        }
        Condition::GoldDeliveryAbove { n } => {
            ctx.active_order_kind.is_gold() && default_gold_delivery(ctx) > *n as f64
        }
        Condition::GoldOrderInvestEligible {} => {
            ctx.active_order_kind.is_gold()
                && default_gold_delivery(ctx) > 3.0
                && !ctx.order_tags.iter().any(|t| t == "breach")
        }
        Condition::OrderHasTag { tag } => ctx.order_tags.iter().any(|t| t == tag),
        Condition::OrderNotHasTag { tag } => !ctx.order_tags.iter().any(|t| t == tag),
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodAboveOrEq { n } => ctx.mood >= *n as f64,
        Condition::MoodBelow { n } => ctx.mood < *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::OwnerMoodGapAbove { .. } => false,
        Condition::OwnerEliteGte { n } => ctx
            .operators
            .iter()
            .find(|op| op.name == owner)
            .is_some_and(|op| op.elite >= *n),
        Condition::OwnerEliteBelow { n } => ctx
            .operators
            .iter()
            .find(|op| op.name == owner)
            .is_some_and(|op| op.elite < *n),
        Condition::PartnerInRoom { name } => ctx.operators.iter().any(|op| op.name == name),
        Condition::TagPresentInRoom { tag } => ctx
            .operators
            .iter()
            .any(|o| o.tags.iter().any(|t| t == tag)),
        Condition::PeerTagInRoom { tag } => ctx
            .operators
            .iter()
            .any(|o| o.name != owner && o.tags.iter().any(|t| t == tag)),
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
        Condition::OperatorInPower { name } => ctx.layout.power_workforce.iter().any(|n| n == name),
        Condition::OperatorInTraining { name } => {
            ctx.layout.training_assist.iter().any(|n| n == name)
        }
        Condition::OperatorInTrade { name } => ctx.layout.trade_workforce.iter().any(|n| n == name),
        Condition::NoPlatformInOtherPower {} => !ctx.layout.other_power_has_platform,
        Condition::OtherPlatformInPower {} => ctx.layout.other_platform_in_power,
        Condition::OtherLateranoInPower {} => ctx.layout.other_laterano_in_power,
        Condition::TiandaoEffVarAllowed {} => tiandao_eff_var_allowed(ctx),
        Condition::ActiveRecipe { kind } => ctx.active_order_kind.as_recipe_kind() == *kind,
        Condition::OwnerLacksBuff { .. } => false,
        Condition::ExternalMomentumGteField {} => {
            ctx.layout.external_momentum() >= ctx.layout.field_momentum()
        }
        Condition::FieldMomentumGtExternal {} => {
            ctx.layout.field_momentum() > ctx.layout.external_momentum()
        }
        Condition::PlatformCountGte { min } => ctx.layout.platform_count_in_power >= *min,
        Condition::OfficeHireSpeedBelow { .. } => false,
    }
}

const KARLAN_TAG: &str = "cc.g.karlan";
const JIE_LIMIT_COUNT_BUFF: &str = "trade_ord_limit_count[000]";
const XUEZHI_VARIABLE2_STEM: &str = "trade_ord_spd_variable2";

fn has_buff_stem(buff_ids: &[String], stem: &str) -> bool {
    buff_ids.iter().any(|b| b.starts_with(stem))
}

/// 市井之道 + 天道酬勤不可单独互叠；有第三方 settled 贡献时天道酬勤才生效。
fn tiandao_eff_var_allowed(ctx: &TradeContext<'_>) -> bool {
    let has_jie = ctx
        .operators
        .iter()
        .any(|o| o.buff_ids.iter().any(|b| b == JIE_LIMIT_COUNT_BUFF));
    let has_xuezhi = ctx
        .operators
        .iter()
        .any(|o| has_buff_stem(&o.buff_ids, XUEZHI_VARIABLE2_STEM));
    if !has_jie || !has_xuezhi {
        return true;
    }
    let third_party_settled: f64 = ctx
        .operators
        .iter()
        .filter(|o| {
            !o.buff_ids.iter().any(|b| b == JIE_LIMIT_COUNT_BUFF)
                && !has_buff_stem(&o.buff_ids, XUEZHI_VARIABLE2_STEM)
        })
        .map(|o| o.settled_eff)
        .sum();
    third_party_settled > 0.0
}

fn order_has_tag(ctx: &TradeContext<'_>, tag: &str) -> bool {
    ctx.order_tags.iter().any(|t| t == tag)
}

fn default_gold_delivery(ctx: &TradeContext<'_>) -> f64 {
    if order_has_tag(ctx, "closure_special") {
        return 2.0;
    }
    3.0
}

fn apply_atom(ctx: &mut TradeContext<'_>, atom: &EffectAtom, owner: &str) {
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

fn apply_peer_absorb(ctx: &mut TradeContext<'_>, action: &Action, owner: &str) {
    let Action::PeerEffAbsorb { rate_per_peer } = action else {
        return;
    };
    let peer_count = ctx.operators.iter().filter(|o| o.name != owner).count();
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

fn apply_mood_action(ctx: &mut TradeContext<'_>, action: &Action, owner: &str) {
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

fn apply_state_write(ctx: &mut TradeContext<'_>, atom: &EffectAtom, _owner: &str) {
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

fn apply_eff_action(ctx: &mut TradeContext<'_>, atom: &EffectAtom, owner: &str) {
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

fn resolve_eff_value(ctx: &TradeContext<'_>, atom: &EffectAtom, owner: &str) -> f64 {
    match &atom.action {
        Action::AddFlatEff { value, .. } => *value,
        Action::AddPerGapEff { rate } => *rate * ctx.order_gap() as f64,
        Action::AddFlatEffFromSelector {
            multiplier, cap, ..
        } => {
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
        Action::StateConsumeToEff {
            key,
            div,
            multiplier,
        } => {
            let Some(sk) = StateKey::parse(key) else {
                return 0.0;
            };
            let state = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            if *div <= 0.0 {
                0.0
            } else {
                (state / div).floor() * multiplier.unwrap_or(1.0)
            }
        }
        _ => 0.0,
    }
}

fn resolve_selector_value(ctx: &TradeContext<'_>, selector: Option<&Selector>, owner: &str) -> f64 {
    match selector {
        Some(Selector::FinalOrderLimit) => ctx.final_order_limit as f64,
        Some(Selector::LimitExcess) => {
            (ctx.final_order_limit - ctx.facility_base_limit).max(0) as f64
        }
        Some(Selector::FacilityLevel) => f64::from(ctx.facility_level),
        Some(Selector::FacilityLevelMinusOne) => f64::from(ctx.facility_level.saturating_sub(1)),
        Some(Selector::TaggedCountInRoom { tag }) => ctx
            .operators
            .iter()
            .filter(|o| o.tags.iter().any(|t| t == tag))
            .count() as f64,
        Some(Selector::LimitContribSum) => {
            ctx.operators.iter().map(|o| o.limit_contrib).sum::<i32>() as f64
        }
        Some(Selector::MeetingMaxLevel) => f64::from(ctx.layout.meeting_max_level),
        Some(Selector::DormLevelSum) => f64::from(ctx.layout.dorm_level_sum),
        Some(Selector::ManuRecipeKinds) => f64::from(ctx.layout.manu_recipe_kinds),
        Some(Selector::EliteFacilityCount) => f64::from(ctx.layout.elite_facility_count),
        Some(Selector::SuiFacilityCount) => f64::from(ctx.layout.sui_facility_count),
        Some(Selector::CappedSuiFacilityCount { max }) => {
            f64::from(ctx.layout.sui_facility_count.min(*max))
        }
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::OrderGap) => ctx.order_gap() as f64,
        // 市井之道读取实际订单数；与摊贩经济共存时，两者分别覆盖
        // current_count 与 final_limit-current_count，合计恒为 final_limit × 4%。
        Some(Selector::OrderCount) => ctx.order_count.min(ctx.final_order_limit) as f64,
        Some(Selector::PeerSettledEffSum) => ctx.peer_settled_eff_sum(),
        Some(Selector::PeerSkillEffSum) => 0.0,
        Some(Selector::OtherOpsSettledEff) => ctx.other_ops_settled_eff(owner),
        Some(Selector::OtherOpsDirectEff) => ctx.other_ops_direct_eff(owner),
        Some(Selector::OtherOpsTotalEff) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .map(|o| o.settled_eff + o.variable_eff + o.direct_eff)
            .sum(),
        Some(Selector::RoomPeerCount) => {
            ctx.operators.iter().filter(|o| o.name != owner).count() as f64
        }
        Some(Selector::RoomOperatorCount) => ctx.operators.len() as f64,
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::OwnerMoodGap) => 0.0,
        Some(Selector::TradeStationCount) => f64::from(ctx.layout.trade_station_count),
        Some(Selector::PowerStationCount) => f64::from(ctx.layout.effective_power_station_count()),
        Some(Selector::PlatformCountInPower) => f64::from(ctx.layout.platform_count_in_power),
        Some(Selector::TaggedCountInControl { .. }) | Some(Selector::ControlOperatorCount) => 0.0,
        Some(Selector::DroneCap) => ctx.layout.drone_cap as f64,
        // 未封顶（最大 6）；目前只有制造站的娜斯提消费，已在制造解释器里截到 5。
        Some(Selector::RhineLifeInBase) => f64::from(ctx.layout.rhine_life_in_base),
        // 贸易站没有技能使用“除自身外”的莱茵口径。
        Some(Selector::RhineLifeInBaseExcludingSelf) => 0.0,
        Some(Selector::GoldDeliveryCount) => default_gold_delivery(ctx),
        Some(Selector::MetalFormulaSkillCountInRoom) => 0.0,
        Some(Selector::StandardSkillCountInRoom) | Some(Selector::RhineSkillCountInRoom) => 0.0,
        Some(Selector::TrainingRoomLevel) => f64::from(ctx.layout.training_room_level),
        Some(Selector::FacilityLevelSumExclMeeting) => {
            f64::from(ctx.layout.facility_level_sum_excl_meeting).min(64.0)
        }
        Some(Selector::TaggedCountInTradeSum { tag }) => {
            f64::from(*ctx.layout.trade_tagged_count_sum.get(tag).unwrap_or(&0))
        }
        Some(Selector::TaggedCountInCurrentTradeRoom { tag }) => ctx
            .operators
            .iter()
            .filter(|operator| operator.tags.iter().any(|candidate| candidate == tag))
            .count() as f64,
        Some(Selector::TradeStationsWithTaggedGte { tag, min }) => f64::from(
            *ctx.layout
                .trade_stations_tagged_gte
                .get(&crate::layout::trade_station_tagged_gte_key(tag, *min))
                .unwrap_or(&0),
        ),
        Some(Selector::TaggedCountInManuSum { tag }) => {
            f64::from(*ctx.layout.manu_tagged_count_sum.get(tag).unwrap_or(&0))
        }
        Some(Selector::StatePoolFloored { .. }) => 0.0,
        None => 0.0,
    }
}

fn apply_limit_action(ctx: &mut TradeContext<'_>, atom: &EffectAtom, owner: &str) {
    match &atom.action {
        Action::ReduceLimit { div, min: _ } => {
            let eff = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            let reduce = (eff / div).floor() as i32;
            ctx.limit_compression += reduce.max(0);
        }
        Action::AddLimitDelta { delta, .. } => {
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

fn apply_order_mechanic(ctx: &mut TradeContext<'_>, atom: &EffectAtom) {
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
        Action::AddOrderLmdBonus { bonus } => {
            ctx.order_lmd_bonus += bonus;
        }
        _ => {}
    }
}
