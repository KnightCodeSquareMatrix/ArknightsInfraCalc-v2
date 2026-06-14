use std::collections::HashSet;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::pool::PowerPool;
use crate::power::{solve_power, PowerRoomInput};
use crate::skill_table::SkillTable;
use crate::layout::LayoutContext;

#[derive(Debug, Clone, Serialize)]
pub struct PowerSearchHit {
    pub name: String,
    pub charge_speed_pct: f64,
    pub mood_drain_delta: f64,
    /// 晨曦等本班产出的虚拟发电站（写入 layout 前快照）。
    pub virtual_power_produced: f64,
    /// 搜索排序分（充能 + 虚拟发电折算，见 [`power_station_score`]）。
    pub score: f64,
}

/// 单站贪心排序：充能纸面 + 虚拟发电 × 折算系数。
///
/// 系数锚点：243 金线 automation trio 每 +1 有效发电约 +25% 制造纸面（温蒂 15 + 森蚺 10）。
pub fn power_station_score(charge_speed_pct: f64, virtual_power_produced: f64) -> f64 {
    const VIRTUAL_POWER_MANU_EQUIV: f64 = 30.0;
    charge_speed_pct + virtual_power_produced * VIRTUAL_POWER_MANU_EQUIV
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerStationAssignment {
    pub station_index: usize,
    pub hit: PowerSearchHit,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerSearchReport {
    pub assignments: Vec<PowerStationAssignment>,
    pub total_charge_speed_pct: f64,
    pub evaluated: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct PowerSearchOptions {
    pub station_count: u8,
    pub mood: f64,
    pub shift_hours: f64,
    pub layout: LayoutContext,
}

impl Default for PowerSearchOptions {
    fn default() -> Self {
        Self {
            station_count: 3,
            mood: 24.0,
            shift_hours: 24.0,
            layout: LayoutContext::search_baseline(),
        }
    }
}

/// 每站 1 人、干员不重复的贪心分配（按 flat hint 降序逐站取最优）。
pub fn search_power_assignment(
    pool: &PowerPool,
    table: &SkillTable,
    options: &PowerSearchOptions,
) -> Result<PowerSearchReport> {
    let start = Instant::now();
    let mut used = HashSet::new();
    let mut assignments = Vec::new();
    let mut total = 0.0;
    let mut evaluated = 0u64;

    for station in 0..options.station_count as usize {
        let mut best: Option<PowerSearchHit> = None;
        for entry in &pool.entries {
            if used.contains(&entry.name) {
                continue;
            }
            let mut layout = options.layout.clone();
            layout.drone_cap = layout.drone_cap.max(135);
            let input = PowerRoomInput {
                operator: entry.to_power_operator(),
                mood: options.mood,
                shift_hours: options.shift_hours,
                layout,
            };
            let result = solve_power(&input, table)?;
            evaluated += 1;
            let hit = PowerSearchHit {
                name: entry.name.clone(),
                charge_speed_pct: result.charge_speed_pct,
                mood_drain_delta: result.mood_drain_delta,
                virtual_power_produced: result.virtual_power_produced,
                score: power_station_score(
                    result.charge_speed_pct,
                    result.virtual_power_produced,
                ),
            };
            if best.as_ref().is_none_or(|b| hit.score > b.score) {
                best = Some(hit);
            }
        }
        let Some(hit) = best else { break };
        used.insert(hit.name.clone());
        total += hit.charge_speed_pct;
        assignments.push(PowerStationAssignment {
            station_index: station,
            hit,
        });
    }

    Ok(PowerSearchReport {
        assignments,
        total_charge_speed_pct: total,
        evaluated,
        elapsed: start.elapsed(),
    })
}

/// 单站 Top-K（用于调试）。
pub fn search_power_top(
    pool: &PowerPool,
    table: &SkillTable,
    options: &PowerSearchOptions,
    top_k: usize,
) -> Result<Vec<PowerSearchHit>> {
    let mut layout = options.layout.clone();
    layout.drone_cap = layout.drone_cap.max(135);

    let mut hits: Vec<PowerSearchHit> = pool
        .entries
        .par_iter()
        .filter_map(|entry| {
            let input = PowerRoomInput {
                operator: entry.to_power_operator(),
                mood: options.mood,
                shift_hours: options.shift_hours,
                layout: layout.clone(),
            };
            let result = solve_power(&input, table).ok()?;
            let score = power_station_score(
                result.charge_speed_pct,
                result.virtual_power_produced,
            );
            Some(PowerSearchHit {
                name: entry.name.clone(),
                charge_speed_pct: result.charge_speed_pct,
                mood_drain_delta: result.mood_drain_delta,
                virtual_power_produced: result.virtual_power_produced,
                score,
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    hits.truncate(top_k);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{default_instances_path, OperatorInstances};
    use crate::layout::resolve_search_baseline_layout;
    use crate::operbox::default_operbox_full_e2_path;
    use crate::operbox::OperBox;
    use crate::pool::build_power_pool;
    use crate::skill_table::default_skill_table_path;
    use crate::skill_table::SkillTable;

    #[test]
    fn greyy2_virtual_power_beats_plain_greyy_on_score() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        if !operbox.owns("承曦格雷伊") {
            return;
        }
        let pool = build_power_pool(&operbox.power_roster(&instances), &instances, &table).unwrap();
        let layout = resolve_search_baseline_layout().unwrap();
        let opts = PowerSearchOptions {
            layout,
            ..Default::default()
        };
        let hits = search_power_top(&pool, &table, &opts, 50).unwrap();
        let greyy2 = hits.iter().find(|h| h.name == "承曦格雷伊").expect("承曦格雷伊");
        let greyy = hits.iter().find(|h| h.name == "格雷伊").expect("格雷伊");
        assert!(
            greyy2.virtual_power_produced > 0.0,
            "E2 晨曦应产出虚拟发电"
        );
        assert!(
            greyy2.score > greyy.score,
            "排序分应体现虚拟发电价值: greyy2={} greyy={}",
            greyy2.score,
            greyy.score
        );
    }

    #[test]
    fn power_assignment_picks_greyy2_for_first_station_ideal_e2() {
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let operbox = OperBox::load(&default_operbox_full_e2_path().unwrap()).unwrap();
        if !operbox.owns("承曦格雷伊") {
            return;
        }
        let pool = build_power_pool(&operbox.power_roster(&instances), &instances, &table).unwrap();
        let layout = resolve_search_baseline_layout().unwrap();
        let opts = PowerSearchOptions {
            station_count: 3,
            layout,
            ..Default::default()
        };
        let report = search_power_assignment(&pool, &table, &opts).unwrap();
        assert_eq!(report.assignments.len(), 3);
        assert_eq!(report.assignments[0].hit.name, "承曦格雷伊");
    }
}
