use std::collections::HashMap;
use std::sync::Arc;

use crate::global_resource::GlobalResourceKey;
use crate::skill_table::SkillTable;
use crate::trade::gold_flow::apply_gold_flow_chain;
use crate::layout::SharedLayout;
use crate::types::{Action, CompiledAtom, Condition, EffectAtom, Phase, Selector, StateKey};
#[derive(Debug, Clone, Default)]
pub struct OperatorRuntime {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    pub compiled_atoms: Arc<[CompiledAtom]>,
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
                compiled_atoms: o.compiled_atoms.clone(),
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
        self.operators.len() as f64
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
            + self.layout.global_inject.trade_eff_pct()
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

fn ops_use_compiled_atoms(ops: &[OperatorRuntime]) -> bool {
    !ops.is_empty() && ops.iter().all(|o| !o.compiled_atoms.is_empty())
}

/// 三路归并预编译 atom（与 `collect_atoms` 全序等价；并列键用 owner 序号打破）。
fn collect_atoms_merged(ops: &[OperatorRuntime]) -> Vec<(usize, usize)> {
    let mut heads = vec![0usize; ops.len()];
    let mut out = Vec::new();
    loop {
        let mut pick: Option<(usize, usize)> = None;
        let mut best_key: Option<((i32, i32), usize, u16)> = None;
        for (oi, op) in ops.iter().enumerate() {
            let h = heads[oi];
            if h >= op.compiled_atoms.len() {
                continue;
            }
            let ca = &op.compiled_atoms[h];
            let key = (ca.sort_key, oi, ca.seq);
            if best_key.is_none_or(|bk| key < bk) {
                best_key = Some(key);
                pick = Some((oi, h));
            }
        }
        let Some((oi, h)) = pick else {
            break;
        };
        out.push((oi, h));
        heads[oi] = h + 1;
    }
    out
}

fn recompute_limit(ctx: &mut TradeContext) {
    ctx.limit_gross = ctx.operators.iter().map(|o| o.limit_contrib).sum();
    ctx.final_order_limit =
        (ctx.facility_base_limit + ctx.limit_gross - ctx.limit_compression).max(1);
}

pub fn apply_trade_phases(ctx: &mut TradeContext, table: &SkillTable) {
    let names: Vec<String> = ctx.operators.iter().map(|o| o.name.clone()).collect();
    if ops_use_compiled_atoms(&ctx.operators) {
        let order = collect_atoms_merged(&ctx.operators);
        apply_atoms_loop_compiled(ctx, table, &names, &order);
        return;
    }
    let atoms = {
        let ops = ctx.operators.clone();
        collect_atoms(&ops, table)
    };
    let legacy: Vec<(&EffectAtom, usize)> = atoms
        .iter()
        .map(|(a, owner)| {
            let idx = ctx
                .operators
                .iter()
                .position(|o| o.name == *owner)
                .expect("owner in room");
            (*a, idx)
        })
        .collect();
    apply_atoms_loop(ctx, table, &names, legacy);
}

fn apply_atoms_loop_compiled(
    ctx: &mut TradeContext,
    table: &SkillTable,
    names: &[String],
    order: &[(usize, usize)],
) {
    let peer_absorb_key = crate::types::Phase::PeerAbsorb.sort_key();
    let mut last_phase_group = 0i32;
    let mut gold_flow_done = false;
    for &(owner_idx, atom_idx) in order {
        let atom = ctx.operators[owner_idx].compiled_atoms[atom_idx].atom.clone();
        let owner_name = ctx.operators[owner_idx].name.clone();
        let phase_group = atom.phase.sort_key();
        if !gold_flow_done && phase_group >= peer_absorb_key && ctx.active_order_kind.is_gold() {
            apply_gold_flow_chain(ctx, table);
            gold_flow_done = true;
        }
        if phase_group > crate::types::Phase::Limit.sort_key()
            && last_phase_group <= crate::types::Phase::Limit.sort_key()
        {
            recompute_limit(ctx);
        }
        last_phase_group = phase_group;

        if !condition_met(&atom.condition, ctx, &owner_name, names) {
            continue;
        }
        apply_atom(ctx, &atom, &owner_name);
    }
    if !gold_flow_done && ctx.active_order_kind.is_gold() {
        apply_gold_flow_chain(ctx, table);
    }

    recompute_limit(ctx);
}

fn apply_atoms_loop(
    ctx: &mut TradeContext,
    table: &SkillTable,
    names: &[String],
    atoms: Vec<(&EffectAtom, usize)>,
) {
    let peer_absorb_key = crate::types::Phase::PeerAbsorb.sort_key();
    let mut last_phase_group = 0i32;
    let mut gold_flow_done = false;
    for (atom, owner_idx) in atoms {
        let owner_name = ctx.operators[owner_idx].name.clone();
        let phase_group = atom.phase.sort_key();
        if !gold_flow_done && phase_group >= peer_absorb_key && ctx.active_order_kind.is_gold() {
            apply_gold_flow_chain(ctx, table);
            gold_flow_done = true;
        }
        if phase_group > crate::types::Phase::Limit.sort_key()
            && last_phase_group <= crate::types::Phase::Limit.sort_key()
        {
            recompute_limit(ctx);
        }
        last_phase_group = phase_group;

        if !condition_met(&atom.condition, ctx, &owner_name, names) {
            continue;
        }
        apply_atom(ctx, atom, &owner_name);
    }
    if !gold_flow_done && ctx.active_order_kind.is_gold() {
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
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::PartnerInRoom { name } => names.iter().any(|n| n == name),
        Condition::TagPresentInRoom { tag } => ctx
            .operators
            .iter()
            .any(|o| o.tags.iter().any(|t| t == tag)),
        Condition::PeerTagInRoom { tag } => ctx.operators.iter().any(|o| {
            o.name != _owner && o.tags.iter().any(|t| t == tag)
        }),
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
        Condition::OperatorInPower { name } => {
            ctx.layout.power_workforce.iter().any(|n| n == name)
        }
        Condition::OperatorInTraining { name } => {
            ctx.layout.training_assist.iter().any(|n| n == name)
        }
        Condition::OperatorInTrade { name } => {
            ctx.layout.trade_workforce.iter().any(|n| n == name)
        }
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
    }
}

const KARLAN_TAG: &str = "cc.g.karlan";
const JIE_LIMIT_COUNT_BUFF: &str = "trade_ord_limit_count[000]";
const XUEZHI_VARIABLE2_STEM: &str = "trade_ord_spd_variable2";

fn has_buff_stem(buff_ids: &[String], stem: &str) -> bool {
    buff_ids.iter().any(|b| b.starts_with(stem))
}

/// 市井之道 + 天道酬勤不可单独互叠；有第三方 settled 贡献时天道酬勤才生效。
fn tiandao_eff_var_allowed(ctx: &TradeContext) -> bool {
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

fn order_has_tag(ctx: &TradeContext, tag: &str) -> bool {
    ctx.order_tags.iter().any(|t| t == tag)
}

fn default_gold_delivery(ctx: &TradeContext) -> f64 {
    if order_has_tag(ctx, "closure_special") {
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
    let Action::PeerEffAbsorb { rate_per_peer } = action else {
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
        Action::AddFlatEff { value, .. } => *value,
        Action::AddPerGapEff { rate } => *rate * ctx.order_gap() as f64,
        Action::AddFlatEffFromSelector { multiplier, cap, .. } => {
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
        Some(Selector::CappedSuiFacilityCount { max }) => {
            f64::from(ctx.layout.sui_facility_count.min(*max))
        }
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::OrderGap) => ctx.order_gap() as f64,
        // 市井之道「每持有 1 笔订单 +4%」：并行订单数不超过当前订单上限。
        // 稳态满槽时按 effective limit 计（工具人表 / 灵孑银崖）；否则用输入 order_count。
        Some(Selector::OrderCount) => {
            let has_jie_market = ctx.operators.iter().any(|o| {
                o.buff_ids.iter().any(|b| b == JIE_LIMIT_COUNT_BUFF)
            });
            let raw = if has_jie_market {
                ctx.final_order_limit
            } else {
                ctx.order_count
            };
            raw.min(ctx.final_order_limit) as f64
        }
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
        Some(Selector::RoomPeerCount) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .count() as f64,
        Some(Selector::RoomOperatorCount) => ctx.operators.len() as f64,
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::TradeStationCount) => f64::from(ctx.layout.trade_station_count),
        Some(Selector::PowerStationCount) => {
            f64::from(ctx.layout.effective_power_station_count())
        }
        Some(Selector::PlatformCountInPower) => f64::from(ctx.layout.platform_count_in_power),
        Some(Selector::TaggedCountInControl { .. }) | Some(Selector::ControlOperatorCount) => 0.0,
        Some(Selector::DroneCap) => ctx.layout.drone_cap as f64,
        Some(Selector::RhineLifeInBase) => f64::from(ctx.layout.rhine_life_in_base),
        Some(Selector::GoldDeliveryCount) => default_gold_delivery(ctx),
        Some(Selector::MetalFormulaSkillCountInRoom) => 0.0,
        Some(Selector::StandardSkillCountInRoom) | Some(Selector::RhineSkillCountInRoom) => 0.0,
        Some(Selector::TrainingRoomLevel) => f64::from(ctx.layout.training_room_level),
        Some(Selector::FacilityLevelSumExclMeeting) => {
            f64::from(ctx.layout.facility_level_sum_excl_meeting).min(64.0)
        }
        Some(Selector::TaggedCountInTradeSum { tag }) => f64::from(
            *ctx.layout
                .trade_tagged_count_sum
                .get(tag)
                .unwrap_or(&0),
        ),
        Some(Selector::TradeStationsWithTaggedGte { tag, min }) => f64::from(
            *ctx.layout
                .trade_stations_tagged_gte
                .get(&crate::layout::trade_station_tagged_gte_key(tag, *min))
                .unwrap_or(&0),
        ),
        Some(Selector::TaggedCountInManuSum { tag }) => f64::from(
            *ctx.layout.manu_tagged_count_sum.get(tag).unwrap_or(&0),
        ),
        Some(Selector::StatePoolFloored { .. }) => 0.0,
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
        Action::AddOrderLmdBonus { bonus } => {
            ctx.order_lmd_bonus += bonus;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::skill_table::SkillTable;
    use crate::layout::LayoutContext;
    use crate::trade::input::{TradeOperator, TradeOrderKind, TradeRoomInput};
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
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert!((ctx.order_eff_skill() - 10.0).abs() < 0.01);
        assert!(order_has_tag(&ctx, "closure_special"));
    }

    #[test]
    fn jie_tier_up_compresses_and_per_order_count() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "孑".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_limit_count[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "peer".into(),
                    elite: 0,
                    buff_ids: vec!["trade_ord_spd[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
            ],
            order_count: Some(10),
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert_eq!(ctx.limit_compression, 4);
        assert_eq!(ctx.final_order_limit, 12);
        let jie = ctx.operators.iter().find(|o| o.name == "孑").unwrap();
        // 稳态按 limit=12 计 per-order
        assert!((jie.variable_eff - 48.0).abs() < 0.01, "var={}", jie.variable_eff);
    }

    #[test]
    fn xuezhi_buckets_peer_settled_sum() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "雪雉".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd_variable2[001]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let xz = ctx.operators.iter().find(|o| o.name == "雪雉").unwrap();
        // peer settled 20+20=40 → floor(40/5)*5 = 35 (cap)
        assert!((xz.variable_eff - 35.0).abs() < 0.01, "xz var={}", xz.variable_eff);
    }

    #[test]
    fn jie_xuezhi_alone_blocks_tiandao() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "孑".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_limit_count[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "雪雉".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd_variable2[001]".into()],
                    tags: vec![],
                    ..Default::default()
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let xz = ctx.operators.iter().find(|o| o.name == "雪雉").unwrap();
        assert!(xz.variable_eff.abs() < 0.01);
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
                ..Default::default()
            }],
            order_count: Some(8),
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
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
            ..Default::default()
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
            active_order_kind: TradeOrderKind::Gold,
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
                    ..Default::default()
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
            active_order_kind: TradeOrderKind::Gold,
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
                    ..Default::default()
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
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let subject = &ctx.operators[0];
        let eff = subject.settled_eff + subject.variable_eff;
        assert!((eff - 20.0).abs() < 0.01, "eff={eff}");
    }

    #[test]
    fn pepe_peer_absorb_zeros_roommates_no_self_gain() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "佩佩".into(),
                    elite: 2,
                    buff_ids: vec![
                        "trade_ord_limit&trade&lv[000]".into(),
                        "trade_ord_pepe[000]".into(),
                    ],
            tags: vec![],
            ..Default::default()
        },
                trade_peer("能天使", "trade_ord_spd[010]"),
                trade_peer("德克萨斯", "trade_ord_spd&cost_P[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let pepe = ctx.operators.iter().find(|o| o.name == "佩佩").unwrap();
        assert!(
            (pepe.settled_eff + pepe.variable_eff).abs() < 0.01,
            "pepe keeps no trade eff from skills"
        );
        for peer in ctx.operators.iter().filter(|o| o.name != "佩佩") {
            assert!(
                (peer.settled_eff + peer.variable_eff).abs() < 0.01,
                "{} eff zeroed",
                peer.name
            );
        }
        assert!((ctx.order_eff_skill() - 0.0).abs() < 0.01);
        assert!(order_has_tag(&ctx, "pepe_exclusive"));
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
            ..Default::default()
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
            active_order_kind: TradeOrderKind::Gold,
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
                    ..Default::default()
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
            active_order_kind: TradeOrderKind::Gold,
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
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
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
            ..Default::default()
        },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                tags: vec![],
                    ..Default::default()
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
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
        let mut layout = LayoutContext::default();
        layout.meeting_max_level = 3;
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "伺夜".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd&meet[000]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert!((ctx.order_eff_skill() - 40.0).abs() < 0.01);
    }

    #[test]
    fn sphinx_ext_with_urrbian_in_base() {
        let table = load_table();
        let mut layout = LayoutContext::default();
        layout.base_workforce = vec!["乌尔比安".into()];
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "深巡".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_ext[001]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
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
                ..Default::default()
            },
                TradeOperator {
                    name: "雷狼龙S空爆".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd3&catap2[000]".into()],
                tags: vec!["cc.g.snhunt".into()],
                ..Default::default()
            },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
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
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(LayoutContext::default()),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let heijian = ctx.operators.first().unwrap();
        assert!(
            (heijian.settled_eff - 5.0).abs() < 0.01,
            "20 dorm → 20 silent echo / 4 = 5%, got {}",
            heijian.settled_eff
        );
    }

    #[test]
    fn heijian_tier_up_silent_echo_halved_divisor() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "黑键".into(),
                elite: 2,
                buff_ids: vec![
                    "trade_ord_spd_bd[010]".into(),
                    "trade_ord_spd_bd_n1[000]".into(),
                ],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(LayoutContext::default()),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let heijian = ctx.operators.first().unwrap();
        assert!(
            (heijian.settled_eff - 10.0).abs() < 0.01,
            "20 silent echo / 2 = 10%, got {}",
            heijian.settled_eff
        );
    }

    #[test]
    fn wuyou_human_fireworks_from_dorm_occupants() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "乌有".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_bd_n2[000]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(LayoutContext::default()),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let wuyou = ctx.operators.first().unwrap();
        assert!(
            (wuyou.settled_eff - 20.0).abs() < 0.01,
            "20 dorm → 20 HF → 20%, got {}",
            wuyou.settled_eff
        );
    }

    #[test]
    fn duoling_mood_with_default_wuyou_fireworks_baseline() {
        let table = load_table();
        let mut layout = LayoutContext::default();
        layout
            .global
            .set(crate::global_resource::GlobalResourceKey::HumanFireworks, 20.0);
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "铎铃".into(),
                    elite: 0,
                    buff_ids: vec!["trade_cost&bd2[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                trade_peer("peer_a", "trade_ord_spd[000]"),
                trade_peer("peer_b", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        for op in &ctx.operators {
            assert!(
                (op.mood_drain_delta + 0.12).abs() < 0.001,
                "{} mood={}",
                op.name,
                op.mood_drain_delta
            );
        }
    }

    #[test]
    fn terra_snhunt_trade_matatabi_from_global_pool() {
        let table = load_table();
        let mut layout = LayoutContext::default();
        layout.global.set(GlobalResourceKey::Matatabi, 12.0);
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "泰拉大陆调查团".into(),
                elite: 0,
                buff_ids: vec!["trade_ord_spd&limit&bd[000]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let terra = ctx.operators.first().unwrap();
        assert!(
            (terra.settled_eff - 41.0).abs() < 0.01,
            "5% flat + 12×3% matatabi = 41%, got {}",
            terra.settled_eff
        );
        assert_eq!(ctx.final_order_limit, 12 + 2);
    }

    #[test]
    fn yanhuolong_zilan_counts_snhunt_tagged_peers_in_room() {
        let table = load_table();
        let snhunt = |name: &str| TradeOperator {
            name: name.into(),
            elite: 2,
            buff_ids: if name == "焰狐龙梓兰" {
                vec!["trade_ord_orchd2[000]".into()]
            } else {
                vec![]
            },
            tags: vec!["cc.g.snhunt".into()],
            ..Default::default()
        };
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                snhunt("焰狐龙梓兰"),
                snhunt("雷狼龙S空爆"),
                snhunt("罗德岛隐秘队"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let zilan = ctx
            .operators
            .iter()
            .find(|o| o.name == "焰狐龙梓兰")
            .unwrap();
        assert!(
            (zilan.settled_eff - 60.0).abs() < 0.01,
            "3 泡影国小队 × 20% = 60%, got {}",
            zilan.settled_eff
        );
        assert_eq!(ctx.final_order_limit, 12 + 3);
    }

    #[test]
    fn qiearchuck_monster_cuisine_from_global_pool() {
        let table = load_table();
        let mut layout = LayoutContext::default();
        layout
            .global
            .set(GlobalResourceKey::MonsterCuisine, 3.0);
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "齐尔查克".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_bd[100]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let qie = ctx.operators.first().unwrap();
        assert!((qie.settled_eff - 3.0).abs() < 0.01);
    }

    #[test]
    fn qiearchuck_monster_cuisine_from_resolved_layout() {
        let table = load_table();
        let blueprint = crate::BaseBlueprint::template_243_use_this().unwrap();
        let mut assignment = crate::BaseAssignment::default();
        assignment.set_room("dorm_1", vec![crate::AssignedOperator::new("森西", 2)]);
        let layout = crate::resolve_base(
            &blueprint,
            &assignment,
            Some(
                &crate::instances::OperatorInstances::load(
                    &crate::instances::default_instances_path().unwrap(),
                )
                .unwrap(),
            ),
            Some(&table),
            24.0,
            None,
        )
        .unwrap()
        .layout;
        let input = TradeRoomInput {
            level: 3,
            operators: vec![TradeOperator {
                name: "齐尔查克".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_spd_bd[100]".into()],
                tags: vec![],
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Arc::new(layout),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        let qie = ctx.operators.first().unwrap();
        assert!(
            (qie.settled_eff - 3.0).abs() < 0.01,
            "森西 Lv3 宿舍 → 3 魔物料理 → +3%"
        );
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
                ..Default::default()
            }],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        assert_eq!(ctx.final_order_limit, 12 + 5);
        let tao = ctx.operators.first().unwrap();
        assert!((tao.mood_drain_delta + 0.25).abs() < 0.001);
    }

    #[test]
    fn collect_atoms_merged_matches_legacy_order() {
        use crate::pool::compile_operator_atoms;

        let table = load_table();
        let buff_sets: [(&str, &[&str]); 3] = [
            ("巫恋", &["trade_ord_spd_variable3[000]"]),
            ("龙舌兰", &["trade_ord_spd[000]", "trade_ord_spd_variable[000]"]),
            ("能天使", &["trade_ord_spd&limit[033]"]),
        ];
        let ops: Vec<OperatorRuntime> = buff_sets
            .iter()
            .map(|(name, bids)| {
                let buff_ids: Vec<String> = bids.iter().map(|s| (*s).into()).collect();
                OperatorRuntime {
                    name: (*name).into(),
                    elite: 2,
                    buff_ids: buff_ids.clone(),
                    tags: vec![],
                    compiled_atoms: compile_operator_atoms(&buff_ids, &table),
                    ..Default::default()
                }
            })
            .collect();

        let legacy = collect_atoms(&ops, &table);
        let legacy_keys: Vec<_> = legacy
            .iter()
            .map(|(a, owner)| (a.phase.sort_key(), a.phase_order, owner.as_str()))
            .collect();

        let order = collect_atoms_merged(&ops);
        let merged_keys: Vec<_> = order
            .iter()
            .map(|&(oi, ai)| {
                let a = &ops[oi].compiled_atoms[ai].atom;
                (a.phase.sort_key(), a.phase_order, ops[oi].name.as_str())
            })
            .collect();

        assert_eq!(legacy_keys, merged_keys);
    }

    fn layout_with_karlan_precision() -> std::sync::Arc<crate::trade::input::TradeLayoutContext> {
        let mut layout = crate::trade::input::TradeLayoutContext::default();
        layout
            .global_inject
            .record_karlan_precision(-15.0, 6);
        std::sync::Arc::new(layout)
    }

    #[test]
    fn kjera_precision_seeds_karlan_ops_before_phases() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                    tags: vec!["cc.g.karlan".into()],
                    ..Default::default()
                },
                TradeOperator {
                    name: "崖心".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[021]".into()],
                    tags: vec!["cc.g.karlan".into()],
                    ..Default::default()
                },
                trade_peer("filler", "trade_ord_spd[000]"),
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: layout_with_karlan_precision(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        // 种子：每名谢拉格 −15% / +6 上限（相位前）
        let silver = ctx.operators.iter().find(|o| o.name == "银灰").unwrap();
        assert!((silver.settled_eff + 15.0).abs() < 0.01);
        assert_eq!(silver.limit_contrib, 6);
        apply_trade_phases(&mut ctx, &table);
        // Constant +20 → 5；Limit +4 → limit_contrib 10
        let silver = ctx.operators.iter().find(|o| o.name == "银灰").unwrap();
        assert!((silver.settled_eff - 5.0).abs() < 0.01, "silver={}", silver.settled_eff);
        assert_eq!(silver.limit_contrib, 10);
        assert_eq!(ctx.final_order_limit, 12 + 10 + 10);
    }

    #[test]
    fn ling_jie_yaxin_kjera_combo() {
        use crate::trade::solver::solve_trade_with_shift;

        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "孑".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_limit_count[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                    tags: vec!["cc.g.karlan".into()],
                    ..Default::default()
                },
                TradeOperator {
                    name: "崖心".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[021]".into()],
                    tags: vec!["cc.g.karlan".into()],
                    ..Default::default()
                },
            ],
            order_count: None,
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: layout_with_karlan_precision(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let r = solve_trade_with_shift(&input, &table, 24.0).unwrap();
        eprintln!(
            "灵孑银崖: trade={:.1}% limit={} pre={:.1} shortcut={:?}",
            r.order_eff_total, r.final_order_limit, r.order_eff_pre_shortcut, r.trade_shortcut
        );
        assert_eq!(r.trade_shortcut.as_deref(), Some("gsl_ling_jie_yaxin"));
        assert!((r.order_eff_total - 125.0).abs() < f64::EPSILON);
        assert_eq!(r.final_order_limit, 31);
    }

    #[test]
    fn jie_order_count_clamped_when_limit_compressed() {
        let table = load_table();
        let input = TradeRoomInput {
            level: 3,
            operators: vec![
                TradeOperator {
                    name: "孑".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_limit_count[000]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                TradeOperator {
                    name: "银灰".into(),
                    elite: 2,
                    buff_ids: vec!["trade_ord_spd&limit[022]".into()],
                    tags: vec![],
                    ..Default::default()
                },
                trade_peer("peer", "trade_ord_spd[000]"),
            ],
            order_count: Some(12),
            mood: 24.0,
            gold_production_lines: None,
            durin_virtual_lines: None,
            human_fireworks: None,
            layout: Default::default(),
            active_order_kind: TradeOrderKind::Gold,
        };
        let mut ctx = TradeContext::from_room(&input);
        apply_trade_phases(&mut ctx, &table);
        // 无灵知：compression=4 → limit=12；OrderCount clamp 12→12 不变
        assert_eq!(ctx.final_order_limit, 12);
        let jie = ctx.operators.iter().find(|o| o.name == "孑").unwrap();
        assert!((jie.variable_eff - 48.0).abs() < 0.01, "var={}", jie.variable_eff);

        // 若上限被压至 8，per-order 应按 8 计而非 12
        let input_tight = TradeRoomInput {
            order_count: Some(12),
            ..input
        };
        // 三人全 +20 settled → compression 6 → limit 10（12+4-6）
        let mut input_heavy = input_tight;
        input_heavy.operators = vec![
            TradeOperator {
                name: "孑".into(),
                elite: 2,
                buff_ids: vec!["trade_ord_limit_count[000]".into()],
                tags: vec![],
                ..Default::default()
            },
            trade_peer("a", "trade_ord_spd[000]"),
            trade_peer("b", "trade_ord_spd[000]"),
        ];
        let mut ctx2 = TradeContext::from_room(&input_heavy);
        apply_trade_phases(&mut ctx2, &table);
        let jie2 = ctx2.operators.iter().find(|o| o.name == "孑").unwrap();
        let effective_orders = ctx2.final_order_limit;
        assert!(
            (jie2.variable_eff - effective_orders as f64 * 4.0).abs() < 0.01,
            "limit={} var={}",
            ctx2.final_order_limit,
            jie2.variable_eff
        );
    }
}
