use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::instances::OperatorInstances;
use crate::operbox::OperBox;
use crate::pool::{
    build_trade_pool, filter_trade_pool, jie_e0_trade_operator, karlan_precision_active,
    JIE_TRADE_NAME, TradePool,
};
use crate::roster::Roster;
use crate::search::{
    hit_closure_shortcut, hit_docus_solo_shortcut, hit_witch_shortcut, pick_docus_trade_hit,
    search_trade_triples,
    search_trade_triples_filtered, SearchTripleFilter, TradeSearchHit, TradeSearchOptions,
};
use crate::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use crate::skill_table::SkillTable;

pub const TRADE_STATIONS_PER_SHIFT: usize = 3;
pub const WORKERS_PER_STATION: usize = 3;
pub const WORKERS_PER_SHIFT: usize = TRADE_STATIONS_PER_SHIFT * WORKERS_PER_STATION;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TradeStationRole {
    /// 巫恋核 / 龙舌兰（`gsl_witch_*`）
    Witch,
    /// 可露希尔特别订单（`gsl_closure_*`）
    Closure,
    /// 但书单走（`gsl_docus_solo`）
    Docus,
    /// 精0 孑摊贩带队（余量班）
    JieE0Lead,
    Plain,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeStationPlan {
    pub station_index: usize,
    pub hit: TradeSearchHit,
    pub role: TradeStationRole,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeShiftPlan {
    pub index: usize,
    pub stations: Vec<TradeStationPlan>,
    pub workers: Vec<String>,
    pub total_score: f64,
    /// `Some(0)` when this shift reuses shift 1 layout (A-B-A).
    pub reused_from_shift: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeRotationReport {
    pub shifts: Vec<TradeShiftPlan>,
    pub elapsed: Duration,
}

struct StationPick {
    hit: TradeSearchHit,
    role: TradeStationRole,
}

fn pick_station(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    filter: SearchTripleFilter,
    preferred_role: TradeStationRole,
) -> Result<StationPick> {
    match search_trade_triples_filtered(pool, table, options, filter.clone()) {
        Ok(report) => Ok(StationPick {
            hit: report.best,
            role: preferred_role,
        }),
        Err(_) if filter.hit_filter.is_some() || filter.must_include_name.is_some() => {
            let report = search_trade_triples(pool, table, options)?;
            Ok(StationPick {
                hit: report.best,
                role: TradeStationRole::Plain,
            })
        }
        Err(e) => Err(e),
    }
}

fn commit_station(
    pick: StationPick,
    station_index: usize,
    used: &mut HashSet<String>,
) -> Result<TradeStationPlan> {
    for name in &pick.hit.names {
        if !used.insert(name.clone()) {
            return Err(Error::msg(format!(
                "贸易站 {} 重复选用 {}",
                station_index + 1,
                name
            )));
        }
    }
    Ok(TradeStationPlan {
        station_index,
        hit: pick.hit,
        role: pick.role,
    })
}

/// Shift A：龙巫 → 可露 → 但书，再按 score 贪心填满（通常正好三站）。
pub fn schedule_meta_shift_from_pool(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    shift_index: usize,
) -> Result<TradeShiftPlan> {
    if pool.entries.len() < WORKERS_PER_STATION {
        return Err(Error::msg(format!(
            "第 {} 班贸易池不足 {} 人（当前 {}）",
            shift_index + 1,
            WORKERS_PER_STATION,
            pool.entries.len()
        )));
    }

    // Meta 站（但书 → 龙巫 → 可露）按单站金条订单搜；Stations 模式会同时过滤源石线，
    // 而 gsl_witch 等 shortcut 仅出现在金条订单，导致过滤失败并 fallback 为 Plain。
    let mut search_opts = per_station_search_options(options);
    search_opts.top_k = 1;

    let mut used = HashSet::new();
    let mut stations = Vec::with_capacity(TRADE_STATIONS_PER_SHIFT);
    let mut total_score = 0.0;

    let meta_slots: [(TradeStationRole, bool); 3] = [
        (TradeStationRole::Docus, true),
        (TradeStationRole::Witch, false),
        (TradeStationRole::Closure, false),
    ];

    for (station_index, (role, is_docus)) in meta_slots.into_iter().enumerate() {
        let sub = filter_trade_pool(pool, &used);
        if sub.entries.len() < WORKERS_PER_STATION {
            return Err(Error::msg(format!(
                "第 {} 班贸易站 {} 可用干员不足 {} 人（已用 {} 人）",
                shift_index + 1,
                station_index + 1,
                WORKERS_PER_STATION,
                used.len()
            )));
        }
        let pick = if is_docus {
            let hit = pick_docus_trade_hit(
                &sub,
                table,
                search_opts.clone(),
                search_opts.layout.as_ref(),
                &used,
                search_opts.top_k,
            )?;
            StationPick { hit, role }
        } else {
            let hit_filter = match role {
                TradeStationRole::Witch => hit_witch_shortcut,
                TradeStationRole::Closure => hit_closure_shortcut,
                _ => unreachable!(),
            };
            pick_station(
                &sub,
                table,
                &search_opts,
                SearchTripleFilter {
                    hit_filter: Some(hit_filter),
                    ..SearchTripleFilter::default()
                },
                role,
            )?
        };
        total_score += pick.hit.score;
        stations.push(commit_station(pick, station_index, &mut used)?);
    }

    let mut workers: Vec<String> = used.into_iter().collect();
    workers.sort();

    Ok(TradeShiftPlan {
        index: shift_index,
        stations,
        workers,
        total_score,
        reused_from_shift: None,
    })
}

fn per_station_search_options(options: &TradeSearchOptions) -> TradeSearchOptions {
    let mut o = options.clone();
    o.order_mode = TradeSearchOrderMode::Single(TradeOrderKind::Gold);
    o
}

/// Shift B：精0 孑带队一站，其余贪心；无孑则全贪心。
pub fn schedule_jie_remainder_shift_from_pool(
    pool: &TradePool,
    table: &SkillTable,
    instances: &OperatorInstances,
    options: &TradeSearchOptions,
    shift_index: usize,
) -> Result<TradeShiftPlan> {
    let mut search_opts = per_station_search_options(options);
    search_opts.top_k = 1;

    if pool.entries.len() < WORKERS_PER_STATION {
        return Err(Error::msg(format!(
            "第 {} 班贸易池不足 {} 人（当前 {}）",
            shift_index + 1,
            WORKERS_PER_STATION,
            pool.entries.len()
        )));
    }

    let mut used = HashSet::new();
    let mut stations = Vec::with_capacity(TRADE_STATIONS_PER_SHIFT);
    let mut total_score = 0.0;

    let jie_lead = !karlan_precision_active(&search_opts.layout.global_inject)
        && jie_e0_trade_operator(instances, table).is_some();

    if jie_lead {
        let sub = filter_trade_pool(pool, &used);
        if sub.entries.len() >= WORKERS_PER_STATION {
            let jie_op = jie_e0_trade_operator(instances, table).unwrap();
            if let Ok(report) = search_trade_triples_filtered(
                &sub,
                table,
                &search_opts,
                SearchTripleFilter {
                    must_include_name: Some(JIE_TRADE_NAME.to_string()),
                    must_operator_override: Some(jie_op),
                    ..SearchTripleFilter::default()
                },
            ) {
                let pick = StationPick {
                    hit: report.best,
                    role: TradeStationRole::JieE0Lead,
                };
                total_score += pick.hit.score;
                stations.push(commit_station(pick, stations.len(), &mut used)?);
            }
        }
    }

    while stations.len() < TRADE_STATIONS_PER_SHIFT {
        let station_index = stations.len();
        let sub = filter_trade_pool(pool, &used);
        if sub.entries.len() < WORKERS_PER_STATION {
            return Err(Error::msg(format!(
                "第 {} 班贸易站 {} 可用干员不足 {} 人（已用 {} 人）",
                shift_index + 1,
                station_index + 1,
                WORKERS_PER_STATION,
                used.len()
            )));
        }
        let pick = pick_station(
            &sub,
            table,
            &search_opts,
            SearchTripleFilter::default(),
            TradeStationRole::Plain,
        )?;
        total_score += pick.hit.score;
        stations.push(commit_station(pick, station_index, &mut used)?);
    }

    let mut workers: Vec<String> = used.into_iter().collect();
    workers.sort();

    Ok(TradeShiftPlan {
        index: shift_index,
        stations,
        workers,
        total_score,
        reused_from_shift: None,
    })
}

/// Greedy: three trade stations, each `search_trade_triples` on pool minus in-shift used.
pub fn schedule_trade_shift(
    roster: &Roster,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &TradeSearchOptions,
    shift_index: usize,
) -> Result<TradeShiftPlan> {
    let pool = build_trade_pool(roster, instances, table)?;
    schedule_trade_shift_from_pool(&pool, table, options, shift_index)
}

pub fn schedule_trade_shift_from_pool(
    pool: &TradePool,
    table: &SkillTable,
    options: &TradeSearchOptions,
    shift_index: usize,
) -> Result<TradeShiftPlan> {
    if pool.entries.len() < WORKERS_PER_STATION {
        return Err(Error::msg(format!(
            "第 {} 班贸易池不足 {} 人（当前 {}）",
            shift_index + 1,
            WORKERS_PER_STATION,
            pool.entries.len()
        )));
    }

    let mut search_opts = per_station_search_options(options);
    search_opts.top_k = 1;

    let mut used = HashSet::new();
    let mut stations = Vec::with_capacity(TRADE_STATIONS_PER_SHIFT);
    let mut total_score = 0.0;

    for station_index in 0..TRADE_STATIONS_PER_SHIFT {
        let sub = filter_trade_pool(pool, &used);
        if sub.entries.len() < WORKERS_PER_STATION {
            return Err(Error::msg(format!(
                "第 {} 班贸易站 {} 可用干员不足 {} 人（已用 {} 人）",
                shift_index + 1,
                station_index + 1,
                WORKERS_PER_STATION,
                used.len()
            )));
        }
        let pick = pick_station(
            &sub,
            table,
            &search_opts,
            SearchTripleFilter::default(),
            TradeStationRole::Plain,
        )?;
        total_score += pick.hit.score;
        stations.push(commit_station(pick, station_index, &mut used)?);
    }

    let mut workers: Vec<String> = used.into_iter().collect();
    workers.sort();

    Ok(TradeShiftPlan {
        index: shift_index,
        stations,
        workers,
        total_score,
        reused_from_shift: None,
    })
}

fn workers_set(plan: &TradeShiftPlan) -> HashSet<String> {
    plan.workers.iter().cloned().collect()
}

fn assert_disjoint(a: &HashSet<String>, b: &HashSet<String>, label: &str) -> Result<()> {
    let overlap: Vec<String> = a.intersection(b).cloned().collect();
    if overlap.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("{label} 存在干员重合: {overlap:?}")))
    }
}

/// Three-shift A-B-A: shift1 龙巫/可露/但书；shift2 精0孑带队 + 余量；shift3 复用 shift1。
pub fn schedule_trade_rotation_a_b_a(
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &TradeSearchOptions,
) -> Result<TradeRotationReport> {
    let start = std::time::Instant::now();

    let roster1 = operbox.trade_roster(instances);
    if roster1.len() < WORKERS_PER_SHIFT * 2 {
        return Err(Error::msg(format!(
            "持有贸易干员至少需 {} 人才能排 A-B-A 两班不重合（当前持有 {}）",
            WORKERS_PER_SHIFT * 2,
            roster1.len()
        )));
    }

    let pool1 = build_trade_pool(&roster1, instances, table)?;
    let shift1 = schedule_meta_shift_from_pool(&pool1, table, options, 0)?;
    let workers1 = workers_set(&shift1);

    let roster2 = operbox.roster_excluding(&workers1);
    if roster2.len() < WORKERS_PER_SHIFT {
        return Err(Error::msg(format!(
            "第 2 班修剪后可用 {} 人，不足 {}",
            roster2.len(),
            WORKERS_PER_SHIFT
        )));
    }
    let pool2 = build_trade_pool(&roster2, instances, table)?;
    let shift2 = schedule_jie_remainder_shift_from_pool(&pool2, table, instances, options, 1)?;
    let workers2 = workers_set(&shift2);
    assert_disjoint(&workers1, &workers2, "第 1 班与第 2 班")?;

    let mut shift3 = shift1.clone();
    shift3.index = 2;
    shift3.reused_from_shift = Some(0);
    assert_disjoint(&workers_set(&shift3), &workers2, "第 3 班与第 2 班")?;

    Ok(TradeRotationReport {
        shifts: vec![shift1, shift2, shift3],
        elapsed: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::default_instances_path;
    use crate::operbox::default_operbox_gongsun_path;
    use crate::skill_table::{default_skill_table_path, SkillTable};

    fn ctx() -> (OperBox, OperatorInstances, SkillTable) {
        let operbox = OperBox::load(&default_operbox_gongsun_path().unwrap()).unwrap();
        let instances = OperatorInstances::load(&default_instances_path().unwrap()).unwrap();
        let table = SkillTable::load(&default_skill_table_path().unwrap()).unwrap();
        (operbox, instances, table)
    }

    #[test]
    fn gongsun_rotation_aba_disjoint_and_reuse() {
        let (operbox, instances, table) = ctx();
        let report =
            schedule_trade_rotation_a_b_a(&operbox, &instances, &table, &TradeSearchOptions::default())
                .unwrap();
        assert_eq!(report.shifts.len(), 3);

        let w1: HashSet<_> = report.shifts[0].workers.iter().cloned().collect();
        let w2: HashSet<_> = report.shifts[1].workers.iter().cloned().collect();
        assert!(w1.is_disjoint(&w2));
        assert_eq!(report.shifts[2].reused_from_shift, Some(0));
        assert_eq!(report.shifts[0].stations.len(), 3);
        assert_eq!(report.shifts[2].workers, report.shifts[0].workers);

        // meta 顺序：但书 → 龙巫 → 可露。公孙盒未拥有但书，首站 docus 过滤失败后 Plain 落位。
        assert_eq!(report.shifts[0].stations.len(), TRADE_STATIONS_PER_SHIFT);
        assert!(
            report.shifts[0].stations[0].role == TradeStationRole::Plain
                || report.shifts[0]
                    .stations
                    .iter()
                    .any(|s| s.role == TradeStationRole::Docus),
            "shift1 meta order starts with docus attempt: {:?}",
            report.shifts[0].stations.iter().map(|s| s.role).collect::<Vec<_>>()
        );
    }

    #[test]
    fn shift2_jie_e0_leads_when_owned() {
        let (operbox, instances, table) = ctx();
        if !operbox.owns(JIE_TRADE_NAME) {
            return;
        }
        let report =
            schedule_trade_rotation_a_b_a(&operbox, &instances, &table, &TradeSearchOptions::default())
                .unwrap();
        let jie_station = report.shifts[1]
            .stations
            .iter()
            .find(|s| s.role == TradeStationRole::JieE0Lead);
        assert!(
            jie_station.is_some(),
            "shift2 should have 孑 e0 lead station: {:?}",
            report.shifts[1].stations
        );
        assert!(
            jie_station
                .unwrap()
                .hit
                .names
                .iter()
                .any(|n| n == JIE_TRADE_NAME)
        );
    }

    #[test]
    fn shift_three_stations_nine_unique_workers() {
        let (operbox, instances, table) = ctx();
        let plan = schedule_trade_shift(
            &operbox.roster(),
            &instances,
            &table,
            &TradeSearchOptions::default(),
            0,
        )
        .unwrap();
        assert_eq!(plan.workers.len(), WORKERS_PER_SHIFT);
        assert_eq!(plan.stations.len(), TRADE_STATIONS_PER_SHIFT);
    }
}
