use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::manufacture::input::{ManuRoomInput, ManuSearchRecipeMode};
use crate::manufacture::solver::{solve_manufacture, ManuProdBreakdown, ManuStorageBreakdown};
use crate::pool::{combinations_triples, ManuPool};
use crate::skill_table::SkillTable;
use crate::layout::{LayoutContext, SharedLayout};
use crate::types::RecipeKind;

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
    pub recipe_mode: ManuSearchRecipeMode,
    pub mood: f64,
    pub top_k: usize,
    pub layout: SharedLayout,
}

impl Default for ManuSearchOptions {
    fn default() -> Self {
        Self {
            level: 3,
            recipe_mode: ManuSearchRecipeMode::default(),
            mood: 24.0,
            top_k: 5,
            layout: Arc::new(LayoutContext::search_baseline()),
        }
    }
}

pub fn search_manufacture_triples(
    pool: &ManuPool,
    table: &SkillTable,
    options: &ManuSearchOptions,
) -> Result<ManuSearchReport> {
    match options.recipe_mode {
        ManuSearchRecipeMode::Lines(scenario) => {
            search_manufacture_split_lines(pool, table, options, scenario)
        }
        ManuSearchRecipeMode::Single(_) => search_manufacture_single_recipe(pool, table, options),
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
    };

    Ok(ManuSearchReport {
        recipe_mode: ManuSearchRecipeMode::Lines(scenario),
        best: best.clone(),
        top: vec![best],
        combinations: gold_report.combinations.saturating_add(br_report.combinations),
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
    let n = pool.entries.len();
    let combos: Vec<[usize; 3]> = combinations_triples(n).collect();
    let combinations = combos.len() as u64;
    let start = Instant::now();

    let mut hits: Vec<ManuSearchHit> = combos
        .par_iter()
        .filter_map(|combo| {
            let ops: Vec<_> = combo
                .iter()
                .map(|i| pool.entries[*i].to_manu_operator())
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
        b.composite_score
            .partial_cmp(&a.composite_score)
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

fn eval_single_recipe_hit(
    base: &ManuRoomInput,
    table: &SkillTable,
    recipe: RecipeKind,
) -> Option<ManuSearchHit> {
    let names = base.operator_names().into_iter().map(str::to_string).collect();
    let mut room = base.clone();
    room.active_recipe = recipe;
    let result = solve_manufacture(&room, table).ok()?;
    let mut per_station = ManuProdBreakdown::default();
    let mut storage = ManuStorageBreakdown::default();
    match recipe {
        RecipeKind::Gold => {
            per_station.gold = result.prod_total;
            storage.gold = result.storage_limit;
        }
        RecipeKind::BattleRecord => {
            per_station.battle_record = result.prod_total;
            storage.battle_record = result.storage_limit;
        }
        RecipeKind::Originium => {
            per_station.originium = result.prod_total;
            storage.originium = result.storage_limit;
        }
        RecipeKind::All => {}
    }
    Some(ManuSearchHit {
        names,
        gold_names: vec![],
        battle_record_names: vec![],
        composite_score: result.prod_total,
        per_station,
        storage,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manufacture::solver::score_manu_composite;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::manufacture::input::{ManuLineScenario, ManuOperator, ManuRoomInput, ManuSearchRecipeMode};
    use crate::pool::build_manufacture_pool;
    use crate::roster::Roster;
    use crate::tier::PromotionTier;

    fn table() -> SkillTable {
        SkillTable::load(&crate::skill_table::default_skill_table_path().unwrap()).unwrap()
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
        let report = search_manufacture_triples(&pool, &table, &ManuSearchOptions::default()).unwrap();
        assert_eq!(
            report.recipe_mode,
            ManuSearchRecipeMode::Lines(ManuLineScenario::standard_four_lines())
        );
        assert!(report.gold_line.is_some());
        assert!(report.battle_record_line.is_some());
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
        let pool = build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
            .unwrap();
        let report = search_manufacture_triples(&pool, &table, &ManuSearchOptions::default()).unwrap();
        let gold = report.gold_line.as_ref().expect("gold line");
        let br = report.battle_record_line.as_ref().expect("exp line");
        assert!(gold.names.contains(&"清流".to_string()));
        assert!(br.names.contains(&"酒神".to_string()));
        assert!((report.best.composite_score - (2.0 * gold.composite_score + 2.0 * br.composite_score)).abs() < 0.01);
        assert!(report.best.composite_score > 342.0);
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
        assert!(
            !qingliu.has_l2_delegate,
            "清流再生能源应已 L1 建模"
        );
        let dongshi = pool.entry("冬时").expect("冬时应在制造池");
        assert!(!dongshi.has_l2_delegate);

        // 公孙盒：森蚺 E1（6★）→ tier_0，instances 无制造技能绑定
        assert!(
            pool.entry("森蚺").is_none(),
            "E1 森蚺无制造技能实例，不应进制造池"
        );
        let skipped_sen = pool
            .skipped
            .iter()
            .find(|(n, _, _)| n == "森蚺");
        assert!(
            skipped_sen.is_none(),
            "森蚺不在制造 roster，不应出现在 skipped"
        );

        let mut opts = ManuSearchOptions::default();
        opts.layout = Arc::new(LayoutContext::search_baseline());
        let report = search_manufacture_triples(&pool, &table, &opts).unwrap();
        let gold = report.gold_line.as_ref().expect("gold line");
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
        let dongshi_room =
            ManuRoomInput::with_operators(3, RecipeKind::Gold, dongshi_ops);
        let dongshi_gold = solve_manufacture(&dongshi_room, &table).unwrap();
        assert!(
            (dongshi_gold.prod_skill - 30.0).abs() < 0.01,
            "冬时站级 3×10%=30，got {}",
            dongshi_gold.prod_skill
        );
        assert_eq!(
            dongshi_gold.storage_limit,
            35,
            "精一冬时 3×5 仓库贡献"
        );

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
        let mut mixed_room =
            ManuRoomInput::with_operators(3, RecipeKind::Gold, mixed_ops);
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
