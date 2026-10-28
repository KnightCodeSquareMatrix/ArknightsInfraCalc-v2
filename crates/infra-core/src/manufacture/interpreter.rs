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
    /// 从满心情起已消耗的心情值。
    pub mood_gap: f64,
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
                mood_gap: (24.0 - input.mood).clamp(0.0, 24.0),
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
                && names.insert(operator.name.as_str())
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
) -> Vec<(&'a EffectAtom, usize)> {
    let mut atoms = Vec::new();
    for (owner_index, op) in ops.iter().enumerate() {
        for bid in &op.buff_ids {
            let Some(skill) = table.get(bid) else {
                continue;
            };
            if skill.facility != "manufacture" {
                continue;
            }
            for atom in &skill.atoms {
                atoms.push((atom, owner_index));
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
    let atoms = collect_manu_atoms(&ctx.operators, table);

    // 干员名在整个循环内不变（apply_* 只改效率/仓库/心情字段），每名干员只克隆一次，
    // 不再对每个 atom 克隆一次。
    let owner_names: Vec<String> = ctx.operators.iter().map(|o| o.name.clone()).collect();
    for (atom, owner_index) in atoms {
        let owner = owner_names[owner_index].as_str();
        if !condition_met(&atom.condition, ctx, owner) {
            continue;
        }
        apply_atom(ctx, atom, owner);
    }
    apply_tagged_control_injects(ctx);
    apply_abyssal_hunters_control(ctx);
}

fn apply_tagged_control_injects(ctx: &mut ManuContext) {
    let rules = ctx.layout.global_inject.manu_tagged().to_vec();
    for op in &mut ctx.operators {
        for rule in &rules {
            if rule.recipe.is_none_or(|recipe| recipe == ctx.active_recipe)
                && op.tags.iter().any(|tag| tag == &rule.target_tag)
            {
                op.skill_eff.add(rule.recipe, rule.value);
            }
        }
    }
}

fn apply_abyssal_hunters_control(ctx: &mut ManuContext) {
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

fn room_has_peer_eff_absorb(ctx: &ManuContext) -> bool {
    ctx.operators.iter().any(|op| {
        op.buff_ids.iter().any(|buff_id| {
            buff_id == "manu_prod_spd&power[000]"
                || buff_id == "manu_prod_spd&power[010]"
                || buff_id == "manu_prod_spd&power[020]"
        })
    })
}

fn condition_met(cond: &Option<Condition>, ctx: &ManuContext, owner: &str) -> bool {
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

fn apply_atom(ctx: &mut ManuContext, atom: &EffectAtom, owner: &str) {
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
