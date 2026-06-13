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
            };
            if best
                .as_ref()
                .is_none_or(|b| hit.charge_speed_pct > b.charge_speed_pct)
            {
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
            Some(PowerSearchHit {
                name: entry.name.clone(),
                charge_speed_pct: result.charge_speed_pct,
                mood_drain_delta: result.mood_drain_delta,
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        b.charge_speed_pct
            .partial_cmp(&a.charge_speed_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    hits.truncate(top_k);
    Ok(hits)
}
