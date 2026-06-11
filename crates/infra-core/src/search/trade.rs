use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::error::Result;
use crate::pool::{combinations_indices, TradePool};
use crate::skill_table::SkillTable;
use crate::trade::input::TradeLayoutContext;
use crate::trade::{solve_trade, TradeRoomInput};

#[derive(Debug, Clone)]
pub struct TradeSearchHit {
    pub names: Vec<String>,
    pub score: f64,
    pub trade_pct: f64,
    pub gold_pct: f64,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TradeSearchReport {
    pub best: TradeSearchHit,
    pub top: Vec<TradeSearchHit>,
    pub combinations: u64,
    pub evaluated: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct TradeSearchOptions {
    pub trade_level: u8,
    pub mood: f64,
    pub top_k: usize,
    /// 制造站赤金真实生产线数（公孙长乐基准常用 4）。
    pub gold_production_lines: u32,
    /// 全基建布局上下文（伺夜/空弦/石英/风絮/状态链等）.
    pub layout: TradeLayoutContext,
}

impl Default for TradeSearchOptions {
    fn default() -> Self {
        Self {
            trade_level: 3,
            mood: 24.0,
            top_k: 5,
            gold_production_lines: 4,
            layout: TradeLayoutContext::search_baseline(),
        }
    }
}

pub fn search_trade_triples(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
) -> Result<TradeSearchReport> {
    let n = pool.entries.len();
    let combinations = crate::pool::n_choose_k_u64(n, 3);
    let indices: Vec<Vec<usize>> = combinations_indices(n, 3).collect();
    let start = Instant::now();

    let mut hits: Vec<TradeSearchHit> = indices
        .par_iter()
        .filter_map(|combo| {
            let ops: Vec<_> = combo
                .iter()
                .map(|&i| pool.entries[i].to_trade_operator())
                .collect();
            let input = TradeRoomInput {
                level: options.trade_level,
                operators: ops,
                order_count: None,
                mood: options.mood,
                gold_production_lines: Some(options.gold_production_lines),
                durin_virtual_lines: None,
                human_fireworks: None,
                layout: options.layout.clone(),
            };
            let result = solve_trade(&input, table).ok()?;
            let names: Vec<String> = input.operators.iter().map(|o| o.name.clone()).collect();
            Some(TradeSearchHit {
                score: result.effective_eff_multiplier,
                trade_pct: result.order_eff_total,
                gold_pct: result.order_mechanic.mechanic_equiv_eff_pct,
                shortcut: result.trade_shortcut,
                names,
            })
        })
        .collect();

    let evaluated = hits.len() as u64;
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = hits.first().cloned().ok_or_else(|| {
        crate::error::Error::msg("trade pool has fewer than 3 ready operators")
    })?;
    let top = hits.into_iter().take(options.top_k).collect();

    Ok(TradeSearchReport {
        best,
        top,
        combinations,
        evaluated,
        elapsed: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::instances::OperatorInstances;
    use crate::pool::build_trade_pool;
    use crate::roster::Roster;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    #[test]
    fn default_search_options_includes_layout_baseline() {
        let opts = TradeSearchOptions::default();
        assert_eq!(opts.layout.meeting_max_level, 3);
        assert_eq!(opts.layout.dorm_occupant_count, 20);
        assert_eq!(opts.layout.monster_cuisine_layers, 3);
        assert!(opts.layout.base_workforce.iter().any(|n| n == "伺夜"));
        assert!(opts.layout.base_workforce.iter().any(|n| n == "乌尔比安"));
    }

    #[test]
    fn roster_search_finds_docus_station() {
        let roster = Roster::load_csv_for_facility(
            &crate::roster::default_roster_path().unwrap(),
            "trade",
        )
        .unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let pool = build_trade_pool(&roster, &instances, &table).unwrap();
        if pool.entries.len() < 3 {
            return;
        }
        let report = search_trade_triples(&pool, &table, &TradeSearchOptions::default()).unwrap();
        assert!(report.evaluated > 0);
        assert!(
            report.top.iter().any(|h| h.names.contains(&"但书".to_string())),
            "但书应出现在 top 结果中: {:?}",
            report.top
        );
    }
}
