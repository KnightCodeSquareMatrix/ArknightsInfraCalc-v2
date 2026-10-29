use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use smallvec::SmallVec;

use crate::efficiency::Efficiency;
use crate::error::Result;
use crate::layout::{LayoutContext, SharedLayout};
use crate::manufacture::input::{
    ManuOperator, ManuOperatorRef, ManuRoomInput, ManuRoomView, ManuSearchRecipeMode,
};
use crate::manufacture::solver::{
    solve_manufacture_light_view, solve_manufacture_view, solve_manufacture_with_shift_view,
    ManuProdBreakdown, ManuStorageBreakdown,
};
use crate::pool::{
    filter_general_manufacture_search_pool, filter_standalone_exact_with, ManuPool,
    StandaloneFilter,
};
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, EffectAtom, RecipeKind, SkillDef};
use crate::FacilityKind;

use super::enumerate::{evaluate_flat_combinations, flat_combinations};

/// 制造站直接效率分解，所有数值均为无量纲小数效率。
///
/// 制造评分公式：
/// ```text
/// final_efficiency = base_efficiency + occupancy_efficiency + skill_efficiency + global_efficiency
/// ```
/// `occupancy_efficiency` 为每名进驻干员 0.010，技能与全局注入也按小数效率表示。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuEfficiencyBreakdown {
    /// 基础效率：100% + 非 0 心情入驻人数 × 1%。
    pub base_efficiency: Efficiency,
    pub occupancy_efficiency: Efficiency,
    pub skill_efficiency: Efficiency,
    pub global_efficiency: Efficiency,
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub unit_output_per_day: f64,
    pub output_per_day: f64,
    /// 仓库上限
    pub storage_limit: i32,
    /// 配方类型
    pub recipe: String,
}

impl ManuEfficiencyBreakdown {
    /// Aggregate per-station breakdowns into a multi-line direct-efficiency breakdown.
    pub fn aggregate_lines(lines: &[(&Self, u8)]) -> Self {
        let mut out = Self {
            recipe: "lines".to_string(),
            ..Self::default()
        };
        for (breakdown, count) in lines {
            let count_i64 = i64::from(*count);
            out.base_efficiency += breakdown.base_efficiency.scale_ratio(count_i64, 1);
            out.occupancy_efficiency += breakdown.occupancy_efficiency.scale_ratio(count_i64, 1);
            out.skill_efficiency += breakdown.skill_efficiency.scale_ratio(count_i64, 1);
            out.global_efficiency += breakdown.global_efficiency.scale_ratio(count_i64, 1);
            out.output_per_day += breakdown.output_per_day * f64::from(*count);
            out.storage_limit += breakdown.storage_limit * i32::from(*count);
        }
        out.final_efficiency = out.base_efficiency + out.skill_efficiency + out.global_efficiency;
        out.unit_output_per_day = 0.0;
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuSearchHit {
    /// Single-recipe triple, or combined multi-line result.
    pub names: Vec<String>,
    /// Split-line search: best triple on gold recipe stations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gold_names: Vec<String>,
    /// Split-line search: best triple on battle-record recipe stations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub battle_record_names: Vec<String>,
    /// 排序主键；多产线时为各产线直接效率之和。
    #[serde(rename = "total_efficiency")]
    pub final_efficiency: Efficiency,
    pub per_station: ManuProdBreakdown,
    pub storage: ManuStorageBreakdown,
    /// 评分明细分解
    pub breakdown: ManuEfficiencyBreakdown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuSearchReport {
    pub recipe_mode: ManuSearchRecipeMode,
    pub best: ManuSearchHit,
    pub top: Vec<ManuSearchHit>,
    pub combinations: u64,
    pub evaluated: u64,
    pub elapsed: Duration,
    /// Present when `recipe_mode` is multi-line split search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gold_line: Option<ManuSearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battle_record_line: Option<ManuSearchHit>,
}

pub(crate) const MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS: f64 = 12.0;

#[derive(Debug, Clone)]
pub struct ManuSearchOptions {
    pub level: u8,
    pub operator_capacity: usize,
    pub recipe_mode: ManuSearchRecipeMode,
    pub mood: f64,
    pub top_k: usize,
    pub layout: SharedLayout,
    /// 定时班次平均效率窗口；`None` 保持独立探索的静态纸面口径。
    pub shift_hours: Option<f64>,
    pub must_include_name: Option<String>,
    pub bake_mode: crate::bake::BakeMode,
    pub full_pool: bool,
}

impl Default for ManuSearchOptions {
    fn default() -> Self {
        Self {
            level: 3,
            operator_capacity: 3,
            recipe_mode: ManuSearchRecipeMode::default(),
            mood: 24.0,
            top_k: 5,
            layout: Arc::new(LayoutContext::search_baseline()),
            shift_hours: None,
            must_include_name: None,
            bake_mode: crate::bake::BakeMode::Auto,
            full_pool: false,
        }
    }
}

impl ManuSearchOptions {
    /// 单个组合的 live 命中构造（含同房兼容、候选投影与班次门槛）。
    ///
    /// 实时搜索、Bake 行生成与 Bake 行级对账共用同一入口，保证三者对同一组合
    /// 产出的 `ManuSearchHit` 逐字段一致。
    pub(crate) fn solve_manufacture_combo(
        &self,
        pool: &ManuPool,
        table: &SkillTable,
        recipe: RecipeKind,
        combo: &[usize],
    ) -> Option<ManuSearchHit> {
        let operators: SmallVec<[_; 5]> = combo
            .iter()
            .map(|idx| pool.entries[*idx].as_manu_operator_ref())
            .collect();
        let members = manufacture_members(&operators);
        if !crate::layout::admission::candidate_admitted(
            &self.layout,
            "manufacture",
            members.clone(),
            table,
            true,
        ) {
            return None;
        }
        let shift_hours = if self.layout.auto_rotation {
            Some(
                self.layout
                    .evaluation_hours(self.shift_hours.unwrap_or(24.0))
                    .ok()?,
            )
        } else {
            self.shift_hours
        };
        if !manufacture_combo_refs_allowed_for_shift(
            &operators,
            table,
            (!self.layout.auto_rotation)
                .then_some(shift_hours)
                .flatten(),
        ) {
            return None;
        }
        let layout = manufacture_combo_layout(&self.layout, &operators);
        let base = ManuRoomView {
            level: self.level,
            operators,
            active_recipe: recipe,
            mood: self.mood,
            layout: &layout,
        };
        evaluate_manufacture_room_view_with_shift(&base, table, recipe, shift_hours)
    }

    /// 池内是否有清零同伴产出的干员；Bake 查表在存在这类干员时不可用。
    pub(crate) fn pool_has_peer_reset(&self, pool: &ManuPool, table: &SkillTable) -> bool {
        pool.entries
            .iter()
            .any(|entry| manufacture_operator_resets_peers(&entry.as_manu_operator_ref(), table))
    }
}

pub fn search_manufacture_triples(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
) -> Result<ManuSearchReport> {
    let _memo_scope = super::ensure_search_memo_scope();
    let mut general_pool = filter_general_manufacture_search_pool(pool);
    if let Some(anchor) = options.must_include_name.as_deref() {
        if general_pool.entry(anchor).is_none() {
            if let Some(entry) = pool.entry(anchor) {
                general_pool.entries.push(entry.clone());
            }
        }
    }
    match options.recipe_mode {
        ManuSearchRecipeMode::Lines(scenario) => {
            search_manufacture_split_lines(&general_pool, table, options, scenario)
        }
        ManuSearchRecipeMode::Single(_) => {
            search_manufacture_single_recipe(&general_pool, table, options)
        }
    }
}

/// 同类产线共用同一三人组：赤金线与经验线分别搜索后再按产线数加权。
fn search_manufacture_split_lines(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
    scenario: crate::manufacture::input::ManuLineScenario,
) -> Result<ManuSearchReport> {
    let start = Instant::now();

    let mut gold_opts = options.clone();
    gold_opts.recipe_mode = ManuSearchRecipeMode::Single(RecipeKind::Gold);
    let gold_report = search_manufacture_single_recipe(pool, table, &gold_opts)?;

    let mut br_opts = options.clone();
    br_opts.recipe_mode = ManuSearchRecipeMode::Single(RecipeKind::BattleRecord);
    let br_report = search_manufacture_single_recipe(pool, table, &br_opts)?;

    let final_efficiency = gold_report
        .best
        .final_efficiency
        .scale_ratio(i64::from(scenario.gold_lines), 1)
        + br_report
            .best
            .final_efficiency
            .scale_ratio(i64::from(scenario.battle_record_lines), 1);

    let breakdown = ManuEfficiencyBreakdown::aggregate_lines(&[
        (&gold_report.best.breakdown, scenario.gold_lines),
        (&br_report.best.breakdown, scenario.battle_record_lines),
    ]);
    debug_assert_eq!(breakdown.final_efficiency, final_efficiency);

    let best = ManuSearchHit {
        names: vec![],
        gold_names: gold_report.best.names.clone(),
        battle_record_names: br_report.best.names.clone(),
        final_efficiency,
        per_station: ManuProdBreakdown {
            gold: gold_report.best.per_station.gold,
            battle_record: br_report.best.per_station.battle_record,
            originium: Efficiency::ZERO,
        },
        storage: ManuStorageBreakdown {
            gold: gold_report.best.storage.gold,
            battle_record: br_report.best.storage.battle_record,
            originium: 0,
        },
        breakdown,
    };

    Ok(ManuSearchReport {
        recipe_mode: ManuSearchRecipeMode::Lines(scenario),
        best: best.clone(),
        top: vec![best],
        combinations: gold_report
            .combinations
            .saturating_add(br_report.combinations),
        evaluated: gold_report.evaluated.saturating_add(br_report.evaluated),
        elapsed: start.elapsed(),
        gold_line: Some(gold_report.best),
        battle_record_line: Some(br_report.best),
    })
}

fn search_manufacture_single_recipe(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
) -> Result<ManuSearchReport> {
    let leaf_started = Instant::now();
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Manufacture);
    let ManuSearchRecipeMode::Single(recipe) = options.recipe_mode else {
        return Err(crate::error::Error::msg(
            "search_manufacture_single_recipe requires Single recipe mode",
        ));
    };
    let memo_key_started = Instant::now();
    let memo_key = super::memo::manu_key(pool, table, options, recipe)
        .ok()
        .flatten();
    let memo_key_elapsed = memo_key_started.elapsed();
    let cached_report = memo_key.as_deref().and_then(super::memo::manu_get);
    perf.set_memo_lookup(
        memo_key_elapsed,
        memo_key.as_ref().map(|_| cached_report.is_some()),
    );
    if let Some(mut report) = cached_report {
        report.elapsed = leaf_started.elapsed();
        perf.finish(0, 0, 0, report.top.len() as u64, false, Default::default());
        return Ok(report);
    }
    let prepare_started = Instant::now();
    let sub = if options.full_pool || options.must_include_name.is_some() {
        pool.clone()
    } else {
        let sub = filter_standalone_exact_with(
            pool,
            FacilityKind::Factory,
            StandaloneFilter::for_recipe(recipe),
        )
        .unwrap_or_else(|| pool.clone());
        if sub.entries.len() >= options.operator_capacity.clamp(1, 3) {
            sub
        } else {
            let fallback = filter_recipe_productive_pool(pool, table, recipe);
            if fallback.entries.len() >= options.operator_capacity.clamp(1, 3) {
                fallback
            } else {
                sub
            }
        }
    };
    let n = sub.entries.len();
    let must_idx = options
        .must_include_name
        .as_ref()
        .and_then(|name| sub.entries.iter().position(|e| e.name == *name));
    if options.must_include_name.is_some() && must_idx.is_none() {
        return Err(crate::error::Error::msg(format!(
            "manufacture pool missing must-include operator {:?}",
            options.must_include_name
        )));
    }
    let operator_capacity = options.operator_capacity.clamp(1, 3);
    let flat = flat_combinations(n, operator_capacity, must_idx.as_slice());
    let combinations = (flat.len() / operator_capacity) as u64;
    let prepare_elapsed = prepare_started.elapsed();
    perf.set_progress(
        combinations,
        0,
        0,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            ..Default::default()
        },
    );
    let start = Instant::now();

    let bake_started = Instant::now();
    let baked_report =
        if !options.layout.auto_rotation && options.bake_mode != crate::bake::BakeMode::Disabled {
            crate::bake::try_baked_manufacture_search(
                &sub,
                table,
                options,
                recipe,
                combinations,
                start,
            )?
        } else {
            None
        };
    let bake_lookup_elapsed = bake_started.elapsed();
    if let Some(report) = baked_report {
        if let Some(key) = memo_key {
            super::memo::manu_insert(key, &report);
        }
        perf.finish(
            combinations,
            0,
            report.evaluated,
            report.top.len() as u64,
            true,
            crate::profile::SearchPhaseDurations {
                prepare: prepare_elapsed,
                bake_lookup: bake_lookup_elapsed,
                ..Default::default()
            },
        );
        return Ok(report);
    }

    perf.set_progress(
        combinations,
        combinations,
        0,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            ..Default::default()
        },
    );
    let evaluate_started = Instant::now();
    perf.mark_live_started();
    let batch_counters = perf.hot_path_batch_counters();
    struct Light {
        idx: usize,
        eff: Efficiency,
        storage: i32,
    }
    let k = operator_capacity;
    let mut hits: Vec<Light> = evaluate_flat_combinations(
        &flat,
        k,
        &batch_counters,
        |ci, combo| {
            let ops: SmallVec<[_; 5]> = combo
                .iter()
                .map(|i| sub.entries[*i].as_manu_operator_ref())
                .collect();
            let members = manufacture_members(&ops);
            if !crate::layout::admission::candidate_admitted(
                &options.layout,
                "manufacture",
                members.clone(),
                table,
                true,
            ) {
                return None;
            }
            let shift_hours = if options.layout.auto_rotation {
                Some(
                    options
                        .layout
                        .evaluation_hours(options.shift_hours.unwrap_or(24.0))
                        .ok()?,
                )
            } else {
                options.shift_hours
            };
            if !manufacture_combo_refs_allowed_for_shift(
                &ops,
                table,
                (!options.layout.auto_rotation)
                    .then_some(shift_hours)
                    .flatten(),
            ) {
                return None;
            }
            let layout = manufacture_combo_layout(&options.layout, &ops);
            let base = ManuRoomView {
                level: options.level,
                operators: ops,
                active_recipe: recipe,
                mood: options.mood,
                layout: &layout,
            };
            let (eff, storage) =
                evaluate_manufacture_room_light(&base, table, recipe, shift_hours)?;
            Some(Light {
                idx: ci,
                eff,
                storage,
            })
        },
        |_| true,
    );
    let evaluate_elapsed = evaluate_started.elapsed();

    let accepted_hits = hits.len() as u64;
    perf.set_progress(
        combinations,
        combinations,
        accepted_hits,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            evaluate: evaluate_elapsed,
            ..Default::default()
        },
    );
    let sort_started = Instant::now();
    hits.sort_by(|a, b| {
        b.eff
            .cmp(&a.eff)
            .then_with(|| b.storage.cmp(&a.storage))
            .then_with(|| {
                let ca = &flat[a.idx * k..a.idx * k + k];
                let cb = &flat[b.idx * k..b.idx * k + k];
                ca.iter()
                    .map(|i| sub.entries[*i].name.as_str())
                    .cmp(cb.iter().map(|i| sub.entries[*i].name.as_str()))
            })
    });

    let evaluated = hits.len() as u64;
    let top_k = options.top_k.min(hits.len());
    let materialize = if hits.is_empty() { 0 } else { top_k.max(1) };
    let mut full: Vec<ManuSearchHit> = Vec::with_capacity(materialize);
    for light in hits.iter().take(materialize) {
        let combo = &flat[light.idx * k..light.idx * k + k];
        let ops: SmallVec<[_; 5]> = combo
            .iter()
            .map(|i| sub.entries[*i].as_manu_operator_ref())
            .collect();
        let layout = manufacture_combo_layout(&options.layout, &ops);
        let base = ManuRoomView {
            level: options.level,
            operators: ops,
            active_recipe: recipe,
            mood: options.mood,
            layout: &layout,
        };
        let shift_hours = if options.layout.auto_rotation {
            Some(
                options
                    .layout
                    .evaluation_hours(options.shift_hours.unwrap_or(24.0))
                    .expect("accepted candidate window"),
            )
        } else {
            options.shift_hours
        };
        let hit = evaluate_manufacture_room_view_with_shift(&base, table, recipe, shift_hours)
            .expect("lazy manufacture hit must re-evaluate");
        assert_eq!(
            hit.final_efficiency, light.eff,
            "lazy re-evaluation drifted"
        );
        full.push(hit);
    }
    let best = full.first().cloned().unwrap_or(empty_hit());
    let top: Vec<ManuSearchHit> = full.into_iter().take(top_k).collect();
    let sort_topk_elapsed = sort_started.elapsed();

    let report = ManuSearchReport {
        recipe_mode: options.recipe_mode,
        best,
        top,
        combinations,
        evaluated,
        elapsed: start.elapsed(),
        gold_line: None,
        battle_record_line: None,
    };
    if let Some(key) = memo_key {
        super::memo::manu_insert(key, &report);
    }
    perf.finish(
        combinations,
        combinations,
        evaluated,
        report.top.len() as u64,
        false,
        crate::profile::SearchPhaseDurations {
            prepare: prepare_elapsed,
            bake_lookup: bake_lookup_elapsed,
            evaluate: evaluate_elapsed,
            sort_topk: sort_topk_elapsed,
        },
    );
    Ok(report)
}

pub(crate) fn manufacture_combo_allowed_for_shift(
    operators: &[ManuOperator],
    table: &SkillTable,
    shift_hours: Option<f64>,
) -> bool {
    let operators: SmallVec<[_; 5]> = operators.iter().map(ManuOperator::as_ref).collect();
    manufacture_combo_refs_allowed_for_shift(&operators, table, shift_hours)
}

fn manufacture_combo_refs_allowed_for_shift(
    operators: &[ManuOperatorRef<'_>],
    table: &SkillTable,
    shift_hours: Option<f64>,
) -> bool {
    if !manufacture_peer_refs_compatible(operators, table) {
        return false;
    }
    if !shift_hours.is_some_and(|hours| hours >= MOOD_GAP_DECAY_SHIFT_LIMIT_HOURS) {
        return true;
    }
    !operators
        .iter()
        .any(|operator| manufacture_operator_ref_has_mood_gap_decay(operator, table))
}

pub(crate) fn manufacture_peers_compatible(operators: &[ManuOperator], table: &SkillTable) -> bool {
    let operators: SmallVec<[_; 5]> = operators.iter().map(ManuOperator::as_ref).collect();
    manufacture_peer_refs_compatible(&operators, table)
}

fn manufacture_peer_refs_compatible(operators: &[ManuOperatorRef<'_>], table: &SkillTable) -> bool {
    !operators
        .iter()
        .any(|operator| manufacture_operator_resets_peers(operator, table))
        || operators.iter().all(|operator| {
            operator.name == "清流" || manufacture_operator_resets_peers(operator, table)
        })
}

fn manufacture_operator_resets_peers(operator: &ManuOperatorRef<'_>, table: &SkillTable) -> bool {
    operator.buff_ids.iter().any(|buff_id| {
        table.get(buff_id).is_some_and(|skill| {
            skill.facility == "manufacture"
                && skill.atoms.iter().any(|atom| {
                    matches!(&atom.action, Action::PeerEffAbsorb { rate_per_peer } if *rate_per_peer == 0.0)
                })
        })
    })
}

pub(crate) fn manufacture_operator_has_mood_gap_decay(
    operator: &ManuOperator,
    table: &SkillTable,
) -> bool {
    manufacture_operator_ref_has_mood_gap_decay(&operator.as_ref(), table)
}

fn manufacture_operator_ref_has_mood_gap_decay(
    operator: &ManuOperatorRef<'_>,
    table: &SkillTable,
) -> bool {
    operator.buff_ids.iter().any(|buff_id| {
        table.get(buff_id).is_some_and(|skill| {
            skill.atoms.iter().any(|atom| {
                atom.selector == Some(crate::types::Selector::OwnerMoodGap)
                    && matches!(
                        &atom.action,
                        Action::AddBucketEffFromSelector { ret_per_step, .. }
                            if *ret_per_step < 0.0
                    )
            })
        })
    })
}

fn manufacture_combo_layout(
    base: &SharedLayout,
    operators: &[ManuOperatorRef<'_>],
) -> SharedLayout {
    let changes_dynamic_count = base.global_inject.manu_count_scaled().iter().any(|rule| {
        operators
            .iter()
            .any(|operator| operator.tags.iter().any(|tag| tag == &rule.target_tag))
    });
    if !changes_dynamic_count {
        return Arc::clone(base);
    }

    let mut layout = (**base).clone();
    let mut base_names: HashSet<_> = layout.base_workforce.iter().cloned().collect();
    let mut manu_names: HashSet<_> = layout.manu_workforce.iter().cloned().collect();
    for operator in operators {
        if base_names.insert(operator.name.to_owned()) {
            layout.base_workforce.push(operator.name.to_owned());
            // 解释器按 base_workforce 去重补算房内莱茵候选；候选一旦进入 base_workforce，
            // 就必须同步进入 rhine_life_in_base，否则会被去重吞掉，娜斯提少算一名。
            if operator
                .tags
                .iter()
                .any(|tag| tag == crate::layout::TAG_RHINE)
            {
                layout.rhine_life_in_base = layout.rhine_life_in_base.saturating_add(1).min(6);
            }
        }
        if !manu_names.insert(operator.name.to_owned()) {
            continue;
        }
        layout.manu_workforce.push(operator.name.to_owned());
        let mut seen_tags = HashSet::new();
        for tag in operator.tags {
            if seen_tags.insert(tag.as_str()) {
                *layout.manu_tagged_count_sum.entry(tag.clone()).or_insert(0) += 1;
            }
        }
    }
    layout
        .global_inject
        .refresh_manu_count_scaled(&layout.manu_tagged_count_sum);
    Arc::new(layout)
}

fn filter_recipe_productive_pool(
    pool: &ManuPool,
    table: &SkillTable,
    recipe: RecipeKind,
) -> ManuPool {
    ManuPool {
        entries: pool
            .entries
            .iter()
            .filter(|entry| {
                entry.buff_ids.iter().any(|buff_id| {
                    table
                        .get(buff_id)
                        .is_some_and(|skill| skill_has_productive_eff_for_recipe(skill, recipe))
                })
            })
            .cloned()
            .collect(),
        skipped: pool.skipped.clone(),
    }
}

fn skill_has_productive_eff_for_recipe(skill: &SkillDef, recipe: RecipeKind) -> bool {
    if skill.facility != "manufacture" {
        return false;
    }
    skill
        .atoms
        .iter()
        .any(|atom| atom_can_contribute_prod_for_recipe(atom, recipe))
}

fn atom_can_contribute_prod_for_recipe(atom: &EffectAtom, recipe: RecipeKind) -> bool {
    if !atom_condition_can_match_recipe(&atom.condition, recipe) {
        return false;
    }
    match &atom.action {
        Action::AddFlatEff {
            value,
            recipe: action_recipe,
        } => *value > 0.0 && recipe_filter_matches(*action_recipe, recipe),
        Action::AddFlatEffFromSelector {
            multiplier,
            recipe: action_recipe,
            ..
        } => *multiplier > 0.0 && recipe_filter_matches(*action_recipe, recipe),
        Action::AddEffRamp {
            initial,
            per_hour,
            cap,
            ..
        } => *cap > 0.0 && (*initial > 0.0 || *per_hour > 0.0),
        Action::AddBucketEffFromSelector {
            ret_per_step, cap, ..
        } => *ret_per_step > 0.0 && *cap > 0.0,
        Action::AddEffFromLimitContribSum { rate } => *rate > 0.0,
        Action::AddEffFromLimitContribTiered {
            low_rate,
            high_rate,
            ..
        } => *low_rate > 0.0 || *high_rate > 0.0,
        Action::StateConsumeToEff {
            div, multiplier, ..
        } => *div > 0.0 && multiplier.unwrap_or(1.0) > 0.0,
        _ => false,
    }
}

fn atom_condition_can_match_recipe(condition: &Option<Condition>, recipe: RecipeKind) -> bool {
    match condition {
        Some(Condition::ActiveRecipe { kind }) => *kind == recipe,
        _ => true,
    }
}

fn recipe_filter_matches(filter: Option<RecipeKind>, recipe: RecipeKind) -> bool {
    matches!(filter, None | Some(RecipeKind::All)) || filter == Some(recipe)
}

fn manufacture_efficiency_sort_key(hit: &ManuSearchHit) -> Efficiency {
    hit.final_efficiency
}

pub(crate) fn compare_manufacture_hits(a: &ManuSearchHit, b: &ManuSearchHit) -> Ordering {
    manufacture_efficiency_sort_key(b)
        .cmp(&manufacture_efficiency_sort_key(a))
        .then_with(|| {
            b.storage
                .gold
                .max(b.storage.battle_record)
                .cmp(&a.storage.gold.max(a.storage.battle_record))
        })
        .then_with(|| a.names.cmp(&b.names))
}

pub(crate) fn evaluate_manufacture_room(
    input: &ManuRoomInput,
    table: &SkillTable,
    recipe: RecipeKind,
    shift_hours: f64,
) -> Option<ManuSearchHit> {
    evaluate_manufacture_room_view_with_shift(&input.as_view(), table, recipe, Some(shift_hours))
}

fn evaluate_manufacture_room_view_with_shift(
    base: &ManuRoomView<'_>,
    table: &SkillTable,
    recipe: RecipeKind,
    shift_hours: Option<f64>,
) -> Option<ManuSearchHit> {
    if !manufacture_peer_refs_compatible(&base.operators, table) {
        return None;
    }
    let names = base
        .operators
        .iter()
        .map(|operator| operator.name.to_owned())
        .collect();
    let mut room = base.clone();
    room.active_recipe = recipe;
    let result = match shift_hours {
        Some(hours) => solve_manufacture_with_shift_view(&room, table, hours).ok()?,
        None => solve_manufacture_view(&room, table).ok()?,
    };
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();
    let recipe_str = match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.final_efficiency;
            storage.gold = result.storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.final_efficiency;
            storage.battle_record = result.storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = result.final_efficiency;
            storage.originium = result.storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };

    let breakdown = ManuEfficiencyBreakdown {
        base_efficiency: result.base_efficiency,
        occupancy_efficiency: result.occupancy_efficiency,
        skill_efficiency: result.skill_efficiency,
        global_efficiency: result.global_efficiency,
        final_efficiency: result.final_efficiency,
        unit_output_per_day: result.unit_output_per_day,
        output_per_day: result.output_per_day,
        storage_limit: result.storage_limit,
        recipe: recipe_str.to_string(),
    };

    Some(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency: result.final_efficiency,
        per_station,
        storage,
        breakdown,
    })
}

/// 只产出排序所需的 (final_efficiency, storage key)，不构造 names / breakdown。
fn evaluate_manufacture_room_light(
    base: &ManuRoomView<'_>,
    table: &SkillTable,
    recipe: RecipeKind,
    shift_hours: Option<f64>,
) -> Option<(Efficiency, i32)> {
    if !manufacture_peer_refs_compatible(&base.operators, table) {
        return None;
    }
    let (final_efficiency, storage_limit) =
        solve_manufacture_light_view(base, table, shift_hours).ok()?;
    let storage = match recipe {
        RecipeKind::Gold | RecipeKind::BattleRecord => storage_limit.max(0),
        _ => 0,
    };
    Some((final_efficiency, storage))
}

fn empty_hit() -> ManuSearchHit {
    ManuSearchHit {
        names: vec![],
        gold_names: vec![],
        battle_record_names: vec![],
        final_efficiency: Efficiency::ZERO,
        per_station: ManuProdBreakdown::default(),
        storage: ManuStorageBreakdown::default(),
        breakdown: ManuEfficiencyBreakdown::default(),
    }
}

fn manufacture_members(
    operators: &[ManuOperatorRef<'_>],
) -> Vec<(String, u8, Vec<String>, Vec<String>)> {
    operators
        .iter()
        .map(|op| {
            (
                op.name.to_owned(),
                op.elite,
                op.buff_ids.to_vec(),
                op.tags.to_vec(),
            )
        })
        .collect()
}
