use serde::Serialize;
use smallvec::SmallVec;
use std::sync::Arc;

use crate::eff_ramp::{manufacture_eff_ramp_cap_time, EffRampStyle, MANU_RAMP_HOURS};
use crate::efficiency::Efficiency;
use crate::error::{Error, Result};
use crate::layout::FacilityKind;
use crate::manufacture::input::{ManuLineScenario, ManuOperatorRef, ManuRoomInput, ManuRoomView};
use crate::manufacture::interpreter::{
    apply_manu_phases, apply_manu_phases_timed, ManuAtomKey, ManuBucketTrace, ManuContext,
    ManuObserver, ManuRampSample, NoManuObserver,
};
use crate::mood::{default_mood_model, operator_net_drain, DrainInputs};
use crate::skill_table::SkillTable;
use crate::types::{Action, CompiledAtom, Condition, RecipeKind, Selector};

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuProdBreakdown {
    pub gold: Efficiency,
    pub battle_record: Efficiency,
    pub originium: Efficiency,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuLineEfficiency {
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub per_station: ManuProdBreakdown,
    pub storage: ManuStorageBreakdown,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuStorageBreakdown {
    pub gold: i32,
    pub battle_record: i32,
    pub originium: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorMoodDrain {
    pub name: String,
    pub drain_delta_per_hour: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuResult {
    pub active_recipe: RecipeKind,
    pub base_efficiency: Efficiency,
    pub occupancy_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub global_efficiency: Efficiency,
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub unit_output_per_day: f64,
    pub output_per_day: f64,
    pub storage_limit: i32,
    pub mood_drain: Vec<OperatorMoodDrain>,
}

pub fn solve_manufacture(input: &ManuRoomInput, table: &SkillTable) -> Result<ManuResult> {
    solve_manufacture_view(&input.as_view(), table)
}

pub(crate) fn solve_manufacture_view(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
) -> Result<ManuResult> {
    solve_manufacture_time_view(input, table, None)
}

pub fn solve_manufacture_with_shift(
    input: &ManuRoomInput,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<ManuResult> {
    solve_manufacture_with_shift_view(&input.as_view(), table, shift_hours)
}

pub(crate) fn solve_manufacture_with_shift_view(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<ManuResult> {
    solve_manufacture_time_view(input, table, Some(shift_hours))
}

/// 无暖机保持原心情分段与量化行为；其 atom 热路径关闭定时观察。
fn solve_manufacture_without_ramp_with_shift(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: f64,
) -> Result<ManuResult> {
    if shift_hours <= 0.0 {
        return solve_manufacture_at_gaps(input, table, &[]);
    }

    let model = default_mood_model()?;
    let start_gap = (model.mood_cap - input.mood).clamp(0.0, model.mood_cap);
    let room_ops: Vec<_> = input
        .operators
        .iter()
        .map(|operator| (operator.name.to_owned(), operator.elite))
        .collect();
    let control_ops = &input.layout.control_operators;
    let full_control = control_ops.len().max(input.layout.control_workforce.len()) >= 5;
    let recipe = match input.active_recipe {
        RecipeKind::Gold => Some("gold"),
        RecipeKind::BattleRecord => Some("battle_record"),
        RecipeKind::Originium => Some("originium"),
        RecipeKind::All => None,
    };
    let drains: Vec<_> = input
        .operators
        .iter()
        .map(|operator| {
            let drain = operator_net_drain(
                model,
                &DrainInputs {
                    name: operator.name,
                    elite: operator.elite,
                    facility: FacilityKind::Factory,
                    room_ops: &room_ops,
                    full_control,
                    control_ops,
                    recipe,
                },
            );
            (operator.name, drain)
        })
        .collect();

    let mut boundaries = vec![0.0, shift_hours];
    for (operator, (_, drain)) in input.operators.iter().zip(&drains) {
        if *drain <= 0.0 {
            continue;
        }
        for threshold in operator_mood_gap_thresholds(operator, table, model.mood_cap) {
            let elapsed = (threshold - start_gap) / drain;
            if elapsed > 0.0 && elapsed < shift_hours {
                boundaries.push(elapsed);
            }
        }
    }
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup_by(|left, right| (*left - *right).abs() < 1e-9);

    let mut base = 0.0;
    let mut occupancy = 0.0;
    let mut skill = 0.0;
    let mut global = 0.0;
    let mut output = 0.0;
    let mut mood_drain = Vec::new();
    for window in boundaries.windows(2) {
        let duration = window[1] - window[0];
        if duration <= 0.0 {
            continue;
        }
        let elapsed = (window[0] + window[1]) / 2.0;
        let gaps: Vec<_> = drains
            .iter()
            .map(|(name, drain)| {
                (
                    *name,
                    (start_gap + drain * elapsed).clamp(0.0, model.mood_cap),
                )
            })
            .collect();
        let result = solve_manufacture_at_gaps(input, table, &gaps)?;
        base += result.base_efficiency.as_f64() * duration;
        occupancy += result.occupancy_efficiency.as_f64() * duration;
        skill += result.skill_efficiency.as_f64() * duration;
        global += result.global_efficiency.as_f64() * duration;
        output += result.output_per_day * duration;
        mood_drain = result.mood_drain;
    }

    let end_gaps: Vec<_> = drains
        .iter()
        .map(|(name, drain)| {
            (
                *name,
                (start_gap + drain * shift_hours).clamp(0.0, model.mood_cap),
            )
        })
        .collect();
    let end = solve_manufacture_at_gaps(input, table, &end_gaps)?;
    let base_efficiency = Efficiency::from_decimal(base / shift_hours);
    let occupancy_efficiency = Efficiency::from_decimal(occupancy / shift_hours);
    let skill_efficiency = Efficiency::from_decimal(skill / shift_hours);
    let global_efficiency = Efficiency::from_decimal(global / shift_hours);
    Ok(ManuResult {
        active_recipe: input.active_recipe,
        base_efficiency,
        occupancy_efficiency,
        skill_efficiency,
        global_efficiency,
        final_efficiency: base_efficiency + skill_efficiency + global_efficiency,
        unit_output_per_day: unit_output_per_day(input.active_recipe),
        output_per_day: output / shift_hours,
        storage_limit: end.storage_limit,
        mood_drain,
    })
}

fn solve_manufacture_at_gaps(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    mood_gaps: &[(&str, f64)],
) -> Result<ManuResult> {
    let mut ctx = ManuContext::from_view(input);
    for operator in &mut ctx.operators {
        if let Some((_, gap)) = mood_gaps.iter().find(|(name, _)| *name == operator.name) {
            operator.mood_gap = *gap;
        }
    }
    apply_manu_phases(&mut ctx, table);

    let recipe = input.active_recipe;
    let occupancy_efficiency = Efficiency::from_percent_points(ctx.prod_base());
    let skill_efficiency = Efficiency::from_percent_points(ctx.prod_skill(recipe));
    let global_efficiency = Efficiency::from_percent_points(
        ctx.prod_total(recipe) - ctx.prod_base() - ctx.prod_skill(recipe),
    );
    let unit_output_per_day = unit_output_per_day(recipe);
    let final_efficiency =
        Efficiency::ONE + occupancy_efficiency + skill_efficiency + global_efficiency;
    Ok(ManuResult {
        active_recipe: recipe,
        base_efficiency: Efficiency::ONE + occupancy_efficiency,
        occupancy_efficiency,
        skill_efficiency,
        global_efficiency,
        final_efficiency,
        unit_output_per_day,
        output_per_day: unit_output_per_day * final_efficiency.as_f64(),
        storage_limit: ctx.storage_limit(recipe),
        mood_drain: ctx
            .mood_drain_summary()
            .into_iter()
            .map(|(name, drain_delta_per_hour)| OperatorMoodDrain {
                name,
                drain_delta_per_hour,
            })
            .collect(),
    })
}

pub fn unit_output_per_day(recipe: RecipeKind) -> f64 {
    match recipe {
        RecipeKind::Gold => 10_000.0,
        RecipeKind::BattleRecord => 8_000.0,
        RecipeKind::Originium => 24.0,
        RecipeKind::All => 0.0,
    }
}

fn operator_mood_gap_thresholds(
    operator: &ManuOperatorRef<'_>,
    table: &SkillTable,
    mood_cap: f64,
) -> Vec<f64> {
    let mut thresholds = Vec::new();
    let missing;
    let atoms = match operator.compiled_atoms {
        Some(atoms) => atoms,
        None => {
            missing = table.compile_manufacture_atoms(operator.buff_ids);
            missing.as_ref()
        }
    };
    for compiled in atoms {
        let atom = &compiled.atom;
        if let Some(Condition::OwnerMoodGapAbove { n }) = &atom.condition {
            thresholds.push(f64::from(*n));
        }
        if atom.selector == Some(Selector::OwnerMoodGap) {
            if let Action::AddBucketEffFromSelector { step, .. } = &atom.action {
                if *step > 0.0 {
                    let mut threshold = *step;
                    while threshold <= mood_cap {
                        thresholds.push(threshold);
                        threshold += *step;
                    }
                }
            }
        }
    }
    thresholds
}

#[derive(Clone, Copy)]
struct ManuRamp {
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
}

#[derive(Default)]
struct ManuTimeEffects {
    ramps: SmallVec<[ManuRamp; 5]>,
    /// 已排序的执行键；后一个消费者只在前一个已细分的区间拟合。
    peer_buckets: SmallVec<[ManuAtomKey; 5]>,
    mood_gap: bool,
    unsupported: bool,
}

impl ManuTimeEffects {
    fn has_ramp(&self) -> bool {
        !self.ramps.is_empty()
    }

    fn from_prepared(input: &ManuRoomView<'_>) -> Self {
        let mut effects = Self::default();
        let mut consumers: SmallVec<[((_, _, _, _), ManuAtomKey); 5]> = SmallVec::new();
        for (owner, operator) in input.operators.iter().enumerate() {
            for (atom_idx, compiled) in operator.compiled_atoms.unwrap_or(&[]).iter().enumerate() {
                let atom = &compiled.atom;
                effects.mood_gap |= atom.selector == Some(Selector::OwnerMoodGap)
                    || matches!(&atom.condition, Some(Condition::OwnerMoodGapAbove { .. }));
                if let Action::AddEffRamp {
                    style,
                    initial,
                    per_hour,
                    cap,
                } = &atom.action
                {
                    effects.ramps.push(ManuRamp {
                        style: *style,
                        initial: *initial,
                        per_hour: *per_hour,
                        cap: *cap,
                    });
                    effects.unsupported |=
                        !initial.is_finite() || !per_hour.is_finite() || !cap.is_finite();
                }
                if matches!(
                    atom.selector.as_ref(),
                    Some(
                        Selector::OtherOpsDirectEff
                            | Selector::OtherOpsTotalEff
                            | Selector::PeerSettledEffSum
                            | Selector::OtherOpsSettledEff
                    )
                ) {
                    effects.unsupported = true;
                }
                if atom.selector == Some(Selector::PeerSkillEffSum) {
                    match &atom.action {
                        Action::AddBucketEffFromSelector {
                            step,
                            ret_per_step,
                            cap,
                        } => {
                            effects.unsupported |=
                                !step.is_finite() || !ret_per_step.is_finite() || !cap.is_finite();
                            consumers.push((
                                (compiled.sort_key, owner, compiled.seq, atom_idx),
                                ManuAtomKey {
                                    owner,
                                    atom: atom_idx,
                                },
                            ));
                        }
                        Action::AddFlatEffFromSelector { cap: None, .. } => {}
                        _ => effects.unsupported = true,
                    }
                }
            }
        }
        consumers.sort_by_key(|(rank, _)| *rank);
        effects.peer_buckets = consumers.into_iter().map(|(_, key)| key).collect();
        effects
    }
}

/// 只为未准备的 owner 取一次 cache；所有后续采样复用同一借用视图。
fn with_manufacture_time<T>(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: Option<f64>,
    solve: impl FnOnce(&ManuRoomView<'_>, &ManuTimeEffects) -> Result<T>,
) -> Result<T> {
    if shift_hours.is_some_and(|hours| !hours.is_finite()) {
        return Err(Error::msg("manufacture shift duration must be finite"));
    }
    let missing: SmallVec<[(usize, Arc<[CompiledAtom]>); 5]> = input
        .operators
        .iter()
        .enumerate()
        .filter(|(_, operator)| operator.compiled_atoms.is_none())
        .map(|(idx, operator)| (idx, table.compile_manufacture_atoms(operator.buff_ids)))
        .collect();
    let prepared = ManuRoomView {
        level: input.level,
        operators: input
            .operators
            .iter()
            .enumerate()
            .map(|(idx, operator)| {
                let compiled_atoms = operator.compiled_atoms.or_else(|| {
                    missing
                        .iter()
                        .find(|(owner, _)| *owner == idx)
                        .map(|(_, atoms)| atoms.as_ref())
                });
                ManuOperatorRef {
                    compiled_atoms,
                    ..*operator
                }
            })
            .collect(),
        active_recipe: input.active_recipe,
        mood: input.mood,
        layout: input.layout,
    };
    let effects = ManuTimeEffects::from_prepared(&prepared);
    if effects.has_ramp() && effects.unsupported {
        return Err(Error::msg(
            "unsupported manufacture warm-up consumer or non-finite ramp parameters",
        ));
    }
    solve(&prepared, &effects)
}

fn solve_manufacture_time_view(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: Option<f64>,
) -> Result<ManuResult> {
    with_manufacture_time(input, table, shift_hours, |input, effects| {
        if effects.has_ramp() {
            return Ok(
                integrate_manufacture_ramp::<true>(input, table, shift_hours, effects)?
                    .into_result(input.active_recipe),
            );
        }
        if let Some(hours) = shift_hours {
            if effects.mood_gap && hours > 0.0 {
                return solve_manufacture_without_ramp_with_shift(input, table, hours);
            }
        }
        solve_manufacture_at_gaps(input, table, &[])
    })
}

struct ManuMoodTimeline<'a> {
    start_gap: f64,
    cap: f64,
    drains: Vec<(&'a str, f64)>,
}

impl<'a> ManuMoodTimeline<'a> {
    fn at(&self, elapsed: f64) -> impl Iterator<Item = (&'a str, f64)> + '_ {
        self.drains.iter().map(move |(name, drain)| {
            (
                *name,
                (self.start_gap + drain * elapsed).clamp(0.0, self.cap),
            )
        })
    }
}

fn warm_mood_timeline<'a>(
    input: &ManuRoomView<'a>,
    table: &SkillTable,
    hours: f64,
    boundaries: &mut Vec<f64>,
) -> Result<ManuMoodTimeline<'a>> {
    let model = default_mood_model()?;
    let start_gap = (model.mood_cap - input.mood).clamp(0.0, model.mood_cap);
    let room_ops: Vec<_> = input
        .operators
        .iter()
        .map(|operator| (operator.name.to_owned(), operator.elite))
        .collect();
    let control_ops = &input.layout.control_operators;
    let full_control = control_ops.len().max(input.layout.control_workforce.len()) >= 5;
    let recipe = match input.active_recipe {
        RecipeKind::Gold => Some("gold"),
        RecipeKind::BattleRecord => Some("battle_record"),
        RecipeKind::Originium => Some("originium"),
        RecipeKind::All => None,
    };
    let mut drains = Vec::new();
    for operator in &input.operators {
        let drain = operator_net_drain(
            model,
            &DrainInputs {
                name: operator.name,
                elite: operator.elite,
                facility: FacilityKind::Factory,
                room_ops: &room_ops,
                full_control,
                control_ops,
                recipe,
            },
        );
        if !drain.is_finite() {
            return Err(Error::msg("manufacture mood drain must be finite"));
        }
        if drain != 0.0 {
            let mut thresholds = operator_mood_gap_thresholds(operator, table, model.mood_cap);
            thresholds.extend([0.0, model.mood_cap]);
            for threshold in thresholds {
                insert_inside(boundaries, (threshold - start_gap) / drain, 0.0, hours);
            }
        }
        drains.push((operator.name, drain));
    }
    Ok(ManuMoodTimeline {
        start_gap,
        cap: model.mood_cap,
        drains,
    })
}

fn insert_inside(boundaries: &mut Vec<f64>, time: f64, start: f64, end: f64) {
    if time.is_finite() && time > start && time < end {
        boundaries.push(time);
    }
}

fn sort_boundaries(boundaries: &mut Vec<f64>) {
    boundaries.sort_by(f64::total_cmp);
    // 仅合并相同的可表示边界，不能用epsilon吞掉真实短段。
    boundaries.dedup();
}

fn timed_manufacture_context<'a, O: ManuObserver>(
    input: &ManuRoomView<'a>,
    table: &SkillTable,
    timeline: Option<&ManuMoodTimeline<'_>>,
    elapsed: f64,
    sample: ManuRampSample,
    observer: &mut O,
) -> ManuContext<'a> {
    let mut ctx = ManuContext::from_view(input);
    ctx.ramp_sample = sample;
    if let Some(timeline) = timeline {
        for (operator, (_, gap)) in ctx.operators.iter_mut().zip(timeline.at(elapsed)) {
            operator.mood_gap = gap;
        }
    }
    apply_manu_phases_timed(&mut ctx, table, observer);
    ctx
}

fn trace_at(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    timeline: Option<&ManuMoodTimeline<'_>>,
    time: f64,
) -> Vec<ManuBucketTrace> {
    let mut trace = Vec::new();
    timed_manufacture_context(
        input,
        table,
        timeline,
        time,
        ManuRampSample::At(time),
        &mut trace,
    );
    trace
}

/// 优先内点；四分位舍入落到端点时，改用相邻可表示内点。
/// 不存在两个不同内点的段仍有真实时长，不能报错或epsilon合并。
fn warm_interior_pair(start: f64, end: f64) -> Option<(f64, f64)> {
    let first = start.next_up();
    let last = end.next_down();
    if !(start < first && first < last && last < end) {
        return None;
    }
    let span = end - start;
    let left = (start + span * 0.25).max(first);
    let right = (start + span * 0.75).min(last);
    if left < right {
        Some((left, right))
    } else {
        Some((first, last))
    }
}

/// 在可表示时间分辨率内选区间内点；若无内点，保留最近的中点端点。
fn warm_interval_sample(start: f64, end: f64) -> f64 {
    let midpoint = start + (end - start) / 2.0;
    let first = start.next_up();
    let last = end.next_down();
    if first <= last && first < end {
        midpoint.clamp(first, last)
    } else {
        midpoint
    }
}

/// 此处只解 selector=k*step 的时间；floor、ret与cap仍唯一位于原解释器。
fn refine_peer_buckets(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    timeline: Option<&ManuMoodTimeline<'_>>,
    keys: &[ManuAtomKey],
    boundaries: &mut Vec<f64>,
) -> Result<()> {
    for key in keys {
        let mut refined = boundaries.clone();
        for window in boundaries.windows(2) {
            let (start, end) = (window[0], window[1]);
            let span = end - start;
            let Some((left_time, right_time)) = warm_interior_pair(start, end) else {
                // 此段至多一个可表示内点；保持其权重，由最终求值结算，
                // 不因舍入形成的端点伪根拒绝组合，也不把该段并入相邻段。
                continue;
            };
            let left_trace = trace_at(input, table, timeline, left_time);
            let right_trace = trace_at(input, table, timeline, right_time);
            let left = left_trace.iter().find(|trace| trace.key == *key);
            let right = right_trace.iter().find(|trace| trace.key == *key);
            let (left, right) = match (left, right) {
                (None, None) => continue,
                (Some(left), Some(right)) if left.step == right.step => (left, right),
                _ => {
                    return Err(Error::msg(
                        "unstable manufacture warm-up consumer execution key",
                    ))
                }
            };
            if left.step <= 0.0 || left.base == right.base {
                continue;
            }
            let slope = (right.base - left.base) / (right_time - left_time);
            let at_start = slope.mul_add(start - left_time, left.base);
            let at_end = slope.mul_add(end - right_time, right.base);
            if !slope.is_finite() || !at_start.is_finite() || !at_end.is_finite() {
                return Err(Error::msg(
                    "manufacture warm-up selector affine arithmetic is not finite",
                ));
            }
            if slope == 0.0 {
                continue;
            }
            let low = at_start.min(at_end) / left.step;
            let high = at_start.max(at_end) / left.step;
            let first = low.floor();
            let last = high.ceil();
            // 防止异常表参数导致不可表示整数或无界阈值枚举；绝不回平均fallback。
            if !first.is_finite()
                || !last.is_finite()
                || first.abs() >= 9_007_199_254_740_992.0
                || last.abs() >= 9_007_199_254_740_992.0
                || last - first > 1_000_000.0
            {
                return Err(Error::msg(
                    "unsupported manufacture warm-up selector threshold range",
                ));
            }
            for bucket in (first as i64)..=(last as i64) {
                let target = bucket as f64 * left.step;
                let fraction = (target - at_start) / (at_end - at_start);
                if fraction > 0.0 && fraction < 1.0 {
                    // 靠近哪侧就从哪侧插值，减少根靠近端点时的消去。
                    let root = if fraction <= 0.5 {
                        fraction.mul_add(span, start)
                    } else {
                        (-(1.0 - fraction)).mul_add(span, end)
                    };
                    insert_inside(&mut refined, root, start, end);
                }
            }
        }
        sort_boundaries(&mut refined);
        *boundaries = refined;
    }
    Ok(())
}

/// 暖机积分的数值摘要；轻量调用不物化完整结果或心情姓名。
struct ManuWarmSummary {
    occupancy_efficiency: Efficiency,
    skill_efficiency: Efficiency,
    global_efficiency: Efficiency,
    final_efficiency: Efficiency,
    storage_limit: i32,
    mood_drain: Vec<OperatorMoodDrain>,
}

impl ManuWarmSummary {
    fn into_result(self, recipe: RecipeKind) -> ManuResult {
        let unit_output_per_day = unit_output_per_day(recipe);
        ManuResult {
            active_recipe: recipe,
            base_efficiency: Efficiency::ONE + self.occupancy_efficiency,
            occupancy_efficiency: self.occupancy_efficiency,
            skill_efficiency: self.skill_efficiency,
            global_efficiency: self.global_efficiency,
            final_efficiency: self.final_efficiency,
            unit_output_per_day,
            output_per_day: unit_output_per_day * self.final_efficiency.as_f64(),
            storage_limit: self.storage_limit,
            mood_drain: self.mood_drain,
        }
    }
}

fn summarize_warm_averages(
    occupancy: f64,
    skill: f64,
    global: f64,
    storage_limit: i32,
    mood_drain: Vec<OperatorMoodDrain>,
) -> ManuWarmSummary {
    let occupancy_efficiency = Efficiency::from_percent_points(occupancy);
    let skill_efficiency = Efficiency::from_percent_points(skill);
    let global_efficiency = Efficiency::from_percent_points(global);
    let final_efficiency =
        Efficiency::ONE + occupancy_efficiency + skill_efficiency + global_efficiency;
    ManuWarmSummary {
        occupancy_efficiency,
        skill_efficiency,
        global_efficiency,
        final_efficiency,
        storage_limit,
        mood_drain,
    }
}

fn context_mood_drains(ctx: &ManuContext<'_>) -> Vec<OperatorMoodDrain> {
    ctx.mood_drain_summary()
        .into_iter()
        .map(|(name, drain_delta_per_hour)| OperatorMoodDrain {
            name,
            drain_delta_per_hour,
        })
        .collect()
}

/// 补偿求和降低分段权重累加误差；不对效率加epsilon或改千分舍入合同。
#[derive(Default)]
struct ManuIntegralSum {
    sum: f64,
    correction: f64,
}

impl ManuIntegralSum {
    fn add(&mut self, value: f64) {
        let combined = self.sum + value;
        self.correction += if self.sum.abs() >= value.abs() {
            (self.sum - combined) + value
        } else {
            (value - combined) + self.sum
        };
        self.sum = combined;
    }

    fn total(self) -> f64 {
        self.sum + self.correction
    }
}

/// 制造暖机积分的唯一owner；轻量/完整调用同一窗口与执行路径。
fn integrate_manufacture_ramp<const FULL: bool>(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: Option<f64>,
    effects: &ManuTimeEffects,
) -> Result<ManuWarmSummary> {
    let hours = shift_hours.unwrap_or(f64::from(MANU_RAMP_HOURS));
    if hours <= 0.0 {
        let ctx = timed_manufacture_context(
            input,
            table,
            None,
            0.0,
            ManuRampSample::At(0.0),
            &mut NoManuObserver,
        );
        let occupancy = ctx.prod_base();
        let skill = ctx.prod_skill(input.active_recipe);
        let global = ctx.prod_total(input.active_recipe) - occupancy - skill;
        let mood_drain = if FULL {
            context_mood_drains(&ctx)
        } else {
            Vec::new()
        };
        return Ok(summarize_warm_averages(
            occupancy,
            skill,
            global,
            ctx.storage_limit(input.active_recipe),
            mood_drain,
        ));
    }
    if effects.peer_buckets.is_empty() && !(shift_hours.is_some() && effects.mood_gap) {
        let ctx = timed_manufacture_context(
            input,
            table,
            None,
            hours / 2.0,
            ManuRampSample::Average(0.0, hours),
            &mut NoManuObserver,
        );
        let occupancy = ctx.prod_base();
        let skill = ctx.prod_skill(input.active_recipe);
        let global = ctx.prod_total(input.active_recipe) - occupancy - skill;
        let mood_drain = if FULL {
            context_mood_drains(&ctx)
        } else {
            Vec::new()
        };
        // 支持的暖机读写只影响效率；无心情时间线时仓库不依赖ramp采样。
        return Ok(summarize_warm_averages(
            occupancy,
            skill,
            global,
            ctx.storage_limit(input.active_recipe),
            mood_drain,
        ));
    }
    let mut boundaries = vec![0.0, hours];
    // None保留静态心情合同，但仍在默认20h窗口内积分暖机。
    let timeline = if shift_hours.is_some() && effects.mood_gap {
        Some(warm_mood_timeline(input, table, hours, &mut boundaries)?)
    } else {
        None
    };
    if !effects.peer_buckets.is_empty() {
        insert_inside(&mut boundaries, 1.0, 0.0, hours);
        for ramp in &effects.ramps {
            if let Some(time) =
                manufacture_eff_ramp_cap_time(ramp.style, ramp.initial, ramp.per_hour, ramp.cap)
            {
                insert_inside(&mut boundaries, time, 0.0, hours);
            }
        }
    }
    sort_boundaries(&mut boundaries);
    refine_peer_buckets(
        input,
        table,
        timeline.as_ref(),
        &effects.peer_buckets,
        &mut boundaries,
    )?;
    let mut occupancy = ManuIntegralSum::default();
    let mut skill = ManuIntegralSum::default();
    let mut global = ManuIntegralSum::default();
    let mut mood_drain = Vec::new();
    for window in boundaries.windows(2) {
        let (start, end) = (window[0], window[1]);
        let elapsed = warm_interval_sample(start, end);
        let sample = if effects.peer_buckets.is_empty() || warm_interior_pair(start, end).is_none()
        {
            // 无足够可表示内点的原子段：暖机取解析平均，保留整段真实权重。
            // 非线性消费者仍只由原解释器在该代表值求值。
            ManuRampSample::Average(start, end)
        } else {
            ManuRampSample::At(elapsed)
        };
        let ctx = timed_manufacture_context(
            input,
            table,
            timeline.as_ref(),
            elapsed,
            sample,
            &mut NoManuObserver,
        );
        let weight = (end - start) / hours;
        let raw_occupancy = ctx.prod_base();
        let raw_skill = ctx.prod_skill(input.active_recipe);
        occupancy.add(raw_occupancy * weight);
        skill.add(raw_skill * weight);
        global.add((ctx.prod_total(input.active_recipe) - raw_occupancy - raw_skill) * weight);
        if FULL && end == hours {
            mood_drain = context_mood_drains(&ctx);
        }
    }
    let end = timed_manufacture_context(
        input,
        table,
        timeline.as_ref(),
        hours,
        ManuRampSample::At(hours),
        &mut NoManuObserver,
    );
    Ok(summarize_warm_averages(
        occupancy.total(),
        skill.total(),
        global.total(),
        end.storage_limit(input.active_recipe),
        mood_drain,
    ))
}

/// 轻量与完整结果共享暖机积分；无暖机仍直接提取原上下文。
pub(crate) fn solve_manufacture_light_view(
    input: &ManuRoomView<'_>,
    table: &SkillTable,
    shift_hours: Option<f64>,
) -> Result<(Efficiency, i32)> {
    with_manufacture_time(input, table, shift_hours, |input, effects| {
        if effects.has_ramp() {
            let result = integrate_manufacture_ramp::<false>(input, table, shift_hours, effects)?;
            return Ok((result.final_efficiency, result.storage_limit));
        }
        if let Some(hours) = shift_hours {
            if effects.mood_gap && hours > 0.0 {
                let result = solve_manufacture_without_ramp_with_shift(input, table, hours)?;
                return Ok((result.final_efficiency, result.storage_limit));
            }
        }
        let mut ctx = ManuContext::from_view(input);
        apply_manu_phases(&mut ctx, table);
        let recipe = input.active_recipe;
        let occupancy = Efficiency::from_percent_points(ctx.prod_base());
        let skill = Efficiency::from_percent_points(ctx.prod_skill(recipe));
        let global = Efficiency::from_percent_points(
            ctx.prod_total(recipe) - ctx.prod_base() - ctx.prod_skill(recipe),
        );
        Ok((
            Efficiency::ONE + occupancy + skill + global,
            ctx.storage_limit(recipe),
        ))
    })
}

/// 按 `ManuLineScenario` 对同一三人组在各配方产线上求值并加权求和。
pub fn evaluate_manufacture_lines(
    input: &ManuRoomInput,
    table: &SkillTable,
    scenario: ManuLineScenario,
) -> Result<ManuLineEfficiency> {
    let mut final_efficiency = Efficiency::ZERO;
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();

    for (recipe, lines) in scenario.active_recipes() {
        let mut room = input.clone();
        room.active_recipe = recipe;
        let result = solve_manufacture(&room, table)?;
        final_efficiency += result.final_efficiency.scale_ratio(i64::from(lines), 1);
        match recipe {
            RecipeKind::Gold => {
                per_station.gold = result.final_efficiency;
                storage.gold = result.storage_limit;
            }
            RecipeKind::BattleRecord => {
                per_station.battle_record = result.final_efficiency;
                storage.battle_record = result.storage_limit;
            }
            RecipeKind::Originium => {
                per_station.originium = result.final_efficiency;
                storage.originium = result.storage_limit;
            }
            RecipeKind::All => {}
        }
    }

    Ok(ManuLineEfficiency {
        final_efficiency,
        per_station,
        storage,
    })
}
