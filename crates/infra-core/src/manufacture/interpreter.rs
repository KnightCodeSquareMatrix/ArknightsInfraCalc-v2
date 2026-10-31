use std::sync::Arc;

use smallvec::SmallVec;

use crate::eff_ramp::{manufacture_eff_ramp_at, manufacture_eff_ramp_avg, MANU_RAMP_HOURS};
use crate::global_resource::RoomState;
use crate::layout::SharedLayout;
use crate::manufacture::input::{ManuRoomInput, ManuRoomView};
use crate::skill_table::SkillTable;
use crate::types::{
    Action, CompiledAtom, Condition, EffectAtom, Phase, RecipeKind, Selector, StateKey,
};

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
pub struct ManuOperatorRuntime<'a> {
    pub name: &'a str,
    pub elite: u8,
    pub buff_ids: &'a [String],
    pub tags: &'a [String],
    pub compiled_atoms: Option<&'a [CompiledAtom]>,
    /// 干员技能直接提供的生产力；`PeerEffAbsorb` 清零他人此项。
    pub skill_eff: RecipeEff,
    /// 按基建设施数量结算的生产力（不被他人归零技能清零）。
    pub layout_eff: RecipeEff,
    pub limit: RecipeLimit,
    pub mood_drain_delta: f64,
    /// 从满心情起已消耗的心情值。
    pub mood_gap: f64,
}

impl ManuOperatorRuntime<'_> {
    pub fn total_eff(&self, recipe: RecipeKind) -> f64 {
        self.skill_eff.for_recipe(recipe) + self.layout_eff.for_recipe(recipe)
    }
}

#[derive(Debug, Clone)]
pub struct ManuContext<'a> {
    pub operators: SmallVec<[ManuOperatorRuntime<'a>; 5]>,
    /// 制造站房间级生产力加成（冬时 per-op +10% 等固定数值，非「干员技能面」）。
    pub station_eff: RecipeEff,
    pub facility_level: u8,
    pub active_recipe: RecipeKind,
    pub layout: SharedLayout,
    pub facility_base_storage: i32,
    pub mood: f64,
    /// 定时制造入口的真实进驻时间或解析平均区间。
    pub(crate) ramp_sample: ManuRampSample,
    pub state_pool: RoomState,
}

impl<'a> ManuContext<'a> {
    pub fn from_room(input: &'a ManuRoomInput) -> Self {
        Self::from_view(&input.as_view())
    }

    pub(crate) fn from_view(input: &ManuRoomView<'a>) -> Self {
        let operators = input
            .operators
            .iter()
            .map(|o| ManuOperatorRuntime {
                name: o.name,
                elite: o.elite,
                buff_ids: o.buff_ids,
                tags: o.tags,
                compiled_atoms: o.compiled_atoms,
                mood_gap: (24.0 - input.mood).clamp(0.0, 24.0),
                ..Default::default()
            })
            .collect();
        Self {
            operators,
            station_eff: RecipeEff::default(),
            facility_level: input.level,
            active_recipe: input.active_recipe,
            layout: Arc::clone(input.layout),
            facility_base_storage: facility_base_storage(input.level),
            mood: input.mood,
            ramp_sample: ManuRampSample::Average(0.0, f64::from(MANU_RAMP_HOURS)),
            state_pool: input.layout.global.to_fixed_room_state(),
        }
    }

    pub fn prod_base(&self) -> f64 {
        if self.mood <= 0.0 {
            0.0
        } else {
            self.operators.len() as f64
        }
    }

    fn effective_rhine_life_in_base(&self) -> u8 {
        let mut names: std::collections::HashSet<&str> = self
            .layout
            .base_workforce
            .iter()
            .map(String::as_str)
            .collect();
        let mut count = usize::from(self.layout.rhine_life_in_base.min(5));
        for operator in &self.operators {
            if operator
                .tags
                .iter()
                .any(|tag| tag == crate::layout::TAG_RHINE)
                && names.insert(operator.name)
            {
                count += 1;
            }
        }
        count.min(5) as u8
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
            .map(|o| (o.name.to_owned(), o.mood_drain_delta))
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum ManuRampSample {
    At(f64),
    Average(f64, f64),
}

/// 键来自输入 owner 和其排序 atom 下标，不依赖可能饱和的 seq。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManuAtomKey {
    pub owner: usize,
    pub atom: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ManuBucketTrace {
    pub key: ManuAtomKey,
    pub base: f64,
    pub step: f64,
}

pub(crate) trait ManuObserver {
    fn bucket(&mut self, key: ManuAtomKey, base: f64, step: f64);
}

pub(crate) struct NoManuObserver;

impl ManuObserver for NoManuObserver {
    #[inline(always)]
    fn bucket(&mut self, _: ManuAtomKey, _: f64, _: f64) {}
}

impl ManuObserver for Vec<ManuBucketTrace> {
    fn bucket(&mut self, key: ManuAtomKey, base: f64, step: f64) {
        self.push(ManuBucketTrace { key, base, step });
    }
}

pub fn apply_manu_phases(ctx: &mut ManuContext<'_>, table: &SkillTable) {
    apply_manu_phases_inner::<false, _>(ctx, table, &mut NoManuObserver);
}

pub(crate) fn apply_manu_phases_timed<O: ManuObserver>(
    ctx: &mut ManuContext<'_>,
    table: &SkillTable,
    observer: &mut O,
) {
    apply_manu_phases_inner::<true, _>(ctx, table, observer);
}

fn apply_manu_phases_inner<const TIMED: bool, O: ManuObserver>(
    ctx: &mut ManuContext<'_>,
    table: &SkillTable,
    observer: &mut O,
) {
    // 仅缺少预备结果的输入持有临时 Arc；已预备的空技能无需再次访问缓存。
    let missing: SmallVec<[(usize, Arc<[CompiledAtom]>); 5]> = ctx
        .operators
        .iter()
        .enumerate()
        .filter(|(_, op)| op.compiled_atoms.is_none())
        .map(|(idx, op)| (idx, table.compile_manufacture_atoms(op.buff_ids)))
        .collect();
    // 从 runtime 复制借用，应用 atom 时只改变可变数值状态。
    let sources: SmallVec<[(&str, &[CompiledAtom]); 5]> = ctx
        .operators
        .iter()
        .enumerate()
        .map(|(idx, op)| {
            let atoms = op.compiled_atoms.unwrap_or_else(|| {
                missing
                    .iter()
                    .find(|(owner_idx, _)| *owner_idx == idx)
                    .expect("unprepared manufacture atoms have a temporary owner")
                    .1
                    .as_ref()
            });
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
    loop {
        // stable sort 的同分顺序：输入 owner 顺序，再按 buff/atom 展开顺序。
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
        let (owner, atoms) = sources[owner_idx];
        let atom_idx = heads[owner_idx];
        let atom = &atoms[atom_idx].atom;
        heads[owner_idx] += 1;
        if condition_met(&atom.condition, ctx, owner) {
            apply_atom::<TIMED, _>(
                ctx,
                atom,
                owner,
                ManuAtomKey {
                    owner: owner_idx,
                    atom: atom_idx,
                },
                observer,
            );
        }
    }
    apply_tagged_control_injects(ctx);
    apply_abyssal_hunters_control(ctx);
}

fn apply_tagged_control_injects(ctx: &mut ManuContext<'_>) {
    let layout = Arc::clone(&ctx.layout);
    let rules = layout.global_inject.manu_tagged();
    for op in &mut ctx.operators {
        for rule in rules {
            if rule.recipe.is_none_or(|recipe| recipe == ctx.active_recipe)
                && op.tags.iter().any(|tag| tag == &rule.target_tag)
            {
                op.skill_eff.add(rule.recipe, rule.value);
            }
        }
    }
}

fn apply_abyssal_hunters_control(ctx: &mut ManuContext<'_>) {
    const GLADIIA_ALPHA: &str = "control_mp_aegir2[000]";
    const GLADIIA_BETA: &str = "control_mp_aegir2[010]";
    const TAG_ABYSSAL: &str = "cc.g.abyssal";

    if room_has_peer_eff_absorb(ctx) {
        return;
    }

    let room_abyssal_count = ctx
        .operators
        .iter()
        .filter(|op| op.tags.iter().any(|t| t == TAG_ABYSSAL))
        .count() as f64;
    if room_abyssal_count == 0.0 {
        return;
    }

    let (rate, cap) = if ctx
        .layout
        .global_inject
        .has_active_source_buff(GLADIIA_BETA)
    {
        (10.0, 90.0)
    } else if ctx
        .layout
        .global_inject
        .has_active_source_buff(GLADIIA_ALPHA)
    {
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

fn room_has_peer_eff_absorb(ctx: &ManuContext<'_>) -> bool {
    ctx.operators.iter().any(|op| {
        op.buff_ids.iter().any(|buff_id| {
            buff_id == "manu_prod_spd&power[000]"
                || buff_id == "manu_prod_spd&power[010]"
                || buff_id == "manu_prod_spd&power[020]"
        })
    })
}

fn condition_met(cond: &Option<Condition>, ctx: &ManuContext<'_>, owner: &str) -> bool {
    let Some(cond) = cond else { return true };
    match cond {
        Condition::ActiveRecipe { kind } => ctx.active_recipe == *kind,
        Condition::MoodAbove { n } => ctx.mood > *n as f64,
        Condition::MoodAboveOrEq { n } => ctx.mood >= *n as f64,
        Condition::MoodBelow { n } => ctx.mood < *n as f64,
        Condition::MoodBelowOrEq { n } => ctx.mood <= *n as f64,
        Condition::OwnerMoodGapAbove { n } => ctx
            .operators
            .iter()
            .find(|operator| operator.name == owner)
            .is_some_and(|operator| operator.mood_gap > f64::from(*n)),
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
        Condition::PartnerInRoom { name } => ctx.operators.iter().any(|o| o.name == name),
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

fn room_has_buff(ctx: &ManuContext<'_>, buff_id: &str) -> bool {
    ctx.operators
        .iter()
        .any(|o| o.buff_ids.iter().any(|b| b == buff_id))
}

fn apply_atom<const TIMED: bool, O: ManuObserver>(
    ctx: &mut ManuContext<'_>,
    atom: &EffectAtom,
    owner: &str,
    key: ManuAtomKey,
    observer: &mut O,
) {
    if matches!(&atom.action, Action::AddEffFromLimitContribSum { .. })
        && room_has_buff(ctx, BUFF_PAOPAO_LIMIT_VAR)
    {
        return;
    }
    match atom.phase {
        Phase::Constant | Phase::EffVar | Phase::LimitVar | Phase::OrderVar => {
            apply_eff_action::<TIMED, _>(ctx, atom, owner, key, observer);
        }
        Phase::Limit => apply_limit_action(ctx, atom, owner),
        Phase::Mood => apply_mood_action(ctx, &atom.action, owner),
        Phase::StateWrite => apply_state_write(ctx, atom, owner),
        Phase::PeerAbsorb => apply_peer_absorb(ctx, &atom.action, owner),
        Phase::GlobalInject | Phase::OrderMechanic | Phase::PeerShare => {}
    }
}

/// 按基建设施数量结算的生产力（冬时/森蚺归零时不计入「其他干员技能面」）。
fn eff_target_is_layout(atom: &EffectAtom) -> bool {
    matches!(
        atom.selector.as_ref(),
        Some(Selector::TradeStationCount)
            | Some(Selector::PowerStationCount)
            | Some(Selector::PlatformCountInPower)
    ) || matches!(
        &atom.action,
        Action::StateConsumeToEff { key, .. }
            if StateKey::parse(key) == Some(StateKey::EngineeringRobot)
    )
}

fn apply_peer_absorb(ctx: &mut ManuContext<'_>, action: &Action, owner: &str) {
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

fn apply_eff_action<const TIMED: bool, O: ManuObserver>(
    ctx: &mut ManuContext<'_>,
    atom: &EffectAtom,
    owner: &str,
    key: ManuAtomKey,
    observer: &mut O,
) {
    let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) else {
        return;
    };
    let value = resolve_eff_value::<TIMED, _>(ctx, atom, owner, key, observer);
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

fn apply_limit_action(ctx: &mut ManuContext<'_>, atom: &EffectAtom, owner: &str) {
    let Some(idx) = ctx.operators.iter().position(|o| o.name == owner) else {
        return;
    };
    match &atom.action {
        Action::AddLimitDelta { delta, recipe } => {
            ctx.operators[idx].limit.add(*recipe, *delta);
        }
        Action::AddLimitFromSelector { multiplier } => {
            let base = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
            let add = (base * multiplier).round() as i32;
            ctx.operators[idx].limit.add(None, add);
        }
        Action::AddLimitFromState { key, multiplier } => {
            let Some(sk) = StateKey::parse(key) else {
                return;
            };
            let state = ctx.state_pool[sk];
            let add = (state.floor() * multiplier).round() as i32;
            ctx.operators[idx].limit.add(None, add);
        }
        _ => {}
    }
}

fn apply_mood_action(ctx: &mut ManuContext<'_>, action: &Action, owner: &str) {
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
            let state = ctx.state_pool[sk];
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

fn apply_state_write(ctx: &mut ManuContext<'_>, atom: &EffectAtom, owner: &str) {
    match &atom.action {
        Action::StateProduce { key, amount } => {
            if let Some(sk) = StateKey::parse(key) {
                let scale = resolve_selector_value(ctx, atom.selector.as_ref(), owner);
                let add = if atom.selector.is_some() {
                    scale * amount
                } else {
                    *amount
                };
                ctx.state_pool[sk] += add;
            }
        }
        Action::StateConvert { from, to, ratio } => {
            let (Some(from_k), Some(to_k)) = (StateKey::parse(from), StateKey::parse(to)) else {
                return;
            };
            let src = ctx.state_pool[from_k];
            ctx.state_pool[to_k] += src * ratio;
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

fn resolve_eff_value<const TIMED: bool, O: ManuObserver>(
    ctx: &ManuContext<'_>,
    atom: &EffectAtom,
    owner: &str,
    key: ManuAtomKey,
    observer: &mut O,
) -> f64 {
    match &atom.action {
        Action::AddEffFromLimitContribSum { rate } => {
            ctx.operators
                .iter()
                .map(|o| o.limit.for_recipe(ctx.active_recipe).max(0))
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
                eff_from_limit_contrib_tiered(
                    o.limit.for_recipe(ctx.active_recipe),
                    *threshold,
                    *low_rate,
                    *high_rate,
                )
            })
            .sum(),
        Action::AddEffRamp {
            initial,
            per_hour,
            cap,
            style,
        } => {
            let sample = if TIMED {
                ctx.ramp_sample
            } else {
                ManuRampSample::Average(0.0, f64::from(MANU_RAMP_HOURS))
            };
            match sample {
                ManuRampSample::At(elapsed) => {
                    manufacture_eff_ramp_at(*style, *initial, *per_hour, *cap, elapsed)
                }
                ManuRampSample::Average(start, end) => {
                    manufacture_eff_ramp_avg(*style, *initial, *per_hour, *cap, start, end)
                }
            }
        }
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
            observer.bucket(key, base, *step);
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
            let state = ctx.state_pool[sk];
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
        "manu_formula_spd[100]"
            | "manu_formula_spd[101]"
            | "manu_formula_spd[102]"
            | "manu_formula_spd[110]"
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

fn manu_skill_family_count_in_room(ctx: &ManuContext<'_>, pred: fn(&str) -> bool) -> f64 {
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

fn room_has_highmore_compat(ctx: &ManuContext<'_>) -> bool {
    ctx.operators
        .iter()
        .flat_map(|o| o.buff_ids.iter())
        .any(|b| b == BUFF_HIGHMORE_COMPAT)
}

fn standard_skill_count_in_room(ctx: &ManuContext<'_>) -> f64 {
    let mut count = manu_skill_family_count_in_room(ctx, is_standard_skill_buff);
    if room_has_highmore_compat(ctx) {
        count += manu_skill_family_count_in_room(ctx, is_rhine_skill_buff);
        count += manu_skill_family_count_in_room(ctx, is_pinecone_skill_buff);
    }
    count
}

fn metal_formula_skill_count_in_room(ctx: &ManuContext<'_>) -> f64 {
    manu_skill_family_count_in_room(ctx, is_metal_formula_buff)
}

fn resolve_selector_value(ctx: &ManuContext<'_>, selector: Option<&Selector>, owner: &str) -> f64 {
    match selector {
        Some(Selector::FacilityLevel) => f64::from(ctx.facility_level),
        Some(Selector::FacilityLevelMinusOne) => f64::from(ctx.facility_level.saturating_sub(1)),
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
        Some(Selector::LimitContribSum) => ctx
            .operators
            .iter()
            .map(|o| o.limit.for_recipe(ctx.active_recipe))
            .sum::<i32>() as f64,
        Some(Selector::PeerSkillEffSum) => ctx
            .operators
            .iter()
            .filter(|o| o.name != owner)
            .map(|o| o.skill_eff.for_recipe(ctx.active_recipe))
            .sum(),
        Some(Selector::Mood) => ctx.mood,
        Some(Selector::OwnerMoodGap) => ctx
            .operators
            .iter()
            .find(|operator| operator.name == owner)
            .map_or(0.0, |operator| operator.mood_gap),
        Some(Selector::DroneCap) => ctx.layout.drone_cap as f64,
        Some(Selector::RhineLifeInBase) => f64::from(ctx.effective_rhine_life_in_base()),
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
