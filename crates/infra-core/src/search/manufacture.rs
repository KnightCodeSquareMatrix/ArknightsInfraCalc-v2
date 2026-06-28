use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::layout::{LayoutContext, SharedLayout};
use crate::manufacture::input::{ManuRoomInput, ManuSearchRecipeMode};
use crate::manufacture::solver::{solve_manufacture, ManuProdBreakdown, ManuStorageBreakdown};
use crate::pool::{
    combinations_indices, combinations_indices_with_anchor, filter_general_manufacture_search_pool,
    filter_standalone_exact_with, ManuPool, StandaloneFilter,
};
use crate::skill_table::SkillTable;
use crate::types::{Action, Condition, EffectAtom, RecipeKind, SkillDef};
use crate::FacilityKind;

/// 制造站评分的完整分解，展示每个因子对 `composite_score` (= `prod_total`) 的贡献。
///
/// 制造评分公式：
/// ```text
/// prod_total = prod_base + prod_skill + prod_global
/// ```
/// 其中 `prod_base` 为房间内干员数 × 100%，`prod_skill` 为技能/站级/布局效率，
/// `prod_global` 为全局注入。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ManuScoreBreakdown {
    /// 房间基础 (人数 × 100%)
    pub prod_base: f64,
    /// 技能效率 (含站级/布局)
    pub prod_skill: f64,
    /// 全局注入
    pub prod_global: f64,
    /// = prod_base + prod_skill + prod_global（即 composite_score）
    pub prod_total: f64,
    /// 仓库上限
    pub storage_limit: i32,
    /// 配方类型
    pub recipe: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuSearchHit {
    /// Single-recipe triple, or legacy combined triple.
    pub names: Vec<String>,
    /// Split-line search: best triple on gold recipe stations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gold_names: Vec<String>,
    /// Split-line search: best triple on battle-record recipe stations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub battle_record_names: Vec<String>,
    /// 排序主键：单配方时为 `prod_total`；多产线时为加权 `composite`。
    pub composite_score: f64,
    pub per_station: ManuProdBreakdown,
    pub storage: ManuStorageBreakdown,
    /// 评分明细分解
    pub breakdown: ManuScoreBreakdown,
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

#[derive(Debug, Clone)]
pub struct ManuSearchOptions {
    pub level: u8,
    pub operator_capacity: usize,
    pub recipe_mode: ManuSearchRecipeMode,
    pub mood: f64,
    pub top_k: usize,
    pub layout: SharedLayout,
    pub must_include_name: Option<String>,
    pub use_baked: bool,
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
            must_include_name: None,
            use_baked: true,
            full_pool: false,
        }
    }
}

pub fn search_manufacture_triples(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
) -> Result<ManuSearchReport> {
    let general_pool = filter_general_manufacture_search_pool(pool);
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

    let composite_score = f64::from(scenario.gold_lines) * gold_report.best.composite_score
        + f64::from(scenario.battle_record_lines) * br_report.best.composite_score;

    let best = ManuSearchHit {
        names: vec![],
        gold_names: gold_report.best.names.clone(),
        battle_record_names: br_report.best.names.clone(),
        composite_score,
        per_station: ManuProdBreakdown {
            gold: gold_report.best.per_station.gold,
            battle_record: br_report.best.per_station.battle_record,
            originium: 0.0,
        },
        storage: ManuStorageBreakdown {
            gold: gold_report.best.storage.gold,
            battle_record: br_report.best.storage.battle_record,
            originium: 0,
        },
        breakdown: ManuScoreBreakdown::default(),
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
    let ManuSearchRecipeMode::Single(recipe) = options.recipe_mode else {
        return Err(crate::error::Error::msg(
            "search_manufacture_single_recipe requires Single recipe mode",
        ));
    };
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
    let combos: Vec<Vec<usize>> = if let Some(anchor) = must_idx {
        combinations_indices_with_anchor(n, operator_capacity, anchor).collect()
    } else {
        combinations_indices(n, operator_capacity).collect()
    };
    let combinations = combos.len() as u64;
    let start = Instant::now();

    if options.use_baked {
        if let Some(report) = crate::bake::try_baked_manufacture_search(
            &sub,
            table,
            options,
            recipe,
            combinations,
            start,
        )? {
            return Ok(report);
        }
    }

    let mut hits: Vec<ManuSearchHit> = combos
        .par_iter()
        .filter_map(|combo| {
            let ops: Vec<_> = combo
                .iter()
                .map(|i| sub.entries[*i].to_manu_operator())
                .collect();
            let base = ManuRoomInput {
                level: options.level,
                operators: ops,
                active_recipe: recipe,
                mood: options.mood,
                layout: Arc::clone(&options.layout),
            };
            eval_single_recipe_hit(&base, table, recipe)
        })
        .collect();

    hits.sort_by(|a, b| {
        manufacture_efficiency_sort_key(b)
            .partial_cmp(&manufacture_efficiency_sort_key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.storage
                    .gold
                    .max(b.storage.battle_record)
                    .cmp(&a.storage.gold.max(a.storage.battle_record))
            })
            .then_with(|| a.names.cmp(&b.names))
    });

    let evaluated = hits.len() as u64;
    let best = hits.first().cloned().unwrap_or(empty_hit());
    let top_k = options.top_k.min(hits.len());
    let top = hits.into_iter().take(top_k).collect();

    Ok(ManuSearchReport {
        recipe_mode: options.recipe_mode,
        best,
        top,
        combinations,
        evaluated,
        elapsed: start.elapsed(),
        gold_line: None,
        battle_record_line: None,
    })
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

fn manufacture_efficiency_sort_key(hit: &ManuSearchHit) -> f64 {
    hit.composite_score
}

fn eval_single_recipe_hit(
    base: &ManuRoomInput,
    table: &SkillTable,
    recipe: RecipeKind,
) -> Option<ManuSearchHit> {
    let names = base
        .operator_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut room = base.clone();
    room.active_recipe = recipe;
    let result = solve_manufacture(&room, table).ok()?;
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();
    let recipe_str = match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.prod_total;
            storage.gold = result.storage_limit;
            "gold"
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.prod_total;
            storage.battle_record = result.storage_limit;
            "battle_record"
        }
        RecipeKind::Originium => {
            per_station.originium = result.prod_total;
            storage.originium = result.storage_limit;
            "originium"
        }
        RecipeKind::All => "all",
    };

    let prod_global = result.prod_total - result.prod_base - result.prod_skill;
    let breakdown = ManuScoreBreakdown {
        prod_base: result.prod_base,
        prod_skill: result.prod_skill,
        prod_global,
        prod_total: result.prod_total,
        storage_limit: result.storage_limit,
        recipe: recipe_str.to_string(),
    };

    Some(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        composite_score: result.prod_total,
        per_station,
        storage,
        breakdown,
    })
}

fn empty_hit() -> ManuSearchHit {
    ManuSearchHit {
        names: vec![],
        gold_names: vec![],
        battle_record_names: vec![],
        composite_score: 0.0,
        per_station: ManuProdBreakdown::default(),
        storage: ManuStorageBreakdown::default(),
        breakdown: ManuScoreBreakdown::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::manufacture::input::{
        ManuLineScenario, ManuOperator, ManuRoomInput, ManuSearchRecipeMode,
    };
    use crate::manufacture::solver::score_manu_composite;
    use crate::pool::{build_manufacture_pool, ManuPoolEntry};
    use crate::roster::Roster;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
    }

    fn test_entry(name: &str, buff_ids: &[&str]) -> ManuPoolEntry {
        ManuPoolEntry {
            name: name.to_string(),
            elite: 0,
            progress: crate::roster::OperatorProgress::elite_only(0),
            buff_ids: buff_ids.iter().map(|id| (*id).to_string()).collect(),
            tags: vec![],
            flat_eff_hint: 0.0,
            has_l2_delegate: false,
            tier: crate::layout::tier::OperatorTier::Standalone,
        }
    }

    #[test]
    fn recipe_productive_fallback_excludes_incompatible_formula_skills() {
        let table = table();
        let pool = ManuPool {
            entries: vec![
                test_entry("炎熔", &["manu_formula_spd[210]"]),
                test_entry("通用标准化", &["manu_prod_spd[000]"]),
                test_entry("赤金工艺", &["manu_formula_spd[100]"]),
            ],
            skipped: vec![],
        };

        let battle_record = filter_recipe_productive_pool(&pool, &table, RecipeKind::BattleRecord);
        assert!(battle_record.entry("通用标准化").is_some());
        assert!(battle_record.entry("炎熔").is_none());
        assert!(battle_record.entry("赤金工艺").is_none());

        let originium = filter_recipe_productive_pool(&pool, &table, RecipeKind::Originium);
        assert!(originium.entry("炎熔").is_some());
    }

    #[test]
    fn manufacture_search_score_is_prod_total_sort_key() {
        let hit = ManuSearchHit {
            names: vec!["a".into()],
            gold_names: vec![],
            battle_record_names: vec![],
            composite_score: 245.0,
            per_station: ManuProdBreakdown {
                gold: 245.0,
                ..Default::default()
            },
            storage: ManuStorageBreakdown::default(),
            breakdown: ManuScoreBreakdown {
                prod_total: 245.0,
                ..Default::default()
            },
        };
        assert_eq!(manufacture_efficiency_sort_key(&hit), hit.composite_score);
        assert_eq!(hit.composite_score, hit.breakdown.prod_total);
    }

    #[test]
    fn shitie_beast_worth_more_on_battle_record_lines_in_composite() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let shitie = ManuOperator::new(
            "食铁兽",
            2,
            instances.resolve_manufacture_buff_ids("食铁兽", PromotionTier::TierUp),
        );
        let generic = ManuOperator::new("芬", 0, vec!["manu_prod_spd[000]".into()]);
        let filler = ManuOperator::new("米格鲁", 0, vec!["manu_prod_spd[000]".into()]);

        let shitie_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![shitie.clone(), generic.clone(), filler.clone()],
        );
        let generic_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                generic.clone(),
                generic.clone(),
                ManuOperator::new("黑角", 0, vec!["manu_prod_spd[000]".into()]),
            ],
        );

        let scenario = ManuLineScenario::standard_four_lines();
        let shitie_score = score_manu_composite(&shitie_room, &table, scenario).unwrap();
        let generic_score = score_manu_composite(&generic_room, &table, scenario).unwrap();
        assert!(
            shitie_score.composite > generic_score.composite,
            "shitie={} generic={}",
            shitie_score.composite,
            generic_score.composite
        );
        assert!(shitie_score.per_station.battle_record > shitie_score.per_station.gold);
    }

    #[test]
    fn standard_four_lines_composite_is_weighted_sum() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let ops: Vec<ManuOperator> = ["蛇屠箱", "黑角", "米格鲁"]
            .iter()
            .map(|name| {
                ManuOperator::new(
                    *name,
                    0,
                    instances.resolve_manufacture_buff_ids(name, PromotionTier::Tier0),
                )
            })
            .collect();
        let room = ManuRoomInput::with_operators(3, RecipeKind::Gold, ops);
        let scenario = ManuLineScenario::standard_four_lines();
        let scored = score_manu_composite(&room, &table, scenario).unwrap();
        let gold = solve_manufacture(
            &ManuRoomInput::with_operators(3, RecipeKind::Gold, room.operators.clone()),
            &table,
        )
        .unwrap();
        let br = solve_manufacture(
            &ManuRoomInput::with_operators(3, RecipeKind::BattleRecord, room.operators),
            &table,
        )
        .unwrap();
        let expected = 2.0 * gold.prod_total + 2.0 * br.prod_total;
        assert!((scored.composite - expected).abs() < 0.01);
    }

    #[test]
    fn default_search_uses_four_line_scenario() {
        let roster = Roster::from_elite_map([("蛇屠箱".into(), 0_u8)].into_iter().collect());
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let pool = build_manufacture_pool(&roster, &instances, &table).unwrap();
        let report =
            search_manufacture_triples(&pool, &table, &ManuSearchOptions::default()).unwrap();
        assert_eq!(
            report.recipe_mode,
            ManuSearchRecipeMode::Lines(ManuLineScenario::standard_four_lines())
        );
        assert!(report.gold_line.is_some());
        assert!(report.battle_record_line.is_some());
    }

    #[test]
    fn level_one_manufacture_search_uses_one_operator_capacity() {
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let roster = Roster::from_elite_map(
            [("芬", 2), ("克洛丝", 2), ("泡普卡", 2)]
                .into_iter()
                .map(|(name, elite)| (name.to_string(), elite))
                .collect(),
        );
        let pool = build_manufacture_pool(&roster, &instances, &table).unwrap();
        let report = search_manufacture_triples(
            &pool,
            &table,
            &ManuSearchOptions {
                level: 1,
                operator_capacity: 1,
                recipe_mode: ManuSearchRecipeMode::Single(RecipeKind::Gold),
                ..ManuSearchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.best.names.len(), 1);
        assert_eq!(report.combinations, 3);
        assert_eq!(report.best.breakdown.prod_base, 1.0);
    }

    #[test]
    fn split_line_search_picks_recipe_specialists() {
        use crate::operbox::{OperBox, OperBoxEntry};

        let entries = vec![
            OperBoxEntry {
                id: "g1".into(),
                name: "清流".into(),
                elite: 1,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
            OperBoxEntry {
                id: "g2".into(),
                name: "斑点".into(),
                elite: 1,
                level: 1,
                own: true,
                potential: 1,
                rarity: 3,
            },
            OperBoxEntry {
                id: "g3".into(),
                name: "砾".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
            OperBoxEntry {
                id: "b1".into(),
                name: "酒神".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "b2".into(),
                name: "白雪".into(),
                elite: 1,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
            OperBoxEntry {
                id: "b3".into(),
                name: "红豆".into(),
                elite: 1,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
            OperBoxEntry {
                id: "f1".into(),
                name: "裁度".into(),
                elite: 0,
                level: 1,
                own: true,
                potential: 1,
                rarity: 5,
            },
        ];
        let operbox = OperBox::from_entries(entries);
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        let report =
            search_manufacture_triples(&pool, &table, &ManuSearchOptions::default()).unwrap();
        let gold = report.gold_line.as_ref().expect("gold line");
        let br = report.battle_record_line.as_ref().expect("exp line");
        assert!(gold.names.contains(&"清流".to_string()));
        assert!(br.names.contains(&"酒神".to_string()));
        assert!(
            (report.best.composite_score - (2.0 * gold.composite_score + 2.0 * br.composite_score))
                .abs()
                < 0.01
        );
        assert!(report.best.composite_score > 342.0);
    }

    #[test]
    fn battle_record_search_picks_christine_wine_god_hongyun() {
        use crate::operbox::{OperBox, OperBoxEntry};

        let entries = vec![
            OperBoxEntry {
                id: "christine".into(),
                name: "Miss.Christine".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 5,
            },
            OperBoxEntry {
                id: "dionysus".into(),
                name: "酒神".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "crownslayer".into(),
                name: "弑君者".into(),
                elite: 2,
                level: 1,
                own: true,
                potential: 1,
                rarity: 6,
            },
            OperBoxEntry {
                id: "hongyun".into(),
                name: "红云".into(),
                elite: 1,
                level: 1,
                own: true,
                potential: 1,
                rarity: 4,
            },
        ];
        let operbox = OperBox::from_entries(entries);
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let pool =
            build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
                .unwrap();

        let report = search_manufacture_triples(
            &pool,
            &table,
            &ManuSearchOptions {
                recipe_mode: ManuSearchRecipeMode::Single(RecipeKind::BattleRecord),
                use_baked: false,
                ..ManuSearchOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            report.best.names,
            vec![
                "Miss.Christine".to_string(),
                "红云".to_string(),
                "酒神".to_string()
            ]
        );
        assert!(
            (report.best.composite_score - 104.0).abs() < 0.01,
            "expected 3 base + 101% skill, got {:?}",
            report.best.breakdown
        );
    }

    #[test]
    fn gongsun_operbox_peer_absorb_operators_pool_and_solve() {
        use crate::manufacture::solver::solve_manufacture;
        use crate::operbox::{default_operbox_gongsun_path, OperBox, OperBoxEntry};

        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = table();
        let roster = operbox.manufacture_roster(&instances);
        let pool = build_manufacture_pool(&roster, &instances, &table).unwrap();

        let qingliu = pool.entry("清流").expect("清流应在制造池");
        assert!(!qingliu.has_l2_delegate, "清流再生能源应已 L1 建模");
        let dongshi = pool.entry("冬时").expect("冬时应在制造池");
        assert!(!dongshi.has_l2_delegate);

        // 公孙盒：森蚺 E1（6★）→ tier_0，instances 无制造技能绑定
        assert!(
            pool.entry("森蚺").is_none(),
            "E1 森蚺无制造技能实例，不应进制造池"
        );
        let skipped_sen = pool.skipped.iter().find(|(n, _, _)| n == "森蚺");
        assert!(
            skipped_sen.is_none(),
            "森蚺不在制造 roster，不应出现在 skipped"
        );

        let mut opts = ManuSearchOptions::default();
        opts.layout = Arc::new(LayoutContext::search_baseline());
        let report = search_manufacture_triples(&pool, &table, &opts).unwrap();
        let gold = report.gold_line.as_ref().expect("gold line");
        assert!(
            !["冬时", "温蒂"]
                .iter()
                .any(|name| gold.names.contains(&name.to_string())),
            "自动化组体系专用干员不应进入普通制造搜索，got {:?}",
            gold.names
        );
        assert!(
            gold.names.contains(&"清流".to_string()),
            "赤金线最优组应含清流，got {:?}",
            gold.names
        );
        assert!(
            (gold.composite_score - 108.0).abs() < 0.5,
            "斑点+清流+砾 赤金纸面≈108%（243_use_this_ 空编制基准），got {}",
            gold.composite_score
        );

        let dongshi_ops: Vec<ManuOperator> = ["冬时", "芬", "克洛丝"]
            .iter()
            .map(|name| {
                let progress = operbox.progress_of(name).unwrap();
                let tier = PromotionTier::from_progress(progress);
                ManuOperator::new(
                    *name,
                    progress.elite,
                    instances.resolve_manufacture_buff_ids(name, tier),
                )
            })
            .collect();
        let dongshi_room = ManuRoomInput::with_operators(3, RecipeKind::Gold, dongshi_ops);
        let dongshi_gold = solve_manufacture(&dongshi_room, &table).unwrap();
        assert!(
            (dongshi_gold.prod_skill - 30.0).abs() < 0.01,
            "冬时站级 3×10%=30，got {}",
            dongshi_gold.prod_skill
        );
        assert_eq!(dongshi_gold.storage_limit, 35, "精一冬时 3×5 仓库贡献");

        let mixed_ops: Vec<ManuOperator> = ["冬时", "清流", "芬"]
            .iter()
            .map(|name| {
                let progress = operbox.progress_of(name).unwrap();
                let tier = PromotionTier::from_progress(progress);
                ManuOperator::new(
                    *name,
                    progress.elite,
                    instances.resolve_manufacture_buff_ids(name, tier),
                )
            })
            .collect();
        let mut mixed_room = ManuRoomInput::with_operators(3, RecipeKind::Gold, mixed_ops);
        mixed_room.layout = Arc::new(LayoutContext::search_baseline());
        let mixed = solve_manufacture(&mixed_room, &table).unwrap();
        assert!(
            (mixed.prod_skill - 70.0).abs() < 0.5,
            "冬时+30 清流 2 贸 layout +40 芬归零 ≈70，got {}",
            mixed.prod_skill
        );

        // E2 森蚺应进池且自动化按发电站数加成
        let sen_e2 = OperBox::from_entries(vec![OperBoxEntry {
            id: "zumama".into(),
            name: "森蚺".into(),
            elite: 2,
            level: 1,
            own: true,
            potential: 1,
            rarity: 6,
        }]);
        let sen_pool =
            build_manufacture_pool(&sen_e2.manufacture_roster(&instances), &instances, &table)
                .unwrap();
        let sen = sen_pool.entry("森蚺").expect("E2 森蚺应在制造池");
        assert!(!sen.has_l2_delegate);
        let sen_room = ManuRoomInput::with_operators(
            3,
            RecipeKind::Gold,
            vec![
                ManuOperator::new(
                    "森蚺",
                    2,
                    instances.resolve_manufacture_buff_ids("森蚺", PromotionTier::TierUp),
                ),
                ManuOperator::new("芬", 0, vec!["manu_prod_spd[000]".into()]),
                ManuOperator::new("克洛丝", 0, vec!["manu_prod_spd[000]".into()]),
            ],
        );
        let mut sen_room = sen_room;
        Arc::make_mut(&mut sen_room.layout).power_station_count = 3;
        assert_eq!(
            instances.resolve_manufacture_buff_ids("森蚺", PromotionTier::TierUp),
            vec!["manu_prod_spd&power[010]".to_string()]
        );
        let sen_gold = solve_manufacture(&sen_room, &table).unwrap();
        // 精二仅自动化·β：10% × 3 发电站 = 30%
        assert!(
            (sen_gold.prod_skill - 30.0).abs() < 0.5,
            "E2 森蚺 3 发电站 automation，got {}",
            sen_gold.prod_skill
        );
    }
}
