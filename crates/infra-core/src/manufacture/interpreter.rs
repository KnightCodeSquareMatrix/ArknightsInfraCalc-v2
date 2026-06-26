use std::collections::HashMap;

use crate::eff_ramp::eff_ramp_avg_20h;
use crate::layout::SharedLayout;
use crate::manufacture::input::ManuRoomInput;
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, EffectAtom, Phase, RecipeKind, Selector, StateKey};

#[derive(Debug, Clone, Default)]
pub struct RecipeEff {
    pub all: f64,
    pub battle_record: f64,
    pub gold: f64,
    pub originium: f64,
}

impl RecipeEff {
    pub fn for_recipe(&self, recipe: RecipeKind) -> f64 {
        self.all
            + match recipe {
                RecipeKind::All => 0.0,
                RecipeKind::BattleRecord => self.battle_record,
                RecipeKind::Gold => self.gold,
                RecipeKind::Originium => self.originium,
            }
    }

    fn add(&mut self, recipe: Option<RecipeKind>, value: f64) {
        match recipe {
            None | Some(RecipeKind::All) => self.all += value,
            Some(RecipeKind::BattleRecord) => self.battle_record += value,
            Some(RecipeKind::Gold) => self.gold += value,
            Some(RecipeKind::Originium) => self.originium += value,
        }
    }

    fn zero(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecipeLimit {
    pub all: i32,
    pub battle_record: i32,
    pub gold: i32,
    pub originium: i32,
}

impl RecipeLimit {
    pub fn for_recipe(&self, recipe: RecipeKind) -> i32 {
        self.all
            + match recipe {
                RecipeKind::All => 0,
                RecipeKind::BattleRecord => self.battle_record,
                RecipeKind::Gold => self.gold,
                RecipeKind::Originium => self.originium,
            }
    }

    fn add(&mut self, recipe: Option<RecipeKind>, delta: i32) {
        match recipe {
            None | Some(RecipeKind::All) => self.all += delta,
            Some(RecipeKind::BattleRecord) => self.battle_record += delta,
            Some(RecipeKind::Gold) => self.gold += delta,
            Some(RecipeKind::Originium) => self.originium += delta,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ManuOperatorRuntime {
    pub name: String,
    pub elite: u8,
    pub buff_ids: Vec<String>,
    pub tags: Vec<String>,
    /// 干员技能直接提供的生产力；`PeerEffAbsorb` 清零他人此项。
    pub skill_eff: RecipeEff,
    /// 按基建设施数量结算的生产力（不被他人归零技能清零）。
    pub layout_eff: RecipeEff,
    pub limit: RecipeLimit,
    /// 技能面仓库贡献（赤金/源石/战斗记录共用；分段仓库→% 只看此项）。
    pub limit_contrib: i32,
    pub mood_drain_delta: f64,
}

impl ManuOperatorRuntime {
    pub fn total_eff(&self, recipe: RecipeKind) -> f64 {
        self.skill_eff.for_recipe(recipe) + self.layout_eff.for_recipe(recipe)
    }
}

#[derive(Debug, Clone)]
pub struct ManuContext {
    pub operators: Vec<ManuOperatorRuntime>,
    /// 制造站房间级生产力加成（冬时 per-op +10% 等固定数值，非「干员技能面」）。
    pub station_eff: RecipeEff,
    pub facility_level: u8,
    pub active_recipe: RecipeKind,
    pub layout: SharedLayout,
    pub facility_base_storage: i32,
    pub mood: f64,
    pub state_pool: HashMap<StateKey, f64>,
}

impl ManuContext {
    pub fn from_room(input: &ManuRoomInput) -> Self {
        let operators = input
            .operators
            .iter()
            .map(|o| ManuOperatorRuntime {
                name: o.name.clone(),
                elite: o.elite,
                buff_ids: o.buff_ids.clone(),
                tags: o.tags.clone(),
                ..Default::default()
            })
            .collect();
        Self {
            operators,
            station_eff: RecipeEff::default(),
            facility_level: input.level,
            active_recipe: input.active_recipe,
            layout: input.layout.clone(),
            facility_base_storage: facility_base_storage(input.level),
            mood: input.mood,
            state_pool: input.layout.global.to_room_state(),
        }
    }

    pub fn prod_base(&self) -> f64 {
        self.operators.len() as f64
    }

    pub fn prod_skill(&self, recipe: RecipeKind) -> f64 {
        let ops: f64 = self.operators.iter().map(|o| o.total_eff(recipe)).sum();
        ops + self.station_eff.for_recipe(recipe)
    }

    pub fn prod_total(&self, recipe: RecipeKind) -> f64 {
        self.prod_base() + self.prod_skill(recipe) + self.layout.global_inject.manu_eff_for(recipe)
    }

    pub fn storage_limit(&self, recipe: RecipeKind) -> i32 {
        let gross: i32 = self
            .operators
            .iter()
            .map(|o| o.limit.for_recipe(recipe))
            .sum();
        (self.facility_base_storage + gross).max(1)
    }

    pub fn mood_drain_summary(&self) -> Vec<(String, f64)> {
        self.operators
            .iter()
            .map(|o| (o.name.clone(), o.mood_drain_delta))
            .collect()
    }
}

pub fn facility_base_storage(level: u8) -> i32 {
    match level {
        1 => 10,
        2 => 15,
        3 => 20,
        _ => 20,
    }
}

pub fn collect_manu_atoms<'a>(
    ops: &[ManuOperatorRuntime],
    table: &'a SkillTable,
) -> Vec<(&'a EffectAtom, String)> {
    let mut atoms = Vec::new();
    for op in ops {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else {
                continue;
            };
            if skill.facility != "manufacture" {
                continue;
            }
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

pub fn apply_manu_phases(ctx: &mut ManuContext, table: &SkillTable) {
    let names: Vec<String> = ctx.operators.iter().map(|o| o.name.clone()).collect();
    let atoms = {
        let ops = ctx.operators.clone();
        collect_manu_atoms(&ops, table)
    };

    for (atom, owner) in atoms {
        if !condition_met(&atom.condition, ctx, &owner) {
            continue;
        }
        apply_atom(ctx, atom, &owner, &names);
    }
    apply_pinus_sylvestris_control(ctx);
    apply_abyssal_hunters_control(ctx);
}

fn apply_pinus_sylvestris_control(ctx: &mut ManuContext) {
    const YANWEI: &str = "焰尾";
    const VIVIANA: &str = "薇薇安娜";
    const TAG_PINUS: &str = "cc.g.pinus";
    const TAG_KNIGHT: &str = "cc.g.knight";
    let has_yanwei = ctx.layout.control_workforce.iter().any(|n| n == YANWEI);
    let has_viviana = ctx.layout.control_workforce.iter().any(|n| n == VIVIANA);
    for op in &mut ctx.operators {
        if has_yanwei
            && ctx.active_recipe == RecipeKind::BattleRecord
            && op.tags.iter().any(|t| t == TAG_PINUS)
        {
            op.skill_eff.add(Some(RecipeKind::BattleRecord), 10.0);
        }
        if has_viviana && op.tags.iter().any(|t| t == TAG_KNIGHT) {
            op.skill_eff.add(None, 7.0);
        }
    }
}

fn apply_abyssal_hunters_control(ctx: &mut ManuContext) {
    const GLADIIA: &str = "歌蕾蒂娅";
    const GLADIIA_ALPHA: &str = "control_mp_aegir2[000]";
    const GLADIIA_BETA: &str = "control_mp_aegir2[010]";
    const TAG_ABYSSAL: &str = "cc.g.abyssal";

    let room_abyssal_count = ctx
        .operators
        .iter()
        .filter(|op| op.tags.iter().any(|t| t == TAG_ABYSSAL))
        .count() as f64;
    if room_abyssal_count == 0.0 {
        return;
    }

    let (rate, cap) = if ctx.layout.control_buff_active(GLADIIA, GLADIIA_BETA) {
        (10.0, 90.0)
    } else if ctx.layout.control_buff_active(GLADIIA, GLADIIA_ALPHA) {
        (5.0, 45.0)
    } else {
        (0.0, 0.0)
    };
    if rate == 0.0 {
        return;
    }

    let abyssal_count = f64::from(
        *ctx.layout
            .manu_tagged_count_sum
            .get(TAG_ABYSSAL)
            .unwrap_or(&0),
    );
    let bonus = (room_abyssal_count * abyssal_count * rate).min(cap);
    ctx.station_eff.add(None, bonus);
}

fn condition_met(cond: &Option<Condition>, ctx: &ManuContext, owner: &str) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::ActiveRecipe { kind } => ctx.active_recipe == *kind,
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::OwnerLacksBuff { buff_id } => ctx
            .operators
            .iter()
            .find(|o| o.name == owner)
            .is_none_or(|o| !o.buff_ids.iter().any(|b| b == buff_id)),
        Condition::OperatorInBase { name } => ctx.layout.base_workforce.iter().any(|n| n == name),
        Condition::OperatorInPower { name } => ctx.layout.power_workforce.iter().any(|n| n == name),
        Condition::OperatorInTraining { name } => {
            ctx.layout.training_assist.iter().any(|n| n == name)
        }
        Condition::OperatorInTrade { name } => ctx.layout.trade_workforce.iter().any(|n| n == name),
        Condition::NoPlatformInOtherPower {} => !ctx.layout.other_power_has_platform,
        Condition::OtherPlatformInPower {} => ctx.layout.other_platform_in_power,
        Condition::OtherLateranoInPower {} => ctx.layout.other_laterano_in_power,
        Condition::PartnerInRoom { name } => ctx.operators.iter().any(|o| o.name == *name),
        Condition::ExternalMomentumGteField {} => {
            ctx.layout.external_momentum() >= ctx.layout.field_momentum()
        }
        Condition::FieldMomentumGtExternal {} => {
            ctx.layout.field_momentum() > ctx.layout.external_momentum()
        }
        _ => false,
    }
}

const BUFF_PAOPAO_LIMIT_VAR: &str = "manu_prod_spd_variable3[000]";

fn room_has_buff(ctx: &ManuContext, buff_id: &str) -> bool {
    ctx.operators
        .iter()
        .any(|o| o.buff_ids.iter().any(|b| b == buff_id))
}

fn apply_atom(ctx: &mut ManuContext, atom: &EffectAtom, owner: &str, names: &[String]) {
    if matches!(&atom.action, Action::AddEffFromLimitContribSum { .. })
        && room_has_buff(ctx, BUFF_PAOPAO_LIMIT_VAR)
    {
        return;
    }
    match atom.phase {
        Phase::Constant | Phase::EffVar | Phase::LimitVar | Phase::OrderVar => {
            apply_eff_action(ctx, atom, owner);
        }
        Phase::Limit => apply_limit_action(ctx, atom, owner),
        Phase::Mood => apply_mood_action(ctx, &atom.action, owner),
        Phase::StateWrite => apply_state_write(ctx, atom, owner),
        Phase::PeerAbsorb => apply_peer_absorb(ctx, &atom.action, owner),
        Phase::GlobalInject | Phase::OrderMechanic | Phase::PeerShare => {}
    }
    let _ = names;
}

/// 按基建设施数量结算的生产力（冬时/森蚺归零时不计入「其他干员技能面」）。
fn eff_target_is_layout(atom: &EffectAtom) -> bool {
    matches!(
        atom.selector.as_ref(),
        Some(Selector::TradeStationCount)
            | Some(Selector::PowerStationCount)
            | Some(Selector::PlatformCountInPower)
    )
}

fn apply_peer_absorb(ctx: &mut ManuContext, action: &Action, owner: &str) {
    let Action::PeerEffAbsorb { rate_per_peer } = action else {
        return;
    };
    let peer_count = ctx.operators.iter().filter(|o| o.name != owner).count();
    for op in &mut ctx.operators {
        if op.name != owner {
            op.skill_eff.zero();
        }
    }
    if *rate_per_peer > 0.0 {
        if let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) {
            ctx.operators[idx]
                .skill_eff
                .add(None, peer_count as f64 * rate_per_peer);
        }
    }
}

fn apply_eff_action(ctx: &mut ManuContext, atom: &EffectAtom, owner: &str) {
    let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) else {
        return;
    };
    let value = resolve_eff_value(ctx, atom, owner);
    let recipe = match &atom.action {
        Action::AddFlatEff { recipe, .. } => *recipe,
        Action::AddFlatEffFromSelector { recipe, .. } => *recipe,
        _ => None,
    };
    if atom.tag.as_deref() == Some("station") {
        ctx.station_eff.add(recipe, value);
        return;
    }
    let target = if eff_target_is_layout(atom) {
        &mut ctx.operators[idx].layout_eff
    } else {
        &mut ctx.operators[idx].skill_eff
    };
    target.add(recipe, value);
}

fn limit_delta_counts_as_contrib(recipe: &Option<RecipeKind>) -> bool {
    matches!(recipe, None | Some(RecipeKind::All))
}

fn apply_limit_action(ctx: &mut ManuContext, atom: &EffectAtom, owner: &str) {
    let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) else {
        return;
    };
    match &atom.action {
        Action::AddLimitDelta { delta, recipe } => {
            ctx.operators[idx].limit.add(*recipe, *delta);
            if limit_delta_counts_as_contrib(recipe) {
                ctx.operators[idx].limit_contrib += delta;
            }
        }
        Action::AddLimitFromSelector { multiplier } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            let add = (base * multiplier).round() as i32;
            ctx.operators[idx].limit.add(None, add);
            ctx.operators[idx].limit_contrib += add;
        }
        Action::AddLimitFromState { key, multiplier } => {
            let Some(sk) = StateKey::parse(key) else {
                return;
            };
            let state = ctx.state_pool.get(&sk).copied().unwrap_or(0.0);
            let add = (state.floor() * multiplier).round() as i32;
            ctx.operators[idx].limit.add(None, add);
            ctx.operators[idx].limit_contrib += add;
        }
        _ => {}
    }
}

fn apply_mood_action(ctx: &mut ManuContext, action: &Action, owner: &str) {
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

fn apply_state_write(ctx: &mut ManuContext, atom: &EffectAtom, owner: &str) {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            if let Some(sk) = StateKey::parse(key) {
                let scale = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
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
    let _ = owner;
}

fn eff_from_limit_contrib_tiered(
    contrib: i32,
    threshold: i32,
    low_rate: f64,
    high_rate: f64,
) -> f64 {
    let c = contrib.max(0);
    if c > threshold {
        c as f64 * high_rate
    } else {
        c as f64 * low_rate
    }
}

fn resolve_eff_value(ctx: &ManuContext, atom: &EffectAtom, owner: &str) -> f64 {
    match &atom.action {
        Action::AddEffFromLimitContribSum { rate } => {
            ctx.operators
                .iter()
                .map(|o| o.limit_contrib.max(0))
                .sum::<i32>() as f64
                * rate
        }
        Action::AddEffFromLimitContribTiered {
            threshold,
            low_rate,
            high_rate,
        } => ctx
            .operators
            .iter()
            .map(|o| {
                eff_from_limit_contrib_tiered(o.limit_contrib, *threshold, *low_rate, *high_rate)
            })
            .sum(),
        Action::AddEffRamp {
            initial,
            per_hour,
            cap,
            style,
        } => eff_ramp_avg_20h(*style, *initial, *per_hour, *cap),
        Action::AddFlatEff { value, .. } => *value,
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

fn is_metal_formula_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "manu_formula_spd[100]" | "manu_formula_spd[101]" | "manu_formula_spd[110]"
    )
}

fn is_standard_skill_buff(buff_id: &str) -> bool {
    matches!(buff_id, "manu_prod_spd[000]" | "manu_prod_spd[010]")
}

fn is_rhine_skill_buff(buff_id: &str) -> bool {
    matches!(
        buff_id,
        "manu_prod_spd[001]" | "manu_prod_spd[011]" | "manu_prod_spd[021]"
    )
}

fn manu_skill_family_count_in_room(ctx: &ManuContext, pred: fn(&str) -> bool) -> f64 {
    ctx.operators
        .iter()
        .flat_map(|o| o.buff_ids.iter())
        .filter(|b| pred(b))
        .count() as f64
}

fn is_pinecone_skill_buff(buff_id: &str) -> bool {
    matches!(buff_id, "manu_prod_spd[002]" | "manu_prod_spd[012]")
}

const BUFF_HIGHMORE_COMPAT: &str = "manu_skill_change[000]";

fn room_has_highmore_compat(ctx: &ManuContext) -> bool {
    ctx.operators
        .iter()
        .flat_map(|o| o.buff_ids.iter())
        .any(|b| b == BUFF_HIGHMORE_COMPAT)
}

fn standard_skill_count_in_room(ctx: &ManuContext) -> f64 {
    let mut count = manu_skill_family_count_in_room(ctx, is_standard_skill_buff);
    if room_has_highmore_compat(ctx) {
        count += manu_skill_family_count_in_room(ctx, is_rhine_skill_buff);
        count += manu_skill_family_count_in_room(ctx, is_pinecone_skill_buff);
    }
    count
}

fn metal_formula_skill_count_in_room(ctx: &ManuContext) -> f64 {
    manu_skill_family_count_in_room(ctx, is_metal_formula_buff)
}

fn resolve_selector_value(ctx: &ManuContext, selector: Option<&Selector>, owner: &str) -> f64 {
    match selector {
        Some(Selector::FacilityLevel) => f64::from(ctx.facility_level),
        Some(Selector::MeetingMaxLevel) => f64::from(ctx.layout.meeting_max_level),
        Some(Selector::DormLevelSum) => f64::from(ctx.layout.dorm_level_sum),
        Some(Selector::ManuRecipeKinds) => f64::from(ctx.layout.manu_recipe_kinds),
        Some(Selector::EliteFacilityCount) => f64::from(ctx.layout.elite_facility_count),
        Some(Selector::SuiFacilityCount) => f64::from(ctx.layout.sui_facility_count),
        Some(Selector::CappedSuiFacilityCount { max }) => {
            f64::from(ctx.layout.sui_facility_count.min(*max))
        }
        Some(Selector::DormOccupantCount) => f64::from(ctx.layout.dorm_occupant_count),
        Some(Selector::TradeStationCount) => f64::from(ctx.layout.trade_station_count),
        Some(Selector::PowerStationCount) => f64::from(ctx.layout.effective_power_station_count()),
        Some(Selector::PlatformCountInPower) => f64::from(ctx.layout.platform_count_in_power),
        Some(Selector::RoomPeerCount) => {
            ctx.operators.iter().filter(|o| o.name != owner).count() as f64
        }
        Some(Selector::RoomOperatorCount) => ctx.operators.len() as f64,
        Some(Selector::LimitContribSum) => {
            ctx.operators.iter().map(|o| o.limit_contrib).sum::<i32>() as f64
        }
        Some(Selector::PeerSkillEffSum) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .map(|o| o.skill_eff.for_recipe(ctx.active_recipe))
            .sum(),
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::DroneCap) => ctx.layout.drone_cap as f64,
        Some(Selector::RhineLifeInBase) => f64::from(ctx.layout.rhine_life_in_base),
        Some(Selector::MetalFormulaSkillCountInRoom) => metal_formula_skill_count_in_room(ctx),
        Some(Selector::StandardSkillCountInRoom) => standard_skill_count_in_room(ctx),
        Some(Selector::RhineSkillCountInRoom) => {
            manu_skill_family_count_in_room(ctx, is_rhine_skill_buff)
        }
        Some(Selector::TaggedCountInRoom { tag }) => ctx
            .operators
            .iter()
            .filter(|o| o.tags.iter().any(|t| t == tag))
            .count() as f64,
        Some(Selector::FacilityLevelSumExclMeeting) => {
            f64::from(ctx.layout.facility_level_sum_excl_meeting).min(64.0)
        }
        Some(Selector::TrainingRoomLevel) => f64::from(ctx.layout.training_room_level),
        Some(Selector::TaggedCountInControl { .. }) | Some(Selector::ControlOperatorCount) => 0.0,
        None => 0.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::control::{solve_control, ControlOperator, ControlRoomInput};
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::LayoutContext;
    use crate::manufacture::input::{ManuOperator, ManuRoomInput};
    use crate::manufacture::solver::solve_manufacture;
    use crate::skill_table::SkillTable;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    fn op(name: &str, elite: u8, buff_ids: Vec<&str>) -> ManuOperator {
        ManuOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    }

    fn manu_op_from_instances(name: &str, elite: u8) -> ManuOperator {
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let tier = PromotionTier::from_elite(elite);
        let tags = instances
            .get(name, tier)
            .map(|i| i.tags.clone())
            .unwrap_or_default();
        ManuOperator {
            name: name.to_string(),
            elite,
            buff_ids: instances.resolve_manufacture_buff_ids(name, tier),
            tags,
        }
    }

    #[test]
    fn flat_prod_all_recipes() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("蛇屠箱", 0, vec!["manu_prod_spd&limit[001]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 10.0).abs() < 0.01);
        assert!((ctx.prod_total(RecipeKind::Gold) - 11.0).abs() < 0.01);
    }

    #[test]
    fn recipe_specific_prod_only_on_matching_recipe() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![op("芬", 0, vec!["manu_formula_spd[010]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.prod_skill(RecipeKind::BattleRecord) - 30.0).abs() < 0.01);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 0.0).abs() < 0.01);
    }

    #[test]
    fn active_recipe_gates_mood() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("卡达", 0, vec!["manu_formula_cost[000]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].mood_drain_delta - 0.0).abs() < 0.01);

        input.active_recipe = RecipeKind::BattleRecord;
        ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].mood_drain_delta + 0.25).abs() < 0.01);
    }

    #[test]
    fn trade_station_count_boosts_gold_only() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("清流", 0, vec!["manu_prod_spd&trade[000]"])],
        );
        Arc::make_mut(&mut input.layout).trade_station_count = 3;
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 60.0).abs() < 0.01);
        assert!((ctx.prod_skill(RecipeKind::BattleRecord) - 0.0).abs() < 0.01);
    }

    #[test]
    fn recipe_specific_storage_limit() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![op("空", 2, vec!["manu_formula_limit[010]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert_eq!(ctx.storage_limit(RecipeKind::BattleRecord), 35);
        assert_eq!(ctx.storage_limit(RecipeKind::Gold), 20);
    }

    #[test]
    fn dongshi_tier0_zeros_peer_skill_eff_and_adds_storage() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 0, vec!["manu_prod_spd&manu[000]"]),
                op("芬", 0, vec!["manu_prod_spd[000]"]),
                op("克洛丝", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        // 科学改造：清零其他干员生产力；3 人 × 5 仓库。
        assert!((ctx.prod_skill(RecipeKind::Gold) - 0.0).abs() < 0.01);
        assert!((ctx.prod_total(RecipeKind::Gold) - 3.0).abs() < 0.01);
        assert_eq!(ctx.storage_limit(RecipeKind::Gold), 35);
    }

    #[test]
    fn automation_power_station_bonus_after_zeroing_peers() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("森蚺", 2, vec!["manu_prod_spd&power[010]"]),
                op("芬", 0, vec!["manu_prod_spd[000]"]),
                op("克洛丝", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        Arc::make_mut(&mut input.layout).power_station_count = 2;
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        // 森蚺 +20% (2 power × 10%), peers zeroed
        assert!((ctx.prod_skill(RecipeKind::Gold) - 20.0).abs() < 0.01);
    }

    #[test]
    fn dongshi_station_eff_not_on_personal_skill_slot() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 1, vec!["manu_prod_spd&manu[100]"]),
                op("芬", 0, vec!["manu_prod_spd[000]"]),
                op("克洛丝", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.station_eff.all - 30.0).abs() < 0.01);
        let dongshi = ctx.operators.iter().find(|o| o.name == "冬时").unwrap();
        assert!((dongshi.skill_eff.all - 0.0).abs() < 0.01);
        assert_eq!(dongshi.limit.all, 15);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 30.0).abs() < 0.01);
    }

    fn automation_group_1_layout() -> LayoutContext {
        LayoutContext::automation_group_1()
    }

    #[test]
    fn automation_group_1_winter_qingliu_wendi_gold() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 1, vec!["manu_prod_spd&manu[100]"]),
                op("清流", 1, vec!["manu_prod_spd&trade[000]"]),
                op("温蒂", 2, vec!["manu_prod_spd&power[020]"]),
            ],
        );
        input.layout = Arc::new(automation_group_1_layout());
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        // 252 自动组1：有效发电 5、贸易 2；冬时站级 30 + 温蒂 5×15% + 清流 2×20% = 145%
        assert!((ctx.station_eff.all - 30.0).abs() < 0.01);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 145.0).abs() < 0.5);
        assert!((ctx.prod_total(RecipeKind::Gold) - 148.0).abs() < 0.5);
        assert_eq!(ctx.storage_limit(RecipeKind::Gold), 35);
    }

    #[test]
    fn automation_group_1_passenger_baseline_gold() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("异客", 2, vec!["manu_prod_spd&power[000]"]),
                op("清流", 1, vec!["manu_prod_spd&trade[000]"]),
                op("温蒂", 2, vec!["manu_prod_spd&power[020]"]),
            ],
        );
        input.layout = Arc::new(automation_group_1_layout());
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        // 异客版：5×5% + 5×15% + 2×20% = 140%（冬时换异客 +5%）
        assert!((ctx.prod_skill(RecipeKind::Gold) - 140.0).abs() < 0.5);
        assert!((ctx.prod_total(RecipeKind::Gold) - 143.0).abs() < 0.5);
    }

    #[test]
    fn terra_snhunt_manu_matatabi_from_global_pool() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout
            .global
            .set(crate::global_resource::GlobalResourceKey::Matatabi, 12.0);
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op(
                "泰拉大陆调查团",
                0,
                vec!["manu_prod_spd&limit&bd[000]".into()],
            )],
        );
        input.layout = Arc::new(layout);
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!(
            (ctx.prod_skill(RecipeKind::Gold) - 17.0).abs() < 0.01,
            "5% flat + 12×1% matatabi = 17%, got {}",
            ctx.prod_skill(RecipeKind::Gold)
        );
        assert_eq!(ctx.storage_limit(RecipeKind::Gold), 20 + 8);
    }

    #[test]
    fn peer_absorb_preserves_trade_station_layout_eff() {
        let table = table();
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 0, vec!["manu_prod_spd&manu[000]"]),
                op("清流", 0, vec!["manu_prod_spd&trade[000]"]),
                op("芬", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        Arc::make_mut(&mut input.layout).trade_station_count = 3;
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        // 科学改造无生产力；清流 trade layout +60% kept, 芬 skill zeroed
        assert!((ctx.prod_skill(RecipeKind::Gold) - 60.0).abs() < 0.01);
    }

    #[test]
    fn manu_huaihu_peer_skill_eff_bucket() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("槐琥", 2, vec!["manu_prod_spd_variable2[000]"]),
                op("芬", 0, vec!["manu_prod_spd[000]"]),
                op("克洛丝", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let huaihu = ctx.operators.iter().find(|o| o.name == "槐琥").unwrap();
        // 2 peers × 15% = 30% → floor(30/5)×5% = 30%
        assert!((huaihu.skill_eff.all - 30.0).abs() < 0.01);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 60.0).abs() < 0.01);
    }

    #[test]
    fn manu_paopao_fire_god_e2_limit_tiered() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op(
                    "泡泡",
                    2,
                    vec!["manu_prod_limit&cost[010]", "manu_prod_spd_variable3[000]"],
                ),
                op("火神", 2, vec!["manu_prod_spd&limit&cost[001]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let paopao = ctx.operators.iter().find(|o| o.name == "泡泡").unwrap();
        // 泡泡 10×1% + 火神 19×3% = 67%
        assert!((paopao.skill_eff.all - 67.0).abs() < 0.01);
    }

    #[test]
    fn manu_paopao_bena_limit_tiered() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op(
                    "泡泡",
                    2,
                    vec!["manu_prod_limit&cost[010]", "manu_prod_spd_variable3[000]"],
                ),
                op("贝娜", 0, vec!["manu_prod_spd&limit&cost[020]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let paopao = ctx.operators.iter().find(|o| o.name == "泡泡").unwrap();
        // 泡泡 10×1% + 贝娜 17×3% = 61%
        assert!((paopao.skill_eff.all - 61.0).abs() < 0.01);
    }

    #[test]
    fn manu_paopao_redcloud_recycle_mutex() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op(
                    "泡泡",
                    2,
                    vec!["manu_prod_limit&cost[010]", "manu_prod_spd_variable3[000]"],
                ),
                op(
                    "红云",
                    1,
                    vec!["manu_prod_limit&cost[0000]", "manu_prod_spd_variable[000]"],
                ),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let paopao = ctx.operators.iter().find(|o| o.name == "泡泡").unwrap();
        // 回收利用不生效；泡泡 10% + 红云 8×1% = 18%
        assert!((paopao.skill_eff.all - 18.0).abs() < 0.01);
    }

    #[test]
    fn manu_hongyun_recycle_limit_sum() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op(
                    "红云",
                    2,
                    vec!["manu_prod_limit&cost[0000]", "manu_prod_spd_variable[000]"],
                ),
                op("蛇屠箱", 0, vec!["manu_prod_limit&cost[001]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let hongyun = ctx.operators.iter().find(|o| o.name == "红云").unwrap();
        // 红云 +8、蛇屠箱 +8 → 16×2% = 32%
        assert!((hongyun.skill_eff.all - 32.0).abs() < 0.01);
    }

    #[test]
    fn dongshi_zeros_hongyun_recycle_skill_eff() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 1, vec!["manu_prod_spd&manu[100]"]),
                op(
                    "红云",
                    2,
                    vec!["manu_prod_limit&cost[0000]", "manu_prod_spd_variable[000]"],
                ),
                op("蛇屠箱", 0, vec!["manu_prod_limit&cost[001]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let hongyun = ctx.operators.iter().find(|o| o.name == "红云").unwrap();
        assert!(
            (hongyun.skill_eff.all - 0.0).abs() < 0.01,
            "冬时应清零红云回收利用，got {}",
            hongyun.skill_eff.all
        );
        // 冬时站级 3×10%=30%，无红云回收
        assert!((ctx.station_eff.all - 30.0).abs() < 0.01);
        assert!((ctx.prod_skill(RecipeKind::Gold) - 30.0).abs() < 0.01);
    }

    #[test]
    fn manu_fen_acute_ramp_20h_avg() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("芬", 0, vec!["manu_prod_spd_addition[030]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].skill_eff.all - 24.25).abs() < 0.01);
    }

    #[test]
    fn manu_kroos_slow_ramp_20h_avg() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("克洛丝", 0, vec!["manu_prod_spd_addition[040]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].skill_eff.all - 23.5).abs() < 0.01);
    }

    #[test]
    fn manu_xiyin_timelapse_ramp_20h_avg() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("稀音", 0, vec!["manu_prod_spd_addition[041]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].skill_eff.all - 23.5).abs() < 0.01);
    }

    #[test]
    fn manu_aroma_routine_clean_ramp_20h_avg() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("阿罗玛", 2, vec!["manu_prod_spd_addition[100]"])],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        assert!((ctx.operators[0].skill_eff.all - 15.5).abs() < 0.01);
    }

    #[test]
    fn dongshi_zeros_fen_ramp_skill_eff() {
        let table = table();
        let input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("冬时", 0, vec!["manu_prod_spd&manu[000]"]),
                op("芬", 0, vec!["manu_prod_spd_addition[030]"]),
                op("克洛丝", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let fen = ctx.operators.iter().find(|o| o.name == "芬").unwrap();
        assert!((fen.skill_eff.all - 0.0).abs() < 0.01);
    }

    #[test]
    fn gladiia_control_boosts_each_abyssal_by_global_hunter_count() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.control_workforce.push("歌蕾蒂娅".to_string());
        layout
            .control_buffs
            .push(("歌蕾蒂娅".to_string(), "control_mp_aegir2[010]".to_string()));
        layout
            .manu_tagged_count_sum
            .insert("cc.g.abyssal".to_string(), 4);

        let mut abyssal_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                ManuOperator {
                    name: "乌尔比安".into(),
                    elite: 2,
                    buff_ids: vec![],
                    tags: vec!["cc.g.abyssal".into()],
                },
                ManuOperator {
                    name: "斯卡蒂".into(),
                    elite: 2,
                    buff_ids: vec![],
                    tags: vec!["cc.g.abyssal".into()],
                },
                op("芬", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        abyssal_room.layout = Arc::new(layout.clone());
        let abyssal = crate::manufacture::solver::solve_manufacture(&abyssal_room, &table).unwrap();
        assert!(
            (abyssal.prod_skill - 95.0).abs() < 0.01,
            "2 abyssal hunters in room × 4 abyssal hunters in manufacture ×10 plus 芬15, got {}",
            abyssal.prod_skill
        );

        let mut normal_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("芬", 0, vec!["manu_prod_spd[000]"])],
        );
        normal_room.layout = Arc::new(layout);
        let normal = crate::manufacture::solver::solve_manufacture(&normal_room, &table).unwrap();
        assert!(
            (normal.prod_skill - 15.0).abs() < 0.01,
            "non-abyssal room must not inherit global abyssal boost, got {}",
            normal.prod_skill
        );

        let mut capped_layout = (*abyssal_room.layout).clone();
        capped_layout
            .manu_tagged_count_sum
            .insert("cc.g.abyssal".to_string(), 12);
        abyssal_room.layout = Arc::new(capped_layout);
        let capped = crate::manufacture::solver::solve_manufacture(&abyssal_room, &table).unwrap();
        assert!(
            (capped.prod_skill - 105.0).abs() < 0.01,
            "tier_up cap 90 plus 芬15, got {}",
            capped.prod_skill
        );
    }

    #[test]
    fn gongsun_243_automation_trio_with_greyy2_virtual_power() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        // 公孙事实布局 243_use_this_（2 金贸）+ 承曦格雷伊晨曦 → 有效发电 4
        let mut layout = crate::layout::resolve_search_baseline_layout().unwrap();
        layout
            .global
            .add(crate::global_resource::GlobalResourceKey::VirtualPower, 1.0);
        assert_eq!(layout.trade_station_count, 2);
        assert_eq!(layout.effective_power_station_count(), 4);

        let ops: Vec<ManuOperator> = ["冬时", "清流", "温蒂"]
            .iter()
            .map(|&name| {
                let (elite, rarity) = match name {
                    "冬时" | "清流" => (1_u8, 4_u8),
                    "温蒂" => (2, 6),
                    _ => (0, 0),
                };
                let tier = PromotionTier::from_elite_rarity_level(elite, rarity, 1);
                let mut buffs = instances.resolve_manufacture_buff_ids(name, tier);
                if name == "温蒂" {
                    // 精二仅生效仿生海龙；instances 无 tier_0 制造绑定时 stepwise 会误叠 [010]+[020]
                    buffs.retain(|b| b == "manu_prod_spd&power[020]");
                }
                ManuOperator::new(name, elite, buffs)
            })
            .collect();
        let mut input = ManuRoomInput::with_operators(3, RecipeKind::Gold, ops);
        input.layout = Arc::new(layout);
        let result = crate::manufacture::solver::solve_manufacture(&input, &table).unwrap();
        // 冬时站级 30 + 清流 2×20% + 温蒂 4×15%（仿生海龙）= 130%
        assert!(
            (result.prod_skill - 130.0).abs() < 0.5,
            "prod_skill={} prod_total={}",
            result.prod_skill,
            result.prod_total
        );
        assert!((result.prod_total - 133.0).abs() < 0.5);
    }

    #[test]
    fn ideal_e2_saria_qingliu_weedy_gold_140_with_greyy2_power() {
        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let mut layout = crate::layout::resolve_search_baseline_layout().unwrap();
        layout
            .global
            .add(crate::global_resource::GlobalResourceKey::VirtualPower, 1.0);
        let ops: Vec<ManuOperator> = ["森蚺", "清流", "温蒂"]
            .iter()
            .map(|&name| {
                let tier = PromotionTier::TierUp;
                ManuOperator::new(name, 2, instances.resolve_manufacture_buff_ids(name, tier))
            })
            .collect();
        let mut input = ManuRoomInput::with_operators(3, RecipeKind::Gold, ops);
        input.layout = Arc::new(layout);
        let result = crate::manufacture::solver::solve_manufacture(&input, &table).unwrap();
        assert_eq!(
            instances.resolve_manufacture_buff_ids("森蚺", PromotionTier::TierUp),
            vec!["manu_prod_spd&power[010]".to_string()]
        );
        // 清流 2×20% + 温蒂 4×15% + 森蚺 4×10% = 140%
        assert!(
            (result.prod_skill - 140.0).abs() < 0.5,
            "prod_skill={} prod_total={}",
            result.prod_skill,
            result.prod_total
        );
        assert!((result.prod_total - 143.0).abs() < 0.5);
    }

    #[test]
    fn pinus_sylvestris_br_room_126_and_gravel_gold_42() {
        use crate::layout::AssignedOperator;
        use crate::layout::{resolve_base, BaseAssignment, BaseBlueprint};

        let table = table();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let blueprint = BaseBlueprint::template_243_use_this().unwrap();

        let mut assignment = BaseAssignment::default();
        assignment.set_room(
            "control",
            vec![
                AssignedOperator::new("焰尾", 2),
                AssignedOperator::new("薇薇安娜", 2),
            ],
        );
        assignment.set_room(
            "manu_1",
            vec![
                AssignedOperator::new("灰毫", 2),
                AssignedOperator::new("远牙", 2),
                AssignedOperator::new("野鬃", 2),
            ],
        );
        assignment.set_room("manu_4", vec![AssignedOperator::new("砾", 2)]);

        let resolved = resolve_base(
            &blueprint,
            &assignment,
            Some(&instances),
            Some(&table),
            24.0,
            None,
        )
        .unwrap();
        let br_room = resolved
            .manu_rooms
            .iter()
            .find(|r| r.id.0 == "manu_1")
            .unwrap();
        let br_input = ManuRoomInput {
            level: br_room.level,
            operators: br_room.operators.clone(),
            active_recipe: RecipeKind::BattleRecord,
            mood: 24.0,
            layout: std::sync::Arc::new(br_room.layout.clone()),
        };
        let br = crate::manufacture::solver::solve_manufacture(&br_input, &table).unwrap();
        // 公孙社区口径 126% = 75 本体 + 60 焰尾 + 21 薇薇安娜；不含人头。
        assert!(
            (br.prod_skill - 126.0).abs() < 1.5,
            "BR prod_total={} prod_skill={} (target skill ~126)",
            br.prod_total,
            br.prod_skill
        );
        assert!(
            (br.prod_total - 129.0).abs() < 1.5,
            "BR prod_total={} prod_skill={}",
            br.prod_total,
            br.prod_skill
        );

        let gravel_room = resolved
            .manu_rooms
            .iter()
            .find(|r| r.id.0 == "manu_4")
            .unwrap();
        let gold_input = ManuRoomInput {
            level: gravel_room.level,
            operators: gravel_room.operators.clone(),
            active_recipe: RecipeKind::Gold,
            mood: 24.0,
            layout: std::sync::Arc::new(gravel_room.layout.clone()),
        };
        let gold = crate::manufacture::solver::solve_manufacture(&gold_input, &table).unwrap();
        assert!(
            (gold.prod_skill - 42.0).abs() < 0.5,
            "砾本体 35% + 薇薇安娜 7% gold skill"
        );
        assert!(
            (gold.prod_total - 45.0).abs() < 3.0,
            "gold prod_total={}",
            gold.prod_total
        );
    }

    fn cangtai_e2_buffs() -> Vec<&'static str> {
        vec!["manu_formula_spd[100]", "manu_skill_spd1[020]"]
    }

    #[test]
    fn cangtai_work_experience_counts_metal_formula_in_room() {
        let table = table();
        let solo = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op("苍苔", 2, cangtai_e2_buffs())],
        );
        let solo_r = crate::manufacture::solver::solve_manufacture(&solo, &table).unwrap();
        // 金属工艺 30 + 打工心得 1×5%
        assert!((solo_r.prod_skill - 35.0).abs() < 0.01);

        let two_metal = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("苍苔", 2, cangtai_e2_buffs()),
                op("夜烟", 2, vec!["manu_formula_spd[100]"]),
            ],
        );
        let two_r = crate::manufacture::solver::solve_manufacture(&two_metal, &table).unwrap();
        // 苍苔 30+10 + 夜烟 30 = 70
        assert!((two_r.prod_skill - 70.0).abs() < 0.01);

        let three_metal = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                op("苍苔", 2, cangtai_e2_buffs()),
                op("夜烟", 2, vec!["manu_formula_spd[100]"]),
                op("斑点", 2, vec!["manu_formula_spd[100]"]),
            ],
        );
        let three_r = crate::manufacture::solver::solve_manufacture(&three_metal, &table).unwrap();
        // 苍苔 30+15 + 夜烟/斑点各 30 = 105
        assert!((three_r.prod_skill - 105.0).abs() < 0.01);

        let mut ctx = ManuContext::from_room(&three_metal);
        apply_manu_phases(&mut ctx, &table);
        let cangtai = ctx.operators.iter().find(|o| o.name == "苍苔").unwrap();
        assert!((cangtai.total_eff(RecipeKind::Gold) - 45.0).abs() < 0.01);
    }

    #[test]
    fn cangtai_with_resolved_instances_on_search_baseline() {
        let table = table();
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let layout = Arc::new(crate::layout::resolve_search_baseline_layout().unwrap());
        let mk = |names: &[(&str, u8)]| {
            let ops: Vec<_> = names
                .iter()
                .map(|(name, elite)| {
                    let tier = PromotionTier::from_elite(*elite);
                    let buffs = instances.resolve_manufacture_buff_ids(name, tier);
                    op(name, *elite, buffs.iter().map(String::as_str).collect())
                })
                .collect();
            ManuRoomInput {
                level: 3,
                active_recipe: RecipeKind::Gold,
                operators: ops,
                mood: 24.0,
                layout: layout.clone(),
            }
        };

        let with_yeyan = solve_manufacture(&mk(&[("苍苔", 2), ("夜烟", 2)]), &table).unwrap();
        assert!((with_yeyan.prod_skill - 70.0).abs() < 0.01);

        let with_thorn =
            solve_manufacture(&mk(&[("苍苔", 2), ("夜烟", 2), ("引星棘刺", 2)]), &table).unwrap();
        let mut ctx = ManuContext::from_room(&mk(&[("苍苔", 2), ("夜烟", 2), ("引星棘刺", 2)]));
        apply_manu_phases(&mut ctx, &table);
        let cangtai = ctx.operators.iter().find(|o| o.name == "苍苔").unwrap();
        assert!((cangtai.total_eff(RecipeKind::Gold) - 45.0).abs() < 0.01);
        // 夜烟 30 + 引星棘刺 30+2×3 + 苍苔 30+3×5 = 111
        assert!(
            (with_thorn.prod_skill - 111.0).abs() < 0.01,
            "got {}",
            with_thorn.prod_skill
        );

        let with_gravel =
            solve_manufacture(&mk(&[("苍苔", 2), ("砾", 2), ("引星棘刺", 2)]), &table).unwrap();
        let mut ctx = ManuContext::from_room(&mk(&[("苍苔", 2), ("砾", 2), ("引星棘刺", 2)]));
        apply_manu_phases(&mut ctx, &table);
        let cangtai_gravel = ctx.operators.iter().find(|o| o.name == "苍苔").unwrap();
        assert!((cangtai_gravel.total_eff(RecipeKind::Gold) - 45.0).abs() < 0.01);
        // 苍苔 45 + 砾 35 + 引星棘刺 36(30 + 2 贸易站×3%)
        assert!(
            (with_gravel.prod_skill - 116.0).abs() < 0.01,
            "got {}",
            with_gravel.prod_skill
        );
    }

    #[test]
    fn christine_banquet_requires_dionysus_in_room() {
        let table = table();
        let christine_e2 = vec!["manu_prod_limit&cost[012]", "manu_prod_spd_double[100]"];
        let solo = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![op("Miss.Christine", 2, christine_e2.clone())],
        );
        let solo_r = solve_manufacture(&solo, &table).unwrap();
        assert!(
            solo_r.prod_skill.abs() < 0.01,
            "solo Christine should not get banquet bonus, got {}",
            solo_r.prod_skill
        );

        let paired = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![
                op("Miss.Christine", 2, christine_e2),
                op("酒神", 2, vec!["manu_formula_spd&limit&cost[010]"]),
            ],
        );
        let paired_r = solve_manufacture(&paired, &table).unwrap();
        let mut ctx = ManuContext::from_room(&paired);
        apply_manu_phases(&mut ctx, &table);
        let christine = ctx
            .operators
            .iter()
            .find(|o| o.name == "Miss.Christine")
            .unwrap();
        assert!((christine.total_eff(RecipeKind::BattleRecord) - 30.0).abs() < 0.01);
        assert!(
            (paired_r.prod_skill - 65.0).abs() < 0.01,
            "got {}",
            paired_r.prod_skill
        );
    }

    #[test]
    fn rifle_finn_standard_alpha_flat_15() {
        let table = table();
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op(
                "历阵锐枪芬",
                0,
                instances
                    .resolve_manufacture_buff_ids("历阵锐枪芬", PromotionTier::Tier0)
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )],
        );
        let result = solve_manufacture(&room, &table).unwrap();
        assert!((result.prod_skill - 15.0).abs() < 0.01);
    }

    fn alanana_room(elite: u8, platforms: u8, with_wenmi: bool) -> ManuRoomInput {
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let tier = PromotionTier::from_elite(elite);
        let mut ops = vec![op(
            "阿兰娜",
            elite,
            instances
                .resolve_manufacture_buff_ids("阿兰娜", tier)
                .iter()
                .map(String::as_str)
                .collect(),
        )];
        if with_wenmi {
            ops.push(op(
                "温米",
                2,
                instances
                    .resolve_manufacture_buff_ids("温米", PromotionTier::TierUp)
                    .iter()
                    .map(String::as_str)
                    .collect(),
            ));
        }
        let mut room = ManuRoomInput::with_operators(3, RecipeKind::Gold, ops);
        Arc::make_mut(&mut room.layout).platform_count_in_power = platforms;
        room
    }

    fn zhijian_room(elite: u8) -> ManuRoomInput {
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let tier = PromotionTier::from_elite(elite);
        let mut room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op(
                "至简",
                elite,
                instances
                    .resolve_manufacture_buff_ids("至简", tier)
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )],
        );
        room.layout = Arc::new(crate::layout::resolve_search_baseline_layout().unwrap());
        room
    }

    #[test]
    fn zhijian_drawing_design_produces_robots_from_layout() {
        let table = table();
        let mut ctx = ManuContext::from_room(&zhijian_room(0));
        apply_manu_phases(&mut ctx, &table);
        let robots = ctx
            .state_pool
            .get(&crate::global_resource::GlobalResourceKey::EngineeringRobot)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (robots - 45.0).abs() < f64::EPSILON,
            "243_use_this_ 设施等级和 45（含办公室 Lv3），got {robots}"
        );
    }

    #[test]
    fn zhijian_mechanical_mastery_e0_on_search_baseline() {
        let table = table();
        let result = solve_manufacture(&zhijian_room(0), &table).unwrap();
        // floor(42/16)×5 = 10%
        assert!(
            (result.prod_skill - 10.0).abs() < 0.01,
            "got {}",
            result.prod_skill
        );
    }

    #[test]
    fn zhijian_mechanical_mastery_e2_on_search_baseline() {
        let table = table();
        let result = solve_manufacture(&zhijian_room(2), &table).unwrap();
        // floor(42/8)×5 = 25%
        assert!(
            (result.prod_skill - 25.0).abs() < 0.01,
            "got {}",
            result.prod_skill
        );
    }

    #[test]
    fn alanana_mechanical_mastery_scales_with_platform_count() {
        let table = table();
        let e0_one = solve_manufacture(&alanana_room(0, 1, false), &table).unwrap();
        assert!(
            (e0_one.prod_skill - 5.0).abs() < 0.01,
            "got {}",
            e0_one.prod_skill
        );

        let e2_three = solve_manufacture(&alanana_room(2, 3, false), &table).unwrap();
        assert!(
            (e2_three.prod_skill - 30.0).abs() < 0.01,
            "got {}",
            e2_three.prod_skill
        );
    }

    #[test]
    fn alanana_lending_hand_requires_wenmi_in_room() {
        let table = table();
        let solo = solve_manufacture(&alanana_room(2, 1, false), &table).unwrap();
        assert!(
            (solo.prod_skill - 10.0).abs() < 0.01,
            "got {}",
            solo.prod_skill
        );

        let paired = solve_manufacture(&alanana_room(2, 1, true), &table).unwrap();
        let mut ctx = ManuContext::from_room(&alanana_room(2, 1, true));
        apply_manu_phases(&mut ctx, &table);
        let alanana = ctx.operators.iter().find(|o| o.name == "阿兰娜").unwrap();
        assert!((alanana.total_eff(RecipeKind::Gold) - 25.0).abs() < 0.01);
        assert!(
            (paired.prod_skill - 55.0).abs() < 0.01,
            "got {}",
            paired.prod_skill
        );
    }

    #[test]
    fn alanana_platform_bonus_survives_dongshi_peer_absorb() {
        let table = table();
        let mut room = alanana_room(2, 2, true);
        room.operators
            .push(op("冬时", 1, vec!["manu_prod_spd&manu[100]"]));
        let mut ctx = ManuContext::from_room(&room);
        apply_manu_phases(&mut ctx, &table);
        let alanana = ctx.operators.iter().find(|o| o.name == "阿兰娜").unwrap();
        assert!(
            (alanana.layout_eff.gold - 20.0).abs() < 0.01,
            "layout 机械精通应保留，got {}",
            alanana.layout_eff.gold
        );
        assert!(
            alanana.skill_eff.gold.abs() < 0.01,
            "搭把手应被冬时清零，got {}",
            alanana.skill_eff.gold
        );
    }

    #[test]
    fn manu_marcille_monster_cuisine() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.global.set(
            crate::global_resource::GlobalResourceKey::MonsterCuisine,
            3.0,
        );
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![op(
                "玛露西尔",
                2,
                vec!["manu_prod_spd_bd[400]", "manu_prod_spd[014]"],
            )],
        );
        input.layout = Arc::new(layout);
        let mut ctx = ManuContext::from_room(&input);
        apply_manu_phases(&mut ctx, &table);
        let marcille = ctx.operators.iter().find(|o| o.name == "玛露西尔").unwrap();
        // 魔物料理 3 层 +3% + 固定 30%
        assert!((marcille.skill_eff.all - 33.0).abs() < 0.01);
    }

    fn a1_filler(name: &str) -> ManuOperator {
        let mut filler = op(name, 0, vec![]);
        filler.tags = vec!["cc.g.a1".into()];
        filler
    }

    #[test]
    fn rifle_finn_reunion_time_scales_with_a1_in_room() {
        let table = table();
        let solo = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("历阵锐枪芬", 2)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (solo.prod_skill - 15.0).abs() < 0.01,
            "solo got {}",
            solo.prod_skill
        );

        let paired_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("历阵锐枪芬", 2),
                a1_filler("芬"),
                a1_filler("克洛丝"),
            ],
        );
        let paired = solve_manufacture(&paired_room, &table).unwrap();
        // 精2 标准化·α +15%；同房 2 名 A1 → +20%（芬/克洛丝仅作 tag 载体）
        assert!(
            (paired.prod_skill - 35.0).abs() < 0.01,
            "paired got {}",
            paired.prod_skill
        );

        let mut ctx = ManuContext::from_room(&paired_room);
        apply_manu_phases(&mut ctx, &table);
        let finn = ctx
            .operators
            .iter()
            .find(|o| o.name == "历阵锐枪芬")
            .unwrap();
        assert!((finn.total_eff(RecipeKind::Gold) - 35.0).abs() < 0.01);
    }

    #[test]
    fn liesha_partner_requires_gumi_in_trade() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.trade_workforce = vec!["古米".into()];
        let mut with_gumi = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![manu_op_from_instances("烈夏", 2)],
        );
        with_gumi.layout = Arc::new(layout);
        let result = solve_manufacture(&with_gumi, &table).unwrap();
        assert!(
            (result.prod_skill - 35.0).abs() < 0.01,
            "with 古米 got {}",
            result.prod_skill
        );

        let without = ManuRoomInput::with_operators(
            3,
            RecipeKind::BattleRecord,
            vec![manu_op_from_instances("烈夏", 2)],
        );
        let solo = solve_manufacture(&without, &table).unwrap();
        assert!(
            solo.prod_skill.abs() < 0.01,
            "without 古米 got {}",
            solo.prod_skill
        );
    }

    #[test]
    fn mizuki_protocol_scales_with_standard_skills_in_room() {
        let table = table();
        let e0 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("水月", 0)],
            ),
            &table,
        )
        .unwrap();
        assert!(e0.prod_skill.abs() < 0.01, "e0 solo got {}", e0.prod_skill);

        let e2 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("水月", 2)],
            ),
            &table,
        )
        .unwrap();
        // 标准化·β +25%；自身 1 个标准化类技能 → 意识协议 +5%
        assert!(
            (e2.prod_skill - 30.0).abs() < 0.01,
            "e2 solo got {}",
            e2.prod_skill
        );

        let paired_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("水月", 2),
                op("史都华德", 0, vec!["manu_prod_spd[000]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&paired_room);
        apply_manu_phases(&mut ctx, &table);
        let mizuki = ctx.operators.iter().find(|o| o.name == "水月").unwrap();
        assert!(
            (mizuki.total_eff(RecipeKind::Gold) - 35.0).abs() < 0.01,
            "paired mizuki got {}",
            mizuki.total_eff(RecipeKind::Gold)
        );
    }

    #[test]
    fn dorothy_theory_scales_with_rhine_skills_in_room() {
        let table = table();
        let e0 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("多萝西", 0)],
            ),
            &table,
        )
        .unwrap();
        assert!(e0.prod_skill.abs() < 0.01, "e0 solo got {}", e0.prod_skill);

        let e2 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("多萝西", 2)],
            ),
            &table,
        )
        .unwrap();
        // 莱茵科技·β +25%；自身 1 个莱茵类技能 → 源石技艺理论应用 +5%
        assert!(
            (e2.prod_skill - 30.0).abs() < 0.01,
            "e2 solo got {}",
            e2.prod_skill
        );

        let paired_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("多萝西", 2),
                op("赫默", 0, vec!["manu_prod_spd[001]"]),
            ],
        );
        let mut ctx = ManuContext::from_room(&paired_room);
        apply_manu_phases(&mut ctx, &table);
        let dorothy = ctx.operators.iter().find(|o| o.name == "多萝西").unwrap();
        assert!(
            (dorothy.total_eff(RecipeKind::Gold) - 35.0).abs() < 0.01,
            "paired dorothy got {}",
            dorothy.total_eff(RecipeKind::Gold)
        );
    }

    #[test]
    fn nasti_costly_gold_scales_with_rhine_life_in_base() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.rhine_life_in_base = 5;
        let mut input = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("娜斯提", 2)],
        );
        input.layout = Arc::new(layout);
        let result = solve_manufacture(&input, &table).unwrap();
        // 莱茵·β 25% + 造价高昂 5×3% capped 15%
        assert!(
            (result.prod_skill - 40.0).abs() < 0.5,
            "prod_skill={}",
            result.prod_skill
        );
    }

    #[test]
    fn stella_prospecting_pack_scales_with_rhine_skills_in_room() {
        let table = table();
        let e0_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("溯光星源", 0)],
        );
        let mut e0_ctx = ManuContext::from_room(&e0_room);
        apply_manu_phases(&mut e0_ctx, &table);
        let e0 = e0_ctx
            .operators
            .iter()
            .find(|o| o.name == "溯光星源")
            .unwrap();
        assert_eq!(e0.limit.all, 0);
        assert_eq!(e0_ctx.storage_limit(RecipeKind::Gold), 20);

        let e2_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("溯光星源", 2)],
        );
        let mut e2_ctx = ManuContext::from_room(&e2_room);
        apply_manu_phases(&mut e2_ctx, &table);
        let e2 = e2_ctx
            .operators
            .iter()
            .find(|o| o.name == "溯光星源")
            .unwrap();
        // 自身莱茵科技·β 计 1 次 → +5 仓库
        assert_eq!(e2.limit.all, 5);
        assert_eq!(e2_ctx.storage_limit(RecipeKind::Gold), 25);

        let paired_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("溯光星源", 2),
                op("赫默", 0, vec!["manu_prod_spd[001]"]),
            ],
        );
        let mut paired_ctx = ManuContext::from_room(&paired_room);
        apply_manu_phases(&mut paired_ctx, &table);
        let stella = paired_ctx
            .operators
            .iter()
            .find(|o| o.name == "溯光星源")
            .unwrap();
        assert_eq!(stella.limit.all, 10);
        assert_eq!(paired_ctx.storage_limit(RecipeKind::Gold), 30);
    }

    #[test]
    fn veyar_craftsman_scales_with_training_room_level() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.training_room_level = 2;
        let mut room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("维伊", 2)],
        );
        room.layout = Arc::new(layout.clone());
        let r2 = solve_manufacture(&room, &table).unwrap();
        assert!(
            (r2.prod_skill - 20.0).abs() < 0.01,
            "lv2 got {}",
            r2.prod_skill
        );

        layout.training_room_level = 3;
        room.layout = Arc::new(layout);
        let r3 = solve_manufacture(&room, &table).unwrap();
        assert!(
            (r3.prod_skill - 30.0).abs() < 0.01,
            "lv3 got {}",
            r3.prod_skill
        );
    }

    #[test]
    fn highmore_compat_extends_standard_skill_count_for_mizuki() {
        let table = table();
        let base_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("水月", 2),
                op("赫默", 0, vec!["manu_prod_spd[001]"]),
            ],
        );
        let mut base_ctx = ManuContext::from_room(&base_room);
        apply_manu_phases(&mut base_ctx, &table);
        let mizuki_base = base_ctx
            .operators
            .iter()
            .find(|o| o.name == "水月")
            .unwrap();
        assert!(
            (mizuki_base.total_eff(RecipeKind::Gold) - 30.0).abs() < 0.01,
            "without 海沫 got {}",
            mizuki_base.total_eff(RecipeKind::Gold)
        );

        let compat_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                manu_op_from_instances("水月", 2),
                manu_op_from_instances("海沫", 0),
                op("赫默", 0, vec!["manu_prod_spd[001]"]),
            ],
        );
        let mut compat_ctx = ManuContext::from_room(&compat_room);
        apply_manu_phases(&mut compat_ctx, &table);
        let mizuki_compat = compat_ctx
            .operators
            .iter()
            .find(|o| o.name == "水月")
            .unwrap();
        // 标准化·β 25% + 2 个标准化类等效（自身 β + 赫默莱茵·α）×5%
        assert!(
            (mizuki_compat.total_eff(RecipeKind::Gold) - 35.0).abs() < 0.01,
            "with 海沫 got {}",
            mizuki_compat.total_eff(RecipeKind::Gold)
        );
    }

    #[test]
    fn highmore_e2_has_standard_beta_flat() {
        let table = table();
        let result = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("海沫", 2)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (result.prod_skill - 25.0).abs() < 0.01,
            "got {}",
            result.prod_skill
        );
    }

    fn room_with_human_fireworks(amount: f64) -> ManuRoomInput {
        let mut layout = LayoutContext::default();
        layout.global.set(
            crate::global_resource::GlobalResourceKey::HumanFireworks,
            amount,
        );
        let mut room = ManuRoomInput::with_operators(3, RecipeKind::Gold, vec![]);
        room.layout = Arc::new(layout);
        room
    }

    #[test]
    fn sui_harvest_scales_with_human_fireworks() {
        let table = table();
        let mut room = room_with_human_fireworks(15.0);
        room.operators = vec![manu_op_from_instances("黍", 2)];
        let result = solve_manufacture(&room, &table).unwrap();
        assert!(
            (result.prod_skill - 5.0).abs() < 0.01,
            "15 fireworks / 3 = 5%, got {}",
            result.prod_skill
        );
    }

    #[test]
    fn jieyun_water_grass_collapsed_from_human_fireworks() {
        let table = table();
        let mut e0 = room_with_human_fireworks(10.0);
        e0.operators = vec![manu_op_from_instances("截云", 0)];
        let r0 = solve_manufacture(&e0, &table).unwrap();
        assert!(
            (r0.prod_skill - 2.0).abs() < 0.01,
            "e0 10/5 = 2%, got {}",
            r0.prod_skill
        );

        let mut e2 = room_with_human_fireworks(10.0);
        e2.operators = vec![manu_op_from_instances("截云", 2)];
        let r2 = solve_manufacture(&e2, &table).unwrap();
        assert!(
            (r2.prod_skill - 4.0).abs() < 0.01,
            "e2 10/5*2 = 4%, got {}",
            r2.prod_skill
        );
    }

    #[test]
    fn rosmontis_thought_chain_from_dorm_occupants() {
        let table = table();
        let e0 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("迷迭香", 0)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (e0.prod_skill - 10.0).abs() < 0.01,
            "e0 20 rings / 2 = 10%, got {}",
            e0.prod_skill
        );

        let e2 = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("迷迭香", 2)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (e2.prod_skill - 20.0).abs() < 0.01,
            "e2 20 rings / 1 = 20%, got {}",
            e2.prod_skill
        );

        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let buffs = instances.resolve_manufacture_buff_ids("迷迭香", PromotionTier::TierUp);
        assert!(
            buffs.contains(&"manu_prod_spd_bd[010]".to_string())
                && !buffs.contains(&"manu_prod_spd_bd[000]".to_string()),
            "e2 should replace 念力 with 意识实体, got {:?}",
            buffs
        );
    }

    #[test]
    fn leadfoot_blurred_vision_flat_at_full_mood() {
        let table = table();
        let result = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("铅踝", 0)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (result.prod_skill - 30.0).abs() < 0.01,
            "full mood baseline +30%, got {}",
            result.prod_skill
        );
    }

    #[test]
    fn fuse_taciturn_flat_at_zero_usaut_drink() {
        let table = table();
        let result = solve_manufacture(
            &ManuRoomInput::with_operators(
                3,
                RecipeKind::Gold,
                vec![manu_op_from_instances("导火索", 2)],
            ),
            &table,
        )
        .unwrap();
        assert!(
            (result.prod_skill - 20.0).abs() < 0.01,
            "baseline +20%, got {}",
            result.prod_skill
        );
    }

    fn control_op(name: &str, elite: u8) -> ControlOperator {
        let instances =
            OperatorInstances::load(&default_instances_path().expect("instances")).unwrap();
        let tier = PromotionTier::from_elite(elite);
        let binding = instances
            .get(name, tier)
            .and_then(|i| i.facilities.get("control"));
        let tier0 = instances
            .get(name, PromotionTier::Tier0)
            .and_then(|i| i.facilities.get("control"));
        let buff_ids = binding
            .map(|b| crate::instances::resolve_buff_ids(tier, b, tier0))
            .unwrap_or_default();
        let tags = instances
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
    fn sui_and_jieyun_benefit_from_control_human_fireworks() {
        let table = table();
        let mut layout = LayoutContext::default();
        layout.sui_facility_count = 2;
        let control_result = solve_control(
            &ControlRoomInput {
                operators: vec![control_op("令", 2), control_op("重岳", 0)],
                mood: 24.0,
                layout: layout.clone(),
            },
            &table,
        );
        layout.global = control_result.global;

        let mut sui_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("黍", 2)],
        );
        sui_room.layout = Arc::new(layout.clone());
        let sui = solve_manufacture(&sui_room, &table).unwrap();
        assert!(
            (sui.prod_skill - 8.0).abs() < 0.01,
            "25 fireworks / 3 = 8%, got {}",
            sui.prod_skill
        );

        let mut jie_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("截云", 2)],
        );
        jie_room.layout = Arc::new(layout);
        let jie = solve_manufacture(&jie_room, &table).unwrap();
        assert!(
            (jie.prod_skill - 10.0).abs() < 0.01,
            "floor(25/5)*2 = 10%, got {}",
            jie.prod_skill
        );
    }

    #[test]
    fn fuse_warehouse_scales_with_usaut_drink_from_control() {
        let table = table();
        let control_result = solve_control(
            &ControlRoomInput::with_operators(vec![
                control_op("战车", 2),
                ControlOperator {
                    name: "凛冬".into(),
                    elite: 2,
                    buff_ids: vec![],
                    tags: vec!["cc.g.usaut".into()],
                },
            ]),
            &table,
        );
        let mut layout = LayoutContext::default();
        layout.global = control_result.global;

        let mut room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![manu_op_from_instances("导火索", 2)],
        );
        room.layout = Arc::new(layout);
        let mut ctx = ManuContext::from_room(&room);
        apply_manu_phases(&mut ctx, &table);
        let fuse = ctx.operators.iter().find(|o| o.name == "导火索").unwrap();
        assert!((fuse.total_eff(RecipeKind::Gold) - 20.0).abs() < 0.01);
        assert_eq!(fuse.limit.all, 2);
        assert_eq!(ctx.storage_limit(RecipeKind::Gold), 22);
    }
}
